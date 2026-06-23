# 32. Ship Lashon as an open-core product (free GPLv3 source, paid signed binary)

## Status

Accepted — 2026-06-11. Supersedes
[ADR-0023](0023-defer-code-signing-for-now.md): code signing moves from
"indefinitely deferred" to a critical-path requirement for the paid binary.

## Context

M0–M9 shipped; Lashon is a working local-first Hebrew dictation +
voice-PC-control app. The question is no longer "what to build next" but "how to
ship and distribute this." A survey of the desktop-dictation landscape informed
the approach:

- Desktop dictation is **commoditizing** — the STT model is freely available
  (Whisper / Parakeet / ivrit-ai), so the differentiator is the feature set,
  UX, and distribution, not the model itself.
- There is demonstrated demand for **one-time-payment, local, private** tools
  over cloud subscriptions.
- **Hebrew is underserved.** General-purpose tools treat Hebrew as a secondary,
  non-RTL-aware language and run vanilla Whisper. Lashon uses the
  Hebrew-specialized ivrit-ai model, which measurably outperforms generic
  Whisper on Hebrew, and is RTL-native with per-app Hebrew injection profiles.
- Existing free / open-source tools are largely cross-platform and
  English-first; none combine Hebrew-first + local + voice PC-control.

Lashon's positioning is therefore **Hebrew-first + fully local + command mode /
recipes**.

## Decision

Ship Lashon as an **open-core product**:

1. **Source stays free and open.** Relicensed **MIT → GPL-3.0-only** (this ADR)
   so the differentiator (command mode, recipes, Hebrew injection) cannot be
   absorbed into a *closed* derivative. GPLv3 still lets anyone use, modify,
   build, share, and sell — it only forbids closing derivatives.
2. **Offer a one-time paid signed binary.** The paid artifact is a
   **signed + notarized + auto-updating** build of the *same* GPLv3 source. The
   value is convenience and trust (no SmartScreen / Gatekeeper warning, real
   auto-update, supporting the project), **not** secret code. A free, unsigned,
   build-it-yourself path always exists.
3. **Never a subscription.** It violates the "free + local for everyone" design
   principle and runs counter to the demonstrated preference in the category.
4. **Primary audience = the Hebrew niche.** Israeli developers, accessibility /
   RSI users, and Hebrew dictation-heavy professions (law, medicine,
   journalism, academia). Lashon is Windows-first for v1.0, focused on the
   underserved Hebrew slice rather than the broad cross-platform market.

### Why GPLv3, not MIT (kept) or a feature-gated split

- **MIT (the old license)** lets a competitor take the Hebrew + command-mode
  work *closed*. For a product whose differentiator is exactly that work, that
  is the one freedom worth withholding.
- **The license does not protect the paid binary.** Every OSI license — MIT and
  GPL alike — permits rebuilding and redistributing the binary (free or paid).
  What protects "pay for the signed build" is **trademark + being the official
  source**, so the "Lashon" name and branding are held back from the grant.
- **No closed feature-gating.** GPLv3 forecloses linking proprietary paid
  features into the app later (a combined work would have to be GPL too). That
  is consistent with the chosen model — the *whole app* is free + open; you pay
  for the convenient binary, not to unlock code. Dual-licensing closed features
  would need a contributor CLA and is explicitly out of scope.

### Distribution: two channels

- **Free channel** — unsigned installers on GitHub Releases; auto-updates from
  `releases/latest/download/latest.json` (minisign-verified per
  [ADR-0017](0017-auto-update-via-tauri-plugin-updater.md)).
- **Paid channel** — signed / notarized installers distributed via a
  merchant-of-record (e.g. Lemon Squeezy / Paddle, which handle global VAT). The
  paid build auto-updates from its **own** minisign-signed endpoint, not the
  GitHub one. Same signing-key discipline as ADR-0017.

### Signing (supersedes ADR-0023)

ADR-0023 deferred signing indefinitely on OSS-economics grounds, naming
"SignPath reputation / a paid cert" as the unblock triggers. Selling a binary
*is* that trigger. Windows signing via **SignPath Foundation** (free for OSS
projects) is the first step; Apple Developer ID notarization ($99/yr) follows
only when the Mac platform is pursued. A paid build that throws "unidentified
developer" is not shippable, so signing is now on the critical path — not
deferred.

## Consequences

- **Relicense fallout (this PR):** `LICENSE` → GPLv3; the `license =` fields on
  the two workspace crates + the STT sidecar → `GPL-3.0-only`; `deny.toml`
  allows the project's own GPL-3.0-only (cargo-deny checks workspace members);
  the license-policy rules in `.claude/rules/security.md` + `CONTRIBUTING.md`
  shift from "no GPL" to "no AGPL / CC-BY-NC"; bundled llama.cpp's MIT notice
  moves from the (now-GPLv3) root `LICENSE` into `NOTICE`.
- **GPLv3 ⇒ no Mac App Store.** GPL's terms conflict with MAS usage rules (the
  VLC precedent). Direct `.dmg` + notarization is unaffected; MAS is simply off
  the table — which it already was.
- **Code signing and packaging** are the remaining work to a `v1.0.0` release.
  These are plumbing, not feature milestones — the product is largely there.
- **TTS (M10 / M11) stays dropped** for v1.0 — a silent dictation + command
  product is coherent. M12 (memory) is optional post-launch polish.

## Alternatives considered

- **Keep MIT, donations only.** Maximal permissiveness, but leaves the
  differentiator open to a closed fork. Rejected because GPLv3 keeps the
  paid-binary option open while still permitting donations — both doors stay
  open.
- **Subscription / cloud SaaS.** Rejected outright — violates the local-first,
  "free for everyone" identity.
- **Feature-gated open-core (free dictation, paid command mode).** Rejected for
  v1.0: it needs closed code (incompatible with one GPLv3 codebase) or a CLA +
  dual-licensing structure. (A *build-time* editioning of command mode, with the
  source staying fully open, is treated separately in
  [ADR-0034](0034-command-mode-editioning.md).)
