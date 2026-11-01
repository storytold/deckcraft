//! Generated interoperability probes. Set DECKCRAFT_VISUAL_OUT to inspect PNGs locally;
//! no presentation files, reference screenshots or font assets are stored in the repository.
use deckcraft_color::Rgba;
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::chart::{ChartTextRole as Role, ChartType};
use deckcraft_model::style::{ColorRef, PictureFill};
use deckcraft_model::text::{Anchor, Bullet, RunProps, TextBody};
use deckcraft_model::theme::ScriptFont;
use deckcraft_model::{Chart, Diagram, DiagramNode, Fill, LayoutType, Presentation, Shape, ShapeId, ShapeKind, Xfrm, defaults};
use std::sync::Arc;

fn textbox(id: u32, text: &str, rect: Xfrm, size: f64) -> Shape {
    let mut body = TextBody::from_text(text);
    body.body.anchor = Some(Anchor::Top);
    for para in &mut body.paragraphs {
        para.props.bullet = Some(Bullet::None);
        for run in &mut para.runs {
            run.props = RunProps { font: Some("+mn-lt".into()), size: Some(size), ..Default::default() };
        }
    }
    Shape { id: ShapeId(id), name: format!("Probe {id}"), xfrm: Some(rect), text: Some(body), ..Default::default() }
}

fn append(p: &mut Presentation, shapes: Vec<Shape>) {
    let layout = defaults::layout_of_kind(p, LayoutType::Blank).unwrap();
    let mut slide = defaults::new_slide(p, layout).unwrap();
    slide.shapes = shapes;
    p.slides.push(Arc::new(slide));
}

fn probes() -> Presentation {
    let mut p = Presentation::default();
    p.slides.clear();
    let theme = &mut Arc::make_mut(&mut p.masters[0]).theme;
    theme.fonts.minor.latin = "Inter".into();
    theme.fonts.minor.ea.clear();
    theme.fonts.minor.scripts = [("Hans", "BIZ UDPGothic"), ("Hant", "Shippori Mincho"), ("Jpan", "BIZ UDMincho")]
        .into_iter()
        .map(|(script, typeface)| ScriptFont { script: script.into(), typeface: typeface.into() })
        .collect();
    let mut shapes = vec![textbox(1001, "Locale-aware font selection", Xfrm::new(40.0, 25.0, 880.0, 60.0), 30.0)];
    for (i, lang) in ["zh-CN", "zh-TW", "ja-JP"].iter().enumerate() {
        let mut shape = textbox(1002 + i as u32, "中文文字 / English 123", Xfrm::new(190.0, 120.0 + i as f64 * 110.0, 690.0, 70.0), 30.0);
        shape.text.as_mut().unwrap().paragraphs[0].runs[0].props.lang = Some((*lang).into());
        shapes.push(textbox(1010 + i as u32, lang, Xfrm::new(40.0, 120.0 + i as f64 * 110.0, 140.0, 60.0), 22.0));
        shapes.push(shape);
    }
    append(&mut p, shapes);
    let mut chart = Chart::sample(ChartType::Column);
    chart.title = Some("Distinct title, axes and wrapped legend".into());
    chart.categories = ["第一组", "第二组", "第三组", "第四组"].into_iter().map(str::to_string).collect();
    for (i, s) in chart.series.iter_mut().enumerate() {
        s.name = ["IIII narrow legend / 第一组", "WWWW wide legend / 第二组", "Mixed labels / 中文示例"][i % 3].into();
    }
    for (role, size, family) in [
        (Role::Default, 14.0, "Inter"),
        (Role::Title, 24.0, "Inter"),
        (Role::Legend, 22.0, "Inter"),
        (Role::ValueAxis, 12.0, "BIZ UDMincho"),
        (Role::CategoryAxis, 16.0, "Inter"),
    ] {
        chart.text_styles.insert(
            role,
            RunProps {
                font: Some(family.into()),
                font_ea: Some("BIZ UDPGothic".into()),
                lang: Some("zh-CN".into()),
                size: Some(size),
                ..Default::default()
            },
        );
    }
    append(
        &mut p,
        vec![Shape {
            id: ShapeId(1100),
            xfrm: Some(Xfrm::new(55.0, 35.0, 850.0, 460.0)),
            kind: ShapeKind::Chart(Box::new(chart.clone())),
            ..Default::default()
        }],
    );
    chart.kind = ChartType::Combo;
    chart.title = Some("Independent secondary axis".into());
    chart.legend = None;
    chart.series[0].kind = Some(ChartType::Column);
    chart.series[1].kind = Some(ChartType::Line);
    chart.series[1].secondary_axis = true;
    chart.series[1].values = vec![Some(200.0), Some(500.0), Some(400.0), Some(800.0)];
    chart.y2_min = Some(0.0);
    chart.y2_max = Some(1000.0);
    chart.y2_major = Some(250.0);
    chart.text_styles.insert(
        Role::SecondaryValueAxis,
        RunProps {
            font: Some("Inter".into()),
            size: Some(17.0),
            fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(180, 40, 30)))),
            ..Default::default()
        },
    );
    append(
        &mut p,
        vec![Shape {
            id: ShapeId(1200),
            xfrm: Some(Xfrm::new(55.0, 35.0, 850.0, 460.0)),
            kind: ShapeKind::Chart(Box::new(chart)),
            ..Default::default()
        }],
    );
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([50, 140, 180, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let media = p.add_media("swatch.png", "image/png", png.into_inner()).unwrap();
    let leaf = textbox(1301, "Nested node", Xfrm::new(10.0, 15.0, 180.0, 60.0), 22.0);
    let picture = Shape {
        id: ShapeId(1302),
        xfrm: Some(Xfrm::new(210.0, 15.0, 90.0, 60.0)),
        kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
        ..Default::default()
    };
    let nested = Shape {
        id: ShapeId(1303),
        xfrm: Some(Xfrm { rot: 12.0, ..Xfrm::new(60.0, 80.0, 640.0, 180.0) }),
        kind: ShapeKind::Group { children: vec![leaf, picture], child: Xfrm::new(0.0, 0.0, 320.0, 90.0) },
        ..Default::default()
    };
    let diagram = Shape {
        id: ShapeId(1304),
        xfrm: Some(Xfrm::new(80.0, 100.0, 800.0, 350.0)),
        kind: ShapeKind::Group { children: vec![nested], child: Xfrm::new(0.0, 0.0, 800.0, 350.0) },
        diagram: Some(Diagram {
            kind: "process".into(),
            imported: true,
            nodes: vec![DiagramNode { text: "Nested node".into(), ..Default::default() }],
            ..Default::default()
        }),
        ..Default::default()
    };
    append(&mut p, vec![textbox(1300, "SmartArt: nested transform + image node", Xfrm::new(40.0, 25.0, 880.0, 60.0), 30.0), diagram]);
    p
}

#[test]
fn generated_layout_probes_round_trip_and_render() {
    let source = probes();
    let bytes = deckcraft_pptx::export(&source).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(back.slides.len(), 4);
    let path = std::env::var_os("DECKCRAFT_VISUAL_OUT").map(std::path::PathBuf::from);
    if let Some(path) = &path {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("synthetic-layout-probes.pptx"), &bytes).unwrap();
        std::fs::write(path.join("font-inspection.json"), serde_json::to_vec_pretty(&deckcraft_engine::fonts::inspect_fonts(&back)).unwrap())
            .unwrap();
    }
    for i in 0..4 {
        let original = render_slide(&source, i, &RenderOpts { scale: 1.5, ..Default::default() });
        let saved = render_slide(&back, i, &RenderOpts { scale: 1.5, ..Default::default() });
        assert_eq!((saved.width, saved.height), (1440, 810));
        assert!(saved.pixels.chunks_exact(4).filter(|p| p[0] < 200).count() > 1000);
        assert_eq!(original.pixels, saved.pixels, "probe {} changed across PPTX export/import", i + 1);
        if let Some(path) = &path {
            std::fs::write(path.join(format!("source-{}.png", i + 1)), original.to_png()).unwrap();
            std::fs::write(path.join(format!("saved-{}.png", i + 1)), saved.to_png()).unwrap();
        }
    }
}
