# 50. The Claude Code hooks bridge

## Status

Accepted — 2026-10-05. Phase C. Builds on the approval card of
[ADR-0048](0048-the-approval-card.md) and the editions of
[ADR-0034](0034-command-mode-editioning.md). This is Ottid's first inbound
listener.

## Context

Claude Code asks in its terminal before it runs a tool the user hasn't
allowed. A user working in another window doesn't see the question, and the
agent sits idle until they look.

Ottid already has a card for exactly this question
([ADR-0048](0048-the-approval-card.md)). It shows every character of the
request, arms Allow only after the whole text has been on screen, and takes a
held hotkey. It queues requests, and the creature turns `--aqua` when one is
waiting.

To use that card for Claude Code, something Ottid didn't start has to connect
to Ottid. Until now, Ottid only connected out: the STT sidecar listens, and
Ottid spawns it ([ADR-0010](0010-harden-the-stt-sidecar-trust-boundary.md)).

The owner set two hard requirements:

- A per-process token, kept in a file only the user can read, and tool
  arguments are never logged.
- On a timeout or a connection failure, Claude Code falls back to its normal
  terminal prompt.

## Decision

### The hook: `PermissionRequest`

Claude Code runs hooks as commands at documented events
(<https://code.claude.com/docs/en/hooks>). `PermissionRequest` fires only when
Claude Code is about to show a permission prompt, and gets the tool's name,
its input and the working folder on stdin.

- **The answer.** A hook answers by printing
  `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"|"deny"}}}`.
  A deny carries a message for the model.
- **No answer.** Exit 0 with nothing on stdout is no decision, and Claude Code
  shows its own prompt. A hook that times out gives no decision either.
- **The entry Ottid installs.** One matcher group: `matcher: "*"`, the exec
  form (the absolute path to `ottid-hook`, `args: []`, no shell),
  `timeout: 120` and a `statusMessage`.

### `ottid-hook` stands aside on any failure

A small binary in `ottid-core` (`src/bin/ottid_hook.rs`).

1. It reads stdin, capped. It checks that the event is `PermissionRequest`
   and that the tool name is plain ASCII. It stands aside for the tools whose
   prompt collects an answer rather than a yes or no (`AskUserQuestion` and
   `ExitPlanMode`): their answers travel back in `updatedInput`, which the
   card can't fill, so an Allow would skip the user's choice.
2. It reads the bridge file and connects.
3. It runs the handshake, asks, and waits.

It prints the allow or deny JSON only when the user answered on the card.
Anything else prints nothing: Ottid not running, a stale or unreadable bridge
file, a refused connection, a wrong token, a malformed or oversized message,
the card lapsing or a timeout. In each case it writes a one-line reason (never
the request) to stderr and exits 0. It never exits 2, never denies because
something broke, and never allows on its own.

**Timeouts.** Connect 2 s, each handshake read 2 s, the answer 100 s. The
listener answers "ask" at 95 s, so it normally speaks first. Claude Code's
timeout for the hook is 120 s, so `ottid-hook` always gives up and exits
cleanly before Claude Code kills it.

**Claude Code going away.** `ottid-hook` watches its parent. If Claude Code
exits, so does the hook, and the closed connection withdraws the card.

- **Windows.** The parent's PID comes from a Toolhelp snapshot. The hook
  opens it with `SYNCHRONIZE` and waits on it. It checks the creation time,
  so a reused PID isn't mistaken for the parent.
- **Unix.** It polls the parent PID.

### The listener runs only while the hook is installed

Ottid checks Claude Code's user settings at start-up and starts the listener
only if its hook is there. The Hub's install starts it, and uninstall stops
it. Exit and restart stop it too. A user who never connects Claude Code has no
listener.

**The endpoint.**

- **Windows.** A named pipe, `\\.\pipe\ottid-agent-<128 random bits in
  hex>`.
  - Its DACL is protected, with one entry: `D:P(A;;GA;;;<the user's SID>)`.
  - It is created with `FILE_FLAG_FIRST_PIPE_INSTANCE` and
    `PIPE_REJECT_REMOTE_CLIENTS`, so Ottid never serves on a pipe somebody
    else created first, and remote clients are refused.
  - Every accepted connection gets a fresh instance.
- **Unix.** `agent.sock`, mode 0600, in a 0700 directory the effective user
  owns. The directory's owner and mode are checked.

**The bridge file.** `bridge.json` holds `{v, endpoint, token}`.

- **Who can read it.** On Windows it is created with the same one-entry DACL.
  On Unix it is 0600 in the 0700 directory.
- **Writing it.** It goes to a temporary name and is renamed, so a reader
  never sees half of it.
- **Removing it.** Stopping removes it, but only if it still names this
  process's endpoint.
- **Where it is.**
  - Windows: `%LOCALAPPDATA%\app.ottid.desktop\agent-bridge`.
  - macOS: `~/Library/Application Support/app.ottid.desktop/agent-bridge`.
  - Linux: `$XDG_RUNTIME_DIR/app.ottid.desktop/agent-bridge`, else the data
    folder.

**Client checks.** The client connects only to an endpoint of that form: a
local pipe with Ottid's prefix, or the socket next to the bridge file.

**Limits.**

- At most 16 open connections. One past the cap is closed at once and falls
  back.
- 2 s for each handshake read.
- Frames are capped: hello 256 B, challenge 512 B, ask 1 MiB, answer 1 KiB.
  The length is checked before the body is read, and empty frames are
  refused.

### The protocol

Frames are a 4-byte big-endian length and a JSON body. Unknown fields are
refused. Both sides send the protocol version, and a mismatch falls back.

1. **Hook → Ottid:** `Hello {v, nonce}`, 32 random bytes.
2. **Ottid → hook:** `Challenge {v, nonce, proof}`. Ottid sends its own
   nonce, and `proof` is an HMAC over both nonces.
3. **Hook → Ottid:** the `Ask` (`{tool, input, cwd}`), sealed: an HMAC over
   both nonces and the body, then the body. The hook checks Ottid's proof, in
   constant time, before it sends anything about the request.
4. **Ottid → hook:** Ottid checks the MAC and puts the card up. It replies
   with `Answer {decision}`, sealed the same way.

**The MACs.** HMAC-SHA256 under the token. Each one starts with its purpose's
label, `ottid-agent-bridge/1/challenge`, `.../ask` or `.../answer`, and every
field is length-prefixed. All three are checked in constant time.

**What this buys.** The token never crosses the wire, and each side proves it
holds it.

- Something squatting on the endpoint without the token learns nothing about
  the request and can't forge an answer.
- A client without the token can't put a card up.
- Fresh nonces from both ends rule out replaying an old exchange.

`hmac` 0.12.1 and `getrandom` 0.3.4 were already in the lockfile. No new
crate enters the tree.

### The token

32 random bytes, minted per process. It exists in Ottid's memory and in the
bridge file, and nowhere else. It is never in a log (its `Debug` is
redacted), an environment variable, a URL, a command line, or Claude Code's
settings. It is deleted with the bridge file when Ottid stops.

After a crash, the leftover file names a pipe that no longer exists. The hook
fails to connect and falls back, and the next start overwrites the file.

### Logging

The listener logs an id, the tool's name, the decision and the time it took.
It never logs the input, the command line or the folder. A wrong token is
logged at `warn`, without detail. `ottid-hook` writes only its reason to
stderr.

### The card

A new request source, `agent`. `Request::for_agent` builds the card:

- **Shell tools** (`Bash`, `PowerShell`) show their command line and folder
  the way Ottid's own `run_command` card does.
- **Every other tool** shows its whole input as pretty-printed JSON, so the
  user sees every argument: a file path, an edit's old and new text, a URL.
- **The eyebrow** names the agent: "Claude Code needs your approval".
- **Unchanged from ADR-0048:** the rendering that shows every character, the
  arm delay, the held hotkey and the queue. Several Claude Code sessions wait
  their turn like any other requests, and the creature shows
  `agent-needs-you`.

**Lapse.** When an Ottid request's 30 s run out, the card denies it. An
agent's request goes back to the agent instead: the card says "Back to the
terminal in Ns", the listener answers "ask", `ottid-hook` prints nothing, and
Claude Code shows its prompt.

**Withdrawal.** A connection that closes withdraws its card. That happens when
Claude Code cancels the prompt or exits, or when the hook times out.

**One request.** An answer covers that one tool call. Ottid doesn't return
`updatedPermissions` ("always allow") or `updatedInput`.

### Installing into Claude Code: opt-in and reversible

The Hub's **Coding agents** section connects and disconnects Claude Code.
Nothing is ever edited silently.

**The preview.** Each action first shows:

- the settings file;
- the exact JSON added under `hooks.PermissionRequest`;
- how many earlier Ottid entries it replaces or removes;
- that the file is backed up first;
- that nothing else in the file changes.

**Applying.** The change is made only when the user confirms, and only to the
file as it was previewed: a SHA-256 fingerprint is checked. If the file
changed in between, the Hub shows the change again against the file as it is
now. Only the Hub window can apply a change.

**The edit.**

- Every other key keeps its bytes and its order. The members are kept as raw
  JSON values; they are never re-serialized.
- A file that isn't valid JSON, isn't an object, has a duplicate key, or has
  a `hooks` or `hooks.PermissionRequest` of the wrong type is left alone, and
  the Hub says why.
- Before any write, the file is copied to `settings.json.ottid-backup-<unix
  secs>` beside it. The new file goes to a temporary name and is renamed.

**Whose entry it is.** A handler is Ottid's if its command's file stem is
`ottid-hook`.

- Install replaces Ottid's earlier entries, for example after Ottid moved.
- Uninstall removes all of them and nothing else. A group left empty is
  dropped, and so are `PermissionRequest` and `hooks` if they end up empty.
- A group that also runs the user's own handlers keeps them.

**Scope: user settings.** That is `~/.claude/settings.json`, or the folder
`CLAUDE_CONFIG_DIR` points to.

- **Why user scope.** The listener and the hook binary belong to the user,
  not to a project.
- **Not project settings** (`.claude/settings.json`). That file is often
  committed, and it would leak one user's binary path to everyone else on the
  repo, where it would fail.
- **Not local settings** (`.claude/settings.local.json`). It is per project,
  so the user would have to connect each project one by one.
- **Other scopes still run.** Hooks merge across scopes, so a project's own
  hooks keep running.
- **Blocking settings.** `disableAllHooks` in the user file is shown in the
  Hub. Managed settings can block user hooks with `allowManagedHooksOnly`; the
  hook then never runs and Claude Code asks as usual.

### The edition

The bridge is command-mode only. The owner chose this from the options under
[ADR-0034](0034-command-mode-editioning.md).

- **The feature.** `agent-hooks` exists in both `ottid-core` and the shell,
  and the `command-mode` feature turns it on.
- **The free edition** compiles none of it: no listener, no `ottid-hook`, no
  Hub section.
- **Why.** The approval broker exists only with command mode
  ([ADR-0048](0048-the-approval-card.md)). A dictation-only build shouldn't
  open an inbound listener.

### Packaging

- **Release.** `release.yml` builds `ottid-hook` (release, `--locked`) for
  every matrix entry that isn't free. It stages the binary in
  `apps/desktop/src-tauri/binaries/ottid-hook/`, which `tauri.conf.json`
  bundles as a resource. The free edition's config bundles nothing from it.
- **Development.** `cargo build -p ottid-core --bin ottid-hook` puts the
  binary next to `ottid.exe` in Cargo's target folder. The shell looks there
  when there is no bundled copy.

### Credit

The Windows named-pipe hardening is derived from Coucou (MIT © 2026 Louis
Raillé), through the hardened fork by YojoTan: the explicit DACL, the first
instance flag, refusing remote clients, and a fresh instance per connection.
`THIRD-PARTY-NOTICES` carries the MIT notice, and `endpoint.rs` has a header
saying so. Only code was taken, read through the GitHub API. No name,
character, icon, sound or media.

## Alternatives considered

- **`PreToolUse`.** It fires on every tool call, including the many Claude
  Code runs without asking. Ottid would either show a card for every file read
  or re-implement Claude Code's permission rules to guess which calls need
  one. `PermissionRequest` fires exactly when Claude Code would ask.
- **Exit code 2 to deny.** `PermissionRequest` doesn't honor it, and a crash
  could read as a deny.
- **HTTP hooks, or a loopback TCP port.** Any local process can reach a
  loopback port, whoever runs it, and so can a web page through DNS
  rebinding. TCP has no per-user access check. A pipe or a socket gets the
  operating system's.
- **The token on the wire as a bearer secret.** A squatter, or a fake
  endpoint, would collect it from the first hook that connected. The HMAC
  handshake proves both sides without revealing it.
- **The token in an environment variable or in Claude Code's settings.**
  Every process Claude Code starts inherits its environment, including the
  tools it runs. Settings files are read by the agent and are often synced or
  committed.
- **A listener that always runs.** It adds an inbound surface for every user,
  including those who never use Claude Code.
- **Installing the hook at first run.** Ottid never edits another program's
  configuration without the user seeing and confirming the change.
- **Deny when the card lapses.** The user may simply be at the terminal.
  Falling back keeps Claude Code usable when Ottid, or the user, is away.

## Consequences

- **Claude Code's prompts can be answered on the card,** from any app, with
  the held hotkey.
- **When Ottid is closed or crashed, or the user is away,** Claude Code
  behaves as before. With no bridge file, or one that names a dead pipe, the
  hook gives up at once. A busy or wedged listener costs at most the 2 s
  connect and handshake timeouts.
- **Same-user code is out of scope.** A process running as the user can read
  the bridge file. It could equally edit Claude Code's settings or type into
  the terminal. The bridge keeps out other users, remote clients, and anything
  without the token.
- **Uninstalling Ottid leaves the hook entry behind.** Claude Code then
  reports that the hook failed to run, and asks as usual. Until the
  installer removes the entry, users disconnect in the Hub first.
- **A moved install** shows as "connected to another copy of Ottid". "Point
  it at this copy" updates the path.
- **The approval chords are taken** while a Claude Code request is on the
  card, as for any request ([ADR-0048](0048-the-approval-card.md)).
- **Not supported:**
  - Claude Code inside WSL. It would need a Linux `ottid-hook` and a way to
    the Windows pipe.
  - Claude Code on a remote machine over SSH.
- **Follow-ups:**
  - An NSIS uninstall step that removes Ottid's entry from Claude Code's
    settings.
  - A command-mode entry in the release matrix. Today it ships only the free
    edition, so the staging step doesn't run yet.
  - Other coding agents, once they have a comparable permission hook.
