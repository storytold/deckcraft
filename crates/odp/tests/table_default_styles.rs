//! Contributor-original fixtures derived from OASIS ODF 1.3 Part 3
//! §§9.1.5, 19.619, 19.679–19.682, 19.730.4/.13. No vendor files/assets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{
    table::{CellConstantState, CellNumberKind, CellValue},
    text::{Anchor, TextDir},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:number='urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0' xmlns:foreign='urn:deckcraft:synthetic:foreign'";
const DEFAULTS: &str = "<style:default-style style:family='table-cell'><style:table-cell-properties fo:background-color='#101010' fo:padding='1pt' fo:border='0.5pt solid #112233'/><style:text-properties fo:font-family='Liberation Sans' fo:font-size='11pt' fo:font-weight='normal' fo:color='#334455'/></style:default-style>";
const STYLES: &str = "<style:style style:name='A' style:family='table-cell'><style:table-cell-properties fo:background-color='#AA2200' fo:padding='2pt' fo:border='2pt solid #AA2200'/><style:text-properties fo:font-size='14pt' fo:font-weight='bold'/></style:style><style:style style:name='B' style:family='table-cell'><style:table-cell-properties fo:background-color='#005599' style:vertical-align='bottom' style:writing-mode='tb-rl'/><style:text-properties fo:font-family='Liberation Serif' fo:font-size='15pt' fo:font-style='italic'/></style:style><style:style style:name='R' style:family='table-cell'><style:table-cell-properties fo:background-color='#227733' fo:padding='3pt' fo:border='3pt solid #227733' style:vertical-align='middle'/><style:text-properties fo:font-size='17pt'/></style:style><style:style style:name='E' style:family='table-cell'><style:table-cell-properties fo:background-color='transparent'/><style:text-properties fo:font-size='21pt'/></style:style>";

fn package(styles: &str, columns: &str, rows: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page draw:name='Original'><draw:frame draw:name='Table' svg:width='400pt' svg:height='200pt'><table:table table:name='Measurements'>{columns}{rows}</table:table></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
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
fn source(columns: &str, rows: &str) -> Vec<u8> {
    package(&format!("{DEFAULTS}{STYLES}"), columns, rows)
}
fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("editable table") };
    table
}
fn table_mut(p: &mut Presentation) -> &mut Table {
    let ShapeKind::Table(table) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("editable table") };
    table
}
fn no_warnings(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}
fn fill(rgb: [u8; 3]) -> Option<Fill> {
    Some(Fill::solid(ColorRef::rgb(Rgba::rgb(rgb[0], rgb[1], rgb[2]))))
}
fn assert_style(cell: &Cell, expected: &str) {
    let (paint, size, font, padding, border, bold) = match expected {
        "A" => (fill([170, 34, 0]), 14.0, "Liberation Sans", 2.0, 2.0, true),
        "B" => (fill([0, 85, 153]), 15.0, "Liberation Serif", 1.0, 0.5, false),
        "R" => (fill([34, 119, 51]), 17.0, "Liberation Sans", 3.0, 3.0, false),
        "E" => (Some(Fill::None), 21.0, "Liberation Sans", 1.0, 0.5, false),
        _ => (fill([16, 16, 16]), 11.0, "Liberation Sans", 1.0, 0.5, false),
    };
    assert_eq!(cell.fill, paint, "{expected}");
    assert_eq!(cell.margins, Some([padding; 4]), "{expected}");
    for side in &cell.borders {
        assert_eq!(side.as_ref().unwrap().width, Some(border), "{expected}");
        assert_eq!(
            side.as_ref().unwrap().fill,
            match expected {
                "A" => fill([170, 34, 0]),
                "R" => fill([34, 119, 51]),
                _ => fill([17, 34, 51]),
            }
        );
    }
    assert_eq!(
        cell.anchor.unwrap_or(Anchor::Top),
        match expected {
            "B" => Anchor::Bottom,
            "R" => Anchor::Middle,
            _ => Anchor::Top,
        }
    );
    assert_eq!(cell.vertical, (expected == "B").then_some(TextDir::EaVertical));
    for paragraph in &cell.text.paragraphs {
        assert_eq!(paragraph.def_props.font.as_deref(), Some(font), "{expected}");
        assert_eq!(paragraph.def_props.size, Some(size), "{expected}");
        for run in &paragraph.runs {
            assert_eq!(run.props.font.as_deref(), Some(font), "{expected}");
            assert_eq!(run.props.size, Some(size), "{expected}");
            assert_eq!(run.props.bold, Some(bold), "{expected}");
            assert_eq!(run.props.fill, fill([51, 68, 85]), "{expected}");
        }
    }
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}

#[test]
fn zero_or_one_declared_column_uses_only_actual_available_defaults() {
    let rows = "<table:table-row><table:table-cell><text:p>one</text:p></table:table-cell><table:table-cell><text:p>two</text:p></table:table-cell></table:table-row>";
    for columns in ["", "<table:table-column table:default-cell-style-name='A'/>"] {
        let p = import(&source(columns, rows)).unwrap();
        no_warnings(&p);
        assert_eq!(table(&p).n_cols(), 2);
        assert_style(&table(&p).rows[0].cells[0], if columns.is_empty() { "default" } else { "A" });
        assert_style(&table(&p).rows[0].cells[1], "default");
    }
}

#[test]
fn explicit_style_selects_a_whole_style_above_row_above_column() {
    let p = import(&source(
        "<table:table-column table:number-columns-repeated='3' table:default-cell-style-name='A'/>",
        "<table:table-row table:default-cell-style-name='R'><table:table-cell><text:p>row</text:p></table:table-cell><table:table-cell table:style-name='E'><text:p>explicit</text:p></table:table-cell><table:table-cell table:style-name='B'><text:p>other explicit</text:p></table:table-cell></table:table-row>",
    )).unwrap();
    no_warnings(&p);
    for (cell, style) in table(&p).rows[0].cells.iter().zip(["R", "E", "B"]) {
        assert_style(cell, style);
    }
}

#[test]
fn repeats_and_structural_wrappers_keep_physical_column_identity() {
    let p = import(&source(
        "<table:table-header-columns><table:table-column table:number-columns-repeated='2' table:default-cell-style-name='A'/></table:table-header-columns><table:table-column-group><table:table-columns><table:table-column table:number-columns-repeated='2' table:default-cell-style-name='B'/></table:table-columns></table:table-column-group><table:table-column/>",
        "<table:table-header-rows><table:table-row table:number-rows-repeated='2'><table:table-cell table:number-columns-repeated='5'><text:p>repeated</text:p></table:table-cell></table:table-row></table:table-header-rows><table:table-row-group><table:table-rows><table:table-row table:default-cell-style-name='R'><table:table-cell table:number-columns-repeated='5'><text:p>row</text:p></table:table-cell></table:table-row></table:table-rows></table:table-row-group>",
    )).unwrap();
    no_warnings(&p);
    assert_eq!(table(&p).n_rows(), 3);
    for row in &table(&p).rows[..2] {
        for (cell, expected) in row.cells.iter().zip(["A", "A", "B", "B", "default"]) {
            assert_style(cell, expected);
        }
    }
    for cell in &table(&p).rows[2].cells {
        assert_style(cell, "R");
    }
}

#[test]
fn spans_and_repeated_covered_positions_do_not_shift_later_column_defaults() {
    let p = import(&source(
        "<table:table-column table:default-cell-style-name='A'/><table:table-column table:number-columns-repeated='2' table:default-cell-style-name='B'/><table:table-column table:default-cell-style-name='R'/>",
        "<table:table-row><table:table-cell table:number-columns-spanned='3' table:number-rows-spanned='2'><text:p>anchor</text:p></table:table-cell><table:covered-table-cell table:number-columns-repeated='2'/><table:table-cell><text:p>right</text:p></table:table-cell></table:table-row><table:table-row><table:covered-table-cell table:number-columns-repeated='3'/><table:table-cell><text:p>below right</text:p></table:table-cell></table:table-row>",
    )).unwrap();
    no_warnings(&p);
    assert_eq!(table(&p).rows[0].cells[0].grid_span, 3);
    assert_eq!(table(&p).rows[0].cells[0].row_span, 2);
    for (r, row) in table(&p).rows.iter().enumerate() {
        for (c, (cell, expected)) in row.cells.iter().zip(["A", "B", "B", "R"]).enumerate() {
            assert_style(cell, expected);
            assert_eq!(cell.h_merge, c > 0 && c < 3);
            assert_eq!(cell.v_merge, r == 1 && c < 3);
        }
    }
}

#[test]
fn named_parent_inheritance_and_explicit_paragraph_and_run_styles_compose() {
    let styles = format!(
        "{DEFAULTS}{STYLES}<style:style style:name='Parent' style:family='table-cell' style:parent-style-name='A'><style:paragraph-properties fo:background-color='#C0D0E0'/><style:table-cell-properties fo:padding-right='4pt'/><style:text-properties fo:font-family='DejaVu Sans'/></style:style><style:style style:name='Child' style:family='table-cell' style:parent-style-name='Parent'><style:text-properties fo:font-size='19pt'/></style:style><style:style style:name='P' style:family='paragraph'><style:text-properties fo:font-size='30pt'/></style:style><style:style style:name='T' style:family='text'><style:text-properties fo:font-size='32pt' fo:font-weight='normal'/></style:style>"
    );
    let p = import(&package(&styles, "<table:table-column table:default-cell-style-name='Child'/>", "<table:table-row><table:table-cell><text:p>base</text:p><text:p text:style-name='P'>paragraph<text:span text:style-name='T'>run</text:span></text:p></table:table-cell></table:table-row>")).unwrap();
    no_warnings(&p);
    let cell = &table(&p).rows[0].cells[0];
    assert_eq!(cell.fill, fill([170, 34, 0]));
    assert_eq!(cell.margins, Some([2.0, 4.0, 2.0, 2.0]));
    assert_eq!(cell.borders[0].as_ref().unwrap().width, Some(2.0));
    let base = &cell.text.paragraphs[0];
    assert_eq!(base.def_props.size, Some(19.0));
    assert_eq!(base.runs[0].props.font.as_deref(), Some("DejaVu Sans"));
    assert_eq!(base.runs[0].props.bold, Some(true));
    assert_eq!(base.props.background, fill([192, 208, 224]));
    let paragraph = &cell.text.paragraphs[1];
    assert_eq!(paragraph.runs[0].props.size, Some(30.0));
    assert_eq!(paragraph.runs[1].props.size, Some(32.0));
    assert_eq!(paragraph.runs[1].props.bold, Some(false));
    assert_eq!(paragraph.runs[1].props.font.as_deref(), Some("DejaVu Sans"));
}

#[test]
fn rectangular_padding_does_not_invent_xml_default_style_owners() {
    let p = import(&source(
        "<table:table-column table:number-columns-repeated='2' table:default-cell-style-name='A'/>",
        "<table:table-row table:default-cell-style-name='R'><table:table-cell><text:p>actual</text:p></table:table-cell></table:table-row>",
    ))
    .unwrap();
    no_warnings(&p);
    assert_style(&table(&p).rows[0].cells[0], "R");
    assert_eq!(table(&p).rows[0].cells[1].fill, None);
    assert_eq!(table(&p).rows[0].cells[1].margins, None);
}

#[test]
fn native_and_odp_roundtrips_preserve_effective_styles_and_exact_constant_authority() {
    let mut p = import(&source(
        "<table:table-column table:default-cell-style-name='A'/><table:table-column table:default-cell-style-name='B'/>",
        "<table:table-row table:number-rows-repeated='2'><table:table-cell table:number-columns-repeated='2' office:value-type='float' office:value='+9007199254740993.00'><text:p>independent cached label</text:p></table:table-cell></table:table-row>",
    )).unwrap();
    no_warnings(&p);
    let expected = CellValue::Number { kind: CellNumberKind::Float, lexical: "+9007199254740993.00".into() };
    for row in &table(&p).rows {
        for (cell, style) in row.cells.iter().zip(["A", "B"]) {
            assert_style(cell, style);
            let CellConstantState::Current(data) = cell.constant_state().unwrap() else { panic!("current scalar") };
            assert_eq!(data.value(), &expected);
            assert_eq!(cell.text.text(), "independent cached label");
        }
    }
    let old_data = table(&p).rows[0].cells[0].data.clone().unwrap();
    table_mut(&mut p).rows[0].cells[0].text.paragraphs[0].runs[0].props.italic = Some(true);
    assert!(Arc::ptr_eq(table(&p).rows[0].cells[0].data.as_ref().unwrap(), &old_data));
    let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    let bytes = export(&native).unwrap();
    let back = import(&bytes).unwrap();
    no_warnings(&back);
    for (r, row) in table(&back).rows.iter().enumerate() {
        for (c, (cell, style)) in row.cells.iter().zip(["A", "B"]).enumerate() {
            assert_style(cell, style);
            assert_eq!(cell.data.as_ref().unwrap().value(), &expected);
            assert_eq!(cell.text.text(), "independent cached label");
            if (r, c) == (0, 0) {
                assert_eq!(cell.text.paragraphs[0].runs[0].props.italic, Some(true));
            }
        }
    }
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_TABLE_DEFAULT_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml", "META-INF/manifest.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(name.replace('/', "_")), part(&bytes, name)).unwrap();
        }
    }
    table_mut(&mut p).rows[0].cells[0].edit_display(|body| *body = TextBody::from_text("edited content")).unwrap();
    assert!(table(&p).rows[0].cells[0].data.is_none());
    assert_eq!(table(&p).rows[0].cells[1].data.as_ref().unwrap().value(), &expected);
    let edited = import(&export(&p).unwrap()).unwrap();
    assert!(table(&edited).rows[0].cells[0].data.is_none());
    assert_eq!(table(&edited).rows[0].cells[0].text.text(), "edited content");
}

#[test]
fn unknown_wrong_family_and_foreign_style_definitions_refuse() {
    for definition in
        ["", "<style:style style:name='Absent' style:family='paragraph'/>", "<foreign:style style:name='Absent' style:family='table-cell'/>"]
    {
        for (columns, rows) in [
            (
                "<table:table-column table:default-cell-style-name='Absent'/>",
                "<table:table-row><table:table-cell table:style-name='E'/></table:table-row>",
            ),
            ("", "<table:table-row table:default-cell-style-name='Absent'><table:table-cell table:style-name='E'/></table:table-row>"),
            ("", "<table:table-row><table:table-cell table:style-name='Absent'/></table:table-row>"),
        ] {
            let bytes = package(&format!("{STYLES}{definition}"), columns, rows);
            assert!(import(&bytes).unwrap_err().0.contains("table-cell style"));
        }
    }
}

#[test]
fn missing_parent_and_cyclic_default_style_chains_refuse() {
    for parent in ["Missing", "Child"] {
        let styles = format!("<style:style style:name='Child' style:family='table-cell' style:parent-style-name='{parent}'/>");
        let bytes =
            package(&styles, "<table:table-column table:default-cell-style-name='Child'/>", "<table:table-row><table:table-cell/></table:table-row>");
        let error = import(&bytes).unwrap_err().0;
        assert!(error.contains(if parent == "Missing" { "missing referenced" } else { "excessive table-cell" }), "{error}");
    }
}

#[test]
fn standard_namespace_aliases_work_and_foreign_default_attribute_is_not_applied() {
    let bytes = source(
        "<table:table-column table:default-cell-style-name='A'/>",
        "<table:table-row foreign:default-cell-style-name='R'><table:table-cell><text:p>label</text:p></table:table-cell></table:table-row>",
    );
    let p = import(&bytes).unwrap();
    assert_style(&table(&p).rows[0].cells[0], "A");
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
    let aliases = part(&bytes, "content.xml")
        .replace("xmlns:table=", "xmlns:grid=")
        .replace("<table:", "<grid:")
        .replace("</table:", "</grid:")
        .replace(" table:", " grid:");
    let styles =
        format!("<office:document-styles {NS} office:version='1.3'><office:styles>{DEFAULTS}{STYLES}</office:styles></office:document-styles>");
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", aliases.as_str()),
        ("styles.xml", styles.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(value.as_bytes()).unwrap();
    }
    let p = import(&z.finish().unwrap().into_inner()).unwrap();
    assert_style(&table(&p).rows[0].cells[0], "A");
}

#[test]
fn unimplemented_inherited_metadata_keeps_warnings_even_when_overridden() {
    for (properties, expected) in [
        ("<style:table-cell-properties fo:border='1pt groove #123456'/>", "relief borders"),
        ("<style:table-cell-properties style:border-line-width='1pt 2pt 3pt'/>", "double-line ratios"),
        ("<style:paragraph-properties style:line-height-at-least='12pt'/>", "minimum/additional line spacing"),
        ("<style:text-properties style:text-rotation-angle='90'/>", "rotation/combining/scaling"),
    ] {
        let styles = format!(
            "{STYLES}<style:style style:name='Unmapped' style:family='table-cell'>{properties}</style:style><style:style style:name='Child' style:family='table-cell' style:parent-style-name='Unmapped'/>"
        );
        for explicit in ["", "table:style-name='E'"] {
            let rows = format!(
                "<table:table-row table:default-cell-style-name='Child'><table:table-cell {explicit}><text:p>display</text:p></table:table-cell></table:table-row>"
            );
            let p = import(&package(&styles, "", &rows)).unwrap();
            assert!(p.tags.iter().any(|(key, text)| key == "odp:warning" && text.contains(expected)), "{:?}", p.tags);
            assert!(export(&p).is_err());
        }
    }
    let styles = format!(
        "{STYLES}<number:number-style style:name='Number'><number:number number:decimal-places='2'/></number:number-style><style:style style:name='Data' style:family='table-cell' style:data-style-name='Number'/><style:style style:name='Child' style:family='table-cell' style:parent-style-name='Data'/>"
    );
    let p = import(&package(
        &styles,
        "<table:table-column table:default-cell-style-name='Child'/>",
        "<table:table-row><table:table-cell table:style-name='E'><text:p>cached</text:p></table:table-cell></table:table-row>",
    ))
    .unwrap();
    assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("typed cell values and number formats")));
    assert!(export(&p).is_err());
}

#[test]
fn hostile_repeats_and_expanded_inherited_text_are_bounded() {
    for count in ["0", "1001", "18446744073709551616", "-1"] {
        for (columns, rows) in [
            (
                format!("<table:table-column table:number-columns-repeated='{count}' table:default-cell-style-name='A'/>"),
                "<table:table-row><table:table-cell/></table:table-row>".into(),
            ),
            (
                "".into(),
                format!(
                    "<table:table-row table:number-rows-repeated='{count}' table:default-cell-style-name='R'><table:table-cell/></table:table-row>"
                ),
            ),
            (
                "".into(),
                format!(
                    "<table:table-row table:default-cell-style-name='R'><table:table-cell table:number-columns-repeated='{count}'/></table:table-row>"
                ),
            ),
        ] {
            assert!(import(&source(&columns, &rows)).is_err());
        }
    }
    let rows = "<table:table-row table:number-rows-repeated='101' table:default-cell-style-name='A'><table:table-cell table:number-columns-repeated='1000'/></table:table-row>";
    let error = import(&source("", rows)).unwrap_err().0;
    assert!(matches!(error.as_str(), "table cell count exceeds limit" | "expanded nested table allocation exceeds limit"), "{error}");
    let styles =
        format!("<style:style style:name='Long' style:family='table-cell'><style:text-properties fo:language='{}'/></style:style>", "a".repeat(7000));
    let p = package(
        &styles,
        "<table:table-column table:number-columns-repeated='100' table:default-cell-style-name='Long'/>",
        "<table:table-row table:number-rows-repeated='100'><table:table-cell table:number-columns-repeated='100'><text:p>x</text:p></table:table-cell></table:table-row>",
    );
    let error = import(&p).unwrap_err().0;
    assert!(error.contains("expanded table"), "{error}");
}
