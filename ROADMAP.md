# DeckCraft roadmap

**Stage: alpha** · next: beta, ~20 points and ~120–210 h away

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-14 · **Change:** major (community PR wave 2026-10-14: ~60 landed items in the progress log, among them the Font, Paragraph, Photo Album, theme colour and font and Date and Time dialogs, presenter and show controls, the SmartArt Text Pane and PPTX corpus slices; breadth 84% after `cargo xtask parity`, ready stays ~55%, stage stays alpha; duplicated rows and headers folded; previous: minor, hours to ~95% per audience added) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

DeckCraft aims at full PowerPoint parity, and to be better: faster, open (a documented zip+JSON
format plus PPTX), scriptable by agents (CLI, JSON control channel, MCP), and available everywhere
(macOS, Windows, Linux, BSD and the web). This page is the summary; the detail is in
[docs/target-app-parity.md](docs/target-app-parity.md) and the work list in
[docs/gaps.md](docs/gaps.md).

## Headline numbers

| | Value | Kind |
|---|---|---|
| **Feature breadth** | **84%** weighted over 191 PowerPoint features (P0 92%, P1 82%, P2 55%) | measured: `cargo xtask parity` over [docs/parity-checklist.md](docs/parity-checklist.md) |
| Menu-bar coverage | 59% of 217 app-specific PowerPoint menu items matched by name (lower bound) | measured: script over the PowerPoint menu dump and the 226 commands at d556ff9 (the registry has grown since; not re-measured) |
| **Ready for real work** | **~55%** | estimated: weighted dimensions below |
| Mainstream practitioner | ~53% | estimated: weekly-use areas × discounts for interaction (×0.92), stability (×0.92) and `.pptx` exchange (×0.88), [method](docs/target-app-parity.md#mainstream-practitioner-and-essentials-user) |
| Essentials user | ~62% | estimated: core features × discounts for launch (×0.90), clarity (×0.95) and opening received files (×0.90), [method](docs/target-app-parity.md#mainstream-practitioner-and-essentials-user) |
| Hours to beta | **~120–210** Opus 5.5 agent hours (~40–70 h with 3–4 agents) | estimated, [docs/roadmap.md](docs/roadmap.md) |
| Hours to full parity | **~365–650** Opus 5.5 agent hours | estimated, [docs/target-app-parity.md](docs/target-app-parity.md) |

### Readiness by audience

| Audience | Ready | Opus 5.5 agent hours to ~95% | Work that dominates |
|---|---:|---|---|
| Full target (ready for real work) | ~55% | 340–600 (~60% parallelizes: ~120–210 h wall clock with 3–4 agents) | Feature depth across all 20 areas, legacy `.ppt`/`.odp` and video export, localization in 11+ languages, GPU raster and hardware decode, AI features |
| Mainstream practitioner | ~53% | 150–250 (~50% parallelizes: ~70–120 h with 3 agents) | `.pptx` exchange (real-deck corpus, round-trip losses, EMF/SVG, embedded fonts) 45–75 h; depth in charts, animations, pictures, text, presenting, print/vector PDF 70–110 h; interaction (menu bar, Format Shape pane, font faces, dialogs) 25–40 h; stability 10–20 h |
| Essentials user | ~62% | 50–90 (~40% parallelizes: ~30–55 h with 2–3 agents) | Opening received decks (corpus subset, EMF, SVG icons) 25–45 h; Windows launch fallback and Close prompt 6–12 h; font faces, Tab, paste, print, show navigation 12–20 h; start screen, menu bar, palette 6–10 h |

Calibration is the same as for the full number (see [docs/target-app-parity.md](docs/target-app-parity.md#calibration-how-the-hours-were-derived)): a fix is 0.5–1 h, a checklist row to PowerPoint depth 2–4 h, a subsystem 6–35 h. The sets are nested, so essentials ≤ mainstream ≤ full. The full number is an additive weighted sum over the dimension table; only mainstream and essentials use multiplicative discounts.

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
| Features (breadth 84% measured, with depth) | ~62% | 140–240 | [docs/parity-checklist.md](docs/parity-checklist.md) |
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
| Application shell | 81% | ~55% | 8–14 | [ui-parity](docs/ui-parity.md) |
| Slides and sections | 92% | ~85% | 3–5 | [checklist](docs/parity-checklist.md) |
| Views | 98% | ~80% | 3–6 | [ui-parity](docs/ui-parity.md) |
| Masters and themes | 92% | ~70% | 8–14 | [checklist](docs/parity-checklist.md) |
| Shapes | 91% | ~75% | 8–14 | [ui-parity](docs/ui-parity.md) |
| Format (fill, line, effects) | 83% | ~65% | 8–14 | [ui-parity](docs/ui-parity.md) |
| Arrange | 100% | ~90% | 1–2 | [ui-parity](docs/ui-parity.md) |
| Text | 83% | ~62% | 18–30 | [gaps](docs/gaps.md#features) |
| Tables | 97% | ~75% | 3–6 | [checklist](docs/parity-checklist.md) |
| Charts | 65% | ~45% | 12–20 | [gaps](docs/gaps.md#charts-data-grid-elements-and-the-newer-chart-types) |
| SmartArt | 50% | ~35% | 8–14 | [gaps](docs/gaps.md#smartart-editing-is-shallow) |
| Pictures | 60% | ~45% | 12–20 | [file formats](docs/file-format-parity.md) |
| Media | 67% | ~60% | 8–14 | [file formats](docs/file-format-parity.md) |
| Transitions | 100% | ~80% | 4–8 | [animation-parity](docs/animation-parity.md) |
| Animations | 85% | ~65% | 10–16 | [animation-parity](docs/animation-parity.md) |
| Slide show and presenter view | 68% | ~60% | 10–16 | [slideshow-parity](docs/slideshow-parity.md) |
| Review | 70% | ~45% | 8–14 | [gaps](docs/gaps.md) |
| Draw (ink) | 50% | ~30% | 5–9 | [hardware-parity](docs/hardware-parity.md) |
| Undo and clipboard | 100% | ~75% | 3–5 | [ui-parity](docs/ui-parity.md) |
| Files (commands; formats above) | 75% | ~50% | in file formats | [file formats](docs/file-format-parity.md) |

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
2. **PPTX on real decks:** the owned corpus now round-trips comments, links and actions, custom
   shows, rotated groups, inherited placeholders, text effects and body layout, theme effects,
   fills, bevels, connectors, tables, charts, animations, media, sections, masters, 3-D models
   and animation bounce end. Still open: a corpus of real decks, #70, #74, #77, EMF+ records,
   MicroType Express font subsets and SVG. 36–64 h.
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
| 2026-10-18 | Equations: OMML equations from PowerPoint are drawn in two dimensions (fractions, scripts, radicals, big operators, brackets, matrices). Insert ▸ Equation turns the selected text into an equation or types a new one in place, in its linear form (`a/b`, `x^2`, `\alpha`), shown as text while the caret is in it; Right Arrow at its end leaves it. Home ▸ Font sets an equation's font, kept in .deckcraft and .pptx files (#148, #296, #604) |
| 2026-10-17 | Opening a file caps absurd effect sizes (glow, soft edges, shadows, reflections, on shapes, text and table text) at what Format Shape allows: a glow of 10^18 pt in a .pptx made rendering ask for 34 GB and abort |
| 2026-10-17 | Opening a .deckcraft file whose shapes repeat an id gives the repeats fresh ids (groups, layouts and masters too), so the deck validates and edits by id find one shape |
| 2026-10-17 | Edit Points: turning a shape into a freeform is one undo step, so Undo brings the original shape back (it used to stay a freeform) |
| 2026-10-17 | While a placeholder's text is being edited, a click on a shape in front of it selects that shape instead of moving the caret, as in PowerPoint (#931) |
| 2026-10-15 | PPTX trigger sequences keep separate On Click steps, including paragraph builds; synthetic import/edit/save/reopen and headless show regressions |
| 2026-10-16 | Export: PNG with a transparent background — the slide background is left out, only the slide's content is drawn (Export… dialog, File › Export, web download, `file.export`/`file.render` `transparentBackground`, `deckcraft-cli render --transparent`) (#784) |
| 2026-10-16 | Arrange ▸ Align: Align to Slide and Align Selected Objects, as in PowerPoint. One object aligns to the slide and several to each other until the other choice is ticked; the choice holds for that selection and also drives Distribute |
| 2026-10-15 | CLI: .txt and .md export includes table cell text, which it used to skip silently (#814) |
| 2026-10-15 | A `.pptx` slide that can't be read is reported, not dropped in silence: `lostSlides` from `file.open`, a message and a repaired copy in the app, a warning from `deckcraft-cli` (#203) |
| 2026-10-16 | AutoFormat as you type: ordinals (1st, 22nd, 103rd) get superscript suffixes after a space or punctuation; Tools ▸ AutoCorrect and `text.autocorrect {ordinals}` turn it off |
| 2026-10-16 | A PowerPoint Show (`.ppsx`) opened from the command line or the file association starts its slide show, as in PowerPoint; File › Open still opens it for editing (#862) |
| 2026-10-15 | PDF export and rendering: `anchorCtr` centres the drawn text block — right-aligned text no longer ran past the box edge (#736) |
| 2026-10-14 | connected MCP rejects unrelated, malformed, incomplete and oversized replies without replay; buffered unsolicited bytes force a fresh connection before sending (#718) |
| 2026-10-14 | Empty bulleted and numbered paragraphs show gray markers while editing, including on first click and after Enter. Markers use the list style and stay out of slide shows and exports. |
| 2026-10-13 | Interface font fallback continues to eligible installed faces when the first covering face is colour or exceeds the 48 MiB limit; document fallback order and the CJK/IME rendering assertions are retained (#101) |
| 2026-10-13 | PDFW2: adopt the shared writer with native unwind recovery and explicit font-embedding failures; preserve app-owned export behavior and keep reader dependencies out of production |
| 2026-10-13 | Eyedropper: Shape Fill, Shape Outline, Font Color and every other colour menu end with an Eyedropper that takes the colour drawn at a point of the slide (hover shows it, a click applies it, Esc cancels); `slide.colorAt` gives the same from scripts and MCP (#367) |
| 2026-10-13 | Slide show: Honeycomb's hexagons cover the whole slide again — the bottom quarter stayed black until the endpoint (#420) |
| 2026-10-13 | Copy and Cut from the ribbon, the slide's right-click menu and the menus put the text on the system clipboard, as Ctrl+C and Ctrl+X do (#431) |
| 2026-10-13 | Video seeks and loops keep newly decoded frames and end-of-stream notifications; stale generations are filtered when consumed (upstream #416). Deterministic queue coverage and repeated seeks across H.264, HEVC, VP9 and AV1 |
| 2026-10-14 | Presenter View handles the same keyboard navigation as the audience window, consumes handled keys before focused buttons, and remains available at the end screen. Regression tests cover presenter-only input, single-step navigation and Escape |
| 2026-10-13 | EMF and WMF rectangle clip regions (`INTERSECTCLIPRECT`, `EXCLUDECLIPRECT`, `EXTSELECTCLIPRGN`) limit later drawing. EMF+ records are still skipped |
| 2026-10-13 | Embedded fonts (`p:embeddedFontLst`) are read, written and used to draw. TrueType, OpenType and Embedded OpenType faces load; MicroType Express subsets are kept for the round trip and still draw in a stand-in |
| 2026-10-13 | Codec parsers (#426): H.264 and HEVC quantiser offsets, Matroska block times, Opus granule timing and QuickTime PCM frame sizes are range-checked before the arithmetic, so a hostile number is a format error instead of an overflow |
| 2026-10-13 | WordArt Transform: Shape Format ▸ Text Effects shows the 40 text warps and No Transform; `format.textWarp` applies one to the selected text. Adjust handles are still open |
| 2026-10-13 | PPTX corpus round-trip for decks we author (open → save → reopen, package parts and text). A notes master's header and footer are kept instead of being replaced by an empty notes master |
| 2026-10-13 | PPTX round trip of comments, hyperlinks and actions, custom shows, rotated groups, and placeholders that inherit from the layout. Paragraph default formatting (`defRPr`, including a link) and action sounds (the clip, stop-previous, and a custom show's "Show and return") are kept. Embedded fonts and comment threads stay open |
| 2026-10-13 | PPTX round trip of text effects, autofit, columns, vertical and RTL text, tab stops, line spacing, theme effects, gradient and pattern fills, 3-D bevels and connectors. A run keeps its inner shadow, reflection and soft edge, and a bevel preset with no kept 3-D XML is written back. Embedded fonts and #70, #74, #77 stay open |
| 2026-10-14 | PPTX: an owned corpus round-trips SmartArt and OLE kept as XML, audio and video bookmarks and trims, a section with a hidden slide, slide-number and date fields, ink (as freeform lines), 3-D models (`.glb`, including the fallback picture) and animation repeat, bounce end and triggers. Relationship-bearing SmartArt and OLE (#203), playback options (#74) and rewind (#77) stay open |
| 2026-10-14 | Chart trendlines are drawn: linear, exponential, logarithmic, polynomial, power and moving average, with forecast and the equation and R² label. The value axis grows to include the fit |
| 2026-10-14 | Chart trendlines round-trip (type, polynomial order, moving-average period, forecast, intercept, equation and R²). An owned corpus slice also checks combo charts with data labels, merged tables with diagonal borders, and text columns, autofit, vertical text and a WordArt warp |
| 2026-10-14 | Insert › Icons: a gallery of Lucide stroke icons (release 1.56.0) inserted as custom shapes, no fill, round stroke scaled from the 24-unit design |
| 2026-10-14 | Size and Position dialog: height, width, rotation, scale and lock aspect ratio in one undo step, from Shape Format › Size and the shape menu |
| 2026-10-14 | Review: Previous and Next comment walk every comment in the deck; the Comments pane lists every slide and highlights the one you landed on |
| 2026-10-13 | View › New Window opens a second window on the same presentation. The window has its own slide and shows edits made in the first window. Arrange All is not included |
| 2026-10-14 | File › New starter decks: Briefing, Lesson, Pitch, Field Notes, Quarterly and Evening Talk, original text on our themes (`file.new {template}`, `file.templates`). The large template gallery is still open |
| 2026-10-14 | Slide Sorter sections: Add Section, rename in place, collapse a section, and drag the bar to move that section and its slides |
| 2026-10-14 | Text: an input method's composition in a table cell is underlined in place, with the caret and candidate window in that cell |
| 2026-10-14 | Edit Photo Album (`insert.photoAlbumEdit`): reopen the album's slides and re-flow them in one undo step, with black and white, rotation, brightness, contrast and an album theme |
| 2026-10-14 | Photo Album dialog (`insert.photoAlbum`): pictures and text boxes become slides in the open presentation, one undo step |
| 2026-10-14 | Design › Colors › Customize Colors… opens Create New Theme Colors: the open master's scheme name and twelve theme colours, applied in one undo |
| 2026-10-14 | Design › Fonts › Customize Fonts… opens Create New Theme Fonts: the open master's scheme name and its heading and body latin faces, applied in one undo |
| 2026-10-14 | Alt Text: Mark as decorative opens checked when the selection already is, so OK no longer clears it |
| 2026-10-14 | Insert Hyperlink: address, email, slide and ScreenTip open from the selection, so OK no longer replaces them with `https://` and an empty tip |
| 2026-10-14 | Insert › Date and Time: the ribbon and Insert menu open a format list (datetime1–datetime13) with a language and Update automatically. OK inserts one field, or the shown text when Update is off. English has every format; en-GB, French, Japanese and Swedish list the numeric date and 24-hour time |
| 2026-10-14 | View: the Zoom dialog takes a percent or Fit and applies it on OK. View ▸ Zoom opens it; the arrow still lists the presets |
| 2026-10-13 | PPTX corpus round-trip for tables, charts, animations, media, sections and masters/layouts (open → save → reopen, package parts and text). A handout master's header, footer, date and slide number are kept |
| 2026-10-13 | Playback stays with its document and its clip (#415): switching decks or closing the last one stops the previous sound, replacing a clip plays the new one, and the duration and decode caches keep the bytes they are keyed by |
| 2026-10-13 | Slide show: a laser pointer (Cmd/Ctrl+L, the show's laser colour, no ink). Checklist row Partial. The presenter window does not move the dot |
| 2026-10-14 | Slide Show: Show Media Controls writes the show setting and hides the playback bar and GIF button during the show |
| 2026-10-14 | During a show, a typed slide number is shown and Enter goes to that slide |
| 2026-10-14 | A custom show plays only its slides: Show in the Custom Shows dialog, Set Up Slide Show, or a click action |
| 2026-10-14 | Presenter view pauses and resets its timer (T, R) and changes notes size (A-/A+, brackets). The clock shows local time. Second-display placement stays with #433 |
| 2026-10-14 | See All Slides and zoom during a show: a thumbnail grid (G, or All), click to jump, and click-to-zoom at 2× (Z). Presenter view has Black, White, All slides and Zoom |
| 2026-10-13 | The command palette skips commands that need a value, so searching for Export runs the export dialog (#79) |
| 2026-10-14 | PNG and JPEG slide export record their resolution. The default 2 pixels per point is 144 dpi, so Preview and print show the slide's real size instead of twice that |
| 2026-10-14 | Design ▸ Effects: named effect schemes (DeckCraft, Soft, Crisp, Lift, Glow, Plain) replace the theme's fills, lines and effects and leave colours and fonts. `design.effects` |
| 2026-10-14 | Slide Master: Preserve on the open master or layout, and Title and Footers placeholder checkboxes. Two commands (`master.preserve`, `master.placeholders`) |
| 2026-10-14 | Trim Media: start, end and fades open from the selected clip, so OK no longer resets them to zero |
| 2026-10-14 | Format Background: a solid fill's transparency, and for a picture transparency, tile, offset, scale, alignment and mirror. Apply to All copies a picture background onto every slide. The same `design.background` command; dialogs stay 27 |
| 2026-10-13 | Editor slide rasters sit on the physical pixel grid, so small text stays sharp on a Retina display instead of being blurred by a linear filter between pixels (#184) |
| 2026-10-14 | Copy and paste with other apps: bold, italic and underline travel as HTML (Pages keeps them); a shape with no text still copies as DeckCraft objects so Ctrl+V pastes it (#182) |
| 2026-10-13 | Animations ▸ Preview plays the current slide's animations on the editor canvas (Play All does the same); Escape or a click stops it. Checklist row Preview in editor re-scored done |
| 2026-10-14 | A two-finger twist on the trackpad rotates the selected shapes, as one undo step. Pinch-zoom is unchanged |
| 2026-10-14 | A trackpad swipe turns the page: sideways when the slide fits the window, and any flick during the slide show. One gesture turns one page. Pinch-zoom is unchanged |
| 2026-10-14 | Format Shape: the Fill transparency slider shows the selected solid fill, and leaving it no longer clears that transparency |
| 2026-10-13 | Web file picker: a picture, sound, video or outline finishes on the document and slide that asked for it, and is dropped if that presentation was closed or the picker dismissed (#414) |
| 2026-10-14 | Paragraph dialog (Home › Paragraph launcher): alignment, first-line and hanging indents, spacing before and after, line spacing, and a Tabs dialog, each applied in one undo step |
| 2026-10-14 | Font dialog (Home › Font, the shape menu, Cmd+T): family, size, bold, italic, underline, colour, strikethrough, baseline, caps and character spacing, applied in one undo step |
| 2026-10-13 | Grid Options: spacing, snap to grid, snap to other objects, and whether gridlines and guides are drawn; the on-screen grid uses that spacing |
| 2026-10-13 | WordArt adjust handles: a selected text warp shows one yellow handle per adjust, and dragging it sets that guide value (one undo step) |
| 2026-10-14 | Draw: pen pressure scales ink width when the pointer reports it (a mouse or a missing reading stays at the pen width; a stroke saved as a flat 0.5 still draws at the pen width). Tilt is not read |
| 2026-10-13 | Saving a presentation keeps the embedded workbook behind an unchanged chart, and the chart part's relationships (#335). Editing the chart drops that workbook instead of writing numbers that no longer match it |
| 2026-10-13 | A presentation whose object ids are used up refuses to grow instead of handing out duplicate ids (#407) |
| 2026-10-13 | Slide transitions like PowerPoint: every transition and option measured frame by frame against PowerPoint's video export and retuned (curves, directions, soft edges, 3-D perspective, contents moving over backgrounds); PowerPoint's own XML elements and defaults read and written (diagonal Wipe, Zoom, Shape In/Out, Glitter Hexagons, Shred In/Out, Page Curl Single); `deckcraft-cli render-transition` renders frames headless |
| 2026-10-13 | Wild text cases measured against PowerPoint (102 synthetic slides): bidi by run language, East Asian vertical text upright, mongolianVert/wordArtVert/wordArtVertRtl, underline styles and strikethrough at the font's metrics, centre/right/decimal tabs, East Asian, Thai, Hindi, Arabic and Hebrew numbering, small caps, all-caps "ß", transparent text, soft hyphens, kinsoku, rtlCol and just/dist anchoring, emoji presentation, fallback faces that no longer move the baseline |
| 2026-10-13 | Render matches PowerPoint more closely on wild shape, picture and table cases: even-odd custom paths, dashes and caps, arrowheads, radial and path gradients, tile rects, rotWithShape, gradient lines, shadows cast by what is drawn (group and perspective too), group fill, picture borders, tiles, reflection, recolour (biLevel, alphaInv, brightness/contrast), table borders and grid default, 20+ preset shapes by their ECMA formulas, default miter joins, and approximate 3-D bevels, cameras and face lighting |
| 2026-10-13 | Slide show for interactive decks: click-triggered animations play (one step per click, repeating, applied in click order), a slide the show returns to keeps its state and its triggered animations go on where they stopped, shapes an animation has hidden cannot be clicked, a shape without an action blocks the one beneath it, hand cursor over clickable shapes, "advance on click" off respected |
| 2026-10-12 | SmartArt import: text laid out on its `dsp:txXfrm` text rectangle, not squeezed into the shape (list layouts no longer show text in narrow columns) |
| 2026-10-12 | Title bar: Export button next to Save, opening the Export dialog on every platform (a shortcut beside File › Export) |
| 2026-10-13 | PPTX export writes modern threaded comments (`p188`) next to the flattened legacy part; replies and resolved state survive a round trip, and real PowerPoint decks stop importing their comments twice (#70) |
| 2026-10-12 | Canvas: a right click selects the shape under the pointer before the context menu opens (`contextclick` pointer kind for the control channel and MCP); in edited text the menu offers text items |
| 2026-10-12 | Canvas: dragging the frame of a text box being edited moves it (move cursor on the frame, one undo step); the caret zone follows rotated boxes |
| 2026-10-13 | Start screen: a missing Recent file is marked and its click shows the error there; entries can be removed from the list (#172) |
| 2026-10-12 | Tables: the caret and text selection show in the cell being edited; clicks and double-clicks place the caret in the clicked cell (and select the word); dragging inner column borders, the right edge and row borders resizes them in one undo step; Tab skips cells hidden under a merge |
| 2026-10-12 | Objects off the slide show and can be selected and moved around it in Normal view, as in PowerPoint; the show, exports and thumbnails still clip to the slide (#270) |
| 2026-10-12 | Korean, Japanese and Chinese input: IME composition shown in place and underlined, one undo step per composition, candidate window at the caret (slide text, notes, fields); interface text (title, notes, outline, sections, comments) borrows installed fonts for scripts the interface fonts lack (#101) |
| 2026-10-12 | Selection Pane like PowerPoint (#133): front-most first, groups fold, drag and drop restacks among siblings, Bring Forward / Send Backward, Show All / Hide All, eye, rename in place, keyboard; Lock (pane padlock, canvas menu) keeps objects from moving, sizing or rotating and round-trips PowerPoint's locking attributes |
| 2026-10-12 | Text keys like PowerPoint for Windows (#268): Ctrl+arrows and Ctrl+Shift+arrows by word (punctuation, CJK, emoji, RTL), Ctrl+Backspace/Delete with smart spacing, Ctrl+Up/Down by paragraph, Ctrl+Home/End, End on wrapped lines, Ctrl+]/[ and Ctrl+Shift+>/< font size, Shift+F3 case cycle, word and paragraph selection |
| 2026-10-12 | EMF/WMF pictures draw their text (fonts, alignment, dx advances, escapement, symbol fonts), pattern and hatch brushes, PATBLT and the XOR/AND see-through shadow; legacy OLE objects show their VML preview and ActiveX controls their pictures |
| 2026-10-12 | File tab (Backstage) like PowerPoint's (#76): File or Alt+F opens Home, New, Open, Info, Save, Save As, Print, Export, Close, Account and Options over the window, Esc or Back returns, arrows walk the side menu; the start screen is File › Home. Pages run the existing file commands; new `file.info`, print slide ranges (`"1-3, 5"`), Print through a PDF, Ctrl+P, and Close/Ctrl+W ask about unsaved changes |
| 2026-10-12 | WordArt text warps (`prstTxWarp`): all 40 presets drawn, curves fitted to PowerPoint by observation; warp adjusts round-trip |
| 2026-10-12 | Render: line widths, dashes and arrowheads inside a group no longer scale with the group's child-coordinate mapping, so a group in a small legacy child space (scale about 1600) draws thin lines instead of covering the slide (#233) |
| 2026-10-12 | `deckcraft-cli help` and each subcommand's `--help` explain the CLI to an agent; `describe` spells out a command's parameters and what it acts on; `commands` lists one command per line |
| 2026-10-11 | Animated GIF export of the deck: the Export… dialog (Animated GIF), `file.export` format `gif`, CLI and MCP (#171) |
| 2026-10-10 | Progress docs restructured to the craftrules standard; full re-measure (breadth 79%, ready ~55%, mainstream ~53%, essentials ~62%, alpha). Landed: Animation Painter (#54), Morph geometry outlines (#27), animated GIF playback (#39), hyperlink colour and following links in the show (#37, #45), date fields in their own format (#36), flipped shapes keep readable text (#35), show transition fix (#46), start screen from the Home button (#49), MCP export lists PDF (#65), OmaStore manifest (#57) |
| 2026-10-09 | v0.4.0 released. UAX #9 bidi and Arabic shaping (#4), Morph with Words and Characters (#5), tables grow to fit and PDF cell text (#14), rotating log file (#7), DX12 default on Windows (#16), H.264/HEVC overflow hardening (#2), PPTX sniffing anywhere in the zip (#19) |
| 2026-10-08 | v0.2.0 and v0.3.0 released: Windows arm64, Flatpak aarch64, AppImage zsync updates, branded DMG |
| 2026-10-07 | v0.1.0 released, signed and notarized; About window with contributor credits (#3); PrintCraft renamed PdfCraft |
| 2026-10-06 | Renamed SlideCraft to DeckCraft. PPTX import/export, PDF export, media playback with pure-Rust codecs, connectors with glue, freeform tools, merge shapes, AutoRecover, gradient editor, web build, release CI, parity catalogue |
| 2026-10-05 | M0: workspace, model, geometry, fonts, text, renderer, engine, MCP, CLI, PowerPoint-style egui UI, slide show engine |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | trivial | Progress log: PPTX trigger click-step round trip. Feature scores and milestone names unchanged |
| 2026-10-16 | trivial | Progress log: PNG export with a transparent background (#784) |
| 2026-10-16 | trivial | Progress log: a `.ppsx` from the system starts the slide show (#862) |
| 2026-10-14 | trivial | remote MCP checks reply IDs, envelopes and bounded newline framing (#718) |
| 2026-10-14 | minor | Gray list markers on empty paragraphs while editing; caret keeps the list indent before typing |
| 2026-10-14 | minor | Community PR wave folded: headline breadth 84% over 191 features (P0 92%, P1 82%, P2 55%) and the feature-area breadths from `cargo xtask parity`; one row per area; Upcoming item 2 merged; per-PR count notes dropped from the progress log. Ready stays ~55%, stage alpha |
| 2026-10-13 | trivial | Progress log: Eyedropper in the colour menus, slide.colorAt (#367) |
| 2026-10-13 | trivial | Progress log: video seek frame-loss race and regression coverage |
| 2026-10-14 | minor | Presenter keyboard navigation and end-screen controls |
| 2026-10-13 | trivial | Progress log: EMF/WMF rectangle clip regions |
| 2026-10-13 | trivial | Progress log: embedded fonts |
| 2026-10-13 | trivial | Progress log: codec parsers reject overflowing numeric fields (#426) |
| 2026-10-13 | trivial | Progress log: owned PPTX corpus round-trip; notes-master header and footer |
| 2026-10-13 | trivial | Progress log: PPTX corpus for comments, links, custom shows, groups and inherited placeholders |
| 2026-10-13 | trivial | Progress log: PPTX corpus for text effects, body layout, theme effects, fills, bevels and connectors |
| 2026-10-14 | trivial | Progress log: PPTX corpus for 3-D models, media bookmarks and animation bounce end |
| 2026-10-14 | trivial | Progress log: chart trendlines are drawn |
| 2026-10-14 | trivial | Progress log: chart trendlines |
| 2026-10-14 | trivial | Progress log: Lucide icon gallery |
| 2026-10-14 | trivial | Progress log: Size and Position dialog. Breadth stays 79%, ready stays ~55% |
| 2026-10-14 | trivial | Progress log: previous and next comment |
| 2026-10-13 | trivial | Progress log: View › New Window |
| 2026-10-14 | trivial | Progress log: six original starter decks |
| 2026-10-14 | trivial | Progress log: Slide Sorter sections |
| 2026-10-14 | trivial | Progress log: IME underlines in table cells |
| 2026-10-14 | trivial | Progress log: Photo Album dialog. Breadth stays 79%, ready ~55% |
| 2026-10-14 | trivial | Progress log: Edit Photo Album. Breadth stays 79%, ready ~55% |
| 2026-10-14 | trivial | Progress log: Create New Theme Colors |
| 2026-10-14 | trivial | Progress log: Create New Theme Fonts |
| 2026-10-14 | trivial | Progress log: Alt Text decorative checkbox |
| 2026-10-14 | trivial | Progress log: Hyperlink dialog follows the selection |
| 2026-10-14 | trivial | Progress log: Insert › Date and Time dialog (28 dialogs, 226 commands; breadth and readiness unchanged) |
| 2026-10-14 | trivial | Progress log: Zoom dialog |
| 2026-10-13 | trivial | Progress log: owned PPTX corpus for more deck kinds; handout-master text |
| 2026-10-13 | trivial | Progress log: media playback follows the open document and clip (#415) |
| 2026-10-13 | trivial | Progress log: laser pointer. Headline P2 36% → 39% to match the checklist (the row moves P2 unweighted 38% → 39%; breadth stays 79%) |
| 2026-10-14 | trivial | Progress log: Show Media Controls |
| 2026-10-14 | trivial | Progress log: go to slide shows the typed number. Breadth stays 79% |
| 2026-10-14 | trivial | Progress log: custom show playback. Breadth stays 79% |
| 2026-10-14 | trivial | Progress log: presenter timer pause/reset, clock and notes size. Breadth stays 79% |
| 2026-10-14 | trivial | Progress log: See All Slides and zoom during a show. Breadth stays 79% |
| 2026-10-13 | trivial | Progress log: command palette Export (#79) |
| 2026-10-14 | trivial | Progress log: Design ▸ Effects |
| 2026-10-14 | trivial | Progress log: Slide Master Preserve and Title/Footers placeholders. Percentages unchanged |
| 2026-10-14 | trivial | Progress log: Trim Media dialog follows the selection |
| 2026-10-14 | trivial | Progress log: Format Background picture and solid options |
| 2026-10-13 | trivial | Progress log: editor rasters on the Retina pixel grid (#184) |
| 2026-10-14 | trivial | Progress log: copy and paste rich text and pictures with other apps |
| 2026-10-13 | trivial | Progress log: animation preview on the editor canvas; breadth 80%, Animations 85% |
| 2026-10-14 | trivial | Progress log: Format Shape fill transparency follows the selection |
| 2026-10-13 | trivial | Progress log: web file picker keeps its document and slide (#414) |
| 2026-10-14 | trivial | Progress log: Paragraph dialog |
| 2026-10-14 | trivial | Progress log: Font dialog |
| 2026-10-13 | trivial | Progress log: Grid Options dialog |
| 2026-10-14 | trivial | Progress log: pen pressure scales ink width |
| 2026-10-13 | trivial | Progress log: chart workbook round-trip (#335) |
| 2026-10-13 | trivial | Progress log: wild text cases |
| 2026-10-13 | trivial | Progress log: slide show for interactive decks |
| 2026-10-12 | minor | Progress log: table cell editing and border resizing |
| 2026-10-12 | trivial | Progress log: File tab (Backstage, #76) |
| 2026-10-12 | trivial | Progress log: lines in scaled groups (#233) |
| 2026-10-11 | minor | Progress log: animated GIF export (#171); P2 breadth 36% → 37%, overall breadth stays 79% |
| 2026-10-10 | minor | Readiness-by-audience table with hours to ~95% for full, mainstream and essentials |
| 2026-10-10 | minor | Added mainstream practitioner (~53%) and essentials user (~62%); features recomputed from written area weights, ready ~56% → ~55% |
| 2026-10-10 | minor | Stage checked against the new core-workflow gate: passes, stays alpha |
| 2026-10-10 | major | Restructured to craftrules `standards/progress-docs.md`: stage, two numbers, dimensions, features, languages; parity estimate moved to docs/target-app-parity.md, "Working today" to its evidence section, the alpha checklist to docs/roadmap.md |
| 2026-10-07 | major | Alpha checklist (~80% to a self-defined alpha), breadth ~80%, overall ~62%, ~190 h to full parity |
