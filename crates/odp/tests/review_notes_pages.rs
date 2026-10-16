//! Independent contributor-original notes composition and editable-master regressions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    notes::{NotesFields, NotesMaster, NotesPage},
    text::{BodyProps, Spacing},
    *,
};
use deckcraft_render::{notes_page_document, render_notes_page};
use std::sync::Arc;

fn export_fixture(p: &Presentation, label: &str) -> Vec<u8> {
    let bytes = deckcraft_odp::export(p).unwrap();
    if let Ok(directory) = std::env::var("DECKCRAFT_ODP_NOTES_REVIEW_OUTPUT") {
        use std::io::{Cursor, Read};
        std::fs::create_dir_all(&directory).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        for name in ["content.xml", "styles.xml"] {
            let mut xml = Vec::new();
            archive.by_name(name).unwrap().read_to_end(&mut xml).unwrap();
            std::fs::write(std::path::Path::new(&directory).join(format!("{label}-{name}")), xml).unwrap();
        }
    }
    bytes
}

fn paint(r: u8, g: u8, b: u8) -> Fill {
    Fill::solid(ColorRef::rgb(Rgba::rgb(r, g, b)))
}
fn frame(id: u32, kind: PhType, rect: Option<Xfrm>) -> Shape {
    Shape { id: ShapeId(id), ph: Some(Placeholder { kind, ..Default::default() }), xfrm: rect, line: Some(Line::none()), ..Default::default() }
}
fn fixture() -> Presentation {
    let mut p = Presentation { slide_size: Size::new(100.0, 50.0), ..Default::default() };
    let mut master = Master { id: MasterId(700), ..Default::default() };
    let mut body = frame(701, PhType::Body, Some(Xfrm::new(20.0, 180.0, 160.0, 60.0)));
    body.fill = Some(paint(20, 100, 180));
    master.shapes.push(body);
    p.notes_masters.push(NotesMaster {
        master: Arc::new(master),
        size: Size::new(200.0, 300.0),
        fields: NotesFields { header: Some("First header".into()), display_header: Some(true), ..Default::default() },
    });
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.notes = TextBody::from_text("Canonical body");
    slide.notes_page = Some(NotesPage {
        master: Some(MasterId(700)),
        body_shape: Some(ShapeId(702)),
        shapes: vec![frame(702, PhType::Body, None)],
        ..Default::default()
    });
    p.fix_next_id();
    p
}

#[test]
fn inherited_notes_fields_stay_live_after_editable_odp_reopen() {
    let mut p = deckcraft_odp::import(&export_fixture(&fixture(), "fields")).unwrap();
    assert_eq!(p.slides[0].notes.text(), "Canonical body");
    assert_eq!(p.slides[0].notes_page.as_ref().unwrap().fields.header, None, "page must retain inherited declaration ownership");
    p.notes_masters[0].fields.header = Some("Changed shared header".into());
    p.notes_masters[0].fields.display_header = Some(false);
    let fields = p.notes_fields(&p.slides[0]);
    assert_eq!(fields.header.as_deref(), Some("Changed shared header"));
    assert_eq!(fields.display_header, Some(false));
}

#[test]
fn inherited_frame_paint_remains_live_through_odp_reopen() {
    for local_override in [false, true] {
        let mut source = fixture();
        if local_override {
            Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[0].fill = Some(paint(20, 100, 180));
        }
        assert_eq!(render_notes_page(&source, 0, &Default::default()).pixel(170, 220), [20, 100, 180, 255]);
        let mut reopened = deckcraft_odp::import(&export_fixture(&source, if local_override { "paint-local" } else { "paint-inherited" })).unwrap();
        for p in [&mut source, &mut reopened] {
            let master = Arc::make_mut(&mut p.notes_masters[0].master);
            master.shapes.iter_mut().find(|s| s.ph_type() == Some(PhType::Body)).unwrap().fill = Some(paint(180, 60, 20));
            let expected = if local_override { [20, 100, 180, 255] } else { [180, 60, 20, 255] };
            assert_eq!(
                render_notes_page(p, 0, &Default::default()).pixel(170, 220),
                expected,
                "shared-master edit respects inherited versus explicitly identical local paint"
            );
            assert_eq!(p.slides[0].notes.text(), "Canonical body");
        }
    }
}

#[test]
fn canonical_body_cannot_be_designated_as_a_live_thumbnail() {
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap().shapes[0].ph.as_mut().unwrap().kind = PhType::SlideImage;
    assert!(p.notes_page_error(&p.slides[0]).is_some(), "body identity cannot designate a role that discards text");
    assert!(deckcraft_odp::export(&p).is_err());
}

#[test]
fn canonical_body_inherits_master_paragraph_area_for_actual_rendering() {
    let mut p = fixture();
    let master = Arc::make_mut(&mut p.notes_masters[0].master);
    let body = &mut master.shapes[0];
    body.fill = Some(Fill::None);
    let mut text = TextBody::from_text("");
    text.body = BodyProps { inset_l: Some(0.0), inset_r: Some(0.0), inset_t: Some(0.0), inset_b: Some(0.0), ..Default::default() };
    text.paragraphs[0].props.background = Some(paint(20, 100, 180));
    text.paragraphs[0].props.line_spacing = Some(Spacing::Pts(24.0));
    body.text = Some(text);
    Arc::make_mut(&mut p.slides[0]).notes = TextBody::from_text("");
    assert_eq!(
        render_notes_page(&p, 0, &Default::default()).pixel(170, 190),
        [20, 100, 180, 255],
        "canonical empty body keeps master paragraph defaults"
    );
    let mut reopened = deckcraft_odp::import(&export_fixture(&p, "paragraph")).unwrap();
    Arc::make_mut(&mut reopened.notes_masters[0].master).shapes[0].text.as_mut().unwrap().paragraphs[0].props.background = Some(paint(180, 60, 20));
    assert_eq!(
        render_notes_page(&reopened, 0, &Default::default()).pixel(170, 190),
        [180, 60, 20, 255],
        "master paragraph defaults stay live after ODP reopen"
    );
}

#[test]
fn hidden_master_graphics_and_background_clear_do_not_hide_local_body() {
    let mut p = fixture();
    Arc::make_mut(&mut p.notes_masters[0].master).background = Some(Background::Fill { fill: paint(120, 40, 150) });
    let page = Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap();
    page.background = Some(Background::Fill { fill: Fill::None });
    page.show_master_shapes = false;
    let image = render_notes_page(&p, 0, &Default::default());
    assert_eq!(image.pixel(5, 5), [255; 4]);
    assert_eq!(image.pixel(170, 220), [20, 100, 180, 255], "local body still inherits role formatting");
}

#[test]
fn notes_rendering_does_not_mutate_source_or_duplicate_canonical_text() {
    let p = fixture();
    let original = p.clone();
    let document = notes_page_document(&p, 0, &Default::default()).unwrap();
    let mut copies = 0;
    deckcraft_model::walk(&document.slides[0].shapes, &mut |s, _| {
        if s.text.as_ref().is_some_and(|t| t.text() == "Canonical body") {
            copies += 1;
        }
    });
    assert_eq!(copies, 1);
    assert_eq!(p, original);
}

#[test]
fn grouped_local_body_suppresses_the_matching_master_body() {
    let mut p = fixture();
    let page = Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap();
    let mut body = page.shapes.remove(0);
    body.xfrm = Some(Xfrm::new(20.0, 180.0, 160.0, 60.0));
    page.shapes.push(Shape {
        id: ShapeId(703),
        xfrm: Some(Xfrm::new(0.0, 0.0, 200.0, 300.0)),
        kind: ShapeKind::Group { children: vec![body], child: Xfrm::new(0.0, 0.0, 200.0, 300.0) },
        ..Default::default()
    });
    let document = notes_page_document(&p, 0, &Default::default()).unwrap();
    let mut bodies = 0;
    deckcraft_model::walk(&document.slides[0].shapes, &mut |shape, _| {
        bodies += usize::from(shape.ph_type() == Some(PhType::Body));
    });
    assert_eq!(bodies, 1, "nested local body overrides the shared role exactly once");
}

#[test]
fn invalid_notes_sizes_references_and_field_budgets_fail_closed() {
    for size in [Size::new(0.0, 300.0), Size::new(f64::NAN, 300.0), Size::new(5001.0, 300.0)] {
        let mut p = fixture();
        Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap().size = Some(size);
        assert_eq!(p.notes_page_error(&p.slides[0]), Some("invalid notes page size"));
        assert!(notes_page_document(&p, 0, &Default::default()).is_none());
        assert!(deckcraft_odp::export(&p).is_err());
    }
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap().master = Some(MasterId(999));
    assert_eq!(p.notes_page_error(&p.slides[0]), Some("notes page uses a missing master"));
    assert!(notes_page_document(&p, 0, &Default::default()).is_none());
    assert!(deckcraft_odp::export(&p).is_err());
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap().fields.header = Some("x".repeat(64 * 1024 + 1));
    assert_eq!(p.notes_page_error(&p.slides[0]), Some("notes fields exceed limits"));
    assert!(notes_page_document(&p, 0, &Default::default()).is_none());
    assert!(deckcraft_odp::export(&p).is_err());
}

#[test]
fn ambiguous_primary_body_roles_cannot_export_an_unreopenable_package() {
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap().shapes.push(frame(704, PhType::Body, Some(Xfrm::new(10.0, 20.0, 100.0, 40.0))));
    assert!(p.notes_page_error(&p.slides[0]).is_some(), "two primary body roles require an explicit canonical resolution");
    assert!(deckcraft_odp::export(&p).is_err());
    let mut p = fixture();
    Arc::make_mut(&mut p.notes_masters[0].master).shapes.push(frame(705, PhType::Body, Some(Xfrm::new(10.0, 20.0, 100.0, 40.0))));
    assert!(p.notes_page_error(&p.slides[0]).is_some(), "ambiguous master body defaults must not pick the first silently");
    assert!(deckcraft_odp::export(&p).is_err());
}

#[test]
fn inherited_notes_line_body_and_default_font_stay_live_after_reopen() {
    for local_override in [false, true] {
        let mut p = fixture();
        let master = Arc::make_mut(&mut p.notes_masters[0].master);
        master.shapes[0].line = Some(Line { width: Some(2.0), fill: Some(paint(20, 100, 180)), ..Default::default() });
        let mut defaults = TextBody::from_text("");
        defaults.body.inset_l = Some(10.0);
        defaults.paragraphs[0].def_props.size = Some(18.0);
        defaults.paragraphs[0].def_props.font = Some("Ubuntu".into());
        master.shapes[0].text = Some(defaults);
        let slide = Arc::make_mut(&mut p.slides[0]);
        slide.notes_page.as_mut().unwrap().shapes[0].line =
            if local_override { Some(Line { width: Some(2.0), fill: Some(paint(20, 100, 180)), ..Default::default() }) } else { None };
        if local_override {
            slide.notes.body.inset_l = Some(10.0);
            slide.notes.paragraphs[0].def_props.size = Some(18.0);
            slide.notes.paragraphs[0].def_props.font = Some("Ubuntu".into());
        }
        let mut reopened = deckcraft_odp::import(&export_fixture(&p, if local_override { "text-local" } else { "text-inherited" })).unwrap();
        for source in [&mut p, &mut reopened] {
            let master = Arc::make_mut(&mut source.notes_masters[0].master);
            master.shapes[0].line.as_mut().unwrap().width = Some(6.0);
            let defaults = master.shapes[0].text.as_mut().unwrap();
            defaults.body.inset_l = Some(35.0);
            defaults.paragraphs[0].def_props.size = Some(30.0);
            defaults.paragraphs[0].def_props.font = Some("Ubuntu Sans".into());
            let document = notes_page_document(source, 0, &Default::default()).unwrap();
            let slide = &document.slides[0];
            let shape = slide.shapes.iter().find(|shape| shape.ph_type() == Some(PhType::Body)).unwrap();
            let context = deckcraft_model::resolve::Ctx::for_slide(&document, slide).unwrap();
            let text = shape.text.as_ref().unwrap();
            let run = deckcraft_model::resolve::run(&context, shape, &text.paragraphs[0], &text.paragraphs[0].runs[0].props);
            let actual = (
                deckcraft_model::resolve::line(&context, shape).0.width,
                deckcraft_model::resolve::body(&context, shape).inset_l,
                run.size,
                run.font.as_deref(),
            );
            let expected = if local_override {
                (Some(2.0), Some(10.0), Some(18.0), Some("Ubuntu"))
            } else {
                (Some(6.0), Some(35.0), Some(30.0), Some("Ubuntu Sans"))
            };
            assert_eq!(actual, expected, "source ownership survives explicit-value equality and editable export");
        }
    }
}

#[test]
fn inherited_grouped_thumbnail_uses_centered_contain_before_conversion() {
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).background = Some(Background::Fill { fill: paint(220, 20, 30) });
    Arc::make_mut(&mut p.notes_masters[0].master).shapes.push(frame(706, PhType::SlideImage, Some(Xfrm::new(50.0, 20.0, 100.0, 100.0))));
    Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap().shapes.push(Shape {
        id: ShapeId(707),
        xfrm: Some(Xfrm::new(0.0, 0.0, 200.0, 300.0)),
        kind: ShapeKind::Group { children: vec![frame(708, PhType::SlideImage, None)], child: Xfrm::new(0.0, 0.0, 200.0, 300.0) },
        ..Default::default()
    });
    p.fix_next_id();
    let image = render_notes_page(&p, 0, &Default::default());
    assert_eq!(image.pixel(100, 30), [255; 4], "2:1 slide is fitted inside the inherited square frame");
    assert_eq!(image.pixel(100, 70), [220, 20, 30, 255]);
    assert_eq!(image.pixel(100, 110), [255; 4]);
}
