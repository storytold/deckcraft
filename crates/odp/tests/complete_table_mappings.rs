//! Contributor-original, independently expected table interoperability fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{table::*, *};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:number='urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:of='urn:oasis:names:tc:opendocument:xmlns:of:1.2' xmlns:v2='urn:deckcraft:formula:presentation-decimal-v2:1'";
fn simple(cells: Vec<Cell>) -> Table {
    Table {
        cols: vec![120.0; cells.len()],
        rows: vec![Row { height: 90.0, cells }],
        style: "none".into(),
        first_row: false,
        band_row: false,
        ..Default::default()
    }
}
fn deck(t: Table) -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(700.0, 400.0), Theme::default(), false);
    p.slides.push(Arc::new(Slide {
        id: SlideId(50),
        layout: defaults::layout_of_kind(&p, LayoutType::Blank).unwrap(),
        show_master_shapes: false,
        shapes: vec![Shape {
            id: ShapeId(51),
            xfrm: Some(Xfrm::new(20.0, 20.0, t.width(), t.height())),
            kind: ShapeKind::Table(t),
            ..Default::default()
        }],
        ..Default::default()
    }));
    p
}
fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(t) = &p.slides[0].shapes[0].kind else { panic!("table missing") };
    t
}
fn clean(p: &Presentation) {
    assert!(!p.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", p.tags)
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}
fn rewrite(bytes: &[u8], mut edit: impl FnMut(&str, String) -> String) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut file = source.by_index(i).unwrap();
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        out.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
        if name.ends_with(".xml") {
            out.write_all(edit(&name, String::from_utf8(data).unwrap()).as_bytes()).unwrap()
        } else {
            out.write_all(&data).unwrap()
        }
    }
    out.finish().unwrap().into_inner()
}
fn source(styles: &str, rows: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation><draw:page><draw:frame svg:width='400pt' svg:height='200pt'><table:table>{rows}</table:table></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(text.as_bytes()).unwrap()
    }
    out.finish().unwrap().into_inner()
}
fn numeric(value: &str) -> CellValue {
    CellValue::Number { kind: CellNumberKind::Float, lexical: value.into() }
}
fn formatted(value: CellValue, format: NumberFormat, expected: &str) -> Cell {
    assert_eq!(format.format_value(&value).unwrap().text, expected);
    let text = TextBody::from_text(expected);
    let data = CellConstant::from_display(value, &text).unwrap();
    Cell { text, data: Some(Arc::new(data)), number_format: Some(Arc::new(format)), ..Cell::new("") }
}
fn number() -> NumberStyle {
    NumberStyle::Number { decimal_places: 2, min_decimal_places: 2, min_integer_digits: 1, grouping: true }
}
#[test]
fn every_native_format_family_has_real_standard_style_and_editable_roundtrip() {
    let date = vec![
        DateFormatPart::Year { long: true },
        DateFormatPart::Literal { text: "-".into() },
        DateFormatPart::Month { long: true, textual: false },
        DateFormatPart::Literal { text: "-".into() },
        DateFormatPart::Day { long: true },
    ];
    let time = vec![
        DateFormatPart::Hours { long: true, elapsed: true },
        DateFormatPart::Literal { text: ":".into() },
        DateFormatPart::Minutes { long: true },
        DateFormatPart::Literal { text: ":".into() },
        DateFormatPart::Seconds { long: true, decimal_places: 0 },
    ];
    let cases = vec![
        (numeric("123.4500"), NumberStyle::General, "123.45"),
        (numeric("1234.5"), number(), "1,234.50"),
        (numeric("0.125"), NumberStyle::Percentage { decimal_places: 1, min_decimal_places: 1, grouping: false }, "12.5%"),
        (
            numeric("12.3"),
            NumberStyle::Currency {
                decimal_places: 2,
                min_decimal_places: 2,
                grouping: false,
                code: "EUR".into(),
                symbol: "€".into(),
                symbol_after: true,
                space: true,
            },
            "12.30 €",
        ),
        (numeric("1234"), NumberStyle::Scientific { decimal_places: 2, min_exponent_digits: 2, exponent_sign: true }, "1.23E+03"),
        (numeric("1.5"), NumberStyle::Fraction { denominator_digits: 2, fixed_denominator: None }, "1 1/2"),
        (CellValue::Date { lexical: "2024-02-29".into() }, NumberStyle::DateTime { parts: date }, "2024-02-29"),
        (CellValue::Duration { lexical: "PT27H".into() }, NumberStyle::Duration { parts: time }, "27:00:00"),
        (CellValue::Boolean { value: true }, NumberStyle::Boolean { true_text: "yes".into(), false_text: "no".into() }, "yes"),
        (CellValue::String { value: "Own text".into() }, NumberStyle::Text, "Own text"),
    ];
    for (index, (value, style, expected)) in cases.into_iter().enumerate() {
        let cell = formatted(value.clone(), NumberFormat { style, ..Default::default() }, expected);
        let original = cell.number_format.clone();
        let bytes = export(&deck(simple(vec![cell]))).unwrap();
        emit_completion_package(&format!("number-format-{index}.odp"), &bytes);
        let xml = part(&bytes, "content.xml");
        assert!(xml.contains("style:data-style-name="));
        assert!(xml.contains("urn:deckcraft:table:number-format:1"));
        let p = import(&bytes).unwrap();
        clean(&p);
        let back = table(&p).cell(0, 0).unwrap();
        assert_eq!(back.number_format, original);
        assert_eq!(back.data.as_ref().unwrap().value(), &value);
        assert_eq!(back.text.text(), expected);
        clean(&import(&export(&p).unwrap()).unwrap());
    }
}
#[test]
fn conditional_colors_locales_zero_and_parentheses_survive_projection() {
    let format = NumberFormat {
        locale: NumberLocale::DeDe,
        style: number(),
        zero_display: Some("nil".into()),
        negative: NegativeDisplay::Parentheses,
        color: Some([5, 15, 25, 255]),
        sections: vec![NumberFormatSection {
            comparison: NumberComparison::Less,
            threshold: "-10".into(),
            style: number(),
            prefix: "low ".into(),
            suffix: String::new(),
            negative: NegativeDisplay::Parentheses,
            color: Some([190, 20, 30, 255]),
        }],
        ..Default::default()
    };
    for (value, expected) in [("-12.5", "(low 12,50)"), ("-2", "(2,00)"), ("0", "nil"), ("1234.5", "1.234,50")] {
        let bytes = export(&deck(simple(vec![formatted(numeric(value), format.clone(), expected)]))).unwrap();
        emit_completion_package(&format!("conditional-format-{value}.odp"), &bytes);
        let xml = part(&bytes, "content.xml");
        assert!(xml.contains("<style:map"));
        assert!(xml.contains("#be141e"));
        let p = import(&bytes).unwrap();
        clean(&p);
        assert_eq!(table(&p).cell(0, 0).unwrap().number_format.as_deref(), Some(&format));
    }
}
#[test]
fn foreign_standard_number_style_is_admitted_only_when_value_and_display_agree() {
    let styles = "<number:number-style style:name='N' number:language='en' number:country='US'><number:number number:decimal-places='2' number:min-integer-digits='1' number:grouping='true'/></number:number-style><style:style style:name='C' style:family='table-cell' style:data-style-name='N'/>";
    for (display, clean_expected) in [("1,234.50", true), ("1,234.51", false)] {
        let rows = format!(
            "<table:table-row><table:table-cell table:style-name='C' office:value-type='float' office:value='1234.5'><text:p>{display}</text:p></table:table-cell></table:table-row>"
        );
        let p = import(&source(styles, &rows)).unwrap();
        assert_eq!(table(&p).cell(0, 0).unwrap().number_format.is_some(), clean_expected);
        if clean_expected {
            clean(&p);
            clean(&import(&export(&p).unwrap()).unwrap())
        } else {
            assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
            assert!(export(&p).is_err())
        }
    }
}
#[test]
fn format_program_projection_and_current_display_are_independent_authorities() {
    let bytes = export(&deck(simple(vec![formatted(numeric("12.5"), NumberFormat { style: number(), ..Default::default() }, "12.50")]))).unwrap();
    for altered in [
        rewrite(&bytes, |_, xml| xml.replace("number:decimal-places=\"2\"", "number:decimal-places=\"3\"")),
        rewrite(&bytes, |_, xml| xml.replace("&quot;decimalPlaces&quot;:2", "&quot;decimalPlaces&quot;:3")),
        rewrite(&bytes, |_, xml| xml.replace("office:value=\"12.5\"", "office:value=\"13.5\"")),
        rewrite(&bytes, |_, xml| xml.replace(">12.50<", ">12.51<")),
    ] {
        assert_ne!(altered, bytes);
        let p = import(&altered).unwrap();
        assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", p.tags);
        assert!(export(&p).is_err());
    }
}
#[test]
fn ordered_blocks_empty_paragraphs_leading_tables_and_stable_ids_survive() {
    let mut cell = Cell { text: TextBody::default(), ..Default::default() };
    cell.blocks = vec![
        CellBlock::Table(NestedTable { id: 17, table: Box::new(simple(vec![Cell::new("inside")])) }),
        CellBlock::Text(CellTextBlock { id: 42, body: TextBody::from_text("after\n\ntail") }),
        CellBlock::Text(CellTextBlock { id: 43, body: TextBody::default() }),
        CellBlock::Table(NestedTable { id: 44, table: Box::new(simple(vec![Cell::new("last")])) }),
    ];
    let bytes = export(&deck(simple(vec![cell]))).unwrap();
    let p = import(&bytes).unwrap();
    clean(&p);
    let c = table(&p).cell(0, 0).unwrap();
    assert!(c.text.paragraphs.is_empty());
    assert_eq!(c.blocks.iter().map(CellBlock::id).collect::<Vec<_>>(), vec![17, 42, 43, 44]);
    let CellBlock::Text(t) = &c.blocks[1] else { panic!() };
    assert_eq!(t.body.text(), "after\n\ntail");
    assert_eq!(t.body.paragraphs.len(), 3);
    let CellBlock::Text(t) = &c.blocks[2] else { panic!() };
    assert!(t.body.paragraphs.is_empty());
    clean(&import(&export(&p).unwrap()).unwrap());
}
#[test]
fn direct_standard_source_order_becomes_editable_recursive_blocks() {
    let child = "<table:table><table:table-row><table:table-cell><text:p>child</text:p></table:table-cell></table:table-row></table:table>";
    let rows = format!(
        "<table:table-row><table:table-cell><text:p>before</text:p>{child}<text:p/><text:p>after</text:p>{child}<text:p>end</text:p></table:table-cell></table:table-row>"
    );
    let p = import(&source("", &rows)).unwrap();
    clean(&p);
    let c = table(&p).cell(0, 0).unwrap();
    assert_eq!(c.text.text(), "before");
    assert_eq!(c.blocks.len(), 4);
    let CellBlock::Text(t) = &c.blocks[1] else { panic!() };
    assert_eq!(t.body.text(), "\nafter");
    clean(&import(&export(&p).unwrap()).unwrap());
}
#[test]
fn depth_and_repeat_products_are_bounded_before_expansion() {
    let mut t = simple(vec![Cell::new("leaf")]);
    for _ in 0..MAX_NESTED_TABLE_DEPTH {
        let mut c = Cell { text: TextBody::default(), ..Default::default() };
        c.append_nested_table(t).unwrap();
        t = simple(vec![c]);
    }
    let bytes = export(&deck(t)).unwrap();
    let p = import(&bytes).unwrap();
    clean(&p);
    table(&p).validate_content().unwrap();
    let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    table(&native).validate_content().unwrap();
    let nested = "<table:table><table:table-row table:number-rows-repeated='1000'><table:table-cell table:number-columns-repeated='1000'><text:p>x</text:p></table:table-cell></table:table-row></table:table>";
    let rows = format!("<table:table-row table:number-rows-repeated='1000'><table:table-cell>{nested}</table:table-cell></table:table-row>");
    assert!(import(&source("", &rows)).is_err());
}
#[test]
fn v2_numeric_boolean_string_error_and_formatted_legacy_have_explicit_languages() {
    let mut t = simple(vec![Cell::new("0"); 5]);
    for (col, expression) in [(0, "=1/8"), (1, "=2>1"), (2, "=\"own\"&\" text\""), (3, "=1/0")] {
        t.set_cell_formula_v2(0, col, expression).unwrap();
    }
    t.set_cell_formula(0, 4, "=1+1").unwrap();
    let c = t.cell_mut(0, 4).unwrap();
    c.number_format = Some(Arc::new(NumberFormat { style: number(), ..Default::default() }));
    c.text = TextBody::from_text("2.00");
    c.formula = Some(Arc::new(c.formula.as_ref().unwrap().with_number_format_display(&c.text).unwrap()));
    let child = simple(vec![Cell::new("editable child")]);
    t.cell_mut(0, 0).unwrap().append_nested_table(child).unwrap();
    let bytes = export(&deck(t)).unwrap();
    let xml = part(&bytes, "content.xml");
    assert!(xml.contains("urn:deckcraft:formula:presentation-decimal-v2:1"));
    assert!(xml.contains("urn:deckcraft:formula:exact-decimal-v1-formatted:1"));
    let p = import(&bytes).unwrap();
    clean(&p);
    let t = table(&p);
    for (col, expected) in [(0, "0.125"), (1, "TRUE"), (2, "own text"), (3, "#DIV/0!"), (4, "2.00")] {
        assert_eq!(t.cell(0, col).unwrap().text.text(), expected);
        assert!(t.cell(0, col).unwrap().formula.is_some())
    }
    assert_eq!(t.cell(0, 0).unwrap().blocks.len(), 1);
    assert_eq!(t.cell(0, 4).unwrap().formula.as_ref().unwrap().profile(), FORMULA_PROFILE_V1_FORMATTED);
}
#[test]
fn repeated_formulas_keep_identical_reference_tokens_at_every_owner() {
    let rows = "<table:table-row><table:table-cell office:value-type='float' office:value='2'><text:p>2</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated='2'><table:table-cell table:number-columns-repeated='2' table:formula='v2:=[.A1]+[.$A$1]' office:value-type='float' office:value='4'><text:p>4</text:p></table:table-cell></table:table-row>";
    let p = import(&source("", rows)).unwrap();
    clean(&p);
    for r in 1..3 {
        for c in 0..2 {
            let f = table(&p).cell(r, c).unwrap().formula.as_ref().unwrap();
            assert_eq!(f.expression(), "=[.A1]+[.$A$1]");
            assert_eq!(f.outcome(), &FormulaOutcome::Number { lexical: "4".into() });
        }
    }
    clean(&import(&export(&p).unwrap()).unwrap());
}
#[test]
fn foreign_openformula_broad_semantics_never_gain_our_v2_profile() {
    for formula in ["of:=1/8", "v2:=1/8"] {
        let rows = format!(
            "<table:table-row><table:table-cell table:formula='{formula}' office:value-type='float' office:value='0.125'><text:p>0.125</text:p></table:table-cell></table:table-row>"
        );
        let p = import(&source("", &rows)).unwrap();
        assert_eq!(table(&p).cell(0, 0).unwrap().formula.is_some(), formula.starts_with("v2:"));
    }
}

#[test]
fn v2_forged_cache_and_display_reject_complete_graph_without_rebinding() {
    for (cache, display) in [("0.126", "0.126"), ("0.125", "0.126")] {
        let rows = format!(
            "<table:table-row><table:table-cell table:formula='v2:=1/8' office:value-type='float' office:value='{cache}'><text:p>{display}</text:p></table:table-cell><table:table-cell table:formula='v2:=[.A1]+1' office:value-type='float' office:value='1.125'><text:p>1.125</text:p></table:table-cell></table:table-row>"
        );
        let p = import(&source("", &rows)).unwrap();
        assert!(table(&p).rows.iter().flat_map(|r| &r.cells).all(|c| c.formula.is_none()));
        assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
        assert!(export(&p).is_err());
    }
}
#[test]
fn strings_that_spell_errors_and_empty_strings_keep_their_actual_type() {
    let mut t = simple(vec![Cell::new("0"); 3]);
    for (col, expr) in [(0, "=\"#NUM!\""), (1, "=1/0"), (2, "=\"\"")] {
        t.set_cell_formula_v2(0, col, expr).unwrap();
    }
    let p = import(&export(&deck(t)).unwrap()).unwrap();
    clean(&p);
    assert_eq!(table(&p).cell(0, 0).unwrap().formula.as_ref().unwrap().outcome(), &FormulaOutcome::String { value: "#NUM!".into() });
    assert_eq!(table(&p).cell(0, 1).unwrap().formula.as_ref().unwrap().outcome(), &FormulaOutcome::Error { code: FormulaError::DivisionByZero });
    assert_eq!(table(&p).cell(0, 2).unwrap().formula.as_ref().unwrap().outcome(), &FormulaOutcome::String { value: String::new() });
}
#[test]
fn nested_unsupported_formula_warning_cannot_be_erased_by_valid_parent_graph() {
    let nested = "<table:table><table:table-row><table:table-cell table:formula='of:=1/8' office:value-type='float' office:value='0.125'><text:p>0.125</text:p></table:table-cell></table:table-row></table:table>";
    let rows = format!(
        "<table:table-row><table:table-cell table:formula='v2:=1+1' office:value-type='float' office:value='2'><text:p>2</text:p>{nested}</table:table-cell></table:table-row>"
    );
    let p = import(&source("", &rows)).unwrap();
    assert!(table(&p).cell(0, 0).unwrap().formula.is_some());
    assert!(p.tags.iter().any(|(k, v)| k == "odp:warning" && v.contains("cell formulas")));
    assert!(export(&p).is_err());
}
#[test]
fn repeated_coordinate_dependent_formula_cache_is_checked_at_each_owner() {
    let rows = "<table:table-row table:number-rows-repeated='2'><table:table-cell table:formula='v2:=ROW()' office:value-type='float' office:value='1'><text:p>1</text:p></table:table-cell></table:table-row>";
    let p = import(&source("", rows)).unwrap();
    assert!(table(&p).rows.iter().all(|r| r.cells[0].formula.is_none()));
    assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
}
#[test]
fn format_extension_lookalikes_extra_fields_and_auxiliary_projection_edits_are_guarded() {
    let format = NumberFormat {
        style: number(),
        sections: vec![NumberFormatSection {
            comparison: NumberComparison::Less,
            threshold: "0".into(),
            style: number(),
            prefix: String::new(),
            suffix: String::new(),
            negative: NegativeDisplay::Minus,
            color: Some([180, 20, 40, 255]),
        }],
        ..Default::default()
    };
    let bytes = export(&deck(simple(vec![formatted(numeric("-2"), format, "-2.00")]))).unwrap();
    for changed in [
        rewrite(&bytes, |_, x| x.replace("urn:deckcraft:table:number-format:1", "urn:foreign:number-format:1")),
        rewrite(&bytes, |_, x| x.replace(" dcfmt:program=", " dcfmt:future='1' dcfmt:program=")),
        rewrite(&bytes, |_, x| x.replace("#b41428", "#001122")),
    ] {
        let p = import(&changed).unwrap();
        assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
        assert!(export(&p).is_err());
    }
}

fn emit_completion_package(name: &str, bytes: &[u8]) {
    if let Ok(directory) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}
