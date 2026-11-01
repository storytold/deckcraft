# Bounded layout fidelity and font diagnostics

This work improves native rendering and PPTX round trips. It does not certify arbitrary
presentations as visually equivalent to PowerPoint, or make every imported object fully editable.

## Font selection

Theme script faces now use the run's language when Unicode `Hani` alone is ambiguous:
`Hans`/`Hant` for Chinese, `Jpan` for Japanese and `Hang` for Korean. Explicit script subtags
win over region; unknown Han language does not silently assume Simplified Chinese. Explicit
East Asian/complex-script run fonts still precede theme script fonts.

Valid embedded TrueType/OpenType/EOT faces precede installed names inside their owning
presentation. They never replace the global family database. The cache is bounded to 16 source
files and 64 MiB of source bytes (individual sources at most 40 MiB); callers may retain faces.
MicroType Express subsets remain preserved but cannot be drawn. Synthetic bold/italic follows
the face actually selected, including glyph fallback.

`font.inspect {}` is a read-only engine/control command, with no undo or journal entry. It returns
segments for slide shape text, table cells, and representative chart labels:

- `slide` is one-based; `shape` is the shape ID. Cell coordinates and paragraph/run indices are
  zero-based. `start`/`end` are Unicode-character offsets within the saved paragraph.
- `requested` is the resolved family cascade; `actual`, `style`, `faceIndex`, `fontSha256` and
  `variationCoordinates` identify the selected face. Hashes cover the actual sfnt bytes,
  combined with the collection index/coordinates, rather than a filename.
- `reason` is `requested`, `embedded`, `family_unavailable`, `glyph_fallback`, `emoji` or
  `missing_glyph`; `missingGlyph`, `syntheticBold` and `syntheticItalic` provide explicit flags.
- Chart rows include `component`; numeric-axis rows set `sampled: true` because the exact ticks
  depend on the final frame. Saved field values are inspected, not live field expansions.

Reports omit document text and local file paths, but font family metadata can itself be private;
inspect reports before sharing them. Bullet glyphs, SmartArt layout-engine text that has no cached
shape, and all specialized chart labels are not covered by this first diagnostic command.
`font.used`/missing-font checks use script-aware requested families. `font.replace` also visits
modeled chart text roles.

## Charts

Chart-space defaults, title, legend, category/value axes, secondary value axis and axis titles
retain separate family/script slots, size, weight, slant, language and fill. Rendering measures
label widths and legend row heights through the same text layout used to paint them. Rebuilt
combination charts retain secondary-axis bounds, units, number format and visibility. Logical
series order is independent of XML plot-group order, so palette and legend associations survive.

Data-only patches remain restricted to categorical data with unchanged series count; they keep
unmodeled raw formatting. Structural changes rebuild modeled state. Changed embedded workbooks
are removed rather than synchronized. Mixed rich-text chart runs, rotations/manual label positions,
per-point/per-series typography and full advanced-chart parity remain future work.

## SmartArt

Cached drawings now retain recursive group transforms and child-coordinate systems. Picture
nodes become picture-filled `dsp:sp` shapes, because the diagram shape tree admits shapes/groups,
not ordinary slide `p:pic` children. The writer bounds traversal to 32 levels and 10,000 nodes.
Unsupported embedded objects warn rather than masquerading as supported nodes.
Flat generated process constraints also account for non-uniform frame scaling.

This preserves existing cached geometry; it does not implement the full SmartArt constraint
language. Editing an imported outline can still re-layout it with DeckCraft's simpler algorithms.
Diagrams without a cached drawing, advanced picture/3-D layouts and complete data-model fidelity
remain open.

## Validation

`cargo test -p deckcraft-pptx --test layout_fidelity` constructs four original synthetic slides:
locale fonts, mixed chart typography with a wrapped legend, a dual-axis combination chart, and a
rotated nested diagram with a generated color swatch. It renders both the source and exported/
reimported model and compares exact pixels within the same build/environment. Set
`DECKCRAFT_VISUAL_OUT` to a local directory to inspect the generated PPTX, PNGs and font report.
No generated deck, screenshot or font binary is checked in. Separate regression tests cover
font isolation/fallback, chart edits and embedded image relationships. Full workspace acceptance
is `cargo xtask ci`.

Same-engine pixel equality catches save regressions; it is not a PowerPoint reference-render
comparison. A local PowerPoint for Mac 16.113.3 open/PDF-export smoke check accepted all four
probe slides without the driver dismissing a repair prompt. It also demonstrated the remaining
gap: the intentionally synthetic cached diagram's geometry does not match its simple data-model
layout, so PowerPoint regenerates that diagram and changes its appearance. Chart automatic
layout/default styling also differs. Native output and screenshots are not distributed.
Continue using a separately validated fallback for presentations requiring unsupported
layout features or native-Office fidelity.
