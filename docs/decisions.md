# Decisions

> **Last reviewed:** 2026-11-07 · **Last updated:** 2026-11-07 · **Change:** minor (recorded the camera-background model, runtime, isolation and accelerator decisions) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

Short records of choices that constrain later work. Each names the alternative that was rejected
and the evidence behind the choice.

## D1. Camera backgrounds use RTen and BiRefNet Lite 512

**Decision:** `crates/matte` (layer 2, pure, wasm-clean) owns the manifest, preprocessing,
postprocessing, compositing and the `Segmenter` seam. `crates/matte-rten` (layer 6, native) runs
`studioludens/birefnet-lite-512` through RTen 0.27 on the CPU.

**Why:** it is pure Rust, MIT/Apache-2.0, builds on Windows, macOS, Linux and the BSDs, needs no
native library and stays out of the wasm build. Measured on 2026-11-07 (CPU, release): RTen 2.10
s/frame inference, 3.96 s end to end at 640x480; onnxruntime 1.43 s/frame; tract-onnx 7.8 s/frame
at 512; tract at 1024 was 33.7 s/frame. RTen's output matches onnxruntime (correlation 1.000000,
max abs difference 4.6e-5). A hand-made RTen thread pool was slower (16.7 s at 21 threads), so the
default RTen pool is used and only `DECKCRAFT_MATTE_THREADS` overrides it.

**Rejected:** `onnx-community/BiRefNet_lite` at 1024×1024 with tract-onnx (224 MB, 33.7 s/frame);
`tract-onnx` at 512 (7.8 s/frame); adding ONNX Runtime (a C/C++ runtime, a new dependency on every
platform). MediaPipe is explicitly out: its runtime and models are not on the licence allowlist.

The weights are MIT and user-downloaded, never committed; the manifest records the exact revision,
size and SHA-256.

## D2. Inference is isolated and runs at a reduced cadence

**Decision:** one worker thread owns the model. At most one frame waits, and the caller offers a new
one only when nothing is waiting and the 2 s minimum interval has elapsed, so a frame already in the
queue is never replaced (`MatteWorker::submit` does replace it when a host calls it directly).
The compositor uses the last matte until a new one arrives, and reports `last_ms`, `runs` and
`reused`. RTen sizes its own CPU pool; `DECKCRAFT_MATTE_THREADS` caps it explicitly.

**Why:** 2.1 s/frame against a 30–60 Hz camera and a UI thread that must keep drawing. Running the
model inline would freeze the window; running it per frame would queue minutes of work.

**Rejected:** inferring on the UI thread; a queue of frames (latency grows without bound).

## D3. Accelerators are optional and behind one seam

**Decision:** the manifest names the runtime and its accelerators; the status query reports the
active accelerator; `set_accelerator` returns one clear message for anything this build cannot run
(`runs on CPU; OpenVINO NPU/GPU not available`). CPU is the default and only implementation today.

**Why:** Intel NPU/GPU numbers through OpenVINO are still being measured. If they justify a backend,
it lands as a separate optional crate with CPU fallback; nothing in the feature changes. DirectML
and required accelerators are excluded. A C/C++ runtime is not added by the camera-background PR.

## D4. Custom backgrounds are DeckCraft state, not OOXML state

**Decision:** `Custom`'s replacement image and the replacement colour live in the `.deckcraft`
document. PPTX writes `<alf:backgroundProps><alf:Custom/></alf:backgroundProps>` and keeps every
other mode exactly; unknown children of the feed and of `backgroundProps` survive through
`Cameo.raw`. **Exception:** a child that names a package relationship (`r:embed`, `r:link`, `r:id`)
is not kept — export creates no such relationship, so such a feed is written back with the mode only
and its other unknown children are lost with it; PowerPoint would otherwise see a dangling reference
and repair the file.

**Why:** the `livefeed` schema (MS-ODRAWXML) gives `Custom` only an `extLst`; it has no colour and no
image relationship. Writing an invented attribute or a dangling relationship would make PowerPoint
repair the file. The mode round-trips; the picture is a DeckCraft extension.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-07 | minor | New: RTen + BiRefNet Lite 512, worker isolation and cadence, the accelerator seam, and OOXML's Custom limitation |
