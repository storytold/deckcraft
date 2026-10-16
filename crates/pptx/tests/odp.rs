//! OpenDocument (.odp) import: a hand-built package becomes slides with text, shapes, a
//! picture, rotation and speaker notes.

use std::io::{Cursor, Write};

use deckcraft_model::{Fill, ShapeKind};
use zip::write::SimpleFileOptions;

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

const NS: &str = r##"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:xlink="http://www.w3.org/1999/xlink""##;

fn odp() -> Vec<u8> {
    let styles = format!(
        r##"<?xml version="1.0"?><office:document-styles {NS}><office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="28cm" fo:page-height="15.75cm"/></style:page-layout></office:automatic-styles></office:document-styles>"##
    );
    let content = format!(
        r##"<?xml version="1.0"?><office:document-content {NS}><office:automatic-styles>
<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" draw:stroke="none"/></style:style>
<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
</office:automatic-styles><office:body><office:presentation>
<draw:page draw:name="One">
<draw:frame presentation:class="title" svg:x="1cm" svg:y="1cm" svg:width="20cm" svg:height="3cm"><draw:text-box><text:p>Hello <text:span text:style-name="T1">ODP</text:span></text:p></draw:text-box></draw:frame>
<draw:custom-shape draw:style-name="gr1" svg:width="4cm" svg:height="2cm" draw:transform="rotate (1.5707963267949) translate (5cm 8cm)"><draw:enhanced-geometry draw:type="ellipse"/></draw:custom-shape>
<draw:frame svg:x="10cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame>
<presentation:notes><draw:frame presentation:class="notes" svg:x="1cm" svg:y="1cm" svg:width="10cm" svg:height="5cm"><draw:text-box><text:p>Speak up</text:p></draw:text-box></draw:frame></presentation:notes>
</draw:page>
<draw:page draw:name="Two"/>
</office:presentation></office:body></office:document-content>"##
    );
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("mimetype", b"application/vnd.oasis.opendocument.presentation".as_slice()),
        ("styles.xml", styles.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("Pictures/a.png", PNG),
    ] {
        z.start_file(name, stored).expect("start");
        z.write_all(data).expect("write");
    }
    z.finish().expect("finish").into_inner()
}

#[test]
fn imports_odp() {
    let bytes = odp();
    assert!(deckcraft_pptx::sniff_odp(&bytes));
    assert!(!deckcraft_pptx::sniff_odp(b"PK\x03\x04 not odp"));
    let p = deckcraft_pptx::import_odp(&bytes).expect("import");
    assert_eq!(p.slides.len(), 2);
    assert!((p.slide_size.width / p.slide_size.height - 28.0 / 15.75).abs() < 1e-3);
    let s = &p.slides[0];
    let title = s.shapes.iter().find_map(|sh| sh.text.as_ref()).expect("text");
    assert_eq!(title.text(), "Hello ODP");
    let shape = s.shapes.iter().find(|sh| matches!(sh.fill, Some(Fill::Solid { .. }))).expect("filled shape");
    let x = shape.xfrm.expect("xfrm");
    assert!((x.rot - 270.0).abs() < 0.5, "rot {}", x.rot);
    assert!(s.shapes.iter().any(|sh| matches!(sh.kind, ShapeKind::Picture { .. })));
    assert_eq!(s.notes.text(), "Speak up");
}
