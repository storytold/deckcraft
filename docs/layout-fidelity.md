# Bounded layout fidelity and font diagnostics

Updated 2026-11-01 (PPTX.14): native composite coordinate semantics and literal column-label
wrapping, extending container fitting, geographic caches and paragraph inheritance.

This work improves native rendering and PPTX round trips. It does not certify arbitrary
presentations as visually equivalent to PowerPoint, or make every imported object fully editable.

## Font selection

Theme script faces now use the run's language when Unicode `Hani` alone is ambiguous:
`Hans`/`Hant` for Chinese, `Jpan` for Japanese and `Hang` for Korean. Explicit script subtags
win over region; unknown Han language does not silently assume Simplified Chinese. Explicit
East Asian/complex-script run fonts still precede theme script fonts.

Valid embedded TrueType/OpenType/EOT faces precede installed names inside their owning
presentation. They never replace the global family database. The cache is bounded to 16 source
files and 64 MiB of retained source plus decoded bytes (individual source/decoded fonts at most
40 MiB); callers may retain faces. Raw and EOT-wrapped MicroType Express TrueType subsets now
decode to sfnt before face selection. Failed decoding is negatively cached, so repeated runs do
not repeatedly expand malformed input. Over-budget entries are used without cache retention.
Synthetic bold/italic follows
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
inspect reports before sharing them. Bullet glyphs and every specialized chart-label case are not covered by this first diagnostic
command. Successfully evaluated uncached SmartArt text becomes ordinary modeled text.
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
number formats and axis/gridline styles into modeled state. Fresh chartEx parts now include
complete, original theme-relative style/color parts and rich-text literal titles. Black-box
bisection found that missing styles and element-text axis IDs could hang native PowerPoint;
its accepted scalar encoding is described below. Rich/mixed-run and per-point labels and
chartEx offsets still need separate coverage.

## Embedded SVG pictures

Embedded SVG sources render directly through resvg 0.48.1 (MIT/Apache-2.0), including paths,
gradients, clipping, transformed groups and text resolved through DeckCraft's font database.
The `asvg:svgBlip` source and authored raster preview are retained separately. Rendering prefers
the vector; decoding failure uses the original preview. SVG-only extension references also import.
Both media IDs participate in copy/reuse/remapping; replacing the picture clears the old SVG.

SVG decoding uses the existing size-bucket image cache. Source XML is capped at 8 MiB, 10,000
elements and 64 levels; DTDs are rejected. Output is capped at 4096 pixels on its longest side.
File/network image references and nested SVG data URLs are disabled; inline raster dimensions
are checked before decoding. Font loading is bounded and does not enable resvg's system scan.
This is an in-process renderer, not a browser or an external document converter. SVG picture text
prioritizes presentation-scoped embedded faces, including MTX/EOT fonts. The image cache includes
the font-source context and retains those sources within its byte budget, preventing the same SVG
from reusing another document's typography. A two-document regression renders the same SVG bytes
with different embedded faces, checks distinct pixels and verifies cache isolation and round trips.

`svg_render` checks vector preference, retained fallback behavior, SVG-only references and repeat
save/reopen pixels. Original native comparisons verify clipping, gradients and transformed text.
No test artwork or native screenshots are checked in.

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
When neither a cached drawing nor an authored fallback is available, the bounded layout
interpreter described below now constructs shapes from the layout definition. Complex text
remapping and independently edited cached shapes remain outside the identity-preserving edit
path. No-cache diagrams retain their data graph rather than fabricating a new graph.

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
| Cached complex diagram, including scaled/nonzero child coordinates | Direct image rendering and retained text/geometry | Uncached layout execution now exists; full native visual equivalence is not established |
| Missing diagram cache with an authored AlternateContent branch | Uses the authored fallback instead of an empty diagram | Without either representation, the bounded layout interpreter is used; see its limits below |
| Named available/embedded sfnt and uncompressed EOT faces, six script probes | Faces selected and shaped; no missing glyphs in the probes | Missing font binaries still require substitution; MTX subsets are now decoded |
| Manual dual-axis chart with rotated labels and styled gridlines | Direct renderer respects bounds, strokes and per-role typography | Automatic label spacing and every rich/per-point label case are not pixel-certified |
| Nine chart-type render matrix | Owned import/render tests pass; all nine pages now open/export in PowerPoint without repair | Automatic plot spacing and all rich/per-point labels are not pixel-certified |

The earlier native-open failures were reproduced and isolated by changing one property at a
time. Complete chart styles and native scalar encoding resolve them; empty style placeholders
and workbook-only changes do not. No Office-generated files, style assets or screenshots are
shipped. A consumer can use direct rendering for these validated representations; successful
native opening alone is not a claim of pixel-equivalent rendering on arbitrary decks.

## Extended rendering implementation and independent checks

### MicroType Express

`crates/fonts/src/mtx/` is an original, safe-Rust implementation derived from the public
[W3C MTX submission](https://www.w3.org/submissions/MTX/), with EOT wrapping per the
[public EOT format](https://www.w3.org/submissions/EOT/). It restores adaptive LZCOMP streams,
compact TrueType contours/composites, split push instructions, CVT, HDMX and VDMX tables, `loca`,
`maxp` instruction limits and sfnt checksums. It never executes font hint programs. There is no
external font decoder, subprocess or converter in the production path.

Independent local encoding used Google's Apache-2.0 sfntly, revision
`a56f5782f209771aa226063757d57e6b5c948478`. Four permitted craft-fonts faces (Poppins, Liberation
Sans, BIZUDMincho and Noto Sans Arabic) total 19,323 glyphs: reconstructed contours, coordinates,
flags, advances and CVT agree with the source. Push encodings can differ while decoded hint
instructions agree. The PPTX integration oracle compares raw MTX and EOT-wrapped MTX against the
uncompressed embedded face: the tested Liberation Sans page is pixel-identical after round trip.
VDMX compression is covered by original synthetic vectors; sfntly does not exercise that path.

Regular tests include an independently encoded original byte vector, original triangle/composite
contours, device metrics, truncation and hostile-input smoke. Optional ignored tests
`independent_encoder_oracle` and
`microtype_embedded_font_matches_uncompressed_render_and_roundtrip` accept local font paths through
`MTX_ORACLE_INPUT`, `MTX_ORACLE_OUTPUT` and `MTX_ORACLE_SOURCE`; no font binaries are checked in.

### chartEx

Histogram interval closure, underflow/overflow buckets and categorical aggregation; Pareto
cumulative sums; independent box-series quartiles, mean markers/lines, inner points and outliers;
waterfall connector visibility; and treemap parent-label modes now affect rendering and survive
model rebuilds. Histogram series options take precedence over chart defaults; a requested bin
count includes underflow/overflow buckets, out-of-range tail bounds are ignored, and the full
observation population determines automatic bin statistics; Scott widths round to two significant
digits, verified with independent native probes across scales. Interval labels retain small finite
values without an integer cast. All buckets share the 1,000-bin
budget, including tails. A final left-closed interval includes the maximum observation.
Waterfall connector overrides are evaluated per series.

Bubble charts now render circles rather than connected scatter lines. Area/width size semantics,
0–300% scale, negative-size visibility and per-point fills survive rebuilding. Numeric axes leave
room for bubble glyphs; explicit y-axis bounds remain unchanged. Tests measure colored pixel areas
and disconnected glyphs, including zero/missing/nonfinite sizes; three original bubble probes
open/export natively. Automatic glyph sizing and plot spacing are not claimed pixel-identical.

The writer emits `axisId`, `binCount` and `binSize` with `val` attributes. The reader accepts both
attribute and element-text forms. This intentionally differs from the published Open XML SDK
3.3.0 Microsoft 365 schema for those three elements: the SDK reports these scalar exceptions,
but native PowerPoint for Mac 16.113.3 accepts them and hangs with element-text axis references.
Original histogram, Pareto, box, waterfall and treemap probes now open/export independently,
as does the combined nine-page chart matrix. Their style parts are original code derived from
public MS-ODRAWXML definitions, not copied vendor styles. Literal chart and axis titles use rich
text because the formula-less `txData/v` form did not display a title natively.
Integration tests check independent series settings, statistical boundaries, rebuilt XML and
changed pixels. Region-map geographic boundaries, every rich data-label layout and every
automatic Office plot-spacing rule are not added by this change.

### Diagrams with no drawing cache

`crates/pptx/src/dgm/layout/` executes layout nodes, ordered data axes, range selection, conditional
branches, variables, recursive `forEach` references, presentation mappings, named constraints and
font rules. It includes linear, cycle, snake, hierarchy, composite, space, text, connector and
pyramid placement paths. Millimetre lengths, degree rotations, preset adjustments, hidden geometry,
text anchors/bullets and subtree font constraints are interpreted. Transition connectors attach
to the data nodes' boundaries; line/bent/curved routes and arrow ends are modeled. Snake layouts
respect flow/growth/breakpoint/continuation settings. Hierarchies allocate space by subtree demand.
A font-rule floor prevents autofit from silently shrinking below the declared minimum; a bounded
search uses only the line compression needed at that floor. Explicit preset adjustments retain
the defaults of unspecified adjustment slots.

Linear parents pack the visible extent of unrotated, non-overflowing composite children, rather
than their unused container padding. Diagram text defaults to 90% line spacing, 35% paragraph
spacing and font-relative insets; explicitly authored spacing overrides those defaults. Paragraph
percentage spacing uses the adjacent line's actual font metrics instead of an assumed 1.2 em.

The layout compiler and geometry resolver have independent 200,000-operation budgets, at most
10,000 instances and 64 recursive levels; ordered data has at most 16,000 points and 32,000
connections. Over-limit graphs and selectors are rejected rather than silently truncated. Unknown
algorithms, unresolved iterator references and exhausted budgets refuse evaluation and preserve
the source/preview path. Source data/layout/style/color graphs remain intact on unchanged export.
The integration fixture has no drawing member, dangling relationship or alternate picture: it
executes a custom nested layout and preserves graph IDs, geometry and exact pixels on round trip.

The earlier three-block native result came from ambiguous child constraints in the synthetic
fixture's composite container. Naming the intended children produces the declared six shapes
in native PowerPoint; measured shape coordinates are now asserted independently in the test.
The valid source and DeckCraft round trip both open/export; the fixture still contains no cached
drawing or alternate preview. Font availability must match across renderers: the comparison
installs the same permitted Inter face, rather than comparing against native font substitution.
The uncached package passes the SDK validator. Native text observations at multiple font sizes
also verify the default line/paragraph spacing and insets.

These paths do **not** establish equivalence to all of PowerPoint's SmartArt layouts.
The complete width/height rule-search policy,
accent-pyramid variants, automatic text rotation and all 2-D connector shapes are not
exhaustively modeled or independently certified. Accordingly chart/SmartArt parity scores stay
partial; a blanket arbitrary-deck replacement claim is not warranted. All implementation/tests
above run without LibreOffice; PowerPoint is used only as a private comparison oracle.

### Private corpus export

A local batch of 50 pages (two 20-page complex-layout presentations plus the nine-chart and
one-diagram probes) exports directly at 3840 pixels wide with the source hashes unchanged.
The two 20-page cases take approximately 5.0 and 4.4 seconds on the validation machine.
Visual inspection covers every contact-sheet page and selected full-resolution source comparisons.
This measures the tested corpus, not every PPTX representation. No private filenames or content
are distributed upstream. The export process has no LibreOffice executable on its restricted PATH.


### Diagram equations, geographic caches and rich data labels (PPTX.8)

The diagram interpreter now compiles named self/child/descendant references into sparse affine
rows. Reference point-type filters and repeated-node pairing are applied before solving.
Coordinate expressions account for ancestor offsets; left/right/centre constraints can determine
extents. Unconstrained geometry remains linked to its container, so a resized composite propagates
to children. Sparse elimination resolves forward chains and algebraically solvable cycles;
Dykstra half-space projection handles lower/upper bounds in the equality system's free variables.
Both operations share the existing work budget. Inconsistent, unbounded and negative-extent
solutions fail explicitly, retaining the original diagram graph instead of guessing a last iteration.

Tests cover a 24-node reference chain, coupled cycles, named descendants, paired edges, tightening
bounds, missing references and contradictions, in addition to the existing layout/connector tests.
Original uncached forward-reference and cycle integration fixtures retain all six shapes and their
geometry across export/import. Native PowerPoint opens both; the forward-reference fixture uses the
six-shape custom layout. The cyclic fixture falls back to three generic blocks in the tested native
version. This is an explicit difference, **not** evidence of cyclic-layout pixel equivalence.
PPTX.9 below adds measured rule search and corrects reference/value semantics; every native fallback policy remains unverified.

Region maps now parse `cx:geography/cx:geoCache/cx:clear` entity polygons, decode PCA rings with
checked integer arithmetic, bind regions to data by entity id/name (not XML order), and draw actual
boundaries with even-odd holes and dateline wrapping. The document supplies all geometry; no online
service, map assets or external converter is used. Mercator and Miller projections use their
spherical formulas; Albers uses 20/50-degree standard parallels and Robinson uses interpolated
five-degree tabulation. These deterministic projection choices do not reproduce every native
viewport heuristic. Attribution/copyright strings remain visible. Unchanged imported geography
keeps its original query/layout XML even through unrelated chart edits.

The original polygon fixture reverses region/data order and has a hole; tests assert value-colour
assignment, the unpainted hole, dateline width, malformed-input limits and rebuild preservation.
The public PCA numerical vector supplies an independent decoder check. The package passes SDK
validation and opens in PowerPoint, but the synthetic non-service geography renders blank there;
therefore this is **not** a native map visual-parity pass. PPTX.9 below decodes observed raw-DEFLATE `binary` caches; unknown encodings
are still retained without decoding; missing/unusable boundaries still display explicitly labelled tiles.

Classic data labels now retain default/point overrides, multiple rich runs/paragraphs, font slots,
size, bold/italic/underline, baseline/spacing, fill, rotation, delete, manual layout, number formats,
separator and value/category/series/percent/bubble-size visibility. Column/bar, line, scatter,
bubble and pie/doughnut labels use the native slide-text renderer. chartEx point visibility and
text-style structures are read and rebuilt; not every chartEx layout applies all label overrides.
Export materializes inherited visibility and text rotation where native PowerPoint expects them,
preventing unwanted series names/legend keys and lost precision. Auto outside labels clear their
rotated extent; manual positions are preserved. Full leader-line/callout routing, collision
avoidance and all rich chart title/legend cases remain outside this change.

`geographic_labels` verifies rendered mixed-run colours, rotation/hidden labels, manual position,
percent precision, geographic holes and forced model rebuilds. Native label observation confirms
the mixed fonts/colours, rotation, hidden middle label and `8.00` format after correction. Native
automatic plot spacing still differs. Original and rebuilt layout probes remain pixel-identical.

Final validation passes all eight `cargo xtask ci` stages, including WASM. The expanded direct
export batch contains 58 pages at 3840 pixels wide; every input hash remains unchanged. These
are rendering, package and regression checks with the native comparison limits above, not a
claim that all 58 pages are pixel-identical to PowerPoint.

Format/algorithm references (no vendor visual assets are included):
- [DrawingML diagram constraints](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.diagrams.constraint)
- [chartEx schema and geography cache](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/e2723b0a-9120-42a5-bd11-c252ccb13c1e)
- [PCA wire algorithm](https://learn.microsoft.com/en-us/bingmaps/rest-services/elevations/point-compression-algorithm)
- [Geographic ring framing](https://learn.microsoft.com/en-us/bingmaps/spatial-data-services/geodata-api)
- [Miller projection](https://proj.org/en/stable/operations/projections/mill.html)


### Measured diagram rules and compressed maps (PPTX.9)

Uncached numeric rules now run after presentation themes, embedded fonts and inheritance are
available. Each candidate uses the actual text renderer's preset text rectangle, insets, shaping,
Unicode line breaks, line/paragraph spacing and overflow. Rules execute in authored order and
replace absolute constraint values or referenced factors; `max` is enforced. Every trial resolves
the dependent constraints and connectors again. Search distinguishes forced word breaks from
normal whitespace/CJK wrapping, stops at a fitting value, and recovers unnecessary box growth
after later font reduction. Reference-linked labels participate in the fit decision; primary and
secondary font constraints remain distinct. Ordinary slides and cached diagrams skip this pass.
The solver/search share a finite operation budget; unknown/nonconverging cases retain the source.

Native probes verify height-before-font versus font-before-height, factor versus absolute height,
font floors, and invalid maxima. The constraints now give an explicit reference precedence over
`val`, as observed natively: the two are alternatives, **not** reference plus an offset. Earlier
additive-cycle tests were replaced with proper proportional cycles and independent bounds.
Cycle layout uses clockwise angles from twelve o'clock, fits actual child sizes on a circle and
centres the occupied bounds of partial arcs. Three original native cycle variants establish
orientation, extent fitting and centring; this does not certify all compound-cycle heuristics.

An independent, private native geographic-cache observation established base64-wrapped RFC1951
raw DEFLATE containing `cx:clear` XML. Both final streams and native SYNC_FLUSH-terminated streams
are decoded. Only a complete, bounded XML document with a `clear` root is accepted. Limits are
16 MiB encoded input, 8 MiB compressed data and 32 MiB expanded XML, with depth/point/ring limits.
Original synthetic tests cover final/flush streams, truncation, invalid XML, trailing input,
expansion limits, version-1 PCA framing and exact clear-versus-binary render/rebuild equivalence.
Source binary bytes remain preserved; no external service or document converter is invoked.

The observed world cache has roughly 535,000 points, motivating a bounded two-million-point
budget rather than truncating a valid map at 200,000. Native world-map comparisons confirm
Robinson for automatic projection and the scope of explicit series fill. Positive and negative
180-degree clipped polygon edges remain distinct, preventing transcontinental seam chords. Native cache boundary
geometry now renders directly; provider hierarchy/geocoding metadata and all automatic viewport
policies remain separate gaps. In particular, the original non-service synthetic cache still
renders blank in PowerPoint despite successful schema/open checks. That is not a visual pass.
No native cache coordinates, vendor assets or screenshots are published.

Funnel point labels now use the shared rich-text renderer, including per-point font/fill,
rotation and hidden labels. Export materializes inherited visibility and end-mark typography,
which native generated labels require to retain font size/fill. End-mark-only imported styles
also resolve correctly. Native funnel probes confirm 30pt red/blue values and a hidden middle
label after this correction; its automatic plot spacing still differs, and native ignores the
funnel text-body rotation attribute in this probe. A regression caught and fixes automatic run colors incorrectly
winning over explicit label-default typography; classic labels share that correction. Import,
forced rebuild and rendered-color checks cover the extended path. Other chartEx label/callout
layout rules remain partial.

Public definitions used here are ECMA-376 Part 1 §21.4 (diagram constraints/rules/parameters),
MS-ODRAWXML `CT_GeoCache`/`CT_Clear`, RFC1951 DEFLATE and the public PCA framing linked above.
Private native observations are interoperability evidence, never shipped assets.

The PPTX.9 direct-export probe set contains nine pages at 3840 pixels wide, exported in about
4.5 seconds locally with unchanged inputs and a PATH containing no document converter. This is a
performance/success observation, not a claim of pixel equivalence for every probe.

PPTX.9 final validation: all eight `cargo xtask ci` gates pass, including the complete workspace
tests and WASM. All native observations above use original probes; native outputs stay private.


### Hierarchical text and geographic joins (PPTX.10)

Diagram font-rule dependencies now track the changed property: a width reference does not
make an unrelated overflowing label force another label to its minimum font size. Measurements
cache one result per shape, reusing translation-only candidates and both overflow/word-break
scores. A separate 32 MiB cumulative shaped-text budget bounds expensive glyph work. Data-point
hierarchy now survives into text layout: `stBulletLvl=2` leaves parent text primary and bullets
children using `secFontSz`. Native child text is bulleted by default; `bulletEnabled` is a
layout variable for authored conditions, not an unconditional rendering switch. Original
native probes verify 24pt primary and 14pt secondary text for start levels 1 and 2,
left/top list alignment, hanging indents and secondary paragraph spacing. Paragraph
indent level alone remains insufficient to turn primary text into secondary text.

Geographic clear/decoded caches join location queries through latitude/longitude and entity
type to entity ids. Category parent levels disambiguate duplicate place names, explicit ids win,
and ambiguous names/conflicting duplicate data rows are not silently matched to the first cache
entry. This is offline cached lookup, not online geocoding. The point budget is shared across
chart series. The authored `viewedRegionType` survives rebuild; `dataOnly` excludes unbound
background regions, and `world` retains a world extent even for sparse cached boundaries.
Intermediate administrative viewport selection, uncached geocoding, and automatic aggregation
of duplicate data rows remain unsupported. No bundled geography or network service is added.

Map data labels now use the common typography/visibility renderer, with numeric point font,
color and hidden overrides tested through forced rebuild and pixel assertions. Classic chart
labels support literal mixed runs; chartEx's public `CT_DataLabel` provides `txPr` styling, not
classic `c:tx/c:rich` replacement text. This distinction is retained rather than inventing
schema-invalid extended-label content. Other chartEx layout/callout heuristics remain partial.

Independent black-box comparison used PowerPoint 16.113.3 and LibreOffice 26.8.0.3 on original
probes. PowerPoint renders the acyclic named-reference layout, but three mutual-reference
variants (free, bounded, anchored) fall back to its generic three blocks. DeckCraft instead
solves supported consistent affine cycles; this is a documented compatibility difference,
not a visual-parity pass. LibreOffice produces no SmartArt text/geometry for the uncached
circle, ordered-text-rule and named-reference probes; map and funnel probes show its unsupported
chart-type message. An exported PDF alone is therefore not evidence of successful rendering.
Private native outputs are never included upstream.

PPTX.10 final validation: all eight `cargo xtask ci` stages and 2,376 tests pass. Fourteen
original/private probes export directly at 3840px width in 5.112 seconds locally, with unchanged
source hashes and no external document converter on PATH. Hierarchical list alignment and map
label visibility/colors were visually inspected after the final rebuild. These timings and
checks do not certify arbitrary-deck pixel equality.


## PPTX.11 — interior trials, numeric maps and rich label bodies (2026-11-01)

Rule search now samples interior geometry before refining a fit. A growing box can take space
from a reference-linked neighbour: the limit can be worse or produce a negative extent while
an interior value fits both labels. Such infeasible trials are rejected individually; malformed
coefficients, invalid text measurements and resource-budget errors still fail explicitly. Search
retains authored order, instance identity, full dependency resolution and the existing work/text
budgets. It is bounded numeric search, not an exhaustive proof over all coupled rule combinations.

A native test also disproved an attempted immutable-bound interpretation: a font rule can replace
a constraint with the same property even when its operator is `gte`. That native behaviour is
retained. An owned height-bound probe opened/exported but rendered differently (native text
outside its shape); it is not counted as a visual-parity success. Cyclic-layout fallback and the
full native rule/algorithm search policy remain open.

Offline maps now bind repeated region rows to their **first** value while all numeric rows still
participate in the colour range. This replaces the earlier speculative duplicate-aggregation
requirement with observed behaviour. A complete cached location → coordinate → entity query
chain produces three correctly coloured native regions; explicit IDs without those queries do
not. Private native cache observations are not shipped as fixtures or assets.

Numeric `valueColors` and `valueColorPositions` import, rebuild and render two/three-stop scales,
including finite numeric/percentage positions, clipping, theme-relative colours and alpha.
The default light/dark endpoints use luminance transforms of the document's accent, not bundled
palette artwork. Native automatic two/three-stop colour probes agree. **Explicit numeric/percent
stop children caused PowerPoint for Mac's PDF export to stall in this test environment**, including
alternate scalar encodings. These are not interoperability passes; the public-format controls are
retained, and their native compatibility remains an explicit investigation item. Normative
position definitions: [MS-ODRAWXML CT_ValueColorPositions](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/f2cae9d3-c8f0-4afc-a293-d3219902efb9)
and [CT_ValueColorEndPosition](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/6b29dfec-eade-47f4-a355-44a2e66dcda5).

`dataOnly` clips a cached world projection without changing its central meridian or stretching
regions. Default map outlines no longer invent white internal borders; explicit series outlines
still render. Attribution includes only displayed regions and retains source copyrights.
Intermediate administrative scopes, uncached provider data and all automatic projection policies
are not certified. No boundary service or proprietary map/font asset is embedded.

Rich chart labels now retain paragraph spacing, margins, tabs, RTL and line-break controls, plus
caps, strike, highlight, underline colour and kerning. Their bounds use whole-body text shaping
rather than adding run widths and counting paragraphs, so soft breaks and mixed sizes allocate
actual space. Flat and hierarchical treemap/sunburst leaf labels use the shared point-label
renderer and honour hidden points. An owned classic two-line mixed-size/decorated label opens,
exports and visibly agrees in its text styling in PowerPoint. The hierarchy probes open/export,
but native ignores the first point's style in the tested files while retaining a later point's
style; this discrepancy and automatic hierarchy/callout placement remain open.

LibreOffice 26.8 remains a comparison oracle, not a render dependency: the valid cached map
exports its unsupported-chart message, the uncached font-rule diagram is blank, and the rich
label's soft break is flattened in extracted PDF text. None of these is counted as a fidelity
pass. DeckCraft's direct export is exercised with no converter on PATH.

Final local validation for this arc: all eight `cargo xtask ci` stages pass (2,383 tests including
three doctests). Five original/private observation packages pass Open XML SDK validation with
zero errors, including the custom-stop package that stalls native PDF export. Thirteen direct
4K probes exported with converter-free PATH and unchanged source hashes; a final six-map rerun
covers the last viewport correction. These results establish bounded renderer coverage, not
pixel identity: world auto margins, some projection outlines and hierarchy placement still differ.


## PPTX.12 — joint rules and rich-label isolation (2026-11-01)

After the authored single-rule pass, unresolved text can now trigger joint moves of rules whose
measured dependency scopes overlap. Connected components retain instance identity and deterministic
traversal. Each candidate re-solves the full affine system, measures actual text, and checks absolute
maxima after reference factors have moved. Infeasible trials are rejected; invalid measurement and
resource-limit errors still propagate. The existing global work budget also covers grouping and
joint trials. Already-fitting independent labels are not changed. Repeated rules for the same
property retain their ordered policy rather than being treated as independent axes.

Original regressions cover a width/height plateau, equality-linked moves that are individually
infeasible, preservation of an unrelated font, and an imported real-text example. The native
real-text probe opens and exports as one intended rounded card, rather than falling back to generic
blocks. Its native dimensions and word breaks nevertheless differ: the current joint search follows
the solved affine relation and a common fraction of each authored range. This is not exhaustive
multidimensional search or a reproduction of every native constraint-priority policy.

Private native map observations cover constant positive, zero and negative data, a single value,
and automatic two/three-colour scales. Constant ranges select the midpoint of the final colour
interval (low/high for two stops; middle/high for three), rather than the lowest colour. Render
regressions include both cases and clipping outside a degenerate range. No boundary assets or
Office-produced files are shipped. Explicit numeric map-stop positions still time out in a fresh
native session with a 45-second allowance; the matching automatic three-stop control opens and
exports normally. This remains a native interoperability gap, not an export pass.

Rich data-label paragraphs now select their corresponding list level before inheriting common
paragraph defaults, rather than applying level-one formatting to every paragraph. Paragraph and
run overrides retain priority, including explicit false booleans. Native probes confirm distinct
30/22/16pt explicit levels and their colours. A `defPPr`-only fallback differs in the tested native
chart (18pt black instead of its authored default); automatic paragraph alignment/spacing also
remains under investigation.

A literal `c:tx/c:rich` replacement is now kept separate from the `txPr` used to style generated
numeric labels. Previously, unused numeric-label font, rotation, margins and spacing could leak
into the literal body. A render-and-rebuild regression uses deliberately conflicting styles.
Automatic label bounds now use shaped text dimensions rather than expanding to the initial plot
placement hint; explicit manual layout still resolves afterwards. Full axis/plot spacing and label
collision policy are not certified.

Final validation: all eight `cargo xtask ci` stages pass (2,389 tests including doctests).
Four original probe packages have zero Open XML SDK errors. Eleven final chart/diagram probes
export directly at 3840px width with no converter on PATH and unchanged source hashes. Seven
constant-map colour comparisons differ from native by at most one RGB unit per channel.
Native confirms that conflicting 60pt/red/rotated numeric styling does not change a literal
24pt horizontal black replacement. LibreOffice exports the rule diagram blank and the map with
an unsupported-chart notice; its explicit-level label text is present. These observations are
kept private and are not substitutes for whole-slide fidelity acceptance.


## PPTX.13 — native container fitting and map-stop interoperability (2026-11-01)

Linear layouts now fit the cross-axis extent independently of main-axis packing. Circular
layouts calculate a separating radius for constrained child rectangles, scale the whole set
uniformly when it exceeds the parent, then choose the orbit and centre its occupied bounds.
Font sizes are retained. Pair work is charged to the existing global budget; partial arcs and
centre nodes use the same geometry. Original regressions cover horizontal/reverse/vertical
packing, oversized cycles, partial/negative arcs, centre nodes and actual imported text.

Six original private probes open and export natively. At 3840px width, the coloured shape bounds
in the circular example differ by at most 0.25pt; all six cases stay within 0.75pt. This measures
shape geometry, not identical glyph rasterisation. A real-text coupled-rule example now respects
the parent height, but selected width and word breaks still differ. Common-fraction coupled
search and recursive constraint-priority behaviour remain incomplete.

A native UI-created custom map stop revealed an interoperability discrepancy:
[MS-ODRAWXML CT_ValueColorPositions](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/f2cae9d3-c8f0-4afc-a293-d3219902efb9)
lists `min`, `mid`, `max`, while the observed native application writes
`minPosition`, `midPosition`, `maxPosition`. The short names caused the earlier native open
failures. Import accepts both spellings, with native names taking precedence; export uses
native names. Number/percent/extreme values keep their `val` attributes as appropriate.
Four original numeric/percent/extreme/mid-only probes now open and export in approximately
one second each; their three painted region colours match the native output exactly.

This is deliberately a documented schema exception. Open XML SDK 3.3.0 flags the Position
suffix both in our output and in the private native UI-saved control. A native open/export
pass is not relabelled as a schema-validation pass. No native-produced document, geographic
asset or screenshot is included upstream.

Literal chart labels now ignore `lstStyle/defPPr`, matching native probes. An explicit level
on the first paragraph supplies the label-wide list level; without it, paragraphs select
individual levels. An explicit first-paragraph alignment applies across the label; subsequent
paragraph alignment and list-level alignment do not replace it. Direct paragraph/run font
properties still override inherited defaults. Numeric `txPr` bodies retain their existing
behaviour. Original tests cover first-level selection, later font overrides, explicit false
run properties, ignored common defaults and alignment. Native probes independently vary these
attributes, including equivalent expanded/self-closing paragraph tags. Automatic collision,
paragraph spacing and every chart-kind-specific rule remain outside this acceptance claim.

The final label probes keep font/colour and relative alignment semantics; absolute label
positions still differ with automatic plot geometry. In four measured cases, glyph positions
are approximately 6.5–7.5pt higher than native, and long labels can wrap differently. These are
retained gaps, not pixel-equivalent label passes. LibreOffice 26.8 exports the oversized cycle
without its diagram and reports the custom map as unsupported; the rich-label text is present.

Final validation: all eight `cargo xtask ci` gates pass (2,394 tests including doctests).
Thirteen final label probes export directly at 3840px width with unchanged source hashes
and no external converter on PATH. Private oracle outputs and toolchains remain excluded.


## Composite coordinate ownership and automatic label widths (PPTX.14)

A layout node's own text algorithm does not position that node within its parent. Explicit
child coordinates are consumed by a composite parent's constraint block; linear and other flow
algorithms supply their own positions. Size/font constraints remain supported on text nodes.
This follows the public [custom SmartArt authoring guidance](https://learn.microsoft.com/en-us/archive/msdn-magazine/2007/february/create-custom-smartart-graphics-for-use-in-the-2007-office-system)
and original two-node native comparisons, including swapped XML order and asymmetric controls.

An axis has two independent quantities. Native composite layout chooses explicit size, start,
end, centre in that priority order and ignores redundant equality coordinates. The solver now
makes that selection before constructing its affine system; inequalities and genuine reference
contradictions retain their bounded checks. Rule adjustments are inserted before axis selection,
so a joint trial cannot accidentally restore an ignored redundant edge. A regression fails
before this ordering fix and fits both dimensions afterwards. A root composite centres its occupied extent and
fits overflowing axes to the containing frame without scaling font sizes. Named descendant
relationships retain their relative offsets through this final placement.

Eighteen original coordinate/rule probes export directly at 3840px width. Painted bounds differ
from their native counterparts by at most 0.75pt, including unequal width/height rule limits,
redundant coordinates, parent/local ownership and flow positioning. The earlier long-text
example now also has the same word breaks. The prior test that treated local text-node edges
as a required `height = 2 * width` relationship was invalid; it now verifies that those edges
do not constrain independent size rules. These tests establish the stated semantics, not a
proof of complete rule ownership or arbitrary recursive layout parity.

Literal clustered-column data labels now enable automatic wrapping by default, using one fifth
of chart width, independent of column gap width and inside/outside placement. This is a native
black-box observation, not a schema requirement. Explicit no-wrap retains intrinsic width;
explicit manual width supplies the measurement rectangle. Numeric labels and other chart kinds
keep their existing behavior pending independent comparisons. Tests inspect actual painted
text dimensions across chart widths, manual sizing and gap changes after export/reimport.

Ten native label probes vary chart width, content, gap width, inside/outside placement and
explicit wrapping. Long-label vertical ellipsis/overflow and exact automatic plot positioning
remain distinct open work. Four representative original packages pass Open XML SDK validation.
No reference documents, native screenshots, geographic assets or private paths are published.

LibreOffice 26.8 exports the two representative uncached composites without their diagrams,
while retaining the rich-label text. These are not counted as visual-parity passes.

A separate ownership control moves the same size/font rule between the composite parent and
its text child. Native output applies the child-local rules but leaves the tested parent-owned
variants unchanged. The general selector evaluator still applies those parent-owned rules;
that observed difference is retained as open work, not included in the geometry acceptance set.

Final validation: all eight workspace CI gates pass, including 2,399 tests and doctests.
The new geometry/label export set contains 28 original pages at 3840px width, with unchanged
input hashes and no external converter on PATH. All upstream changes are source/tests/docs.
