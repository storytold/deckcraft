# DeckCraft roadmap: milestones and what's next

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-22 · **Change:** trivial (beta item 9 drops Play From, now in; previous: minor, native chart payloads stored in bounded ZIP members; previous: major, community PR wave 2026-10-14: PPTX corpus, presenter and show controls, review navigation, starter decks, Grid Options and New Window, WordArt gallery and handles, pen pressure; beta items 9 and 10 narrowed; duplicated rows and headers folded; previous: minor, core-workflow alpha gate added) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

Forward-looking: where DeckCraft is going and in what order. The one-page summary is
[ROADMAP.md](../ROADMAP.md); every shortfall is in [gaps.md](gaps.md); numbers are in
[target-app-parity.md](target-app-parity.md). Hours are Opus 5.5 agent wall-clock hours.

## Alpha gate: PowerPoint's core workflows

What a typical PowerPoint user does every day, checked end to end on macOS, the main platform
(craftrules `standards/progress-docs.md`, "The core-workflow gate"). Every workflow passes, so
DeckCraft stays **alpha**. The "partial" rows don't block the workflow, but they are its weakest
points and lead the beta plan.

| Core workflow | Works end to end? | Evidence | Hours to full pass |
|---|---|---|---|
| Build a deck from scratch (theme, layouts, titles and bullets, shapes, pictures from file), save, close and reopen it | yes | Commands for all of it; `.deckcraft` and `.pptx` round-trip tests (`crates/pptx/tests/it/roundtrip.rs`, `crates/format`); AutoRecover. Close discards edits without asking (#60, PR #75); image *paste* fails on Windows/Linux (#53), though insert-from-file works | 1–3 |
| Open a colleague's `.pptx`, edit it, save it back as `.pptx` that PowerPoint opens | partial (not blocking) | The reader is lenient and keeps unknown objects. Files open in PowerPoint without repair (checked 2026-10-06). But only generated decks have been tested; text inside EMF/WMF pictures is missing; comment threads, media options and rewind are lost (#70, #74, #77) | 31–55 |
| Present: full-screen show with transitions, animations, media, presenter view with notes, next slide and timer | yes | 48/48 transitions, 64 effects, triggers, builds, media playback, presenter window, See All Slides and zoom. The presenter timer pauses and resets, and notes have a size control. A custom show plays only its slides. Typing a slide number during the show shows it, and Enter goes there. Presenter view isn't placed on the second display automatically (drag it there; #433) | 2–4 |
| Add business content: tables, charts with their data, simple diagrams | partial (not blocking) | Tables with styles, merge/split, all D or P. Charts insert with the common types and edit data in the Chart Data dialog (P0 partial); SmartArt is basic (pre-drawn shapes) | 12–20 |
| Share the deck: PDF of slides, notes or handouts; images | yes | `file.export` PDF with text layer, links, bookmarks, handouts 1–9; PNG/JPEG. Artwork is raster in the PDF; no direct print (print the PDF) | 10–16 |
| Review: comments with replies, resolve, spelling | partial (not blocking) | Comments, replies and resolve work and survive `.deckcraft` and `.pptx` (#70); Previous and Next walk every slide and the pane lists them; spelling uses installed Hunspell dictionaries (only a short built-in list where none is installed) | 4–8 |

## Current focus (2026-10-10)

1. **Stop losing work and crashing:** land Close's save prompt (#60, PR #75), the Windows GPU
   startup crashes (#40, #59), image paste on Windows/Linux (#53). 6–12 h.
2. **PPTX on real decks:** the owned corpus (`crates/pptx/tests/it/corpus*.rs`) round-trips decks we
   author: tables, charts, animations, media, sections, masters and layouts, comments, links and
   actions, custom shows, rotated groups, inherited placeholders, text effects and body layout,
   theme effects, fills, bevels, connectors, 3-D models and animation bounce end, and keeps notes-
   and handout-master text, paragraph defaults and action sounds. Still open: decks from outside
   this repo, media options and rewind on save (#74, #77), relationship-bearing SmartArt and OLE
   (#203), EMF+ records and SVG. 31–55 h.
3. **Review the open PRs:** equations (#55), browser print (#66), Morph alignment (#67), macOS
   open-with (#68), font lookup (#80), Ukrainian (#34, after the catalog exists). 4–8 h.

## Plan to beta (~120–210 h; ~40–70 h wall clock with 3–4 agents)

Beta means ~75% ready for real work and opening and saving PowerPoint's `.pptx` reliably
(craftrules `standards/progress-docs.md`). Ranked:

| # | Work | Hours | Moves |
|---|---|---|---|
| 1 | Stability: #60, #40/#59, soak and fuzz the editor | 10–20 | Stability 60 → 85% |
| 2 | PPTX real-deck corpus and fixes | 20–35 | File formats 58 → 75% |
| 3 | PPTX round-trip losses (#70, #74, #77), `.potx`/`.ppsx`/`.pptm` content types, MicroType embedded-font subsets | 8–15 | File formats → 80% |
| 4 | EMF/WMF and SVG pictures | 8–14 | File formats, Pictures |
| 5 | Vector PDF, printing | 10–16 | File formats, Files |
| 6 | Localization infrastructure + zh-Hans, es, fr, de, ja (with IME) | 27–46 | Localization 8 → ~40% |
| 7 | Font weights (#69), image paste (#53), Tab in text (#78) | 6–11 | Features |
| 8 | Charts: data grid, elements | 6–10 | Features |
| 10 | Presenter view on the right display, swap displays | 2–4 | Hardware, Slide show |
| 9 | Animation Pane: effect options (the timeline and Play From are in) | 3–6 | Features |
| 11 | Native macOS menu bar, Format Shape pane depth, start screen and window chrome fixes | 13–22 | UI/UX 60 → 75% |
| 12 | Performance baseline and the obvious fixes | 4–8 | Performance |
| **Total** | | **~120–210** | Ready ~55% → ~75% |

Parallelizes well: 1 (engine/app), 2–5 (pptx, render, pdf), 6 (ui-egui catalogs), 8–11 (UI)
touch different crates. Needs a human: the owner opening PPTX outputs in PowerPoint and making
reference renders, a Windows tester with the affected GPUs, native speakers to review translations.

## After beta: toward full parity (~245–440 h more)

SmartArt depth, the remaining animation effects and motion paths, edit-point segment types and
3-D depth, bundled proofing dictionaries, Zoom depth, recording, video export, legacy `.ppt` import
and `.odp` (owner decision), GPU rasterization, hardware video decode, pen tilt (pressure already
scales ink width) and pinch zoom, the other 21 languages with RTL mirroring, a larger template
gallery (six original starters ship), and local AI features (owner decision on models). Itemized
with hours in [gaps.md](gaps.md).

## Milestones

From the local execution plan (`plan/execution-plan.md`, gitignored). Status as of 2026-10-10.

| # | Milestone | Status | Open |
|---|---|---|---|
| M0 | Skeleton and vertical slice | done | |
| M1 | Editing core | done | |
| M2 | PPTX import/export | done for generated decks | real-deck corpus, round-trip losses |
| M3 | Shapes complete | mostly | edit points, Format Shape pane depth, bevel/3-D |
| M4 | Design | mostly | Notes/Handout masters, variants, Designer |
| M5 | Pictures and media | mostly | SVG/EMF, remove background, compress, icons, WMA/WMV |
| M6 | Transitions and animations | mostly | Animation Pane timeline, remaining effects, transition sounds |
| M7 | Slide show | mostly | second-display placement, laser, record |
| M8 | Tables and charts | partly | chart data grid, elements, chartex types, draw table |
| M9 | SmartArt, equations, WordArt | partly | SmartArt layouts and PowerPoint-diagram fidelity, equations (PR #55); the text pane, warp gallery and warp handles are in |
| M10 | Views | done | Arrange All (grid options and New Window are in) |
| M11 | Output | partly | vector PDF, SVG, GIF, MP4, print |
| M12 | Review | partly | dictionaries, AutoCorrect, find/replace dialog, compare |
| M13 | Automation and CLI parity | done | |
| M14 | 1.0 polish and release | partly | i18n, native menu bar, signed releases done (v0.4.0) |

## History: the 2026-10-07 alpha checklist

Before the craftrules stages existed, DeckCraft defined "alpha" as: install a signed build on
macOS, Windows or Linux, make a real deck from scratch or from a PowerPoint file, present it with
transitions, animations and media, save back to `.pptx`/PDF without losing work, and hit no crashes
on common paths. On 2026-10-07 it was ~80% there with these blockers:

| Blocker (2026-10-07) | Status 2026-10-10 |
|---|---|
| First real release run: signing, notarization, installers verified on each OS | Signed releases v0.1.0–v0.4.0 published; installers not hand-checked on each OS; Windows startup crashes on some GPUs (#40, #59) |
| PPTX fidelity on a corpus of real decks | Owned corpus checks in CI (tables, charts, animations, media, sections, masters, comments, links, custom shows, groups, inherited placeholders, text effects, body layout, theme effects, fills, bevels, connectors, 3-D models); notes- and handout-master text kept. Decks from outside this repo and #74/#77 stay open; beta item 2 |
| Presenter view and Animation Pane polish | Still open; beta items 9 and 10 |
| Soak and fuzz the editor | Still open; beta item 1 |
| UI fidelity pass on the most-used ribbon groups and dialogs; first run | Partly (start-screen fixes #20, #49); beta item 11 |
| Mascot app icon, README screenshots, user docs | App icon (with a scalable SVG, #29) and README screenshots in place; user docs open |

By the standard's definitions DeckCraft is **alpha** (~55% ready); these blockers now feed the beta
plan above.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-22 | trivial | Beta item 9: Animation Pane ▸ Play From landed, the item keeps effect options (3–6 h) |
| 2026-10-18 | minor | Native chart payloads use referenced ZIP members and share media byte/item limits; old inline sidecars remain readable. Synthetic tests cover multi-megabyte layout, all shape owners, shared budgets, missing/corrupt members and invalid references. Scores and milestone scope unchanged |
| 2026-10-15 | trivial | Native save/reopen retains imported chart workbooks and related parts; headless command tests cover a PPTX → native → PPTX cycle and stale-workbook rejection. Milestone scope and scores unchanged |
| 2026-10-15 | trivial | PPTX trigger click steps survive save, including by-paragraph builds. Interactive-deck gaps remain media actions, audio triggers and click-box geometry; milestone scope and feature scores unchanged |
| 2026-10-14 | trivial | M13 reliability: remote MCP validates IDs and envelopes, caps replies and rejects incomplete frames; #718 |
| 2026-10-14 | minor | Community PR wave folded: one Review gate row, one Current focus item 2, one beta row each for items 9 and 10 (timeline and timer controls are in), one after-beta paragraph, one M9 and M10 row, one corpus row in the 2026-10-07 history |
| 2026-10-13 | trivial | Beta item 3: embedded fonts are in, except MicroType Express subsets |
| 2026-10-13 | minor | M9: the WordArt Transform gallery applies a text warp; adjust handles stay open |
| 2026-10-13 | trivial | Owned PPTX corpus check started; notes-master header and footer kept. Beta item 2 stays open for a larger corpus |
| 2026-10-13 | trivial | Owned PPTX corpus covers comments, links and actions, custom shows, rotated groups and inherited placeholders. Beta item 2 stays open for a larger corpus, embedded fonts and #70/#74/#77 |
| 2026-10-13 | trivial | Owned PPTX corpus covers text effects, body layout, theme effects, fills, bevels and connectors. Beta item 2 stays open for a larger corpus, embedded fonts and #70/#74/#77 |
| 2026-10-14 | trivial | Current focus: `corpus_media.rs` keeps 3-D models and animation bounce end; the real-deck corpus stays open |
| 2026-10-14 | trivial | Chart trendlines are drawn. The charts gap stays open for the data grid, chartex and secondary-axis export; the real-deck corpus (item 2) stays open |
| 2026-10-14 | trivial | Chart trendlines round-trip in an owned corpus slice; the real-deck corpus (item 2) stays open |
| 2026-10-14 | trivial | Review workflow: previous and next comment across the deck |
| 2026-10-14 | minor | After beta: six original starter decks exist; a larger template gallery remains |
| 2026-10-14 | trivial | View: Zoom dialog (percent or Fit) |
| 2026-10-13 | trivial | Owned PPTX corpus covers tables, charts, animations, media, sections and masters; handout-master text kept. Beta item 2 stays open for a larger corpus |
| 2026-10-14 | trivial | Present workflow notes go to slide number entry |
| 2026-10-14 | trivial | Present workflow notes custom show playback |
| 2026-10-14 | trivial | Present workflow notes presenter timer pause/reset, clock and notes size. Display placement stays open |
| 2026-10-14 | trivial | Present workflow notes See All Slides and zoom |
| 2026-10-13 | trivial | Canvas animation preview landed; beta item 9 keeps the timeline and preview from the selected effect |
| 2026-10-13 | minor | M10: Grid Options dialog; New Window stays open |
| 2026-10-13 | minor | M9: WordArt adjust handles drag; the Transform gallery and command stay open |
| 2026-10-14 | trivial | After beta: pen pressure scales ink width; tilt and pinch zoom remain |
| 2026-10-10 | minor | Added the alpha gate (six core workflows): all pass, two with partial depth; stage stays alpha |
| 2026-10-10 | major | Created: current focus, ranked beta plan with hours, milestones M0–M14 with status, the old alpha checklist as history |

PDF writer consolidation preserves the current raster-artwork export and its selectable
text layer, notes and handouts. It does not advance vector-PDF or printing parity.
