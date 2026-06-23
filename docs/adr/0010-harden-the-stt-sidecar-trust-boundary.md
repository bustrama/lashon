# 10. Harden the STT sidecar trust boundary

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** Lashon contributors
- **Context source:** a security review of the `m3-tongue-ui` branch; refines
  [ADR-0002](0002-grpc-loopback-tcp-transport.md)

## Context

A security review of the STT sidecar boundary found three weaknesses; a
fourth — a process leak — surfaced while verifying the fix. The
sidecar is a Python process the Rust core spawns; they speak gRPC over a
loopback TCP port (ADR-0002), and the sidecar handles transcripts and
microphone audio — the most privacy-sensitive data Lashon touches.

1. **The loopback gRPC port is unauthenticated.** `add_insecure_port`
   constrains *where* a caller sits (`127.0.0.1`) but not *who* it is. Any
   other process running as the same user can connect and call
   `TranscribeBytes` — driving Lashon's microphone-fed STT, or denying service
   to it. ADR-0002 explicitly deferred "a per-launch shared-secret handshake"
   to "its own ADR before any release that handles real transcripts." This is
   that ADR.
2. **The sidecar interpreter resolves through a bare `PATH`.** On the
   from-source path the core ran `Command::new("python")`. Windows
   `CreateProcess` searches the current working directory, so a `python.exe`
   dropped in the CWD would run with the app's privileges.
3. **The model is size-checked, not hash-checked, on boot.** Boot detection
   used a cheap exists-and-size check; the SHA-256 ran only on freshly
   downloaded files. A same-size `model.bin` swapped into `LASHON_MODELS_ROOT`
   was never re-verified — and a tampered CT2 model is arbitrary native
   behaviour inside `ctranslate2`.
4. **A failed sidecar spawn leaks its process.** `spawn()` launches the
   sidecar, then waits for the stdout handshake. On any error — a handshake
   timeout, most commonly — it returns before building the `Sidecar` whose
   `Drop` kills the child, and the `tokio` `Command` was not set to kill on
   drop, so the launched `python -m lashon_stt.server` is orphaned. A polling
   caller (the desktop app's first-run model wait) re-spawns on every failed
   poll, so orphans accumulate — each loading the 1.6 GB model — until memory
   is exhausted. Found while verifying this change.

## Decision

**A per-process auth token on the gRPC boundary.** On startup the sidecar
mints a token (`secrets.token_hex(32)`) and prints a two-line stdout
handshake — `LASHON_STT_TOKEN=<hex>` then `LASHON_STT_PORT=<port>`. The Rust
core parses both, attaches the token to every call as `x-lashon-auth`
metadata, and the sidecar rejects any call lacking it with `UNAUTHENTICATED`
(constant-time compared). The token never touches disk or a log line — it
lives only in the stdout pipe, which only the parent process can read.

**Absolute interpreter resolution.** The core resolves the sidecar's Python
interpreter to an absolute path by searching `PATH` directories only — never
the current directory, and never an empty `PATH` entry, which the OS treats as
the CWD. `LASHON_PYTHON` still overrides the choice.

**Boot-time model integrity.** Every present model file is SHA-256-verified
against the manifest on every boot, not only right after a download; a file
that fails is re-downloaded. This already runs on the background warm-up
thread, so the cost stays off the UI.

**No leaked sidecar on a failed spawn.** The sidecar `Command` is configured
`kill_on_drop`, so a `Child` dropped on any `spawn()` error path takes its
process down with it rather than leaving an orphan to accumulate.

The transport stays plaintext loopback (`add_insecure_port`): the token is the
*authentication*, the loopback bind remains the *locality* guarantee.
ADR-0002's "one transport, three operating systems" rationale is untouched.

## Alternatives considered

- **TLS / mTLS on the loopback socket.** Encrypts a hop that never leaves the
  host and adds certificate plumbing; the threat is an unauthenticated *local*
  caller, which a shared secret answers directly. Rejected as disproportionate.
- **A UDS / named pipe with filesystem permissions.** ADR-0002 already weighed
  and deferred this for the *transport*. It constrains who can *connect* but
  does not by itself authenticate a *caller*; orthogonal to the token and not
  revisited here.
- **A `.verified` marker file beside the model.** Cheap on boot, but the marker
  sits in the same user-writable directory as the model — an attacker who can
  swap `model.bin` can forge the marker. Not a trust anchor. The manifest
  (shipped with the app, signed once code-signing lands) is the only anchor, so
  the file itself must be hashed.
- **Documenting the bare-`PATH` launch as dev-only.** Leaves a real local
  code-execution path open in every developer's environment. Rejected — the
  absolute resolution is a few lines and removes the class of bug.

## Consequences

- The stdout handshake is now **two lines**. It remains a cross-language
  contract shared by `services/stt-sidecar/src/lashon_stt/server.py` and
  `packages/shared-rust/src/sidecar.rs`; the token line must precede the port
  line, and changing either is a breaking change to both, together.
- Boot adds one SHA-256 pass over the model (~1.6 GB) — a few seconds on the
  background warm-up thread, surfaced as a "verifying…" status. The server is
  reachable throughout; only `model_ready` is delayed.
- No new dependency in either language: the token uses Python's `secrets` and
  `tonic`'s metadata; the interpreter search uses Rust `std`; `kill_on_drop` is
  built into `tokio`'s `Command`. The CI license scan is unaffected.
- `kill_on_drop` stops the process leak, but a caller that keeps polling a
  failing `spawn()` still re-launches the sidecar each retry (now
  spawning-then-killing, not leaking). Caching or backing off failed spawns is
  a separate follow-up.
- The `healthcheck` integration test exercises the authenticated path on every
  CI run; the handshake reader, the token parser, and the `PATH` resolver are
  unit-tested in `lashon-core`.
- The token authenticates a *caller*, not a *channel*: it is per-process, does
  not survive a sidecar restart, and is not a substitute for the eventual
  UDS/named-pipe hardening should ADR-0002's transport ever be revisited.
