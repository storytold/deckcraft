//! Independent contributor-original ODF fixtures; no application/vendor files or assets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{
    table::{CellBlock, CellConstantState, CellNumberKind, CellValue},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{io::Cursor, io::Write, sync::Arc};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";

fn fixture(styles: &str, columns: &str, rows: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page draw:name='Synthetic'><draw:frame svg:width='300pt' svg:height='150pt'><table:table>{columns}{rows}</table:table></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!("<office:document-styles {NS} office:version='1.3'><office:styles>{styles}</office:styles></office:document-styles>");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("styles.xml", styles.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(value.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("expected editable table") };
    table
}

fn table_mut(p: &mut Presentation) -> &mut Table {
    let ShapeKind::Table(table) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("expected editable table") };
    table
}

fn paint(r: u8, g: u8, b: u8) -> Option<Fill> {
    Some(Fill::solid(ColorRef::rgb(Rgba::rgb(r, g, b))))
}

fn warning(p: &Presentation, fragment: &str) -> bool {
    p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains(fragment))
}

#[test]
fn selected_cell_geometry_is_independent_of_paragraph_bags_and_xml_child_order() {
    // ODF property bags remain separate even when they reuse the same FO attribute.
    // Paragraph borders/padding are unsupported, and must still produce their warning.
    let cell = "<style:table-cell-properties fo:background-color='#123456' fo:padding='2pt' fo:border='1pt solid #2468AC'/>";
    let paragraph = "<style:paragraph-properties fo:background-color='#ABCDEF' fo:padding='9pt' fo:border='7pt solid #FEDCBA'/>";
    for bags in [format!("{cell}{paragraph}"), format!("{paragraph}{cell}")] {
        let styles = format!("<style:style style:name='Separated' style:family='table-cell'>{bags}</style:style>");
        for (columns, row_default, cell_style) in [
            ("<table:table-column table:default-cell-style-name='Separated'/>", "", ""),
            ("", "table:default-cell-style-name='Separated'", ""),
            ("", "", "table:style-name='Separated'"),
        ] {
            let rows = format!(
                "<table:table-row {row_default}><table:table-cell {cell_style}><text:p>separate areas</text:p></table:table-cell></table:table-row>"
            );
            let p = import(&fixture(&styles, columns, &rows)).unwrap();
            let cell = &table(&p).rows[0].cells[0];
            assert_eq!(cell.fill, paint(0x12, 0x34, 0x56));
            assert_eq!(cell.margins, Some([2.0; 4]), "paragraph padding must not become cell padding");
            for border in &cell.borders {
                let border = border.as_ref().unwrap();
                assert_eq!(border.width, Some(1.0), "paragraph borders must not become cell borders");
                assert_eq!(border.fill, paint(0x24, 0x68, 0xAC));
            }
            assert_eq!(cell.text.paragraphs[0].props.background, paint(0xAB, 0xCD, 0xEF));
            assert!(warning(&p, "paragraph padding, borders"));
            assert!(export(&p).is_err());
        }
    }
}

#[test]
fn overridden_ancestor_relief_border_keeps_source_only_loss_warning() {
    let styles = "<style:style style:name='Relief' style:family='table-cell'><style:table-cell-properties fo:border='2pt groove #123456'/></style:style><style:style style:name='Plain' style:family='table-cell' style:parent-style-name='Relief'><style:table-cell-properties fo:border='1pt solid #123456'/></style:style><style:style style:name='Explicit' style:family='table-cell'/>";
    // A declared default and its chain are source metadata even if all cells override it.
    let rows = "<table:table-row><table:table-cell table:style-name='Explicit'><text:p>safe display</text:p></table:table-cell></table:table-row>";
    let p = import(&fixture(styles, "<table:table-column table:default-cell-style-name='Plain'/>", rows)).unwrap();
    assert!(warning(&p, "relief borders"), "unsupported ancestor declaration must not disappear: {:?}", p.tags);
    assert!(export(&p).is_err());
}

#[test]
fn exact_value_and_rich_display_survive_different_noncontiguous_column_contexts() {
    let styles = "<style:style style:name='Large' style:family='table-cell'><style:text-properties fo:font-size='24pt'/></style:style><style:style style:name='Small' style:family='table-cell'><style:text-properties fo:font-size='10pt'/></style:style><style:style style:name='Emphasis' style:family='text'><style:text-properties fo:font-style='italic'/></style:style>";
    let columns = "<table:table-column table:default-cell-style-name='Large'/><table:table-column table:default-cell-style-name='Small'/><table:table-column table:default-cell-style-name='Large'/>";
    let rows = "<table:table-row table:number-rows-repeated='2'><table:table-cell table:number-columns-repeated='3' office:value-type='percentage' office:value='-0.000000000000000001e+2'><text:p>label<text:s text:c='2'/><text:span text:style-name='Emphasis'>kept</text:span><text:tab/>tab<text:line-break/>next</text:p><text:p/></table:table-cell></table:table-row>";
    let mut p = import(&fixture(styles, columns, rows)).unwrap();
    let expected = CellValue::Number { kind: CellNumberKind::Percentage, lexical: "-0.000000000000000001e+2".into() };
    for row in &table(&p).rows {
        for (cell, size) in row.cells.iter().zip([24.0, 10.0, 24.0]) {
            let data = cell.data.as_ref().unwrap();
            assert_eq!(data.value(), &expected);
            assert_eq!(data.display_baseline(), &["label  kept\ttab\u{b}next".to_owned(), String::new()]);
            assert!(matches!(cell.constant_state().unwrap(), CellConstantState::Current(_)));
            assert_eq!(cell.text.paragraphs[0].def_props.size, Some(size));
            assert!(cell.text.paragraphs[0].runs.iter().any(|run| run.text == "kept" && run.props.italic == Some(true)));
        }
    }
    // Repartitioning rich runs without changing content must not authenticate a new scalar.
    let data = table(&p).rows[0].cells[0].data.clone().unwrap();
    table_mut(&mut p).rows[0].cells[0].text.paragraphs[0].runs[0].props.bold = Some(true);
    assert!(Arc::ptr_eq(&data, table(&p).rows[0].cells[0].data.as_ref().unwrap()));
    let reopened = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    let back = import(&export(&reopened).unwrap()).unwrap();
    for row in &table(&back).rows {
        for cell in &row.cells {
            assert_eq!(cell.data.as_ref().unwrap().value(), &expected);
            assert_eq!(cell.data.as_ref().unwrap().display_baseline(), data.display_baseline());
        }
    }
}

#[test]
fn scalar_fallback_without_paragraphs_receives_each_selected_text_style() {
    let styles = "<style:style style:name='First' style:family='table-cell'><style:text-properties fo:font-size='29pt' fo:font-weight='bold'/></style:style><style:style style:name='Second' style:family='table-cell'><style:text-properties fo:font-size='13pt' fo:font-weight='normal'/></style:style>";
    let columns = "<table:table-column table:default-cell-style-name='First'/><table:table-column table:default-cell-style-name='Second'/>";
    let rows = "<table:table-row><table:table-cell table:number-columns-repeated='2' office:value-type='float' office:value='+00042.000'/></table:table-row>";
    let p = import(&fixture(styles, columns, rows)).unwrap();
    for (cell, (size, bold)) in table(&p).rows[0].cells.iter().zip([(29.0, true), (13.0, false)]) {
        assert_eq!(cell.text.text(), "+00042.000");
        assert_eq!(cell.text.paragraphs[0].runs[0].props.size, Some(size));
        assert_eq!(cell.text.paragraphs[0].runs[0].props.bold, Some(bold));
        assert!(matches!(cell.constant_state().unwrap(), CellConstantState::Current(_)));
        assert_eq!(cell.data.as_ref().unwrap().display_baseline(), &["+00042.000".to_owned()]);
    }
}

#[test]
fn final_unused_column_reference_is_strict_and_chain_limit_is_exact() {
    let rows = "<table:table-row><table:table-cell><text:p>only source cell</text:p></table:table-cell></table:table-row>";
    for count in [32, 33] {
        let mut styles = String::new();
        for index in 0..count {
            let parent = if index + 1 < count { format!("style:parent-style-name='S{}'", index + 1) } else { String::new() };
            styles.push_str(&format!("<style:style style:name='S{index}' style:family='table-cell' {parent}/>"));
        }
        let columns = "<table:table-column/><table:table-column table:default-cell-style-name='S0'/>";
        let result = import(&fixture(&styles, columns, rows));
        if count == 32 {
            assert!(result.is_ok(), "32 identities are allowed: {result:?}");
        } else {
            assert!(result.unwrap_err().0.contains("excessive table-cell"));
        }
    }
    let columns = "<table:table-column/><table:table-column table:default-cell-style-name='Missing'/>";
    assert!(import(&fixture("", columns, rows)).unwrap_err().0.contains("missing referenced ODF table-cell style"));
}

#[test]
fn alternating_column_styles_cannot_reset_expanded_payload_budget_per_run() {
    // Each individual style run is small, but all physical results are charged together.
    let styles = format!(
        "<style:style style:name='A' style:family='table-cell'><style:text-properties fo:language='{}'/></style:style><style:style style:name='B' style:family='table-cell'><style:text-properties fo:language='{}'/></style:style>",
        "a".repeat(1800),
        "b".repeat(1800)
    );
    let columns: String =
        (0..90).map(|index| format!("<table:table-column table:default-cell-style-name='{}'/>", if index % 2 == 0 { "A" } else { "B" })).collect();
    let rows = "<table:table-row table:number-rows-repeated='70'><table:table-cell table:number-columns-repeated='90'><text:p>small</text:p></table:table-cell></table:table-row>";
    let error = import(&fixture(&styles, &columns, rows)).unwrap_err().0;
    assert!(error.contains("expanded table"), "{error}");
}

#[test]
fn nested_table_columns_do_not_enter_outer_physical_index() {
    let styles = "<style:style style:name='OuterLeft' style:family='table-cell'><style:table-cell-properties fo:background-color='#112233'/></style:style><style:style style:name='OuterRight' style:family='table-cell'><style:table-cell-properties fo:background-color='#445566'/></style:style>";
    let columns = "<table:table-column table:default-cell-style-name='OuterLeft'/><table:table-column table:default-cell-style-name='OuterRight'/>";
    let rows = "<table:table-row><table:table-cell><text:p>outer</text:p><table:table><table:table-column table:number-columns-repeated='1000'/><table:table-row><table:table-cell><text:p>nested</text:p></table:table-cell></table:table-row></table:table><text:p>tail</text:p></table:table-cell><table:table-cell><text:p>right</text:p></table:table-cell></table:table-row>";
    let p = import(&fixture(styles, columns, rows)).unwrap();
    assert_eq!(table(&p).n_cols(), 2);
    assert_eq!(table(&p).n_rows(), 1);
    assert_eq!(table(&p).rows[0].cells[0].fill, paint(0x11, 0x22, 0x33));
    assert_eq!(table(&p).rows[0].cells[1].fill, paint(0x44, 0x55, 0x66));
    let check = |p: &Presentation| {
        assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"));
        let t = table(p);
        assert_eq!((t.n_rows(), t.n_cols()), (1, 2));
        assert_eq!(t.rows[0].cells[0].fill, paint(0x11, 0x22, 0x33));
        assert_eq!(t.rows[0].cells[1].fill, paint(0x44, 0x55, 0x66));
        assert_eq!(t.rows[0].cells[0].text.text(), "outer");
        let blocks = &t.rows[0].cells[0].blocks;
        assert_eq!(blocks.len(), 2);
        let CellBlock::Table(child) = &blocks[0] else { panic!("ordered nested table") };
        assert_eq!((child.table.n_rows(), child.table.n_cols()), (1, 1000));
        assert_eq!(child.table.rows[0].cells[0].text.text(), "nested");
        assert!(child.table.rows[0].cells[0].fill.is_none());
        assert!(matches!(&blocks[1], CellBlock::Text(tail) if tail.body.text() == "tail"));
    };
    check(&p);
    let original = p.clone();
    let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    check(&native);
    let back = import(&export(&native).unwrap()).unwrap();
    check(&back);
    check(&import(&export(&back).unwrap()).unwrap());
    assert!(p == original, "roundtrips must not mutate the imported source");
}

#[test]
fn covered_default_style_keeps_hidden_content_and_value_guards() {
    let styles = "<style:style style:name='Hidden' style:family='table-cell'><style:table-cell-properties fo:background-color='#667788'/><style:text-properties fo:font-size='18pt'/></style:style>";
    let columns = "<table:table-column/><table:table-column table:default-cell-style-name='Hidden'/>";
    let rows = "<table:table-row><table:table-cell table:number-columns-spanned='2'><text:p>visible</text:p></table:table-cell><table:covered-table-cell office:value-type='float' office:value='123'><text:p>hidden</text:p></table:covered-table-cell></table:table-row>";
    let p = import(&fixture(styles, columns, rows)).unwrap();
    let covered = &table(&p).rows[0].cells[1];
    assert!(covered.is_covered());
    assert_eq!(covered.fill, paint(0x66, 0x77, 0x88));
    assert_eq!(covered.text.paragraphs[0].def_props.size, Some(18.0));
    assert!(covered.data.is_none());
    assert!(warning(&p, "covered cell content"));
    assert!(warning(&p, "typed cell values"));
    assert!(export(&p).is_err());
}

#[test]
fn child_cell_shorthand_replaces_inherited_sides_before_local_side_overrides() {
    let styles = "<style:default-style style:family='table-cell'><style:table-cell-properties fo:padding-left='11pt' fo:border-right='8pt solid #112233'/></style:default-style><style:style style:name='Child' style:family='table-cell'><style:table-cell-properties fo:padding='2pt' fo:padding-bottom='3pt' fo:border='1pt solid #445566' fo:border-left='4pt solid #778899'/></style:style>";
    for (columns, rows) in [
        (
            "<table:table-column table:default-cell-style-name='Child'/>",
            "<table:table-row><table:table-cell><text:p>owned</text:p></table:table-cell></table:table-row>",
        ),
        ("", "<table:table-row table:default-cell-style-name='Child'><table:table-cell><text:p>owned</text:p></table:table-cell></table:table-row>"),
    ] {
        let p = import(&fixture(styles, columns, rows)).unwrap();
        let cell = &table(&p).rows[0].cells[0];
        assert_eq!(cell.margins, Some([2.0, 2.0, 2.0, 3.0]));
        for (side, width) in cell.borders.iter().zip([4.0, 1.0, 1.0, 1.0]) {
            assert_eq!(side.as_ref().unwrap().width, Some(width));
        }
    }
}
