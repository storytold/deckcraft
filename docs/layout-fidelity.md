# Bounded layout fidelity and font diagnostics

Updated 2026-11-02 (PPTX.48): centered snake fitting and rich-column baseline
constraints; integrated native comparisons retain unresolved differences.

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


## Font-rule binding and shared constraint groups (PPTX.15)

Rule selection is not just a list of destination nodes. Native parent font rules bind a
same-owner constraint with matching `type`, `for`, `forName` and `ptType`. Omitted `for`
and `ptType` normalize to `self` and `all`. Different selectors that happen to select the
same node do not establish the same binding. Original controls vary named/broad selection,
child/descendant axes and explicit/default point types. Remote size rules and an unmapped
container's inherited self-font rule do not fit child text in these controls.

A parent font rule updates all members selected by its constraint. A text node's local
rule can bind an ancestor constraint. An explicit shared equality or bound synchronizes
font changes across its selected members; ordinary defaults remain independent. The
search stores one multi-member adjustment, preserves prior adjustments on rejected trials
and detects overlapping property groups before joint search.

Synchronization does not widen a local fit trigger. With a local rule on the first node,
a fitting short label leaves both nodes at their initial size even when the second overflows.
A moderately long first label shrinks both shared members only until that first label fits.
A parent rule selecting both instead fits the longer label. Ten real-text variants exercise
these differences and export/reimport font/geometry preservation. They include an explicit
font without any rule: ordinary text-box autofit must not silently shrink it.

These are bounded native observations, not complete SmartArt conformance. Further probes
show that a local font rule does not propagate through a default reference constraint in
the same way as a parent-owned rule; explicit reference equality changes that behavior.
The generic reference solver still needs that distinction. A composite with its own mapped
text also differs from native in geometry/font inheritance. Invalid custom-layout probes
that native replaces with generic blocks or empty output are excluded from acceptance,
not counted as matching the intended layout. Map provider/viewport selection and long rich
label overflow remain separate open items. All probes use contributor-original layout XML;
no native-produced files, screenshots, fonts or user content are published.


Native comparison: of forty original ownership/group controls, thirty-nine agree in font
sizes and painted shape bounds within 0.75pt. The mapped-composite own-text control is an
explicit failure, not included in that passing count. The ten real-text regression variants
have native counterparts; four representative packages have zero Open XML SDK errors.

Final validation: all eight workspace CI stages pass, including 2,403 tests and doctests.


## Local font references and rule traversal (PPTX.16)

Native controls distinguish a displayed local font change from changing the constraint's
initial value. A default reference retains its original size when a different node fits
locally. Explicit font equalities propagate in both directions, including scaled references;
font bounds can tighten connected members. Parent constraint changes retain their existing
reference behavior. The implementation keeps local font values separate from their prior
constraint value and solves the connected font equations/bounds in a second bounded pass.
Unrelated font components are not activated. A regression reproduces unrelated nodes changing
size before the reachability fix and retains both sizes afterwards.

Children now apply their rules before the enclosing layout, preserving sibling and same-node
XML order. Numeric endpoints are search limits, not inherited font floors for every child.
A child that already fits therefore need not be enlarged to a parent's later rule endpoint.
The initial adjacent-integer search used in this stage was superseded by the displayed-font
measurement and one-point stopping policy verified in PPTX.21 below.
Local factor rules evaluate the selected reference size, including a zero initial multiplier.

Ten native default/bound controls use 24pt and 48pt source labels: unspecified sibling fonts
remain 36pt, default/equality references resolve their factors, and reference inequalities
alone do not choose the initial size. Explicit rules can subsequently activate those bounds.
The uncached diagram default is therefore 36pt; cached drawings retain their stored text.

Ten real-text local-reference/order regressions preserve font sizes and geometry through
export/reimport. The wider native comparison has forty-two controls: thirty-eight match
font sizes exactly, all painted shape bounds differ by at most 0.75pt, and four scaled
reverse-reference cases differ by one point in the dependent font. Those four differences were not exact font matches at this stage; PPTX.21 below resolves
them with independent font-search and reference-rounding controls.
Mapped-composite own-text geometry, native fallback/search policies, automatic geographic
viewport policy and long rich-label overflow remain separate work. No native outputs or
user/private assets are included in this change.

Final validation: all eight workspace CI gates pass (2,408 tests including doctests).
Four representative original packages pass Open XML SDK validation. The final CLI exports
all 60 original probe pages directly at 3840px width with unchanged input hashes and no
external converter on PATH. LibreOffice exports the two representative local-reference
diagrams without their content; those blank outputs are not fidelity passes.


## Composite geometry and mapped text ownership (PPTX.17)

An uncached composite's own visible shape encloses the occupied descendant shapes rather
than painting its entire provisional frame. Nested composites resolve from their children
outwards. Empty composites have zero occupied geometry; a space algorithm can retain its
specified frame without inventing a text layout. An explicitly sized root fits within its
resolved frame, rather than being re-centred in the original graphic-frame dimensions.

The composite's data mapping can point to text laid out by a descendant. Its visible frame
and text frame are then different. The matched text node owns the font, margins and text
rectangle; the outer frame must not rewrap that text using its larger width. This ownership
is retained through rule fitting and export/reimport. Text measurement keys include relative
mapped rectangles. The default current-point mapping remains distinct from an explicitly
empty mapping. Missing style labels use the default node label; unknown labels retain the
default quick style without taking unrelated colour-list entries.

Nineteen original non-rotated controls now match native font sizes. Seventeen have visible
geometry whose same-colour painted bounds differ by at most 0.25pt at 4K; two intentionally
empty composite controls remain empty. Painted bounds are measured against both raster
outputs, so portions hidden by other shapes are not compared with a complete vector box.
Four real-text controls exercise both mapped children with and without local fitting rules,
including original-source SDK validation and text/geometry round trips. These are geometry
and typography checks, not an assertion of identical antialiasing or all text baselines.

Four independent rotation controls expose an additional difference: native rotation changes
the dimensions of the shape within the allocated footprint and can counter-rotate its text.
They remain failing controls (up to 59pt painted-bound difference), not acceptance passes.
Omitting the mapping point type also changes native colour assignment in another control;
that case is retained as an open selector/style interaction. Scaled reverse-reference font
rounding, full search/fallback policy, administrative map viewports and long rich-label
vertical overflow remain open. No global parity score or full-converter-replacement claim
is made.

The final direct-render batch contains 51 contributor-original pages, all exported at
3840px width with unchanged source hashes and no converter on PATH. Workspace CI passes
all eight gates, including 2,412 tests and doctests. All native output remains private;
this change publishes source, generated-in-test XML and documentation only.

LibreOffice exports the two representative mapped-text diagrams without diagram content
(zero extracted text and only page-background drawings); these are not fidelity passes.


## Rotated footprints and independent text frames (PPTX.18)

Rotation is applied after constraint solving, flow placement and occupied-container fitting.
A shape maximizes its local rectangle area inside its allocated axis-aligned footprint:
`c*w+s*h <= W`, `s*w+c*h <= H`, where `c` and `s` are the absolute cosine and sine.
The optimum can lie on either single active edge or their intersection; proportional
shrinking does not reproduce the native narrow-rectangle controls. Parent packing continues
to use the original allocated footprint, so a child's rotation does not move its siblings.

Text algorithms retain a separate frame. `autoTxRot` supports follow-shape (`none`),
upright (`upr`, the [Office default](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376/526e0ee0-208f-42e5-97f0-3f272c1093c6)) and gravity (`grav`) behavior, including quarter-turn
boundary ties and wrapped angles. Preset adjustments determine the text area before its
orientation is resolved. A composite borrows its matched text algorithm's final frame and
font, independently of the composite's own physical rotation. Rule measurements use that
frame after theme/font resolution. Only after fitting are frames materialized as ordinary
editable text: compatible insets on the physical shape, or a separate frameless text shape.
A failed or budget-limited text search logs the failure and materializes the unmeasured
layout instead of losing the independent frame; it does not claim successful fitting.

Seven separate ordinary-text control pages exposed a related shared-layout error. Opposing
insets that cross now collapse at their midpoint, preserving asymmetric margins on both
axes. For LTR wrapped text narrower than one glyph, right alignment retains the start edge
and centred overflow shifts the paragraph as a block while retaining positive line slack.
Explicit no-wrap retains signed alignment slack. These fixes apply to regular text boxes,
not to named diagrams or particular pages. The controls vary both axes, three anchors and
alignments, widths, wrapping, glyph widths and spaces. Extracted glyph origins differ from
native by at most 0.086pt horizontally and 0.452pt vertically in this set; this is not a
claim of identical rasterisation or untested RTL overflow behavior.

Forty-two original rotated/fit/mapped diagram controls match native font-size sets and
same-colour painted bounds within 0.25pt at 4K. Four fitted mapped-text variants also pass
Open XML SDK validation and retain editable text, font sizes and geometry after export and
reimport. An invalid-rule variant verifies that the fallback retains orientation. Independent
unit tests check area optimality against feasible samples and native angle-boundary cases.
All 49 original pages export directly at 3840px width with unchanged source hashes and
no external converter on PATH. Native-generated output remains private and uncommitted.

The four rotation failures reported in PPTX.17 are addressed by this phase. Omitted-point-type
colour assignment, four scaled reverse-font rounding cases, full native search/fallback
semantics, administrative/automatic map viewport selection, rich-label vertical overflow,
plot spacing and callout routing remain explicit limits. No global parity score is raised.

Final workspace CI passes all eight gates, including 2,417 tests and doctests. Sixty-five
previous reference-font/composite probes retain their font sizes with unchanged sources;
the same four scaled reverse-font cases retain their known one-point native differences.
The earlier regression claim about vector geometry was based on empty path lists from raster
PDF output and is withdrawn; see the pixel-based recheck in PPTX.22 below.
The four previously failing rotation controls now match native same-colour painted bounds
exactly at the tested 4K raster resolution; this does not assert whole-image equality.

## Administrative map scope and regional projection (PPTX.19)

The geographic model now retains entity types, parent links and provider extents, including
ancestors with an extent but no polygon. The reader joins both child-query and parent-query
tables by entity identity, independently of table order. Rebuilding a chart writes legal
`ST_EntityType` names and the parent-query table; an extent-only ancestor remains an extent,
not invented boundary geometry. The lineage reader limits nodes, work and metadata bytes;
scope traversal uses an iterative, budgeted graph walk, so cycles cannot recurse indefinitely.

Automatic scope selects the nearest shared administrative ancestor of the bound data.
An explicit scope imposes a minimum administrative level. In the public
[mapping-level definition](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/4f06cf76-5e65-4c54-aa9b-f6996c7cbb8b),
`countryRegionList` means continent. Only descendant regions paint; cached ancestor geometry
provides projection context without overpainting its children. `dataOnly` hides background
regions without changing that context. Missing, oversized or untyped singleton hierarchies
retain the earlier bounded fallback rather than inventing a country or global view.

Regional automatic/Robinson views use spherical Albers with secants at one-sixth and
five-sixths of the selected latitude range, following the public
[USGS projection guidance](https://pubs.usgs.gov/pp/1453/report.pdf).
The symmetric-secant limit is cylindrical equal-area. Provider extents take precedence over
the polygon union when available. Longitude wrapping is centred on the central meridian:
rounding between extent metadata and compressed coordinates can otherwise send a vertex
almost 360 degrees away. A contributor-original southern-hemisphere/dateline regression
failed before this correction and passes afterwards. No country names, IDs, boundary assets
or fitted per-country constants are embedded in the implementation.

Validation and remaining differences:

- Twenty-five private original-derived geographic controls cover one region, neighbouring
  regions and regions on different continents across five viewport modes, four explicit
  projections and six additional countries. All open/export in PowerPoint and export directly
  at 3840px width with unchanged source hashes and no external converter on PATH. Four initially
  blank native exports were retried after allowing native chart initialization; blank outputs
  were not accepted as comparisons.
- The seven automatic single-country cases have coloured-land-mask intersection-over-union
  of 95.85–99.60% at 4K. This measures painted areas, not text, coastlines, tiny-island bounds or
  whole-slide equality. Native simplification and rasterization still differ. A regional
  projection-coordinate control agrees within 0.072pt after fitting scale and translation;
  that fitted transform is an observation, not a production constant.
- The original continental controls lack the common ancestor's provider extent. PowerPoint
  fetches and saves additional geographic data; a descendant-only union is not equivalent.
  The paired-country `dataOnly` control improves from 69.72% coloured-mask overlap to 98.38%
  when both renderers receive the same saved, complete cache. The native cache is private
  observation data and is never shipped. No network map provider is silently invoked.
- Global viewport placement still differs by about 22pt in these controls. The selection
  of countries is correct, but this is explicitly **not** world-map visual parity. Missing
  ancestor context, all provider projection policies, simplification and global plot fitting
  remain work items alongside the earlier SmartArt/font and rich-label limits.
- An original synthetic hierarchy with an extent-only ancestor survives two forced rebuilds,
  passes Open XML SDK validation with zero errors and opens/exports in PowerPoint without
  dismissing a repair dialog. Source tests cover malformed extents, node budgets, cycles,
  requested scope, untyped legacy caches, southern/equatorial Albers and the dateline seam.

Final workspace CI passes all eight gates, including 2,425 tests and doctests. Chart/SmartArt
parity scores remain partial. Production rendering still invokes neither Office nor LibreOffice.

## Global map framing and projection limits (PPTX.20)

A global view now preserves each cached region's crossing of the dateline using its valid
west/east extent. It does not split that region across opposite edges of the world. Noncrossing
and malformed extents leave the original longitudes alone. Complete global caches fit their
actual projected bounds; an extra full-globe rectangle had incorrectly shifted and shrunk the
painted geography. An explicitly global view with only local cached geometry retains a global
frame as a bounded fallback. Explicit data-only cropping remains supported.

Private native controls vary chart width, height, offset, font size and explicit projection.
They show a consistent 5.775pt plot inset. Automatic/global Albers selections use Robinson,
whereas regional automatic/Robinson selections use the regional Albers implementation. The
native Robinson vertical normalization approaches 4/3 as chart size increases; this compatibility
approximation replaces the standard 1.3523 factor, but does not reproduce native finite-size
quantization exactly. Global Mercator and Miller views cap their southern geometry at 79°S;
regional views and Robinson retain their full latitude range. Mercator's finite pole limit is
atan(sinh(pi)), approximately 85.05112878°, independently of that chart-level southern cap.

Validation and limits:

- Twenty-one unchanged original/private-cache controls open/export in PowerPoint and export
  directly at 3840px width. Default-world coloured-mask IoU is 95.67%; explicit global Mercator
  and Miller reach 96.97% and 97.22%. Across sizes, coloured-mask IoU ranges from 92.83% to
  99.18%, and all-land IoU from 94.93% to 99.08%. These are painted-mask intersections over
  unions, not whole-slide equality or a general percentage of PowerPoint compatibility.
- Eight cylindrical controls independently recover a southern limit within 0.00002° of 79°S
  from native vector coordinates. The fitted retained-ring controls differ by less than
  0.0006pt. This verifies the projection rule, not every native boundary vertex.
- Source and native-saved global caches contain the same polygon data. The observed
  change is rendering/framing, not a refreshed boundary dataset. No boundary assets are bundled.
- Native simplification removes vertices and tiny rings. This still differs, especially on
  small charts; a tiny faraway island can change a mask's bounding box by tens of points despite
  high area overlap. Full painted bounding-box equality is therefore **not** claimed.
- All 25 previous geographic controls also export with unchanged source hashes. Seven
  single-country automatic views now have 96.86–99.76% coloured-mask IoU. The earlier
  single-country global-view case improves from 46.32% to 95.67%. Three incomplete-ancestor
  continental controls move from 59.601% to 59.585%; that missing-context failure remains
  explicit and is not counted as a fidelity pass.
- Original synthetic regressions cover dateline continuity, invalid extents, global/regional
  projection selection, incomplete-cache framing and the scope of southern cropping. They use
  contributor-created rectangles, not provider boundary assets.

Missing ancestor geometry, provider viewport policy for incomplete caches, simplification and
finite-size quantization remain open. SmartArt rule/fallback and rich-label overflow limits from
the preceding sections are unchanged. Production rendering invokes no external document converter.

Final workspace CI passes all eight gates, including 2,429 tests and doctests. Parity scores
remain partial; no native/provider/user assets are included.


## Displayed font trials and fractional references (PPTX.21)

Uncached SmartArt font fitting distinguishes authored defaults, temporary local font values,
and displayed font sizes. Conflating these stages produced one-point differences in scaled
reverse references even when the rule's own text displayed at the expected size.

- Authored constant font sizes round before ordinary reference defaults are evaluated. Each
  subsequent acyclic reference edge rounds its own result; a fractional chain is not rounded
  only once at the end. Cyclic components retain their rounded affine solution rather than
  repeatedly rounding around a cycle, which could drift or oscillate.
- A local font search starts from the displayed whole-point size and stops when its remaining
  absolute interval is at most one point. Geometry searches retain finer convergence. The
  selected fractional endpoint remains available to connected local reference equations.
- Every trial is measured using the whole-point fonts that will actually paint, including
  secondary fonts and their recalculated text insets. Reference propagation itself retains
  fractional values until that candidate is materialized.
- A local inequality activates only if the displayed sizes violate it. An already satisfied
  bound leaves its linked default unchanged. After a connected bound moves the group, remaining
  bounds are checked again within the existing work budget. Activated constraints propagate
  fractional values before the final display rounding.

These policies are black-box observations, not promises made by the OOXML schema. They were
checked with equality/lower/upper bounds, factors from 0.4 to 2, different text lengths and box
sizes, source-versus-target rules, forced font floors and multi-edge fractional defaults. The
forced-floor controls distinguish an actual 14pt endpoint from an intermediate trial that only
*displays* at 14pt; dependent fonts can legitimately differ between those two cases.

Validation:

- All 108 new pages match native primary/secondary font-size sets: 60 reference/rule cases,
  24 forced-floor cases and 24 fractional-default/secondary-font cases. This includes the four
  previously failing scaled reverse-reference controls.
- All 154 earlier diagram controls match native font-size sets. Their source hashes are
  unchanged. The earlier claim that all filled-path geometry was identical is withdrawn:
  the regression checker compared empty path lists from raster PDF artwork. PPTX.22 below
  records the replacement pixel-bound measurements.
- The current CLI directly exports the 60-page reference matrix as 3840x2160 PNGs with no
  external converter on PATH. Selected direct images and native counterparts were visually
  inspected; font-set agreement is not whole-slide or line-break agreement.
- Original synthetic regressions failed before the correction and pass afterward. Full
  workspace CI passes all eight gates, with 2,431 tests including doctests.
- LibreOffice 26.8 exports the same 60-page uncached reference matrix without diagram content
  (zero text characters). This is an explicit comparison failure, not a fidelity pass.

Two visual differences were observed in these controls: colours and some short-label word
wrapping, even when font sizes agree. PPTX.22 below separates the invalid shared-part colour
probe from genuine helper-visibility and colour-index defects. Complete
native cyclic-font, search and fallback policies still need broader controls. Geographic
simplification/incomplete provider context and rich-label overflow/callout limits are unchanged.
No native output, provider asset, user content or oracle toolchain is distributed.

## Helper visibility and layout-order colours (PPTX.22)

An uncached diagram's text algorithm can be a nonpainting layout helper, a text-only shape
with `hideGeom`, or a visible shape. These cases now retain their distinct painting behaviour:

- A missing shape or `type="none"` helper still supplies text measurements and bounds; its
  own text does not paint. PPTX.35 below corrects the earlier assumption that an enclosing
  shape should borrow this helper’s text. Generated separate text frames retain the source
  shape’s hidden state through export/reimport.
- A shape with `hideGeom="1"` keeps its text visible, uses colour-list index zero, and does
  not advance the style's colour sequence. Each style label retains its own sequence.
- Connectors consume a colour slot in layout order. Moving connectors behind nodes for
  painting no longer changes subsequent colours.

The native probes also corrected an earlier diagnosis. The multi-page font matrix reused a
single colours part. PowerPoint changed later diagrams' starting colour with that packaging;
giving each diagram its own colours part removed the apparent omitted-point-type discrepancy.
The public OOXML part constraints require one [Diagram Colors part per diagram](https://c-rex.net/samples/ooxml/e1/Part1/OOXML_P1_Fundamentals_Diagram_topic_ID0EJPMM.html)
and one [Diagram Style part per diagram](https://c-rex.net/samples/ooxml/e1/Part1/OOXML_P1_Fundamentals_Diagram_topic_ID0EYNNM.html).
The final controls use independent data, layout, colours and style parts. The invalid shared-part
colour behaviour is not treated as a target rendering rule.

Validation and limits:

- Sixteen original hidden/absent/none/connector controls match native text-colour counts and
  standalone text visibility; eight integration variants verify colours, hidden-state handling
  and exact rendered pixels after export/reimport. The initial regression failed before the fix.
- Forty-one original pages open/export in native PowerPoint and render directly to 3840x2160
  PNG without an external converter on PATH, with unchanged source hashes. Thirty-eight have
  visible same-colour bounds within 0.25pt of the native raster. Three explicit failures remain:
  two connector-first composite allocation cases and one empty space-helper allocation case.
  Fully occluded native paths are excluded from bounds measurement, not counted as matching
  absent pixels. Colour/text-count agreement does not establish whole-slide equivalence.
- The old 154-case regression was remeasured from raster pixels: 140 have unchanged nonempty
  same-colour bounds at 0.5pt sampling, ten provide no measurable coloured region, and four
  rotated composites differ from their pre-PPTX.18 baselines. Those four were independently
  validated against native 4K painted bounds in PPTX.18. Empty masks are not geometry passes.
- Full workspace CI passes all eight gates with 2,432 tests including doctests. Selected
  before/after/native images were visually inspected; native output stays private and uncommitted.

Some near-threshold short-label wrapping still differs with the same installed font. Controls
that substituted a different font natively are excluded from font-metric conclusions. Global
ownership when multiple text algorithms map the same data point, composite allocation involving
nonpainting helpers/connectors, full cyclic-font/search/fallback policies, colour interpolation,
map simplification/incomplete provider context and rich-label overflow remain open. This phase
does not certify full LibreOffice replacement.

## Allocation bounds and recursive composite fitting (PPTX.23)

Layout allocation and painting are distinct. Space, text and connector algorithms can reserve
a frame without emitting a shape. Their frames now survive until composite fitting finishes;
pure allocation helpers are then removed before import/export produces drawing shapes. Text
helpers with real data remain available for measured rules and mapped text ownership.

A positive width **or** height contributes to occupied bounds, including a zero-width vertical
allocation or a zero-height horizontal one. A point with both dimensions zero does not. Empty
composites still occupy no space. Connector allocation contributes before endpoint routing,
independently of connector paint order. Each nested composite fits its occupied children into
its own allocated frame before its parent places the result; this was previously done only at
the root. Public background on the separate algorithm roles and composite-local constraints
is in the [DrawingML SmartArt authoring documentation](https://learn.microsoft.com/en-us/archive/msdn-magazine/2007/february/create-custom-smartart-graphics-for-use-in-the-2007-office-system).
Exact fitting policies here are checked through independent native observations.

Validation:

- Thirty-nine new original allocation controls vary algorithm, missing/hidden/visible shape,
  placement, zero dimensions, overflow, parent shape and nesting. Fourteen passed before this
  phase; all 39 now match native visible same-colour bounds within 0.25pt at 3840px width.
- All 41 prior visibility/colour controls now pass the same bounds comparison, including the
  three allocation failures recorded in PPTX.22. This gives 80 passing allocation/colour pages;
  it is not a whole-slide pixel-equality claim.
- All 39 rebuilt PPTX pages reopen/export in PowerPoint without a script dismissing repair
  prompts. The nonempty coloured-path bounds, font-size sets and extracted text match the
  corresponding native exports of the original sources exactly at the recorded precision.
- Against the PPTX.22 executable, all 154 earlier single-page controls produce pixel-identical
  direct PNGs at 2px/pt. All 154 font-size sets are unchanged and match their native references.
  This is a fresh image comparison, independent of the withdrawn empty-vector check.
- Eighty allocated-frame unit variants and three nesting variants cover the new rules. Six
  integration variants retain only the two real drawing shapes and preserve geometry and exact
  render pixels after export/reimport. The coupled affine-reference test checks solved frames
  before composite fitting, rather than incorrectly equating those with final painted edges.
- Full workspace CI passes all eight gates with 2,435 tests including doctests. Direct exports
  run without an external converter on PATH, with unchanged source hashes. Selected 4K native
  and direct images were visually inspected. LibreOffice 26.8 exports the 39 uncached controls
  without their diagram text (zero extracted characters), which remains a comparison failure.

A separate eight-page cross-container constraint matrix improved from zero to two passing
cases. Six remain explicit failures: four ancestor-to-descendant constraint-scope cases and
two overflowing inner frames with differing scale. Probes rendered using native fallback are
excluded from intended-layout parity counts. Full native search/fallback and cyclic-font rules,
near-threshold font wrapping, duplicate-point text ownership, colour interpolation, geographic
provider/simplification policy and rich-label overflow/callouts remain open. No global parity
score or complete LibreOffice-replacement claim is raised.

## Overflowing composite end edges (PPTX.24)

An overflowing composite axis scales from the frame origin, or from the occupied minimum
when it is negative. Scaled end edges are fitted to the frame **before** the resulting occupied
bounds are centered. Removing positive leading space before fitting changes the final size:
for a 20mm frame and a child starting at 5mm with width 35mm, the native result is about
17.142857mm wide, rather than 20mm. Equivalent start/end, start/size, size/end and size/center
constraints agree. Non-overflowing axes translate without clipping or enlargement.

A fully disjoint child retains its scaled extent and aligns its end to the frame. Multiple such
children can therefore overlap. A child starting exactly at the boundary can instead have zero
extent; the comparison tolerates floating-point evaluation noise so translated frames do not
accidentally take the disjoint branch. Horizontal and vertical axes follow the same rule.
Mapped text frames follow geometry while font sizes remain unchanged. Occupied bounds are
computed without the previous temporary vector, and all passes count against the work budget.

Validation uses contributor-original layout definitions and private native observations:

- Forty edge-pair controls and 21 boundary/multiple-child controls match native visible
  same-colour bounds at 4K. Twelve additional controls cover far-outside starts, exact-boundary
  starts, negative positions and overlapping disjoint children.
- All 73 controls match native coloured-path geometry within 0.009pt. This comparison reads
  the actual imported model rectangles, including group offsets, against nonempty native PDF
  paths; it does not compare two empty vector lists. Model inspection rounds to 0.01pt.
- Seventy-one of the 73 visible coloured-pixel checks pass within 0.5pt. Two very thin text
  shapes retain 0.75pt and 1pt visible-mask differences despite matching geometry; native text
  obscures parts of their fill. They are retained raster failures, not whole-slide parity passes.
- Fifty-eight unit variants cover individual and shared-axis geometry, actual positions,
  disjoint overlap, unchanged perpendicular dimensions and unchanged font sizes.
- The separate eight-case cross-container matrix improves from two to four passes: both inner
  overflow differences are resolved. All four ancestor-to-descendant constraint-scope failures
  remain explicit. Other native-fallback probes are excluded from intended-layout comparisons.
- The 80 prior allocation/colour pages still pass; all 154 earlier direct PNGs are identical
  to the PPTX.23 baseline, and their font-size sets remain unchanged and native-matching.
  Source hashes are checked against native-export receipts. No external converter is on the
  direct renderer's PATH. Full workspace CI passes all eight gates with 2,437 tests, including
  doctests. Representative native/direct images were inspected, including both raster failures.

This phase does not change descendant-selector semantics. Native font constraints reach deeper
than geometry constraints in the tested contexts; a generalized scope rule is not yet established.
Full native rule-search/fallback, cyclic-font policy, near-threshold wrapping, duplicate-point
text ownership, colour interpolation, geographic provider/simplification policy and rich-label
overflow/callouts remain open. It is not a claim of complete LibreOffice replacement.

## Local geometry scope and size precedence (PPTX.25)

The constraint language distinguishes layout relationships and data-point selectors; their
public definitions are documented in the [constraint API](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.diagrams.constraint)
and [point-type enumeration](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.diagrams.elementvalues).
The native behavior below is established through independent original controls, rather than
inferred from the schema alone.

Composite and space algorithms establish local size scopes. An ancestor's descendant width
or height constraint does not enter that scope, while descendant font constraints can still
reach the text. Flow containers can consume ancestor size defaults, but an explicit local size
takes precedence. Composite size constraints use layout selectors; adding a non-`all` point-type
selector does not select their geometry in the measured cases. Positional constraints are
consumed by composite parents through child relationships, not descendant relationships.
The same applicability rules now govern numeric-rule binding and dependency tracking.

The nearest local geometry scope is recorded during the existing tree walk, so applicability
does not add an ancestor walk per candidate. The affine system retains each equation's owner
to keep local size precedence without changing text-constraint precedence. Equal-size occupied
bounds also tolerate floating-point roundoff before choosing the overflow branch; otherwise a
translated child as wide as its frame could be clipped despite having no real overflow.

Validation:

- All eight original cross-container controls now match native visible coloured bounds within
  0.25pt at 4K, resolving the four ancestor-scope failures remaining after PPTX.24. Representative
  native/direct results were visually inspected.
- All 48 new context, mapping, point-type and font controls match actual native geometry;
  all 48 font-size sets also match. Direct versus iterator context and empty versus populated
  presentation mappings are varied independently.
- A broader 186-case matrix improves from 92 to 120 model-geometry matches, using a 0.025pt
  tolerance for model inspection rounded to 0.01pt. It includes context selection, three
  container algorithms, local-default binding and implicit versus explicit edge sizes.
  **54 remain failures**; 12 native-fallback pages are separately excluded from intended-layout
  comparisons. This is not a claim that the full selector/default-layout matrix passes.
- The 73 prior overflow controls still match native model geometry within 0.009pt. All 154
  earlier 2px/pt PNGs are identical to the PPTX.24 baseline, with unchanged/native-matching
  font-size sets. All 80 earlier allocation/colour controls still pass. Source hashes are
  checked against the native-export receipts.
- Three new tests cover 15 scope/precedence variants; 12 more translated equal-size variants
  protect the floating-point fix. The earlier cyclic-system test now checks that ignored
  descendant positions leave the leaf in its local frame, while retaining the coupled size
  cycle and inequality assertions.
- Final `cargo xtask ci` passes all eight stages and 2,440 tests including doctests.

Unspecified names and implicit/inverted edge sizes still differ in the expanded matrix.
Unconstrained flow sizing and finite diagram quantization also need further native validation.
Full native search/fallback and cyclic-font policies, colour interpolation, text ownership,
geographic provider/simplification policy and rich-label overflow remain open. No global parity
score or complete LibreOffice-replacement claim is raised.


## Implicit endpoints and positive display extents (PPTX.26)

For a composite child with only a start coordinate, the opposite edge defaults to the
container's corresponding edge. With only an end coordinate, the start defaults to zero.
A centre without a size allocates no extent and leaves its start at the default origin,
but another node can still reference that authored centre value. Adding an explicit size
changes this behavior: start, end or centre then positions that size normally.

Endpoints and display sizes cannot share a single affine representation in inverted cases.
For example, a 20mm frame with an authored left edge of 25mm retains a right-edge reference
of 20mm and a centre reference of 22.5mm, while displaying a 5mm width starting at 25mm.
Width references see the positive 5mm. Explicit negative sizes and negative-factor size
references also display positive sizes; they do not mirror an asymmetric shape.
Independent paired-coordinate and witness-shape probes establish these distinctions.

The solver now keeps endpoint/centre variables separately, with signed spans and positive
size relations. It resolves sign choices under the shared work budget and rejects repeated
sign states. It does not claim exhaustive nonlinear/cyclic branch search. Rules treat a
rejected sign state as an infeasible candidate. Font defaults read the resolved reference
values, rather than reconstructing raw endpoints from display rectangles. Initial local
sizes use the same absolute-size convention so nested children retain their proportions.

Zero-width or zero-height occupied frames can still be centred without division by zero.
Linear packing tolerates floating-point roundoff when checking whether a composite fits its
allocation. This preserves an existing native-measured nested-card test whose arithmetic
order exposed a false overflow during the solver change; its expected result is unchanged.

Validation:

- All 60 prior implicit/explicit edge cases now match native nonempty PDF paths. The older
  186-case matrix improves from 120 to 144 matches at 0.025pt model-inspection tolerance;
  30 failures remain, with 12 native-fallback pages separately excluded.
- All 140 new edge-pair, signed-reference, witness-position and paired-coordinate cases match
  native geometry within 0.009pt. The broader new matrix has 176 cases and improves from 56
  to 152 matches, with no formerly matching case regressing. The other 24 failures concern
  local self-sizing combined with parent end/centre positions; they are not parity passes.
- All 60 new two-shape 4K visible-colour-bound comparisons pass within 0.5pt. Representative
  native/direct images were visually inspected, including asymmetric inverted and zero-width
  shapes. Geometry agreement alone is not a whole-slide pixel-equivalence claim.
- All 73 earlier overflow geometry controls still pass within 0.009pt. All 154 earlier 2px/pt
  PNGs remain identical to the PPTX.25 executable, with unchanged/native-matching font-size sets.
  Source hashes are checked against native-export receipts; direct export has no document
  converter on PATH.
- Four new unit tests cover 72 variants of endpoint references, negative-factor positioning,
  translation, zero-axis centring and inherited local dimensions. A new integration test retains
  graph identities, positive unmirrored geometry and identical rendered pixels through export
  and reimport. Existing cyclic-system and numerical-rule tests remain green.
- All 176 new native/direct font-size sets match, including the cases whose geometry is still
  unresolved. The 39 allocation, 16 helper-visibility and 60 unique-colour controls also pass
  their 4K visible-bound comparisons; the earlier 25 colour-sequence controls also pass. These
  metrics do not imply whole-slide equality.
- Final `cargo xtask ci` passes all eight stages, including 2,445 tests and doctests.

Native behavior for local self-size versus parent end/centre positioning remains unresolved,
as do unnamed selectors and automatic flow dimensions. Full native rule search/fallback,
cyclic-font policies, geographic context/simplification and rich-label overflow remain open.
No global parity score or complete LibreOffice-replacement claim is raised.


## Parent allocations versus self-size fallbacks (PPTX.27)

A child's self width/height is not the same kind of constraint as a parent allocation.
An explicit parent size wins over the child's self size. A local child allocation still
wins over more distant defaults, so this does not globally reverse ancestor precedence.
Two parent coordinates determine the allocated extent before axis-priority selection;
the child's self size must not remove the second coordinate from that calculation.
Local self-size `gte`/`lte` constraints do not allocate initial geometry, including when
no explicit parent size exists.

When the parent supplies only a start, a self size can supply the extent. When the parent
supplies only an end or centre, that local fallback does not establish an allocation:
native layout retains the authored end/centre reference but collapses the other coordinates
and the displayed extent to zero. For example, a self width of 5mm with parent right=15mm
retains a right reference of 15mm, but width, left and centre references are zero. Adding
parent width=12mm instead produces left=3mm and width=12mm. Witness shapes verify these
reference values independently of final centring and clipping.

Equation ownership and numeric-rule binding now distinguish local self sizes from child
allocations. Before choosing the two axis equations, local self sizes yield to parent
coordinate pairs. Unallocated self sizes retain their authored positional reference without
inventing a display rectangle. The additional pass is bounded by the shared work budget.

Validation:

- The previous 176-case edge/self-size matrix now passes in full at 0.025pt model-inspection
  tolerance, resolving all 24 previously reported self-size/parent-position failures.
- All 195 new intended-layout controls in the core parent-allocation, local-policy and vertical
  matrices match native geometry within 0.009pt. Nine self-referential-size controls make native
  PowerPoint choose its generic three-node fallback; they are excluded from intended-layout
  comparisons, not counted as faithful renderings of their original definitions.
- Including 12 further scope controls, the full 216-case matrix improves from 53 to 202 matches.
  Five geometry differences remain: one cross-scope self-size default and four space-container
  placement cases. Native/direct font-size sets match for all 207 non-fallback cases; the other
  nine differ and remain explicit. Font agreement is not whole-slide equivalence.
- All 72 horizontal policy controls match visible coloured bounds within 0.5pt at 4K.
  Representative native/direct images were visually inspected, including collapsed extents
  and retained end/centre references. Native source hashes match export receipts.
- All 154 prior 2px/pt PNGs remain identical to the PPTX.26 executable, with unchanged/native-
  matching fonts. All 73 prior overflow geometries still match within 0.009pt. The older
  186-case matrix remains at 144 matches, 30 failures and 12 native-fallback exclusions.
- Three new tests cover 136 parent/self-size, local-bound, paired-coordinate and witness-reference
  variants across both axes. Final `cargo xtask ci` passes all eight stages and 2,448 tests
  including doctests; the 21 diagram preservation/round-trip tests remain green.

A self-size can expose an ancestor default through a composite scope in a further native
control; space containers have distinct allocation/centred-placement behavior. Those cases
are still open, together with unnamed selectors, automatic flow sizing, full native rule and
fallback policies, cyclic fonts, geographic context/simplification and rich-label overflow.
No global parity score or complete LibreOffice-replacement claim is raised.


## Space containers and inherited size defaults (PPTX.28)

A leaf space reserves its allocation. A space with children instead encloses their occupied
bounds and centres the group in its allocated frame. It does not resize an overflowing child;
its enclosing composite may subsequently fit that group. A painted space uses the children's
occupied rectangle, rather than retaining a larger empty background. The space's allocation
also takes precedence over a child's self dimensions, without suppressing its font defaults.

A composite's local size scope can accept an ancestor replacement for an existing self-size
fallback. Such a replacement must retain its fallback role: it cannot turn a parent end/centre
coordinate into a sized allocation. Explicit local child allocations still win. A nested linear
layout reopens ancestor size defaults even inside a composite or space container. Constraint
applicability and rule binding use the same scope information. Extra scans and placement are
charged to the existing shared work budget; no input-specific names or dimensions are used.

Validation:

- All 16 previously failing space-container controls now match native nonempty path geometry
  at 0.025pt inspection tolerance. They vary child self sizes, parent allocations, painted and
  nonpainting containers, and one/two children.
- The prior 216-case matrix now has 207 geometry matches and nine native generic-fallback
  exclusions, resolving its five scope/space failures. All 176 earlier edge controls still pass.
- A new 94-case matrix covers root/nested containers, coordinate roles, overflow, negative sizes
  and three-level allocation scopes. Geometry matches improve from 44 to 93 with no previous
  match regressing. The other case makes native PowerPoint select its generic fallback and is
  excluded; the intended layout is not reproduced natively. All 93 non-fallback font-size sets
  match. The maximum geometry difference is 0.0224pt; small native flow quantization remains.
- Six named native controls confirm centring of an already-fitted composite inside a space.
  Their inspected geometry differs by at most 0.01pt. Three old unit tests had assumed that a
  space did not centre its children; their position assertions now account for that operation,
  while their existing expected dimensions remain unchanged.
- At 4K, 14/16 original space controls match visible coloured bounds within 0.5pt. Two overlap
  cases retain a 2.5pt difference in the first exposed pink pixel, despite matching model/path
  geometry and the other colour bounds. Clearing text in 16 independently exported controls
  removes the discrepancy: all 16 pass the same 4K colour-bound check. This isolates a remaining
  typography/occlusion issue; the original two cases are not counted as raster-fidelity passes.
  Representative native/direct images were visually inspected. Hash receipts bind each native
  export to the unchanged input, and direct rendering has no document converter on PATH.
- The older 186-case matrix improves from 144 to 152 geometry matches, with 22 failures and
  12 native-fallback exclusions remaining. All 73 overflow geometries still match within 0.009pt.
  All 154 previous direct PNGs remain identical to the PPTX.27 executable, with unchanged and
  native-matching font-size sets.
- Three new tests cover 96 space, fallback and nested-linear variants. Final `cargo xtask ci`
  passes all eight stages and 2,451 tests including doctests; the 21 diagram preservation and
  round-trip tests remain green.

Unnamed selectors, automatic flow sizing, full rule/fallback search, cyclic font policies,
duplicate-point text ownership and colour interpolation remain partial. Geographic context,
simplification/quantization and rich-label overflow/collision remain open. These checks neither
raise global parity scores nor establish complete LibreOffice replacement.


## Child defaults and size inequalities (PPTX.29)

A later unnamed child (`for="ch"`, absent or empty `forName`) geometric declaration
must not overwrite the dimensions or positions already assigned by the same layout node.
Constraint resolution and numeric-rule binding now retain the same earlier assignment,
including its reference and factor. Descendant (`for="des"`) constraints have distinct
native behavior; this change deliberately does not generalize child precedence to them.
No geometry policy is applied to font selectors.

Width, height and diameter `gte`/`lte` constraints do not allocate initial geometry in
the measured native cases. They also do not cap the tested width-growth rules. Treating
these declarations as mandatory affine inequalities could reject a valid layout or
change its sizes; they are now excluded from both allocation and rule binding. Other
numeric properties retain the bounded affine inequality solver. Three solver unit tests
and one cyclic import/round-trip test now express their mathematical bounds through an
auxiliary property, preserving chain/cycle/contradiction coverage without asserting a
native size-bound behavior contradicted by observation. These solver tests do not certify
native handling of their cyclic definitions.

Validation:

- A new 129-case original matrix varies declaration order, absent/empty/named selectors,
  default/explicit equality operators, existing/self dimensions, rules and four algorithms.
  Geometry matches rise from nine to 46 at 0.025pt model-inspection tolerance, without a
  prior match regressing. All 40 intended composite controls and six axis controls match.
  Fifty-four linear/space/cycle controls retain automatic-size or finite-frame differences.
  Twenty-nine definitions instead trigger native generic fallback and are excluded from
  intended-layout matching, not counted as parity passes.
- Forty paired controls compare bounds with otherwise identical unbounded definitions:
  width under linear/space/cycle placement, and height/diameter under those algorithms plus
  composite placement. All native pairs have identical path rectangles; all 40 direct pairs
  now do too (previously five). This is bounds-invariance evidence, not full placement parity
  for the flow controls. Native widths without explicit allocations still expose separate
  intrinsic-size and rounding differences.
- Eighteen size-bound/rule controls and all 18 non-fallback selector-priority controls pass
  visible coloured bounds within 0.5pt at 4K. Representative native/direct images were
  inspected, including the native generic fallback, narrow self sizing and text-driven
  growth. Input hashes match native export receipts. Native fallback differs visibly from
  the currently evaluated custom layout; open/export success alone is not a fidelity pass.
- The older 186-case matrix improves from 152 to 155 matches, retaining 19 failures and
  12 native-fallback exclusions. All 176 signed-edge controls and all 73 overflow controls
  still match. The other prior matrices remain at 207/216 and 93/94, with their nine and one
  native-fallback cases separated. All 154 earlier 2px/pt PNGs remain pixel-identical to the
  PPTX.28 executable, with unchanged and native-matching font-size sets.
- Four new unit tests cover 48 selector/operator, algorithm/bound and rule-binding variants.
  Full `cargo xtask ci` passes all eight stages and 2,455 tests including doctests; all 21
  diagram import/round-trip tests pass. Production paths remain bounded safe Rust, without
  an external document converter.

Native duplicate named definitions can trigger generic fallback even when both numeric
values agree. Early unnamed declarations followed by named assignments have additional
fallback behavior. These policies, descendant defaults, full rule/search and cyclic fonts
remain incomplete. Geographic provider context, simplification/quantization, and rich-label
overflow/collision remain open; complete LibreOffice replacement is not established.


## Intrinsic flow sizing and finite linear fit (PPTX.30)

Unconstrained children of linear and cycle layouts now begin with intrinsic 100 mm
width/height instead of inheriting an equal share of a later parent allocation. Explicit
child dimensions still participate in constraint resolution before placement. This policy
is not generalized to hierarchy/snake layouts, whose packing remains different.

For root linear layouts, a bounded normalized ten-trial search reproduces the observed
finite scale across four directions, several frame sizes and one to three children. It
models measured outputs; it is not a claim about PowerPoint's implementation. Cross-axis
fitting stays independent of primary-axis scaling, and font sizes are not geometrically
scaled. Bare self `sp` and `sibSp` properties do not insert gaps: a spacer is a participating
child allocation. Repacking already painted composite bounds does not apply another root
allocation search.

Validation:

- A new 111-case original native matrix covers missing/self sizes, four layout algorithms,
  four flow directions, parent sizes, nested flow and extreme dimensions. Strict matches
  improve from 12 to 47 at 0.025pt model-inspection tolerance, with no earlier match lost.
  Breakdown: default-size controls 11/48, direction/fit controls 22/38, nested controls
  9/20, and five extreme linear controls 5/5. The other 64 remain failures, including
  large hierarchy/nested differences; all 111 native/direct font-size sets agree.
- Twelve independent spacing controls vary one/two/three children, `sp`/`sibSp`, and
  zero/nonzero values. Native geometry is invariant within each group, and all 12 direct
  geometries match. Two new tests cover 28 intrinsic-allocation and direction/frame
  variants. Existing spacing and geometry assertions were updated from exact-fill
  assumptions using independent native controls. The abstract conditional-mapping unit
  checks interpreter associations/fractions only: its corresponding native probe is blank
  and is explicitly not native-layout parity evidence.
- All 38 direction/fit controls, five extreme linear controls and one text-growth control
  pass visible coloured bounds within 0.5pt at 4K. This is weaker than the strict geometry
  test and does not certify whole-slide equality. The text-growth control retains about
  0.22pt width error from combined rule-endpoint/flow fitting. A separate nested-composite
  control shows approximately 135pt direct versus 121.38pt native card widths and remains
  an explicit failure. Representative native/direct raster pairs were visually inspected.
- The prior 129-case matrix improves from 46 to 64 matches, retaining 36 failures and
  29 native-fallback exclusions. Earlier matrices remain at 176/176, 207/216 (nine native
  exclusions), 93/94 (one exclusion), 155/186 (19 failures and 12 exclusions), and 73/73
  overflow geometries. None of the previously matched cases in these matrices regresses.
- Of 154 prior 2px/pt direct PNGs, 151 are pixel-identical to the preceding executable.
  Three linear-rule controls have subpixel changes; their measured coloured bounds and
  font sizes are unchanged. All 154 font-size sets remain equal to native. The three
  before/after/native raster comparisons were inspected; this is not a pixel-equality
  claim for those cases.
- Full `cargo xtask ci` passes all eight stages and 2,457 tests including doctests; the
  21 diagram import/round-trip tests pass. Input hashes bind native receipts to original
  synthetic sources. One malformed experimental namespace triggered a repair dialog;
  it was rejected, not repaired or counted. Corrected controls were exported separately.

Remaining flow gaps include nested linear cross-axis extent, repeated parent/child
allocation, cycle radius/scale precision, hierarchy/snake fitting, and rule-endpoint
quantization. Native duplicate-definition fallback, descendant selectors, full text-driven
search, cyclic fonts, geographic provider context/boundary simplification and rich-label
overflow/collision remain partial. No complete LibreOffice replacement is established.


## Nested flow capacity and parent-fit reflow (PPTX.31)

A nested linear layout's allocated rectangle is not its cross-axis capacity. Linear and
space containers can expose space from an ancestor; a composite establishes the next
capacity boundary. Conflating allocation with capacity prematurely shrank intrinsic child
sizes, with large errors in both orientations. Placement now carries the capacity separately.
Nested overflow triggers finite primary-axis fitting; roundoff at an exactly full allocation
does not spuriously trigger it.

A composite that fits its occupied content changes the allocations of its children. A linear
child must lay out again in the fitted allocation, rather than merely scale its previously
painted rectangles. Reflow uses the retained constraint solution and restores permitted
intrinsic cross sizes before the group is centred. It shares the existing recursion/work
budgets and verifies that the replacement has the same node count. Rigid siblings still
shrink; a reflowed child can extend slightly beyond the final frame, as the native controls do.
This is not an implementation of every native repeated-fit/rounding policy.

Validation:

- A new 132-case original native matrix varies horizontal/vertical direction, one/two flow
  children, primary and cross sizes, outer capacities and intermediate composite/space/linear
  containers. Strict geometry matches improve from 39 to 83 at 0.025pt tolerance, without
  losing any previous match. Maximum measured error drops from 311.8086pt to 0.2408pt.
  Forty-nine finite-reflow precision differences remain explicit failures; this does not
  certify the older hierarchy/cycle failures. All 132 native/direct font-size sets agree.
- All three original packages open/export natively and validate with zero Open XML SDK
  errors. Measurements bind to source hashes. The authoritative controls use a larger
  square canvas so both orientations and the largest frames are fully visible. Earlier
  narrow-canvas exports clip vertical shapes and are not counted as intended-layout results.
- Direct 3840-by-3840 renders pass visible coloured bounds within 0.5pt for 131 of the
  132 original controls. One narrow text-bearing child has a 0.75pt exposed-fill difference;
  its previous and current direct bounds are identical, and its strict geometry matches.
  An independently exported text-free control has identical native path geometry and passes
  the visible-bound check, isolating text occlusion/rasterization from allocation. The
  original failure remains recorded. Representative raster pairs and the enlarged failing
  before/after/native detail were visually inspected.
- All 154 earlier direct PNGs are pixel-identical to the PPTX.30 executable, with unchanged
  and native-matching font sizes. Earlier matrices remain at 47/111, 64/129 (29 native
  exclusions), 155/186 (19 failures, 12 exclusions), 176/176, 207/216 (nine exclusions),
  93/94 (one exclusion) and 73/73 overflow geometries. No previously matched case is lost.
- Three new unit tests cover 40 capacity-boundary, orientation, parent-fit reflow and exact-fit
  variants. Full `cargo xtask ci` passes all eight stages and 2,460 tests including doctests;
  all 21 diagram import/round-trip tests pass. No user content, native artifacts or fonts
  are included in the source change.

The remaining nested differences involve accumulation of finite scaling across repeated
fits, especially multiple linear ancestors. They are distinct from the corrected capacity
and parent-fit behavior. Full text-driven search/fallback, cyclic/hierarchy placement,
provider-specific map context/boundaries and rich-label overflow/collision remain incomplete.
Complete LibreOffice replacement is not established.


## Non-shrinking circle radius fitting (PPTX.32)

A full-circle layout with one orbiting node or two opposite orbiting nodes does not occupy
an entire disc. Limiting its radius to half the smaller frame dimension can compress node
separation by tens of points. Such layouts now derive an upper radius from all pairwise
occupied-width/height constraints, including a centre node when present. Non-shrinking
full-circle controls use a finite radius fit; non-degenerate circles refine the feasible
radius bracket once more. Minimum non-overlap separation remains enforced. Every radius
trial charges the shared work budget, including the earlier partial-arc fit.

The change is deliberately limited to non-shrinking full circles and the span-independent
single orbiting node. A broader coupled-size/radius candidate improved the first matrix
but regressed three earlier matches and failed independent narrow-arc controls by up to
167.89pt. That candidate was rejected. Partial arcs, coupled shrinking, and the remaining
sub-point radius differences are not solved by the accepted change.

Validation:

- Four contributor-original packages, 256 slides, validate with zero Open XML SDK errors
  and open/export natively. Hash-bound measurements compare actual coloured native paths
  with direct imported model rectangles at a strict 0.025pt threshold. There are no native
  fallback exclusions in these four matrices.
- The initial 136 controls cover node counts, square/mixed dimensions, frame sizes, start
  angles, half/full spans and centre mapping. Matches improve from 9 to 46; maximum error
  drops from 49.4861pt to 0.4059pt. Ninety failures remain explicit.
- Sixty independent full-circle controls vary 2/3/4/6/8 nodes, fractional mixed rectangles,
  fractional frames, negative/positive start angles and centre mapping. Matches improve
  from 1 to 34; maximum error drops from 123.3133pt to 0.2201pt. Twenty-six remaining
  differences, chiefly sparse-orbit precision and shrinking, are not counted as matches.
- Sixty independent partial-arc controls include narrow, reversed and 270-degree spans.
  Matches improve from 1 to 3 through span-independent single-node behavior. Fifty-seven
  failures remain, including a 117.524pt error. No original strict match is lost across
  any of the four matrices. All 256 direct/native font-size sets agree.
- Direct 3840-by-2160 renders pass visible coloured bounds within 0.5pt for all 108
  initial fitting controls, all 28 mixed controls and all 60 full-circle holdouts. The
  partial-arc holdout passes 48/60 and retains 12 visible failures. This weaker raster
  bound does not replace the strict geometry threshold above or certify text layout.
  Representative before/after/native pairs, including an unchanged partial-arc failure,
  were visually inspected.
- All 154 earlier direct PNGs are pixel-identical to the preceding PPTX.31 executable,
  with unchanged, native-matching font sizes. Earlier geometry matrices improve from
  47/111 to 58/111 and from 64/129 to 82/129 (29 native exclusions); they retain their
  other failures. Matrices at 83/132, 155/186 (12 exclusions), 176/176, 207/216 (nine
  exclusions) and 93/94 (one exclusion) remain unchanged. Matched-case sets do not regress.
- Three new unit tests cover independent native mixed-node observations under translation,
  span invariance for a single orbiting node, and radius-trial budget exhaustion. Full
  `cargo xtask ci` passes all eight stages and 2,463 tests including doctests; all 21 diagram
  import/round-trip tests pass. The source change contains no user content or native assets.

Full text-driven SmartArt search/fallback, native nested-fit precision, hierarchy/snake
placement, partial arcs and coupled node shrinking remain incomplete. Geographic provider
context, boundary simplification and rich-label overflow/collision are unchanged gaps.
Complete LibreOffice replacement is not established.


## Coupled cycle fitting and mapped-text refitting (PPTX.33)

Cycle placement now preserves the state of successful size/radius trials. A failed size
trial rolls back the radius; an accepted trial retains its fitted radius as the next
upper bound. Recomputing the final radius from the original cap gives a different result,
especially for narrow arcs and oversized nodes. The bounded search uses ten radius trials
and ten size trials after at most 64 bracket probes. Exactly tangent rectangles trigger
size fitting. All pair scans, text scans and radius trials share the work budget.

The initial cap measures the continuous circular arc, including cardinal extrema between
nodes. Full circles include the closing interval; partial arcs do not. Fewer than three
intervals use a frame-perimeter cap. Reversed paths keep their authored winding while
capacity uses the forward magnitude. Fraction-first angle arithmetic avoids unnecessary
intermediate overflow; an empty cycle with a centre mapping cannot underflow its count.

A successful unscaled layout is fitted again only when its mapped text algorithms contain
substantive run content. Unconnected data, document labels and non-text composite mappings
do not trigger this pass. Breakable spaces, formatting characters and standalone nonspacing
marks differ from nonbreaking spaces and literal run tabs/newlines. An empty paragraph
with an `a:br` is distinct from a newline inside an `a:t`. The predicate uses Unicode
categories, not slide indices or language-specific labels. The mapping walk is bounded and
includes nested text algorithms and fields. This is a tested cycle-refit policy, not a
replacement for font shaping or a claim of complete text-driven rule-search equivalence.

Validation uses original synthetic packages and hash-bound native observations:

- The preceding 256 controls improve from 83 to 256 strict geometry matches. The 136
  initial controls, 60 full-circle holdouts and 60 partial/reversed holdouts all pass.
- Four new matrices add 250 controls: 90 continuous-arc caps, 72 data-identity/mapping
  variants, 72 independently randomized mixed-size/frame/angle controls and 16 independent
  reversed asymmetric paths. Matches improve from 41 to 250. Combined, these 506 controls
  improve from 124 to 506 at 0.025pt tolerance; maximum error falls from 167.506pt to 0.01309pt.
  No intended native paths are excluded as fallbacks, and all 506 font-size sets agree.
- Seventy-two independently exported text-free versions, 18 mapped-text controls and 36
  whitespace controls add 126 strict comparisons, improving from 15 to 126. The maximum
  final error across all 632 controls is 0.01349pt. These eleven packages validate with
  zero Open XML SDK errors. Unicode line/paragraph separators required correcting the
  private JSON-log reader to split on LF only; no malformed records were silently skipped.
- Removing text changes native geometry in 17 of the 72 text-free versions. Those images
  cannot substitute for the original text-bearing acceptance checks. Additional multilingual,
  hidden-shape and nested-mapping probes informed the predicate; they are not counted as
  blanket geometry passes for unsupported mappings or fallback layouts.
- At 3840 by 2160, the original 506 controls pass visible coloured bounds within 0.5pt
  in 484 cases. Eighteen visible failures and four unmeasurable cases remain explicit.
  Four failures involve tiny identity controls; the randomized set has fourteen visible
  failures plus four cases without enough exposed fill pixels. All original 256 controls,
  all 90 cap controls and all 16 reversed controls pass this weaker raster check.
  Font clipping and occlusion are not certified by matching model rectangles. Representative
  before/after/native pairs were visually inspected. The final mapped-text change preserves
  all 506 complete imported inspection models, so these raster observations remain applicable.
- All 154 earlier direct PNGs remain pixel-identical to the preceding executable, with
  unchanged, native-matching font-size sets. The earlier 111-case matrix improves from
  58 to 73 strict matches. Other matrices retain 83/132, 82/129 (29 native exclusions),
  155/186 (12 exclusions), 176/176, 207/216 (nine exclusions), 93/94 (one exclusion), and
  73/73 overflow matches. No previously matched case is lost.
- LibreOffice's black-box export of the 72 randomized uncached definitions is blank on
  every page, with no drawing, image or text objects. It is recorded as an unsupported
  comparator result, not a fidelity pass. The direct Rust measurements do not call it.
- Seven new tests cover native full/partial/reversed controls, exact tangency, empty centre
  mapping, huge angles, mapped Unicode run content, nested field mappings and work limits.
  Full `cargo xtask ci` passes all eight stages and 2,470 tests including doctests; all 21
  diagram import/round-trip tests pass. Only source, dependencies, tests and documentation
  are published; no user or native assets are included.

The previously reported partial-arc and coupled-shrinking failures are resolved for these
controls. Rotation along the path, arbitrary nested-cycle/text-rule interactions, full
native search/fallback, duplicate definitions, descendant defaults, hierarchy/snake fitting
and finite nested-flow precision remain open. Geographic provider context, boundary
simplification and rich-label overflow/collision are unchanged gaps. This does not establish
arbitrary-deck pixel equivalence or complete LibreOffice replacement.


## Measured cycle refitting and path rotation (PPTX.34)

A cycle's `rotPath="alongPath"` now rotates leaf shapes by their actual orbit angle after
fitting the authored shape rotation. It does not shrink the shape a second time, rotate
a centre-mapped leaf, or rotate a child container's independently laid-out subtree.
Text orientation uses the final combined shape angle. The public OOXML
[rotation-path enumeration](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.diagrams.rotationpathvalues?view=openxml-3.0.1)
and black-box original controls establish this bounded behavior.

The PPTX.33 presence predicate was sufficient for its original corpus, but new controls
show that text presence alone does not decide a second radius fit. Short labels at a small
font can fit in one pass; longer labels at the same font, or the same label with larger
margins, can overflow. The measured importer now places once, measures the displayed text,
and selectively refits cycles whose mapped text overflows. The same resolved constraints
are retained. Each rule trial recomputes this decision, and the existing text-measurement
cache reuses unchanged translations. Unmeasured layout retains the conservative mapped-text
predicate. This is not an exhaustive replacement for native text-driven search.

Cycle packing uses the occupied extents of supported composite, space and linear children,
rather than carrying unused allocation margins around the orbit. A painted linear container
fits its child bounds before its parent packs it. Authored container rotation remains
independent of the cycle's leaf-path rotation. All tree scans, geometry trials and measuring
passes share bounded work; invalid measurement counts, nonfinite values and negative
scores are rejected. No page numbers or label strings select production behavior.

Validation uses five original, hash-bound packages, each with zero Open XML SDK errors:

- Forty leaf/centre path-rotation controls improve from eight to 40 strict matches. They
  vary counts, frames, positive/negative spans, start angles and empty/present labels.
- Forty-eight combined rotation/text/container controls improve from six to 48 matches;
  16 independent nested controls improve from zero to 12, with four native-blank cases
  explicitly excluded. A separate 36-case algorithm probe improves from one to four
  measurable matches, with 32 native-blank cases excluded. Blank native output does not
  establish supported fallback or an equality pass.
- Forty font/margin/label controls improve from 32 to 40 matches. Together, the 180 original
  pages contain 144 measurable intended geometries: 47 become 144 matches within 0.025pt,
  with no previously matched case lost. Maximum error falls from 26.1270pt to 0.01220pt.
- Actual 3840-by-2160 painted raster bounds are checked independently of imported shape
  names. This matters when the same name identifies both a painted shape and its transparent
  text overlay. Direct PDF output uses raster graphics plus searchable text, so an empty
  PDF path extraction is not treated as either missing graphics or a passing comparison.
  Representative direct/native pages, including retained text-visibility differences, were
  visually inspected. All 144 measurable controls pass visible coloured bounds within 0.5pt; the same
  36 native-blank controls remain unmeasurable and are not counted as passes.
- All prior 632 cycle geometries still pass. All 506 original complete inspection models
  remain unchanged. All 154 earlier PNGs are pixel-identical to PPTX.33, and their font sets
  remain unchanged and native-matching. Earlier matrices retain 83/132, 73/111, 82/129
  (29 exclusions), 155/186 (12 exclusions), 176/176, 207/216 (nine exclusions), 93/94
  (one exclusion), and 73/73 overflow matches, with no lost matched case.
- Three new unit tests and one editable round-trip test cover independent native positions,
  centre/leaf/container rotation, text-measurement validation, occupied frames and actual
  short/long imported labels. All 163 layout/library tests and 22 diagram preservation
  tests pass. Full `cargo xtask ci` passes all eight stages and 2,474 tests including doctests.

Geometry agreement does not certify the nested text: some native nested controls are blank
where direct rendering retains a label. The font/margin corpus retains long-label clipping
and line-layout differences despite matching shape bounds and font-size sets. The previous
18 visible raster failures and four unmeasurable cases in the original 506-control corpus
remain explicit; these are not resolved by unchanged model geometry. Full search/fallback,
nested text ownership, duplicate definitions, descendant defaults, hierarchy/snake and finite
nested-flow precision, geographic provider context/boundary simplification, and rich-label
overflow/collision remain open. This does not establish complete LibreOffice replacement.


## Independent typography and mapped visibility (PPTX.35)

A layout node's own primary font, secondary font and margins are local to that node.
They are not defaults for descendant text nodes. Child/descendant constraints, references
and a child's own constraints still establish the supported explicit relationships.
This distinction survives placement: an absent secondary font keeps its default meaning
instead of silently taking the parent's secondary font. Default secondary display sizes
round to whole points, as explicit displayed fonts already do.

A visible enclosing shape can borrow a matching visible text shape's frame and typography.
It does not borrow text from a missing/`none` shape or a `hideGeom` text-only shape. These
helpers still occupy layout space and retain their underlying data; the text-only shape
paints its own text. A hidden text helper's overflow does not trigger the cycle's extra
radius pass. Ordinary visible and text-only labels can still trigger that pass. The change
corrects the overly broad mapped-helper assumption recorded in PPTX.22 and the nested
visibility differences exposed by PPTX.34.

Original black-box controls separate scope, visibility, frame size and rule behavior:

- A 72-page package varies composite/space/linear parents, small/large text frames,
  parent/child/self/default font scope, and visible/absent/none shapes. Font-set agreement
  improves from 18 to 72; concatenated text agreement improves from 21 to 72. All 72 also
  match extracted line segmentation. Twenty-four pages have visible native labels and
  48 intentionally suppress them; empty outputs are not confused with displayed-font tests.
  Maximum matched glyph-origin differences are 0.280pt horizontally and 0.984pt vertically,
  so this is not a pixel-equivalent baseline claim. Strict geometry improves from 57 to 72.
- A separate 42-page package varies primary/secondary scope, explicit references and
  text-only children. Font sets improve from 12 to 30 matches; strict geometry improves
  from 24 to 30. Twelve parent-font-reference cases cause PowerPoint to use a different
  fallback layout; direct rendering still evaluates the authored layout. Those are retained
  failures, not removed from the denominator. Representative fallback output was inspected.
  Bullet text is present in both raster outputs, but direct searchable PDF text omits the
  separately painted bullet glyph; concatenated PDF strings do not certify its appearance.
- Together the two packages improve from 81 to 102 strict geometry matches among 114 pages,
  with no previously matched case lost. The same 102 pages pass independent 3840-by-2160
  visible coloured bounds within 0.5pt; the 12 fallback cases fail the raster comparison too.
  Both input packages have zero Open XML SDK errors and successful hash-bound native exports.
  Representative direct/native pages cover suppression, wrapping, text-only children, bullets
  and the retained fallback failure. No external converter is invoked by direct rendering.
- The preceding 48 combined path/text controls now all match font/text presence, and 12 of
  16 nested controls match where previously none did. Four native-blank text-container
  controls remain different. All 144 measurable geometries in the preceding 180-page matrix
  remain strict matches; its 36 native-blank exclusions remain explicit. Long-label clipping
  and baseline differences in the earlier 40-case font/margin matrix are not claimed fixed.
- All 632 preceding cycle geometries remain strict matches, and the original 506 complete
  inspection models are unchanged. All 154 older PNGs remain pixel-identical to PPTX.34,
  with unchanged, native-matching font-size sets. Earlier geometry matrices retain 83/132,
  73/111, 82/129 (29 exclusions), 155/186 (12 exclusions), 176/176, 207/216 (nine exclusions),
  93/94 (one exclusion), and 73/73 overflow matches without losing a matched case.
- Two new unit tests cover scope/visibility variants and the hidden-versus-displayed refit
  decision. A four-variant integration test preserves visibility, whole-point secondary
  fonts, geometry and rendered pixels through an editable round trip. All 165 library tests
  and 23 diagram preservation tests pass. Full `cargo xtask ci` passes all eight stages
  and 2,477 tests including doctests.

Full native fallback/search, cyclic font references, arbitrary nested mappings, duplicate
names, descendant defaults, baseline/overflow precision, hierarchy/snake fitting and nested
flow precision remain incomplete. Geographic provider context, boundary simplification and
rich-label overflow/collision remain open. No global parity score or complete LibreOffice
replacement is claimed. Published changes contain source, tests and documentation only.


## PPTX.36 — upstream integration and independent fallback controls

The upstream text-inset change conflicts with the previously observed native midpoint
behavior. A new contributor-original deck has sixteen controls: equal and unequal
positive insets, extreme overrun, crossed signed insets and signed insets that still fit,
each with wrapping on and off. The package passes Open XML SDK validation and opens
and exports in PowerPoint for Mac 16.113.3 without a repair prompt being dismissed.
The input hash is unchanged. Native glyph centres stay within 0.015pt of the crossed
interval midpoint. Proportional scaling differs by up to 52.167pt in these controls.
The merge retains midpoint collapse, the document-scoped embedded-font path, and the
new upstream monospace fallback API. Tests cover the signed cases and an extreme
asymmetric midpoint; upstream signed-inset and line-breaking coverage is retained.

Additional SmartArt evidence separates previously conflated failure causes:

- Thirty-six ownership controls still trigger native fallback whether the parent maps
  text or not, and whether its shape is visible, hidden or absent. Duplicate mapped text
  alone is not an established explanation for the twelve prior fallback differences.
- Forty-eight reference-option controls vary names, explicit/default selectors, XML
  order, equality operators, source defaults and factors. Six constant/local controls
  retain the intended layout; the other 42 fall back.
- Forty-eight source/target controls vary text/space/composite/linear source algorithms,
  parent versus local declarations, and parent/sibling/self targets. All eight sibling
  references declared at their common parent match font sets. The remaining 40 do not.
  This supports investigating property scope and dependency validity; it does not justify
  a blanket rejection of font references or claim a complete native-fallback algorithm.
- Forty additional native fallback controls cover one through eight nodes and five
  canvas aspect ratios. They are diagnostic inputs for grid fitting, not direct-render
  parity passes. Generic fallback selection, geometry and typography remain unfinished.

No public native render, document, font or private corpus asset is added. Full native
SmartArt search/fallback, remaining map provider/viewport and rich-label overflow/collision
work stays open. This integration does not raise parity scores or certify complete
LibreOffice replacement.

Integration validation: all eight `cargo xtask ci` stages pass, covering 2,520 tests
including doctests. All sixteen new controls preserve text and line segmentation,
with direct/native horizontal glyph-centre differences below 0.019pt. All 154 earlier
PNG exports remain pixel-identical to PPTX.35 and retain native-matching font sets.
All 632 earlier cycle geometries still pass the strict comparison (maximum 0.014pt),
and the original 506 complete inspection models are unchanged. These regression
results do not erase the earlier raster failures or unmeasurable cases.


## PPTX.37 — invalid font graphs and measured recovery

Uncached layouts now distinguish an invalid font-dependency graph from a failed numeric
search. Declarations resolve at their owning layout node; a font computed inside a child
is not automatically available as a sibling declaration at its parent. Duplicate initial
font declarations, missing references, incompatible reference axes and dependency cycles
trigger recovery for the observed cases. Local primary defaults and valid secondary-font
references remain available. Selector overlap detects self-dependency even when the two
selector strings differ. Malformed input and exhausted work budgets still return errors.

Recovery preserves the original data and layout parts. It creates editable rectangular
cards, maps each root node and its ordered descendants, and supplies native-style bullet
paragraphs. A bounded grid search chooses the largest card capacity, preferring more
columns for tied candidates and centring incomplete rows. Normalized integer ratios avoid
misclassifying exact fractional fits after unit conversion. A shared measured font search
uses whole-point sizes between 5 and 65, with rounded secondary fonts and proportional
margins. Recovery bullet indents update with every trial rather than retaining the initial
font's indent; explicit paragraph indentation remains authoritative. Bounded work, finite
frames and malformed measurement checks apply to this path too.

Independent contributor-original evidence (PowerPoint 16.113.3, private local outputs):

| Matrix | Cases | Native font matches before → after | Recovered geometry matches |
|---|---:|---:|---:|
| Reference dependency graphs | 56 | 28 → 56 | 28/28 |
| Defaults, missing sources and declaration ordering | 24 | 4 → 23 | 19/19 |
| Selector overlap and duplicates | 8 | 2 → 8 | 6/6 |
| Short/long/mixed text and descendant bullets | 26 | 3 → 21 | 26/26 |
| Independent node counts and aspect ratios | 32 | 0 → 26 | 32/32 |
| Bullet scaling controls | 8 | 0 → 8 | 8/8 |
| Earlier reference target controls | 48 | 8 → 48 | 40/40 |
| Earlier reference option controls | 48 | 6 → 48 | 42/42 |
| Earlier fallback grid controls | 40 | 0 → 34 | 40/40 |
| Earlier parent mapping/visibility controls | 36 | 0 → 36 | 36/36 |
| **Total** | **326** | **51 → 308** | **277/277** |

All 277 recovered geometries match within 0.025pt; the maximum measured difference is
0.01320pt. No previously matching font set regresses. The 49 unrecovered controls are not
counted as newly passing recovery geometries. Separate 3840-pixel-wide raster checks for
the 40 grid, 32 independent-count, 26 mixed-text and eight bullet controls all match visible
filled bounds within 0.5pt. This is shape-bound evidence, not pixel-identical typography.
Representative direct/native renderings were visually inspected. Additional original
bullet-boundary controls validate discrete indentation and saved DrawingML text defaults;
these observations are not added to the 326-case pass count. Inputs validate with the
Open XML SDK, native exports retain source hashes, and no native-produced asset is published.

All eight `cargo xtask ci` stages pass, covering 2,524 tests including doctests (168 library
and 24 diagram preservation tests). New tests cover declaration scopes/cycles, translated
native grid positions, work/measurement rejection and editable export/reimport with retained
data identities, layout XML and rendered pixels. All 632 prior cycle geometries remain
passing and 506 complete models remain unchanged. All 154 earlier PNGs remain pixel-identical
with native-matching font sets; these unaffected cases were checked before the final
recovery-only indentation adjustment.

Eighteen font-set differences remain: one missing-source equality with a zero factor,
five mixed/long-text controls, six independent-count controls and six earlier grid controls.
The latter 17 differ by 1–2pt. Matching shape bounds do not hide them. Identical installed
and embedded open font bytes do not guarantee identical native text measurement; glyph
advance and baseline differences still require investigation. Direct PDF text extraction
also omits separately painted bullet glyphs, so extracted strings are not used to claim
visual bullet absence. Duplicate geometry constraints, arbitrary mapping/hierarchy/snake
recovery and complete text-rule search remain unproven. Earlier raster failures remain
open. Geographic provider/viewport/boundary behavior and rich-label overflow/collision are
unchanged by this arc. Complete LibreOffice replacement is not established.


## PPTX.38 — native static-font metrics and wrap boundaries

Static TrueType alphabetic text now follows the independently observed legacy-kerning
path. DrawingML shaping disables GPOS pair adjustments for these faces and applies the
first eligible horizontal OpenType format-0 `kern` subtable, including the empty-first-table
case. Vertical, minimum-distance and cross-stream tables are not horizontal pair lists.
Parsing and iteration remain bounded; malformed data and overflow do not panic. An explicit
kerning-off feature still wins. General font API callers, RTL/complex scripts, AAT, CFF and
variable-font shaping retain their previous paths; those exclusions are not new parity claims.

Nominal static-font advances round to eighth-points before continuous pair adjustments and
character spacing. Rounding the already-kerned advance produces a different result. Line
breaking no longer permits an extra 0.01pt of overrun: independent native boxes distinguish
exactly fitting words from words 0.01pt too wide. This small tolerance otherwise moves an
entire character to the wrong line. Duplicate static/variable font candidates at the same
family, style, weight and slant prefer the variable candidate within one atomic load, without
reordering families or replacing previously loaded faces. This avoids letting filesystem order
choose a different shaping implementation.

Contributor-original black-box evidence uses PowerPoint 16.113.3 and matched font files:

- Seventy-two text/size/kerning controls, 28 static/variable/table controls, 48 GPOS pair/class/
  extension/flag controls, eight legacy-table-order controls and an additional 84 size controls.
  The static-font nominal/pair behavior is verified independently of SmartArt fitting. Variable
  fonts remain on the modern path; their metric differences are not claimed solved.
- Eighteen narrow text boxes validate both sides of two exact wrapping boundaries. Additional
  21 width and 41 height controls isolate a remaining SmartArt fit discrepancy: ordinary text
  wrapping and diagram font selection must not be treated as the same acceptance criterion.
- Sixteen renamed-font diagnostics are not counted as metric-parity passes because native
  variable weight selection differs. An initial export with font substitution was rejected;
  font identity was verified before accepting its replacement export.
- All new source packages are programmatic, SDK-valid and native-openable. Source hashes are
  checked; original fonts, input files and native-generated outputs are not published here.

The existing 326-case recovery matrix now matches 321 native font-size sets, up from 308;
all 13 newly matching cases are additions, with no previous match lost. All 277 recovered
geometries still match within 0.025pt (maximum 0.01320pt), and all 106 independent 3840px
filled-bound checks remain within 0.5pt. Five font differences remain: a zero-factor equality
with a missing source, three long-word fitting cases and one descendant-bullet height case.
An intermediate candidate matched 323 font sets but retained inaccurate glyph spacing and
wrap boundaries; that larger subtotal is not reported as the final result.

All 632 earlier cycle geometry checks still pass, with 506 full inspection models unchanged.
All 154 older native font-size sets and input hashes remain unchanged. Of those renders,
three are pixel-identical to the previous build and 151 change with the text-metric correction;
coloured shape bounds remain unchanged. Ignoring extraction-only whitespace, 151 cases have
comparable native text sequences: 58 improve their glyph-origin RMS error by more than 0.01pt,
none worsen by more than 0.01pt, and 93 are stable within that threshold. Three cases are not
measurable by this text comparison and are not counted as text-position passes. These are
position/geometry observations, not blanket pixel equivalence. Representative native/before/
after renderings, including a remaining long-word failure, were visually inspected.

All eight local `cargo xtask ci` gates pass (2,527 tests including doctests; 168 PPTX library,
24 diagram preservation and 105 text tests). New regression tests cover legacy subtable
order, truncated bytes and saturating arithmetic, duplicate-font selection, independently
measured glyph positions and exact wrap boundaries. Hosted CI is reported separately.

Complete text-driven diagram rule search, geometric duplicate/default recovery, hierarchy/
snake/nested fitting, older cycle raster failures, geographic provider/context/viewport and
boundary simplification, and rich-label vertical overflow/collision remain open. This arc
does not establish full PowerPoint parity or complete LibreOffice replacement.

## PPTX.39 — inactive zero font equalities

An explicitly authored `op="equ"` font constraint with an exact zero constant or
reference factor is inactive. It does not assign a one-point font, overwrite a prior
font, declare a source for another reference, or create a duplicate dependency. The
compiler excludes these constraints before graph analysis and rule evaluation, while
preserving the original source XML. Ordinary zero assignments, negative values and
small nonzero values retain their different behavior. Constraint collection remains
bounded before filtering; no additional input-controlled search or allocation is added.

Contributor-original native controls cover 34 primary-font cases, 34 secondary-font
cases and 36 scope/declaration/reference cases. All 104 font-size sets match PowerPoint.
The 70 primary/scope text sequences also agree. Secondary bullet text is omitted by
DeckCraft's searchable PDF text extraction even though it is visibly rendered; the
27 affected extraction comparisons are not counted as text-sequence passes. Native
and direct renderings of representative secondary-bullet and reference-chain cases
were visually checked. Programmatic source hashes match the native export receipts.

The prior 326-case recovery matrix improves from 321 to 322 matching font-size sets,
with no previously matching case lost. Its 277 recovered geometries remain within
0.025pt, with maximum error 0.01320pt. All 632 earlier cycle geometries remain within
0.025pt (maximum 0.01349pt); 506 complete inspection models are unchanged. Three
long-word cases and one descendant-bullet height case still have font-fit differences.
These results establish the zero-equality semantics, not general text-fitting parity.

Complete text-driven search, remaining diagram layouts, geographic context and
boundary simplification, and rich-label vertical overflow/collision remain open.
No native-generated documents, images, fonts or provider data are distributed with
this change. Full LibreOffice replacement is not established by this regression arc.

All eight local `cargo xtask ci` gates pass (2,528 tests including doctests; 169 PPTX
library and 24 diagram-preservation tests). The new regression covers primary/secondary
fonts, local/child scope, constants, references, duplicates and prior declarations.


## PPTX.40 — parallel layout, geographic and label improvements

These changes execute document-owned rules and data. They do not identify a diagram
by a vendor template name, bundle geographic assets, or certify arbitrary PPTX files.

Snake packing uses resolved child sizes and recomputes end-of-canvas breaks during
bounded fit trials. Balanced and fixed breakpoints, row spacing, traversal direction
and nested occupied bounds have independent native controls. Text refitting has its
own measurement mode: horizontal overflow and forced word splits count; total body
height counts once an authored paragraph spans multiple lines. An empty paragraph
is no longer incorrectly classified as a forced word split.

The new original snake matrix improves from 0 to 433 of 491 strict geometry matches.
The 58 retained failures comprise 56 uneven centered-row cases, one nested linear
case and one literal-linefeed height case. The earlier flow matrix improves from
73 to 85 of 111 with no prior match lost. All 632 prior cycle geometries still match,
and 506 previously retained inspection models are unchanged. Twenty sampled 4K
visible-bound checks have 17 matches and three known layout failures; this is not
whole-deck pixel equivalence or complete hierarchy/rule-search coverage.

Recovered SmartArt selects and lays out text using nominal glyph advances and
available widths rounded to 1/200pt, with typographic ascent/descent-based line
metrics. Ordinary slide text retains its existing metrics. The policies are explicit
in the text API; an internal recovered-frame marker regenerates on diagram import
and is not written as a nonstandard DrawingML attribute. Cross-size original-font
controls distinguish this policy from both continuous metrics and ordinary 1/8pt
painted advances. All 274 new native controls match font sets and whitespace-normalized
line sequences; the retained 326-case font matrix improves from 322 to 326 without
losing a previous match. Its geometry snapshots remain unchanged. PDF bullet
extraction differences are not counted as missing visible bullets or a text pass.

Maps now apply the initial Euclidean 0.375pt screen-space sampling stage. Ring
sampling resets independently; closing edges remain closed without forcing a rejected
last vertex back into tiny islands. Original Robinson/Mercator/Miller controls at two
sizes match all 576 retained-vertex results; 64 independent threshold/direction cases
also match. Eight 3840×2160 raster comparisons improve blue-region IoU, but viewport
and heavier simplification differences remain visible. The country example improves
95.67% to 95.82% IoU; this does not establish complete map fidelity. LibreOffice opens
three map controls but emits unsupported-chart placeholders, retained as failed visual
controls. An inspection of 27 distinct geographic payloads found the already handled
plain-XML, finalized raw-DEFLATE and SYNC_FLUSH envelopes; no additional encoding is
claimed missing without concrete evidence. Native geographic comparisons isolate
application processes because source hashes alone do not prevent stale provider-cache
reuse between documents.

Automatic unrotated literal column labels now distinguish ellipsis, clip and explicit
overflow, retain native insets and oversized first lines, and cap the automatic box
at half the chart height. Truncation does not alter stored chart text; the ellipsis
inherits the first omitted run's style while the original line metrics remain intact.
The typographic line policy is opt-in and respects mixed fonts and authored spacing.
Visible line-count controls improve from 10 to 72 of 73; the remaining negative-column
case retains a plot-position difference. Both earlier long-label examples now show
five lines with ellipsis, and all ten retained label bounding-box RMS comparisons
improve. These are visible raster diagnostics, not extracted chart-text comparisons.
Exact marker width, plot positions, rotated/manual labels, other chart kinds and
collision/callout behavior remain separate gaps.

All evidence uses original generated inputs and private native observations. No
native-generated documents, provider data, fonts, screenshots or user decks are
included. Full replacement of LibreOffice remains unproven while these gaps remain.

The final integrated build passes all eight local `cargo xtask ci` stages (2,551
tests including doctests). Its 133 retained label-page renders and eight 4K map
images are identical to the independently validated branch outputs. These checks
verify integration; they do not turn the known native differences into passes.


## PPTX.41 — centered packing, paragraph identity and rotated labels

Same-direction centered snake rows carry two pending centering updates instead of
independently centering against a single maximum row. A wider row absorbs pending
offsets before their propagation; the final group centers its actual occupied bounds.
This uses constant additional state within the existing bounded layout budget. The
source matrix improves from 433 to 457 of 491 strict geometry matches before the
shared text change, with no lost matches. All 217 full-size same-direction controls
match, including a separate 24-case holdout. A separate 48-case size/fit set has 31
matches, retaining 17 failures; a fit-only behavior is not inferred from the passing
unscaled cases. Reverse-centered and nested-linear fit behavior remain open.

Literal LF inside DrawingML run text restarts paragraph formatting, unlike `a:br`
and vertical-tab soft breaks. The importer and exporter now preserve this distinction.
Layout temporarily expands those segments for spacing, first-line indents, bullets
and numbering, then maps line, glyph, decoration, caret and grapheme indices back to
the authored paragraph. Expansion is capped at the importer's paragraph budget; over-budget text retains
the ordinary line-break path and its source. The stored text and paragraph count
are not modified. A styled
literal separator contributes to its preceding line's metrics, including nonempty
lines; an ordinary end-paragraph mark retains its existing semantics. The shared
fix closes one more retained snake case, bringing the integrated matrix to 458/491.

A separate 72-case original LF/soft-break/paragraph matrix improves from 64 to 72
strict geometry matches. All 51 ordinary text controls retain their visible line
counts; relative raster line-pitch matches within 0.5pt improve from 36 to 50. An
existing empty soft-break case remains at 0.75pt. Twelve new mixed-separator, empty,
trailing, split-run, numbering and RTL controls retain eleven matches within 0.5pt;
the remaining mixed-size separator improves from 13.25pt to 1pt relative pitch error.
It remains a failure at the stated tolerance. Native PDF text extraction omits some
visible repeated glyphs, so these checks use rendered ink rather than treating text
extraction as visual truth. Exact mixed-size baseline precision remains open.

Maps add a second, regional complexity stage above 1,000 surviving nonclosing
vertices after initial sampling. Collapsed single-point rings do not consume the
budget; two-point lines do. Sampling retains the first edge, applies axis distances,
and reduces near-collinear runs with two deferred area checks. Reversal cusps and
closing edges survive. Scans and buffers remain bounded by the existing point count;
curved paths retain the prior fallback. No geographical dataset is included.

The compiled Rust reducer matches all 3,168 ring observations across eight new
projection/size conditions: 96 new shapes plus 300 budget rings per condition,
Robinson/Mercator/Miller/regional Albers at full and half sizes. An affine fit to
native initial vertices isolates reduction semantics; it does not prove viewport
identity. Sixteen end-to-end 4K blue-region comparisons have no IoU regressions;
the retained country example improves from 95.82% to 97.32%. New global controls
remain at 79.60–83.05%, with regional Albers at 96.70–98.18%. Seventeen earlier exact
distance/area boundary cases remain failed, separately listed rather than hidden by
an epsilon or input-specific rule. Four LibreOffice map exports still produce
unsupported-chart placeholders and remain visual failures.

Literal column-label layout now computes the maximum-area text budget before
rotation, rotates background and text together, and places the rotated outer bounds
with the observed fixed endpoint gap. Tested manual factor dimensions are fixed before
rotation: a missing or zero partner makes the label empty, while small positive boxes
retain fill and vertically clipped text. Explicit overflow and the literal body's own
anchor are retained. Eight independent anchor controls confirm that a literal
replacement does not inherit label/chart `txPr` anchoring. Unsupported angle/edge
combinations keep their prior fallback rather than borrowing an unverified formula.

In 133 retained label pages, visible line-band matches improve from 112 to 125 with
no prior match lost. In 242 new pages, 229 match; the remaining cases include unusual
raw angles, edge-mode dimensions and very thin clipping. Median red-ink bounding-box
RMS errors improve from 108.44 to 5.74pt in the primary rotation set, 261.63 to 4.98pt
in its independent rotation set, 106.99 to 5.78pt in manual controls, and 317.33 to
3.92pt in independent wrapping controls. Two individual bounding-box errors regress
slightly (3.767→3.913pt and 6.856→7.594pt); they remain recorded, even though line
counts match. Chart plot offsets, exact marker widths and broader chart kinds are
not declared solved by these improvements.

Only original programmatic tests, source and documentation are included. Native
observations, provider data and private decks remain outside the commit. This arc
improves supported conversion and does not establish complete LibreOffice replacement.

All eight final local `cargo xtask ci` stages pass (2,570 tests including doctests).
The final executable reproduces all 375 retained/new label renders, sixteen 4K map
images and 75 LF/text-page renders from the validated results. These are integration
checks, not additional claims of native pixel identity.


## PPTX.42 — rule steps, nested flow, map viewports and chart axes

Width rules keep searching when an emergency word break conceals overflow. Height
rules fit the resulting lines without requiring every word intact. Successful size-rule
endpoints use five bisections over the authored range, as observed across different
initial values, endpoints, fonts and rule orders. Rejected endpoints retain the bounded
interior search. Diagram rule measurement uses typographic line metrics. A later font
change can release space grown by an earlier rule; an earlier font change does not
retroactively release space grown afterward. An exactly fitting original extent is
retained rather than approximated from above.

All eighteen new composite controls and two independent twenty-four-case sets match
native geometry within 0.025pt. The latter cover different initial/extreme dimensions
and three open font families at four sizes. The eighteen paired linear-parent controls
retain small packing differences, up to 0.143pt; they are not counted as strict matches.
The primary thirty-six pages retain all native font-size sets, and matching extracted
PDF line sequences improve from thirty to thirty-six. Representative native/before/after
renderings were inspected, including the formerly narrow, forcibly broken long word.
All programmatic packages pass SDK validation and hash-bound native open/export.

A snake's linear children first expose their intrinsic cross-axis extent. After the
parent fit, each retains its fitted flow size, restores that cross extent, and refits
the flow at its current allocation. Repacking centers those restored dimensions without
shrinking them again. All 107 new controls match, including a separately generated
48-case holdout with one to three leaves and four directions. Sixteen visible-colour
checks match within one pixel (0.295pt). Two twenty-case multi-wrapper sets retain
16 and 15 matches; their insufficient-capacity failures remain explicit.

Integration retains 458/491 snake geometry matches with no lost match, 632/632 older
cycle matches and 506 unchanged inspection models. All 600 retained/new recovery
font-size sets match. The older 111-case flow matrix remains 85/111: known differences
include hierarchy defaults, root linear defaults and small nested-flow precision errors.
These are distinct from untested rule branches. Off-diagonal coupled search and broader
hierarchy/pyramid text-rule interactions remain coverage gaps, not invented failures.

Map fitting now uses the observed viewport expansion before mapping into the visible
plot, and Robinson uses the native chart's angular normalization. Thirty original
projection/viewport controls, including independent dimensions, preserve 11,878 of
11,880 retained-vertex sequences. Global coordinates differ by at most 0.000331pt;
an Albers control retains a 0.001124pt difference and two vertex-sequence failures.
The earlier seventeen exact-threshold failures are not reclassified as passes.

Cached ring orientation now determines nonzero fill and label containment consistently:
same-direction overlaps remain filled, opposite directions cancel, and islands inside
holes remain filled. Thirteen original native fill controls pass, including five
independent rotated/nonrectangular cases. Across the retained thirty-eight 4K map
comparisons, the combined viewport/fill changes improve every foreground intersection
over union, with no regression. Representative country and Albers scores improve from
97.32% to 99.94% and from 98.18% to 99.89%. These are mask comparisons, not whole-slide
pixel identity. A tall Miller case only improves from 84.03% to 84.06%; regional paint
order remains unresolved. Three new sparse-world controls expose both an incorrect
global extent and paint-order differences; they fail before and after this change.
LibreOffice still exports the new map controls without their geographic graphics,
so its ability to open them is not counted as visual parity.

Category label visibility is separate from deletion of the category axis. Rebuilding
a chart preserves both states, the authored label offset, and paragraph line spacing.
Category offset and explicit spacing adjust the automatic plot reserve; a fixed inner
plot remains fixed. Undecorated column charts with both axes' labels hidden retain
their observed point margin. The extra measurement pass has explicit category-count
and text-byte budgets. This does not replace the default visible-axis gutter model.

All 102 new axis pages and six forcibly rebuilt packages pass schema validation;
rebuilt chart parts retain the observed visibility, offset and spacing semantics.
Among 95 comparable before/after plot cases, 36 improve, 57 are unchanged and two
regress; mean four-edge RMS error falls from 18.045pt to 14.204pt. The two regressions
increase from 19.035pt to 19.400pt and from 20.502pt to 21.546pt: corrected relative
offsets expose the existing absolute-gutter error. Eleven double-hidden plot frames
match native pixel bounds, and four additional controls distinguish hiding text from
deleting the axis. All 375 retained data-label ink bounding boxes are unchanged;
373 masks are identical and two differ by three/one pixels where axis ink overlaps.
Default axis margins, long category wrapping, endpoint reserves and rich-label
ellipsis alignment remain open.

The integrated executable reproduces all forty final map images and all 477 chart
pages from the separately verified implementations. This checks integration consistency,
not native equivalence of the explicitly failing cases above. Final local `cargo xtask ci`
passes all eight stages and 2,586 tests including doctests; this is not a hosted-CI claim.

Only source, original programmatic tests and documentation are published. Native
observations, provider data, fonts and private decks remain outside the commit. These
improvements do not establish complete LibreOffice replacement.


## PPTX.43 — measured fitting, alternating lines, sparse maps and ellipses

A measured root linear layout retains its exact geometry when text fits. Forced
word breaks or overflow request the finite fitting pass. A nested linear layout
allocated by a composite keeps that allocation unless its geometry overflows;
it does not inherit the root-only text trigger. Twenty-four original root/nested
controls and twenty-four independent cross-font/reverse-flow controls match native
geometry within 0.025pt. The trigger set has twenty-four informative controls;
sixteen generator duplicates are explicitly excluded from that count.

The prior thirty-six size-rule controls improve from 18 to 32 strict matches. A
new forty-eight-case linear/text-rule set improves from 16 to 37. Eleven of sixteen
unequal-sibling/rule controls match. Cumulative finite fits and earlier heights
retained after later sibling growth remain unresolved; these results do not establish
complete coupled rule replay. The old 111-case flow matrix retains 85 matches.

Alternating centered snake lines now carry separate flow extent, physical child
boundary, delayed alignment and growth. Column flow retains the physical-width
boundary observed in native rendering. The same-direction path, ten fitting trials
and bounded work budget remain in place. The original 491-case matrix improves
from 458 to 489, without losing a prior pass. All 447 retained reverse controls
match, and an independent 48-case set improves from 18 to 42. That last set was
used to refine the general rule after an initial frozen model matched 41 cases;
it is not presented as entirely unseen validation. Three already-failing pages
have worse maximum or mean coordinate error even though the overall mean
coordinate error falls from 12.813pt to 0.992pt. A separate, post-freeze thirty-two
case set improves from 6 to 29 without losing a pass or worsening those metrics.
The two original and nine newer failing cases remain recorded. Fifteen inspected
4K raster comparisons contain nine matches and six known failures; geometry
matching is not a claim of whole-image pixel parity.

Sparse map caches now fit the actual painted geographic extent instead of forcing
a synthetic full-globe frame. Administrative bounds still supply projection and
dateline context, including fallback when selected geometry is absent. All thirty-eight
older 4K map images remain pixel-identical. Twelve new source-bound native controls
have a maximum absolute vertex error of 0.000135pt, without affine fitting. Seven
diagnostic/independent foreground masks have 99.9817–99.9974% intersection over union;
two missing-selected-geometry gray masks reach 99.9603%. Three overlapping-region
controls retain correct overall extent but wrong paint order: whole-painted-area
scores near 99.917% conceal selected-colour scores of only 30.62–40.46%. They remain
failures. The four corresponding LibreOffice exports omit geographic graphics;
opening the package is not visual parity.

Automatic column-label ellipses reserve width using the first vertically omitted
character's style. If that reservation removes more characters, the painted marker
takes the first removed character's style without re-fitting the reserved width.
Markers follow the paragraph's trailing side, including RTL paragraphs. Alignment
uses the reserved line, while the automatic background encloses the aligned retained
prefixes; the marker is excluded. Authored ellipsis text remains ordinary text,
and manually fixed backgrounds keep their authored dimensions.

The 102 new horizontal controls reduce mean background horizontal-edge RMS error
from 6.771pt to 0.125pt and text horizontal-edge RMS from 5.919pt to 0.150pt, with
no regression in either measure. A further nine independent rotation controls
separate six negative-quarter-turn improvements (3.666pt to 0.322pt) from three
unchanged, pre-existing 180-degree wrapping/overflow failures. All 111 new pages
pass schema validation and hash-bound native open/export. Among 375 old pages,
191 retain identical foreground/background masks; the other 184 reduce mean text
four-edge RMS error from 7.919pt to 4.516pt. Their 140 comparable backgrounds improve
from 6.860pt to 3.627pt without regression. Seventeen text bounding boxes worsen;
several previously compensated an incorrect plot center. Fixed-inner controls
isolate the corrected marker geometry, but the remaining small-angle/inset and
absolute-gutter differences are not dismissed or counted as passes. Previously
matching line-band counts remain 181/184. Native/before/after images, including
visible regressions and the 180-degree failures, were inspected.

The final integrated executable retains all 632 older cycle geometries, 506 unchanged
inspection models, all 600 recovery font-size sets and all 107 nested-flow controls.
It reproduces all fifty final map images and all 588 chart-page renders from the
verified implementations. The 102 retained axis pages include three long-label
pages intentionally changed by this ellipsis correction; their text RMS errors
improve from 2.560/5.715/7.550pt to 2.160/3.559/5.916pt, while the plot geometry
remains unchanged. The other ninety-nine are pixel-identical to the prior revision.
These are integration-consistency checks, not native equivalence for the failing
cases above. Local `cargo xtask ci` passes all eight stages and 2,592 tests including
doctests; hosted CI is reported separately.

Only source, original programmatic tests and documentation are published. Native
observations, provider data, fonts and private decks remain outside the commit.
This is a bounded fidelity improvement, not complete LibreOffice replacement.


## PPTX.44 — nested early exits, map paint order and axis font metrics

When nested linear children restore their intrinsic cross extents after snake
fitting, a line can exceed the available frame. The layout now retains visited
lines in working coordinates and centers unvisited children in their prior
allocations. An early return skips column-axis swapping and growth reflection;
a completed layout still performs both. This fixes one original case from
122.788pt to 0.0081pt maximum error and leaves the other 490 models unchanged,
raising the original matrix from 489/491 to 490/491. All sixteen new independent
controls and six growth-direction controls match. The retained nested matrices
remain matched. Ten selected 4K comparisons agree within one pixel in visible
bounds; that does not establish whole-image equality.

Same-direction fitting still has a stateful positioning gap. A separate frozen
model predicts all 199 measured scales, but fitting trials leave placement state
that is not yet reproduced. It is not part of the production change. Additional
unequal-sibling text-rule controls also reject a simple interleaved-rule ordering
fix: only four of eighteen match. Sixteen targeted invalidation controls show
that width growth and later font release can preserve earlier allocations. The
current production rule search is unchanged; coupled stage replay remains open.

Geographic rendering now orders materialized projected paths before screen-space
boundary reduction. Data regions follow point order, backgrounds follow lexical
entity identifiers, and batches above 128 regions exchange the first and middle
slots. Extent-only entries do not count; tiny paths that collapse during later
reduction do. This is an observed ordering rule, not a claim about the native
implementation's internal sorting algorithm. Projection and work limits remain
unchanged.

Twenty-nine complete native path sequences match the generic order, with three
additional small causal controls. Independent 181-, 157- and 641-region predictions
were recorded before native export. Both 641-region controls match the entire
order, with absolute vertex error below 0.000188pt and no fitted coordinate
transform. Two native-empty controls remain excluded, and an earlier duplicate-
geometry construction is retained as invalid evidence rather than counted.

Among 56 original-source 4K map comparisons, foreground-mask intersection over
union improves on 39 and is unchanged on 17, with none worsening. The older
38-case subset has 31 improvements and seven unchanged results. The heavy-world
Robinson score improves from 82.5267% to 99.7725%; the old three overlap controls
reach 99.9976%. Two independent 641-region foreground scores improve from
19.0064% to 99.9012% and from 98.3718% to 99.9799%. Their full-colour class agreement
reaches 99.8221% and 99.8150%. Exact RGB is not solved: colour rounding and edge
pixels differ, two broad painted/gray masks worsen by roughly one pixel, and
one large control's exact full-page pixel fraction decreases despite the corrected
paint order. These measures are reported separately. LibreOffice still exports
unsupported-chart text with no geographic foreground for the new large controls.

Explicit category, value and secondary-value fonts now govern their own tick
density, measurement and placement instead of inheriting the chart default's
size for geometry. Supported undecorated column charts with horizontal axis text
use natural typographic line height, ascent and measured widest value-label advance
for axis reserves. Automatic frames retain 5pt outside padding and a 6pt minimum
reserve; authored outer plots apply reserves directly. Fixed inner plots remain
authoritative. Decorations, secondary series and unsupported axis directions keep
the existing path. Frames whose reserves overlap use the existing approximate
layout instead of collapsing the new plot to zero; exact cramped-frame native
constraints remain unresolved.

Eight independent font-role controls preserve geometry when only the chart default
changes. Independent mixed-size/mixed-font controls cover automatic and manual
outer frames; a third permitted font family confirms the same metric formula.
The first native export after installing that font had a first-page discrepancy;
a byte-identical source reopened under a new name matched, and both observations
are retained. Twelve stable rendered holdout pages have plot-edge RMS error at
most 0.354pt. Public tests contain only original generated cases and permitted fonts.

Across 107 measurable retained/new axis pages, mean plot-edge RMS falls from
14.776pt to 0.386pt, with 78 improvements, 29 unchanged comparisons and no
regressions. Three plots extending outside the slide cannot supply complete
visible edge measurements and remain excluded from this metric.

Among 375 old label pages, 23 foreground/background masks are unchanged. The
348 comparable changed text boxes improve mean four-edge RMS from 6.160pt to
3.231pt: 320 improve, eighteen regress and ten are effectively equal at the
0.1pt threshold. Their 283 comparable backgrounds improve from 5.936pt to
2.715pt, with 262 improvements and twelve regressions. No formerly correct
visible line-band count is lost. All seventeen text regressions recorded in
PPTX.43 recover to PPTX.42 or better; this does not erase the eighteen new ones.

Seventeen of those eighteen now have bar-geometry RMS at most 0.236pt: more
accurate chart positions expose remaining text placement, clipping and sampling
differences. The other case has an interior category-axis reserve error; its bar
geometry improves from 15.977pt to 11.578pt while label error worsens by 0.333pt.
The largest text regression is 2.355pt. Two genuine plot-collapse defects found
in the intermediate implementation were fixed using the generic fallback;
all three affected 40pt-high controls reproduce the earlier full-page pixels.
The regressions, first-load font anomaly and existing 180-degree failures were
visually inspected and remain failures. All 111 retained ellipsis pages are
pixel-identical to the preceding revision.

The final executable reproduces all 56 map images and 608 chart-page renders
from the verified implementations. It retains all 632 cycle geometries, 506
unchanged cycle inspection models and 600 recovery font-size sets. The wider
1,534-case geometry check has 1,478 strict matches: its only changed prior
pass count is the corrected nested case; new restored-flow controls also pass.
Known earlier failures remain in that denominator. These checks establish
integration consistency, not native equality for the explicitly failing pages.
Local `cargo xtask ci` passes all eight stages and 2,600 tests including
doctests. Hosted CI is tracked separately.

The observed geographic cache audit covers 132 entries, including 121 binary
and eleven clear entries; 27 unique binary payloads use supported raw-DEFLATE
stream forms. No observed unknown binary envelope remains. Seventeen retained
boundary-threshold sequence failures and two Albers sequence failures persist
when rerun through the final analytic viewport; they are numerical/reduction
gaps rather than the earlier fitted-transform artifact. A separate accepted
native control reveals a longitude-continuity discrepancy for accumulated
coordinates outside the usual range; it remains under investigation. No
provider geometry is added to the code or tests.

Only source, original programmatic tests and documentation are published. Private
native observations, provider geometry and all document/font assets stay outside
the commit. The remaining rule, small-frame, rotated-label and map-threshold gaps
mean this is not blanket PowerPoint parity or complete LibreOffice replacement.


## PPTX.45 — accumulated map coordinates and centered half turns

Geographic ring decoding now keeps accumulated longitudes rather than folding
coordinates before the renderer can use region extents. Rebuilding preserves
explicit coordinate deltas, including a deliberately long edge: shortest-path
unwrapping would change the authored geometry. Coordinate, point, ring and
exact-integer budgets remain bounded. World projection uses signed, region-relative
continuity; regional views still canonicalize equivalent longitudes.

The retained 56 map images are pixel-identical. Two historical chord failures
improve foreground IoU from 36.718% to 98.804% and from 40.893% to 99.863%; a third
historical control is unchanged. Fourteen independent controls have six improvements,
eight unchanged results and no regressions. Twenty-one previously out-of-domain
ring sequences now match native absolute coordinates. The seventeen threshold
sequences and two Albers sequence failures remain unchanged and unresolved.

Forced encoding/reopening preserves geographic rings, bounds, bindings, projection
and rendered pixels on fourteen controls. This is not a native full-rebuild pass:
PowerPoint still produces an empty geographic plot for a fully regenerated chart.
The pre-change implementation produces identical package parts in a paired canonical
control. Replacing only the polygon encoding in two original packages preserves
native vertices and whole-image pixels, isolating the new encoding from the existing
full-chart writer failure. Entity enum spelling is normalized on rebuilding; raw
model JSON identity is not claimed.

Centered automatic column labels at exactly +180 or -180 degrees now retain their
full unwrapped text for default, clip, ellipsis and overflow policies. Stored text
and authored policy remain unchanged. Wrapped bodies use capped-corner placement;
explicit no-wrap overflow centers the complete body before chart clipping, including
unrotated horizontal labels. Manual and non-centered half turns retain the previous
path and remain unproved.

Eighteen changed pages among 29 targeted controls improve text and background bounds;
eleven are pixel-identical. All visible text-band counts match. Among 375 retained
pages, 371 text/background masks are unchanged and four half-turn pages improve.
The two half-turn regressions introduced by the previous axis-margin change recover,
leaving sixteen of its eighteen text-box regressions. A separate wrapped-overflow
background mismatch comes from line-break selection, not an accepted background-only
sizing fix. Gridline appearance and other documented modes remain incomplete.

The final integrated executable reproduces all 73 map images and all 628 chart-page
renders from their frozen validated references. A separate source-integrity check
confirms that all 64 retained linear-rule inspection models are unchanged, excluding
the private staged-search experiments. Local full CI: all eight stages pass, with 2,605 tests including doctests. Hosted CI is
tracked separately.

The staged SmartArt rule-search experiments are not part of this revision. Independent
controls support local rule evaluation followed by persistent parent allocation, but
height endpoint and state-mutation differences remain. Only source, original code tests
and documentation are published; native observations and document assets remain private.
This is not blanket PowerPoint parity or complete LibreOffice replacement.


## PPTX.46 — staged linear rules and chart advance precision

### Independent linear text rules

A child's first flow-axis rule resolves before sibling allocations are packed. Later
height/width/font rules measure the resulting allocation rather than the original box.
That allocation pass does not run text fitting: fitting there and again at final placement
shrinks the flow twice. Local trials retain the enclosing cross-axis capacity so that an
oversized authored height cannot suppress a necessary font reduction. Only the most recent
intersecting box growth is eligible for release after a later font rule.

The staged path is bounded and applies to independent unrotated rectangular text leaves
under a linear flow and optional composite ancestors. Referenced constraints, other shape
text frames and other enclosing algorithms retain the general solver. Selector grouping is
linear in the collected controls; work accounting covers allocation, scans and searches.
Original tests cover scope rejection, exhausted work budgets, wrapping after sibling
allocation, and font reduction within a constrained cross axis.

The paired native geometry matrix contains 1,619 original cases: strict matches within
0.025pt increase from 1,499 to 1,552, with no previously matching case lost and no error
increase above 0.025pt. An independent 16-case boundary group improves from zero to eleven;
its remaining cases include rotated/elliptical text and one 0.042pt size discrepancy.
The initially tested local-height restoration worsened two boundary cases; preserving cross
capacity fixes both, and those rejected results remain in private validation notes. A visual
comparison separately checks text wrapping and container dimensions on representative cases.
This does not close stateful multi-phase height searches, coupled references or snake abort
behavior; the 67 nonmatching cases remain failures rather than being removed from the matrix. Sixteen additional
long-word/font controls are unchanged from the prior implementation: ten match and six still
fail. They are a separate next-stage investigation, not extra passes in the 1,619-case matrix.

### Rich column-label advances

Rich column labels opt into a distinct chart metric policy: nominal legacy glyph advances
use 1/200pt precision, while continuous GPOS positions remain unchanged. Line fitting rounds
both accumulated and available width to 1/200pt for comparisons, without rounding stored
positions or tab accumulation. Ordinary slide and diagram policies retain their existing
behavior. Rounding only the available width was independently falsified and is not used.

The integration matches the frozen candidate on all 656 pages in 43 original packages.
On the 628 retained pages, mean red-text bounding-box RMS changes from 1.812 to 1.792pt and
background RMS from 1.116 to 1.064pt. On 28 new controls, text RMS changes from 4.388 to
0.625pt; matching line-band counts increase from seventeen to twenty-one among twenty-one
changed comparable cases. No previously matching line-band count or text-presence check is
lost. Forty-eight-glyph painting controls independently reduce center RMS from 0.764 to
0.054pt for Inter and from 0.484 to 0.042pt for Lato.

Small regressions are still counted: fourteen retained red-text and two retained background
bounding-box increases at 1.5x, plus one new Lato background increase. All seventeen flagged
pages were rerendered at 4x; remaining old red increases are at most 0.052pt, and the new Lato
case increases by 0.087pt against an existing plot-center error. These observations do not
remove the original counts. Manual placement, non-centered half turns, plot alignment,
synthetic-font line differences and fractional baseline/hinting differences remain open.
The rejected first precision candidate broke five Open Sans pages; the final candidate
restores all five exactly to their previous pixels.

### Geographic cache rebuilding

The model retains the document's provider identity instead of substituting a new provider.
Binary cache queries use their own explicit entity-type vocabulary, distinct from visible
schema clear XML. Rebuilt caches use finished raw-DEFLATE streams. Child-enumeration keys
retain authored type order and multiplicity, and distinguish an absent result from an
explicitly empty result. Child IDs include boundary-less entities. These query tables are
preserved rather than inferred from parent edges.

Large ring collections split into at most 4,096 rings per polygon without changing ring
order. Independent native controls put a hole or island across that split and remain exact.
The writer bounds provider bytes, total points/rings, entities, lineage and child references,
metadata, expanded XML and compressed output. Invalid provider values become empty; a
geometry-only empty-provider cache uses clear XML, while encodable child-query tables retain
a binary representation. An oversized or unencodable cache falls back to bounded clear
geometry/lineage; binary-only query tables are omitted in that out-of-budget fallback.

All 24 forced rebuilds pass schema validation. Geometry, bounds, data bindings and child-query
keys/states survive reopening; existing empty-kind normalization and entity ordering prevent
a claim of complete raw-model identity. All 102 renderer checks match their frozen references:
78 original sources, including 73 retained controls, and 24 rebuilt documents compared with
their original sources. Native PowerPoint rebuilds are separately tested: thirteen of fourteen
have exactly matching whole-image pixels and map paths. This includes restoring all 638 gray
background regions in a 641-region control, two multilevel empty/absent-result controls, and
two hole/island split-boundary controls.

One complex Robinson native rebuild still fails: 7,053 image pixels change, with four old gray
paths becoming three, while 240 other gray paths and selected blue geometry remain exact.
Separate causal controls trace this residual to the existing reader rejecting decoded
polygons whose declared point count differs from the actual count. Seven independent native
controls accept smaller, larger and zero declarations; the bounded reader repair is a
separate, not-yet-integrated change. Earlier blank outputs, negative provider/type controls and a failed native hybrid are
retained as failures. The seventeen rendering threshold sequences and two Albers precision
failures are unchanged. These tests neither establish every provider/entity-type branch nor
make unsupported LibreOffice map output a passing reference.

Final local `cargo xtask ci` passes all eight stages with 2,621 tests including doctests,
including the upstream About-page integration. Renderer source is unchanged by that integration.
The final executable reproduces all 102 frozen map images and 656 chart pages. Eighty selected
linear/boundary geometries exactly match the full-matrix candidate; its production rule logic
is unchanged. Native and renderer comparisons retain their distinct acceptance criteria.


## PPTX.47 — geographic counts, manual labels and height reclaim

### Geographic point counts are advisory

A valid encoded polygon is no longer discarded when its declared `numPoints` differs
from its decoded point count. Independent original controls vary that declaration from
zero through values below and above the actual count, including a ring with a hole.
Native PowerPoint keeps the geometry in every case. Decoder validity follows the actual
payload; the declaration never drives an allocation. Shared chart-wide limits cap decoded
points, attempted rings and visited polygons. Empty/invalid polygons consume work, and a
truncated or over-budget polygon is rejected atomically instead of keeping a valid prefix.

This restores 805 nonempty rings that were incorrectly filtered from a complex cache.
Changing only the cache prefix or polygon partition did not repair the mismatch; restoring
the valid decoded rings did. Eight fresh native source/rebuild pairs, including the complex
Robinson case and seven independent controls, have identical pixels and all vector paths.
The Robinson case now retains all 249 paths; the previous rebuild retained 248.

Thirty-one forced rebuilds pass schema validation and retain geometry, bindings and query
states. Existing entity ordering and empty-kind normalization are unchanged. Integrated
rendering reproduces all 140 frozen candidate images: 102 retained inputs, seven new
controls and 31 new rebuilds. Among the 102 retained inputs, 36 improve toward native and
66 are pixel-identical, with no measured mask regressions. New rebuilds render exactly like
their candidate source inputs. Files saved earlier with already-dropped geometry cannot
recover those absent rings. Sparse controls still have the inherited attribution-text
mismatch; their whole-image gray-mask score is not relabeled as a pass. Seventeen threshold
sequences and two Albers precision sequences remain unresolved.

### Fixed manual rich-label anchoring

Supported fixed single-column labels take horizontal alignment from the first paragraph.
Default or true `anchorCtr` centers the retained line union inside the authored insets;
explicit false retains left/right edges. Centered paragraphs remain centered. Automatic
ellipsis width is excluded from the centered union, and the displacement is applied in
text coordinates before rotation. Background and clip rectangles retain their dimensions.
Only the renderer's working body is normalized; stored text and authored anchor properties
remain intact. An unconditional recentering approach was falsified and is not used.

Seven new original packages provide 43 native controls covering explicit anchors, mixed
paragraph alignment, insets, rotation, RTL and fonts. The final integration reproduces all
699 frozen pages in 50 packages. Compared with the previous candidate, 654 pages retain
identical pixels, 43 improve the standard text bounding-box metric, and two thin clipping
strips improve separately; no new regression is found. Mean text-box RMS on the 43 changed
comparable pages improves from 8.688 to 0.375pt. Yellow background outer bounds are unchanged;
this does not mean every background-colored pixel is unchanged, since moving text changes
its occlusion. Existing fractional baselines, hinting, marker attachment, automatic baseline
clamping, justified text, multiple columns and manual half turns remain outside this fix.

### Reclaim height in rule order using its growth metric

After a later font rule, height reclaim tests ordinary line overflow rather than requiring
an unbroken word. Otherwise a long word that legitimately wraps prevents returning height
the smaller font no longer needs. Width reclaim retains the word-preservation metric. Pending reclaim runs immediately after the
font stage, before a later width rule can change the wrapping budget. Delaying reclaim until
the end incorrectly uses that future width to reduce the height of earlier wrapped lines.
Original evaluator tests distinguish the two measurements and retain the unresolved word
wrap while checking that the original height is restored. A separate test widens the text
after font fitting and verifies that the preceding multi-line height remains intact.

The 1,619 retained native geometry cases are unchanged. A separate 16-case long-word/font
matrix improves from ten to twelve strict matches: four height errors fall from 14.094 or
23.484pt to at most 0.127pt, with two still above the 0.025pt tolerance because of width.
Twenty-four additional original order controls improve from fourteen to seventeen matches;
eight errors shrink, including font-before-width cases. Together the 1,659 cases improve
from 1,576 to 1,581 matches, with no lost prior match and no error increase above 0.025pt.

A subsequent independent eight-case holdout freezes predictions before native export and
changes dimensions, font limits and text boundaries. Strict matches improve from two to
seven; the one remaining root-width discrepancy is 0.137pt. All eight predicted heights
match within 0.025pt. This group is additional to the 1,659-case matrix.

Rejected steps remain explicit. Property-specific reclaim without correcting its timing
worsened two newly measured height errors from 6.648 to 62.012pt; immediate stage-local
reclaim corrects both. An emergency-word-break allocation candidate gains fifteen matches
but loses five existing ones; it remains an experiment, not part of this change. Broader
allocation, coupled rules and snake abort paths remain open. No complete external-converter
replacement is claimed.

### Integration and publication

All new published material is Rust source, contributor-original programmatic tests and
technical documentation. Native documents, vendor/provider geometry, screenshots, fonts,
user content and evidence paths remain private. Final local `cargo xtask ci` passes all eight stages with 2,631 tests including doctests.
Hosted CI is tracked separately. The earlier interrupted run is not counted as a pass.


## PPTX.48 — centered snake fitting and column-label baselines

### Keep fitting and placement state separate

Centered same-direction snake fitting now measures logical extents after pending
alignment changes, using a bounded normalized search. Nominal and fitted alignment
references remain separate. A growing held boundary replaces the following reference;
its nominal correction is scaled once. A leftward correction restores the portion of
pending growth consumed by line expansion. Raw-length rejection, centered-flow overflow,
cross-axis overflow and final logical overflow stop at different stages. Unvisited
children retain zero or nominal placement seeds as appropriate, and partial exits do
not incorrectly apply the completed layout's transpose or reflection.

The worker's 1,425-case, 61-source geometry matrix improves from 1,254 to 1,410 strict
matches, with no lost match or increased measured error. All 334 comparisons with an
independent private geometry model agree within 0.025pt; the largest difference is
0.013405pt, including rounded inspection coordinates. An initial candidate lost eight
prior matches and is rejected. Independent reference tests include a six-case holdout
that initially predicted five correctly; the corrected model explains all six, and a
subsequent four-case holdout predicts all four. The retrospective correction is not
counted as an original successful prediction.

Integration deduplicates those sources with the preceding text-rule matrix: 1,921
cases in 80 groups improve from 1,681 to 1,837 strict matches. No prior match is lost,
and no common-box maximum or mean error worsens by more than 0.025pt. All 1,425 shared
worker geometries are reproduced exactly. These are geometric comparisons, not a
claim of complete pixel equivalence or coverage of every SmartArt algorithm.

Visible-bound checks initially pass nine of fourteen cases at 4K. Of the five large-canvas
failures rechecked at eight pixels per point, four meet the same 0.35pt threshold; one
remains at 0.375pt. Those higher-resolution observations do not rewrite the original
nine-of-fourteen result. Fifteen worker cases still fail strict geometry: six
same-direction cases, including four clipping-only mismatches, and nine pre-existing
reverse cases. Hierarchy defaults, coupled text-rule allocation and broader reverse
abort behavior remain incomplete.

### Centered column labels stop at the bar baseline

Centered labels constrain their center to leave three points inside the bar baseline.
Automatic labels use their capped rotated extent; fixed manual labels use the unrotated
height. Authored offsets are applied afterwards, followed by the existing chart-frame
clipping. Explicit wrapped overflow and a retained oversized first line expand from
the positioned cap. No-wrap overflow keeps the full body centered on the constrained
center. Stored label text and authored properties are preserved.

Thirty-four new original pages in five packages pass schema validation and native
open/export. An initially transient native rotation result is retained, while two
subsequent exports of identical source bytes agree on all six affected pages. A package
with invalid XML ordering is excluded and replaced with an independently validated
source; neither the invalid run nor the transient is silently counted as a pass.

Across 733 pages, 621 retain identical pixels and 112 change. The standard text-bound
metric improves on 107 changed pages, worsens on two at coarse resolution and is unchanged
on three. Mean RMS over changed comparable pages falls from 10.917 to 0.988pt. Background
bounds improve on 105 of 108 comparable changed pages, with no regression. Both coarse
text regressions improve when separately measured at eight pixels per point, but remain
reported in the coarse counts. Line-band matches increase from 104 to 109 among the 112
changed pages, without losing an earlier match.

The retained 699 pages contain 619 identical pages, 78 text-bound improvements and the
two coarse regressions. Among the 34 new pages, two are identical, 29 improve the text
metric, three have equal bounds despite changed pixels, and none regress. Some asymmetric
inset controls still wrap to four lines where native uses three. Native category-axis
ink can appear over a label where the renderer currently paints the label over the axis.
Manual edge coordinates, axis geometry, glyph hinting and marker attachment also remain
open. This is a bounded column-label repair, not complete chart-label parity.

### Integration and publication

The integrated build reproduces all 733 frozen label pages and all 140 geographic
reference images exactly. Local `cargo xtask ci` passes all eight stages, with 2,642
tests including doctests; hosted CI is tracked separately. Native
screenshots, source decks, provider geometry, fonts and local evidence paths remain
private; publication includes only Rust source, original programmatic tests and these
documents. Complete external-converter replacement is not claimed.


## PPTX.49 — hierarchy attachment and authored height search

### Fit hierarchy children before attaching their parent

Hierarchy-child rows retain a shared edge, honor their direction and sibling gap,
and fit width and height with one bounded scale. Zero-area children do not consume
sibling spacing. The root uses the first direct child as parent and the second as
its child container. A nested hierarchy-child container exposes its occupied
rectangle; a flat text child retains its own attachment origin. Additional direct
children remain at the hierarchy origin and contribute to occupied bounds. Authored
positions do not shift these algorithmic attachment points.

After nested children are placed, the root fits occupied width and height uniformly
and centers the result. One-axis-zero children still contribute their visible extent;
double-zero editable objects remain in the model even when native export paints none.
All searches, scans and subtree transformations consume the existing shared work budget.

The child-hierarchy arc preserves the preceding 1,921 cases, improving twelve strict
matches, and adds 98 original controls. The root arc leaves all 2,019 child-arc geometries
unchanged and adds 57 controls: 52 strict matches within 0.025pt, with five editable-versus-
painted zero-area identity differences explicitly retained. All 57 painted-box comparisons
pass. Ten root raster controls pass visible bounds within 0.2pt at eight pixels per point;
the largest measured difference is 0.125pt. This is not a whole-image pixel-equivalence claim.

### Reclaim height within the original authored interval

After font fitting, height reclaim searches the original authored height interval,
while retaining the previously accepted height as the fallback if later trials fail.
Width reclaim keeps its existing fitted endpoint. Twelve original controls and eight
independent holdouts distinguish the authored interval from repeatedly narrowed ranges;
all twenty native heights lie on the original authored search grid. The implementation
matches eighteen of twenty heights within 0.025pt; two existing font-stage cases retain
about 28.701pt height error. Whole-box matches improve from three to nine of twenty.
Remaining width, height and allocation differences stay in the strict denominator.

The combined native matrix contains 2,096 unique cases and improves from
1,854 to 1,980 strict matches, with no lost prior match and no
common-box maximum or mean error increase above 0.025pt. A remote run initially reported
six regressions because an unavailable requested font was substituted. Font diagnostics
identified the actual faces; the same binary on the original validation environment
passes all 24 affected font controls without source or tolerance changes. Those remote
observations remain recorded as environment failures, not successful regression tests.

Text-triggered hierarchy-root fitting, additional container/connector interactions,
centered snake clipping/reverse aborts and broader coupled rule search remain incomplete.
Zero-area identities and genuine visible residuals are tracked separately.

### Integration and publication

The feature passes all eight local `cargo xtask ci` stages with 2,660 tests including
doctests. Synchronizing upstream bounded font shaping adds six tests: the merged tree
passes all eight stages with 2,666 tests. DrawingML kerning suppression and legacy
pairs remain intact across shaping-piece boundaries, with an explicit regression test.
The merged binary retains all 2,096 geometry results and reproduces all 733 frozen
chart pages and all 140 map reference images exactly. Hosted CI is tracked separately.
Publication is limited to Rust source, contributor-original programmatic tests and
technical documentation. Native documents, screenshots, fonts, user content, provider
geometry and private evidence paths remain unpublished. Complete external-converter
replacement is not claimed.


## PPTX.50 — occupied root flows, font plateaus and rich-label margins

### Root linear containers

Measured root linear flows now execute child algorithms before packing their occupied
geometry. Unused allocation space does not itself trigger fitting. Actual cross-axis
overflow constrains the affected child allocation and reruns that child; the subsequent
finite flow fit preserves the resolved cross dimensions. Nested flows keep their prior
allocation path. Endpoint comparisons tolerate floating-point conversion roundoff;
independent capacities 0.001mm below the exact boundary still reject that endpoint.

Sixty new original native controls distinguish root-only text fitting, mixed child
algorithms, reverse direction, cross-axis overflow and exact fit boundaries. All sixty
match within 0.025pt. The container delta improves five of the retained 2,096 cases and
leaves the other 2,091 geometries unchanged. Twelve selected raster checks at 8 pixels
per point (7,680 by 4,320) match colored-fill bounds; one existing whole-ink edge remains
0.125pt different. This is not a full-image pixel-equivalence claim.

The affine-cycle regression still checks exact 30mm pre-layout constraint resolution.
Its export check separately verifies the real forced-word-break trigger and the finite
painted fit, instead of conflating constraint resolution with rendered dimensions.

### Font-search plateaus

A shrinking font endpoint may preserve the forced-word-break count while improving the
intended font fit. The linear search accepts this non-worsening endpoint; an endpoint
with a worse score remains rejected. Twelve independently authored root/nested controls
validate font limits from 20 to 28pt, including limits that retain the same break count.
All twelve font sizes now agree with native output. Together with the twenty existing
height controls, all 32 measured font sets agree.

The twenty height controls now match every height within 0.025pt, correcting the two
previous 28.701pt height errors. Whole-box matches improve from nine to ten of twenty;
width and placement errors remain separately visible. The combined 2,168-case geometry
matrix improves from 2,000 to 2,052 strict matches with no lost prior match and no
common-box maximum or mean error increase exceeding 0.025pt. The remaining 116 strict
failures remain in the denominator, including nonpainted identity differences.

### Rich column-label margins

The renderer applies the resolved top inset to both vertical edges of its working rich
label body, preserving the authored bottom inset for editing and serialization. Custom
margin detection happens before this normalization. Even a one-EMU authored deviation
selects the custom path. Wrapped automatic half turns with custom margins use bounded
minimal-width measurement before painting at the resulting natural width. Centered
custom overflowing labels position their complete body, with the existing chart clip.

The original 814-page matrix keeps 738 full pages pixel-identical. Among 75 comparable
changed text-bound measurements, 73 improve, one stays equal and one regresses; mean
RMS falls from 29.554 to 1.111pt. One additional changed page recovers previously missing
text, matching all seventeen native line bands. Of the retained 733 pages, 727 remain
identical and six change without a measured text-bound regression.

The disclosed coarse rotated-label regression increases RMS by 0.510pt (2.357 to
2.867pt). All four local line bounds improve in a separate high-resolution comparison,
but this does not erase the coarse result or the remaining marker-prefix centering
mismatch. Previously recorded chart failures remain open. Axis paint order, broader
chart families and complete manual/justified label layout are not resolved by this arc.

### Integration and limits

The combined binary reproduces all 814 frozen chart pages and all 140 map images
exactly. Full local `cargo xtask ci` passes all eight stages with 2,684 tests including
doctests; hosted checks remain separate.
Private native images and synthetic observation files are not distributed. Publication
contains source, original programmatic tests and documentation only. Complete external
converter replacement remains unproven.


## PPTX.51 — reverse alignment phases and measured dimension progress

### Retained reverse-flow reference

Nominal reverse snake placement preserves its alignment reference when intervening
short rows consume the visible offset. Forward growth updates that reference and
scheduled advances translate it. A following row crossing the reflected boundary can
therefore use the surviving reference rather than incorrectly resetting its origin.
Nominal, fitted and restored placement are explicit phases; unit scale alone cannot
identify them. Work remains bounded by the shared layout budget.

Independent histories and holdouts distinguish different first-row widths, zero to
two neutral pairs, boundary positions and fitting. The 593-case reverse subset improves
from 572 to 581 strict matches without losing a match or increasing measured maximum
or mean error beyond 0.025pt. An original 153.072pt error falls to 0.0081pt. Its visible
bounds improve from a 277-pixel difference to zero in the 4K comparison. Two independent
controls improve from 427/463 pixels to one pixel. These are colored visible-bound
measurements, not full-image pixel equivalence.

An observable pack/repack regression protects the fitted tail and all unchanged
repacked rectangles. Removing the phase distinction or substituting a scale-only test
makes this regression fail. The native fitting counterexample itself remains unresolved;
its old error is retained rather than widened by the nominal repair. Twelve cases in
the reverse subset still fail strict comparison.

### Progress without changing the measured text budget

General growing absolute width/height rules can change an allocation while rotation or
inscribed text geometry leaves the measured text budget unchanged. Such an endpoint is
retained only when its score does not worsen and the affected node identities, placed
sizes, text rectangles, fonts, margins and text rotation remain equivalent. Comparisons
allow only floating-point roundoff. Factor rules, shrinking dimensions and changed
budgets retain the preceding search path. Earlier score-only candidates failed existing
coupled-size tests and were rejected; those tests were not weakened.

Forty-one new original controls retain thirteen strict matches, with no lost match or
measured regression. The general-rule delta changes two retained and four new geometries.
Three selected rotated cases match painted colored bounds within 0.0081pt and have exact
colored-fill bounds at eight pixels per point. The original model-box checker compares
unrotated boxes with native painted bounds; its failures remain in the original tally.
Rotation-corrected measurements are separate: two original painted-bound errors fall
from 101.728pt to 0.0081pt, and an independent error from 107.718pt to 0.0071pt.

Of 14,096 inspected shapes in the general-rule matrix, five font-property sets change,
all toward native sizes. Three become exact; two remain 17pt against native 16pt.
Ellipse allocation/order, that remaining font difference, and broader rule sequencing
are not resolved by this change.

### Native point-label persistence

A chart's numeric point-label indices serialize as JSON object keys. Internally tagged
shape deserialization buffers those keys as strings, which previously made a native
file with point labels fail to reopen. The reader now accepts string and numeric index
representations without changing the serialized format. Invalid and aliased indices
are rejected. An original save/load test failed before the fix; the format test suite
passes afterward. Two original packages totaling seven pages preserve rendered pixels
through native save/reload.

### Integration and remaining scope

The combined geometry matrix has 2,225 unique cases: 2,069 to 2,078 strict matches within
0.025pt, no lost match and no common-box maximum/mean increase beyond 0.025pt. All final
geometries exactly reproduce the accepted isolated changes. The remaining 147 strict
failures include rotated model-versus-painted bounds and nonpainted identities; they
remain explicit rather than being removed from the denominator.

The upstream synchronization preserves absolute-point bullet metadata, image-fill and
export validation improvements, alongside these import changes. Its new optional bullet
size field is defaulted to null when comparing the older inspection schema; no non-null
property is discarded. All 14,096 text-property sets match the isolated candidate after
this schema default, and all 326 recovery pages preserve geometry and PDF text/font
positions. The final binary reproduces all 814 frozen chart pages, 140 map images and
six selected high-resolution layout images exactly. Full local `cargo xtask ci` passes
all eight stages with 2,707 tests including doctests; hosted checks remain separate.

The existing seventeen map-reduction sequences, two Albers precision sequences and
rich-label/axis paint limitations remain open. Only source, original programmatic tests
and documentation are published; private native output and observation decks are not.
Complete external converter replacement is not established.

### PPTX.70: chart text, axis placement and physical frames

Chart text now uses explicit axis roles and preserves authored crossings, tick
positions and label visibility independently of axis deletion. Crossing values follow
Office's documented crossed-axis interpretation ([MS-OI29500 §2.1.1447](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/c5ba9569-da34-4530-9743-b040df140552)), confirmed by independent native controls. Physical chart
frames keep font and stroke sizes in slide points through the validated group
transforms. Generated numeric General labels have a bounded character budget;
authored number formats and data-label precision retain their own behavior.

Category ticks retain complete lines that fit the frame and apply the authored
overflow policy. Ellipsis layout reserves marker width and then positions the
retained literal text independently of the marker. Low-side exact line pitches
normalize before baseline selection; independently authored high-side metrics
retain their previous behavior. Dense flat grid strokes share bounded paint
work without dropping their accepted endpoints.

Validation:

- The final integrated executable reproduces all 1,218 source-bound worker chart
  renders. Relative to the accepted parent/P60 chart baseline, 14 pages are
  identical and 1,203 improve native RGB error. One raster-backed PDF page
  increases by three RGB units over five pixels; it remains an explicit residual.
- All 44 frozen historical masks and 222 valid-font category comparisons retain
  or improve their previous errors. Two font-invalid category controls stay
  excluded. Seventeen remaining PDF/crop observations are retained separately;
  their checked direct 4x output matches the preceding candidate.
- Independent fractional-pitch controls cover three open fonts, natural-height
  boundaries and a six-page Line transfer. The transfer's 18 continuous native
  baselines match its paired Column controls exactly. Across 110 new direct
  comparisons, 24 improve and 86 stay exact against the preceding candidate.
- Fourteen independent Column/Line reflection controls retain the native chart
  center, physical size and upright category order through own, parent and nested
  flips. Maximum native plot-edge error is below 0.000085pt. All fourteen direct
  renders improve over the prior renderer. Two nested ordinary-text witnesses
  lie above the page and are checked in unbounded PDF extraction; this is not a
  claim of fourteen visible witnesses. Native Line smoothing and marker defaults
  still differ and are retained as a separate rendering gap.
- Six selected integrated direct 4x pages reproduce the reviewed candidate
  exactly, including truncation, multiline ellipsis alignment, fractional pitch
  and an unchanged high-side control. Native/before/after contacts were reviewed.
- All 6,940 archived models, 166 same-direction controls, 13,622 text bodies,
  661 PDF text pages, 90 retained 4x rasters and 140 map images remain exact.
- Full integrated `cargo xtask ci`: all eight stages pass, 2,829 tests including
  doctests, eight existing ignored tests.

Known high-side frame offsets and broader chart/text placement differences remain
open. The PDF observations above are not waived by the separate direct-raster
checks. Native SmartArt search and geographic precision remain incomplete.
Categorical geographic color dimensions are not yet modeled. The inspected cache
corpus contains only clear XML and recognized raw-DEFLATE envelopes; other
envelopes remain unverified, rather than demonstrated failures. This is not full
LibreOffice replacement.
