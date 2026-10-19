# File-format parity with PowerPoint

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-18 · **Change:** minor (native chart payloads stored in bounded ZIP members; previous: major, community PR wave 2026-10-14: seven owned PPTX corpus files, embedded fonts, EMF/WMF clip regions, threaded comments (#70), chart trendlines and workbooks (#335), SmartArt written as a diagram, tags and custom XML kept, picture export DPI; duplicated rows and headers folded; previous: trivial, lines in scaled groups, #233) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

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
| `.pptx` presentation | Editor | yes | yes | Broad (see below); an owned corpus of decks we author round-trips tables, charts, animations, media, sections, masters and layouts, comments, links and actions, custom shows, rotated groups, inherited placeholders, text effects and body layout, theme effects, fills, bevels, connectors, 3-D models and animation bounce end. Not measured against PowerPoint renders or decks from outside this repo | `crates/pptx/tests/corpus.rs`, `corpus_kinds.rs`, `corpus_elements.rs`, `corpus_style.rs`, `corpus_themes.rs`, `corpus_charts.rs`, `corpus_media.rs`, `roundtrip.rs`, `sample.rs`, `malformed.rs`, `sniff.rs`; opened in PowerPoint by hand 2026-10-06 |
| `.potx` template | Editor | yes (as a presentation) | no: written with the `.pptx` content type | `file.saveTemplate` is Save As under another name | none |
| `.ppsx` slide show | Editor | yes (as a presentation) | no: written as `.pptx` content | Handed over by the system (command line, file association) it starts the slide show, as in PowerPoint; File › Open opens it for editing. The macOS Finder event arrives with #432 | `crates/ui-egui/src/opening.rs` tests |
| `.pptm` / `.potm` / `.ppsm` macro-enabled | Editor | no | no | Should open and keep `vbaProject.bin` without running it | none |
| `.ppt` / `.pot` / `.pps` 97–2003 binary | Editor | no | no | Owner decision; import only would be enough | none |
| `.odp` OpenDocument | Editor | no | no | | none |
| `.thmx` Office theme | Viewer | no | no | Browse for themes is a P2 row | none |
| `.deckcraft` native (zip + JSON) | n/a | yes (also `.slidecraft`) | yes | Lossless for the DeckCraft model, forward compatible | `crates/format` tests |
| PDF | export | no | yes | Slides, notes pages, handouts (1–9); real-text layer with embedded subsets, links, bookmarks. Slide artwork is a raster image at print resolution, not vector | `crates/pdf` tests |
| Outline `.rtf` / `.txt` | export (RTF), import (outline) | `.txt` outline; `.md` as Markdown (headings are slides, list items bullets) (`slide.fromOutline`) | `.txt` outline | No RTF | engine tests |
| PNG / JPEG slide images | export | n/a | yes (`file.export`, `deckcraft-cli render`) | Matches the editor's renderer. PNG `pHYs` and JPEG JFIF density are `scale` × 72 dpi (default scale 2 is 144 dpi), so Preview shows the slide's real size. PNG can leave the slide background out (`transparentBackground`, Export… “Transparent background”, `deckcraft-cli render --transparent`), so only the slide's content is opaque (#784) | render, engine, UI and CLI tests |
| TIFF / BMP / GIF slide images | export | n/a | no | Easy (the `image` crate writes them) | none |
| Animated GIF of the deck | export | n/a | yes (`file.export` format `gif`, Export…, `deckcraft-cli`, MCP `export`) | One frame per slide from the slide renderer; size small/medium/large/extra large or 16–3840 px wide, 0.1–600 s per slide, loop forever or play once, slide range, hidden slides skipped; a 256-colour palette per frame (exact, or NeuQuant above 256 colours). Transitions and build animations are not animated | `crates/engine/src/gif_export.rs` tests (decoded frames, size, delay, loop, hidden slides, hostile params), MCP, CLI and export dialog tests |
| MP4 / MOV video of the deck | export | n/a | no | Needs timeline render + H.264 encode | none |
| Print | yes | n/a | no | PR #66 (browser) open | none |

## Pictures and media inside decks

Native files retain a nonempty chart `kept` sidecar: relationships, part names, content types
and `nativeEntry` references live in `presentation.json`; exact binary payloads live in distinct
`chart-parts/<index>` ZIP members. This keeps workbooks and drawings through a native save before
exporting back to PPTX without expanding workbook bytes into JSON numbers. Chart parts and
media share the 4 GiB inflated-byte and 100,000-item budgets; each member is read through the
existing 2 GiB entry limit. Missing chart members are reported by `load_checked` and left empty.
A present chart member that fails decoding, CRC or its entry limit refuses the file immediately,
so later corrupt members cannot reuse the remaining inflation budget. Invalid, reused or
ambiguous references are refused. A synthetic 3 MiB payload test
checks the ZIP layout and bounded reads, alongside shared-budget and all-owner round trips.

Older native files without a sidecar still load. Earlier inline chart byte arrays are read
under the existing manifest-size limit, then charged to the shared budget; saving migrates them
to ZIP members. Builds predating chart-sidecar support ignore it and lose it on resave. The
intermediate build that required inline `bytes` cannot read the new referenced representation.

| Kind | PowerPoint | DeckCraft | Notes |
|---|---|---|---|
| PNG, JPEG, GIF (animated), BMP, TIFF, WebP | yes | yes | `image` 0.25; animated GIFs play since #39 |
| SVG | yes (with PNG fallback) | **no** | The PNG fallback shows; SVG files can't be inserted |
| EMF / WMF | yes | partial | Drawn as vectors (paths, pens, pattern and hatch brushes, bitmaps, text in the document's fonts with the file's character advances). Rectangle clip regions (`INTERSECTCLIPRECT`, `EXCLUDECLIPRECT`, `EXTSELECTCLIPRGN`) limit later drawing. EMF+ only pictures play every EMF+ drawing record (brushes incl. gradients and textures, pens, paths, curves, images, strings, transforms, clipping; see `crates/metafile/src/emfplus.rs` for the few exceptions). Legacy OLE objects (VML preview) and ActiveX controls show their pictures |
| HEIC | yes (macOS) | no | |
| Audio: MP3, AAC/M4A, ALAC, WAV, AIFF, CAF, FLAC, Ogg Vorbis, Opus | yes (Ogg/Opus partly) | yes | Pure-Rust decoders in `crates/media`, `crates/opus` |
| WMA | yes (Windows) | embedded, not playable | |
| Video: H.264, HEVC in MP4/MOV | yes | yes | Pure-Rust, software decode |
| Video: VP9, AV1 in WebM/MKV | partly (OS codecs) | yes | Ahead of PowerPoint for Mac |
| WMV | yes (Windows) | embedded, not playable | |
| Online video (YouTube embeds) | yes | no | Cloud feature |
| Embedded OLE objects | yes | kept on round trip (opaque), not insertable or editable | `ShapeKind::Opaque` |
| Embedded fonts | yes | yes | `p:embeddedFontLst` read and written. TrueType, OpenType and Embedded OpenType faces are used to draw; MicroType Express subsets are kept and written back, not drawn |
| 3-D models (`.glb`) | yes | kept | The model part round-trips (`p:contentPart`, including an `mc:AlternateContent` choice and its fallback picture). Not drawn as a mesh and not inserted from the ribbon |

## PPTX coverage (PresentationML / DrawingML)

What `crates/pptx` reads and writes, from ECMA-376. "Kept" means the XML is preserved untouched for
the round trip even though DeckCraft doesn't model it.

| Part / feature | Read | Write | Notes |
|---|---|---|---|
| OPC package, content types, relationships | yes | yes | Content types found anywhere in the zip (#19); zip bombs capped; relationship cycles bounded |
| Presentation, slide size, sections (p14), custom shows | yes | yes | |
| Slide masters, layouts, placeholders and inheritance | yes | yes | |
| Themes: colour, font (Latin/EA/CS), effect schemes, background styles | yes | yes | Unknown theme parts kept |
| Notes slides and notes master | yes | yes | Header, footer, date and slide-number text on the notes master is kept. Edited in View ▸ Notes Master. Notes keep their run and paragraph formatting (bold, italic, underline, size, levels, indents, bullets), shown and kept when edited in the notes pane and Notes Page (`crates/pptx/tests/notes.rs`). PDF notes pages print them as plain text; levels and bullets inherited from a custom notes master aren't shown |
| Handout master | yes | yes | Header, footer, date and slide-number text is kept. Edited in View ▸ Handout Master |
| Shapes: preset geometry (~180 presets), custom geometry, groups, connectors | yes | yes | Presets re-derived in `crates/geom/src/preset.rs` (and `preset/extra.rs`). Outlines, dashes, arrowheads and ink inside a group, and text, keep their size in points whatever the group's child-coordinate scale (#233). |
| Fills: solid, gradient, picture (stretch/tile/crop), pattern, group | yes | yes | |
| Lines: dash, compound, caps, joins, arrowheads | yes | yes | |
| Text: runs, paragraphs, bullets, numbering, levels, autofit, columns, vertical, RTL, script fonts, fields, hyperlinks | yes | yes | Round-trip tests incl. `justLow`, date fields and paragraph `defRPr` (the defaults a placeholder's runs inherit, including a link). A click or hover action keeps its sound, `endSnd`, and a custom show's "Show and return". Gradient and outline on a run, autofit, columns, vertical and RTL text, tab stops and line spacing round-trip (`corpus_style.rs`) |
| Effects: shadow, glow, soft edges, reflection | yes | yes | On a run, inner shadow, reflection and soft edges are kept along with the outer shadow and glow |
| 3-D: `scene3d`, `sp3d` | kept | kept | A bevel preset with no `sp3d` of its own is written back on shapes and theme effect styles. Bevels, cameras, face lighting and extrusion sides draw approximately |
| Text warp (`prstTxWarp`) | yes | yes | All 40 presets rendered (`crates/geom/src/warp.rs`); adjust values (`avLst`) round-trip |
| Pictures with crop, recolour, duotone, alpha, luminance | yes | yes | |
| Tables with styles | yes | yes | Merges and both diagonal borders round-trip |
| Charts (`c:chartSpace`) | yes | yes | Original XML kept in `Chart.raw`. Trendlines (type, order or period, forecast, intercept, equation and R²) are read, written and drawn. A combo chart's secondary value axis is read and is not written back when the chart is rebuilt. An unchanged chart also keeps its embedded workbook and the other parts its relationships point at, under fresh `keptN` names so they never replace a part DeckCraft writes (#335); editing the chart drops that workbook instead of writing a stale one |
| Charts (`cx:` chartex: waterfall, funnel, treemap, sunburst, histogram, Pareto, box) | yes | yes | Unchanged parts kept, with their embedded workbook (#335); edited and new ones written from the model with a plain-chart fallback. Treemap and sunburst hierarchy levels kept; region maps kept, drawn as tiles (no map geometry) |
| SmartArt (`dgm`) | the drawing part as a group (text laid out on its `dsp:txXfrm` rectangle), and the outline from the data model (Text Pane) | as a diagram: data model from the outline; layout, quick style and colour parts (the file's own kept until the layout is switched); the group as the cached drawing | DeckCraft's layout definitions are simple; a diagram without a cached drawing isn't drawn. SmartArt kept only as XML, without its relationship parts, still loses the drawing and data parts (#203) |
| Equations (OMML, `a14:m`) | no | no | PR #55 open |
| Audio / video (`p14:media`, trim, fades, bookmarks) | yes | partly | Playback options lost on save (#74) |
| Ink (`contentPart`) | yes | as a group of freeform lines | PowerPoint sees shapes, not ink |
| Transitions incl. p14/p15 (Vortex, Morph…) | all 48 | all 48 | `every_transition_kind_round_trips` |
| Animations: timing tree, build paragraphs, triggers, motion paths | yes (flattened to the Animation Pane list) | yes (rebuilt) | `every_animation_effect_survives`; repeat, triggers and bounce end kept; rewind lost (#77) |
| Comments: legacy | yes | yes | |
| Comments: modern threaded (`p188`) | yes | yes | Written next to the flattened legacy part; replies and resolved state round-trip (`threads_and_resolved_round_trip`) |
| Unknown slide-level elements (`graphicFrame` OLE, extensions, `mc:AlternateContent`) | kept | kept | Rendered as a labelled placeholder with the preview picture when present |
| Embedded fonts | yes | yes | MicroType Express subsets kept, not drawn |
| Tags (`p:custDataLst`) on the presentation, slides and shapes | yes | yes | Add-ins such as think-cell keep their identity; `tags_survive_round_trip` |
| Custom XML data, sensitivity label (`docMetadata/LabelInfo.xml`), custom document properties | kept verbatim | yes | SharePoint metadata and the label survive a save; `custom_xml_label_and_custom_properties_survive_save` |
| VBA | no | no | |
| Encryption (password to open) | no | no | |

### What would move the PPTX number

1. A larger owned corpus (corporate templates, SmartArt, chartex charts, EMF, embedded fonts,
   every transition and animation) in a corpus repo. `crates/pptx/tests/corpus.rs` and
   `corpus_kinds.rs` already check open → save → reopen (parts and text) for decks we author:
   tables, charts, animations, media, sections and masters/layouts among them. Still needed:
   render checks and a PowerPoint reference render for each (owner runs PowerPoint).
2. The round-trip losses above (#70, #74, #77).
3. SVG rendering. EMF+ records inside EMF/WMF are still not drawn.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-18 | minor | Native chart payloads use referenced ZIP members and share media byte/item limits; old inline sidecars remain readable. Synthetic tests cover multi-megabyte layout, all shape owners, shared budgets, missing/corrupt members and invalid references. Scores and milestone scope unchanged |
| 2026-10-15 | trivial | Native `.deckcraft` files serialize a nonempty chart `kept` sidecar (relationships, part names, content types and bytes). Older files without it still load; empty sidecars are omitted. Headless PPTX → edit → native save/reopen → PPTX checks keep the workbook and drawing bytes; editing chart data still drops the stale workbook |
| 2026-10-15 | trivial | PPTX interactive timing sequences keep later On Click steps and paragraph click builds (`trigger_steps.rs`); mixed starts and independent buttons survive import/edit/save/reopen |
| 2026-10-15 | minor | A `.pptx` slide that can't be read is reported instead of dropped in silence: `file.open` and `file.openBytes` return `lostSlides`, the app shows a message and opens a repaired copy (Save asks for a new name), and `deckcraft-cli` warns on stderr (#203) |
| 2026-10-16 | minor | PNG slide export with a transparent background (#784) |
| 2026-10-16 | trivial | A `.ppsx` opened from the command line or the file association starts the slide show (#862) |
| 2026-10-14 | minor | Community PR wave folded: one row each for `.pptx` (seven owned corpus files), outline, 3-D models, embedded fonts, text, effects, 3-D (extrusion sides draw now), SmartArt (written as a diagram) and VBA (tags and custom XML are kept) |
| 2026-10-12 | minor | SmartArt import lays each shape's text out on its `dsp:txXfrm` text rectangle instead of the shape box |
| 2026-10-13 | minor | EMF/WMF rectangle clip regions limit later drawing; EMF+ records remain |
| 2026-10-13 | minor | Embedded fonts read and written; TrueType, OpenType and Embedded OpenType faces drawn |
| 2026-10-13 | minor | Owned corpus round-trip (parts and text). Notes-master header, footer, date and slide-number text is written back |
| 2026-10-13 | minor | Owned corpus for comments, links and actions, custom shows, rotated groups and inherited placeholders. Paragraph `defRPr` and action sounds (including custom-show return) survive a save |
| 2026-10-13 | minor | Owned corpus for text effects, body layout, theme effects, fills, bevels and connectors. A run's inner shadow, reflection and soft edge, and a bevel preset with no kept 3-D XML, survive a save |
| 2026-10-14 | minor | 3-D models (`.glb`) round-trip, including an alternate-content fallback picture; animation bounce end is read and written (`corpus_media.rs`) |
| 2026-10-14 | minor | Chart trendlines are drawn (linear, exponential, logarithmic, polynomial, power, moving average, forecast, equation and R²) |
| 2026-10-14 | minor | Chart trendlines round-trip (`corpus_charts.rs`); merges, diagonal borders, columns, autofit, vertical text and WordArt warp are in that slice |
| 2026-10-13 | minor | Add-in tags on the presentation, slides and shapes read and written |
| 2026-10-13 | minor | Custom XML parts, the sensitivity label and custom properties kept on save |
| 2026-10-13 | minor | Owned corpus round-trip for tables, charts, animations, media, sections and masters. Handout-master header, footer, date and slide-number text is written back |
| 2026-10-14 | minor | PNG and JPEG slide export record their resolution (`scale` × 72 dpi; the default 2× picture is 144 dpi) |
| 2026-10-13 | trivial | Unchanged charts keep the embedded workbook (`ppt/embeddings/`) and chart relationships (#335) |
| 2026-10-12 | minor | EMF/WMF text and pattern brushes drawn; OLE (VML) and ActiveX previews read |
| 2026-10-12 | minor | Text warps rendered; their adjust values read and written |
| 2026-10-12 | trivial | Groups with a small legacy child coordinate space draw outlines, dashes, arrowheads and ink at their own width (#233) |
| 2026-10-11 | minor | Speaker notes: formatting shown in the notes pane and Notes Page and kept when they are edited |
| 2026-10-11 | minor | Animated GIF of the deck: export yes (#171) |
| 2026-10-10 | major | Created from PowerPoint 16.113.4's `CFBundleDocumentTypes`, `crates/pptx` reader/writer and tests, and the open round-trip issues |
| 2026-10-13 | minor | Comments: the writer emits the modern threaded part (`p188`) next to the flattened legacy one; the reader prefers the modern part and scales its EMU positions (#70) |
