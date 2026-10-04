"""Post-processing for raw STT output.

Whisper-family models occasionally leak special tokens or control characters
into the decoded text. ``sanitize`` strips them. The pattern set is lifted from
docs/roadmap.md §1.2.
"""
from __future__ import annotations

import re

# <|...|> Whisper special tokens (e.g. <|he|>, <|0.00|>).
_SPECIAL_TOKEN = re.compile(r"<\|.*?\|>")
# <ctrlNN> control markers.
_CTRL_MARKER = re.compile(r"<ctrl\d+>")
# C0 control characters, keeping tab (\x09) and newline (\x0a).
_CONTROL_CHARS = re.compile(r"[\x00-\x08\x0b-\x1f]")


def sanitize(text: str) -> str:
    """Strip Whisper special tokens and control characters from decoded text."""
    text = _SPECIAL_TOKEN.sub("", text)
    text = _CTRL_MARKER.sub("", text)
    text = _CONTROL_CHARS.sub("", text)
    return text.strip()
