use super::*;
use deckcraft_model::chart::ChartType;
use deckcraft_model::style::ColorRef;
use deckcraft_model::text::TextBody;
use deckcraft_model::{Chart, Geom, Presentation, Shape, ShapeId, ShapeKind, ShapeStyle, Table, Xfrm};

fn deck_with(shapes: Vec<Shape>) -> Presentation {
    let mut p = Presentation::default();
    let s = std::sync::Arc::make_mut(&mut p.slides[0]);
    s.shapes = shapes;
    p
}

#[test]
fn blank_slide_renders_white_background() {
    let p = Presentation::default();
    let img = render_slide(&p, 0, &RenderOpts { scale: 0.25, ..Default::default() });
    assert_eq!((img.width, img.height), (240, 135));
    assert_eq!(img.pixel(5, 5), [255, 255, 255, 255]);
}

#[test]
fn shape_fill_uses_theme_accent() {
    let sh = Shape {
        id: ShapeId(500),
        xfrm: Some(Xfrm::new(100.0, 100.0, 200.0, 100.0)),
        style: Some(ShapeStyle::accent(deckcraft_color::SchemeSlot::Accent1)),
        ..Default::default()
    };
    let p = deck_with(vec![sh]);
    let img = render_slide(&p, 0, &RenderOpts { scale: 1.0, ..Default::default() });
    let accent = p.masters[0].theme.colors.get(deckcraft_color::SchemeSlot::Accent1);
    let px = img.pixel(200, 150);
    assert!((px[0] as i32 - accent.r as i32).abs() <= 2 && (px[2] as i32 - accent.b as i32).abs() <= 2, "{px:?} vs {accent:?}");
    assert_eq!(img.pixel(50, 50), [255, 255, 255, 255]);
}

#[test]
fn text_draws_ink() {
    let mut sh = Shape { id: ShapeId(501), xfrm: Some(Xfrm::new(0.0, 0.0, 960.0, 540.0)), text_box: true, ..Default::default() };
    let mut t = TextBody::from_text("HELLO WORLD");
    t.paragraphs[0].runs[0].props.size = Some(120.0);
    sh.text = Some(t);
    let p = deck_with(vec![sh]);
    let img = render_slide(&p, 0, &RenderOpts { scale: 0.5, ..Default::default() });
    let dark = img.pixels.as_chunks::<4>().0.iter().filter(|px| px[0] < 128).count();
    assert!(dark > 500, "dark pixels {dark}");
}

#[test]
fn every_preset_renders_without_panic() {
    let shapes: Vec<Shape> = deckcraft_geom::preset::CATALOG
        .iter()
        .enumerate()
        .map(|(i, pr)| Shape {
            id: ShapeId(1000 + i as u32),
            xfrm: Some(Xfrm::new((i % 16) as f64 * 60.0, (i / 16) as f64 * 55.0, 50.0, 45.0)),
            geom: Geom::preset(pr.name),
            style: Some(ShapeStyle::accent(deckcraft_color::SchemeSlot::Accent2)),
            ..Default::default()
        })
        .collect();
    let p = deck_with(shapes);
    let img = render_slide(&p, 0, &RenderOpts { scale: 1.0, edit: true, ..Default::default() });
    assert_eq!(img.width, 960);
}

#[test]
fn effects_tables_charts_render() {
    let mut sh = Shape {
        id: ShapeId(600),
        xfrm: Some(Xfrm::new(50.0, 50.0, 200.0, 100.0)),
        style: Some(ShapeStyle::accent(deckcraft_color::SchemeSlot::Accent1)),
        ..Default::default()
    };
    sh.effects = Some(deckcraft_model::Effects {
        outer_shadow: Some(deckcraft_model::style::Shadow {
            color: ColorRef::rgb(deckcraft_color::Rgba::BLACK),
            blur: 8.0,
            dist: 6.0,
            dir: 45.0,
            inner: false,
            sx: 1.0,
            sy: 1.0,
            kx: 0.0,
            ky: 0.0,
            align: String::new(),
            rotate_with_shape: false,
        }),
        glow: Some(deckcraft_model::style::Glow { color: ColorRef::rgb(deckcraft_color::Rgba::rgb(255, 200, 0)), radius: 6.0 }),
        soft_edge: Some(4.0),
        ..Default::default()
    });
    let mut t = Table::new(3, 3, 300.0, 30.0);
    t.cell_mut(0, 0).unwrap().text = TextBody::from_text("Head");
    let tbl = Shape { id: ShapeId(601), xfrm: Some(Xfrm::new(300.0, 50.0, 300.0, 90.0)), kind: ShapeKind::Table(t), ..Default::default() };
    let mut charts = vec![];
    for (i, k) in [
        ChartType::Column,
        ChartType::Bar,
        ChartType::Line,
        ChartType::Pie,
        ChartType::Doughnut,
        ChartType::Area,
        ChartType::Scatter,
        ChartType::StackedColumn,
    ]
    .iter()
    .enumerate()
    {
        charts.push(Shape {
            id: ShapeId(700 + i as u32),
            xfrm: Some(Xfrm::new(i as f64 * 110.0, 300.0, 100.0, 100.0)),
            kind: ShapeKind::Chart(Box::new(Chart::sample(*k))),
            ..Default::default()
        });
    }
    let mut all = vec![sh, tbl];
    all.extend(charts);
    let p = deck_with(all);
    let img = render_slide(&p, 0, &RenderOpts { scale: 1.0, ..Default::default() });
    // Shadow darkens below-right of the shape.
    let px = img.pixel(258, 156);
    assert!(px[0] < 250, "{px:?}");
}

#[test]
fn placeholder_prompts_only_in_edit_view() {
    let p = Presentation::default();
    let edit = render_slide(&p, 0, &RenderOpts { scale: 0.5, edit: true, ..Default::default() });
    let show = render_slide(&p, 0, &RenderOpts { scale: 0.5, ..Default::default() });
    let ink = |i: &Image| i.pixels.as_chunks::<4>().0.iter().filter(|px| px[0] < 200).count();
    assert!(ink(&edit) > 100);
    assert_eq!(ink(&show), 0);
}

#[test]
fn png_encodes() {
    let p = Presentation::default();
    let img = render_slide(&p, 0, &RenderOpts { scale: 0.1, ..Default::default() });
    let png = img.to_png();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    assert!(!img.to_jpeg(80).is_empty());
}

#[test]
fn crop_dest_math() {
    let r = crop_dest(Rect::new(0.0, 0.0, 100.0, 100.0), [0.5, 0.0, 0.0, 0.0]);
    assert!((r.x0 + 100.0).abs() < 1e-9 && (r.width() - 200.0).abs() < 1e-9);
    let r = crop_dest(Rect::new(0.0, 0.0, 100.0, 100.0), [f64::NAN, 2.0, -50.0, 0.0]);
    assert!(r.x0.is_finite() && r.y1.is_finite());
}

#[test]
fn custom_path_parse() {
    let p = parse_path("M 0 0 L 10 0 C 1 2 3 4 5 6 Q 1 1 2 2 Z garbage L x y");
    assert!(p.elements().len() >= 5);
    assert!(parse_path("").elements().is_empty());
}

#[test]
fn blend_ends_match_the_slides_and_mixes_backdrops() {
    let sh = Shape {
        id: ShapeId(500),
        xfrm: Some(Xfrm::new(100.0, 100.0, 200.0, 100.0)),
        style: Some(ShapeStyle::accent(deckcraft_color::SchemeSlot::Accent1)),
        ..Default::default()
    };
    let mut p = deck_with(vec![sh.clone()]);
    let mut dark = (*p.slides[0]).clone();
    dark.background =
        Some(deckcraft_model::Background::Fill { fill: deckcraft_model::Fill::solid(ColorRef::rgb(deckcraft_color::Rgba::rgb(0, 0, 0))) });
    p.slides.push(std::sync::Arc::new(dark));
    let o = RenderOpts { scale: 0.5, ..Default::default() };
    let shapes = vec![(sh, false)];
    // At the ends the frame is the slide itself; the shape stays opaque throughout.
    assert_eq!(render_blend(&p, 0, 1, 1.0, &shapes, &o).pixels, render_slide(&p, 1, &o).pixels);
    let mid = render_blend(&p, 0, 1, 0.5, &shapes, &o);
    let px = mid.pixel(5, 5);
    assert!((px[0] as i32 - 128).abs() <= 3, "{px:?}");
    assert_eq!(mid.pixel(100, 75), render_slide(&p, 1, &o).pixel(100, 75));
    // Hostile input: bad indices and NaN never panic.
    assert_eq!(render_blend(&p, 0, 9, 0.5, &shapes, &o).width, 0);
    let _ = render_blend(&p, 0, 1, f64::NAN, &shapes, &o);
}
