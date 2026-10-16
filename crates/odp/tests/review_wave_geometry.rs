//! Independent ODF 1.3 §19.145 path-command equivalence through renderer geometry.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_odp::{MIME, import};
use std::io::{Cursor, Write};

fn path(input: &str) -> deckcraft_geom::BezPath {
    let xml = format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0'><office:body><office:presentation><draw:page><draw:custom-shape svg:width='100pt' svg:height='50pt'><draw:enhanced-geometry svg:viewBox='0 0 100 50' draw:enhanced-path='{input}'/></draw:custom-shape></draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", &xml),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(value.as_bytes()).unwrap();
    }
    let p = import(&z.finish().unwrap().into_inner()).unwrap();
    deckcraft_render::shape_geometry(&p.slides[0].shapes[0], 100.0, 50.0).paths[0].path.clone()
}
#[test]
fn repeated_quadrant_command_letters_can_be_omitted_without_changing_geometry() {
    for command in ['X', 'Y'] {
        let explicit = path(&format!("M 0 0 {command} 50 50 {command} 100 0 N"));
        let implicit = path(&format!("M 0 0 {command} 50 50 100 0 N"));
        assert_eq!(explicit.elements(), implicit.elements(), "ODF allows repeated command letters to be omitted: {command}");
    }
}
