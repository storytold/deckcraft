# Bounded layout fidelity and font diagnostics

Updated 2026-11-01: direct chart layout rendering, cached-diagram coordinates and cross-renderer probes.

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
unmodeled raw formatting. For embedded XLSX sources with direct, fixed-size, one-dimensional
A1 ranges, the writer updates the chart caches and source cells transactionally. Series names,
category labels, numeric values and blanks are supported; cell styles, unrelated cells/formulas,
other worksheets and unrelated ZIP entries survive. Calculation is requested on the next open;
DeckCraft does not evaluate spreadsheet formulas. Namespace prefixes are retained.

The nested workbook reader accepts at most 2,048 entries, 8 MiB per entry, 32 MiB total and
100,000 addressed cells. External/indirect references, resized ranges, formula target cells,
shared/array formulas, table-backed/protected sheets and signed workbooks are not synchronized.
Those edits retain the existing stale-workbook removal fallback, rather than publish mismatching
caches and cells. Structural chart changes rebuild modeled state.

Manual plot rectangles, title/legend/axis-title positions, per-role text-body rotation and explicit
axis/gridline styles now reach the renderer and survive rebuilds. Factor and edge modes are
resolved separately with finite, bounded coordinates. Rotated axis titles are rotated once, not
once by their text body and again by the axis transform. Combo legends group primary-axis series
before secondary-axis series; line keys and manual horizontal spacing reflect their actual layout.
The shared DrawingML line writer retains width, caps, joins and dash geometry; gradients retain
all stops. `layoutTarget` is emitted only for plot areas: putting it on a title/legend can prevent
PowerPoint from opening the file even though the base schema admits the element.

The chartEx reader/writer now carries per-role fonts and rotation, series paints, numeric bounds,
number formats and axis/gridline styles into modeled state. Series axis references use element
text (the chartEx schema), not the ordinary-chart `val` attribute. This closes specific lost-style
paths, not all advanced chart parity: rich/mixed-run and per-point labels, chartEx offsets and
complete native chartEx interoperability still need separate coverage.

## SmartArt

Cached drawings now retain recursive group transforms and child-coordinate systems. Picture
nodes become picture-filled `dsp:sp` shapes, because the diagram shape tree admits shapes/groups,
not ordinary slide `p:pic` children. The writer bounds traversal to 32 levels and 10,000 nodes.
Unsupported embedded objects warn rather than masquerading as supported nodes.
Flat generated process constraints also account for non-uniform frame scaling. Imported cached
root `chOff/chExt` coordinates no longer assume the outer graphic-frame dimensions. When an
AlternateContent diagram choice lacks a usable cached drawing, an authored fallback branch is
selected if available; a valid cache still takes precedence.

For imported diagrams with a complete retained graph, the original data model (including point
and connection IDs), cached drawing, layout, style, colors, media and relationships survive export.
Parts receive fresh names and internal targets are rewritten without regenerating diagram IDs.
A fingerprint is sealed after import normalization; edits to the cached geometry invalidate reuse.
Retained graph traversal is bounded to 512 parts and 64 MiB, sharing the import sidecar budget.
Native saves externalize diagram payloads into the existing bounded `chart-parts/` ZIP namespace.

Text Pane edits bind by model ID and presentation-point association, never by matching strings.
Single-line node/title changes update the data model, cached drawing and visible model together,
preserving geometry and existing run formatting. Duplicate labels remain independent. Missing or
ambiguous bindings reject the edit without changing the diagram. Undo/redo and native save/reopen
are tested. Structural/layout/style changes still use DeckCraft's simpler layout algorithms.
This does not implement the full constraint language: absent drawing caches with no authored
fallback, complex text remapping and independently edited cached shapes remain outside the
preservation path.

## Validation

`cargo test -p deckcraft-pptx --test layout_fidelity` constructs four original synthetic slides:
locale fonts, mixed chart typography with a wrapped legend, a dual-axis combination chart, and a
rotated nested diagram with a generated color swatch. It renders both the source and exported/
reimported model and compares exact pixels within the same build/environment. Set
`DECKCRAFT_VISUAL_OUT` to a local directory to inspect the generated PPTX, PNGs and font report.
No generated deck, screenshot or font binary is checked in. Separate regression tests cover
font isolation/fallback, chart edits and embedded image relationships. Full workspace acceptance
is `cargo xtask ci`.

`diagram_preservation` adds original synthetic tests for opaque drawing/ID preservation,
duplicate-label edits, presentation-point bindings, geometry invalidation, command undo/redo and
native save/reopen. Workbook unit tests inspect both caches and embedded cells, unrelated data,
namespace handling and rejected formula/conflict cases. `DECKCRAFT_DIAGRAM_OUT` optionally writes
synthetic probes locally; it is not required by CI.

A local PowerPoint for Mac 16.113.3 black-box check opens and exports the probes without dismissing
a repair dialog. An original synthetic diagram and its no-op round trip render identically;
a single-node text edit changes pixels only in that text region. A PowerPoint-saved copy of that
probe also preserves its presentation-point data through the same operations. All four pages of
a separately PowerPoint-saved synthetic font/chart/nested-diagram probe render identically before
and after DeckCraft's no-op PPTX round trip. These are preservation checks against the same native
renderer, not a claim that DeckCraft's renderer matches PowerPoint on arbitrary files. Generating
complex layout constraints or altering a diagram's structure remains a separate limitation.
The synchronized XLSX was also opened in Excel for Mac 16.113.3: the edited name/value,
unrelated Notes sheet and original formula were read back successfully. Native outputs and
screenshots are not distributed.

The additional tests are `chart_visual_format` (manual rectangles, strokes, title rotation and
chartEx modeled styles), `font_render_matrix` (Latin ligatures/italic, CJK, Arabic, Thai and
Devanagari, embedded sfnt/EOT, missing-face reporting), and the cached-layout extensions in
`diagram_preservation`. Set `DECKCRAFT_RENDER_PROBES` to emit their original probes locally.
Tests assert rendered coordinates/colors and selected font faces, not merely non-empty XML.

The direct-render validation uses **the same input file** for DeckCraft and PowerPoint. Six
PowerPoint-saved original synthetic cached diagrams (hierarchy with three levels, cycle, radial,
matrix, pyramid and picture-process layout) were rendered at 3840x2160 with both renderers and
visually compared. The manual-layout dual-axis chart and the multilingual font page were also
opened/exported successfully and compared directly. These cover imported cached drawings; they
must not be interpreted as proof that DeckCraft generates all SmartArt constraints identically.

| Case | Verified result | Remaining condition |
|---|---|---|
| Cached complex diagram, including scaled/nonzero child coordinates | Direct image rendering and retained text/geometry | Arbitrary constraint regeneration without cache is not implemented |
| Missing diagram cache with an authored AlternateContent branch | Uses the authored fallback instead of an empty diagram | A file with neither representation cannot be reconstructed exactly |
| Named available/embedded sfnt and uncompressed EOT faces, six script probes | Faces selected and shaped; no missing glyphs in the probes | Missing font binaries and MicroType Express subsets cannot be made equivalent by substitution |
| Manual dual-axis chart with rotated labels and styled gridlines | Direct renderer respects bounds, strokes and per-role typography | Automatic label spacing and every rich/per-point label case are not pixel-certified |
| Nine chart-type render matrix | Owned import/render tests pass | Fresh chartEx native-open/export failed in the local PowerPoint check; this is a failed check, not a pass or an untested feature |

A separate black-box attempt to insert Waterfall through PowerPoint's own menu also lost the
application connection, so the local chartEx failure is not yet attributed solely to emitted XML.
It remains an explicit interoperability investigation. No Office-generated files or screenshots
are shipped. A consumer can use direct rendering for the validated representations above; an
arbitrary-deck replacement claim requires the remaining named cases to pass too.
