# Slide show frame performance

> **Last updated:** 2026-10-25 · **Change:** created (before/after the slide-show layers and
> WebAssembly SIMD)

What one slide-show frame costs while something plays, before and after the show started
compositing animated slides from layers rendered once (`crates/ui-egui/src/show_layers.rs`) and the
web build started using WebAssembly SIMD.

## How it was measured

- **What is timed:** the app's whole frame (`SlideApp::logic` and `ui`, slide rendering, egui
  tessellation), driven headlessly at 60 Hz in a 1920×1080 window (the slide drawn at 2 px per
  point). The GPU upload itself isn't timed; the texture bytes each frame sends are reported.
- **Which frames:** only frames where something plays: an animation step on the scenario's slide
  (one click after the other, to the last step), the 1 s or 2 s of a transition, or 3 s of GIFs.
- **Native:** `cargo test --release -p deckcraft-ui-egui --lib show_frame_bench -- --ignored --nocapture`,
  release build, two render threads.
- **Browser:** the same scenarios and driver built for `wasm32-unknown-unknown` (release, with and
  without `+simd128`), bound with wasm-bindgen and run in Node 22, whose V8 is Chrome's WebAssembly
  engine; single-threaded, as in the browser.
- **Machine:** a 2-vCPU x86_64 cloud container (7 GB). A typical laptop is faster; compare the
  columns with each other, not with another machine.
- **Before:** upstream `main` at 38d2ade. **After:** this change.

| Scenario | What plays |
|---|---|
| agenda | Sample deck, slide 2: six bullets fading in by paragraph (four clicks) |
| process | Sample deck, slide 4: four shadowed shapes zooming in one after another |
| photo | A full-slide photo, a title and five shapes flying, spinning, wiping, growing and following a path at once, with shadows and a glow |
| gif | A full-slide photo, a title and two animated GIFs (480×360, 24 and 16 frames of 50 ms) |
| push | Sample deck: title slide to the agenda, a Push transition (already composited before) |
| morph | Sample deck: section header to the process slide, a Morph transition (still rendered every frame) |

## Browser (WebAssembly), as shipped before → after

CPU per frame in ms: mean, and 95th percentile in brackets. "Late" is the share of frames over
16.7 ms (a 60 Hz frame).

| Scenario | Before (no SIMD) | Before + SIMD only | After (layers + SIMD) | Late before → after | Upload per frame before → after |
|---|---|---|---|---|---|
| agenda | 15.15 (16.68) | 12.89 (14.04) | **0.08** (0.09) | 4% → 0% | 8.29 MB → 0 |
| process | 27.60 (38.18) | 18.94 (25.24) | **0.07** (0.08) | 90% → 0% | 8.29 MB → 0 |
| photo | 282.41 (295.42) | 118.18 (129.23) | **0.06** (0.11) | 100% → 0% | 8.29 MB → 0 |
| gif | 127.24 (376.72) | 46.54 (140.06) | **2.41** (21.37) | 34% → 7% | 2.86 MB → 0.22 MB |
| push | 0.16 (0.07) | 0.12 (0.05) | 0.13 (0.05) | 0% → 0% | 0.14 MB → 0.14 MB |
| morph | 21.93 (22.72) | 7.42 (7.73) | **7.53** (8.47) | 98% → 0% | 8.36 MB → 8.36 MB |

- The photo slide played at **3.5 frames a second** in the browser; its animation frames now cost
  0.06 ms of CPU and send nothing to the GPU, so it plays at the display's rate.
- An animated GIF used to cost a whole-slide render each time it changed frame (377 ms at the
  95th percentile: about 2.7 GIF frames a second); now each GIF frame is drawn once, the first time
  it shows (the remaining late frames, about 21 ms, are those first draws), and kept.
- Morph is faster only through SIMD (about 3×): it still renders every frame (see
  [gaps.md](../gaps.md#performance)).
- The after column with the release pipeline's `wasm-opt -Oz` pass: photo 0.04 ms, gif 2.38 ms,
  morph 7.19 ms; the size optimisation costs nothing measurable here.

The compositing alone, without SIMD: agenda 0.17, process 0.08, photo 0.05, gif 6.61 (p95 59.58)
ms per frame. SIMD alone makes the CPU renderer 1.2–3× faster (2.4× on the photo slide).

## Native (desktop), before → after

| Scenario | Before | After | Late before → after | Upload per frame before → after |
|---|---|---|---|---|
| agenda | 7.54 (9.72) | **0.03** (0.04) | 0% → 0% | 8.29 MB → 0 |
| process | 11.92 (17.31) | **0.03** (0.04) | 10% → 0% | 8.29 MB → 0 |
| photo | 59.92 (63.27) | **0.03** (0.03) | 100% → 0% | 8.29 MB → 0 |
| gif | 10.71 (32.09) | **0.82** (7.80) | 34% → 0% | 2.86 MB → 0.22 MB |
| push | 0.10 (0.02) | 0.17 (0.05) | 0% → 0% | 0.14 MB → 0.14 MB |
| morph | 6.31 (6.38) | 6.59 (7.15) | 0% → 0% | 8.36 MB → 8.36 MB |

## What it costs: entering a slide

A composited slide's layers are rendered once, while the slide rests after it appears (one layer a
frame, never in the frame that draws the whole slide; all at once if an animation starts first).
CPU over the first 30 frames of the slide, total and worst single frame, in ms:

| Scenario | Browser before (no SIMD) | Browser before + SIMD | Browser after | Native before | Native after |
|---|---|---|---|---|---|
| agenda | 40 / 38 | 29 / 28 | 98 / 42 | 11 / 11 | 36 / 14 |
| process | 9 / 8 | 7 / 6 | 41 / 6 | 4 / 4 | 23 / 4 |
| photo | 263 / 262 | 119 / 118 | 221 / 103 | 54 / 53 | 114 / 52 |
| gif | 4215 / 395 | 1505 / 139 | 807 / 139 | 357 / 35 | 236 / 42 |

The slide's first picture costs what it did; building the layers adds about one more whole-slide
render, spread over the next few frames while nothing moves. Leaving a composited slide right after
an animation ends renders it once more for the transition to start from.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-25 | major | Created: frame cost before and after the slide-show layers and WebAssembly SIMD, native and in V8 |
