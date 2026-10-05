# Train your own wake word for Ottid

Ottid's wake-word detector runs [openWakeWord](https://github.com/dscripka/openWakeWord) —
a small ONNX classifier that listens for one specific spoken phrase. Each
phrase needs its own classifier file. To use a custom wake word
(**"Hey Ottid"**, your own name, anything you like), train one in Google
Colab: about 30–60 minutes, free, all in your browser. The trained file then
runs locally on your machine — nothing leaves your device at runtime.

> **The built-in wake phrase is still "Hey Lashon".** Ottid used to be called
> Lashon. A wake-word model is trained on a spoken phrase, so renaming the app
> doesn't change what it listens for. A "Hey Ottid" model is in the works and
> will ship in a future update. Until then, say "Hey Lashon", or train your own
> phrase below.

## Before you start

- A Google account (Colab's free tier is enough).
- ~30–60 minutes for the training run.
- A distinctive phrase, 2–3 syllables. "hey ottid", "okay luna" — good.
  "hello", "yes", a single word — bad, they'll fire constantly.

That's it. No local Python, no GPU, no big downloads on your machine until the
very end.

## Step by step

### 1. Open the training notebook in Colab

Open this link:

> **[Open Ottid's training notebook in Colab](https://colab.research.google.com/drive/1zzKpSnqVkUDD3FyZ-Yxw3grF7L0R1rlk#scrollTo=step1_preview)**

It's a Colab notebook prepared for Ottid — the openWakeWord training pipeline
with sensible defaults for a Hebrew wake phrase.

> **Note — the notebook is still being polished.** Expect a couple of cells
> that need small adjustments (target phrase, batch size on a busy Colab GPU,
> the export filename). If a cell errors, the message usually points at the
> exact line to tweak. We'll fold the fixes into the linked notebook itself
> as they're confirmed.

### Already have a wake word? Try a prepared one first

If you'd rather not train at all, the openWakeWord project ships a small
library of ready-to-use classifiers ("Hey Jarvis", "Alexa", "Hey Mycroft",
"Hey Rhasspy" and others) at **<https://openwakeword.com/library>**. The four
listed above are also offered as one-click opt-in downloads in Ottid's
Settings Hub → **Wake word** → **More wake words** — they're
[CC-BY-NC-4.0](https://creativecommons.org/licenses/by-nc/4.0/), so Ottid
shows a "Non-commercial" badge before installing them.

### 2. Switch to a GPU runtime

In Colab's menu: **Runtime → Change runtime type → Hardware accelerator → T4
GPU → Save**.

Without a GPU the training takes hours. With the free T4 it's ~30–60 minutes.

### 3. Confirm the target phrase

The notebook's default is still `target_word = "hey lashon"`. Find that cell
near the top and change it to `"hey ottid"`, your own name, or anything else —
Hebrew phrases work too (the underlying TTS includes Hebrew voices).

### 4. Run all cells

Choose **Runtime → Run all** and let it work. The notebook will:

1. Install openWakeWord and dependencies (~1 min).
2. Synthesise hundreds of recordings of your phrase across many voices, accents
   and pitches using a TTS model.
3. Augment them with background noise and room reverberation.
4. Download a large set of precomputed "negative" audio features (~30 GB on
   Colab's disk — fast over Google's network).
5. Train a small classifier head (10–30 minutes of GPU time).
6. Convert the result to ONNX.

You'll see progress logs as it goes. The dataset download is the slowest
single cell; the training cells show loss curves.

### 5. Download the model

When training finishes the notebook produces a file named after your phrase,
e.g. `hey_ottid.onnx`. In Colab's left sidebar **Files** panel, right-click
the file and choose **Download**.

### 6. Install it in Ottid

Drop the downloaded `.onnx` into Ottid's wake-words folder:

| OS | Path |
|---|---|
| **Windows** | `%LOCALAPPDATA%\app.ottid.desktop\models\wakewords\` |
| **macOS** | `~/Library/Application Support/app.ottid.desktop/models/wakewords/` |
| **Linux** | `~/.local/share/app.ottid.desktop/models/wakewords/` |

Still on a Lashon 1.x build? Use `dev.lashon.desktop` in place of
`app.ottid.desktop`. Ottid moves that folder over the first time it starts.
(Installed release builds do this. A debug build, such as `npm run tauri dev`,
leaves the old folder alone unless `OTTID_ADOPT_LEGACY=1` is set —
[ADR-0046](adr/0046-debug-builds-leave-pre-rename-data-alone.md).)

The Hub picker reads filename stems from this folder and turns them into
friendly names — `hey_ottid.onnx` shows up as **Hey Ottid**, `my_dragon.onnx`
as **My Dragon**, and so on (underscores and hyphens become spaces, each word
is capitalised).

### 7. Pick it in the Settings Hub

In Ottid, double-click the creature on your screen → **Settings Hub** →
**Wake word**. (Right-clicking Ottid, or the tray icon, and choosing
**Settings** opens the same window.)
- Toggle **Enable** on.
- Select your model from the dropdown.

The wake worker live-reloads in under a second. Say your phrase — Ottid hops
and chimes, its lamp lights up gold, and it starts writing on a notepad:
dictation is open. While the wake word is armed, Ottid's resting lamp is slate
grey instead of dim peach — the microphone is open.

## Tips

- **Phrase quality matters more than training time.** A distinctive 2–3
  syllable phrase trains and detects better than a single word.
- **Avoid common conversation words.** "Hello", "yes", "computer" will fire
  all the time.
- **Tune sensitivity in the Hub, not the model.** If detection is too eager or
  too dull, slide sensitivity instead of retraining.
- **The Hub picker reflects what's in the folder.** Drop `.onnx` files in,
  re-open the **Wake word** section, and they appear.

## Sharing what you train

Classifiers you train are yours — you can publish them with any licence you
choose (Hugging Face is a common host). Others can drop them into their own
`wakewords/` folder.

> The pretrained classifiers that ship with openWakeWord's GitHub releases
> ("hey_jarvis", "alexa", "hey_mycroft", "hey_rhasspy", …) are
> **CC-BY-NC-SA-4.0** — fine for personal, local use, but they cannot be
> redistributed in a commercial bundle. Models you train yourself avoid that
> restriction entirely.

## Troubleshooting

- **Colab errors during the dataset download.** Colab sometimes throttles.
  Wait a few minutes and re-run the failing cell.
- **"Out of memory" during training.** Reduce the batch size in the training
  cell (the notebook usually has a comment about this).
- **The picker doesn't show my model.** Confirm the file is directly in
  `wakewords/` (not a subfolder) and ends in `.onnx`. The picker label is a
  title-cased rendering of the filename stem.
- **The picker shows it but the wake word never fires.** Check **Enable** is
  on, and that you're saying the exact phrase you trained. Try increasing
  sensitivity. The dev console prints `wake word: detected` when it fires.

## Technical note

The Ottid wake-word engine ([ADR-0016](adr/0016-wake-word-engine.md)) expects
classifiers with an input shape of `[1, 16, 96]` — 16 audio embeddings of 96
dimensions each. openWakeWord's automated training produces exactly that shape
by default, so there is nothing extra to configure. A classifier trained with
a different framework or window size won't load — `ottid_core::wake::CLASSIFIER_WINDOW`
would need adjusting in code.
