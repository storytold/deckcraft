# Where DeckCraft falls short of PowerPoint

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (localization gap narrowed after Ukrainian UI) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

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
| 1 | PPTX never tested against a corpus of real decks | File format | B | 20–35 | [file-format-parity.md](file-format-parity.md) |
| 2 | Data loss and crashes on common paths (#60, #40, #59) | Stability | B | 6–12 | this file |
| 3 | PPTX round trip drops comment threads, media options, rewind (#70, #74, #77) | File format | B | 3–6 | [file-format-parity.md](file-format-parity.md) |
| 4 | EMF/WMF and SVG pictures don't render | File format | B | 8–14 | [file-format-parity.md](file-format-parity.md) |
| 5 | Target languages beyond English missing; Ukrainian UI available | Localization | B (5 languages) | 55–90 | [localization-parity.md](localization-parity.md) |
| 6 | No printing | Feature | B | 5–8 | [file-format-parity.md](file-format-parity.md) |
| 7 | Fonts: only Regular, Bold and Italic faces (#69) | Feature | B | 3–5 | [ui-parity.md](ui-parity.md) |
| 8 | Image paste fails on Windows and Linux (#53) | Feature | B | 2–4 | [ui-parity.md](ui-parity.md) |
| 9 | Presenter view isn't placed on the second display | Hardware | B | 3–5 | [slideshow-parity.md](slideshow-parity.md) |
| 10 | Animation Pane: no timeline, thin preview; 64 of ~190 effects | Feature | B (pane) | 10–16 | [animation-parity.md](animation-parity.md) |
| 11 | Charts: data grid editing, elements, `chartex` types | Feature | B (data, elements) | 12–20 | [parity-checklist.md](parity-checklist.md) |
| 12 | Format Shape pane thinner than PowerPoint's | UI/UX | B | 6–10 | [ui-parity.md](ui-parity.md) |
| 13 | No native macOS menu bar | UI/UX | B | 3–5 | [ui-parity.md](ui-parity.md) |
| 14 | PDF export rasterizes slide artwork | File format | B | 5–8 | [file-format-parity.md](file-format-parity.md) |
| 15 | Slides rasterize on the CPU; no performance baseline | Performance / hardware | | 15–30 | [hardware-parity.md](hardware-parity.md) |

## Stability

### Close discards an edited presentation without asking (#60)
- **Evidence:** issue #60; quitting prompts, `file.close` doesn't. PR #75 is open.
- **Impact:** silent data loss on a common path. The worst kind of bug for a presentation app.
- **Estimate:** 0.5–1 h (land and test #75). **Beta blocker.**

### Startup crashes on some Windows GPUs (#40, #59)
- **Evidence:** #59 (Intel UHD, access violation in `igvk64.dll` during Vulkan enumeration), #40
  (WordCraft, GridCraft and DeckCraft all fail on one Windows 11 PC). The DX12 default (#16) fixed
  AMD's `atio6axx.dll` crash only.
- **Impact:** the app never opens for those users.
- **Estimate:** 4–8 h (backend probing outside the process or a safe-mode fallback, a software
  renderer path). Needs a tester with the hardware. **Beta blocker.**

### No soak or long-session testing
- **Evidence:** unit tests (504), PPTX malformed/fuzz tests and the command panic guard exist; no
  random command sequences over big decks, no memory or undo-depth soak.
- **Impact:** unknown crash and slowdown rate in long sessions.
- **Estimate:** 4–8 h.

## File formats

Detail in [file-format-parity.md](file-format-parity.md).

### PPTX has never met a corpus of real decks
- **Evidence:** every PPTX test (`crates/pptx/tests`, 30 tests) uses decks DeckCraft generates;
  the 2026-10-07 alpha checklist already listed this as open. No reference renders from PowerPoint
  exist to compare against.
- **Impact:** the main reason to switch is "open my PowerPoint decks". Fidelity on real decks
  (corporate templates, SmartArt, `chartex` charts, embedded fonts, EMF) is unknown.
- **Estimate:** 20–35 h: build an owned corpus in `storytold/photocraft-corpus` style (craftrules
  `standards/test-corpora.md`), add import → render → export → re-import checks, have the owner open
  outputs in PowerPoint, fix what breaks. **Beta blocker.**

### Round trip loses comment threads, media playback options and animation rewind (#70, #74, #77)
- **Evidence:** issues filed 2026-10-10 by an automated reviewer; the writer emits legacy comments
  only (no `p188` modern comments with replies and resolved state, though the reader understands
  them); media playback options and the animation rewind flag aren't written back.
- **Impact:** a deck opened and saved silently loses review state and show behaviour.
- **Estimate:** 3–6 h. **Beta blocker.**

### EMF/WMF and SVG pictures don't render
- **Evidence:** `crates/render/src/images.rs` decodes through `image` 0.25 (PNG, JPEG, WebP, GIF,
  TIFF, BMP); nothing reads EMF, WMF or SVG. Decks from Windows often hold EMF (pasted charts,
  Visio, logos); PowerPoint's Insert Icons stores SVG with a PNG fallback.
- **Impact:** blank pictures in real decks; SVG icons show only their raster fallback, and SVG files
  can't be inserted.
- **Estimate:** 8–14 h (an SVG renderer through `resvg`-style crates, an EMF/WMF record player for
  the common records). **Beta blocker** for EMF.

### Template, slide-show and macro variants are saved as plain `.pptx`
- **Evidence:** `crates/pptx/src/write/mod.rs` always writes the
  `presentationml.presentation.main+xml` content type; `.potx`/`.ppsx` open, `.pptm`/`.potm`/`.ppsm`
  aren't recognised.
- **Impact:** a template saved as `.potx` isn't a template to PowerPoint; macro decks can't be
  opened even to view.
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

### No video or animated GIF export
- **Evidence:** checklist row missing; PowerPoint exports MP4/MOV and animated GIF.
- **Impact:** sharing a talk as a video is a common request.
- **Estimate:** 10–16 h (render the timeline frame by frame; an H.264 encoder is the hard part,
  FilmCraft has one to copy).

### Embedded fonts aren't read or written
- **Evidence:** no `p:embeddedFontLst` handling in `crates/pptx`.
- **Impact:** decks that rely on embedded fonts reflow.
- **Estimate:** 3–5 h.

## Features

### Font weights beyond Regular, Bold and Italic (#69)
- **Evidence:** #69; the font database maps a family to four faces.
- **Impact:** Light, Medium and Semibold headings (common in modern templates) render wrong.
- **Estimate:** 3–5 h. **Beta blocker.**

### Image paste fails on Windows and Linux (#53)
- **Evidence:** #53 (v0.3.0, Fedora AppImage and Windows).
- **Impact:** copy-paste of screenshots is a core workflow.
- **Estimate:** 2–4 h. **Beta blocker.**

### Tab inside a text box ends text entry (#78)
- **Evidence:** #78.
- **Impact:** tab stops and indented lists can't be typed.
- **Estimate:** 1–2 h.

### Charts: data grid, elements and the newer chart types
- **Evidence:** checklist rows "Edit chart data" (P0, partial), "Chart elements" (partial), "Other
  chart types" (partial); `chartex` parts (waterfall, funnel, treemap, sunburst, histogram, box &
  whisker) aren't read.
- **Impact:** most business decks have charts; editing their data is routine.
- **Estimate:** 12–20 h. Data and elements are **beta blockers** (6–10 h).

### SmartArt is pre-drawn shapes only
- **Evidence:** `crates/pptx/src/read/shapes.rs` reads SmartArt's drawing part as a group; no text
  pane, no layout switching, the diagram data isn't regenerated.
- **Impact:** editing a SmartArt diagram from PowerPoint breaks it.
- **Estimate:** 10–16 h.

### Animation Pane, effect count and preview
Detail in [animation-parity.md](animation-parity.md). 10–16 h; the pane is a **beta blocker**.

### Notes and Handout masters
- **Evidence:** checklist row missing; PPTX writes a notes master but there's no view to edit it,
  and no handout master at all.
- **Estimate:** 4–6 h.

### Edit points, text warp, 3-D
- **Evidence:** checklist rows "Edit points" (P1 missing), "Text warp transforms", "Bevel and 3-D
  rotation" (missing; `scene3d`/`sp3d` kept as raw XML on round trip).
- **Estimate:** 4–6 h (edit points), 6–10 h (warp), 8–12 h (3-D render approximation).

### Proofing: spelling, thesaurus, AutoCorrect
- **Evidence:** `review.spelling` checks a small built-in list of frequent misspellings
  (`crates/engine/src/cmd/review.rs`); no dictionaries, no thesaurus, no AutoCorrect.
- **Estimate:** 8–14 h (Hunspell-format dictionaries under an open licence per language).

### Equations
- **Evidence:** checklist row missing; PR #55 (OMML editor, +10.5k lines) is open.
- **Estimate:** 3–6 h to review, land and round-trip.

### Smaller missing rows
Zoom (summary/section/slide, 6–10 h), reuse slides (2–4 h), laser pointer (1–2 h), transition
sounds (1–2 h), artistic effects (6–10 h), remove background (6–10 h; needs a model or a classical
matte), compress pictures (2–3 h), insert screenshot (2–3 h), record audio/screen and record slide
show (8–12 h), compare presentations (6–10 h), password protection (4–6 h, ECMA-376 agile
encryption), ink to shape/text (6–10 h), icons and 3-D models (8–14 h, needs an open icon set).

## UI/UX

Detail in [ui-parity.md](ui-parity.md).

- **No native macOS menu bar.** `menus::menu_tree()` exists but nothing installs it. 3–5 h. **Beta
  blocker.**
- **Format Shape pane thinner than PowerPoint's** (Fill/Line/Effects/Size/Text Box pages, picture
  and texture options, 3-D). 6–10 h. **Beta blocker.**
- **First-run and start screen** (#17, #76: missing File overlay, broken layout; recent files and
  templates thin). 3–5 h.
- **Window chrome** (#71 macOS controls misaligned, #26 GNOME title bar, #41 system light/dark).
  2–4 h.
- **Icon consistency across Craft apps** (#73, #56). 2–4 h; coordinate in craftrules.
- **Dialogs**: 27 dialogs vs PowerPoint's ~60 (Font, Bullets and Numbering, Find/Replace dialog,
  Replace Fonts, Action Settings, Print, Insert Object…). 8–14 h.
- **Command palette defaults to an Export entry that fails** (#79). 0.5 h.

## Hardware

Detail in [hardware-parity.md](hardware-parity.md).

- **Presenter view isn't placed on the second display**; the show doesn't pick the projector. 3–5 h.
  **Beta blocker.**
- **CPU slide rasterization** (`vello_cpu`); PowerPoint renders on the GPU. 12–20 h (vello hybrid
  or wgpu path).
- **Software-only video decode**; PowerPoint uses VideoToolbox/Media Foundation. 8–14 h (copy
  FilmCraft's hardware decode).
- **No pen pressure or tilt for ink**; no pinch-zoom or trackpad gestures. 3–6 h.

## Localization

Detail in [localization-parity.md](localization-parity.md).

- **Target languages beyond English missing.** English and Ukrainian now have a UI catalog and
  system-language selection (#15, #30). PowerPoint for Mac ships 26 languages. Additional
  translations need 3–16 h per language plus native review. Beta needs at least Simplified
  Chinese, Spanish, French, German and Japanese (~30–45 h).
- **No IME composition in text editing** (CJK input). 3–6 h.
- **UI isn't mirrored for right-to-left languages.** 6–10 h.
- **Browser build draws Arabic and Hebrew as missing-glyph boxes in exports** (#81). 1–2 h.

## Performance

- **No baseline.** Nothing measures open time, render time per slide, show frame rate or memory
  against PowerPoint. 4–8 h for a benchmark suite in `xtask`.
- **Morph aligns outlines every frame** (#67, PR open). 1 h.

## Ecosystem

- **8 original themes, no template gallery.** PowerPoint offers dozens of designs online and
  offline. 8–14 h for a larger original set (asset policy: our own designs only).
- **No add-ins or VBA.** Out of scope by policy; MCP/CLI automation is the alternative.

## AI features

- **No Designer, Copilot, Speaker Coach, live subtitles or Translate.** All are Microsoft cloud
  services; an offline rule-based Designer is in the catalogue (P2). Local models (speech-to-text
  for subtitles, translation) need an owner decision on licensed models. 20–40 h.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Narrowed localization gap: Ukrainian UI catalog and language selection exist; other target languages, IME and RTL UI remain |
| 2026-10-10 | minor | Noted that no gap blocks alpha (core-workflow gate passes) |
| 2026-10-10 | major | Created: every gap from the 2026-10-10 re-measure and the 16 open GitHub issues, ranked, with beta blockers marked |
