use super::{Affine, Hit, Point, Rect, Shape, ShapeId, ShapeKind, TextSel, Vec2, Xfrm, cmd, xfrm_of};
use deckcraft_model::text::RunKind;
use serde_json::json;

pub(super) fn is_text_first(st: &crate::DocState, id: ShapeId) -> bool {
    st.shape(id).is_some_and(|s| s.ph.is_some() || s.text_box)
}

/// The topmost text-holding shape under `p` inside group `sh`, recursing into nested groups.
/// Group children live in the group's child space (`ShapeKind::Group::child`), so the point is
/// mapped through `group_child_affine`, exactly as `hit_shape` does for group hit-testing.
pub(super) fn text_member_at(st: &crate::DocState, sh: &Shape, p: Point, tol: f64) -> Option<ShapeId> {
    let ShapeKind::Group { children, child } = &sh.kind else { return None };
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let ch = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
    member_at(children, deckcraft_geom::group_child_affine(&x, ch), p, tol)
}

/// Depth-first (topmost-first) search of `children` for a text-holding shape under `p`.
/// `to_abs` maps this level's child space to slide space.
fn member_at(children: &[Shape], to_abs: Affine, p: Point, tol: f64) -> Option<ShapeId> {
    let q = to_abs.inverse() * p;
    for c in children.iter().rev() {
        let Some(cx) = c.xfrm else { continue };
        let local = cx.affine().inverse() * q;
        if local.x < -tol || local.y < -tol || local.x > cx.w + tol || local.y > cx.h + tol {
            continue;
        }
        if let ShapeKind::Group { children: gc, child: gch } = &c.kind {
            let gch = Rect::new(gch.x, gch.y, gch.x + gch.w, gch.y + gch.h);
            let nested = to_abs * deckcraft_geom::group_child_affine(&cx, gch);
            if let Some(id) = member_at(gc, nested, p, tol) {
                return Some(id);
            }
        } else if matches!(c.kind, ShapeKind::Shape | ShapeKind::Connector { .. }) && !c.is_line() {
            return Some(c.id);
        }
    }
    None
}

/// The box whose text is being edited on the slide (not speaker notes, not a table cell).
pub(super) struct EditedBox {
    pub(super) id: ShapeId,
    pub(super) xfrm: Xfrm,
    /// Its frame can be grabbed to move it: not a line (no frame to grab) and not locked.
    pub(super) movable: bool,
}

/// Whether a visible shape drawn in front of `target` is under `p`: the shapes above it in its
/// list, topmost first, and inside a group that holds it, the members above it.
pub(super) fn shape_in_front(st: &crate::DocState, shapes: &[Shape], target: ShapeId, p: Point, tol: f64, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    for s in shapes.iter().rev() {
        if s.id == target {
            return false;
        }
        if deckcraft_model::find_shape(std::slice::from_ref(s), target).is_some() {
            // The group holding the text: look among its members.
            return shape_in_front(st, s.children(), target, p, tol, depth + 1);
        }
        if !s.hidden && hit_shape(st, s, p, tol).is_some() {
            return true;
        }
    }
    false
}

pub(super) fn edited_box(st: &crate::DocState) -> Option<EditedBox> {
    let t = st.selection.text.as_ref()?;
    if t.notes || t.cell.is_some() {
        return None;
    }
    let sh = st.shape(t.shape)?;
    Some(EditedBox { id: t.shape, xfrm: xfrm_of(&st.doc, &st.selection, sh), movable: !sh.is_line() && !sh.locked })
}

/// Where a point falls against the box of the text being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditZone {
    /// Well inside: the caret goes there.
    Inside,
    /// On the frame: a band `tol` wide on either side of the box edge.
    Frame,
    Outside,
}

/// Classify slide point `p` against box `x`, in the box's own (rotated, flipped) frame. Inside
/// small boxes the band narrows to a quarter of the box so their text stays reachable.
pub(super) fn edit_zone(x: &Xfrm, p: Point, tol: f64) -> EditZone {
    let l = x.affine().inverse() * p;
    let (w, h) = (x.w.max(0.0), x.h.max(0.0));
    let tol = if tol.is_finite() { tol.max(0.0) } else { 0.0 };
    if !(l.x.is_finite() && l.y.is_finite()) || l.x < -tol || l.y < -tol || l.x > w + tol || l.y > h + tol {
        return EditZone::Outside;
    }
    let inner = tol.min(w / 4.0).min(h / 4.0);
    if l.x > inner && l.y > inner && l.x < w - inner && l.y < h - inner { EditZone::Inside } else { EditZone::Frame }
}

pub(super) fn contains_rect(outer: Rect, inner: Rect) -> bool {
    inner.x0 >= outer.x0 && inner.y0 >= outer.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

pub(super) fn constrain_45(a: Point, b: Point) -> Point {
    let d = b - a;
    let ang = d.y.atan2(d.x);
    let snapped = (ang / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
    let len = d.hypot();
    a + Vec2::new(snapped.cos(), snapped.sin()) * len
}

/// A line-like shape's box for endpoints `a` → `b` (flips encode direction).
pub fn line_xfrm(a: Point, b: Point, _rot: f64) -> Xfrm {
    let r = Rect::from_points(a, b);
    Xfrm { x: r.x0, y: r.y0, w: r.width(), h: r.height(), rot: 0.0, flip_h: b.x < a.x, flip_v: b.y < a.y }
}

/// New box when dragging resize handle `h` by `d` (slide space), keeping the shape's rotation.
pub fn resize_box(o: &Xfrm, h: u8, d: Vec2, keep_aspect: bool, from_center: bool) -> Xfrm {
    // Work in the shape's rotated frame.
    let rot = Affine::rotate(-o.rot.to_radians());
    let ld = rot * Point::new(d.x, d.y);
    let (mut l, mut t, mut r, mut b) = (0.0, 0.0, o.w, o.h);
    let (sx, sy) = (if o.flip_h { -1.0 } else { 1.0 }, if o.flip_v { -1.0 } else { 1.0 });
    let (dx, dy) = (ld.x * sx, ld.y * sy);
    match h {
        0 => {
            l += dx;
            t += dy;
        }
        1 => t += dy,
        2 => {
            r += dx;
            t += dy;
        }
        3 => r += dx,
        4 => {
            r += dx;
            b += dy;
        }
        5 => b += dy,
        6 => {
            l += dx;
            b += dy;
        }
        _ => l += dx,
    }
    if from_center {
        // Symmetric growth about the centre.
        let gw = match h {
            0 | 6 | 7 => -dx,
            2..=4 => dx,
            _ => 0.0,
        };
        let gh = match h {
            0..=2 => -dy,
            4..=6 => dy,
            _ => 0.0,
        };
        l = -gw;
        r = o.w + gw;
        t = -gh;
        b = o.h + gh;
    }
    let mut w = r - l;
    let mut hh = b - t;
    if keep_aspect && o.w > 1e-9 && o.h > 1e-9 && !matches!(h, 1 | 3 | 5 | 7) {
        let ar = o.w / o.h;
        if (w / ar).abs() > hh.abs() {
            hh = w.abs() / ar * hh.signum().max(if hh == 0.0 { 1.0 } else { hh.signum() });
        } else {
            w = hh.abs() * ar * w.signum().max(if w == 0.0 { 1.0 } else { w.signum() });
        }
        if matches!(h, 0 | 6) {
            l = r - w;
        } else {
            r = l + w;
        }
        if matches!(h, 0 | 2) {
            t = b - hh;
        } else {
            b = t + hh;
        }
        if from_center {
            let (cx, cy) = (o.w / 2.0, o.h / 2.0);
            l = cx - w / 2.0;
            r = cx + w / 2.0;
            t = cy - hh / 2.0;
            b = cy + hh / 2.0;
        }
    }
    // Dragging past the opposite edge flips.
    let (mut fh, mut fv) = (o.flip_h, o.flip_v);
    if r < l {
        std::mem::swap(&mut l, &mut r);
        fh = !fh;
    }
    if b < t {
        std::mem::swap(&mut t, &mut b);
        fv = !fv;
    }
    let (nw, nh) = (r - l, b - t);
    // New centre in slide space: old centre + rotated offset of the local centre change.
    let local_c = Point::new((l + r) / 2.0 * sx + if sx < 0.0 { o.w } else { 0.0 }, (t + b) / 2.0 * sy + if sy < 0.0 { o.h } else { 0.0 });
    let c = o.affine() * Point::new(if o.flip_h { o.w - local_c.x } else { local_c.x }, if o.flip_v { o.h - local_c.y } else { local_c.y });
    Xfrm { x: c.x - nw / 2.0, y: c.y - nh / 2.0, w: nw, h: nh, rot: o.rot, flip_h: fh, flip_v: fv }
}

/// Whether `p` lies in a cell of table `id`, not in the grab band around it (`cell_at` would take
/// the nearest cell).
pub(super) fn in_table_cell(st: &crate::DocState, id: ShapeId, p: Point) -> bool {
    st.shape(id).and_then(|sh| cmd::table::table_frame(st, sh)).is_some_and(|f| {
        let q = f.local(p);
        (0.0..=f.width()).contains(&q.x) && (0.0..=f.height()).contains(&q.y) && f.cell_at(q).is_some()
    })
}

pub(super) fn hit_shape(st: &crate::DocState, sh: &Shape, p: Point, tol: f64) -> Option<Hit> {
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let inv = x.affine().inverse();
    let local = inv * p;
    // Table borders resize rows and columns.
    let near = local.x >= -tol && local.y >= -tol && local.x <= x.w + tol && local.y <= x.h + tol;
    if near && let Some(f) = cmd::table::table_frame(st, sh) {
        match f.border_at(f.local(p), tol) {
            Some(cmd::table::TableBorder::Column(col)) => return Some(Hit::TableColumn { id: sh.id, col }),
            Some(cmd::table::TableBorder::Row(row)) => return Some(Hit::TableRow { id: sh.id, row }),
            None => {}
        }
    }
    match &sh.kind {
        ShapeKind::Group { children, child } => {
            // A group is hit when any child is hit; the group is what gets selected.
            let ch = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
            let m = deckcraft_geom::group_child_affine(&x, ch);
            let q = m.inverse() * p;
            for c in children.iter().rev() {
                if let Some(cx) = c.xfrm {
                    let cl = cx.affine().inverse() * q;
                    if cl.x >= -tol && cl.y >= -tol && cl.x <= cx.w + tol && cl.y <= cx.h + tol {
                        return Some(Hit::Shape { id: sh.id, text: false });
                    }
                }
            }
            None
        }
        ShapeKind::Ink { strokes } => {
            for s in strokes {
                if s.points.iter().any(|(px, py, _)| (Point::new(*px, *py) - p).hypot() <= tol + s.width) {
                    return Some(Hit::Shape { id: sh.id, text: false });
                }
            }
            None
        }
        _ => {
            if sh.is_line() {
                let geo = deckcraft_render::shape_geometry(sh, x.w, x.h);
                let lw = sh.line.as_ref().and_then(|l| l.width).unwrap_or(1.0);
                if deckcraft_geom::near_path(&geo.outline(), local, tol.max(lw)) {
                    return Some(Hit::Shape { id: sh.id, text: false });
                }
                return None;
            }
            let inside = local.x >= -tol && local.y >= -tol && local.x <= x.w + tol && local.y <= x.h + tol;
            if !inside {
                return None;
            }
            let has_text = matches!(sh.kind, ShapeKind::Shape) && (sh.text.as_ref().is_some_and(|t| !t.is_empty()) || sh.ph.is_some() || sh.text_box);
            // Empty interiors of unfilled, text-less shapes don't catch clicks.
            let filled = !matches!(sh.fill, Some(deckcraft_model::Fill::None)) || sh.style.is_some();
            let geo = deckcraft_render::shape_geometry(sh, x.w, x.h);
            let in_geo = {
                use deckcraft_geom::KurboShape;
                geo.outline().winding(local) != 0 || deckcraft_geom::near_path(&geo.outline(), local, tol)
            };
            if has_text
                || sh.ph.is_some()
                || matches!(
                    sh.kind,
                    ShapeKind::Picture { .. }
                        | ShapeKind::Table(_)
                        | ShapeKind::Chart(_)
                        | ShapeKind::Media(_)
                        | ShapeKind::Model3d { .. }
                        | ShapeKind::Opaque { .. }
                )
                || (filled && in_geo)
                || deckcraft_geom::near_path(&geo.outline(), local, tol)
            {
                return Some(Hit::Shape { id: sh.id, text: has_text || matches!(sh.kind, ShapeKind::Shape) });
            }
            None
        }
    }
}

/// Text position under slide point `p` in shape `sh`.
pub fn text_pos(st: &crate::DocState, sh: &Shape, p: Point) -> (usize, usize) {
    cmd::text::text_pos_at(st, &TextSel { shape: sh.id, ..Default::default() }, p)
}

/// Row heights of table `t` as drawn: rows grow to fit their text.
pub fn table_rows(st: &crate::DocState, t: &deckcraft_model::Table) -> Vec<f64> {
    cmd::table_drawn_heights(&st.doc, &st.selection, t)
}

/// The equation run (paragraph, run) under slide point `p` in a non-table shape, if any. The
/// equation is one atom whose characters all sit at its edges, so this tests the point against
/// the atom's horizontal extent and its line instead of the nearest caret position.
pub(super) fn math_run_at(st: &crate::DocState, sh: &Shape, p: Point) -> Option<(usize, usize)> {
    let body = sh.text.as_ref()?;
    if !body.paragraphs.iter().any(|q| q.runs.iter().any(|r| matches!(r.kind, RunKind::Math { .. }))) {
        return None;
    }
    // A box that holds nothing but one equation is that equation: any point in it opens the editor.
    if let [q] = body.paragraphs.as_slice()
        && let [r] = q.runs.as_slice()
        && matches!(r.kind, RunKind::Math { .. })
    {
        return Some((0, 0));
    }
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let local = x.affine().inverse() * p;
    let t = TextSel { shape: sh.id, ..Default::default() };
    let l = cmd::text::layout_for(st, &t)?;
    for (pi, q) in body.paragraphs.iter().enumerate() {
        let mut at = 0;
        for (ri, r) in q.runs.iter().enumerate() {
            let end = at + r.char_len();
            if matches!(r.kind, RunKind::Math { .. })
                && let Some((x0, top, bottom)) = l.caret(deckcraft_text::Pos { para: pi, ch: at })
                && let Some((x1, _, _)) = l.caret(deckcraft_text::Pos { para: pi, ch: end })
                && local.x >= x0.min(x1) - 2.0
                && local.x <= x0.max(x1) + 2.0
                && local.y >= top
                && local.y <= bottom
            {
                return Some((pi, ri));
            }
            at = end;
        }
    }
    None
}

/// The request that opens the equation editor on one equation run.
pub(super) fn equation_dialog(id: ShapeId, cell: Option<(usize, usize)>, paragraph: usize, run: usize) -> crate::UiRequest {
    let mut params = json!({"edit": true, "id": id, "paragraph": paragraph, "run": run});
    if let (Some((r, c)), Some(o)) = (cell, params.as_object_mut()) {
        o.insert("cell".into(), json!([r, c]));
    }
    crate::UiRequest::Dialog { id: "equation".into(), params }
}

/// Which table cell contains a table-local point (`heights` from [`table_rows`]).
pub(super) fn deckcraft_render_cell(t: &deckcraft_model::Table, heights: &[f64], p: Point) -> (usize, usize) {
    let mut y = 0.0;
    let mut row = heights.len().saturating_sub(1);
    for (i, h) in heights.iter().enumerate() {
        if p.y < y + h {
            row = i;
            break;
        }
        y += h;
    }
    let mut x = 0.0;
    let mut col = t.cols.len().saturating_sub(1);
    for (i, w) in t.cols.iter().enumerate() {
        if p.x < x + w {
            col = i;
            break;
        }
        x += w;
    }
    (row, col)
}
