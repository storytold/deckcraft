//! Independent source fixtures for empty-frame retention and contextual policy authority.
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

use deckcraft_model::{Presentation, resolve, text::LineHeight};
use deckcraft_odp::{MIME, export, import};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";

fn style(name: &str, parent: Option<&str>, attrs: &str) -> String {
    let parent = parent.map(|name| format!(" style:parent-style-name='{name}'")).unwrap_or_default();
    format!(
        "<style:style style:name='{name}' style:family='paragraph'{parent}><style:paragraph-properties {attrs}/><style:text-properties fo:font-family='BIZ UDPGothic' fo:font-size='19pt'/></style:style>"
    )
}

fn assert_single_text_property_bag_per_style(bytes: &[u8]) {
    use quick_xml::{events::Event, name::ResolveResult, reader::NsReader};
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut seen = 0;
    for part in ["content.xml", "styles.xml"] {
        let mut xml = Vec::new();
        zip.by_name(part).unwrap().read_to_end(&mut xml).unwrap();
        let mut reader = NsReader::from_reader(xml.as_slice());
        reader.config_mut().expand_empty_elements = true;
        let mut stack: Vec<Option<usize>> = Vec::new();
        loop {
            match reader.read_event().unwrap() {
                Event::Start(el) => {
                    let (namespace, local) = reader.resolver().resolve_element(el.name());
                    let style_namespace =
                        matches!(namespace, ResolveResult::Bound(ns) if ns.as_ref() == b"urn:oasis:names:tc:opendocument:xmlns:style:1.0");
                    if style_namespace
                        && local.as_ref() == b"text-properties"
                        && let Some(Some(count)) = stack.last_mut()
                    {
                        *count += 1;
                    }
                    let style = style_namespace && local.as_ref() == b"style";
                    if style {
                        seen += 1;
                    }
                    stack.push(style.then_some(0));
                }
                Event::End(_) => {
                    if let Some(Some(count)) = stack.pop() {
                        assert!(count <= 1, "a generated style in {part} has duplicate text-properties bags");
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    assert!(seen > 0);
}

#[test]
fn notes_marker_bridge_keeps_sparse_shared_and_complete_unshared_latin_defaults() {
    use deckcraft_model::{
        Master, MasterId, PhType, Placeholder, Shape, ShapeId, TextBody, Xfrm,
        notes::{NotesMaster, NotesPage},
    };
    for shared in [false, true] {
        let mut p = Presentation::default();
        let mut parent_text = TextBody::from_text("");
        parent_text.paragraphs[0].def_props.font = Some("BIZ UDPGothic".into());
        parent_text.paragraphs[0].def_props.size = Some(19.0);
        parent_text.paragraphs[0].props.line_height = Some(LineHeight::Minimum { points: 24.0 });
        if shared {
            p.notes_masters.push(NotesMaster {
                master: Arc::new(Master {
                    id: MasterId(801),
                    shapes: vec![Shape {
                        id: ShapeId(802),
                        ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
                        text: Some(parent_text),
                        xfrm: Some(Xfrm::new(10.0, 100.0, 200.0, 180.0)),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        let slide = Arc::make_mut(&mut p.slides[0]);
        slide.notes = TextBody::from_text("Explicit equal default\n\nInherited default");
        for (index, paragraph) in slide.notes.paragraphs.iter_mut().enumerate() {
            if !shared || index == 0 {
                paragraph.def_props.font = Some("BIZ UDPGothic".into());
                paragraph.def_props.size = Some(19.0);
            }
        }
        slide.notes_page = Some(NotesPage {
            master: shared.then_some(MasterId(801)),
            body_shape: Some(ShapeId(803)),
            shapes: vec![Shape {
                id: ShapeId(803),
                ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
                xfrm: (!shared).then_some(Xfrm::new(10.0, 100.0, 200.0, 180.0)),
                ..Default::default()
            }],
            ..Default::default()
        });
        p.fix_next_id();
        let before = p.clone();
        for _ in 0..2 {
            let bytes = export(&p).unwrap();
            assert_single_text_property_bag_per_style(&bytes);
            p = import(&bytes).unwrap();
            clean(&p);
            let paragraphs = &p.slides[0].notes.paragraphs;
            assert_eq!(paragraphs[0].def_props.size, Some(19.0));
            assert_eq!(paragraphs[0].def_props.font.as_deref(), Some("BIZ UDPGothic"));
            assert!(paragraphs[1].runs.is_empty(), "default-style probes must not become real runs");
            for paragraph in &paragraphs[1..] {
                assert_eq!(paragraph.def_props.size, (!shared).then_some(19.0));
                assert_eq!(paragraph.def_props.font.as_deref(), (!shared).then_some("BIZ UDPGothic"));
            }
        }
        assert_eq!(before.slides[0].notes.text(), p.slides[0].notes.text());
        if shared {
            Arc::make_mut(&mut p.notes_masters[0].master).shapes[0].text.as_mut().unwrap().paragraphs[0].def_props.size = Some(31.0);
            let prepared = deckcraft_render::notes_page_document(&p, 0, &Default::default()).unwrap();
            let text = prepared.slides[0].shapes[0].text.as_ref().unwrap();
            assert_eq!(text.paragraphs[0].def_props.size, Some(19.0));
            assert_eq!(text.paragraphs[2].def_props.size, Some(31.0));
        }
    }
}

fn frame(paragraphs: &str) -> String {
    format!("<draw:frame svg:width='300pt' svg:height='150pt'><draw:text-box>{paragraphs}</draw:text-box></draw:frame>")
}

fn package(common: &str, automatic: &str, shapes: &str, master_notes: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles>{automatic}</office:automatic-styles><office:body><office:presentation><draw:page draw:name='Review' draw:master-page-name='Master'>{shapes}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles>{common}</office:styles><office:automatic-styles><style:page-layout style:name='Paper'><style:page-layout-properties fo:page-width='600pt' fo:page-height='400pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='Master' style:page-layout-name='Paper'>{master_notes}</style:master-page></office:master-styles></office:document-styles>"
    );
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='styles.xml' manifest:media-type='text/xml'/></manifest:manifest>"
    );
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in
        [("mimetype", MIME), ("content.xml", content.as_str()), ("styles.xml", styles.as_str()), ("META-INF/manifest.xml", manifest.as_str())]
    {
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(data.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn clean(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning" || key == "odp:semantic-audit"), "{:?}", p.tags);
}

#[test]
fn wholly_empty_policy_frame_and_typing_defaults_survive_native_rewrite() {
    for (attrs, policy) in [
        ("style:line-height-at-least='27.123456789pt'", LineHeight::Minimum { points: 27.123456789 }),
        ("fo:line-height='normal'", LineHeight::Normal),
    ] {
        let p = import(&package("", &style("P", None, attrs), &frame("<text:p text:style-name='P'/>"), "")).unwrap();
        clean(&p);
        assert_eq!(p.slides[0].shapes.len(), 1, "an explicit policy makes an empty frame meaningful");
        let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        let reopened = import(&export(&native).unwrap()).unwrap();
        clean(&reopened);
        for deck in [&p, &reopened] {
            let body = deck.slides[0].shapes[0].text.as_ref().unwrap();
            assert_eq!(body.paragraphs.len(), 1);
            assert!(body.paragraphs[0].is_empty());
            assert_eq!(body.paragraphs[0].props.line_height, Some(policy));
            assert_eq!(body.paragraphs[0].def_props.font.as_deref(), Some("BIZ UDPGothic"));
            assert_eq!(body.paragraphs[0].def_props.size, Some(19.0));
        }
    }
}

#[test]
fn unrelated_normal_paragraph_cannot_launder_an_inherited_mixed_style() {
    let common = style("Ancestor", None, "style:line-spacing='-2pt'");
    let automatic =
        format!("{}{}", style("Normal", None, "fo:line-height='normal'"), style("Guarded", Some("Ancestor"), "style:line-height-at-least='26pt'"));
    let source = package(&common, &automatic, &frame("<text:p text:style-name='Normal'>Original</text:p>"), "");
    let mut p = import(&source).unwrap();
    assert_eq!(p.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].props.line_height, Some(LineHeight::Normal));
    assert!(p.tags.iter().any(|(key, _)| key == "odp:semantic-audit"));
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Edited".into();
    assert!(export(&p).is_err());
    assert!(p.media.iter().any(|media| media.content_type == "application/vnd.deckcraft.odp-source" && media.data.as_slice() == source));
}

fn notes_effective_policy(p: &Presentation) -> Option<LineHeight> {
    let slide = &p.slides[0];
    let page = slide.notes_page.as_ref().unwrap();
    let frame = page.shapes.iter().find(|shape| Some(shape.id) == page.body_shape).unwrap();
    let ctx = resolve::Ctx::for_notes(p, slide).unwrap();
    // Canonical notes are attached to the frame transiently by the notes renderer/writer.
    // Their master paragraph defaults are inherited before ordinary text resolution.
    let mut paragraph = slide.notes.paragraphs[0].clone();
    if let Some(parent) = p
        .notes_master_for(slide)
        .unwrap()
        .shapes
        .iter()
        .find(|shape| shape.ph_type() == Some(deckcraft_model::PhType::Body))
        .and_then(|shape| shape.text.as_ref())
        .and_then(|text| text.paragraphs.iter().find(|parent| parent.level == paragraph.level).or_else(|| text.paragraphs.first()))
    {
        paragraph.props.inherit(&parent.props);
    }
    resolve::para(&ctx, frame, &paragraph).line_height
}

#[test]
fn notes_local_minimum_cannot_reenable_a_modifier_disabled_by_shared_normal() {
    let common = format!("{}<style:style style:name='Role' style:family='presentation'/>", style("Shared", None, "fo:line-height='normal'"));
    let automatic = style("Local", Some("Shared"), "style:line-height-at-least='31pt'");
    let notes_frame = |paragraph: &str| {
        format!(
            "<draw:frame presentation:class='notes' presentation:style-name='Role' draw:text-style-name='Shared' svg:x='20pt' svg:y='100pt' svg:width='300pt' svg:height='180pt'><draw:text-box>{paragraph}</draw:text-box></draw:frame>"
        )
    };
    let master_notes = format!("<presentation:notes>{}</presentation:notes>", notes_frame("<text:p text:style-name='Shared'/>"));
    let shapes = format!(
        "{}<presentation:notes>{}</presentation:notes>",
        frame("<text:p>Slide</text:p>"),
        notes_frame("<text:p text:style-name='Local'>Spoken</text:p>")
    );
    let original = package(&common, &automatic, &shapes, &master_notes);
    let p = import(&original).unwrap();
    const WARNING: &str = "ODF notes local line-height modifiers depend on shared Normal; editable ownership is incomplete; original source retained and edited export blocked";
    assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value == WARNING));
    let local = &p.slides[0].notes.paragraphs[0].props;
    assert_eq!(local.line_height, None, "dormant minimum remains source-only rather than becoming a local active policy");
    assert_eq!(local.line_spacing, None);
    assert_eq!(p.notes_masters[0].master.shapes[0].text.as_ref().unwrap().paragraphs[0].props.line_height, Some(LineHeight::Normal));
    assert_eq!(
        notes_effective_policy(&p),
        Some(LineHeight::Normal),
        "full ODF inheritance must remain authoritative after local ownership reconstruction"
    );
    let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    assert_eq!(native, p, "the precise source-dependent guard and source archive survive native persistence");
    assert_eq!(notes_effective_policy(&native), Some(LineHeight::Normal));
    for edit_kind in 0..3 {
        let mut edited = native.clone();
        match edit_kind {
            0 => Arc::make_mut(&mut edited.slides[0]).notes.paragraphs[0].runs[0].text = "Edited notes".into(),
            1 => Arc::make_mut(&mut edited.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Unrelated slide edit".into(),
            _ => {
                Arc::make_mut(&mut edited.notes_masters[0].master).shapes[0].text.as_mut().unwrap().paragraphs[0].props.line_height =
                    Some(LineHeight::Minimum { points: 24.0 })
            }
        }
        let before = edited.clone();
        assert!(export(&edited).is_err(), "source-dependent notes ownership must refuse edit kind {edit_kind}");
        assert_eq!(edited, before, "refused export must leave the source model unchanged");
        assert!(
            edited.media.iter().any(|media| media.content_type == "application/vnd.deckcraft.odp-source" && media.data.as_slice() == original),
            "exact original bytes remain recoverable"
        );
        assert!(edited.tags.iter().any(|(key, value)| key == "odp:warning" && value == WARNING));
    }
}
