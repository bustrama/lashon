//! The Claude Code hooks bridge
//! ([ADR-0050](../../../../docs/adr/0050-claude-code-hooks-bridge.md)).
//!
//! Claude Code asks before it runs a tool the user hasn't allowed. Once the
//! user installs the bridge from the Hub, Claude Code's `PermissionRequest`
//! hook runs `ottid-hook`, which hands the request to Ottid's approval card
//! and prints the user's answer back. Anything short of an answer prints
//! nothing, and Claude Code asks in its terminal prompt as it always did.
//!
//! This is Ottid's first inbound listener. What keeps it to the user:
//!
//! - **The endpoint.** A named pipe whose DACL admits only the user's SID and
//!   which refuses remote clients, or a socket in a directory only the user
//!   can open. Its name is random per process.
//! - **The token.** Minted per process, kept only in a file only the user can
//!   read, and never logged, put in an environment variable or a URL, or
//!   written into Claude Code's settings. It never crosses the wire either:
//!   each side proves it holds the token with an HMAC over fresh nonces from
//!   both ends, checked in constant time, so a stranger on the endpoint can
//!   neither ask nor answer.
//! - **Limits.** Every frame has a size cap, every read before the user's
//!   turn a short timeout, and the listener a cap on open connections.
//! - **Logs.** An id, the tool's name, the decision and the time it took.
//!   Never the tool's input or the folder it runs in.
//!
//! The modules:
//!
//! - [`wire`]: length-prefixed frames, capped per message.
//! - [`message`]: the four messages of an exchange.
//! - [`auth`]: the token, the nonces and the MACs.
//! - [`hook`]: the hook's stdin and stdout, as Claude Code documents them.
//! - [`client`]: what `ottid-hook` does, falling back to Claude Code's own
//!   prompt on any failure.
//! - [`server`]: one connection's exchange, and the listener.
//! - [`endpoint`]: the bridge file, the pipe or socket, and their ACLs.
//! - [`claude_settings`]: adding and removing the hook in Claude Code's user
//!   settings.

pub mod auth;
pub mod claude_settings;
pub mod codex_settings;
pub mod runner;
pub mod client;
pub mod endpoint;
pub mod hook;
pub mod message;
pub mod server;
pub mod wire;

use std::time::Duration;

pub use auth::Token;
pub use client::{Fallback, Options};
pub use hook::{HookInput, Verdict};
pub use server::{AgentAsk, AskFn, Bridge};

/// The protocol version both ends send. A mismatch falls back to Claude
/// Code's prompt.
pub const VERSION: u32 = 1;

/// The asking agent, as the approval card names it.
pub const AGENT: &str = "Claude Code";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

impl Agent {
    pub fn name(self) -> &'static str {
        match self { Self::Claude => "Claude Code", Self::Codex => "Codex" }
    }
    pub fn is_claude(&self) -> bool { *self == Self::Claude }
}

/// The only hook event the bridge answers. Wired to any other, `ottid-hook`
/// stays silent.
pub const HOOK_EVENT: &str = "PermissionRequest";

/// How long `ottid-hook` gives Ottid's endpoint to accept it.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// How long either end waits for the other's next handshake message. Both
/// send theirs at once, so this is generous.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the listener waits for the user before it answers "ask". It may
/// wait behind other requests on the card first, then up to
/// `approval::TIMEOUT` on screen.
pub const SERVER_WAIT: Duration = Duration::from_secs(95);

/// How long `ottid-hook` waits for the answer: a little longer than the
/// listener, so the listener's "ask" normally arrives first.
pub const CLIENT_WAIT: Duration = Duration::from_secs(100);

/// The `timeout` the installed hook gets in Claude Code's settings, in
/// seconds. Above [`CLIENT_WAIT`], so `ottid-hook` always gives up first and
/// exits cleanly rather than being cancelled.
pub const HOOK_TIMEOUT_SECS: u64 = 120;

/// Open connections the listener serves at once. Each is one Claude Code
/// permission prompt; one past the cap is closed at once, and its prompt
/// falls back to the terminal.
pub const MAX_CONNECTIONS: usize = 16;
