# DeckCraft vs Microsoft PowerPoint: parity assessment

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (full re-measure against PowerPoint for Mac 16.113.4; replaces the parity sections of ROADMAP.md) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

The authoritative answer to "how close is DeckCraft to PowerPoint, and how much work is left?".
[ROADMAP.md](../ROADMAP.md) summarizes it; [gaps.md](gaps.md) itemizes every shortfall;
[parity-checklist.md](parity-checklist.md) is the row-by-row feature checklist behind the breadth
number.

## Headline

| Number | Value | Kind |
|---|---|---|
| **Feature breadth** (weighted checklist) | **79%** over 191 features (P0 92%, P1 75%, P2 36%) | **measured**: `cargo xtask parity` formula over [parity-checklist.md](parity-checklist.md), rows re-scored 2026-10-10 |
| Menu-bar coverage | **59%** (128 of 217 app-specific PowerPoint menu items have a DeckCraft command or UI label of the same name) | **measured** (heuristic lower bound): script matching the 2026-10-07 PowerPoint menu dump against the 226 engine command labels and the UI's strings; excludes the app, Window and Help menus and the subtitle language lists |
| **Ready for real work** | **~56%** | **estimated**: weighted dimension table below |
| Stage | **alpha** (beta is ~19 points and ~120–210 h away) | judgement against craftrules `standards/progress-docs.md` |
| Remaining effort to beta | **~120–210 Opus 5.5 agent hours** | estimated, itemized in [roadmap.md](roadmap.md) |
| Remaining effort to full parity | **~365–650 Opus 5.5 agent hours** | estimated, by dimension below |

The menu figure is lower than the checklist figure because PowerPoint's menu bar lists every chart
type, SmartArt category, action button and media source as its own item, and many DeckCraft
equivalents are one parameterized command (`insert.chart {type}`) that the label match cannot see.
Read it as a floor, not as a second breadth number.

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
| Features (breadth × depth) | 30% | ~66% | 140–240 | [parity-checklist.md](parity-checklist.md), [animation-parity.md](animation-parity.md), [slideshow-parity.md](slideshow-parity.md) | Breadth 79% measured. Depth lags: 37 of 191 rows are partial, and users found depth bugs in rows scored done within two days (#53, #60, #69, #70, #74, #77, #78) |
| File formats | 20% | ~58% | 60–110 | [file-format-parity.md](file-format-parity.md) | PPTX reads and writes every main part and keeps unknown objects; files open in PowerPoint without repair. But it has never met a corpus of real decks, EMF/WMF/SVG pictures don't decode, comments/media/rewind settings are lost on round trip, and there is no `.ppt`, `.pptm`, `.odp`, video or GIF export |
| UI/UX fidelity | 15% | ~60% | 30–50 | [ui-parity.md](ui-parity.md) | All 10 ribbon tabs and the contextual tabs exist; Format Shape pane, Animation Pane, dialogs and menus are thinner than PowerPoint's; no native macOS menu bar; user reports #17, #71, #73, #76 |
| Stability | 10% | ~60% | 10–20 | [gaps.md](gaps.md) | Never-crash rules, a panic guard round every command, PPTX fuzz/malformed tests. But startup crashes on some Windows GPUs (#40, #59) and Close discards edits without asking (#60) |
| Performance | 5% | ~55% | 10–20 | [hardware-parity.md](hardware-parity.md) | Unmeasured against PowerPoint. Slides rasterize on the CPU (`vello_cpu`); Morph re-aligns outlines every frame (#67) |
| Hardware | 5% | ~40% | 25–45 | [hardware-parity.md](hardware-parity.md) | GPU-composited window (wgpu), but CPU slide raster, software-only video decode, presenter view not placed on the second display, no pen pressure, no pinch zoom |
| Platforms | 5% | ~80% | 5–10 | [ROADMAP.md](../ROADMAP.md) | Ahead in reach: macOS, Windows x64/x86/arm64, Linux (5 formats), FreeBSD and the web, all from one release workflow. Held back by the Windows GPU crashes and Linux/Windows clipboard gaps |
| Localization | 5% | ~8% | 55–90 + native review | [localization-parity.md](localization-parity.md) | English only; no string catalog. PowerPoint for Mac ships 26 languages. Bidi and Arabic shaping in slide text exist |
| Ecosystem (templates, add-ins) | 3% | ~25% | 10–20 | [gaps.md](gaps.md) | 8 original themes vs PowerPoint's large template gallery; no add-ins or VBA (out of scope by policy) |
| AI features | 2% | ~10% | 20–40 (owner decision on models) | [gaps.md](gaps.md) | No Designer, Copilot, Speaker Coach, live subtitles or Translate (all Microsoft cloud services). Agents drive every command over MCP instead |
| **Total** | 100% | **~56%** | **~365–650** | | |
| Automation (✱, not weighted) | | ahead | 0 | [mcp.md](mcp.md), [control-protocol.md](control-protocol.md) | Every command is scriptable from the CLI, a JSON control channel and MCP, headless or against the running app; PowerPoint for Mac has AppleScript and VBA only |

About 60% of the hours parallelize across crates (formats, render, text, UI, media, localization
catalogs). Needs a human: native-speaker review of every translation, the owner's decision on AI
models and on legacy `.ppt` import, running PowerPoint to make reference renders and to confirm
round-tripped files open cleanly, and hardware we don't have (Windows GPUs from #40/#59, pens,
multi-display rigs).

## By feature area

Breadth is measured from the checklist; "ready" adds depth and is estimated. Hours are to
PowerPoint parity for that area.

| Area | Breadth (measured) | Ready (est.) | Hours | Main shortfalls | Doc |
|---|---:|---:|---|---|---|
| Application shell | 71% | ~55% | 8–14 | No native macOS menu bar, thin start screen (templates, recent), one window per presentation missing | [ui-parity.md](ui-parity.md) |
| Files (commands) | 69% | ~50% | see file formats | Print, reuse slides, passwords, legacy and macro formats | [file-format-parity.md](file-format-parity.md) |
| Slides and sections | 90% | ~85% | 3–5 | Zoom (summary/section/slide), copying slides between decks keeps formatting only partly | [parity-checklist.md](parity-checklist.md) |
| Views | 90% | ~80% | 3–6 | Snap to grid and grid options, New Window | [ui-parity.md](ui-parity.md) |
| Masters and themes | 85% | ~70% | 8–14 | Notes and Handout masters, theme variants, `.thmx`, effect schemes | [parity-checklist.md](parity-checklist.md) |
| Shapes | 86% | ~75% | 8–14 | Edit points, text warp, WordArt depth | [ui-parity.md](ui-parity.md) |
| Format (fill, line, effects) | 81% | ~65% | 8–14 | Format Shape pane depth, bevel and 3-D, picture/texture fill options | [ui-parity.md](ui-parity.md) |
| Arrange | 100% | ~90% | 1–2 | Align to slide vs selected as a menu choice, More Rotation Options | [ui-parity.md](ui-parity.md) |
| Text | 80% | ~62% | 18–30 | Font weights beyond Regular/Bold/Italic (#69), Tab in text boxes (#78), IME, true vertical CJK, dictionaries, AutoCorrect, thesaurus, equations (PR #55 open) | [localization-parity.md](localization-parity.md) |
| Tables | 89% | ~75% | 3–6 | Draw table, borders and cell effects depth | [parity-checklist.md](parity-checklist.md) |
| Charts | 65% | ~45% | 12–20 | Data grid editing, chart elements, the `chartex` types (waterfall, funnel, treemap, sunburst, histogram, box) | [parity-checklist.md](parity-checklist.md) |
| SmartArt | 33% | ~25% | 10–16 | Text pane, layout switching, the diagram data model on save | [parity-checklist.md](parity-checklist.md) |
| Pictures | 48% | ~45% | 12–20 | SVG/EMF/WMF, clipboard paste on Windows/Linux (#53), artistic effects, remove background, compress | [file-format-parity.md](file-format-parity.md) |
| Media | 67% | ~60% | 8–14 | WMA/WMV decode, record audio/screen, icons and 3-D models, playback options lost on PPTX round trip (#74) | [file-format-parity.md](file-format-parity.md) |
| Transitions | 92% | ~80% | 4–8 | Transition sounds, 3-D transitions approximated in 2-D | [animation-parity.md](animation-parity.md) |
| Animations | 79% | ~65% | 10–16 | 64 of PowerPoint's ~190 effects, Animation Pane timeline, preview, rewind lost on save (#77) | [animation-parity.md](animation-parity.md) |
| Slide show and presenter view | 66% | ~60% | 10–16 | Second-display placement, laser pointer, recording, subtitles, ink depth | [slideshow-parity.md](slideshow-parity.md) |
| Review | 60% | ~45% | 8–14 | Modern comment threads on round trip (#70), compare, version history, real spelling dictionaries | [file-format-parity.md](file-format-parity.md) |
| Draw (ink) | 33% | ~30% | 5–9 | Pen pressure, ink to shape/text/math, lasso | [hardware-parity.md](hardware-parity.md) |
| Undo and clipboard | 88% | ~75% | 3–5 | Paste Special, image paste (#53) | [ui-parity.md](ui-parity.md) |
| Automation ✱ | 100% | ahead | 0 | | [mcp.md](mcp.md) |
| Platforms ✱ | 67% | ~80% | see platforms | | [ROADMAP.md](../ROADMAP.md) |
| **Total (feature areas)** | **79%** | **~66%** | **~140–240** | | |

## What works today (evidence)

Carried from the 2026-10-07 ROADMAP status and updated with what has landed since.

- **Document model:** presentations, slides, masters and 11 standard layouts with placeholder
  inheritance (slide → layout → master → theme), sections, notes, comments, custom shows,
  header/footer fields, embedded media, unlimited undo on shared slide snapshots.
- **Themes:** 8 original themes with colour and font schemes, custom colours and fonts,
  backgrounds (solid, gradient, picture, pattern), slide size presets.
- **Shapes:** ~150 preset geometries with adjustment handles, text boxes, pictures (crop,
  corrections, animated GIFs that play and pause), tables with styles, charts (column, bar, line,
  area, pie, doughnut, scatter and more), basic SmartArt, groups, connectors glued to sites,
  freeform/curve/scribble, merge shapes, ink, action buttons, WordArt; flipped shapes keep their
  text readable.
- **Text:** in-place editing with caret, selection, keyboard and mouse; every Home-tab font and
  paragraph control; bullets and numbering; levels; autofit; columns; vertical text; date fields in
  their own format; hyperlinks drawn in the theme's hyperlink colour and followed in the show.
  UAX #9 bidi and Arabic shaping with HarfRust, script font slots with per-cluster fallback,
  `justLow` kashida, bidi caret and selection.
- **Editing:** select, marquee, move, resize, rotate, adjust; smart guides; nudge; duplicate;
  z-order; group/ungroup; align/distribute; Format Painter; Selection pane; clipboard.
- **Transitions and animations:** 48 transitions including Morph (objects, words, characters,
  geometry outlines), 64 animation effects (entrance, emphasis, exit, motion paths, media),
  triggers, by-paragraph builds, Animation Painter, on a shared timeline engine.
- **Slide show:** full screen, keyboard/mouse navigation, blank screens, go-to-slide, pen,
  presenter window, rehearse timings, custom shows, Set Up Show, Reading View; links and shape
  actions followed on click.
- **Views:** Normal, Outline, Slide Sorter, Notes Page, Reading View, Slide Master; zoom;
  grayscale.
- **Review:** comments with replies and resolve, accessibility checker, a basic spelling list.
- **Media:** MP3, AAC/M4A, ALAC, WAV, AIFF, CAF, FLAC, Ogg Vorbis and Opus audio; H.264, HEVC,
  VP9 and AV1 video in MP4/MOV/WebM/MKV with pure-Rust decoders; poster frames, trim, fades,
  volume, loop, play across slides.
- **Files:** `.deckcraft` (zip + JSON), PPTX import/export (also opens `.potx`/`.ppsx`), PDF
  export (slides, notes, handouts 1–9 per page, text layer, links, bookmarks), PNG/JPEG, outline
  text, AutoRecover.
- **Automation:** 226 commands, every one reachable from `deckcraft-cli`, the JSON control channel
  and MCP.
- **Platforms:** signed releases for macOS (universal), Windows x64/x86/arm64, Linux
  (AppImage/deb/rpm/tar.gz/Flatpak, x86_64 and aarch64), FreeBSD and the web (v0.4.0, 2026-10-09).

## Calibration: how the hours were derived

From this repo's own history (`git log`, `gh pr list`, the 2026-10-07 ROADMAP):

- **The bulk build:** engine, model, renderer, text, UI, show engine, PPTX, PDF, media and the
  release pipeline (today 117k lines of Rust, 59k of them media codecs and containers ported from FilmCraft) took about
  **30 wall-clock hours with up to three agents**, i.e. roughly 60–90 agent hours, landing publicly
  between 2026-10-05 22:13 and 10-06 02:28. That is ~100 agent hours for today's 79% breadth / ~56%
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
| 2026-10-10 | major | Created from ROADMAP.md's "How far from full parity" and "Status" sections; full re-measure against PowerPoint for Mac 16.113.4 (Info.plist, lproj, menu dump), 226 commands, 191-row checklist (79%), weighted dimension table (~56% ready), hours recalibrated from git history |
