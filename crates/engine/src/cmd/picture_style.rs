//! Picture Format ▸ Picture Styles: PowerPoint's quick styles for pictures, each a frame (shape,
//! outline) and effects applied as one step, replacing the previous style.

use deckcraft_color::{ColorTransform, Rgba};
use deckcraft_model::style::{Effects, Reflection, Shadow};
use deckcraft_model::{ColorRef, Geom, Line, Shape};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, edit_shapes, has_selection, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "picture.style",
            "Picture Styles",
            ["Picture Format", "Picture Styles"],
            None,
            "{style: simpleFrameWhite|dropShadowRectangle|softEdgeOval|… | none (see picture.styles), ids?}",
            has_selection,
            style
        ),
        cmd!(query "picture.styles", "Picture Style Gallery", [], None, "{} → [{id, label}]", always, |_s, _p| Ok(json!(
            STYLES.iter().map(|(id, label)| json!({"id": id, "label": label})).collect::<Vec<_>>()
        ))),
    ]
}

/// (id, label), in PowerPoint's gallery order.
pub const STYLES: [(&str, &str); 24] = [
    ("simpleFrameWhite", "Simple Frame, White"),
    ("beveledMatteWhite", "Beveled Matte, White"),
    ("metalFrame", "Metal Frame"),
    ("dropShadowRectangle", "Drop Shadow Rectangle"),
    ("roundedDiagonalCornerWhite", "Rounded Diagonal Corner, White"),
    ("snipDiagonalCornerWhite", "Snip Diagonal Corner, White"),
    ("moderateFrameWhite", "Moderate Frame, White"),
    ("rotatedWhite", "Rotated, White"),
    ("perspectiveShadowWhite", "Perspective Shadow, White"),
    ("softEdgeRectangle", "Soft Edge Rectangle"),
    ("doubleFrameBlack", "Double Frame, Black"),
    ("thickMatteBlack", "Thick Matte, Black"),
    ("simpleFrameBlack", "Simple Frame, Black"),
    ("beveledOvalBlack", "Beveled Oval, Black"),
    ("compoundFrameBlack", "Compound Frame, Black"),
    ("centerShadowRectangle", "Center Shadow Rectangle"),
    ("softEdgeOval", "Soft Edge Oval"),
    ("reflectedRoundedRectangle", "Reflected Rounded Rectangle"),
    ("reflectedBevelWhite", "Reflected Bevel, White"),
    ("metalRoundedRectangle", "Metal Rounded Rectangle"),
    ("moderateFrameBlack", "Moderate Frame, Black"),
    ("bevelRectangle", "Bevel Rectangle"),
    ("bevelPerspective", "Bevel Perspective"),
    ("metalOval", "Metal Oval"),
];

/// The look of a style: geometry, outline (colour, width) and effects.
pub struct Look {
    pub preset: &'static str,
    pub frame: Option<(Rgba, f64)>,
    pub shadow: Option<&'static str>,
    pub soft_edge: Option<f64>,
    pub reflection: bool,
    pub bevel: Option<&'static str>,
    pub rot: Option<f64>,
}

pub fn look(id: &str) -> Option<Look> {
    let white = Rgba::rgb(0xFF, 0xFF, 0xFF);
    let black = Rgba::rgb(0, 0, 0);
    let silver = Rgba::rgb(0xC0, 0xC0, 0xC0);
    let l = |preset, frame, shadow| Look { preset, frame, shadow, soft_edge: None, reflection: false, bevel: None, rot: None };
    Some(match id {
        "simpleFrameWhite" => l("rect", Some((white, 7.0)), Some("soft")),
        "beveledMatteWhite" => Look { bevel: Some("circle"), ..l("rect", Some((white, 6.0)), Some("soft")) },
        "metalFrame" => Look { bevel: Some("angle"), ..l("rect", Some((silver, 7.0)), Some("soft")) },
        "dropShadowRectangle" => l("rect", None, Some("drop")),
        "roundedDiagonalCornerWhite" => l("round2DiagRect", Some((white, 6.0)), Some("soft")),
        "snipDiagonalCornerWhite" => l("snip2DiagRect", Some((white, 6.0)), Some("soft")),
        "moderateFrameWhite" => l("rect", Some((white, 4.5)), Some("soft")),
        "rotatedWhite" => Look { rot: Some(356.0), ..l("rect", Some((white, 6.0)), Some("soft")) },
        "perspectiveShadowWhite" => l("rect", Some((white, 4.0)), Some("perspective")),
        "softEdgeRectangle" => Look { soft_edge: Some(10.0), ..l("rect", None, None) },
        "doubleFrameBlack" => l("rect", Some((black, 6.0)), None),
        "thickMatteBlack" => l("rect", Some((black, 10.0)), Some("soft")),
        "simpleFrameBlack" => l("rect", Some((black, 5.0)), None),
        "beveledOvalBlack" => Look { bevel: Some("circle"), ..l("ellipse", Some((black, 4.0)), Some("soft")) },
        "compoundFrameBlack" => l("rect", Some((black, 4.5)), None),
        "centerShadowRectangle" => l("rect", None, Some("center")),
        "softEdgeOval" => Look { soft_edge: Some(12.0), ..l("ellipse", None, None) },
        "reflectedRoundedRectangle" => Look { reflection: true, ..l("roundRect", None, None) },
        "reflectedBevelWhite" => Look { reflection: true, bevel: Some("circle"), ..l("rect", Some((white, 4.0)), None) },
        "metalRoundedRectangle" => Look { bevel: Some("angle"), ..l("roundRect", Some((silver, 6.0)), Some("soft")) },
        "moderateFrameBlack" => l("rect", Some((black, 4.5)), Some("soft")),
        "bevelRectangle" => Look { bevel: Some("angle"), ..l("rect", None, Some("soft")) },
        "bevelPerspective" => Look { bevel: Some("relaxedInset"), ..l("rect", None, Some("perspective")) },
        "metalOval" => Look { bevel: Some("angle"), ..l("ellipse", Some((silver, 6.0)), Some("soft")) },
        _ => return None,
    })
}

fn shadow(kind: &str) -> Shadow {
    let base = Shadow {
        color: ColorRef::rgb(Rgba::BLACK).with(ColorTransform::Alpha(40000)),
        blur: 4.0,
        dist: 3.0,
        dir: 45.0,
        inner: false,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: "tl".into(),
        rotate_with_shape: false,
    };
    match kind {
        "drop" => Shadow { blur: 12.0, dist: 6.0, dir: 90.0, ..base },
        "center" => Shadow { blur: 16.0, dist: 0.0, ..base },
        "perspective" => Shadow { sy: 0.23, kx: -20.0, dir: 90.0, dist: 0.0, blur: 6.0, align: "bl".into(), ..base },
        _ => base,
    }
}

/// Apply style `id` (or clear the style with `none`) to `sh`.
pub fn apply(sh: &mut Shape, id: &str) -> bool {
    if id == "none" {
        sh.geom = Geom::preset("rect");
        sh.line = None;
        sh.effects = None;
        return true;
    }
    let Some(lk) = look(id) else { return false };
    sh.geom = Geom::preset(lk.preset);
    sh.line = Some(lk.frame.map_or_else(Line::none, |(c, w)| Line::solid(ColorRef::rgb(c), w)));
    let e = Effects {
        outer_shadow: lk.shadow.map(shadow),
        soft_edge: lk.soft_edge,
        reflection: lk.reflection.then_some(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.35, dist: 0.0 }),
        bevel: lk.bevel.map(str::to_string),
        ..Default::default()
    };
    sh.effects = (!e.is_empty()).then_some(e);
    if let (Some(r), Some(x)) = (lk.rot, sh.xfrm.as_mut()) {
        x.rot = r;
    }
    true
}

fn style(s: &mut Session, p: &Value) -> Result<Value> {
    let id = str_param(p, "style").ok_or_else(|| bad("picture.style", "missing `style`"))?.to_string();
    if id != "none" && look(&id).is_none() {
        return Err(bad("picture.style", format!("unknown style `{id}`; see picture.styles")));
    }
    edit_shapes(s, p, "picture.style", |sh| {
        apply(sh, &id);
        Ok(())
    })
}
