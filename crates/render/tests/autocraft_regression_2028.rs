use deckcraft_color::Rgba;
use deckcraft_model::chart::ChartType;
use deckcraft_model::style::{ColorRef, Fill};
use deckcraft_model::{Chart, Presentation, Shape, ShapeId, ShapeKind, Xfrm};
use deckcraft_render::{RenderOpts, render_slide};

#[test]
fn pie_legend_keys_take_the_point_colours() {
    let mut c = Chart::sample(ChartType::Pie);
    c.title = None;
    c.legend = Some("r".into());
    c.series[0].point_fills = vec![(0, Fill::solid(ColorRef::rgb(Rgba::rgb(255, 0, 255))))];
    let sh = Shape { id: ShapeId(2028), xfrm: Some(Xfrm::new(0.0, 0.0, 960.0, 540.0)), kind: ShapeKind::Chart(Box::new(c)), ..Default::default() };
    let mut p = Presentation::default();
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes = vec![sh];
    let img = render_slide(&p, 0, &RenderOpts { scale: 1.0, ..Default::default() });
    // The pie stays well left of x = 850; only the legend column lies beyond it.
    let key = (850..img.width).flat_map(|x| (0..img.height).map(move |y| (x, y))).filter(|(x, y)| img.pixel(*x, *y)[..3] == [255, 0, 255]).count();
    assert!(key > 20, "the legend key of the overridden slice is drawn in its colour: {key} px");
}
