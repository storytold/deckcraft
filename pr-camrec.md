## Summary

Recorded Cameo: camera frames are encoded per slide and committed as a normal video object.

- `CameoRecorder` in `deckcraft-media`: start/stop per slide, bounded frames, caps, camera-lost handling, fake camera tests.
- `show.cameo` creates/replaces the recorded clip and is undoable.
- H.264 via the encoder from #608 + `Mp4Writer`; temp MP4 -> poster + `a:videoFile`/`p14:media` + `p:video` timing.
- PPTX read/write round trip for live and recorded Cameo; background props preserved raw.

Closes the recorded-Cameo part of #1813.

## Verification

- 6/6 media camera-recorder tests, 13 new engine recorded-Cameo tests, 6/6 PPTX cameo tests pass on the source branch.
- Two pre-existing camera-base tests fail for fixture naming/selection order; left untouched and reported upstream.
