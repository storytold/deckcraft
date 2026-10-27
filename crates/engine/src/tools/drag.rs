use super::hit::{constrain_45, line_xfrm, resize_box};
use super::*;

impl Session {
    pub(super) fn drag(&mut self, p: Point, mods: Mods, tol: f64) -> Result<Value> {
        let Some(drag) = self.tool.drag.clone() else { return Ok(Value::Null) };
        match drag {
            Drag::Guide { index } => {
                let vertical = self.active().and_then(|d| d.doc.guides.get(index)).is_some_and(|g| g.vertical);
                let pos = if vertical { p.x } else { p.y };
                // Guides move in whole points unless snapping is off, as in PowerPoint.
                let pos = if self.prefs.snap_to_grid && !mods.alt { pos.round() } else { pos };
                self.tool.guide_drag = Some((index, pos));
            }
            Drag::Move { ids, start, origs, moved, duplicate, enter_text } => {
                let mut d = p - start;
                if !moved && (d.hypot() < tol.max(1.0) || origs.is_empty()) {
                    return Ok(Value::Null);
                }
                if !moved {
                    self.begin(if duplicate { "Duplicate" } else { "Move" });
                    if duplicate {
                        // Alt-drag: drag a copy, leave the original.
                        self.execute_dup_in_place(&ids)?;
                    }
                }
                let ids_now = if duplicate { self.doc()?.selection.shapes.clone() } else { ids.clone() };
                if mods.shift {
                    if d.x.abs() > d.y.abs() {
                        d.y = 0.0;
                    } else {
                        d.x = 0.0;
                    }
                }
                // Smart guides on the moved group's bounds.
                let bounds = origs.iter().map(|(_, x)| x.bounds()).reduce(|a, b| a.union(b)).unwrap_or_default();
                let moved_b = bounds + d;
                let (snap, guides) = self.snap_rect(moved_b, &ids_now, tol * 1.5, !mods.cmd);
                d += snap;
                self.tool.guides = guides;
                let origs2: Vec<(ShapeId, Xfrm)> =
                    if duplicate { ids_now.iter().copied().zip(origs.iter().map(|o| o.1)).collect() } else { origs.clone() };
                self.edit(|doc, sel| {
                    let list = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("move", "no slide"))?;
                    for (id, o) in &origs2 {
                        if let Some(sh) = deckcraft_model::find_shape_mut(list, *id) {
                            sh.xfrm = Some(Xfrm { x: o.x + d.x, y: o.y + d.y, ..*o });
                        }
                    }
                    // A connector dragged away from shapes that stay put comes unglued at those ends.
                    let moved: Vec<ShapeId> = origs2.iter().map(|o| o.0).collect();
                    for sh in list.iter_mut().filter(|s| moved.contains(&s.id)) {
                        if let ShapeKind::Connector { start, end } = &mut sh.kind {
                            for c in [start, end] {
                                if c.is_some_and(|(to, _)| !moved.contains(&to)) {
                                    *c = None;
                                }
                            }
                        }
                    }
                    Ok(())
                })?;
                self.tool.drag =
                    Some(Drag::Move { ids: ids_now, start, origs: origs2, moved: true, duplicate: false, enter_text: enter_text.filter(|_| false) });
            }
            Drag::Resize { id, handle, orig, start } => {
                let nx = resize_box(&orig, handle, p - start, mods.shift || self.is_picture(id), mods.alt);
                let nx = self.snap_resize(nx, &orig, handle, id, tol);
                self.set_xfrm(id, nx)?;
            }
            Drag::LineEnd { id, end, orig } => {
                let a = orig.affine();
                let (p0, p1) = (a * Point::new(0.0, 0.0), a * Point::new(orig.w, orig.h));
                let mut q = p;
                let other = if end == 0 { p1 } else { p0 };
                if mods.shift {
                    q = constrain_45(other, q);
                }
                let glue = self.update_sites(q, tol, Some(id));
                if let Some((_, _, at)) = glue {
                    q = at;
                }
                let (s0, s1) = if end == 0 { (q, p1) } else { (p0, q) };
                self.set_xfrm(id, line_xfrm(s0, s1, orig.rot))?;
                self.edit(|doc, sel| {
                    if let Some(list) = crate::shapes_mut(doc, sel) {
                        crate::connect::set_glue(list, id, end, glue.map(|g| (g.0, g.1)));
                    }
                    Ok(())
                })?;
            }
            Drag::Rotate { ids, center, start, origs } => {
                let ang = (p.y - center.y).atan2(p.x - center.x);
                let mut delta = (ang - start).to_degrees();
                for (id, o) in &origs {
                    let mut r = o.rot + delta;
                    if mods.shift {
                        r = (r / 15.0).round() * 15.0;
                        delta = r - o.rot;
                    }
                    let mut nx = *o;
                    nx.rot = ((r % 360.0) + 360.0) % 360.0;
                    if ids.len() > 1 {
                        // Rotate positions around the common centre too.
                        let c = o.center();
                        let rc = Affine::translate(center.to_vec2()) * Affine::rotate(delta.to_radians()) * Affine::translate(-center.to_vec2()) * c;
                        nx.x = rc.x - o.w / 2.0;
                        nx.y = rc.y - o.h / 2.0;
                    }
                    self.set_xfrm(*id, nx)?;
                }
            }
            Drag::Adjust { id, index, orig } => {
                let st = self.doc()?;
                let Some(sh) = st.shape(id).cloned() else { return Ok(Value::Null) };
                let local = orig.affine().inverse() * p;
                let geo = deckcraft_render::shape_geometry(&sh, orig.w, orig.h);
                let Some(h) = geo.handles.iter().find(|h| h.index == index) else { return Ok(Value::Null) };
                let free2d = matches!(sh.geom.preset_name(), Some(n) if n.contains("Callout") && !n.contains("Arrow"));
                let vals: Vec<(usize, f64)> = if free2d {
                    let dx = (local.x - orig.w / 2.0) / orig.w.max(1e-6) * 1e5;
                    let dy = (local.y - orig.h / 2.0) / orig.h.max(1e-6) * 1e5;
                    vec![(0, dx.round()), (1, dy.round())]
                } else {
                    vec![(index, h.value_at(local))]
                };
                self.edit(|doc, sel| {
                    let list = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("adjust", "no slide"))?;
                    if let Some(sh) = deckcraft_model::find_shape_mut(list, id)
                        && let deckcraft_model::Geom::Preset { name, adj } = &mut sh.geom
                    {
                        let defaults = deckcraft_geom::preset::info(name).map(|x| x.defaults).unwrap_or(&[]);
                        for (i, v) in &vals {
                            while adj.len() <= *i {
                                let k = adj.len();
                                adj.push(defaults.get(k).copied().unwrap_or(0.0));
                            }
                            if let Some(slot) = adj.get_mut(*i) {
                                *slot = *v;
                            }
                        }
                    }
                    Ok(())
                })?;
            }
            Drag::Vertex { id, path, index } => {
                self.tool.drag = Some(Drag::Vertex { id, path, index });
                self.edit_vertex(id, path, index, p)?;
                return Ok(Value::Null);
            }
            Drag::Handle { id, path, index, out } => {
                self.tool.drag = Some(Drag::Handle { id, path, index, out });
                let side = if out { "out" } else { "in" };
                // Shift, Ctrl or Alt held while dragging a handle makes the point smooth, straight or
                // corner, as in PowerPoint; Cmd and Option count as Ctrl and Alt.
                let held = if mods.shift {
                    Some("smooth")
                } else if mods.cmd {
                    Some("straight")
                } else if mods.alt {
                    Some("corner")
                } else {
                    None
                };
                if let Err(e) = self.execute_inner_move_handle(id, path, index, side, p, held) {
                    log::debug!("edit handle: {e}");
                }
                return Ok(Value::Null);
            }
            Drag::Warp { id, index, name, base, orig } => {
                let v = {
                    let st = self.doc()?;
                    let Some(sh) = st.shape(id) else { return Ok(Value::Null) };
                    let tr = deckcraft_render::shape_geometry(sh, orig.w, orig.h).text_rect;
                    let hs = deckcraft_geom::warp::handles(&name, &base, tr);
                    let Some(h) = hs.iter().find(|h| h.index == index) else { return Ok(Value::Null) };
                    h.value_at(orig.affine().inverse() * p)
                };
                self.edit(|doc, sel| {
                    let list = crate::shapes_mut(doc, sel).ok_or_else(|| cmd::bad("adjust", "no slide"))?;
                    if let Some(sh) = deckcraft_model::find_shape_mut(list, id) {
                        set_warp_adj(sh, &name, &base, index, v);
                    }
                    Ok(())
                })?;
            }
            Drag::Marquee { start, .. } => {
                self.tool.marquee = Some(Rect::from_points(start, p));
            }
            Drag::Create { start } => {
                let line = matches!(&self.tool.kind, ToolKind::Shape { preset } if deckcraft_geom::preset::is_line_like(preset));
                let x = if line {
                    let mut q = if mods.shift { constrain_45(start, p) } else { p };
                    if let Some((_, _, at)) = self.update_sites(q, tol, None) {
                        q = at;
                    }
                    let s0 = self.glue_at(start, tol).map(|g| g.2).unwrap_or(start);
                    line_xfrm(s0, q, 0.0)
                } else if matches!(&self.tool.kind, ToolKind::Shape { preset } if preset == DRAW_TABLE)
                    && crate::cmd::table::table_at(self, &Value::Null, (start.x, start.y)).is_some()
                {
                    // The pencil inside a table draws a straight border, across or down.
                    let q = if (p.x - start.x).abs() >= (p.y - start.y).abs() { Point::new(p.x, start.y) } else { Point::new(start.x, p.y) };
                    let r = Rect::from_points(start, q);
                    Xfrm::new(r.x0, r.y0, r.width(), r.height())
                } else {
                    let mut r = Rect::from_points(start, p);
                    if mods.shift {
                        let s = r.width().max(r.height());
                        let sx = if p.x < start.x { -s } else { s };
                        let sy = if p.y < start.y { -s } else { s };
                        r = Rect::from_points(start, start + Vec2::new(sx, sy));
                    }
                    if mods.alt {
                        let d = p - start;
                        r = Rect::new(start.x - d.x.abs(), start.y - d.y.abs(), start.x + d.x.abs(), start.y + d.y.abs());
                    }
                    Xfrm::new(r.x0, r.y0, r.width(), r.height())
                };
                self.tool.preview = Some(x);
            }
            Drag::TextSelect { shape, press, .. } => {
                // In a table, leaving the cell the drag started in selects whole cells, as in
                // PowerPoint; coming back to that cell selects text in it again.
                let st = self.doc()?;
                if let Some(start) = press.as_ref().and_then(|t| t.cell)
                    && let Some(f) = st.shape(shape).and_then(|sh| cmd::table::table_frame(st, sh))
                    && let Some(cell) = f.cell_at(f.local(p))
                {
                    if cell != start {
                        self.select(|_, sel| {
                            sel.text = None;
                            sel.shapes = vec![shape];
                            sel.cells = Some((shape, start, cell));
                        })?;
                        return Ok(Value::Null);
                    }
                    if st.selection.text.is_none() {
                        self.select(|_, sel| {
                            sel.cells = None;
                            sel.text = press;
                        })?;
                    }
                }
                let st = self.doc()?;
                if let Some(t) = st.selection.text.as_ref().filter(|t| t.shape == shape) {
                    let pos = cmd::text::text_pos_at(st, t, p);
                    self.select(|_, sel| {
                        if let Some(t) = sel.text.as_mut() {
                            t.caret = pos;
                        }
                    })?;
                }
            }
            Drag::TableBorder { id, column, index, start, size, frame, moved } => {
                let local = frame.inverse() * p;
                let d = if column { local.x - start.x } else { local.y - start.y };
                if !moved && d.abs() < tol.max(1.0) {
                    return Ok(Value::Null);
                }
                if !moved {
                    self.begin(if column { "Column Width" } else { "Row Height" });
                }
                // An inner column border moves between its two columns (the table keeps its
                // width); the right edge widens the last column and the table. A row's height is
                // a minimum: rows still grow to fit their text.
                if column {
                    let width = (size + d).max(cmd::table::MIN_COLUMN_WIDTH);
                    self.execute("table.columnWidth", &json!({"id": id, "col": index, "width": width, "keepTotal": true}))?;
                } else {
                    self.execute("table.rowHeight", &json!({"id": id, "row": index, "height": (size + d).max(1.0)}))?;
                }
                self.tool.drag = Some(Drag::TableBorder { id, column, index, start, size, frame, moved: true });
            }
            Drag::Freeform { mode, mut points, pressed } => {
                // Dragging scribbles (except for curves, which only take clicked points).
                if pressed && mode != "curve" && points.last().is_none_or(|l| (*l - p).hypot() >= 1.5) {
                    points.push(p);
                }
                let mut v = points.clone();
                if mode == "curve" {
                    v.push(p);
                }
                self.tool.path_preview = v;
                self.tool.drag = Some(Drag::Freeform { mode, points, pressed });
            }
            Drag::Ink { mut points } => {
                points.push(self.ink_sample(p));
                self.tool.ink_preview = points.clone();
                self.tool.drag = Some(Drag::Ink { points });
            }
        }
        Ok(Value::Null)
    }

    /// Moves a vertex during a drag (one undo step for the whole drag).
    fn edit_vertex(&mut self, id: ShapeId, path: usize, index: usize, p: Point) -> Result<()> {
        let r = cmd::points::move_point_raw(self, id, path, index, p);
        if let Err(e) = r {
            log::debug!("edit point: {e}");
        }
        Ok(())
    }

    /// Moves a control handle during a drag. `held` is the point type set by the modifier held
    /// during the drag; the point keeps that type for later drags, as after `shape.pointType`.
    fn execute_inner_move_handle(&mut self, id: ShapeId, path: usize, index: usize, side: &str, p: Point, held: Option<&str>) -> Result<Value> {
        let kind = match held {
            Some(k) => Some(k.to_string()),
            None => self.tool.vertex_kind.clone().filter(|(s, pp, i, _)| (*s, *pp, *i) == (id, path, index)).map(|k| k.3),
        };
        let r =
            cmd::points::move_handle_raw(self, &json!({"id": id.0, "path": path, "index": index, "side": side, "x": p.x, "y": p.y, "type": kind}))?;
        if let Some(k) = held {
            self.tool.vertex_kind = Some((id, path, index, k.to_string()));
        }
        Ok(r)
    }
}
