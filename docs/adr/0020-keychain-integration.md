# 20. Keychain integration for cloud API keys

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Lashon contributors
- **Context source:** Milestone M7, Phase 2
  ([`../stories/m7-provider-mux.md`](../stories/m7-provider-mux.md)).
  Related: [ADR-0010](0010-harden-the-stt-sidecar-trust-boundary.md)
  (secrets never reach logs).

## Context

M7 introduces cloud STT and LLM providers, each requiring an API key. The
security rules (`.claude/rules/security.md`) are explicit:

> "Keys live only in the OS keychain (`keyring`); `.env` files are git-ignored."
> "Never log transcript content, audio, or PII — not even at debug level."

Cloud API keys are high-value secrets: a leaked key lets a third party run
inference on the owner's account and generate costs. The key must:

1. Never be written to disk in plaintext.
2. Never appear in logs, crash reports, or Tauri event payloads.
3. Survive an app restart — the user must not re-enter keys after a reboot.
4. Be retrievable by the Rust provider code without requiring the user to
   re-enter it every session.
5. Work on all three target platforms (Windows, macOS, Linux) without
   requiring additional user-visible setup on Windows and macOS (the most
   common platforms).

Two design decisions are needed: which Rust mechanism to use, and what
interface to expose to the Tauri frontend.

## Decision

### Storage mechanism: the `keyring` crate

`keyring` wraps the native OS credential store on each target:

- **Windows** — Windows Credential Manager (Win32 `CredWrite` / `CredRead`).
  Keys appear as "Generic" credentials under the name `lashon/<key_name>`.
  No extra runtime dependencies; no extra entitlements.
- **macOS** — Keychain Services. Items appear in the user's login keychain
  under the service `"lashon"` and account `"<key_name>"`. The app must
  hold the `keychain-access-groups` entitlement in `entitlements.plist`
  (already required for audio capture; no new entitlement category).
- **Linux** — `libsecret` (GNOME Keyring) or `kwallet` via the D-Bus Secret
  Service API. The `keyring` crate's `SecretService` backend handles both;
  the `keyring` crate feature `linux-native` selects it.

The `keyring` crate is already listed in `docs/tech-stack.md` as the intended
mechanism. M7 is the first milestone that makes it load-bearing.

**Version:** `keyring 3.x`. The implementer pins the exact version in
`packages/shared-rust/Cargo.toml` after confirming the latest stable release
(do not write a version here — pinning is the implementer's job).

### `lashon-core::keychain` module

A thin, cross-platform wrapper with three functions:

```rust
pub fn store_key(key_name: &str, secret: &str) -> anyhow::Result<()>;
pub fn get_key(key_name: &str) -> anyhow::Result<Option<String>>;
pub fn delete_key(key_name: &str) -> anyhow::Result<()>;
```

Service name: `"lashon"` (constant; not configurable). This scopes all
Lashon keys together in the OS credential store.

`get_key` returns `Option<String>` — `None` if the key is absent (not yet
stored), `Err` if the OS keychain is unavailable (e.g. no daemon on Linux).
Callers distinguish "not stored" from "keychain error".

The module is in `lashon-core` (not the Tauri shell) so it can be called
directly from provider constructors without going through a Tauri command.

### Tauri command surface

Two commands, both in the Tauri shell, both calling `lashon-core::keychain`:

```rust
// Stores a key. The `secret` argument never appears in any return value,
// event, or log line. On success, returns Ok(()); on failure returns an
// opaque error string (no key material in the error).
#[tauri::command]
async fn save_api_key(stage: String, provider: String, secret: String)
    -> Result<(), String>;

// Returns true if a key is stored for this (stage, provider) pair.
// Never returns the key value.
#[tauri::command]
async fn has_api_key(stage: String, provider: String) -> bool;
```

There is intentionally no `get_api_key` command. The frontend can store a
key and can check presence, but it cannot retrieve the raw value. This is
the same principle as ADR-0010 ("the token travels only on the stdout pipe
... never logged").

### Key naming convention

```
key_name = "<stage>.<provider>"
```

Examples: `"stt.groq"`, `"stt.openai"`, `"stt.elevenlabs"`, `"stt.deepgram"`,
`"stt.assemblyai"`, `"llm.anthropic"`, `"llm.openai"`, `"llm.groq"`,
`"llm.minimax"`, `"llm.deepseek"`, `"llm.mistral"`, `"llm.together"`,
`"llm.openrouter"`. Ollama (local or remote) has no key — the endpoint URL
is a non-secret setting in `settings.json`.

### Provider construction: lazy, on-demand

A cloud provider is not constructed at app startup. It is constructed lazily
when it is first set as the active provider. The constructor calls
`keychain::get_key` to retrieve the secret. If the key is absent, the
constructor succeeds but marks the provider as `unconfigured`; the first
`transcribe` / `chat` call returns `Err(ProviderError::KeyNotFound { provider:
"groq" })`. The dictation FSM turns this into a user-visible error toast:
"Provider 'Groq' needs an API key — configure it in Settings."

This means app startup is fast regardless of how many cloud providers the user
has configured, and a keychain read error (e.g. the Linux daemon is not
running) surfaces as a per-call error rather than a startup crash.

### Linux caveat and env-var fallback

On Linux headless environments (CI runners, SSH sessions, Wayland sessions
without a running keyring daemon) `keyring`'s `SecretService` backend will
return an error when asked to store or retrieve keys. This is expected and
documented.

For headless and CI use, each provider checks for a
`LASHON_<STAGE>_<PROVIDER>_KEY` environment variable as a fallback before
consulting the keychain:

```
LASHON_STT_GROQ_KEY
LASHON_STT_OPENAI_KEY
LASHON_LLM_ANTHROPIC_KEY
… etc.
```

These env vars are never set in CI by default. They are documented in
`CONTRIBUTING.md` for developers who want to run cloud integration tests
locally without a keyring daemon. They are also useful for server or
containerized Lashon deployments (not the primary use case, but not broken).

The precedence is: keychain first; env var as fallback.

### Crash reporter scrubbing

The crash reporter (when it ships in M13) must never include keychain reads
or the raw key values. The existing pattern from ADR-0010 — "the token never
reaches a log line" — is extended to all cloud keys. The `tracing` call sites
in `lashon-core::keychain` use `tracing::debug!("keychain store: {key_name}")`,
never the secret value.

## Alternatives considered

- **`.env` file in the app data directory** — explicitly rejected by
  `.claude/rules/security.md`. A file on disk can be read by any process
  running as the same user, copied with a directory backup, and does not get
  scrubbed from crash dumps automatically.
- **Tauri's `tauri-plugin-store` encrypted store** — `tauri-plugin-store`
  stores data in a JSON file; it can optionally encrypt with `tauri-plugin-stronghold`,
  which uses the IOTA Stronghold library backed by Argon2 password-derived
  encryption. This is more portable (works on Linux without a daemon) but
  requires the user to set a master password and adds a heavy dependency.
  The OS keychain — which uses the user's login credentials as the master
  secret — is simpler for the user and is already the conventional mechanism
  for desktop apps.
- **Secrets in `settings.json` encrypted at rest** — same concerns as
  the `.env` approach; the encryption key has to live somewhere.
- **Asking for the key on every session** — poor UX; the `keyring` crate
  solves the persistence problem directly.

## Consequences

- `packages/shared-rust/Cargo.toml` gains `keyring = "=<version>"`.
  The `linux-native` feature is enabled on Linux targets
  (`[target.'cfg(target_os = "linux")'.dependencies]`).
- `packages/shared-rust/src/keychain.rs` — new module.
- The Tauri app capability `capabilities/hub.json` is unchanged; the new
  Tauri commands (`save_api_key`, `has_api_key`) do not require new
  file-system or network permissions beyond what the hub window already has.
- `apps/desktop/src-tauri/entitlements.plist` — on macOS, add
  `keychain-access-groups` if not already present (check the M4/M6 entitlements).
- The Linux developer documentation (`CONTRIBUTING.md`) gains a section on
  the keyring daemon requirement and the `LASHON_*_KEY` env-var fallbacks.
- Cloud provider integration tests are tagged `#[ignore]` and documented as
  requiring either a keyring daemon or the env-var fallback.
