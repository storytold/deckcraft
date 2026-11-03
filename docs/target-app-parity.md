# DeckCraft vs Microsoft PowerPoint: parity assessment

> **PPTX.59 update 2026-11-03:** PDF text extraction now preserves the source character index across authored hard breaks. All eight retained multiline identity failures are fixed; 661 unique rendered pages remain pixel-identical, and all 253 diagnostic text identities now match native output. Broader SmartArt, chart, map and LibreOffice parity remains open; feature scores are unchanged.

> **PPTX.52 update 2026-11-02:** Independent text-rule stages restore local footprints before preset/rotation measurement, and numeric font fitting uses fine diagram advances. Reversed centered fitting retains the final trial and partial-placement state. The combined 2,300-case matrix improves from 2,096 to 2,145 strict matches with no lost prior match. A rotated raw-box metric worsens while its actual painted bounds improve to 0.008pt; both are retained. The 155 remaining strict failures and wider map/chart/text limits remain open; feature scores are unchanged.

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-22 · **Change:** trivial (no break after `?`, `!`, `}`, `+` inside a word; previous: trivial, Animations shortfall: preview from the selected effect is in; before that: major, community PR wave 2026-10-14: breadth 84% over 191 features (P0 92%, P1 82%, P2 55%) after `cargo xtask parity`; area table and What works today folded to one row or bullet each; 320 engine commands; ready ~55% and stage alpha unchanged; previous: minor, hours to ~95% per audience added) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

The authoritative answer to "how close is DeckCraft to PowerPoint, and how much work is left?".
[ROADMAP.md](../ROADMAP.md) summarizes it; [gaps.md](gaps.md) itemizes every shortfall;
[parity-checklist.md](parity-checklist.md) is the row-by-row feature checklist behind the breadth
number.

## Headline

| Number | Value | Kind |
|---|---|---|
| **Feature breadth** (weighted checklist) | **84%** over 191 features (P0 92%, P1 82%, P2 55%) | **measured**: `cargo xtask parity` formula over [parity-checklist.md](parity-checklist.md); rows re-scored by the 2026-10-14 community PR wave |
| Menu-bar coverage | **59%** (128 of 217 app-specific PowerPoint menu items have a DeckCraft command or UI label of the same name) | **measured** (heuristic lower bound): script matching the 2026-10-07 PowerPoint menu dump against the 226 engine command labels of the d556ff9 snapshot (not re-measured since; the registry is 328 now) and the UI's strings; excludes the app, Window and Help menus and the subtitle language lists |
| **Ready for real work** | **~55%** | **estimated**: weighted dimension table below (was ~56% until the features row was recomputed from written area weights) |
| **Mainstream practitioner** | **~53%** | **estimated**: [method below](#mainstream-practitioner-and-essentials-user) |
| **Essentials user** | **~62%** | **estimated**: [method below](#mainstream-practitioner-and-essentials-user) |
| Stage | **alpha** (beta is ~20 points and ~120–210 h away) | judgement: ~55% is in the alpha band and all six core workflows pass the [alpha gate](roadmap.md#alpha-gate-powerpoints-core-workflows) |
| Remaining effort to beta | **~120–210 Opus 5.5 agent hours** | estimated, itemized in [roadmap.md](roadmap.md) |
| Remaining effort to full parity | **~365–650 Opus 5.5 agent hours** | estimated, by dimension below |

The menu figure is lower than the checklist figure because PowerPoint's menu bar lists every chart
type, SmartArt category, action button and media source as its own item, and many DeckCraft
equivalents are one parameterized command (`insert.chart {type}`) that the label match cannot see.
Read it as a floor, not as a second breadth number.

## Readiness by audience

| Audience | Ready | Opus 5.5 agent hours to ~95% | Work that dominates |
|---|---:|---|---|
| Full target (ready for real work) | ~55% | 340–600 (~60% parallelizes: ~120–210 h wall clock with 3–4 agents) | Feature depth across all 20 areas, legacy `.ppt`/`.odp` and video export, localization in 11+ languages, GPU raster and hardware decode, AI features |
| Mainstream practitioner | ~53% | 150–250 (~50% parallelizes: ~70–120 h with 3 agents) | `.pptx` exchange (real-deck corpus, round-trip losses, EMF/SVG, embedded fonts) 45–75 h; depth in charts, animations, pictures, text, presenting, print/vector PDF 70–110 h; interaction (menu bar, Format Shape pane, font faces, dialogs) 25–40 h; stability 10–20 h |
| Essentials user | ~62% | 50–90 (~40% parallelizes: ~30–55 h with 2–3 agents) | Opening received decks (corpus subset, EMF, SVG icons) 25–45 h; Windows launch fallback and Close prompt 6–12 h; font faces, Tab, paste, print, show navigation 12–20 h; start screen, menu bar, palette 6–10 h |

Calibration is the same as for the full number (see [Calibration](#calibration-how-the-hours-were-derived)): a fix is 0.5–1 h, a checklist row to PowerPoint depth 2–4 h, a subsystem 6–35 h. The sets are nested, so essentials ≤ mainstream ≤ full. The full number is an additive weighted sum over the dimension table; only mainstream and essentials use multiplicative discounts.

## Target and how it was measured

- **Target:** Microsoft PowerPoint for Mac **16.113.4** (build 16.113.26100421), the Microsoft 365
  build installed at `/Applications/Microsoft PowerPoint.app`. PowerPoint for Windows has a few
  more features (Ink Replay, OLE editing, more export types, Hindi and Vietnamese UI); where it
  matters this document says so.
- **The installed app, black-box** (AGENTS.md §2: names and listings only):
  - `Info.plist` `CFBundleDocumentTypes`: the formats PowerPoint opens and saves (13 roles; table in
    [file-format-parity.md](file-format-parity.md)).
  - `Contents/Resources/*.lproj`: 30 localizations plus Base (26 languages and 4 regional variants; table in
    [localization-parity.md](localization-parity.md)).
  - The menu-bar dump `plan/powerpoint/menu-tree.txt` (491 lines, observed 2026-10-07, local and
    gitignored) and the ribbon/pane observations in `plan/powerpoint/01-observed-ui.md`.
- **The code on `origin/main` at d556ff9:** 226 engine commands (`crates/engine/src/cmd/*`), 48
  transitions and 64 animation effects (`crates/model/src/anim.rs`), 504 `#[test]` functions, the
  PPTX reader/writer (`crates/pptx`, 9.0k lines), 27 dialogs and 10 ribbon tabs
  (`crates/ui-egui`).
- **User reports:** 24 GitHub issues opened 2026-10-08 to 10-10 (16 still open), the best evidence
  of depth we have.
- **Not measured:** rendering fidelity against PowerPoint (no reference renders or real-deck corpus
  exist yet), performance against PowerPoint, and any workflow end to end with a real user's deck.

## By dimension

"Ready" is how close each dimension is to what a professional who presents with PowerPoint needs.
The weights are what those users actually depend on, written down so the total can be recomputed.

| Dimension | Weight | Ready | Hours to full | Doc | Evidence |
|---|---:|---:|---|---|---|
| Features (breadth × depth) | 30% | ~62% | 140–240 | [parity-checklist.md](parity-checklist.md), [animation-parity.md](animation-parity.md), [slideshow-parity.md](slideshow-parity.md) | Breadth 84% measured. Depth lags: partial rows remain, and users found depth bugs in rows scored done within two days (#53, #60, #69, #70, #74, #77, #78) |
| File formats | 20% | ~58% | 60–110 | [file-format-parity.md](file-format-parity.md) | PPTX reads and writes every main part and keeps unknown objects; files open in PowerPoint without repair. But it has never met a corpus of real decks, EMF/WMF/SVG pictures don't decode, comments and media settings are lost on round trip, and there is no `.ppt`, `.pptm`, `.odp`, video or GIF export |
| UI/UX fidelity | 15% | ~60% | 30–50 | [ui-parity.md](ui-parity.md) | All 10 ribbon tabs and the contextual tabs exist; Format Shape pane, Animation Pane, dialogs and menus are thinner than PowerPoint's; no native macOS menu bar; user reports #17, #71, #73, #76 |
| Stability | 10% | ~60% | 10–20 | [gaps.md](gaps.md) | Never-crash rules, a panic guard round every command, PPTX fuzz/malformed tests. But startup crashes on some Windows GPUs (#40, #59) and Close discards edits without asking (#60) |
| Performance | 5% | ~55% | 10–20 | [hardware-parity.md](hardware-parity.md) | Unmeasured against PowerPoint. Slides rasterize on the CPU (`vello_cpu`); Morph aligns outlines once per transition (#67) |
| Hardware | 5% | ~40% | 25–45 | [hardware-parity.md](hardware-parity.md) | GPU-composited window (wgpu), but CPU slide raster, software-only video decode, presenter view not placed on the second display, no pen tilt, no pinch zoom |
| Platforms | 5% | ~80% | 5–10 | [ROADMAP.md](../ROADMAP.md) | Ahead in reach: macOS, Windows x64/x86/arm64, Linux (5 formats), FreeBSD and the web, all from one release workflow. Held back by the Windows GPU crashes and the Windows image-paste gap (#1414; Linux not re-checked) |
| Localization | 5% | ~8% | 55–90 + native review | [localization-parity.md](localization-parity.md) | English only; no string catalog. PowerPoint for Mac ships 26 languages. Bidi and Arabic shaping in slide text exist |
| Ecosystem (templates, add-ins) | 3% | ~25% | 10–20 | [gaps.md](gaps.md) | 8 original themes and 6 original starter decks; PowerPoint's large template gallery remains; no add-ins or VBA (out of scope by policy) |
| AI features | 2% | ~10% | 20–40 (owner decision on models) | [gaps.md](gaps.md) | No Designer, Copilot, Speaker Coach, live subtitles or Translate (all Microsoft cloud services). Agents drive every command over MCP instead |
| **Total** | 100% | **~55%** (Σ weight × ready = 55.3) | **~365–650** | | |
| Automation (✱, not weighted) | | ahead | 0 | [mcp.md](mcp.md), [control-protocol.md](control-protocol.md) | Every command is scriptable from the CLI, a JSON control channel and MCP, headless or against the running app; PowerPoint for Mac has AppleScript and VBA only |

About 60% of the hours parallelize across crates (formats, render, text, UI, media, localization
catalogs). Needs a human: native-speaker review of every translation, the owner's decision on AI
models and on legacy `.ppt` import, running PowerPoint to make reference renders and to confirm
round-tripped files open cleanly, and hardware we don't have (Windows GPUs from #40/#59, pens,
multi-display rigs).

## Mainstream practitioner and essentials user

Two narrower readings of "ready", computed the same way in every Craft app. The stage still follows the full number above and the alpha gate.

### Mainstream practitioner: ~53%

The typical professional who builds and presents decks every week: a consultant, teacher, sales or
product person. Left out: add-ins, Copilot/Designer and other cloud AI, co-authoring and admin
features, pens and recording hardware, and languages other than the user's own. Depth here is the
depth of the parts of each area this user touches (common chart types, the effects people use,
presenting rather than recording), so it's higher than the full-area numbers; exchange with
PowerPoint is taken out of the areas and charged once, as a discount.

| Area (weekly use) | Weight | Depth | Evidence |
|---|---:|---:|---|
| Text: typing, fonts, paragraphs, bullets | 18 | 70 | Every Home-tab control; font weights (#69), Tab in text boxes (#78) |
| Slides, layouts, sections, views | 10 | 82 | Checklist 90% breadth in both areas |
| Masters, themes, backgrounds | 8 | 75 | Slide Master view, 8 themes; Notes/Handout masters rarely used |
| Shapes, format, arrange | 14 | 75 | Presets, connectors, smart guides; Format Shape pane thinner |
| Pictures: insert, crop, styles | 8 | 62 | Works from file; paste fails on Windows (#1414); crop partial |
| Tables | 5 | 78 | Styles, merge/split, grow-to-fit |
| Charts: column, bar, line, pie with data | 6 | 58 | Chart Data dialog; elements partial |
| Transitions and common animations | 8 | 76 | 48/48 transitions; effects cover ~85% of use; pane has no timeline |
| Presenting: show, presenter view | 10 | 70 | Works; not auto-placed on the second display; no laser |
| Audio and video on slides | 3 | 62 | Plays; slow video in the show on Linux (#91) |
| Comments | 3 | 60 | Threads, resolve |
| Undo and clipboard | 4 | 75 | Unlimited undo; Paste Special partial |
| Open, save, PDF export | 3 | 65 | No print; raster PDF artwork |
| **Weighted depth** | 100 | **71.3** | |

| Discount | Factor | Evidence |
|---|---:|---|
| Interaction fidelity | ×0.92 | No native macOS menu bar, thinner Format Shape pane, Tab ends text entry (#78), font faces (#69), window chrome (#71) |
| Stability on real machines | ×0.92 | Startup crashes on some Windows GPUs (#40, #59); Close discards edits (#60; PR #75 was closed unmerged); video stalls in the editor on Ubuntu (#91); arrows reported dead in the show on Linux Mint (#17) |
| File exchange with PowerPoint users | ×0.88 | Harsher than VectorCraft's ×0.92 because swapping `.pptx` is daily work for this user: never tested on real decks, EMF pictures blank, icons from PowerPoint don't come through (#56), comment threads and media options lost on save (#70, #74) |

71.3 × 0.92 × 0.92 × 0.88 = **53.1 → ~53%**. This is about the same as the full number, unlike
apps where mainstream comes out well above it. The dimensions mainstream leaves out
(localization, ecosystem, AI) weigh only 10% of DeckCraft's full number, and the biggest penalty,
`.pptx` exchange, hits every mainstream user.

### Essentials user: ~62%

Someone who makes a few simple decks: open or start a deck, type, add pictures, present, export
a PDF. Leaves out advanced options, pro workflows and exchange edge cases, plus everything the
mainstream number leaves out.

| Core feature | Weight | Depth | Evidence |
|---|---:|---:|---|
| Start a deck from a theme (start screen) | 8 | 70 | Works; layout problems on the start screen (#17, #76) |
| Type titles and bullets, basic formatting | 20 | 78 | Works; #69, #78 |
| New slide, layout, reorder, duplicate | 12 | 88 | Works |
| Insert a picture | 10 | 65 | From file yes; paste fails on Windows (#1414) |
| Basic shapes and text boxes | 8 | 85 | Works |
| Theme colours and background | 6 | 80 | Works |
| Transitions | 6 | 85 | Works |
| Simple animations | 5 | 80 | Works |
| Present full screen and move through slides | 10 | 85 | Works; #17 on Linux Mint |
| Undo / redo | 5 | 95 | Works |
| Save and reopen | 6 | 85 | Works; Close doesn't ask to save (#60) |
| Export PDF / print | 4 | 70 | PDF yes, print no |
| **Weighted depth** | 100 | **80.1** | |

| Discount | Factor | Evidence |
|---|---:|---|
| Launch and stability | ×0.90 | Some Windows PCs can't start the app (#40, #59), and the workaround is an environment variable; Close loses work (#60) |
| Discoverability and UI clarity | ×0.95 | Familiar ribbon and tabs; no native menu bar, start-screen glitches |
| Opening files people send them | ×0.90 | `.pptx` opens, but real decks are untested and EMF pictures and PowerPoint icons can come in blank (#56) |

80.1 × 0.90 × 0.95 × 0.90 = **61.6 → ~62%**.

### User evidence (GitHub, 2026-10-10, maintainers excluded)

- **38 issues** from 22 reporters and **48 PRs** by people other than the owner (bidi/Arabic,
  Morph, tables, logging, packaging, a Gentoo overlay; open fix PRs for #53, #64, #70, #72, #74,
  #77). **Praise threads or "switched from PowerPoint" reports: 0**
  so far; the only comment threads are "same here" confirmations (#17, #33).
- **Open issues (27):** 13 on the core path (#91 video playback, #60 Close, #59 and #40 startup
  crashes, #53 image paste, #78 Tab in text, #69 font faces, #70/#74/#77 `.pptx` round trip, #56
  icons from PowerPoint, #17 show navigation and file menu, #76 start screen), 14 polish or niche
  (#81 and #64 browser exports, #79 palette, #72 browser comment timestamps, #73 icon consistency,
  #71 and #26 window chrome, #41 system theme, #32 text transparency, #18 desktop shortcut, #15 and
  #30 languages, #58 README link, #50 DMG signature).
- Reading: people try DeckCraft for real decks and hit the core path. Nobody has reported
  replacing PowerPoint yet, which fits ~53–55%.

## By feature area

Breadth is measured from the checklist; "ready" adds depth and is estimated. Hours are to
PowerPoint parity for that area. **Weight** is how much a PowerPoint user relies on the area
(points out of 110); the total "ready" is the weighted mean of the rows (6,905 / 110 = 62.8%; the
Features row of the dimension table keeps ~62% until the next full re-measure). Automation and Platforms are scored elsewhere.

| Area | Weight | Breadth (measured) | Ready (est.) | Hours | Main shortfalls | Doc |
|---|---:|---:|---:|---|---|---|
| Application shell | 5 | 81% | ~55% | 8–14 | No native macOS menu bar, contextual tabs and multiple-window handling thinner than PowerPoint's | [ui-parity.md](ui-parity.md) |
| Files (commands) | 4 | 75% | ~50% | see file formats | Print, passwords, legacy and macro formats, video export; reuse slides is partial | [file-format-parity.md](file-format-parity.md) |
| Slides and sections | 6 | 92% | ~85% | 3–5 | Zoom (summary/section/slide) is partial, copying slides between decks keeps formatting only partly | [parity-checklist.md](parity-checklist.md) |
| Views | 4 | 98% | ~80% | 3–6 | Arrange All (Grid Options and New Window are in) | [ui-parity.md](ui-parity.md) |
| Masters and themes | 7 | 92% | ~70% | 8–14 | Notes/Handout master view polish, effect-scheme depth, Designer suggestions | [parity-checklist.md](parity-checklist.md) |
| Shapes | 7 | 91% | ~75% | 8–14 | Edit points depth, WordArt depth (the Transform gallery and warp adjust handles are in) | [ui-parity.md](ui-parity.md) |
| Format (fill, line, effects) | 6 | 83% | ~65% | 8–14 | Format Shape pane depth, bevel and 3-D, picture/texture fill options | [ui-parity.md](ui-parity.md) |
| Arrange | 3 | 100% | ~90% | 1–2 | More Rotation Options | [ui-parity.md](ui-parity.md) |
| Text | 15 | 83% | ~62% | 18–30 | Font weights beyond Regular/Bold/Italic (#69), true vertical CJK, dictionaries, AutoFormat as you type, thesaurus depth, equations (PR #55 open) | [localization-parity.md](localization-parity.md) |
| Tables | 5 | 97% | ~75% | 3–6 | Draw table depth, borders and cell effects depth | [parity-checklist.md](parity-checklist.md) |
| Charts | 7 | 65% | ~45% | 12–20 | Data grid editing, chart elements, the `chartex` types (waterfall, funnel, treemap, sunburst, histogram, box) | [parity-checklist.md](parity-checklist.md) |
| SmartArt | 4 | 50% | ~35% | 8–14 | PowerPoint diagrams editable (read their data model), the data model on save, more layouts | [parity-checklist.md](parity-checklist.md) |
| Pictures | 7 | 60% | ~45% | 12–20 | SVG, EMF+ records, clipboard paste on Windows (#1414), remove background, artistic-effect and correction depth | [file-format-parity.md](file-format-parity.md) |
| Media | 4 | 67% | ~60% | 8–14 | WMA/WMV decode, record audio/screen, 3-D models and stock content, playback options lost on PPTX round trip (#74) | [file-format-parity.md](file-format-parity.md) |
| Transitions | 4 | 100% | ~80% | 4–8 | 3-D transitions approximated in 2-D | [animation-parity.md](animation-parity.md) |
| Animations | 6 | 85% | ~65% | 10–16 | 64 of PowerPoint's ~190 effects | [animation-parity.md](animation-parity.md) |
| Slide show and presenter view | 8 | 68% | ~60% | 10–16 | Second-display placement, laser from presenter view, recording, subtitles, ink depth | [slideshow-parity.md](slideshow-parity.md) |
| Review | 3 | 70% | ~45% | 8–14 | Compare depth, version history, real spelling dictionaries | [file-format-parity.md](file-format-parity.md) |
| Draw (ink) | 2 | 50% | ~30% | 5–9 | Pen tilt, ink to math, lasso, ink to shape/text depth | [hardware-parity.md](hardware-parity.md) |
| Undo and clipboard | 3 | 100% | ~75% | 3–5 | Image paste on Windows (#1414) | [ui-parity.md](ui-parity.md) |
| Automation ✱ | — | 100% | ahead | 0 | | [mcp.md](mcp.md) |
| Platforms ✱ | — | 67% | ~80% | see platforms | | [ROADMAP.md](../ROADMAP.md) |
| **Total (feature areas)** | 110 | **84%** | **~62%** | **~140–240** | | |

## What works today (evidence)

Carried from the 2026-10-07 ROADMAP status and updated with what has landed since.

- **Document model:** presentations, slides, masters and 11 standard layouts with placeholder
  inheritance (slide → layout → master → theme), sections, notes, comments, custom shows,
  header/footer fields, embedded media, unlimited undo on shared slide snapshots.
- **Themes:** 8 original themes with colour and font schemes, custom colours and fonts
  (Design › Colors › Customize Colors… and Design › Fonts › Customize Fonts… open Create New
  Theme Colors and Create New Theme Fonts on the open master: its twelve theme colours, its
  heading and body latin faces; one scheme on that master, not a library of saved schemes),
  effect schemes (Design ▸ Effects: DeckCraft, Soft, Crisp, Lift, Glow, Plain), backgrounds
  (solid, gradient, picture, pattern; a solid fill's transparency; picture transparency, tile,
  offset, scale, alignment and mirror; Apply to All copies a picture), slide size presets. File ›
  New also offers 6 original starter decks (Briefing, Lesson, Pitch, Field Notes, Quarterly,
  Evening Talk).
- **Shapes:** ~180 preset geometries with adjustment handles, text boxes, pictures (crop,
  corrections, animated GIFs that play and pause), tables with styles, charts (column, bar, line,
  area, pie, doughnut, scatter and more), basic SmartArt, groups, connectors glued to sites,
  freeform/curve/scribble, merge shapes, ink, action buttons, WordArt; flipped shapes keep their
  text readable.
- **Text:** in-place editing with caret, selection, keyboard and mouse; every Home-tab font and
  paragraph control; bullets and numbering; levels; autofit; columns; vertical text; date fields in
  their own format (Insert › Date and Time picks the format and language, or inserts the sample as
  fixed text); hyperlinks drawn in the theme's hyperlink colour and followed in the show.
  UAX #9 bidi and Arabic shaping with HarfRust, script font slots with per-cluster fallback,
  `justLow` kashida, bidi caret and selection.
- **Editing:** select, marquee, move, resize, rotate, adjust; smart guides; nudge; duplicate;
  z-order; group/ungroup; align/distribute; Format Painter; Selection pane; clipboard.
- **Transitions and animations:** 48 transitions including Morph (objects, words, characters,
  geometry outlines), 64 animation effects (entrance, emphasis, exit, motion paths, media),
  triggers, by-paragraph builds, Animation Painter, on a shared timeline engine.
- **Slide show:** full screen, keyboard/mouse navigation, blank screens, go-to-slide (the typed
  number is shown), See All Slides, click-to-zoom, pen, laser pointer, presenter window (black,
  white, zoom, the slide grid, a timer that pauses and resets, a local clock, and notes size),
  rehearse timings, custom shows played on their own, Set Up Show, Reading View; links and shape
  actions followed on click.
- **Views:** Normal, Outline, Slide Sorter, Notes Page, Reading View, Slide Master (Preserve on
  the open master or layout, and Title and Footers placeholders); zoom (dialog: percent or Fit);
  Grid Options; New Window; grayscale.
- **Review:** comments with replies and resolve, previous and next across the deck, accessibility
  checker, a basic spelling list.
- **Media:** MP3, AAC/M4A, ALAC, WAV, AIFF, CAF, FLAC, Ogg Vorbis and Opus audio; H.264, HEVC,
  VP9 and AV1 video in MP4/MOV/WebM/MKV with pure-Rust decoders; poster frames, trim, fades,
  volume, loop, play across slides.
- **Files:** `.deckcraft` (zip + JSON), PPTX import/export (also opens `.potx`/`.ppsx`), PDF
  export (slides, notes, handouts 1–9 per page, text layer, links, bookmarks), PNG/JPEG, outline
  text, AutoRecover.
- **Automation:** 328 engine commands (`cmd!` registrations in `crates/engine/src/cmd/*`), every
  one reachable from `deckcraft-cli`, the JSON control channel and MCP. The menu-bar measurement
  and the d556ff9 snapshot still count 226.
- **Platforms:** signed releases for macOS (universal), Windows x64/x86/arm64, Linux
  (AppImage/deb/rpm/tar.gz/Flatpak, x86_64 and aarch64), FreeBSD and the web (v0.4.0, 2026-10-09).

## Calibration: how the hours were derived

From this repo's own history (`git log`, `gh pr list`, the 2026-10-07 ROADMAP):

- **The bulk build:** engine, model, renderer, text, UI, show engine, PPTX, PDF, media and the
  release pipeline (today 117k lines of Rust, 59k of them media codecs and containers ported from FilmCraft) took about
  **30 wall-clock hours with up to three agents**, i.e. roughly 60–90 agent hours, landing publicly
  between 2026-10-05 22:13 and 10-06 02:28. That is ~100 agent hours for today's 79% breadth / ~55%
  ready, with community PRs.
- **Catalogue rows** (connectors with glue, freeform tools, merge shapes, AutoRecover, the gradient
  editor) took **1–2 agent hours each** to reach "done" (commits 10-06 00:49–01:34, parallel agents).
- **Recent PRs** (2026-10-08 to 10-10): Animation Painter +189 lines, animated GIF playback +393,
  hyperlink fixes +474, Morph geometry +781, UAX #9 bidi and Arabic shaping +1,902. At the ~150–300
  tested lines per agent hour these suggest, that is 0.5–1 h, 1–3 h and 6–12 h respectively.
  PR open-to-merge times (often 10–30 h) measure the review queue, not the work, and were not used.

Scale used: a fix 0.5–1 h; a checklist row to presence 1–2 h; a row to PowerPoint depth 2–4 h; a
subsystem (localization infrastructure, a real-deck corpus with fixes, EMF rendering) 6–35 h. The
tail costs more per point than the bulk did: the remaining work is depth, fidelity and verification,
which the first 100 hours mostly skipped.

**Why the full-parity estimate rose from ~190 h (2026-10-07) to ~365–650 h:** the old figure
covered the checklist rows, ribbon depth and performance only (~135 h of it is comparable to this
document's 140–240 h for features plus 30–50 h for UI). New in this measure: localization (55–90 h;
there is none yet), hardware (25–45 h), legacy and missing formats including `.ppt`, `.odp`, EMF/WMF
and video export, a real-deck PPTX corpus, AI features, and the depth bugs users found in features
scored done (16 open issues).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-22 | trivial | Animations shortfall: preview from the selected effect (Play From) is in. Percentages unchanged |
| 2026-10-17 | trivial | A word no longer breaks after `?`, `!`, `}` or `+` (a URL's query wraps at the edge as in PowerPoint 2016 for Windows), as it already didn't after `/` and `\`. Headline percentages unchanged |
| 2026-10-16 | trivial | Arrange shortfall: Align to Slide and Align Selected Objects are in the Align menu. Percentages unchanged |
| 2026-10-14 | minor | Community PR wave folded: one headline breadth row (84% over 191 features after `cargo xtask parity`; duplicate checklist rows removed), one row per feature area with the latest breadth and shortfalls, one Themes / Slide show / Views / Review / Automation bullet each (320 engine commands). SmartArt ready ~35% (Text Pane, 6e40335) moves the area mean to 62.8%; dimension and headline percentages unchanged |
| 2026-10-13 | minor | Shapes shortfall "text warp" narrowed to adjust handles: the Transform gallery applies a preset |
| 2026-10-14 | minor | Media remainder: Insert › Icons places Lucide stroke icons. 3-D models and stock content remain. Headline percentages unchanged; the checklist row stays missing |
| 2026-10-14 | trivial | Size and Position dialog. Command count 226 → 227. Breadth stays 79%, ready stays ~55%, UI stays ~60% |
| 2026-10-14 | trivial | 228 commands; Review lists previous and next comment. Breadth stays 79%; the comments checklist row stays D |
| 2026-10-14 | minor | Ecosystem evidence: six original starter decks. The dimension stays ~25% because PowerPoint's large gallery is still absent |
| 2026-10-14 | trivial | Working today: 227 commands. Breadth stays 79%, ready ~55%. The d556ff9 snapshot (226 commands, 27 dialogs) is unchanged |
| 2026-10-14 | trivial | Working today: 228 commands. Breadth stays 79%, ready ~55%. The d556ff9 snapshot (226 commands, 27 dialogs) is unchanged |
| 2026-10-14 | minor | What works today: Create New Theme Colors. The d556ff9 snapshot (226 commands, 27 dialogs) and the breadth / ready percentages are unchanged |
| 2026-10-14 | minor | What works today: Create New Theme Fonts. The d556ff9 snapshot (226 commands, 27 dialogs) and the breadth / ready percentages are unchanged |
| 2026-10-14 | trivial | Insert › Date and Time noted under what works today. The d556ff9 snapshot (226 commands, 27 dialogs) and the readiness percentages are unchanged |
| 2026-10-14 | trivial | Zoom dialog takes a percent or Fit. Breadth stays 79%; the zoom checklist row stays D |
| 2026-10-14 | trivial | Slide show evidence includes a visible go-to-slide number. Breadth stays 79% |
| 2026-10-14 | trivial | Slide show evidence includes custom show playback. Breadth stays 79% |
| 2026-10-14 | trivial | Slide show evidence includes presenter timer pause/reset, a local clock and notes size. Breadth stays 79% |
| 2026-10-14 | trivial | Slide show evidence includes See All Slides and zoom. Breadth stays 79% |
| 2026-10-13 | trivial | Essentials discoverability note: the command palette's Export failure (#79) is fixed; the ×0.95 discount stays for the menu bar and start screen |
| 2026-10-14 | minor | Design ▸ Effects noted under what works today. Percentages stay 79% / ~60% / ~55%. The d556ff9 snapshot and the menu measurement stay at 226 commands; the live registry is 227 |
| 2026-10-14 | minor | Slide Master Preserve and Title/Footers placeholders. Automation in "what works today" is 236 commands; the menu-bar measurement and the d556ff9 snapshot still count 226. Percentages unchanged |
| 2026-10-11 | minor | Animated GIF export (#171) moves the checklist's video/GIF row from missing to partial: P2 breadth 36% → 37%; overall breadth (79%), dimension scores and stage unchanged |
| 2026-10-14 | minor | Format Background picture and solid options noted under what works today. Percentages, the 226-command count and the d556ff9 dialog snapshot stay as they are |
| 2026-10-13 | trivial | Preview in editor scored done: breadth 79% → 80%, Animations area 79% → 85% |
| 2026-10-14 | trivial | Command count 227 → 229 (Paragraph dialog and Tabs). Breadth and ready percentages unchanged |
| 2026-10-14 | trivial | Command count 226 → 227 (Font dialog). Breadth and ready percentages unchanged |
| 2026-10-13 | minor | Views: Grid Options dialog landed (spacing, snap, display). Remaining in that row: New Window. Percentages unchanged |
| 2026-10-13 | minor | Shapes shortfall "text warp" narrowed: yellow adjust handles drag; the gallery and command remain |
| 2026-10-14 | trivial | Draw and hardware blurbs: pen pressure scales ink width when reported; percentages and hour ranges unchanged |
| 2026-10-10 | minor | Added the readiness-by-audience table: each number gets its own hours to ~95% (full 340–600, mainstream 150–250, essentials 50–90). The full number stays an additive weighted sum; no change to the percentages |
| 2026-10-10 | minor | Added the mainstream practitioner (~53%) and essentials user (~62%) numbers with written weights, discounts and user evidence. Features dimension recomputed from written area weights (66% judgement → 62.4% weighted mean), which moves ready for real work from ~56% to ~55%; stage unchanged |
| 2026-10-10 | minor | Stage checked against the core-workflow gate (all six pass); stays alpha |
| 2026-10-10 | major | Created from ROADMAP.md's "How far from full parity" and "Status" sections; full re-measure against PowerPoint for Mac 16.113.4 (Info.plist, lproj, menu dump), 226 commands, 191-row checklist (79%), weighted dimension table (~56% ready), hours recalibrated from git history |

### PPTX.52: local text allocation and reversed fitting state

Local primary-axis allocations are restored before preset text bounds and orientation;
clipped cross-axis growth does not reopen a previously allocated flow. Fine advances
correct two 17pt numeric font selections to the independently observed 16pt result.
The five checkpoint fonts match, while a forced-font control distinguishes nine native
lines from the old eight-line measurement. Painting advances and bullet origins remain
separate open work; this change does not replace ordinary slide text metrics.

Reversed centered fitting distinguishes raw, logical and cross-capacity rejection,
preserves the final search trial, and keeps nominal/fitted/restored alignment lifetimes.
The complete 2,300-case matrix retains every identity and threshold: 2,096 to 2,145
strict matches, 2,246 unchanged, and every output equals its isolated expected result.
One rotated case retains its worse unrotated metric, with rotation-corrected painted
error 0.00752pt; the other two rotated controls retain 0.00809pt bounds. Six independent
height-replay controls reject uniform checkpoint reuse, so broader rule-search history
remains incomplete. The map threshold ledger still has seventeen boundary sequences
and two Albers sequences; no geographic assets or native documents are published.

PPTX.52 integration validation: all eight local CI stages and 2,713 tests pass. The final binary preserves all 814 retained chart pages and 140 map images exactly, and five selected reverse-layout rasters match the accepted isolated result pixel for pixel. Across 14,096 inspected text-property sets, only the two expected 17pt-to-16pt font corrections change; all 326 recovery pages retain geometry and PDF text/font positions.

### PPTX.53: fitting checkpoints and diagram text origins

Independent original controls distinguish resetting a cross-size search from resuming
its local fit. Both fitting checkpoints must be retained. A nested flow with unresolved
sibling text keeps its allocation history; nesting by itself does not invalidate a
checkpoint. The implementation shares the existing work and text-measurement budgets.
The 2,364-case comparison improves from 2,174 to 2,227 strict geometry matches;
all 53 changed cases pass, with no lost prior match or increased measured error.

Materialized uncached SmartArt text now records fine glyph advances separately from
line metrics. Ordinary and cached DrawingML do not acquire that flag, and the flag
is not written as an invented PPTX attribute. At text materialization, valid bullet
paragraphs derive missing hanging defaults from their resolved secondary font while
preserving authored margin and indent independently. Font-rule trials refresh only
the unauthored sides instead of retaining an earlier trial's computed indent. An overrunning marker starts
text at the marker end without an extra artificial gap. Existing recovered defaults
remain separate.

The direct text comparison against the preceding implementation improves absolute
glyph positions on 52 of 326 pages, with no measured regression and 274 raw text-record
matches. The 82-page drawing comparison keeps 66 exact line-text/font-size sets;
66 pages improve glyph advances, 15 are unchanged and one has no common comparable
line. Fifty-seven valid new bullet controls show 47 improved text origins and 10
unchanged; two fractional-font controls did not satisfy their discriminator premise
and are excluded. Ordinary marker quantization still differs by up to 0.063pt. The drawing set
retains 16 line-text/font-size mismatches, and the recovery set retains 27 complete-text
mismatches; no existing mismatch is counted as a pass. Twelve bullet prefixes are
absent from PDF text extraction even though raster checks show the painted bullets.

These results do not close the remaining 137 strict raw-box mismatches (including
retained rotated-shape representations whose painted bounds were checked separately),
the 17 geographic
threshold and two projection residuals, or retained chart/line-layout differences.
Ten rule/direct endpoint controls verify default refresh after a font change, including
independently authored margins and indents. Eight byte-preserving small-font controls
expose a retained 11pt default-margin gap: the native margin is 4.5pt rather than the
current 9pt. Its prefix error reaches 4.498pt; the larger default bands are not a
universal font-to-margin rule. No universal PowerPoint or LibreOffice replacement is claimed.

The 77-page bullet control set improves 51 prefix origins against the preceding
implementation, leaves 26 unchanged and worsens none. Four improvements specifically
remove stale defaults introduced by the initial candidate; the static 11pt gap remains.
Final integration preserves all 2,364 accepted geometry results, 485 isolated text-page
records and 42 selected rasters, plus all 814 chart pages and 140 map images. All eight
local CI stages and 2,720 tests including doctests pass. Hosted checks are
reported separately.

### PPTX.54: intermediate reversed-flow alignment

A retained nominal reference can move the next reversed line before the following
width crosses the final reflected boundary. Preserve that intermediate advance and
carry a bounded correction amount to the next forward line. An ordinary reference
that already equals the flow extent keeps its existing pending growth; fitted and
restored placement retain their separate reference lifetimes. No document, font,
page-number or dimension-specific branch is introduced.

Sixty-six new diagnostic pages include 36 independently parameterized holdout pages
covering both sides of the intermediate boundary, equality at the reflected limit
and an already-pending advance. All 66 pass the actual importer geometry check. A
standalone mutation using the preceding centering code fails the native-bound test;
the updated code passes. The full 2,430-case geometry comparison improves from
2,239 to 2,295 strict matches within 0.025pt, with 2,374 unchanged. All 56 changed
cases pass, with no prior match lost and no maximum or mean error increase.

The remaining 135 strict raw-box mismatches, small-font bullet defaults, text metrics,
geographic threshold/projection differences and chart-layout limits remain open.
Rejected variants and failed hypotheses remain recorded. No complete PowerPoint
equivalence or LibreOffice replacement is claimed.

PPTX.54 validation: all eight local CI stages and 2,722 tests including doctests
pass. The final binary preserves all 485 text-page records, 42 selected text rasters,
814 chart pages and 140 map images. Four direct raster comparisons at a shared 4K
point scale reduce colored-bound errors from 134–294px to 1–2px. Native PDF color
quantization and page-box rounding are recorded separately; content is not realigned.
The actual before/candidate/native contact was visually reviewed. Hosted checks are
reported separately.

### PPTX.55: customized diagram font precedence

The data-point `custT` flag distinguishes customized text from automatic layout
font selection. Customized text retains each run's authored size through layout
font trials. Missing run sizes use the independently observed 36pt diagram default;
paragraph defaults and layout constraints do not replace it. Generated bullet
margins follow the first customized run, while independently authored margins and
indents still win. Non-customized text retains automatic layout font selection.

Sixteen original controls vary the customization flag, mixed-run ordering,
paragraph defaults, authored indents and active shrinking rules. Eight additional
controls independently change presentation, master, list and theme defaults and
include an uncustomized negative control. The ordinary witness text responds to the
presentation-default intervention; the customized target does not. A separate
master-only witness prediction was refuted and is retained in the private evidence.
Non-whitespace font-size matching improves from 6 to all 24 pages. The long overflow
case still differs in extracted trailing whitespace and by up to 1.669pt in baseline
placement; these are not counted as complete text or layout matches.

The native-backed regression fails on the preceding implementation because an
authored 14pt run becomes 27pt. The repaired run sizes and margins survive native
save/reload and PPTX reimport without invented PPTX attributes. Actual same-scale
before/candidate/native crops were visually reviewed. The 11pt generated margin
remains 9pt versus native 4.5pt; this patch does not refit its quantization rule.
Recovered customized text and empty/break/field-run precedence still need positive
native controls. Additional reverse-boundary diagnostics remain unresolved.

PPTX.55 validation: all eight local CI stages and 2,723 tests including
doctests pass. The final binary retains all 2,430 geometry results (2,295 strict matches), all 485 text-page
records and 42 selected rasters, all 814 chart pages and 140 map images. All 24 new
control pages match
the isolated candidate's model, raw text and 4x raster exactly. Complete SmartArt,
geographic and chart parity and full LibreOffice replacement remain unproven.

### PPTX.56: retain the root dimension-rule fit stage

A root linear dimension search retains a finite fit when text still overflows.
The allocator now records whether it actually reduced the flow to fit capacity,
so the final stage does not apply that reduction twice. Later font or height
rules that resolve the text overflow remove the need for the extra text fit.
The change uses allocation state and measured overflow, with bounded existing
search work; it does not select behavior by document, font or page identity.

Thirty-two original native controls distinguish a no-op width rule from a direct
endpoint, reverse the width/height search order, change later font rules, and
compare frames below, at and above the children's combined extent. Eight controls
transpose the primary axis and reverse vertical flow; their preregistered scale
predictions agree with native results within 0.00000122. All controls
pass format validation and open/export successfully. The first candidate gained
51 strict geometry matches but lost 29 by fitting already allocated flows again;
it was rejected. The revised candidate improves 2,314 to 2,370 strict matches
within 0.025pt across 2,462 cases, with 2,393 unchanged and no lost or worse results.
The 92 remaining strict raw-box mismatches include known orientation-related
representation differences as well as unresolved geometry; they are not claimed
fixed. Separate newer reverse-layout diagnostics also remain unresolved.

Two regression tests cover native-observed capacity cases and removal of the
refit after a successful later font rule. Restoring the previous production
logic makes the dimension-rule regression fail. Four actual 4,096-pixel-wide
before/candidate/native comparisons remain within two pixels on shape bounds
in both versions: the new numerical gain is subpixel, not a claimed visible
raster improvement. Overflowing text and its remaining clipped-glyph differences
are still visible in those controls.

New independent fractional-font controls also expose a retained PPTX.55 problem:
a customized 27.75pt run receives a generated 22.5pt margin instead of the native
18pt. Truncating font sizes with the existing default-size bands fixes that case but
regresses the separate 21.75pt control, so that proposal was rejected and is not
part of this patch. Exact 21pt controls also expose an incorrect existing band;
this is not proof that truncation alone caused the discrepancy.
Fractional glyph advances, painted font sizes and generated defaults require
separate treatment; the static 11pt default and broader text/layout gaps remain.

The initial text comparison preserves 476 of 485 raw-text records and 41 of 42
selected rasters exactly. All nine changed text pages retain the complete
non-whitespace character sequences, font sizes and counts, and their absolute
glyph-origin RMS improves. One page's maximum individual glyph error increases
by 0.01861pt while its RMS improves; existing text-origin errors remain. Actual
raster comparisons on that page, a rotated control and the changed-raster page
also reduce total RGB error against native output. This is not complete text
parity. Final refactor integration exactly reproduces all 2,462 geometry results,
485 text records and 42 selected rasters of the isolated root-fit candidate.
All 814 retained chart pages, 140 map images and 24 custom-font control pages
remain unchanged. All eight local CI stages and 2,725 tests including doctests
pass, with eight existing ignored tests retained; hosted checks are separate.

### PPTX.57: use the authored axis end when choosing the automatic end

A chart with only its maximum fixed must choose its automatic minimum relative
to that maximum. The previous calculation used the largest data value instead.
For example, values 8 and 9.5 with maximum 10 now use 0–10; values 9 and 9.5 use
8.4–10 after major-unit rounding. The negative-axis case is symmetric. Explicit
both-end bounds, both-automatic bounds, and no-headroom percentage scales retain
their separate policies. Non-finite inputs retain existing finite filtering,
clamping and bounded step selection.

Eleven independently authored fixed-bound controls reproduce the native axis
ranges, including positive/negative data, threshold equality, fractional scale,
and shortened plots. The regression test fails against the prior production
calculation and passes with the correction. The final candidate was compared
against the actual previous build on 1,119 pages in 91 packages: 1,100 are
pixel-identical and every changed page has lower native RGB error. The 28 native
bar controls have 18 improved bounds and no worsened bounds; all 140 retained
map renders are identical. All 44 previously frozen local regression masks also have no worsening;
whole-page gains do not waive local defects. All eight local CI stages pass,
with 2,726 tests including doctests (eight existing tests remain ignored).

This is an axis-range correction, not complete chart fidelity. Two retained
out-of-range bar controls still have a 1.5-point clipping difference. A separate
both-automatic control still has a 51-point bar endpoint difference; its native
8.7–9.6 range differs from the existing 8.9–9.6 result. Category text placement,
axis visibility, PDF compositing, and broader rich-label layout remain open.
SmartArt rule search, generated bullet defaults, reverse/cyclic placement, and
remaining map coordinate/projection differences also remain open. The separate
combined chart-layout candidate is not included in this change. Only original
Rust, tests and documentation are published; private native observations and
source decks remain excluded.

### PPTX.58: resolve generated SmartArt hanging defaults by font interval

Uncached valid-layout SmartArt can choose the correct font but still indent its
paragraph incorrectly. The former uniform size formula gives a generated 9pt
margin at 11pt and 22.5pt at a customized 27.75pt; native controls require 4.5pt
and 18pt respectively. The generated lookup now uses the effective secondary
font, or the first customized run, with boundaries at 12, 16, 20 and 28 points.
Customized fractional run sizes and advances remain intact. An authored margin
or indent still wins independently, and recovered layouts keep their separate
policy. This is an empirically validated compatibility policy, not a claim that
OOXML specifies those intervals.

Twelve fractional/integer pairs and 24 independent boundary controls distinguish
font painting, logical advances and generated sides. The boundary controls vary
font family, frame geometry and customized versus automatic sizes; all 24 pass.
Across 253 diagnostic pages, all 180 directly measurable margin/indent observations
match native output, improving from 95. Of 245 pages with strict glyph identity,
87 improve absolute origin RMS, 158 are unchanged and none worsen. Absolute,
relative, horizontal and vertical maximum-error comparisons also have no worsening.
Eight preexisting invisible-PDF-text identity failures remain separate: visible
text and the editable model retain the full word, but PDF extraction substitutes
a control character after a hard break. Their source-order and raster comparisons
improve on six pages and remain unchanged on two; they are not counted as strict
identity passes.

The candidate retains all 2,462 geometry and 15,598 text-property records exactly.
Of 485 retained rendered text pages, 471 are unchanged and all 14 changed pages
retain strict text identity and improve native absolute origin RMS. All 326 recovery
pages remain exact. Of 42 retained raster samples, 39 are identical and all three
changed samples reduce the error within the complete native target region.
Actual before/candidate/native images were reviewed. The old production formula
fails the native-backed 11pt regression. Full eight-stage CI passes, with 2,726
tests including doctests and eight existing ignored tests.

This closes the previously recorded 11pt, 21pt and customized 27.75pt generated-side
defects. It does not close the remaining 92 raw-box mismatches, arbitrary text-driven
rule search, reverse/cyclic layout differences, fractional painting and vertical
origins, PDF extraction defects, chart layout or the 17 map-threshold and two Albers
precision cases. The separate automatic-axis and chart-layout candidates are not
included. Only original source, programmatic tests and documentation are published;
native observations and source decks remain private.

### PPTX.59: keep hard-break positions in the PDF text layer

An authored break contributes one character to layout even when its run stores
an empty string. The PDF text layer previously joined raw run strings, then used
layout's character indices to select glyph text. After a break, those indices
selected the next character or an empty range, corrupting copying and extraction
while the rasterized word still looked correct. Export now obtains the canonical
paragraph text, including break positions, before slicing it for each glyph run.
The source model, layout and painted glyph positions are unchanged.

The regression decodes the actual exported PDF's text-showing operators through
its ToUnicode map. It covers multibyte text, leading and repeated breaks, literal
line feeds combined with authored breaks, and a break run containing an ignored
stored string. The old production code fails because a displayed glyph has no
text mapping; the corrected exporter passes all 19 PDF tests.

The final candidate was rendered against the actual preceding version on 661
unique pages in 53 original packages. The 485-page retained set and 253-page
diagnostic set overlap by 77 pages, which are counted once. Every page is
pixel-identical. Only eight text records change, and all eight now preserve the
complete native character sequence: diagnostic identity improves from 245/253
to 253/253 with no lost identity. The preceding exclusion for these eight
multiline PDF extraction failures is therefore resolved. Full eight-stage CI
passes with 2,727 tests including doctests; eight existing tests remain ignored.

This fixes source-index mapping in PDF export, not the remaining text-layout
differences. Fractional painting, vertical origins, the broader raw-box and
rule-search mismatches, chart layout and dense-axis behavior, and map reduction
and projection precision remain open. No general LibreOffice-replacement claim
is made. Only original Rust, programmatic tests and documentation are published.
