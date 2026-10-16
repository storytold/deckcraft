//! Contributor-original ODF constant values. Public semantics: ODF 1.3 Part3
//! office:value-type and its value-attribute table. No spreadsheets/calculation
//! semantics, vendor fixtures, fonts or rendered assets are used here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{
    table::{CellBlock, CellConstant, CellConstantState, CellNumberKind, CellValue, Row},
    text::{Paragraph, Run, RunKind, RunProps, TextBody},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:math='http://www.w3.org/1998/Math/MathML' xmlns:number='urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0' xmlns:of='urn:oasis:names:tc:opendocument:xmlns:of:1.2'";
const VALUES: &str = "ODF typed cell values and number formats";

fn package(styles: &str, tables: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page>{tables}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!("<office:document-styles {NS} office:version='1.3'><office:styles>{styles}</office:styles></office:document-styles>");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("styles.xml", styles.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn frame(rows: &str) -> String {
    format!("<draw:frame svg:width='400pt' svg:height='200pt'><table:table><table:table-column/>{rows}</table:table></draw:frame>")
}
fn cell(attrs: &str, display: &str) -> String {
    format!("<table:table-cell {attrs}>{display}</table:table-cell>")
}
fn source(attrs: &str, display: &str) -> Vec<u8> {
    package("", &frame(&format!("<table:table-row>{}</table:table-row>", cell(attrs, display))))
}
fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(t) = &p.slides[0].shapes[0].kind else { panic!("editable table missing") };
    t
}
fn table_mut(p: &mut Presentation) -> &mut Table {
    let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("editable table missing") };
    t
}
fn first(p: &Presentation) -> &Cell {
    &table(p).rows[0].cells[0]
}
fn no_warnings(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}
fn value(p: &Presentation) -> &CellValue {
    let CellConstantState::Current(data) = first(p).constant_state().unwrap() else { panic!("current constant missing") };
    data.value()
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}
fn assert_reopens(p: &Presentation, expected: &CellValue, display: &str) -> Vec<u8> {
    no_warnings(p);
    assert_eq!(value(p), expected);
    assert_eq!(first(p).text.text(), display);
    let native = deckcraft_format::load(&deckcraft_format::save(p).unwrap()).unwrap();
    assert_eq!(value(&native), expected);
    assert_eq!(first(&native).text.text(), display);
    let bytes = export(&native).unwrap();
    let back = import(&bytes).unwrap();
    no_warnings(&back);
    assert_eq!(value(&back), expected);
    assert_eq!(first(&back).text.text(), display);
    bytes
}
fn native(value: CellValue, display: TextBody) -> Presentation {
    let data = Arc::new(CellConstant::from_display(value, &display).unwrap());
    let table = Table {
        cols: vec![200.0],
        rows: vec![Row { height: 60.0, cells: vec![Cell { text: display, data: Some(data), ..Cell::new("") }] }],
        style: "none".into(),
        first_row: false,
        band_row: false,
        ..Default::default()
    };
    let mut p = defaults::blank_presentation(Size::new(400.0, 240.0), Theme::default(), false);
    p.slides.push(Arc::new(Slide {
        id: SlideId(50),
        layout: defaults::layout_of_kind(&p, LayoutType::Blank).unwrap(),
        show_master_shapes: false,
        shapes: vec![Shape { id: ShapeId(51), xfrm: Some(Xfrm::new(20.0, 20.0, 200.0, 60.0)), kind: ShapeKind::Table(table), ..Default::default() }],
        ..Default::default()
    }));
    p
}

#[test]
fn explicit_strings_differing_equal_and_empty_keep_stored_ownership() {
    for stored in ["stored", "shown", ""] {
        let p = import(&source(&format!("office:value-type='string' office:string-value='{stored}'"), "<text:p>shown</text:p>")).unwrap();
        let bytes = assert_reopens(&p, &CellValue::String { value: stored.into() }, "shown");
        assert!(part(&bytes, "content.xml").contains(&format!("office:string-value=\"{stored}\"")));
    }
    for attrs in ["", "office:value-type='string'"] {
        let p = import(&source(attrs, "<text:p>123</text:p>")).unwrap();
        no_warnings(&p);
        assert!(first(&p).data.is_none(), "plain text must not acquire numeric or stored string ownership");
        let bytes = export(&p).unwrap();
        assert!(!part(&bytes, "content.xml").contains("office:string-value="));
        assert!(first(&import(&bytes).unwrap()).data.is_none());
    }
}

#[test]
fn stored_attribute_whitespace_and_xml_metacharacters_are_independent_of_display() {
    let stored = "\tstart\nline\rnext & < > \" ' 終";
    let p = native(CellValue::String { value: stored.into() }, TextBody::from_text("display"));
    let bytes = assert_reopens(&p, &CellValue::String { value: stored.into() }, "display");
    let xml = part(&bytes, "content.xml");
    for escaped in ["&#x9;", "&#xA;", "&#xD;", "&amp;", "&lt;", "&quot;"] {
        assert!(xml.contains(escaped), "missing {escaped}: {xml}");
    }
}

#[test]
fn all_xml_boolean_spellings_write_canonical_booleans_without_display_parsing() {
    for (lexical, expected) in [("true", true), ("1", true), ("false", false), ("0", false)] {
        let p = import(&source(&format!("office:value-type='boolean' office:boolean-value='{lexical}'"), "<text:p>Custom label</text:p>")).unwrap();
        let bytes = assert_reopens(&p, &CellValue::Boolean { value: expected }, "Custom label");
        assert!(part(&bytes, "content.xml").contains(&format!("office:boolean-value=\"{expected}\"")));
    }
}

#[test]
fn numeric_precision_exponents_and_signed_zero_are_not_reformatted_through_f64() {
    for lexical in ["9007199254740993", "+001.2300E+02", "-0", "-0.000e-99", ".5", "1.", "4.9406564584124654e-324", "1.7976931348623157e308"] {
        let p = import(&source(&format!("office:value-type='float' office:value='{lexical}'"), "<text:p>cached label</text:p>")).unwrap();
        let expected = CellValue::Number { kind: CellNumberKind::Float, lexical: lexical.into() };
        let bytes = assert_reopens(&p, &expected, "cached label");
        assert!(part(&bytes, "content.xml").contains(&format!("office:value=\"{lexical}\"")));
    }
    let p = import(&source("office:value-type='float' office:value=' &#x9;+1.2300E+02&#xA; '", "<text:p>label</text:p>")).unwrap();
    assert_reopens(&p, &CellValue::Number { kind: CellNumberKind::Float, lexical: "+1.2300E+02".into() }, "label");
}

#[test]
fn percentage_and_currency_keep_the_source_number_separate_from_cached_display() {
    let p = import(&source("office:value-type='percentage' office:value='0.5'", "<text:p>50%</text:p>")).unwrap();
    assert_reopens(&p, &CellValue::Number { kind: CellNumberKind::Percentage, lexical: "0.5".into() }, "50%");
    for currency in [None, Some(""), Some("USD"), Some("通貨"), Some("  ¤\t\n\r ")] {
        let expected = CellValue::Number { kind: CellNumberKind::Currency { currency: currency.map(str::to_owned) }, lexical: "0002.500".into() };
        let p = native(expected.clone(), TextBody::from_text("special price"));
        let bytes = assert_reopens(&p, &expected, "special price");
        let xml = part(&bytes, "content.xml");
        assert_eq!(xml.contains("office:currency="), currency.is_some());
        assert!(xml.contains("office:value=\"0002.500\""));
    }
}

#[test]
fn absent_paragraphs_use_existing_source_cache_fallback_without_inventing_formatting() {
    for (attrs, expected, display) in [
        ("office:value-type='string' office:string-value='stored'", CellValue::String { value: "stored".into() }, "stored"),
        ("office:value-type='string' office:string-value=''", CellValue::String { value: String::new() }, ""),
        ("office:value-type='boolean' office:boolean-value='1'", CellValue::Boolean { value: true }, "1"),
        ("office:value-type='percentage' office:value='0.5'", CellValue::Number { kind: CellNumberKind::Percentage, lexical: "0.5".into() }, "0.5"),
    ] {
        let p = import(&source(attrs, "")).unwrap();
        assert_reopens(&p, &expected, display);
    }
}

#[test]
fn rich_display_and_format_only_changes_preserve_current_data() {
    let expected = CellValue::Number { kind: CellNumberKind::Percentage, lexical: "0.5".into() };
    let display = TextBody {
        paragraphs: vec![
            Paragraph {
                runs: vec![
                    Run::new("50"),
                    Run::with("%", RunProps { bold: Some(true), ..Default::default() }),
                    Run { kind: RunKind::Break, ..Run::new("") },
                    Run::new("label"),
                ],
                ..Default::default()
            },
            Paragraph { runs: vec![Run::new("second paragraph")], ..Default::default() },
        ],
        ..Default::default()
    };
    let mut p = native(expected.clone(), display);
    let data = first(&p).data.as_ref().unwrap().clone();
    let cell = &mut table_mut(&mut p).rows[0].cells[0];
    cell.text.paragraphs[0].runs[0].props.italic = Some(true);
    cell.text.paragraphs[0].props.margin_left = Some(12.0);
    cell.margins = Some([1.0, 2.0, 3.0, 4.0]);
    assert!(matches!(cell.constant_state().unwrap(), CellConstantState::Current(_)));
    assert!(Arc::ptr_eq(cell.data.as_ref().unwrap(), &data));
    let bytes = export(&p).unwrap();
    let q = import(&bytes).unwrap();
    no_warnings(&q);
    assert_eq!(value(&q), &expected);
    assert_eq!(first(&q).text.text(), first(&p).text.text());
    let first_para = &first(&q).text.paragraphs[0];
    assert_eq!(first_para.runs[0].props.italic, Some(true));
    assert!(first_para.runs.iter().any(|run| run.props.bold == Some(true)));
    assert!(first_para.runs.iter().any(|run| run.kind == RunKind::Break));
    assert_eq!(first(&q).text.paragraphs.len(), 2);
    assert_eq!(first_para.props.margin_left, Some(12.0));
    assert_eq!(first(&q).margins, Some([1.0, 2.0, 3.0, 4.0]));
}

#[test]
fn direct_stale_display_edits_lower_to_text_without_rebinding_or_mutating_caller() {
    let mut p = native(CellValue::Boolean { value: false }, TextBody::from_text("before"));
    table_mut(&mut p).rows[0].cells[0].text = TextBody::from_text("after");
    assert_eq!(first(&p).constant_state().unwrap(), CellConstantState::Stale);
    let bytes = export(&p).unwrap();
    let xml = part(&bytes, "content.xml");
    assert!(!xml.contains("office:boolean-value="));
    assert!(xml.contains("office:value-type=\"string\""));
    let q = import(&bytes).unwrap();
    no_warnings(&q);
    assert!(first(&q).data.is_none());
    assert_eq!(first(&q).text.text(), "after");
    assert!(first(&p).data.is_some(), "export must not mutate its caller");
    let q = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    assert!(first(&q).data.is_none());
    assert_eq!(first(&q).text.text(), "after");
}

fn guarded(bytes: &[u8], expected: &str) -> Presentation {
    let p = import(bytes).unwrap();
    assert!(p.tags.iter().any(|(key, message)| key == "odp:warning" && message.starts_with(expected)), "{:?}", p.tags);
    assert_eq!(p.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
    assert!(export(&p).is_err(), "unrepresented table semantics must remain guarded");
    p
}

#[test]
fn invalid_missing_mixed_and_extraneous_scalar_attributes_are_not_reinterpreted() {
    for attrs in [
        "office:value='1'",
        "office:string-value=''",
        "office:currency='USD'",
        "office:value-type='float'",
        "office:value-type='boolean'",
        "office:value-type='boolean' office:boolean-value='TRUE'",
        "office:value-type='boolean' office:boolean-value='2'",
        "office:value-type='string' office:string-value='text' office:value='1'",
        "office:value-type='float' office:value='1' office:currency='USD'",
        "office:value-type='currency' office:value='1' office:boolean-value='false'",
        "office:value-type='float' office:value='1' office:date-value='2026-10-15'",
        "office:value-type='date' office:date-value='2026-02-30'",
        "office:value-type='time' office:time-value='PT1S' office:date-value='2026-10-15'",
        "office:value-type='void'",
        "office:value-type='unknown'",
    ] {
        let p = guarded(&source(attrs, "<text:p>display</text:p>"), VALUES);
        assert!(first(&p).data.is_none(), "unsupported scalar must not receive an editable constant: {attrs}");
        assert_eq!(first(&p).text.text(), "display");
    }
    for lexical in ["NaN", "INF", "-INF", "1e309", "1e-9999", "0x10", "1,5", "1e+", ".", "+", "", "１２"] {
        let p = guarded(&source(&format!("office:value-type='float' office:value='{lexical}'"), "<text:p>display</text:p>"), VALUES);
        assert!(first(&p).data.is_none(), "{lexical}");
    }
}

#[test]
fn fields_math_unsupported_formulas_and_formats_keep_guards_while_nested_values_roundtrip() {
    for content in ["<text:p><text:page-number>1</text:page-number></text:p>", "<text:p><math:math><math:mi>x</math:mi></math:math></text:p>"] {
        let p = guarded(&source("office:value-type='float' office:value='1'", content), VALUES);
        assert!(first(&p).data.is_none());
    }
    let supported = import(&source("office:value-type='float' office:value='2' table:formula='of:=1+1'", "<text:p>2</text:p>")).unwrap();
    no_warnings(&supported);
    assert!(first(&supported).data.is_none(), "formula caches are never independent constants");
    assert_eq!(first(&supported).formula.as_ref().unwrap().expression(), "=1+1");
    let reopened = import(&export(&supported).unwrap()).unwrap();
    assert_eq!(first(&reopened).formula, first(&supported).formula);
    for extra in ["table:formula='of:=4/2'", "table:number-matrix-columns-spanned='1'", "table:number-matrix-rows-spanned='1'"] {
        let p = guarded(&source(&format!("office:value-type='float' office:value='2' {extra}"), "<text:p>2</text:p>"), "ODF cell formulas");
        assert!(first(&p).data.is_none());
    }
    let styles = "<number:number-style style:name='format'><number:number number:decimal-places='2'/></number:number-style><style:style style:name='N' style:family='table-cell' style:data-style-name='format'/>";
    guarded(
        &package(
            styles,
            &frame(
                "<table:table-row><table:table-cell table:style-name='N' office:value-type='float' office:value='1'><text:p>1</text:p></table:table-cell></table:table-row>",
            ),
        ),
        VALUES,
    );
    let bytes = source(
        "office:value-type='float' office:value='1'",
        "<text:p>outer label</text:p><table:table><table:table-row><table:table-cell><text:p>nested</text:p></table:table-cell></table:table-row></table:table><text:p>tail</text:p>",
    );
    let p = import(&bytes).unwrap();
    let check = |p: &Presentation| {
        no_warnings(p);
        assert_eq!(value(p), &CellValue::Number { kind: CellNumberKind::Float, lexical: "1".into() });
        assert_eq!(first(p).text.text(), "outer label");
        assert_eq!(first(p).blocks.len(), 2);
        let CellBlock::Table(child) = &first(p).blocks[0] else { panic!("nested owner") };
        assert_eq!(child.table.cell(0, 0).unwrap().text.text(), "nested");
        assert!(child.table.cell(0, 0).unwrap().data.is_none());
        assert!(matches!(&first(p).blocks[1], CellBlock::Text(tail) if tail.body.text() == "tail"));
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
}

#[test]
fn explicit_and_span_implied_covered_values_never_attach_hidden_constants() {
    let hidden = "office:value-type='float' office:value='7'";
    for covered in
        [format!("<table:covered-table-cell {hidden}><text:p>hidden</text:p></table:covered-table-cell>"), cell(hidden, "<text:p>hidden</text:p>")]
    {
        let rows = format!("<table:table-row>{}{covered}</table:table-row>", cell("table:number-columns-spanned='2'", "<text:p>anchor</text:p>"));
        let p = guarded(&package("", &frame(&rows)), VALUES);
        assert!(table(&p).rows[0].cells[1].is_covered());
        assert!(table(&p).rows[0].cells[1].data.is_none());
    }
    let rows = format!(
        "<table:table-row>{}</table:table-row><table:table-row>{}</table:table-row>",
        cell("table:number-rows-spanned='2'", ""),
        cell(hidden, "")
    );
    let p = guarded(&package("", &frame(&rows)), VALUES);
    assert!(table(&p).rows[1].cells[0].v_merge);
    assert!(table(&p).rows[1].cells[0].data.is_none());
}

#[test]
fn repeated_constant_cells_preserve_value_and_display_without_rebinding() {
    let rows = "<table:table-row table:number-rows-repeated='3'><table:table-cell table:number-columns-repeated='4' office:value-type='float' office:value='9007199254740993'><text:p>cached</text:p></table:table-cell></table:table-row>";
    let p = import(&package("", &frame(rows))).unwrap();
    no_warnings(&p);
    assert_eq!(table(&p).rows.len(), 3);
    for row in &table(&p).rows {
        assert_eq!(row.cells.len(), 4);
        for cell in &row.cells {
            assert_eq!(cell.text.text(), "cached");
            assert_eq!(cell.data.as_ref().unwrap().value(), &CellValue::Number { kind: CellNumberKind::Float, lexical: "9007199254740993".into() });
        }
    }
    let q = import(&export(&p).unwrap()).unwrap();
    no_warnings(&q);
    assert_eq!(table(&q).rows.len(), 3);
    assert!(table(&q).rows.iter().all(|row| row.cells.iter().all(|cell| cell.data.is_some())));
}

#[test]
fn metadata_is_charged_for_every_repeat_and_cumulatively_across_tables() {
    let stored = "x".repeat(8192);
    let row = |columns, rows| {
        format!(
            "<table:table-row table:number-rows-repeated='{rows}'><table:table-cell table:number-columns-repeated='{columns}' office:value-type='string' office:string-value='{stored}'><text:p>x</text:p></table:table-cell></table:table-row>"
        )
    };
    let p = import(&package("", &frame(&row(10, 10)))).unwrap();
    no_warnings(&p);
    assert_eq!(table(&p).rows.len(), 10);
    // Expanded-tree preflight runs before row/cell cloning and can reject the
    // repeated allocation before the later rich-text attachment accounting.
    let error = import(&package("", &frame(&row(100, 1).repeat(100)))).unwrap_err();
    assert!(
        matches!(error.0.as_str(), "expanded table text exceeds limit" | "expanded nested table allocation exceeds limit"),
        "unexpected bounded-import refusal: {error}"
    );
    // Reuse the independently successful 100-cell table above. None of these
    // small tables can trip its own budget; their aggregate must trip import's.
    let frames = frame(&row(10, 10)).repeat(100);
    let error = import(&package("", &frames)).unwrap_err();
    assert!(
        matches!(error.0.as_str(), "expanded table content exceeds total size limit" | "expanded nested table allocation exceeds total limit"),
        "unexpected aggregate refusal: {error}"
    );

    // Compare observable repeat thresholds instead of assuming Rust's evolving
    // rich Cell/Run/Paragraph struct sizes. Additional stored metadata must make
    // typed cells reach the same table limit sooner than identical plain display.
    let first_refused_rows = |typed| {
        (1..=100)
            .find(|rows| {
                let row = if typed {
                    row(100, *rows)
                } else {
                    format!("<table:table-row table:number-rows-repeated='{rows}'><table:table-cell table:number-columns-repeated='100' office:value-type='string'><text:p>x</text:p></table:table-cell></table:table-row>")
                };
                match import(&package("", &frame(&row))) {
                    Ok(p) => {
                        no_warnings(&p);
                        false
                    }
                    Err(error) => {
                        assert!(matches!(error.0.as_str(), "expanded table text exceeds limit" | "expanded nested table allocation exceeds limit"), "unexpected bounded-import refusal: {error}");
                        true
                    }
                }
            })
            .expect("bounded repeated cells must reach the per-table budget")
    };
    assert!(first_refused_rows(true) < first_refused_rows(false), "stored metadata must be charged for each repeated attachment");
}

#[test]
fn odp_attribute_boundary_refuses_unreopenable_native_strings_without_losing_native_data() {
    for bytes in [8192, 8193] {
        let expected = CellValue::String { value: "x".repeat(bytes) };
        let p = native(expected.clone(), TextBody::from_text("display"));
        let q = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        assert_eq!(value(&q), &expected);
        if bytes == 8192 {
            assert_reopens(&p, &expected, "display");
        } else {
            assert!(export(&p).unwrap_err().0.contains("ODP XML attribute size limit"));
            assert_eq!(value(&p), &expected);
            let error =
                import(&source(&format!("office:value-type='string' office:string-value='{}'", "x".repeat(bytes)), "<text:p>display</text:p>"))
                    .unwrap_err();
            assert_eq!(error.0, "XML attributes exceed size limit");
        }
    }
    // The reader's bound is decoded UTF-8 bytes, not XML escape length or glyphs.
    for stored in ["\t".repeat(8192), format!("{}ab", "終".repeat(2730))] {
        assert_eq!(stored.len(), 8192);
        let expected = CellValue::String { value: stored };
        let p = native(expected.clone(), TextBody::from_text("display"));
        assert_reopens(&p, &expected, "display");
    }
}

#[test]
fn constructor_rejects_invalid_xml_characters_and_oversized_metadata() {
    for value in [
        CellValue::String { value: "bad\u{1}".into() },
        CellValue::String { value: "x".repeat(64 * 1024 + 1) },
        CellValue::Number { kind: CellNumberKind::Currency { currency: Some("x".repeat(65)) }, lexical: "1".into() },
        CellValue::Number { kind: CellNumberKind::Currency { currency: Some("bad\u{1}".into()) }, lexical: "1".into() },
        CellValue::Number { kind: CellNumberKind::Float, lexical: "1".repeat(129) },
    ] {
        assert!(CellConstant::from_display(value, &TextBody::from_text("display")).is_err());
    }
    for kind in [RunKind::Field { field: "datetime".into() }, RunKind::Math { omml: "<m:oMath/>".into() }] {
        let body =
            TextBody { paragraphs: vec![Paragraph { runs: vec![Run { kind, ..Run::new("cached") }], ..Default::default() }], ..Default::default() };
        assert!(CellConstant::from_display(CellValue::Boolean { value: false }, &body).is_err());
    }
}

#[test]
fn generated_typed_constant_xml_can_be_independently_schema_validated() {
    let values = [
        CellValue::String { value: "stored\t\n\r & < > \" 終".into() },
        CellValue::Boolean { value: false },
        CellValue::Number { kind: CellNumberKind::Float, lexical: "9007199254740993".into() },
        CellValue::Number { kind: CellNumberKind::Percentage, lexical: "0.50".into() },
        CellValue::Number { kind: CellNumberKind::Currency { currency: None }, lexical: "-0.00".into() },
        CellValue::Number { kind: CellNumberKind::Currency { currency: Some(String::new()) }, lexical: "1e+02".into() },
        CellValue::Number { kind: CellNumberKind::Currency { currency: Some("通貨".into()) }, lexical: "+2.5".into() },
    ];
    let mut p = native(values[0].clone(), TextBody::from_text("cached display"));
    let typed_table = table_mut(&mut p);
    typed_table.cols = vec![200.0 / values.len() as f64; values.len()];
    typed_table.rows[0].cells = values
        .iter()
        .map(|value| {
            let mut cell = Cell::new("cached display");
            cell.data = Some(Arc::new(CellConstant::from_display(value.clone(), &cell.text).unwrap()));
            cell
        })
        .collect();
    let bytes = export(&p).unwrap();
    let q = import(&bytes).unwrap();
    no_warnings(&q);
    for (cell, expected) in table(&q).rows[0].cells.iter().zip(&values) {
        assert_eq!(cell.data.as_ref().unwrap().value(), expected);
        assert_eq!(cell.text.text(), "cached display");
    }
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_TABLE_VALUE_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml", "META-INF/manifest.xml"] {
            let file = std::path::Path::new(&dir).join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, part(&bytes, name)).unwrap();
        }
    }
}
