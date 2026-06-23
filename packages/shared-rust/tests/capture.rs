//! Audio capture smoke test — records briefly from the default input device.
//!
//! Marked `#[ignore]`: it needs a working microphone. Run explicitly:
//!
//! ```text
//! cargo test -p lashon-core --test capture -- --ignored
//! ```

use std::thread;
use std::time::Duration;

use lashon_core::audio::{AudioCapture, TARGET_RATE};

#[test]
#[ignore = "requires a microphone"]
fn captures_audio_from_the_default_device() {
    let mut capture = AudioCapture::new();
    capture.start().expect("start capture");
    thread::sleep(Duration::from_millis(600));
    let pcm = capture.stop();
    assert!(!pcm.is_empty(), "captured no audio samples");
    // ~0.6 s of 16 kHz audio is on the order of thousands of samples.
    assert!(
        pcm.len() > TARGET_RATE as usize / 10,
        "implausibly few samples"
    );
}
