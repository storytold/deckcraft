//! Crop mode, as in PowerPoint: Picture Format › Crop (or a double-click on the picture) puts crop
//! handles on the selected picture's frame. Dragging a handle moves that edge of the frame while
//! the picture stays where it is on the slide, so the edge cuts into it (or, dragged outward, pads
//! it); dragging inside the frame moves the picture under the frame. Enter, Escape, Crop again
//! or a click elsewhere ends it. The crop is the picture fill's `srcRect`: the fraction of the
//! picture cut off each side (left, top, right, bottom).

use super::{Drag, Hit, Point, Result, Session, ShapeId, ShapeKind, Value, Xfrm, box_handles, cmd, json, xfrm_of};

/// The handle index for dragging the picture inside the frame (0..8 are the frame's handles: NW,
/// N, NE, E, SE, S, SW, W).
pub const CROP_PAN: u8 = 8;
/// Smallest frame a crop leaves, in points.
const MIN_SIDE: f64 = 4.0;
/// Most a side is padded (negative crop), as `picture.crop` allows.
const MAX_PAD: f64 = 10.0;

impl super::ToolState {
    /// The picture in crop mode, while it is still the one selected (unlocked) picture.
    pub fn cropping(&self, st: &crate::DocState) -> Option<ShapeId> {
        self.crop.filter(|id| {
            st.selection.shapes.as_slice() == [*id]
                && st.selection.text.is_none()
                && st.shape(*id).is_some_and(|sh| !sh.locked && matches!(sh.kind, ShapeKind::Picture { .. }))
        })
    }
}

/// The picture's crop (left, top, right, bottom fractions).
fn crop_of(st: &crate::DocState, id: ShapeId) -> Option<[f64; 4]> {
    match &st.shape(id)?.kind {
        ShapeKind::Picture { fill } => Some(fill.crop),
        _ => None,
    }
}

/// The whole picture under a frame `x` cropped by `crop`, in the frame's own coordinates:
/// (left, top, width, height).
fn picture_box(x: &Xfrm, crop: [f64; 4]) -> (f64, f64, f64, f64) {
    let [l, t, r, b] = crop;
    let w = x.w / (1.0 - l - r).max(0.01);
    let h = x.h / (1.0 - t - b).max(0.01);
    (-l * w, -t * h, w, h)
}

/// The corners of the whole picture (cropped parts included) of frame `x` cropped by `crop`, in
/// slide points, clockwise from the top left: what crop mode shows dimmed around the frame.
pub fn crop_picture_corners(x: &Xfrm, crop: [f64; 4]) -> [Point; 4] {
    let (px, py, w, h) = picture_box(x, crop);
    let a = x.affine();
    [Point::new(px, py), Point::new(px + w, py), Point::new(px + w, py + h), Point::new(px, py + h)].map(|q| a * q)
}

/// The crop of frame `(x0, y0)..(x1, y1)` (in the original frame's coordinates) over the picture
/// box `(px, py, w, h)`.
fn crop_for(frame: (f64, f64, f64, f64), picture: (f64, f64, f64, f64)) -> [f64; 4] {
    let (x0, y0, x1, y1) = frame;
    let (px, py, w, h) = picture;
    let c = [(x0 - px) / w, (y0 - py) / h, (px + w - x1) / w, (py + h - y1) / h];
    c.map(|v| v.clamp(-MAX_PAD, 0.99))
}

/// Frame and crop after dragging crop handle `handle` of frame `orig` (cropped by `crop`) from
/// `start` to `p` (slide points). The picture stays put; the frame's rotation and flips are kept.
pub fn crop_drag(orig: &Xfrm, crop: [f64; 4], handle: u8, start: Point, p: Point) -> (Xfrm, [f64; 4]) {
    let inv = orig.affine().inverse();
    let (q, q0) = (inv * p, inv * start);
    let picture = picture_box(orig, crop);
    if handle == CROP_PAN {
        let d = q - q0;
        let moved = (picture.0 + d.x, picture.1 + d.y, picture.2, picture.3);
        return (*orig, crop_for((0.0, 0.0, orig.w, orig.h), moved));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (0.0, 0.0, orig.w, orig.h);
    // Handles: NW, N, NE, E, SE, S, SW, W.
    let (left, top, right, bottom) = (matches!(handle, 0 | 6 | 7), matches!(handle, 0..=2), matches!(handle, 2..=4), matches!(handle, 4..=6));
    // The edge can't pass the opposite one, nor go further out than the most padding allowed.
    let (min_x, max_x) = (picture.0 - MAX_PAD * picture.2, picture.0 + picture.2 * (1.0 + MAX_PAD));
    let (min_y, max_y) = (picture.1 - MAX_PAD * picture.3, picture.1 + picture.3 * (1.0 + MAX_PAD));
    if left {
        x0 = q.x.clamp(min_x, x1 - MIN_SIDE);
    }
    if right {
        x1 = q.x.clamp(x0 + MIN_SIDE, max_x);
    }
    if top {
        y0 = q.y.clamp(min_y, y1 - MIN_SIDE);
    }
    if bottom {
        y1 = q.y.clamp(y0 + MIN_SIDE, max_y);
    }
    let center = orig.affine() * Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let mut x = *orig;
    (x.w, x.h) = (x1 - x0, y1 - y0);
    (x.x, x.y) = (center.x - x.w / 2.0, center.y - x.h / 2.0);
    (x, crop_for((x0, y0, x1, y1), picture))
}

impl Session {
    /// Crop mode's handles under `p`: a handle of the frame, else inside the frame (the picture).
    pub(super) fn hit_crop(&self, p: Point, tol: f64) -> Option<Hit> {
        let st = self.active()?;
        let id = self.tool.cropping(st)?;
        let x = xfrm_of(&st.doc, &st.selection, st.shape(id)?);
        let (pts, _) = box_handles(&x);
        // The nearest handle in reach: on a small picture they all are.
        let near = pts.iter().enumerate().map(|(i, q)| (i, (*q - p).hypot())).filter(|(_, d)| *d <= tol * 2.0).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = near {
            return Some(Hit::Crop { id, handle: i as u8 });
        }
        let q = x.affine().inverse() * p;
        (q.x >= 0.0 && q.y >= 0.0 && q.x <= x.w && q.y <= x.h).then_some(Hit::Crop { id, handle: CROP_PAN })
    }

    pub(super) fn begin_crop(&mut self, id: ShapeId, handle: u8, p: Point) -> Result<Value> {
        let st = self.doc()?;
        let (Some(sh), Some(crop)) = (st.shape(id), crop_of(st, id)) else { return Ok(Value::Null) };
        let orig = xfrm_of(&st.doc, &st.selection, sh);
        self.begin("Crop");
        self.tool.drag = Some(Drag::Crop { id, handle, orig, crop, start: p });
        Ok(json!({"crop": handle}))
    }

    pub(super) fn drag_crop(&mut self, id: ShapeId, handle: u8, orig: Xfrm, crop: [f64; 4], start: Point, p: Point) -> Result<()> {
        let (x, c) = crop_drag(&orig, crop, handle, start, p);
        if handle != CROP_PAN {
            self.set_xfrm(id, x)?;
        }
        self.edit(|doc, sel| {
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("picture.crop", "no slide"))?;
            if let Some(sh) = deckcraft_model::find_shape_mut(list, id)
                && let ShapeKind::Picture { fill } = &mut sh.kind
            {
                fill.crop = c;
            }
            Ok(())
        })
    }

    /// Leave crop mode (Enter, Escape, a click elsewhere). `true` when it was on.
    pub(super) fn end_crop(&mut self) -> bool {
        self.tool.crop.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn dragging_an_edge_crops_and_the_picture_stays_put() {
        let orig = Xfrm::new(100.0, 100.0, 200.0, 100.0);
        // The right edge in by 50 points: a quarter of the picture is cut off.
        let (x, c) = crop_drag(&orig, [0.0; 4], 3, Point::new(300.0, 150.0), Point::new(250.0, 150.0));
        assert!(close(x.x, 100.0) && close(x.w, 150.0) && close(x.h, 100.0), "{x:?}");
        assert!(close(c[2], 0.25) && close(c[0], 0.0), "{c:?}");
        // The whole picture is where it was.
        let corners = crop_picture_corners(&x, c);
        assert!(close(corners[0].x, 100.0) && close(corners[2].x, 300.0) && close(corners[2].y, 200.0), "{corners:?}");
        // Then the top-left corner in by 10 × 20: the frame starts there, the picture doesn't move.
        let (y, d) = crop_drag(&x, c, 0, Point::new(100.0, 100.0), Point::new(110.0, 120.0));
        assert!(close(y.x, 110.0) && close(y.y, 120.0) && close(y.w, 140.0) && close(y.h, 80.0), "{y:?}");
        assert!(close(d[0], 0.05) && close(d[1], 0.2) && close(d[2], 0.25) && close(d[3], 0.0), "{d:?}");
        assert!(close(crop_picture_corners(&y, d)[0].x, 100.0));
    }

    #[test]
    fn dragging_inside_moves_the_picture_under_the_frame() {
        let orig = Xfrm::new(0.0, 0.0, 100.0, 100.0);
        let crop = [0.25, 0.0, 0.25, 0.0];
        let (x, c) = crop_drag(&orig, crop, CROP_PAN, Point::new(50.0, 50.0), Point::new(60.0, 50.0));
        assert_eq!(x, orig, "the frame stays");
        // The picture (200 wide) moved right by 10: 5 % less cut on the left, 5 % more on the right.
        assert!(close(c[0], 0.2) && close(c[2], 0.3), "{c:?}");
    }

    #[test]
    fn an_edge_stops_short_of_the_other_and_an_outward_drag_pads() {
        let orig = Xfrm::new(0.0, 0.0, 100.0, 50.0);
        let (x, _) = crop_drag(&orig, [0.0; 4], 7, Point::new(0.0, 25.0), Point::new(500.0, 25.0));
        assert!(close(x.w, MIN_SIDE), "{x:?}");
        let (x, c) = crop_drag(&orig, [0.0; 4], 5, Point::new(50.0, 50.0), Point::new(50.0, 75.0));
        assert!(close(x.h, 75.0) && close(c[3], -0.5), "{x:?} {c:?}");
    }

    #[test]
    fn crop_mode_drags_the_handles_of_the_selected_picture() {
        use crate::tools::{Mods, PointerEvent, PointerKind};
        let mut s = Session::with_new();
        let png = deckcraft_render::Image { width: 4, height: 4, pixels: [255, 0, 0, 255].repeat(16) }.to_png();
        s.execute("insert.picture", &json!({"name": "p.png", "data": crate::cmd::base64_encode(&png)})).unwrap();
        let st = s.doc().unwrap();
        let id = st.selection.shapes[0];
        s.execute("shape.setBounds", &json!({"id": id, "x": 100, "y": 100, "w": 200, "h": 120})).unwrap();
        let st = s.doc().unwrap();
        let x = xfrm_of(&st.doc, &st.selection, st.shape(id).unwrap());
        s.execute("picture.cropMode", &json!({})).unwrap();
        assert_eq!(s.tool.cropping(s.doc().unwrap()), Some(id));
        let ev = |kind, p: Point| PointerEvent { kind, x: p.x, y: p.y, mods: Mods::default(), tol: 4.0, pressure: None };
        // The right edge's handle, dragged in by a quarter of the width.
        let (from, to) = (Point::new(x.x + x.w, x.y + x.h / 2.0), Point::new(x.x + x.w * 0.75, x.y + x.h / 2.0));
        s.pointer(ev(PointerKind::Down, from)).unwrap();
        s.pointer(ev(PointerKind::Drag, to)).unwrap();
        s.pointer(ev(PointerKind::Up, to)).unwrap();
        let st = s.doc().unwrap();
        let crop = crop_of(st, id).unwrap();
        assert!(close(crop[2], 0.25), "{crop:?}");
        let nx = xfrm_of(&st.doc, &st.selection, st.shape(id).unwrap());
        assert!(close(nx.w, x.w * 0.75) && close(nx.x, x.x), "{nx:?}");
        // Still cropping; Escape ends it, and Undo takes the crop back in one step.
        assert_eq!(s.tool.cropping(s.doc().unwrap()), Some(id));
        s.key("escape", Mods::default()).unwrap();
        assert_eq!(s.tool.cropping(s.doc().unwrap()), None);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(crop_of(s.doc().unwrap(), id), Some([0.0; 4]));
        // Crop mode wants one picture selected; and once it is deselected, crop mode doesn't come
        // back when it is selected again.
        s.execute("picture.cropMode", &json!({"on": true})).unwrap();
        s.select(|_, sel| sel.shapes.clear()).unwrap();
        assert!(s.execute("picture.cropMode", &json!({"on": true})).is_err());
        s.pointer(ev(PointerKind::Down, Point::new(1.0, 1.0))).unwrap();
        s.pointer(ev(PointerKind::Up, Point::new(1.0, 1.0))).unwrap();
        s.select(|_, sel| sel.shapes = vec![id]).unwrap();
        assert_eq!(s.tool.cropping(s.doc().unwrap()), None);
    }

    #[test]
    fn a_rotated_frame_crops_along_its_own_edges() {
        let mut orig = Xfrm::new(0.0, 0.0, 100.0, 100.0);
        orig.rot = 90.0;
        // Its own right edge is at the bottom on the slide: drag that up by 40.
        let a = orig.affine();
        let (start, to) = (a * Point::new(100.0, 50.0), a * Point::new(60.0, 50.0));
        let (x, c) = crop_drag(&orig, [0.0; 4], 3, start, to);
        assert!(close(x.w, 60.0) && close(x.rot, 90.0), "{x:?}");
        assert!(close(c[2], 0.4), "{c:?}");
    }
}
