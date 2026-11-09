# Live captions and subtitles

> **Last reviewed:** 2026-11-07 · **Last updated:** 2026-11-07 · **Change:** created (#1811)

Slide Show ▸ Captions & Subtitles recognises what the presenter says with a speech model that runs
on this machine and draws the text over the slide, or in the letterbox beside it. Nothing is sent
anywhere: the model is downloaded once, the microphone is read locally, and inference makes no
network call.

The feature is one row of [slideshow-parity.md](slideshow-parity.md); the choices behind it are in
[decisions.md](decisions.md).

## The model

`openai/whisper-tiny` — Apache-2.0 weights, multilingual, 153.5 MB on disk.

| File | Bytes | SHA-256 |
|---|---|---|
| `model.safetensors` | 151,061,672 | `7ebd0e69e78190ffe1438491fa05cc1f5c1aa3a4c4db3bc1723adbb551ea2395` |
| `config.json` | 1,983 | `ffdccec4f3211f4c63310f2b7098f309fe70f3952cedc5e4d11e43f5b2379b98` |
| `tokenizer.json` | 2,480,466 | `27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566` |

- Revision: `169d4a4341b33bc18d8881c4b69c2e104e1cc0af` of
  <https://huggingface.co/openai/whisper-tiny>.
- Licence: Apache-2.0 (the Hugging Face model card at the pinned revision). The manifest records
  it, and Subtitle Settings shows it,
  with the source, total size and per-file hashes, before anything is downloaded.
- Weights are never committed to this repository and never shipped in a release: the manifest in
  `crates/asr/src/model.rs` (`model::whisper_tiny`) is the only part that lives here.
- The hashes above were measured on 2026-11-07 by downloading the three files and running
  `sha256sum` on them; they match the manifest byte for byte.

## Installing the model

Two paths, both supported:

1. **In the app.** Slide Show ▸ Subtitle Settings ▸ **Download**. The dialog shows the source,
   size, licence and hashes; the desktop host fetches the files over HTTPS (`ureq`) from the
   model hub only (a manifest URL on another host or a plain-HTTP URL is refused before any
   request), and reports progress. Each file is written to an exclusive, process-tagged `.part`
   file, checked against the manifest's size and SHA-256, and only then renamed, so an interrupted
   download never looks installed and a pre-planted file or symlink is not followed. A
   `manifest.json` provenance sidecar is written beside the files when the set is complete. The
   download installs under the first root below; a folder chosen with **Choose Files…** is never a
   download target.
2. **By hand.** Put `model.safetensors`, `config.json` and `tokenizer.json` in a `whisper-tiny`
   folder under one of the roots below, then press **Check Again**, which re-checks the folder in
   use. Or point the app at a folder elsewhere with **Choose Files…** (pick any of the three),
   which checks that folder immediately. A folder chosen this way is used for the current session
   only — the dialog labels it so — and after a restart discovery searches the roots again, so
   copy the three files into one of them to keep the model installed.

The model directory is resolved first hit wins, the same shape as the other Craft apps:

```text
$DECKCRAFT_MODELS/whisper-tiny/                     (the environment variable names the root)
%APPDATA%/DeckCraft/models/whisper-tiny             (Windows)
$XDG_CONFIG_HOME/deckcraft/models/whisper-tiny      (Linux, or the next line when it is unset)
~/.config/deckcraft/models/whisper-tiny             (Linux, XDG_CONFIG_HOME unset)
~/Library/Application Support/DeckCraft/models/whisper-tiny   (macOS)
models/whisper-tiny/                                (beside the executable)
models/whisper-tiny/                                (in the source tree, for development)
```

A missing or incomplete set disables the ribbon entry with an explanation; the show itself is
never blocked and never panics. Discovery checks presence first (cheap enough for a frame), then
hashes every file on a worker thread; the entry is enabled only when that check reports `ready`,
and the loader verifies size and SHA-256 again before it reads anything. The dialog shows
"Checking the files…" while the hash runs and marks a corrupt set as corrupt, with Download
available. The app makes no network call at start-up, on file open or when a pane opens.

## Settings

Settings are per user (`prefs.json`, `Prefs::subtitles`), not part of the presentation:

| Setting | Values |
|---|---|
| Always use subtitles | off by default |
| Spoken language | detect, or one of 17 named languages in the dialog; `show.subtitles` accepts any of Whisper's 99 language tokens |
| Subtitle language | the caption's language (translation is not implemented yet) |
| Microphone | system default, or a named input device |
| Position | bottom of slide, top of slide, below slide, above slide |
| Text size | 8–96 pt (default 24) |
| Maximum lines | 1–8 (default 3) |
| Processor | CPU (default), or a requested accelerator; an unavailable one falls back to the CPU with a note |

A deck carried to another machine keeps that machine's microphone, language and position, as
PowerPoint's per-user show settings do. The caption text is not stored anywhere and disappears
when the show ends.

## Commands

Both are ordinary engine commands, so the ribbon, the dialog, `deckcraft-cli run`, the JSON control
channel and MCP all use them:

- `show.subtitles {on?, spoken?, subtitle?, position?, microphone?, fontSizePt?, maxLines?,
  accelerator?}` → `{settings, provider}`. Changes only the fields it names; a supplied field of
  the wrong type is a `bad` error, an empty `microphone` is refused (`null` selects the system
  default), and a language tag outside Whisper's list is refused.
- `show.subtitles.state {}` → `{settings, provider: {state, detail?, model?, language?,
  microphone?, accelerator?, capabilities?}}`. Takes no parameters (a non-empty object is an
  error). `provider` is host-reported: `idle`, `starting`, `running`, `failed` or `stopped`.

## Processors and the accelerator seam

The default backend is pure Rust on the CPU and runs on every supported native platform (the
browser build ships no provider); no accelerator, driver or C/C++ runtime is required or fetched.

| Platform | Usable today | Optional later |
|---|---|---|
| Windows x64 | CPU | OpenVINO (Intel NPU/GPU) |
| Linux x64 (and BSDs) | CPU | OpenVINO on Intel Linux |
| macOS (Intel and Apple silicon) | CPU | CPU (no backend planned) |
| wasm (browser) | off: no provider ships | — |

The seam is `deckcraft_asr::accelerator`: an `Accelerator` enum (`cpu`, `openVino`),
`resolve(requested, detected)` returning `Capabilities { available, detected, chosen, requested,
note }`, and the manifest's `runtime` hint (`candle-cpu`). Settings carry the request
(`accelerator`), the provider reports the result in
`ProviderStatus::{accelerator, capabilities}`, and `show.subtitles.state` exposes both. Requesting
`openVino` on a build without that backend keeps running on the CPU and reports
`"runs on CPU; OpenVINO NPU/GPU not available"`; a detected OpenVINO installation is listed under
`detected` but not under `available`. A future OpenVINO crate replaces the engine selection in
`WhisperTranscriber::start` and nothing else; the CPU path stays the default and the fallback.

Detection reads `INTEL_OPENVINO_DIR`, `OPENVINO_DEVICE` or `DECKCRAFT_OPENVINO`; it never loads a
library, so a machine with OpenVINO installed but no backend behaves exactly like one without.

## How it behaves

- `crates/asr-candle` owns a worker thread: it verifies every model file against the manifest
  (size and SHA-256) before reading it, runs one silent warm-up decode, opens the microphone
  (cpal), and keeps the last 30 seconds of 16 kHz mono audio (its own downmix and resampler). It
  encodes that window and decodes greedily with no timestamps, but never starts a decode sooner
  than 2 s after the previous one finished: a decode takes 1.2–2.7 s here, so a shorter floor
  would build a backlog and saturate the CPU. A partial appears when a decode finishes (2–3 s in
  the quiet measurement below; the capture queue holds up to 60 s at 48 kHz stereo and drops the
  oldest audio when a longer decode outpaces it).
  A line becomes final after ~0.9 s without speech; the window then starts fresh, and while no
  speech has been heard since the last final line the worker keeps only the newest second instead
  of re-decoding the same audio. When a transcript is longer than the line cap, the caption shows
  its newest words — the beginning scrolls off — so the audience never loses what is being said.
- The UI thread does not wait for inference while captions run: `crates/ui-egui/src/subtitles.rs`
  polls the channel once a frame, the show draws the caption after the slide (above its media
  controls, below the pen and the bars), and asks egui for a repaint every 120 ms while the stream
  is running or a download is in progress. Stopping does not wait either: `stop` sets the worker's
  flag and hands the join to a short-lived reaper thread, so ending the show or turning subtitles
  off never blocks the UI thread for the length of the decode already in flight (2–3 s on an idle
  machine, up to 27 s on a loaded one, per the measurements below). A worker that is still
  starting stops between its stages — after the load, after the warm-up, or before the microphone
  opens.
- Captions survive slide changes; they are covered by a blank screen and by See All Slides; they
  end with the show. Ending the show, turning subtitles off, or losing the microphone stops the
  provider and releases the model and the device; the reported status becomes `stopped` (a
  `failed` status stays visible).
- A provider that fails after starting (the microphone is unplugged, the model file is damaged)
  reports once in the status bar and is not restarted by itself. To retry, turn subtitles off and
  on again, change the microphone or the spoken language, or start another show; the caption's
  position, text size, line count and subtitle language do not restart the stream. A start that
  fails behaves the same way: it is reported once per show, not once per frame. The show keeps
  running. A completed download, or a background hash check that reports `ready`, clears the
  failure so the same show retries.
- The browser build has no provider (no microphone capture, no 153 MB model): the settings and the
  caption overlay compile, and the ribbon entry is disabled with an explanation.

## Measured behaviour

Measured on 2026-11-07, Windows x64, CPU only, release build, model `openai/whisper-tiny` at
revision `169d4a4`, model files on local disk, with `DECKCRAFT_WHISPER_MODEL` and
`DECKCRAFT_WHISPER_WAV` set:

- Load: **0.55 s** warm (1.37 s the first time after the download; the set is 153.5 MB).
- Transcript of the 11 s recording, 30 s window (the whole utterance fits): **"And so my fellow
  Americans ask not what your country can do for you ask what you can do for your country."**
  Punctuation-free besides the words themselves, exactly as the sentence was spoken.
- Per-decode latency by window length, after a warm-up decode: 2 s tail **1.24 s**, 4 s **1.46 s**,
  8 s **1.97 s**, 16 s **2.08 s**, 30 s **2.74 s**. The same test repeated while the machine was
  busy elsewhere varied between 4 s and 27 s per decode, so treat these as an idle-machine
  figure, not a guarantee.
- The provider's warm-up (a one-second silent decode) keeps the first-use allocation cost out of
  the first caption; it took 3.1 s when the machine was loaded.

The recording is `samples_jfk.wav` from <https://huggingface.co/datasets/Narsil/candle-examples>
(352,078 bytes, 16 kHz mono 16-bit PCM, 11 s, a public-domain US government recording). The test
that produced this is `crates/asr-candle/tests/whisper_real_audio.rs`, ignored by default:

```text
DECKCRAFT_WHISPER_MODEL=<model dir> DECKCRAFT_WHISPER_WAV=<jfk.wav> \
cargo test -p deckcraft-asr-candle --test whisper_real_audio --release -- --ignored --nocapture
```

Not measured: word error rate over a corpus (the check above is one sentence, not a WER
measurement), latency on other machines or with other microphones, non-English accuracy, and how
quickly the sliding window settles on a long talk. Whisper tiny is the smallest model; expect
noticeably lower accuracy than a cloud service.

## Crates

- `crates/asr` (L2, `wasm32`-checked): provider protocol and fake provider, audio downmix and
  resampling, caption geometry, settings, and the model manifest with SHA-256 verification and
  directory discovery. No platform dependency.
- `crates/asr-candle` (L6, native only): candle Whisper, the mel filterbank, greedy decoding,
  cpal capture, and the worker that turns audio into partial and final lines.
- `crates/ui-egui` (L6): the settings dialog, ribbon group, model download/import UI, and the
  caption overlay on the show's frame.
- `crates/engine` (L5): `show.subtitles` and `show.subtitles.state`; `Prefs::subtitles`;
  `Session::subtitle_status`.
- `apps/deckcraft`: the HTTPS download hook (`ureq`).
