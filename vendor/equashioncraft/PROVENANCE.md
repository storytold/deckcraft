# Provenance

equaSHion is the equation editor extracted from WordCraft, with three edit-model tests taken
from DeckCraft. It has its own git history: the files were imported with `git archive` from the
commits below, not with `git filter-repo`, so the upstream history is not carried over.

## Sources

| Repository | Commit | Branch | Role |
|---|---|---|---|
| `shuff57/wordcraft` (fork of `storytold/wordcraft`) | `dbfe1b1` | `equation-editor` | Base implementation: math tree, OMML, linear format, templates, edit model, equation layout, font database, rasteriser |
| `shuff57/deckcraft` | `7972cf5` | — | Three edit-model tests only (see below) |

Every file was read from those commits with
`git -C <repo> archive <commit> <paths> | tar -x -C <scratch>`, never from a working tree.

## What was taken

### equashion-math (`crates/math`) <- WordCraft `crates/math`

Taken whole: `lib.rs`, `edit.rs`, `edit/tests.rs`, `linear.rs`, `omml.rs`, `templates.rs`,
`tests.rs`, `xml.rs`, `Cargo.toml` (dependencies `serde`, `quick-xml`).

Changed:
- Package renamed `wordcraft-math` -> `equashion-math`; the crate doc comment no longer names
  WordCraft.
- `edit.rs`: DeckCraft's exponent rules (see DIVERGENCE.md).
- `edit/tests.rs`: three tests appended from DeckCraft `7972cf5`
  `crates/math/src/edit/tests.rs`: `a_single_digit_exponent_ends_at_the_next_term`,
  `letters_keep_going_in_an_exponent`, `a_parenthesised_exponent_is_the_group_itself`. Nothing
  else was taken from DeckCraft.

### equashion-font (`crates/font`) <- WordCraft `crates/fonts` (trimmed)

Taken: `src/fontdb.rs` (`FontFace`, `FontDb`, outlines, `fallback_for`, the system font catalog
behind `cfg(not(target_arch = "wasm32"))`), and from `src/lib.rs` `shape`, `feature`, `is_rtl`,
`first_glyph`, `ShapedGlyph` and `DEFAULT_FAMILY`, with the tests that only need bundled fonts.

Trimmed:
- `word.rs` (Word font-name resolution), `build.rs` and the craft-fonts Japanese/Chinese fonts
  (`CraftFont`, `japanese_*_fonts`, `chinese_fonts`, `ui_cjk_fonts`), `tests_sysfonts.rs`,
  `tests/system_fonts.rs`, the `variable_font_named_instances_are_styles` test (it read fonts
  from the system font folders) and the two craft-fonts tests.
- `FaceRef` (a `&'static` handle made by leaking an `Arc` per face). Scenes name faces with the
  new `FaceId(u32)` (the face's process-unique id) and backends resolve it with the new
  `FontDb::face_by_id`.
- Bundled fonts: Source Serif 4 Regular/Italic/Bold/BoldItalic and Inter Regular, byte-identical
  copies of WordCraft's `assets/fonts/` files, with their OFL texts. Source Sans 3, Source Serif
  4 Semibold, Inter Medium/SemiBold and JetBrains Mono are not bundled.
- `FALLBACK_FAMILY` (an unknown family's replacement and the first fallback face) was "Source
  Sans 3"; it is now `DEFAULT_FAMILY`, "Source Serif 4". The last-resort face is Source Serif 4
  Regular.

Later added (not from WordCraft): STIX Two Math Regular, bundled after the text faces as the
fallback for math symbols they lack (see DIVERGENCE.md and ATTRIBUTION.md).

Added: `FontDb::bundled_only()` (no font folders, system fallback off), `FaceId`,
`FontFace::face_id`, `FontDb::face_by_id`, and tests that pin the bundled family list and the
default family.

### equashion-layout (`crates/layout`) <- WordCraft `crates/layout/src/math.rs`

Taken: all of `math.rs` (`MathBox`, `SeqGeo`, `Lay`, `EqLayout`, `hit_geo`, `layout_equation`,
`layout_math_wrapped`, `MathBox::last_line_width`, the drawn delimiters, radicals, big
operators and braces, line wrapping and shrink-to-fit) and its 32 unit tests. Nothing else from
WordCraft's layout crate: `para.rs`, `display.rs`, `hit.rs`, `table.rs`, `fields.rs` and the
paragraph engine in `lib.rs` stay out.

Host coupling cut, at the smallest seam:
- `wordcraft_doc::props::Rgb` -> a local `Rgb(pub u8, pub u8, pub u8)` (`scene.rs`).
- `wordcraft_geom::Rect` (used only to emit fraction bars as `Draw::Fill`) -> `kurbo::Rect`.
- `crate::display::Draw` -> the new backend-neutral `Scene { items: Vec<Prim>, width, height }`
  with `Prim::{Glyphs, Rule, Path}`, mirroring `math.rs`'s own `Item`. `emit()` lost its `alpha`
  argument and the `Draw::Glyphs` fields `text` and `link`, which equations never set.
  `EqLayout { draws, width, height }` became `EqLayout { scene }` with `width()` and
  `height()`.
- `FontDb::global()` (5 call sites: `Faces::new`, `ink`, `Lay::pick`, `layout_equation` and
  the tests' `face()` helper) and `wordcraft_fonts::shape` (1 site, `Lay::text`) -> an explicit
  `&dyn MathFonts` argument, a trait implemented for `FontDb`. The trait has `face`, `fallback_for`, `outline`
  as specified plus `face_by_id`, which a backend needs to draw a `Prim::Glyphs`.
- `layout_math` and `layout_math_wrapped` took the base `&FontFace` only to read its family;
  they now take the family name.
- The process-wide `CACHE` static behind `layout_cached` -> an optional, host-owned
  `LayoutCache` (`LayoutCache::layout`); `layout_src` is the uncached form. `Src` and `Cached`
  are public.
- Added `layout_equation_wrapped` (`layout_equation` with a line width; the goldens use it for
  the wrapping and shrink-to-fit cases). `layout_math`, `hit_geo` and `emit` became public.
- `layout_equation` makes the canvas tall enough for every caret (see DIVERGENCE.md).

The only reach into paragraph layout was `para.rs` calling `layout_cached` and storing the
`MathBox`; nothing in `math.rs` called into `para.rs` or `display.rs` apart from `Draw`.

Tests: the 32 `math.rs` unit tests, with the same assertions, now laid out against
`FontDb::bundled_only()` in Inter as upstream. `layout_cache_returns_the_same_box` and
`emit_places_glyphs_relative_to_origin` were rewritten for `LayoutCache` and `Prim`. From
`layout/src/tests.rs`, the equation tests were rewritten to call `layout_equation` /
`layout_math_wrapped` directly (`src/tests.rs`): taller-than-text with glyphs, bar and radical
(plus the caret-width check that replaced the paragraph cluster check), hostile and broken
OMML input, wrapping at operators, display style. Not ported, because they test the paragraph
rather than the equation: `display_equation_is_centred_on_its_own_line`,
`line_before_a_display_equation_is_not_justified`, and the line-padding half of
`display_equation_line_has_room_above_and_below`.

### equashion-render (`crates/render`) <- WordCraft `crates/render/src/lib.rs` (parts)

Taken: `render_eq`, the `Draw::Glyphs` branch of `draw` (synthetic bold: the outline also
stroked at `upem * 0.03`; synthetic italic: the `-0.21` skew), the `Draw::Fill` and
`Draw::Path` branches as `Prim::Rule` and `Prim::Path`, `Rendered` with `to_straight` and
`pixel`, `MAX_SIDE`. Not taken: page, region and thumbnail rendering, images, watermarks,
lines, marks, shapes, PNG/JPEG export (and with it the `image` dependency). vello_cpu is used
single-threaded (`num_threads: 0`, as `render_eq` did) without its `multithreading` feature.

Added: `render_scene` (any scene, optional background), `glyph_transform`, and an SVG backend
(`scene_to_svg`, `fmt2`) that draws the same primitives with the same transforms.

### equashion-egui (`crates/egui`) <- WordCraft `crates/ui-egui/src/eqdialog.rs`

Imported with `git -C ~/wordcraft archive dbfe1b1 crates/ui-egui/src/eqdialog.rs`; DeckCraft's
`7972cf5:crates/ui-egui/src/eqdialog.rs` was read for comparison only. WordCraft's dialog
already had DeckCraft's Ctrl+Z/Y, Ctrl+C/X (via egui `Copy`/`Cut` events), Tab between places
and Esc-asks-before-discarding, so no DeckCraft code was taken.

Taken (into `src/lib.rs` and `src/palette.rs`): the canvas (layout, shrink-to-fit and caret
scrolling, active-place tint, selection, caret blink, placeholder text), the input mapping
(`handle_input`), mouse placement, drag and shift-click selection, the palette tabs, groups,
symbol sets and button faces (`glyph` -> `palette::label`), the linear field and display
checkbox, the Esc discard logic.

Changed:
- No host types: `WordApp`, `Session`, `app.run("insert.equation" / "equation.update")`,
  `sync_live`, `finish`, `wordcraft_doc::{Pos, Path}`, `serde_json` targets, Track Changes, the
  Insert / Update / Cancel buttons and `tl!` are gone. The host reads `EditorResponse
  { changed, committed, cancelled }` and the equation (`linear()`, `omml()`, `math()`).
- `theme::*` (`Tokens`, `theme::regular`, `theme::semibold`, `APP_COLOR`) -> `ui.visuals()`
  colours and `FontId::proportional` / `FontId::monospace`. `theme::semibold` was a
  `FontFamily::Name`, which panics on a context that never registered it.
- Palette buttons were egui text buttons (the symbols came from the host's egui fonts); they
  are now custom buttons whose faces are small equations laid out and rasterised by
  equashion-render (structures show their empty places as boxes), tinted with the visuals'
  text colour, with the old face text as their accessible label. They are clickable but not
  focusable.
- The canvas was a white texture with dark ink; the ink is now rendered white on transparent
  and tinted with `visuals.strong_text_color()` on `visuals.extreme_bg_color`.
- Ids: `Id::new("eq-canvas")` / `"eq-linear"` -> derived from a per-widget `Id`; the texture
  name `"equation-canvas"` -> `equashion/editor/<id>/canvas`, palette textures
  `equashion/palette/<key>`. The canvas texture is updated in place instead of reloaded.
- Focus: WordCraft's canvas took the keyboard back whenever the linear field did not hold it;
  it now only does so while no widget has focus, and gives up typing once another widget
  takes focus. Esc is handled only when the canvas or linear field had focus (the canvas keeps
  focus on Esc via its focus-lock filter).
- Ctrl+C / Ctrl+X also act on the key event when the platform sent no `Copy` / `Cut` event in
  that frame (kittest and some backends send only the key).
- Enter commits and resets the Esc baseline; Enter with a blank equation is reported as a
  commit (WordCraft refused it with a message); the host decides.
- Family "Cambria Math" -> `DEFAULT_FAMILY`; canvas width follows the available width.

Tests (`src/tests.rs`, 25): ported from the dialog's 40 kittest tests, the ones that do not
need the document: typing `x^2`, `_` `/` `sqrt` and Greek names, arrows and Backspace, undo /
redo, Tab, Esc asks before discarding, shift-arrows plus a palette click wrapping the
selection, the quadratic formula, typing after a palette click (now also asserting the caret
and focus), palette structures at the caret, empty places, click placement, shift-click,
linear field, hostile input, typed LaTeX, `lim_x`. New: Ctrl+C / Ctrl+X, Greek and operator
tabs, a cross-row drag, typing once under multi-pass frames and with two editors, leaving a
host text field alone, texture namespacing, the load/revert API. `tests/shared_context.rs`
runs the widget on a context with and without a host-registered `FontFamily::Name("semibold")`
and checks that fonts and styles are unchanged, plus a control test showing the named-family
panic. Not ported (they test the document or the app): insert / update / re-edit, live
editing in the page, Track Changes, Cancel undoing the live edit, the ribbon command,
Alt+=, double-click in the page, short-window layout.

### equashion-web (`apps/equashion-web`, new)

Written for this repository. Packaging follows WordCraft `dbfe1b1` `apps/wordcraft-web`
(`Cargo.toml`, `Trunk.toml`, `index.html`, and the `WebRunner` start-up and loading-message
handling of `src/web.rs`, read with `git archive`). Changed from that reference: eframe's
`glow` renderer (WebGL2) instead of `wgpu` (WebGPU with a WebGL2 fallback), no `rfd`, file
drop or `Services`; `wasm-bindgen` pinned to `=0.2.128`; output to the workspace `dist/`;
the `window.equashion` hooks are new. The look (`src/theme.rs`: colour tokens, light and dark
themes, Geist fonts) and the About tab are new and were not taken from WordCraft.

### Corpus, goldens, xtask (new)

- `corpus/equations.tsv`: 44 typed equations written for this repository.
- `corpus/omml/*.xml`: OMML taken verbatim from test fixtures (there are no `.docx`/`.pptx`
  fixture files with equations in either repository; the OMML lives in the Rust test sources):
  - `wordcraft_docx_fixture_ssup.xml` <- WordCraft `crates/docx/tests/fixtures.rs:378` (the
    `mm:oMath` element of the paragraph XML; a non-`m` prefix),
  - `wordcraft_docx_roundtrip_frac.xml` <- WordCraft `crates/docx/tests/roundtrip.rs:775`,
  - `wordcraft_docx_roundtrip_display_rad.xml` <- WordCraft `crates/docx/tests/roundtrip.rs:776`,
  - `wordcraft_docx_roundtrip_foreign_attrs.xml` <- WordCraft
    `crates/docx/tests/roundtrip.rs:933`,
  - `deckcraft_pptx_roundtrip_frac_rpr.xml` <- DeckCraft `crates/pptx/tests/roundtrip.rs:345`,
  - `deckcraft_pptx_roundtrip_frac.xml` <- DeckCraft `crates/pptx/tests/roundtrip.rs:695`.
- `tests/golden/`: generated by `cargo xtask bless`.
- `xtask`: written for this repository (`check`, `bless`, `web`, `serve` with a std-only static
  file server, `smoke`); WordCraft's xtask commands (assets,
  layers, parity, packaging, ...) were not taken. `.cargo/config.toml`, `rustfmt.toml` and
  `clippy.toml` are WordCraft's, unchanged.
- `Cargo.lock` was seeded from WordCraft's lock at `dbfe1b1`, so every shared dependency
  resolves to the same version (kurbo 0.13.1, skrifa 0.47.0, harfrust 0.13.3, quick-xml 0.38.4,
  vello_cpu 0.2.0, ...). No `cargo update` was run.

## Not taken

Everything else in WordCraft: the document model, docx/odt/rtf/html/markdown/pdf formats,
paragraph/table/page layout, the engine, MCP server, the egui app (the equation dialog was ported later as `equashion-egui`, see above), the web app,
packaging, brand assets and docs, Source Sans 3, JetBrains Mono, the hyphenation data.

## Licence

WordCraft at `dbfe1b1` is `MIT OR Apache-2.0` (`license` field of its workspace `Cargo.toml`,
`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`). `LICENSE-MIT` and `LICENSE-APACHE` are copied
unchanged; they keep the upstream copyright line ("ArtCraft Team and the WordCraft
contributors"), which both licences require to be preserved. `NOTICE` is rewritten for this
repository: it keeps that copyright line, adds "Modified by shuff57, 2026", and lists only the
third-party material bundled here.

## Galleries (`crates/math/src/gallery.rs`, `gallery.json`)

The structure galleries (fractions through matrices, 222 templates), the eight symbol sets and the
fourteen ready-made equations are WordCraft's `math_gallery.rs` at v0.5.0 (e4b7d15, Apache-2.0),
exported once through WordCraft's own OMML writer and equaSHion's OMML reader, so each entry is an
equaSHion tree. Only the ids, labels, linear text and symbols are carried; the font properties
WordCraft attaches to every node were dropped.
