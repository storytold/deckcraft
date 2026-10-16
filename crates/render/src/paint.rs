//! Fills: solid, gradient, picture and pattern paints.

use deckcraft_color::Rgba;
use deckcraft_geom::preset::FillMode;
use deckcraft_model::resolve::Ctx;
use deckcraft_model::style::{ColorRef, Fill, GradientShape, PictureAdjust, PictureFill, PictureMode};
use deckcraft_model::{PhType, Presentation};
use kurbo::{Affine, BezPath, Point, Rect, Shape as _};
use vello_cpu::peniko;

use crate::canvas::{self, Canvas, CanvasImage, Paint};
use crate::{images, shade};

#[allow(clippy::too_many_arguments)]
pub fn fill_path(
    ctx: &mut dyn Canvas,
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
            ctx.set_paint(fade(col, alpha));
            ctx.fill_path(path);
        }
        Fill::Gradient(g) => {
            if g.stops.is_empty() {
                return;
            }
            // `rotWithShape="0"`: the gradient keeps to the slide's axes when the shape is
            // rotated or flipped, laid out over the box the shape covers on the slide.
            let c = m.as_coeffs();
            let turned = c[1].abs() > 1e-9 || c[2].abs() > 1e-9 || c[0] < 0.0 || c[3] < 0.0;
            let path_kind = matches!(&g.shape, GradientShape::Path { path: k, .. } if matches!(k.as_str(), "rect" | "shape"));
            if !g.rotate_with_shape && turned && !path_kind && m.determinant().abs() > 1e-12 {
                let on_slide = m.transform_rect_bbox(bounds);
                let upright = deckcraft_model::style::Gradient { rotate_with_shape: true, ..g.clone() };
                let paint_path = m * path.clone();
                fill_path(ctx, rctx, &Fill::Gradient(upright), ph, &paint_path, on_slide, Affine::IDENTITY, mode, alpha, pres);
                return;
            }
            // A linear gradient spans its tile rectangle (the shape's box unless `tileRect` moves
            // its edges); a radial one ends on the circle around it, from a focus placed in the
            // shape's own box (PowerPoint's "from corner" presets centre the tile on the corner).
            let tile = tile_box(bounds, g.tile);
            let mut stops: Vec<(f64, Rgba)> = g.stops.iter().map(|s| (s.pos.clamp(0.0, 1.0), shade(rctx.color(&s.color, ph), mode))).collect();
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            // Rectangle and outline paths: rasterised (see `pathgrad`), on vector canvases too.
            if let GradientShape::Path { path: kind, focus } = &g.shape
                && matches!(kind.as_str(), "rect" | "shape")
            {
                let ramp = crate::ramp::expand(&stops);
                let area = m.transform_rect_bbox(path.bounding_box());
                canvas::pixels(ctx, area, &mut |v, pre| crate::pathgrad::fill(v, kind, *focus, &ramp, bounds, path, pre * m, alpha));
                return;
            }
            ctx.set_paint(gradient_in(&g.shape, stops, bounds, tile, alpha));
            ctx.fill_path(path);
        }
        Fill::Picture(pf) => {
            // `rotWithShape="0"`: upright on the slide, stretched over the box the turned shape
            // covers, clipped to it.
            let c = m.as_coeffs();
            let turned = c[1].abs() > 1e-9 || c[2].abs() > 1e-9 || c[0] < 0.0 || c[3] < 0.0;
            if pf.upright && turned && m.determinant().abs() > 1e-12 {
                let on_slide = m.transform_rect_bbox(bounds);
                let on_path = m * path.clone();
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_clip_layer(&on_path);
                picture(ctx, pres, &resolved(rctx, pf, ph), on_slide, Affine::IDENTITY, None);
                ctx.pop_layer();
                return;
            }
            ctx.push_clip_layer(path);
            picture(ctx, pres, &resolved(rctx, pf, ph), bounds, m, None);
            ctx.pop_layer();
        }
        Fill::Pattern(p) => {
            let fg = rctx.color(&p.fg, ph);
            let bg = rctx.color(&p.bg, ph);
            // Pixels (an 8 x 8 tile); vector canvases get this path's area rasterized.
            let area = m.transform_rect_bbox(path.bounding_box());
            canvas::pixels(ctx, area, &mut |v, pre| crate::pattern::fill(v, &p.preset, fg, bg, alpha, path, pre * m));
        }
    }
}

/// `pf` with its duotone colours resolved to plain RGB against the slide's colours and the
/// placeholder colour `ph` (theme background pictures recolour with `phClr`), so the decoded-image
/// cache can apply and key them without a colour scheme.
pub fn resolved<'a>(rctx: &Ctx, pf: &'a PictureFill, ph: Option<Rgba>) -> std::borrow::Cow<'a, PictureFill> {
    let Some((d, l)) = &pf.adjust.duotone else { return std::borrow::Cow::Borrowed(pf) };
    let mut out = pf.clone();
    out.adjust.duotone = Some((ColorRef::rgb(rctx.color(d, ph)), ColorRef::rgb(rctx.color(l, ph))));
    std::borrow::Cow::Owned(out)
}

/// `bounds` with its edges moved in by `tile` (left, top, right, bottom insets as fractions of its
/// size; negative moves them out), as `a:tileRect` and `a:fillRect` lay out a fill.
pub fn tile_box(bounds: Rect, tile: [f64; 4]) -> Rect {
    if tile.iter().all(|v| *v == 0.0) || tile.iter().any(|v| !v.is_finite()) {
        return bounds;
    }
    let (w, h) = (bounds.width(), bounds.height());
    let [l, t, r, b] = tile;
    let x0 = bounds.x0 + l * w;
    let x1 = bounds.x1 - r * w;
    let y0 = bounds.y0 + t * h;
    let y1 = bounds.y1 - b * h;
    // An inverted tile (insets past each other) lays out nothing sensible: keep the box.
    if x1 - x0 <= 1e-9 || y1 - y0 <= 1e-9 { bounds } else { Rect::new(x0, y0, x1, y1) }
}

/// `c` with its alpha multiplied by `alpha`.
pub(crate) fn fade(c: Rgba, alpha: f64) -> Rgba {
    c.with_alpha((c.a as f64 * alpha.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8)
}

/// The paint of a gradient of `shape` with `stops` (position, colour) spanning `bounds`, faded
/// by `alpha`, ramping between stops as PowerPoint does (see `ramp`).
pub fn gradient(shape: &GradientShape, stops: Vec<(f64, Rgba)>, bounds: Rect, alpha: f64) -> Paint {
    gradient_in(shape, stops, bounds, bounds, alpha)
}

/// [`gradient`] laid out over the tile rectangle `tile` (`a:tileRect`; `bounds` when there is
/// none): a linear gradient spans the tile, a circle one ends on the circle around it, from a
/// focus placed in `bounds`.
pub fn gradient_in(shape: &GradientShape, mut stops: Vec<(f64, Rgba)>, bounds: Rect, tile: Rect, alpha: f64) -> Paint {
    for s in &mut stops {
        s.0 = s.0.clamp(0.0, 1.0);
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let stops: Vec<(f32, Rgba)> = crate::ramp::expand(&stops).into_iter().map(|(o, c)| (o as f32, fade(c, alpha))).collect();
    match shape {
        GradientShape::Linear { angle, .. } => {
            // Angle measured clockwise from left→right; span the tile along that direction.
            let a = angle.to_radians();
            let (dx, dy) = (a.cos(), a.sin());
            let c = tile.center();
            let half = (tile.width() * dx.abs() + tile.height() * dy.abs()) / 2.0;
            Paint::Linear { start: Point::new(c.x - dx * half, c.y - dy * half), end: Point::new(c.x + dx * half, c.y + dy * half), stops }
        }
        GradientShape::Path { focus, .. } => circle_gradient(*focus, bounds, tile, stops),
    }
}

/// A `circle` path gradient over `bounds` with focus rectangle `focus` (`fillToRect` insets, as
/// fractions). As PowerPoint draws it (fitted to its exports): the first stop is a point at the
/// focus (the middle of the focus rectangle) and the last is the circle around the whole box (its
/// centre, half its diagonal), with circles in between moving from one to the other. A focus off
/// the centre so bunches the colours on its near side; a gradient "from a corner" reaches its
/// last colour at that corner too, not only at the far one.
///
/// With a tile rectangle (`tile`, otherwise `bounds` itself) the end circle is the one around
/// the tile, while the focus stays placed in `bounds`.
pub fn circle_gradient(focus: [f64; 4], bounds: Rect, tile: Rect, stops: Vec<(f32, Rgba)>) -> Paint {
    let fx = bounds.x0 + bounds.width() * (focus[0] + (1.0 - focus[2])) / 2.0;
    let fy = bounds.y0 + bounds.height() * (focus[1] + (1.0 - focus[3])) / 2.0;
    let c = tile.center();
    let r = (tile.width().hypot(tile.height()) / 2.0).max(1e-6);
    let f = Point::new(fx, fy);
    if !(f.is_finite() && c.is_finite() && r.is_finite()) {
        return Paint::Radial { focus: c, center: c, radius: r, stops };
    }
    // A focus on or outside the end circle has no circle through it: keep it just inside.
    let d = f - c;
    let f = if d.hypot() > r * 0.999 { c + d * (r * 0.999 / d.hypot()) } else { f };
    Paint::Radial { focus: f, center: c, radius: r, stops }
}

/// Draw a picture fill into `bounds` (shape-local) honouring crop and stretch/tile modes; an animated
/// GIF shows its frame at `gif_time` seconds.
pub fn picture(ctx: &mut dyn Canvas, pres: &Presentation, pf: &PictureFill, bounds: Rect, m: Affine, gif_time: Option<f64>) {
    let Some(item) = pres.media(pf.media) else {
        missing(ctx, bounds);
        return;
    };
    let fallback = ctx.fallback();
    let px_scale = crate::scale_of(m) * fallback.map_or(1.0, |f| f.scale);
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
    // Vector canvases embed an unchanged JPEG or PNG file as it is.
    let encoded = fallback
        .filter(|_| gif_time.is_none() && pf.adjust == PictureAdjust::default())
        .and_then(|_| images::encoded_size(&item.data))
        .map(|(w, h)| CanvasImage { width: w as f64, height: h as f64, pixmap: None, encoded: Some(&item.data) });
    let image = match encoded {
        Some(img) => img,
        None => {
            // Decode for the whole image's size on the canvas (print resolution on vector
            // canvases), not the box's: a cropped picture shows only part of it, magnified (#762).
            // Metafiles (WMF, EMF) are drawn to pixels at that size too.
            let want = (dest.width().abs().max(dest.height().abs()) * px_scale).max(1.0);
            let frame = gif_time.and_then(|t| images::gif_pixmap(&item.data, &pf.adjust, t));
            let Some(pm) = frame.or_else(|| images::decode_for(&item.data, want, &pf.adjust)) else {
                missing(ctx, bounds);
                return;
            };
            CanvasImage { width: pm.width() as f64, height: pm.height() as f64, pixmap: Some(pm), encoded: None }
        }
    };
    let (iw, ih) = (image.width, image.height);
    if iw < 1.0 || ih < 1.0 {
        return;
    }
    let alpha = pf.alpha.unwrap_or(1.0).clamp(0.0, 1.0);
    if let PictureMode::Tile { sx, sy, tx, ty, flip, align } = &pf.mode {
        // The tile's size: the picture's own size (96 dpi) scaled. A scale of zero or less draws
        // nothing, as in PowerPoint.
        if !(*sx > 0.0 && *sy > 0.0 && sx.is_finite() && sy.is_finite()) {
            return;
        }
        let (nw, nh) = images::natural_size(&item.data).unwrap_or((iw, ih));
        // PowerPoint draws the tile at a whole number of output pixels.
        let k = px_scale.max(1e-9);
        let snap = |v: f64| (v * k).round().max(1.0) / k;
        let (tile_w, tile_h) = (snap(nw * 0.75 * sx), snap(nh * 0.75 * sy));
        if !(tile_w > 1e-6 && tile_h > 1e-6 && tile_w.is_finite() && tile_h.is_finite()) {
            return;
        }
        let o = tile_origin(bounds, align, (tile_w, tile_h), (*tx, *ty));
        let (mirror_x, mirror_y) = (matches!(flip.as_str(), "x" | "xy"), matches!(flip.as_str(), "y" | "xy"));
        if let Some(px) = ctx.raster()
            && let Some(pm) = &image.pixmap
        {
            // Every other tile is mirrored across the flip axes: a reflected repeat.
            let ext = |mirror: bool| if mirror { peniko::Extend::Reflect } else { peniko::Extend::Repeat };
            let tiled = vello_cpu::Image {
                image: vello_cpu::ImageSource::Pixmap(pm.clone()),
                sampler: peniko::ImageSampler { x_extend: ext(mirror_x), y_extend: ext(mirror_y), quality: peniko::ImageQuality::High, alpha: 1.0 },
            };
            px.set_transform(m);
            if alpha < 0.999 {
                px.push_layer(None, None, Some(alpha as f32), None, None);
            }
            px.set_paint(tiled);
            px.set_paint_transform(Affine::translate(o.to_vec2()) * Affine::scale_non_uniform(tile_w / iw, tile_h / ih));
            px.fill_rect(&bounds);
            px.reset_paint_transform();
            if alpha < 0.999 {
                px.pop_layer();
            }
            return;
        }
        // Vector canvases: one picture per tile covering `bounds` (the caller clips), mirrored
        // tiles drawn flipped; past a few thousand tiles the fill is rasterized instead.
        let first = |o: f64, t: f64, lo: f64| ((lo - o) / t).floor() as i64;
        let (i0, j0) = (first(o.x, tile_w, bounds.x0), first(o.y, tile_h, bounds.y0));
        let (i1, j1) = (first(o.x, tile_w, bounds.x1), first(o.y, tile_h, bounds.y1));
        let count = (i1 - i0 + 1).saturating_mul(j1 - j0 + 1);
        if !(0..=4000).contains(&count) {
            let area = m.transform_rect_bbox(bounds);
            let tile = |v: &mut vello_cpu::RenderContext, pre: Affine| {
                let Some(pm) = images::decode_for(&item.data, (tile_w.max(tile_h) * px_scale).max(1.0), &pf.adjust) else { return };
                let (pw, ph) = (pm.width() as f64, pm.height() as f64);
                let ext = |mirror: bool| if mirror { peniko::Extend::Reflect } else { peniko::Extend::Repeat };
                v.set_transform(pre * m);
                v.push_layer(Some(&bounds.to_path(0.1)), None, Some(alpha as f32), None, None);
                v.set_paint(vello_cpu::Image {
                    image: vello_cpu::ImageSource::Pixmap(pm),
                    sampler: peniko::ImageSampler {
                        x_extend: ext(mirror_x),
                        y_extend: ext(mirror_y),
                        quality: peniko::ImageQuality::High,
                        alpha: 1.0,
                    },
                });
                v.set_paint_transform(Affine::translate(o.to_vec2()) * Affine::scale_non_uniform(tile_w / pw, tile_h / ph));
                v.fill_rect(&bounds);
                v.reset_paint_transform();
                v.pop_layer();
            };
            canvas::pixels(ctx, area, &mut { tile });
            return;
        }
        for j in j0..=j1 {
            for i in i0..=i1 {
                let r = Rect::new(o.x + i as f64 * tile_w, o.y + j as f64 * tile_h, 0.0, 0.0);
                let r = Rect::new(r.x0, r.y0, r.x0 + tile_w, r.y0 + tile_h);
                let fx = mirror_x && i.rem_euclid(2) == 1;
                let fy = mirror_y && j.rem_euclid(2) == 1;
                // A mirrored tile: the same rectangle drawn flipped about its centre.
                let flip = Affine::translate(r.center().to_vec2())
                    * Affine::scale_non_uniform(if fx { -1.0 } else { 1.0 }, if fy { -1.0 } else { 1.0 })
                    * Affine::translate(-r.center().to_vec2());
                draw_image(ctx, &image, iw, ih, r, m * flip, alpha);
            }
        }
        return;
    }
    draw_image(ctx, &image, iw, ih, dest, m, alpha);
}

/// Where the first tile of a tiled picture fill goes in `bounds`: its corner, edge middle or
/// centre named by `align` (`tl`, `t`, `tr`, `l`, `ctr`, `r`, `bl`, `b`, `br`) on the same point
/// of the box, then moved by the offset (points).
pub fn tile_origin(bounds: Rect, align: &str, tile: (f64, f64), offset: (f64, f64)) -> Point {
    let (ax, ay) = match align {
        "t" => (0.5, 0.0),
        "tr" => (1.0, 0.0),
        "l" => (0.0, 0.5),
        "ctr" => (0.5, 0.5),
        "r" => (1.0, 0.5),
        "bl" => (0.0, 1.0),
        "b" => (0.5, 1.0),
        "br" => (1.0, 1.0),
        _ => (0.0, 0.0),
    };
    let (ox, oy) = (if offset.0.is_finite() { offset.0 } else { 0.0 }, if offset.1.is_finite() { offset.1 } else { 0.0 });
    Point::new(bounds.x0 + ax * (bounds.width() - tile.0) + ox, bounds.y0 + ay * (bounds.height() - tile.1) + oy)
}

fn draw_image(ctx: &mut dyn Canvas, image: &CanvasImage, iw: f64, ih: f64, dest: Rect, m: Affine, alpha: f64) {
    let t = m * Affine::translate(dest.origin().to_vec2()) * Affine::scale_non_uniform(dest.width() / iw, dest.height() / ih);
    ctx.set_transform(t);
    ctx.draw_image(image, alpha);
}

fn missing(ctx: &mut dyn Canvas, b: Rect) {
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
pub fn media_glyph(ctx: &mut dyn Canvas, video: bool, b: Rect) {
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
pub fn placeholder_icon(ctx: &mut dyn Canvas, kind: PhType, b: Rect) {
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
