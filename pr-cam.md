## Summary

Live camera capture for the Camera Format tab and Cameo preview.

- `deckcraft-media::camera`: wasm-safe `CameraIn`/`CameraSession`/`CameraEvent`/`CameraFrame` traits, status mapping (`off/requesting/live/denied/noDevice/busy/lost/unsupported`) and a `FakeCamera` test source.
- `apps/deckcraft/src/camera/native.rs`: `cameras = "=0.3.2"` backend for Windows/macOS/Linux, lazy worker, bounded latest-frame channel, device/format enumeration, unplug/denied/busy handling; FreeBSD/wasm stub.
- `Services.camera_in` injection, `DECKCRAFT_NO_CAMERA`, `SlideApp.camera` host, editor/show Cameo rendering, Camera Format Preview toggle and Record ▸ Cameo.
- macOS `NSCameraUsageDescription` + camera entitlement; Flatpak `--device=all` for both manifests.

Closes part of #1813. Camera backgrounds are #1824; recorded Cameo is a separate stacked PR.

## Verification

- `cargo check -p deckcraft -p deckcraft-ui-egui -p deckcraft-media --tests` passes on Windows.
- 3 media camera tests + 12 UI camera tests pass; clippy clean (source branch).
- Linux CI must prove the `cameras`/v4l bindgen build; CI was not changed in this feature PR per AGENTS.
