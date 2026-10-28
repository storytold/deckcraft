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
