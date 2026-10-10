# Hardware parity with PowerPoint

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created from the renderer, media and windowing code) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365); PowerPoint for Windows where noted

What hardware PowerPoint uses and what DeckCraft uses, per platform. Dimension score: **~40%
ready** (estimated), **25–45 h** to parity. Performance (unmeasured, ~55%) is tracked here too
because it mostly follows from hardware use.

## Graphics and rendering

| Feature | PowerPoint | DeckCraft macOS | DeckCraft Windows | DeckCraft Linux/BSD | DeckCraft web |
|---|---|---|---|---|---|
| Window compositing on the GPU | Metal / DirectX | wgpu (Metal) | wgpu, DX12 only by default (#16) | wgpu (Vulkan, GL) | WebGPU, WebGL2 fallback |
| Slide rasterization | GPU | **CPU** (`vello_cpu`) | CPU | CPU | CPU (wasm) |
| Transitions and Morph frames | GPU | CPU-rendered slide images composited as textured quads on the GPU | same | same | same |
| High-DPI / Retina | yes | yes | yes | yes | yes |
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
| Pinch to zoom, two-finger gestures | yes | no |
| Pen pressure and tilt for ink (Apple Pencil via Sidecar, Windows Ink) | yes | no: ink strokes have constant width |
| Touch screens (Windows) | yes | basic pointer only |
| IME input methods for CJK text | yes | no (see [localization-parity.md](localization-parity.md)) |
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
| Pen pressure, pinch zoom | 3–6 |
| Benchmark suite (`cargo xtask bench`) and a first performance pass | 6–12 (Performance dimension) |
| **Hardware total** | **~26–45** |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created: rendering, media, displays, input and performance against PowerPoint 16.113 |
