## PowerPoint behaviour

PowerPoint's Camera Format/Camera Effects expose camera **backgrounds** for the live feed: normal, removed, blur and custom/replacement (`alf:liveFeedProps/backgroundProps`: Normal, Removed, Blur, Custom).

## Licensing search (2026-11-04)

A fully permissive path exists, so this is not blocked on MediaPipe:

- `imgly/background-removal-rs` (crate `imgly-bgremove`) is **MIT OR Apache-2.0** and supports ONNX Runtime and pure-Rust Tract backends.
- Its default models are MIT on Hugging Face: `imgly/isnet-general-onnx`, `onnx-community/BiRefNet-ONNX`, `onnx-community/BiRefNet_lite-ONNX`.
- `ort` (ONNX Runtime bindings) and `tract-onnx` are **MIT OR Apache-2.0**.

The remaining question is product/performance: a live 30 fps camera needs a small model or a GPU backend, and model files are large (download/cache, not vendored). That is why this is a separate feature, not part of the Camera Format shell.

## Scope

- Choose a permissively licensed segmenter (the `imgly-bgremove` + BiRefNet Lite path is the leading candidate).
- Add background Normal/Removed/Blur/Replacement to Cameo preview and recording, with a model-download/cache story.
- Preserve an imported cameo's `backgroundProps` in PPTX even before rendering is supported.
- Do not add MediaPipe or any non-dual-licensed model/runtime.

## Acceptance

Background removal/blur/replacement works on live Cameo with a provably dual-licensed model; PPTX background props round-trip.
