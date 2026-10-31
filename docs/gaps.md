# Where DeckCraft falls short of PowerPoint

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-30 · **Change:** trivial (animation Effect Options now show readable labels; Animation Pane gap remains) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)
> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-28 · **Change:** trivial (AutoFormat lists run independently of Replace text as you type; previous: minor, Animation Pane ▸ Play From; previous: minor, native chart payloads stored in bounded ZIP members; previous: major, community PR wave 2026-10-14: soak tests, PPTX corpus, comment threads (#70), embedded fonts, SmartArt Text Pane, Animation Pane timeline, Hunspell spelling, thesaurus, AutoCorrect, 14 new dialogs, pen pressure and trackpad gestures; duplicated entries and headers folded; previous: minor, 3-D bevel, camera and face lighting rendered) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)
> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-20 · **Change:** trivial (empty chart number-format sections hide values; previous: minor, native chart payloads stored in bounded ZIP members; previous: major, community PR wave 2026-10-14: soak tests, PPTX corpus, comment threads (#70), embedded fonts, SmartArt Text Pane, Animation Pane timeline, Hunspell spelling, thesaurus, AutoCorrect, 14 new dialogs, pen pressure and trackpad gestures; duplicated entries and headers folded; previous: minor, 3-D bevel, camera and face lighting rendered) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)
> **Last reviewed:** 2026-10-27 · **Last updated:** 2026-10-27 · **Change:** minor (docs refresh against merged PRs: #67 and #77 landed, #75 closed unmerged, #53 closed, Windows image paste is #1414; one header line; previous: minor, Animation Pane ▸ Play From; previous: minor, native chart payloads stored in bounded ZIP members; previous: major, community PR wave 2026-10-14: soak tests, PPTX corpus, comment threads (#70), embedded fonts, SmartArt Text Pane, Animation Pane timeline, Hunspell spelling, thesaurus, AutoCorrect, 14 new dialogs, pen pressure and trackpad gestures; duplicated entries and headers folded; previous: minor, 3-D bevel, camera and face lighting rendered) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

Every known shortfall, one entry each, ranked by how much it stops someone who presents with
PowerPoint from switching. This is the work list: pick from the top. Numbers are summarized in
[target-app-parity.md](target-app-parity.md); the feature-by-feature scores are in
[parity-checklist.md](parity-checklist.md). Hours are Opus 5.5 agent wall-clock hours including
tests (calibration in [target-app-parity.md](target-app-parity.md#calibration-how-the-hours-were-derived)).
`B` marks a gap that blocks **beta**. None blocks **alpha**: all six core workflows pass the
[alpha gate](roadmap.md#alpha-gate-powerpoints-core-workflows).

When a gap closes, delete its entry, update the parity doc it points to, and add a line to the
revision history.

## Ranked: the top 15

| # | Gap | Category | Beta | Hours | Doc |
|---|---|---|:-:|---|---|
| 1 | PPTX corpus is only decks we author (no PowerPoint reference renders) | File format | B | 20–35 | [file-format-parity.md](file-format-parity.md) |
| 2 | Data loss and crashes on common paths (#60, #40, #59) | Stability | B | 6–12 | this file |
| 3 | PPTX round trip drops media playback options (#74) | File format | B | 2–4 | [file-format-parity.md](file-format-parity.md) |
| 4 | SVG pictures don't render (EMF/WMF do) | File format | B | 4–8 | [file-format-parity.md](file-format-parity.md) |
| 5 | No localization: English only | Localization | B (infra + 5 languages) | 55–90 | [localization-parity.md](localization-parity.md) |
| 6 | No printing | Feature | B | 5–8 | [file-format-parity.md](file-format-parity.md) |
| 7 | Fonts: only Regular, Bold and Italic faces (#69) | Feature | B | 3–5 | [ui-parity.md](ui-parity.md) |
| 8 | Image paste fails on Windows (#1414); Linux not re-checked | Feature | B | 2–4 | [ui-parity.md](ui-parity.md) |
| 9 | Presenter view isn't placed on the second display | Hardware | B | 3–5 | [slideshow-parity.md](slideshow-parity.md) |
| 10 | Animation Pane: 64 of ~190 effects, effect options (readable labels, timeline and Play From done) | Feature | B (pane) | 7–13 | [animation-parity.md](animation-parity.md) |
| 11 | Charts: data grid editing, elements, `chartex` types | Feature | B (data, elements) | 12–20 | [parity-checklist.md](parity-checklist.md) |
| 12 | Format Shape pane thinner than PowerPoint's | UI/UX | B | 6–10 | [ui-parity.md](ui-parity.md) |
| 13 | No native macOS menu bar | UI/UX | B | 3–5 | [ui-parity.md](ui-parity.md) |
| 14 | PDF export rasterizes slide artwork | File format | B | 5–8 | [file-format-parity.md](file-format-parity.md) |
| 15 | Slides rasterize on the CPU; no performance baseline | Performance / hardware | | 15–30 | [hardware-parity.md](hardware-parity.md) |

## Stability

### Close discards an edited presentation without asking (#60)
- **Evidence:** issue #60 is open; quitting prompts, `file.close` doesn't. PR #75 was closed without merging.
- **Impact:** silent data loss on a common path. The worst kind of bug for a presentation app.
- **Estimate:** 0.5–1 h (a new prompt and its tests, since #75 was not merged). **Beta blocker.**

### Startup crashes on some Windows GPUs (#40, #59)
- **Evidence:** #59 (Intel UHD, access violation in `igvk64.dll` during Vulkan enumeration), #40
  (WordCraft, GridCraft and DeckCraft all fail on one Windows 11 PC). The DX12 default (#16) fixed
  AMD's `atio6axx.dll` crash only.
- **Impact:** the app never opens for those users.
- **Estimate:** 4–8 h (backend probing outside the process or a safe-mode fallback, a software
  renderer path). Needs a tester with the hardware. **Beta blocker.**

### Soak testing has no memory or timing budgets
- **Evidence:** `crates/engine/src/soak_tests.rs` runs seeded random command sequences over a
  ~100-slide deck (validity, selection, undo/redo exactness, bounded undo, save/reopen in both
  formats every 100 steps); `DECKCRAFT_SOAK_STEPS` runs it long. It found a chart `clamp` panic
  that blanked whole slides. `crates/ui-egui/src/ui_soak_tests.rs` drives the whole window with
  seeded random clicks, drags, keys (plain, Ctrl, Shift), typing and scrolling over the sample
  deck, checking document validity, bounded undo/journal/thumbnail/texture caches, zero caught
  command and render panics (`Session::panics_caught`, `deckcraft_render::panics_caught`) and a
  mean frame under 250 ms; 4 × 3000-action runs are clean.
- **Impact:** long-session crash rate is sampled in CI; RSS is not measured directly.
- **Estimate:** 1–2 h (RSS sampling per platform).

### A control-server timing test may fail under load
- **Evidence:** `silent_refused_peers_do_not_delay_the_next_refusal` in
  `apps/deckcraft/src/control_server.rs` asserts that a refusal arrives in under 1 s, so it can
  fail on a busy machine although the server behaves correctly.
- **Estimate:** under 1 h (a looser bound or a logical clock).

## File formats

Detail in [file-format-parity.md](file-format-parity.md).

### PPTX corpus is only decks we author
- **Evidence:** every PPTX test (`crates/pptx/tests`) uses decks built in this repo; the
  2026-10-07 alpha checklist already listed this as open, and no reference renders from PowerPoint
  exist to compare against. The owned corpus checks open → save → reopen (package parts and text):
  `corpus.rs` (a talk with a table, chart, picture, link and comment; a notes master's header and
  footer), `corpus_kinds.rs` (tables, charts, animations, media, sections, several masters and
  layouts; a handout master's header, footer, date and slide number), `corpus_elements.rs`
  (comments, hyperlinks and actions, custom shows, grouped rotated objects, placeholders that
  inherit from the layout), `corpus_style.rs` (text effects, autofit, columns, vertical and RTL
  text, tab stops, line spacing, theme effects, gradient and pattern fills, 3-D bevels,
  connectors), `corpus_themes.rs` (PowerPoint-shaped themes and two masters with their layouts,
  effects, text and shape hyperlinks and actions), `corpus_charts.rs` (a combo chart with data
  labels and trendlines, a merged table with diagonal borders, text columns, autofit, vertical
  text, a WordArt warp) and `corpus_media.rs` (SmartArt and OLE kept as XML, with no diagram or
  embedding parts, which stay with #203; audio and video bookmarks and trims; a section holding a
  hidden slide; slide-number and date fields; ink as freeform lines; `.glb` 3-D models; animation
  repeat, bounce end and triggers). Losses these checks found are fixed: notes- and handout-master
  text, paragraph default run properties (`defRPr`, including a link), action sounds, a run's
  inner shadow, reflection and soft edge, a bevel preset with no kept `sp3d`, and text Mouse Over
  actions. Still open: media options (#74) and a corpus of decks from
  outside this repo.
- **Impact:** the main reason to switch is "open my PowerPoint decks". Fidelity on real decks
  (corporate templates, SmartArt, `chartex` charts, embedded fonts, EMF) is still mostly unknown.
- **Estimate:** 20–35 h: grow the owned corpus in `storytold/photocraft-corpus` style (craftrules
  `standards/test-corpora.md`), add render checks, have the owner open outputs in PowerPoint, fix
  what breaks. **Beta blocker.**

### Round trip loses media playback options (#74)
- **Evidence:** issue #74 (filed 2026-10-10 by an automated reviewer) is open: media playback options aren't written back. Animation rewind (#77) is fixed (PR #83 merged). Comment threads left with #70: the writer now emits
  the `p188` modern part next to the legacy one.
- **Impact:** a deck opened and saved silently loses show behaviour.
- **Estimate:** 2–4 h. **Beta blocker.**

### SVG pictures don't render
- **Evidence:** `crates/render/src/images.rs` decodes through `image` 0.25 (PNG, JPEG, WebP, GIF,
  TIFF, BMP), and EMF/WMF through `deckcraft-metafile` (ported from WordCraft: paths, fills, pens,
  pattern and hatch brushes, bitmaps and text; rectangle clip regions are applied, EMF+ only
  pictures play their EMF+ records).
  Nothing reads SVG; PowerPoint's Insert Icons stores SVG with a PNG fallback.
- **Impact:** SVG icons show only their raster fallback, and SVG files can't be inserted.
- **Estimate:** 4–8 h (an SVG renderer through `resvg`-style crates).

### Macro-enabled variants aren't recognized
- **Evidence:** `.pptx`, `.potx` and `.ppsx` open and save with their own main content types
  (`crates/pptx/src/lib.rs` `export_as`); `.pptm`/`.potm`/`.ppsm` aren't recognized.
- **Impact:** macro decks can't be opened even to view.
- **Estimate:** 2–4 h (content types; keep `vbaProject.bin` untouched without running it).

### No legacy `.ppt`/`.pot`/`.pps` import
- **Evidence:** PowerPoint for Mac still opens and saves the 97–2003 binary formats
  (`CFBundleDocumentTypes`); DeckCraft doesn't read them.
- **Impact:** archives of older decks don't open.
- **Estimate:** 20–35 h (import only, from the public [MS-PPT] spec). Owner decision whether it's
  worth it.

### No OpenDocument `.odp`
- **Evidence:** PowerPoint opens and saves `.odp`.
- **Impact:** LibreOffice users and public-sector exchange.
- **Estimate:** 12–20 h (read and write the common subset).

### PDF export rasterizes slide artwork
- **Evidence:** `crates/pdf` places each slide as an image under a selectable text layer, links and
  bookmarks.
- **Impact:** large PDFs; blurry when zoomed or printed.
- **Estimate:** 5–8 h (vector paths, fills and text through the PDF writer). **Beta blocker.**

### No printing
- **Evidence:** checklist row "Print" is missing; PR #66 (browser print) is open.
- **Impact:** handouts and speaker notes are printed in many workplaces.
- **Estimate:** 5–8 h (print through the PDF path and each OS's print dialog). **Beta blocker.**

### No video export; the animated GIF has no transitions or builds
- **Evidence:** checklist row "Export video (MP4) / animated GIF" is partial: `file.export` writes
  an animated GIF with one frame per slide (#171), but there is no MP4/MOV export, and the GIF
  doesn't animate transitions or build animations.
- **Impact:** sharing a talk as a video is a common request.
- **Estimate:** 10–16 h (render the timeline frame by frame, which the GIF can then reuse for
  transitions and builds; an H.264 encoder is the hard part, FilmCraft has one to copy).

### Embedded MicroType Express font subsets aren't drawn
- **Evidence:** `p:embeddedFontLst` is read and written. A raw TrueType/OpenType face, or one inside an Embedded OpenType wrapper, is loaded and used to draw. A MicroType Express subset (`application/x-fontdata` with neither form) is kept and written back, and the text still uses a stand-in.
- **Impact:** decks that embed a MicroType subset reflow until that face is installed.
- **Estimate:** 3–5 h.

## Features

### Font weights beyond Regular, Bold and Italic (#69)
- **Evidence:** #69; the font database maps a family to four faces.
- **Impact:** Light, Medium and Semibold headings (common in modern templates) render wrong.
- **Estimate:** 3–5 h. **Beta blocker.**

### Image paste fails on Windows (#1414)
- **Evidence:** #53 is closed (PR #88 pastes an image-only clipboard as a picture); #1414 (Ctrl+V fails to paste external images on Windows) is open. Linux was not re-checked after #53 closed.
- **Impact:** copy-paste of screenshots is a core workflow.
- **Estimate:** 2–4 h. **Beta blocker.**

### Charts: data grid, elements and the newer chart types
- **Evidence:** checklist rows "Edit chart data" (P0, partial), "Chart elements" (partial), "Other
  chart types" (partial); `chartex` parts (waterfall, funnel, treemap, sunburst, histogram, Pareto,
  box & whisker) are read, drawn, inserted, edited and saved as `chartEx` (kept unchanged, else
  written from the model, with a plain chart fallback). Waterfalls float and total, funnels,
  treemaps (squarified, nested by level with group banners) and sunbursts (one ring per level)
  have their own layouts, histograms and Paretos bin their data (count, width or Scott's rule),
  and box & whisker charts use exclusive or inclusive quartiles with 1.5 IQR whiskers. Region maps are kept
  (their cached geography too, also through edits) and drawn as value-coloured tiles with a
  colour scale, since DeckCraft ships no map boundaries. Radar charts are drawn polar (filled
  or lines), stock charts as high-low lines with close dashes or up/down bars and saved as
  `c:stockChart`, and combo charts start as columns with a line. Trendlines round-trip
  (`corpus_charts.rs`) and are drawn: linear, exponential, logarithmic, polynomial, power and
  moving average, with forecast and the equation and R² label. A moving average is drawn with
  neither label. A combo chart's secondary value axis is read (`y2_*`,
  `Series.secondary_axis`) and is not written back when the chart is rebuilt.
  An unchanged chart's embedded workbook now survives save (#335); editing the chart still
  drops it rather than writing a stale spreadsheet. The workbook and other retained chart parts
  also survive `.deckcraft` save/reopen (`crates/engine/tests/it/chart_native.rs`), with binary
  payloads in bounded ZIP entries sharing the media load budget (`crates/format` tests).
- **Impact:** most business decks have charts; editing their data is routine.
- **Estimate:** 12–20 h. Data and elements are **beta blockers** (6–10 h).

### SmartArt editing is shallow
- **Evidence:** a SmartArt graphic keeps its outline (`Shape.diagram`) and has a Text Pane, Add
  Shape, Promote, Demote, Remove and layout switching (`crates/engine/src/cmd/smartart.rs`). On
  `.pptx` the outline is written as a real diagram (data, layout, quick style, colours and the
  cached drawing; `crates/pptx/src/dgm.rs`), and PowerPoint diagrams read their outline from the
  data model, so they get the Text Pane too. 15 layouts (bullet and block lists, five process, two cycle and
  a radial, hierarchy, two pyramids, two matrices), four SmartArt Styles and Change Colors
  (colourful or one accent). Still missing: DeckCraft's own layout definitions are simple (not yet
  checked in PowerPoint itself); PowerPoint has ~200 layouts, picture layouts and 3-D styles; editing an
  imported diagram's outline redraws it with DeckCraft's layout; a diagram with no cached drawing
  isn't drawn.
- **Impact:** a PowerPoint diagram edited in DeckCraft changes look.
- **Estimate:** 6–10 h.

### Animation Pane, effect count and preview
Animations ▸ Preview and the pane's Play All play the current slide's animations on the editor
canvas (Escape or a click stops them); with an effect selected the pane's button reads Play From
and starts at that effect. The pane has an advanced timeline with draggable bars
(`crates/ui-egui/src/panels/animtimeline.rs`). Still open: most of the effect gallery. Detail in
[animation-parity.md](animation-parity.md). 7–13 h; the pane is a
**beta blocker**.

### Interactive decks: media-action links, audio-triggered effects and click boxes
- **Evidence:** the slide show plays click-triggered animations, keeps each slide's state across
  visits and ignores shapes an animation has hidden (`crates/anim/src/interact.rs`). Still open,
  found by running three classroom game decks (Jeopardy-style boards, hide-and-reveal, score
  counters): a click on a shape whose action is "play sound" (`ppaction://media`, 126 and 486
  shapes in two decks) does nothing; triggers that start on `onStopAudio` or `begin` conditions are
  not read as triggers; click boxes are the shape's bounding box (a hexagon's corners count) and do
  not follow a shape that a motion path has moved. Trigger sequences now keep their separate
  click steps on PPTX save, including paragraph builds (`crates/pptx/tests/it/trigger_steps.rs`).
- **Impact:** games built around sound effects on buttons, audio-triggered effects or moving
  clickable shapes do not play as authored.
- **Estimate:** 4–8 h.

### Notes and Handout masters
- **Evidence:** View ▸ Handout Master and View ▸ Notes Master edit both masters (placeholders,
  background, a slides-per-page preview for handouts) and PPTX keeps them. Notes pages (view and
  PDF) place the slide and notes where the notes master does and print its header, date, footer
  and page number in their own fonts, sizes and colours; PDF handouts print the handout master's.
  Notes Master view previews the slide in its slide-image placeholder. PDF pages lay master text
  out with the text engine (wrapping, mixed formatting), and the Notes Page view draws the same
  layout. Left: the view draws it in the interface font (sizes, colours and weights kept).
- **Estimate:** done for beta.

### Edit points, text warp, 3-D
- **Evidence:** checklist rows "Edit points" (P1 partial: Edit Points converts a shape to a
  freeform, vertices drag, Ctrl/Cmd-click adds or deletes, smooth/straight/corner points and Bézier
  handles, Add Point, Delete Segment (closed outlines), Straight/Curved Segment and Open/Close Path
  on a segment, and Open/Close Path on a point), "Text warp transforms" (partial: all 40
  `prstTxWarp` presets render, their adjusts round-trip, Shape Format ▸ Text Effects ▸ Transform
  applies one with `format.textWarp`, and a selected warp shows a yellow handle per adjust that
  dragging sets; arc/circle placement can differ from PowerPoint by 1–3 % of the path length),
  "Bevel and 3-D rotation" (partial: bevels, camera presets and rotations and face lighting render
  as approximations and `scene3d`/`sp3d` round-trip as raw XML; extrusions draw their sides under
  turned cameras, each lit for its own normal; the preset light rigs, rig directions and materials
  shade bevels and sides approximately (fitted to PowerPoint for three-point/matte only); contours
  draw as a band round the face outline (not round the extruded silhouette); Format Shape ▸
  Effects has 3-D Format and 3-D Rotation).
- **Estimate:** 1–2 h (fitting the other rigs and materials to PowerPoint renders).

### Proofing: spelling, thesaurus, AutoCorrect
- **Evidence:** `review.spelling` checks with Hunspell-format dictionaries the system already has
  (`crates/engine/src/spell.rs`: affixes, cross products, one level of continuation, compounds,
  case rules, suggestions), each run in its own language (`lang` from PPTX), with a user dictionary
  for Add to Dictionary. Red squiggles on the current slide come from a worker thread, with a
  context menu to correct, ignore or add; table cells and speaker notes are squiggled too, each
  with a right-click menu; repeated words (`the the`) are flagged with Delete Repeated Word;
  Review › Language marks the selection's proofing language or Do Not Check Spelling, or the
  whole presentation's (`format.languageAll`: every run, notes, masters, layouts and the default
  text style) (`lang`/`noProof` round-trip in PPTX). No grammar checking. The web build checks on the UI
  thread a few milliseconds per frame. No dictionaries ship with DeckCraft, so without one
  installed (always on Windows and the web) only a short list of common misspellings is found.
  `review.thesaurus` looks up the selected word (checklist row partial). AutoCorrect has
  PowerPoint's options (two initial capitals, sentence and day capitals, caps lock, fractions, an
  editable replace list), and AutoFormat superscripts ordinals (1st) and starts bulleted and
  numbered lists (`* `, `1. `) as you type; no undo of a single correction. Automatic lists start
  independently of Replace text as you type, and Backspace right after one puts the typed lead
  back, which PowerPoint does not.
- **Estimate:** 1–2 h (bundle openly licensed dictionaries; under 1 h for the AutoFormat option).

### Copying text and cell ranges
- **Evidence:** a copied text range carries no media, so a picture bullet in it is lost when
  pasted into another deck; the writer saves a bullet whose picture is missing as `•`. A copied
  range of table cells is tab-separated text, not a table, so pasting it makes text rather than
  cells.
- **Estimate:** 2–4 h.

### Equations
- **Evidence:** checklist row missing; PR #55 (OMML editor, +10.5k lines) is open.
- **Estimate:** 3–6 h to review, land and round-trip.

### Smaller missing rows
Zoom: live thumbnails, the zoom-in animation and the native `p166:sldZm` element (3–5 h;
`insert.zoom` makes linked pictures with Return to Zoom), reuse slides pane with thumbnails
(1–2 h; `slide.reuseList` picks which slides of a file to insert, #1103), laser pointer from the presenter window (1–2 h),
artistic effects (3–6 h for the other 15 of 22; 7 approximated and round-tripped), remove
background (6–10 h; needs a model or a classical matte), insert screenshot (2–3 h), record
audio/screen and record slide show (8–12 h), compare presentations beyond accepting and rejecting each revision, or all (#850; per-object accept and reject, markers on the slide: 2–4 h), password protection (4–6 h, ECMA-376
agile encryption), ink to text and math beyond Ink to Shape (6–10 h), 3-D models and stock content
(6–10 h; Insert › Icons places a Lucide stroke set).

## UI/UX

Detail in [ui-parity.md](ui-parity.md).

- **No native macOS menu bar.** `menus::menu_tree()` exists but nothing installs it. 3–5 h. **Beta
  blocker.**
- **Format Shape pane thinner than PowerPoint's** (Fill/Line/Effects/Size/Text Box pages, picture
  and texture options, 3-D). 6–10 h. **Beta blocker.**
- **File tab depth** (#76 landed the Backstage): no online templates, cloud places, printer
  choice, video/GIF/CD exports or document protection. 4–8 h for the parts that need no cloud.
- **Window chrome** (#71 macOS controls misaligned, #26 GNOME title bar, #41 system light/dark).
  2–4 h.
- **Icon consistency across Craft apps** (#73, #56). 2–4 h; coordinate in craftrules.
- **Dialogs**: 42 dialogs vs PowerPoint's ~60. In since the d556ff9 count of 27: Font, Paragraph
  and Tabs, Columns, Bullets and Numbering, Find and Replace, Replace Fonts, Action Settings, Size and
  Position, Date and Time, Photo Album (Edit Photo Album reopens it), Create New Theme Colors and
  Create New Theme Fonts (each edits the open master's scheme, not a saved library; theme fonts
  are latin only), Grid Options, AutoCorrect and Paste Special. Format Background picture options
  are pane controls on `design.background`, not another dialog. Still missing: Print, Insert
  Object…. 4–8 h.

## Hardware

Detail in [hardware-parity.md](hardware-parity.md).

- **Presenter view isn't placed on the second display**; the show doesn't pick the projector. 3–5 h.
  **Beta blocker.**
- **CPU slide rasterization** (`vello_cpu`); PowerPoint renders on the GPU. 12–20 h (vello hybrid
  or wgpu path).
- **Software-only video decode**; PowerPoint uses VideoToolbox/Media Foundation. 8–14 h (copy
  FilmCraft's hardware decode).
- **No pen tilt and no pinch-zoom** (it does not keep the point under the pointer). Pen pressure
  scales ink width when the pointer reports it (a mouse or a missing reading stays at the pen
  width; a stroke saved as a flat `0.5` still draws at the pen width). A two-finger twist rotates
  the selected shapes, and a trackpad swipe turns the page when the slide fits the window and
  during the show. 2–4 h.

## Localization

Detail in [localization-parity.md](localization-parity.md).

- **English only, no string catalog** (#15, #30; Ukrainian PR #34 open). PowerPoint for Mac ships
  26 languages. Infrastructure 8–14 h, then 3–16 h per language plus native review. Beta needs the
  infrastructure and at least Simplified Chinese, Spanish, French, German and Japanese (~30–45 h).
- **Hangul in the interface needs an installed Korean font** (#101). craft-fonts has no Korean face
  (Noto CJK is excluded by the asset policy), so the web build draws Korean file names, notes and
  outline text as boxes; desktop borrows Malgun Gothic, Apple SD Gothic Neo or Noto Sans CJK.
  Adding an openly licensed Korean face (e.g. Nanum Gothic, OFL) to craft-fonts: 1–2 h.
- **UI isn't mirrored for right-to-left languages.** 6–10 h.
- **Browser build draws Arabic and Hebrew as missing-glyph boxes in exports** (#81). 1–2 h.

## Performance

- **Baseline measured, not compared.** `cargo xtask bench` (and `deckcraft-cli bench FILE`) times
  open, per-slide render, save, PPTX round trip and PDF export, and peak memory, against a saved
  baseline; show frame rate isn't measured and there are no PowerPoint numbers to set beside it.
  2–4 h.

## Ecosystem

- **Six original starter decks; PowerPoint's large template gallery remains.** File › New and the
  start screen offer Briefing, Lesson, Pitch, Field Notes, Quarterly and Evening Talk
  (`file.new {template}`), each a few slides on one of our themes. Dozens of further offline
  designs, and any online gallery, are still absent. 4–8 h for a larger original set (asset policy:
  our own designs only).
- **No add-ins or VBA.** Out of scope by policy; MCP/CLI automation is the alternative.

## AI features

- **No Designer, Copilot, Speaker Coach, live subtitles or Translate.** All are Microsoft cloud
  services; an offline rule-based Designer is in the catalogue (P2). Local models (speech-to-text
  for subtitles, translation) need an owner decision on licensed models. 20–40 h.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | trivial | Animation effect options display readable labels for every gallery value; the broader Animation Pane gap remains |
| 2026-10-28 | trivial | Automatic lists now run independently of Replace text as you type; proofing gap description updated |
| 2026-10-27 | minor | Refreshed against merged PRs: removed the Morph alignment entry (#67 merged 2026-10-14); #77 (animation rewind, PR #83) is fixed, leaving #74 in the round-trip gap; #75 was closed unmerged, so #60 stays open with no fix PR; #53 is closed and the image-paste gap is now Windows #1414; Reuse Slides (#1103) and Compare reject (#850) noted in the smaller rows; the duplicate header line is gone |
| 2026-10-25 | trivial | New entries: copying text and cell ranges (no media, no table), a control-server timing test that may fail under load; Proofing notes that automatic lists follow Replace text as you type and that Backspace puts the typed lead back |
| 2026-10-24 | trivial | AutoFormat starts bulleted and numbered lists as you type (`* `, `-- `, `> `, `1. `, `a) ` and the rest), with an AutoCorrect option to turn it off |
| 2026-10-22 | minor | Animation Pane ▸ Play From starts the preview at the selected effect; gap 10 keeps the effect gallery (7–13 h) |
| 2026-10-24 | trivial | Columns dialog (number and spacing of text columns); dialog count 41 → 42 |
| 2026-10-20 | trivial | Explicit empty chart number-format sections hide their values; blank whole-code General fallback is retained. Synthetic formatter regressions; chart-element scores and readiness estimates unchanged |
| 2026-10-18 | minor | Native chart payloads use referenced ZIP members and share media byte/item limits; old inline sidecars remain readable. Synthetic tests cover multi-megabyte layout, all shape owners, shared budgets, missing/corrupt members and invalid references. Scores and milestone scope unchanged |
| 2026-10-15 | trivial | Native saves retain chart package parts and relationships, including charts in groups; edited chart data still does not export a stale source workbook. Chart editing gaps and scores remain unchanged |
| 2026-10-15 | trivial | PPTX trigger sequences keep later On Click steps instead of converting them to After Previous; mixed starts, independent buttons and paragraph builds have synthetic round-trip regressions |
| 2026-10-16 | trivial | AutoFormat superscripts ordinals (1st, 22nd) as you type, with an AutoCorrect option to turn it off; automatic bulleted lists remain |
| 2026-10-15 | minor | Edit Points: Straight/Curved Segment on a right-clicked segment and Open/Close Path on a point (`shape.segmentType`, `shape.openPath`, `shape.closePath`); left: Delete Segment and Add Point in the menu |
| 2026-10-15 | trivial | Compare: slide-level Reject and Reject All landed; the entry keeps per-object accept and reject and the slide markers |
| 2026-10-14 | trivial | Fixed #718: remote MCP no longer accepts unrelated or buffered stale success; loopback regressions cover invalid envelopes, incomplete and oversized frames |
| 2026-10-14 | minor | Community PR wave folded: one entry each for the soak tests (engine and UI), the PPTX corpus (seven owned corpus files), edit points / text warp / 3-D, proofing (Hunspell spelling, thesaurus, AutoCorrect), the smaller missing rows, dialogs (41 vs ~60; Print and Insert Object left) and pen/trackpad input; ranked rows 3, 4 and 10 de-duplicated (comment threads #70 and the pane timeline are done); the fixed command-palette entry (#79) removed |
| 2026-10-13 | minor | EMF/WMF rectangle clip regions (`INTERSECTCLIPRECT`, `EXCLUDECLIPRECT`, `EXTSELECTCLIPRGN`) limit later drawing. EMF+ records remain |
| 2026-10-13 | minor | Embedded fonts are read, written and drawn (TrueType, OpenType, Embedded OpenType). The gap is now MicroType Express subsets, which are kept but not drawn |
| 2026-10-13 | minor | WordArt Transform gallery: Shape Format ▸ Text Effects lists the 40 presets and No Transform; `format.textWarp` applies one (adjust handles still open) |
| 2026-10-13 | minor | Edit Points: smooth/straight/corner point types, Bézier handle dragging, right-click point menu, no selection box while editing |
| 2026-10-13 | minor | Edit Points: convert to freeform, drag, add and delete vertices (`shape.editPoints`, `shape.movePoint`, `shape.addPoint`, `shape.deletePoint`); left: point types and control handles |
| 2026-10-13 | minor | Owned PPTX corpus round-trip (parts and text). Notes-master header and footer text is kept; the corpus gap is now a larger set and PowerPoint reference renders |
| 2026-10-13 | minor | Owned PPTX corpus for comments, links and actions, custom shows, rotated groups and inherited placeholders. Paragraph `defRPr` and action sounds kept. Embedded fonts and #70/#74/#77 stay open |
| 2026-10-13 | minor | Owned PPTX corpus for text effects, body layout, theme effects, fills, bevels and connectors. A run's inner shadow, reflection and soft edge, and a bevel preset with no kept 3-D XML, are kept. #70, #74, #77 and embedded fonts stay open |
| 2026-10-13 | minor | Benchmark suite (`cargo xtask bench`, `deckcraft-cli bench`): the performance gap is now frame rate and a PowerPoint comparison |
| 2026-10-14 | minor | Owned corpus `corpus_media.rs`: 3-D model parts and animation bounce end now round-trip. The real-deck corpus and #70/#74/#77 stay open |
| 2026-10-14 | trivial | Corpus slice: themes, masters, effects and links; text Mouse Over actions round-trip |
| 2026-10-14 | trivial | A moving average trendline is drawn without an equation or an R² label |
| 2026-10-14 | minor | Chart trendlines are drawn (six types, forecast, equation and R²). The charts gap stays open for the data grid, chartex and secondary-axis export; the real-deck corpus gap stays open |
| 2026-10-14 | minor | Chart trendlines round-trip. The real-deck corpus gap stays open; secondary-axis export and drawing trendlines stay in the charts gap |
| 2026-10-14 | trivial | 3-D contours |
| 2026-10-14 | trivial | 3-D light rigs and materials; extrusion sides lit per side |
| 2026-10-14 | trivial | 3-D extrusions draw their sides under turned cameras |
| 2026-10-14 | minor | Insert › Icons places Lucide 1.56.0 stroke icons as custom shapes. The checklist row stays missing until 3-D models and stock content exist |
| 2026-10-14 | minor | Size and Position dialog landed (position, size, rotation, scale, lock aspect, one undo). The dialog gap remains Font, Bullets and Numbering, Find/Replace, Replace Fonts, Action Settings, Print, Insert Object and the rest of the ~60 |
| 2026-10-14 | minor | File › New has six original starter decks; a large template gallery is still open |
| 2026-10-14 | minor | An IME composition in a table cell is underlined in place, and the caret and candidate window sit in that cell |
| 2026-10-14 | trivial | Photo Album dialog counted (28 vs ~60). Font, Bullets, Find, Replace Fonts, Action Settings, Print and Insert Object remain |
| 2026-10-14 | trivial | Edit Photo Album reuses the Photo Album dialog (still 28 vs ~60). Font, Bullets, Find, Replace Fonts, Action Settings, Print and Insert Object remain |
| 2026-10-14 | minor | Create New Theme Colors is in (dialogs 27 → 28). Font, Bullets and Numbering, Find/Replace, Replace Fonts, Action Settings, Print and Insert Object stay open |
| 2026-10-14 | minor | Create New Theme Fonts is in (dialogs 27 → 28): heading and body latin on the open master. Font, Bullets and Numbering, Find/Replace, Replace Fonts, Action Settings, Print and Insert Object stay open |
| 2026-10-14 | minor | Insert › Date and Time dialog. Dialog count 27 → 28; Font, Bullets and Numbering, Find/Replace, Replace Fonts, Action Settings, Print and Insert Object are still missing |
| 2026-10-13 | minor | Artistic effects: 7 of 22 (blur, pencil grayscale, line drawing, film grain, cutout, glow edges, photocopy) applied, removed and round-tripped (`a14:imgLayer`) |
| 2026-10-14 | trivial | Notes Page view draws master text wrapped, as the PDF does |
| 2026-10-14 | trivial | PDF master header/footer text wraps and keeps mixed formatting |
| 2026-10-14 | trivial | Notes Master slide preview; master header/footer text keeps its formatting |
| 2026-10-14 | minor | Notes pages and PDF handouts follow the notes and handout masters |
| 2026-10-14 | minor | Notes Master view: the masters gap is now notes pages and PDF handouts following the masters |
| 2026-10-14 | minor | Handout Master view: the masters gap is now the Notes Master view and handout text in PDF |
| 2026-10-13 | minor | Owned PPTX corpus for tables, charts, animations, media, sections and masters. Handout-master text is kept; the corpus gap is now a larger set and PowerPoint reference renders |
| 2026-10-13 | minor | Laser pointer in the slide show (checklist row Partial). Highlighter, eraser and a laser driven from presenter view remain |
| 2026-10-13 | trivial | Command palette skips commands that need a value, so Export opens the dialog (#79) |
| 2026-10-14 | minor | Design ▸ Effects applies a named effect scheme. The checklist row stays P, so breadth stays 79%. Dialogs stay 27 |
| 2026-10-14 | minor | Slide Master Preserve and Title/Footers placeholders. Checklist letters unchanged (breadth stays 79%); dialogs stay 27 |
| 2026-10-14 | minor | Format Background picture and solid options are in the existing pane. Dialogs stay 27 |
| 2026-10-13 | minor | AutoCorrect options: case rules, fractions, ellipsis, a larger built-in list and your own entries (`text.autocorrect`, Tools ▸ AutoCorrect) |
| 2026-10-14 | trivial | Notes spelling menu, Review › Language with Do Not Check Spelling, repeated words |
| 2026-10-14 | trivial | Spelling squiggles in table cells and notes, and on the web |
| 2026-10-14 | minor | Spelling squiggles while typing, Add to Dictionary, per-run languages and compounds |
| 2026-10-14 | minor | Spelling checks with installed Hunspell dictionaries and suggests corrections |
| 2026-10-13 | minor | Animation preview plays the current slide on the editor canvas; the pane gap keeps the timeline, preview from the selected effect, and the remaining effects |
| 2026-10-14 | minor | A two-finger twist rotates the selected shapes. Pinch-zoom is still open |
| 2026-10-14 | minor | A trackpad swipe (and the mouse wheel, in the show) turns the page. Pinch-zoom remains |
| 2026-10-14 | minor | Paragraph dialog sets alignment, indentation, spacing and line spacing in one step; its Tabs… button opens a standalone Tabs dialog |
| 2026-10-14 | minor | Font dialog sets typeface, size, style, underline, colour, strikethrough, baseline, caps and character spacing in one step |
| 2026-10-13 | minor | Grid Options dialog sets spacing, snap and whether gridlines and guides show; the on-screen grid follows that spacing |
| 2026-10-13 | minor | WordArt adjust handles: a selected text warp shows one yellow handle per adjust, and dragging it sets that guide value (gallery and command still open) |
| 2026-10-14 | minor | Pen pressure scales ink width when the pointer reports it; tilt, pinch-zoom and other trackpad gestures remain |
| 2026-10-13 | trivial | Chart workbook round-trip (#335) noted under the charts gap; editing chart data is still open |
| 2026-10-13 | minor | Shape, picture and table rendering checked against PowerPoint on ~150 synthetic edge-case slides; 3-D bevels, cameras and face lighting now render approximately (gap narrowed to extrusion, rigs and the 3-D pane) |
| 2026-10-13 | minor | Interactive decks: triggers, return-to-slide state and click rules landed in the slide show; the remaining pieces are one new entry under Features |
| 2026-10-12 | minor | IME composition in slide text and notes, and CJK interface text with real glyphs (#101): the IME gap is now Korean on the web and table-cell underlines |
| 2026-10-12 | minor | EMF/WMF text, pattern and hatch brushes drawn; legacy OLE and ActiveX previews shown; the SVG gap keeps only EMF clip regions |
| 2026-10-12 | minor | Text warps render (all 40 presets, adjusts round-trip); the warp gap is now the gallery, command and handles |
| 2026-10-11 | minor | Removed "Tab inside a text box ends text entry" (#78, fixed) |
| 2026-10-11 | minor | Animated GIF export landed (#171); the entry now covers video export and animating transitions in the GIF |
| 2026-10-10 | minor | Noted that no gap blocks alpha (core-workflow gate passes) |
| 2026-10-10 | major | Created: every gap from the 2026-10-10 re-measure and the 16 open GitHub issues, ranked, with beta blockers marked |
