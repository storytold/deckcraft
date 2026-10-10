//! Fills: solid, gradient, picture and pattern paints.

use deckcraft_color::Rgba;
use deckcraft_geom::preset::FillMode;
use deckcraft_model::resolve::Ctx;
use deckcraft_model::style::{Fill, GradientShape, PictureFill, PictureMode};
use deckcraft_model::{PhType, Presentation};
use kurbo::{Affine, BezPath, Point, Rect, Shape as _};
use vello_cpu::{RenderContext, peniko};

use crate::{color, images, shade};

#[allow(clippy::too_many_arguments)]
pub fn fill_path(
    ctx: &mut RenderContext,
    rctx: &Ctx,
    fill: &Fill,
    ph: Option<Rgba>,
    path: &BezPath,
    bounds: Rect,
    m: Affine,
    mode: FillMode,
    alpha: f64,
    pres: &Presentation,
) {
    ctx.set_transform(m);
    match fill {
        Fill::None | Fill::Group | Fill::Background => {}
        Fill::Solid { color: c } => {
            let col = shade(rctx.color(c, ph), mode);
            ctx.set_paint(color(col, alpha));
            ctx.fill_path(path);
        }
        Fill::Gradient(g) => {
            if g.stops.is_empty() {
                return;
            }
            let mut stops: Vec<(f32, peniko::Color)> =
                g.stops.iter().map(|s| (s.pos.clamp(0.0, 1.0) as f32, color(shade(rctx.color(&s.color, ph), mode), alpha))).collect();
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            let stops: Vec<peniko::ColorStop> = stops.into_iter().map(|(o, c)| peniko::ColorStop { offset: o, color: c.into() }).collect();
            let grad = match &g.shape {
                GradientShape::Linear { angle, .. } => {
                    // Angle measured clockwise from left→right; span the bounds along that direction.
                    let a = angle.to_radians();
                    let (dx, dy) = (a.cos(), a.sin());
                    let c = bounds.center();
                    let half = (bounds.width() * dx.abs() + bounds.height() * dy.abs()) / 2.0;
                    peniko::Gradient::new_linear(Point::new(c.x - dx * half, c.y - dy * half), Point::new(c.x + dx * half, c.y + dy * half))
                        .with_stops(stops.as_slice())
                }
                GradientShape::Path { focus, .. } => {
                    let fx = bounds.x0 + bounds.width() * (focus[0] + (1.0 - focus[2])) / 2.0;
                    let fy = bounds.y0 + bounds.height() * (focus[1] + (1.0 - focus[3])) / 2.0;
                    // Reach the farthest corner, so a gradient from a corner covers the whole box.
                    let r = [(bounds.x0, bounds.y0), (bounds.x1, bounds.y0), (bounds.x0, bounds.y1), (bounds.x1, bounds.y1)]
                        .iter()
                        .map(|(x, y)| ((x - fx).powi(2) + (y - fy).powi(2)).sqrt())
                        .fold(1e-6, f64::max);
                    peniko::Gradient::new_radial(Point::new(fx, fy), r as f32).with_stops(stops.as_slice())
                }
            };
            ctx.set_paint(grad);
            ctx.fill_path(path);
        }
        Fill::Picture(pf) => {
            ctx.push_clip_layer(path);
            picture(ctx, pres, pf, bounds, m, None);
            ctx.pop_layer();
        }
        Fill::Pattern(p) => {
            let fg = rctx.color(&p.fg, ph);
            let bg = rctx.color(&p.bg, ph);
            ctx.set_paint(color(bg, alpha));
            ctx.fill_path(path);
            ctx.push_clip_layer(path);
            ctx.set_paint(color(fg, alpha));
            pattern(ctx, &p.preset, bounds, m);
            ctx.pop_layer();
        }
    }
}

/// Draw a pattern's foreground marks over `bounds` (8 pt cells).
fn pattern(ctx: &mut RenderContext, preset: &str, b: Rect, m: Affine) {
    let k = crate::scale_of(m);
    let cell = 6.0 / if k.is_finite() { k.clamp(0.25, 4.0) } else { 1.0 };
    let cell = cell.max(2.0);
    let mut p = BezPath::new();
    let (nx, ny) = (((b.width() / cell).ceil() as i64).clamp(0, 2000), ((b.height() / cell).ceil() as i64).clamp(0, 2000));
    let lw = cell * 0.18;
    let dense = preset.starts_with("dk") || preset.contains("50") || preset.contains("75") || preset.contains("90");
    for j in 0..=ny {
        for i in 0..=nx {
            let x = b.x0 + i as f64 * cell;
            let y = b.y0 + j as f64 * cell;
            let r = match preset {
                p if p.contains("Horz") || p == "horz" || p.contains("horzBrick") => Rect::new(x, y, x + cell, y + lw),
                p if p.contains("Vert") || p == "vert" => Rect::new(x, y, x + lw, y + cell),
                p if p.contains("cross") || p.contains("Grid") || p.contains("grid") => {
                    p_add(&mut p_rect(x, y, cell, lw), &mut BezPath::new());
                    Rect::new(x, y, x + cell, y + lw).union(Rect::new(x, y, x + lw, y + cell))
                }
                _ => {
                    let d = if dense { cell * 0.35 } else { cell * 0.2 };
                    Rect::new(x, y, x + d, y + d)
                }
            };
            p.extend(r.to_path(0.1).iter());
            if preset.contains("cross") || preset.contains("Grid") || preset.contains("grid") {
                p.extend(Rect::new(x, y, x + lw, y + cell).to_path(0.1).iter());
            }
        }
    }
    ctx.fill_path(&p);
}

fn p_rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    Rect::new(x, y, x + w, y + h).to_path(0.1)
}
fn p_add(_a: &mut BezPath, _b: &mut BezPath) {}

/// Draw a picture fill into `bounds` (shape-local) honouring crop and stretch/tile modes; an animated
/// GIF shows its frame at `gif_time` seconds.
pub fn picture(ctx: &mut RenderContext, pres: &Presentation, pf: &PictureFill, bounds: Rect, m: Affine, gif_time: Option<f64>) {
    let Some(item) = pres.media(pf.media) else {
        missing(ctx, bounds);
        return;
    };
    let px_scale = crate::scale_of(m);
    let want = (bounds.width().max(bounds.height()) * px_scale).max(1.0);
    let frame = gif_time.and_then(|t| images::gif_pixmap(&item.data, &pf.adjust, t));
    let Some(pm) = frame.or_else(|| images::decode_for(&item.data, want, &pf.adjust)) else {
        missing(ctx, bounds);
        return;
    };
    let (iw, ih) = (pm.width() as f64, pm.height() as f64);
    if iw < 1.0 || ih < 1.0 {
        return;
    }
    let dest = match &pf.mode {
        PictureMode::Stretch { fill_rect } => {
            let [l, t, r, b] = *fill_rect;
            let inner = Rect::new(
                bounds.x0 + l * bounds.width(),
                bounds.y0 + t * bounds.height(),
                bounds.x1 - r * bounds.width(),
                bounds.y1 - b * bounds.height(),
            );
            crate::crop_dest(inner, pf.crop)
        }
        PictureMode::Tile { .. } => bounds,
    };
    let alpha = pf.alpha.unwrap_or(1.0).clamp(0.0, 1.0);
    let image = vello_cpu::Image {
        image: vello_cpu::ImageSource::Pixmap(pm.clone()),
        sampler: peniko::ImageSampler { quality: peniko::ImageQuality::High, ..Default::default() },
    };
    if let PictureMode::Tile { sx, sy, tx, ty, .. } = &pf.mode {
        let (sx, sy) = (if *sx > 0.0 { *sx } else { 1.0 }, if *sy > 0.0 { *sy } else { 1.0 });
        let tile_w = iw * 0.75 * sx;
        let tile_h = ih * 0.75 * sy;
        let nx = ((bounds.width() / tile_w.max(1.0)).ceil() as i64).clamp(0, 200);
        let ny = ((bounds.height() / tile_h.max(1.0)).ceil() as i64).clamp(0, 200);
        for j in -1..=ny {
            for i in -1..=nx {
                let r = Rect::new(bounds.x0 + tx + i as f64 * tile_w, bounds.y0 + ty + j as f64 * tile_h, 0.0, 0.0);
                let r = Rect::new(r.x0, r.y0, r.x0 + tile_w, r.y0 + tile_h);
                draw_image(ctx, &image, iw, ih, r, m, alpha);
            }
        }
        return;
    }
    draw_image(ctx, &image, iw, ih, dest, m, alpha);
}

fn draw_image(ctx: &mut RenderContext, image: &vello_cpu::Image, iw: f64, ih: f64, dest: Rect, m: Affine, alpha: f64) {
    let t = m * Affine::translate(dest.origin().to_vec2()) * Affine::scale_non_uniform(dest.width() / iw, dest.height() / ih);
    ctx.set_transform(t);
    if alpha < 0.999 {
        ctx.push_layer(None, None, Some(alpha as f32), None, None);
    }
    ctx.set_paint(image.clone());
    ctx.fill_rect(&Rect::new(0.0, 0.0, iw, ih));
    if alpha < 0.999 {
        ctx.pop_layer();
    }
}

fn missing(ctx: &mut RenderContext, b: Rect) {
    ctx.set_paint(peniko::Color::from_rgba8(230, 230, 230, 255));
    ctx.fill_rect(&b);
    ctx.set_paint(peniko::Color::from_rgba8(160, 160, 160, 255));
    ctx.set_stroke(kurbo::Stroke::new(1.0));
    let mut p = BezPath::new();
    p.move_to((b.x0, b.y0));
    p.line_to((b.x1, b.y1));
    p.move_to((b.x1, b.y0));
    p.line_to((b.x0, b.y1));
    ctx.stroke_path(&p);
}

/// The speaker / film glyph on audio and video objects without a poster.
pub fn media_glyph(ctx: &mut RenderContext, video: bool, b: Rect) {
    let s = b.width().min(b.height());
    let c = b.center();
    if video {
        let r = s * 0.18;
        ctx.set_paint(peniko::Color::from_rgba8(255, 255, 255, 220));
        ctx.fill_path(&kurbo::Circle::new(c, r * 1.4).to_path(0.1));
        let mut tri = BezPath::new();
        tri.move_to((c.x - r * 0.45, c.y - r * 0.7));
        tri.line_to((c.x + r * 0.75, c.y));
        tri.line_to((c.x - r * 0.45, c.y + r * 0.7));
        tri.close_path();
        ctx.set_paint(peniko::Color::from_rgba8(30, 30, 30, 255));
        ctx.fill_path(&tri);
        return;
    }
    let k = s / 48.0;
    let mut p = BezPath::new();
    p.move_to((c.x - 14.0 * k, c.y - 6.0 * k));
    p.line_to((c.x - 6.0 * k, c.y - 6.0 * k));
    p.line_to((c.x + 4.0 * k, c.y - 15.0 * k));
    p.line_to((c.x + 4.0 * k, c.y + 15.0 * k));
    p.line_to((c.x - 6.0 * k, c.y + 6.0 * k));
    p.line_to((c.x - 14.0 * k, c.y + 6.0 * k));
    p.close_path();
    ctx.set_paint(peniko::Color::from_rgba8(0x2E, 0x6F, 0xD8, 255));
    ctx.fill_path(&p);
    let mut w = BezPath::new();
    for r in [9.0, 15.0] {
        let arc = kurbo::Arc::new((c.x + 4.0 * k, c.y), (r * k, r * k), -0.8, 1.6, 0.0);
        w.extend(arc.path_elements(0.1));
    }
    ctx.set_stroke(kurbo::Stroke::new(2.5 * k).with_caps(kurbo::Cap::Round));
    ctx.stroke_path(&w);
}

/// A content-type icon drawn in empty picture/chart/table placeholders.
pub fn placeholder_icon(ctx: &mut RenderContext, kind: PhType, b: Rect) {
    let s = (b.width().min(b.height()) * 0.18).clamp(10.0, 48.0);
    let c = Point::new(b.center().x, b.center().y - s * 0.3);
    let r = Rect::new(c.x - s / 2.0, c.y - s / 2.0, c.x + s / 2.0, c.y + s / 2.0);
    ctx.set_paint(peniko::Color::from_rgba8(0x8C, 0x8C, 0x8C, 255));
    ctx.set_stroke(kurbo::Stroke::new(s * 0.06).with_join(kurbo::Join::Round));
    match kind {
        PhType::Chart => {
            for (i, h) in [0.45, 0.8, 0.6].iter().enumerate() {
                let x = r.x0 + s * (0.1 + 0.3 * i as f64);
                ctx.fill_rect(&Rect::new(x, r.y1 - s * h, x + s * 0.2, r.y1));
            }
        }
        PhType::Table => {
            ctx.stroke_path(&r.to_path(0.1));
            let mut p = BezPath::new();
            for k in 1..3 {
                let x = r.x0 + s * k as f64 / 3.0;
                let y = r.y0 + s * k as f64 / 3.0;
                p.move_to((x, r.y0));
                p.line_to((x, r.y1));
                p.move_to((r.x0, y));
                p.line_to((r.x1, y));
            }
            ctx.stroke_path(&p);
        }
        _ => {
            ctx.stroke_path(&r.to_path(0.1));
            let mut m = BezPath::new();
            m.move_to((r.x0, r.y1));
            m.line_to((r.x0 + s * 0.35, r.y0 + s * 0.45));
            m.line_to((r.x0 + s * 0.6, r.y0 + s * 0.7));
            m.line_to((r.x0 + s * 0.75, r.y0 + s * 0.55));
            m.line_to((r.x1, r.y1));
            m.close_path();
            ctx.fill_path(&m);
            ctx.fill_path(&kurbo::Circle::new((r.x0 + s * 0.72, r.y0 + s * 0.25), s * 0.1).to_path(0.1));
        }
    }
}
