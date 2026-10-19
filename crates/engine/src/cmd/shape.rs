//! Shape Format: position and size, rotation, flips, geometry, fill, outline, effects, styles,
//! names and alt text.

use deckcraft_color::{ColorTransform, Rgba, SchemeSlot};
use deckcraft_model::style::{Dash, Glow, Gradient, GradientShape, GradientStop, LineEnd, PatternFill, PictureFill, PictureMode, Reflection, Shadow};
use deckcraft_model::{ColorRef, Effects, Fill, Geom, Line, ShapeStyle};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

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
            "{color? | none?: true | gradient?: {stops: [[pos, color]], angle?, kind?: linear|radial} | picture?: base64 | pattern?: {preset, fg, bg} | transparency?: 0..1 | reset?: true, ids?}; pictureProps?: {tile?: bool, offsetX?, offsetY?: pt, scaleX?, scaleY?: 0..10, align?: tl|t|tr|l|ctr|r|bl|b|br, mirror?: none|x|y|xy, stretch?: [l, t, r, b] fractions, rotateWithShape?: bool} adjusts a picture fill",
            has_selection,
            fill
        ),
        cmd!(
            "shape.line",
            "Shape Outline",
            ["Shape Format", "Shape Styles"],
            None,
            "{color?, none?, width?: pt, dash?: solid|dot|dash|lgDash|dashDot|sysDot|sysDash, cap?: flat|round|square, join?: round|bevel|miter, compound?: single|double|thickThin|thinThick|triple, head?: none|triangle|stealth|diamond|oval|arrow, tail?, headSize?: sm|med|lg, tailSize?, reset?, ids?}",
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
        cmd!("picture.reset", "Reset Picture", ["Picture Format", "Adjust"], None, "{ids?}", has_selection, picture_reset),
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

/// Keeps a side length in `0..=MAX_PT` so a huge value cannot exhaust memory when rendering.
/// An overflow to infinity lands on the limit; NaN becomes 0.
fn clamp_side(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, MAX_PT) }
}

/// Keeps a coordinate within `±POS_LIMIT`.
fn clamp_pos(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(-POS_LIMIT, POS_LIMIT) }
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

const SIZE_POS: &str = "shape.sizePosition";
/// Smallest and largest side the dialog will write, in points (220 in).
const MIN_PT: f64 = 0.01;
const MAX_PT: f64 = 15_840.0;
const POS_LIMIT: f64 = 100_000.0;
const MAX_SCALE: f64 = 10_000.0;

#[derive(Clone, Copy)]
struct SizeSpec {
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    rot: Option<f64>,
    scale_w: Option<f64>,
    scale_h: Option<f64>,
    lock: bool,
}

impl SizeSpec {
    fn width_set(self) -> bool {
        self.w.is_some() || self.scale_w.is_some()
    }
    fn height_set(self) -> bool {
        self.h.is_some() || self.scale_h.is_some()
    }
    fn any(self) -> bool {
        self.x.is_some() || self.y.is_some() || self.width_set() || self.height_set() || self.rot.is_some()
    }
}

fn finite_key(p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(bad(SIZE_POS, format!("`{key}` must be a finite number"))),
        },
    }
}

fn in_range(v: Option<f64>, key: &str, lo: f64, hi: f64) -> Result<Option<f64>> {
    if let Some(n) = v
        && !(lo..=hi).contains(&n)
    {
        return Err(bad(SIZE_POS, format!("`{key}` is out of range")));
    }
    Ok(v)
}

fn size_spec(p: &Value) -> Result<SizeSpec> {
    let lock = match p.get("lockAspect") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(SIZE_POS, "`lockAspect` must be true or false")),
    };
    Ok(SizeSpec {
        x: in_range(finite_key(p, "x")?, "x", -POS_LIMIT, POS_LIMIT)?,
        y: in_range(finite_key(p, "y")?, "y", -POS_LIMIT, POS_LIMIT)?,
        w: in_range(finite_key(p, "w")?, "w", MIN_PT, MAX_PT)?,
        h: in_range(finite_key(p, "h")?, "h", MIN_PT, MAX_PT)?,
        rot: finite_key(p, "rot")?,
        scale_w: in_range(finite_key(p, "scaleW")?, "scaleW", f64::MIN_POSITIVE, MAX_SCALE)?,
        scale_h: in_range(finite_key(p, "scaleH")?, "scaleH", f64::MIN_POSITIVE, MAX_SCALE)?,
        lock,
    })
}

fn norm_deg(r: f64) -> f64 {
    let r = r % 360.0;
    if r < 0.0 { r + 360.0 } else { r }
}

/// Width wins when both sides are set with the aspect locked, matching `shape.resize`.
fn apply_size(x: &mut deckcraft_geom::Xfrm, spec: SizeSpec) {
    let ar = if x.h > 0.0 { x.w / x.h } else { 1.0 };
    let mut nw = x.w;
    let mut nh = x.h;
    if let Some(p) = spec.scale_w {
        nw = x.w * p / 100.0;
    }
    if let Some(p) = spec.scale_h {
        nh = x.h * p / 100.0;
    }
    if let Some(w) = spec.w {
        nw = w;
    }
    if let Some(h) = spec.h {
        nh = h;
    }
    if spec.lock && (spec.width_set() || spec.height_set()) {
        if spec.width_set() {
            nh = if ar > 0.0 { nw / ar } else { nh };
        } else {
            nw = nh * ar;
        }
    }
    x.w = nw.clamp(MIN_PT, MAX_PT);
    x.h = nh.clamp(MIN_PT, MAX_PT);
    if let Some(px) = spec.x {
        x.x = px;
    }
    if let Some(py) = spec.y {
        x.y = py;
    }
    if let Some(r) = spec.rot {
        x.rot = norm_deg(r);
    }
}

fn size_position(s: &mut Session, p: &Value) -> Result<Value> {
    let spec = size_spec(p)?;
    if !spec.any() {
        let (x, y, w, h, rot) = {
            let st = s.doc()?;
            let id = targets(s, p)?.first().copied().ok_or_else(|| bad(SIZE_POS, "nothing selected"))?;
            let sh = st.shape(id).ok_or_else(|| bad(SIZE_POS, "no such shape"))?;
            let x = xfrm_of(&st.doc, &st.selection, sh);
            (x.x, x.y, x.w, x.h, x.rot)
        };
        s.ui_requests.push(crate::UiRequest::Dialog {
            id: "sizePosition".into(),
            params: json!({"x": x, "y": y, "w": w, "h": h, "rot": rot, "scaleW": 100.0, "scaleH": 100.0, "lockAspect": false}),
        });
        return Ok(Value::Null);
    }
    let changed = {
        let ids = targets(s, p)?;
        let st = s.doc()?;
        let free: Vec<_> = ids.iter().copied().filter(|id| !st.shape(*id).is_some_and(|sh| sh.locked)).collect::<Vec<_>>();
        if !ids.is_empty() && free.is_empty() {
            return Err(bad(SIZE_POS, "the object is locked"));
        }
        free.iter().any(|id| {
            st.shape(*id).is_some_and(|sh| {
                let mut x = xfrm_of(&st.doc, &st.selection, sh);
                let before = x;
                apply_size(&mut x, spec);
                x != before
            })
        })
    };
    if !changed {
        return Ok(Value::Null);
    }
    edit_geometry(s, p, SIZE_POS, |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            apply_size(x, spec);
        }
        Ok(())
    })
}

fn rotate(s: &mut Session, p: &Value) -> Result<Value> {
    // A trackpad twist sends many small steps. `gesture: move` keeps them as one undo;
    // `gesture: end` commits that step and does not rotate again.
    if p.get("gesture").and_then(Value::as_str) == Some("end") {
        s.commit_named("Rotation");
        return Ok(json!({}));
    }
    if p.get("gesture").and_then(Value::as_str) == Some("move") {
        s.begin("Rotation");
    }
    let (abs, by) = (f64_param(p, "deg"), f64_param(p, "by"));
    edit_geometry(s, p, "shape.rotate", |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            let r = abs.unwrap_or(x.rot) + by.unwrap_or(0.0);
            x.rot = ((r % 360.0) + 360.0) % 360.0;
        }
        Ok(())
    })
}

fn flip(s: &mut Session, p: &Value) -> Result<Value> {
    let v = str_param(p, "axis").unwrap_or("horizontal").starts_with('v');
    edit_geometry(s, p, "shape.flip", |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            if v {
                x.flip_v = !x.flip_v;
            } else {
                x.flip_h = !x.flip_h;
            }
            // `Xfrm::affine` flips first and rotates second; a true mirror also negates the rotation.
            x.rot = (360.0 - x.rot_norm()) % 360.0;
        }
        Ok(())
    })
}

fn adjust(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").unwrap_or(0).min(16);
    let v = f64_param(p, "value").ok_or_else(|| bad("shape.adjust", "missing `value`"))?;
    edit_geometry(s, p, "shape.adjust", |sh| {
        if let Geom::Preset { name, adj } = &mut sh.geom {
            let defaults = deckcraft_geom::preset::info(name).map(|x| x.defaults).unwrap_or(&[]);
            while adj.len() <= i {
                let k = adj.len();
                adj.push(defaults.get(k).copied().unwrap_or(0.0));
            }
            if let Some(slot) = adj.get_mut(i) {
                *slot = v;
            }
        }
        Ok(())
    })
}

/// Rectangles and rounded rectangles become custom geometry whose corners are smoothed
/// ([`deckcraft_geom::squircle`]); the radius is the rounded rectangle's own unless `radius` says.
fn smooth_corners(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "shape.smoothCorners";
    let smoothing =
        f64_param(p, "smoothing").filter(|v| v.is_finite()).ok_or_else(|| bad(CMD, "missing `smoothing` (0–100)"))?.clamp(0.0, 100.0) / 100.0;
    let radius = f64_param(p, "radius").filter(|v| v.is_finite()).map(|v| v.max(0.0));
    edit_geometry(s, p, CMD, |sh| {
        let Some(x) = sh.xfrm else { return Err(bad(CMD, "the shape has no size of its own")) };
        let ss = x.w.min(x.h).max(0.0);
        let own = match &sh.geom {
            Geom::Preset { name, adj } if name == "roundRect" => Some(ss * adj.first().copied().unwrap_or(16667.0).clamp(0.0, 50000.0) / 100000.0),
            Geom::Preset { name, .. } if name == "rect" => Some(ss * 16667.0 / 100000.0),
            _ => None,
        };
        // A custom outline qualifies only when it is a squircle this command drew with `radius`
        // (re-smoothing as the slider moves), never an arbitrary freeform.
        let custom = match (&sh.geom, radius) {
            (Geom::Custom { paths }, Some(r)) => match paths.as_slice() {
                [only] => (0..=100).any(|k| deckcraft_geom::squircle::path_d(only.w, only.h, r, f64::from(k) / 100.0) == only.d),
                _ => false,
            },
            _ => false,
        };
        let r = match (radius, own) {
            (Some(r), _) if own.is_some() || custom => r,
            (None, Some(r)) => r,
            _ => return Err(bad(CMD, "only rectangles, rounded rectangles and shapes already smoothed with this `radius` can be smoothed")),
        };
        sh.geom = Geom::Custom {
            paths: vec![deckcraft_model::CustomPath {
                w: x.w,
                h: x.h,
                d: deckcraft_geom::squircle::path_d(x.w, x.h, r, smoothing),
                ..Default::default()
            }],
        };
        Ok(())
    })
}

fn change(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "preset").ok_or_else(|| bad("shape.change", "missing `preset`"))?;
    let info = deckcraft_geom::preset::info(name).ok_or_else(|| bad("shape.change", format!("unknown preset `{name}`")))?;
    edit_geometry(s, p, "shape.change", |sh| {
        sh.geom = Geom::preset(info.name);
        Ok(())
    })
}

/// `{stops: [[pos 0–1, color], …], kind?: linear|radial|rectangular|path, angle?: deg (linear),
/// focus?: [l, t, r, b] fractions (radial/rectangular/path: where the first stop sits),
/// rotateWithShape?}`.
pub(crate) fn gradient_param(v: &Value) -> Option<Gradient> {
    let stops: Vec<GradientStop> = v
        .get("stops")?
        .as_array()?
        .iter()
        .filter_map(|s| {
            let a = s.as_array()?;
            Some(GradientStop { pos: a.first()?.as_f64()?.clamp(0.0, 1.0), color: color_value(a.get(1)?)? })
        })
        .collect();
    if stops.len() < 2 {
        return None;
    }
    let focus = v
        .get("focus")
        .and_then(Value::as_array)
        .and_then(|a| {
            let f = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite()).map(|x| x.clamp(0.0, 1.0));
            Some([f(0)?, f(1)?, f(2)?, f(3)?])
        })
        .unwrap_or([0.5, 0.5, 0.5, 0.5]);
    let mut stops = stops;
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    let path = |p: &str| GradientShape::Path { path: p.into(), focus };
    let shape = match v.get("kind").and_then(Value::as_str) {
        Some("radial" | "circle") => path("circle"),
        Some("rectangular" | "rect") => path("rect"),
        Some("path" | "shape") => path("shape"),
        _ => GradientShape::Linear {
            angle: v.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()).unwrap_or(90.0).rem_euclid(360.0),
            scaled: false,
        },
    };
    Some(Gradient { tile: [0.0; 4], stops, shape, rotate_with_shape: v.get("rotateWithShape").and_then(Value::as_bool).unwrap_or(true) })
}

fn fill(s: &mut Session, p: &Value) -> Result<Value> {
    let new_fill: Option<Option<Fill>> = if bool_or(p, "reset", false) {
        Some(None)
    } else if bool_or(p, "none", false) {
        Some(Some(Fill::None))
    } else if let Some(c) = color_param(p, "color") {
        Some(Some(Fill::solid(c)))
    } else if let Some(g) = p.get("gradient") {
        Some(Some(Fill::Gradient(
            gradient_param(g).ok_or_else(|| bad("shape.fill", "gradient needs at least two `stops`: [[0, \"#fff\"], [1, \"#000\"]]"))?,
        )))
    } else if let Some(pt) = p.get("pattern") {
        Some(Some(Fill::Pattern(PatternFill {
            preset: pt.get("preset").and_then(Value::as_str).unwrap_or("pct50").to_string(),
            fg: pt.get("fg").and_then(color_value).unwrap_or(ColorRef::scheme(SchemeSlot::Accent1)),
            bg: pt.get("bg").and_then(color_value).unwrap_or(ColorRef::scheme(SchemeSlot::Bg1)),
        })))
    } else if p.get("picture").is_some() {
        let (name, bytes) = super::insert::media_bytes(&with_param(p, "data", p.get("picture").cloned().unwrap_or_default()), "shape.fill")?;
        let ct = super::insert::content_type(&name, &bytes);
        let media = s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad("shape.fill", NO_ID_SPACE)))?;
        Some(Some(Fill::Picture(PictureFill { media, ..Default::default() })))
    } else {
        None
    };
    let transparency = f64_param(p, "transparency");
    let pic_props = p.get("pictureProps").filter(|v| v.is_object());
    if new_fill.is_none() && transparency.is_none() && pic_props.is_none() {
        return Err(bad("shape.fill", "give `color`, `none`, `gradient`, `pattern`, `picture`, `pictureProps`, `transparency` or `reset`"));
    }
    // Transparency on an inherited (style) fill needs the effective colour.
    let st = s.doc()?;
    let ctx_fill: Vec<(deckcraft_model::ShapeId, Option<Fill>)> = {
        let ctx = super::text::ctx_for(&st.doc, &st.selection);
        targets(s, p)?
            .into_iter()
            .filter_map(|id| {
                st.shape(id).map(|sh| {
                    (
                        id,
                        ctx.as_ref().and_then(|c| {
                            let (f, ph) = deckcraft_model::resolve::fill(c, sh);
                            f.map(|f| match f {
                                Fill::Solid { color } => Fill::solid(ColorRef::rgb(c.color(&color, ph))),
                                other => other,
                            })
                        }),
                    )
                })
            })
            .collect()
    };
    edit_shapes(s, p, "shape.fill", |sh| {
        if let Some(f) = &new_fill {
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
            let base = sh.fill.clone().or_else(|| ctx_fill.iter().find(|(i, _)| *i == sh.id).and_then(|(_, f)| f.clone()));
            if let Some(Fill::Solid { mut color }) = base {
                color.mods.retain(|m| !matches!(m, ColorTransform::Alpha(_)));
                if t > 0.0 {
                    color.mods.push(ColorTransform::Alpha(((1.0 - t.clamp(0.0, 1.0)) * 100_000.0).round() as i32));
                }
                sh.fill = Some(Fill::Solid { color });
            }
        }
        Ok(())
    })
}

/// Applies Format Shape ▸ Picture or texture fill options to a picture fill.
fn picture_props(pf: &mut PictureFill, pp: &Value) {
    let f = |k: &str| pp.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    if let Some(up) = pp.get("rotateWithShape").and_then(Value::as_bool) {
        pf.upright = !up;
    }
    match pp.get("tile").and_then(Value::as_bool) {
        Some(true) if !matches!(pf.mode, PictureMode::Tile { .. }) => {
            pf.mode = PictureMode::Tile { tx: 0.0, ty: 0.0, sx: 1.0, sy: 1.0, flip: "none".into(), align: "tl".into() };
        }
        Some(false) if !matches!(pf.mode, PictureMode::Stretch { .. }) => pf.mode = PictureMode::default(),
        _ => {}
    }
    match &mut pf.mode {
        PictureMode::Tile { tx, ty, sx, sy, flip, align } => {
            if let Some(v) = f("offsetX") {
                *tx = v.clamp(-4000.0, 4000.0);
            }
            if let Some(v) = f("offsetY") {
                *ty = v.clamp(-4000.0, 4000.0);
            }
            if let Some(v) = f("scaleX") {
                *sx = v.clamp(0.01, 10.0);
            }
            if let Some(v) = f("scaleY") {
                *sy = v.clamp(0.01, 10.0);
            }
            if let Some(m) = pp.get("mirror").and_then(Value::as_str).filter(|m| matches!(*m, "none" | "x" | "y" | "xy")) {
                *flip = m.to_string();
            }
            if let Some(a) =
                pp.get("align").and_then(Value::as_str).filter(|a| matches!(*a, "tl" | "t" | "tr" | "l" | "ctr" | "r" | "bl" | "b" | "br"))
            {
                *align = a.to_string();
            }
        }
        PictureMode::Stretch { fill_rect } => {
            if let Some(arr) = pp.get("stretch").and_then(Value::as_array) {
                for (slot, v) in fill_rect.iter_mut().zip(arr) {
                    if let Some(v) = v.as_f64().filter(|v| v.is_finite()) {
                        *slot = v.clamp(-10.0, 0.99);
                    }
                }
            }
        }
    }
}

fn line_end(v: &str) -> Option<LineEnd> {
    if v == "none" { None } else { Some(LineEnd { kind: v.to_string(), w: "med".into(), len: "med".into() }) }
}

fn line(s: &mut Session, p: &Value) -> Result<Value> {
    let reset = bool_or(p, "reset", false);
    let none = bool_or(p, "none", false);
    let color = color_param(p, "color");
    let width = f64_param(p, "width").map(|w| w.clamp(0.0, 1584.0));
    let dash = str_param(p, "dash").map(|d| Dash::Preset { name: d.to_string() });
    let head = str_param(p, "head").map(line_end);
    let tail = str_param(p, "tail").map(line_end);
    let cap = str_param(p, "cap").map(|c| match c {
        "round" => deckcraft_model::style::LineCap::Round,
        "square" => deckcraft_model::style::LineCap::Square,
        _ => deckcraft_model::style::LineCap::Flat,
    });
    let compound = str_param(p, "compound").map(|c| match c {
        "double" => deckcraft_model::style::Compound::Double,
        "thickThin" => deckcraft_model::style::Compound::ThickThin,
        "thinThick" => deckcraft_model::style::Compound::ThinThick,
        "triple" => deckcraft_model::style::Compound::Triple,
        _ => deckcraft_model::style::Compound::Single,
    });
    let end_size = |k: &str| str_param(p, k).filter(|v| matches!(*v, "sm" | "med" | "lg")).map(str::to_string);
    let (head_size, tail_size) = (end_size("headSize"), end_size("tailSize"));
    let join = str_param(p, "join").map(|c| match c {
        "bevel" => deckcraft_model::style::LineJoin::Bevel,
        "miter" => deckcraft_model::style::LineJoin::Miter,
        _ => deckcraft_model::style::LineJoin::Round,
    });
    edit_shapes(s, p, "shape.line", |sh| {
        if reset {
            sh.line = None;
            return Ok(());
        }
        let l = sh.line.get_or_insert_with(Line::default);
        if none {
            l.fill = Some(Fill::None);
        }
        if let Some(c) = &color {
            l.fill = Some(Fill::solid(c.clone()));
        }
        if width.is_some() {
            l.width = width;
            if l.fill.is_none() || matches!(l.fill, Some(Fill::None)) && !none {
                l.fill = Some(Fill::solid(ColorRef::scheme(SchemeSlot::Tx1)));
            }
        }
        if dash.is_some() {
            l.dash.clone_from(&dash);
        }
        let keep_size = |old: &Option<LineEnd>, new: &Option<LineEnd>| {
            new.clone().map(|mut n| {
                if let Some(o) = old {
                    n.w.clone_from(&o.w);
                    n.len.clone_from(&o.len);
                }
                n
            })
        };
        if let Some(h) = &head {
            l.head = keep_size(&l.head, h);
        }
        if let Some(t) = &tail {
            l.tail = keep_size(&l.tail, t);
        }
        if cap.is_some() {
            l.cap = cap;
        }
        if join.is_some() {
            l.join = join;
        }
        if compound.is_some() {
            l.compound = compound;
        }
        // Arrowhead size (width and length together), kept when the kind changes.
        for (end, size) in [(&mut l.head, &head_size), (&mut l.tail, &tail_size)] {
            if let (Some(e), Some(sz)) = (end.as_mut(), size) {
                e.w.clone_from(sz);
                e.len.clone_from(sz);
            }
        }
        Ok(())
    })
}

fn shadow_preset(v: &Value) -> Option<Option<Shadow>> {
    let black = |a: i32| ColorRef::rgb(Rgba::BLACK).with(ColorTransform::Alpha(a));
    let base = |inner: bool| Shadow {
        color: black(40000),
        blur: 4.0,
        dist: 3.0,
        dir: 45.0,
        inner,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: "tl".into(),
        rotate_with_shape: false,
    };
    match v {
        Value::String(s) => Some(match s.as_str() {
            "none" => None,
            "inner" => Some(base(true)),
            "perspective" => Some(Shadow { sy: 0.23, kx: -1200000.0 / 60000.0, dir: 90.0, dist: 0.0, blur: 6.0, align: "bl".into(), ..base(false) }),
            "bottom" => Some(Shadow { dir: 90.0, ..base(false) }),
            "center" => Some(Shadow { dist: 0.0, blur: 8.0, ..base(false) }),
            _ => Some(base(false)),
        }),
        Value::Object(o) => Some(Some(Shadow {
            color: o.get("color").and_then(color_value).unwrap_or(black(40000)),
            blur: o.get("blur").and_then(Value::as_f64).unwrap_or(4.0).clamp(0.0, 200.0),
            dist: o.get("dist").and_then(Value::as_f64).unwrap_or(3.0).clamp(0.0, 200.0),
            dir: o.get("dir").and_then(Value::as_f64).unwrap_or(45.0),
            inner: o.get("inner").and_then(Value::as_bool).unwrap_or(false),
            ..base(false)
        })),
        _ => None,
    }
}

fn effects(s: &mut Session, p: &Value) -> Result<Value> {
    let reset = bool_or(p, "reset", false);
    let shadow = p.get("shadow").and_then(shadow_preset);
    let glow: Option<Option<Glow>> = p.get("glow").map(|g| match g {
        Value::Object(o) => Some(Glow {
            color: o.get("color").and_then(color_value).unwrap_or(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(40000))),
            radius: o.get("radius").and_then(Value::as_f64).unwrap_or(8.0).clamp(0.0, 150.0),
        }),
        Value::Number(n) => Some(Glow {
            color: ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(40000)),
            radius: n.as_f64().unwrap_or(8.0).clamp(0.0, 150.0),
        }),
        _ => None,
    });
    let soft = p.get("softEdges").map(|v| v.as_f64().filter(|x| *x > 0.0).map(|x| x.min(100.0)));
    let refl = str_param(p, "reflection").map(|r| match r {
        "none" => None,
        "half" => Some(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.55, dist: 4.0 }),
        "full" => Some(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.9, dist: 4.0 }),
        _ => Some(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.35, dist: 0.0 }),
    });
    let three_d = p.get("threeD").filter(|v| v.is_object());
    let bevel = str_param(p, "bevel").map(|b| if b == "none" { None } else { Some(b.to_string()) });
    let shadow_props = p.get("shadowProps").filter(|v| v.is_object()).cloned();
    let glow_props = p.get("glowProps").filter(|v| v.is_object()).cloned();
    let refl_props = p.get("reflectionProps").filter(|v| v.is_object()).cloned();
    edit_shapes(s, p, "shape.effects", |sh| {
        if reset {
            sh.effects = None;
            return Ok(());
        }
        let e = sh.effects.get_or_insert_with(Effects::default);
        if let Some(sw) = &shadow {
            match sw {
                Some(x) if x.inner => {
                    e.inner_shadow = Some(x.clone());
                    e.outer_shadow = None;
                }
                Some(x) => {
                    e.outer_shadow = Some(x.clone());
                    e.inner_shadow = None;
                }
                None => {
                    e.outer_shadow = None;
                    e.inner_shadow = None;
                }
            }
        }
        if let Some(g) = &glow {
            e.glow.clone_from(g);
        }
        if let Some(sf) = soft {
            e.soft_edge = sf;
        }
        if let Some(r) = &refl {
            e.reflection.clone_from(r);
        }
        if bevel.is_some() || three_d.is_some() {
            let mut s3 = e.raw3d.as_deref().map(super::three_d::Shape3d::parse).unwrap_or_default();
            if let Some(b) = &bevel {
                s3.apply(&json!({"bevelTop": b.as_deref().unwrap_or("none")}));
            }
            if let Some(t) = three_d {
                if t.get("reset").and_then(Value::as_bool) == Some(true) {
                    s3 = super::three_d::Shape3d::default();
                }
                s3.apply(t);
            }
            super::three_d::store(e, &s3);
        }
        if let Some(v) = &shadow_props {
            let slot = if e.inner_shadow.is_some() { &mut e.inner_shadow } else { &mut e.outer_shadow };
            let mut x = slot.take().or_else(|| shadow_preset(&json!("outer")).flatten()).ok_or_else(|| bad("shape.effects", "no shadow preset"))?;
            if let Some(c) = v.get("color").and_then(color_value) {
                let a = x.color.alpha();
                x.color = with_alpha(c, a);
            }
            if let Some(t) = num(v, "transparency") {
                x.color = with_alpha(x.color.clone(), 1.0 - t.clamp(0.0, 1.0));
            }
            if let Some(b) = num(v, "blur") {
                x.blur = b.clamp(0.0, 100.0);
            }
            if let Some(d) = num(v, "dist") {
                x.dist = d.clamp(0.0, 200.0);
            }
            if let Some(d) = num(v, "dir") {
                x.dir = d.rem_euclid(360.0);
            }
            *slot = Some(x);
        }
        if let Some(v) = &glow_props {
            let mut g =
                e.glow.take().unwrap_or(Glow { color: ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(40000)), radius: 8.0 });
            if let Some(c) = v.get("color").and_then(color_value) {
                let a = g.color.alpha();
                g.color = with_alpha(c, a);
            }
            if let Some(t) = num(v, "transparency") {
                g.color = with_alpha(g.color.clone(), 1.0 - t.clamp(0.0, 1.0));
            }
            if let Some(r) = num(v, "radius") {
                g.radius = r.clamp(0.0, 150.0);
            }
            e.glow = (g.radius > 0.0).then_some(g);
        }
        if let Some(v) = &refl_props {
            let mut r = e.reflection.take().unwrap_or(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.35, dist: 0.0 });
            if let Some(t) = num(v, "transparency") {
                r.start_alpha = 1.0 - t.clamp(0.0, 1.0);
            }
            if let Some(sz) = num(v, "size") {
                r.end_pos = sz.clamp(0.0, 1.0);
            }
            if let Some(d) = num(v, "dist") {
                r.dist = d.clamp(0.0, 100.0);
            }
            if let Some(b) = num(v, "blur") {
                r.blur = b.clamp(0.0, 100.0);
            }
            e.reflection = Some(r);
        }
        Ok(())
    })
}

/// A finite number at `key` of an effect's properties.
fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// `c` with its alpha set to `a` (0–1); fully opaque drops the transform.
fn with_alpha(mut c: ColorRef, a: f64) -> ColorRef {
    c.mods.retain(|m| !matches!(m, ColorTransform::Alpha(_)));
    if a < 1.0 {
        c.mods.push(ColorTransform::Alpha((a.clamp(0.0, 1.0) * 100_000.0).round() as i32));
    }
    c
}

/// Theme quick styles: 7 rows (outline only, solid fill variants, intense…) × 6 accents.
pub fn quick_style_for(index: usize) -> (ShapeStyle, Option<Fill>, Option<Line>) {
    let slots = [
        SchemeSlot::Dk1,
        SchemeSlot::Accent1,
        SchemeSlot::Accent2,
        SchemeSlot::Accent3,
        SchemeSlot::Accent4,
        SchemeSlot::Accent5,
        SchemeSlot::Accent6,
    ];
    let slot = slots.get(index % 7).copied().unwrap_or(SchemeSlot::Accent1);
    let row = index / 7;
    let mut st = ShapeStyle::accent(slot);
    let c = ColorRef::scheme(slot);
    match row {
        0 => {
            // Colored outline, light fill.
            st.fill_ref = (1, ColorRef::scheme(SchemeSlot::Lt1));
            st.line_ref = (2, c);
            st.font_ref.1 = Some(ColorRef::scheme(SchemeSlot::Dk1));
            (st, None, None)
        }
        1 => (st, None, None),
        2 => {
            st.fill_ref = (1, c.clone().with(ColorTransform::LumMod(20000)).with(ColorTransform::LumOff(80000)));
            st.line_ref = (0, c.clone());
            st.font_ref.1 = Some(c.with(ColorTransform::LumMod(50000)));
            (st, None, Some(Line::none()))
        }
        3 => {
            st.fill_ref = (2, c);
            (st, None, Some(Line::none()))
        }
        4 => {
            st.fill_ref = (3, c.clone());
            st.effect_ref = (3, c);
            (st, None, Some(Line::none()))
        }
        _ => {
            st.fill_ref = (1, c.clone().with(ColorTransform::LumMod(75000)));
            st.line_ref = (1, ColorRef::scheme(SchemeSlot::Lt1));
            (st, None, None)
        }
    }
}

fn quick_style(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").unwrap_or(1).min(41);
    let (st, f, l) = quick_style_for(i);
    edit_shapes(s, p, "shape.quickStyle", |sh| {
        sh.style = Some(st.clone());
        sh.fill.clone_from(&f);
        sh.line.clone_from(&l);
        sh.effects = None;
        if let Some(t) = sh.text.as_mut() {
            deckcraft_model::edit::format_all(t, &|r| r.fill = None);
        }
        Ok(())
    })
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

    use super::*;
    use serde_json::json;

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
        st.interaction = Some(crate::Interaction { label: "Move".into(), doc: st.doc.clone(), selection: st.selection.clone() });
        s.execute("shape.rotate", &json!({"gesture": "end", "ids": []})).unwrap();
        assert_eq!(s.active().unwrap().interaction.as_ref().unwrap().label, "Move");
    }
}
