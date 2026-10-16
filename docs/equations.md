# Structured equations and inline editing

This integration combines the structured math and galleries from
[PR #192](https://github.com/storytold/deckcraft/pull/192) with the paragraph editing behavior
from [PR #771](https://github.com/storytold/deckcraft/pull/771), using its subsequent native-OMML
port (`761790de627651a61d9747e62f919fcbe918fbe4`) as the interaction reference. It preserves
#192's commit ancestry and includes upstream main through
`9859da39255c0262cd592bb57101d7f2fa961f72`.

## Using equations

- Place a text caret in a paragraph or table cell, then choose Insert → Equation (or Alt+=).
  The equation is inserted there; selected prose is replaced. With no text caret, a new
  equation box is created.
- Type in the shaded field. `a/b` makes a fraction, `x^2` makes a superscript, and `\alpha`
  becomes α. Unknown commands remain literal. The placeholder is an editor hint, never saved
  as equation content.
- The Equation tab contains structures, symbols and built-in equations. Tab moves through
  slots. Click a numerator, denominator or script to place its caret.
- Right Arrow at the root equation's end returns to normal text without inserting a space.
  From an inner slot it first moves out of that slot. Left Arrow at a neighboring equation
  re-enters it. Normal text retains the surrounding prose's formatting.
- Undo/Redo, selection, copy/cut, native saves and PPTX saves use the session's document model.
  Clearing an existing equation updates the document immediately.
- To change an equation's typeface, click into it, open Home, and choose a family in Font.
  Latin Modern Math provides the traditional LaTeX appearance. The choice applies to the
  entire equation, including an empty field, and survives undo/redo and native/PPTX saves.
  Inline equations inherit the surrounding font initially; separate equation boxes start
  in STIX Two Math. Unavailable families and missing symbols fall back to the bundled math
  fonts. Installed and session-added families can also be selected on desktop.

## Build

Use DeckCraft's pinned craft-fonts checkout (`43913056ce82a3fdad0ef69f6b88d1dc922a8104`) and,
before building, prepare the math fonts:

```sh
python3 scripts/equation-fonts.py ../craft-fonts
cargo run -p deckcraft -- --sample
```

A different font directory can be passed to the script and supplied as `CRAFT_FONTS_DIR`.
The script prepares STIX Two Math v2.13b171 (OFL-1.1) and Latin Modern Math v1.959
(GUST Font License), retaining their licences and Latin Modern's source README and manifest.
It verifies each file's SHA-256 hash and adds both entries to the external font manifest,
preserving unrelated entries. Cargo itself never downloads fonts. Native and web builds
embed both math fonts. Without them, Ubuntu remains usable but some symbols may be missing.

CI prepares both fonts explicitly. Release packaging through the shared Craft workflow must
also prepare them, or adopt a craft-fonts revision containing them, before publishing this
feature. No release workflow or external font-repository contribution is published here.

## Implementation and scope

Equations are native `RunKind::Math` OMML objects, not #771's earlier private text-run marker.
equaSHion supplies the tree, conversions, templates and layout. It is vendored with provenance
and licences so builds do not depend on a contributor's mutable repository. Its upstream
Adobe font assets are excluded; see [the vendor notes](../vendor/equashion/README.md).

The session owns the math caret, selection, parser state and undo history. The UI paints an
editing overlay and sends commands. The text renderer supplies the same caret geometry used
for clicks, including table cells, right-to-left paragraphs and rotated text. Equations remain
atomic when selecting or editing the surrounding prose. Recognized OMML, MathML and delimited
LaTeX paste through the shared engine commands.

The unfinished Unicode/LaTeX mode controls, conversion buttons, Normal Text, equation numbers,
and misleading Display/Inline placement controls from the draft are omitted. Inline insertion
is determined by the actual text caret. Mathematical display style in imported OMML remains
supported. This does not claim complete PowerPoint equation parity or cover every imported
math extension; unsupported content is preserved where supported by equaSHion's OMML reader.

Commands include `insert.equation`, `equation.edit`, `equation.leave`, `equation.key`,
`equation.template`, `equation.update`, `equation.get`, `equation.place`, and `equation.fromLinear`.
Gallery choices go through `equation.template`, sharing selection and undo with typing.

## Validation of the combined branch

Checked on macOS with Rust 1.95.0 and the font setup above:

| Check | Result |
|---|---|
| `cargo test --workspace --locked -- --test-threads=1` | 1,747 passed; 6 ignored; no failures |
| `cargo test --manifest-path vendor/equashion/Cargo.toml --workspace` | 258 passed; 1 ignored; no failures |
| `cargo build -p deckcraft --locked` | Desktop executable builds |
| `cargo xtask wasm` | All 23 checked crates pass |
| `cargo xtask pdf-deps`, `cargo xtask assets`, `cargo xtask layers` | Pass |
| Formatting, staged diff whitespace, vendored library strict Clippy | Pass |

The UI tests exercise actual Equation ribbon clicks, typing, fraction slots, arrow exit,
prose formatting, Home font selection and click-to-reenter. Their rendered placeholder,
fraction, exit and Latin Modern Math previews were inspected. Engine tests also cover
native/PPTX saves, font changes, history, clipboard operations, selected-prose replacement,
adjacent equations and table/rotated/RTL hit testing.

The parallel workspace run had one unrelated notes-spelling UI test failure; that test passed
in isolation and the full workspace passed with serial test execution.

`cargo xtask ci` still stops at existing Rust 1.95 Clippy warnings on main. The warnings are
`collapsible_match`, `nonminimal_bool` and `manual_range_contains` in the media, model,
animation, existing pointer and layer-checking code. A full workspace Clippy run with only
those three categories allowed passes; no lint allowances were added to production code.
The remaining CI gates were run separately as listed above.
