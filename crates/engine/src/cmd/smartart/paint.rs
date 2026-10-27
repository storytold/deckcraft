use super::layout::layout_raw;
use super::nodes::Item;
use super::*;

fn slot(i: usize) -> SchemeSlot {
    [SchemeSlot::Accent1, SchemeSlot::Accent2, SchemeSlot::Accent3, SchemeSlot::Accent4, SchemeSlot::Accent5, SchemeSlot::Accent6][i % 6]
}

/// The accent shape `i` is drawn in under Change Colors: `colorful` cycles the accents, `accentN`
/// keeps one.
pub(super) fn accent(d: &Diagram, i: usize) -> SchemeSlot {
    let digit = |s: &str, k: usize| s.chars().nth(k).and_then(|c| c.to_digit(10)).map(|n| n as usize);
    if let Some(r) = d.colors.strip_prefix("range")
        && let Some(a @ 1..=6) = digit(r, 0)
    {
        // Colorful Range: alternate between two neighbouring accents.
        return slot(a - 1 + i % 2);
    }
    match d.colors.strip_prefix("accent").and_then(|r| digit(r, 0)) {
        Some(n @ 1..=6) => slot(n - 1),
        _ => slot(i),
    }
}

/// Gradient Range: each shape a step darker than the one before (the steps cycle).
pub(super) fn shade(d: &Diagram, i: usize) -> Option<ColorTransform> {
    d.colors.ends_with("Grad").then(|| ColorTransform::LumMod(100_000 - (i % 4) as i32 * 15_000))
}

/// The 3-D XML (`scene3d` + `sp3d`) for a 3-D SmartArt Style: a top bevel (`w`, `h` in points)
/// lit by the three-point rig.
fn three_d(bevel: &str, w: f64, h: f64, camera: &str) -> String {
    format!(
        "<a:scene3d><a:camera prst=\"{camera}\"/><a:lightRig rig=\"threePt\" dir=\"t\"/></a:scene3d><a:sp3d prstMaterial=\"matte\"><a:bevelT w=\"{}\" h=\"{}\" prst=\"{bevel}\"/></a:sp3d>",
        (w * 12700.0).round() as i64,
        (h * 12700.0).round() as i64
    )
}

/// Brick Scene's camera: the diagram's shapes seen from above and to the side.
pub const BRICK_CAMERA: &str = "isometricOffAxis2Top";

/// Whether `style` is one of the 3-D SmartArt Styles.
pub fn is_3d(style: &str) -> bool {
    matches!(style, "polished" | "inset" | "cartoon" | "powder" | "brick")
}

/// Fill, outline, text colour and effects for the SmartArt style. `light` shapes (the bullet
/// boxes of the bullet lists) are a pale tint of the accent with dark text.
pub(super) fn paint(d: &Diagram, sh: &mut Shape, i: usize, light: bool) {
    let a = accent(d, i);
    let base = match shade(d, i) {
        Some(t) => ColorRef::scheme(a).with(t),
        None => ColorRef::scheme(a),
    };
    let tint = |pct: i32| ColorRef::scheme(a).with(ColorTransform::LumMod(100_000 - pct * 1000)).with(ColorTransform::LumOff(pct * 1000));
    let dark_text = light || matches!(d.style.as_str(), "outline" | "powder");
    sh.style = Some(ShapeStyle::accent(a));
    match d.style.as_str() {
        _ if light => {
            sh.fill = Some(Fill::solid(tint(80)));
            sh.line = Some(Line::solid(tint(60), 1.0));
        }
        "outline" => {
            sh.fill = Some(Fill::solid(ColorRef::scheme(SchemeSlot::Lt1)));
            sh.line = Some(Line::solid(base.clone(), 2.25));
        }
        "subtle" => {
            sh.fill = Some(Fill::solid(base.clone()));
            sh.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.5));
            sh.effects = Some(Effects { outer_shadow: Some(shadow(4.0, 3.0)), ..Default::default() });
        }
        "polished" | "inset" | "cartoon" | "powder" | "brick" => {
            let (bevel, bw, line, fill) = match d.style.as_str() {
                "polished" => ("circle", 12.0, Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 0.75)), base.clone()),
                "inset" => ("relaxedInset", 10.0, None, base.clone()),
                "cartoon" => ("softRound", 8.0, Some(Line::solid(ColorRef::scheme(SchemeSlot::Tx1), 2.0)), base.clone()),
                "powder" => ("convex", 14.0, None, tint(40)),
                _ => ("slope", 6.0, Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.0)), base.clone()),
            };
            sh.fill = Some(Fill::solid(fill));
            sh.line = Some(line.unwrap_or_else(|| Line { fill: Some(Fill::None), ..Default::default() }));
            sh.effects = Some(Effects {
                outer_shadow: (d.style != "inset").then(|| shadow(6.0, 4.0)),
                bevel: Some(bevel.into()),
                raw3d: Some(three_d(bevel, bw, bw * 0.6, if d.style == "brick" { BRICK_CAMERA } else { "orthographicFront" })),
                ..Default::default()
            });
        }
        "intense" => {
            sh.fill = Some(Fill::Gradient(Gradient {
                stops: vec![
                    GradientStop { pos: 0.0, color: tint(30) },
                    GradientStop { pos: 1.0, color: base.clone().with(ColorTransform::LumMod(75_000)) },
                ],
                shape: GradientShape::Linear { angle: 90.0, scaled: false },
                rotate_with_shape: true,
                tile: [0.0; 4],
            }));
            sh.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.0));
            sh.effects = Some(Effects { outer_shadow: Some(shadow(8.0, 5.0)), ..Default::default() });
        }
        _ => {
            sh.fill = Some(Fill::solid(base.clone()));
            sh.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.5));
        }
    }
    if let Some(body) = &mut sh.text {
        let c = if dark_text { ColorRef::scheme(SchemeSlot::Tx1) } else { ColorRef::scheme(SchemeSlot::Lt1) };
        for p in &mut body.paragraphs {
            for r in &mut p.runs {
                r.props.fill = Some(Fill::solid(c.clone()));
            }
        }
    }
}

fn shadow(blur: f64, dist: f64) -> Shadow {
    Shadow {
        color: ColorRef::rgb(deckcraft_model::Rgba { r: 0, g: 0, b: 0, a: 255 }).with(ColorTransform::Alpha(40_000)),
        blur,
        dist,
        dir: 90.0,
        inner: false,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: String::new(),
        rotate_with_shape: false,
    }
}

/// A shape with text: the first line as the title (centred, larger), the rest as bullets. With
/// `title` false every line is a bullet (the bullet boxes of the bullet lists).
pub(super) fn text_shape(preset: &str, x: Xfrm, lines: &[&DiagramNode], title: bool) -> Shape {
    let mut body = TextBody::default();
    body.body.autofit = Some(AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 });
    let top = lines.first().map(|n| if title { n.level } else { n.level.saturating_sub(1) }).unwrap_or(0);
    for (k, n) in lines.iter().enumerate() {
        let mut para = Paragraph::new(&n.text);
        let head = title && k == 0;
        if head {
            para.props.align = Some(Align::Center);
        } else {
            para.level = n.level.saturating_sub(top).saturating_sub(1).min(MAX_LEVEL);
            para.props.align = Some(Align::Left);
            para.props.bullet = Some(deckcraft_model::text::Bullet::Char { char: "•".into() });
            para.props.margin_left = Some(12.0 * (para.level as f64 + 1.0));
            para.props.indent = Some(-10.0);
        }
        let size = if head { 20.0 } else { 14.0 };
        for r in &mut para.runs {
            r.props.size = Some(size);
        }
        para.end_props.size = Some(size);
        body.paragraphs.push(para);
    }
    if body.paragraphs.is_empty() {
        body.paragraphs.push(Paragraph::new(""));
    }
    if !title {
        body.body.anchor = Some(deckcraft_model::text::Anchor::Top);
    }
    Shape { name: "SmartArt shape".into(), xfrm: Some(x), geom: Geom::preset(preset), text: Some(body), ..Default::default() }
}

/// A plain shape with no text (arrows between steps).
pub(super) fn deco(preset: &str, x: Xfrm, rot: f64) -> Shape {
    let mut x = x;
    x.rot = rot;
    Shape { name: "SmartArt arrow".into(), xfrm: Some(x), geom: Geom::preset(preset), ..Default::default() }
}

pub(super) fn line_shape(a: (f64, f64), b: (f64, f64)) -> Shape {
    let mut x = Xfrm::new(a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs());
    x.flip_h = (b.0 < a.0) != (b.1 < a.1);
    Shape {
        name: "SmartArt connector".into(),
        xfrm: Some(x),
        geom: Geom::preset("line"),
        fill: Some(Fill::None),
        line: Some(Line::solid(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::LumMod(60_000)), 1.25)),
        ..Default::default()
    }
}

/// The diagram's shapes laid out in `area` (ids not yet assigned).
pub fn layout(d: &Diagram, area: Xfrm) -> Vec<Shape> {
    layout_items(d, area).into_iter().map(|(s, _)| s).collect()
}

/// Lay out `d`, painted with its style and colours, each shape with the nodes it shows; text is
/// shrunk per shape so its longest word fits on a line.
pub(super) fn layout_items(d: &Diagram, area: Xfrm) -> Vec<Item> {
    let mut v = layout_raw(d, area);
    for (sh, _) in &mut v {
        fit_words(sh);
    }
    v
}

/// Rough advance of a character as a fraction of the font size (a sans text face).
const CHAR_W: f64 = 0.6;

/// Smallest font scale shrinking to fit goes to.
pub(super) const MIN_SCALE: f64 = 0.25;

/// Shrink the shape's text (all paragraphs together, as PowerPoint's per-shape font size does) so
/// no word is wider than its line and would break mid-word. Overflow in height is left to the
/// body's shrink-on-overflow autofit.
fn fit_words(sh: &mut Shape) {
    let Some(w) = sh.xfrm.map(|x| x.w) else { return };
    // Round shapes keep their text in the inscribed rectangle (about 71% of the width).
    let text_w = match sh.geom.preset_name() {
        Some("ellipse" | "gear6" | "gear9") => w * 0.71,
        _ => w,
    };
    let Some(body) = &mut sh.text else { return };
    let avail = text_w - body.body.inset_l.unwrap_or(7.2) - body.body.inset_r.unwrap_or(7.2);
    let mut scale: f64 = 1.0;
    for p in &body.paragraphs {
        let line = avail - p.props.margin_left.unwrap_or(0.0).max(0.0);
        for r in &p.runs {
            let size = r.props.size.unwrap_or(18.0);
            let word = r.text.split_whitespace().map(|t| t.chars().count()).max().unwrap_or(0) as f64;
            let need = word * size * CHAR_W;
            if need > 0.0 {
                scale = scale.min(line.max(0.0) / need);
            }
        }
    }
    let scale = scale.max(MIN_SCALE);
    if scale >= 0.999 {
        return;
    }
    let shrink = |v: &mut Option<f64>| {
        if let Some(x) = v {
            *x = (*x * scale * 2.0).floor() / 2.0;
        }
    };
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            shrink(&mut r.props.size);
        }
        shrink(&mut p.end_props.size);
        shrink(&mut p.props.margin_left);
        if let Some(i) = &mut p.props.indent {
            *i *= scale;
        }
    }
}

/// A picture placeholder for node `node`: its picture when it has one, else a pale box.
pub(super) fn picture_shape(d: &Diagram, preset: &str, x: Xfrm, i: usize, node: usize) -> Shape {
    let mut sh = Shape { name: "SmartArt picture".into(), xfrm: Some(x), geom: Geom::preset(preset), ..Default::default() };
    paint(d, &mut sh, i, true);
    if let Some(m) = d.nodes.get(node).and_then(|n| n.picture) {
        sh.fill = Some(Fill::Picture(deckcraft_model::style::PictureFill { media: m, ..Default::default() }));
        sh.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.5));
    }
    sh
}
