//! Shape Format: position and size, rotation, flips, geometry, fill, outline, effects, styles,
//! names and alt text.

use std::collections::HashMap;

use deckcraft_color::{ColorTransform, Rgba, SchemeSlot};
use deckcraft_model::style::{Dash, Glow, Gradient, GradientShape, GradientStop, LineEnd, PatternFill, PictureFill, PictureMode, Reflection, Shadow};
use deckcraft_model::{ColorRef, Effects, Fill, Geom, Line, ShapeId, ShapeStyle};
use serde_json::{Value, json};

use super::{
    CommandSpec, NO_ID_SPACE, bad, bool_or, bool_param, cmd, color_param, color_value, edit_geometry, edit_shapes, f64_param, has_selection,
    has_slide, id_param, ok, str_param, targets, usize_param, with_param, xfrm_of,
};
use crate::{Result, Session};

mod appearance;
mod freeform;
mod picture;
mod size;

pub(crate) use appearance::gradient_param;
pub use appearance::quick_style_for;
use appearance::{adjust, change, effects, fill, line, picture_props, quick_style, smooth_corners};
pub use freeform::freeform_path;
use freeform::{connect, freeform, ink_to_shape, sites};
use picture::{crop, media_options, picture_adjust, picture_change, picture_reset};
use size::{clamp_pos, clamp_side, flip, rotate, size_position};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.connect",
            "Connect Shapes",
            [],
            None,
            "{from: shape id, to: shape id, fromSite?, toSite?: site index (default: the closest pair), id?: existing line to glue (else a new connector), preset?: straightConnector1|bentConnector3|curvedConnector3} → {id}",
            has_slide,
            connect
        ),
        cmd!(
            "shape.freeform",
            "Freeform",
            ["Insert", "Shapes"],
            None,
            "{points: [[x, y], …] slide pt (≥ 2), closed?: bool (filled shape), smooth?: bool (curve through the points)} → {id}",
            has_slide,
            freeform
        ),
        cmd!(
            "draw.inkToShape",
            "Ink to Shape",
            ["Draw", "Convert"],
            None,
            "{} → {shapes: [ids], kept: n} turns recognisable ink strokes on the slide into lines, rectangles, ellipses and triangles",
            has_slide,
            ink_to_shape
        ),
        cmd!(query "shape.sites", "Connection Sites", [], None, "{id} → [[x, y]] connection sites in slide points", has_slide, sites),
        cmd!(
            "shape.move",
            "Move",
            [],
            None,
            "{dx?: pt (relative), dy?: pt (relative), ids?} | {x?: pt (absolute), y?: pt (absolute), ids?}",
            has_selection,
            move_by
        ),
        cmd!("shape.resize", "Size", ["Shape Format", "Size"], None, "{w?: pt, h?: pt, lockAspect?: bool, ids?}", has_selection, resize),
        cmd!("shape.setBounds", "Position and Size", [], None, "{x, y, w, h, ids?}", has_selection, set_bounds),
        cmd!(
            "shape.rotate",
            "Rotation",
            ["Shape Format", "Arrange", "Rotate"],
            None,
            "{deg?: absolute, by?: relative, ids?, gesture?: move|end}",
            has_selection,
            rotate
        ),
        cmd!(
            "shape.sizePosition",
            "Size and Position…",
            ["Shape Format", "Size"],
            None,
            "{x?, y?: pt, w?, h?: pt, rot?: deg, scaleW?, scaleH?: percent of the current size (100 = unchanged), lockAspect?: bool, ids?} — with none of those keys, opens the dialog",
            has_selection,
            size_position
        ),
        cmd!("shape.flip", "Flip", ["Shape Format", "Arrange", "Rotate"], None, "{axis: horizontal|vertical, ids?}", has_selection, flip),
        cmd!("shape.adjust", "Adjust Shape", [], None, "{index, value (file units), ids?}", has_selection, adjust),
        cmd!("shape.change", "Change Shape", ["Shape Format", "Insert Shapes", "Edit Shape"], None, "{preset, ids?}", has_selection, change),
        cmd!(
            "shape.smoothCorners",
            "Smooth Corners",
            ["Shape Format", "Insert Shapes", "Edit Shape"],
            None,
            "{smoothing: 0..100 (%), radius?: pt (default: the rounded rectangle's own), ids?} → rectangle outline with smoothed (squircle) corners",
            has_selection,
            smooth_corners
        ),
        cmd!(
            "shape.fill",
            "Shape Fill",
            ["Shape Format", "Shape Styles"],
            None,
            "{color? | none?: true | gradient?: {stops: [[pos, color]], angle?, kind?: linear|radial} | picture?: base64 | pattern?: {preset, fg, bg} | transparency?: 0..1 | reset?: true, ids?}; pictureProps?: {tile?: bool, offsetX?, offsetY?: pt, scaleX?, scaleY?: 0..10, align?: tl|t|tr|l|ctr|r|bl|b|br, mirror?: none|x|y|xy, stretch?: [l, t, r, b] fractions, rotateWithShape?: bool} adjusts a picture fill; a group also formats its members",
            has_selection,
            fill
        ),
        cmd!(
            "shape.line",
            "Shape Outline",
            ["Shape Format", "Shape Styles"],
            None,
            "{color?, none?, width?: pt, dash?: solid|dot|dash|lgDash|dashDot|sysDot|sysDash, cap?: flat|round|square, join?: round|bevel|miter, compound?: single|double|thickThin|thinThick|triple, head?: none|triangle|stealth|diamond|oval|arrow, tail?, headSize?: sm|med|lg, tailSize?, reset?, ids?}; a group also formats its members",
            has_selection,
            line
        ),
        cmd!(
            "shape.effects",
            "Shape Effects",
            ["Shape Format", "Shape Styles"],
            None,
            "{shadow?: none|outer|inner|perspective|{color, blur, dist, dir}, glow?: none|{color, radius}, softEdges?: pt, reflection?: none|tight|half|full, bevel?: none|circle|…, threeD?: {camera?: preset, rotX?, rotY?, rotZ?: deg, z?: pt, bevelTop?, bevelBottom?: none|circle|…, bevelTopW?, bevelTopH?, bevelBottomW?, bevelBottomH?, depth?, contour?: pt, material?, lighting?, reset?: true}, shadowProps?: {color?, transparency?: 0..1, blur?: pt, dist?: pt, dir?: deg}, glowProps?: {color?, transparency?, radius?: pt}, reflectionProps?: {transparency?, size?: 0..1, dist?: pt, blur?: pt}, reset?, ids?} (the *Props keys adjust the current effect, adding a default one when there is none)",
            has_selection,
            effects
        ),
        cmd!(
            "shape.quickStyle",
            "Quick Styles",
            ["Home", "Drawing"],
            None,
            "{index: 0..41 (theme style row×accent), ids?}",
            has_selection,
            quick_style
        ),
        cmd!("shape.rename", "Rename", ["Selection Pane"], None, "{name, id?}", has_selection, rename),
        cmd!("shape.altText", "Alt Text", ["Shape Format", "Accessibility"], None, "{text, decorative?: bool, ids?}", has_selection, alt_text),
        cmd!("shape.visible", "Show/Hide", ["Selection Pane"], None, "{visible?: bool, ids?}", has_selection, visible),
        cmd!(
            "shape.lock",
            "Lock",
            ["Selection Pane"],
            None,
            "{locked?: bool (default: toggle), ids?} (a group locks or unlocks with everything in it)",
            has_selection,
            lock
        ),
        cmd!("shape.setDefault", "Set as Default Shape", [], None, "{}", has_selection, set_default),
        cmd!(
            "picture.crop",
            "Crop",
            ["Picture Format", "Size"],
            None,
            "{left?: fraction 0..1, top?: fraction 0..1, right?: fraction 0..1, bottom?: fraction 0..1, ids?}",
            has_selection,
            crop
        ),
        cmd!(
            "picture.adjust",
            "Corrections",
            ["Picture Format", "Adjust"],
            None,
            "{brightness?: -1..1, contrast?: -1..1, saturation?: 0..4, grayscale?: bool, transparency?: 0..1, reset?: bool, ids?}",
            has_selection,
            picture_adjust
        ),
        cmd!(
            "picture.reset",
            "Reset Picture",
            ["Picture Format", "Adjust"],
            None,
            "{ids?} (discards the picture's formatting: adjustments, artistic effect, crop, transparency, picture-style effects and outline; the frame shape from Change Shape or Crop to Shape, the size and the position stay)",
            has_selection,
            picture_reset
        ),
        cmd!(
            "picture.change",
            "Change Picture",
            ["Picture Format", "Adjust"],
            None,
            "{path? | data: base64, name?, ids?}",
            has_selection,
            picture_change
        ),
        cmd!(
            "media.options",
            "Playback",
            ["Playback"],
            None,
            "{autoplay?, loop?, rewind?, acrossSlides?, hide?, fullScreen?, volume?: 0..1, trimStart?: ms, trimEnd?: ms, fadeIn?: ms, fadeOut?: ms, ids?}",
            has_selection,
            media_options
        ),
    ]
}

fn move_by(s: &mut Session, p: &Value) -> Result<Value> {
    let (dx, dy) = (f64_param(p, "dx"), f64_param(p, "dy"));
    let (ax, ay) = (f64_param(p, "x"), f64_param(p, "y"));
    edit_geometry(s, p, "shape.move", |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            x.x = clamp_pos(ax.unwrap_or(x.x) + dx.unwrap_or(0.0));
            x.y = clamp_pos(ay.unwrap_or(x.y) + dy.unwrap_or(0.0));
        }
        Ok(())
    })
}

fn resize(s: &mut Session, p: &Value) -> Result<Value> {
    let (w, h) = (f64_param(p, "w").map(|v| v.max(0.0)), f64_param(p, "h").map(|v| v.max(0.0)));
    let lock = bool_or(p, "lockAspect", false);
    edit_geometry(s, p, "shape.resize", |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            let ar = if x.h > 0.0 { x.w / x.h } else { 1.0 };
            let (cx, cy) = (x.x + x.w / 2.0, x.y + x.h / 2.0);
            match (w, h, lock) {
                (Some(w), _, true) => {
                    x.w = w;
                    x.h = if ar > 0.0 { w / ar } else { x.h };
                }
                (None, Some(h), true) => {
                    x.h = h;
                    x.w = h * ar;
                }
                _ => {
                    x.w = w.unwrap_or(x.w);
                    x.h = h.unwrap_or(x.h);
                }
            }
            x.w = clamp_side(x.w);
            x.h = clamp_side(x.h);
            // Rotated shapes resize about their centre; others keep the top-left.
            if x.rot != 0.0 {
                x.x = clamp_pos(cx - x.w / 2.0);
                x.y = clamp_pos(cy - x.h / 2.0);
            }
        }
        Ok(())
    })
}

fn set_bounds(s: &mut Session, p: &Value) -> Result<Value> {
    edit_geometry(s, p, "shape.setBounds", |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            x.x = clamp_pos(f64_param(p, "x").unwrap_or(x.x));
            x.y = clamp_pos(f64_param(p, "y").unwrap_or(x.y));
            x.w = clamp_side(f64_param(p, "w").unwrap_or(x.w));
            x.h = clamp_side(f64_param(p, "h").unwrap_or(x.h));
        }
        Ok(())
    })
}

/// The fill a `shape.fill` request sets: `Some(None)` clears it, `Some(Some(_))` sets it, and `None`
/// leaves it as it is.
fn requested_fill(s: &mut Session, p: &Value) -> Result<Option<Option<Fill>>> {
    if bool_or(p, "reset", false) {
        return Ok(Some(None));
    }
    if bool_or(p, "none", false) {
        return Ok(Some(Some(Fill::None)));
    }
    if let Some(c) = color_param(p, "color") {
        return Ok(Some(Some(Fill::solid(c))));
    }
    if let Some(g) = p.get("gradient") {
        let gradient = gradient_param(g).ok_or_else(|| bad("shape.fill", "gradient needs at least two `stops`: [[0, \"#fff\"], [1, \"#000\"]]"))?;
        return Ok(Some(Some(Fill::Gradient(gradient))));
    }
    if let Some(pt) = p.get("pattern") {
        return Ok(Some(Some(Fill::Pattern(PatternFill {
            preset: pt.get("preset").and_then(Value::as_str).unwrap_or("pct50").to_string(),
            fg: pt.get("fg").and_then(color_value).unwrap_or(ColorRef::scheme(SchemeSlot::Accent1)),
            bg: pt.get("bg").and_then(color_value).unwrap_or(ColorRef::scheme(SchemeSlot::Bg1)),
        }))));
    }
    if p.get("picture").is_some() {
        let (name, bytes) = super::insert::media_bytes(&with_param(p, "data", p.get("picture").cloned().unwrap_or_default()), "shape.fill")?;
        let ct = super::insert::content_type(&name, &bytes);
        let media = s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad("shape.fill", NO_ID_SPACE)))?;
        return Ok(Some(Some(Fill::Picture(PictureFill { media, ..Default::default() }))));
    }
    Ok(None)
}

/// The effective fill of each targeted shape, for a transparency change on an inherited (style) fill.
fn inherited_fills(s: &Session, p: &Value) -> Result<HashMap<ShapeId, Option<Fill>>> {
    let st = s.doc()?;
    let ctx = super::text::ctx_for(&st.doc, &st.selection);
    let mut fills = HashMap::new();
    for id in targets(s, p)? {
        if let Some(sh) = st.shape(id) {
            deckcraft_model::walk(std::slice::from_ref(sh), &mut |sh, _| {
                let fill = ctx.as_ref().and_then(|c| {
                    let (f, ph) = deckcraft_model::resolve::fill(c, sh);
                    f.map(|f| match f {
                        Fill::Solid { color } => Fill::solid(ColorRef::rgb(c.color(&color, ph))),
                        other => other,
                    })
                });
                fills.insert(sh.id, fill);
            });
        }
    }
    Ok(fills)
}

/// Applies a `shape.fill` request to one shape: the fill, then its picture options, then transparency.
fn apply_fill(
    sh: &mut deckcraft_model::Shape,
    new_fill: Option<&Option<Fill>>,
    pic_props: Option<&Value>,
    transparency: Option<f64>,
    inherited: &HashMap<ShapeId, Option<Fill>>,
) -> Result<()> {
    if let Some(f) = new_fill {
        sh.fill.clone_from(f);
    }
    if let (Some(pp), Some(Fill::Picture(pf))) = (pic_props, sh.fill.as_mut()) {
        picture_props(pf, pp);
    }
    if let (Some(t), Some(Fill::Picture(pf))) = (transparency, sh.fill.as_mut()) {
        let a = 1.0 - t.clamp(0.0, 1.0);
        pf.alpha = (a < 1.0).then_some(a);
        return Ok(());
    }
    if let Some(t) = transparency {
        let base = sh.fill.clone().or_else(|| inherited.get(&sh.id).cloned().flatten());
        if let Some(Fill::Solid { mut color }) = base {
            color.mods.retain(|m| !matches!(m, ColorTransform::Alpha(_)));
            if t > 0.0 {
                color.mods.push(ColorTransform::Alpha(((1.0 - t.clamp(0.0, 1.0)) * 100_000.0).round() as i32));
            }
            sh.fill = Some(Fill::Solid { color });
        }
    }
    Ok(())
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("shape.rename", "missing `name`"))?.to_string();
    edit_shapes(s, p, "shape.rename", |sh| {
        sh.name = name.clone();
        Ok(())
    })
}

fn alt_text(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").unwrap_or("").to_string();
    let deco = bool_param(p, "decorative");
    edit_shapes(s, p, "shape.altText", |sh| {
        sh.descr = text.clone();
        if let Some(d) = deco {
            sh.decorative = d;
        }
        Ok(())
    })
}

fn visible(s: &mut Session, p: &Value) -> Result<Value> {
    let v = bool_param(p, "visible");
    edit_shapes(s, p, "shape.visible", |sh| {
        sh.hidden = !v.unwrap_or(sh.hidden);
        Ok(())
    })
}

/// Lock: a locked object can be selected, formatted and typed in, but not moved, sized, rotated,
/// flipped, reshaped or cropped. Locking or unlocking a group does the same to everything in it
/// (a child can still be unlocked on its own afterwards), as PowerPoint does.
fn lock(s: &mut Session, p: &Value) -> Result<Value> {
    let v = bool_param(p, "locked");
    edit_shapes(s, p, "shape.lock", |sh| {
        let to = v.unwrap_or(!sh.locked);
        set_locked(sh, to, 0);
        Ok(())
    })
}

fn set_locked(sh: &mut deckcraft_model::Shape, to: bool, depth: usize) {
    sh.locked = to;
    if depth > 64 {
        return;
    }
    if let Some(children) = sh.children_mut() {
        for c in children {
            set_locked(c, to, depth + 1);
        }
    }
}

fn set_default(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let Some(sh) = st.selected_shapes().first().map(|x| (*x).clone()) else { return ok() };
    // New shapes drawn in this session take this look (Set as Default Shape).
    s.default_look = Some(super::format::Painted {
        run: sh.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).map(|r| r.props.clone()),
        para: None,
        fill: sh.fill.clone(),
        line: sh.line.clone(),
        effects: sh.effects.clone(),
        style: sh.style.clone(),
    });
    ok()
}

fn crop(s: &mut Session, p: &Value) -> Result<Value> {
    let c = [f64_param(p, "left"), f64_param(p, "top"), f64_param(p, "right"), f64_param(p, "bottom")];
    edit_geometry(s, p, "picture.crop", |sh| {
        if let deckcraft_model::ShapeKind::Picture { fill } = &mut sh.kind {
            for (i, v) in c.iter().enumerate() {
                if let (Some(v), Some(slot)) = (v, fill.crop.get_mut(i)) {
                    *slot = v.clamp(-10.0, 0.95);
                }
            }
        }
        Ok(())
    })
}

fn picture_adjust(s: &mut Session, p: &Value) -> Result<Value> {
    edit_shapes(s, p, "picture.adjust", |sh| {
        if let deckcraft_model::ShapeKind::Picture { fill } = &mut sh.kind {
            let a = &mut fill.adjust;
            if bool_or(p, "reset", false) {
                *a = Default::default();
            }
            if let Some(v) = f64_param(p, "brightness") {
                a.brightness = v.clamp(-1.0, 1.0);
            }
            if let Some(v) = f64_param(p, "contrast") {
                a.contrast = v.clamp(-1.0, 1.0);
            }
            if let Some(v) = f64_param(p, "saturation") {
                a.saturation = Some(v.clamp(0.0, 4.0));
            }
            if let Some(v) = bool_param(p, "grayscale") {
                a.grayscale = v;
            }
            if let Some(v) = f64_param(p, "transparency") {
                fill.alpha = Some((1.0 - v).clamp(0.0, 1.0));
            }
        }
        Ok(())
    })
}

fn picture_reset(s: &mut Session, p: &Value) -> Result<Value> {
    edit_shapes(s, p, "picture.reset", |sh| {
        if let deckcraft_model::ShapeKind::Picture { fill } = &mut sh.kind {
            fill.adjust = Default::default();
            fill.crop = [0.0; 4];
            fill.alpha = None;
            if let Some(a) = fill.artistic.take() {
                fill.media = a.original;
                fill.svg = a.svg;
            }
        }
        sh.effects = None;
        sh.line = None;
        Ok(())
    })
}

fn picture_change(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = super::insert::media_bytes(p, "picture.change")?;
    let ct = super::insert::content_type(&name, &bytes);
    let media = s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad("picture.change", NO_ID_SPACE)))?;
    edit_shapes(s, p, "picture.change", |sh| {
        if let deckcraft_model::ShapeKind::Picture { fill } = &mut sh.kind {
            fill.media = media;
            fill.crop = [0.0; 4];
            fill.artistic = None;
            fill.svg = None;
        }
        Ok(())
    })
}

fn media_options(s: &mut Session, p: &Value) -> Result<Value> {
    edit_shapes(s, p, "media.options", |sh| {
        if let deckcraft_model::ShapeKind::Media(m) = &mut sh.kind {
            if let Some(v) = bool_param(p, "autoplay") {
                m.autoplay = v;
            }
            if let Some(v) = bool_param(p, "loop") {
                m.loop_play = v;
            }
            if let Some(v) = bool_param(p, "rewind") {
                m.rewind = v;
            }
            if let Some(v) = bool_param(p, "acrossSlides") {
                m.play_across_slides = v;
            }
            if let Some(v) = bool_param(p, "hide") {
                m.hide_while_not_playing = v;
            }
            if let Some(v) = bool_param(p, "fullScreen") {
                m.full_screen = v;
            }
            if let Some(v) = f64_param(p, "volume") {
                m.volume = v.clamp(0.0, 1.0);
            }
            let ms = |k: &str| p.get(k).and_then(Value::as_u64).map(|v| v.min(u32::MAX as u64) as u32);
            if let Some(v) = ms("trimStart") {
                m.trim_start_ms = v;
            }
            if let Some(v) = ms("trimEnd") {
                m.trim_end_ms = v;
            }
            if let Some(v) = ms("fadeIn") {
                m.fade_in_ms = v;
            }
            if let Some(v) = ms("fadeOut") {
                m.fade_out_ms = v;
            }
        }
        Ok(())
    })?;
    Ok(json!({}))
}

fn sites(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id", "shape.sites")?.ok_or_else(|| bad("shape.sites", "missing `id`"))?;
    let st = s.doc()?;
    let sh = st.shape(id).ok_or_else(|| bad("shape.sites", "no such shape"))?;
    Ok(json!(crate::connect::sites(&st.doc, &st.selection, sh).iter().map(|q| [q.x, q.y]).collect::<Vec<_>>()))
}

fn connect(s: &mut Session, p: &Value) -> Result<Value> {
    let (Some(from), Some(to)) = (id_param(p, "from", "shape.connect")?, id_param(p, "to", "shape.connect")?) else {
        return Err(bad("shape.connect", "missing `from` / `to`"));
    };
    let st = s.doc()?;
    let site_list = |id| st.shape(id).map(|sh| crate::connect::sites(&st.doc, &st.selection, sh)).unwrap_or_default();
    let (a, b) = (site_list(from), site_list(to));
    if a.is_empty() || b.is_empty() {
        return Err(bad("shape.connect", "both shapes need connection sites (lines and groups have none)"));
    }
    let pick = |v: &[deckcraft_geom::Point], key: &str| usize_param(p, key).filter(|i| *i < v.len());
    let (ia, ib) = match (pick(&a, "fromSite"), pick(&b, "toSite")) {
        (Some(i), Some(j)) => (i, j),
        (fi, fj) => {
            let mut best = (f64::MAX, 0, 0);
            for (i, pa) in a.iter().enumerate().filter(|(i, _)| fi.is_none_or(|f| f == *i)) {
                for (j, pb) in b.iter().enumerate().filter(|(j, _)| fj.is_none_or(|f| f == *j)) {
                    let d = (*pa - *pb).hypot();
                    if d < best.0 {
                        best = (d, i, j);
                    }
                }
            }
            (best.1, best.2)
        }
    };
    let (p0, p1) = (a[ia], b[ib]);
    let id = match id_param(p, "id", "shape.connect")? {
        Some(id) => id,
        None => {
            let preset = str_param(p, "preset").unwrap_or("straightConnector1");
            let r = s.execute("shape.insert", &json!({"preset": preset, "rect": [p0.x, p0.y, 1.0, 1.0]}))?;
            deckcraft_model::ShapeId(r.get("id").and_then(Value::as_u64).ok_or_else(|| bad("shape.connect", "insert failed"))? as u32)
        }
    };
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.connect", "no slide"))?;
        let sh = deckcraft_model::find_shape_mut(list, id).ok_or_else(|| bad("shape.connect", "no such line"))?;
        if !sh.is_line() {
            return Err(bad("shape.connect", "`id` is not a line or connector"));
        }
        sh.xfrm = Some(crate::tools::line_xfrm(p0, p1, 0.0));
        sh.kind = deckcraft_model::ShapeKind::Connector { start: Some((from, ia as u32)), end: Some((to, ib as u32)) };
        Ok(())
    })?;
    Ok(json!({"id": id.0, "fromSite": ia, "toSite": ib}))
}

/// Path data through `pts` (shape-local): straight segments, or a Catmull-Rom curve as cubics.
pub fn freeform_path(pts: &[deckcraft_geom::Point], closed: bool, smooth: bool) -> String {
    let f = |v: f64| {
        let s = format!("{:.2}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let mut d = format!("M {} {}", f(pts[0].x), f(pts[0].y));
    let n = pts.len();
    if smooth && n > 2 {
        let at = |i: isize| -> deckcraft_geom::Point {
            if closed { pts[i.rem_euclid(n as isize) as usize] } else { pts[i.clamp(0, n as isize - 1) as usize] }
        };
        let segs = if closed { n } else { n - 1 };
        for i in 0..segs as isize {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let c1 = p1 + (p2 - p0) / 6.0;
            let c2 = p2 - (p3 - p1) / 6.0;
            d += &format!(" C {} {} {} {} {} {}", f(c1.x), f(c1.y), f(c2.x), f(c2.y), f(p2.x), f(p2.y));
        }
    } else {
        for q in &pts[1..] {
            d += &format!(" L {} {}", f(q.x), f(q.y));
        }
    }
    if closed {
        d += " Z";
    }
    d
}

fn freeform(s: &mut Session, p: &Value) -> Result<Value> {
    let pts: Vec<deckcraft_geom::Point> = p
        .get("points")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| Some(deckcraft_geom::Point::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?)))
                .filter(|q| q.x.is_finite() && q.y.is_finite())
                .take(20_000)
                .collect()
        })
        .unwrap_or_default();
    if pts.len() < 2 {
        return Err(bad("shape.freeform", "need at least two points"));
    }
    let closed = bool_or(p, "closed", false) && pts.len() > 2;
    let smooth = bool_or(p, "smooth", false);
    let mut bb = deckcraft_geom::Rect::from_points(pts[0], pts[0]);
    for q in &pts {
        bb = bb.union_pt(*q);
    }
    let (w, h) = (bb.width().max(1.0), bb.height().max(1.0));
    let local: Vec<deckcraft_geom::Point> = pts.iter().map(|q| deckcraft_geom::Point::new(q.x - bb.x0, q.y - bb.y0)).collect();
    let d = freeform_path(&local, closed, smooth);
    // Closed shapes take the default shape style, open paths the line style.
    let r = s.execute("shape.insert", &json!({"preset": if closed { "rect" } else { "line" }, "rect": [bb.x0, bb.y0, w, h]}))?;
    let id = deckcraft_model::ShapeId(r.get("id").and_then(Value::as_u64).ok_or_else(|| bad("shape.freeform", "insert failed"))? as u32);
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.freeform", "no slide"))?;
        let sh = deckcraft_model::find_shape_mut(list, id).ok_or_else(|| bad("shape.freeform", "insert failed"))?;
        sh.geom = Geom::Custom {
            paths: vec![deckcraft_model::CustomPath {
                w,
                h,
                d: d.clone(),
                fill: if closed { deckcraft_geom::preset::FillMode::Norm } else { deckcraft_geom::preset::FillMode::None },
                stroke: true,
            }],
        };
        sh.xfrm = Some(deckcraft_geom::Xfrm::new(bb.x0, bb.y0, w, h));
        sh.kind = deckcraft_model::ShapeKind::Shape;
        sh.name = format!("Freeform {}", id.0);
        Ok(())
    })?;
    Ok(json!({"id": id.0}))
}

#[cfg(test)]
mod size_position_tests {
    use serde_json::json;

    use crate::Session;

    fn deck() -> (Session, u32) {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 50.0, 200.0, 100.0]})).unwrap()["id"].as_u64().unwrap() as u32;
        (s, id)
    }

    fn geom(s: &mut Session, id: u32) -> (f64, f64, f64, f64, f64) {
        let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
        let b = &v["box"];
        (b["x"].as_f64().unwrap(), b["y"].as_f64().unwrap(), b["w"].as_f64().unwrap(), b["h"].as_f64().unwrap(), b["rot"].as_f64().unwrap_or(0.0))
    }

    fn undos(s: &Session) -> usize {
        s.doc().unwrap().history.undo.len()
    }

    #[test]
    fn sets_position_size_and_rotation_in_one_undo() {
        let (mut s, id) = deck();
        let n = undos(&s);
        s.execute("shape.sizePosition", &json!({"x": 80.0, "y": 90.0, "w": 240.0, "h": 60.0, "rot": 370.0})).unwrap();
        assert_eq!(geom(&mut s, id), (80.0, 90.0, 240.0, 60.0, 10.0));
        assert_eq!(undos(&s), n + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Size and Position…");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 200.0, 100.0, 0.0));
    }

    #[test]
    fn lock_aspect_and_scale_share_one_step() {
        let (mut s, id) = deck();
        s.execute("shape.sizePosition", &json!({"w": 300.0, "lockAspect": true})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 300.0, 150.0, 0.0));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("shape.sizePosition", &json!({"h": 50.0, "lockAspect": true})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 100.0, 50.0, 0.0));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("shape.sizePosition", &json!({"scaleW": 25.0, "scaleH": 150.0})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 50.0, 150.0, 0.0));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("shape.sizePosition", &json!({"scaleH": 50.0, "lockAspect": true})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 100.0, 50.0, 0.0));
        // An explicit width wins over a scale on the same axis.
        s.execute("shape.sizePosition", &json!({"w": 80.0, "scaleW": 50.0})).unwrap();
        assert_eq!(geom(&mut s, id).2, 80.0);
    }

    #[test]
    fn partial_fields_leave_the_rest_and_a_repeat_does_not_undo() {
        let (mut s, id) = deck();
        let n = undos(&s);
        s.execute("shape.sizePosition", &json!({"y": 12.0, "rot": -90.0})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 12.0, 200.0, 100.0, 270.0));
        assert_eq!(undos(&s), n + 1);
        let n = undos(&s);
        s.execute("shape.sizePosition", &json!({"x": 40.0, "y": 12.0, "w": 200.0, "h": 100.0, "rot": 270.0})).unwrap();
        assert_eq!(undos(&s), n, "identical geometry is not an undo step");
    }

    #[test]
    fn no_fields_opens_the_dialog_without_editing() {
        let (mut s, id) = deck();
        let n = undos(&s);
        s.execute("shape.sizePosition", &json!({})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 200.0, 100.0, 0.0));
        assert_eq!(undos(&s), n);
        let req = s.ui_requests.last().unwrap();
        match req {
            crate::UiRequest::Dialog { id, params } => {
                assert_eq!(id, "sizePosition");
                assert_eq!(params["w"], 200.0);
                assert_eq!(params["h"], 100.0);
                assert_eq!(params["x"], 40.0);
                assert_eq!(params["rot"], 0.0);
                assert_eq!(params["scaleW"], 100.0);
                assert_eq!(params["lockAspect"], false);
            }
            other => panic!("expected a dialog, got {other:?}"),
        }
    }

    #[test]
    fn bad_fields_do_not_edit() {
        let (mut s, id) = deck();
        let n = undos(&s);
        for p in [
            json!({"w": "wide"}),
            json!({"h": -5.0}),
            json!({"scaleW": 0.0}),
            json!({"x": 1e9}),
            json!({"rot": "spin"}),
            json!({"lockAspect": "yes"}),
        ] {
            let err = s.execute("shape.sizePosition", &p).unwrap_err();
            assert!(err.to_string().contains("sizePosition"), "{err}");
            assert_eq!(geom(&mut s, id), (40.0, 50.0, 200.0, 100.0, 0.0));
            assert_eq!(undos(&s), n);
        }
    }

    #[test]
    fn scale_is_per_shape_and_nothing_selected_is_disabled() {
        let (mut s, a) = deck();
        let b = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [10.0, 10.0, 40.0, 80.0]})).unwrap()["id"].as_u64().unwrap() as u32;
        s.execute("shape.sizePosition", &json!({"ids": [a, b], "scaleW": 200.0})).unwrap();
        assert_eq!(geom(&mut s, a).2, 400.0);
        assert_eq!(geom(&mut s, a).3, 100.0);
        assert_eq!(geom(&mut s, b).2, 80.0);
        assert_eq!(geom(&mut s, b).3, 80.0);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Size and Position…");
        s.execute("edit.select", &json!({"ids": []})).unwrap();
        let err = s.execute("shape.sizePosition", &json!({})).unwrap_err();
        assert!(err.to_string().contains("nothing selected"), "{err}");
    }
}

/// Draw › Ink to Shape: each recognisable stroke of the slide's ink becomes a preset shape
/// outlined in the stroke's colour and width; the rest stays ink. One undo step.
fn ink_to_shape(s: &mut Session, _p: &Value) -> Result<Value> {
    use crate::inkshape::{Recognized, recognize};
    use deckcraft_model::{Shape, ShapeId, ShapeKind};
    let shapes = s.doc()?.shapes();
    let Some(ink) = shapes.iter().find(|sh| matches!(sh.kind, ShapeKind::Ink { .. })) else {
        return Ok(json!({"shapes": [], "kept": 0}));
    };
    let (ink_id, off) = (ink.id, ink.xfrm.map(|x| (x.x, x.y)).unwrap_or((0.0, 0.0)));
    let ShapeKind::Ink { strokes } = &ink.kind else { return Ok(json!({"shapes": [], "kept": 0})) };
    let found: Vec<Option<Recognized>> = strokes
        .iter()
        .map(|st| if st.highlighter { None } else { recognize(&st.points.iter().map(|(x, y, _)| (x + off.0, y + off.1)).collect::<Vec<_>>()) })
        .collect();
    if found.iter().all(Option::is_none) {
        return Ok(json!({"shapes": [], "kept": strokes.len()}));
    }
    s.edit(|doc, sel| {
        let mut new = vec![];
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("draw.inkToShape", "no slide"))?;
        let Some(pos) = list.iter().position(|sh| sh.id == ink_id) else { return Err(bad("draw.inkToShape", "ink gone")) };
        let ShapeKind::Ink { strokes } = &mut list[pos].kind else { return Err(bad("draw.inkToShape", "not ink")) };
        let all = std::mem::take(strokes);
        let mut kept = vec![];
        let mut made = vec![];
        for (st, r) in all.into_iter().zip(found.iter()) {
            let Some(r) = r else {
                kept.push(st);
                continue;
            };
            let (preset, xfrm) = match *r {
                Recognized::Line { x0, y0, x1, y1 } => {
                    ("line", crate::tools::line_xfrm(deckcraft_geom::Point::new(x0, y0), deckcraft_geom::Point::new(x1, y1), 0.0))
                }
                Recognized::Rect { x, y, w, h } => ("rect", deckcraft_geom::Xfrm::new(x, y, w, h)),
                Recognized::Ellipse { x, y, w, h } => ("ellipse", deckcraft_geom::Xfrm::new(x, y, w, h)),
                Recognized::Triangle { x, y, w, h, flip_v } => ("triangle", deckcraft_geom::Xfrm { flip_v, ..deckcraft_geom::Xfrm::new(x, y, w, h) }),
            };
            made.push((preset, xfrm, st.color, st.width));
        }
        let remaining = kept.len();
        *strokes = kept;
        if remaining == 0 {
            list.remove(pos);
        }
        for (preset, xfrm, color, width) in made {
            let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("draw.inkToShape", NO_ID_SPACE))?);
            let name = format!(
                "{} {}",
                match preset {
                    "line" => "Straight Connector",
                    "rect" => "Rectangle",
                    "ellipse" => "Oval",
                    _ => "Isosceles Triangle",
                },
                id.0
            );
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("draw.inkToShape", "no slide"))?;
            list.push(Shape {
                id,
                name,
                xfrm: Some(xfrm),
                geom: Geom::preset(preset),
                fill: Some(Fill::None),
                line: Some(Line::solid(ColorRef::rgb(color), width.clamp(0.25, 100.0))),
                kind: ShapeKind::Shape,
                ..Default::default()
            });
            new.push(id);
        }
        sel.shapes = new.clone();
        Ok(json!({"shapes": new.iter().map(|i| i.0).collect::<Vec<_>>(), "kept": remaining}))
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use deckcraft_model::ShapeId;

    use super::*;
    use serde_json::json;

    fn nested_group() -> (Session, Vec<ShapeId>, ShapeId, ShapeId) {
        let mut s = Session::with_new();
        let mut ids = vec![];
        for i in 0..4 {
            let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40 + i * 100, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();
            s.execute("shape.line", &json!({"width": i + 1, "dash": "dash"})).unwrap();
            ids.push(ShapeId(id as u32));
        }
        let group = |s: &mut Session, ids: &[ShapeId]| {
            s.execute("edit.select", &json!({"ids": ids})).unwrap();
            ShapeId(s.execute("arrange.group", &json!({})).unwrap()["id"].as_u64().unwrap() as u32)
        };
        let inner = group(&mut s, &ids[..2]);
        let outer = group(&mut s, &[inner, ids[2]]);
        (s, ids[..3].to_vec(), outer, ids[3])
    }

    #[test]
    fn group_fill_reaches_nested_members_in_one_undo_step() {
        let (mut s, members, group, outside) = nested_group();
        let before = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
        let untouched = serde_json::to_value(s.doc().unwrap().shape(outside)).unwrap();
        s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
        for id in &members {
            assert_eq!(s.doc().unwrap().shape(*id).unwrap().fill, Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 0, 0)))));
        }
        assert_eq!(s.doc().unwrap().selection.shapes, vec![group]);
        assert_eq!(serde_json::to_value(s.doc().unwrap().shape(outside)).unwrap(), untouched);
        let filled = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&s.doc().unwrap().doc).unwrap(), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&s.doc().unwrap().doc).unwrap(), filled);

        s.execute("shape.fill", &json!({"none": true})).unwrap();
        assert!(members.iter().all(|id| s.doc().unwrap().shape(*id).unwrap().fill == Some(Fill::None)));
        s.execute("shape.fill", &json!({"reset": true})).unwrap();
        assert!(members.iter().all(|id| s.doc().unwrap().shape(*id).unwrap().fill.is_none()));
    }

    #[test]
    fn group_outline_reaches_members_without_resetting_their_other_properties() {
        let (mut s, members, group, outside) = nested_group();
        let before = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
        // Explicit ids work even when another object is selected.
        s.execute("edit.select", &json!({"ids": [outside]})).unwrap();
        s.execute("shape.line", &json!({"ids": [group], "color": "#00ff00"})).unwrap();
        for (i, id) in members.iter().enumerate() {
            let line = s.doc().unwrap().shape(*id).unwrap().line.as_ref().unwrap();
            assert_eq!(line.fill, Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 255, 0)))));
            assert_eq!(line.width, Some((i + 1) as f64));
            assert_eq!(line.dash, Some(Dash::Preset { name: "dash".into() }));
        }
        assert_eq!(s.doc().unwrap().selection.shapes, vec![outside]);
        let outlined = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&s.doc().unwrap().doc).unwrap(), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&s.doc().unwrap().doc).unwrap(), outlined);
        s.execute("shape.line", &json!({"ids": [group], "none": true})).unwrap();
        assert!(members.iter().all(|id| s.doc().unwrap().shape(*id).unwrap().line.as_ref().unwrap().fill == Some(Fill::None)));
        s.execute("shape.line", &json!({"ids": [group], "reset": true})).unwrap();
        assert!(members.iter().all(|id| s.doc().unwrap().shape(*id).unwrap().line.is_none()));
    }

    #[test]
    fn group_fill_transparency_keeps_each_members_inherited_colour() {
        let (mut s, members, group, _) = nested_group();
        s.execute("shape.fill", &json!({"ids": [members[1]], "color": "#00ff00"})).unwrap();
        let colours: Vec<_> = {
            let st = s.doc().unwrap();
            let ctx = super::super::text::ctx_for(&st.doc, &st.selection).unwrap();
            members
                .iter()
                .map(|id| {
                    let (fill, ph) = deckcraft_model::resolve::fill(&ctx, st.shape(*id).unwrap());
                    let Some(Fill::Solid { color }) = fill else { panic!("solid theme fill") };
                    ctx.color(&color, ph)
                })
                .collect()
        };
        s.execute("shape.fill", &json!({"ids": [group], "transparency": 0.5})).unwrap();
        for (id, colour) in members.iter().zip(colours) {
            let mut expected = ColorRef::rgb(colour);
            expected.mods.push(ColorTransform::Alpha(50000));
            assert_eq!(s.doc().unwrap().shape(*id).unwrap().fill, Some(Fill::solid(expected)));
        }
    }

    fn rotation(s: &Session) -> f64 {
        let st = s.active().unwrap();
        let id = st.selection.shapes[0];
        st.shape(id).unwrap().xfrm.unwrap().rot
    }

    #[test]
    fn a_twist_is_one_undo_and_wraps_past_a_full_turn() {
        let mut s = Session::with_new();
        s.execute("edit.select", &json!({"ids": []})).unwrap();
        let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 80.0, 40.0]})).unwrap();
        let id = id.get("id").and_then(Value::as_u64).unwrap();
        s.execute("edit.select", &json!({"ids": [id]})).unwrap();
        assert!((rotation(&s) - 0.0).abs() < 0.01);

        s.execute("shape.rotate", &json!({"by": 10.0, "gesture": "move"})).unwrap();
        s.execute("shape.rotate", &json!({"by": 350.0, "gesture": "move"})).unwrap();
        assert!((rotation(&s) - 0.0).abs() < 0.01, "10 + 350 wraps to 0, got {}", rotation(&s));
        s.execute("shape.rotate", &json!({"by": 20.0, "gesture": "move"})).unwrap();
        s.execute("shape.rotate", &json!({"gesture": "end", "ids": []})).unwrap();
        assert!((rotation(&s) - 20.0).abs() < 0.01);
        assert_eq!(s.active().unwrap().history.undo.last().unwrap().label, "Rotation");
        assert!(s.active().unwrap().interaction.is_none());

        s.execute("edit.undo", &json!({})).unwrap();
        assert!((rotation(&s) - 0.0).abs() < 0.01, "one undo undoes the whole twist");
        s.execute("edit.redo", &json!({})).unwrap();
        assert!((rotation(&s) - 20.0).abs() < 0.01);
    }

    #[test]
    fn ending_a_twist_does_not_commit_a_drag() {
        let mut s = Session::with_new();
        s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [10.0, 10.0, 30.0, 30.0]})).unwrap();
        let st = s.active_mut().unwrap();
        st.interaction = Some(crate::Interaction { label: "Move".into(), doc: st.doc.clone(), selection: st.selection.clone(), vertex_kind: None });
        s.execute("shape.rotate", &json!({"gesture": "end", "ids": []})).unwrap();
        assert_eq!(s.active().unwrap().interaction.as_ref().unwrap().label, "Move");
    }
}
