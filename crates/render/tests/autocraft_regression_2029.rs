use deckcraft_model::chart::ChartType;
use deckcraft_model::style::Line;
use deckcraft_model::{Chart, ColorRef, Presentation, Rgba, Shape, ShapeId, ShapeKind, Xfrm};
use deckcraft_render::{RenderOpts, render_slide};

/// Rows and columns of the pixels matching `want` in the legend's key column, right of the plot.
fn key_extent(kind: ChartType, want: &dyn Fn([u8; 4]) -> bool) -> (usize, usize) {
    let mut c = Chart::sample(kind);
    c.title = None;
    c.legend = Some("r".into());
    c.series.truncate(2);
    for (s, (name, rgb)) in c.series.iter_mut().zip([("A", Rgba::rgb(255, 0, 0)), ("B", Rgba::rgb(0, 0, 255))]) {
        s.name = name.into();
        s.line = Some(Line::solid(ColorRef::rgb(rgb), 3.0));
    }
    let sh = Shape { id: ShapeId(1), xfrm: Some(Xfrm::new(0.0, 0.0, 960.0, 540.0)), kind: ShapeKind::Chart(Box::new(c)), ..Default::default() };
    let mut p = Presentation::default();
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes = vec![sh];
    let img = render_slide(&p, 0, &RenderOpts { scale: 1.0, ..Default::default() });
    let hits: Vec<(u32, u32)> =
        (870..img.width).flat_map(|x| (0..img.height).map(move |y| (x, y))).filter(|(x, y)| want(img.pixel(*x, *y))).collect();
    let rows = hits.iter().map(|h| h.1).collect::<std::collections::BTreeSet<_>>().len();
    let cols = hits.iter().map(|h| h.0).collect::<std::collections::BTreeSet<_>>().len();
    (rows, cols)
}

fn red(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] < 60 && p[2] < 60
}

fn blue(p: [u8; 4]) -> bool {
    p[0] < 60 && p[1] < 60 && p[2] > 200
}

#[test]
fn line_chart_legend_key_is_a_stretch_of_the_line() {
    // A 3 pt line key is a few pixels tall; a filled square key is over ten.
    for want in [&red as &dyn Fn([u8; 4]) -> bool, &blue] {
        let (rows, cols) = key_extent(ChartType::Line, want);
        assert!((2..=5).contains(&rows), "line key is {rows} rows tall");
        assert!(cols >= 8, "line key is {cols} columns wide");
    }
}

#[test]
fn line_with_markers_legend_key_has_a_marker_on_the_line() {
    // The marker is wider than the line but smaller than a filled square.
    for want in [&red as &dyn Fn([u8; 4]) -> bool, &blue] {
        let (rows, cols) = key_extent(ChartType::LineMarkers, want);
        assert!((6..=10).contains(&rows), "marker key is {rows} rows tall");
        assert!(cols >= 8, "marker key is {cols} columns wide");
    }
}
