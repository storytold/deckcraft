# Where things moved in #1433

[#1433](https://github.com/storytold/deckcraft/pull/1433) (merge `ee875abf`) split the largest
source files into modules. Nothing changed in behaviour, but a branch that edits an old file now
conflicts with it. This note says which new modules received each old file's code, so you can place
your hunks by hand after `git merge origin/main`. Paths are relative to `crates/` unless noted; each
old file stays as the parent module and keeps its re-exports, so import paths are unchanged.

To find the exact new home of one function: `git grep -n 'fn name' origin/main -- crates` (or
`git log -S'fn name' --oneline origin/main -- crates`).

## Plain renames (content unchanged)

| Old | New |
|---|---|
| `engine/src/cmd/mod.rs` | `engine/src/cmd.rs` |
| `opus/src/celt/mod.rs`, `opus/src/silk/mod.rs` | `opus/src/celt.rs`, `opus/src/silk.rs` |
| `pptx/src/read/mod.rs`, `pptx/src/write/mod.rs` | `pptx/src/read.rs`, `pptx/src/write.rs` |
| `ui-egui/src/selpane.rs` | `ui-egui/src/panels/selpane.rs` |
| `ui-egui/src/backstage_tests.rs` | `ui-egui/src/backstage/tests.rs` |
| `ui-egui/src/selpane_tests.rs`, `ui-egui/src/sorter_tests.rs` | `ui-egui/src/panels/selpane/ui_tests.rs`, `ui-egui/src/sorter/ui_tests.rs` |

`ui-egui/src` files moved into a folder: `guides_tests`, `preview`, `ruler_tests`, `squiggles` →
`canvas/`; `columns_dialog_tests`, `credits`, `font_dialog_tests`, `paragraph_dialog_tests`,
`photo_album_tests`, `preferences_tests`, `size_position_tests`, `spelling_tests`,
`theme_colors_tests`, `theme_fonts_tests`, `updates_ui` → `dialogs/`; `animtimeline`,
`background_tests`, `fillui`, `notes`, `pattern_fill_tests`, `smartart_tests`, `thesaurus_tests`,
`thumbs` → `panels/`; `align_tests`, `drawtable_tests`, `effects_tests`, `icons_popup_tests`,
`inktoshape_tests`, `master_layout_tests`, `qat` → `ribbon/`.

## Split files (old file → new modules, all under the old file's folder)

Each old file keeps its module declarations and any code that was not moved; several rows move
only the tests.

| Old file | New modules |
|---|---|
| `apps/deckcraft/src/control_server.rs` | `control_server/tests.rs` (its tests) |
| `engine/src/cmd/design.rs` | `design/{colors, master_view, slide_background, slide_setup, browse_theme_tests, tests}.rs` |
| `engine/src/cmd/format.rs` | `format/{font, paragraph, painter, ruler, text_effects}.rs` |
| `engine/src/cmd/insert.rs` | `insert/{album, chart, links, picture, text_fields, icon_and_picture_tests}.rs` |
| `engine/src/cmd/shape.rs` | `shape/{appearance, freeform, picture, size}.rs` |
| `engine/src/cmd/smartart.rs` | `smartart/{diagrams, edit, layout, nodes, paint}.rs` |
| `engine/src/cmd/text.rs` | `text/{autocorrect, caret, editing, formatting, ime, layout}.rs` |
| `engine/src/tools.rs` | `tools/{click, drag, geometry, hit, pointer}.rs` |
| `fonts/src/lib.rs`, `format/src/lib.rs` | `src/tests.rs` (their tests) |
| `geom/src/preset.rs` | `preset/{build, builder, catalog, shapes, tests}.rs` |
| `media/src/player.rs` | `player/tests.rs` |
| `render/src/lib.rs` | `src/{geometry, opts, raster, renderer, slide, stroke, text_draw}.rs` |
| `text/src/lib.rs` | `src/{autonum, breaking, layout, run, shape, style, symbol}.rs` |
| `ui-egui/src/lib.rs` | `src/{app, state, services, clock, ime_hooks, tests}.rs`; the `SlideApp` impl is in `app.rs` and `app/{clipboard, commands, control, files, frame, ime, input, keys, playback}.rs` |
| `ui-egui/src/panes.rs` (deleted) | `panels.rs` and `panels/{animation, background, comments, designer, format_shape, smartart, thesaurus}.rs` (format-shape pane → `format_shape.rs`, slide background → `background.rs`, animation pane → `animation.rs`, comments and revisions → `comments.rs`) |
| `ui-egui/src/backstage.rs` | `backstage/{files, info, print}.rs` |
| `ui-egui/src/canvas.rs` | `canvas/{context_menu, guides, ime, input, media, overlays, rulers, scroll, tests}.rs` |
| `ui-egui/src/capture.rs` | `capture/tests.rs` |
| `ui-egui/src/dialogs.rs` | `dialogs/{about, alt_text, body, columns, date_time, equation, export, find, font, grid_options, hyperlink, language, paragraph, photo_album, preferences, size_position, slide_size, tabs, theme_colors, theme_fonts, trim, zoom, tests}.rs`; dialogs not listed (for example most bodies) are in `body.rs` |
| `ui-egui/src/icons.rs` | `icons/{draw, geometry}.rs` |
| `ui-egui/src/ribbon.rs` | `ribbon/{animations, arrange, chart, design, draw, galleries, home, insert, masters, picture_format, playback, review, shape_format, slide_show, smartart, table, title_bar, transitions, view}.rs`; one file per ribbon tab, plus `galleries.rs` for the shared gallery popups |
| `ui-egui/src/show.rs` | `show/{keys, media, presenter, render, ui, zoom, tests}.rs` |

Also touched: `engine/src/file_access.rs` (16 lines), `docs/file-format-parity.md`, `docs/gaps.md`
and `packaging/common.sh` (one line each, path references).

## Carrying a branch forward

1. `git merge origin/main` (never rebase) and let git list the conflicting old files.
2. For each conflicting hunk, find the new module above, apply the hunk there by hand, and keep
   the old file's side only for module declarations. `ui-egui/src/panes.rs` is deleted: do not
   restore it, move its hunks into `panels.rs` or the listed `panels/` modules (the crate exposes
   `panels` and aliases it as `panes`).
3. Run `cargo xtask ci`.
