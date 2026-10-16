//! Independent current-authority and malformed-grid checks in every presentation owner.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    table::{CellConstant, CellValue},
    *,
};
use std::sync::Arc;

fn table(stale: bool) -> Shape {
    let mut t = Table::new(1, 1, 80.0, 30.0);
    let c = &mut t.rows[0].cells[0];
    c.text = TextBody::from_text("shown");
    c.data = Some(Arc::new(CellConstant::from_display(CellValue::Boolean { value: true }, &c.text).unwrap()));
    if stale {
        c.text = TextBody::from_text("edited");
    }
    Shape { id: ShapeId(500), xfrm: Some(Xfrm::new(0., 0., 80., 30.)), hidden: true, kind: ShapeKind::Table(t), ..Default::default() }
}
fn group(shape: Shape) -> Shape {
    Shape { id: ShapeId(501), kind: ShapeKind::Group { children: vec![shape], child: Xfrm::new(0.0, 0.0, 80.0, 30.0) }, ..Default::default() }
}
fn master(id: u32, shape: Shape, layout: bool) -> Arc<Master> {
    let mut m = Master { id: MasterId(id), layouts: vec![Layout { id: LayoutId(id + 1), ..Default::default() }], ..Default::default() };
    if layout {
        m.layouts[0].shapes.push(group(shape));
    } else {
        m.shapes.push(group(shape));
    }
    Arc::new(m)
}
fn fixture(owner: usize, stale: bool) -> Presentation {
    let mut p = Presentation {
        masters: vec![Arc::new(Master {
            id: MasterId(100),
            layouts: vec![Layout { id: LayoutId(101), ..Default::default() }],
            ..Default::default()
        })],
        slides: vec![Arc::new(Slide { id: SlideId(200), layout: LayoutId(101), hidden: true, show_master_shapes: false, ..Default::default() })],
        notes_master: None,
        ..Default::default()
    };
    p.notes_masters.clear();
    p.handout_master = None;
    let t = table(stale);
    match owner {
        0 => Arc::make_mut(&mut p.slides[0]).shapes.push(t),
        1 => Arc::make_mut(&mut p.slides[0]).shapes.push(group(t)),
        2 | 3 => p.masters.push(master(300, t, owner == 3)),
        4 | 5 => p.notes_master = Some(master(310, t, owner == 5)),
        6 | 7 => p.notes_masters.push(notes::NotesMaster { master: master(320, t, owner == 7), ..Default::default() }),
        8 | 9 => p.handout_master = Some(master(330, t, owner == 9)),
        _ => {
            Arc::make_mut(&mut p.slides[0]).notes_page =
                Some(notes::NotesPage { shapes: vec![group(t)], show_master_shapes: false, ..Default::default() })
        }
    }
    p.fix_next_id();
    p
}

fn assert_current_boolean_is_present(p: &Presentation, owner: usize, extension: &str) {
    let mut found = 0;
    let mut shapes = |items: &[Shape]| {
        deckcraft_model::walk(items, &mut |shape, _| {
            if let ShapeKind::Table(t) = &shape.kind {
                for cell in t.rows.iter().flat_map(|row| &row.cells) {
                    if let Some(data) = &cell.data {
                        assert_eq!(data.value(), &CellValue::Boolean { value: true });
                        assert_eq!(cell.text.text(), "shown");
                        assert!(matches!(cell.constant_state().unwrap(), table::CellConstantState::Current(_)));
                        found += 1;
                    }
                }
            }
        });
    };
    for master in p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|m| &m.master)) {
        shapes(&master.shapes);
        for layout in &master.layouts {
            shapes(&layout.shapes);
        }
    }
    for slide in &p.slides {
        shapes(&slide.shapes);
        if let Some(page) = &slide.notes_page {
            shapes(&page.shapes);
        }
    }
    assert!(found > 0, "owner {owner}, {extension}: typed Boolean authority must survive, including unused owners");
}

#[test]
fn pptx_templates_and_shows_preserve_current_authority_and_reject_stale_records_in_all_owners() {
    for owner in 0..11 {
        for stale in [false, true] {
            let p = fixture(owner, stale);
            let before = p.clone();
            for ext in ["pptx", "potx", "ppsx"] {
                let result = deckcraft_pptx::export_as(&p, ext);
                if stale {
                    let error = result.unwrap_err().to_string();
                    assert!(error.contains("stale typed table display"), "owner {owner}, {ext}: {error}");
                } else {
                    let bytes = result.unwrap();
                    let back = deckcraft_pptx::import(&bytes).unwrap();
                    deckcraft_model::table::validate_constants(&back).unwrap();
                    assert_current_boolean_is_present(&back, owner, ext);
                    // A second export must also verify the restored typed authority.
                    deckcraft_pptx::export_as(&back, ext).unwrap();
                }
            }
            assert_eq!(p, before, "export mutated its caller");
        }
    }
}

#[test]
fn raw_malformed_grid_and_covered_records_are_guarded_before_pptx_sanitization() {
    for case in 0..4 {
        let mut p = fixture(0, true);
        let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
        match case {
            0 => t.cols.clear(),
            1 => t.rows[0].cells.insert(0, Cell::new("anchor")),
            2 => t.rows[0].cells[0].h_merge = true,
            _ => t.rows[0].cells[0].v_merge = true,
        }
        let before = p.clone();
        let error = deckcraft_pptx::export(&p).unwrap_err().to_string();
        assert!(error.contains("typed constants"), "{error}");
        assert_eq!(p, before);
    }
}

#[test]
fn unused_handout_layout_keeps_current_table_children_media_and_slide_targets() {
    use deckcraft_model::{
        style::{Fill, PictureFill},
        table::CellBlock,
        text::{Action, Hyperlink},
    };
    use std::io::Cursor;
    let mut p = fixture(9, false);
    Arc::make_mut(&mut p.slides[0]).id = SlideId(90_000);
    let target = p.slides[0].id;
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([31, 73, 119, 255])))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let media = p.add_media("original-handout-layout.png", "image/png", png.into_inner()).unwrap();
    let layout = &mut Arc::make_mut(p.handout_master.as_mut().unwrap()).layouts[0];
    layout.name = "Original unused handout layout".into();
    layout.background = Some(Background::Fill { fill: Fill::Picture(PictureFill { media, ..Default::default() }) });
    let ShapeKind::Group { children, .. } = &mut layout.shapes[0].kind else { panic!("group") };
    let ShapeKind::Table(table) = &mut children[0].kind else { panic!("table") };
    table.cell_mut(0, 0).unwrap().append_nested_table(Table::new(1, 1, 50., 20.)).unwrap();
    table.cell_mut(0, 0).unwrap().append_text_block(TextBody::from_text("preserved handout tail")).unwrap();
    table.cell_mut(0, 0).unwrap().text.paragraphs[0].runs[0].props.link = Some(Hyperlink {
        action: Action::Slide { slide: target },
        tooltip: String::new(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: Some(target),
        sound_name: None,
    });
    p.fix_next_id();
    let original = p.clone();
    let bytes = deckcraft_pptx::export(&p).unwrap();
    if let Ok(dir) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
        std::fs::write(std::path::Path::new(&dir).join("handout-unused-layouts.pptx"), &bytes).unwrap();
    }
    let mut back = deckcraft_pptx::import(&bytes).unwrap();
    assert_ne!(back.slides[0].id, target);
    for _ in 0..2 {
        assert_current_boolean_is_present(&back, 9, "handout regression");
        let layout = &back.handout_master.as_ref().unwrap().layouts[0];
        assert_eq!(layout.name, "Original unused handout layout");
        let ShapeKind::Group { children, .. } = &layout.shapes[0].kind else { panic!("group") };
        let ShapeKind::Table(table) = &children[0].kind else { panic!("table") };
        let cell = table.cell(0, 0).unwrap();
        assert_eq!(cell.blocks.len(), 2);
        assert!(matches!(&cell.blocks[0],CellBlock::Table(child) if child.table.n_rows()==1 && child.table.n_cols()==1));
        assert!(matches!(&cell.blocks[1],CellBlock::Text(tail) if tail.body.text()=="preserved handout tail"));
        let link = cell.text.paragraphs[0].runs[0].props.link.as_ref().unwrap();
        assert_eq!(link.action, Action::Slide { slide: back.slides[0].id });
        assert_eq!(link.zoom_return, Some(back.slides[0].id));
        let Some(Background::Fill { fill: Fill::Picture(fill) }) = &layout.background else { panic!("picture background") };
        assert_eq!(back.media(fill.media).unwrap().name, "original-handout-layout.png");
        assert!(back.media(fill.media).unwrap().data.starts_with(b"\x89PNG"));
        back = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
    }
    assert!(p == original, "export mutated original handout authority");
}

#[test]
fn unused_handout_layout_cannot_alias_two_original_slides_to_one_actual_target() {
    use std::io::{Cursor, Read, Write};
    let mut p = fixture(9, false);
    let mut second = (*p.slides[0]).clone();
    second.id = SlideId(201);
    p.slides.push(Arc::new(second));
    p.fix_next_id();
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut forged = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut edits = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if name == "ppt/handoutMasters/handoutMaster1.xml" {
            let xml = String::from_utf8(data).unwrap();
            assert_eq!(xml.matches("ppt/slides/slide2.xml").count(), 1);
            data = xml.replace("ppt/slides/slide2.xml", "ppt/slides/slide1.xml").into_bytes();
            edits += 1;
        }
        forged.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        forged.write_all(&data).unwrap();
    }
    assert_eq!(edits, 1);
    let bytes = forged.finish().unwrap().into_inner();
    match deckcraft_pptx::import(&bytes) {
        Err(error) => assert!(error.to_string().contains("duplicate actual handout slide owner"), "{error}"),
        Ok(_) => panic!("unused handout resource rebound two original slide identities to one actual slide"),
    }
}

#[test]
fn unused_handout_external_media_preserves_distinct_native_identity_and_shared_references() {
    use deckcraft_model::style::{Fill, PictureFill};
    let mut p = fixture(9, false);
    let url = "https://example.org/shared.svg";
    let mut add = |name: &str, content_type: &str| {
        let id = MediaId(p.alloc_id().unwrap());
        p.media.push(MediaItem { id, name: name.into(), content_type: content_type.into(), data: Arc::new(Vec::new()), link: Some(url.into()) });
        id
    };
    let a = add("unused-background.png", "image/png");
    let b = add("unused-picture.svg", "image/svg+xml");
    let visible = add("visible-core.svg", "image/svg+xml");
    let picture = |id: u32, media| Shape {
        id: ShapeId(id),
        kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
        ..Default::default()
    };
    let master = Arc::make_mut(p.handout_master.as_mut().unwrap());
    master.shapes.push(picture(650, visible));
    master.layouts[0].background = Some(Background::Fill { fill: Fill::Picture(PictureFill { media: a, ..Default::default() }) });
    master.layouts[0].shapes.extend([picture(651, b), picture(652, a)]);
    p.fix_next_id();
    let original = p.clone();
    let mut back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    for _ in 0..2 {
        let master = back.handout_master.as_ref().unwrap();
        let layout = &master.layouts[0];
        let Some(Background::Fill { fill: Fill::Picture(bg) }) = &layout.background else { panic!("background") };
        let ShapeKind::Picture { fill: other } = &layout.shapes[1].kind else { panic!("other") };
        let ShapeKind::Picture { fill: shared } = &layout.shapes[2].kind else { panic!("shared") };
        let ShapeKind::Picture { fill: visible } = &master.shapes[0].kind else { panic!("visible") };
        assert_eq!(bg.media, shared.media);
        assert_ne!(bg.media, other.media);
        assert_ne!(bg.media, visible.media);
        assert_ne!(other.media, visible.media);
        let a = back.media(bg.media).unwrap();
        let b = back.media(other.media).unwrap();
        let visible = back.media(visible.media).unwrap();
        assert_eq!((a.name.as_str(), a.content_type.as_str(), a.link.as_deref()), ("unused-background.png", "image/png", Some(url)));
        assert_eq!((b.name.as_str(), b.content_type.as_str(), b.link.as_deref()), ("unused-picture.svg", "image/svg+xml", Some(url)));
        assert_ne!(visible.name, "unused-background.png");
        assert_ne!(visible.name, "unused-picture.svg");
        assert!(a.data.is_empty() && b.data.is_empty());
        back = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
    }
    assert!(p == original, "external resource export mutated native identities");
}
