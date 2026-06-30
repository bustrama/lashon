# 39. Ship v1.0.0 unsigned, Windows-only free edition (interim)

## Status

Accepted — 2026-06-30. An **explicit, recorded exception** to the "never ship a
release installer without code signing" invariant
([`.claude/rules/security.md`](../../.claude/rules/security.md),
[ADR-0006](0006-release-packaging-and-signing.md)) — the same kind of recorded
exception the unsigned `v0.1.0` preview took. Decided by the product owner.

## Context

v1.0.0 is ready: the free, dictation-only edition
([ADR-0034](0034-command-mode-editioning.md)) with unbounded long-form dictation
([ADR-0037](0037-tail-only-windowed-redecode.md),
[ADR-0038](0038-tolerate-long-pauses-in-dictation-endpointing.md)). Code-signing
— Authenticode for the installer and the minisign updater key — is **not yet
wired**; it is the documented v1.0.0 critical-path item
([ADR-0032](0032-ship-as-open-core-product.md),
[ADR-0033](0033-focus-on-windows-for-v1.md)). The owner chose to publish v1.0.0
now rather than block the launch on signing.

## Decision

Publish **v1.0.0 as a public, unsigned, Windows-only, free-edition** GitHub
Release.

- **Unsigned** — an explicit exception to the signing rule, recorded here.
  Consequences accepted: Windows SmartScreen warns on first launch ("More info →
  Run anyway"); the in-app updater has **no valid signature** for this release,
  so auto-update is effectively deferred until signing lands.
- **Windows-only** — the macOS matrix entries are dropped for v1.0.0
  (Windows-first, ADR-0033); re-add them when macOS ships.
- **Free edition** — built with `--no-default-features` (drops command mode,
  ADR-0034) and `VITE_LASHON_EDITION=free` (drops the command-mode Hub surface),
  and `--no-sign` (no updater signing without a key). All three are set in
  `release.yml`.

A **signed v1.0.x** follow-up supersedes this exception once the signing key
(Authenticode + `TAURI_SIGNING_PRIVATE_KEY`) is set up; at that point the
updater can resume and the SmartScreen warning goes away.

## Consequences

- The free dictation-only build reaches users now, unblocked by signing.
- Users see a SmartScreen warning and must "Run anyway" — documented in the
  release notes.
- Auto-update does not carry this version forward; the next, signed release will
  re-establish the updater path.
- This is interim: the invariant stands, and the next release closes the
  exception.

## Alternatives considered

- **Hold for signing** — rejected by the owner: ship the dictation product now;
  signing follows.
- **Unsigned pre-release** — rejected: the owner wants a public v1.0.0, not a
  flagged beta.
