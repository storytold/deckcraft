//! Crop (Picture Format ▸ Size ▸ Crop), the way PowerPoint does it.
//!
//! A picture's `crop` fractions say which part of the image fills its box, so the whole image sits
//! at a rectangle of its own in the box's local points ([`image_rect`]). Cropping keeps that image
//! rectangle where it is on the slide and moves the box's edges over it: the visible part never
//! stretches. In crop mode ([`picture.cropMode`]) the box's edges are dragged with the crop
//! handles, the image is moved under the box by dragging it and scaled from its own corners.
//!
//! Everything here works in the box's local frame (before rotation and flips), so it holds for
//! rotated and flipped pictures alike.

use deckcraft_geom::{Point, Rect, Xfrm};
use deckcraft_model::{ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, id_param, str_param, targets, xfrm_of};
use crate::{Result, Session};

/// Smallest box or image side a crop gesture leaves, in points.
pub const MIN_SIDE: f64 = 1.0;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "picture.cropMode",
            "Crop",
            ["Picture Format", "Size", "Crop"],
            None,
            "{on?: bool (default: toggle), id?} — show the crop handles on one selected picture: drag them to crop, drag the picture to move it inside, its round handles to scale it; Enter, Escape or a click elsewhere ends",
            has_one_picture,
            crop_mode
        ),
        cmd!(
            "picture.cropAspect",
            "Aspect Ratio",
            ["Picture Format", "Size", "Crop", "Aspect Ratio"],
            None,
            "{ratio: \"w:h\" (1:1, 2:3, 3:4, 3:5, 4:5, 3:2, 4:3, 5:3, 5:4, 16:9, 16:10…), ids?} — crop to that shape, as large as the picture allows, centred",
            has_picture,
            crop_aspect
        ),
        cmd!(
            "picture.cropFill",
            "Fill / Fit",
            ["Picture Format", "Size", "Crop"],
            None,
            "{mode?: fill|fit (default fill), ids?} — keep the box; fill: the picture covers it, fit: the whole picture shows inside it",
            has_picture,
            crop_fill
        ),
    ]
}

pub fn has_picture(s: &Session) -> std::result::Result<(), String> {
    has_selection(s)?;
    let any = s.active().is_some_and(|d| d.selected_shapes().iter().any(|sh| matches!(sh.kind, ShapeKind::Picture { .. })));
    if any { Ok(()) } else { Err("select a picture".into()) }
}

/// Crop mode works on one picture: with anything else selected too it is off.
pub fn has_one_picture(s: &Session) -> std::result::Result<(), String> {
    has_selection(s)?;
    let one = s.active().is_some_and(|d| matches!(d.selected_shapes().as_slice(), [sh] if matches!(sh.kind, ShapeKind::Picture { .. })));
    if one { Ok(()) } else { Err("select one picture".into()) }
}

/// The whole image in the box's local points (`w`×`h` box, `crop` = left, top, right, bottom).
pub fn image_rect(w: f64, h: f64, crop: [f64; 4]) -> Option<Rect> {
    let [l, t, r, b] = crop;
    let (fx, fy) = (1.0 - l - r, 1.0 - t - b);
    if !(fx > 1e-6 && fy > 1e-6 && w.is_finite() && h.is_finite()) {
        return None;
    }
    let (iw, ih) = (w / fx, h / fy);
    let (x0, y0) = (-l * iw, -t * ih);
    let r = Rect::new(x0, y0, x0 + iw, y0 + ih);
    (r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()).then_some(r)
}

/// The crop fractions that put the image at `img` in a `w`×`h` box (inverse of [`image_rect`]).
pub fn crop_of(w: f64, h: f64, img: Rect) -> Option<[f64; 4]> {
    let (iw, ih) = (img.width(), img.height());
    if !(iw > 1e-6 && ih > 1e-6) {
        return None;
    }
    let c = [-img.x0 / iw, -img.y0 / ih, (img.x1 - w) / iw, (img.y1 - h) / ih];
    c.iter().all(|v| v.is_finite()).then_some(c)
}

/// The box that shows local rectangle `f` of box `orig` in place on the slide (rotation and flips
/// kept). Points of `orig`'s frame at `q` are at `q - f.origin()` in the new one.
pub fn frame_xfrm(orig: &Xfrm, f: Rect) -> Xfrm {
    let c = orig.affine() * f.center();
    let (w, h) = (f.width(), f.height());
    Xfrm { x: c.x - w / 2.0, y: c.y - h / 2.0, w, h, ..*orig }
}

/// Handle positions of a local rectangle, in the order of box handles: NW, N, NE, E, SE, S, SW, W.
pub fn rect_handles(r: Rect) -> [Point; 8] {
    let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    [
        Point::new(r.x0, r.y0),
        Point::new(cx, r.y0),
        Point::new(r.x1, r.y0),
        Point::new(r.x1, cy),
        Point::new(r.x1, r.y1),
        Point::new(cx, r.y1),
        Point::new(r.x0, r.y1),
        Point::new(r.x0, cy),
    ]
}

/// Which sides a handle moves: (left, top, right, bottom).
fn sides(handle: u8) -> [bool; 4] {
    match handle {
        0 => [true, true, false, false],
        1 => [false, true, false, false],
        2 => [false, true, true, false],
        3 => [false, false, true, false],
        4 => [false, false, true, true],
        5 => [false, false, false, true],
        6 => [true, false, false, true],
        _ => [true, false, false, false],
    }
}

/// Crop handle `handle` of a `w`×`h` box dragged by local `d`: the new frame (orig's local points).
pub fn drag_frame(w: f64, h: f64, handle: u8, d: deckcraft_geom::Vec2) -> Rect {
    let [l, t, r, b] = sides(handle);
    let mut x0 = if l { d.x } else { 0.0 };
    let mut y0 = if t { d.y } else { 0.0 };
    let mut x1 = if r { w + d.x } else { w };
    let mut y1 = if b { h + d.y } else { h };
    if l {
        x0 = x0.min(x1 - MIN_SIDE);
    } else if r {
        x1 = x1.max(x0 + MIN_SIDE);
    }
    if t {
        y0 = y0.min(y1 - MIN_SIDE);
    } else if b {
        y1 = y1.max(y0 + MIN_SIDE);
    }
    Rect::new(x0, y0, x1, y1)
}

/// The image's own handle `handle` dragged by local `d`, the opposite side staying put. Corners
/// keep the picture's proportions (`free` lets them go); sides stretch one way.
pub fn drag_image(img: Rect, handle: u8, d: deckcraft_geom::Vec2, free: bool) -> Rect {
    let [l, t, r, b] = sides(handle);
    let corner = (l || r) && (t || b);
    if corner && !free {
        let anchor = Point::new(if l { img.x1 } else { img.x0 }, if t { img.y1 } else { img.y0 });
        let from = Point::new(if l { img.x0 } else { img.x1 }, if t { img.y0 } else { img.y1 });
        let diag = from - anchor;
        let len2 = diag.hypot2();
        if len2 <= 1e-9 {
            return img;
        }
        let min = MIN_SIDE / img.width().min(img.height()).max(1e-9);
        let s = ((from + d - anchor).dot(diag) / len2).max(min);
        let to = anchor + diag * s;
        return Rect::from_points(anchor, to);
    }
    let mut n = img;
    if l {
        n.x0 = (img.x0 + d.x).min(img.x1 - MIN_SIDE);
    }
    if r {
        n.x1 = (img.x1 + d.x).max(img.x0 + MIN_SIDE);
    }
    if t {
        n.y0 = (img.y0 + d.y).min(img.y1 - MIN_SIDE);
    }
    if b {
        n.y1 = (img.y1 + d.y).max(img.y0 + MIN_SIDE);
    }
    n
}

/// The one picture crop mode works on: given by `id` or the one selected, on the slide itself.
fn crop_target(s: &Session, p: &Value, cmd: &str) -> Result<ShapeId> {
    let st = s.doc()?;
    let id = match id_param(p, "id", cmd)? {
        Some(i) => i,
        None => match st.selection.shapes.as_slice() {
            [one] => *one,
            _ => return Err(bad(cmd, "select one picture")),
        },
    };
    let sh = st.shape(id).ok_or_else(|| bad(cmd, "no such shape"))?;
    if !matches!(sh.kind, ShapeKind::Picture { .. }) {
        return Err(bad(cmd, "only a picture can be cropped"));
    }
    if sh.locked {
        return Err(bad(cmd, "the object is locked"));
    }
    if !st.shapes().iter().any(|t| t.id == id) {
        return Err(bad(cmd, "a picture in a group can't be cropped; ungroup it first"));
    }
    Ok(id)
}

fn crop_mode(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(s.tool.crop.is_none());
    if !on {
        s.exit_crop();
        return Ok(json!({"cropping": Value::Null}));
    }
    let id = crop_target(s, p, "picture.cropMode")?;
    s.select(|_, sel| {
        sel.shapes = vec![id];
        sel.text = None;
        sel.cells = None;
    })?;
    s.tool.edit_points = None;
    s.tool.edit_vertex = None;
    s.tool.crop = Some(id);
    Ok(json!({"cropping": id.0}))
}

/// Each target picture's box (slide points) and crop, before the change.
fn pictures(s: &Session, p: &Value) -> Result<Vec<(ShapeId, Xfrm, [f64; 4])>> {
    let ids = targets(s, p)?;
    let st = s.doc()?;
    Ok(ids
        .iter()
        .filter_map(|id| {
            let sh = st.shape(*id)?;
            match &sh.kind {
                ShapeKind::Picture { fill } if !sh.locked => Some((*id, xfrm_of(&st.doc, &st.selection, sh), fill.crop)),
                _ => None,
            }
        })
        .collect())
}

/// Store a new box and crop on each picture (`None` leaves the box).
pub(crate) fn store(s: &mut Session, cmd: &str, items: &[(ShapeId, Option<Xfrm>, [f64; 4])]) -> Result<Value> {
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
        for (id, x, c) in items {
            if let Some(sh) = deckcraft_model::find_shape_mut(list, *id) {
                if let Some(x) = x {
                    sh.xfrm = Some(*x);
                }
                if let ShapeKind::Picture { fill } = &mut sh.kind {
                    fill.crop = *c;
                }
            }
        }
        Ok(())
    })?;
    Ok(json!({"cropped": items.len()}))
}

/// `picture.crop` with the frame following the crop (PowerPoint): the image keeps its size and
/// place and the box shrinks or grows over it.
pub(crate) fn crop_keeping_scale(s: &mut Session, p: &Value, want: [Option<f64>; 4]) -> Result<Value> {
    let mut out = vec![];
    for (id, x, old) in pictures(s, p)? {
        let Some(img) = image_rect(x.w, x.h, old) else { continue };
        let mut c = old;
        for (slot, v) in c.iter_mut().zip(want) {
            if let Some(v) = v.filter(|v| v.is_finite()) {
                *slot = v.clamp(-10.0, 0.95);
            }
        }
        // Keep at least a sliver showing either way.
        if c[0] + c[2] > 0.99 || c[1] + c[3] > 0.99 {
            return Err(bad("picture.crop", "the crop leaves nothing of the picture"));
        }
        let (iw, ih) = (img.width(), img.height());
        let f = Rect::new(img.x0 + c[0] * iw, img.y0 + c[1] * ih, img.x1 - c[2] * iw, img.y1 - c[3] * ih);
        out.push((id, Some(frame_xfrm(&x, f)), c));
    }
    store(s, "picture.crop", &out)
}

fn parse_ratio(v: &Value) -> Option<f64> {
    let r = match v {
        Value::String(t) => {
            let (a, b) = t.split_once([':', '/', 'x'])?;
            a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?
        }
        Value::Array(a) => a.first()?.as_f64()? / a.get(1)?.as_f64()?,
        _ => v.as_f64()?,
    };
    (r.is_finite() && (0.01..=100.0).contains(&r)).then_some(r)
}

fn crop_aspect(s: &mut Session, p: &Value) -> Result<Value> {
    let ratio = p.get("ratio").and_then(parse_ratio).ok_or_else(|| bad("picture.cropAspect", "ratio must be like \"16:9\""))?;
    let mut out = vec![];
    for (id, x, c) in pictures(s, p)? {
        let Some(img) = image_rect(x.w, x.h, c) else { continue };
        // The largest box of that shape inside the picture, centred on it.
        let (iw, ih) = (img.width(), img.height());
        let (w, h) = if iw / ih > ratio { (ih * ratio, ih) } else { (iw, iw / ratio) };
        let ctr = img.center();
        let f = Rect::new(ctr.x - w / 2.0, ctr.y - h / 2.0, ctr.x + w / 2.0, ctr.y + h / 2.0);
        let Some(nc) = crop_of(w, h, img - f.origin().to_vec2()) else { continue };
        out.push((id, Some(frame_xfrm(&x, f)), nc));
    }
    let r = store(s, "picture.cropAspect", &out)?;
    // Then straight into crop mode to adjust it, as PowerPoint does.
    if let [(id, ..)] = out.as_slice() {
        s.tool.crop = Some(*id);
    }
    Ok(r)
}

fn crop_fill(s: &mut Session, p: &Value) -> Result<Value> {
    let fit = match str_param(p, "mode").unwrap_or("fill") {
        "fill" => false,
        "fit" => true,
        m => return Err(bad("picture.cropFill", format!("mode {m:?} is not fill or fit"))),
    };
    let mut out = vec![];
    for (id, x, c) in pictures(s, p)? {
        let Some(img) = image_rect(x.w, x.h, c) else { continue };
        let (kx, ky) = (x.w / img.width(), x.h / img.height());
        let k = if fit { kx.min(ky) } else { kx.max(ky) };
        let (w, h) = (img.width() * k, img.height() * k);
        let (x0, y0) = ((x.w - w) / 2.0, (x.h - h) / 2.0);
        let Some(nc) = crop_of(x.w, x.h, Rect::new(x0, y0, x0 + w, y0 + h)) else { continue };
        out.push((id, None, nc));
    }
    store(s, "picture.cropFill", &out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_geom::Vec2;

    #[test]
    fn image_rect_and_crop_of_round_trip() {
        let c = [0.1, 0.2, 0.3, 0.05];
        let img = image_rect(60.0, 75.0, c).unwrap();
        assert!((img.width() - 100.0).abs() < 1e-9 && (img.height() - 100.0).abs() < 1e-9);
        let back = crop_of(60.0, 75.0, img).unwrap();
        for (a, b) in back.iter().zip(c) {
            assert!((a - b).abs() < 1e-9);
        }
        assert!(image_rect(10.0, 10.0, [0.5, 0.0, 0.5, 0.0]).is_none());
    }

    #[test]
    fn frame_handles_keep_a_minimum_size() {
        let f = drag_frame(100.0, 50.0, 3, Vec2::new(-500.0, 0.0));
        assert!((f.width() - MIN_SIDE).abs() < 1e-9);
        let f = drag_frame(100.0, 50.0, 0, Vec2::new(10.0, 5.0));
        assert_eq!((f.x0, f.y0, f.x1, f.y1), (10.0, 5.0, 100.0, 50.0));
    }

    #[test]
    fn image_corners_keep_proportions() {
        let img = Rect::new(0.0, 0.0, 200.0, 100.0);
        let n = drag_image(img, 4, Vec2::new(200.0, 0.0), false);
        assert!((n.width() / n.height() - 2.0).abs() < 1e-9);
        assert_eq!((n.x0, n.y0), (0.0, 0.0));
        let n = drag_image(img, 3, Vec2::new(50.0, 0.0), false);
        assert_eq!((n.width(), n.height()), (250.0, 100.0));
    }

    #[test]
    fn ratios_parse() {
        assert!((parse_ratio(&json!("16:9")).unwrap() - 16.0 / 9.0).abs() < 1e-12);
        assert_eq!(parse_ratio(&json!([4, 3])), Some(4.0 / 3.0));
        assert!(parse_ratio(&json!("0:3")).is_none());
        assert!(parse_ratio(&json!("a:b")).is_none());
    }
}
