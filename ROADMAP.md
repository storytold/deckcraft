# DeckCraft roadmap

DeckCraft aims at full PowerPoint parity — and to be better: faster, open (a documented zip+JSON
format plus PPTX), scriptable by agents (CLI, JSON control channel, MCP), and available everywhere
(macOS, Windows, Linux, BSD and the web).

## Status (2026-10-07)

**Working today**
- Document model: presentations, slides, masters and 11 standard layouts with placeholder inheritance
  (slide → layout → master → theme), sections, notes, comments, custom shows, header/footer fields,
  embedded media, unlimited undo built on shared slide snapshots.
- Themes: 8 original themes with colour and font schemes, custom colours/fonts, backgrounds,
  slide size presets.
- Shapes: ~150 preset geometries with adjustment handles, text boxes, pictures (with crop and
  adjustments), tables with styles, charts (column, bar, line, area, pie, doughnut, scatter and more),
  basic SmartArt, groups, connectors, ink, action buttons, WordArt.
- Text: in-place editing with caret, selection, keyboard and mouse; every Home-tab font and paragraph
  control; bullets and numbering; levels; autofit; columns; vertical text; fields; hyperlinks.
  Right-to-left and Arabic text: UAX #9 bidi (levels per paragraph, L1/L2 per line, mirrored
  brackets), contextual HarfRust shaping across style runs, `a:latin`/`a:ea`/`a:cs` slots with
  per-cluster fallback through theme script fonts, RTL start edge and bullets, `justLow` kashida,
  bidi caret/hit-testing/selection (`format.rtl`). Arrow keys still move logically (visual
  movement with caret affinity lands with the keyboard sprint).
- Editing: select, marquee, move, resize, rotate, adjust; smart guides; nudge; duplicate; z-order;
  group/ungroup; align/distribute; Format Painter; Selection pane; clipboard incl. images.
- Transitions (fade, push, wipe, split, cover, uncover, zoom, morph…) and animations (entrance,
  emphasis, exit, motion paths, triggers, by-paragraph builds) on a shared timeline engine.
- Slide show: full screen, keyboard/mouse navigation, blank screens, go-to-slide, pen, presenter
  window, rehearse timings, custom shows, Set Up Show; Reading View.
- Views: Normal (thumbnails with sections, slide, notes), Outline, Slide Sorter, Notes Page,
  Slide Master; zoom; grayscale.
- Review: comments, accessibility checker, spelling.
- Audio and video: MP3, AAC/M4A, ALAC, WAV, AIFF, CAF, FLAC, Ogg Vorbis and Opus audio; H.264,
  HEVC, VP9 and AV1 video in MP4/MOV/WebM/MKV (pure-Rust decoders); poster frames, trim, fades,
  volume, loop, rewind, play across slides, hide during show, full screen; an in-place control bar
  in the editor and `media.play/pause/stop/seek/info/posterFrame` for agents. WMA/WMV are
  recognised and embedded but not yet playable.
- PDF export: slides, notes pages and handouts (1–9 per page) with a selectable real-text layer, hyperlinks and slide bookmarks (`file.export {format: "pdf", layout}`, File › Export…).
- UI: PowerPoint-style ribbon with contextual tabs, ~240 original icons, status bar, panes, command
  palette, light/dark.
- Automation: ~200 commands, every one reachable from `deckcraft-cli`, the app's JSON control
  channel and the MCP server (headless or connected to the running app).
- Web: `apps/deckcraft-web` runs the same UI in the browser (trunk; WebGPU with WebGL2 fallback),
  opening the sample deck; Open/Insert use the browser file picker, Save/Export download.
- Release CI: pushes to `release` build a draft GitHub Release (macOS universal dmg, Windows
  x64/x86 msi + zip, Linux AppImage/deb/rpm/tar.gz + Flatpak, FreeBSD tar.gz, web zip); signing
  secrets live in the `release` environment. Version: `cargo xtask version`.

**Next (in order):** PPTX corpus hardening against real-world decks ·
Animation Pane and presenter view polish · vector PDF artwork · Format Shape pane depth (3-D,
picture/texture options) · edit points · native macOS menu bar · print · Notes/Handout masters ·
equations · SVG pictures · WMA/WMV decoding.

Milestone details live in `plan/execution-plan.md` (M0–M14, local planning notes).

## How close to an alpha (estimate, 2026-10-07)

An **alpha** here means: someone can install a signed build on macOS, Windows or Linux, make a real
deck from scratch or from a PowerPoint file, present it with transitions, animations and media,
save it back to `.pptx`/PDF without losing work, and hit no crashes on the common paths.

**DeckCraft is about 80% of the way to that alpha — roughly 30–40 wall-clock hours of a single
Claude Opus 5.5 agent** (about 12–18 hours with three agents in parallel).

| Alpha blocker | State | Estimate |
|---|---|---|
| First real release run: signing, notarization, installers verified on each OS | Done for macOS + Windows signing: v0.1.0 published (all platforms built); installers still to be hand-checked on each OS | 2 h |
| PPTX fidelity on a corpus of real decks (import, round-trip, opens without repair) | Verified on generated decks only | 8 h |
| Presenter view and Animation Pane polish (timeline, reorder, preview) | Working, rough | 6 h |
| Soak and fuzz the editor (random command sequences, big decks, undo/redo) for crashes and slowness | Unit tests and guards only | 5 h |
| UI fidelity pass on the most-used ribbon groups and dialogs; first-run experience | Mostly there | 6 h |
| Mascot app icon (owner), README/site screenshots, user docs | Placeholder icon | 3 h |

Already alpha-ready: editing, text, shapes (incl. connectors, freeform, merge), themes and
backgrounds, tables, charts, transitions, animations, slide show, audio/video playback, PDF export,
AutoRecover, CLI/MCP automation, web build.

## How far from full parity (estimate, 2026-10-07)

**Breadth: ~80% weighted** (P0 core 93%, P1 74%, P2 37%) over the 187 features of the PowerPoint catalogue, scored row by
row in [docs/parity.md](docs/parity.md) (`cargo xtask parity` recomputes it). Many features scored
done still lack some of PowerPoint's options, dialogs or pixel fidelity, so **overall parity
including depth is about 62%**.

**Remaining work to 100%: about 190 wall-clock hours of a single Claude Opus 5.5 agent** (±30%),
or roughly 65–90 hours with four agents in parallel on separate crates:

| Work | Estimate |
|---|---|
| Alpha blockers above | 35 h |
| Open P0/P1 rows (vector PDF, print, edit points, SVG, multi-monitor, draw table, SmartArt text pane, Notes/Handout masters…) | 45 h |
| Open P2 rows (equations, video export, 3-D, remove background, thesaurus, compare, record show…) | 40 h |
| Depth and pixel fidelity of every ribbon group, dialog and pane against PowerPoint | 50 h |
| Performance (incremental rendering, GPU raster, streaming media decode) | 20 h |

Basis: in this session the engine, renderer, UI, show engine, PPTX, PDF, media and release
pipeline were built in about 30 wall-clock hours with up to three agents in parallel; recent
catalogue rows (connectors, freeform, merge shapes, AutoRecover, gradients) took 1–2 hours each.

## Agents: CLI and MCP

Every command is reachable from `deckcraft-cli` (`run`, `describe`, `commands`, `app` for the running
window, `render`, `convert`), from MCP (`deckcraft-cli mcp`, optionally `--connect PORT`), and from the
app's JSON control channel (`deckcraft --control PORT`).
