//! Independent contributor-original template ownership and source-loss regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::*;
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0'";
fn package(common: &str, masters: &str, pages: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation>{pages}</office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles>{common}</office:styles><office:automatic-styles><style:page-layout style:name='Physical'><style:page-layout-properties fo:page-width='120pt' fo:page-height='80pt'/></style:page-layout></office:automatic-styles><office:master-styles>{masters}</office:master-styles></office:document-styles>"
    );
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", &content),
        ("styles.xml", &styles),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn template(name: &str) -> String {
    format!(
        "<style:presentation-page-layout style:name='{name}'><presentation:placeholder presentation:object='text' svg:x='1pt' svg:y='2pt' svg:width='3pt' svg:height='4pt'/></style:presentation-page-layout>"
    )
}
fn warning(p: &Presentation) -> bool {
    p.tags.iter().any(|(key, _)| key == "odp:warning")
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    z.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}

#[test]
fn regular_master_presentation_style_ownership_is_guarded_with_or_without_templates() {
    for attr in ["presentation:style-name='Shared'", "presentation:class-names='Shared'"] {
        for selected in [false, true] {
            let common = format!(
                "{}<style:style style:name='Shared' style:family='presentation'><style:graphic-properties draw:fill='solid' draw:fill-color='#123456'/></style:style>",
                if selected { template("T") } else { String::new() }
            );
            let masters = format!(
                "<style:master-page style:name='M' style:page-layout-name='Physical'><draw:rect {attr} svg:x='1pt' svg:y='2pt' svg:width='30pt' svg:height='20pt'/></style:master-page>"
            );
            let pages = format!(
                "<draw:page draw:name='Page' draw:master-page-name='M' {}/>",
                if selected { "presentation:presentation-page-layout-name='T'" } else { "" }
            );
            let bytes = package(&common, &masters, &pages);
            let p = import(&bytes).unwrap();
            assert!(warning(&p), "master shared-style ownership was silently flattened: {attr}; selected={selected}");
            assert_eq!(p.presentation_templates.len(), usize::from(selected));
            assert_eq!(p.slides[0].presentation_template.is_some(), selected);
            assert!(p.media.iter().any(|m| m.content_type == "application/vnd.deckcraft.odp-source" && m.data.as_slice() == bytes));
        }
    }
}

#[test]
fn unsupported_unused_template_does_not_hide_supported_selection_or_independent_object_guard() {
    let common = format!("{}<style:presentation-page-layout style:name='Unsupported' draw:name='unmapped'/>", template("Good"));
    let p = import(&package(&common, "", "<draw:page presentation:presentation-page-layout-name='Good'><draw:rect presentation:placeholder='false' svg:x='1pt' svg:y='2pt' svg:width='3pt' svg:height='4pt'/></draw:page>")).unwrap();
    assert!(warning(&p));
    assert_eq!(p.presentation_templates.len(), 1);
    assert_eq!(p.presentation_templates[0].name, "Good");
    assert_eq!(p.slides[0].presentation_template, Some(p.presentation_templates[0].id));
    assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("regular-page")), "{:?}", p.tags);
}

#[test]
fn same_namespace_under_foreign_parent_never_becomes_admitted_definition() {
    let common = format!("<draw:g>{}</draw:g>{}", template("WrongOwner"), template("Good"));
    let p = import(&package(&common, "", "<draw:page presentation:presentation-page-layout-name='Good'/>")).unwrap();
    assert!(warning(&p));
    assert_eq!(p.presentation_templates.len(), 1);
    assert_eq!(p.presentation_templates[0].name, "Good");
    assert_eq!(p.slides[0].presentation_template, Some(p.presentation_templates[0].id));
}

#[test]
fn exported_subnormal_coordinates_and_unused_equal_slots_roundtrip_without_geometry_application() {
    let mut p = defaults::blank_presentation(Size::new(120., 80.), Theme::default(), false);
    let bounds = PresentationTemplateBox::new(f64::from_bits(1), -f64::from_bits(1), 3., 4.).unwrap();
    let slots = vec![
        PresentationTemplateSlot { id: PresentationTemplateSlotId(17), role: PresentationRole::Text, bounds },
        PresentationTemplateSlot { id: PresentationTemplateSlotId(18), role: PresentationRole::Text, bounds },
    ];
    p.presentation_templates = vec![Arc::new(PresentationTemplate {
        id: PresentationTemplateId(91),
        name: "Unused".into(),
        display_name: Some("\tA\rB\nC & 漢".into()),
        slots,
    })];
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    p.slides.push(Arc::new(Slide { id: SlideId(92), layout, show_master_shapes: false, ..Default::default() }));
    let bytes = export(&p).unwrap();
    assert!(!part(&bytes, "content.xml").contains("presentation:placeholder"));
    let back = import(&bytes).unwrap();
    assert!(!warning(&back), "{:?}", back.tags);
    assert!(back.slides[0].shapes.is_empty());
    assert_eq!(back.slides[0].presentation_template, None);
    assert_eq!(back.presentation_templates[0].display_name, p.presentation_templates[0].display_name);
    assert_eq!(back.presentation_templates[0].slots.len(), 2);
    for slot in &back.presentation_templates[0].slots {
        assert_eq!(slot.bounds.x.to_bits(), bounds.x.to_bits());
        assert_eq!(slot.bounds.y.to_bits(), bounds.y.to_bits());
    }
    assert_ne!(back.presentation_templates[0].slots[0].id, back.presentation_templates[0].slots[1].id);
}
