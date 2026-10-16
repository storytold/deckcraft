//! Contributor-original structured notes and hostile foreign-edit witnesses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

use deckcraft_model::{
    notes::{NotesDateTime, NotesFields, NotesMaster, NotesPage},
    text::{RunKind, TextBody},
    *,
};

fn shape(id: u32, role: PhType, xfrm: Option<Xfrm>) -> Shape {
    Shape {
        id: ShapeId(id),
        ph: Some(Placeholder { kind: role, idx: if role == PhType::Body { 1 } else { 0 }, ..Default::default() }),
        xfrm,
        ..Default::default()
    }
}

fn fixture() -> Presentation {
    let mut p = Presentation {
        masters: vec![Arc::new(Master {
            id: MasterId(10),
            name: "Original slide master".into(),
            layouts: vec![Layout { id: LayoutId(20), name: "Original layout".into(), show_master_shapes: true, ..Default::default() }],
            ..Default::default()
        })],
        slides: vec![Arc::new(Slide { id: SlideId(30), layout: LayoutId(20), ..Default::default() })],
        notes_master: None,
        ..Default::default()
    };
    p.notes_masters.clear();
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([37, 104, 172, 255])))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let media = p.add_media("notes-original.png", "image/png", png.into_inner()).unwrap();
    let mut body = shape(901, PhType::Body, Some(Xfrm::new(54., 270., 432., 447.125)));
    body.text = Some(TextBody::from_text(""));
    body.text.as_mut().unwrap().body.inset_l = Some(12.25);
    body.text.as_mut().unwrap().paragraphs[0].def_props.size = Some(17.);
    let mut footer = shape(902, PhType::Footer, Some(Xfrm::new(40., 690., 320., 20.)));
    let mut footer_text = TextBody::from_text("Cached footer");
    footer_text.paragraphs[0].runs[0].kind = RunKind::Field { field: "footer".into() };
    footer.text = Some(footer_text);
    let art = Shape {
        id: ShapeId(903),
        xfrm: Some(Xfrm::new(420., 40., 40., 40.)),
        kind: ShapeKind::Picture { fill: style::PictureFill { media, ..Default::default() } },
        ..Default::default()
    };
    let master = Master { id: MasterId(900), name: "Live original notes master".into(), shapes: vec![body, footer, art], ..Default::default() };
    p.notes_masters.push(NotesMaster {
        master: Arc::new(master),
        size: Size::new(540., 720.),
        fields: NotesFields {
            footer: Some("Shared footer".into()),
            display_footer: Some(true),
            date_time: Some(NotesDateTime::Current { format: "datetime7".into(), lang: Some("fr-BE".into()) }),
            ..Default::default()
        },
    });
    let mut second = p.notes_masters[0].clone();
    second.master = Arc::new(Master {
        id: MasterId(950),
        name: "Landscape original notes master".into(),
        shapes: vec![shape(951, PhType::Body, Some(Xfrm::new(50., 250., 600., 200.)))],
        ..Default::default()
    });
    second.size = Size::new(720., 540.);
    p.notes_masters.push(second);
    p.notes_masters.push(NotesMaster {
        master: Arc::new(Master {
            id: MasterId(970),
            name: "Unused original notes master".into(),
            shapes: vec![Shape {
                id: ShapeId(971),
                xfrm: Some(Xfrm::new(1., 2., 3., 4.)),
                kind: ShapeKind::Picture { fill: style::PictureFill { media, ..Default::default() } },
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });
    let prototype = (*p.slides[0]).clone();
    p.slides = (0..3)
        .map(|index| {
            let mut slide = prototype.clone();
            slide.id = SlideId(1000 + index * 10);
            slide.notes = TextBody::from_text(&format!("Speaker {}\nSecond paragraph", index + 1));
            slide.notes.paragraphs[0].runs[0].props.bold = Some(true);
            let body = 1100 + index * 10;
            slide.notes_page = Some(NotesPage {
                master: Some(MasterId(if index == 2 { 950 } else { 900 })),
                body_shape: Some(ShapeId(body)),
                shapes: vec![shape(body, PhType::Body, None), shape(body + 1, PhType::SlideImage, Some(Xfrm::new(54., 36., 432., 200.)))],
                fields: if index == 1 {
                    NotesFields { footer: Some(String::new()), display_footer: Some(false), ..Default::default() }
                } else {
                    NotesFields::default()
                },
                ..Default::default()
            });
            Arc::new(slide)
        })
        .collect();
    p.fix_next_id();
    p
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut file = zip.by_index(index).unwrap();
            let name = file.name().to_string();
            let mut data = vec![];
            file.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}

fn changed(bytes: &[u8], mut edit: impl FnMut(&str, &mut Vec<u8>) -> bool) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, mut data) in entries(bytes) {
        if !edit(&name, &mut data) {
            continue;
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn replace(bytes: &[u8], part: &str, old: &str, new: &str) -> Vec<u8> {
    let mut hits = 0;
    let out = changed(bytes, |name, data| {
        if name == part {
            let xml = String::from_utf8(data.clone()).unwrap();
            assert!(xml.contains(old));
            *data = xml.replacen(old, new, 1).into_bytes();
            hits += 1;
        }
        true
    });
    assert_eq!(hits, 1);
    out
}

fn flat_nodes(xml: &str) -> Vec<deckcraft_pptx::XmlEl> {
    let doc = deckcraft_pptx::parse_xml(xml.as_bytes()).unwrap();
    doc.root.find("graph").unwrap().children_named("node").cloned().collect()
}
fn flat_member_value(xml: &str, name: &str) -> (u32, String) {
    let nodes = flat_nodes(xml);
    let id =
        nodes.iter().flat_map(|node| node.children_named("member")).find(|member| member.attr("name") == Some(name)).unwrap().u32("ref").unwrap();
    let value = nodes.iter().find(|node| node.u32("id") == Some(id)).unwrap().text();
    (id, value)
}
fn flat_replace_scalar(xml: &str, id: u32, value: &str) -> String {
    let mut xml = xml.to_string();
    let tag = format!("<dc:node id=\"{id}\" ");
    let start = xml.find(&tag).unwrap();
    let begin = xml[start..].find('>').unwrap() + start + 1;
    let end = xml[begin..].find("</dc:node>").unwrap() + begin;
    xml.replace_range(begin..end, value);
    xml
}
fn flat_scalar_id(xml: &str, value: &str) -> u32 {
    flat_nodes(xml).iter().find(|node| node.elements().next().is_none() && node.text() == value).unwrap().u32("id").unwrap()
}

#[test]
fn structured_notes_keep_native_sharing_unused_resources_sizes_and_override_presence() {
    let source = fixture();
    let before = source.clone();
    for extension in ["pptx", "potx", "ppsx"] {
        let bytes = deckcraft_pptx::export_as(&source, extension).unwrap();
        emit_completion_package(&format!("structured-notes.{extension}"), &bytes);
        let restored = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(restored.notes_masters.len(), 3);
        assert!(restored.notes_master.is_none());
        let first = restored.slides[0].notes_page.as_ref().unwrap();
        let second = restored.slides[1].notes_page.as_ref().unwrap();
        let third = restored.slides[2].notes_page.as_ref().unwrap();
        assert_eq!(first.master, second.master);
        assert_ne!(first.master, third.master);
        assert_eq!(second.fields.footer.as_deref(), Some(""));
        assert_eq!(second.fields.display_footer, Some(false));
        assert_eq!(restored.notes_page_size(&restored.slides[2]), Size::new(720., 540.));
        assert!(restored.notes_masters.iter().any(|master| master.master.name == "Unused original notes master"));
        for (actual, original) in restored.slides.iter().zip(&source.slides) {
            assert_eq!(actual.notes, original.notes);
            assert!(restored.notes_page_error(actual).is_none());
            let page = actual.notes_page.as_ref().unwrap();
            assert!(find_shape(&page.shapes, page.body_shape.unwrap()).unwrap().text.is_none());
        }
        assert_eq!(restored.media.iter().filter(|item| item.name == "notes-original.png").count(), 1);
        let twice = deckcraft_pptx::import(&deckcraft_pptx::export(&restored).unwrap()).unwrap();
        assert_eq!(twice.slides[0].notes, source.slides[0].notes);
    }
    assert_eq!(source, before);
}

#[test]
fn core_projection_has_one_master_standard_live_roles_and_theme_overrides() {
    let parts = entries(&deckcraft_pptx::export(&fixture()).unwrap());
    assert_eq!(parts.iter().filter(|(name, _)| name.starts_with("ppt/notesMasters/") && name.ends_with(".xml")).count(), 1);
    for index in 1..=3 {
        let name = format!("ppt/notesSlides/notesSlide{index}.xml");
        let xml = String::from_utf8(parts.iter().find(|(part, _)| part == &name).unwrap().1.clone()).unwrap();
        assert!(xml.contains("type=\"sldImg\""));
        assert!(xml.contains("showMasterSp=\"0\""));
        let rel = format!("ppt/notesSlides/_rels/notesSlide{index}.xml.rels");
        let rel = String::from_utf8(parts.iter().find(|(part, _)| part == &rel).unwrap().1.clone()).unwrap();
        assert!(rel.contains("/themeOverride\""));
        assert!(rel.contains(&format!("../slides/slide{index}.xml")));
    }
    assert!(!parts.iter().any(|(name, _)| name.contains("thumbnail") || name.contains("live-notes-thumbnail")));
}

#[test]
fn actual_foreign_notes_text_edit_refuses_stale_metadata_instead_of_restoring_old_body() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    let edited = replace(&bytes, "ppt/notesSlides/notesSlide1.xml", "Speaker 1", "Foreign edited notes");
    assert!(deckcraft_pptx::import(&edited).unwrap_err().to_string().contains("projection changed"));
}

#[test]
fn metadata_alone_cannot_invent_master_geometry_even_with_matching_saved_core_hash() {
    let source = fixture();
    let bytes = deckcraft_pptx::export(&source).unwrap();
    let mut forged_source = source.clone();
    Arc::make_mut(&mut forged_source.notes_masters[0].master).shapes[0].xfrm.as_mut().unwrap().h = 449.125;
    let forged = deckcraft_pptx::export(&forged_source).unwrap();
    let resource_digest = |bytes: &[u8]| {
        let xml = String::from_utf8(entries(bytes).into_iter().find(|(name, _)| name == "ppt/notesSlides/notesSlide1.xml").unwrap().1).unwrap();
        flat_member_value(&xml, "resources").1
    };
    let old_digest = resource_digest(&bytes);
    let forged_digest = resource_digest(&forged);
    assert_ne!(old_digest, forged_digest);
    let edited = changed(&bytes, |name, data| {
        if name == "ppt/notesMasters/notesMaster1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            let id = flat_scalar_id(&xml, "447.125");
            *data = flat_replace_scalar(&xml, id, "449.125").into_bytes();
        } else if name.starts_with("ppt/notesSlides/notesSlide") && name.ends_with(".xml") {
            let xml = String::from_utf8(data.clone()).unwrap();
            assert!(xml.contains(&old_digest));
            *data = xml.replace(&old_digest, &forged_digest).into_bytes();
        }
        true
    });
    let error = deckcraft_pptx::import(&edited).unwrap_err().to_string();
    assert!(error.contains("metadata does not reproduce"), "{error}");
}

#[test]
fn changed_or_deleted_notes_media_never_restores_stale_native_images() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    for remove in [false, true] {
        let mut hits = 0;
        let edited = changed(&bytes, |name, data| {
            if name.starts_with("ppt/media/") && name.ends_with(".png") {
                hits += 1;
                if remove {
                    return false;
                }
                if let Some(byte) = data.last_mut() {
                    *byte ^= 1;
                }
            }
            true
        });
        assert!(hits > 0);
        assert!(deckcraft_pptx::import(&edited).is_err());
    }
}

#[test]
fn deleted_notes_part_is_not_resurrected_from_master_metadata() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    let edited = changed(&bytes, |name, _| name != "ppt/notesSlides/notesSlide1.xml");
    assert!(deckcraft_pptx::import(&edited).unwrap_err().to_string().contains("missing notes slide part"));
}

#[test]
fn extension_namespace_spoof_is_rejected_before_native_authority_attachment() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    let edited = replace(&bytes, "ppt/notesSlides/notesSlide1.xml", "xmlns:dc=\"urn:deckcraft:pptx:notes:1\"", "xmlns:dc=\"urn:attacker:notes\"");
    assert!(deckcraft_pptx::import(&edited).is_err());
}

#[test]
fn empty_canonical_notes_body_still_preserves_editable_decorated_page() {
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).notes = TextBody::default();
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    assert_eq!(back.slides[0].notes, TextBody::default());
    assert!(back.slides[0].notes_page.is_some());
}

#[test]
fn canonical_body_line_break_segmentation_and_vertical_tabs_roundtrip() {
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).notes = TextBody {
        paragraphs: vec![Paragraph {
            runs: vec![Run::new("One\nTwo\u{b}Three"), Run { kind: RunKind::Break, ..Run::new("") }, Run::new("Four")],
            ..Default::default()
        }],
        ..Default::default()
    };
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    assert_eq!(back.slides[0].notes, p.slides[0].notes);
}

#[test]
fn missing_extensions_leave_ordinary_editable_notes_and_live_previews() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    let stripped = changed(&bytes, |name, data| {
        if name.ends_with(".xml") {
            let mut xml = String::from_utf8(data.clone()).unwrap();
            while let Some(start) = xml.find("<p:ext uri=\"urn:deckcraft:pptx:notes:1\">") {
                let end = start + xml.get(start..).unwrap().find("</p:ext>").unwrap() + "</p:ext>".len();
                xml.replace_range(start..end, "");
            }
            *data = xml.into_bytes();
        }
        true
    });
    let edited = replace(&stripped, "ppt/notesSlides/notesSlide1.xml", "Speaker 1", "Edited core notes");
    let back = deckcraft_pptx::import(&edited).unwrap();
    assert!(back.slides[0].notes.text().contains("Edited core notes"));
    let mut preview = false;
    walk(&back.slides[0].notes_page.as_ref().unwrap().shapes, &mut |shape, _| preview |= shape.ph_type() == Some(PhType::SlideImage));
    assert!(preview);
}

#[test]
fn hostile_projection_counter_is_refused_before_regenerated_writer_arithmetic() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    let edited = changed(&bytes, |name, data| {
        if name == "ppt/notesSlides/notesSlide1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            let (id, value) = flat_member_value(&xml, "image");
            assert_eq!(value, "1");
            *data = flat_replace_scalar(&xml, id, "4294967295").into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&edited).unwrap_err().to_string().contains("counter exceeds"));
}

#[test]
fn global_footer_dependency_requires_a_notes_declaration() {
    let mut source = fixture();
    source.header_footer.footer = true;
    source.header_footer.footer_text = "Presentation footer".into();
    source.notes_masters[0].fields.footer = None;
    let error = deckcraft_pptx::export(&source).unwrap_err().to_string();
    assert!(error.contains("unbound presentation-wide footer"));
    source.notes_masters[0].fields.footer = Some("Explicit notes footer".into());
    let bytes = deckcraft_pptx::export(&source).unwrap();
    assert_eq!(deckcraft_pptx::import(&bytes).unwrap().notes_masters[0].fields.footer.as_deref(), Some("Explicit notes footer"));
}

fn emit_completion_package(name: &str, bytes: &[u8]) {
    if let Ok(directory) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}
