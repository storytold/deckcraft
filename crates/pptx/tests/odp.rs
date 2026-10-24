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

const NS: &str = r##"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0""##;

fn odp() -> Vec<u8> {
    let styles = format!(
        r##"<?xml version="1.0"?><office:document-styles {NS}><office:automatic-styles><style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="28cm" fo:page-height="15.75cm"/></style:page-layout><style:style style:name="Mdp1" style:family="drawing-page"><style:drawing-page-properties draw:fill="solid" draw:fill-color="#003366"/></style:style><style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ffcc00"/></style:style></office:automatic-styles><office:master-styles><style:master-page style:name="Dark" style:display-name="Dark Blue" draw:style-name="Mdp1"><draw:rect draw:style-name="gr1" svg:x="0cm" svg:y="15cm" svg:width="28cm" svg:height="0.75cm"/><draw:frame presentation:class="title" presentation:placeholder="true" svg:x="1cm" svg:y="1cm" svg:width="20cm" svg:height="3cm"/></style:master-page></office:master-styles></office:document-styles>"##
    );
    let content = format!(
        r##"<?xml version="1.0"?><office:document-content {NS}><office:automatic-styles>
<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" draw:stroke="none"/></style:style>
<style:style style:name="dp2" style:family="drawing-page"><style:drawing-page-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>
<style:style style:name="co1" style:family="table-column"><style:table-column-properties style:column-width="4cm"/></style:style>
<style:style style:name="ce1" style:family="table-cell"><style:graphic-properties draw:fill="solid" draw:fill-color="#cccccc"/></style:style>
<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
</office:automatic-styles><office:body><office:presentation>
<draw:page draw:name="One" draw:master-page-name="Dark">
<draw:frame svg:x="1cm" svg:y="10cm" svg:width="12cm" svg:height="3cm"><table:table><table:table-column table:style-name="co1" table:number-columns-repeated="3"/><table:table-row><table:table-cell table:style-name="ce1" table:number-columns-spanned="2"><text:p>Wide</text:p></table:table-cell><table:covered-table-cell/><table:table-cell table:number-rows-spanned="2"><text:p>Tall</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell><text:p>a</text:p></table:table-cell><table:table-cell><text:p>b</text:p></table:table-cell><table:covered-table-cell/></table:table-row></table:table></draw:frame>
<draw:frame presentation:class="title" svg:x="1cm" svg:y="1cm" svg:width="20cm" svg:height="3cm"><draw:text-box><text:p>Hello <text:span text:style-name="T1">ODP</text:span></text:p></draw:text-box></draw:frame>
<draw:custom-shape draw:style-name="gr1" svg:width="4cm" svg:height="2cm" draw:transform="rotate (1.5707963267949) translate (5cm 8cm)"><draw:enhanced-geometry draw:type="ellipse"/></draw:custom-shape>
<draw:frame svg:x="10cm" svg:y="5cm" svg:width="2cm" svg:height="2cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame>
<presentation:notes><draw:frame presentation:class="notes" svg:x="1cm" svg:y="1cm" svg:width="10cm" svg:height="5cm"><draw:text-box><text:p>Speak up</text:p></draw:text-box></draw:frame></presentation:notes>
</draw:page>
<draw:page draw:name="Two" draw:style-name="dp2"/>
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

    // Tables: widths, text, fill and merges.
    let t = s
        .shapes
        .iter()
        .find_map(|sh| match &sh.kind {
            ShapeKind::Table(t) => Some(t),
            _ => None,
        })
        .expect("table");
    assert_eq!(t.cols.len(), 3);
    assert!((t.cols[0] - 4.0 * 72.0 / 2.54).abs() < 0.1);
    assert_eq!(t.rows.len(), 2);
    assert_eq!(t.rows[0].cells[0].text.text(), "Wide");
    assert_eq!(t.rows[0].cells[0].grid_span, 2);
    assert!(matches!(t.rows[0].cells[0].fill, Some(Fill::Solid { .. })));
    assert!(t.rows[0].cells[1].h_merge);
    assert_eq!(t.rows[0].cells[2].row_span, 2);
    assert!(t.rows[1].cells[2].v_merge && !t.rows[1].cells[2].h_merge);
    assert_eq!(t.rows[1].cells[1].text.text(), "b");

    // The master page becomes a layout with its background and decoration, not its placeholders.
    let (_, lay) = p.layout(s.layout).expect("layout");
    assert_eq!(lay.name, "Dark Blue");
    assert!(lay.background.is_some());
    assert_eq!(lay.shapes.len(), 1);
    assert!(s.background.is_none());
    // A page's own background.
    assert!(p.slides[1].background.is_some());

    // Saving as .pptx keeps the master's layout and the table.
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).expect("export")).expect("reimport");
    let (_, lay) = back.layout(back.slides[0].layout).expect("layout");
    assert_eq!(lay.name, "Dark Blue");
    assert_eq!(lay.shapes.len(), 1);
    assert!(back.slides[0].shapes.iter().any(|sh| matches!(sh.kind, ShapeKind::Table(_))));
}
