//! Contributor-original unused notes-layout resources and hostile package edits.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    table::{CellBlock, CellConstant, CellConstantState, CellValue},
    text::{Action, Hyperlink},
    *,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

fn fixture(named: bool, notes: bool) -> Presentation {
    let mut p = defaults::new_presentation(None);
    p.notes_master = None;
    p.notes_masters.clear();
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([23, 91, 137, 255])))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let media = p.add_media("unused-layout-original.png", "image/png", png.into_inner()).unwrap();
    Arc::make_mut(&mut p.slides[0]).id = SlideId(90_000);
    let target = p.slides[0].id;
    Arc::make_mut(&mut p.slides[0]).notes = TextBody::from_text(if notes { "Actual canonical notes" } else { "" });
    let mut t = Table::new(2, 2, 200., 80.);
    let cell = t.cell_mut(0, 0).unwrap();
    cell.text = TextBody::from_text("independent boolean label");
    cell.data = Some(Arc::new(CellConstant::from_display(CellValue::Boolean { value: true }, &cell.text).unwrap()));
    cell.text.paragraphs[0].runs[0].props.link = Some(Hyperlink {
        action: Action::Slide { slide: target },
        tooltip: String::new(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: Some(target),
        sound_name: None,
    });
    let mut child = Table::new(1, 1, 70., 30.);
    child.cell_mut(0, 0).unwrap().text = TextBody::from_text("unused nested original");
    cell.append_nested_table(child).unwrap();
    cell.append_text_block(TextBody::from_text("ordered continuation")).unwrap();
    t.set_cell_formula_v2(1, 1, "=2+3").unwrap();
    let picture = style::PictureFill { media, ..Default::default() };
    let shapes = vec![
        Shape { id: ShapeId(950), xfrm: Some(Xfrm::new(0., 0., 200., 80.)), kind: ShapeKind::Table(t), ..Default::default() },
        Shape {
            id: ShapeId(951),
            xfrm: Some(Xfrm::new(210., 0., 20., 20.)),
            kind: ShapeKind::Picture { fill: picture.clone() },
            ..Default::default()
        },
        Shape { id: ShapeId(952), kind: ShapeKind::Connector { start: Some((ShapeId(950), 1)), end: Some((ShapeId(951), 2)) }, ..Default::default() },
    ];
    let master = Arc::new(Master {
        id: MasterId(800),
        name: "Original notes owner".into(),
        shapes: vec![Shape {
            id: ShapeId(900),
            xfrm: Some(Xfrm::new(20., 100., 400., 300.)),
            ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
            text: Some(TextBody::from_text("Original master prompt")),
            ..Default::default()
        }],
        layouts: vec![Layout {
            id: LayoutId(801),
            name: "Unused original notes layout".into(),
            background: Some(Background::Fill { fill: style::Fill::Picture(picture) }),
            shapes: vec![
                Shape { id: ShapeId(949), kind: ShapeKind::Group { children: shapes, child: Xfrm::new(0., 0., 240., 100.) }, ..Default::default() },
                Shape {
                    id: ShapeId(953),
                    ph: Some(Placeholder { kind: PhType::Title, ..Default::default() }),
                    text: Some(TextBody::from_text("Unused ordinary layout title")),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    });
    if named {
        p.notes_masters.push(notes::NotesMaster { master, ..Default::default() });
    } else {
        p.notes_master = Some(master);
    }
    p.fix_next_id();
    p
}
fn master(p: &Presentation) -> &Master {
    p.notes_master.as_deref().or_else(|| p.notes_masters.first().map(|notes| notes.master.as_ref())).unwrap()
}
fn check(p: &Presentation, notes: bool) {
    let m = master(p);
    assert_eq!(m.name, "Original notes owner");
    assert_eq!(m.shapes[0].text.as_ref().unwrap().text(), "Original master prompt");
    assert_eq!(m.layouts.len(), 1);
    let l = &m.layouts[0];
    assert_eq!(l.name, "Unused original notes layout");
    assert_eq!(l.shapes[1].ph_type(), Some(PhType::Title));
    assert_eq!(l.shapes[1].text.as_ref().unwrap().text(), "Unused ordinary layout title");
    let ShapeKind::Group { children, .. } = &l.shapes[0].kind else { panic!("group") };
    let ShapeKind::Table(t) = &children[0].kind else { panic!("table") };
    t.validate_content().unwrap();
    t.validate_formulas().unwrap();
    let owner = t.cell(0, 0).unwrap();
    assert!(matches!(owner.constant_state().unwrap(), CellConstantState::Current(data) if data.value() == &CellValue::Boolean { value: true }));
    assert_eq!(owner.text.text(), "independent boolean label");
    assert_eq!(owner.blocks.len(), 2);
    assert!(matches!(&owner.blocks[0], CellBlock::Table(child) if child.table.cell(0,0).unwrap().text.text() == "unused nested original"));
    assert!(matches!(&owner.blocks[1], CellBlock::Text(tail) if tail.body.text() == "ordered continuation"));
    assert_eq!(t.cell(1, 1).unwrap().text.text(), "5");
    assert_eq!(t.cell(1, 1).unwrap().formula.as_ref().unwrap().expression(), "=2+3");
    let link = owner.text.paragraphs[0].runs[0].props.link.as_ref().unwrap();
    assert_eq!(link.action, Action::Slide { slide: p.slides[0].id });
    assert_eq!(link.zoom_return, Some(p.slides[0].id));
    let ShapeKind::Picture { fill } = &children[1].kind else { panic!("picture") };
    let media = p.media(fill.media).unwrap();
    assert_eq!(media.name, "unused-layout-original.png");
    assert!(media.data.starts_with(b"\x89PNG"));
    assert!(matches!(&l.background, Some(Background::Fill { fill: style::Fill::Picture(bg) }) if bg.media == fill.media));
    assert!(
        matches!(&children[2].kind, ShapeKind::Connector { start: Some((a,1)), end: Some((b,2)) } if *a == children[0].id && *b == children[1].id)
    );
    assert_eq!(p.slides[0].notes.text(), if notes { "Actual canonical notes" } else { "" });
}
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}
fn changed(bytes: &[u8], mut edit: impl FnMut(&str, &mut Vec<u8>) -> bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, mut data) in entries(bytes) {
        if edit(&name, &mut data) {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(&data).unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}
fn member_ref(xml: &str, member: &str) -> u32 {
    let root = deckcraft_pptx::parse_xml(xml.as_bytes()).unwrap().root;
    root.find("graph")
        .unwrap()
        .children_named("node")
        .flat_map(|node| node.children_named("member"))
        .find(|value| value.attr("name") == Some(member))
        .unwrap()
        .u32("ref")
        .unwrap()
}
fn scalar(xml: &str, id: u32, value: &str) -> String {
    let mut xml = xml.to_string();
    let start = xml.find(&format!("<dc:node id=\"{id}\" ")).unwrap();
    let start = xml[start..].find('>').unwrap() + start + 1;
    let end = xml[start..].find("</dc:node>").unwrap() + start;
    xml.replace_range(start..end, value);
    xml
}

#[test]
fn unused_layout_resources_keep_typed_authority_media_links_connectors_and_original_body() {
    for named in [false, true] {
        for notes in [false, true] {
            for ext in ["pptx", "potx", "ppsx"] {
                let p = fixture(named, notes);
                let original = p.clone();
                let bytes = deckcraft_pptx::export_as(&p, ext).unwrap();
                if named
                    && notes
                    && let Ok(directory) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR")
                {
                    std::fs::create_dir_all(&directory).unwrap();
                    std::fs::write(std::path::Path::new(&directory).join(format!("unused-notes-layout-resources.{ext}")), &bytes).unwrap();
                }
                let parts = entries(&bytes);
                assert!(!parts.iter().any(|(name, _)| name.contains("notesLayout")), "native unused metadata must not invent a DrawingML peer");
                let back = deckcraft_pptx::import(&bytes).unwrap();
                assert_ne!(back.slides[0].id, p.slides[0].id, "fixture must exercise slide-ID remapping");
                check(&back, notes);
                let twice = deckcraft_pptx::import(&deckcraft_pptx::export_as(&back, ext).unwrap()).unwrap();
                check(&twice, notes);
                assert!(p == original, "export mutated its canonical unused source");
            }
        }
    }
}
#[test]
fn unused_layout_rejects_invalid_ids_raw_extensions_geometry_background_and_zoom_target() {
    for case in 0..7 {
        let mut p = fixture(false, false);
        let original_layout = master(&p).layouts[0].clone();
        let m = Arc::make_mut(p.notes_master.as_mut().unwrap());
        match case {
            0 => m.layouts.push(original_layout),
            1 => m.layouts[0].id = LayoutId(0),
            2 => m.layouts[0].raw_ext = Some("<foreign/>".into()),
            3 => m.layouts[0].shapes[0].xfrm = Some(Xfrm::new(f64::NAN, 0., 1., 1.)),
            4 => m.layouts[0].background = Some(Background::Ref { idx: 9999, color: style::ColorRef::rgb(deckcraft_color::Rgba::rgb(1, 2, 3)) }),
            5 => {
                let ShapeKind::Group { children, .. } = &mut m.layouts[0].shapes[0].kind else { panic!("group") };
                children[1].id = children[0].id;
            }
            _ => {
                let ShapeKind::Group { children, .. } = &mut m.layouts[0].shapes[0].kind else { panic!("group") };
                let ShapeKind::Table(t) = &mut children[0].kind else { panic!("table") };
                t.cell_mut(0, 0).unwrap().text.paragraphs[0].runs[0].props.link.as_mut().unwrap().zoom_return = Some(SlideId(999_999));
            }
        }
        let before = serde_json::to_vec(&p).unwrap();
        assert!(matches!(deckcraft_pptx::export(&p), Err(deckcraft_pptx::PptxError::Unsupported(_))), "case {case}");
        assert_eq!(serde_json::to_vec(&p).unwrap(), before, "refusal mutated case {case}");
    }
}
#[test]
fn unused_layout_media_deletion_and_foreign_page_edit_cannot_restore_hidden_resources() {
    let bytes = deckcraft_pptx::export(&fixture(true, true)).unwrap();
    let missing = changed(&bytes, |name, _| !name.starts_with("ppt/media/"));
    assert!(deckcraft_pptx::import(&missing).is_err());
    let edited = changed(&bytes, |name, data| {
        if name == "ppt/notesSlides/notesSlide1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            assert!(xml.contains("Actual canonical notes"));
            *data = xml.replace("Actual canonical notes", "Foreign current body").into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&edited).is_err());
}
#[test]
fn unused_layout_cannot_claim_projected_authority_even_without_a_page_digest() {
    let bytes = deckcraft_pptx::export(&fixture(false, false)).unwrap();
    let forged = changed(&bytes, |name, data| {
        if name == "ppt/notesMasters/notesMaster1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            let id = member_ref(&xml, "projected");
            *data = scalar(&xml, id, "true").into_bytes();
        }
        true
    });
    let error = deckcraft_pptx::import(&forged).unwrap_err().to_string();
    assert!(error.contains("unused notes layout table cannot acquire a projected authority"), "{error}");
}
#[test]
fn forged_unused_grid_and_freshly_rehashed_changed_neutral_master_are_rejected() {
    let bytes = deckcraft_pptx::export(&fixture(false, false)).unwrap();
    let malformed = changed(&bytes, |name, data| {
        if name == "ppt/notesMasters/notesMaster1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            let id = member_ref(&xml, "gridSpan");
            *data = scalar(&xml, id, "2").into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&malformed).is_err());
    let forged = changed(&bytes, |name, data| {
        if name == "ppt/notesMasters/notesMaster1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap().replace("DeckCraft notes projection", "Forged neutral projection");
            let mut core = xml.clone();
            let start = core.find("<p:extLst>").unwrap();
            let end = core.find("</p:extLst>").unwrap() + "</p:extLst>".len();
            core.replace_range(start..end, "");
            let canonical = deckcraft_pptx::parse_xml(core.as_bytes()).unwrap().root.to_xml();
            let hash: String = Sha256::digest(canonical.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
            let id = member_ref(&xml, "projection");
            *data = scalar(&xml, id, &hash).into_bytes();
        }
        true
    });
    let error = deckcraft_pptx::import(&forged).unwrap_err().to_string();
    assert!(error.contains("actual neutral master"), "{error}");
}
