# Local changes to the vendored engine

`vendor/equashioncraft/` is upstream equaSHion at
`fed687c6b5af025a1afc3484d6b4b167ff0679dc`, renamed to EquaSHionCraft. This file lists every
change made while vendoring, so a future update can re-apply or drop them. Everything else is
byte-identical to upstream after reversing the `equashioncraft` → `equashion` rename.

## Rename

- Crates `equashion-*` → `equashioncraft-*`; Rust module prefix `equashion_` → `equashioncraft_`;
  product name `equaSHion` → `EquaSHionCraft` in prose; environment variables `EQUASHION_*` →
  `EQUASHIONCRAFT_*`; texture-name prefix `equashion/` → `equashioncraft/`.
- The five `Cargo.toml` files depend on the DeckCraft workspace (path dependencies, workspace
  versions), set `publish = false`, and keep upstream's `too_many_arguments` and
  `type_complexity` allowances.

## Deprecation fixes (required by DeckCraft's `-D warnings` gate)

- `crates/math/src/xml.rs`: `Attribute::decode_and_unescape_value` →
  `decoded_and_normalized_value(XmlVersion::Implicit1_0, ..)` (quick-xml 0.41).
- `crates/egui/src/lib.rs`, `crates/egui/src/tests.rs`: `egui::Id::new` → `egui::Id::unique`.
- `crates/egui/src/palette.rs`, `crates/egui/src/tests.rs`: `egui::WidgetType` → `egui::Role`.

## Correctness fixes from review

- `crates/math/src/edit.rs` (`Cmd::Leaf`): advance the caret by the inserted node's unit count,
  not 1, so a typed function word (`\max`) leaves the caret after the whole word. Covered by
  `typing_a_function_word_leaves_the_caret_after_the_whole_word`.
- `crates/egui/src/gallery.rs` (`builtins_ui`): apply a ready-made equation through
  `replace_equation`, so it is an undoable edit that Esc can discard.

## Not changed

- `crates/math/src/latex.rs` prints `\middle|` for a multi-item delimiter, but the linear parser
  has no `\middle` handling, so that form does not round-trip. Fixing it needs a delimiter state
  machine in `linear.rs`; left for upstream.
