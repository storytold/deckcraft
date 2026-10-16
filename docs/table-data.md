# Table data, structured notes and review interchange

> **Last reviewed:** 2026-10-16 · **Last updated:** 2026-10-16 · **Status:** supported profiles implemented; final integrated validation passed. Complete OpenFormula, ODF and PowerPoint parity are not claimed.

Table data keeps stored values, formula source and visible rich text separate. Ordinary text
editing makes a cell literal; it never guesses a number from the text. Formatting changes the
visible result without changing the authoritative value. Committed edits recalculate dependents
within bounded work budgets, and undo restores the complete transaction.

## Formula profiles

| Profile | Numeric contract | Grammar |
|---|---|---|
| `exact-decimal-v1` | Frozen: 28 normalized significant digits, scale −28…28 | `+`, `-`, `*`, unary signs, parentheses and same-table references |
| `exact-decimal-v1-formatted` | The same frozen arithmetic, with independent number formatting | The v1 grammar |
| `presentation-decimal-v2` | 28 significant decimal digits, scale −96…96; bounded decimal arithmetic | Arithmetic including `/`, `^`, postfix `%`, comparisons, `&`, strings, error constants, functions, same-table cells and rectangular ranges |

V2 division rounds to 28 significant digits, ties to even. `SQRT` uses bounded decimal digit
extraction; fractional `POWER` returns a certified rounded result or `#NUM!` when certification
or numeric bounds cannot be met. Integer powers are bounded. These operations use decimal
arithmetic rather than binary floating-point conversion.

References use `[.A1]`, `[.$A$1]` and `[.A1:.B3]`; `$` independently marks an absolute row or
column. Function arguments use semicolons, for example `=SUM([.A1:.A3];2)`. Cross-table,
cross-document and external references are outside the profile. The function catalog is:

| Family | Functions |
|---|---|
| Aggregates | `SUM`, `PRODUCT`, `MIN`, `MAX`, `AVERAGE`, `COUNT`, `COUNTA`, `COUNTBLANK` |
| Criteria | `SUMIF`, `COUNTIF`, `AVERAGEIF` |
| Logic and errors | `IF`, `IFERROR`, `IFNA`, `TRUE`, `FALSE`, `NA`, `AND`, `OR`, `NOT` |
| Tests | `ISBLANK`, `ISNUMBER`, `ISTEXT`, `ISLOGICAL`, `ISERROR`, `ISERR`, `ISNA` |
| Numeric | `ABS`, `SIGN`, `INT`, `TRUNC`, `ROUND`, `ROUNDUP`, `ROUNDDOWN`, `MOD`, `POWER`, `SQRT` |
| Text | `CONCATENATE`, `LEN`, `LEFT`, `RIGHT`, `MID` |
| Calendar | `DATE`, `YEAR`, `MONTH`, `DAY`, `TIME`, `HOUR`, `MINUTE`, `SECOND` |

Criteria use deterministic, case-sensitive whole-cell Unicode matching: `*` matches any sequence,
`?` one scalar and `~` escapes the following character. Calendar serials use Gregorian dates with
epoch **1899-12-30** and no fictitious 1900 leap day. Authored temporal strings remain distinct
from numeric serials; formatting does not perform host-timezone conversion.

Errors are `#VALUE!`, `#NUM!`, `#DEPEND!`, `#DIV/0!`, `#REF!`, `#NAME?`, `#N/A` and `#CYCLE!`.
Formula input is bounded to 4,096 bytes, 256 AST nodes, depth 32 and 32 arguments per call;
recalculation also charges shared dependency and numeric-work budgets. Unsupported functions
and arbitrary foreign calculation settings do not acquire inferred semantics.

## Formats and ordered cell content

Formats cover General, Number, Percentage, Currency, Scientific, Fraction, DateTime, Duration,
Boolean and Text. Explicit locales are `en-US`, `fr-BE` and `de-DE`; output never consults the
host locale. Formats support bounded precision, grouping, currency code/symbol placement,
negative signs or parentheses, zero display, literal prefixes/suffixes, colors and ordered
conditional sections. Date/time parts and elapsed hours are typed. This is a defined format
model, not arbitrary spreadsheet format-code execution.

A cell owns its first text body followed by an ordered sequence of additional text bodies and
nested tables. Each child has a stable cell-local ID; table paths identify children through their
owning row, column and ID. Limits are 16 nested levels, 1,024 additional blocks per cell and
1,000,000 aggregate cells/blocks. Paint, hit testing, caret navigation, IME, find and font
collection traverse the same content ownership.

Row/column edits and merge/split use checked transactions and rewrite formula references.
Assignment requires a visible unmerged target cell; a valid merged grid elsewhere in the table
is allowed. V2 reads covered cells as empty; legacy profiles keep their stricter rules. Merge
converts included stored values/formulas to literals while retaining their displayed text,
styles and ordered children; formulas outside the merged region remain formulas.
Pencil/eraser commands use checked reference rewriting and preserve ordered nested content.
New split cells are empty; erased borders merge displayed content without retaining stale values.
Pointer mapping follows painted row heights and composed transforms; eraser tolerance and nearest
border selection use slide-space distances, including scaled groups.

Commands include `table.setCellValue`, `table.setCellFormula`, `table.recalculate`,
`table.setCellNumberFormat`, `table.convertToLiteral`, `table.insertNested`, `table.removeNested`,
`table.enterNested` and `table.leaveNested`. Existing structural/style commands accept nested
owners. Formula, Value, Number Format and Nested Table draft editors expose these operations
through the UI and commit through the engine.

## ODP and PPTX representations

Native and ODP persistence preserve the supported typed values, formulas, number styles and
ordered nested content. The frozen v1 import rules remain strict; the newer profiles carry their
own explicit contract. ODP export uses standard structures with documented foreign attributes
where needed; a schema-valid extension is not evidence of exhaustive ODF calculation support.

PPTX emits editable standard tables and group projections for nested content. DrawingML table
cells have no direct peer for native formula authority or recursive cell ownership. Bounded,
transparent typed DeckCraft metadata records those distinctions alongside the visible core.
On import, metadata is accepted only after fresh regeneration agrees with actual core XML,
relationships and media. A foreign edit that makes the projection stale explicitly refuses
restoration instead of overwriting the foreign edit with old values. Other applications can
edit the standard projection; they are not assumed to calculate DeckCraft formulas or execute
DeckCraft metadata.

Structured notes export uses one core PPTX notes master and notes size, with editable page
artwork/defaults materialized and theme overrides where needed. Verified bounded metadata
retains native master sharing, unused masters and layouts, and per-page sizes. Unused layout
tables, pictures, backgrounds, connectors and slide links retain their typed resource closure;
they do not acquire invented core notes-layout parts. Only the canonical slide notes
body supplies core note text. Native/ODP pages retain their own master association, dimensions,
fields and live slide previews. Structured-notes comparison remains a guarded boundary.
Unused handout layouts use a bounded resource record beside the editable core handout master,
with checked media dependencies and remapped layout, shape, connector and slide identities.
Presentation export retains an explicitly empty text-default list rather than inventing defaults
that change notes inheritance when the file is reopened.

## Advanced comments

Comments have stable message/author identities, persistent random wire GUIDs for newly authored
local comments, Active/Resolved/Closed statuses, nested replies, rich bodies, text ranges, task
metadata and attachments backed by actual file bytes. Commands and the rich review UI support
authoring and editing; native/ODP/PPTX persistence and supported copy/reuse/comparison operations
carry the complete identity, media and slide-reference closure. Original-source warnings still
block guarded cross-document transfers.

Native/ODP text ranges count Unicode scalars, including paragraph separators. PPTX writes a
verified standard shape anchor and retains the exact native range in inbound-verified metadata;
foreign Office character-position units have not been proven equivalent. Exact foreign text-range
anchoring is therefore not claimed. Unknown rich-source attributes, elements, actions and
resources are diagnosed and source-retained rather than silently treated as editable semantics.
Per-hyperlink optional `sound_name` is typed and round-trips exactly; generic ODP sound links
still refuse unsupported action semantics.

Comment replies are bounded to depth 32. Native input preflights group depth 64 and nested-table
depth 16 iteratively before recursive decoding; excessively deep hostile input returns an error.
Attachment import checks desktop file metadata or browser file size before allocating the body,
enforces bounded reads and refuses nonregular desktop files. A failed read cannot fall back to
an unbounded reader. ODP annotations reject XML-invalid body characters, diagnose unknown inline
styles and validate range ends, including pages containing only orphan ends.
Native document creation dates use a separate bounded standard ODP metadata field, preserving
accepted lexical whitespace and CR exactly. Annotation dates do not supply document creation
or modification dates; the existing modification-date policy remains unchanged.

## Remaining limits and verification

Functions/settings outside this catalog and arbitrary format programs remain open. Regular placeholder/style inheritance, template application, newer paragraph
line-height PPTX mapping, advanced paragraph paints, arbitrary settings and private edited-source
cache preservation also retain their existing guards. External-consumer acceptance and exact
PowerPoint font/layout fidelity remain unproven; see [ODP support](odp-support.md).

Focused model, engine, native, ODP, PPTX, UI and hostile-input checks cover these profiles, with
native↔ODP/PPTX edit workflows and visual inspection of the table/review editors. Final aggregate
CI passes all seven gates: **3,249 unique tests passed, zero failed, nine ignored**. The final XML batch passes **306 schema checks**; 37 additional XML parts receive namespace checks only. ODF number-format foreign attributes use the ODF 1.3 extended-document validation rule. All 716 frozen Rust/config/schema hashes remain unchanged. The earlier 2,908-pass checkpoint predates this completion work.

## Revision history

| Date | Change |
|---|---|
| 2026-10-16 | Documented implemented formula/format/nesting, structured-notes PPTX and advanced-comment profiles, limits and completed final validation |
