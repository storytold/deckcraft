#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_geom::Rect;
use deckcraft_model::{Background, ColorRef, Effects, Fill, Geom, Presentation, Rgba, Shape, ShapeId, Slide, TextBody, Xfrm};

use crate::{MorphChange, MorphFrame, morph_frame};

fn bx(id: u32, name: &str, x: f64) -> Shape {
    Shape { id: ShapeId(id), name: name.into(), xfrm: Some(Xfrm::new(x, 100.0, 200.0, 100.0)), ..Default::default() }
}

fn frame(p: &Presentation, a: &Slide, b: &Slide, t: f64) -> MorphFrame {
    morph_frame(p, a, b, t, &|_| false)
}

fn slides(from: Vec<Shape>, to: Vec<Shape>) -> (Slide, Slide) {
    (Slide { shapes: from, ..Default::default() }, Slide { shapes: to, ..Default::default() })
}

fn region(c: MorphChange) -> Rect {
    match c {
        MorphChange::Region(r) => r,
        other => panic!("not a region: {other:?}"),
    }
}

#[test]
fn a_moving_box_changes_only_where_it_moves() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let (a, b) = slides(vec![bx(1, "Box", 0.0), bx(2, "Fixed", 700.0)], vec![bx(1, "Box", 400.0), bx(2, "Fixed", 700.0)]);
    let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
    assert!(f1.same_backdrop && f1.reach.iter().all(Option::is_some));
    let r = region(f2.changed_since(&f1));
    let (x1, x2) = (f1.shapes[0].0.xfrm.unwrap(), f2.shapes[0].0.xfrm.unwrap());
    assert!(r.x0 <= x1.x && r.x1 >= x1.x + x1.w && r.x0 <= x2.x && r.x1 >= x2.x + x2.w, "{r:?}");
    // The box that stays put is outside the region, and so is most of the slide.
    assert!(r.x1 < 700.0 && r.width() < 960.0 / 2.0, "{r:?}");
    assert_eq!(f2.changed_since(&f2.clone()), MorphChange::Nothing);
}

#[test]
fn a_rotated_box_covers_its_corners() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let turned = |x| Shape { xfrm: Some(Xfrm { rot: 45.0, ..Xfrm::new(x, 100.0, 200.0, 100.0) }), ..bx(1, "Box", x) };
    let (a, b) = slides(vec![turned(0.0)], vec![turned(400.0)]);
    let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
    let r = region(f2.changed_since(&f1));
    // A 200 x 100 box turned 45 degrees spans (200 + 100) / sqrt 2 = 212 each way.
    let at = f2.shapes[0].0.xfrm.unwrap();
    let c = at.center();
    assert!(r.x0 <= c.x - 106.0 && r.x1 >= c.x + 106.0 && r.y0 <= c.y - 106.0 && r.y1 >= c.y + 106.0, "{r:?}");
}

#[test]
fn a_speech_bubble_covers_its_tail() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let bubble = |x| Shape { geom: Geom::Preset { name: "wedgeRectCallout".into(), adj: vec![] }, ..bx(1, "Bubble", x) };
    let (a, b) = slides(vec![bubble(0.0)], vec![bubble(400.0)]);
    let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
    let r = region(f2.changed_since(&f1));
    // The box ends at y = 200; the tail points 62.5% of its height below the centre, to y = 212.5.
    assert!(r.y1 >= 212.5, "{r:?}");
}

#[test]
fn a_line_widens_the_region() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let thick = |x| Shape { line: Some(deckcraft_model::Line::solid(ColorRef::rgb(Rgba::BLACK), 20.0)), ..bx(1, "Box", x) };
    let (a, b) = slides(vec![thick(0.0)], vec![thick(400.0)]);
    let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
    let (plain_a, plain_b) = slides(vec![bx(1, "Box", 0.0)], vec![bx(1, "Box", 400.0)]);
    let plain = region(frame(&p, &plain_a, &plain_b, 0.4).changed_since(&frame(&p, &plain_a, &plain_b, 0.3)));
    let r = region(f2.changed_since(&f1));
    assert!(r.width() >= plain.width() + 40.0 && r.height() >= plain.height() + 40.0, "{r:?} vs {plain:?}");
}

#[test]
fn shapes_that_can_draw_beyond_their_box_redraw_everything() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let cases: Vec<(&str, Box<dyn Fn(Shape) -> Shape>)> = vec![
        ("text", Box::new(|s| Shape { text: Some(TextBody::from_text("Hi")), ..s })),
        ("effects", Box::new(|s| Shape { effects: Some(Effects { soft_edge: Some(4.0), ..Default::default() }), ..s })),
        ("3-D", Box::new(|s| Shape { effects: Some(Effects { raw3d: Some("<a:scene3d/>".into()), ..Default::default() }), ..s })),
        ("custom geometry", Box::new(|s| Shape { geom: Geom::Custom { paths: vec![deckcraft_model::CustomPath::default()] }, ..s })),
        ("group", Box::new(|s| Shape { kind: deckcraft_model::ShapeKind::Group { children: vec![], child: Xfrm::default() }, ..s })),
    ];
    for (what, make) in cases {
        let (a, b) = slides(vec![make(bx(1, "Box", 0.0))], vec![make(bx(1, "Box", 400.0))]);
        let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
        assert_eq!(f2.changed_since(&f1), MorphChange::Everything, "{what}");
    }
}

#[test]
fn a_still_text_box_next_to_a_moving_box_is_not_redrawn() {
    // Only changed shapes need a known reach: the text sits still while the box moves.
    let p = deckcraft_model::defaults::new_presentation(None);
    let title = Shape { text: Some(TextBody::from_text("Title")), ..bx(2, "Title", 700.0) };
    let (a, b) = slides(vec![bx(1, "Box", 0.0), title.clone()], vec![bx(1, "Box", 400.0), title]);
    let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
    assert!(f1.text.is_empty());
    let r = region(f2.changed_since(&f1));
    assert!(r.x1 < 700.0, "{r:?}");
}

#[test]
fn a_backdrop_that_cross_fades_redraws_everything() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let (a, mut b) = slides(vec![bx(1, "Box", 0.0)], vec![bx(1, "Box", 400.0)]);
    assert!(frame(&p, &a, &b, 0.5).same_backdrop);
    b.background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba { r: 200, g: 0, b: 0, a: 255 })) });
    let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
    assert!(!f1.same_backdrop);
    assert_eq!(f2.changed_since(&f1), MorphChange::Everything);
}

#[test]
fn a_different_set_of_shapes_redraws_everything() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let (a, b) = slides(vec![bx(1, "Box", 0.0)], vec![bx(1, "Box", 400.0), bx(2, "Later", 0.0)]);
    let shown = frame(&p, &a, &b, 0.3);
    let hidden = morph_frame(&p, &a, &b, 0.4, &|id| id == ShapeId(2));
    assert_eq!(hidden.changed_since(&shown), MorphChange::Everything);
}

#[test]
fn a_shape_that_fades_in_changes_where_it_stands() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let (a, b) = slides(vec![bx(1, "Box", 0.0)], vec![bx(1, "Box", 0.0), bx(2, "New", 600.0)]);
    let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
    let r = region(f2.changed_since(&f1));
    assert!(r.x0 >= 500.0 && r.x1 <= 900.0, "{r:?}");
}

#[test]
fn a_morphing_outline_changes_everywhere_it_may_reach() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let star = |x| Shape { geom: Geom::Preset { name: "star5".into(), adj: vec![] }, ..bx(1, "Shape", x) };
    let (a, b) = slides(vec![bx(1, "Shape", 0.0)], vec![star(0.0)]);
    let (f1, f2) = (frame(&p, &a, &b, 0.3), frame(&p, &a, &b, 0.4));
    assert!(!f2.paths.is_empty());
    assert_eq!(f2.changed_since(&f1), MorphChange::Everything);
}
