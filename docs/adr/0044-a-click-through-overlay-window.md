# 44. A large click-through overlay window

## Status

Accepted — 2026-10-04. Phase B, part 1. Carries out the window half of
[ADR-0040](0040-the-overlay-becomes-a-living-creature.md) ("the overlay window
grows"); the creature itself (WebGL) is Phase B2, the approval card Phase B3.

## Context

Until now the overlay (window `main`) was a 104 × 104 window around the mark,
drawn by `Tongue.svelte`. The frontend resized it to fit command mode's cards,
dragged it through a Rust loop it started and stopped, and decided
click-through itself (`clickThrough.ts`).

ADR-0040's creature does not fit in that window. Its arms reach above the body,
the ceiling placement hangs from the top edge, props stick out, and the
approval card (B3) needs room next to it. A window that resizes around
whatever is drawn is fragile, and the old one had a standing bug: **clicks on
elements marked `data-interactive` sometimes fell through to the app below.**

The root cause of that bug was where the decision was made. `clickThrough.ts`
polled the cursor from the webview on a `setInterval`, and each tick was async:
it asked Rust for the cursor, hit-tested in JavaScript, then asked Rust to set
the flag. So:

- ticks overlapped, and their `setIgnoreCursorEvents` calls could land out of
  order;
- the cached flag (`currentIgnoring`) was set before the call and never rolled
  back when a call failed or was overtaken, and the OS was only called when the
  cache changed. Once the cache said "accepting" while the window was really
  ignoring the mouse, nothing corrected it, and clicks on the marked elements
  went to the app below until the cursor left and came back;
- the window's position was read in its own async hop, while the Rust drag
  loop was moving the window, so the hit test could use a stale origin;
- on top of that, the marker sat on `.tongue`, the wrapper the window was sized
  to (against its own comment), so a hit took the mouse for the whole window;
  and `data-tauri-drag-region` turned presses on the mark into native drags,
  which swallowed clicks and made double-clicks maximize the window.

Coucou (MIT) solved the same problem for its desktop companion with a cursor
poll in Rust that owns the flag. Its `LICENSE-ASSETS.md` reserves its names,
character, artwork, icons, sounds and media, so we ported mechanics only.

## Decision

1. **One fixed, large, non-activating window.** `main` is 420 × 520 CSS px
   (scaled per display and capped to its work area): transparent, undecorated,
   always on top, out of the taskbar. `focusable: false` gives it
   `WS_EX_NOACTIVATE` on Windows, so clicking it never takes focus from the
   user's app. It is created hidden with `focus: false`, so the first show does
   not activate it either, and shown once it is placed.
2. **The Rust shell owns the window.** Position, size and the click-through
   flag are set only by `apps/desktop/src-tauri/src/overlay.rs`; the overlay's
   capability grants the webview no window permissions. The geometry (layout,
   snapping, monitor choice, hit-testing, gaze) is plain code in
   `ottid_core::overlay`, unit-tested.
3. **Click-through by a single-owner cursor poll.** One thread reads the global
   cursor (physical pixels: `GetCursorPos` under per-monitor-v2 DPI
   awareness), hit-tests it, and sets `set_ignore_cursor_events` in the same
   tick: every 16 ms, every 8 ms while dragging, every 200 ms while hidden. It
   remembers the flag it set and sets it again after a failure, a move, or a
   hide and show. The frontend only reports **regions**: every element marked
   `data-interactive="<name>"`, measured in physical pixels relative to the
   window, with the scale they were measured at. The poll rescales them if the
   window has since moved to a display with another scale, adds an 8 px margin,
   and keeps the mouse while a drag is held. Everything else passes through.
4. **Three placements, saved.** Taskbar, float and ceiling
   (`overlay.placement`), plus the stage centre in physical pixels
   (`overlay.anchor`). The creature stands in a 220 × 120 CSS px stage; the
   window is laid out around it and clamped into the work area, with the cards
   above the stage or below it. The user picks a placement from the tray menu
   or the Hub, or drags the creature: past 4 px a press becomes a drag, the poll
   moves the window, and the drop snaps to the taskbar or the top edge when
   within 40 px of it, and floats otherwise. Leaving an edge for float from the
   menu lifts Ottid by 80 px so a drag doesn't snap it straight back. Displays
   are re-read twice a second; a change, or the OS having moved the window,
   lays it out again. The old `tongue.position` is migrated once.
5. **Events.** The shell sends the frontend `overlay:layout` (the frame: size,
   stage, card side, scale), `overlay:hover` (the region under the cursor) and
   `overlay:gaze` (where the creature should look: `tanh` of the distance from
   its eyes to the cursor). The webview sees no mouse moves while it ignores
   the mouse, so hover and gaze come from the poll.
6. **The creature contract (for B2).** `src/lib/creature` exports `Creature`
   with `CreatureProps`: `state` (the twelve design-system states, mapped from
   the FSM by `creatureState()`), `placement`, `gaze`, `level`, `wakeArmed`,
   `dragging`, `hovered`, `pokes` and `mode`. Today it is a stand-in (the mark
   with a lamp halo). The creature marks its hit area
   `data-interactive="creature"` and stands in the stage the way its placement
   says. B2 replaces the component behind the export; the overlay does not
   change.
7. **What came from Coucou.** The cursor poll and click-through toggle, the
   display check, the cursor and button reads, the damped spring and
   cubic-bezier easing, the hover rules that keep a card open, and the gaze
   mapping, each rewritten for Ottid. The files are listed, with Coucou's MIT
   notice, in `THIRD-PARTY-NOTICES`, and each carries a header comment.

## Consequences

- **The overlay takes no keyboard input.** It never has focus, so the old Esc
  shortcuts (deny the confirm card, cancel, hide) are gone. Hide is a menu
  item. Keyboard approval of the card (B3) has to come through a global
  hotkey; B3 decides how.
- The poll is one thread doing a cursor read per frame while Ottid is visible,
  and five per second while hidden.
- Every interactive element must carry `data-interactive`. Anything unmarked
  is click-through, by design.
- Command mode's cards no longer grow the window. They get a maximum height
  inside it and scroll.
- **Wayland** has no global cursor, so there the window is click-through and
  display-only; the hotkeys and the tray menu still work. macOS and Linux
  compile and read the cursor through Tauri; only Windows is tested.
- After **Hide** then **Show**, the window is shown with `SW_SHOW`, which may
  activate it once.
- The taskbar placement stands on the bottom of the work area. With the
  taskbar on a side or at the top, Ottid stands on the bottom edge of the
  screen instead.

## Alternatives considered

- **Keep the small window and resize it around the content.** Rejected. The
  creature's reach changes every frame, and every resize is an async round
  trip that races the drawing (the M8.3 cropping bugs).
- **Fix the webview poll.** Rejected. Serialising the ticks and rolling back
  the cache would still decide one IPC hop away from the cursor read and the
  OS flag. A single owner in Rust removes the race instead of narrowing it.
- **Per-pixel click-through from the window's alpha** (`WS_EX_LAYERED` hit
  testing). Rejected. WebView2 doesn't draw through a per-pixel-alpha layered
  window, so the OS can't hit-test the webview's alpha, and it isn't portable.
- **One window per surface** (creature, cards). Rejected for now. It doubles
  the placement and z-order work, and the cards have to move with the creature.
