//! STT sidecar lifecycle and gRPC client.
//!
//! The sidecar is a Python gRPC server (`services/stt-sidecar`). It binds an
//! ephemeral loopback port and prints a two-line stdout handshake —
//! `LASHON_STT_TOKEN=<hex>` then `LASHON_STT_PORT=<n>`. We parse both, connect
//! a `tonic` client, and attach the token to every call so the sidecar can
//! reject any other local process. See `docs/adr/0002` and `docs/adr/0010`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::Channel;

use crate::stt_proto::stt;
use stt::stt_client::SttClient;

/// Prefix the sidecar prints with its per-process auth token, before the port.
const TOKEN_LINE_PREFIX: &str = "LASHON_STT_TOKEN=";

/// Prefix the sidecar prints once its gRPC server is listening.
const PORT_LINE_PREFIX: &str = "LASHON_STT_PORT=";

/// gRPC metadata key the auth token rides in. Must match `_AUTH_METADATA_KEY`
/// in `services/stt-sidecar/src/lashon_stt/server.py`.
const AUTH_METADATA_KEY: &str = "x-lashon-auth";

/// Max gRPC message size for the STT transport, in bytes. The gRPC default is
/// 4 MB, which caps a `TranscribeBytes` request at ~65 s of 16 kHz f32 PCM
/// (64 KB/s) — fine under the old 30 s take cap, but a take now runs up to the
/// 5-minute backstop (~19 MB), so the final decode of a long take was rejected
/// `ResourceExhausted` (docs/adr/0037). 64 MB leaves generous headroom. Must be
/// matched by `grpc.max_receive_message_length` in the Python sidecar's
/// `server.py` — change both together.
const MAX_GRPC_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// How long to wait for the sidecar to announce its port.
const PORT_WAIT: Duration = Duration::from_secs(8);

/// Parse a `LASHON_STT_PORT=<n>` line from the sidecar's stdout.
///
/// Pure — the parsing contract is unit-tested below.
pub fn parse_port_line(line: &str) -> Option<u16> {
    line.trim()
        .strip_prefix(PORT_LINE_PREFIX)
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|&port| port != 0)
}

/// Parse a `LASHON_STT_TOKEN=<hex>` line from the sidecar's stdout.
///
/// The token is rejected unless it is non-empty and purely ASCII alphanumeric
/// — the sidecar mints it with `secrets.token_hex`. Pure; unit-tested below.
pub fn parse_token_line(line: &str) -> Option<String> {
    line.trim()
        .strip_prefix(TOKEN_LINE_PREFIX)
        .filter(|token| !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_owned)
}

/// The sidecar's stdout handshake: its auth token and its gRPC port.
struct Handshake {
    port: u16,
    token: String,
}

/// Folds the sidecar's stdout lines into a [`Handshake`].
///
/// Split out from the IO loop so the contract — the token line must arrive
/// before the port line — is unit-testable.
#[derive(Default)]
struct HandshakeReader {
    token: Option<String>,
}

impl HandshakeReader {
    /// Process one stdout line. `Ok(Some(_))` once the port line completes the
    /// handshake, `Ok(None)` to keep reading, `Err` on a contract violation.
    fn push(&mut self, line: &str) -> Result<Option<Handshake>> {
        if let Some(token) = parse_token_line(line) {
            self.token = Some(token);
        } else if let Some(port) = parse_port_line(line) {
            let token = self
                .token
                .take()
                .ok_or_else(|| anyhow!("STT sidecar announced its port before its auth token"))?;
            return Ok(Some(Handshake { port, token }));
        }
        Ok(None)
    }
}

/// Attaches the sidecar's per-process auth token to every outgoing gRPC call,
/// so the sidecar can reject calls from any other local process (ADR-0010).
#[derive(Clone)]
pub struct AuthInterceptor {
    token: MetadataValue<Ascii>,
}

impl Interceptor for AuthInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> std::result::Result<tonic::Request<()>, tonic::Status> {
        request
            .metadata_mut()
            .insert(AUTH_METADATA_KEY, self.token.clone());
        Ok(request)
    }
}

/// A running STT sidecar: the child process, its port, and its auth token.
///
/// On Windows, [`Sidecar`] also owns a Win32 [`JobHandle`](job_object::JobHandle).
/// The sidecar process is assigned to the job at spawn time; the job carries
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so when the parent Lashon process
/// terminates — even abruptly, without running `Drop` — Windows kills the
/// sidecar with it. Without this, `kill_on_drop(true)` is best-effort and a
/// hard Ctrl+C of `cargo run` (or a Tauri panic) leaves the PyInstaller-frozen
/// sidecar alive, holding file locks on its `_internal\*.pyd` files and
/// breaking the next build.
pub struct Sidecar {
    child: Child,
    port: u16,
    token: MetadataValue<Ascii>,
    /// Owns the job; kept alive for the sidecar's lifetime so the OS holds the
    /// kill-on-close invariant. The leading underscore says: never read,
    /// only Drop matters.
    #[cfg(windows)]
    _job: job_object::JobHandle,
}

impl Sidecar {
    /// The loopback port the sidecar's gRPC server listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Open a gRPC client connected to this sidecar, authenticated with the
    /// sidecar's per-process token.
    pub async fn client(&self) -> Result<SttClient<InterceptedService<Channel, AuthInterceptor>>> {
        let channel = Channel::from_shared(format!("http://127.0.0.1:{}", self.port))
            .context("building the STT sidecar endpoint")?
            .connect()
            .await
            .context("connecting gRPC client to the STT sidecar")?;
        let interceptor = AuthInterceptor {
            token: self.token.clone(),
        };
        // Raise both directions off the 4 MB default so a long take's PCM
        // upload (and any large response) isn't rejected mid-dictation — the
        // server's receive limit is bumped to match (docs/adr/0037).
        Ok(SttClient::with_interceptor(channel, interceptor)
            .max_encoding_message_size(MAX_GRPC_MESSAGE_BYTES)
            .max_decoding_message_size(MAX_GRPC_MESSAGE_BYTES))
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        // Best-effort kill — the M0 sidecar holds no state worth a graceful stop.
        let _ = self.child.start_kill();
    }
}

/// Path to `services/stt-sidecar/src`, resolved from the crate location.
///
/// M0 runs the sidecar from source. `CARGO_MANIFEST_DIR` is baked at compile
/// time and is correct for `cargo` / `tauri dev` builds; a PyInstaller-frozen
/// sidecar replaces this path in Milestone M13.
fn sidecar_src_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR == <repo>/packages/shared-rust
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../services/stt-sidecar/src")
}

/// Executable extensions to try when resolving a program on `PATH`.
#[cfg(windows)]
const EXE_EXTS: &[&str] = &["", ".exe"];
#[cfg(not(windows))]
const EXE_EXTS: &[&str] = &[""];

/// Resolve a program to an absolute executable path by searching `PATH`.
///
/// Deliberately never searches the current working directory: Windows
/// `CreateProcess` — which `Command::new("python")` uses — would, letting a
/// `python.exe` dropped in the CWD hijack the sidecar launch (ADR-0010). A
/// `program` that already contains a path separator is taken as an explicit
/// path. `is_exe` decides whether a candidate is runnable: production passes
/// `Path::is_file`, tests inject a fake.
fn resolve_in_path(
    program: &str,
    path: Option<&OsStr>,
    exts: &[&str],
    is_exe: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let probe = |stem: &Path| -> Option<PathBuf> {
        exts.iter().find_map(|ext| {
            let mut candidate = stem.as_os_str().to_owned();
            candidate.push(ext);
            let candidate = PathBuf::from(candidate);
            is_exe(&candidate).then_some(candidate)
        })
    };
    if program.contains(['/', '\\']) {
        return probe(Path::new(program));
    }
    std::env::split_paths(path?).find_map(|dir| {
        // An empty `PATH` entry resolves to the current directory — skip it,
        // or a binary dropped in the CWD could hijack the launch.
        (!dir.as_os_str().is_empty())
            .then(|| probe(&dir.join(program)))
            .flatten()
    })
}

/// Resolve an absolute path to the Python interpreter that runs the STT
/// sidecar from source. `LASHON_PYTHON` overrides the choice; otherwise
/// `python`, then `python3`, are looked up on `PATH`.
fn resolve_python() -> Result<PathBuf> {
    let path = std::env::var_os("PATH");
    if let Some(explicit) = std::env::var("LASHON_PYTHON")
        .ok()
        .filter(|value| !value.is_empty())
    {
        return resolve_in_path(&explicit, path.as_deref(), EXE_EXTS, |p| p.is_file())
            .ok_or_else(|| anyhow!("LASHON_PYTHON=\"{explicit}\" was not found"));
    }
    ["python", "python3"]
        .into_iter()
        .find_map(|name| resolve_in_path(name, path.as_deref(), EXE_EXTS, |p| p.is_file()))
        .ok_or_else(|| anyhow!("no 'python' interpreter found on PATH — set LASHON_PYTHON"))
}

/// Build the command that launches the STT sidecar.
fn sidecar_command() -> Result<Command> {
    // `LASHON_STT_SIDECAR` overrides with a direct executable path (a frozen
    // build, or an integration test). Otherwise run the Python module from
    // source, resolving the interpreter to an absolute path first.
    let mut command = match std::env::var_os("LASHON_STT_SIDECAR") {
        Some(path) => Command::new(path),
        None => {
            let python =
                resolve_python().context("locating the Python interpreter for the STT sidecar")?;
            let mut command = Command::new(python);
            command
                .arg("-m")
                .arg("lashon_stt.server")
                .env("PYTHONPATH", sidecar_src_dir());
            command
        }
    };
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    // Kill the sidecar if its `Child` is dropped without a clean handoff — e.g.
    // when `spawn()` returns early on a handshake timeout. Without this the
    // orphaned process survives, and a polling caller leaks one per retry.
    command.kill_on_drop(true);
    // CREATE_NO_WINDOW — the sidecar must never flash a console window when the
    // GUI app spawns it (the frozen build is a console-subsystem executable).
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    Ok(command)
}

/// Spawn the STT sidecar and wait until it announces its handshake.
pub async fn spawn() -> Result<Sidecar> {
    let mut child = sidecar_command()?
        .spawn()
        .context("spawning the STT sidecar process")?;

    // On Windows, attach the child to a Job Object **before** doing anything
    // else with it: a handshake timeout from here on returns through
    // `Sidecar::drop` / the job's drop, both of which kill the process. The
    // job is the durable guarantee — `kill_on_drop` only fires for graceful
    // exits.
    #[cfg(windows)]
    let job = attach_to_kill_on_close_job(&child)
        .context("attaching the STT sidecar to its kill-on-close job object")?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("STT sidecar stdout was not captured"))?;

    let handshake = timeout(PORT_WAIT, read_handshake(stdout))
        .await
        .context("timed out waiting for the STT sidecar handshake")??;

    let token = MetadataValue::try_from(handshake.token)
        .context("STT sidecar auth token is not valid gRPC metadata")?;

    // Only the port is logged — the token never reaches a log line.
    tracing::debug!(port = handshake.port, "STT sidecar listening");
    Ok(Sidecar {
        child,
        port: handshake.port,
        token,
        #[cfg(windows)]
        _job: job,
    })
}

/// Create a Win32 Job Object configured to kill its members when its handle
/// is closed, then assign the freshly-spawned sidecar to it. Used by
/// `sidecar::spawn` and `llama_server::spawn` alike.
#[cfg(windows)]
pub(crate) fn attach_to_kill_on_close_job(child: &Child) -> Result<job_object::JobHandle> {
    use std::os::windows::io::RawHandle;
    let raw: RawHandle = child
        .raw_handle()
        .ok_or_else(|| anyhow!("STT sidecar process has no Win32 handle"))?;
    let job = job_object::JobHandle::new()?;
    // Safety: `raw` is the OS handle of a process this function just spawned;
    // it is valid for as long as `child` is alive, which the caller guarantees
    // outlives this call.
    unsafe { job.assign_process(raw) }?;
    Ok(job)
}

/// Win32 Job Object wrapper — on Windows, every spawned sidecar is assigned to
/// one so the OS kills it if the parent Lashon process exits without running
/// destructors. `kill_on_drop` is best-effort; the job is the durable
/// guarantee. See [`Sidecar`]. Shared with `crate::llama_server` (and any
/// future subprocess that needs the same posture).
#[cfg(windows)]
pub(crate) mod job_object {
    use std::os::windows::io::RawHandle;

    use anyhow::{anyhow, bail, Result};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// Owns one Win32 Job Object handle. Dropping the last handle to a job
    /// configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the OS kill
    /// every process in that job — including grandchildren the parent never
    /// got to clean up.
    pub struct JobHandle(HANDLE);

    // A Job Object handle is a kernel object; the Win32 APIs we call against
    // it are documented thread-safe. Sending it across threads is safe.
    unsafe impl Send for JobHandle {}
    unsafe impl Sync for JobHandle {}

    impl JobHandle {
        /// Create an unnamed Job Object with `KILL_ON_JOB_CLOSE` set.
        pub fn new() -> Result<Self> {
            // Safety: a null `lpJobAttributes` and a null name request the
            // default security descriptor and an anonymous job — both are
            // documented as valid inputs.
            let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
                .map_err(|err| anyhow!("CreateJobObjectW: {err}"))?;
            if handle.is_invalid() {
                bail!("CreateJobObjectW returned an invalid handle");
            }
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let bytes = std::mem::size_of_val(&info) as u32;
            // Safety: `info` is a valid, fully-initialized
            // JOBOBJECT_EXTENDED_LIMIT_INFORMATION for the lifetime of the
            // call; `bytes` is its actual size.
            unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    bytes,
                )
            }
            .map_err(|err| {
                // Make sure the half-built job doesn't leak.
                let _ = unsafe { CloseHandle(handle) };
                anyhow!("SetInformationJobObject: {err}")
            })?;
            Ok(JobHandle(handle))
        }

        /// Add a process to the job. The caller guarantees `process` is a
        /// valid, still-open Win32 process handle (e.g. taken from
        /// `tokio::process::Child::raw_handle`).
        ///
        /// # Safety
        ///
        /// `process` must be a valid `HANDLE` to an open Win32 process. Passing
        /// a stale or closed handle is undefined behaviour.
        pub unsafe fn assign_process(&self, process: RawHandle) -> Result<()> {
            // `RawHandle` is `*mut c_void`; `HANDLE` is also a `*mut c_void`
            // newtype — the cast is the documented bridge from std to windows.
            let process_handle = HANDLE(process as *mut _);
            AssignProcessToJobObject(self.0, process_handle)
                .map_err(|err| anyhow!("AssignProcessToJobObject: {err}"))?;
            Ok(())
        }
    }

    impl Drop for JobHandle {
        fn drop(&mut self) {
            // Closing the last handle is what triggers KILL_ON_JOB_CLOSE.
            // Errors here are non-actionable — log nothing; the process is
            // already shutting down.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

async fn read_handshake(stdout: tokio::process::ChildStdout) -> Result<Handshake> {
    let mut lines = BufReader::new(stdout).lines();
    let mut reader = HandshakeReader::default();
    while let Some(line) = lines
        .next_line()
        .await
        .context("reading STT sidecar stdout")?
    {
        if let Some(handshake) = reader.push(&line)? {
            return Ok(handshake);
        }
    }
    Err(anyhow!("STT sidecar exited before announcing a port"))
}

/// A sidecar health-probe result, serialized to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthReport {
    pub ok: bool,
    /// True once the STT model is warm — i.e. transcription will succeed. On
    /// first run the sidecar downloads the model, so this stays false for a
    /// while after `ok` becomes true.
    pub model_ready: bool,
    pub detail: String,
}

/// Tauri-managed state: the running sidecar, spawned lazily on first use.
#[derive(Default)]
pub struct SidecarState(Mutex<Option<Arc<Sidecar>>>);

/// Get the running STT sidecar, spawning it lazily on first use.
pub async fn ready_sidecar(state: &SidecarState) -> Result<Arc<Sidecar>> {
    let mut slot = state.0.lock().await;
    if slot.is_none() {
        *slot = Some(Arc::new(spawn().await?));
    }
    Ok(slot.as_ref().expect("sidecar was just set").clone())
}

/// Probe the STT sidecar's `HealthCheck`, spawning it first if needed.
///
/// Never fails: a transport or process error becomes
/// `HealthReport { ok: false, .. }` so the debug surface can always render.
pub async fn healthcheck(state: &SidecarState) -> HealthReport {
    match probe(state).await {
        Ok(report) => report,
        Err(error) => HealthReport {
            ok: false,
            model_ready: false,
            detail: format!("{error:#}"),
        },
    }
}

async fn probe(state: &SidecarState) -> Result<HealthReport> {
    let sidecar = ready_sidecar(state).await?;
    let mut client = sidecar.client().await?;
    let response = client
        .health_check(stt::HealthCheckRequest {})
        .await
        .context("STT sidecar HealthCheck RPC")?
        .into_inner();

    let serving = response.status == stt::ServingStatus::Serving as i32;
    let detail = if response.detail.is_empty() {
        format!("status code {}", response.status)
    } else {
        response.detail
    };
    Ok(HealthReport {
        ok: serving,
        model_ready: response.model_ready,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_port_line_accepts_the_contract_line() {
        assert_eq!(parse_port_line("LASHON_STT_PORT=44676"), Some(44676));
        assert_eq!(parse_port_line("LASHON_STT_PORT=44676\n"), Some(44676));
        assert_eq!(parse_port_line("  LASHON_STT_PORT=1420  "), Some(1420));
    }

    #[test]
    fn parse_port_line_rejects_everything_else() {
        assert_eq!(parse_port_line("starting up..."), None);
        assert_eq!(parse_port_line("LASHON_STT_PORT="), None);
        assert_eq!(parse_port_line("LASHON_STT_PORT=0"), None);
        assert_eq!(parse_port_line("LASHON_STT_PORT=99999999"), None);
        assert_eq!(parse_port_line("PORT=8080"), None);
    }

    #[test]
    fn parse_token_line_accepts_a_hex_token() {
        assert_eq!(
            parse_token_line("LASHON_STT_TOKEN=deadbeef00"),
            Some("deadbeef00".to_string())
        );
        assert_eq!(
            parse_token_line("  LASHON_STT_TOKEN=abc123\n"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn parse_token_line_rejects_everything_else() {
        assert_eq!(parse_token_line("LASHON_STT_TOKEN="), None);
        assert_eq!(parse_token_line("LASHON_STT_TOKEN=has space"), None);
        assert_eq!(parse_token_line("LASHON_STT_TOKEN=bad/char"), None);
        assert_eq!(parse_token_line("LASHON_STT_PORT=44676"), None);
    }

    #[test]
    fn handshake_needs_the_token_before_the_port() {
        let mut reader = HandshakeReader::default();
        assert!(reader
            .push("starting up...")
            .expect("noise is ignored")
            .is_none());
        assert!(reader
            .push("LASHON_STT_TOKEN=cafe1234")
            .expect("token line")
            .is_none());
        let handshake = reader
            .push("LASHON_STT_PORT=51000")
            .expect("port line completes the handshake")
            .expect("handshake is ready");
        assert_eq!(handshake.port, 51000);
        assert_eq!(handshake.token, "cafe1234");
    }

    #[test]
    fn handshake_rejects_a_port_before_its_token() {
        let mut reader = HandshakeReader::default();
        assert!(
            reader.push("LASHON_STT_PORT=51000").is_err(),
            "a port line with no preceding token must be a contract violation"
        );
    }

    #[test]
    fn resolve_in_path_returns_the_first_match_in_path_order() {
        let path = std::env::join_paths(["/first", "/second"]).expect("join PATH");
        // Both directories "contain" python; the earlier one must win.
        let found = resolve_in_path("python", Some(path.as_os_str()), &[""], |_| true);
        assert_eq!(
            found.as_deref(),
            Some(PathBuf::from("/first").join("python").as_path())
        );
    }

    #[test]
    fn resolve_in_path_tries_executable_extensions() {
        let path = std::env::join_paths(["/bin"]).expect("join PATH");
        let want = PathBuf::from("/bin").join("python.exe");
        let found = resolve_in_path("python", Some(path.as_os_str()), &["", ".exe"], |p| {
            p == want
        });
        assert_eq!(found.as_deref(), Some(want.as_path()));
    }

    #[test]
    fn resolve_in_path_never_searches_the_current_directory() {
        // An empty PATH entry resolves to the CWD; searching it would let a
        // `python` dropped in the working directory hijack the launch.
        let path = std::env::join_paths(["", "ignored"]).expect("join PATH");
        let found = resolve_in_path("python", Some(path.as_os_str()), &[""], |p| {
            // Only a bare, unanchored `python` — what an empty entry yields.
            p == Path::new("python")
        });
        assert_eq!(found, None, "the empty (CWD) PATH entry must be skipped");
    }

    #[test]
    fn resolve_in_path_takes_an_explicit_path_verbatim() {
        let explicit = if cfg!(windows) {
            r"C:\python\python.exe"
        } else {
            "/opt/python/bin/python"
        };
        let found = resolve_in_path(explicit, None, &[""], |p| p == Path::new(explicit));
        assert_eq!(found.as_deref(), Some(Path::new(explicit)));
    }

    #[test]
    fn resolve_in_path_returns_none_when_nothing_matches() {
        let path = std::env::join_paths(["/nowhere"]).expect("join PATH");
        assert_eq!(
            resolve_in_path("python", Some(path.as_os_str()), &[""], |_| false),
            None
        );
        // A bare name with no PATH set resolves to nothing.
        assert_eq!(resolve_in_path("python", None, &[""], |_| true), None);
    }

    #[test]
    fn health_report_serializes_for_the_frontend() {
        let json = serde_json::to_string(&HealthReport {
            ok: true,
            model_ready: true,
            detail: "תקין".to_string(),
        })
        .expect("serialize HealthReport");
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"model_ready\":true"));
        assert!(json.contains("תקין"));
    }

    #[test]
    fn sidecar_src_dir_points_at_the_python_package() {
        assert!(sidecar_src_dir().ends_with("services/stt-sidecar/src"));
    }

    /// Drops the job handle while a real Windows child process is in it and
    /// asserts the OS killed the child. This is the whole point of the job:
    /// `kill_on_drop` is best-effort, the job is the durable guarantee.
    #[cfg(windows)]
    #[test]
    fn job_object_kills_its_child_on_drop() {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        use std::thread::sleep;
        use std::time::{Duration, Instant};

        // A process that runs long enough to outlive the job if the job
        // *didn't* kill it. `timeout` is a built-in Windows command; with
        // `nobreak` it waits the full 30 s without responding to keys.
        //
        // CREATE_NO_WINDOW — the same focus-steal guard the production spawn
        // sites use (sidecar, llama-server, run_command, recipe runtime).
        // Without it, running this test from a console-less parent (an IDE or
        // background test runner) pops a `cmd` console window that steals
        // foreground focus. See .claude/rules/recipes.md.
        let mut child = Command::new("cmd")
            .args(["/c", "timeout", "/t", "30", "/nobreak"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the long-lived dummy process");

        {
            let job = super::job_object::JobHandle::new().expect("create job");
            // Safety: `child` is alive and its `RawHandle` is valid for the
            // duration of `assign_process`.
            unsafe { job.assign_process(child.as_raw_handle()) }.expect("assign");
            // `job` is dropped here -> `CloseHandle` -> the OS kills the child.
        }

        // The OS delivers KILL_ON_JOB_CLOSE asynchronously; poll briefly.
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(Some(_status)) = child.try_wait() {
                return; // killed as expected
            }
            sleep(Duration::from_millis(25));
        }

        // Cleanup if the test is about to fail so we don't leak.
        let _ = child.kill();
        let _ = child.wait();
        panic!("child survived after the job handle was dropped");
    }
}
