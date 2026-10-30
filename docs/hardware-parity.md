# Hardware parity with PowerPoint

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-28 · **Change:** minor (decoded YUV planes and queued RGBA frames share checked video memory limits) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365); PowerPoint for Windows where noted

What hardware PowerPoint uses and what DeckCraft uses, per platform. Dimension score: **~40%
ready** (estimated), **25–45 h** to parity. Performance (unmeasured, ~55%) is tracked here too
because it mostly follows from hardware use.

## Graphics and rendering

| Feature | PowerPoint | DeckCraft macOS | DeckCraft Windows | DeckCraft Linux/BSD | DeckCraft web |
|---|---|---|---|---|---|
| Window compositing on the GPU | Metal / DirectX | wgpu (Metal) | wgpu, DX12 only by default (#16) | wgpu (Vulkan, GL) | WebGPU, WebGL2 fallback |
| Slide rasterization | GPU | **CPU** (`vello_cpu`) | CPU | CPU | CPU (wasm) |
| Transitions and Morph frames | GPU | CPU-rendered slide images composited as textured quads on the GPU | same | same | same |
| Picture export resolution | 2 px per point, tagged so the file is the slide's size | PNG `pHYs` and JPEG density, 144 dpi at the default 2 px per point | same | same | same |
| High-DPI / Retina | yes | yes; editor slide rasters sit on the physical pixel grid (#184) | yes | yes | yes |
| Wide gamut / HDR display | partly (P3 images) | no (sRGB) | no | no | no |
| Startup on all GPUs | yes | yes | **no**: crashes in some Intel/AMD drivers (#40, #59) | not reported | n/a |
| Software fallback when no GPU | yes (WARP on Windows) | no | no | no | no |

## Media

| Feature | PowerPoint | DeckCraft |
|---|---|---|
| Video decode | Hardware (AVFoundation/VideoToolbox, Media Foundation) | Software only: pure-Rust H.264, HEVC, VP9, AV1 (`crates/h264`, `hevc`, `vp9`, `av1`) |
| 4K video in the show | smooth on hardware decode | unmeasured; likely CPU-bound |
| Audio output | Core Audio / WASAPI | `cpal` (Core Audio, WASAPI, ALSA); browser Web Audio |
| Microphone (record audio, narration) | yes | no |
| Camera (Cameo, record with camera) | yes | no |
| Screen recording | yes | no |

The software decoder reserves checked `width × height × 4` RGBA storage for its bounded feed and
ready frames from the same per-decoder budget used for codec planes. It rejects a frame whose RGBA
allocation would exceed that reservation before conversion.

## Displays

| Feature | PowerPoint | DeckCraft |
|---|---|---|
| Detect the projector, show on it, presenter view on the laptop | yes | **no**: the show goes full screen on the current display; presenter view opens as a normal window (egui viewport) |
| Swap displays, choose the monitor in Set Up Show | yes | no |
| Mirroring-aware behaviour | yes | no |

## Input devices

| Feature | PowerPoint | DeckCraft |
|---|---|---|
| Keyboard and presentation clickers (Page Up/Down, B, Esc) | yes | yes |
| Mouse and trackpad scroll | yes | yes |
| Pinch to zoom, two-finger gestures | yes | partial: pinch zoom does not keep the point under the pointer; a two-finger twist rotates the selected shapes |
| Two-finger swipe between slides | yes | yes: when the slide fits the window, and during the show. A zoomed slide still pans |
| Pen pressure and tilt for ink (Apple Pencil via Sidecar, Windows Ink) | yes | pressure scales the stroke when the pointer reports it (a mouse or a missing reading draws at the pen width); tilt is not read |
| Touch screens (Windows) | yes | basic pointer only |
| IME input methods for CJK text | yes | yes: composed in place, underlined, candidate window at the caret (#101) |
| Dictation | yes (macOS) | no |

## Performance

Nothing is measured against PowerPoint yet: no benchmark of open time, render time per slide,
show frame rate or memory exists in `xtask`. Known: slides rasterize on the CPU, Morph aligns
outlines every frame (#67, fix open), and the decoders run in software.

## Estimates

| Work | Hours |
|---|---|
| Presenter view on the right display, choose monitor, swap | 3–5 |
| GPU slide rasterization (vello's GPU/hybrid path) | 12–20 |
| Hardware video decode (copy FilmCraft's VideoToolbox / Media Foundation / VA-API paths) | 8–14 |
| Windows GPU startup robustness: backend probing, software fallback (#40, #59) | 4–8 (counted under Stability) |
| Pen tilt, pinch zoom, trackpad gestures | 2–5 |
| Benchmark suite (`cargo xtask bench`) and a first performance pass | 6–12 (Performance dimension) |
| **Hardware total** | **~25–44** |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-28 | minor | Codec plane and queued RGBA frame storage share checked video memory budgets; arithmetic boundary and codec playback tests pass |
| 2026-10-14 | minor | PNG and JPEG slide export record 144 dpi at the default 2 pixels per point, so a Retina picture opens at the slide's size |
| 2026-10-13 | trivial | Editor slide rasters sit on the physical pixel grid (#184) |
| 2026-10-14 | trivial | Community PR wave folded: one row each for pinch/two-finger gestures and pen pressure |
| 2026-10-14 | minor | A two-finger twist on the trackpad rotates the selected shapes, as one undo step. Pinch-zoom is unchanged |
| 2026-10-14 | minor | A sideways trackpad swipe turns the page in Normal view (when the slide fits) and in the show. Pinch-zoom is unchanged |
| 2026-10-14 | minor | Pen pressure scales ink width; tilt, pinch zoom and trackpad gestures remain (estimate 3–6 h → 2–5 h) |
| 2026-10-12 | minor | IME input methods (#101) |
| 2026-10-10 | major | Created: rendering, media, displays, input and performance against PowerPoint 16.113 |
