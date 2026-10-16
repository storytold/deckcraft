//! Independent original XML witnesses based on OASIS ODF 1.2/1.3 and XSD 2004.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{
    Presentation, ShapeKind, Table, TextBody,
    table::{CellBlock, CellValue},
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:o='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:t='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:x='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:s='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0'";

fn source(version: &str, cells: &str) -> Vec<u8> {
    let content = format!(
        "<o:document-content {NS} o:version='{version}'><o:body><o:presentation><d:page d:name='Review'><d:frame s:width='400pt' s:height='80pt'><t:table t:name='Review'><t:table-column t:number-columns-repeated='2'/><t:table-row>{cells}</t:table-row></t:table></d:frame></d:page></o:presentation></o:body></o:document-content>"
    );
    let styles = format!("<o:document-styles {NS} o:version='{version}'><o:styles/></o:document-styles>");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in [
        ("mimetype", MIME),
        ("content.xml", &content),
        ("styles.xml", &styles),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("table") };
    table
}
fn value(p: &Presentation, column: usize) -> &CellValue {
    table(p).rows[0].cells[column].data.as_ref().unwrap().value()
}
fn no_warnings(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}
fn source_bytes(p: &Presentation) -> &[u8] {
    &p.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data
}
fn part(package: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(package)).unwrap();
    let mut body = String::new();
    archive.by_name(name).unwrap().read_to_string(&mut body).unwrap();
    body
}
fn emit(name: &str, package: &[u8]) {
    if let Ok(root) = std::env::var("DECKCRAFT_TEMPORAL_REVIEW_XML_DIR") {
        let directory = std::path::Path::new(&root).join(name);
        std::fs::create_dir_all(&directory).unwrap();
        for name in ["content.xml", "styles.xml", "META-INF/manifest.xml"] {
            std::fs::write(directory.join(name.replace('/', "_")), part(package, name)).unwrap();
        }
    }
}

#[test]
fn literal_xml_whitespace_normalizes_before_exact_scalar_capture() {
    for version in ["1.2", "1.3"] {
        let bytes = source(
            version,
            "<t:table-cell o:value-type='date' o:date-value=' \t2004-02-29\r\n '><x:p>calendar cache</x:p></t:table-cell><t:table-cell o:value-type='time' o:time-value='&#x9;-P0007MT89.001200S&#xD;&#xA;'><x:p>duration cache</x:p></t:table-cell>",
        );
        let p = import(&bytes).unwrap();
        no_warnings(&p);
        let date = CellValue::Date { lexical: "  2004-02-29  ".into() };
        let duration = CellValue::Duration { lexical: "\t-P0007MT89.001200S\r\n".into() };
        assert_eq!(value(&p, 0), &date);
        assert_eq!(value(&p, 1), &duration);
        let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        assert_eq!(source_bytes(&native), bytes);
        let output = export(&native).unwrap();
        let back = import(&output).unwrap();
        no_warnings(&back);
        assert_eq!(value(&back, 0), &date);
        assert_eq!(value(&back, 1), &duration);
        assert_eq!(table(&back).rows[0].cells[0].text.text(), "calendar cache");
        assert_eq!(table(&back).rows[0].cells[1].text.text(), "duration cache");
        emit(&format!("normalization-{version}"), &output);
    }
}

#[test]
fn namespace_rebinding_and_unqualified_attributes_cannot_authenticate_values() {
    for attributes in [
        "xmlns:o='urn:deckcraft:review:foreign' o:value-type='date' o:date-value='2004-02-29'",
        "o:value-type='date' date-value='2004-02-29'",
        "value-type='time' o:time-value='PT7H'",
        "xmlns:v='urn:deckcraft:review:foreign' o:value-type='time' v:time-value='PT7H'",
    ] {
        let bytes = source("1.3", &format!("<t:table-cell {attributes}><x:p>2004-02-29</x:p></t:table-cell>"));
        let p = import(&bytes).unwrap();
        assert!(table(&p).rows[0].cells[0].data.is_none(), "{attributes}");
        assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "{attributes}");
        assert!(export(&p).is_err(), "{attributes}");
        assert_eq!(source_bytes(&p), bytes);
    }
    // Prefix spelling carries no authority; the exact standard namespace does.
    let bytes = source(
        "1.3",
        "<t:table-cell xmlns:v='urn:oasis:names:tc:opendocument:xmlns:office:1.0' v:value-type='date' v:date-value='2004-02-29+13:59'><x:p>alias cache</x:p></t:table-cell>",
    );
    let p = import(&bytes).unwrap();
    no_warnings(&p);
    assert_eq!(value(&p, 0), &CellValue::Date { lexical: "2004-02-29+13:59".into() });
    let output = export(&p).unwrap();
    emit("namespace-alias", &output);
}

#[test]
fn default_namespace_nested_tables_preserve_outer_temporal_authority() {
    let nested = "<table xmlns='urn:oasis:names:tc:opendocument:xmlns:table:1.0'><table-row><table-cell><x:p>inner cache</x:p></table-cell></table-row></table>";
    for (attributes, expected) in [
        ("o:value-type='date' o:date-value='2004-02-29'", CellValue::Date { lexical: "2004-02-29".into() }),
        ("o:value-type='time' o:time-value='PT7H'", CellValue::Duration { lexical: "PT7H".into() }),
    ] {
        let bytes = source("1.3", &format!("<t:table-cell {attributes}><x:p>outer cache</x:p>{nested}<x:p>tail</x:p></t:table-cell>"));
        let p = import(&bytes).unwrap();
        let check = |p: &Presentation| {
            no_warnings(p);
            assert_eq!(value(p, 0), &expected);
            let cell = &table(p).rows[0].cells[0];
            assert_eq!(cell.text.text(), "outer cache");
            assert_eq!(cell.blocks.len(), 2);
            let CellBlock::Table(child) = &cell.blocks[0] else { panic!("ordered nested table") };
            assert_eq!(child.table.cell(0, 0).unwrap().text.text(), "inner cache");
            assert!(child.table.cell(0, 0).unwrap().data.is_none());
            assert!(matches!(&cell.blocks[1], CellBlock::Text(tail) if tail.body.text() == "tail"));
        };
        check(&p);
        let original = p.clone();
        let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        check(&native);
        let back = import(&export(&native).unwrap()).unwrap();
        check(&back);
        check(&import(&export(&back).unwrap()).unwrap());
        assert_eq!(source_bytes(&p), bytes);
        assert!(p == original, "export must not mutate source authority");
    }
}

#[test]
fn actual_cached_edit_regenerates_current_authority_and_retains_original_source() {
    let bytes = source(
        "1.3",
        "<t:table-cell o:value-type='date' o:date-value='2004-02-29-00:00'><x:p>first cache</x:p></t:table-cell><t:table-cell o:value-type='time' o:time-value='-P0007MT89.001200S'><x:p>second cache</x:p></t:table-cell>",
    );
    for touched in [0, 1] {
        let mut p = import(&bytes).unwrap();
        no_warnings(&p);
        let other = value(&p, 1 - touched).clone();
        let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
        t.rows[0].cells[touched].edit_display(|body| *body = TextBody::from_text("review edited cache")).unwrap();
        assert!(t.rows[0].cells[touched].data.is_none());
        let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        assert_eq!(source_bytes(&native), bytes);
        let generated = export(&native).unwrap();
        let back = import(&generated).unwrap();
        no_warnings(&back);
        assert!(table(&back).rows[0].cells[touched].data.is_none());
        assert_eq!(table(&back).rows[0].cells[touched].text.text(), "review edited cache");
        assert_eq!(value(&back, 1 - touched), &other);
        assert_eq!(source_bytes(&native), bytes);
        emit(&format!("actual-edit-{touched}"), &generated);
    }
}
