//! Contributor-original exact temporal constants from OASIS ODF 1.3 Part 3
//! §§19.374/.386/.389 and W3C XML Schema Part 2 (2004) §§3.2.6/.7/.9.
//! Values remain lexical; cached display is independent and no calculation is implied.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{
    table::{CellConstantState, CellValue},
    text::RunKind,
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:number='urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0' xmlns:math='http://www.w3.org/1998/Math/MathML' xmlns:of='urn:oasis:names:tc:opendocument:xmlns:of:1.2' xmlns:foreign='urn:deckcraft:original:temporal-test'";
const VALUES: &str = "ODF typed cell values and number formats";

fn package(content: &str, styles: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", content),
        ("styles.xml", styles),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(value.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn fixture(styles: &str, columns: &str, rows: &str) -> Vec<u8> {
    package(
        &format!(
            "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page draw:name='Temporal'><draw:frame svg:width='400pt' svg:height='200pt'><table:table table:name='Original'>{columns}{rows}</table:table></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
        ),
        &format!("<office:document-styles {NS} office:version='1.3'><office:styles>{styles}</office:styles></office:document-styles>"),
    )
}
fn source(attrs: &str, body: &str) -> Vec<u8> {
    fixture("", "<table:table-column/>", &format!("<table:table-row><table:table-cell {attrs}>{body}</table:table-cell></table:table-row>"))
}
fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("editable table") };
    table
}
fn table_mut(p: &mut Presentation) -> &mut Table {
    let ShapeKind::Table(table) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("editable table") };
    table
}
fn value(cell: &Cell) -> &CellValue {
    let CellConstantState::Current(data) = cell.constant_state().unwrap() else { panic!("current exact constant") };
    data.value()
}
fn no_warnings(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}
fn warning(p: &Presentation, fragment: &str) -> bool {
    p.tags.iter().any(|(key, text)| key == "odp:warning" && text.contains(fragment))
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    z.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}
fn attrs(kind: &str, lexical: &str) -> String {
    let key = if kind == "date" { "date" } else { "time" };
    let lexical = lexical.replace('&', "&amp;").replace('\'', "&apos;").replace('\t', "&#x9;").replace('\r', "&#xD;").replace('\n', "&#xA;");
    format!("office:value-type='{kind}' office:{key}-value='{lexical}'")
}
fn expected(kind: &str, lexical: &str) -> CellValue {
    if kind == "date" { CellValue::Date { lexical: lexical.into() } } else { CellValue::Duration { lexical: lexical.into() } }
}
fn rewrites(p: &Presentation, expected: &CellValue, display: &str) -> Vec<u8> {
    let mut current = p.clone();
    let mut bytes = Vec::new();
    for _ in 0..2 {
        no_warnings(&current);
        assert_eq!(value(&table(&current).rows[0].cells[0]), expected);
        assert_eq!(table(&current).rows[0].cells[0].text.text(), display);
        current = deckcraft_format::load(&deckcraft_format::save(&current).unwrap()).unwrap();
        assert_eq!(value(&table(&current).rows[0].cells[0]), expected);
        bytes = export(&current).unwrap();
        current = import(&bytes).unwrap();
    }
    no_warnings(&current);
    assert_eq!(value(&table(&current).rows[0].cells[0]), expected);
    assert_eq!(table(&current).rows[0].cells[0].text.text(), display);
    bytes
}
fn qa(name: &str, bytes: &[u8]) {
    if let Ok(root) = std::env::var("DECKCRAFT_TEMPORAL_VALUES_XML_DIR") {
        let dir = std::path::Path::new(&root).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml", "META-INF/manifest.xml"] {
            std::fs::write(dir.join(name.replace('/', "_")), part(bytes, name)).unwrap();
        }
    }
}

#[test]
fn date_and_datetime_lexical_spellings_survive_two_native_and_odp_rewrites() {
    for (index, lexical) in [
        "0001-01-01",
        "2000-02-29",
        "9999-12-31",
        "2024-02-29Z",
        "2024-02-29+00:00",
        "2024-02-29-00:00",
        "2024-02-29+14:00",
        "2024-02-29-14:00",
        "2024-02-29T00:00:00",
        "2024-02-29T23:59:59.0012300Z",
        "2024-02-29T24:00:00.000+14:00",
        "2024-02-29T01:02:03-00:00",
    ]
    .into_iter()
    .enumerate()
    {
        let p = import(&source(&attrs("date", lexical), "<text:p>independent date label</text:p>")).unwrap();
        let bytes = rewrites(&p, &expected("date", lexical), "independent date label");
        let content = part(&bytes, "content.xml");
        assert!(content.contains(&format!("office:date-value=\"{lexical}\"")));
        assert!(!content.contains("office:time-value="));
        qa(&format!("date-{index}"), &bytes);
    }
}

#[test]
fn durations_keep_sign_zero_fraction_and_calendar_components_without_arithmetic() {
    for (index, lexical) in
        ["P0D", "PT0S", "-PT0.000S", "-P01DT02H03M04.500S", "P1Y2M3DT4H5M6.700S", "P1347M", "PT999H", "PT60S", "P0000Y000M000DT000H000M000.0100S"]
            .into_iter()
            .enumerate()
    {
        let p = import(&source(&attrs("time", lexical), "<text:p>elapsed label</text:p>")).unwrap();
        let bytes = rewrites(&p, &expected("time", lexical), "elapsed label");
        let content = part(&bytes, "content.xml");
        assert!(content.contains(&format!("office:time-value=\"{lexical}\"")));
        assert!(!content.contains("office:date-value="));
        qa(&format!("duration-{index}"), &bytes);
    }
}

#[test]
fn xml_edge_whitespace_is_retained_as_exact_decoded_attribute_data() {
    for (kind, lexical) in [("date", " \t2024-02-29T01:02:03.000-00:00\n\r "), ("time", "\t -P00DT01H00M00.000S \r\n")] {
        let p = import(&source(&attrs(kind, lexical), "<text:p>clean display</text:p>")).unwrap();
        let bytes = rewrites(&p, &expected(kind, lexical), "clean display");
        let content = part(&bytes, "content.xml");
        for escaped in ["&#x9;", "&#xA;", "&#xD;"] {
            assert!(content.contains(escaped));
        }
        qa(&format!("whitespace-{kind}"), &bytes);
    }
}

#[test]
fn raw_256_byte_temporal_boundary_is_checked_before_scalar_attachment() {
    let duration = format!("P{}D", "9".repeat(254));
    let datetime = format!("2024-02-29T01:02:03.{}Z", "0".repeat(235));
    assert_eq!(duration.len(), 256);
    assert_eq!(datetime.len(), 256);
    for (kind, lexical) in [("time", duration), ("date", datetime)] {
        let p = import(&source(&attrs(kind, &lexical), "<text:p>bounded label</text:p>")).unwrap();
        let bytes = rewrites(&p, &expected(kind, &lexical), "bounded label");
        qa(&format!("max-{kind}"), &bytes);
        let oversized = format!(" {lexical}");
        let p = import(&source(&attrs(kind, &oversized), "<text:p>bounded label</text:p>")).unwrap();
        assert!(table(&p).rows[0].cells[0].data.is_none());
        assert!(warning(&p, VALUES));
        assert!(export(&p).is_err());
    }
}

#[test]
fn explicit_empty_display_and_absent_paragraph_fallback_are_distinct() {
    for (kind, lexical) in [("date", "2024-02-29+14:00"), ("time", "-P1DT2H")] {
        for (body, display) in [("<text:p/>", ""), ("", lexical)] {
            let p = import(&source(&attrs(kind, lexical), body)).unwrap();
            rewrites(&p, &expected(kind, lexical), display);
        }
    }
}

#[test]
fn rich_display_and_effective_styles_do_not_rebind_temporal_constants() {
    let styles = "<style:default-style style:family='table-cell'><style:text-properties fo:font-family='Liberation Sans' fo:font-size='11pt'/></style:default-style><style:style style:name='Parent' style:family='table-cell'><style:table-cell-properties fo:background-color='#ABCDEF' fo:padding='2pt' fo:border='1pt solid #123456'/><style:text-properties fo:font-size='19pt'/></style:style><style:style style:name='Column' style:family='table-cell' style:parent-style-name='Parent'/><style:style style:name='Strong' style:family='text'><style:text-properties fo:font-weight='bold'/></style:style>";
    for kind in ["date", "time"] {
        let lexical = if kind == "date" { "2000-02-29T01:02:03.1200+02:30" } else { "-P12MT60.000S" };
        let rows = format!(
            "<table:table-row><table:table-cell {}><text:p>cached <text:span text:style-name='Strong'>label</text:span><text:line-break/>second line</text:p><text:p>next paragraph</text:p></table:table-cell></table:table-row>",
            attrs(kind, lexical)
        );
        let mut p = import(&fixture(styles, "<table:table-column table:default-cell-style-name='Column'/>", &rows)).unwrap();
        no_warnings(&p);
        let original = table(&p).rows[0].cells[0].data.clone().unwrap();
        let display = "cached label\u{b}second line\nnext paragraph";
        assert_eq!(table(&p).rows[0].cells[0].text.text(), display);
        {
            let cell = &mut table_mut(&mut p).rows[0].cells[0];
            cell.text.paragraphs[0].runs[0].props.italic = Some(true);
            assert!(Arc::ptr_eq(cell.data.as_ref().unwrap(), &original));
            assert_eq!(value(cell), &expected(kind, lexical));
        }
        let bytes = rewrites(&p, &expected(kind, lexical), display);
        let back = import(&bytes).unwrap();
        let cell = &table(&back).rows[0].cells[0];
        assert_eq!(cell.fill, Some(Fill::solid(ColorRef::rgb(Rgba::rgb(171, 205, 239)))));
        assert_eq!(cell.margins, Some([2.0; 4]));
        assert_eq!(cell.borders[0].as_ref().unwrap().width, Some(1.0));
        assert_eq!(cell.borders[0].as_ref().unwrap().fill, Some(Fill::solid(ColorRef::rgb(Rgba::rgb(18, 52, 86)))));
        assert_eq!(cell.text.paragraphs[0].runs[0].props.size, Some(19.0));
        assert_eq!(cell.text.paragraphs[0].runs[0].props.font.as_deref(), Some("Liberation Sans"));
        assert_eq!(cell.text.paragraphs[0].runs[0].props.italic, Some(true));
        assert!(cell.text.paragraphs[0].runs.iter().any(|r| r.props.bold == Some(true)));
        assert!(cell.text.paragraphs[0].runs.iter().any(|r| r.kind == RunKind::Break));
        assert_eq!(cell.text.paragraphs.len(), 2);
        qa(&format!("rich-{kind}"), &bytes);
    }
}

#[test]
fn repeated_and_merged_anchor_values_survive_without_covered_metadata() {
    for (kind, lexical) in [("date", "2024-02-29"), ("time", "P1Y2MT3H")] {
        for merged in [false, true] {
            let cells = if merged {
                format!(
                    "<table:table-cell {} table:number-columns-spanned='2'><text:p>anchor</text:p></table:table-cell><table:covered-table-cell/>",
                    attrs(kind, lexical)
                )
            } else {
                format!("<table:table-cell {} table:number-columns-repeated='2'><text:p>cache</text:p></table:table-cell>", attrs(kind, lexical))
            };
            let rows = format!("<table:table-row table:number-rows-repeated='3'>{cells}</table:table-row>");
            let p = import(&fixture("", "<table:table-column table:number-columns-repeated='2'/>", &rows)).unwrap();
            let bytes = rewrites(&p, &expected(kind, lexical), if merged { "anchor" } else { "cache" });
            let back = import(&bytes).unwrap();
            assert_eq!(table(&back).rows.len(), 3);
            for row in &table(&back).rows {
                assert_eq!(value(&row.cells[0]), &expected(kind, lexical));
                if merged {
                    assert_eq!(row.cells[0].grid_span, 2);
                    assert!(row.cells[1].is_covered());
                    assert!(row.cells[1].data.is_none());
                } else {
                    assert_eq!(value(&row.cells[1]), &expected(kind, lexical));
                }
            }
        }
    }
}

#[test]
fn temporal_values_do_not_change_cached_display_pixels_or_original_recovery() {
    let original = source(&attrs("date", "2024-02-29"), "<text:p>cached label</text:p>");
    let original_copy = original.clone();
    let mut p = import(&original).unwrap();
    let before = deckcraft_render::render_slide(&p, 0, &Default::default());
    let body = table(&p).rows[0].cells[0].text.clone();
    table_mut(&mut p).rows[0].cells[0].data =
        Some(Arc::new(deckcraft_model::table::CellConstant::from_display(expected("time", "-PT0.00S"), &body).unwrap()));
    let after = deckcraft_render::render_slide(&p, 0, &Default::default());
    assert_eq!((before.width, before.height), (after.width, after.height));
    assert_eq!(before.pixels, after.pixels);
    let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    let recovery = native.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap();
    assert_eq!(recovery.data.as_slice(), original_copy);
    let output = export(&native).unwrap();
    assert_eq!(original, original_copy);
    let back = import(&output).unwrap();
    assert_eq!(value(&table(&back).rows[0].cells[0]), &expected("time", "-PT0.00S"));
    let rendered = deckcraft_render::render_slide(&native, 0, &Default::default());
    assert_eq!(after.pixels, rendered.pixels);
}

#[test]
fn invalid_or_mismatched_temporal_attributes_keep_source_and_refuse_export() {
    let cases = [
        "office:value-type='date'",
        "office:value-type='time'",
        "office:date-value='2024-02-29'",
        "office:time-value='PT1H'",
        "office:value-type='date' office:time-value='PT1H'",
        "office:value-type='time' office:date-value='2024-02-29'",
        "office:value-type='date' office:date-value='2024-02-29' office:currency='USD'",
        "office:value-type='time' office:time-value='PT1H' office:value='1'",
        "office:value-type='date' office:date-value='2024-02-29' office:boolean-value='true'",
        "office:value-type='time' office:time-value='PT1H' office:string-value='x'",
        "office:value-type='date' office:date-value='2024-02-29' office:time-value='PT1H'",
        "office:value-type='string' office:date-value='2024-02-29'",
    ];
    for attributes in cases {
        let bytes = source(attributes, "<text:p>safe cache</text:p>");
        let p = import(&bytes).unwrap();
        assert!(table(&p).rows[0].cells[0].data.is_none());
        assert_eq!(table(&p).rows[0].cells[0].text.text(), "safe cache");
        assert!(warning(&p, VALUES), "{attributes}");
        assert!(export(&p).is_err());
        assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
    }
}

#[test]
fn invalid_and_out_of_profile_temporal_lexicals_are_never_inferred_from_display() {
    for (kind, lexicals) in [
        (
            "date",
            &[
                "0000-01-01",
                "-0001-01-01",
                "10000-01-01",
                "+2024-01-01",
                "1900-02-29",
                "2024-04-31",
                "2024-2-01",
                "2024-01-01+14:01",
                "2024-01-01z",
                "2024-01-01T24:00:01",
                "2024-01-01T12:00:00.",
                "2024-01-01T12:00:00+15:00",
                "2024-01-01 T00:00:00",
                "２０２４-01-01",
                "\u{a0}2024-01-01",
            ][..],
        ),
        (
            "time",
            &[
                "P",
                "PT",
                "P1DT",
                "+PT1S",
                "P-1D",
                "PT.5S",
                "PT1.S",
                "P1.5D",
                "PT1e3S",
                "P1W",
                "P1D1Y",
                "PT1M1H",
                "PT1S1S",
                "PT1SZ",
                "PT 1S",
                "pt1s",
                "PT١S",
                "PT1S\u{a0}",
            ][..],
        ),
    ] {
        for lexical in lexicals {
            let p = import(&source(&attrs(kind, lexical), "<text:p>2024-02-29</text:p>")).unwrap();
            assert!(table(&p).rows[0].cells[0].data.is_none(), "{kind}: {lexical}");
            assert!(warning(&p, VALUES), "{kind}: {lexical}");
            assert!(export(&p).is_err());
        }
    }
}

#[test]
fn formula_matrix_math_and_covered_guards_remain_while_nested_owners_keep_scalar_authority() {
    for (scalar, expected) in [
        (attrs("date", "2024-02-29"), expected("date", "2024-02-29")),
        (attrs("time", "PT1.250S"), expected("time", "PT1.250S")),
        ("office:value-type='string' office:string-value='stored'".into(), CellValue::String { value: "stored".into() }),
        ("office:value-type='boolean' office:boolean-value='true'".into(), CellValue::Boolean { value: true }),
        (
            "office:value-type='float' office:value='1.25'".into(),
            CellValue::Number { kind: deckcraft_model::table::CellNumberKind::Float, lexical: "1.25".into() },
        ),
    ] {
        for guard in ["table:formula='of:=1'", "table:number-matrix-columns-spanned='1'", "table:number-matrix-rows-spanned='1'"] {
            let p = import(&source(&format!("{scalar} {guard}"), "<text:p>cached</text:p>")).unwrap();
            assert!(table(&p).rows[0].cells[0].data.is_none());
            assert!(table(&p).rows[0].cells[0].formula.is_none(), "unsupported caches cannot authenticate executable formulas");
            let diagnostic = if guard.starts_with("table:formula=") { "ODF cell formulas" } else { VALUES };
            assert!(warning(&p, diagnostic));
            assert!(export(&p).is_err());
        }
        let p = import(&source(&scalar, "<text:p>cached<math:math><math:mi>x</math:mi></math:math></text:p>")).unwrap();
        assert!(table(&p).rows[0].cells[0].data.is_none());
        assert!(warning(&p, VALUES));
        assert!(export(&p).is_err());

        let bytes = source(
            &scalar,
            "<text:p>outer</text:p><table:table table:name='Nested'><table:table-row><table:table-cell><text:p>inner</text:p></table:table-cell></table:table-row></table:table><text:p>tail</text:p>",
        );
        let p = import(&bytes).unwrap();
        let check = |p: &Presentation| {
            no_warnings(p);
            let owner = &table(p).rows[0].cells[0];
            assert_eq!(value(owner), &expected);
            assert_eq!(owner.text.text(), "outer");
            assert_eq!(owner.blocks.len(), 2);
            let deckcraft_model::table::CellBlock::Table(child) = &owner.blocks[0] else { panic!("nested owner") };
            assert_eq!(child.table.cell(0, 0).unwrap().text.text(), "inner");
            assert!(child.table.cell(0, 0).unwrap().data.is_none());
            assert!(matches!(&owner.blocks[1], deckcraft_model::table::CellBlock::Text(tail) if tail.body.text() == "tail"));
        };
        check(&p);
        let original = p.clone();
        let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        check(&native);
        let back = import(&export(&native).unwrap()).unwrap();
        check(&back);
        check(&import(&export(&back).unwrap()).unwrap());
        assert!(p == original, "roundtrips must not mutate source authority");
        assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);

        let rows = format!(
            "<table:table-row><table:table-cell table:number-columns-spanned='2'><text:p>anchor</text:p></table:table-cell><table:covered-table-cell {scalar}><text:p>hidden</text:p></table:covered-table-cell></table:table-row>"
        );
        let p = import(&fixture("", "<table:table-column table:number-columns-repeated='2'/>", &rows)).unwrap();
        assert!(table(&p).rows[0].cells[1].data.is_none());
        assert!(warning(&p, VALUES));
        assert!(warning(&p, "covered cell content"));
        assert!(export(&p).is_err());
    }
}

#[test]
fn inherited_number_formats_remain_warned_for_each_temporal_kind() {
    let styles = "<number:date-style style:name='Number'><number:year/></number:date-style><style:style style:name='Data' style:family='table-cell' style:data-style-name='Number'/><style:style style:name='Inherited' style:family='table-cell' style:parent-style-name='Data'/>";
    for (kind, lexical) in [("date", "2024-02-29"), ("time", "PT3H")] {
        let rows =
            format!("<table:table-row><table:table-cell {}><text:p>cached</text:p></table:table-cell></table:table-row>", attrs(kind, lexical));
        let p = import(&fixture(styles, "<table:table-column table:default-cell-style-name='Inherited'/>", &rows)).unwrap();
        assert!(warning(&p, VALUES));
        assert!(export(&p).is_err());
    }
}

#[test]
fn namespace_aliases_resolve_standard_values_without_accepting_foreign_value_attributes() {
    let bytes = source(&attrs("date", "2024-02-29"), "<text:p>label</text:p>");
    let content = part(&bytes, "content.xml")
        .replace("xmlns:office=", "xmlns:o=")
        .replace("<office:", "<o:")
        .replace("</office:", "</o:")
        .replace(" office:", " o:");
    let p = import(&package(&content, &part(&bytes, "styles.xml"))).unwrap();
    no_warnings(&p);
    assert_eq!(value(&table(&p).rows[0].cells[0]), &expected("date", "2024-02-29"));
    let foreign = import(&source("office:value-type='date' foreign:date-value='2024-02-29'", "<text:p>label</text:p>")).unwrap();
    assert!(table(&foreign).rows[0].cells[0].data.is_none());
    assert!(warning(&foreign, VALUES));
    assert!(export(&foreign).is_err());
    // A foreign table lookalike cannot silently become a standard nested owner;
    // the independent foreign-content guard still blocks regenerated output.
    let p = import(&source(&attrs("date", "2024-02-29"), "<text:p>label</text:p><foreign:table/>")).unwrap();
    assert_eq!(value(&table(&p).rows[0].cells[0]), &expected("date", "2024-02-29"));
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert!(export(&p).is_err());
}

#[test]
fn content_edits_clear_only_touched_temporal_authority_and_native_stale_save_is_safe() {
    for (kind, lexical) in [("date", "2024-02-29"), ("time", "-PT0.000S")] {
        let rows = format!(
            "<table:table-row><table:table-cell {} table:number-columns-repeated='2'><text:p>before</text:p></table:table-cell></table:table-row>",
            attrs(kind, lexical)
        );
        let mut p = import(&fixture("", "<table:table-column table:number-columns-repeated='2'/>", &rows)).unwrap();
        table_mut(&mut p).rows[0].cells[0].edit_display(|body| *body = TextBody::from_text("after")).unwrap();
        assert!(table(&p).rows[0].cells[0].data.is_none());
        assert_eq!(value(&table(&p).rows[0].cells[1]), &expected(kind, lexical));
        let q = import(&export(&p).unwrap()).unwrap();
        no_warnings(&q);
        assert_eq!(table(&q).rows[0].cells[0].text.text(), "after");
        assert!(table(&q).rows[0].cells[0].data.is_none());
        assert_eq!(value(&table(&q).rows[0].cells[1]), &expected(kind, lexical));
        table_mut(&mut p).rows[0].cells[1].text = TextBody::from_text("direct stale edit");
        assert_eq!(table(&p).rows[0].cells[1].constant_state().unwrap(), CellConstantState::Stale);
        let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        assert!(table(&native).rows[0].cells[1].data.is_none());
        assert_eq!(table(&native).rows[0].cells[1].text.text(), "direct stale edit");
        assert!(table(&p).rows[0].cells[1].data.is_some(), "save cannot mutate caller");
    }
}
