# DeckCraft architecture

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-15 · **Change:** minor (renderer canvas, vector PDF; previous: minor, community PR wave 2026-10-14: 320 engine commands, 41 dialogs, the `metafile` crate, 1,641 tests; duplicated L2/L5/L6 rows and headers folded; previous: minor, transition frames) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

How DeckCraft is built today. The binding rules (layering, everything is a command, thin UI, never
break wasm, never crash) are in [AGENTS.md](../AGENTS.md) §3–§5; this document describes what
exists. 202k lines of Rust in 24 crates, 3 apps and `xtask`; 1,641 `#[test]` functions
(`cargo xtask stats` counts them per crate).

## Layers

`cargo xtask layers` enforces that a crate depends only on lower layers (the table in
`xtask/src/layers.rs`, which is authoritative; AGENTS.md §4 gives the simplified picture), and that
nothing below L6 uses egui, eframe, winit or rfd.

| Layer | Crates | What they do |
|---|---|---|
| L0 | `geom`, `color`, `metafile`; standalone `bitstream`, `matroska`, `ogg`, `opus` | EMU/pt units, transforms, ~180 preset shape generators re-derived from ECMA-376, adjust handles, hit testing; RGB/HSL and DrawingML colour transforms; MKV/WebM and Ogg demuxers, the Opus decoder; EMF/WMF read into a neutral vector model (`metafile`) |
| L1 | `model`, `fonts`; `h264`, `hevc`, `vp9`, `av1`, `isobmff` | Presentation, masters, layouts, slides, shapes, text, themes, tables, charts, transitions, animations, placeholder inheritance (`resolve`); font database (craft-fonts + system) with metric-compatible substitutes for Office fonts (Carlito for Calibri, Caladea for Cambria, Liberation for Arial/Times); video decoders and the MP4/MOV demuxer |
| L2 | `text`, `anim`, `format`, `media` | Shaping (HarfRust), UAX #9 bidi, UAX #14 line breaking, autofit, caret hit testing; per-shape animation state over time, the show state machine, transition frame layouts, Morph matching; `.deckcraft` (zip + JSON + media) and outline text; media probing, decoding, mixer and clock |
| L3 | `render`, `pptx` | `vello_cpu` raster of slides, masters and layouts (fills, outlines, effects, pictures, tables, charts, Morph frames); PPTX import/export from ECMA-376 |
| L4 | `pdf` | App-owned export layout and rendering through the shared `craft-pdf` writer |
| L5 | `engine` | Sessions, documents, selection, undo history on shared slide snapshots, the command registry (320 commands in `cmd/*`), pointer tools, keys, AutoRecover, the panic guard (`guard.rs`) |
| L6 | `ui-egui`, `mcp` | The PowerPoint-style interface (ribbon with 10 tabs + contextual tabs, canvas, thumbnails, panes, 41 dialogs, show and presenter windows, the JSON control channel); the MCP server (JSON-RPC 2.0 over stdio), headless or connected to a running app |
| apps | `deckcraft` (desktop: eframe/wgpu, cpal audio, logging), `deckcraft-cli` (run, render, convert, mcp, app), `deckcraft-web` (trunk; WebGPU with WebGL2 fallback), `xtask` | Exempt from layering |

The media codecs and containers are clean-room pure Rust, 59k lines, shared in spirit with
FilmCraft.

## Data model

A `Presentation` holds a theme set, slide masters with their layouts, slides, sections, custom
shows, comments, media and document properties. Shapes are a tree (`ShapeKind`: preset or custom
geometry, picture, table, chart, group, connector, ink, media, SmartArt-as-group, and `Opaque` for
anything read from PPTX that DeckCraft doesn't model: its XML is kept and written back). Text
bodies hold paragraphs of runs with DrawingML properties; formatting resolves slide → layout →
master → theme at render time. Transitions live on slides; animations are the flat ordered list the
Animation Pane shows, rebuilt into PowerPoint's timing tree on save.

## Commands and frontends

Every user-visible action is a `CommandSpec` (id, label, ribbon/menu place, shortcut, params doc,
`enabled`, `run`) in `crates/engine/src/cmd/*`. The UI, `deckcraft-cli run`, the JSON control
channel (`deckcraft --control PORT`, [control-protocol.md](control-protocol.md)) and MCP
([mcp.md](mcp.md)) all dispatch through `Session::execute`, so they behave identically.
Programmatic calls never open dialogs. Pointer gestures go through `Session::pointer`, keys through
`Session::key`.

## Rendering and the show

The renderer draws a slide to an RGBA pixmap on the CPU; the UI uploads it as an egui texture.
Transitions are layers of textured quads over the old and new slide images, or over their
backgrounds and contents rendered apart (Dynamic Content), from `anim::transition_frame`: 3-D
effects project cards through a perspective camera and `anim::layer_mesh` subdivides them so
textures stay straight; soft edges are per-corner opacities. The show renders the slides once per
transition and only composites cached textures each frame; `render::composite_transition` is the
CPU twin used by `deckcraft-cli render-transition` and the tests. The show window and the presenter view are egui viewports; animation state per shape
comes from `anim::timeline` each frame. Animations ▸ Preview plays that same state on the editor
canvas for the current slide, then draws the slide at rest. The renderer draws through a small `Canvas` trait
(`crates/render/src/canvas.rs`): `vello_cpu` for pixels, and a krilla canvas in `crates/pdf` that
writes PDF paths, paints, pictures and real text. What the renderer only draws in pixels (shapes
with blur-based effects or a fill overlay, pattern fills, rectangle and shape path gradients, text
shadows and glows, colour emoji, WMF/EMF pictures) reaches a vector canvas as a pixmap of that
element alone (`canvas::pixels`); `vector: false` keeps the older whole-slide raster with an
invisible text layer.

## File I/O

- **PPTX** (`crates/pptx`): lenient reader (unknown parts skipped, malformed parts logged and
  ignored, sizes and depths bounded, zip bombs capped), writer that PowerPoint opens without
  repair. Unknown elements round-trip through `Opaque` and `raw_ext` fields.
- **Native** (`crates/format`): zip with `mimetype`, `presentation.json` and `media/<id>`; readers
  ignore unknown fields; `.slidecraft` files from before the rename still open.
- Formats and their gaps: [file-format-parity.md](file-format-parity.md).

## Quality gates

`cargo xtask ci`: rustfmt check, clippy `-D warnings`, tests, layering, asset attribution
(`cargo xtask assets`), wasm32 check. CI runs it on Ubuntu for every PR and push to `main`;
macOS is built and tested by the release workflow only. Releases (pushes to `release`) build signed
macOS, Windows, Linux, FreeBSD and web artifacts.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-14 | minor | Community PR wave folded: one L2/L5/L6 row each; command registry 320 (`cmd!` registrations in `cmd/*`), 41 dialogs (titled in `dialogs.rs`), 202k lines, 24 crates (`metafile` added at L0), 1,641 tests. The per-PR rows below give the counts each PR saw on its own branch |
| 2026-10-14 | minor | Size and Position dialog (`shape.sizePosition`): position, size, rotation, scale and lock aspect in one undo step. 227 commands, 28 dialogs |
| 2026-10-14 | trivial | Command registry: 228 (`comment.next`, `comment.prev`) |
| 2026-10-14 | trivial | Photo Album dialog and `insert.photoAlbum` (227 commands, 28 dialogs) |
| 2026-10-14 | trivial | Edit Photo Album (`insert.photoAlbumEdit`) reopens the Photo Album dialog and re-flows those slides (228 commands, 28 dialogs) |
| 2026-10-14 | minor | Create New Theme Colors (`design.colors` with no params); command count stays 226 |
| 2026-10-14 | minor | Create New Theme Fonts (`design.fonts` with no params); command count stays 226 |
| 2026-10-14 | minor | Insert › Date and Time dialog (`insert.dateTime` with no keys). 28 dialogs; command count stays 226 |
| 2026-10-14 | minor | Command registry is 227 (`design.effects`). Dialogs stay 27. The test-count line stays 504 |
| 2026-10-14 | minor | Command registry counted at 236, including `master.preserve` and `master.placeholders`. Dialogs stay 27. The test-count line stays 504. The menu measurement and the d556ff9 snapshot still count 226 |
| 2026-10-13 | trivial | Animation preview draws the current slide's timeline on the editor canvas |
| 2026-10-14 | minor | Paragraph and Tabs dialogs; command registry 229; 29 dialogs |
| 2026-10-14 | minor | Font dialog; command registry 227; 28 dialogs |
| 2026-10-13 | minor | Transitions: perspective meshes, background/content parts, CPU compositor |
| 2026-10-12 | minor | Renderer draws through a `Canvas` trait; the PDF export is a vector canvas |
| 2026-10-10 | major | Created from the code on main (d556ff9) and AGENTS.md §4 |

The PDF writer dependency enables only `write`. Krilla handles serialization and simple-text
shaping for notes and footers; slide layout, rasterization, font selection, text mapping,
links and outlines remain app-owned. PDF embedding and all Hayro production dependencies
are disabled. Native builds enable parallel compression on the same pinned Krilla backend;
WASM keeps it disabled. Generated-PDF assertions use a std-only parser as a development dependency.
