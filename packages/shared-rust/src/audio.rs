//! Microphone capture — records mono float32 PCM and resamples it, on the fly,
//! to the 16 kHz the STT pipeline and Silero VAD expect.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;

/// Sample rate the STT pipeline (Whisper) and Silero VAD expect.
pub const TARGET_RATE: u32 = 16_000;

type SharedBuffer = Arc<Mutex<Vec<f32>>>;

/// The result of probing the default microphone — onboarding shows it so the
/// user knows whether Lashon can hear them (`docs/adr/0013`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum MicProbe {
    /// The default input device opened and a capture stream started.
    Ready,
    /// No input device is present at all — nothing is plugged in.
    NoDevice,
    /// A device exists but the capture stream could not be opened. The most
    /// common cause is the OS withholding microphone access from Lashon.
    Unavailable { reason: String },
}

/// Probe the default microphone: open it and briefly start a capture stream.
///
/// On macOS, *starting* a capture stream is what raises the OS
/// microphone-permission prompt on first use — so this call doubles as the
/// permission request the onboarding mic step needs. The stream uses a no-op
/// callback and is dropped immediately; no audio is ever buffered or retained
/// (`.claude/rules/security.md`).
pub fn probe_input() -> MicProbe {
    let Some(device) = cpal::default_host().default_input_device() else {
        return MicProbe::NoDevice;
    };
    let supported = match device.default_input_config() {
        Ok(supported) => supported,
        Err(err) => {
            return MicProbe::Unavailable {
                reason: err.to_string(),
            }
        }
    };
    let config: cpal::StreamConfig = supported.config();
    let on_error = |err| tracing::warn!("microphone probe stream error: {err}");

    // The callback discards every frame — the probe only needs the stream to
    // open and start, not its audio.
    let built = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(&config, move |_: &[f32], _| {}, on_error, None)
        }
        cpal::SampleFormat::I16 => {
            device.build_input_stream(&config, move |_: &[i16], _| {}, on_error, None)
        }
        cpal::SampleFormat::U16 => {
            device.build_input_stream(&config, move |_: &[u16], _| {}, on_error, None)
        }
        other => {
            return MicProbe::Unavailable {
                reason: format!("unsupported input sample format: {other:?}"),
            }
        }
    };
    let stream = match built {
        Ok(stream) => stream,
        Err(err) => {
            return MicProbe::Unavailable {
                reason: err.to_string(),
            }
        }
    };
    match stream.play() {
        Ok(()) => MicProbe::Ready,
        Err(err) => MicProbe::Unavailable {
            reason: err.to_string(),
        },
    }
    // `stream` is dropped here — capture ends at once.
}

/// A microphone capture. `start()` opens the default input device; `stop()`
/// ends the take and returns it as 16 kHz mono float32 PCM.
pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    buffer: SharedBuffer,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Begin recording from the default input device. Captured audio is
    /// downmixed to mono and resampled to [`TARGET_RATE`] as it arrives, so
    /// the buffer is always 16 kHz.
    pub fn start(&mut self) -> Result<()> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or_else(|| anyhow!("no default audio input device"))?;
        let supported = device
            .default_input_config()
            .context("querying the default input config")?;
        let source_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let config: cpal::StreamConfig = supported.config();

        let buffer = Arc::clone(&self.buffer);
        buffer.lock().expect("audio buffer lock").clear();
        let on_error = |err| tracing::error!("audio input stream error: {err}");

        // A plain mutex-guarded buffer suffices for a bounded push-to-talk
        // take; the lock-free rolling ring buffer of docs/roadmap.md §1.1
        // arrives with always-on wake-word capture.
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                let mut resampler = Resampler::new(source_rate, TARGET_RATE);
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        let mono = to_mono(data, channels, |s| s);
                        capture_chunk(&buffer, &mut resampler, &mono);
                    },
                    on_error,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let mut resampler = Resampler::new(source_rate, TARGET_RATE);
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let mono = to_mono(data, channels, |s| f32::from(s) / 32_768.0);
                        capture_chunk(&buffer, &mut resampler, &mono);
                    },
                    on_error,
                    None,
                )
            }
            other => return Err(anyhow!("unsupported input sample format: {other:?}")),
        }
        .context("building the audio input stream")?;

        stream.play().context("starting the audio input stream")?;
        self.stream = Some(stream);
        Ok(())
    }

    /// Stop recording and return the take as 16 kHz mono float32 PCM.
    pub fn stop(&mut self) -> Vec<f32> {
        self.stream = None; // dropping the cpal stream ends capture
        std::mem::take(&mut *self.buffer.lock().expect("audio buffer lock"))
    }

    /// Buffered samples from `cursor` to the current end, copied without
    /// consuming them — lets a caller run voice-activity detection while a
    /// take is still recording. Samples are 16 kHz mono.
    pub fn samples_since(&self, cursor: usize) -> Vec<f32> {
        let buffer = self.buffer.lock().expect("audio buffer lock");
        buffer
            .get(cursor..)
            .map(<[f32]>::to_vec)
            .unwrap_or_default()
    }
}

impl Default for AudioCapture {
    fn default() -> Self {
        Self::new()
    }
}

/// Start an always-on capture for wake-word detection.
///
/// Resampled 16 kHz mono chunks arrive on the returned receiver. The returned
/// stream must be kept alive — dropping it ends capture. Unlike [`AudioCapture`]
/// this has no bounded take: the wake-word worker owns it for the app's life.
pub fn open_wake_stream() -> Result<(cpal::Stream, std::sync::mpsc::Receiver<Vec<f32>>)> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or_else(|| anyhow!("no default audio input device"))?;
    let supported = device
        .default_input_config()
        .context("querying the default input config")?;
    let source_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let config: cpal::StreamConfig = supported.config();

    let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
    let on_error = |err| tracing::error!("wake-word audio stream error: {err}");

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let mut resampler = Resampler::new(source_rate, TARGET_RATE);
            let tx = tx.clone();
            device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    let mono = to_mono(data, channels, |s| s);
                    let mut out = Vec::new();
                    resampler.push(&mono, &mut out);
                    if !out.is_empty() {
                        let _ = tx.send(out);
                    }
                },
                on_error,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let mut resampler = Resampler::new(source_rate, TARGET_RATE);
            let tx = tx.clone();
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let mono = to_mono(data, channels, |s| f32::from(s) / 32_768.0);
                    let mut out = Vec::new();
                    resampler.push(&mono, &mut out);
                    if !out.is_empty() {
                        let _ = tx.send(out);
                    }
                },
                on_error,
                None,
            )
        }
        other => return Err(anyhow!("unsupported input sample format: {other:?}")),
    }
    .context("building the wake-word audio stream")?;

    stream
        .play()
        .context("starting the wake-word audio stream")?;
    Ok((stream, rx))
}

/// Resample a mono chunk to 16 kHz and append it to the shared buffer.
fn capture_chunk(buffer: &SharedBuffer, resampler: &mut Resampler, mono: &[f32]) {
    let mut out = Vec::new();
    resampler.push(mono, &mut out);
    if let Ok(mut buf) = buffer.lock() {
        buf.extend_from_slice(&out);
    }
}

/// Downmix interleaved multi-channel samples to a mono `f32` buffer, averaging
/// each frame's channels.
fn to_mono<T: Copy>(data: &[T], channels: usize, to_f32: impl Fn(T) -> f32) -> Vec<f32> {
    let channels = channels.max(1);
    data.chunks(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().copied().map(&to_f32).sum();
            sum / frame.len() as f32
        })
        .collect()
}

/// A streaming linear resampler.
///
/// It carries the fractional read position and the unconsumed input tail
/// across calls, so a stream resampled chunk by chunk has no discontinuity at
/// the chunk boundaries — unlike resampling each chunk in isolation. Linear
/// interpolation is adequate for STT input and VAD; a polyphase resampler can
/// replace it if transcription quality ever warrants.
struct Resampler {
    /// Source samples per output sample — `from_rate / to_rate`.
    ratio: f64,
    /// Fractional read position within `pending`.
    pos: f64,
    /// Input samples fed but not yet fully consumed.
    pending: Vec<f32>,
}

impl Resampler {
    fn new(from_rate: u32, to_rate: u32) -> Self {
        Self {
            ratio: f64::from(from_rate) / f64::from(to_rate),
            pos: 0.0,
            pending: Vec::new(),
        }
    }

    /// Feed source-rate mono samples; append the resampled output to `out`.
    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(input);
        // Emit a sample for every read position with a neighbour to its right
        // to interpolate toward.
        while (self.pos as usize) + 1 < self.pending.len() {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            out.push(self.pending[i] + (self.pending[i + 1] - self.pending[i]) * frac);
            self.pos += self.ratio;
        }
        // Drop input the next output can no longer need, and rebase `pos`.
        // `pos` may have stepped past the end, so clamp before draining.
        let consumed = (self.pos as usize).min(self.pending.len());
        self.pending.drain(..consumed);
        self.pos -= consumed as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_downsamples_three_to_one() {
        // 48 kHz -> 16 kHz is exactly 3:1: one output sample per three inputs.
        let mut resampler = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        let input: Vec<f32> = (0..9).map(|i| i as f32).collect();
        resampler.push(&input, &mut out);
        assert_eq!(out, vec![0.0, 3.0, 6.0]);
    }

    #[test]
    fn resampler_preserves_values_at_the_same_rate() {
        let mut resampler = Resampler::new(16_000, 16_000);
        let mut out = Vec::new();
        resampler.push(&[0.1, 0.2, 0.3, 0.4], &mut out);
        resampler.push(&[0.5, 0.6], &mut out);
        // A one-sample latency: the stream's tail lags one push behind.
        assert_eq!(out, vec![0.1, 0.2, 0.3, 0.4, 0.5]);
    }

    #[test]
    fn resampler_carries_phase_across_chunk_boundaries() {
        // The same nine samples as `downsamples_three_to_one`, split across
        // two pushes — the result must be identical.
        let mut resampler = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        resampler.push(&[0.0, 1.0, 2.0, 3.0], &mut out);
        resampler.push(&[4.0, 5.0, 6.0, 7.0, 8.0], &mut out);
        assert_eq!(out, vec![0.0, 3.0, 6.0]);
    }

    #[test]
    fn to_mono_averages_each_stereo_frame() {
        assert_eq!(to_mono(&[1.0f32, 0.0, 0.4, 0.6], 2, |s| s), vec![0.5, 0.5]);
    }

    #[test]
    fn samples_since_returns_the_unconsumed_tail() {
        let capture = AudioCapture::new();
        capture
            .buffer
            .lock()
            .unwrap()
            .extend_from_slice(&[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(capture.samples_since(2), vec![0.3, 0.4]);
        assert!(capture.samples_since(4).is_empty());
        assert!(capture.samples_since(99).is_empty());
    }
}
