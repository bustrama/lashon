//! The hook client against a real in-process listener (ADR-0049): the
//! named pipe (Windows) or socket (Unix) with its ACLs, the bridge file, the
//! handshake, and what the hook would print for each outcome.
//!
//! Only the approval card is played by the test: the listener's ask
//! function answers as the user would, or never does.
//!
//! ```sh
//! cargo test -p ottid-core --test agent_bridge
//! ```

#![cfg(feature = "agent-hooks")]

use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ottid_core::agent_bridge::client::{self, Fallback, Options};
use ottid_core::agent_bridge::endpoint::{write_bridge_file, BRIDGE_FILE};
use ottid_core::agent_bridge::server::Limits;
use ottid_core::agent_bridge::{hook, AgentAsk, AskFn, Bridge, Token, Verdict, MAX_CONNECTIONS};
use serde_json::{json, Value};

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn stdin(command: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "session_id": "abc123",
        "transcript_path": "C:\\Users\\x\\.claude\\projects\\p\\t.jsonl",
        "cwd": "C:\\Users\\דנה\\פרויקט",
        "permission_mode": "default",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": { "command": command, "description": "מחיקת תיקייה" },
        "permission_suggestions": []
    }))
    .unwrap()
}

fn options(dir: &Path) -> Options {
    let mut options = Options::new(dir.join(BRIDGE_FILE));
    options.wait = Duration::from_secs(10);
    options
}

fn limits() -> Limits {
    Limits {
        handshake: Duration::from_secs(2),
        wait: Duration::from_secs(10),
    }
}

/// The card, answering every request with `verdict` and keeping what it was
/// shown.
fn card(verdict: Verdict, shown: Arc<std::sync::Mutex<Vec<AgentAsk>>>) -> AskFn {
    Arc::new(move |ask| {
        shown.lock().unwrap().push(ask);
        Box::pin(async move { verdict })
    })
}

/// What `ottid-hook` would print.
fn printed(result: Result<Verdict, Fallback>) -> Option<Value> {
    result
        .ok()
        .and_then(hook::output)
        .map(|text| serde_json::from_str(&text).unwrap())
}

#[test]
fn allow_on_the_card_prints_allow() {
    let dir = tempfile::tempdir().unwrap();
    let shown = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = block_on(async {
        let _bridge =
            Bridge::start_with(dir.path(), card(Verdict::Allow, shown.clone()), limits()).unwrap();
        client::ask(
            &stdin("rm -rf build"),
            &options(dir.path()),
            std::future::pending(),
        )
        .await
    });
    assert_eq!(result, Ok(Verdict::Allow));
    assert_eq!(
        printed(result),
        Some(json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": { "behavior": "allow" }
            }
        }))
    );
    // The card got the request whole, Hebrew and all, and nothing else of
    // the hook input (no transcript path, no session id).
    let shown = shown.lock().unwrap();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].tool, "Bash");
    assert_eq!(
        shown[0].input,
        json!({ "command": "rm -rf build", "description": "מחיקת תיקייה" })
    );
    assert_eq!(shown[0].cwd.as_deref(), Some("C:\\Users\\דנה\\פרויקט"));
}

#[test]
fn deny_on_the_card_prints_deny_with_a_message() {
    let dir = tempfile::tempdir().unwrap();
    let shown = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = block_on(async {
        let _bridge = Bridge::start_with(dir.path(), card(Verdict::Deny, shown), limits()).unwrap();
        client::ask(
            &stdin("git push --force"),
            &options(dir.path()),
            std::future::pending(),
        )
        .await
    });
    assert_eq!(result, Ok(Verdict::Deny));
    let out = printed(result).unwrap();
    assert_eq!(out["hookSpecificOutput"]["decision"]["behavior"], "deny");
    assert_eq!(
        out["hookSpecificOutput"]["decision"]["message"],
        hook::DENY_MESSAGE
    );
}

#[test]
fn a_card_that_times_out_leaves_it_to_claude_codes_prompt() {
    // The card's own timeout answers "ask", never deny.
    let dir = tempfile::tempdir().unwrap();
    let shown = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = block_on(async {
        let _bridge = Bridge::start_with(dir.path(), card(Verdict::Ask, shown), limits()).unwrap();
        client::ask(&stdin("ls"), &options(dir.path()), std::future::pending()).await
    });
    assert_eq!(result, Ok(Verdict::Ask));
    assert_eq!(printed(result), None);
}

#[test]
fn a_user_who_never_answers_gets_the_terminal_prompt() {
    // The listener's own limit: it answers "ask".
    let dir = tempfile::tempdir().unwrap();
    let never: AskFn = Arc::new(|_| Box::pin(std::future::pending()));
    let short = Limits {
        handshake: Duration::from_secs(2),
        wait: Duration::from_millis(300),
    };
    let result = block_on(async {
        let _bridge = Bridge::start_with(dir.path(), never, short).unwrap();
        client::ask(&stdin("ls"), &options(dir.path()), std::future::pending()).await
    });
    assert_eq!(result, Ok(Verdict::Ask));
    assert_eq!(printed(result), None);
}

/// Flags when the card's request is dropped, which withdraws it.
struct Withdrawn(Arc<AtomicBool>);

impl Drop for Withdrawn {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn waiting(flag: Arc<AtomicBool>) -> AskFn {
    Arc::new(move |_| {
        let guard = Withdrawn(flag.clone());
        Box::pin(async move {
            let _guard = guard;
            std::future::pending::<Verdict>().await
        })
    })
}

#[test]
fn the_hooks_own_timeout_falls_back_and_withdraws_the_card() {
    let dir = tempfile::tempdir().unwrap();
    let withdrawn = Arc::new(AtomicBool::new(false));
    let result = block_on(async {
        let _bridge = Bridge::start_with(dir.path(), waiting(withdrawn.clone()), limits()).unwrap();
        let mut opts = options(dir.path());
        opts.wait = Duration::from_millis(300);
        let result = client::ask(&stdin("ls"), &opts, std::future::pending()).await;
        // The listener notices the hook is gone.
        for _ in 0..50 {
            if withdrawn.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        result
    });
    assert_eq!(result, Err(Fallback::Timeout));
    assert_eq!(printed(result), None);
    assert!(
        withdrawn.load(Ordering::SeqCst),
        "the card's request was withdrawn"
    );
}

#[test]
fn claude_code_going_away_withdraws_the_card() {
    let dir = tempfile::tempdir().unwrap();
    let withdrawn = Arc::new(AtomicBool::new(false));
    let result = block_on(async {
        let _bridge = Bridge::start_with(dir.path(), waiting(withdrawn.clone()), limits()).unwrap();
        let gone = tokio::time::sleep(Duration::from_millis(200));
        let result = client::ask(&stdin("ls"), &options(dir.path()), gone).await;
        for _ in 0..50 {
            if withdrawn.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        result
    });
    assert_eq!(result, Err(Fallback::Cancelled));
    assert!(
        withdrawn.load(Ordering::SeqCst),
        "the card's request was withdrawn"
    );
}

#[test]
fn a_bad_token_never_reaches_the_card() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counting: AskFn = {
        let calls = calls.clone();
        Arc::new(move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Verdict::Allow })
        })
    };
    let result = block_on(async {
        let bridge = Bridge::start_with(dir.path(), counting, limits()).unwrap();
        // The client reads a bridge file with the right endpoint and the
        // wrong token.
        let stored: Value = serde_json::from_slice(&std::fs::read(bridge.file()).unwrap()).unwrap();
        let endpoint = stored["endpoint"].as_str().unwrap().to_string();
        let elsewhere = tempfile::tempdir().unwrap();
        write_bridge_file(elsewhere.path(), &endpoint, &Token::generate().unwrap()).unwrap();
        #[cfg(unix)]
        {
            // On Unix the client only connects to the socket next to its
            // bridge file, so point it at the real one.
            std::fs::copy(
                elsewhere.path().join(BRIDGE_FILE),
                dir.path().join("forged.json"),
            )
            .unwrap();
        }
        let file = if cfg!(unix) {
            dir.path().join("forged.json")
        } else {
            elsewhere.path().join(BRIDGE_FILE)
        };
        let mut opts = options(dir.path());
        opts.bridge_file = file;
        let result = client::ask(&stdin("rm -rf /"), &opts, std::future::pending()).await;
        drop(bridge);
        result
    });
    // The listener's proof fails under the wrong token: the client stops
    // before sending the request, and prints nothing.
    assert_eq!(result, Err(Fallback::NotOttid));
    assert_eq!(printed(result), None);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn ottid_not_running_falls_back_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let shown = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = block_on(async {
        let bridge = Bridge::start_with(dir.path(), card(Verdict::Allow, shown), limits()).unwrap();
        // Ottid quits: the bridge file goes with it.
        drop(bridge);
        client::ask(&stdin("ls"), &options(dir.path()), std::future::pending()).await
    });
    assert_eq!(result, Err(Fallback::NotRunning));
    assert_eq!(printed(result), None);
}

#[test]
fn a_listener_that_is_down_falls_back() {
    // A bridge file left by an Ottid that crashed: its endpoint is gone.
    let dir = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    let endpoint = r"\\.\pipe\ottid-agent-0123456789abcdef0123456789abcdef".to_string();
    #[cfg(unix)]
    let endpoint = dir.path().join("agent.sock").to_str().unwrap().to_string();
    write_bridge_file(dir.path(), &endpoint, &Token::generate().unwrap()).unwrap();
    let started = std::time::Instant::now();
    let result = block_on(client::ask(
        &stdin("ls"),
        &options(dir.path()),
        std::future::pending(),
    ));
    assert_eq!(result, Err(Fallback::Connect));
    assert_eq!(printed(result), None);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "no long wait for a dead listener"
    );
}

#[test]
fn ottid_quitting_mid_request_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let withdrawn = Arc::new(AtomicBool::new(false));
    let result = block_on(async {
        let bridge = Bridge::start_with(dir.path(), waiting(withdrawn.clone()), limits()).unwrap();
        let asking = tokio::spawn({
            let opts = options(dir.path());
            async move { client::ask(&stdin("ls"), &opts, std::future::pending()).await }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(bridge);
        asking.await.unwrap()
    });
    assert_eq!(result, Err(Fallback::Closed));
    assert_eq!(printed(result), None);
    assert!(
        !dir.path().join(BRIDGE_FILE).exists(),
        "the bridge file goes with Ottid"
    );
}

#[test]
fn several_sessions_are_answered_each_their_own() {
    let dir = tempfile::tempdir().unwrap();
    // Allows `ls`, denies anything else: each session must get the answer
    // to its own request.
    let judge: AskFn = Arc::new(|ask| {
        let verdict = if ask.input["command"] == "ls" {
            Verdict::Allow
        } else {
            Verdict::Deny
        };
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            verdict
        })
    });
    let results = block_on(async {
        let _bridge = Bridge::start_with(dir.path(), judge, limits()).unwrap();
        let mut sessions = Vec::new();
        for command in ["ls", "rm -rf x", "ls", "del y"] {
            let opts = options(dir.path());
            sessions.push(tokio::spawn(async move {
                client::ask(&stdin(command), &opts, std::future::pending()).await
            }));
        }
        let mut results = Vec::new();
        for session in sessions {
            results.push(session.await.unwrap());
        }
        results
    });
    assert_eq!(
        results,
        vec![
            Ok(Verdict::Allow),
            Ok(Verdict::Deny),
            Ok(Verdict::Allow),
            Ok(Verdict::Deny)
        ]
    );
}

#[test]
fn a_connection_past_the_cap_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let asked = Arc::new(AtomicUsize::new(0));
    let holding: AskFn = {
        let asked = asked.clone();
        Arc::new(move |_| {
            asked.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        })
    };
    let (held_results, result) = block_on(async {
        let _bridge = Bridge::start_with(dir.path(), holding, limits()).unwrap();
        let mut held = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            let opts = options(dir.path());
            held.push(tokio::spawn(async move {
                client::ask(&stdin("ls"), &opts, std::future::pending()).await
            }));
        }
        // Every slot taken: each held request is on the card.
        for _ in 0..250 {
            if asked.load(Ordering::SeqCst) == MAX_CONNECTIONS {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let held_results: Vec<bool> = held.iter().map(|session| session.is_finished()).collect();
        let result = client::ask(&stdin("ls"), &options(dir.path()), std::future::pending()).await;
        for session in held {
            session.abort();
        }
        (held_results, result)
    });
    assert_eq!(
        asked.load(Ordering::SeqCst),
        MAX_CONNECTIONS,
        "the held requests reached the card"
    );
    assert!(
        held_results.iter().all(|finished| !finished),
        "the held requests still wait"
    );
    // One past the cap is closed at once, and its prompt falls back.
    assert_eq!(result, Err(Fallback::Closed));
    assert_eq!(printed(result), None);
}

#[test]
fn a_hook_wired_to_another_event_never_answers() {
    let dir = tempfile::tempdir().unwrap();
    let shown = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = block_on(async {
        let _bridge =
            Bridge::start_with(dir.path(), card(Verdict::Allow, shown.clone()), limits()).unwrap();
        let pre_tool_use = serde_json::to_vec(&json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "ls" }
        }))
        .unwrap();
        client::ask(&pre_tool_use, &options(dir.path()), std::future::pending()).await
    });
    assert!(matches!(result, Err(Fallback::Input(_))));
    assert_eq!(printed(result), None);
    assert!(shown.lock().unwrap().is_empty());
}

/// The binary Claude Code runs: on input it won't answer, it prints nothing
/// and exits 0, so Claude Code shows its prompt without a hook error. (This
/// input is refused before Ottid is looked for, so the test never reaches a
/// real Ottid running on this machine.)
#[test]
fn the_hook_binary_stands_aside_silently() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let exe = env!("CARGO_BIN_EXE_ottid-hook");
    let inputs: [&[u8]; 3] = [
        b"not json",
        br#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}"#,
        b"",
    ];
    for input in inputs {
        let mut command = Command::new(exe);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW (.claude/rules/recipes.md).
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().expect("spawn ottid-hook");
        child.stdin.take().unwrap().write_all(input).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "exit {:?}", out.status.code());
        assert!(
            out.stdout.is_empty(),
            "stdout: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.starts_with("ottid-hook: "), "stderr: {stderr}");
    }
}
