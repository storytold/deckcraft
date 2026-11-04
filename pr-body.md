## Summary

PowerPoint parity for the Record tab and the Camera Format tab, built on the recording stack from #614/#624/#636.

- **Recording**: `show.record` keeps slide timings, per-slide narration, pen/highlighter ink and laser traces; the Record tab starts it from beginning or current slide.
- **Record view**: full-screen dark recording shell with timer, Record/Pause/Reset/Undo, notes/script pane, slide canvas, ink toolbar, navigation, Views and Export hooks.
- **Audio**: Record ▸ Audio captures the microphone and inserts a WAV audio shape on the current slide.
- **Preview**: Record ▸ Preview plays recorded timings and narration without writing anything.
- **Clear Recording / Reset to Cameo**: current/all-slide cleanup of recorded narration, camera/screen media, ink, laser and advance timings, one undo step; Reset restores the live Cameo.
- **Save as Show**: writes a `.ppsx` copy through the normal Save dialog.
- **Camera Format**: Cameo model + `alf:liveFeedProps`/`asvg:svgBlip` PPTX round trip, `insert.cameo`, 9 Camera Styles, Apply to All Slides, Camera Shape/Border/Effects/Format Object, Alt Text pane, Arrange, Crop/Height/Width/Size and Position.
- **Camera capture**: live camera backend (`cameras` 0.3.2) behind a wasm-safe trait, preview in the editor and show, device/status handling, macOS/Flatpak permissions.

## Issues closed

Closes #1812, #1814, #1816, #1817, #1818, #1819, #1820.
Part of #1813 (Cameo model, Camera Format and live preview; recorded Cameo and camera backgrounds are follow-ups).

## Verification

- `cargo check -p deckcraft-engine -p deckcraft-ui-egui -p deckcraft-pptx` passes.
- `cargo test -p deckcraft-pptx --test it cameo` (in progress at opening; result will be posted).
- Camera tests and capture tests pass on the source branches.
- The screen-recording and recorded-Cameo branches are stacked separately.

## Notes

- Camera backgrounds are deferred to #1824: the permissive path (`imgly-bgremove` + MIT ISNet/BiRefNet models) is documented, but it is a separate model/runtime feature.
- Video export is not duplicated here; it stays with #608/#616/#622/#628/#629/#686/#1321.
