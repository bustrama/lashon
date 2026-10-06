//! The listener in Ottid: one exchange per connection, and the loop that
//! accepts them.
//!
//! A connection gets a short time for each handshake message and is closed
//! at the first thing wrong: a frame over its cap, a version it doesn't
//! know, a request whose MAC fails, a tool name the card can't show. Only
//! then is the request handed to the approval card. While the user decides,
//! the listener watches the connection: if the hook goes away (Claude Code
//! timed it out, or exited, or the user answered in the terminal), the
//! request is dropped, which takes it off the card.
//!
//! What is logged: a connection id, the tool's name, the decision and how
//! long it took, or why a connection was refused. Never the tool's input,
//! the folder, or the token.

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::{watch, Semaphore};

use super::auth::{Nonce, Purpose, Session, Token};
use super::endpoint::{self, Listener};
use super::hook::{valid_tool_name, Verdict};
use super::message::{Answer, Ask, Challenge, Hello};
use super::wire::{self, FrameError, MAX_ANSWER, MAX_ASK, MAX_CHALLENGE, MAX_HELLO};
use super::{HANDSHAKE_TIMEOUT, MAX_CONNECTIONS, SERVER_WAIT, VERSION};

const LOG: &str = "ottid::agent_bridge";

/// A request for the approval card.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentAsk {
    pub agent: super::Agent,
    pub tool: String,
    pub input: Value,
    pub cwd: Option<String>,
}

/// Asks the user. The future is dropped if the hook goes away first, which
/// must withdraw the request.
pub type AskFn =
    Arc<dyn Fn(AgentAsk) -> Pin<Box<dyn Future<Output = Verdict> + Send>> + Send + Sync>;

/// How long the listener waits at each step.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub handshake: Duration,
    /// How long the user has before the answer is "ask".
    pub wait: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            handshake: HANDSHAKE_TIMEOUT,
            wait: SERVER_WAIT,
        }
    }
}

/// Why a connection was closed without asking the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A handshake message didn't come in time, or the connection broke.
    Handshake,
    /// Another protocol version.
    Version,
    /// A message that isn't what it should be.
    Malformed,
    /// A frame over its cap.
    TooLarge,
    /// The request's MAC failed: the client doesn't hold the token.
    BadToken,
    /// A tool name the card can't show as is.
    BadTool,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Handshake => "handshake",
            Refusal::Version => "version",
            Refusal::Malformed => "malformed",
            Refusal::TooLarge => "too-large",
            Refusal::BadToken => "bad-token",
            Refusal::BadTool => "bad-tool",
        }
    }
}

/// How a connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Served {
    /// The answer was sent.
    Answered(Verdict),
    /// The hook went away first; the request was withdrawn.
    Gone,
    /// Closed without asking the user.
    Refused(Refusal),
}

/// Serve one connection.
pub async fn serve<S>(stream: S, token: &Token, ask: &AskFn, id: u64, limits: Limits) -> Served
where
    S: AsyncRead + AsyncWrite,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let started = Instant::now();

    let hello = match step(limits.handshake, wire::read_frame(&mut reader, MAX_HELLO)).await {
        Ok(hello) => hello,
        Err(refusal) => return refused(id, refusal),
    };
    let Ok(hello) = serde_json::from_slice::<Hello>(&hello) else {
        return refused(id, Refusal::Malformed);
    };
    if hello.v != VERSION {
        return refused(id, Refusal::Version);
    }
    let Some(theirs) = Nonce::from_hex(&hello.nonce) else {
        return refused(id, Refusal::Malformed);
    };
    let Ok(ours) = Nonce::generate() else {
        return refused(id, Refusal::Handshake);
    };
    let session = Session::new(token, theirs, ours);

    let challenge = Challenge {
        v: VERSION,
        nonce: ours.to_hex(),
        proof: session.proof(),
    };
    let Ok(challenge) = serde_json::to_vec(&challenge) else {
        return refused(id, Refusal::Handshake);
    };
    if let Err(refusal) = step(
        limits.handshake,
        wire::write_frame(&mut writer, &challenge, MAX_CHALLENGE),
    )
    .await
    {
        return refused(id, refusal);
    }

    let sealed = match step(limits.handshake, wire::read_frame(&mut reader, MAX_ASK)).await {
        Ok(sealed) => sealed,
        Err(refusal) => return refused(id, refusal),
    };
    let Some(body) = session.open(Purpose::Ask, &sealed) else {
        return refused(id, Refusal::BadToken);
    };
    let Ok(request) = serde_json::from_slice::<Ask>(body) else {
        return refused(id, Refusal::Malformed);
    };
    if !valid_tool_name(&request.tool) {
        return refused(id, Refusal::BadTool);
    }
    let tool = request.tool.clone();
    tracing::info!(target: LOG, id, tool = %tool, "agent permission request");

    let asking = ask(AgentAsk {
        agent: request.agent,
        tool: request.tool,
        input: request.input,
        cwd: request.cwd,
    });
    let verdict = tokio::select! {
        verdict = tokio::time::timeout(limits.wait, asking) => verdict.unwrap_or(Verdict::Ask),
        // Nothing more is due from the hook: anything it sends, or its end
        // closing, means it is gone. Dropping `asking` withdraws the card.
        _ = gone(&mut reader) => {
            tracing::info!(
                target: LOG,
                id,
                tool = %tool,
                ms = elapsed_ms(started),
                "agent permission request withdrawn: the hook went away"
            );
            return Served::Gone;
        }
    };

    let answer = Answer {
        decision: verdict.code().to_string(),
    };
    let Ok(answer) = serde_json::to_vec(&answer) else {
        return Served::Gone;
    };
    let sealed = session.seal(Purpose::Answer, &answer);
    let sent = step(
        limits.handshake,
        wire::write_frame(&mut writer, &sealed, MAX_ANSWER),
    )
    .await;
    tracing::info!(
        target: LOG,
        id,
        tool = %tool,
        decision = verdict.code(),
        delivered = sent.is_ok(),
        ms = elapsed_ms(started),
        "agent permission request answered"
    );
    match sent {
        Ok(()) => Served::Answered(verdict),
        Err(_) => Served::Gone,
    }
}

/// Resolves when the other end closes, breaks, or sends anything.
async fn gone<R: AsyncRead + Unpin>(reader: &mut R) {
    let mut byte = [0u8; 1];
    let _ = reader.read(&mut byte).await;
}

async fn step<T, F>(limit: Duration, future: F) -> Result<T, Refusal>
where
    F: Future<Output = Result<T, FrameError>>,
{
    match tokio::time::timeout(limit, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(FrameError::TooLarge { .. })) => Err(Refusal::TooLarge),
        Ok(Err(FrameError::Empty)) => Err(Refusal::Malformed),
        Ok(Err(FrameError::Closed | FrameError::Io(_))) | Err(_) => Err(Refusal::Handshake),
    }
}

fn refused(id: u64, refusal: Refusal) -> Served {
    match refusal {
        // Worth seeing: someone reached the endpoint without the token.
        Refusal::BadToken => {
            tracing::warn!(target: LOG, id, reason = refusal.code(), "agent connection refused")
        }
        _ => tracing::debug!(target: LOG, id, reason = refusal.code(), "agent connection refused"),
    }
    Served::Refused(refusal)
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The running listener and its bridge file. Dropping it stops the
/// listener, ends every open connection (each request falls back to Claude
/// Code's prompt) and deletes the bridge file.
pub struct Bridge {
    file: PathBuf,
    endpoint: String,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl Bridge {
    /// Mint a token, open the endpoint and write the bridge file in `dir`.
    /// Call from within a Tokio runtime.
    pub fn start(dir: &Path, ask: AskFn) -> io::Result<Self> {
        Self::start_with(dir, ask, Limits::default())
    }

    pub fn start_with(dir: &Path, ask: AskFn, limits: Limits) -> io::Result<Self> {
        let token = Token::generate()?;
        endpoint::prepare_dir(dir)?;
        let listener = Listener::bind(dir)?;
        let endpoint = listener.endpoint().to_string();
        let file = endpoint::write_bridge_file(dir, &endpoint, &token)?;
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(accept_loop(listener, token, ask, limits, stopped));
        tracing::info!(target: LOG, "agent bridge listening");
        Ok(Self {
            file,
            endpoint,
            stop,
            task,
        })
    }

    /// Where the hook finds the endpoint and the token.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.task.abort();
        endpoint::remove_bridge_file(&self.file, &self.endpoint);
        tracing::info!(target: LOG, "agent bridge stopped");
    }
}

async fn accept_loop(
    mut listener: Listener,
    token: Token,
    ask: AskFn,
    limits: Limits,
    stopped: watch::Receiver<bool>,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut next_id: u64 = 0;
    loop {
        let stream = match listener.accept().await {
            Ok(stream) => stream,
            Err(err) => {
                tracing::warn!(target: LOG, "agent bridge accept failed: {err}");
                if listener.is_broken() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        next_id += 1;
        let id = next_id;
        let Ok(slot) = slots.clone().try_acquire_owned() else {
            // Full: this prompt falls back to the terminal.
            tracing::warn!(target: LOG, id, "agent bridge at its connection cap; closing");
            drop(stream);
            continue;
        };
        let (token, ask, mut stopped) = (token.clone(), ask.clone(), stopped.clone());
        tokio::spawn(async move {
            tokio::select! {
                _ = serve(stream, &token, &ask, id, limits) => {}
                _ = stopped.wait_for(|stop| *stop) => {}
            }
            drop(slot);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_bridge::client::{self, Fallback, Options};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn limits() -> Limits {
        Limits {
            handshake: Duration::from_millis(300),
            wait: Duration::from_millis(400),
        }
    }

    fn options() -> Options {
        Options {
            agent: super::super::Agent::Claude,
            bridge_file: PathBuf::from("unused"),
            connect_timeout: Duration::from_millis(200),
            handshake_timeout: Duration::from_millis(300),
            wait: Duration::from_secs(5),
        }
    }

    fn answering(verdict: Verdict, calls: Arc<AtomicUsize>) -> AskFn {
        Arc::new(move |_ask| {
            calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { verdict })
        })
    }

    fn body() -> Vec<u8> {
        serde_json::to_vec(&Ask {
            agent: super::super::Agent::Claude,
            tool: "Bash".into(),
            input: serde_json::json!({ "command": "echo שלום" }),
            cwd: None,
        })
        .unwrap()
    }

    /// The real client against `serve`, over an in-memory stream.
    fn exchange(
        client_token: Token,
        server_token: Token,
        ask: AskFn,
        body: Vec<u8>,
    ) -> (Result<Verdict, Fallback>, Served) {
        block_on(async move {
            let (client_end, server_end) = tokio::io::duplex(1 << 16);
            let server =
                tokio::spawn(
                    async move { serve(server_end, &server_token, &ask, 1, limits()).await },
                );
            let verdict = client::exchange(
                client_end,
                &client_token,
                &body,
                &options(),
                std::future::pending(),
            )
            .await;
            (verdict, server.await.unwrap())
        })
    }

    #[test]
    fn the_users_answer_reaches_the_client() {
        for verdict in [Verdict::Allow, Verdict::Deny, Verdict::Ask] {
            let calls = Arc::new(AtomicUsize::new(0));
            let token = Token::from_bytes([1; 32]);
            let (got, served) = exchange(
                token.clone(),
                token,
                answering(verdict, calls.clone()),
                body(),
            );
            assert_eq!(got, Ok(verdict));
            assert_eq!(served, Served::Answered(verdict));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn codex_identity_and_complete_patch_reach_the_card_authenticated() {
        let input = serde_json::json!({"command":"*** Begin Patch\n*** Add File: שלום.txt\n+hello שלום\n*** End Patch", "description":"Write a file"});
        let expected = input.clone();
        let ask: AskFn = Arc::new(move |request| {
            assert_eq!(request.agent, super::super::Agent::Codex);
            assert_eq!(request.tool, "apply_patch");
            assert_eq!(request.input, expected);
            Box::pin(async { Verdict::Deny })
        });
        let body = serde_json::to_vec(&Ask {
            agent: super::super::Agent::Codex,
            tool: "apply_patch".into(), input, cwd: Some("C:\\פרויקט".into()),
        }).unwrap();
        let token = Token::from_bytes([1; 32]);
        let (got, served) = exchange(token.clone(), token, ask, body);
        assert_eq!(got, Ok(Verdict::Deny));
        assert_eq!(served, Served::Answered(Verdict::Deny));
    }

    #[test]
    fn a_client_without_the_token_never_reaches_the_card() {
        // The client checks the proof first and stops. Its request never
        // leaves it, so the listener only sees the connection close.
        let calls = Arc::new(AtomicUsize::new(0));
        let (got, served) = exchange(
            Token::from_bytes([2; 32]),
            Token::from_bytes([1; 32]),
            answering(Verdict::Allow, calls.clone()),
            body(),
        );
        assert_eq!(got, Err(Fallback::NotOttid));
        assert_eq!(served, Served::Refused(Refusal::Handshake));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_request_sealed_with_another_token_is_refused_unasked() {
        // A client that skips the proof check and seals with a guessed token.
        let calls = Arc::new(AtomicUsize::new(0));
        let ask = answering(Verdict::Allow, calls.clone());
        let served = block_on(async move {
            let (client_end, server_end) = tokio::io::duplex(1 << 16);
            let server_token = Token::from_bytes([1; 32]);
            let server =
                tokio::spawn(
                    async move { serve(server_end, &server_token, &ask, 7, limits()).await },
                );
            let (mut reader, mut writer) = tokio::io::split(client_end);
            let ours = Nonce::generate().unwrap();
            let hello = serde_json::to_vec(&Hello {
                v: VERSION,
                nonce: ours.to_hex(),
            })
            .unwrap();
            wire::write_frame(&mut writer, &hello, MAX_HELLO)
                .await
                .unwrap();
            let challenge = wire::read_frame(&mut reader, MAX_CHALLENGE).await.unwrap();
            let challenge: Challenge = serde_json::from_slice(&challenge).unwrap();
            let theirs = Nonce::from_hex(&challenge.nonce).unwrap();
            let guessed = Token::from_bytes([3; 32]);
            let sealed = Session::new(&guessed, ours, theirs).seal(Purpose::Ask, &body());
            wire::write_frame(&mut writer, &sealed, MAX_ASK)
                .await
                .unwrap();
            server.await.unwrap()
        });
        assert_eq!(served, Served::Refused(Refusal::BadToken));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    fn raw_hello(hello: &[u8]) -> Served {
        let hello = hello.to_vec();
        block_on(async move {
            let (client_end, server_end) = tokio::io::duplex(1 << 16);
            let ask = answering(Verdict::Allow, Arc::new(AtomicUsize::new(0)));
            let token = Token::from_bytes([1; 32]);
            let server =
                tokio::spawn(async move { serve(server_end, &token, &ask, 1, limits()).await });
            let (_reader, mut writer) = tokio::io::split(client_end);
            let _ = tokio::io::AsyncWriteExt::write_all(&mut writer, &hello).await;
            let served = server.await.unwrap();
            drop(writer);
            served
        })
    }

    #[test]
    fn a_hello_over_its_cap_is_refused_before_it_is_read() {
        let mut frame = ((MAX_HELLO + 1) as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(&[b'x'; 8]);
        assert_eq!(raw_hello(&frame), Served::Refused(Refusal::TooLarge));
    }

    #[test]
    fn a_silent_client_is_dropped_after_the_handshake_limit() {
        assert_eq!(raw_hello(&[]), Served::Refused(Refusal::Handshake));
    }

    #[test]
    fn another_version_or_a_bad_hello_is_refused() {
        let other = wire::encode(br#"{"v":2,"nonce":"00"}"#, MAX_HELLO).unwrap();
        assert_eq!(raw_hello(&other), Served::Refused(Refusal::Version));
        let junk = wire::encode(b"hello", MAX_HELLO).unwrap();
        assert_eq!(raw_hello(&junk), Served::Refused(Refusal::Malformed));
        let short = wire::encode(br#"{"v":1,"nonce":"abcd"}"#, MAX_HELLO).unwrap();
        assert_eq!(raw_hello(&short), Served::Refused(Refusal::Malformed));
    }

    #[test]
    fn a_tool_name_the_card_cant_show_is_refused() {
        let calls = Arc::new(AtomicUsize::new(0));
        let token = Token::from_bytes([1; 32]);
        let body = serde_json::to_vec(&Ask {
            agent: super::super::Agent::Claude,
            tool: "Bash\u{202E}".into(),
            input: serde_json::json!({}),
            cwd: None,
        })
        .unwrap();
        let (got, served) = exchange(
            token.clone(),
            token,
            answering(Verdict::Allow, calls.clone()),
            body,
        );
        assert_eq!(got, Err(Fallback::Closed));
        assert_eq!(served, Served::Refused(Refusal::BadTool));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_user_who_doesnt_answer_in_time_gets_ask() {
        let token = Token::from_bytes([1; 32]);
        let never: AskFn = Arc::new(|_| Box::pin(std::future::pending()));
        let (got, served) = exchange(token.clone(), token, never, body());
        assert_eq!(got, Ok(Verdict::Ask));
        assert_eq!(served, Served::Answered(Verdict::Ask));
    }

    /// Flags when the request is dropped, which is what withdraws the card.
    struct Withdrawn(Arc<AtomicBool>);

    impl Drop for Withdrawn {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn the_hook_going_away_withdraws_the_request() {
        let withdrawn = Arc::new(AtomicBool::new(false));
        let flag = withdrawn.clone();
        let waiting: AskFn = Arc::new(move |_| {
            let guard = Withdrawn(flag.clone());
            Box::pin(async move {
                let _guard = guard;
                std::future::pending::<Verdict>().await
            })
        });
        let served = block_on(async move {
            let (client_end, server_end) = tokio::io::duplex(1 << 16);
            let token = Token::from_bytes([1; 32]);
            let server_token = token.clone();
            let mut long = limits();
            long.wait = Duration::from_secs(30);
            let server =
                tokio::spawn(
                    async move { serve(server_end, &server_token, &waiting, 1, long).await },
                );
            let gone = tokio::time::sleep(Duration::from_millis(100));
            let verdict = client::exchange(client_end, &token, &body(), &options(), gone).await;
            assert_eq!(verdict, Err(Fallback::Cancelled));
            // The client's end is dropped with `exchange`.
            server.await.unwrap()
        });
        assert_eq!(served, Served::Gone);
        assert!(
            withdrawn.load(Ordering::SeqCst),
            "the card's request was dropped"
        );
    }
}
