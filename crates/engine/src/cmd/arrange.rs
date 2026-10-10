//! Arrange: order, group, align, distribute, rotate and flip.

use deckcraft_geom::{Point, Rect, Xfrm};
use deckcraft_model::{Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("arrange.bringToFront", "Bring to Front", ["Arrange"], Some("Cmd+Shift+]"), "{ids?}", has_selection, |s, p| order(s, p, 2)),
        cmd!("arrange.bringForward", "Bring Forward", ["Arrange"], Some("Cmd+]"), "{ids?}", has_selection, |s, p| order(s, p, 1)),
        cmd!("arrange.sendBackward", "Send Backward", ["Arrange"], Some("Cmd+["), "{ids?}", has_selection, |s, p| order(s, p, -1)),
        cmd!("arrange.sendToBack", "Send to Back", ["Arrange"], Some("Cmd+Shift+["), "{ids?}", has_selection, |s, p| order(s, p, -2)),
        cmd!("arrange.reorder", "Reorder Objects", ["Arrange"], None, "{id, index: position in the stack (0 = back)}", has_slide, reorder),
        cmd!("arrange.group", "Group", ["Arrange"], Some("Cmd+Alt+G"), "{ids?}", has_two, group),
        cmd!("arrange.ungroup", "Ungroup", ["Arrange"], Some("Cmd+Alt+Shift+G"), "{ids?}", has_selection, ungroup),
        cmd!("arrange.regroup", "Regroup", ["Arrange"], Some("Cmd+Alt+J"), "{}", has_doc, regroup),
        cmd!(
            "arrange.align",
            "Align",
            ["Arrange", "Align or Distribute"],
            None,
            "{edge: left|center|right|top|middle|bottom, to?: slide|selection (default: slide for one object), ids?}",
            has_selection,
            align
        ),
        cmd!(
            "arrange.distribute",
            "Distribute",
            ["Arrange", "Align or Distribute"],
            None,
            "{dir: horizontal|vertical, to?: slide|selection, ids?}",
            has_selection,
            distribute
        ),
        cmd!("arrange.rotateLeft", "Rotate Left 90°", ["Arrange", "Rotate or Flip"], None, "{ids?}", has_selection, |s, p| s
            .execute("shape.rotate", &with_param(p, "by", json!(-90.0)))),
        cmd!("arrange.rotateRight", "Rotate Right 90°", ["Arrange", "Rotate or Flip"], None, "{ids?}", has_selection, |s, p| s
            .execute("shape.rotate", &with_param(p, "by", json!(90.0)))),
        cmd!("arrange.flipHorizontal", "Flip Horizontal", ["Arrange", "Rotate or Flip"], None, "{ids?}", has_selection, |s, p| s
            .execute("shape.flip", &with_param(p, "axis", json!("horizontal")))),
        cmd!("arrange.flipVertical", "Flip Vertical", ["Arrange", "Rotate or Flip"], None, "{ids?}", has_selection, |s, p| s
            .execute("shape.flip", &with_param(p, "axis", json!("vertical")))),
        cmd!("arrange.nudge", "Nudge", [], None, "{dx, dy, ids?} (arrow keys: 6 pt, with Alt 1 pt)", has_selection, |s, p| s
            .execute("shape.move", p)),
    ]
}

fn order(s: &mut Session, p: &Value, how: i32) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("arrange", "no slide"))?;
        match how {
            2 => {
                let (mut moved, rest): (Vec<Shape>, Vec<Shape>) = list.drain(..).partition(|x| ids.contains(&x.id));
                list.extend(rest);
                list.append(&mut moved);
            }
            -2 => {
                let (mut moved, rest): (Vec<Shape>, Vec<Shape>) = list.drain(..).partition(|x| ids.contains(&x.id));
                moved.extend(rest);
                *list = moved;
            }
            1 => {
                for i in (0..list.len().saturating_sub(1)).rev() {
                    let (a, b) = (list.get(i).map(|x| ids.contains(&x.id)), list.get(i + 1).map(|x| ids.contains(&x.id)));
                    if a == Some(true) && b == Some(false) {
                        list.swap(i, i + 1);
                    }
                }
            }
            _ => {
                for i in 1..list.len() {
                    let (a, b) = (list.get(i - 1).map(|x| ids.contains(&x.id)), list.get(i).map(|x| ids.contains(&x.id)));
                    if a == Some(false) && b == Some(true) {
                        list.swap(i - 1, i);
                    }
                }
            }
        }
        Ok(())
    })?;
    ok()
}

fn reorder(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id", "arrange.reorder")?.ok_or_else(|| bad("arrange.reorder", "missing `id`"))?;
    let to = usize_param(p, "index").ok_or_else(|| bad("arrange.reorder", "missing `index`"))?;
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("arrange.reorder", "no slide"))?;
        let i = list.iter().position(|x| x.id == id).ok_or_else(|| bad("arrange.reorder", "not a top-level shape"))?;
        let sh = list.remove(i);
        list.insert(to.min(list.len()), sh);
        Ok(())
    })?;
    ok()
}

/// Bounds of shapes (rotation included).
fn union_bounds(xs: &[Xfrm]) -> Option<Rect> {
    let mut it = xs.iter().map(Xfrm::bounds);
    let first = it.next()?;
    Some(it.fold(first, |a, b| a.union(b)))
}

fn group(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|doc, sel| {
        let current = super::current_shapes(doc, sel).cloned().unwrap_or_default();
        let mut members: Vec<Shape> = current.iter().filter(|x| ids.contains(&x.id)).cloned().collect();
        if members.len() < 2 {
            return Err(bad("arrange.group", "select two or more objects on the slide"));
        }
        for m in &mut members {
            if m.xfrm.is_none() {
                m.xfrm = Some(xfrm_of(doc, sel, m));
            }
            // Placeholders lose their link when grouped (PowerPoint keeps them out of groups).
            m.ph = None;
        }
        let xs: Vec<Xfrm> = members.iter().filter_map(|m| m.xfrm).collect();
        let b = union_bounds(&xs).ok_or_else(|| bad("arrange.group", "nothing to group"))?;
        let gx = Xfrm::new(b.x0, b.y0, b.width(), b.height());
        let id = ShapeId(doc.alloc_id());
        let n = super::current_shapes(doc, sel).map(|v| v.len()).unwrap_or(0);
        let g = Shape {
            id,
            name: format!("Group {}", n + 1),
            xfrm: Some(gx),
            kind: ShapeKind::Group { children: members, child: gx },
            ..Default::default()
        };
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("arrange.group", "no slide"))?;
        let at = list.iter().rposition(|x| ids.contains(&x.id)).unwrap_or(list.len());
        list.insert(at + 1, g);
        list.retain(|x| !ids.contains(&x.id));
        sel.shapes = vec![id];
        Ok(json!({"id": id}))
    })
}

/// A child's xfrm in slide space, given its group.
pub(crate) fn child_to_parent(group: &Xfrm, ch: &Xfrm, c: &Xfrm) -> Xfrm {
    let sx = if ch.w.abs() > 1e-9 { group.w / ch.w } else { 1.0 };
    let sy = if ch.h.abs() > 1e-9 { group.h / ch.h } else { 1.0 };
    let a = deckcraft_geom::group_child_affine(group, Rect::new(ch.x, ch.y, ch.x + ch.w, ch.y + ch.h));
    let center = a * Point::new(c.x + c.w / 2.0, c.y + c.h / 2.0);
    let (w, h) = (c.w * sx.abs(), c.h * sy.abs());
    let mut rot = c.rot + group.rot;
    let (mut fh, mut fv) = (c.flip_h, c.flip_v);
    if group.flip_h {
        fh = !fh;
        rot = -c.rot + group.rot;
    }
    if group.flip_v {
        fv = !fv;
        rot = -rot + 2.0 * group.rot;
    }
    Xfrm { x: center.x - w / 2.0, y: center.y - h / 2.0, w, h, rot: ((rot % 360.0) + 360.0) % 360.0, flip_h: fh, flip_v: fv }
}

fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("arrange.ungroup", "no slide"))?;
        let mut out = Vec::with_capacity(list.len());
        let mut new_sel = vec![];
        for sh in list.drain(..) {
            if ids.contains(&sh.id)
                && let (ShapeKind::Group { children, child }, Some(gx)) = (&sh.kind, sh.xfrm)
            {
                for c in children {
                    let mut c = c.clone();
                    if let Some(cx) = c.xfrm {
                        c.xfrm = Some(child_to_parent(&gx, child, &cx));
                    }
                    // Nested groups scale too: their child space stays, their box is mapped.
                    new_sel.push(c.id);
                    out.push(c);
                }
                continue;
            }
            out.push(sh);
        }
        *list = out;
        sel.shapes = new_sel.clone();
        Ok(json!({"ids": new_sel}))
    })
}

fn regroup(s: &mut Session, _p: &Value) -> Result<Value> {
    // Regroup the most recently ungrouped objects (from the undo history).
    let st = s.doc()?;
    let last = st.history.undo.iter().rev().find(|e| e.label == "Ungroup").map(|e| e.selection.shapes.clone());
    match last {
        Some(prev_group) => {
            // The ungroup's selection held the group id; its children are the current selection when nothing else changed.
            let _ = prev_group;
            let ids = s.doc()?.selection.shapes.clone();
            if ids.len() >= 2 { s.execute("arrange.group", &json!({"ids": ids})) } else { Err(bad("arrange.regroup", "nothing to regroup")) }
        }
        None => Err(bad("arrange.regroup", "nothing to regroup")),
    }
}

fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let edge = str_param(p, "edge").unwrap_or("left").to_string();
    let ids = targets(s, p)?;
    let st = s.doc()?;
    let size = st.doc.slide_size;
    let boxes: Vec<(ShapeId, Xfrm)> = ids.iter().filter_map(|id| st.shape(*id).map(|x| (*id, xfrm_of(&st.doc, &st.selection, x)))).collect();
    let to_slide = match str_param(p, "to") {
        Some("slide") => true,
        Some(_) => false,
        None => boxes.len() < 2,
    };
    let reference = if to_slide {
        Rect::new(0.0, 0.0, size.width, size.height)
    } else {
        union_bounds(&boxes.iter().map(|b| b.1).collect::<Vec<_>>()).unwrap_or_default()
    };
    edit_shapes(s, p, "arrange.align", |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            let b = x.bounds();
            let (dx, dy) = match edge.as_str() {
                "left" => (reference.x0 - b.x0, 0.0),
                "center" => (reference.center().x - b.center().x, 0.0),
                "right" => (reference.x1 - b.x1, 0.0),
                "top" => (0.0, reference.y0 - b.y0),
                "middle" => (0.0, reference.center().y - b.center().y),
                "bottom" => (0.0, reference.y1 - b.y1),
                _ => (0.0, 0.0),
            };
            x.x += dx;
            x.y += dy;
        }
        Ok(())
    })
}

fn distribute(s: &mut Session, p: &Value) -> Result<Value> {
    let horiz = !str_param(p, "dir").unwrap_or("horizontal").starts_with('v');
    let ids = targets(s, p)?;
    let st = s.doc()?;
    let size = st.doc.slide_size;
    let mut boxes: Vec<(ShapeId, Rect)> =
        ids.iter().filter_map(|id| st.shape(*id).map(|x| (*id, xfrm_of(&st.doc, &st.selection, x).bounds()))).collect();
    let to_slide = str_param(p, "to") == Some("slide") || boxes.len() < 3;
    if boxes.is_empty() {
        return ok();
    }
    boxes.sort_by(|a, b| if horiz { a.1.x0.total_cmp(&b.1.x0) } else { a.1.y0.total_cmp(&b.1.y0) });
    let total: f64 = boxes.iter().map(|b| if horiz { b.1.width() } else { b.1.height() }).sum();
    let (start, end) = if to_slide {
        if horiz { (0.0, size.width) } else { (0.0, size.height) }
    } else {
        let f = boxes.first().map(|b| b.1).unwrap_or_default();
        let l = boxes.last().map(|b| b.1).unwrap_or_default();
        if horiz { (f.x0, l.x1) } else { (f.y0, l.y1) }
    };
    let n = boxes.len();
    let gap = if n > 1 { ((end - start) - total) / (n - 1) as f64 } else { 0.0 };
    let mut targets_pos: Vec<(ShapeId, f64)> = vec![];
    let mut cur = start;
    if n == 1 {
        let b = boxes.first().map(|b| b.1).unwrap_or_default();
        cur = start + ((end - start) - if horiz { b.width() } else { b.height() }) / 2.0;
    }
    for (id, b) in &boxes {
        let d = cur - if horiz { b.x0 } else { b.y0 };
        targets_pos.push((*id, d));
        cur += (if horiz { b.width() } else { b.height() }) + gap;
    }
    edit_shapes(s, p, "arrange.distribute", |sh| {
        if let (Some(x), Some((_, d))) = (sh.xfrm.as_mut(), targets_pos.iter().find(|(i, _)| *i == sh.id)) {
            if horiz {
                x.x += d;
            } else {
                x.y += d;
            }
        }
        Ok(())
    })
}
