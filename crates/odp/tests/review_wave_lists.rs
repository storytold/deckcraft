//! List labels verified through actual renderer placement, not importer/exporter symmetry.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{text::Bullet, *};
use std::sync::Arc;

fn deck(text: &str) -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(720.0, 405.0), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let mut body = TextBody::from_text(text);
    for para in &mut body.paragraphs {
        para.props.bullet = Some(Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 1 });
        para.def_props.font = Some("Liberation Sans".into());
        para.def_props.size = Some(18.0);
    }
    let shape = Shape { id: ShapeId(101), xfrm: Some(Xfrm::new(10.0, 10.0, 400.0, 300.0)), text: Some(body), ..Default::default() };
    p.slides.push(Arc::new(Slide { id: SlideId(100), layout, shapes: vec![shape], show_master_shapes: false, ..Default::default() }));
    p
}
fn assert_label(p: &Presentation, paragraph: usize, label: &str) {
    let placed = deckcraft_render::place_slide(p, 0);
    let run = placed.texts[0].layout.runs.iter().find(|r| r.para == paragraph && r.cells.is_empty()).unwrap();
    assert_eq!(
        run.glyphs.iter().map(|g| g.0).collect::<Vec<_>>(),
        label.chars().map(|c| run.face.glyph_for(c)).collect::<Vec<_>>(),
        "paragraph {paragraph} must show {label}"
    );
}
#[test]
fn empty_numbered_paragraph_does_not_consume_a_number_on_export() {
    let p = deck("One\n\nTwo");
    assert_label(&p, 2, "2.");
    let q = deckcraft_odp::import(&deckcraft_odp::export(&p).unwrap()).unwrap();
    assert_label(&q, 2, "2.");
}
#[test]
fn plain_paragraph_breaks_the_numbered_sequence_on_export() {
    let mut p = deck("One\nPlain\nNew list");
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[1].props.bullet = Some(Bullet::None);
    assert_label(&p, 2, "1.");
    let q = deckcraft_odp::import(&deckcraft_odp::export(&p).unwrap()).unwrap();
    assert_label(&q, 2, "1.");
}

#[test]
fn empty_restart_to_one_overrides_a_nondefault_list_start_on_export() {
    let mut p = deck("One\n\nNext");
    let body = Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap();
    for para in &mut body.paragraphs {
        para.props.bullet = Some(Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 4 });
    }
    body.paragraphs[1].props.number_restart = Some(1);
    assert_label(&p, 0, "4.");
    assert_label(&p, 2, "1.");
    let q = deckcraft_odp::import(&deckcraft_odp::export(&p).unwrap()).unwrap();
    assert_label(&q, 0, "4.");
    assert_label(&q, 2, "1.");
}
