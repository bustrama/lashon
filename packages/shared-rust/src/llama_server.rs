//! Lifecycle management for the bundled `llama-server` subprocess
//! (docs/adr/0025).
//!
//! Mirrors the STT sidecar's posture (see [`crate::sidecar`]): on
//! Windows the spawned child is attached to a `KILL_ON_JOB_CLOSE` job
//! object so a parent crash never leaves an orphan holding the GGUF
//! file open or the GPU memory pinned. On other platforms the
//! `kill_on_drop` flag handles the same job for graceful shutdown.
//!
//! The server speaks OpenAI-compatible HTTP on a loopback port the OS
//! picks for us; `LlamaServer::base_url` returns the
//! `http://127.0.0.1:<port>/v1` shape `OpenAiCompatLlmProvider`
//! expects. `LocalLlmProvider` (in `crate::llm::local`) is the
//! consumer.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Instant};

#[cfg(windows)]
use crate::sidecar::{attach_to_kill_on_close_job, job_object};

/// Environment variable that overrides the bundled `llama-server.exe`
/// path. Set by integration tests + by developers who want to point
/// at a freshly-built llama.cpp binary instead of the bundled one.
pub const LLAMA_SERVER_ENV: &str = "LASHON_LLAMA_SERVER";

/// How long to wait for the server's `/health` endpoint to first
/// return `200 OK`. Cold model load (1.83 GB GGUF) plus Vulkan device
/// init on a busy box can take 10–30 s; the budget is forgiving.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);

/// How often the spawn loop pokes `/health` while waiting for the
/// server to come up. 100 ms keeps the readiness-to-first-chat latency
/// tight without burning CPU.
const HEALTH_POLL: Duration = Duration::from_millis(100);

/// A live `llama-server` instance. Drop kills the child and, on
/// Windows, closes the job object that pins it.
///
/// Construct via [`spawn`]. The instance is `Send + Sync`; the Tauri
/// shell holds it inside an `Arc<Mutex<...>>` next to the STT
/// `SidecarState`.
pub struct LlamaServer {
    child: Child,
    port: u16,
    model_path: PathBuf,
    /// Owns the kill-on-close job for the spawned child. Never read
    /// after assignment — only `Drop` matters.
    #[cfg(windows)]
    _job: job_object::JobHandle,
}

impl LlamaServer {
    /// Loopback port the server is bound to.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Path of the GGUF this server was launched against. Used when
    /// the user switches model in the Hub — we compare and respawn
    /// only on actual changes.
    pub fn model_path(&self) -> &PathBuf {
        &self.model_path
    }

    /// The full `http://127.0.0.1:<port>/v1` base URL the
    /// `OpenAiCompatLlmProvider` consumes. Same shape as Ollama local.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        // Best-effort graceful kill; the job object is the durable
        // guarantee on Windows. Errors here are uninteresting — the
        // server is going away one way or the other.
        let _ = self.child.start_kill();
    }
}

/// Resolve where `llama-server.exe` lives.
///
/// - If `LASHON_LLAMA_SERVER` is set, use that path verbatim (dev /
///   integration tests).
/// - Otherwise fall back to `bundled_exe` — the path the Tauri shell
///   computes from `tauri::path::resource_dir()`. Passing the resource
///   path in here keeps `lashon-core` free of any `tauri::*` deps.
pub fn resolve_server_exe(bundled_exe: PathBuf) -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os(LLAMA_SERVER_ENV) {
        let path = PathBuf::from(explicit);
        if !path.is_file() {
            bail!(
                "{LLAMA_SERVER_ENV}=\"{}\" does not point at a file",
                path.display()
            );
        }
        return Ok(path);
    }
    if !bundled_exe.is_file() {
        bail!(
            "bundled llama-server binary not found at {}",
            bundled_exe.display()
        );
    }
    Ok(bundled_exe)
}

/// Ask the OS for a free loopback port. Bind, read the assigned port,
/// drop the listener — between this drop and the child's bind there
/// is a tiny TOCTOU window, but the cost of losing the race is just
/// `spawn` returning an error and the caller retrying.
fn pick_free_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("binding 127.0.0.1:0 for port pick")?;
    let addr = listener
        .local_addr()
        .context("reading the bound port number")?;
    Ok(addr.port())
}

/// Configuration the Tauri shell hands `spawn` on each launch.
pub struct SpawnConfig {
    /// Absolute path to `llama-server.exe` (or its OS equivalent).
    pub server_exe: PathBuf,
    /// Absolute path to the GGUF file the server should load. The
    /// caller (`crate::model::local_llm_resolved_path`) already
    /// verifies the file exists; we pass it through.
    pub model_path: PathBuf,
    /// Context window in tokens the server is launched with. The M8.1
    /// dispatcher assumed 4096 was enough (docs/adr/0025 §8); the M8.2
    /// OS-control tranche grew the system prompt past that, so the
    /// Tauri shell now spawns with 16 K. Qwen3-1.7B natively supports
    /// 32 K — set this to whatever the chain length demands within the
    /// model's window. KV-cache memory scales linearly with context.
    pub ctx_size: u32,
    /// Number of layers to offload to GPU. `999` means "all of them"
    /// — llama-server clamps internally to the model's layer count.
    /// With Vulkan unavailable, llama-server falls back to CPU
    /// automatically; the flag is a no-op in that path.
    pub n_gpu_layers: u32,
}

/// Spawn a fresh `llama-server` and wait until `/health` returns
/// `200 OK`. On success, the returned [`LlamaServer`] owns the child;
/// dropping it kills the subprocess.
pub async fn spawn(config: SpawnConfig) -> Result<LlamaServer> {
    if !config.model_path.is_file() {
        bail!(
            "llama-server: model file not found at {}",
            config.model_path.display()
        );
    }
    let port = pick_free_loopback_port()?;
    let mut command = Command::new(&config.server_exe);
    command
        .arg("--model")
        .arg(&config.model_path)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--ctx-size")
        .arg(config.ctx_size.to_string())
        .arg("--n-gpu-layers")
        .arg(config.n_gpu_layers.to_string())
        // `--jinja` enables Jinja chat templates so Qwen3's embedded
        // template (including its `<tool_call>` tool-calling format)
        // is used.
        .arg("--jinja")
        // Single user, single slot — we don't multiplex.
        .arg("--parallel")
        .arg("1")
        // Persistent KV-cache reuse across turns. Without this, each
        // Command-mode turn re-prefills the entire prompt — the M8.2
        // dispatch sends a ~5 K-token system prompt + ~600-token tool
        // catalogue verbatim every turn, plus the growing
        // conversation tail. With `--cache-reuse N`, any continuation
        // whose first N tokens match a previous request shares that
        // prefix's KV state and only the divergent suffix is
        // prefilled. The stable prefix (system + tools) is by far the
        // largest fraction of every turn's prompt, so the expected
        // reduction in turn-2+ prefill cost is 60–90% on a typical
        // chain. 256 is a conservative floor — most chains share
        // multiple-K-token prefixes and the cache will hit on much
        // more than 256 tokens; the flag just sets the minimum match
        // length, not a cap. Tracks the upstream llama.cpp guidance
        // for agent loops.
        .arg("--cache-reuse")
        .arg("256")
        // Expose the `/slots` REST endpoint so a future PR can
        // save / load slot KV state across app restarts (warmup on
        // first chat) and inspect slot reuse stats during debugging.
        // Off by default; no functional behaviour change today beyond
        // the endpoint becoming available.
        .arg("--slots")
        // The Hub triggers warmup explicitly on the model-install path
        // via a one-token chat; the spawn-time warmup just doubles
        // first-launch latency.
        .arg("--no-warmup")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Kill the child if its `Child` is dropped without a clean
        // handoff — e.g. when `spawn()` returns early on a health
        // timeout. The job object is the durable guarantee; this is
        // belt-and-braces.
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW — the GUI app must never flash a console
        // window when it spawns llama-server. Same rationale as the
        // STT sidecar.
        command.creation_flags(0x0800_0000);
    }

    tracing::info!(
        server = %config.server_exe.display(),
        model = %config.model_path.display(),
        port,
        ctx = config.ctx_size,
        gpu_layers = config.n_gpu_layers,
        cache_reuse = 256,
        slots_endpoint = true,
        "spawning llama-server"
    );

    let mut child = command.spawn().context("spawning llama-server")?;

    // Attach to the kill-on-close job object **before** the health
    // wait — a hang here returns through Drop, which we want the job
    // to clean up if the OS reaps us mid-wait.
    #[cfg(windows)]
    let job = attach_to_kill_on_close_job(&child)
        .context("attaching llama-server to its kill-on-close job object")?;

    // Drain stdout + stderr into Lashon's tracing. Without a reader the
    // piped buffers eventually fill and llama-server blocks on its next
    // write — a latent hang that would only manifest after thousands of
    // chat turns. The forwarder also parses the slot-stats lines
    // ("slot update_slots: id N | task M | … n_past = X, n_tokens = Y")
    // and re-emits them as a structured INFO event on
    // `lashon::llama_server::slot` so the user can measure
    // `--cache-reuse` (`docs/adr/0025`, PR #67) effectiveness from
    // Lashon's own logs without grepping subprocess output. Everything
    // else falls through to DEBUG under `lashon::llama_server`.
    if let Some(stdout) = child.stdout.take() {
        forward_llama_lines(stdout, "stdout");
    }
    if let Some(stderr) = child.stderr.take() {
        forward_llama_lines(stderr, "stderr");
    }

    let server = LlamaServer {
        child,
        port,
        model_path: config.model_path.clone(),
        #[cfg(windows)]
        _job: job,
    };

    timeout(HEALTH_TIMEOUT, wait_healthy(port))
        .await
        .context("timed out waiting for llama-server /health")??;

    tracing::info!(port, base_url = %server.base_url(), "llama-server is healthy");
    Ok(server)
}

/// Poll `GET http://127.0.0.1:<port>/health` until it returns `200`.
/// llama-server reports `503 Service Unavailable` while the model
/// loads; both `503` and "connection refused" trigger another poll.
async fn wait_healthy(port: u16) -> Result<()> {
    let url = format!("http://127.0.0.1:{port}/health");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .context("building the health-probe HTTP client")?;
    let started = Instant::now();
    loop {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(_) | Err(_) => {
                if started.elapsed() > HEALTH_TIMEOUT {
                    return Err(anyhow!(
                        "llama-server did not become healthy within {:?}",
                        HEALTH_TIMEOUT
                    ));
                }
                sleep(HEALTH_POLL).await;
            }
        }
    }
}

/// Tauri-managed state: the running `llama-server`, spawned lazily on
/// first chat. Matches the [`crate::sidecar::SidecarState`] pattern so
/// the Tauri shell wires it the same way (`.manage(...)` in the
/// builder, then `tauri::State<'_, LlamaServerState>` in commands).
#[derive(Default)]
pub struct LlamaServerState(Mutex<Option<Arc<LlamaServer>>>);

/// Get a running `llama-server` configured for `requested.model_path`.
///
/// Behaviour:
///   - If nothing is running, spawn fresh from `requested`.
///   - If a server is running against the same model, return it.
///   - If a server is running against a different model, stop the old
///     one and spawn a new one. The model file (1.83+ GB GGUF) and
///     the GPU layers move with this restart — the cost is the cold
///     load latency the Hub already surfaces.
pub async fn ready_llama_server(
    state: &LlamaServerState,
    requested: SpawnConfig,
) -> Result<Arc<LlamaServer>> {
    let mut slot = state.0.lock().await;
    if let Some(running) = slot.as_ref() {
        if running.model_path() == &requested.model_path {
            return Ok(running.clone());
        }
        tracing::info!(
            old = %running.model_path().display(),
            new = %requested.model_path.display(),
            "llama-server model changed — restarting"
        );
        // Drop the Arc so the server shuts down before we spawn a
        // replacement; otherwise both may briefly hold the GGUF open
        // and we double-allocate GPU memory.
        *slot = None;
    }
    let server = Arc::new(spawn(requested).await?);
    *slot = Some(server.clone());
    Ok(server)
}

/// Stop the currently-running server (if any). Used by the Hub when
/// the user deletes the active model or switches to a cloud provider.
pub async fn stop_llama_server(state: &LlamaServerState) {
    let mut slot = state.0.lock().await;
    *slot = None;
}

/// Parsed `slot update_slots: …` line emitted by llama-server on every
/// prompt-processing turn.
///
/// Field semantics, per llama.cpp upstream (`tools/server/server.cpp`,
/// the `slot.print_timings`-adjacent log site):
///
/// - `n_past` — total prompt-processing position after this batch, i.e.
///   the size of the prompt the model just consumed.
/// - `n_tokens` — tokens **newly** prefilled this turn. With
///   `--cache-reuse N`, this is just the divergent suffix; the cached
///   prefix is not re-prefilled.
///
/// Reuse therefore = `n_past − n_tokens`. On Lashon's typical Command-
/// mode turn (~5 K-token system prompt + ~600-token tool catalogue +
/// growing tail), turns 2+ should show `n_tokens` ≪ `n_past`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotStats {
    slot: u32,
    task: u64,
    n_past: u64,
    n_tokens: u64,
}

/// Extract a `<marker><whitespace?><digits>` value from `line`,
/// ignoring the rest of the string. Tolerates leading whitespace after
/// the marker because llama-server right-aligns its numeric fields
/// (`id  0`, `task 12`); the terminator is the first non-digit
/// character, which matches the surrounding `,`, ` `, and `|` separators.
fn parse_uint_after<T: std::str::FromStr>(line: &str, marker: &str) -> Option<T> {
    let after = line.split_once(marker)?.1.trim_start();
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Pull `SlotStats` out of a llama-server log line, returning `None` on
/// anything that doesn't carry the four fields. The line ordering is
/// stable upstream (`id` → `task` → … → `n_past` → `n_tokens`); we
/// don't depend on the exact prefix because llama.cpp has reshuffled
/// the leading log decoration between releases (`slot update_slots:`
/// vs `slot update_slots :` vs `slot update_slots`).
fn parse_slot_stats(line: &str) -> Option<SlotStats> {
    if !line.contains("slot ") {
        return None;
    }
    let slot: u32 = parse_uint_after(line, "id ")?;
    let task: u64 = parse_uint_after(line, "task ")?;
    let n_past: u64 = parse_uint_after(line, "n_past = ")?;
    let n_tokens: u64 = parse_uint_after(line, "n_tokens = ")?;
    Some(SlotStats {
        slot,
        task,
        n_past,
        n_tokens,
    })
}

/// Render a reuse percentage `cached / total` with one decimal place.
/// `total == 0` (degenerate) returns `0.0`.
fn reuse_percentage(cached: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (cached as f64 / total as f64) * 100.0
}

/// Pump a llama-server output pipe line-by-line into Lashon's
/// `tracing` subscriber. The task lives for the lifetime of the pipe —
/// EOF (the server exiting) ends the loop and the spawned task
/// returns.
///
/// `stream` is the human label (`"stdout"` / `"stderr"`) attached as a
/// tracing field so post-hoc log readers can distinguish the two
/// streams. We don't differentiate semantically: llama-server's noisy
/// startup banner comes out of stderr; the slot-stats lines come out
/// of stdout in modern builds but landed on stderr in earlier
/// releases. Forwarding both keeps the parser format-stable across
/// upgrades.
fn forward_llama_lines<R: AsyncRead + Unpin + Send + 'static>(reader: R, stream: &'static str) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(stats) = parse_slot_stats(&line) {
                let cached = stats.n_past.saturating_sub(stats.n_tokens);
                tracing::info!(
                    target: "lashon::llama_server::slot",
                    slot = stats.slot,
                    task = stats.task,
                    prompt_tokens = stats.n_past,
                    prefilled = stats.n_tokens,
                    cached,
                    reuse_pct = format!("{:.1}", reuse_percentage(cached, stats.n_past)),
                    stream,
                    "llama-server slot turn"
                );
            } else {
                tracing::debug!(target: "lashon::llama_server", stream, "{line}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that mutate the `LLAMA_SERVER_ENV` process-global env var
    /// must run serially — Rust's test runner is parallel by default,
    /// so without this guard one test's `set_var` can race another
    /// test's `remove_var` and the override stops being visible. The
    /// race was harmless when the only crate-side tests numbered ~200;
    /// the M8.2 OS-tools tranche pushed the parallel count high enough
    /// that the race fired regularly.
    static ENV_VAR_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_server_exe_honours_env_override() {
        let _guard = ENV_VAR_LOCK.lock().unwrap();
        // The env-var override must point at a real file. Use the
        // current cargo binary as a stand-in; it's guaranteed to
        // exist on any host running this test.
        let cargo_path = std::env::var_os("CARGO").map(PathBuf::from);
        let cargo_path = match cargo_path {
            Some(p) if p.is_file() => p,
            _ => return, // skip — no CARGO env in this run
        };
        std::env::set_var(LLAMA_SERVER_ENV, &cargo_path);
        let resolved = resolve_server_exe(PathBuf::from("/nonexistent/bundled")).unwrap();
        assert_eq!(resolved, cargo_path);
        std::env::remove_var(LLAMA_SERVER_ENV);
    }

    #[test]
    fn resolve_server_exe_requires_a_real_bundled_file() {
        let _guard = ENV_VAR_LOCK.lock().unwrap();
        std::env::remove_var(LLAMA_SERVER_ENV);
        let err = resolve_server_exe(PathBuf::from("/definitely/not/here.exe"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("not found"),
            "missing-file error must be explicit: {err}"
        );
    }

    #[test]
    fn pick_free_loopback_port_returns_a_real_port() {
        let port = pick_free_loopback_port().expect("loopback should always have ports");
        assert!(port > 0, "OS-assigned port must be non-zero");
    }

    /// Canonical "prompt done" line from upstream llama.cpp ~build 4500+.
    /// The parser must extract all four fields verbatim so the tracing
    /// event surfaces the right reuse numbers.
    #[test]
    fn parse_slot_stats_extracts_canonical_prompt_done_line() {
        let line =
            "slot update_slots: id  0 | task 12 | prompt done, n_past = 5234, n_tokens = 1734";
        let stats = parse_slot_stats(line).expect("canonical line must parse");
        assert_eq!(stats.slot, 0);
        assert_eq!(stats.task, 12);
        assert_eq!(stats.n_past, 5234);
        assert_eq!(stats.n_tokens, 1734);
    }

    /// "prompt processing progress" lines carry the same fields plus
    /// a `progress = 0.331` tail. The parser ignores the tail and is
    /// not confused by the trailing fields.
    #[test]
    fn parse_slot_stats_handles_progress_lines() {
        let line = "slot update_slots: id  1 | task 47 | prompt processing progress, \
                    n_past = 5234, n_tokens = 1734, progress = 0.331";
        let stats = parse_slot_stats(line).expect("progress line must parse");
        assert_eq!(stats.slot, 1);
        assert_eq!(stats.task, 47);
        assert_eq!(stats.n_past, 5234);
        assert_eq!(stats.n_tokens, 1734);
    }

    /// Lines without all four fields fall through to DEBUG forwarding.
    /// Specifically: `slot release: ...` carries `n_past` but no
    /// `n_tokens`; `slot launch_slot_: ...` carries neither.
    #[test]
    fn parse_slot_stats_returns_none_when_fields_missing() {
        assert!(parse_slot_stats(
            "slot release: id  0 | task 12 | stop processing: n_past = 5267, truncated = 0"
        )
        .is_none());
        assert!(parse_slot_stats("slot launch_slot_: id  0 | task 12 | processing task").is_none());
        assert!(parse_slot_stats("a non-slot log line about model loading").is_none());
    }

    /// Cache reuse is the most useful number — the parser test above
    /// guarantees the right inputs, this guarantees the right output
    /// arithmetic.
    #[test]
    fn reuse_percentage_matches_expectations() {
        // No reuse — every token freshly prefilled.
        assert!((reuse_percentage(0, 5234) - 0.0).abs() < 0.01);
        // Full reuse — divergent suffix was zero tokens (degenerate).
        assert!((reuse_percentage(5234, 5234) - 100.0).abs() < 0.01);
        // The expected steady-state on Lashon's Command-mode chains:
        // ~5500 cached / ~5750 total ≈ 95.6%. The whole point of
        // `--cache-reuse` is to put us in this band on turn 2+.
        assert!((reuse_percentage(5500, 5750) - 95.65).abs() < 0.05);
    }

    /// Degenerate guard — empty prompt should not panic on division.
    #[test]
    fn reuse_percentage_handles_zero_total() {
        assert!((reuse_percentage(0, 0) - 0.0).abs() < 0.01);
    }
}
