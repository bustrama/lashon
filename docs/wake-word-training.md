# Train your own wake word for Lashon

Lashon's wake-word detector runs [openWakeWord](https://github.com/dscripka/openWakeWord) —
a small ONNX classifier that listens for one specific spoken phrase. Each
phrase needs its own classifier file. To use a custom wake word
(**"Hey Lashon"**, your own name, anything you like), train one in Google
Colab: about 30–60 minutes, free, all in your browser. The trained file then
runs locally on your machine — nothing leaves your device at runtime.

## Before you start

- A Google account (Colab's free tier is enough).
- ~30–60 minutes for the training run.
- A distinctive phrase, 2–3 syllables. "hey lashon", "okay luna" — good.
  "hello", "yes", a single word — bad, they'll fire constantly.

That's it. No local Python, no GPU, no big downloads on your machine until the
very end.

## Step by step

### 1. Open the training notebook in Colab

Open this link:

> **[Open Lashon's training notebook in Colab](https://colab.research.google.com/drive/1zzKpSnqVkUDD3FyZ-Yxw3grF7L0R1rlk#scrollTo=step1_preview)**

It's a Colab notebook prepared for Lashon — the openWakeWord training pipeline
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
listed above are also offered as one-click opt-in downloads in Lashon's
Settings Hub → **Wake word** → **More wake words** — they're
[CC-BY-NC-4.0](https://creativecommons.org/licenses/by-nc/4.0/), so Lashon
shows a "Non-commercial" badge before installing them.

### 2. Switch to a GPU runtime

In Colab's menu: **Runtime → Change runtime type → Hardware accelerator → T4
GPU → Save**.

Without a GPU the training takes hours. With the free T4 it's ~30–60 minutes.

### 3. Confirm the target phrase

The notebook is set to `target_word = "hey lashon"` by default. To train a
different phrase, find that cell near the top and change it — Hebrew phrases
work too (the underlying TTS includes Hebrew voices).

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
e.g. `hey_lashon.onnx`. In Colab's left sidebar **Files** panel, right-click
the file and choose **Download**.

### 6. Install it in Lashon

Drop the downloaded `.onnx` into Lashon's wake-words folder:

| OS | Path |
|---|---|
| **Windows** | `%LOCALAPPDATA%\dev.lashon.desktop\models\wakewords\` |
| **macOS** | `~/Library/Application Support/dev.lashon.desktop/models/wakewords/` |
| **Linux** | `~/.local/share/dev.lashon.desktop/models/wakewords/` |

The Hub picker reads filename stems from this folder and turns them into
friendly names — `hey_lashon.onnx` shows up as **Hey Lashon**, `my_dragon.onnx`
as **My Dragon**, and so on (underscores and hyphens become spaces, each word
is capitalised).

### 7. Pick it in the Settings Hub

In Lashon, double-click the tongue → **Settings Hub** → **Wake word**:
- Toggle **Enable** on.
- Select your model from the dropdown.

The wake worker live-reloads in under a second. Say your phrase — the tongue
switches to listening and dictation opens.

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

The Lashon wake-word engine ([ADR-0016](adr/0016-wake-word-engine.md)) expects
classifiers with an input shape of `[1, 16, 96]` — 16 audio embeddings of 96
dimensions each. openWakeWord's automated training produces exactly that shape
by default, so there is nothing extra to configure. A classifier trained with
a different framework or window size won't load — `lashon_core::wake::CLASSIFIER_WINDOW`
would need adjusting in code.
