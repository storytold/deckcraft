//! Contributor-original table semantic-loss guards. No external documents or assets.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_odp::{MIME, export, import};
use std::io::{Cursor, Write};

const VALUES: &str = "ODF typed cell values and number formats";
const FORMULAS: &str = "ODF cell formulas";
const COVERED: &str = "ODF covered cell content";
const MALFORMED_DEFAULT: &str = "ODF default cell style data-format metadata";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:number='urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0' xmlns:of='urn:oasis:names:tc:opendocument:xmlns:of:1.2'";
fn package(styles: &str, table: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page><draw:frame svg:width='400pt' svg:height='200pt'>{table}</draw:frame></draw:page></office:presentation></office:body></office:document-content>"
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
fn table(cells: &str) -> String {
    let anchor = if cells.starts_with("<table:covered-table-cell") { "<table:table-cell table:number-columns-spanned='2'/>" } else { "" };
    format!("<table:table><table:table-column/><table:table-row>{anchor}{cells}</table:table-row></table:table>")
}
fn cell(tag: &str, attrs: &str, content: &str) -> String {
    format!("<{tag} {attrs}>{content}</{tag}>")
}
fn guarded(styles: &str, table: &str, expected: &str) -> deckcraft_model::Presentation {
    let bytes = package(styles, table);
    let p = import(&bytes).unwrap();
    assert!(p.tags.iter().any(|(key, warning)| key == "odp:warning" && warning.starts_with(expected)), "missing {expected}: {:?}", p.tags);
    let Err(error) = export(&p) else { panic!("unrepresented table semantics were exportable") };
    assert!(error.to_string().contains(expected), "unexpected refusal: {error}");
    assert_eq!(p.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
    p
}

#[test]
fn unsupported_or_covered_stored_value_attributes_still_guard() {
    for tag in ["table:table-cell", "table:covered-table-cell"] {
        for attrs in [
            "office:string-value=''",
            "office:value='0'",
            "office:boolean-value='false'",
            "office:date-value='2026-10-15'",
            "office:time-value='PT0S'",
            "office:currency='USD'",
        ] {
            guarded("", &table(&cell(tag, attrs, "<text:p>shown</text:p>")), VALUES);
        }
    }
    // Ordinary explicit stored strings now have an editable owner, including
    // equal and empty values. Hidden covered values remain outside this slice.
    for value in ["shown", "stored", ""] {
        guarded(
            "",
            &table(&cell("table:covered-table-cell", &format!("office:string-value='{value}' office:value-type='string'"), "<text:p>shown</text:p>")),
            VALUES,
        );
    }
}

#[test]
fn non_string_and_unknown_value_types_guard_without_needing_a_value_attribute() {
    for tag in ["table:table-cell", "table:covered-table-cell"] {
        for value in ["boolean", "currency", "date", "float", "percentage", "time", "void", "unrecognized", ""] {
            guarded("", &table(&cell(tag, &format!("office:value-type='{value}'"), "")), VALUES);
        }
    }
}

#[test]
fn plain_text_only_strings_and_empty_covered_cells_remain_editable() {
    for count in [1, 3, 30] {
        for text in ["", "<text:p>Ordinary cell</text:p>"] {
            let cells = format!(
                "{}{}<table:covered-table-cell> \n\t </table:covered-table-cell>",
                cell("table:table-cell", "office:value-type='string'", text).repeat(count - 1),
                cell("table:table-cell", "office:value-type='string' table:number-columns-spanned='2'", text)
            );
            let p = import(&package("", &table(&cells))).unwrap();
            assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
            let back = import(&export(&p).unwrap()).unwrap();
            let deckcraft_model::ShapeKind::Table(t) = &back.slides[0].shapes[0].kind else { panic!("table missing") };
            assert_eq!(t.rows[0].cells.len(), count + 1);
            assert_eq!(t.rows[0].cells[0].text.text(), if text.is_empty() { "" } else { "Ordinary cell" });
        }
    }
}

#[test]
fn covered_paragraphs_lists_and_empty_paragraph_formatting_are_not_discarded_silently() {
    for content in [
        "<text:p>Hidden cell</text:p>",
        "<text:p/>",
        "<text:list><text:list-item><text:p>Hidden item</text:p></text:list-item></text:list>",
        "hidden invalid raw text",
    ] {
        guarded("", &table(&cell("table:covered-table-cell", "office:value-type='string'", content)), COVERED);
    }
}

#[test]
fn formulas_and_both_matrix_spans_guard_in_ordinary_and_covered_cells() {
    for tag in ["table:table-cell", "table:covered-table-cell"] {
        for attrs in ["table:formula='of:=1+1'", "table:number-matrix-columns-spanned='1'", "table:number-matrix-rows-spanned='1'"] {
            guarded("", &table(&cell(tag, attrs, "")), FORMULAS);
        }
    }
}

#[test]
fn named_and_parent_number_formats_guard_including_covered_cells() {
    let styles = "<number:number-style style:name='N'><number:number number:decimal-places='2'/></number:number-style><style:style style:name='Parent' style:family='table-cell' style:data-style-name='N'/><style:style style:name='Child' style:family='table-cell' style:parent-style-name='Parent'/>";
    for tag in ["table:table-cell", "table:covered-table-cell"] {
        for name in ["Parent", "Child"] {
            guarded(styles, &table(&cell(tag, &format!("table:style-name='{name}'"), "")), VALUES);
        }
    }
}

#[test]
fn row_and_column_default_cell_number_formats_remain_guarded() {
    let styles = "<number:number-style style:name='N'><number:number/></number:number-style><style:style style:name='C' style:family='table-cell' style:data-style-name='N'/>";
    for (columns, row_attrs) in [
        ("<table:table-column table:default-cell-style-name='C'/>", ""),
        ("<table:table-column/>", "table:default-cell-style-name='C'"),
        ("<table:table-column table:default-cell-style-name='C' table:number-columns-repeated='3'/>", "table:default-cell-style-name='C'"),
    ] {
        guarded(
            styles,
            &format!(
                "<table:table>{columns}<table:table-row {row_attrs}><table:table-cell office:value-type='string'/></table:table-row></table:table>"
            ),
            VALUES,
        );
    }
}

#[test]
fn malformed_default_style_data_format_is_defensively_guarded() {
    // Intentionally malformed: data-style-name belongs to named style:style, not default-style.
    guarded("<style:default-style style:family='table-cell' style:data-style-name='N'/>", &table("<table:table-cell/>"), MALFORMED_DEFAULT);
}

#[test]
fn nested_table_owners_and_bounded_warning_deduplication_are_preserved() {
    let bytes = package("", &table(&cell("table:table-cell", "", &table("<table:table-cell><text:p>Inner</text:p></table:table-cell>"))));
    let p = import(&bytes).unwrap();
    let check = |p: &deckcraft_model::Presentation| {
        assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"));
        let deckcraft_model::ShapeKind::Table(t) = &p.slides[0].shapes[0].kind else { panic!("outer table") };
        let owner = t.cell(0, 0).unwrap();
        assert!(owner.text.is_empty());
        assert_eq!(owner.blocks.len(), 1);
        let deckcraft_model::table::CellBlock::Table(child) = &owner.blocks[0] else { panic!("nested owner") };
        assert_eq!(child.table.cell(0, 0).unwrap().text.text(), "Inner");
        assert!(child.table.cell(0, 0).unwrap().blocks.is_empty());
    };
    check(&p);
    let original = p.clone();
    let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    check(&native);
    let back = import(&export(&native).unwrap()).unwrap();
    check(&back);
    check(&import(&export(&back).unwrap()).unwrap());
    assert!(p == original, "roundtrips must not mutate source owners");
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
    let cells = cell("table:table-cell", "office:value-type='date' office:date-value='PRIVATE VALUE'", "<text:p>shown</text:p>").repeat(100);
    let p = guarded("", &table(&cells), VALUES);
    assert_eq!(p.tags.iter().filter(|(key, warning)| key == "odp:warning" && warning.starts_with(VALUES)).count(), 1);
    assert!(!p.tags.iter().any(|(_, warning)| warning.contains("PRIVATE VALUE")));
}
