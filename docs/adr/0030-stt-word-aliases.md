# 30. Post-STT word-aliases — `stt.word_aliases` settings shape

## Status

Accepted — landed in PR #81 alongside the dispatcher cascade wire-up
(ADR-0029).

## Context

Whisper biases its tokenizer toward common high-frequency tokens. On
English-leaning audio "Claude" (rare proper noun) consistently lands
as "cloud" (common noun) — confirmed in the user's testing this
session. Per-user contact-name homonyms (Kookie / Kuki) and Hebrew
transliteration drift (קלאוד / קלוד) have the same shape: the
mismatch is **per-user vocabulary, applied identically to every
take**, fixable once.

Two layer options for the fix:

1. **STT initial_prompt.** Probabilistic — bias Whisper's tokenizer
   at the source. Requires a sidecar gRPC schema bump (new field on
   the transcribe RPC) and per-take prompt construction.
2. **Post-STT word substitution.** Deterministic — Whisper produces
   whatever it produces; a fixed-table substitution layer corrects
   recurring misrecognitions before any downstream consumer sees the
   transcript.

These aren't mutually exclusive — the initial_prompt is prevention,
substitution is the cure for what slipped through. We ship the
cheaper one first.

## Decision

Add a deterministic post-STT word-substitution layer keyed off
`stt.word_aliases` in `settings.json`. Schema:

```json
{
  "stt.word_aliases": {
    "cloud": "claude",
    "kookie": "kuki",
    "קלאוד": "קלוד"
  }
}
```

Substitution semantics (see `lashon_core::transcript::apply_aliases`):

- **Case-insensitive matching.** Author writes the key in any case;
  lookup lowercases the key on init. `cloud`, `Cloud`, `CLOUD` all
  match an alias declared as `cloud`.
- **Word-boundary respect.** Only whole tokens substitute; `cloudy`
  stays `cloudy`. A "token" is a maximal run of `is_alphanumeric()`
  (Unicode-aware) plus underscore.
- **Capitalisation-preserving output.** `Cloud` → titlecased
  replacement (`Claude`); `CLOUD` → uppercase (`CLAUDE`);
  `cloud` → the alias value verbatim (author controls case via the
  map). Hebrew has no case so this is a no-op for Hebrew tokens.
- **Punctuation passes through.** `Hi, cloud!` → `Hi, claude!`.
- **Empty alias map is a free pass.** No allocation, no walk; the
  hot path on a user with no corrections set has zero overhead.

Applied in `apps/desktop/src-tauri/src/command_mode.rs::run` between
STT and the cascade pre-pass (ADR-0029), so **both the recipe
cascade and the LLM full planner see the corrected transcript**.
Single point of fix for per-user vocabulary issues.

## Settings UI

A new Hub section "תיקוני זיהוי / Voice corrections"
([`VoiceCorrectionsSection.svelte`](../../apps/desktop/src/lib/voice/VoiceCorrectionsSection.svelte))
manages the map. Two Tauri commands (`get_word_aliases` /
`set_word_aliases`) read/write the setting; the commands strip
empty-key entries defensively.

## Tracing posture

The substitution call logs **the count of aliases applied** at INFO
when at least one substitution fires. Never logs the keys or the
values — the security rule `.claude/rules/security.md` forbids
logging transcript content, and aliases can carry contact names that
qualify as PII. The structural log is enough to confirm "the
substitution happened on this take" without leaking what changed.

## Consequences

- **Solves the originating "claude → cloud" issue end-to-end.** The
  cascade sees `claude` in the intent text; the LLM sees `claude` in
  the user prompt; recipes that extract `{recipient}` get `claude`
  as the value (not `cloud`).
- **One source of truth per user.** Adding a new contact's
  misrecognition fix is one row in the Hub, not a per-recipe edit.
- **Zero cost on users who don't use it.** Empty-map short-circuit
  in `apply_aliases` means the substitution call is a `HashMap::is_empty()`
  check and a clone of the input transcript.
- **Doesn't help inside the spoken word — only token-level.** If
  Whisper hears "claude says cloud" the cascade extracts whichever
  half is in the slot; aliases can correct one direction but not
  disambiguate context-sensitive cases. The STT initial_prompt
  layer (future) can biases the model away from the wrong word in
  the first place.
- **Pairs with future initial_prompt work** — once that ships,
  fewer substitutions actually trigger (Whisper produces "claude"
  natively more often) but the safety-net layer stays valuable for
  whatever still slips through.

## What's NOT in scope

- **Per-recipe `Parameter.aliases`.** Considered (would have been
  schema-level recipe-author-controlled aliases on individual
  slots) and rejected: global word-aliases in user settings cover
  the same cases with one config point and benefit the LLM path
  too.
- **Multi-word aliases** (`"cookie monster" → "kuki monster"`).
  Could add later; v1 is single-token only. The tokenizer splits
  on whitespace so a "cookie monster" alias key wouldn't match
  anyway in the current implementation.
- **Regex-based aliases.** Same — fixed-string substitution is
  enough for v1 and the case-preservation heuristic gets confused
  by capture groups.

## Notes

The `stt.word_aliases` setting is not migrated to the keychain
(it's not a secret — it's a vocabulary list). Lives in
`settings.json` alongside other Hub-managed preferences.
