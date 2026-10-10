# DeckCraft roadmap

**Stage: alpha** · next: beta, ~19 points and ~120–210 h away

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (core-workflow alpha gate added; stage stays alpha; previous: major, full re-measure against PowerPoint for Mac 16.113.4; restructured to the craftrules progress-docs standard) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

DeckCraft aims at full PowerPoint parity, and to be better: faster, open (a documented zip+JSON
format plus PPTX), scriptable by agents (CLI, JSON control channel, MCP), and available everywhere
(macOS, Windows, Linux, BSD and the web). This page is the summary; the detail is in
[docs/target-app-parity.md](docs/target-app-parity.md) and the work list in
[docs/gaps.md](docs/gaps.md).

## Headline numbers

| | Value | Kind |
|---|---|---|
| **Feature breadth** | **79%** weighted over 191 PowerPoint features (P0 92%, P1 75%, P2 36%) | measured: `cargo xtask parity` over [docs/parity-checklist.md](docs/parity-checklist.md) |
| Menu-bar coverage | 59% of 217 app-specific PowerPoint menu items matched by name (lower bound) | measured: script over the PowerPoint menu dump and the 226 commands |
| **Ready for real work** | **~56%** | estimated: weighted dimensions below |
| Hours to beta | **~120–210** Opus 5.5 agent hours (~40–70 h with 3–4 agents) | estimated, [docs/roadmap.md](docs/roadmap.md) |
| Hours to full parity | **~365–650** Opus 5.5 agent hours | estimated, [docs/target-app-parity.md](docs/target-app-parity.md) |

**Why alpha:** all six of PowerPoint's core workflows pass the alpha gate
([docs/roadmap.md](docs/roadmap.md#alpha-gate-powerpoints-core-workflows)): build, open and save
`.pptx`, present, add tables and charts, share as PDF, review. Core workflows work end to end (build a deck, open and save `.pptx`, present with
transitions, animations, media and presenter view, export PDF), but depth, fidelity and polish
have known gaps, and PPTX has never been tested on a corpus of real decks. Beta needs ~75% ready
and no blocking gap in `.pptx`: the real-deck corpus, the round-trip losses (#70, #74, #77),
EMF/SVG pictures, the data-loss and startup-crash bugs (#60, #40, #59) and the beta list in
[docs/roadmap.md](docs/roadmap.md).

## By dimension

| Dimension | Ready | Hours to full | Doc |
|---|---:|---|---|
| Features (breadth 79% measured, with depth) | ~66% | 140–240 | [docs/parity-checklist.md](docs/parity-checklist.md) |
| UI/UX fidelity | ~60% | 30–50 | [docs/ui-parity.md](docs/ui-parity.md) |
| File formats (PPTX, PDF, legacy, export) | ~58% | 60–110 | [docs/file-format-parity.md](docs/file-format-parity.md) |
| Hardware (GPU raster, video decode, displays, pens) | ~40% | 25–45 | [docs/hardware-parity.md](docs/hardware-parity.md) |
| Localization | ~8% | 55–90 + native review | [docs/localization-parity.md](docs/localization-parity.md) |
| Performance (unmeasured) | ~55% | 10–20 | [docs/hardware-parity.md](docs/hardware-parity.md#performance) |
| Stability | ~60% | 10–20 | [docs/gaps.md](docs/gaps.md#stability) |
| Platforms | ~80% | 5–10 | [Downloads in README](README.md#downloads) |
| Ecosystem (templates; add-ins out of scope) | ~25% | 10–20 | [docs/gaps.md](docs/gaps.md#ecosystem) |
| AI features | ~10% | 20–40 | [docs/gaps.md](docs/gaps.md#ai-features) |
| Automation ✱ (CLI, control channel, MCP) | ahead | 0 | [docs/mcp.md](docs/mcp.md) |

## Features

| Area | Breadth (measured) | Ready (est.) | Hours | Doc |
|---|---:|---:|---|---|
| Application shell | 71% | ~55% | 8–14 | [ui-parity](docs/ui-parity.md) |
| Slides and sections | 90% | ~85% | 3–5 | [checklist](docs/parity-checklist.md) |
| Views | 90% | ~80% | 3–6 | [ui-parity](docs/ui-parity.md) |
| Masters and themes | 85% | ~70% | 8–14 | [checklist](docs/parity-checklist.md) |
| Shapes | 86% | ~75% | 8–14 | [ui-parity](docs/ui-parity.md) |
| Format (fill, line, effects) | 81% | ~65% | 8–14 | [ui-parity](docs/ui-parity.md) |
| Arrange | 100% | ~90% | 1–2 | [ui-parity](docs/ui-parity.md) |
| Text | 80% | ~62% | 18–30 | [gaps](docs/gaps.md#features) |
| Tables | 89% | ~75% | 3–6 | [checklist](docs/parity-checklist.md) |
| Charts | 65% | ~45% | 12–20 | [gaps](docs/gaps.md#charts-data-grid-elements-and-the-newer-chart-types) |
| SmartArt | 33% | ~25% | 10–16 | [gaps](docs/gaps.md#smartart-is-pre-drawn-shapes-only) |
| Pictures | 48% | ~45% | 12–20 | [file formats](docs/file-format-parity.md) |
| Media | 67% | ~60% | 8–14 | [file formats](docs/file-format-parity.md) |
| Transitions | 92% | ~80% | 4–8 | [animation-parity](docs/animation-parity.md) |
| Animations | 79% | ~65% | 10–16 | [animation-parity](docs/animation-parity.md) |
| Slide show and presenter view | 66% | ~60% | 10–16 | [slideshow-parity](docs/slideshow-parity.md) |
| Review | 60% | ~45% | 8–14 | [gaps](docs/gaps.md) |
| Draw (ink) | 33% | ~30% | 5–9 | [hardware-parity](docs/hardware-parity.md) |
| Undo and clipboard | 88% | ~75% | 3–5 | [ui-parity](docs/ui-parity.md) |
| Files (commands; formats above) | 69% | ~50% | in file formats | [file formats](docs/file-format-parity.md) |

## Languages

English only today; no string catalog yet (Ukrainian is in review as PR #34). Detail:
[docs/localization-parity.md](docs/localization-parity.md).

| Language | Code | UI strings | Status | Hours to full |
|---|---|---:|---|---|
| English | en | 100% | full | 0 |
| Simplified Chinese | zh-Hans | 0% | none | 6–10 |
| Spanish | es | 0% | none | 3–5 |
| Hindi | hi | 0% | none | 4–7 |
| Arabic | ar | 0% (slide text: bidi and shaping done) | none | 10–16 |
| French | fr | 0% | none | 3–5 |
| Portuguese | pt | 0% | none | 3–5 |
| Indonesian | id | 0% | none | 3–5 |
| Japanese | ja | 0% | none | 4–7 |
| German | de | 0% | none | 3–5 |
| Korean | ko | 0% | none | 4–7 |
| Vietnamese | vi | 0% | none | 3–5 |

Plus 8–14 h once for the catalog infrastructure. Other languages shipped: none. PowerPoint for Mac
ships 26.

## Upcoming

Ranked; detail and the full beta plan in [docs/roadmap.md](docs/roadmap.md).

1. **No data loss, no startup crashes:** Close's save prompt (#60), Windows GPU crashes (#40, #59),
   image paste (#53), editor soak tests. 10–20 h.
2. **PPTX on real decks:** owned corpus, round-trip checks, fixes; #70, #74, #77; EMF/WMF and SVG.
   36–64 h.
3. **Localization:** catalog, system language (#15), zh-Hans, es, fr, de, ja with IME. 27–46 h.
4. **Output:** vector PDF, printing. 10–16 h.
5. **Depth that users hit:** font weights (#69), chart data and elements, Animation Pane timeline,
   presenter view on the right display, native macOS menu bar, Format Shape pane. 33–56 h.

## Agents: CLI and MCP

Every command is reachable from `deckcraft-cli` (`run`, `describe`, `commands`, `app` for the running
window, `render`, `convert`), from MCP (`deckcraft-cli mcp`, optionally `--connect PORT`), and from the
app's JSON control channel (`deckcraft --control PORT`).

## Progress log

| Date | What landed |
|---|---|
| 2026-10-10 | Progress docs restructured to the craftrules standard; full re-measure (breadth 79%, ready ~56%, alpha). Landed: Animation Painter (#54), Morph geometry outlines (#27), animated GIF playback (#39), hyperlink colour and following links in the show (#37, #45), date fields in their own format (#36), flipped shapes keep readable text (#35), show transition fix (#46), start screen from the Home button (#49), MCP export lists PDF (#65), OmaStore manifest (#57) |
| 2026-10-09 | v0.4.0 released. UAX #9 bidi and Arabic shaping (#4), Morph with Words and Characters (#5), tables grow to fit and PDF cell text (#14), rotating log file (#7), DX12 default on Windows (#16), H.264/HEVC overflow hardening (#2), PPTX sniffing anywhere in the zip (#19) |
| 2026-10-08 | v0.2.0 and v0.3.0 released: Windows arm64, Flatpak aarch64, AppImage zsync updates, branded DMG |
| 2026-10-07 | v0.1.0 released, signed and notarized; About window with contributor credits (#3); PrintCraft renamed PdfCraft |
| 2026-10-06 | Renamed SlideCraft to DeckCraft. PPTX import/export, PDF export, media playback with pure-Rust codecs, connectors with glue, freeform tools, merge shapes, AutoRecover, gradient editor, web build, release CI, parity catalogue |
| 2026-10-05 | M0: workspace, model, geometry, fonts, text, renderer, engine, MCP, CLI, PowerPoint-style egui UI, slide show engine |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Stage checked against the new core-workflow gate: passes, stays alpha |
| 2026-10-10 | major | Restructured to craftrules `standards/progress-docs.md`: stage, two numbers, dimensions, features, languages; parity estimate moved to docs/target-app-parity.md, "Working today" to its evidence section, the alpha checklist to docs/roadmap.md |
| 2026-10-07 | major | Alpha checklist (~80% to a self-defined alpha), breadth ~80%, overall ~62%, ~190 h to full parity |
