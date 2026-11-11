# Decisions: live subtitles

> **Last reviewed:** 2026-11-07 · **Last updated:** 2026-11-07 · **Change:** created

The choices behind Slide Show ▸ Captions & Subtitles (#1811), recorded so a later change can see
what was weighed. The feature and its numbers are in [captions.md](captions.md); the licence
policy the whole batch shares is [AGENTS.md](../AGENTS.md#licence-allowlist), and the model's own
provenance is recorded in [NOTICE](../NOTICE) and in `model::whisper_tiny`.

## The engine: Whisper tiny through candle

**Chosen: `openai/whisper-tiny` run by `candle-transformers` on the CPU.**

- Offline and permissive. Whisper's weights are Apache-2.0 (the model card at the pinned
  revision); candle is `MIT OR Apache-2.0`; tokenizers and
  cpal are Apache-2.0. All are inside the [shared licence allowlist](../AGENTS.md#licence-allowlist). No cloud service, no account,
  no network call during inference.
- Pure Rust, no `unsafe`. A binding to whisper.cpp would drag C/C++ and an FFI boundary into a
  workspace that forbids `unsafe`; candle is Rust and the workspace lints hold.
- Small enough to download on a laptop: 153.5 MB, 151 MB of it weights. `whisper-large-v3-turbo`,
  parakeet and canary are 0.6–1.6 GB and either need a Python/CUDA runtime or have no Rust
  implementation in this workspace.
- Multilingual (99 language tokens) with Whisper's own language detection, so the spoken-language
  setting can detect or pin a language.

The trade-off is accuracy: tiny has the weakest WER of the Whisper sizes. It is the only one that
fits the download-and-run-on-CPU budget agreed for this batch; the manifest is data, so a larger
`whisper-*` entry can be added later without touching the protocol.

`candle-transformers` 0.11 exposes the model (`Whisper::load`, encoder/decoder) but not a decode
loop, so `crates/asr-candle` implements greedy decoding with no timestamps itself, and builds the
Whisper mel filterbank (Slaney scale, librosa `norm="slaney"`) rather than shipping it as an asset.
It was verified against a real recording, not only by loading — see [captions.md](captions.md) for
the transcript.

## Model files: never committed, never shipped

Following the shared policy:

- The repository has a **manifest** ([`deckcraft_asr::model::whisper_tiny`](../crates/asr/src/model.rs)):
  id, repository, exact revision, each file's name, byte size, SHA-256, download URL, and the
  model card's licence (Apache-2.0).
- The user downloads the files through Subtitle Settings (a consent view showing source, size,
  licence and hashes, then HTTPS through the desktop host's `ureq` hook) or places them by hand in
  the documented directory. Both paths are supported; the dialog shows the folder either way.
- Files are written beside their final name, checked against the size and SHA-256, and only then
  renamed, so an interrupted download never looks installed. A missing or incomplete model
  disables the feature with an explanation and never blocks the show; a hand-placed file whose
  contents are wrong is caught by the full check on download and on **Choose Files…**, not by the
  presence check that discovery and show start use.
- No network at start-up, on file open, on pane open, or during inference.

Model directories are resolved first hit wins: `$DECKCRAFT_MODELS`, the platform config directory
plus `models`, `models/` beside the executable, then `models/` in the source tree.

## CPU first, with an accelerator seam

The default backend is pure Rust on the CPU, on Windows, macOS, Linux and the BSDs, and is out of
the wasm build. No accelerator is required and no C/C++ runtime is added. Rather than hard-code
that, the feature asks `deckcraft_asr::accelerator::resolve(requested, detected)` for what to run
and reports the answer in the provider status: `cpu` today; a detected-but-unbuildable accelerator
stays in `detected`, the chosen one is the CPU, and the note says why in one sentence. The model
manifest records its runtime (`candle-cpu`). An optional OpenVINO backend for Intel x86 (Windows,
Linux) can then land as its own crate that selects a different engine in
`WhisperTranscriber::start`; the CPU path and every caller stay as they are.

## The provider lives in the UI host

The microphone and the model belong to the machine, not the document, so the native backend lives
in `deckcraft-asr-candle` (L6) and is driven by `deckcraft-ui-egui`; the engine only records the
host-reported `Session::subtitle_status`. Consequences:

- One implementation for macOS, Windows, Linux and the BSDs (cpal, candle); the browser builds the
  settings and the overlay but no provider, so the ribbon entry is disabled there with an
  explanation.
- The UI thread does not wait for inference while captions run: the provider runs on its own
  thread and the overlay polls a channel once per frame. Starting a show does not stall the first
  frame beyond building the provider; stopping joins the worker so no thread or microphone is left
  behind, which means a stop can wait for the decode already in flight.
- `show.subtitles` and `show.subtitles.state` are ordinary engine commands, so the control channel,
  the CLI and MCP drive the same settings as the ribbon and the dialog.

## Settings are per user, not per deck

`Prefs::subtitles` (in the app's `prefs.json`) holds the on switch, spoken and subtitle language,
microphone, position, text size, maximum lines and the processor (accelerator). Reasons:

- They describe the room and the machine: which microphone is plugged in, which language the
  presenter speaks, where the caption fits on this projector. A deck shown on another computer
  should use that computer's microphone and language, not the author's.
- PPTX has no element for live captions; writing them into the presentation would be a private
  extension PowerPoint would drop or repair, and `Preload`/`show` settings that are per-user
  already follow this rule (Show Media Controls, Keep mouse cursor visible).
- The engine's `Prefs` is already the per-user settings channel the desktop persists at exit, so
  the control channel and MCP see and change the same values.

The caption text itself is not stored anywhere: it is this show's caption and disappears when the
show ends.
