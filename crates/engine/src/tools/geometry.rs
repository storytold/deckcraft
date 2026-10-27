use super::hit::table_rows;
use super::*;

impl Session {
    /// End the freeform being drawn: create the shape (if it has at least two points).
    pub fn finish_freeform(&mut self, closed: bool) -> Result<Value> {
        let Some(Drag::Freeform { mode, points, .. }) = self.tool.drag.take() else { return Ok(Value::Null) };
        self.tool.path_preview.clear();
        self.tool.path_closing = false;
        if !self.tool.sticky {
            self.tool.kind = ToolKind::Select;
        }
        if points.len() < 2 {
            return Ok(Value::Null);
        }
        let pts: Vec<[f64; 2]> = points.iter().map(|q| [q.x, q.y]).collect();
        self.execute("shape.freeform", &json!({"points": pts, "closed": closed, "smooth": mode == "curve"}))
    }

    /// The connection site within glue range of `p` (excluding shape `exclude`).
    pub(super) fn glue_at(&self, p: Point, tol: f64) -> Option<(ShapeId, u32, Point)> {
        let st = self.active()?;
        crate::connect::nearest_site(&st.doc, &st.selection, st.shapes(), p, (tol * 2.0).max(6.0), None)
    }

    /// Show the sites of the shape near `p` and return the site an end at `p` would glue to.
    pub(super) fn update_sites(&mut self, p: Point, tol: f64, exclude: Option<ShapeId>) -> Option<(ShapeId, u32, Point)> {
        let (sites, glue) = match self.active() {
            Some(st) => {
                let shapes = st.shapes();
                let near = crate::connect::site_shape_at(&st.doc, &st.selection, shapes, p, tol * 3.0, exclude);
                let sites = near
                    .and_then(|id| shapes.iter().find(|s| s.id == id))
                    .map(|s| crate::connect::sites(&st.doc, &st.selection, s))
                    .unwrap_or_default();
                (sites, crate::connect::nearest_site(&st.doc, &st.selection, shapes, p, (tol * 2.0).max(6.0), exclude))
            }
            None => (vec![], None),
        };
        self.tool.sites = sites;
        self.tool.glue = glue.map(|g| g.2);
        glue
    }

    pub(super) fn set_xfrm(&mut self, id: ShapeId, x: Xfrm) -> Result<()> {
        // Rows scale from their drawn heights: a stored height is only a minimum (often 0).
        let drawn = self.active().and_then(|st| match &st.shape(id)?.kind {
            ShapeKind::Table(t) => Some(table_rows(st, t)),
            _ => None,
        });
        self.edit(|doc, sel| {
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("tool", "no slide"))?;
            if let Some(sh) = deckcraft_model::find_shape_mut(list, id) {
                sh.xfrm = Some(x);
                if let ShapeKind::Table(t) = &mut sh.kind {
                    // Tables resize their columns and rows proportionally.
                    let heights: Vec<f64> = match &drawn {
                        Some(d) if d.len() == t.rows.len() => d.clone(),
                        _ => t.rows.iter().map(|r| r.height).collect(),
                    };
                    let (tw, th) = (t.width().max(1e-6), heights.iter().map(|h| h.max(0.0)).sum::<f64>());
                    let kx = x.w / tw;
                    t.cols.iter_mut().for_each(|c| *c *= kx);
                    if th > 1e-6 {
                        let ky = x.h / th;
                        t.rows.iter_mut().zip(&heights).for_each(|(r, h)| r.height = h.max(0.0) * ky);
                    }
                }
            }
            Ok(())
        })
    }

    pub(super) fn set_xfrm_cmd(&mut self, id: ShapeId, x: Xfrm) -> Result<Value> {
        self.execute("shape.setBounds", &json!({"id": id, "x": x.x, "y": x.y, "w": x.w, "h": x.h}))?;
        if x.flip_h || x.flip_v {
            self.edit(|doc, sel| {
                if let Some(sh) = crate::shapes_mut(doc, sel).and_then(|l| deckcraft_model::find_shape_mut(l, id))
                    && let Some(xx) = sh.xfrm.as_mut()
                {
                    xx.flip_h = x.flip_h;
                    xx.flip_v = x.flip_v;
                }
                Ok(())
            })?;
        }
        Ok(Value::Null)
    }

    pub(super) fn refit_after_resize(&mut self, id: ShapeId) -> Result<()> {
        // A text box resized by hand keeps wrapping at its new width.
        let st = self.doc()?;
        if st.shape(id).is_some_and(|s| s.text_box) {
            let _ = cmd::text::fit_text_box(self, id);
        }
        Ok(())
    }

    pub(super) fn execute_dup_in_place(&mut self, ids: &[ShapeId]) -> Result<()> {
        self.edit(|doc, sel| {
            let list = cmd::current_shapes(doc, sel).cloned().unwrap_or_default();
            let mut copies: Vec<Shape> = list.iter().filter(|x| ids.contains(&x.id)).cloned().collect();
            let boxes: Vec<Xfrm> = copies.iter().map(|c| xfrm_of(doc, sel, c)).collect();
            cmd::edit::reid("edit.duplicate", doc, &mut copies)?;
            for (c, b) in copies.iter_mut().zip(boxes) {
                c.xfrm = Some(b);
                c.ph = None;
            }
            let new_ids: Vec<ShapeId> = copies.iter().map(|c| c.id).collect();
            let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("duplicate", "no slide"))?;
            shapes.extend(copies);
            sel.shapes = new_ids;
            Ok(())
        })
    }

    /// The drawing guide within `tol` of `p` (only while guides are shown).
    pub fn guide_at(&self, p: Point, tol: f64) -> Option<usize> {
        if !self.tool.guides_visible {
            return None;
        }
        let st = self.active()?;
        st.doc
            .guides
            .iter()
            .enumerate()
            .map(|(i, g)| (i, if g.vertical { (p.x - g.pos).abs() } else { (p.y - g.pos).abs() }))
            .filter(|(_, d)| *d <= tol.max(2.0))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Smart guides: snap `r` edges/centres to the slide and other shapes. Returns the offset to
    /// apply and the guides to draw.
    pub(super) fn snap_rect(&self, r: Rect, exclude: &[ShapeId], tol: f64, enabled: bool) -> (Vec2, Vec<Guide>) {
        self.snap_edges(r, exclude, tol, enabled, [true; 3], [true; 3])
    }

    /// `snap_rect` looking only at some of `r`'s edges: `xs_on` is left, centre, right and `ys_on`
    /// top, centre, bottom. Resizing passes just the edges being dragged, so a fixed edge that
    /// already sits on a line (distance 0) doesn't win and leave the dragged edge unsnapped.
    fn snap_edges(&self, r: Rect, exclude: &[ShapeId], tol: f64, enabled: bool, xs_on: [bool; 3], ys_on: [bool; 3]) -> (Vec2, Vec<Guide>) {
        let Some(st) = self.active() else { return (Vec2::ZERO, vec![]) };
        if !enabled || !self.prefs.smart_guides {
            return (Vec2::ZERO, vec![]);
        }
        let size = st.doc.slide_size;
        let mut xs: Vec<(f64, Rect)> = vec![
            (0.0, Rect::new(0.0, 0.0, 0.0, size.height)),
            (size.width / 2.0, Rect::new(size.width / 2.0, 0.0, size.width / 2.0, size.height)),
            (size.width, Rect::new(size.width, 0.0, size.width, size.height)),
        ];
        let mut ys: Vec<(f64, Rect)> = vec![
            (0.0, Rect::new(0.0, 0.0, size.width, 0.0)),
            (size.height / 2.0, Rect::new(0.0, size.height / 2.0, size.width, size.height / 2.0)),
            (size.height, Rect::new(0.0, size.height, size.width, size.height)),
        ];
        if self.tool.guides_visible {
            for g in &st.doc.guides {
                if g.vertical {
                    xs.push((g.pos, Rect::new(g.pos, 0.0, g.pos, size.height)));
                } else {
                    ys.push((g.pos, Rect::new(0.0, g.pos, size.width, g.pos)));
                }
            }
        }
        for sh in st.shapes() {
            if exclude.contains(&sh.id) || sh.hidden {
                continue;
            }
            let b = xfrm_of(&st.doc, &st.selection, sh).bounds();
            for x in [b.x0, b.center().x, b.x1] {
                xs.push((x, b));
            }
            for y in [b.y0, b.center().y, b.y1] {
                ys.push((y, b));
            }
        }
        let mut best_x: Option<(f64, f64, Rect)> = None;
        for edge in [r.x0, r.center().x, r.x1].into_iter().zip(xs_on).filter_map(|(e, on)| on.then_some(e)) {
            for (x, b) in &xs {
                let d = x - edge;
                if d.abs() <= tol && best_x.is_none_or(|(bd, _, _)| d.abs() < bd.abs()) {
                    best_x = Some((d, *x, *b));
                }
            }
        }
        let mut best_y: Option<(f64, f64, Rect)> = None;
        for edge in [r.y0, r.center().y, r.y1].into_iter().zip(ys_on).filter_map(|(e, on)| on.then_some(e)) {
            for (y, b) in &ys {
                let d = y - edge;
                if d.abs() <= tol && best_y.is_none_or(|(bd, _, _)| d.abs() < bd.abs()) {
                    best_y = Some((d, *y, *b));
                }
            }
        }
        let mut off = Vec2::ZERO;
        let mut guides = vec![];
        if let Some((d, x, b)) = best_x {
            off.x = d;
            let rr = r + Vec2::new(d, 0.0);
            guides.push(Guide { vertical: true, pos: x, from: b.y0.min(rr.y0), to: b.y1.max(rr.y1) });
        } else if self.prefs.snap_to_grid && xs_on.contains(&true) {
            let at = if xs_on[0] {
                r.x0
            } else if xs_on[2] {
                r.x1
            } else {
                r.center().x
            };
            off.x = deckcraft_geom::snap(at, self.prefs.grid_spacing) - at;
        }
        if let Some((d, y, b)) = best_y {
            off.y = d;
            let rr = r + Vec2::new(0.0, d);
            guides.push(Guide { vertical: false, pos: y, from: b.x0.min(rr.x0), to: b.x1.max(rr.x1) });
        } else if self.prefs.snap_to_grid && ys_on.contains(&true) {
            let at = if ys_on[0] {
                r.y0
            } else if ys_on[2] {
                r.y1
            } else {
                r.center().y
            };
            off.y = deckcraft_geom::snap(at, self.prefs.grid_spacing) - at;
        }
        (off, guides)
    }

    /// Snap the edges that resize handle `handle` drags in the resized box `x` to the smart guides.
    pub(super) fn snap_resize(&mut self, x: Xfrm, _orig: &Xfrm, handle: u8, id: ShapeId, tol: f64) -> Xfrm {
        if x.rot != 0.0 {
            self.tool.guides.clear();
            return x;
        }
        let r = x.rect();
        // Only the dragged edges move, so only they look for a line to snap to. The handles sit in
        // the box's unflipped frame: in a flipped box, also one that a drag past the opposite edge
        // just flipped, the dragged handle is on the other side.
        let (left, right) = (matches!(handle, 0 | 6 | 7), matches!(handle, 2..=4));
        let (top, bottom) = (matches!(handle, 0..=2), matches!(handle, 4..=6));
        let (moves_left, moves_right) = if x.flip_h { (right, left) } else { (left, right) };
        let (moves_top, moves_bottom) = if x.flip_v { (bottom, top) } else { (top, bottom) };
        let (off, guides) = self.snap_edges(r, &[id], tol * 1.5, true, [moves_left, false, moves_right], [moves_top, false, moves_bottom]);
        let mut nx = x;
        if moves_left {
            nx.x += off.x;
            nx.w -= off.x;
        } else if moves_right {
            nx.w += off.x;
        }
        if moves_top {
            nx.y += off.y;
            nx.h -= off.y;
        } else if moves_bottom {
            nx.h += off.y;
        }
        nx.w = nx.w.max(0.0);
        nx.h = nx.h.max(0.0);
        self.tool.guides = guides;
        nx
    }

    /// One ink sample. No device reading is full pen width (`1`), not the old placeholder `0.5`.
    pub(super) fn ink_sample(&self, p: Point) -> (f64, f64, f32) {
        let pressure = match self.tool.pressure {
            Some(v) if v.is_finite() => v.clamp(0.0, 1.0),
            _ => 1.0,
        };
        (p.x, p.y, pressure)
    }

    pub(super) fn add_ink(&mut self, points: Vec<(f64, f64, f32)>) -> Result<Value> {
        let ToolKind::Ink { mode, color, width } = self.tool.kind.clone() else { return Ok(Value::Null) };
        let stroke = deckcraft_model::InkStroke { points, color, width, highlighter: mode == "highlighter" };
        // Strokes join the slide's ink layer (one ink shape per slide).
        let existing = self.doc()?.shapes().iter().rev().find(|s| matches!(s.kind, ShapeKind::Ink { .. })).map(|s| s.id);
        self.edit(|doc, sel| {
            let id = match existing {
                Some(id) => id,
                None => {
                    let id = ShapeId(doc.alloc_id().ok_or_else(|| cmd::bad("ink.draw", cmd::NO_ID_SPACE))?);
                    let size = doc.slide_size;
                    let list = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("ink", "no slide"))?;
                    list.push(Shape {
                        id,
                        name: "Ink".into(),
                        xfrm: Some(Xfrm::new(0.0, 0.0, size.width, size.height)),
                        kind: ShapeKind::Ink { strokes: vec![] },
                        ..Default::default()
                    });
                    id
                }
            };
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("ink", "no slide"))?;
            if let Some(sh) = deckcraft_model::find_shape_mut(list, id)
                && let ShapeKind::Ink { strokes } = &mut sh.kind
            {
                strokes.push(stroke);
            }
            Ok(json!({"ink": id}))
        })
    }

    pub(super) fn erase_at(&mut self, p: Point, tol: f64) -> Result<Value> {
        let st = self.doc()?;
        let Some(ink) = st.shapes().iter().rev().find(|s| matches!(s.kind, ShapeKind::Ink { .. })).map(|s| s.id) else { return Ok(Value::Null) };
        let before = st.doc.clone();
        let sel_before = st.selection.clone();
        self.edit(|doc, sel| {
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("ink", "no slide"))?;
            if let Some(sh) = deckcraft_model::find_shape_mut(list, ink)
                && let ShapeKind::Ink { strokes } = &mut sh.kind
            {
                strokes.retain(|s| !s.points.iter().any(|(x, y, _)| (Point::new(*x, *y) - p).hypot() <= tol * 2.0 + s.width));
            }
            Ok(())
        })?;
        let vertex_kind = self.tool.vertex_kind.clone();
        if let Some(st) = self.active_mut() {
            crate::push_undo(st, HistoryEntry { label: "Erase".into(), doc: before, selection: sel_before, vertex_kind });
        }
        Ok(Value::Null)
    }
}
