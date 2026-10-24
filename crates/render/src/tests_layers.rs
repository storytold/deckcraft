//! Drawing single shapes, single paragraphs and moving groups, for the slide show's layers.

use super::*;
use deckcraft_model::style::ColorRef;
use deckcraft_model::text::{Paragraph, Run, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, Xfrm};

fn deck_with(shapes: Vec<Shape>) -> Presentation {
    let mut p = Presentation::default();
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes = shapes;
    p
}

fn red_box(id: u32, x: f64, y: f64, w: f64, h: f64) -> Shape {
    Shape { id: ShapeId(id), xfrm: Some(Xfrm::new(x, y, w, h)), fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(220, 20, 20)))), ..Default::default() }
}

fn ink(img: &Image, x0: u32, x1: u32, y0: u32, y1: u32) -> usize {
    (y0..y1.min(img.height)).flat_map(|y| (x0..x1.min(img.width)).map(move |x| (x, y))).filter(|(x, y)| img.pixel(*x, *y)[3] > 0).count()
}

#[test]
fn one_shape_alone_measures_and_draws_in_its_own_region() {
    let p = deck_with(vec![red_box(1, 100.0, 50.0, 200.0, 100.0), red_box(2, 500.0, 300.0, 100.0, 100.0)]);
    let probe = RenderOpts { scale: 0.5, size: Some((1, 1)), ..Default::default() };
    let (_, drawn) = render_slide_shapes(&p, 0, &[ShapeId(1)], ShapePart::Whole, false, &probe);
    let d = drawn.expect("the shape draws");
    // 0.5 px/pt: the box is 50,25 – 150,75 (its outline reaches a little further).
    assert!(d.x0 <= 50.0 && d.x0 > 45.0 && d.y0 <= 25.0 && d.y0 > 20.0 && d.x1 >= 150.0 && d.x1 < 155.0, "{d:?}");
    // Drawn into just that region, it matches the same pixels of all the slide's shapes.
    let region = RenderOpts { scale: 0.5, size: Some((120, 70)), offset: (-40.0, -15.0), ..Default::default() };
    let (part, _) = render_slide_shapes(&p, 0, &[ShapeId(1)], ShapePart::Whole, false, &region);
    let whole = render_slide_part(&p, 0, SlidePart::Content, &RenderOpts { scale: 0.5, ..Default::default() });
    for (x, y) in [(20u32, 20u32), (60, 35), (100, 55), (5, 5)] {
        assert_eq!(part.pixel(x, y), whole.pixel(x + 40, y + 15), "at {x},{y}");
    }
    assert_eq!(ink(&part, 0, 120, 0, 70), ink(&whole, 40, 160, 15, 85));
    // The other shape is left out; with the backdrop the slide's white is under it.
    assert_eq!(
        ink(
            &render_slide_shapes(&p, 0, &[ShapeId(1)], ShapePart::Whole, false, &RenderOpts { scale: 0.5, ..Default::default() }).0,
            240,
            310,
            140,
            210
        ),
        0
    );
    let (back, _) = render_slide_shapes(&p, 0, &[], ShapePart::Whole, true, &RenderOpts { scale: 0.5, ..Default::default() });
    assert_eq!(back.pixel(300, 200), [255, 255, 255, 255]);
}

#[test]
fn a_paragraph_alone_draws_only_its_line() {
    let para = |t: &str| Paragraph { runs: vec![Run::new(t)], ..Default::default() };
    let s = Shape {
        id: ShapeId(7),
        xfrm: Some(Xfrm::new(100.0, 100.0, 400.0, 200.0)),
        fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(200, 200, 240)))),
        text: Some(TextBody { paragraphs: vec![para("First line"), para("Second line"), para("Third line")], ..Default::default() }),
        ..Default::default()
    };
    let p = deck_with(vec![s]);
    let o = RenderOpts { scale: 1.0, ..Default::default() };
    let (whole, _) = render_slide_shapes(&p, 0, &[ShapeId(7)], ShapePart::Whole, false, &o);
    let (second, d) = render_slide_shapes(&p, 0, &[ShapeId(7)], ShapePart::Paragraph(1), false, &o);
    let d = d.expect("the paragraph draws");
    // No box fill: only glyphs, on one line, below the first and above the third.
    assert_eq!(second.pixel(110, 110)[3], 0, "the fill is left out");
    assert!(ink(&second, 100, 500, 100, 300) > 50, "the paragraph's glyphs are drawn");
    let (first, d1) = render_slide_shapes(&p, 0, &[ShapeId(7)], ShapePart::Paragraph(0), false, &o);
    let d1 = d1.expect("first paragraph");
    assert!(d1.y1 <= d.y1 && d1.y0 < d.y0, "the first paragraph sits above the second: {d1:?} {d:?}");
    assert!(ink(&first, 100, 500, 100, 300) > 50);
    assert!(ink(&whole, 100, 500, 100, 300) > ink(&second, 100, 500, 100, 300));
}

#[test]
fn an_animated_group_moves_its_children() {
    let child = red_box(11, 0.0, 0.0, 100.0, 100.0);
    let g = Shape {
        id: ShapeId(10),
        xfrm: Some(Xfrm::new(100.0, 100.0, 100.0, 100.0)),
        kind: ShapeKind::Group { children: vec![child], child: Xfrm::new(0.0, 0.0, 100.0, 100.0) },
        ..Default::default()
    };
    let p = deck_with(vec![g]);
    let moved =
        |id: ShapeId| if id == ShapeId(10) { ShapeState { offset: Vec2::new(300.0, 0.0), ..Default::default() } } else { ShapeState::default() };
    let img = render_slide(&p, 0, &RenderOpts { scale: 1.0, state: Some(&moved), ..Default::default() });
    assert_eq!(img.pixel(150, 150), [255, 255, 255, 255], "the group left its place");
    let px = img.pixel(450, 150);
    assert!(px[0] > 200 && px[1] < 60, "the child is drawn where the group moved: {px:?}");
    let m = shape_motion(&p, 0, ShapeId(10), &moved(ShapeId(10))).expect("motion");
    assert_eq!(m.moved * Point::new(0.0, 0.0), Point::new(400.0, 100.0));
    assert_eq!(m.still * Point::new(0.0, 0.0), Point::new(100.0, 100.0));
}
