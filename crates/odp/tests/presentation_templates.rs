//! Contributor-original resources from ODF 1.3 §§16.43–44, 19.407, 19.413.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::*;
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0'";
fn package(content: &str, styles: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", content),
        ("styles.xml", styles),
        (
            "META-INF/manifest.xml",
            "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='application/vnd.oasis.opendocument.presentation'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='styles.xml' manifest:media-type='text/xml'/></manifest:manifest>",
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn source(defs: &str, pages: &str) -> Vec<u8> {
    package(
        &format!(
            "<office:document-content {NS} office:version='1.3'><office:body><office:presentation>{pages}</office:presentation></office:body></office:document-content>"
        ),
        &format!("<office:document-styles {NS} office:version='1.3'><office:styles>{defs}</office:styles></office:document-styles>"),
    )
}
fn page(attrs: &str, objects: &str) -> String {
    format!("<draw:page draw:name='Page' {attrs}>{objects}</draw:page>")
}
fn slot(role: &str, x: &str) -> String {
    format!("<presentation:placeholder presentation:object='{role}' svg:x='{x}' svg:y='2pt' svg:width='30pt' svg:height='40pt'/>")
}
fn definition(name: &str, slots: &str) -> String {
    format!("<style:presentation-page-layout style:name='{name}'>{slots}</style:presentation-page-layout>")
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();
    out
}
fn no_warning(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}
fn guarded(bytes: &[u8]) -> Presentation {
    let p = import(bytes).unwrap();
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "unrepresented semantics must remain guarded");
    assert!(p.media.iter().any(|m| m.content_type == "application/vnd.deckcraft.odp-source" && m.data.as_slice() == bytes));
    p
}
fn selected(p: &Presentation) -> Vec<Option<String>> {
    p.slides
        .iter()
        .map(|slide| slide.presentation_template.map(|id| p.presentation_templates.iter().find(|template| template.id == id).unwrap().name.clone()))
        .collect()
}
fn semantic(p: &Presentation) -> Vec<(String, Option<String>, Vec<(PresentationRole, PresentationTemplateBox)>)> {
    p.presentation_templates.iter().map(|t| (t.name.clone(), t.display_name.clone(), t.slots.iter().map(|s| (s.role, s.bounds)).collect())).collect()
}
fn native() -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(320., 180.), Theme::default(), false);
    let roles = [
        PresentationRole::Title,
        PresentationRole::Outline,
        PresentationRole::Subtitle,
        PresentationRole::Text,
        PresentationRole::Graphic,
        PresentationRole::Object,
        PresentationRole::Chart,
        PresentationRole::Table,
        PresentationRole::OrgChart,
        PresentationRole::Page,
        PresentationRole::Notes,
        PresentationRole::Handout,
        PresentationRole::Header,
        PresentationRole::Footer,
        PresentationRole::DateTime,
        PresentationRole::PageNumber,
        PresentationRole::Text,
        PresentationRole::Text,
    ];
    let slots: Vec<_> = roles
        .into_iter()
        .enumerate()
        .map(|(i, role)| PresentationTemplateSlot {
            id: PresentationTemplateSlotId(50_010 + i as u32),
            role,
            bounds: PresentationTemplateBox::new(i as f64 - 4., i as f64, 40., 20.).unwrap(),
        })
        .collect();
    p.presentation_templates = vec![
        Arc::new(PresentationTemplate {
            id: PresentationTemplateId(50_000),
            name: "A".into(),
            display_name: Some("Same\tlabel\nline\rreturn & < > ' \" 漢".into()),
            slots: slots.clone(),
        }),
        Arc::new(PresentationTemplate {
            id: PresentationTemplateId(60_000),
            name: "B".into(),
            display_name: Some("Same\tlabel\nline\rreturn & < > ' \" 漢".into()),
            slots,
        }),
        Arc::new(PresentationTemplate {
            id: PresentationTemplateId(70_000),
            name: "Unused".into(),
            display_name: Some(String::new()),
            slots: vec![],
        }),
    ];
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    for (i, id) in [50_000, 50_000, 60_000].into_iter().enumerate() {
        p.slides.push(Arc::new(Slide {
            id: SlideId(90_000 + i as u32),
            layout,
            presentation_template: Some(PresentationTemplateId(id)),
            show_master_shapes: false,
            shapes: vec![Shape {
                id: ShapeId(91_000 + i as u32),
                xfrm: Some(Xfrm::new(20., 30., 80., 40.)),
                fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(30, 90, 150)))),
                ..Default::default()
            }],
            ..Default::default()
        }));
    }
    p.next_id = 92_000;
    p
}

#[test]
fn equal_distinct_a_a_b_unused_all_roles_order_and_labels_reopen_as_resources() {
    let p = native();
    let native_back = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    assert_eq!(native_back.presentation_templates, p.presentation_templates);
    assert_eq!(
        native_back.slides.iter().map(|s| s.presentation_template).collect::<Vec<_>>(),
        p.slides.iter().map(|s| s.presentation_template).collect::<Vec<_>>()
    );
    let bytes = export(&p).unwrap();
    let xml = part(&bytes, "styles.xml");
    assert_eq!(xml.matches("<style:presentation-page-layout ").count(), 3);
    assert_eq!(xml.matches("<presentation:placeholder ").count(), 36);
    for reference in ["&#9;", "&#10;", "&#13;"] {
        assert!(xml.contains(reference));
    }
    assert!(!part(&bytes, "content.xml").contains("<presentation:placeholder"));
    let back = import(&bytes).unwrap();
    no_warning(&back);
    assert_eq!(semantic(&back), semantic(&p));
    assert_eq!(selected(&back), vec![Some("A".into()), Some("A".into()), Some("B".into())]);
    assert_ne!(back.presentation_templates[0].id, back.presentation_templates[1].id);
    assert_ne!(back.presentation_templates[0].slots[16].id, back.presentation_templates[0].slots[17].id);
    assert_ne!(back.presentation_templates[0].id, p.presentation_templates[0].id, "ODP stores no native resource ids");
    assert_eq!(back.slides.iter().map(|s| s.shapes.len()).collect::<Vec<_>>(), vec![1, 1, 1]);
    export(&back).unwrap();
    if let Some(dir) = std::env::var_os("DECKCRAFT_TEMPLATE_XML_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml", "META-INF/manifest.xml"] {
            std::fs::write(dir.join(name.replace('/', "_")), part(&bytes, name)).unwrap();
        }
    }
}

#[test]
fn selection_and_resource_edits_do_not_apply_boxes_or_change_actual_render_pixels() {
    let mut p = native();
    let shapes: Vec<_> = p.slides.iter().map(|s| s.shapes.clone()).collect();
    let layout: Vec<_> = p.slides.iter().map(|s| s.layout).collect();
    let pixels = deckcraft_render::render_slide(&p, 0, &Default::default()).pixels;
    Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds = PresentationTemplateBox::new(1000., 2000., 80., 90.).unwrap();
    Arc::make_mut(&mut p.presentation_templates[0]).slots.swap(16, 17);
    Arc::make_mut(&mut p.slides[0]).presentation_template = Some(p.presentation_templates[1].id);
    assert_eq!(p.slides.iter().map(|s| s.shapes.clone()).collect::<Vec<_>>(), shapes);
    assert_eq!(p.slides.iter().map(|s| s.layout).collect::<Vec<_>>(), layout);
    assert_eq!(deckcraft_render::render_slide(&p, 0, &Default::default()).pixels, pixels);
}

#[test]
fn zero_resources_missing_and_explicit_empty_selections_never_invent_a_template() {
    let empty = source("", &page("", ""));
    let p = import(&empty).unwrap();
    no_warning(&p);
    assert!(p.presentation_templates.is_empty());
    let bytes = source(&definition("Unused", ""), &(page("", "") + &page("presentation:presentation-page-layout-name=''", "")));
    let p = import(&bytes).unwrap();
    no_warning(&p);
    assert_eq!(p.presentation_templates.len(), 1);
    assert_eq!(selected(&p), vec![None, None]);
    let p = import(&export(&p).unwrap()).unwrap();
    no_warning(&p);
    assert_eq!(selected(&p), vec![None, None]);
}

#[test]
fn real_namespace_aliases_and_physical_units_preserve_ordered_boxes() {
    let defs = definition("名", &slot("text", "-2.54cm"));
    let bytes = source(&defs, &page("presentation:presentation-page-layout-name='名'", ""));
    let alias = |xml: String, from: &str, to: &str| {
        xml.replace(&format!("<{from}:"), &format!("<{to}:"))
            .replace(&format!("</{from}:"), &format!("</{to}:"))
            .replace(&format!(" {from}:"), &format!(" {to}:"))
            .replace(&format!("xmlns:{from}="), &format!("xmlns:{to}="))
    };
    let content = alias(part(&bytes, "content.xml"), "presentation", "p");
    let styles = alias(alias(part(&bytes, "styles.xml"), "style", "s"), "presentation", "p");
    let p = import(&package(&content, &styles)).unwrap();
    no_warning(&p);
    assert!((p.presentation_templates[0].slots[0].bounds.x + 72.).abs() < 1e-12);
    assert_eq!(selected(&p), vec![Some("名".into())]);
}

#[test]
fn unsupported_coordinate_role_attribute_and_child_forms_remain_source_guarded() {
    for slots in [
        slot("text", "10%"),
        slot("text", "10px"),
        slot("text", "1e2pt"),
        slot("unknown", "1pt"),
        slot("text", "1pt").replace("svg:height='40pt'", ""),
        slot("text", "1pt").replace("/>", " draw:name='extra'/>"),
        slot("text", "1pt").replace("/>", "><text:p>extra</text:p></presentation:placeholder>"),
    ] {
        let bytes = source(&definition("A", &slots), &page("presentation:presentation-page-layout-name='A'", ""));
        let p = guarded(&bytes);
        assert!(p.presentation_templates.is_empty());
        assert_eq!(selected(&p), vec![None]);
    }
}

#[test]
fn known_template_definitions_in_wrong_context_are_not_admitted() {
    let bytes = source(&definition("A", &slot("text", "1pt")), &page("presentation:presentation-page-layout-name='A'", ""));
    let styles = part(&bytes, "styles.xml").replace("office:styles>", "office:automatic-styles>");
    let p = guarded(&package(&part(&bytes, "content.xml"), &styles));
    assert!(p.presentation_templates.is_empty());
    let styles = part(&bytes, "styles.xml")
        .replace("<office:styles>", "<office:styles><style:style style:name='Outer' style:family='graphic'>")
        .replace("</office:styles>", "</style:style></office:styles>");
    let p = guarded(&package(&part(&bytes, "content.xml"), &styles));
    assert!(p.presentation_templates.is_empty());
}

#[test]
fn foreign_namespace_lookalikes_do_not_acquire_resource_or_reference_ownership() {
    let bytes = source(&definition("A", &slot("text", "1pt")), &page("", ""));
    let styles = part(&bytes, "styles.xml").replace("xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0'", "xmlns:style='urn:foreign'");
    let p = guarded(&package(&part(&bytes, "content.xml"), &styles));
    assert!(p.presentation_templates.is_empty());
    let styles = part(&bytes, "styles.xml");
    let content = part(&bytes, "content.xml")
        .replace("xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'", "xmlns:presentation='urn:foreign'")
        .replace("<draw:page ", "<draw:page presentation:presentation-page-layout-name='A' ");
    let p = guarded(&package(&content, &styles));
    assert_eq!(p.presentation_templates.len(), 1);
    assert_eq!(selected(&p), vec![None]);
}

#[test]
fn duplicate_dangling_and_malformed_names_fail_precisely() {
    let a = definition("A", "");
    for (defs, attrs, message) in [
        (format!("{a}{a}"), "", "duplicate presentation template resource name"),
        (a.clone(), "presentation:presentation-page-layout-name='Missing'", "dangling presentation template reference"),
        (definition("bad name", ""), "", "bounded NCName"),
        (a.clone(), "presentation:presentation-page-layout-name='bad name'", "bounded NCName"),
    ] {
        let error = import(&source(&defs, &page(attrs, ""))).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn regular_page_object_semantics_remain_guarded_independently_of_template_support() {
    for attr in
        ["presentation:class='text'", "presentation:placeholder='false'", "presentation:user-transformed='true'", "presentation:class-names='S'"]
    {
        let object = format!(
            "<draw:frame {attr} svg:x='0pt' svg:y='0pt' svg:width='10pt' svg:height='10pt'><draw:text-box><text:p>cached</text:p></draw:text-box></draw:frame>"
        );
        for selected_template in [false, true] {
            let defs = if selected_template { definition("A", "") } else { String::new() };
            let reference = if selected_template { "presentation:presentation-page-layout-name='A'" } else { "" };
            let p = guarded(&source(&defs, &page(reference, &object)));
            assert_eq!(p.presentation_templates.len(), usize::from(selected_template));
            assert_eq!(selected(&p), vec![selected_template.then(|| "A".into())]);
        }
    }
    let bytes = source(
        &definition("A", ""),
        &page(
            "presentation:presentation-page-layout-name='A'",
            "<presentation:notes><draw:frame presentation:class='notes' svg:x='0pt' svg:y='0pt' svg:width='100pt' svg:height='100pt'><draw:text-box><text:p>canonical notes</text:p></draw:text-box></draw:frame></presentation:notes>",
        ),
    );
    let p = import(&bytes).unwrap();
    no_warning(&p);
    assert_eq!(p.slides[0].notes.text(), "canonical notes");
}

#[test]
fn page_presentation_style_ownership_is_guarded_even_if_it_can_be_flattened() {
    let defs = format!("{}<style:style style:name='S' style:family='presentation'/>", definition("A", ""));
    let bytes = source(
        &defs,
        &page(
            "presentation:presentation-page-layout-name='A'",
            "<draw:frame presentation:style-name='S' svg:x='0pt' svg:y='0pt' svg:width='10pt' svg:height='10pt'><draw:text-box><text:p>cached</text:p></draw:text-box></draw:frame>",
        ),
    );
    let p = guarded(&bytes);
    assert_eq!(p.presentation_templates.len(), 1);
}

#[test]
fn handout_and_nonpage_references_remain_guarded_while_named_definitions_survive() {
    let bytes =
        source(&definition("A", ""), &page("", "<draw:frame presentation:presentation-page-layout-name='A' svg:width='10pt' svg:height='10pt'/>"));
    let p = guarded(&bytes);
    assert_eq!(p.presentation_templates.len(), 1);
    assert_eq!(selected(&p), vec![None]);
    let styles=part(&bytes,"styles.xml").replace("</office:document-styles>","<office:automatic-styles><style:page-layout style:name='P'/></office:automatic-styles><office:master-styles><style:handout-master style:page-layout-name='P' presentation:presentation-page-layout-name='A'/></office:master-styles></office:document-styles>");
    let p = guarded(&package(&source_content(&page("", "")), &styles));
    assert_eq!(p.presentation_templates.len(), 1);
}
fn source_content(pages: &str) -> String {
    format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation>{pages}</office:presentation></office:body></office:document-content>"
    )
}

#[test]
fn hostile_slot_count_and_name_limits_fail_before_expanded_model_allocations() {
    let slots = slot("text", "1pt").repeat(257);
    assert!(import(&source(&definition("A", &slots), &page("", ""))).unwrap_err().to_string().contains("slot count exceeds limit"));
    let name = "A".repeat(8193);
    assert!(import(&source(&definition(&name, ""), &page("", ""))).unwrap_err().to_string().contains("XML attributes exceed size limit"));
}

#[test]
fn direct_export_rejects_dangling_duplicate_and_nonfinite_model_resources() {
    for mode in 0..4 {
        let mut p = native();
        match mode {
            0 => Arc::make_mut(&mut p.slides[0]).presentation_template = Some(PresentationTemplateId(999)),
            1 => Arc::make_mut(&mut p.presentation_templates[1]).name = "A".into(),
            2 => Arc::make_mut(&mut p.presentation_templates[1]).id = p.presentation_templates[0].id,
            _ => Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds.x = f64::NAN,
        }
        assert!(export(&p).is_err());
    }
}
