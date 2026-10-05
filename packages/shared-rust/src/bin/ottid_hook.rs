//! `ottid-hook` — Claude Code's `PermissionRequest` hook for Ottid
//! (ADR-0050).
//!
//! Claude Code runs it when it is about to ask the user for permission,
//! with the request as JSON on stdin. It hands the request to Ottid's
//! approval card and prints the user's answer as the documented decision
//! object. When there is no answer for any reason (Ottid isn't running, the
//! bridge is off, the card timed out, anything went wrong) it prints
//! nothing and exits 0, and Claude Code asks in its own prompt as usual. It
//! never allows or denies on its own.
//!
//! The one line on stderr says why it stood aside. Claude Code keeps a
//! hook's stderr in its debug log only; the line never holds the request.

use std::io::{Read, Write};

use ottid_core::agent_bridge::{client, hook, wire::MAX_ASK};

/// The most stdin read: a request over the bridge's frame cap falls back
/// anyway, and the hook input carries a little more than the request.
const MAX_STDIN: usize = MAX_ASK + (64 << 10);

fn main() {
    let mut stdin = Vec::new();
    let read = std::io::stdin()
        .lock()
        .take(MAX_STDIN as u64 + 1)
        .read_to_end(&mut stdin);
    if read.is_err() {
        stand_aside("couldn't read the hook input");
        return;
    }
    if stdin.len() > MAX_STDIN {
        stand_aside("the request is too large for the approval card");
        return;
    }
    let Some(options) = client::Options::for_this_user() else {
        stand_aside("no per-user data folder to find Ottid in");
        return;
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            stand_aside("couldn't start");
            return;
        }
    };
    let verdict = runtime.block_on(client::ask(&stdin, &options, client::parent_gone()));
    match verdict.map(hook::output) {
        Ok(Some(decision)) => {
            let mut stdout = std::io::stdout().lock();
            // Nothing else is ever written to stdout.
            if stdout
                .write_all(decision.as_bytes())
                .and_then(|()| stdout.flush())
                .is_err()
            {
                stand_aside("couldn't write the decision");
            }
        }
        Ok(None) => stand_aside("no decision from the approval card"),
        Err(fallback) => stand_aside(&fallback.to_string()),
    }
}

fn stand_aside(why: &str) {
    eprintln!("ottid-hook: {why}; Claude Code asks in its own prompt");
}
