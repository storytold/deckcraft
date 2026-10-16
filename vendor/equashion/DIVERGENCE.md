# Divergence from WordCraft dbfe1b1

Behaviour that differs from the imported WordCraft code, and why. Structural changes (the
`Scene` seam, the `MathFonts` trait, the trimmed font crate) are in PROVENANCE.md; this file is
about what a user would notice.

## Edit model: exponent rules from DeckCraft 7972cf5

Three DeckCraft tests were ported into `crates/math/src/edit/tests.rs`. All three failed against
WordCraft's `edit.rs`. In each case DeckCraft's behaviour was chosen, because it matches how
people type the formulas (and MathLive): the exponent stops where a reader would expect it to.
The rules were ported into `edit.rs` from DeckCraft's `edit.rs`. All 134 WordCraft math tests
still pass unchanged.

| Test | WordCraft dbfe1b1 | Now (DeckCraft 7972cf5) |
|---|---|---|
| `a_single_digit_exponent_ends_at_the_next_term` | `mv^2/2` became `mv^(2/2)`; `x^2sin x` became `x^(2 sin x)` | A digits-only exponent ends at the next character that is not a digit, `.`, `^`, `_` or whitespace: `(mv^2)/2`, `x^2 · sin x`. `2^10` still keeps going. |
| `letters_keep_going_in_an_exponent` | `x^n+1` left the exponent at `+` (`x^n + 1`); `e^ipi` stayed `e^(ipi)` | An operator leaves a digits-only exponent (`x^2+1`) but stays in a letter exponent (`x^(n+1)`, `2^(n−1)`). A three-letter run ending in `pi` converts the `pi` (`e^(iπ)`). |
| `a_parenthesised_exponent_is_the_group_itself` | `x^(n+1)` dropped the parentheses but left the caret in the exponent | The parentheses are still dropped, and closing the group also takes the caret out of the exponent. |

What changed in `edit.rs`:

- `type_char_raw`: before handling the character, a caret at the end of a digits-only script
  slot steps out of it unless the character is a digit, `.`, `^`, `_`, whitespace or a control
  character. It is skipped inside a typed LaTeX `{...}` argument (`brace_depth > 0`), so
  `x^{2a}` still works. DeckCraft has no LaTeX brace handling, so this guard is new.
- `leave_func_arg`: an operator leaves a script slot only when the slot holds digits.
- `unwrap_frac_paren`: after unwrapping the group of a script slot, the caret steps out of the
  slot. Fractions, radicals and big-operator limits behave as before.
- `autoconvert`: a three-letter run ending in `pi` converts the `pi`.
- New helpers `slot_is_digits` and `exit_script_slot`, taken from DeckCraft.

Side effect: `x^2 y` used to give `x^(2y)`; it now gives `x^2 y`, because the `y` ends the
digit exponent. The same applies to subscripts (`a_1b` is `a_1 b`, `x_i+1` is `x_(i+1)`).

## Default equation font

WordCraft's equation editor (`eqdialog.rs`, `EQ_FAMILY`) asked for "Cambria Math", which is
not bundled. Unless the system had it installed, the font database fell back to Source Sans 3
without saying so. equaSHion names its default family,
`equashion_font::DEFAULT_FAMILY` = "Source Serif 4", bundles all four styles of it, and pins
that family in the golden tests. An unknown family still falls back, now to Source Serif 4.
"Cambria Math" is still present in some OMML test data in `crates/math/src/tests.rs`, because
Word writes it there, but it is never a default.

## Editor canvas height

`layout_equation` sized the canvas from the equation's ink (with a floor of 0.78 em above and
0.25 em below the baseline). A row's caret reaches at least 0.22 em below its baseline even
where the row has no ink there, so in a display fraction such as `lim_{x→0} sin x / x` the
caret in the denominator stuck out of the canvas by about a point. The canvas now also covers
every caret row. Found by the corpus property test
(`crates/layout/tests/corpus.rs`), which checks that every caret lies inside the canvas.

## Symbols the bundled fonts lack: STIX Two Math fallback

WordCraft's `FontDb::global()` loads system fonts for characters its bundled fonts lack.
`FontDb::bundled_only()` does not, and Source Serif 4 and Inter have no glyph for ∇ (U+2207),
∮ (U+222E), ⌈ ⌉ ⌊ ⌋ (U+2308–230B) or ⟨ ⟩ (U+27E8, U+27E9); these used to draw as .notdef boxes.
equaSHion now bundles STIX Two Math Regular (OFL, unmodified, 838 KB) after the text faces, so
`fallback_for` finds it for exactly those characters (the fallback order is the default family
first, then load order, so nothing Source Serif 4 or Inter covers moves). The `maxwell_*`,
`oint`, `floor_ceil` and `angle` goldens changed accordingly; no other golden did. Pinned by
`symbols_the_text_faces_lack_resolve_to_stix_two_math` and `no_corpus_equation_draws_a_missing_glyph`
(`crates/layout/tests/corpus.rs`), the golden test (no .notdef glyph) and the widget's
`every_palette_face_is_drawn_from_the_bundled_fonts`.

Visible side effects: ∮ comes from STIX at the big-operator size, so it is heavier than the
Source Serif ∫ and its limits sit a little lower; ∇ is slightly lighter than the surrounding
Source Serif glyphs.

## Typed bracket commands

The typed-text parser now knows `\langle`, `\rangle`, `\lfloor`, `\rfloor`, `\lceil` and
`\rceil` (WordCraft reported them as unknown commands and kept them as literal text). They
become the characters ⟨ ⟩ ⌊ ⌋ ⌈ ⌉ as text, the same as typing those characters; with `\left` /
`\right` they are still text (those two are ignored, as before), so the drawn angle and floor
delimiters of the layout are only reached from OMML.

## Closing a parenthesis ends the structure it closes

`sqrt(x)+1`, `sin(x)+1` and `1/(x-1)+2` used to leave the caret inside the radical, the function
argument or the denominator after the `)`, so the `+1` was typed into it (`√(x+1)`, a doubled
`sin((x))`). Closing a parenthesis that is the whole slot now ends the structure, the way
`x^(n+1)` already did and the way Desmos behaves: the group's parentheses are dropped (the
structure already groups) and the caret moves to just after it. `sqrt x+1` without parentheses
still keeps taking what follows. Needed so equations typed for graphing mean what they say.
