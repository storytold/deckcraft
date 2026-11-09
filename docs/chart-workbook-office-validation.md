# Chart workbook validation in desktop Office

> **Last reviewed:** 2026-11-08 · **Last updated:** 2026-11-09 · **Change:** trivial (revision history) · **Status:** measured on Mac only; workbook export fix remains in [PR #241](https://github.com/storytold/deckcraft/pull/241).

A working **Edit Data** command does not establish that an imported workbook survived.
PowerPoint for Mac can reconstruct a one-sheet workbook from chart caches. That can hide
loss of additional sheets, formulas and formatting. This protocol separates reconstruction,
preservation, recalculation and persistence for [issue #1047](https://github.com/storytold/deckcraft/issues/1047).
It adds independent native validation of the existing fix, without implementing it again.

## Measured run

Tested 2026-11-08, macOS **27.0.1 (26A434)**, PowerPoint **16.114.4 (26110224)** and Excel
**16.114.4 (26110224)**, Office Home & Business 2021, French UI, AutoSave off.
No preferences were changed. Formula results below demonstrate automatic recalculation in
this session; no claim is made about other calculation modes, Windows or Office web apps.
Both applications were quit after the test. No Office-generated files or screenshots are
included in the repository.

The baseline CLI was built from `26c1ac83` (based on `fc35ad67`; its additional change
concerns slide import). The workbook writer was also inspected at `c4674f24` on main.
The candidate CLI was built from PR #241 commit `a01e062a`. These are separate checkouts,
not a merge of that old branch into current main. A merge-tree check against `c4674f24`
reported conflicts in the model chart, chart reader and chart writer; successful Office
validation is not evidence that the branch merges cleanly today.

| Input / action | Package before Office | Observed in desktop Office |
|---|---|---|
| Baseline newly inserted column chart | No embedded XLSX | Edit Data opened Excel with reconstructed **Feuil1**, one sheet, used range A1:B3; B2 was 1. The command was available despite the missing workbook. |
| Baseline title-only edit of the two-sheet source described below | Embedded workbook removed | Edit Data again opened reconstructed Feuil1, one sheet, used range A1:B3. Original Sheet1/Notes names, extra cells and formulas were gone. |
| PR #241 title-only edit of the same source | Workbook bytes exactly matched the source | Edit Data opened both **Sheet1** and **Notes**; Notes retained its sentinel and formula result 2. |
| PR #241 data edit, Sales values changed to 7,2 | Both sheets and formulas retained; formula caches were still 3 and 2, with fullCalcOnLoad requested | Excel showed Sheet1!B2=7, Sheet1!D2=9 and Notes!B2=14, with the sentinel retained. |
| Native edit of that PR output, B2 changed from 7 to 8; close Excel, save/close/reopen PowerPoint, Edit Data again | Office output kept only as local evidence | Reopened workbook retained both sheets, B2=8, D2=10 and Notes!B2=16; Notes!A2 still contained the sentinel. |

The title-only workbook SHA-256 before Office was
`db31cdf1c3ef698ac2d50c6d70b50baa497efd97fa4826c24192edc5a916453a`
for both source and PR output. This is a byte-preservation check before Office touches it,
not an expectation that Office will save identical ZIP bytes.
The saved local package independently retained B2=8, the D2 `SUM(B2:B3)` formula
with cached result 10, and the Notes!B2 `Sheet1!B2*2` formula with cached result 16.
The eight existing PR #241 `chart_workbook` integration tests also passed locally.
The PR's newly generated workbook was inspected in the package; its new-chart native
edit/save/reopen path was **not** separately exercised in this run.

Protocol clarification on 2026-11-08: the checks below now require pre-Office linkage
verification, chart/workbook agreement and separate one-sheet new-chart expectations.
These additions describe required future checks; they do not expand the measured run above.

## Reproduce with owned synthetic input

Do not use issue attachments or linked reproduction files. Use separate baseline and
candidate binaries, and keep generated outputs in an ignored local directory. The following
commands use the existing CLI and the same chart data as the measured run:

```sh
# Set DECKCRAFT to the binary built from the revision under test.
"$DECKCRAFT" run --cmd 'file.new={"blank":true}' \
  --cmd 'slide.new={"layout":"blank"}' \
  --cmd 'insert.chart={"type":"column","categories":["Alpha","Beta"],"series":[{"name":"Sales","values":[1,2]}]}' \
  --save new-chart.pptx
```

For the preservation control, create a workbook containing the following original data.
The measured source was generated with GridCraft, not Office:

| Sheet | Cell | Content |
|---|---|---|
| Sheet1 | A1:B3 | Category, Sales / Alpha, 1 / Beta, 2 |
| Sheet1 | D1 | Dependent |
| Sheet1 | D2 | =SUM(B2:B3), cached result 3 |
| Notes | A1:B1 | Sentinel, Formula |
| Notes | A2 | Keep this sheet |
| Notes | B2 | =Sheet1!B2*2, cached result 2 |

With a GridCraft CLI checkout, this generates that workbook:

```sh
"$GRIDCRAFT" run \
  --cmd 'range.setValues={"range":"A1:D3","values":[["Category","Sales",null,"Dependent"],["Alpha",1,null,"=SUM(B2:B3)"],["Beta",2]]}' \
  --cmd 'home.insertSheet={"name":"Notes"}' \
  --cmd 'range.setValues={"range":"A1:B2","values":[["Sentinel","Formula"],["Keep this sheet","=Sheet1!B2*2"]]}' \
  --out source.xlsx
```

Attach it to a copy of the baseline chart package using the public OOXML package structure:

1. Store `source.xlsx` as `ppt/embeddings/data.xlsx`.
2. Add a package relationship from `ppt/charts/chart1.xml` to `../embeddings/data.xlsx`,
   with ID `chartData` and type
   `http://schemas.openxmlformats.org/officeDocument/2006/relationships/package`.
   Its relationship part is `ppt/charts/_rels/chart1.xml.rels`.
3. Add `c:externalData` with `r:id="chartData"` and child `c:autoUpdate val="0"` to
   `c:chartSpace`, before `c:printSettings`, `c:userShapes` or `c:extLst` if present.
   Use the standard chart and officeDocument relationships namespace declarations.
4. Register `/ppt/embeddings/data.xlsx` in `[Content_Types].xml` with content type
   `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`.
5. Preserve other package entries and ZIP it as `source.pptx`. Verify the chart's
   references are Sheet1!$B$1, Sheet1!$A$2:$A$3 and Sheet1!$B$2:$B$3.

This assembly uses our own generated files and the public format, without copying an
Office document or asset. Keep the source immutable. Export each variation through each
binary, including a title-only edit that should not change workbook data:

```sh
"$DECKCRAFT" run --in source.pptx --cmd 'edit.selectAll={}' \
  --cmd 'chart.options={"title":"Renamed chart"}' --save title-edit.pptx
"$DECKCRAFT" run --in source.pptx --cmd 'edit.selectAll={}' \
  --cmd 'chart.data={"categories":["Alpha","Beta"],"series":[{"name":"Sales","values":[7,2]}]}' \
  --save data-edit.pptx
```

Before opening Office, check every output, including the newly inserted chart and every
fallback-regeneration variation, for an embedded XLSX part, its package content type, the
chart's package relationship targeting that part, and `c:externalData` using that relationship
ID. Resolve the relationship target and open the workbook to verify its categories, series
names and values match the chart references and caches. Missing or broken workbook linkage
is a generation failure even if Office later reconstructs Edit Data from the caches.

For the title-only control, also compare the embedded workbook bytes with the source.
Inspect the data-edit cells/formulas and workbook calculation properties, and record the
actual revisions and file hashes. Do not treat cached formula values as recalculated results.

## Native acceptance procedure

1. Record platform, PowerPoint/Excel versions, language, AutoSave state and calculation
   settings if explicitly inspected. Do not change settings to manufacture a passing result.
2. Open a disposable output copy in PowerPoint. Select the chart and choose
   **Chart Design > Edit Data in Excel**. Record errors, repair prompts, sheet names and
   used ranges. A reconstructed sheet counts as reconstruction, not preservation.
3. For the two-sheet title control, check both sheets, the sentinel, formulas and values.
   For the data edit, expect B2=7, D2=9 and Notes!B2=14 after Excel recalculates.
   In PowerPoint, compare chart categories Alpha/Beta, series Sales and plotted values 7/2
   with the workbook; the title-only control instead retains 1/2. Record discrepancies.
4. For that two-sheet data-edit case, change Sheet1!B2 to 8 in Excel. Confirm D2=10 and
   Notes!B2=16, then return to PowerPoint and confirm the chart shows Sales values 8/2
   against Alpha/Beta. Close the embedded workbook, save the presentation, close it and
   reopen it in PowerPoint. Verify the chart still matches those categories and values;
   invoke Edit Data again and verify the same values, formulas, sentinel and both worksheets.
5. Run a newly inserted chart as a separate case. Its generated Sheet1 has B1=Sales,
   A2=Alpha, B2=1, A3=Beta and B3=2 (A1 is blank), matching the chart's categories and Sales values.
   Change B2 to 8, confirm the chart shows Alpha/Beta with Sales values 8/2, then close the
   workbook, save/close/reopen the presentation and verify both the chart and Edit Data
   retain those cells and values. This one-sheet case has no D2 formula, Notes sheet or
   sentinel; do not apply the two-sheet preservation expectations to it. Cover added
   points/series separately: record the exact expected categories, series and cell matrix,
   verify their workbook linkage before Office, and compare chart/workbook data before
   and after a native edit and save/reopen. Fallback regeneration has different preservation
   guarantees, so state which extra cells, formulas or sheets it is expected to retain.
6. Inspect the saved local package as a second persistence check, without committing it.
   Close disposable test documents, discard unused test-only changes, then quit both apps.
   Preserve unrelated user documents.

Do not close #1047 based only on this run. Updating PR #241 against current main and
rerunning its native new-chart/fallback cases remain separate work. Main still needs the
existing export fix; a reconstructed cache cannot restore the lost workbook contents.

## Revision history

| Date | Change |
| --- | --- |
| 2026-11-08 | Recorded the measured Mac title/data preservation run and its exact source revisions and limits. Added the pre-Office linkage, chart/workbook agreement and separate new-chart expectations as future acceptance checks. |
| 2026-11-09 | Added the guide revision history and separate supplemental log entries; preserved the historical measurements and source pointers. |
