# File-format parity with PowerPoint

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created; formats taken from PowerPoint 16.113.4's Info.plist, PPTX coverage measured from `crates/pptx`) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

Every format PowerPoint reads or writes, DeckCraft's support, fidelity and how it is tested.
Gaps are itemized in [gaps.md](gaps.md#file-formats). Dimension score: **~58% ready**
(estimated), **60–110 h** to parity.

Weights inside this dimension: PPTX 70%, PDF export 10%, image export 5%, legacy binary formats
5%, everything else (templates, shows, macros, ODP, video, GIF, outline) 10%.

## Documents

From `/Applications/Microsoft PowerPoint.app/Contents/Info.plist` `CFBundleDocumentTypes` (Editor =
PowerPoint opens and saves; Viewer = opens only).

| Format | PowerPoint | DeckCraft read | DeckCraft write | Fidelity / notes | Tested by |
|---|---|---|---|---|---|
| `.pptx` presentation | Editor | yes | yes | Broad (see below); never measured on real decks | `crates/pptx/tests/roundtrip.rs`, `sample.rs`, `malformed.rs`, `sniff.rs` (30 tests); opened in PowerPoint by hand 2026-10-06 |
| `.potx` template | Editor | yes (as a presentation) | no: written with the `.pptx` content type | `file.saveTemplate` is Save As under another name | none |
| `.ppsx` slide show | Editor | yes (as a presentation) | no: written as `.pptx` content | Doesn't start in show mode | none |
| `.pptm` / `.potm` / `.ppsm` macro-enabled | Editor | no | no | Should open and keep `vbaProject.bin` without running it | none |
| `.ppt` / `.pot` / `.pps` 97–2003 binary | Editor | no | no | Owner decision; import only would be enough | none |
| `.odp` OpenDocument | Editor | no | no | | none |
| `.thmx` Office theme | Viewer | no | no | Browse for themes is a P2 row | none |
| `.deckcraft` native (zip + JSON) | n/a | yes (also `.slidecraft`) | yes | Lossless for the DeckCraft model, forward compatible | `crates/format` tests |
| PDF | export | no | yes | Slides, notes pages, handouts (1–9); real-text layer with embedded subsets, links, bookmarks. Slide artwork is a raster image at print resolution, not vector | `crates/pdf` tests |
| Outline `.rtf` / `.txt` | export (RTF), import (outline) | `.txt`/`.md` outline (`slide.fromOutline`) | `.txt` outline | No RTF | engine tests |
| PNG / JPEG slide images | export | n/a | yes (`file.export`, `deckcraft-cli render`) | Matches the editor's renderer | render tests |
| TIFF / BMP / GIF slide images | export | n/a | no | Easy (the `image` crate writes them) | none |
| Animated GIF of the deck | export | n/a | no | | none |
| MP4 / MOV video of the deck | export | n/a | no | Needs timeline render + H.264 encode | none |
| Print | yes | n/a | no | PR #66 (browser) open | none |

## Pictures and media inside decks

| Kind | PowerPoint | DeckCraft | Notes |
|---|---|---|---|
| PNG, JPEG, GIF (animated), BMP, TIFF, WebP | yes | yes | `image` 0.25; animated GIFs play since #39 |
| SVG | yes (with PNG fallback) | **no** | The PNG fallback shows; SVG files can't be inserted |
| EMF / WMF | yes | **no** | Common in decks made on Windows: blank picture |
| HEIC | yes (macOS) | no | |
| Audio: MP3, AAC/M4A, ALAC, WAV, AIFF, CAF, FLAC, Ogg Vorbis, Opus | yes (Ogg/Opus partly) | yes | Pure-Rust decoders in `crates/media`, `crates/opus` |
| WMA | yes (Windows) | embedded, not playable | |
| Video: H.264, HEVC in MP4/MOV | yes | yes | Pure-Rust, software decode |
| Video: VP9, AV1 in WebM/MKV | partly (OS codecs) | yes | Ahead of PowerPoint for Mac |
| WMV | yes (Windows) | embedded, not playable | |
| Online video (YouTube embeds) | yes | no | Cloud feature |
| Embedded OLE objects | yes | kept on round trip (opaque), not insertable or editable | `ShapeKind::Opaque` |
| 3-D models (`.glb`) | yes | no | |
| Embedded fonts | yes | no | `p:embeddedFontLst` ignored |

## PPTX coverage (PresentationML / DrawingML)

What `crates/pptx` reads and writes, from ECMA-376. "Kept" means the XML is preserved untouched for
the round trip even though DeckCraft doesn't model it.

| Part / feature | Read | Write | Notes |
|---|---|---|---|
| OPC package, content types, relationships | yes | yes | Content types found anywhere in the zip (#19); zip bombs capped; relationship cycles bounded |
| Presentation, slide size, sections (p14), custom shows | yes | yes | |
| Slide masters, layouts, placeholders and inheritance | yes | yes | |
| Themes: colour, font (Latin/EA/CS), effect schemes, background styles | yes | yes | Unknown theme parts kept |
| Notes slides and notes master | yes | yes | No notes-master editing view |
| Handout master | no | no | |
| Shapes: preset geometry (~150 presets), custom geometry, groups, connectors | yes | yes | Presets re-derived in `crates/geom/src/preset.rs` |
| Fills: solid, gradient, picture (stretch/tile/crop), pattern, group | yes | yes | |
| Lines: dash, compound, caps, joins, arrowheads | yes | yes | |
| Effects: shadow, glow, soft edges, reflection | yes | yes | |
| 3-D: `scene3d`, `sp3d` | kept | kept | Not rendered |
| Text: runs, paragraphs, bullets, numbering, levels, autofit, columns, vertical, RTL, script fonts, fields, hyperlinks | yes | yes | Round-trip tests incl. `justLow` and date fields |
| Text warp (`prstTxWarp`) | yes | yes | Not rendered |
| Pictures with crop, recolour, duotone, alpha, luminance | yes | yes | |
| Tables with styles | yes | yes | |
| Charts (`c:chartSpace`) | yes | yes | Original XML kept in `Chart.raw` |
| Charts (`cx:` chartex: waterfall, funnel, treemap, sunburst, histogram, box) | no | no | |
| SmartArt (`dgm`) | drawing part only, as a group | as shapes | Diagram data model lost on save |
| Equations (OMML, `a14:m`) | no | no | PR #55 open |
| Audio / video (`p14:media`, trim, fades, bookmarks) | yes | partly | Playback options lost on save (#74) |
| Ink (`contentPart`) | yes | as a group of freeform lines | PowerPoint sees shapes, not ink |
| Transitions incl. p14/p15 (Vortex, Morph…) | all 48 | all 48 | `every_transition_kind_round_trips` |
| Animations: timing tree, build paragraphs, triggers, motion paths | yes (flattened to the Animation Pane list) | yes (rebuilt) | `every_animation_effect_survives`; rewind lost (#77) |
| Comments: legacy | yes | yes | |
| Comments: modern threaded (`p188`) | yes | no (written as legacy) | Replies and resolved state lost (#70) |
| Unknown slide-level elements (`graphicFrame` OLE, extensions, `mc:AlternateContent`) | kept | kept | Rendered as a labelled placeholder with the preview picture when present |
| Tags, custom XML, VBA | no / no / no | | |
| Embedded fonts | no | no | |
| Encryption (password to open) | no | no | |

### What would move the PPTX number

1. An owned corpus of real-world-shaped decks (corporate templates, SmartArt, chartex charts, EMF,
   embedded fonts, every transition and animation) in a corpus repo, with import → render → export
   → re-import checks in CI and a PowerPoint reference render for each (owner runs PowerPoint).
2. The round-trip losses above (#70, #74, #77).
3. EMF/WMF and SVG rendering.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created from PowerPoint 16.113.4's `CFBundleDocumentTypes`, `crates/pptx` reader/writer and tests, and the open round-trip issues |
