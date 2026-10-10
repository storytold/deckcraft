# DeckCraft roadmap: milestones and what's next

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (core-workflow alpha gate added; stage stays alpha; previous: major, created; milestones carried from the local execution plan, the beta plan built from gaps.md) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

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
| Build a deck from scratch (theme, layouts, titles and bullets, shapes, pictures from file), save, close and reopen it | yes | Commands for all of it; `.deckcraft` and `.pptx` round-trip tests (`crates/pptx/tests/roundtrip.rs`, `crates/format`); AutoRecover. Close discards edits without asking (#60, PR #75); image *paste* fails on Windows/Linux (#53), though insert-from-file works | 1–3 |
| Open a colleague's `.pptx`, edit it, save it back as `.pptx` that PowerPoint opens | partial (not blocking) | The reader is lenient and keeps unknown objects. Files open in PowerPoint without repair (checked 2026-10-06). But only generated decks have been tested; EMF/WMF pictures render blank; comment threads, media options and rewind are lost (#70, #74, #77) | 31–55 |
| Present: full-screen show with transitions, animations, media, presenter view with notes, next slide and timer | yes | 48/48 transitions, 64 effects, triggers, builds, media playback, presenter window. Presenter view isn't placed on the second display automatically (drag it there) | 3–5 |
| Add business content: tables, charts with their data, simple diagrams | partial (not blocking) | Tables with styles, merge/split, all D or P. Charts insert with the common types and edit data in the Chart Data dialog (P0 partial); SmartArt is basic (pre-drawn shapes) | 12–20 |
| Share the deck: PDF of slides, notes or handouts; images | yes | `file.export` PDF with text layer, links, bookmarks, handouts 1–9; PNG/JPEG. Artwork is raster in the PDF; no direct print (print the PDF) | 10–16 |
| Review: comments with replies, resolve, spelling | partial (not blocking) | Comments, replies and resolve work and survive `.deckcraft`; threads are flattened on `.pptx` save (#70); spelling is a small built-in list | 4–8 |

## Current focus (2026-10-10)

1. **Stop losing work and crashing:** land Close's save prompt (#60, PR #75), the Windows GPU
   startup crashes (#40, #59), image paste on Windows/Linux (#53). 6–12 h.
2. **PPTX on real decks:** build an owned corpus, round-trip checks in CI, fix what breaks;
   comment threads, media options and rewind on save (#70, #74, #77); EMF/WMF and SVG. 31–55 h.
3. **Review the open PRs:** equations (#55), browser print (#66), Morph alignment (#67), macOS
   open-with (#68), font lookup (#80), Ukrainian (#34, after the catalog exists). 4–8 h.

## Plan to beta (~120–210 h; ~40–70 h wall clock with 3–4 agents)

Beta means ~75% ready for real work and opening and saving PowerPoint's `.pptx` reliably
(craftrules `standards/progress-docs.md`). Ranked:

| # | Work | Hours | Moves |
|---|---|---|---|
| 1 | Stability: #60, #40/#59, soak and fuzz the editor | 10–20 | Stability 60 → 85% |
| 2 | PPTX real-deck corpus and fixes | 20–35 | File formats 58 → 75% |
| 3 | PPTX round-trip losses (#70, #74, #77), `.potx`/`.ppsx`/`.pptm` content types, embedded fonts | 8–15 | File formats → 80% |
| 4 | EMF/WMF and SVG pictures | 8–14 | File formats, Pictures |
| 5 | Vector PDF, printing | 10–16 | File formats, Files |
| 6 | Localization infrastructure + zh-Hans, es, fr, de, ja (with IME) | 27–46 | Localization 8 → ~40% |
| 7 | Font weights (#69), image paste (#53), Tab in text (#78) | 6–11 | Features |
| 8 | Charts: data grid, elements | 6–10 | Features |
| 9 | Animation Pane timeline, preview, effect options | 5–8 | Features |
| 10 | Presenter view on the right display, timer controls | 3–5 | Hardware, Slide show |
| 11 | Native macOS menu bar, Format Shape pane depth, start screen and window chrome fixes | 13–22 | UI/UX 60 → 75% |
| 12 | Performance baseline and the obvious fixes | 4–8 | Performance |
| **Total** | | **~120–210** | Ready ~55% → ~75% |

Parallelizes well: 1 (engine/app), 2–5 (pptx, render, pdf), 6 (ui-egui catalogs), 8–11 (UI)
touch different crates. Needs a human: the owner opening PPTX outputs in PowerPoint and making
reference renders, a Windows tester with the affected GPUs, native speakers to review translations.

## After beta: toward full parity (~245–440 h more)

SmartArt editing, chartex chart types, the remaining animation effects and motion paths, Notes and
Handout masters, edit points, text warp and 3-D, proofing dictionaries, Zoom, recording, video and
GIF export, legacy `.ppt` import and `.odp` (owner decision), GPU rasterization and hardware video
decode, the other 21 languages with RTL mirroring, a larger original template gallery, and local AI
features (owner decision on models). Itemized with hours in [gaps.md](gaps.md).

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
| M9 | SmartArt, equations, WordArt | partly | SmartArt text pane and layouts, equations (PR #55), text warps |
| M10 | Views | done | grid options, New Window |
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
| PPTX fidelity on a corpus of real decks | Still open; now beta item 2 |
| Presenter view and Animation Pane polish | Still open; beta items 9 and 10 |
| Soak and fuzz the editor | Still open; beta item 1 |
| UI fidelity pass on the most-used ribbon groups and dialogs; first run | Partly (start-screen fixes #20, #49); beta item 11 |
| Mascot app icon, README screenshots, user docs | App icon (with a scalable SVG, #29) and README screenshots in place; user docs open |

By the standard's definitions DeckCraft is **alpha** (~55% ready); these blockers now feed the beta
plan above.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Added the alpha gate (six core workflows): all pass, two with partial depth; stage stays alpha |
| 2026-10-10 | major | Created: current focus, ranked beta plan with hours, milestones M0–M14 with status, the old alpha checklist as history |
