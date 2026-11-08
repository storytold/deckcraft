# Camera backgrounds

> **Last reviewed:** 2026-11-07 · **Last updated:** 2026-11-07 · **Change:** minor (Camera Format backgrounds: Normal/Remove/Blur/Replace persist in `.deckcraft` and PPTX `alf:backgroundProps`, with a pure pipeline and an RTen BiRefNet Lite 512 backend; live capture waits on #1837) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

PowerPoint's Camera Format tab gives a Cameo four backgrounds: **Normal**, **Removed**, **Blur**
and **Custom** (a replacement picture). A Cameo is an ordinary picture with the marker
`alf:liveFeedProps` in `p:cNvPicPr/a:extLst`; the mode is a child of its `alf:backgroundProps`.

## What is wired now

| Piece | State |
|---|---|
| Model and mode saved in `.deckcraft` | yes: `CameraBackground` on `Shape.cameo`, four modes, undoable |
| PPTX import/export of `alf:backgroundProps` | yes: Normal/Removed/Blur/Custom; unknown children kept through `Cameo.raw` |
| Camera Format ▸ Camera Background UI | yes: four on-state buttons and a model panel; disabled with the reason when the model is missing |
| Engine commands | `cameo.background`, `cameo.background.state`, `cameo.backgroundApplyToAll`, `cameo.backgroundPreview` |
| Offscreen pipeline proof | `cameo.backgroundPreview` (base64 PNG in/out) and `deckcraft-cli camera-background` |
| Native inference | `crates/matte-rten`: RTen 0.27 on a worker thread, CPU only |
| Live camera capture and the live composited preview | **not yet**: issue #1837 owns capture. `Services.matte` exposes `MatteService::process` for the seam; the UI says so |

The background mode is document state and never needs the model: `cameo.background` stores
Normal, Removed, Blur or Custom (a PPTX can import Blur or Removed) with or without one.
Segmentation is not document state: the host installs a `deckcraft_matte::Segmenter` (CLI, tests)
or points the session at a loader for the installed model (desktop, MCP). Without a model,
`cameo.backgroundPreview` and the Camera Format controls refuse a non-Normal mode with an
actionable message, never a panic and never a partial write.

## Model manifest

The weights are **never committed**. The repository carries only this manifest; the user installs
them through the panel (consented HTTPS download) or places the file by hand.

| Field | Value |
|---|---|
| id | `birefnet-lite-512` |
| repository | `studioludens/birefnet-lite-512` |
| revision | `962e67969ec6acec9c19fab76acee4f5e22e990c` |
| file | `onnx/model.onnx` |
| bytes | 191,877,254 |
| SHA-256 | `1cb0fb360dadd15af77c639085d77a9df67db0c64315560c3de005f676345ac2` |
| licence | MIT (the repository's LICENSE) |
| input | `input_image`, `[1, 3, 512, 512]` f32, RGB /255, ImageNet mean `[0.485, 0.456, 0.406]`, std `[0.229, 0.224, 0.225]`, NCHW |
| output | `output_image`, `[1, 1, 512, 512]` logits -> sigmoid -> 8-bit alpha |

The download URL is derived from the repository and revision:
`https://huggingface.co/studioludens/birefnet-lite-512/resolve/962e67969ec6acec9c19fab76acee4f5e22e990c/onnx/model.onnx`.

The file was re-downloaded and measured on 2026-11-07; its SHA-256 and size match the manifest.
Discovery (`probe`) checks presence and size only — hashing 191 MB at startup or on a status query
would stall the caller — and `deckcraft_matte::manifest::verify_file` runs where the bytes matter:
`RtenSegmenter::load` hashes again before it parses the model, and `install_from`/`download_to`
verify before renaming `model.onnx.part` into place (and remove the part file on every failure).

### Install paths

`<model root>` is the first hit of:

1. `$DECKCRAFT_MODELS`
2. the app's config directory `models/` (`%APPDATA%\DeckCraft\models`,
   `~/Library/Application Support/DeckCraft/models`, `$XDG_CONFIG_HOME/deckcraft/models`)
3. `models/` beside the executable
4. `models/` in the working directory (source-tree convenience)

The file is `<model root>/birefnet-lite-512/onnx/model.onnx`; a provenance `manifest.json` sits
beside the `onnx/` directory. `DECKCRAFT_NO_MATTE` keeps the desktop service out of a session.

## Measured speed and cadence

Measured on this Windows x64 machine, release build, CPU only, 2026-11-07:

| Backend / model | Input | Time per frame | Result |
|---|---|---|---|
| **RTen 0.27 / `birefnet-lite-512`**, RTen's default CPU pool | 512×512 | **2.10 s** inference (maintainer's measurement); **3.96 s** end to end with 640×480 alpha and feather | chosen: output matches onnxruntime exactly (correlation 1.000000, max abs diff 4.6e-5) |
| RTen with a hand-made rayon pool (21 threads / 8 threads) | 512×512 | 16.7 s / 18.2 s | rejected: constructing a pool for RTen is much slower than letting it size its own; only `DECKCRAFT_MATTE_THREADS=N` forces one |
| onnxruntime | 512×512 | 1.43 s | reference only; a C/C++ runtime would be a new dependency |
| tract-onnx 0.23 / `birefnet-lite-512` | 512×512 | 7.8 s | rejected: 3.7× slower than RTen |
| tract-onnx 0.23 / `BiRefNet_lite` (onnx-community) | 1024×1024 | 33.7 s | rejected: unusable live, and 224 MB |

Two seconds per frame is far below a camera's frame rate, so inference never runs on the UI thread:
`MatteWorker` owns one thread, keeps at most one pending frame and offers a new one only when nothing
is waiting and the interval has elapsed (a queued frame is not replaced), and the compositor reuses
the last matte until a new one arrives. `Timings` reports `last_ms`, `runs` and `reused` frames; the
panel shows them. The default minimum interval is 2 s. RTen sizes its own CPU pool to the machine's
physical cores; `DECKCRAFT_MATTE_THREADS=N` caps it for a constrained machine (a hand-made pool was
measured slower, so it is not the default).

The real-model test is ignored by default:

```sh
DECKCRAFT_BIREFNET_MODEL=/path/to/model.onnx cargo test -p deckcraft-matte-rten --test real_model -- --ignored --nocapture
```

## Accelerators

| Platform | Default | Optional |
|---|---|---|
| Windows x64 | CPU (RTen) | future Intel OpenVINO backend, CPU fallback |
| Linux x86_64 | CPU (RTen) | future Intel OpenVINO backend, CPU fallback |
| macOS (Intel and Apple silicon) | CPU (RTen) | CPU for now |
| FreeBSD and other BSDs | CPU (RTen) | CPU for now |
| wasm (`deckcraft-web`) | no inference; controls disabled with the message | none |

The manifest records `runtime = "rten"`, `default_accelerator = cpu` and `accelerators = [cpu]`.
`cameo.background.state` reports `accelerator` and `accelerators`, and
`cameo.backgroundPreview` accepts `accelerator`. A requested accelerator this build does not have is
refused with `runs on CPU; OpenVINO NPU/GPU not available`; the feature behaves identically on CPU.
No C/C++ runtime is added by this PR, and DirectML is not used.

## Using it

```sh
# Save a mode on every Cameo (undoable; `all` needs no selection).
deckcraft-cli run --in talk.deckcraft \
  --cmd 'cameo.background={"mode":"blur","all":true}' --save talk.deckcraft

# Or name the shapes: a scripted run has no selection, so `ids` (or `id`) does the targeting.
deckcraft-cli run --in talk.deckcraft \
  --cmd 'cameo.background={"mode":"removed","ids":[7]}' --save talk.deckcraft

# Prove the pipeline offscreen with the installed model.
deckcraft-cli camera-background --image frame.png --mode removed --out cutout.png
deckcraft-cli camera-background --image frame.png --mode custom --background studio.png --out composed.png
```

Over the control channel and MCP, every command is the same:
`cameo.background`, `cameo.background.state`, `cameo.backgroundApplyToAll`,
`cameo.backgroundPreview`. `cameo.backgroundPreview` takes base64 PNGs and refuses an image whose
header is larger than 4096×4096 before decoding it.

## Limits

- OOXML's livefeed schema gives `Custom` only an `extLst`: it has no colour and no replacement-image
  relationship. PowerPoint therefore keeps the Custom mode but cannot carry DeckCraft's replacement
  picture or colour. Both stay in the `.deckcraft` file, and a PPTX round trip keeps the mode.
- Live capture (#1837) is not in this PR. The compositing seam (`MatteService::process`) and the
  offscreen preview are, so the capture work only has to feed frames into it.
- The model is 192 MB and is not in the repository or release packages.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-07 | minor | New: four background modes with structured `.deckcraft` and PPTX `alf:backgroundProps` round trip; pure `matte` pipeline and compositing; RTen `birefnet-lite-512` backend at 2.10 s/frame with cached mattes; offscreen preview command and CLI; accelerator seam (CPU today) |
