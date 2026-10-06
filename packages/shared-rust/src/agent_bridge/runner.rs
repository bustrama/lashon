use std::io::{Read, Write};

use super::{client, hook, wire::MAX_ASK};

/// The most stdin read: a request over the bridge's frame cap falls back
/// anyway, and the hook input carries a little more than the request.
const MAX_STDIN: usize = MAX_ASK + (64 << 10);

pub fn run(agent: super::Agent) {
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
    let Some(mut options) = client::Options::for_this_user() else {
        stand_aside("no per-user data folder to find Ottid in");
        return;
    };
    options.agent = agent;
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
    if let Some(mut activity) = super::activity::Activity::parse(&stdin) {
        if agent == super::Agent::Codex {
            activity.title = super::codex_settings::user_settings_path()
                .and_then(|path| path.parent().map(|dir| dir.join("session_index.jsonl")))
                .and_then(|path| super::activity::codex_title(&path, &activity.session));
        }
        options.wait = std::time::Duration::from_secs(1);
        options.connect_timeout = std::time::Duration::from_millis(200);
        options.handshake_timeout = std::time::Duration::from_millis(200);
        let _ = runtime.block_on(client::notify(activity, &options));
        // Informational only: never returns a permission or continuation decision.
        println!("{{}}");
        return;
    }
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
    eprintln!("ottid-hook: {why}; the agent uses its own approval flow");
}
