//! Shape Format: position and size, rotation, flips, geometry, fill, outline, effects, styles,
//! names and alt text.

use std::collections::HashMap;

use deckcraft_color::{ColorTransform, Rgba, SchemeSlot};
use deckcraft_model::style::{Dash, Glow, Gradient, GradientShape, GradientStop, LineEnd, PatternFill, PictureFill, PictureMode, Reflection, Shadow};
use deckcraft_model::{ColorRef, Effects, Fill, Geom, Line, ShapeId, ShapeStyle};
use serde_json::{Value, json};

use super::{
    CommandSpec, NO_ID_SPACE, bad, bool_or, bool_param, cmd, color_param, color_value, current_shapes, edit_geometry, edit_shapes, f64_param,
    has_selection, has_slide, id_param, ok, opt_usize_param, str_param, targets, usize_param, with_param, xfrm_of,
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
            "shape.setBoundsMany",
            "Set Final Boxes",
            [],
            None,
            "{items: [{id, x, y, w, h}]} (puts several objects at their final boxes in one atomic step; `arrange.align`/`arrange.distribute` use it so a shared session gets one confirmed command and one undo step)",
            has_slide,
            set_bounds_many
        ),
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
            "{color? | none?: true | background?: true (the slide background shows through; shapes and text boxes only) | gradient?: {stops: [[pos 0..1, color]], kind?: linear|radial|rectangular|path, angle?: deg (linear), focus?: [l, t, r, b] fractions (radial, rectangular and path: where the first stop sits), rotateWithShape?: bool (false keeps linear and radial gradients upright on the slide; rectangular and path gradients turn with the shape when drawn)} | picture?: base64 | pattern?: {preset, fg, bg} | transparency?: 0..1 | reset?: true, ids?}; pictureProps?: {tile?: bool, offsetX?, offsetY?: pt, scaleX?, scaleY?: 0..10, align?: tl|t|tr|l|ctr|r|bl|b|br, mirror?: none|x|y|xy, stretch?: [l, t, r, b] fractions, rotateWithShape?: bool} adjusts a picture fill; a group also formats its members",
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
            "{shadow?: none|outer|inner|perspective|{color, blur, dist, dir}, glow?: none|{color, radius}, softEdges?: pt, reflection?: none|tight|half|full, bevel?: none|circle|…, threeD?: {camera?: preset, rotX?, rotY?, rotZ?: deg, z?: pt, bevelTop?, bevelBottom?: none|circle|…, bevelTopW?, bevelTopH?, bevelBottomW?, bevelBottomH?, depth?, contour?: pt, material?, lighting?, reset?: true}, shadowProps?: {color?, transparency?: 0..1, size?: 0.01..2 (1 = 100%), blur?: pt, dist?: pt, dir?: deg}, glowProps?: {color?, transparency?, radius?: pt}, reflectionProps?: {transparency?, size?: 0..1, dist?: pt, blur?: pt}, reset?, ids?} (the *Props keys adjust the current effect, adding a default one when there is none)",
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
            "shape.setField",
            "Set Exact Value",
            [],
            None,
            "{field: fill|line|text, value: the exact model value as JSON, ids?} (restores the value an undoable step replaced; the fill, line and format commands cannot express the whole model, so collaboration undo uses this)",
            has_selection,
            set_field
        ),
        cmd!(
            "shape.setFields",
            "Set Exact Values",
            [],
            None,
            "{items: [{id, field: fill|line|text, value}]} (restores several values in one atomic step; a group's fill/outline undo uses it)",
            has_slide,
            set_fields
        ),
        cmd!(
            "shape.restore",
            "Restore Shapes",
            [],
            None,
            "{items: [{shape: the whole shape as JSON, parent?: the group that holds it, index: position among its siblings (0 = back), animations?: [entries that name it]}], remove?: [ids this step takes out first]} (moves between a group and its members: undo/redo of arrange.group/ungroup, and undo of a shape insert or delete)",
            has_slide,
            restore
        ),
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

/// Most objects one `shape.setBoundsMany` may move in one step.
const MAX_BOUNDS_ITEMS: usize = 4096;

/// Put several objects at their final boxes in one atomic step.
///
/// `arrange.align` and `arrange.distribute` move every target at once, and a shared session needs
/// that as **one** confirmed command with **one** undo step (RFC §3.2.1). The boxes are final
/// values computed by the sender; no replica recomputes the reference. Every id is checked before
/// the first write, so a stale member refuses the whole operation instead of moving half a
/// selection, and a locked object is left where it is (as with `shape.move`).
fn set_bounds_many(s: &mut Session, p: &Value) -> Result<Value> {
    let items = p.get("items").and_then(Value::as_array).ok_or_else(|| bad("shape.setBoundsMany", "missing `items`"))?;
    if items.is_empty() || items.len() > MAX_BOUNDS_ITEMS {
        return Err(bad("shape.setBoundsMany", format!("`items` must hold 1–{MAX_BOUNDS_ITEMS} boxes")));
    }
    let mut boxes: Vec<(ShapeId, f64, f64, f64, f64)> = Vec::with_capacity(items.len());
    for item in items {
        let id =
            item.get("id").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad("shape.setBoundsMany", "missing `id`"))?;
        let mut v = [0.0f64; 4];
        for (i, key) in ["x", "y", "w", "h"].iter().enumerate() {
            v[i] = item
                .get(*key)
                .and_then(Value::as_f64)
                .filter(|f| f.is_finite())
                .ok_or_else(|| bad("shape.setBoundsMany", format!("missing `{key}`")))?;
        }
        if boxes.iter().any(|(other, ..)| *other == ShapeId(id)) {
            return Err(bad("shape.setBoundsMany", "a box names the same shape twice"));
        }
        boxes.push((ShapeId(id), clamp_pos(v[0]), clamp_pos(v[1]), clamp_side(v[2]), clamp_side(v[3])));
    }
    s.edit(|doc, sel| {
        // A shape without an explicit transform gets the box it resolves to before the final one.
        let resolved: Vec<(ShapeId, deckcraft_model::Xfrm)> = boxes
            .iter()
            .filter_map(|(id, ..)| {
                current_shapes(doc, sel)
                    .and_then(|list| deckcraft_model::find_shape(list, *id))
                    .filter(|sh| sh.xfrm.is_none())
                    .map(|sh| (*id, xfrm_of(doc, sel, sh)))
            })
            .collect();
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.setBoundsMany", "no slide"))?;
        for (id, ..) in &boxes {
            if deckcraft_model::find_shape(&shapes[..], *id).is_none() {
                return Err(bad("shape.setBoundsMany", format!("no shape {}", id.0)));
            }
        }
        let mut n = 0usize;
        for (id, x, y, w, h) in &boxes {
            let Some(sh) = deckcraft_model::find_shape_mut(&mut shapes[..], *id) else { continue };
            if sh.locked {
                continue;
            }
            let xf = sh.xfrm.get_or_insert_with(|| resolved.iter().find(|(other, _)| other == id).map(|(_, xf)| *xf).unwrap_or_default());
            xf.x = *x;
            xf.y = *y;
            xf.w = *w;
            xf.h = *h;
            n += 1;
        }
        if n == 0 {
            return Err(bad("shape.setBoundsMany", "the objects are locked"));
        }
        Ok(json!({"updated": n}))
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
    if bool_or(p, "background", false) {
        return Ok(Some(Some(Fill::Background)));
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

/// Longest model value a `shape.setField` restore may carry, as JSON bytes.
const MAX_FIELD_JSON: usize = 256 * 1024;
/// Most paragraphs and runs one restored text body may hold.
const MAX_PARAGRAPHS: usize = 10_000;
const MAX_RUNS: usize = 50_000;

/// Set one shape field to an exact model value.
///
/// The collaboration adapter uses this to put back the value an undoable step replaced: the
/// user-facing `shape.fill`, `shape.line` and `format.*` commands accept only a subset of the
/// model (a gradient's tile and scaling, a custom dash, a run's whole formatting have no
/// argument), so a lossless restore cannot go through them. The value is still hostile input:
/// it is size-bounded and deserialised into the model type before anything is touched.
fn set_field(s: &mut Session, p: &Value) -> Result<Value> {
    let field = str_param(p, "field").ok_or_else(|| bad("shape.setField", "missing `field` (fill, line or text)"))?;
    let value = p.get("value").cloned().unwrap_or(Value::Null);
    let bytes = serde_json::to_vec(&value).map_err(|e| bad("shape.setField", format!("invalid value: {e}")))?;
    if bytes.len() > MAX_FIELD_JSON {
        return Err(bad("shape.setField", "the value is too large"));
    }
    match field {
        "fill" => {
            let fill = decode_field::<Fill>(&value, "fill")?;
            edit_shapes(s, p, "shape.setField", |sh| {
                sh.fill.clone_from(&fill);
                Ok(())
            })
        }
        "line" => {
            let line = decode_field::<Line>(&value, "line")?;
            edit_shapes(s, p, "shape.setField", |sh| {
                sh.line.clone_from(&line);
                Ok(())
            })
        }
        "text" => {
            let body = decode_field::<deckcraft_model::text::TextBody>(&value, "text")?
                .ok_or_else(|| bad("shape.setField", "a text body cannot be cleared"))?;
            if body.paragraphs.len() > MAX_PARAGRAPHS || body.paragraphs.iter().map(|q| q.runs.len()).sum::<usize>() > MAX_RUNS {
                return Err(bad("shape.setField", "the text body is too large"));
            }
            edit_shapes(s, p, "shape.setField", |sh| {
                sh.text = Some(body.clone());
                Ok(())
            })
        }
        other => Err(bad("shape.setField", format!("unknown field `{other}` (fill, line or text)"))),
    }
}

/// One model value from the wire JSON; `null` means "unset".
fn decode_field<T: serde::de::DeserializeOwned>(v: &Value, what: &str) -> Result<Option<T>> {
    if v.is_null() {
        return Ok(None);
    }
    serde_json::from_value(v.clone()).map(Some).map_err(|e| bad("shape.setField", format!("invalid {what}: {e}")))
}

/// Most objects one `shape.setFields` merge may restore.
const MAX_FIELDS_ITEMS: usize = 4096;
/// Longest combined JSON of all values, so one operation stays inside the wire message limit.
const MAX_FIELDS_JSON: usize = 192 * 1024;

/// Set several shapes' fields in one atomic step.
///
/// A group's `shape.fill`/`shape.line` also formats every visible member, and undo must put all of
/// them back. The values are validated and deserialised before the first shape is touched, so a
/// bad member refuses the whole operation instead of leaving half a group restored.
fn set_fields(s: &mut Session, p: &Value) -> Result<Value> {
    enum Set {
        Fill(Option<Fill>),
        Line(Option<Line>),
        Text(deckcraft_model::text::TextBody),
    }
    let items = p.get("items").and_then(Value::as_array).ok_or_else(|| bad("shape.setFields", "missing `items`"))?;
    if items.is_empty() || items.len() > MAX_FIELDS_ITEMS {
        return Err(bad("shape.setFields", format!("`items` must hold 1–{MAX_FIELDS_ITEMS} values")));
    }
    let mut prepared = Vec::with_capacity(items.len());
    let mut total = 0usize;
    for item in items {
        let id = item.get("id").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad("shape.setFields", "missing `id`"))?;
        let field = str_param(item, "field").ok_or_else(|| bad("shape.setFields", "missing `field` (fill, line or text)"))?;
        let value = item.get("value").cloned().unwrap_or(Value::Null);
        let bytes = serde_json::to_vec(&value).map_err(|e| bad("shape.setFields", format!("invalid value: {e}")))?;
        total = total.saturating_add(bytes.len());
        if total > MAX_FIELDS_JSON {
            return Err(bad("shape.setFields", "the values are too large"));
        }
        let set = match field {
            "fill" => Set::Fill(decode_field::<Fill>(&value, "fill")?),
            "line" => Set::Line(decode_field::<Line>(&value, "line")?),
            "text" => {
                let body = decode_field::<deckcraft_model::text::TextBody>(&value, "text")?
                    .ok_or_else(|| bad("shape.setFields", "a text body cannot be cleared"))?;
                if body.paragraphs.len() > MAX_PARAGRAPHS || body.paragraphs.iter().map(|q| q.runs.len()).sum::<usize>() > MAX_RUNS {
                    return Err(bad("shape.setFields", "the text body is too large"));
                }
                Set::Text(body)
            }
            other => return Err(bad("shape.setFields", format!("unknown field `{other}` (fill, line or text)"))),
        };
        prepared.push((ShapeId(id), set));
    }
    s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.setFields", "no slide"))?;
        for (id, set) in &prepared {
            let sh = deckcraft_model::find_shape_mut(&mut shapes[..], *id).ok_or_else(|| bad("shape.setFields", format!("no shape {}", id.0)))?;
            match set {
                Set::Fill(fill) => sh.fill.clone_from(fill),
                Set::Line(line) => sh.line.clone_from(line),
                Set::Text(body) => sh.text = Some(body.clone()),
            }
        }
        Ok(json!({"updated": prepared.len()}))
    })
}

/// Most shapes one `shape.restore` may put back in one step.
const MAX_RESTORE_ITEMS: usize = 64;
/// Longest combined JSON of the shape images and their animations; the wire message limit is
/// 256 KiB, so the rest of the envelope must fit in what is left.
const MAX_RESTORE_JSON: usize = 192 * 1024;

/// One shape a `shape.restore` puts back, with its place among its siblings.
#[derive(Clone)]
struct Restored {
    shape: deckcraft_model::Shape,
    parent: Option<ShapeId>,
    index: usize,
    animations: Vec<deckcraft_model::Animation>,
}

/// Put back shapes a collaborative step removed (ADR 0010): the undo of a shape delete, or the
/// redo of a shape, picture or media insert.
///
/// Each item is the whole model value of one shape, so the shape returns with its id, text,
/// geometry and every other field; a group image carries its children, so one item restores a
/// whole subtree. `parent` and `index` put it back where it was in the z-order, clamped to the
/// list that exists now, so neighbours that came and went do not block the restore. The
/// animations that named the shape (or were triggered by it) come along: the delete dropped
/// them and cleared the triggers of other effects, and the restore re-attaches both.
///
/// The value is hostile input even though the server builds it: it is size-bounded,
/// deserialised into the model types before anything is touched, and an id already on the slide
/// refuses the whole operation, not half of it.
fn restore(s: &mut Session, p: &Value) -> Result<Value> {
    let items = p.get("items").and_then(Value::as_array).ok_or_else(|| bad("shape.restore", "missing `items`"))?;
    if items.is_empty() || items.len() > MAX_RESTORE_ITEMS {
        return Err(bad("shape.restore", format!("`items` must hold 1–{MAX_RESTORE_ITEMS} shapes")));
    }
    let mut total = 0usize;
    let mut prepared = Vec::with_capacity(items.len());
    for item in items {
        let value = item.get("value").ok_or_else(|| bad("shape.restore", "missing `value`"))?;
        total = total.saturating_add(serde_json::to_vec(value).map_err(|e| bad("shape.restore", format!("invalid shape: {e}")))?.len());
        let mut animations = Vec::new();
        match item.get("animations") {
            None | Some(Value::Null) => {}
            Some(Value::Array(list)) => {
                for a in list {
                    total = total.saturating_add(serde_json::to_vec(a).map_err(|e| bad("shape.restore", format!("invalid animation: {e}")))?.len());
                    animations.push(
                        serde_json::from_value::<deckcraft_model::Animation>(a.clone())
                            .map_err(|e| bad("shape.restore", format!("invalid animation: {e}")))?,
                    );
                }
            }
            Some(_) => return Err(bad("shape.restore", "`animations` must be a list")),
        }
        if total > MAX_RESTORE_JSON {
            return Err(bad("shape.restore", "the images are too large"));
        }
        let shape: deckcraft_model::Shape = serde_json::from_value(value.clone()).map_err(|e| bad("shape.restore", format!("invalid shape: {e}")))?;
        let index = item.get("index").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
        let parent = item.get("parent").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).map(ShapeId);
        prepared.push(Restored { shape, parent, index, animations });
    }
    // Shapes this step takes out first. The undo of `arrange.group` removes the group and puts
    // its members back at the top level; the redo removes the members and puts the group back.
    let remove = match p.get("remove") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(list)) => {
            if list.len() > MAX_RESTORE_ITEMS {
                return Err(bad("shape.restore", format!("`remove` must hold at most {MAX_RESTORE_ITEMS} ids")));
            }
            let mut ids = Vec::with_capacity(list.len());
            for value in list {
                let id =
                    value.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad("shape.restore", "`remove` must be a list of shape ids"))?;
                ids.push(ShapeId(id));
            }
            ids
        }
        Some(_) => return Err(bad("shape.restore", "`remove` must be a list of shape ids")),
    };
    // Insert siblings in the order they sat in, and let a round of the loop put a parent back
    // before its children: the payload may name a group and a child of it that is not one image.
    prepared.sort_by_key(|item| item.index);
    s.edit(|doc, sel| {
        if sel.target != crate::Target::Slides {
            return Err(bad("shape.restore", "a restore puts shapes back on a slide, not on a master"));
        }
        {
            let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.restore", "no slide"))?;
            for id in &remove {
                let list = deckcraft_model::sibling_list_mut(shapes, *id)
                    .ok_or_else(|| bad("shape.restore", format!("shape {} to remove is not on the slide", id.0)))?;
                list.retain(|s| s.id != *id);
            }
            let mut present = Vec::new();
            deckcraft_model::walk(shapes, &mut |sh, _| present.push(sh.id));
            for item in &prepared {
                let mut ids = Vec::new();
                subtree_ids(&item.shape, &mut ids, 0);
                if let Some(clash) = ids.iter().find(|id| present.contains(id)) {
                    return Err(bad("shape.restore", format!("shape {} is already on the slide", clash.0)));
                }
                present.extend(ids);
            }
            let mut pending: Vec<Restored> = prepared.clone();
            loop {
                let total = pending.len();
                let mut deferred = Vec::new();
                for item in pending.drain(..) {
                    let slot = match item.parent {
                        None => Some(&mut *shapes),
                        Some(parent) => deckcraft_model::find_shape_mut(&mut shapes[..], parent).and_then(|group| group.children_mut()),
                    };
                    match slot {
                        Some(slot) => {
                            let at = item.index.min(slot.len());
                            slot.insert(at, item.shape);
                        }
                        None => deferred.push(item),
                    }
                }
                if deferred.is_empty() {
                    break;
                }
                // Nothing moved this round: the remaining parents are missing or not groups.
                if deferred.len() == total {
                    return Err(bad("shape.restore", "a parent named by the restore is missing or is not a group"));
                }
                pending = deferred;
            }
        }
        let mut ids = Vec::new();
        for item in &prepared {
            subtree_ids(&item.shape, &mut ids, 0);
        }
        let slide = doc.slides.get_mut(sel.slide).ok_or_else(|| bad("shape.restore", "no slide"))?;
        let slide = std::sync::Arc::make_mut(slide);
        for item in &prepared {
            for anim in &item.animations {
                if ids.contains(&anim.shape) {
                    if !slide.animations.iter().any(|a| a == anim) {
                        slide.animations.push(anim.clone());
                    }
                } else if let Some(trigger) = anim.trigger.filter(|t| ids.contains(t)) {
                    // The delete cleared this effect's trigger; put it back on the same entry.
                    let mut cleared = anim.clone();
                    cleared.trigger = None;
                    if let Some(at) = slide.animations.iter_mut().find(|a| a.trigger.is_none() && **a == cleared) {
                        at.trigger = Some(trigger);
                    }
                }
            }
        }
        Ok(json!({"restored": prepared.len()}))
    })
}

/// Every id in a shape and its descendants.
fn subtree_ids(shape: &deckcraft_model::Shape, out: &mut Vec<ShapeId>, depth: usize) {
    if depth > 64 {
        return;
    }
    out.push(shape.id);
    for child in shape.children() {
        subtree_ids(child, out, depth + 1);
    }
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
    // A targeted shape inside another targeted group is reached through the group: editing it again
    // would toggle it back.
    let mut ids = super::targets(s, p)?;
    if ids.len() > 1 {
        let doc = s.doc()?;
        // Walk each distinct group once, however often `ids` repeats it.
        let mut once = std::collections::HashSet::new();
        ids.retain(|id| once.insert(*id));
        let mut inside = std::collections::HashSet::new();
        for id in &ids {
            if let Some(g) = doc.shape(*id) {
                deckcraft_model::walk(g.children(), &mut |c, _| {
                    inside.insert(c.id);
                });
            }
        }
        ids.retain(|id| !inside.contains(id));
    }
    let p = &json!({"ids": ids, "locked": p.get("locked")});
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use deckcraft_model::ShapeId;

    use super::*;
    use serde_json::json;

    #[test]
    fn set_fields_validates_every_value_before_touching_a_shape() {
        let mut s = Session::with_new();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40, 100, 80, 60]})).expect("a")["id"].as_u64().expect("id");
        let b = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [200, 100, 80, 60]})).expect("b")["id"].as_u64().expect("id");
        let bad = json!({"items": [
            {"id": a, "field": "fill", "value": {"kind": "background"}},
            {"id": b, "field": "fill", "value": {"kind": "nonsense"}},
        ]});
        assert!(s.execute("shape.setFields", &bad).is_err(), "a bad value refuses the whole merge");
        let fills_after_bad: Vec<Option<Fill>> = s.doc().expect("doc").doc.slides[0].shapes.iter().map(|sh| sh.fill.clone()).collect();
        assert!(fills_after_bad.iter().all(Option::is_none), "nothing was written");

        let good = json!({"items": [
            {"id": a, "field": "fill", "value": {"kind": "background"}},
            {"id": b, "field": "fill", "value": {"kind": "none"}},
        ]});
        s.execute("shape.setFields", &good).expect("a good merge");
        let doc = s.doc().expect("doc").doc.clone();
        let fill_of = |id: u64| doc.slides[0].shapes.iter().find(|sh| sh.id.0 as u64 == id).and_then(|sh| sh.fill.clone());
        assert_eq!(fill_of(a), Some(Fill::Background));
        assert_eq!(fill_of(b), Some(Fill::None));
    }

    #[test]
    fn set_bounds_many_moves_every_target_in_one_atomic_step() {
        let mut s = blank();
        let mut ids = Vec::new();
        for x in [40, 200, 360] {
            ids.push(s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap());
        }
        let out = s
            .execute(
                "shape.setBoundsMany",
                &json!({"items": [
                    {"id": ids[0], "x": 10, "y": 20, "w": 80, "h": 60},
                    {"id": ids[1], "x": 150, "y": 20, "w": 80, "h": 60},
                    {"id": ids[2], "x": 290, "y": 20, "w": 80, "h": 60},
                ]}),
            )
            .expect("move all");
        assert_eq!(out["updated"].as_u64(), Some(3));
        let boxes: Vec<(f64, f64)> = {
            let st = s.doc().unwrap();
            st.doc.slides[0].shapes.iter().map(|sh| (sh.xfrm.unwrap().x, sh.xfrm.unwrap().y)).collect()
        };
        assert_eq!(boxes, vec![(10.0, 20.0), (150.0, 20.0), (290.0, 20.0)]);

        // One step, not three: a single undo puts the whole selection back.
        s.execute("edit.undo", &json!({})).unwrap();
        let boxes: Vec<f64> = {
            let st = s.doc().unwrap();
            st.doc.slides[0].shapes.iter().map(|sh| sh.xfrm.unwrap().x).collect()
        };
        assert_eq!(boxes, vec![40.0, 200.0, 360.0]);
    }

    #[test]
    fn set_bounds_many_refuses_a_missing_or_repeated_shape_before_writing() {
        let mut s = blank();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [200, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();

        let missing = json!({"items": [
            {"id": a, "x": 10, "y": 10, "w": 80, "h": 60},
            {"id": 9999, "x": 10, "y": 10, "w": 80, "h": 60},
        ]});
        assert!(s.execute("shape.setBoundsMany", &missing).is_err(), "an unknown id refuses the whole step");
        let xs: Vec<f64> = {
            let st = s.doc().unwrap();
            st.doc.slides[0].shapes.iter().map(|sh| sh.xfrm.unwrap().x).collect()
        };
        assert_eq!(xs, vec![40.0, 200.0], "nothing moved");

        let repeated = json!({"items": [
            {"id": a, "x": 10, "y": 10, "w": 80, "h": 60},
            {"id": b, "x": 10, "y": 10, "w": 80, "h": 60},
            {"id": a, "x": 30, "y": 30, "w": 80, "h": 60},
        ]});
        assert!(s.execute("shape.setBoundsMany", &repeated).is_err(), "one id twice is ambiguous");

        s.execute("shape.lock", &json!({"ids": [a], "locked": true})).unwrap();
        assert!(
            s.execute("shape.setBoundsMany", &json!({"items": [{"id": a, "x": 10, "y": 10, "w": 80, "h": 60}]})).is_err(),
            "a locked target is not moved"
        );
    }

    /// One empty slide, so the sample deck's shapes do not shift the ids and the z-order.
    fn blank() -> Session {
        let mut s = Session::with_new();
        let title = s.doc().unwrap().doc.slides[0].id;
        s.execute("slide.new", &json!({"layout": "blank", "at": 1})).unwrap();
        s.execute("slide.delete", &json!({"ids": [title.0]})).unwrap();
        s
    }

    #[test]
    fn restore_puts_a_shape_back_in_its_z_order_with_its_animation() {
        let mut s = blank();
        let mut ids = Vec::new();
        for x in [40, 200, 360] {
            ids.push(s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap());
        }
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [ids[1]]})).unwrap();
        let (value, index, animations, animation) = {
            let st = s.doc().unwrap();
            let slide = &st.doc.slides[0];
            let shape = deckcraft_model::find_shape(&slide.shapes, ShapeId(ids[1] as u32)).unwrap();
            let index = slide.shapes.iter().position(|sh| sh.id == shape.id).unwrap();
            let animations: Vec<Value> = slide.animations.iter().map(|a| serde_json::to_value(a).unwrap()).collect();
            (serde_json::to_value(shape).unwrap(), index, animations, slide.animations[0].clone())
        };
        s.execute("edit.delete", &json!({"ids": [ids[1]]})).unwrap();
        assert!(s.execute("animation.get", &json!({})).unwrap().as_array().unwrap().is_empty(), "the delete drops its animation");

        s.execute("shape.restore", &json!({"items": [{"value": value, "index": index, "animations": animations}]})).unwrap();
        let st = s.doc().unwrap();
        let slide = &st.doc.slides[0];
        let order: Vec<u64> = slide.shapes.iter().map(|sh| sh.id.0 as u64).collect();
        assert_eq!(order, ids, "the shape is back where it sat");
        assert_eq!(slide.animations.len(), 1);
        assert_eq!(slide.animations[0], animation, "its animation came back");
    }

    #[test]
    fn restore_moves_between_a_group_and_its_members_in_one_step() {
        let mut s = blank();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [200, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();
        let (va, vb) = {
            let st = s.doc().unwrap();
            let slide = &st.doc.slides[0];
            (
                serde_json::to_value(deckcraft_model::find_shape(&slide.shapes, ShapeId(a as u32)).unwrap()).unwrap(),
                serde_json::to_value(deckcraft_model::find_shape(&slide.shapes, ShapeId(b as u32)).unwrap()).unwrap(),
            )
        };
        s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
        let group = s.execute("arrange.group", &json!({})).unwrap()["id"].as_u64().unwrap();
        let value = {
            let st = s.doc().unwrap();
            serde_json::to_value(deckcraft_model::find_shape(&st.doc.slides[0].shapes, ShapeId(group as u32)).unwrap()).unwrap()
        };
        assert_eq!(s.doc().unwrap().doc.slides[0].shapes.len(), 1, "the group holds both shapes");

        // Undo of the group: remove it and put its members back at the top level.
        s.execute(
            "shape.restore",
            &json!({
                "items": [{"value": va, "index": 0}, {"value": vb, "index": 1}],
                "remove": [group],
            }),
        )
        .unwrap();
        {
            let st = s.doc().unwrap();
            let order: Vec<u64> = st.doc.slides[0].shapes.iter().map(|sh| sh.id.0 as u64).collect();
            assert_eq!(order, [a, b], "both members are back in their order");
        }

        // Redo of the group: remove the members and put the whole group image back.
        s.execute(
            "shape.restore",
            &json!({
                "items": [{"value": value, "index": 0}],
                "remove": [a, b],
            }),
        )
        .unwrap();
        let st = s.doc().unwrap();
        let group = deckcraft_model::find_shape(&st.doc.slides[0].shapes, ShapeId(group as u32)).expect("the group is back");
        assert_eq!(group.children().len(), 2, "with its members inside");
        assert_eq!(st.doc.slides[0].shapes.len(), 1);
    }

    #[test]
    fn restore_refuses_a_missing_removal_before_writing_anything() {
        let mut s = blank();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();
        let value = {
            let st = s.doc().unwrap();
            serde_json::to_value(deckcraft_model::find_shape(&st.doc.slides[0].shapes, ShapeId(a as u32)).unwrap()).unwrap()
        };
        let err = s.execute("shape.restore", &json!({"items": [{"value": value, "index": 0}], "remove": [9999]}));
        assert!(err.is_err(), "a removal that is not on the slide refuses the whole step");
        assert_eq!(s.doc().unwrap().doc.slides[0].shapes.len(), 1, "nothing was inserted");
    }

    #[test]
    fn restore_puts_a_group_child_back_under_its_parent() {
        let mut s = blank();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [200, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
        let group = s.execute("arrange.group", &json!({})).unwrap()["id"].as_u64().unwrap();
        let value = {
            let st = s.doc().unwrap();
            let g = deckcraft_model::find_shape(&st.doc.slides[0].shapes, ShapeId(group as u32)).unwrap();
            serde_json::to_value(&g.children()[0]).unwrap()
        };
        s.execute("edit.delete", &json!({"ids": [a]})).unwrap();
        s.execute("shape.restore", &json!({"items": [{"value": value, "parent": group, "index": 0}]})).unwrap();
        {
            let st = s.doc().unwrap();
            let g = deckcraft_model::find_shape(&st.doc.slides[0].shapes, ShapeId(group as u32)).unwrap();
            assert_eq!(g.children().len(), 2, "the child is back inside the group");
            assert_eq!(g.children()[0].id.0 as u64, a, "at the bottom of the group's order");
        }
        let before = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
        let err = s.execute("shape.restore", &json!({"items": [{"value": {"id": 900, "kind": {"kind": "shape"}}, "parent": 9999, "index": 0}]}));
        assert!(err.is_err(), "a missing parent refuses the restore");
        assert_eq!(serde_json::to_value(&s.doc().unwrap().doc).unwrap(), before, "nothing was written");
    }

    #[test]
    fn restore_refuses_a_shape_id_that_is_already_on_the_slide() {
        let mut s = blank();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40, 100, 80, 60]})).unwrap()["id"].as_u64().unwrap();
        let value = serde_json::to_value(s.doc().unwrap().shape(ShapeId(a as u32)).unwrap()).unwrap();
        let err = s.execute("shape.restore", &json!({"items": [{"value": value, "index": 0}]}));
        assert!(err.is_err(), "the id is taken");
        assert_eq!(s.doc().unwrap().doc.slides[0].shapes.len(), 1, "no duplicate was written");
    }

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

    #[test]
    fn background_fill_shows_the_slide_background_and_survives_a_save() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.execute("design.background", &json!({"color": "#0000c8"})).unwrap();
        let id = ShapeId(s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap()["id"].as_u64().unwrap() as u32);
        let inside = |s: &mut Session| s.execute("slide.colorAt", &json!({"x": 200, "y": 150})).unwrap()["color"].clone();
        assert_ne!(inside(&mut s), "0000C8", "a new shape takes the theme's fill");
        s.execute("shape.fill", &json!({"background": true})).unwrap();
        assert_eq!(s.doc().unwrap().shape(id).unwrap().fill, Some(Fill::Background));
        assert_eq!(inside(&mut s), "0000C8", "the slide background shows through the shape");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().shape(id).unwrap().fill, None, "one undo step returns to the theme's fill");
        s.execute("edit.redo", &json!({})).unwrap();
        // Transparency changes a solid or picture fill only.
        s.execute("shape.fill", &json!({"transparency": 0.5})).unwrap();
        assert_eq!(s.doc().unwrap().shape(id).unwrap().fill, Some(Fill::Background));

        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        let filled = back.slides.iter().flat_map(|sl| sl.shapes.iter()).filter(|sh| sh.fill == Some(Fill::Background)).count();
        assert_eq!(filled, 1, "the shape keeps the background fill after a save and reopen");
    }

    /// A picture cannot save a slide background fill, so `background` leaves its fill alone.
    #[test]
    fn background_fill_skips_pictures() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let shape = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap()["id"].as_u64().unwrap();
        let png = deckcraft_render::Image { width: 2, height: 2, pixels: [255, 0, 0, 255].repeat(4) }.to_png();
        let pic = s
            .execute("insert.picture", &json!({"data": crate::cmd::base64_encode(&png), "name": "red.png", "rect": [400, 100, 100, 100]}))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let fill = |s: &Session, id: u64| s.doc().unwrap().shape(ShapeId(id as u32)).unwrap().fill.clone();
        let err = s.execute("shape.fill", &json!({"background": true, "ids": [pic]})).unwrap_err();
        assert!(err.to_string().contains("shapes and text boxes"), "{err}");
        assert_eq!(fill(&s, pic), None);

        s.execute("edit.select", &json!({"ids": [shape, pic]})).unwrap();
        let group = s.execute("arrange.group", &json!({})).unwrap()["id"].as_u64().unwrap();
        s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
        s.execute("shape.fill", &json!({"background": true})).unwrap();
        assert_eq!(fill(&s, shape), Some(Fill::Background));
        assert!(matches!(fill(&s, pic), Some(Fill::Solid { .. })), "the picture keeps its fill");
        assert_eq!(fill(&s, group), None, "the group's own fill is cleared");
        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        let mut filled = 0;
        for slide in &back.slides {
            deckcraft_model::walk(&slide.shapes, &mut |sh, _| filled += usize::from(sh.fill == Some(Fill::Background)));
        }
        assert_eq!(filled, 1, "the shape keeps the background fill after a save and reopen");
    }

    /// A picture filled with its group's fill keeps that fill behind its transparent pixels when
    /// the group's shapes take the slide background fill.
    #[test]
    fn background_fill_keeps_the_group_fill_a_picture_uses() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let shape = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap()["id"].as_u64().unwrap();
        let png = deckcraft_render::Image { width: 2, height: 2, pixels: [0, 0, 0, 0].repeat(4) }.to_png();
        let pic = s
            .execute("insert.picture", &json!({"data": crate::cmd::base64_encode(&png), "name": "clear.png", "rect": [400, 100, 100, 100]}))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        s.execute("edit.select", &json!({"ids": [shape, pic]})).unwrap();
        let group = s.execute("arrange.group", &json!({})).unwrap()["id"].as_u64().unwrap();
        let red = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(255, 0, 0))));
        s.edit(|doc, _| {
            for slide in &mut doc.slides {
                let slide = std::sync::Arc::make_mut(slide);
                if let Some(g) = slide.shape_mut(ShapeId(group as u32)) {
                    g.fill.clone_from(&red);
                }
                if let Some(p) = slide.shape_mut(ShapeId(pic as u32)) {
                    p.fill = Some(Fill::Group);
                }
            }
            Ok(())
        })
        .unwrap();
        let behind = |s: &mut Session| s.execute("slide.colorAt", &json!({"x": 450, "y": 150})).unwrap()["color"].clone();
        assert_eq!(behind(&mut s), "FF0000", "the group's fill shows through the picture");
        s.execute("edit.select", &json!({"ids": [group]})).unwrap();
        s.execute("shape.fill", &json!({"background": true})).unwrap();
        let fill = |s: &Session, id: u64| s.doc().unwrap().shape(ShapeId(id as u32)).unwrap().fill.clone();
        assert_eq!(fill(&s, shape), Some(Fill::Background));
        assert_eq!(fill(&s, pic), Some(Fill::Group));
        assert_eq!(fill(&s, group), red, "the group keeps the fill its picture uses");
        assert_eq!(behind(&mut s), "FF0000", "the group's fill still shows through the picture");
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

    #[test]
    fn set_field_puts_back_the_exact_values_the_user_commands_cannot_express() {
        use deckcraft_model::style::{Compound, Dash, Gradient, GradientShape, GradientStop, LineAlign, LineCap, LineJoin};

        let mut s = Session::with_new();
        let id =
            s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 80.0, 60.0], "text": "Hello"})).unwrap()["id"].as_u64().unwrap();
        let id = ShapeId(id as u32);
        // A gradient with a tile and a custom dash: neither has an argument on `shape.fill` /
        // `shape.line`, so a lossless restore can only go through the exact value.
        let fill = Some(Fill::Gradient(Gradient {
            stops: vec![
                GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::rgb(255, 0, 0)) },
                GradientStop { pos: 1.0, color: ColorRef::rgb(Rgba::rgb(0, 0, 255)) },
            ],
            shape: GradientShape::Linear { angle: 45.0, scaled: true },
            flip: None,
            rotate_with_shape: true,
            tile: [0.1, 0.2, 0.3, 0.4],
        }));
        let line = Some(Line {
            fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(0, 255, 0)))),
            width: Some(3.5),
            dash: Some(Dash::Custom { pattern: vec![(1.0, 2.0), (3.0, 4.0)] }),
            cap: Some(LineCap::Round),
            join: Some(LineJoin::Miter),
            miter_limit: Some(8),
            align: Some(LineAlign::Inside),
            compound: Some(Compound::Double),
            head: None,
            tail: None,
        });
        s.execute("shape.setField", &json!({"field": "fill", "value": serde_json::to_value(&fill).unwrap()})).unwrap();
        s.execute("shape.setField", &json!({"field": "line", "value": serde_json::to_value(&line).unwrap()})).unwrap();
        let doc = s.doc().unwrap();
        let sh = doc.shape(id).unwrap();
        assert_eq!(sh.fill, fill);
        assert_eq!(sh.line, line);
        let text = serde_json::to_value(sh.text.as_ref().unwrap()).unwrap();
        s.execute("shape.setField", &json!({"field": "text", "value": text})).unwrap();
        assert_eq!(s.doc().unwrap().shape(id).unwrap().text.as_ref().unwrap().text(), "Hello");
        // `null` puts the field back to "unset" (inherited).
        s.execute("shape.setField", &json!({"field": "fill", "value": null})).unwrap();
        s.execute("shape.setField", &json!({"field": "line", "value": null})).unwrap();
        let doc = s.doc().unwrap();
        let sh = doc.shape(id).unwrap();
        assert!(sh.fill.is_none() && sh.line.is_none());
        assert_eq!(sh.text.as_ref().unwrap().text(), "Hello", "the text is untouched");
    }

    #[test]
    fn set_field_refuses_a_hostile_or_unknown_value_without_editing() {
        let mut s = Session::with_new();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [10.0, 10.0, 30.0, 30.0]})).unwrap();
        let before = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
        assert!(s.execute("shape.setField", &json!({"field": "geometry", "value": {}})).is_err());
        assert!(s.execute("shape.setField", &json!({"field": "fill", "value": "red"})).is_err());
        assert!(s.execute("shape.setField", &json!({"field": "text", "value": 7})).is_err());
        assert!(s.execute("shape.setField", &json!({"field": "text", "value": null})).is_err());
        assert!(s.execute("shape.setField", &json!({"field": "line", "value": {"width": "wide"}})).is_err());
        assert_eq!(serde_json::to_value(&s.doc().unwrap().doc).unwrap(), before);
    }
}
