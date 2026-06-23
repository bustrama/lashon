---
description: Hebrew is a first-class concern — test it at every layer, handle RTL correctly
globs: ["**/*"]
---

# Hebrew

Hebrew is first-class, not an afterthought — it is the product.

## Testing

- Every layer that handles text is tested with Hebrew — never English-only.
- Exercise mixed Hebrew/English (code-switching) too; it is common in real use.
- The WER benchmark over `tests/hebrew-corpus/` gates CI — see
  [`docs/testing.md`](../../docs/testing.md).

## Text injection

- Hebrew text injection must take the clipboard path (the UIA `SetValue` probe
  is skipped when Hebrew is detected). Some apps need explicit RTL marks
  (U+202B/U+202C) or a post-paste delay — that is what the per-app injection
  profiles are for.
- Dictation must snapshot and restore any pre-existing clipboard contents.

## Display

- Right-to-left ordering and combining marks must survive every transform.
  When in doubt, add a Hebrew test case and a mixed-script one.
