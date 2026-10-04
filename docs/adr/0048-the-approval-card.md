# 48. The approval card

## Status

Accepted — 2026-10-04. Phase B, part 3. Builds on the non-activating overlay of
[ADR-0044](0044-a-click-through-overlay-window.md) and the creature of
[ADR-0040](0040-the-overlay-becomes-a-living-creature.md). It replaces the
event-based confirmation round trips of
[ADR-0024](0024-command-mode-tool-dispatcher.md) (`command:confirm`) and
[ADR-0029](0029-cascade-as-dispatcher-pre-pass.md) (`recipe:confirm`).

## Context

Before a destructive step runs, Ottid asks the user. Until now there were two
separate ways of asking:

- **Command mode** emitted `command:confirm` and waited 30 s for
  `command:confirm:reply`. The island showed an Allow/Deny card.
- **Recipes** emitted `recipe:confirm`, but nothing in the frontend listened
  for it. Every `run_shell` step in a recipe waited 30 s and was denied,
  whether it was run from the Hub or matched by the voice cascade.

The card itself had gaps:

- It showed `run_command`'s command whole, but cut every other tool's
  arguments at 240 characters.
- Allow was live the moment the card appeared. The 700 ms arm delay in
  `docs/design-system.md` was never built, so a click meant for something
  else could approve.
- A bidi override or a zero-width character could make the command read
  differently from what would run.

ADR-0044 made the overlay non-activating. Tab, Enter and Esc never reach it,
so a keyboard or screen-reader user had no way to answer the card at all.
And if the user had hidden Ottid, a request waited where nobody could see it.

## Decision

### One broker

Every request for the user's yes or no goes through one broker:

- **The queue and its rules** live in `ottid_core::approval::Queue`. It is
  pure, the time is passed in, and it is unit-tested.
- **The shell's `approval` module** adds what needs Tauri: the reply channels,
  events, commands, the timeout task, the hotkeys and the window.
- **Asking.** Command mode awaits `approval::ask`. The recipe runtime's
  synchronous gate blocks on `approval::ask_blocking`. It asks from inside
  an async run on a Tauri tokio worker, so it blocks in `block_in_place`,
  which hands the worker's other tasks to another worker meanwhile.
- **Answering.** Only the overlay's card arms and answers a request. The Hub
  and the tutorial load the same frontend, so `approval_armed` and
  `approval_answer` refuse a call from any other webview (`not-the-card`).

One request is on screen at a time. Others wait in order, and the card shows
how many are waiting.

### The rules, enforced in Rust

- A request's clock starts when it is **shown**, not when it is asked.
- **Allow** is taken only when all three hold:
  - the request is the one on screen;
  - the card has reported it armed;
  - 700 ms have passed since it was shown.
- **Deny** is always taken.
- **Timeout.** A request on screen for 30 s without an answer is denied.
- **Withdrawal.** If the asker goes away (a new take cancels the command-mode
  task), its request is withdrawn.
- **Cancelling a voice-triggered recipe.** The recipe runtime waits for the
  card on a blocked thread, which aborting the take doesn't reach: a later
  Allow would still have run the step. So each take carries an
  `ottid_core::approval::Cancel`. Cancelling the take, or starting a new one,
  withdraws the request it waits on, which answers it Deny, and denies any it
  asks after. An answer that races the cancel is taken as Deny too.

The frontend's gate is for the user. The broker enforces the same rules on
its own, so a frontend bug or a hotkey cannot get round them.

### The card shows everything

- **What it shows.** For `run_command`, the command and its working
  directory. For any other tool, every argument as pretty-printed JSON.
- **Nothing is cut off.** A request that doesn't fit scrolls inside the card,
  within the island's maximum height.
- **Characters that would not show for what they are** become `U+XXXX`
  badges. That covers:
  - controls;
  - format characters (bidi overrides and isolates, zero-width characters,
    tags);
  - lone surrogates, and unassigned and private-use code points;
  - line and paragraph separators;
  - every space except the ASCII one, and the blank braille cell;
  - everything else Unicode marks as default-ignorable: variation selectors,
    the Hangul fillers, the combining grapheme joiner;
  - combining marks, except Hebrew marks on a Hebrew letter. An overlay mark
    disguises what it is drawn on (`=` with U+0338 looks like `≠`), and a
    decomposed accent is a different file name from the composed one.

  Niqqud, dagesh and cantillation on a Hebrew letter show as they are, up
  to five different marks on a letter: a dagesh or mappiq, a shin or sin
  dot, a vowel, a meteg and an accent. A mark repeated on the letter, or
  one past five, is a badge too, since a tall stack of marks draws over the
  lines around it.
- **Direction.** The text is shown left to right, in its own bidi isolate
  inside the RTL interface, and in the order it runs. Ordinary bidi would
  reorder a command line around its Hebrew words: `Copy-Item "דוח" "ארכיון"`
  drew its two arguments swapped, a Hebrew path drew its folders backwards,
  and a `>` between Hebrew words drew as `<`. So each word with a non-ASCII
  character is its own isolate, where Hebrew reads right to left. Between
  the isolates there is only ASCII and the non-ASCII characters PowerShell
  parses as syntax: its dashes (U+2013–U+2015) and smart quotes
  (U+2018–U+201E). A scan of every BMP code point through PowerShell's parser
  found no others but spaces, which are already badges. Inside a Hebrew
  word's isolate, a smart quote would be drawn on the far side of the word,
  and a command after it could look quoted. None of these turns a line
  around.

### Allow arms

**When it arms.** Allow arms once the whole request has been on screen for
**700 ms**. The 700 ms count only while all of these hold:

- the fonts are loaded and the text is laid out;
- the card has finished opening;
- the end of the scroll region has been in view at least once;
- the page is visible.

Leaving the screen starts the count over.

**Once armed**, the card tells the broker (`approval_armed`). Allow shows the
remaining delay as a fill, which reduced motion leaves out.

**Clicking early.** A click on Allow before it arms doesn't count. The card
says what is missing: scroll to the end, or wait a moment.

**Only a pointer clicks.** The buttons are out of the tab order and a press
doesn't focus them. If the overlay ever had the keyboard (after its menu),
Enter or Space would otherwise press a focused button. Allow also ignores a
click with no presses (`detail` 0), which is what Enter, Space or an
accessibility tool's invoke produces; it says the hotkey instead. The
keyboard path is the hotkeys, which the broker gates the same way.

### Keyboard: two hotkeys while a request is pending

- **The chords.** Holding `Ctrl+Shift+Y` for a second allows, and
  `Ctrl+Shift+N` denies.
- **Allow takes a hold.** Other apps bind `Ctrl+Shift+Y` (VS Code's debug
  console, Firefox's downloads), and a habit presses it as a tap. So a tap
  never allows: the chord has to stay down for `approval::HOLD` (1 s). While
  it is held, the Allow button fills; under reduced motion it is outlined
  instead. Let go too soon, and the card shows and says "hold it for a
  second". Deny takes a tap: a wrong Deny runs nothing.
- **Only the whole chord, held, allows.** The hotkey's events can't tell
  that on their own. On Windows, global-hotkey sends the press from the main
  thread and then starts a thread that polls the main key alone and sends
  the release when it comes up. Letting go of Ctrl or Shift goes unseen, and
  a release that never came would turn a tap into a hold. So only the hold's
  timer allows (`ottid_core::approval::Hold`): every 50 ms it asks the OS
  whether Ctrl, Shift and Y are all still down
  (`ottid_core::overlay::keyboard`, `GetAsyncKeyState`), and anything else
  ends the hold without allowing. A release before the timer allows is
  short, however long it lasted. Where the OS can't be asked, the Allow
  hotkey isn't registered or offered.
- **Only while needed.** The shell registers them only while a request is
  pending and unregisters them after.
- **Any layout.** They are physical keys (`Code::KeyY` and `Code::KeyN`, which
  global-hotkey registers as `VK_Y` and `VK_N`), so they work under the Hebrew
  layout or any other. `MOD_NOREPEAT` keeps a held key from repeating.
- **Reserved.** Hotkey validation rejects both chords, in any spelling, as
  Ottid bindings.
- **Allow pressed before the card arms** isn't taken, and no hold starts.
  The broker sends `approval:nudge`, and the card scrolls a page further
  through a long request and says so. For a keyboard user, this is the only
  way through a long command. Once everything has been seen, the card says
  to wait a moment.
- **A chord another app already holds** is left out of the card and out of the
  announcement.

### Screen readers

The overlay never has focus, so the card reaches a screen reader through live
regions:

- **An assertive region** reads each request once, when it is shown: the whole
  request, with hidden characters named; the two hotkeys; and the time limit.
- **A polite region** says why Allow isn't taken yet.

### The creature

While a request waits, Ottid shows the aqua **needs you** state
(`agent-needs-you`), whoever is asking: a command-mode tool, a recipe step, or
later an agent. The polite state announcement says "waiting for your
approval", not the agent wording.

### Visibility

If the user hid Ottid, the broker shows it while a request waits and hides it
again afterwards. If the user shows or hides Ottid from the menu in the
meantime, that choice stands.

### Shortcuts the overlay page owns

The overlay page used to call the plugin's `unregisterAll` before
re-registering its chords. The plugin keeps a single map for the shortcuts
registered from JavaScript and from Rust, so that call also removed the
approval hotkeys.

The page now unregisters only its own chords, one at a time. The webview's
capability `global-shortcut:allow-unregister-all` is replaced by
`global-shortcut:allow-unregister`.

## Alternatives considered

- **Make the overlay focusable while a card is pending.** It would take focus
  from the user's app, which ADR-0044 rules out. The user may be mid-sentence,
  and their next Enter would land in the card.
- **Enter and Esc as global hotkeys.** They would capture Enter and Esc in
  every app for up to 30 s, and Enter is the key a typing user is most likely
  to press without looking. That is an accidental approval.
- **A rarer chord for Allow, such as `Ctrl+Alt+Shift+Y`.** It makes a clash
  less likely, but one press would still allow, and `Ctrl+Alt` is `AltGr` on
  many layouts. A hold rules out the tap of a habit, whatever app it was
  meant for.
- **A separate, focusable approval window.** It takes focus too, with the same
  risk.
- **Fix the recipe listener and keep two flows.** Two flows had already
  drifted apart once, and that is how the recipe bug happened.
- **Enforce the gate only in the card.** The hotkeys bypass the card, and so
  would any bug in it. The broker checks for itself.
- **Truncate long requests behind a "show more".** The user has to be able to
  see every character before approving, and the arm delay should count only
  once they could have.

## Consequences

- **Recipe `run_shell` steps can be approved again.** Before this change, every
  one of them was denied.
- **The chords are taken while a request is pending.** For up to 30 s per
  request, `Ctrl+Shift+Y` and `Ctrl+Shift+N` reach Ottid instead of the app in
  front. That app's own binding for them, such as Chrome's private window or
  VS Code's debug console, waits until the card is answered. Pressing one
  out of habit only denies, or shows how to allow. Neither chord can be bound as an Ottid
  hotkey.
- **A queued request waits longer.** It may wait behind others before its
  30 s start. The blocking recipe asker has a 600 s backstop, so its thread is
  freed even if the broker never answers.
- **Showing the hidden overlay doesn't take focus.** Every show is
  `SW_SHOWNOACTIVATE` ([ADR-0044](0044-a-click-through-overlay-window.md)),
  and the window is `WS_EX_NOACTIVATE`. Revealing it for a card was observed
  not to change the foreground window, even while Ottid's own process owned
  the foreground, when an activation would have been allowed.
- **Focus stays with the user's app.** The risks left are the overlay's
  right-click menu and a button keeping focus, which the card prevents.
  - **The menu.** It brings the overlay to the front.
    `ottid_core::overlay::Handback` holds the give-back until the item picked
    is known, then hands the front back to the user's app.
  - **The menu's exceptions.** Settings, Tutorial and the logs folder open a
    window that takes the front. The give-back is settled after Settings and
    Tutorial run, so it still happens if their window couldn't take the
    front. After the logs folder, it happens only if the folder couldn't be
    opened.
  - **A backstop.** A click answer hands the foreground back with
    `Foreground::give_back`, to the window that was in front when the card
    came on screen, should the overlay ever end up in front.
- **The free edition has no broker.** Command mode is compiled out
  ([ADR-0034](0034-command-mode-editioning.md)), and the card never shows.
- **A debug build can show a sample card** with `approval_preview` (`long` and
  `hidden` samples too). Answering it runs nothing.
- **The `confirm` creature state is no longer entered** by the app. It stays in
  the creature data model, because user-made creatures
  ([ADR-0041](0041-user-authored-creatures-are-data.md)) define a pose for
  every state.
