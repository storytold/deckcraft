use super::layout::layout_raw;
use super::nodes::Item;
use super::{
    Align, AutoFit, ColorRef, ColorTransform, Diagram, DiagramNode, Effects, Fill, Geom, Gradient, GradientShape, GradientStop, Line, MAX_LEVEL,
    Paragraph, SchemeSlot, Shadow, Shape, ShapeStyle, TextBody, Xfrm,
};
use deckcraft_model::RunProps;
use deckcraft_model::text::RunKind;

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
                flip: None,
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

/// Lay out `d`, painted with its style and colours, each shape with the nodes it shows. All the
/// text of a graphic is sized alike, as PowerPoint does: the one scale that lets every shape's
/// longest word fit on a line, or the size the user set (`Diagram::font_size`) for every node.
/// Text that still overflows down a shape is fitted on the slide by [`share_shrink`], again at
/// one scale for every shape.
pub(super) fn layout_items(d: &Diagram, area: Xfrm) -> Vec<Item> {
    let mut v = layout_raw(d, area);
    let scale = v.iter().map(|(sh, _)| word_scale(sh, 1.0)).fold(1.0_f64, f64::min);
    for (sh, _) in &mut v {
        // A set size replaces the automatic one outright: the long word's scale would otherwise
        // still narrow the bullets' indents.
        match d.font_size.filter(|z| z.is_finite() && *z > 0.0) {
            Some(size) => set_size(sh, size.clamp(1.0, 4000.0)),
            None => apply_scale(sh, scale),
        }
        if let Some(c) = &d.text_color {
            set_color(sh, c);
        }
        if let Some(o) = &d.text_style {
            set_props(sh, o);
        }
    }
    v
}

/// How much a resized graphic stretches its children on the slide: the group's box over its child
/// space, per axis (1 where the child space is empty).
pub(super) fn child_scale(group: &Xfrm, child: &Xfrm) -> (f64, f64) {
    let k = |g: f64, c: f64| if c.abs() > 1e-9 && (g / c).is_finite() && (g / c).abs() > 1e-9 { (g / c).abs() } else { 1.0 };
    (k(group.w, child.w), k(group.h, child.h))
}

/// The text of `sh` laid out as the renderer draws it (None: no text or no box): in the box the
/// shape has on the slide, its own size stretched by the group's `scale` ([`child_scale`]).
pub(super) fn laid_out(ctx: &deckcraft_model::resolve::Ctx<'_>, sh: &Shape, scale: (f64, f64)) -> Option<deckcraft_text::TextLayout> {
    let body = sh.text.as_ref()?;
    let x = sh.xfrm?;
    let rect = deckcraft_render::shape_geometry(sh, x.w * scale.0, x.h * scale.1).text_rect;
    let opts = deckcraft_text::Opts { rect, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false };
    Some(deckcraft_text::layout(ctx, sh, body, &opts))
}

/// The font scale and line reduction shrink-on-overflow gives the text of `sh` (None: it does not shrink).
pub(super) fn fitted(ctx: &deckcraft_model::resolve::Ctx<'_>, sh: &Shape, scale: (f64, f64)) -> Option<(f64, f64)> {
    sh.text.as_ref().filter(|b| matches!(b.body.autofit, Some(AutoFit::Shrink { .. })))?;
    let l = laid_out(ctx, sh, scale)?;
    Some((l.font_scale, l.line_reduction))
}

/// One shrink-on-overflow scale for every shape of an automatically sized graphic, as PowerPoint
/// sizes SmartArt text alike: the smallest scale any shape's text needs to fit its box, across or
/// down (a node of many lines as much as a long word), saved on every shape. A saved scale is
/// drawn as it is, so no shape shrinks on its own. A set size (`AutoFit::None`) is left alone.
pub(super) fn share_shrink(ctx: &deckcraft_model::resolve::Ctx<'_>, shapes: &mut [Shape], stretch: (f64, f64)) {
    let Some((fit, reduction)) = shapes.iter().filter_map(|sh| fitted(ctx, sh, stretch)).min_by(|a, b| a.0.total_cmp(&b.0)) else { return };
    // The rule layout_items applies at generation, against the displayed widths: a word an edit
    // made too long for its line breaks mid-word without overflowing down, so it shrinks every node.
    let words = shapes
        .iter()
        .filter(|sh| sh.text.as_ref().is_some_and(|b| matches!(b.body.autofit, Some(AutoFit::Shrink { .. }))))
        .map(|sh| word_scale(sh, stretch.0))
        .fold(1.0_f64, f64::min);
    let scale = fit.min(words);
    if scale >= 1.0 {
        return;
    }
    for sh in shapes {
        if let Some(t) = sh.text.as_mut()
            && let Some(AutoFit::Shrink { font_scale, line_reduction }) = &mut t.body.autofit
        {
            (*font_scale, *line_reduction) = (Some(scale), reduction);
        }
    }
}

/// Every run of the shape's text at `size`; a fixed size turns shrink-on-overflow off, so no node
/// is fitted to a size of its own.
pub(super) fn set_size(sh: &mut Shape, size: f64) {
    let Some(body) = &mut sh.text else { return };
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            r.props.size = Some(size);
        }
        p.end_props.size = Some(size);
    }
    body.body.autofit = Some(AutoFit::None);
}

pub(super) fn set_color(sh: &mut Shape, c: &ColorRef) {
    let Some(body) = &mut sh.text else { return };
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            r.props.fill = Some(Fill::solid(c.clone()));
        }
        p.end_props.fill = Some(Fill::solid(c.clone()));
    }
}

/// Every run of the shape's text without a colour of its own.
pub(super) fn unset_color(sh: &mut Shape) {
    let Some(body) = &mut sh.text else { return };
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            r.props.fill = None;
        }
        p.end_props.fill = None;
    }
}

/// The Font dialog's settings (`Diagram::text_style`) on every run of the shape's text: only the properties `o` sets.
pub(super) fn set_props(sh: &mut Shape, o: &RunProps) {
    let Some(body) = &mut sh.text else { return };
    let put = |r: &mut RunProps| {
        if o.font.is_some() {
            r.font = o.font.clone();
        }
        r.bold = o.bold.or(r.bold);
        r.italic = o.italic.or(r.italic);
        if o.underline.is_some() {
            r.underline = o.underline.clone();
        }
        r.strike = o.strike.or(r.strike);
        r.baseline = o.baseline.or(r.baseline);
        r.caps = o.caps.or(r.caps);
        r.spacing = o.spacing.or(r.spacing);
    };
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            put(&mut r.props);
        }
        put(&mut p.end_props);
    }
}

/// Clear All Formatting on the shape's text: every run property but the language and the link, as `format.clear` does
/// on text.
pub(super) fn clear_props(sh: &mut Shape) {
    let Some(body) = &mut sh.text else { return };
    let clear = |r: &mut RunProps| *r = RunProps { lang: r.lang.take(), link: r.link.take(), ..Default::default() };
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            clear(&mut r.props);
        }
        clear(&mut p.end_props);
    }
}

/// Rough advance of a character as a fraction of the font size (a sans text face).
const CHAR_W: f64 = 0.6;

/// Smallest font scale shrinking to fit goes to.
pub(super) const MIN_SCALE: f64 = 0.25;

/// The scale (`MIN_SCALE..=1`) at which no word of the shape's text is wider than its line and
/// would break mid-word. Overflow in height is left to the body's shrink-on-overflow autofit.
fn word_scale(sh: &Shape, stretch: f64) -> f64 {
    let Some(w) = sh.xfrm.map(|x| x.w * stretch) else { return 1.0 };
    // Round shapes keep their text in the inscribed rectangle (about 71% of the width).
    let text_w = match sh.geom.preset_name() {
        Some("ellipse" | "gear6" | "gear9") => w * 0.71,
        _ => w,
    };
    let Some(body) = &sh.text else { return 1.0 };
    let avail = text_w - body.body.inset_l.unwrap_or(7.2) - body.body.inset_r.unwrap_or(7.2);
    let mut scale: f64 = 1.0;
    for p in &body.paragraphs {
        let line = avail - p.props.margin_left.unwrap_or(0.0).max(0.0);
        // A word split across runs by formatting is one word: sum its fragments' widths, and end
        // it only at whitespace or a line break.
        let mut word = 0.0;
        let mut fit = |word: &mut f64| {
            if *word > 0.0 {
                scale = scale.min(line.max(0.0) / *word);
            }
            *word = 0.0;
        };
        for r in &p.runs {
            if matches!(r.kind, RunKind::Break) {
                fit(&mut word);
                continue;
            }
            let size = r.props.size.unwrap_or(18.0);
            for c in r.text.chars() {
                if c.is_whitespace() {
                    fit(&mut word);
                } else {
                    word += size * CHAR_W;
                }
            }
        }
        fit(&mut word);
    }
    scale.max(MIN_SCALE)
}

/// Shrink the shape's text (all paragraphs together, as PowerPoint's per-shape font size does).
fn apply_scale(sh: &mut Shape, scale: f64) {
    if scale >= 0.999 {
        return;
    }
    let Some(body) = &mut sh.text else { return };
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

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::text::Run;

    fn shape(runs: Vec<Run>) -> Shape {
        let mut sh = Shape { xfrm: Some(Xfrm::new(0.0, 0.0, 100.0, 40.0)), ..Default::default() };
        sh.text = Some(TextBody { paragraphs: vec![Paragraph { runs, ..Default::default() }], ..Default::default() });
        sh
    }

    #[test]
    fn a_word_split_across_runs_is_measured_whole() {
        let run = |t: &str| Run::new(t);
        // 10 characters at 18 pt need 108 pt; the line is 100 - 14.4 = 85.6 pt. Each half (54 pt) fits.
        let whole = word_scale(&shape(vec![run("Internatio")]), 1.0);
        assert!(whole < 1.0);
        let split = word_scale(&shape(vec![run("Inter"), run("natio")]), 1.0);
        assert!((split - whole).abs() < 1e-9, "{split} vs {whole}");
        // A space or a line break between the fragments ends the word.
        assert_eq!(word_scale(&shape(vec![run("Inter "), run("natio")]), 1.0), 1.0);
        let brk = Run { kind: RunKind::Break, ..Run::new("") };
        assert_eq!(word_scale(&shape(vec![run("Inter"), brk, run("natio")]), 1.0), 1.0);
    }
}
