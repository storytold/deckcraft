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
fn table_rows_of_zero_height_grow_to_fit_text() {
    // pandoc and python-pptx write `<a:tr h="0">`: the stored row height is only a minimum.
    let mut t = Table::new(3, 2, 300.0, 0.0);
    for (r, row) in [["Col A", "Col B"], ["1", "alpha"], ["2", "beta"]].iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            t.cell_mut(r, c).unwrap().text = TextBody::from_text(text);
        }
    }
    let tbl = Shape { id: ShapeId(610), xfrm: Some(Xfrm::new(100.0, 100.0, 300.0, 0.0)), kind: ShapeKind::Table(t.clone()), ..Default::default() };
    let p = deck_with(vec![tbl]);
    let ctx = deckcraft_model::resolve::Ctx::for_slide(&p, &p.slides[0]).unwrap();
    let hs = crate::table::row_heights(&ctx, &t, &deckcraft_text::NoFields);
    assert_eq!(hs.len(), 3);
    assert!(hs.iter().all(|h| *h > 10.0), "{hs:?}");
    let tops: Vec<f64> = crate::table::cell_rects(&t, &hs).iter().filter(|(_, c, _)| *c == 0).map(|(_, _, r)| r.y0).collect();
    assert!(tops.len() == 3 && tops.windows(2).all(|w| w[1] > w[0] + 10.0), "{tops:?}");
    // A taller stored height is kept.
    let mut tall = t;
    tall.rows[1].height = 200.0;
    assert_eq!(crate::table::row_heights(&ctx, &tall, &deckcraft_text::NoFields)[1], 200.0);
    // The PDF text layer gets every cell's text, rows one below the other.
    let placed = place_slide(&p, 0);
    let top = |s: &str| {
        let pt = placed.texts.iter().find(|pt| pt.body.text() == s).unwrap_or_else(|| panic!("no placed text {s:?}"));
        (pt.transform * kurbo::Point::new(0.0, pt.layout.lines[0].top)).y
    };
    assert!(top("Col A") < top("1") && top("1") < top("2"));
    assert!((top("alpha") - top("1")).abs() < 1.0 && (top("beta") - top("2")).abs() < 1.0);
    assert!(top("Col B") >= 100.0);
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
    assert_eq!(render_blend(&p, 0, 1, 1.0, &shapes, &[], &[], &o).pixels, render_slide(&p, 1, &o).pixels);
    let mid = render_blend(&p, 0, 1, 0.5, &shapes, &[], &[], &o);
    let px = mid.pixel(5, 5);
    assert!((px[0] as i32 - 128).abs() <= 3, "{px:?}");
    assert_eq!(mid.pixel(100, 75), render_slide(&p, 1, &o).pixel(100, 75));
    // Hostile input: bad indices and NaN never panic.
    assert_eq!(render_blend(&p, 0, 9, 0.5, &shapes, &[], &[], &o).width, 0);
    let _ = render_blend(&p, 0, 1, f64::NAN, &shapes, &[], &[], &o);
}

fn text_shape(id: u32, text: &str, x: f64, y: f64) -> Shape {
    let mut t = TextBody::from_text(text);
    for p in &mut t.paragraphs {
        for r in &mut p.runs {
            r.props.size = Some(40.0);
        }
    }
    Shape { id: ShapeId(id), xfrm: Some(Xfrm::new(x, y, 600.0, 120.0)), text_box: true, text: Some(t), ..Default::default() }
}

#[test]
fn morph_text_tokens_are_words_or_characters() {
    let p = Presentation::default();
    let rctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
    let s = text_shape(1, "Hello big  world\nHello", 0.0, 0.0);
    let l = deckcraft_text::layout(
        &rctx,
        &s,
        s.text.as_ref().unwrap(),
        &Opts { rect: Rect::new(0.0, 0.0, 600.0, 120.0), fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false },
    );
    assert_eq!(morph_text::token_keys(&l, false), ["Hello", "big", "world", "Hello"]);
    assert_eq!(morph_text::token_keys(&l, true).concat(), "Hellobigworld".to_string() + "Hello");
    assert_eq!(morph_text::token_keys(&l, true).len(), 18);
}

#[test]
fn morph_text_ends_match_the_slides() {
    let from = Xfrm::new(100.0, 60.0, 600.0, 120.0);
    let to = Xfrm::new(200.0, 300.0, 600.0, 120.0);
    let mut p = deck_with(vec![text_shape(1, "Hello wide world", from.x, from.y)]);
    let mut b = (*p.slides[0]).clone();
    b.shapes = vec![text_shape(1, "world says Hello", to.x, to.y)];
    p.slides.push(std::sync::Arc::new(b));
    let o = RenderOpts { scale: 0.5, ..Default::default() };
    let frame = |t: f64| {
        let x = Xfrm::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t, 600.0, 120.0);
        let mut sb = text_shape(10, "world says Hello", 0.0, 0.0);
        let mut sa = text_shape(11, "Hello wide world", 0.0, 0.0);
        sb.xfrm = Some(x);
        sa.xfrm = Some(x);
        let tm = TextMorph { old: ShapeId(11), new: ShapeId(10), from, to, chars: false, t };
        render_blend(&p, 0, 1, t, &[(sb, false), (sa, true)], &[tm], &[], &o)
    };
    // Warm up: faces load lazily (a background scan runs), so the first render may still fall back.
    let _ = (render_slide(&p, 0, &o), render_slide(&p, 1, &o), frame(0.5));
    let close = |a: &Image, b: &Image| a.pixels.iter().zip(&b.pixels).filter(|(x, y)| x.abs_diff(**y) > 8).count();
    let d0 = close(&frame(0.0), &render_slide(&p, 0, &o));
    assert!(d0 < 40, "start differs {d0}");
    let d1 = close(&frame(1.0), &render_slide(&p, 1, &o));
    assert!(d1 < 40, "end differs {d1}");
    // Halfway the shared words are on their way: ink between the two texts' rows.
    let mid = frame(0.5);
    let ink = (95..140).flat_map(|y| (0..480).map(move |x| (x, y))).filter(|&(x, y)| mid.pixel(x, y)[0] < 128).count();
    assert!(ink > 50, "ink {ink}");
}

#[test]
fn morph_path_ends_match_the_slides() {
    let from = Xfrm::new(100.0, 100.0, 200.0, 200.0);
    let to = Xfrm::new(500.0, 150.0, 300.0, 150.0);
    let form = |id: u32, g: &str, x: Xfrm| Shape {
        id: ShapeId(id),
        xfrm: Some(x),
        geom: Geom::preset(g),
        style: Some(ShapeStyle::accent(deckcraft_color::SchemeSlot::Accent1)),
        ..Default::default()
    };
    let mut p = deck_with(vec![form(1, "ellipse", from)]);
    let mut b = (*p.slides[0]).clone();
    b.shapes = vec![form(1, "star5", to)];
    p.slides.push(std::sync::Arc::new(b));
    let o = RenderOpts { scale: 0.5, ..Default::default() };
    let frame = |t: f64| {
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        let x = Xfrm::new(lerp(from.x, to.x), lerp(from.y, to.y), lerp(from.w, to.w), lerp(from.h, to.h));
        let pm = PathMorph { id: ShapeId(10), from: Geom::preset("ellipse"), from_box: from, to_box: to, t };
        render_blend(&p, 0, 1, t, &[(form(10, "star5", x), false)], &[], &[pm], &o)
    };
    // The morph splits the outlines into more curves, which flatten slightly differently: edge pixels
    // may differ a little, a different shape differs a lot.
    let differ = |a: &Image, b: &Image| a.pixels.iter().zip(&b.pixels).filter(|(x, y)| x.abs_diff(**y) > 60).count();
    // At the start the star is still the circle, at the end the star.
    let d0 = differ(&frame(0.0), &render_slide(&p, 0, &o));
    assert!(d0 < 40, "start differs {d0}");
    let d1 = differ(&frame(1.0), &render_slide(&p, 1, &o));
    assert!(d1 < 40, "end differs {d1}");
    // Without the morph the start would show the star.
    let star = render_blend(&p, 0, 1, 0.0, &[(form(10, "star5", from), false)], &[], &[], &o);
    assert!(differ(&star, &render_slide(&p, 0, &o)) > 400);
    // Halfway the shape is filled around the middle of its box.
    let mid = frame(0.5);
    let c = (from.center().to_vec2() + to.center().to_vec2()) * 0.5 * 0.5;
    assert_ne!(mid.pixel(c.x as u32, c.y as u32), [255, 255, 255, 255]);
}
