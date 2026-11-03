//! `review.contrast`: the contrast of each text run with what the slide draws behind it.
//!
//! The slide is drawn once per text shape with that shape's text left out. Each pixel under a
//! run's glyph boxes is compared, by WCAG relative luminance, with the run's colour drawn over
//! it, so a photo, a gradient, a card or a see-through band behind the text counts as drawn.
//! The answer gives the smallest, 5th-percentile and median contrast of each run, the share of
//! pixels that reach the ratio WCAG 2.2 asks for at the run's drawn size and weight, and what
//! lies behind the run.

use std::collections::BTreeMap;

use deckcraft_color::Rgba;
use deckcraft_geom::{Affine, Point, Rect};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::style::{ColorRef, Fill};
use deckcraft_model::text::{Run, RunKind, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, Slide};
use deckcraft_render::{Image, Placed, PlacedText, RenderOpts};
use deckcraft_text::{GlyphRun, TextLayout};
use serde_json::{Value, json};

use crate::cmd::{bad, f64_or, opt_usize_param};
use crate::{Result, Session};

const CMD: &str = "review.contrast";
/// Text shapes measured in one call: each one costs a drawing of the slide.
const MAX_SHAPES: usize = 64;
/// Runs reported by one call.
const MAX_RUNS: usize = 5_000;
/// Glyph boxes read per text shape.
const MAX_GLYPHS: usize = 20_000;
/// Pixels read per run, about: a run covering more is read on a coarser grid.
const MAX_SAMPLES: f64 = 10_000.0;
/// Pixels kept per run, whatever the grid gives.
const MAX_KEPT: usize = 40_000;
/// Largest drawing, in pixels (64 MB of RGBA).
const MAX_PIXELS: f64 = 16_000_000.0;
/// Entries `ids` may hold.
const MAX_IDS: usize = 10_000;

/// WCAG 2.2 conformance level: AA (success criterion 1.4.3) or AAA (1.4.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Aa,
    Aaa,
}

/// What every measurement on one slide shares.
struct Scene<'a> {
    doc: &'a Presentation,
    index: usize,
    slide: &'a Slide,
    ctx: &'a Ctx<'a>,
    placed: &'a Placed,
    /// The layout's and master's graphics the slide shows, front to back.
    graphics: Vec<Behind>,
    scale: f64,
    level: Level,
}

/// Something drawn behind slide text: its box on the slide, what kind of paint it is and whether
/// it hides what lies under it.
struct Behind {
    rect: Rect,
    kind: &'static str,
    opaque: bool,
}

/// One model run's drawn pieces, gathered across lines and font faces.
struct RunMeasure {
    color: Rgba,
    /// Opacity the colour is drawn with: its alpha times the text's own transparency.
    opacity: f64,
    /// Smallest drawn size in points, to 2 decimals.
    size: f64,
    /// Every piece is drawn bold.
    bold: bool,
    /// Glyph boxes in text space, each with the transform that takes it to image pixels.
    boxes: Vec<(Rect, Affine)>,
    /// Slide-space bounding box of the glyph boxes.
    bounds: Option<Rect>,
}

/// Contrast statistics of one run's pixels.
struct Ratios {
    min: f64,
    p5: f64,
    median: f64,
    /// Share of pixels at or above the required ratio.
    pass: f64,
}

pub(super) fn contrast(s: &mut Session, p: &Value) -> Result<Value> {
    let level = level_param(p)?;
    let ids = ids_param(p)?;
    let st = s.doc()?;
    let doc: &Presentation = &st.doc;
    let index = opt_usize_param(p, "slide", CMD)?.unwrap_or(st.selection.slide);
    let Some(slide) = doc.slides.get(index) else { return Err(bad(CMD, format!("no slide {index}"))) };
    if let Some(ids) = &ids
        && let Some(id) = ids.iter().find(|id| deckcraft_model::find_shape(&slide.shapes, **id).is_none())
    {
        return Err(bad(CMD, format!("no shape {id} on slide {index}")));
    }
    let placed = deckcraft_render::place_slide(doc, index);
    let texts: Vec<&PlacedText> = placed.texts.iter().filter(|t| wanted(slide, &placed, t, ids.as_deref())).collect();
    if texts.len() > MAX_SHAPES {
        return Err(bad(CMD, format!("slide {index} has {} text shapes; give `ids` to measure at most {MAX_SHAPES} at a time", texts.len())));
    }
    let Some(ctx) = Ctx::for_slide(doc, slide) else { return Ok(json!([])) };
    let sc = Scene {
        doc,
        index,
        slide,
        ctx: &ctx,
        placed: &placed,
        graphics: background_graphics(doc, slide, &ctx),
        scale: measure_scale(doc, f64_or(p, "scale", 1.0)),
        level,
    };
    let mut out = vec![];
    for t in texts {
        measure_text(&sc, t, &mut out);
        if out.len() > MAX_RUNS {
            return Err(bad(CMD, format!("the text on slide {index} has more than {MAX_RUNS} runs; give `ids` to measure fewer shapes at a time")));
        }
    }
    Ok(Value::Array(out))
}

fn level_param(p: &Value) -> Result<Level> {
    match p.get("level") {
        None | Some(Value::Null) => Ok(Level::Aa),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("AA") => Ok(Level::Aa),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("AAA") => Ok(Level::Aaa),
        Some(v) => Err(bad(CMD, format!("`level` is \"AA\" or \"AAA\", not {v}"))),
    }
}

/// `ids` as shape ids, `None` when absent. An entry that is not a shape id is an error, so a
/// mistyped id does not leave its shape unmeasured without a word.
fn ids_param(p: &Value) -> Result<Option<Vec<ShapeId>>> {
    let list = match p.get("ids") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) if a.len() <= MAX_IDS => a,
        Some(Value::Array(_)) => return Err(bad(CMD, format!("`ids` holds at most {MAX_IDS} shape ids"))),
        Some(v) => return Err(bad(CMD, format!("`ids` is an array of shape ids, not {v}"))),
    };
    list.iter()
        .map(|v| {
            v.as_u64().and_then(|n| u32::try_from(n).ok()).map(ShapeId).ok_or_else(|| bad(CMD, format!("`ids` holds {v}, which is not a shape id")))
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// Pixels per point for the drawings: `scale` within 0.25–4, lowered so that a drawing stays
/// within MAX_PIXELS and the renderer's largest side.
fn measure_scale(doc: &Presentation, scale: f64) -> f64 {
    let scale = if scale.is_finite() { scale.clamp(0.25, 4.0) } else { 1.0 };
    let (w, h) = (doc.slide_size.width.max(1.0), doc.slide_size.height.max(1.0));
    let cap = (MAX_PIXELS / (w * h)).sqrt().min(f64::from(deckcraft_render::MAX_SIDE) / w.max(h));
    if cap.is_finite() { scale.min(cap) } else { scale }
}

/// Whether placed text `t` is measured: text of the slide's own shapes, not a table (its cells
/// are drawn even when the table's text is left out), picked by `ids` or inside a group `ids`
/// picks.
fn wanted(slide: &Slide, placed: &Placed, t: &PlacedText, ids: Option<&[ShapeId]>) -> bool {
    if !t.own || deckcraft_model::find_shape(&slide.shapes, t.shape).is_none_or(|s| matches!(s.kind, ShapeKind::Table(_))) {
        return false;
    }
    let Some(ids) = ids else { return true };
    // The shape, then the groups it sits in, innermost first.
    let mut at = placed.shapes.iter().position(|ps| ps.id == t.shape);
    for _ in 0..=64 {
        let Some(ps) = at.and_then(|k| placed.shapes.get(k)) else { return false };
        if ids.contains(&ps.id) {
            return true;
        }
        at = ps.parent;
    }
    false
}

/// Measures the runs of placed text `t` and appends one entry per model run to `out`.
fn measure_text(sc: &Scene, t: &PlacedText, out: &mut Vec<Value>) {
    let Some(shape) = deckcraft_model::find_shape(&sc.slide.shapes, t.shape) else { return };
    // Warped text (Text Effects ▸ Transform) is drawn bent along paths, away from the boxes the
    // layout gives its glyphs, as the renderer draws it when the text is not turned.
    if t.layout.rotation == 0.0 && deckcraft_render::text_warp(sc.ctx, shape, Rect::new(0.0, 0.0, 100.0, 100.0)).is_some() {
        return;
    }
    let runs = collect_runs(t, sc.scale);
    if runs.is_empty() {
        return;
    }
    let opts = RenderOpts { scale: sc.scale, skip_text: Some(t.shape), ..Default::default() };
    let img = deckcraft_render::render_slide(sc.doc, sc.index, &opts);
    for ((para, run), m) in runs {
        let required = required_ratio(sc.level, m.size, m.bold);
        let (Some(r), Some(bounds)) = (ratios(&sample_boxes(&img, &m.boxes), m.color, m.opacity, required), m.bounds) else { continue };
        let text = t.body.paragraphs.get(para).and_then(|p| p.runs.get(run)).map(|r| r.text.as_str()).unwrap_or_default();
        out.push(json!({
            "slide": sc.index,
            "shape": t.shape,
            "para": para,
            "run": run,
            "text": text,
            "color": format!("#{}", m.color.hex()),
            "sizePt": m.size,
            "bold": m.bold,
            "required": required,
            "ratio": {"min": round2(r.min), "p5": round2(r.p5), "median": round2(r.median)},
            "passShare": (r.pass * 1000.0).round() / 1000.0,
            "backdrop": backdrop(sc, shape, bounds),
        }));
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The glyph boxes of placed text `t`, by (paragraph, model run). Bullets, glyphs from colour
/// fonts and text drawn with no fill are left out.
fn collect_runs(t: &PlacedText, scale: f64) -> BTreeMap<(usize, usize), RunMeasure> {
    let mut out: BTreeMap<(usize, usize), RunMeasure> = BTreeMap::new();
    // The drawn size: text space is points unless the transform scales it.
    let k = t.transform.determinant().abs().sqrt();
    if !t.transform.is_finite() || !k.is_finite() || k < 1e-9 {
        return out;
    }
    let mut starts: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut budget = MAX_GLYPHS;
    for g in &t.layout.runs {
        let Some(&(first, _)) = g.cells.first() else { continue };
        let opacity = f64::from(g.color.a) / 255.0 * g.alpha;
        if !opacity.is_finite() || opacity <= 0.0 || g.face.is_color() {
            continue;
        }
        let para_starts = starts.entry(g.para).or_insert_with(|| run_starts(&t.body, &t.layout, g.para));
        let Some(run) = para_starts.iter().rposition(|&s| s <= first) else { continue };
        let size = round2(g.size * k);
        // Bold from weight 700 (CSS's `bold`) or synthetic bold; a SemiBold face (600) is not bold.
        let bold = g.fake_bold || g.face.weight >= 700.0;
        let m = out.entry((g.para, run)).or_insert_with(|| RunMeasure {
            color: g.color,
            opacity: opacity.min(1.0),
            size,
            bold,
            boxes: vec![],
            bounds: None,
        });
        m.size = m.size.min(size);
        m.bold &= bold;
        glyph_boxes(g, t.transform, scale, &mut budget, m);
    }
    out.retain(|_, m| !m.boxes.is_empty());
    out
}

/// Where each model run of paragraph `para` starts in the paragraph's drawn text. A field draws
/// its current value (a slide number, a date) in place of its stored text: when the paragraph
/// holds one field, the field's drawn length is what the layout's character count leaves for it.
fn run_starts(body: &TextBody, layout: &TextLayout, para: usize) -> Vec<usize> {
    let Some(p) = body.paragraphs.get(para) else { return vec![] };
    let mut lens: Vec<usize> = p.runs.iter().map(Run::char_len).collect();
    let stored = lens.iter().fold(0usize, |a, n| a.saturating_add(*n));
    let fields: Vec<usize> = p.runs.iter().enumerate().filter(|(_, r)| matches!(r.kind, RunKind::Field { .. })).map(|(i, _)| i).collect();
    if let (Some(drawn), [f]) = (layout.boundaries.get(para).map(|b| b.len().saturating_sub(1)), fields.as_slice())
        && let Some(len) = lens.get_mut(*f)
    {
        *len = len.saturating_add(drawn).saturating_sub(stored);
    }
    let mut at = 0usize;
    lens.iter()
        .map(|n| {
            let start = at;
            at = at.saturating_add(*n);
            start
        })
        .collect()
}

/// Adds the glyph boxes of glyph run `g` to `m`: each glyph's advance across and the face's
/// ascent to its descent down, turned with the glyph when it stands upright in vertical text.
/// Spaces and control characters have no box.
fn glyph_boxes(g: &GlyphRun, to_slide: Affine, scale: f64, budget: &mut usize, m: &mut RunMeasure) {
    let k = g.size / g.face.upem.max(1.0);
    let (ascent, descent) = (g.face.ascent * k, g.face.descent * k);
    let to_px = Affine::scale(scale) * to_slide;
    for (&(gid, x, y), &(_, ch)) in g.glyphs.iter().zip(&g.cells) {
        if ch.is_whitespace() || ch.is_control() {
            continue;
        }
        let Some(left) = budget.checked_sub(1) else { return };
        *budget = left;
        let advance = g.face.advance(gid) * k;
        let rect = Rect::new(x, y - ascent, x + advance, y + descent).abs();
        if !rect.is_finite() || rect.area() <= 0.0 {
            continue;
        }
        let turn = match g.upright_centre(gid, x, y) {
            Some((cx, cy)) => Affine::rotate_about(-std::f64::consts::FRAC_PI_2, Point::new(cx, cy)),
            None => Affine::IDENTITY,
        };
        let on_slide = (to_slide * turn).transform_rect_bbox(rect);
        m.bounds = Some(m.bounds.map_or(on_slide, |b| b.union(on_slide)));
        m.boxes.push((rect, to_px * turn));
    }
}

/// The pixels of `img` under glyph boxes `boxes`, read on a grid spaced so that about
/// MAX_SAMPLES pixels are read, plus a box's centre pixel when the grid misses the box.
fn sample_boxes(img: &Image, boxes: &[(Rect, Affine)]) -> Vec<[u8; 4]> {
    let area: f64 = boxes.iter().map(|(r, m)| r.area() * m.determinant().abs()).filter(|a| a.is_finite()).sum();
    let step = (area / MAX_SAMPLES).sqrt().ceil();
    let step = if step.is_finite() { step.max(1.0) } else { 1.0 };
    let mut out = vec![];
    for (r, m) in boxes {
        if out.len() >= MAX_KEPT {
            break;
        }
        let before = out.len();
        sample_box(img, *r, *m, step, &mut out);
        if out.len() == before
            && let Some(px) = pixel_at(img, *m * r.center())
        {
            out.push(px);
        }
    }
    out
}

/// Appends the pixels of `img` whose centres lie in box `r` mapped by `m`, every `step` pixels
/// across and down.
fn sample_box(img: &Image, r: Rect, m: Affine, step: f64, out: &mut Vec<[u8; 4]>) {
    if !m.is_finite() || m.determinant().abs() < 1e-12 {
        return;
    }
    let inv = m.inverse();
    let clip = Rect::new(0.0, 0.0, f64::from(img.width), f64::from(img.height));
    let bb = m.transform_rect_bbox(r).intersect(clip);
    if !bb.is_finite() || bb.area() <= 0.0 {
        return;
    }
    // Grid points sit at multiples of `step`, so neighbouring boxes share one grid.
    let mut y = (bb.y0 / step).floor() * step;
    while y < bb.y1 && out.len() < MAX_KEPT {
        let mut x = (bb.x0 / step).floor() * step;
        while x < bb.x1 {
            let c = Point::new(x + 0.5, y + 0.5);
            if r.contains(inv * c)
                && let Some(px) = pixel_at(img, c)
            {
                out.push(px);
            }
            x += step;
        }
        y += step;
    }
}

fn pixel_at(img: &Image, p: Point) -> Option<[u8; 4]> {
    if p.x.is_finite() && p.y.is_finite() && p.x >= 0.0 && p.y >= 0.0 { img.straight_at(p.x as u32, p.y as u32) } else { None }
}

/// The contrast of `color` at `opacity`, drawn over each pixel, against that pixel: smallest,
/// 5th percentile, median and the share at or above `required`.
fn ratios(pixels: &[[u8; 4]], color: Rgba, opacity: f64, required: f64) -> Option<Ratios> {
    let mut v: Vec<f64> = pixels
        .iter()
        .map(|&[r, g, b, _]| {
            let under = Rgba::rgb(r, g, b);
            over(color, opacity, under).contrast(under)
        })
        .filter(|c| c.is_finite())
        .collect();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let at = |f: f64| v.get(((n as f64 * f) as usize).min(n.saturating_sub(1))).copied();
    let pass = v.iter().filter(|c| **c >= required).count() as f64 / n.max(1) as f64;
    Some(Ratios { min: v.first().copied()?, p5: at(0.05)?, median: at(0.5)?, pass })
}

/// `color` at `opacity` drawn over the opaque colour `under`.
fn over(color: Rgba, opacity: f64, under: Rgba) -> Rgba {
    let mix = |c: u8, u: u8| (f64::from(c) * opacity + f64::from(u) * (1.0 - opacity)) / 255.0;
    Rgba::from_f64(mix(color.r, under.r), mix(color.g, under.g), mix(color.b, under.b), 1.0)
}

/// The ratio WCAG 2.2 asks for: large text, 18 pt or more, or 14 pt or more and bold, needs 3 at
/// AA and 4.5 at AAA; other text 4.5 at AA and 7 at AAA.
fn required_ratio(level: Level, size: f64, bold: bool) -> f64 {
    let large = size >= 18.0 || (bold && size >= 14.0);
    match (level, large) {
        (Level::Aa, true) => 3.0,
        (Level::Aa, false) | (Level::Aaa, true) => 4.5,
        (Level::Aaa, false) => 7.0,
    }
}

/// What the slide draws behind the text of `shape` over `bounds` (slide points), front to back:
/// the shape's own fill, the slide's shapes drawn before it, the layout's and master's graphics,
/// then the slide background. The search stops at the first opaque one whose box holds
/// `bounds`; more than one on the way gives `mixed`.
fn backdrop(sc: &Scene, shape: &Shape, bounds: Rect) -> &'static str {
    let shapes = &sc.placed.shapes;
    let me = shapes.iter().position(|ps| ps.id == shape.id);
    let own = me.and_then(|k| shapes.get(k)).map(|ps| (shape, ps.rect));
    let earlier = shapes
        .get(..me.unwrap_or(0))
        .unwrap_or_default()
        .iter()
        .rev()
        .filter(|ps| !ps.is_group && ps.rect.intersect(bounds).area() > 0.0)
        .filter_map(|ps| Some((deckcraft_model::find_shape(&sc.slide.shapes, ps.id)?, ps.rect)));
    let mut found: Vec<&'static str> = vec![];
    for (s, rect) in own.into_iter().chain(earlier) {
        let Some((kind, opaque)) = fill_kind(sc.ctx, sc.slide, s) else { continue };
        found.push(kind);
        if opaque && rect.contains_rect(bounds) {
            return verdict(&found);
        }
    }
    for b in sc.graphics.iter().filter(|b| b.rect.intersect(bounds).area() > 0.0) {
        found.push(b.kind);
        if b.opaque && b.rect.contains_rect(bounds) {
            return verdict(&found);
        }
    }
    found.push(background_kind(sc.ctx, sc.slide));
    verdict(&found)
}

fn verdict(found: &[&'static str]) -> &'static str {
    match found {
        &[one] => one,
        _ => "mixed",
    }
}

/// The layout's and master's own graphics the slide shows (placeholders are not drawn on a
/// slide), front to back. Only top-level shapes count: a group's members are left out.
fn background_graphics(doc: &Presentation, slide: &Slide, ctx: &Ctx) -> Vec<Behind> {
    let (show_layout, show_master) = resolve::show_master_shapes(slide, ctx.layout);
    let mut out = vec![];
    if show_layout && let Some(layout) = ctx.layout {
        push_graphics(&Ctx::for_layout(doc, ctx.master, layout), slide, &layout.shapes, &mut out);
    }
    if show_master {
        push_graphics(&Ctx::for_master(doc, ctx.master), slide, &ctx.master.shapes, &mut out);
    }
    out
}

fn push_graphics(ctx: &Ctx, slide: &Slide, shapes: &[Shape], out: &mut Vec<Behind>) {
    for s in shapes.iter().rev().filter(|s| s.ph.is_none() && !s.hidden) {
        let x = resolve::xfrm(ctx, s);
        if !x.is_finite() {
            continue;
        }
        if let Some((kind, opaque)) = fill_kind(ctx, slide, s) {
            out.push(Behind { rect: x.affine().transform_rect_bbox(Rect::new(0.0, 0.0, x.w, x.h)), kind, opaque });
        }
    }
}

/// What shape `s` paints behind text, and whether that paint hides what lies under it, as the
/// renderer fills each kind. `None` for shapes that paint no area: groups, lines, ink and shapes
/// with no fill.
fn fill_kind(ctx: &Ctx, slide: &Slide, s: &Shape) -> Option<(&'static str, bool)> {
    match &s.kind {
        ShapeKind::Picture { fill } => Some(("picture", fill.alpha.is_none_or(|a| a >= 1.0))),
        ShapeKind::Opaque { preview: Some(_), .. } => Some(("picture", true)),
        // A poster frame, or a dark box for a video without one; a sound shows only a small icon.
        ShapeKind::Media(mc) => Some(("picture", mc.poster.is_some() || mc.video)),
        ShapeKind::Table(_) | ShapeKind::Chart(_) => Some(("shapeFill", false)),
        ShapeKind::Group { .. } | ShapeKind::Connector { .. } | ShapeKind::Ink { .. } => None,
        ShapeKind::Shape if s.is_line() => None,
        ShapeKind::Shape | ShapeKind::Model3d { .. } | ShapeKind::Opaque { preview: None, .. } => shape_fill_kind(ctx, slide, s),
    }
}

/// What the resolved fill of shape `s` paints, and whether it is opaque everywhere.
fn shape_fill_kind(ctx: &Ctx, slide: &Slide, s: &Shape) -> Option<(&'static str, bool)> {
    let (fill, ph) = resolve::fill(ctx, s);
    let solid = |c: &ColorRef| ctx.color(c, ph).a == 255;
    match fill? {
        Fill::None => None,
        Fill::Solid { color } => Some(("shapeFill", solid(&color))),
        Fill::Gradient(g) => Some(("shapeFill", !g.stops.is_empty() && g.stops.iter().all(|st| solid(&st.color)))),
        Fill::Pattern(pf) => Some(("shapeFill", solid(&pf.fg) && solid(&pf.bg))),
        Fill::Picture(pf) => Some(("picture", pf.alpha.is_none_or(|a| a >= 1.0))),
        Fill::Background => Some((background_kind(ctx, slide), true)),
        Fill::Group => Some(("shapeFill", false)),
    }
}

/// The kind of the slide's background fill.
fn background_kind(ctx: &Ctx, slide: &Slide) -> &'static str {
    match resolve::background(ctx, Some(slide)).0 {
        Fill::Gradient(_) => "gradient",
        Fill::Picture(_) => "picture",
        Fill::Pattern(_) => "pattern",
        Fill::None | Fill::Solid { .. } | Fill::Group | Fill::Background => "solid",
    }
}
