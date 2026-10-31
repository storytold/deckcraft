//! Owned corpus: text effects, body layout, theme effects, fills, bevels and connectors.
//! Open → save → reopen, twice, and compare the model.

use std::sync::Arc;

use deckcraft_color::{ColorTransform, Rgba, SchemeSlot};
use deckcraft_geom::Xfrm;
use deckcraft_model::style::{ColorRef, Dash, Effects, Fill, Glow, Gradient, GradientShape, GradientStop, Line, PatternFill, Reflection, Shadow};
use deckcraft_model::text::{Align, AutoFit, BodyProps, ParaProps, Paragraph, Run, RunProps, Spacing, TabStop, TextBody, TextDir};
use deckcraft_model::{Geom, Presentation, Shape, ShapeId, ShapeKind, defaults};

fn shape(p: &mut Presentation, name: &str, x: f64, y: f64, w: f64, h: f64) -> Shape {
    Shape { id: ShapeId(p.alloc_id().expect("id")), name: name.into(), xfrm: Some(Xfrm::new(x, y, w, h)), ..Default::default() }
}

fn shadow(inner: bool, dir: f64) -> Shadow {
    Shadow {
        color: ColorRef::rgb(if inner { Rgba::rgb(0, 0, 80) } else { Rgba::BLACK }).with(ColorTransform::Alpha(50_000)),
        blur: if inner { 4.0 } else { 8.0 },
        dist: if inner { 2.0 } else { 5.0 },
        dir,
        inner,
        sx: if inner { 1.0 } else { 0.5 },
        sy: 1.0,
        kx: 0.0,
        ky: if inner { 0.0 } else { 15.0 },
        align: if inner { String::new() } else { "ctr".into() },
        rotate_with_shape: inner,
    }
}

fn glow() -> Glow {
    Glow { color: ColorRef::scheme(SchemeSlot::Accent3), radius: 10.0 }
}

fn reflection() -> Reflection {
    Reflection { blur: 2.0, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.75, dist: 6.0 }
}

fn gradient(angle: f64) -> Fill {
    Fill::Gradient(Gradient {
        stops: vec![
            GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::rgb(255, 80, 0)) },
            GradientStop { pos: 1.0, color: ColorRef::scheme(SchemeSlot::Accent1) },
        ],
        shape: GradientShape::Linear { angle, scaled: true },
        rotate_with_shape: true,
        tile: [0.0; 4],
        flip: None,
    })
}

fn styled_run() -> RunProps {
    RunProps {
        fill: Some(gradient(20.0)),
        outline: Some(Line { dash: Some(Dash::Preset { name: "dash".into() }), ..Line::solid(ColorRef::rgb(Rgba::rgb(0, 40, 80)), 1.5) }),
        shadow: Some(shadow(false, 135.0)),
        inner_shadow: Some(shadow(true, 45.0)),
        glow: Some(glow()),
        reflection: Some(reflection()),
        soft_edge: Some(4.0),
        ..Default::default()
    }
}

fn style_deck() -> Presentation {
    let mut p = defaults::new_presentation(None);
    let master = Arc::make_mut(p.masters.first_mut().expect("master"));
    master.theme.format.effects[0] = Effects {
        glow: Some(glow()),
        reflection: Some(reflection()),
        // Preset only: no kept `scene3d`/`sp3d`. A save must still write the bevel.
        bevel: Some("circle".into()),
        ..Default::default()
    };

    let layout = defaults::layout_of_kind(&p, deckcraft_model::LayoutType::Blank).unwrap_or_default();
    let mut s = defaults::new_slide(&mut p, layout).expect("slide");

    let mut word = shape(&mut p, "Styled text", 40.0, 40.0, 400.0, 180.0);
    word.text = Some(TextBody {
        body: BodyProps {
            autofit: Some(AutoFit::Shrink { font_scale: Some(0.8), line_reduction: 0.2 }),
            columns: Some(2),
            col_spacing: Some(12.0),
            vert: Some(TextDir::Vertical270),
            rtl_col: Some(true),
            ..Default::default()
        },
        paragraphs: vec![Paragraph {
            props: ParaProps {
                align: Some(Align::Right),
                rtl: Some(true),
                line_spacing: Some(Spacing::Pct(1.5)),
                space_before: Some(Spacing::Pts(6.0)),
                tabs: Some(vec![
                    TabStop { pos: 36.0, align: "l".into() },
                    TabStop { pos: 72.0, align: "ctr".into() },
                    TabStop { pos: 108.0, align: "r".into() },
                    TabStop { pos: 144.0, align: "dec".into() },
                ]),
                ..Default::default()
            },
            runs: vec![Run::with("Gradient glow", styled_run())],
            ..Default::default()
        }],
        ..Default::default()
    });

    let mut path = shape(&mut p, "Path gradient", 40.0, 260.0, 140.0, 100.0);
    path.fill = Some(Fill::Gradient(Gradient {
        stops: vec![
            GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::rgb(10, 20, 30)) },
            GradientStop { pos: 0.5, color: ColorRef::scheme(SchemeSlot::Accent5) },
            GradientStop { pos: 1.0, color: ColorRef::rgb(Rgba::rgb(240, 240, 240)) },
        ],
        shape: GradientShape::Path { path: "circle".into(), focus: [0.2, 0.2, 0.2, 0.2] },
        rotate_with_shape: false,
        tile: [0.0; 4],
        flip: None,
    }));

    let mut patt = shape(&mut p, "Pattern", 200.0, 260.0, 120.0, 80.0);
    patt.fill =
        Some(Fill::Pattern(PatternFill { preset: "smCheck".into(), fg: ColorRef::scheme(SchemeSlot::Accent4), bg: ColorRef::rgb(Rgba::WHITE) }));

    let mut named = shape(&mut p, "Bevel preset", 460.0, 260.0, 80.0, 60.0);
    named.effects = Some(Effects { bevel: Some("relaxedInset".into()), ..Default::default() });

    let mut bevel = shape(&mut p, "Bevel", 340.0, 260.0, 100.0, 80.0);
    bevel.effects = Some(Effects {
        raw3d: Some(
            "<a:scene3d><a:camera prst=\"orthographicFront\"><a:rot lat=\"0\" lon=\"0\" rev=\"0\"/></a:camera><a:lightRig rig=\"threePt\" dir=\"t\"/></a:scene3d><a:sp3d><a:bevelT w=\"63500\" h=\"25400\" prst=\"angle\"/></a:sp3d>"
                .into(),
        ),
        ..Default::default()
    });

    let left = shape(&mut p, "Left", 40.0, 400.0, 80.0, 40.0);
    let right = shape(&mut p, "Right", 240.0, 400.0, 80.0, 40.0);
    let (left_id, right_id) = (left.id, right.id);
    let mut cx = shape(&mut p, "Connector", 120.0, 420.0, 120.0, 0.0);
    cx.kind = ShapeKind::Connector { start: Some((left_id, 3)), end: Some((right_id, 1)) };
    cx.geom = Geom::preset("straightConnector1");
    cx.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Tx1), 1.5));

    s.shapes.extend([word, path, patt, named, bevel, left, right, cx]);
    p.slides.push(Arc::new(s));
    p
}

fn round(p: &Presentation) -> Presentation {
    let bytes = deckcraft_pptx::export(p).expect("export");
    deckcraft_pptx::import(&bytes).expect("import")
}

fn by_name<'a>(p: &'a Presentation, name: &str) -> &'a Shape {
    p.slides.iter().flat_map(|s| &s.shapes).find(|s| s.name == name).unwrap_or_else(|| panic!("missing {name}"))
}

fn run_of(p: &Presentation) -> &RunProps {
    &by_name(p, "Styled text").text.as_ref().expect("text").paragraphs[0].runs[0].props
}

#[test]
fn style_kinds_round_trip() {
    let p = style_deck();
    let once = round(&p);
    let twice = round(&once);

    let body = &by_name(&once, "Styled text").text.as_ref().expect("text").body;
    assert_eq!(body.autofit, Some(AutoFit::Shrink { font_scale: Some(0.8), line_reduction: 0.2 }));
    assert_eq!(body.columns, Some(2));
    assert_eq!(body.col_spacing, Some(12.0));
    assert_eq!(body.vert, Some(TextDir::Vertical270));
    assert_eq!(body.rtl_col, Some(true));
    let para = &by_name(&once, "Styled text").text.as_ref().expect("text").paragraphs[0].props;
    assert_eq!(para.rtl, Some(true));
    assert_eq!(para.align, Some(Align::Right));
    assert_eq!(para.line_spacing, Some(Spacing::Pct(1.5)));
    assert_eq!(para.space_before, Some(Spacing::Pts(6.0)));
    assert_eq!(
        para.tabs.as_deref(),
        Some(
            [
                TabStop { pos: 36.0, align: "l".into() },
                TabStop { pos: 72.0, align: "ctr".into() },
                TabStop { pos: 108.0, align: "r".into() },
                TabStop { pos: 144.0, align: "dec".into() },
            ]
            .as_slice()
        )
    );
    text_run_effects_survive(&p, &once, &twice);
    theme_and_bevel_survive(&once, &twice);
    connector_glue_survives(&p, &once, &twice);
}

fn text_run_effects_survive(p: &Presentation, once: &Presentation, twice: &Presentation) {
    let got = run_of(once);
    let expect = run_of(p);
    assert_eq!(got.fill, expect.fill, "text gradient");
    assert_eq!(got.outline, expect.outline, "text outline");
    assert_eq!(got.shadow, expect.shadow, "text shadow");
    assert_eq!(got.glow, expect.glow, "text glow");
    assert_eq!(got.inner_shadow, expect.inner_shadow, "text inner shadow");
    assert_eq!(got.reflection, expect.reflection, "text reflection");
    assert_eq!(got.soft_edge, expect.soft_edge, "text soft edge");
    assert_eq!(run_of(twice), run_of(once));
    assert_eq!(
        by_name(twice, "Styled text").text.as_ref().map(|t| (&t.body, &t.paragraphs[0].props)),
        by_name(once, "Styled text").text.as_ref().map(|t| (&t.body, &t.paragraphs[0].props))
    );

    assert_eq!(by_name(once, "Path gradient").fill, by_name(p, "Path gradient").fill);
    assert_eq!(by_name(once, "Pattern").fill, by_name(p, "Pattern").fill);
    assert_eq!(by_name(twice, "Path gradient").fill, by_name(once, "Path gradient").fill);
    assert_eq!(by_name(twice, "Pattern").fill, by_name(once, "Pattern").fill);
}

fn theme_and_bevel_survive(once: &Presentation, twice: &Presentation) {
    let theme = &once.masters[0].theme.format.effects[0];
    assert_eq!(theme.glow, Some(glow()));
    assert_eq!(theme.reflection, Some(reflection()));
    assert_eq!(theme.bevel.as_deref(), Some("circle"), "theme bevel preset");
    let theme2 = &twice.masters[0].theme.format.effects[0];
    assert_eq!(theme2.glow, theme.glow);
    assert_eq!(theme2.reflection, theme.reflection);
    assert_eq!(theme2.bevel, theme.bevel);
    assert_eq!(theme2.raw3d, theme.raw3d);

    let named = by_name(once, "Bevel preset").effects.as_ref().expect("effects");
    assert_eq!(named.bevel.as_deref(), Some("relaxedInset"));
    assert!(named.raw3d.as_deref().unwrap_or("").contains("relaxedInset"));
    assert_eq!(by_name(twice, "Bevel preset").effects.as_ref().and_then(|e| e.bevel.as_deref()), Some("relaxedInset"));

    let bevel = by_name(once, "Bevel").effects.as_ref().expect("effects");
    assert_eq!(bevel.bevel.as_deref(), Some("angle"));
    let raw = bevel.raw3d.as_deref().unwrap_or("");
    assert!(raw.contains("bevelT") && raw.contains("angle") && raw.contains("scene3d"), "{raw}");
    assert_eq!(by_name(twice, "Bevel").effects.as_ref().and_then(|e| e.raw3d.as_deref()), Some(raw));
}

fn connector_glue_survives(p: &Presentation, once: &Presentation, twice: &Presentation) {
    let c = by_name(once, "Connector");
    assert!(matches!(c.kind, ShapeKind::Connector { start: Some((s, 3)), end: Some((e, 1)) } if s == left_of(p) && e == right_of(p)), "{:?}", c.kind);
    assert_eq!(c.line, by_name(p, "Connector").line);
    assert_eq!(c.geom, by_name(p, "Connector").geom);
    assert_eq!(by_name(twice, "Connector").kind, c.kind);
}

fn left_of(p: &Presentation) -> ShapeId {
    by_name(p, "Left").id
}
fn right_of(p: &Presentation) -> ShapeId {
    by_name(p, "Right").id
}
