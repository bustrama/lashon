//! What `ottid-hook` does with one permission request.
//!
//! It reads the hook's input, finds Ottid through the bridge file, checks
//! that the endpoint holds the token, sends the request, and waits for the
//! answer. Every way that can go wrong is a [`Fallback`]: the hook prints
//! nothing and Claude Code asks in its own prompt. An allow is printed only
//! for an answer whose MAC proves it came from this exchange; nothing here
//! ever allows or denies on its own.

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite};

use super::auth::{Nonce, Purpose, Session, Token, MAC_LEN};
use super::endpoint::{self, BridgeFile, BridgeFileError, BRIDGE_FILE};
use super::hook::{self, InputError, Verdict};
use super::message::{Answer, Ask, Challenge, Hello};
use super::wire::{self, FrameError, MAX_ANSWER, MAX_ASK, MAX_CHALLENGE, MAX_HELLO};
use super::{CLIENT_WAIT, CONNECT_TIMEOUT, HANDSHAKE_TIMEOUT, VERSION};

/// Where the client looks and how long it waits.
#[derive(Debug, Clone)]
pub struct Options {
    pub agent: super::Agent,
    pub bridge_file: PathBuf,
    pub connect_timeout: Duration,
    pub handshake_timeout: Duration,
    /// How long to wait for the user's answer.
    pub wait: Duration,
}

impl Options {
    pub fn new(bridge_file: PathBuf) -> Self {
        Self {
            agent: super::Agent::Claude,
            bridge_file,
            connect_timeout: CONNECT_TIMEOUT,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            wait: CLIENT_WAIT,
        }
    }

    /// The bridge file where Ottid writes it for this user.
    pub fn for_this_user() -> Option<Self> {
        endpoint::default_dir().map(|dir| Self::new(dir.join(BRIDGE_FILE)))
    }
}

/// Why the hook leaves the request to Claude Code's prompt. Safe to print
/// on stderr: none of these carries the request or the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Fallback {
    #[error("{0}")]
    Input(InputError),
    #[error("Ottid isn't running, or the bridge isn't on")]
    NotRunning,
    #[error("Ottid's bridge file can't be read")]
    BadBridgeFile,
    #[error("couldn't connect to Ottid")]
    Connect,
    #[error("Ottid didn't answer the handshake")]
    Handshake,
    #[error("the endpoint couldn't prove it is this user's Ottid")]
    NotOttid,
    #[error("the request is too large for the approval card")]
    TooLarge,
    #[error("Ottid closed the connection without an answer")]
    Closed,
    #[error("no answer in time")]
    Timeout,
    #[error("Ottid's answer failed its check")]
    BadAnswer,
    #[error("Claude Code went away")]
    Cancelled,
}

/// Hand one hook input to Ottid and wait for the user. `cancelled` resolves
/// when Claude Code goes away; the request is then withdrawn from the card.
pub async fn ask<C>(stdin: &[u8], options: &Options, cancelled: C) -> Result<Verdict, Fallback>
where
    C: Future<Output = ()>,
{
    let request = hook::parse(stdin).map_err(Fallback::Input)?;
    let body = serde_json::to_vec(&Ask {
        agent: options.agent,
        tool: request.tool,
        input: request.input,
        cwd: request.cwd,
    })
    .map_err(|_| Fallback::TooLarge)?;
    if MAC_LEN + body.len() > MAX_ASK {
        return Err(Fallback::TooLarge);
    }
    let bridge = BridgeFile::read(&options.bridge_file).map_err(|err| match err {
        BridgeFileError::Missing => Fallback::NotRunning,
        BridgeFileError::Invalid => Fallback::BadBridgeFile,
    })?;
    let stream = tokio::time::timeout(
        options.connect_timeout,
        endpoint::connect(&bridge.endpoint, &options.bridge_file),
    )
    .await
    .map_err(|_| Fallback::Connect)?
    .map_err(|_| Fallback::Connect)?;
    exchange(stream, &bridge.token, &body, options, cancelled).await
}

/// The exchange itself, over any stream: hello, challenge, request, answer.
pub async fn exchange<S, C>(
    stream: S,
    token: &Token,
    body: &[u8],
    options: &Options,
    cancelled: C,
) -> Result<Verdict, Fallback>
where
    S: AsyncRead + AsyncWrite,
    C: Future<Output = ()>,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let ours = Nonce::generate().map_err(|_| Fallback::Handshake)?;
    let limit = options.handshake_timeout;

    let hello = serde_json::to_vec(&Hello {
        v: VERSION,
        nonce: ours.to_hex(),
    })
    .map_err(|_| Fallback::Handshake)?;
    within(limit, wire::write_frame(&mut writer, &hello, MAX_HELLO)).await?;

    let challenge = within(limit, wire::read_frame(&mut reader, MAX_CHALLENGE)).await?;
    let challenge: Challenge =
        serde_json::from_slice(&challenge).map_err(|_| Fallback::Handshake)?;
    if challenge.v != VERSION {
        return Err(Fallback::Handshake);
    }
    let theirs = Nonce::from_hex(&challenge.nonce).ok_or(Fallback::Handshake)?;
    let session = Session::new(token, ours, theirs);
    if !session.check_proof(&challenge.proof) {
        return Err(Fallback::NotOttid);
    }

    let sealed = session.seal(Purpose::Ask, body);
    within(limit, wire::write_frame(&mut writer, &sealed, MAX_ASK)).await?;

    tokio::pin!(cancelled);
    let answer = tokio::select! {
        read = tokio::time::timeout(options.wait, wire::read_frame(&mut reader, MAX_ANSWER)) => {
            match read {
                Ok(Ok(answer)) => answer,
                Ok(Err(err)) => return Err(frame_fallback(err)),
                Err(_) => return Err(Fallback::Timeout),
            }
        }
        () = &mut cancelled => return Err(Fallback::Cancelled),
    };
    let body = session
        .open(Purpose::Answer, &answer)
        .ok_or(Fallback::BadAnswer)?;
    let answer: Answer = serde_json::from_slice(body).map_err(|_| Fallback::BadAnswer)?;
    Verdict::from_code(&answer.decision).ok_or(Fallback::BadAnswer)
}

/// A handshake step, within its time limit.
async fn within<T, F>(limit: Duration, step: F) -> Result<T, Fallback>
where
    F: Future<Output = Result<T, FrameError>>,
{
    match tokio::time::timeout(limit, step).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(frame_fallback(err)),
        Err(_) => Err(Fallback::Handshake),
    }
}

fn frame_fallback(err: FrameError) -> Fallback {
    match err {
        FrameError::Closed | FrameError::Io(_) => Fallback::Closed,
        FrameError::TooLarge { .. } | FrameError::Empty => Fallback::BadAnswer,
    }
}

/// Resolves when the process that started the hook (Claude Code) exits, so
/// the request is withdrawn from the card rather than left for nobody. If
/// that can't be watched, it never resolves, and the hook's own wait is the
/// limit.
pub async fn parent_gone() {
    parent::gone().await
}

#[cfg(windows)]
mod parent {
    use std::mem::size_of;

    use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess, WaitForSingleObject,
        INFINITE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    /// Owns a handle and closes it.
    struct Owned(HANDLE);

    // A process handle may be waited on from any thread.
    unsafe impl Send for Owned {}

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle is ours and closed once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    enum Parent {
        Alive(Owned),
        Gone,
        Unknown,
    }

    pub async fn gone() {
        match open_parent() {
            Parent::Gone => {}
            Parent::Unknown => std::future::pending().await,
            Parent::Alive(handle) => {
                let (tx, rx) = tokio::sync::oneshot::channel::<()>();
                // A plain thread, not the runtime's blocking pool: the
                // runtime waits for its pool when it shuts down, and this
                // wait may outlive the request.
                let spawned = std::thread::Builder::new()
                    .name("ottid-hook-parent".into())
                    .spawn(move || {
                        let handle = handle;
                        // SAFETY: a valid process handle with SYNCHRONIZE.
                        unsafe { WaitForSingleObject(handle.0, INFINITE) };
                        let _ = tx.send(());
                    });
                if spawned.is_err() {
                    return std::future::pending().await;
                }
                if rx.await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        }
    }

    fn parent_pid() -> Option<u32> {
        // SAFETY: a process snapshot, walked with a correctly sized entry and
        // closed by `Owned`.
        unsafe {
            let snapshot = Owned(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?);
            let me = GetCurrentProcessId();
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            Process32FirstW(snapshot.0, &mut entry).ok()?;
            loop {
                if entry.th32ProcessID == me {
                    return Some(entry.th32ParentProcessID);
                }
                Process32NextW(snapshot.0, &mut entry).ok()?;
            }
        }
    }

    fn created(process: HANDLE) -> Option<u64> {
        let (mut creation, mut exit, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: a valid process handle and four out-parameters.
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) }
            .ok()?;
        Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }

    fn open_parent() -> Parent {
        let Some(pid) = parent_pid() else {
            return Parent::Unknown;
        };
        // SAFETY: plain OpenProcess; the handle is owned below.
        let opened = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        };
        let handle = match opened {
            Ok(handle) => Owned(handle),
            // No such process: the parent has already exited.
            Err(err) if err.code() == ERROR_INVALID_PARAMETER.to_hresult() => return Parent::Gone,
            Err(_) => return Parent::Unknown,
        };
        // A process that started after this one can't be its parent: the
        // parent exited and its id was reused.
        // SAFETY: the pseudo-handle of this process.
        let mine = created(unsafe { GetCurrentProcess() });
        match (created(handle.0), mine) {
            (Some(parent), Some(mine)) if parent > mine => Parent::Gone,
            (Some(_), Some(_)) => Parent::Alive(handle),
            _ => Parent::Unknown,
        }
    }
}

#[cfg(unix)]
mod parent {
    use std::time::Duration;

    const LOOK: Duration = Duration::from_millis(250);

    pub async fn gone() {
        // Orphaned children are adopted by init or a subreaper, so the
        // parent id changes when the parent exits.
        let first = std::os::unix::process::parent_id();
        loop {
            tokio::time::sleep(LOOK).await;
            if std::os::unix::process::parent_id() != first {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_bridge::message::Hello;
    use serde_json::json;

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn options() -> Options {
        Options {
            agent: super::super::Agent::Claude,
            bridge_file: PathBuf::from("unused"),
            connect_timeout: Duration::from_millis(200),
            handshake_timeout: Duration::from_millis(300),
            wait: Duration::from_millis(500),
        }
    }

    fn stdin() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": { "command": "git push --force" },
            "cwd": "C:\\repo"
        }))
        .unwrap()
    }

    /// Plays the listener's side by hand: `answer` says what to send back
    /// once the request is in, given the session.
    async fn fake_listener<S>(
        stream: S,
        token: Token,
        proof_token: Token,
        answer: impl FnOnce(&Session<'_>) -> Option<Vec<u8>>,
    ) where
        S: AsyncRead + AsyncWrite,
    {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let hello = wire::read_frame(&mut reader, MAX_HELLO).await.unwrap();
        let hello: Hello = serde_json::from_slice(&hello).unwrap();
        let theirs = Nonce::from_hex(&hello.nonce).unwrap();
        let ours = Nonce::generate().unwrap();
        let proving = Session::new(&proof_token, theirs, ours);
        let challenge = serde_json::to_vec(&Challenge {
            v: VERSION,
            nonce: ours.to_hex(),
            proof: proving.proof(),
        })
        .unwrap();
        wire::write_frame(&mut writer, &challenge, MAX_CHALLENGE)
            .await
            .unwrap();
        let session = Session::new(&token, theirs, ours);
        let Ok(sealed) = wire::read_frame(&mut reader, MAX_ASK).await else {
            return;
        };
        assert!(session.open(Purpose::Ask, &sealed).is_some());
        if let Some(reply) = answer(&session) {
            let _ = wire::write_frame(&mut writer, &reply, MAX_ANSWER).await;
        }
        // Hold the connection until the client is done with it.
        let _ = wire::read_frame(&mut reader, 1).await;
    }

    fn sealed_answer(session: &Session<'_>, decision: &str) -> Vec<u8> {
        let body = serde_json::to_vec(&Answer {
            decision: decision.into(),
        })
        .unwrap();
        session.seal(Purpose::Answer, &body)
    }

    fn run(
        answer: impl FnOnce(&Session<'_>) -> Option<Vec<u8>> + Send + 'static,
        proof_token: Token,
    ) -> Result<Verdict, Fallback> {
        block_on(async move {
            let token = Token::from_bytes([5; 32]);
            let (client, server) = tokio::io::duplex(1 << 16);
            let listener = tokio::spawn(fake_listener(server, token.clone(), proof_token, answer));
            let body = serde_json::to_vec(&json!({ "tool": "Bash", "input": {} })).unwrap();
            let verdict = exchange(client, &token, &body, &options(), std::future::pending()).await;
            listener.abort();
            verdict
        })
    }

    #[test]
    fn a_sealed_allow_is_taken() {
        let verdict = run(
            |s| Some(sealed_answer(s, "allow")),
            Token::from_bytes([5; 32]),
        );
        assert_eq!(verdict, Ok(Verdict::Allow));
    }

    #[test]
    fn a_sealed_deny_and_ask_are_taken() {
        assert_eq!(
            run(
                |s| Some(sealed_answer(s, "deny")),
                Token::from_bytes([5; 32])
            ),
            Ok(Verdict::Deny)
        );
        assert_eq!(
            run(
                |s| Some(sealed_answer(s, "ask")),
                Token::from_bytes([5; 32])
            ),
            Ok(Verdict::Ask)
        );
    }

    #[test]
    fn an_endpoint_without_the_token_is_never_sent_the_request() {
        // A stranger on the endpoint can't make the proof: the client stops
        // before the request leaves it.
        let verdict = run(|_| None, Token::from_bytes([6; 32]));
        assert_eq!(verdict, Err(Fallback::NotOttid));
    }

    #[test]
    fn an_unsealed_or_forged_allow_falls_back() {
        let plain = run(
            |_| Some(br#"{"decision":"allow"}"#.to_vec()),
            Token::from_bytes([5; 32]),
        );
        assert_eq!(plain, Err(Fallback::BadAnswer));

        let forged = run(
            |_| {
                let other = Token::from_bytes([7; 32]);
                let session = Session::new(
                    &other,
                    Nonce::from_bytes([0; 32]),
                    Nonce::from_bytes([0; 32]),
                );
                Some(sealed_answer(&session, "allow"))
            },
            Token::from_bytes([5; 32]),
        );
        assert_eq!(forged, Err(Fallback::BadAnswer));
    }

    #[test]
    fn an_unknown_decision_falls_back() {
        let verdict = run(
            |s| Some(sealed_answer(s, "always")),
            Token::from_bytes([5; 32]),
        );
        assert_eq!(verdict, Err(Fallback::BadAnswer));
    }

    #[test]
    fn no_answer_in_time_falls_back() {
        let verdict = run(|_| None, Token::from_bytes([5; 32]));
        assert_eq!(verdict, Err(Fallback::Timeout));
    }

    #[test]
    fn a_listener_that_hangs_up_falls_back() {
        let verdict = block_on(async {
            let token = Token::from_bytes([5; 32]);
            let (client, server) = tokio::io::duplex(1024);
            drop(server);
            exchange(client, &token, b"{}", &options(), std::future::pending()).await
        });
        assert_eq!(verdict, Err(Fallback::Closed));
    }

    #[test]
    fn a_listener_that_says_nothing_falls_back() {
        let verdict = block_on(async {
            let token = Token::from_bytes([5; 32]);
            let (client, _server) = tokio::io::duplex(1024);
            exchange(client, &token, b"{}", &options(), std::future::pending()).await
        });
        assert_eq!(verdict, Err(Fallback::Handshake));
    }

    #[test]
    fn claude_code_going_away_stops_the_wait() {
        let verdict = block_on(async {
            let token = Token::from_bytes([5; 32]);
            let (client, server) = tokio::io::duplex(1 << 16);
            let listener =
                tokio::spawn(fake_listener(server, token.clone(), token.clone(), |_| {
                    None
                }));
            let gone = tokio::time::sleep(Duration::from_millis(50));
            let mut long = options();
            long.wait = Duration::from_secs(30);
            let verdict = exchange(client, &token, b"{}", &long, gone).await;
            listener.abort();
            verdict
        });
        assert_eq!(verdict, Err(Fallback::Cancelled));
    }

    #[test]
    fn bad_input_falls_back_before_ottid_is_looked_for() {
        let verdict = block_on(ask(b"{}", &options(), std::future::pending()));
        assert_eq!(verdict, Err(Fallback::Input(InputError::WrongEvent)));
    }

    #[test]
    fn no_bridge_file_means_ottid_isnt_running() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options();
        opts.bridge_file = dir.path().join(BRIDGE_FILE);
        let verdict = block_on(ask(&stdin(), &opts, std::future::pending()));
        assert_eq!(verdict, Err(Fallback::NotRunning));
    }

    #[test]
    fn a_request_over_the_frame_cap_falls_back_before_connecting() {
        let huge = serde_json::to_vec(&json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Write",
            "tool_input": { "file_path": "C:\\big.txt", "content": "א".repeat(MAX_ASK) }
        }))
        .unwrap();
        let verdict = block_on(ask(&huge, &options(), std::future::pending()));
        assert_eq!(verdict, Err(Fallback::TooLarge));
    }

    #[test]
    fn every_fallback_prints_nothing() {
        // Whatever went wrong, stdout stays empty and Claude Code asks.
        for fallback in [
            Fallback::NotRunning,
            Fallback::Connect,
            Fallback::NotOttid,
            Fallback::Timeout,
            Fallback::BadAnswer,
            Fallback::Cancelled,
        ] {
            let printed = Err::<Verdict, _>(fallback).ok().and_then(hook::output);
            assert_eq!(printed, None, "{fallback:?}");
            // The message is safe to put on stderr: no request, no token.
            assert!(!fallback.to_string().is_empty());
        }
    }
}
