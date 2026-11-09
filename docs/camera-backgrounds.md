# Camera backgrounds

> **Last reviewed:** 2026-11-07 · **Last updated:** 2026-11-07 · **Change:** minor (Camera Format backgrounds: Normal/Remove/Blur/Replace persist in `.deckcraft` and PPTX `alf:backgroundProps`, with a pure pipeline and an RTen BiRefNet Lite 512 backend; live capture waits on #1837) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

PowerPoint's Camera Format tab gives a Cameo four backgrounds: **Normal**, **Removed**, **Blur**
and **Custom** (a replacement picture). A Cameo is an ordinary picture with the marker
`alf:liveFeedProps` in `p:cNvPicPr/a:extLst`; the mode is a child of its `alf:backgroundProps`.

## What is wired now

| Piece | State |
|---|---|
| Model and mode saved in `.deckcraft` | yes: `CameraBackground` on `Shape.cameo`, four modes, undoable |
| PPTX import/export of `alf:backgroundProps` | yes: Normal/Removed/Blur/Custom; unknown children kept through `Cameo.raw` **except** children that name a package relationship (`r:embed`/`r:link`/`r:id`) — a feed that does is replaced by one carrying only the mode, because export creates no such relationship and PowerPoint would see a dangling reference |
| Camera Format ▸ Camera Background UI | yes: four on-state buttons and a model panel; disabled with the reason when the model is missing |
| Engine commands | `cameo.background`, `cameo.background.state`, `cameo.backgroundApplyToAll`, `cameo.backgroundPreview` |
| Offscreen pipeline proof | `cameo.backgroundPreview` (base64 `frame` in, base64 PNG out) and `deckcraft-cli camera-background` |
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
would stall the caller — and `deckcraft_matte::manifest` verifies where the bytes matter:
`RtenSegmenter::load` reads the file **once**, hashes those bytes and parses the same buffer, so a
file swapped between a hash and a reopen cannot be run; `install_from` opens the picked file once
and copies it, and the copy is verified and renamed into place. Temporary files are created with
`create_new` under a per-run random name (never the fixed `model.onnx.part`), and a symbolic link at
the source, the target or the temporary name is refused. Every failure removes its temporary file.
The download has a 30-minute total budget (about 106 KB/s minimum) on top of the per-read stall
detector, and the panel's Downloading state offers **Cancel**.

### Install paths

`<model root>` is the first hit of:

1. `$DECKCRAFT_MODELS`
2. the app's config directory `models/` (`%APPDATA%\DeckCraft\models`,
   `~/Library/Application Support/DeckCraft/models`, `$XDG_CONFIG_HOME/deckcraft/models`)
3. `models/` beside the executable
4. `models/` in the working directory (source-tree convenience)

The file is `<model root>/birefnet-lite-512/onnx/model.onnx`; a provenance `manifest.json` sits
beside the `onnx/` directory.

### Configuration

| Variable | Default | Effect |
|---|---|---|
| `DECKCRAFT_MODELS` | unset | A model root tried first; when unset the config directory, the executable's `models/` and the working directory's `models/` are tried in that order |
| `DECKCRAFT_NO_MATTE` | unset | Set at all (any value, including empty) and the desktop app installs no native matte service or loader, so the Camera Background panel runs with no model |
| `DECKCRAFT_MATTE_THREADS` | unset or `default` | RTen sizes its own CPU pool; a number (1..=64, clamped) builds an explicit pool, which was measured slower (16.7 s vs 3.96 s per frame) and exists only for constrained machines and benchmarks |

The download URL is fixed in the manifest (`MODEL_REVISION`); nothing is fetched at startup, and
the only network request is the one the user consents to.

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
the last matte until a new one arrives. The model load (verify 191 MB, parse the graph) runs on
that thread too: the first `process` after a model appears answers `None` until the first matte
exists, so the UI keeps drawing. `Timings` reports `last_ms`, `runs` and `reused` frames; the panel
shows them. The default minimum interval is 2 s. RTen sizes its own CPU pool to the machine's
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
| wasm (`deckcraft-web`) | no inference: Normal still works and is enabled; Removed, Blur and Custom are disabled with the message | none |

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
`cameo.backgroundPreview`. The preview's inputs are named for their roles — `frame` (the camera
image) and `background` (the replacement picture), both base64 — because `cameo.background` uses
`image` for a media id and `imageData` for inline bytes. The preview decodes any format the `image`
crate accepts, not only PNG: JPEG works too. `cameo.background` and `cameo.backgroundApplyToAll`
return `{mode, ids, changed}` in every case; `all` together with `id`/`ids` is refused rather than
silently preferred, and slide indices are 0-based.

The preview is synchronous (about 2 s of CPU for a segmentation mode) and bounded: a frame or
replacement over 1280 px on a side, over 64 MB of base64, or a zero side is refused from the header
before the decoder allocates it. The matte of an unchanged frame is cached per session, so a preview
loop with the same camera frame segments once. The desktop UI does not use the preview command for
live frames: it calls `MatteService::process`, which composites on the caller with the worker's last
matte (see below).

## Limits

- The preview accepts a frame or replacement up to 1280 px on a side (1 MP) and 64 MB of base64;
  a zero-sized or larger one is refused with the limit named. `MAX_DIM`/`MAX_PIXELS` (4096 px,
  16.7 MP) still bound the pure pipeline for a host that hands it a bigger frame.
- Inline replacement pictures (`imageData`) are limited to 16 MB decoded and to a 2 GB total media
  budget per presentation; the header's dimensions are checked before the bytes are stored.
- `feather` and `blur` are numbers in 0..=64 and are clamped into that range; a present non-number
  or non-finite value is refused, not replaced by the default. `feather` defaults to 2. `blur`
  defaults to 0, which picks 1/24 of the shorter side (clamped to 2..=24); `blur: 1` or more is
  used as given. On the CLI, `--blur 0` means that default radius, not "no blur".
- The model download has a 30-minute total budget and a 30 s per-read stall detector; a cancel or a
  timeout removes the partial file and the panel can retry or import.
- OOXML's livefeed schema gives `Custom` only an `extLst`: it has no colour and no replacement-image
  relationship. PowerPoint therefore keeps the Custom mode but cannot carry DeckCraft's replacement
  picture or colour. Both stay in the `.deckcraft` file, and a PPTX round trip keeps the mode.
- Live capture (#1837) is not in this PR. The compositing seam (`MatteService::process`) and the
  offscreen preview are, so the capture work only has to feed frames into it.
- The model is 192 MB and is not in the repository or release packages.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-09 | minor | Bounded preview and inline pictures, verified single-read model load, cancellable downloadable model |
| 2026-11-07 | minor | New: four background modes with structured `.deckcraft` and PPTX `alf:backgroundProps` round trip; pure `matte` pipeline and compositing; RTen `birefnet-lite-512` backend at 2.10 s/frame with cached mattes; offscreen preview command and CLI; accelerator seam (CPU today) |
