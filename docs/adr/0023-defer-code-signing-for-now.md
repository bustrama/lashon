# 23. Defer code signing until the project has reputation or budget

- **Status:** Superseded by [ADR-0032](0032-ship-as-open-core-product.md) — the
  decision to sell a signed binary puts code signing back on the critical path;
  the OSS-economics rationale below is retained for the history.
- **Date:** 2026-05-20
- **Deciders:** Lashon contributors
- **Context source:** [ADR-0006](0006-release-packaging-and-signing.md) (the
  v0.1.0 unsigned exception this ADR makes indefinite); milestone M13.

## Context

[ADR-0006](0006-release-packaging-and-signing.md) published `v0.1.0` as an
explicitly-marked unsigned GitHub pre-release with the rationale that
"obtaining a certificate and signing every binary is the immediate follow-up
for `v0.1.x`." Five unsigned pre-releases later (`v0.1.0` through `v0.5.0`),
no certificate has been obtained. This ADR records why, and what the path
forward looks like.

The cost surface for the three OS code-signing regimes:

- **Windows.** An EV / OV code-signing certificate from Sectigo / DigiCert /
  SSL.com costs $200–600 / year; Certum Open Source is ~$30 / year on a
  physical USB token shipped from Poland; Azure Trusted Signing is ~$10 /
  month with a business-identity verification. The free path is the
  [SignPath Foundation](https://signpath.org/) OSS sponsorship, which
  requires demonstrated repo reputation (stars, contributor count, sustained
  activity) — realistically a 6–12-month threshold for a fresh repo at
  `v0.5.0`.
- **macOS.** The Apple Developer Program is $99 / year. There is no
  OSS-sponsored equivalent; Apple does not extend free notarization to open
  source the way SignPath does for Windows.
- **Linux.** AppImage runs unsigned without warnings. `.deb` / `.rpm` could
  be GPG-signed for free if and when those formats ship.

The project is OSS with no commercial revenue and a single maintainer.
Several hundred dollars a year on code signing is not justified at the
project's current scale, and the free OSS-sponsorship path for Windows is
not yet open to a repo of this age.

## Decision

**Code signing is indefinitely deferred.** Lashon ships unsigned releases for
the foreseeable future. The signing portion of milestone M13 is gated on one
of the following triggers, whichever comes first:

1. The repo crosses SignPath Foundation's OSS-sponsorship reputation
   threshold (estimated 6–12 months of sustained activity, contributor
   growth, and visible adoption). At that point we apply for Windows code
   signing through SignPath, free.
2. A deliberate decision to spend $30 / year on Certum Open Source (Windows)
   or $10 / month on Azure Trusted Signing.
3. A sponsor or umbrella organisation extends an Apple Developer account to
   the project (Eclipse Foundation, ASF, etc. — slow paths, but possible).

**In-app auto-update signing is unaffected.**
[ADR-0017](0017-auto-update-via-tauri-plugin-updater.md) introduces
`tauri-plugin-updater` with minisign-based update signing. Minisign is OSS
(BSD-licensed), free, and entirely independent of OS-level code signing — it
authenticates the update payload, not the installer. Users get
cryptographically-verified updates even though the installer itself is
unsigned.

**Cross-OS releases land unsigned.**
[ADR-0018](0018-cross-os-installer-matrix.md) brings the macOS `.dmg` and
Linux `.AppImage` to the release pipeline. The Windows NSIS installer
continues to ship unsigned. Each per-OS packaging doc documents the
user-facing workaround:

- Windows: "More info → Run anyway" on the SmartScreen warning
  (`docs/packaging-windows.md`).
- macOS: right-click → Open the first time to bypass Gatekeeper
  (`docs/packaging-macos.md`).
- Linux: no action needed (`docs/packaging-linux.md`).

## Consequences

- Every Lashon release until one of the triggers fires continues to show
  SmartScreen / Gatekeeper warnings on first launch. The documented
  workarounds carry the gap.
- New-user friction is non-zero. Some users will be put off by the warning,
  and the cost lands disproportionately on non-developer Hebrew speakers
  less likely to right-click → Open or "Run anyway" on instinct. This is a
  known cost of the OSS-with-no-budget posture, accepted deliberately.
- ADR-0006's "the immediate follow-up for `v0.1.x`" sentence is superseded
  by this ADR. ADR-0006's status line points here.
- The signing-related M13 scope in `docs/roadmap.md` is split: auto-update
  lands in `v0.6.0` (ADR-0017), cross-OS packaging lands in `v0.6.0`
  (ADR-0018), code signing remains deferred without a target version.
- When a trigger fires the implementation work is mostly already in place:
  for Windows / SignPath, the existing `release.yml` (ADR-0018) needs a
  signing step calling SignPath's submission API; for macOS / Apple
  Developer, the existing `entitlements.plist` and `tauri-action` macOS
  bundling are ready, and `APPLE_*` GitHub Actions secrets plus the
  `--sign` / `--notarize` flags get wired in.

## Alternatives considered

- **Pay $30 / year for Certum Open Source.** Rejected for now: cheap, but
  still a paid recurring spend and requires a physical USB token shipped
  from Poland. Revisit if SignPath rejects the application or takes too
  long.
- **Self-signed certificate.** Rejected: SmartScreen ignores it entirely —
  no improvement over unsigned. ADR-0006 covered this.
- **Skip the OSS-sponsorship route; go straight to Azure Trusted Signing.**
  Rejected: $10 / month is small but recurring, and Azure requires a
  business-identity verification that is awkward for an individual
  maintainer.
- **Apply to SignPath now and accept the wait.** Rejected for now: the
  reputation bar means SignPath is realistically a future option, not a
  present one. Revisit once the repo has a year of public activity and
  external contributor traffic.
