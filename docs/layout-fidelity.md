# Bounded layout fidelity and font diagnostics

Updated 2026-11-02 (PPTX.31): nested SmartArt capacity and parent-fit reflow,
extending global geographic framing, rotated diagram footprints and rich-label layout.

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

- A missing shape or `type="none"` helper still supplies text measurements, bounds and mapped
  text to its owning shape, but its own text does not paint a second time. Generated separate
  text frames inherit the source shape's hidden state through export/reimport.
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
