//! Independent, contributor-original continuation fixtures. No producer files or assets.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_model::{
    notes::{NotesMaster, NotesPage},
    text::{Action, Hyperlink},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";

fn crc(bytes: &[u8]) -> u32 {
    let mut result = !0u32;
    for byte in bytes {
        result ^= u32::from(*byte);
        for _ in 0..8 {
            result = (result >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(result & 1));
        }
    }
    !result
}

/// Exact stored ZIP32 records, with optional caller-controlled descriptor bytes.
/// Central records deliberately appear in reverse physical order.
fn raw(parts: &[(String, Vec<u8>)], descriptors: &[Option<Vec<u8>>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut directory = Vec::new();
    for (index, (name, payload)) in parts.iter().enumerate() {
        let start = bytes.len() as u32;
        let size = payload.len() as u32;
        let checksum = crc(payload);
        let descriptor = descriptors.get(index).and_then(Option::as_ref);
        let flags = 0x800 | if descriptor.is_some() { 8 } else { 0 };
        bytes.extend_from_slice(b"PK\x03\x04");
        for n in [20u16, flags, 0, 0, 0] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        for n in if descriptor.is_some() { [0, 0, 0] } else { [checksum, size, size] } {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(payload);
        if let Some(descriptor) = descriptor {
            bytes.extend_from_slice(descriptor);
        }
        let mut central = b"PK\x01\x02".to_vec();
        for n in [20u16, 20, flags, 0, 0, 0] {
            central.extend_from_slice(&n.to_le_bytes());
        }
        for n in [checksum, size, size] {
            central.extend_from_slice(&n.to_le_bytes());
        }
        for n in [name.len() as u16, 0, 0, 0, 0] {
            central.extend_from_slice(&n.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&start.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
        directory.push(central);
    }
    let start = bytes.len() as u32;
    for entry in directory.iter().rev() {
        bytes.extend_from_slice(entry);
    }
    let size = bytes.len() as u32 - start;
    bytes.extend_from_slice(b"PK\x05\x06");
    for n in [0u16, 0, parts.len() as u16, parts.len() as u16] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&start.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

fn package(content: String) -> Vec<(String, Vec<u8>)> {
    vec![
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("META-INF/manifest.xml".into(), format!("<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/></manifest:manifest>").into_bytes()),
    ]
}

fn members(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut members = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        members.insert(file.name().into(), data);
    }
    members
}

fn descriptor(payload: &[u8], signed: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if signed {
        bytes.extend_from_slice(b"PK\x07\x08");
    }
    for value in [crc(payload), payload.len() as u32, payload.len() as u32] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn source_text() -> Vec<(String, Vec<u8>)> {
    // Deliberate unmapped standard paragraph property selects the source-patch path.
    package(format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page><draw:frame svg:width='100pt' svg:height='80pt'><draw:text-box><text:p text:style-name='P'>Earlier</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    ))
}

fn edited_source(source: &[u8]) -> Presentation {
    let mut p = import(source).unwrap();
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Later".into();
    p
}

#[test]
fn independent_descriptors_end_at_both_physical_boundaries_with_exact_lexical_edits() {
    for signed in [false, true] {
        let mut parts = source_text();
        // Real ZIP markers inside an original PNG payload never delimit records.
        let mut png = deckcraft_render::Image { width: 1, height: 1, pixels: vec![41, 83, 127, 255] }.to_png();
        png.extend_from_slice(b"PK\x03\x04PK\x07\x08");
        parts.push(("Pictures/marker.png".into(), png));
        let manifest = String::from_utf8(parts[2].1.clone()).unwrap().replace(
            "</manifest:manifest>",
            "<manifest:file-entry manifest:full-path='Pictures/marker.png' manifest:media-type='image/png'/></manifest:manifest>",
        );
        parts[2].1 = manifest.into_bytes();
        let tails = parts.iter().map(|(_, data)| Some(descriptor(data, signed))).collect::<Vec<_>>();
        let source = raw(&parts, &tails);
        let desired = edited_source(&source);
        let output = export(&desired).unwrap();
        let before = members(&source);
        let after = members(&output);
        assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
        for (name, data) in before {
            let expected = if name == "content.xml" { String::from_utf8(data).unwrap().replace("Earlier", "Later").into_bytes() } else { data };
            assert_eq!(after[&name], expected, "{name}");
        }
        assert_eq!(import(&output).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Later");
        assert_eq!(desired.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
    }
}

#[test]
fn independent_descriptors_reject_every_nearby_extent_and_corrupt_word() {
    let parts = source_text();
    let baseline = edited_source(&raw(&parts, &[]));
    for signed in [false, true] {
        for index in [1, parts.len() - 1] {
            let proper = descriptor(&parts[index].1, signed);
            for length in 0..=25 {
                if length == proper.len() {
                    continue;
                }
                let mut tails = vec![None; parts.len()];
                let mut tail = proper.clone();
                tail.resize(length, 0);
                tails[index] = Some(tail);
                let mut desired = baseline.clone();
                desired.media.iter_mut().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data =
                    Arc::new(raw(&parts, &tails));
                assert!(export(&desired).unwrap_err().to_string().contains("ZIP32"), "signed={signed}, entry={index}, extent={length}");
            }
            for byte in 0..proper.len() {
                let mut tails = vec![None; parts.len()];
                let mut tail = proper.clone();
                tail[byte] ^= 1;
                tails[index] = Some(tail);
                let mut desired = baseline.clone();
                desired.media.iter_mut().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data =
                    Arc::new(raw(&parts, &tails));
                assert!(export(&desired).unwrap_err().to_string().contains("ZIP32"), "signed={signed}, entry={index}, byte={byte}");
            }
        }
    }
}

fn text_shape(id: u32, label: &str) -> Shape {
    Shape {
        id: ShapeId(id),
        name: label.into(),
        xfrm: Some(Xfrm::new(10.0, 10.0, 100.0, 50.0)),
        text: Some(TextBody::from_text(label)),
        ..Default::default()
    }
}

fn grouped_label(label: &str) -> Shape {
    Shape {
        id: ShapeId(8),
        name: "group".into(),
        xfrm: Some(Xfrm::new(0.0, 0.0, 200.0, 200.0)),
        kind: ShapeKind::Group { children: vec![text_shape(9, label)], child: Xfrm::new(0.0, 0.0, 200.0, 200.0) },
        ..Default::default()
    }
}

fn owners_fixture() -> Presentation {
    let mut p = Presentation::default();
    let first = p.slides[0].as_ref().clone();
    p.slides.clear();
    for id in [21, 22] {
        let mut master_body = text_shape(7, "Master body");
        master_body.ph = Some(Placeholder { kind: PhType::Body, ..Default::default() });
        p.notes_masters.push(NotesMaster {
            master: Arc::new(Master { id: MasterId(id), shapes: vec![master_body, grouped_label("Master artwork")], ..Default::default() }),
            ..Default::default()
        });
        let mut slide = first.clone();
        slide.id = SlideId(id);
        slide.name = format!("Page {id}");
        slide.shapes = vec![text_shape(7, "Slide paragraph"), grouped_label("Slide group")];
        slide.shapes[0].click = Some(Hyperlink {
            action: Action::Slide { slide: SlideId(if id == 21 { 22 } else { 21 }) },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
            sound_name: None,
        });
        slide.animations = vec![Animation { shape: ShapeId(7), paragraph: Some(0), trigger: Some(ShapeId(9)), ..Default::default() }];
        slide.notes = TextBody::from_text(&format!("Canonical {id}"));
        let mut body = text_shape(7, "unused");
        body.text = None;
        body.ph = Some(Placeholder { kind: PhType::Body, ..Default::default() });
        slide.notes_page = Some(NotesPage {
            master: Some(MasterId(id)),
            body_shape: Some(ShapeId(7)),
            shapes: vec![body, grouped_label("Local artwork")],
            ..Default::default()
        });
        p.slides.push(Arc::new(slide));
    }
    p
}

fn ids(xml: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for rest in xml.split("xml:id=\"").skip(1) {
        let id = rest.split('"').next().unwrap();
        assert!(ids.insert(id.to_string()), "duplicate xml:id {id}");
    }
    ids
}

#[test]
fn numeric_shape_ids_are_scoped_across_notes_masters_pages_groups_and_slide_timing() {
    let mut p = owners_fixture();
    for _ in 0..2 {
        let bytes = export(&p).unwrap();
        let parts = members(&bytes);
        let content = String::from_utf8(parts["content.xml"].clone()).unwrap();
        let styles = String::from_utf8(parts["styles.xml"].clone()).unwrap();
        let content_ids = ids(&content);
        let style_ids = ids(&styles);
        for id in [21, 22] {
            for shape in [7, 8, 9] {
                assert!(content_ids.contains(&format!("slide{id}-shape{shape}")));
                assert!(content_ids.contains(&format!("notesPage{id}-shape{shape}")));
                assert!(style_ids.contains(&format!("notesMaster{id}-shape{shape}")));
            }
            assert!(content_ids.contains(&format!("slide{id}-shape7-p0")));
            assert!(content_ids.contains(&format!("notesPage{id}-shape7-p0")));
            assert!(style_ids.contains(&format!("notesMaster{id}-shape7-p0")));
            assert!(content.contains(&format!("smil:targetElement=\"slide{id}-shape7-p0\"")));
            assert!(content.contains(&format!("smil:begin=\"slide{id}-shape9.click\"")));
            assert!(content.contains(&format!("xlink:href=\"#Page {id}\"")));
        }
        assert!(content_ids.is_disjoint(&style_ids));
        let reopened = import(&bytes).unwrap();
        assert!(!reopened.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", reopened.tags);
        for (original, reopened) in p.slides.iter().zip(&reopened.slides) {
            assert_eq!(reopened.notes.text(), original.notes.text());
            assert_eq!(reopened.animations.len(), 1);
            let animation = &reopened.animations[0];
            assert_eq!(animation.paragraph, Some(0));
            assert_eq!(find_shape(&reopened.shapes, animation.shape).unwrap().text.as_ref().unwrap().text(), "Slide paragraph");
            assert_eq!(find_shape(&reopened.shapes, animation.trigger.unwrap()).unwrap().text.as_ref().unwrap().text(), "Slide group");
        }
        p.slides.reverse();
        p.notes_masters.reverse();
    }
}

fn text_area_package(area: &str, equations: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page><draw:custom-shape svg:width='200pt' svg:height='100pt'><draw:enhanced-geometry draw:type='rectangle' svg:viewBox='-20 30 120 70' draw:enhanced-path='M -20 30 L 100 30 100 100 -20 100 Z N' draw:text-areas='{area}'>{equations}</draw:enhanced-geometry></draw:custom-shape></draw:page></office:presentation></office:body></office:document-content>"
    );
    raw(&package(content), &[])
}

#[test]
fn full_nonzero_text_areas_accept_independent_vertical_formula_rectangles() {
    let equations = "<draw:equation draw:name='左' draw:formula='left'/><draw:equation draw:name='上' draw:formula='top'/><draw:equation draw:name='右' draw:formula='?左 + width'/><draw:equation draw:name='下' draw:formula='?上 + height'/>";
    let literal = import(&text_area_package("-20 30 100 100", "")).unwrap();
    let p = import(&text_area_package("-20 30 100 100 ?左 ?上 ?右 ?下", equations)).unwrap();
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
    assert_eq!(p.slides[0].shapes[0].geom, literal.slides[0].shapes[0].geom);
    assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Shape));
    assert!(p.slides[0].shapes[0].text.is_none());
    let reopened = import(&export(&p).unwrap()).unwrap();
    assert!(matches!(reopened.slides[0].shapes[0].kind, ShapeKind::Shape));
}

#[test]
fn second_vertical_area_formula_failures_and_custom_coordinates_retain_recovery() {
    for (area, equations) in [
        ("-20 30 100 100 -20 30 99.999999999 100", ""),
        ("-20 30 100 100 -20 30 ?右 100", "<draw:equation draw:name='右' draw:formula='right - 1'/>"),
        ("-20 30 100 100 -20 30 ?cycle 100", "<draw:equation draw:name='cycle' draw:formula='?cycle'/>"),
        ("-20 30 100 100 -20 30 ?bad 100", "<draw:equation draw:name='bad' draw:formula='1/0'/>"),
        ("-20 30 100 100 -20 30 100 100 0", ""),
        ("-20 30 100 100,-20 30 100 100", ""),
    ] {
        let source = text_area_package(area, equations);
        let p = import(&source).unwrap();
        assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Opaque { .. }), "{area}");
        assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "{area}");
        assert!(export(&p).is_err(), "{area}");
        assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
    }
}
