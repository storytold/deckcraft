//! Independent source-scope, exact-cache and lossy-display admission fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{Presentation, ShapeKind, Table};
use deckcraft_odp::{MIME, export, import};
use std::io::{Cursor, Write};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:of='urn:oasis:names:tc:opendocument:xmlns:of:1.2' xmlns:math='http://www.w3.org/1998/Math/MathML'";

fn package(cells: &str, declarations: &str, prelude: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} {declarations} office:version='1.3'>{prelude}<office:body><office:presentation><draw:page><draw:frame svg:width='300pt' svg:height='40pt'><table:table><table:table-row>{cells}</table:table-row></table:table></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn formula(scope: &str, expression: &str, cache: &str, display: &str) -> String {
    format!("<table:table-cell {scope} table:formula='{expression}' office:value-type='float' office:value='{cache}'>{display}</table:table-cell>")
}

fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(t) = &p.slides[0].shapes[0].kind else { panic!() };
    t
}

fn assert_no_executable_formula(bytes: &[u8]) -> Presentation {
    let p = import(bytes).unwrap();
    assert!(table(&p).rows.iter().flat_map(|r| &r.cells).all(|c| c.formula.is_none() && c.data.is_none()));
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert!(export(&p).is_err());
    let source = p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap();
    assert_eq!(source.data.as_slice(), bytes);
    p
}

#[test]
fn source_value_prefix_is_resolved_at_formula_owner_scope() {
    let good = formula("xmlns:alias='urn:oasis:names:tc:opendocument:xmlns:of:1.2'", "alias:=1+1", "2", "<text:p>2</text:p>");
    let p = import(&package(&good, "xmlns:alias='urn:foreign'", "")).unwrap();
    assert!(table(&p).cell(0, 0).unwrap().formula.is_some());
    for (scope, expression) in [
        ("xmlns:of='urn:foreign'", "of:=1+1"),
        ("xmlns:of='urn:oasis:names:tc:opendocument:xmlns:of:1.3'", "of:=1+1"),
        ("", "unbound:=1+1"),
        ("", "=1+1"),
    ] {
        assert_no_executable_formula(&package(&formula(scope, expression, "2", "<text:p>2</text:p>"), "", ""));
    }
}

#[test]
fn binary64_equal_but_decimal_unequal_source_cache_cannot_authenticate_formula() {
    let cell = formula("", "of:=9007199254740993+1", "9007199254740993", "<text:p>9007199254740993</text:p>");
    assert_no_executable_formula(&package(&cell, "", ""));
    let cell = formula("", "of:=0.1+0.2", "+003e-1", "<text:p>00.3000</text:p>");
    let p = import(&package(&cell, "", "")).unwrap();
    assert_eq!(table(&p).cell(0, 0).unwrap().formula.as_ref().unwrap().expression(), "=0.1+0.2");
    assert_eq!(table(&p).cell(0, 0).unwrap().text.text(), "00.3000");
}

#[test]
fn source_math_unknown_fields_and_ruby_cannot_become_ordinary_formula_cache_text() {
    for display in [
        "<text:p><math:math><math:mn>2</math:mn></math:math></text:p>",
        "<text:p><text:variable-get text:name='external'>2</text:variable-get></text:p>",
        "<text:p><text:ruby><text:ruby-base>2</text:ruby-base><text:ruby-text>annotation</text:ruby-text></text:ruby></text:p>",
        "<text:p><text:span><math:math><math:mn>2</math:mn></math:math></text:span></text:p>",
    ] {
        let bytes = package(&formula("", "of:=1+1", "2", display), "", "");
        assert_no_executable_formula(&bytes);
    }
}

#[test]
fn unsupported_formula_anywhere_prevents_partial_table_graph_admission() {
    for unsupported in ["of:=1/2", "of:=SUM([.A1])", "of:=[.A1]", "foreign:=1+1"] {
        let first = formula("", unsupported, "2", "<text:p>2</text:p>");
        let second = formula("", "of:=[.A1]+1", "3", "<text:p>3</text:p>");
        assert_no_executable_formula(&package(&(first + &second), "", ""));
    }
}

#[test]
fn calculation_settings_block_execution_and_cannot_be_laundered_by_old_native_tags() {
    let bytes = package(&formula("", "of:=1+1", "2", "<text:p>2</text:p>"), "", "<table:calculation-settings table:precision-as-shown='true'/>");
    let mut p = assert_no_executable_formula(&bytes);
    p.tags.retain(|(key, _)| key != "odp:warning" && key != "odp:imported");
    assert!(deckcraft_odp::validate_source_diagnostics(&p).unwrap_err().to_string().contains("calculation-settings"));
    assert!(export(&p).is_err());
}
