//! Picture Format ▸ Color: Recolor presets and Set Transparent Color.

use deckcraft_color::{Rgba, SchemeSlot};
use deckcraft_geom::{Affine, Point, Rect, Xfrm};
use deckcraft_model::style::{PictureAdjust, PictureMode};
use deckcraft_model::{ColorRef, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, bool_or, cmd, current_shapes, edit_shapes, has_selection, str_param, targets, text, xfrm_of};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "picture.recolor",
            "Recolor",
            ["Picture Format", "Adjust", "Color"],
            None,
            "{preset: none|grayscale|sepia|washout|bw25|bw50|bw75|accent1Dark…accent6Dark|accent1Light…accent6Light|tx2Dark|tx2Light (see picture.recolors), ids?}",
            has_selection,
            recolor
        ),
        cmd!(query "picture.recolors", "Recolor Gallery", [], None, "{} → [{id, label}]", always, |_s, _p| Ok(json!(
            presets().into_iter().map(|(id, label)| json!({"id": id, "label": label})).collect::<Vec<_>>()
        ))),
        cmd!(
            "picture.transparentColor",
            "Set Transparent Color",
            ["Picture Format", "Adjust", "Color"],
            None,
            "{color: #RRGGBB | at: [x, y] | none: true, ids?} (pixels of that colour become transparent; `at` takes the colour of the picture's own pixel under that slide point, as a click does in PowerPoint)",
            has_selection,
            transparent_color
        ),
    ]
}

const ACCENTS: [(&str, &str, SchemeSlot); 7] = [
    ("tx2", "Text color 2", SchemeSlot::Dk2),
    ("accent1", "Accent color 1", SchemeSlot::Accent1),
    ("accent2", "Accent color 2", SchemeSlot::Accent2),
    ("accent3", "Accent color 3", SchemeSlot::Accent3),
    ("accent4", "Accent color 4", SchemeSlot::Accent4),
    ("accent5", "Accent color 5", SchemeSlot::Accent5),
    ("accent6", "Accent color 6", SchemeSlot::Accent6),
];

/// (id, label), in PowerPoint's gallery order: colour modes, then dark and light variations.
pub fn presets() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = [
        ("none", "No Recolor"),
        ("grayscale", "Grayscale"),
        ("sepia", "Sepia"),
        ("washout", "Washout"),
        ("bw25", "Black and White: 25%"),
        ("bw50", "Black and White: 50%"),
        ("bw75", "Black and White: 75%"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    for (id, label, _) in ACCENTS {
        v.push((format!("{id}Dark"), format!("{label} Dark")));
    }
    for (id, label, _) in ACCENTS {
        v.push((format!("{id}Light"), format!("{label} Light")));
    }
    v
}

/// Set `a`'s recolour to `preset`, clearing any earlier one. `false` for an unknown preset.
pub fn apply(a: &mut PictureAdjust, preset: &str) -> bool {
    let washed = a.brightness == 0.7 && a.contrast == -0.7;
    let mut n = PictureAdjust { grayscale: false, duotone: None, bilevel: None, ..a.clone() };
    if washed {
        n.brightness = 0.0;
        n.contrast = 0.0;
    }
    let rgb = |r, g, b| ColorRef::rgb(Rgba::rgb(r, g, b));
    match preset {
        "none" => {}
        "grayscale" => n.grayscale = true,
        // PowerPoint's sepia is a duotone from dark brown to light cream.
        "sepia" => n.duotone = Some((rgb(0x40, 0x2A, 0x10), rgb(0xF2, 0xE3, 0xC6))),
        // Washout is PowerPoint's +70% brightness, −70% contrast.
        "washout" => {
            n.brightness = 0.7;
            n.contrast = -0.7;
        }
        "bw25" => n.bilevel = Some(0.25),
        "bw50" => n.bilevel = Some(0.5),
        "bw75" => n.bilevel = Some(0.75),
        other => {
            let Some((slot, dark)) = ACCENTS.iter().find_map(|(id, _, slot)| {
                other.strip_prefix(id).and_then(|rest| match rest {
                    "Dark" => Some((*slot, true)),
                    "Light" => Some((*slot, false)),
                    _ => None,
                })
            }) else {
                return false;
            };
            let c = ColorRef::scheme(slot);
            n.duotone = Some(if dark { (rgb(0, 0, 0), c) } else { (c, rgb(0xFF, 0xFF, 0xFF)) });
        }
    }
    *a = n;
    true
}

fn recolor(s: &mut Session, p: &Value) -> Result<Value> {
    let preset = str_param(p, "preset").ok_or_else(|| bad("picture.recolor", "missing `preset`"))?.to_string();
    if !presets().iter().any(|(id, _)| *id == preset) {
        return Err(bad("picture.recolor", format!("unknown preset `{preset}`; see picture.recolors")));
    }
    edit_shapes(s, p, "picture.recolor", |sh| {
        if let ShapeKind::Picture { fill } = &mut sh.kind {
            apply(&mut fill.adjust, &preset);
        }
        Ok(())
    })
}

fn transparent_color(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(at) = p.get("at") {
        let (id, c) = picture_pixel_at(s, p, at)?;
        return edit_shapes(s, &json!({"ids": [id]}), "picture.transparentColor", |sh| {
            if let ShapeKind::Picture { fill } = &mut sh.kind {
                fill.adjust.clear_color = Some(c);
            }
            Ok(())
        });
    }
    let c = if bool_or(p, "none", false) {
        None
    } else {
        let hex = str_param(p, "color").ok_or_else(|| bad("picture.transparentColor", "give `color` (#RRGGBB) or `none: true`"))?;
        Some(Rgba::from_hex(hex.trim_start_matches('#')).ok_or_else(|| bad("picture.transparentColor", format!("bad colour `{hex}`")))?)
    };
    edit_shapes(s, p, "picture.transparentColor", |sh| {
        if let ShapeKind::Picture { fill } = &mut sh.kind {
            fill.adjust.clear_color = c;
        }
        Ok(())
    })
}

/// The topmost target picture under slide point `at` and the colour of its own pixel there: the
/// picture as stored, before corrections and recolouring, so the colour matches the pixels that
/// `clear_color` is compared with.
fn picture_pixel_at(s: &Session, p: &Value, at: &Value) -> Result<(ShapeId, Rgba)> {
    const CMD: &str = "picture.transparentColor";
    let xy = |i: usize| at.get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
    let (Some(x), Some(y)) = (xy(0), xy(1)) else {
        return Err(bad(CMD, "`at` is [x, y] in slide units"));
    };
    let mut ids = targets(s, p)?;
    let st = s.doc()?;
    let shapes = current_shapes(&st.doc, &st.selection).ok_or_else(|| bad(CMD, "no slide"))?;
    // Same order as the hit-test: later in the shape list is in front, and a group's children sit
    // where the group is. The selection order says nothing about what is on top.
    ids.sort_by_cached_key(|id| std::cmp::Reverse(stack_path(shapes, *id)));
    for id in ids {
        let Some((sh, xf, to_slide)) = placed(&st.doc, &st.selection, shapes, id, Affine::IDENTITY, 0) else { continue };
        let ShapeKind::Picture { fill } = &sh.kind else { continue };
        // The camera turns the picture where it is drawn, so the click is mapped back through it too.
        let turn = text::ctx_for(&st.doc, &st.selection).and_then(|c| deckcraft_render::camera_turn(&c, sh, xf.w, xf.h)).unwrap_or(Affine::IDENTITY);
        let local = (to_slide * turn).inverse() * Point::new(x, y);
        if !(local.is_finite() && (0.0..=xf.w).contains(&local.x) && (0.0..=xf.h).contains(&local.y)) {
            continue;
        }
        let PictureMode::Stretch { fill_rect: [l, t, r, b] } = fill.mode else {
            return Err(bad(CMD, "a tiled picture has no single pixel under a point; give `color`"));
        };
        let inner = Rect::new(l * xf.w, t * xf.h, xf.w - r * xf.w, xf.h - b * xf.h);
        let dest = deckcraft_render::crop_dest(inner, fill.crop);
        let img =
            st.doc.media(fill.media).and_then(|m| deckcraft_render::decode_image(&m.data)).ok_or_else(|| bad(CMD, "the picture can't be read"))?;
        let (u, v) = ((local.x - dest.x0) / dest.width(), (local.y - dest.y0) / dest.height());
        if !((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)) {
            continue;
        }
        let (w, h) = (img.width(), img.height());
        let px = ((u * f64::from(w)) as u32).min(w.saturating_sub(1));
        let py = ((v * f64::from(h)) as u32).min(h.saturating_sub(1));
        let Some(c) = img.get_pixel_checked(px, py) else { continue };
        // A transparent pixel keeps a colour nobody sees (often black): clearing it would clear
        // that colour where it is drawn.
        if c[3] == 0 {
            return Err(bad(CMD, "that spot of the picture is already transparent"));
        }
        return Ok((id, Rgba::rgb(c[0], c[1], c[2])));
    }
    Err(bad(CMD, "the point is not on a selected picture"))
}

/// Where `id` sits in the paint order: its index in each list from the slide down to it. A larger
/// path is drawn later, so in front.
fn stack_path(shapes: &[Shape], id: ShapeId) -> Option<Vec<usize>> {
    for (i, sh) in shapes.iter().enumerate() {
        if sh.id == id {
            return Some(vec![i]);
        }
        if let ShapeKind::Group { children, .. } = &sh.kind
            && let Some(mut rest) = stack_path(children, id)
        {
            rest.insert(0, i);
            return Some(rest);
        }
    }
    None
}

/// Shape `id` among `shapes` (searched into groups), its box, and the map from that box to slide
/// space.
fn placed<'a>(
    doc: &deckcraft_model::Presentation,
    sel: &crate::Selection,
    shapes: &'a [Shape],
    id: ShapeId,
    to_slide: Affine,
    depth: usize,
) -> Option<(&'a Shape, Xfrm, Affine)> {
    if depth > 64 {
        return None;
    }
    for sh in shapes {
        let x = xfrm_of(doc, sel, sh);
        if sh.id == id {
            return Some((sh, x, to_slide * x.affine()));
        }
        if let ShapeKind::Group { children, child } = &sh.kind {
            let ch = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
            if let Some(found) = placed(doc, sel, children, id, to_slide * deckcraft_geom::group_child_affine(&x, ch), depth + 1) {
                return Some(found);
            }
        }
    }
    None
}
