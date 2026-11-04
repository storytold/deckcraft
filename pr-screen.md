## Summary

Record ▸ Screen Recording, stacked on the video-export/H.264 branch.

- `ScreenIn` region capture behind a host trait; `scap` backend target-gated, FreeBSD/wasm stub.
- Region picker, record/pause/stop, optional audio/mouse.
- `ScreenRecorder` encodes with `deckcraft_h264::Encoder` and muxes with `craft_media::isobmff::Mp4Writer` to a temp MP4; inserted as a normal video media shape with poster and `p:video` timing.
- Reader distinguishes click-to-play from autoplay video.

Closes #1815.

## Verification

See branch commit; tests use synthetic frames/fixture MP4, no screen device in CI.
