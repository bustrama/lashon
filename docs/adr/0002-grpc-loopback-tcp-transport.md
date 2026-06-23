# 2. STT/TTS sidecar transport: loopback TCP, not UDS / named pipe

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Lashon contributors
- **Relation:** refines a sidecar-transport detail
- **Update (2026-05-18):** the per-launch shared-secret handshake anticipated
  in *Consequences* now exists — see
  [ADR-0010](0010-harden-the-stt-sidecar-trust-boundary.md).

## Context

The Python ML sidecars expose a gRPC service consumed by the Rust core. The
original build plan specified that the STT sidecar listen "on a loopback UDS /
named pipe".

The Rust gRPC client is `tonic`. `tonic` ships first-class transports for TCP
and for Unix domain sockets, but has **no built-in Windows named-pipe
transport** — using one would require a custom `Connected` implementation over
`tokio::net::windows::named_pipe`. Windows is Lashon's primary target and a
required CI and smoke-test platform, so a transport that does not work cleanly
on Windows is not viable.

## Decision

The sidecar binds its gRPC server to `127.0.0.1` on an **ephemeral port** —
it asks the OS for a free port (port `0`). Immediately after binding it prints
the chosen port to stdout on a single, fixed-format line:

```
LASHON_STT_PORT=<port>
```

The Rust parent process spawns the sidecar, reads stdout until it sees that
line, parses the port, and connects the `tonic` client to
`http://127.0.0.1:<port>`.

## Rationale

- **One transport, three operating systems.** Identical code on Windows, macOS,
  and Linux — no `#[cfg(...)]` per-OS transport forks.
- **Native to `tonic`.** Zero custom transport code on a security-sensitive IPC
  path.
- **Ephemeral port.** Avoids fixed-port collisions and the flakiness they cause
  in CI and on busy developer machines.
- **Loopback-only.** Binding `127.0.0.1` keeps the surface on the local host.

## Alternatives considered

- **UDS everywhere.** Works on macOS and Linux, not on Windows. Rejected — no
  single code path.
- **Custom named-pipe `tonic` transport on Windows, UDS elsewhere.** Viable,
  but meaningful custom code on the IPC path, forked per OS. Deferred.

## Consequences

- A loopback TCP port is briefly bindable/observable by other local processes.
  This is acceptable for the current milestones; hardening — UDS on Unix, a
  named pipe on Windows, or a per-launch shared-secret handshake — remains open
  and would get its own ADR before any release that handles real transcripts.
- The stdout port-handshake line is the sidecar↔host contract. Its format is
  shared by `apps/desktop/src-tauri/src/sidecar.rs` and
  `services/stt-sidecar/src/lashon_stt/server.py`; changing it is a breaking
  change to both, together.
