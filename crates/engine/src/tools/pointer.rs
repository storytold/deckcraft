use super::hit::{EditZone, contains_rect, edit_zone, edited_box, hit_shape, in_table_cell, is_text_first, shape_in_front};
use super::*;

impl Session {
    pub fn set_tool(&mut self, kind: ToolKind) {
        self.tool.kind = kind;
        self.tool.drag = None;
        self.tool.preview = None;
        self.tool.marquee = None;
    }

    /// What's under `p` on the current slide.
    pub fn hit_test(&self, p: Point, tol: f64) -> Option<Hit> {
        let st = self.active()?;
        // In Edit Points, the edited shape's vertices come first.
        if let Some(id) = self.tool.editing_points(st)
            && let Some(sh) = st.shape(id)
        {
            if let Some((path, index)) = self.tool.edit_vertex
                && let Some((hin, hout, _)) = cmd::points::slide_handles(&st.doc, &st.selection, sh, path, index)
            {
                for (h, out) in [(hout, true), (hin, false)] {
                    if h.is_some_and(|q| (q - p).hypot() <= tol * 1.6) {
                        return Some(Hit::Handle { id, path, index, out });
                    }
                }
            }
            if let Some((path, index)) = cmd::points::vertex_at(&st.doc, &st.selection, sh, p, tol * 1.6) {
                return Some(Hit::Vertex { id, path, index });
            }
        }
        // Handles of selected shapes first.
        if st.selection.text.is_none() || st.selection.shapes.len() == 1 {
            for id in st.selection.shapes.iter().rev() {
                let Some(sh) = st.shape(*id) else { continue };
                // A locked object shows no live handles: it can't be sized, rotated or reshaped.
                if sh.locked {
                    continue;
                }
                let x = xfrm_of(&st.doc, &st.selection, sh);
                if sh.is_line() {
                    let a = x.affine();
                    for (end, q) in [(0u8, a * Point::new(0.0, 0.0)), (1u8, a * Point::new(x.w, x.h))] {
                        if (q - p).hypot() <= tol * 1.6 {
                            return Some(Hit::LineEnd { id: *id, end });
                        }
                    }
                    continue;
                }
                let geo = deckcraft_render::shape_geometry(sh, x.w, x.h);
                for hd in &geo.handles {
                    if (x.affine() * hd.pos - p).hypot() <= tol * 1.6 {
                        return Some(Hit::Adjust { id: *id, index: hd.index });
                    }
                }
                for hd in wordart_handles(&st.doc, &st.selection, sh, x.w, x.h) {
                    if (x.affine() * hd.pos - p).hypot() <= tol * 1.6 {
                        return Some(Hit::Warp { id: *id, index: hd.index });
                    }
                }
                let (pts, rot) = box_handles(&x);
                if (rot - p).hypot() <= tol * 1.8 && st.selection.text.is_none() {
                    return Some(Hit::Rotate { id: *id });
                }
                for (i, q) in pts.iter().enumerate() {
                    if (*q - p).hypot() <= tol * 1.6 {
                        return Some(Hit::Resize { id: *id, handle: i as u8 });
                    }
                }
            }
        }
        // The frame of the text being edited: grabbing it moves the box, unless a shape in front
        // of the text covers that spot (#931).
        if let Some(e) = edited_box(st)
            && e.movable
            && edit_zone(&e.xfrm, p, tol) == EditZone::Frame
            && !shape_in_front(st, st.shapes(), e.id, p, tol, 0)
        {
            return Some(Hit::Frame { id: e.id });
        }
        // Shapes, topmost first.
        // Locked objects are still picked (to format them or type in them); they just don't move.
        for sh in st.shapes().iter().rev() {
            if sh.hidden {
                continue;
            }
            if let Some(h) = hit_shape(st, sh, p, tol) {
                return Some(h);
            }
        }
        None
    }

    /// Feed a pointer event to the active tool. Returns what happened (for agents).
    pub fn pointer(&mut self, ev: PointerEvent) -> Result<Value> {
        let tol = if ev.tol > 0.0 { ev.tol } else { 4.0 };
        let p = Point::new(ev.x, ev.y);
        self.tool.pressure = ev.pressure.filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0));
        if !(p.x.is_finite() && p.y.is_finite()) {
            return Ok(Value::Null);
        }
        // A press (not a hover) settles an input method's composition, as clicking does in PowerPoint.
        if !matches!(ev.kind, PointerKind::Move) {
            self.settle_composition();
        }
        match ev.kind {
            PointerKind::Move => {
                if let Some(Drag::Freeform { points, mode, .. }) = &self.tool.drag {
                    let closing = points.len() > 2 && points.first().is_some_and(|s| (*s - p).hypot() <= tol * 2.0);
                    let mut v = points.clone();
                    v.push(p);
                    self.tool.path_preview = v;
                    self.tool.path_smooth = mode == "curve";
                    self.tool.path_closing = closing;
                    return Ok(Value::Null);
                }
                self.tool.hover = self.hit_test(p, tol);
                self.tool.guide_hover =
                    if self.tool.hover.is_none() && matches!(self.tool.kind, ToolKind::Select) { self.guide_at(p, tol) } else { None };
                let line_tool = matches!(&self.tool.kind, ToolKind::Shape { preset } if deckcraft_geom::preset::is_line_like(preset));
                if line_tool {
                    self.update_sites(p, tol, None);
                } else {
                    self.tool.sites.clear();
                    self.tool.glue = None;
                }
                Ok(Value::Null)
            }
            PointerKind::Down => self.down(p, ev.mods, tol),
            PointerKind::Drag => self.drag(p, ev.mods, tol),
            PointerKind::Up => self.up(p, ev.mods),
            PointerKind::DoubleClick => {
                let before = self.text_selection();
                let r = self.double_click(p, tol)?;
                self.paint_if_selection_changed(before, r)
            }
            PointerKind::ContextClick => self.context_click(p, tol),
            PointerKind::TripleClick => {
                if self.active().is_some_and(|d| d.selection.text.is_some()) {
                    let before = self.text_selection();
                    let r = self.execute("text.selectParagraph", &json!({}))?;
                    self.paint_if_selection_changed(before, r)
                } else {
                    Ok(Value::Null)
                }
            }
        }
    }

    /// Format Painter applies once a text range is selected (drag, word or paragraph click).
    fn paint_text_selection(&mut self) -> Result<Option<Value>> {
        if self.painter.is_some() && self.doc()?.selection.text.as_ref().is_some_and(|t| t.is_range()) {
            return self.execute("format.painterApply", &json!({})).map(Some);
        }
        Ok(None)
    }

    /// Where the text selection is: its shape, cell or notes, anchor and caret.
    fn text_selection(&self) -> Option<TextTarget> {
        self.active().and_then(|d| d.selection.text.as_ref()).map(|t| (t.shape, t.cell, t.notes, t.anchor, t.caret))
    }

    /// A double or triple click paints only the range it selected, not an older one it left alone.
    fn paint_if_selection_changed(&mut self, before: Option<TextTarget>, r: Value) -> Result<Value> {
        if self.text_selection() == before {
            return Ok(r);
        }
        self.paint_text_selection().map(|a| a.unwrap_or(r))
    }

    pub(crate) fn begin(&mut self, label: &str) {
        let vertex_kind = self.tool.vertex_kind.clone();
        if let Some(st) = self.active_mut()
            && st.interaction.is_none()
        {
            st.interaction = Some(crate::Interaction { label: label.into(), doc: st.doc.clone(), selection: st.selection.clone(), vertex_kind });
        }
    }

    fn commit(&mut self) {
        if let Some(st) = self.active_mut()
            && let Some(i) = st.interaction.take()
            && !Arc::ptr_eq(&i.doc, &st.doc)
        {
            crate::push_undo(st, HistoryEntry { label: i.label, doc: i.doc, selection: i.selection, vertex_kind: i.vertex_kind });
        }
    }

    /// Cancels a point or handle drag in Edit Points when Esc is pressed: ends the drag, puts back the
    /// document and selection saved at the press, and adds no undo step. Returns whether a drag was cancelled.
    pub(crate) fn cancel_point_drag(&mut self) -> bool {
        if !matches!(self.tool.drag, Some(Drag::Vertex { .. } | Drag::Handle { .. })) {
            return false;
        }
        self.tool.drag = None;
        if let Some(st) = self.active_mut()
            && st.interaction.as_ref().is_some_and(|i| i.label == "Edit Points")
            && let Some(i) = st.interaction.take()
        {
            // A modifier held during the drag may have set a point type; the drag is undone, so is that.
            let kind = i.vertex_kind;
            if !Arc::ptr_eq(&i.doc, &st.doc) {
                st.doc = i.doc;
                st.selection = i.selection;
                st.revision += 1;
            }
            self.tool.vertex_kind = kind;
        }
        true
    }

    /// End the interaction only when it is the one `label` started. A twist must not commit a drag.
    pub(crate) fn commit_named(&mut self, label: &str) {
        let ours = self.active().and_then(|st| st.interaction.as_ref()).is_some_and(|i| i.label == label);
        if ours {
            self.commit();
        }
    }

    fn down(&mut self, p: Point, mods: Mods, tol: f64) -> Result<Value> {
        if let Some(Drag::Freeform { mode, mut points, .. }) = self.tool.drag.clone() {
            // Next click of a freeform: close on the start point, else add a vertex.
            if points.len() > 2 && points.first().is_some_and(|s| (*s - p).hypot() <= tol * 2.0) {
                self.tool.drag = Some(Drag::Freeform { mode, points, pressed: false });
                return self.finish_freeform(true);
            }
            points.push(p);
            self.tool.path_preview = points.clone();
            self.tool.drag = Some(Drag::Freeform { mode, points, pressed: true });
            return Ok(Value::Null);
        }
        match self.tool.kind.clone() {
            ToolKind::Shape { preset } if is_freeform_tool(&preset) => {
                self.tool.drag = Some(Drag::Freeform { mode: preset.clone(), points: vec![p], pressed: true });
                self.tool.path_preview = vec![p];
                self.tool.path_smooth = preset == "curve";
                self.tool.path_closing = false;
                return Ok(Value::Null);
            }
            ToolKind::Shape { preset } if preset == TABLE_ERASER => {
                // Click a border to merge the cells on either side; elsewhere does nothing.
                return Ok(self.execute("table.eraseLine", &json!({"at": [p.x, p.y], "tol": tol.max(4.0)})).unwrap_or(Value::Null));
            }
            ToolKind::Shape { .. } | ToolKind::TextBox => {
                self.tool.drag = Some(Drag::Create { start: p });
                self.tool.preview = Some(Xfrm::new(p.x, p.y, 0.0, 0.0));
                return Ok(Value::Null);
            }
            ToolKind::Ink { mode, .. } => {
                if mode == "eraser" {
                    return self.erase_at(p, tol);
                }
                let sample = self.ink_sample(p);
                self.tool.drag = Some(Drag::Ink { points: vec![sample] });
                self.tool.ink_preview = vec![sample];
                return Ok(Value::Null);
            }
            ToolKind::Select => {}
        }
        // Clicking inside the text being edited moves the caret / starts a text selection.
        let before = self.text_selection();
        if let Some((shape, pos)) = self.place_caret(p, tol, mods.shift)? {
            let press = self.doc()?.selection.text.clone();
            self.tool.drag = Some(Drag::TextSelect { shape, before, press });
            return Ok(json!({"caret": [pos.0, pos.1]}));
        }
        let hit = self.hit_test(p, tol);
        // Edit Points: drag a vertex, Ctrl-click one to delete it or the outline to add one;
        // a click anywhere else leaves Edit Points.
        if let Some(eid) = self.doc().ok().and_then(|st| self.tool.editing_points(st)) {
            if let Some(Hit::Vertex { id, path, index }) = hit {
                if mods.cmd {
                    return self.execute("shape.deletePoint", &json!({"id": id, "path": path, "index": index}));
                }
                self.tool.edit_vertex = Some((path, index));
                self.begin("Edit Points");
                self.tool.drag = Some(Drag::Vertex { id, path, index });
                return Ok(json!({"vertex": [path, index]}));
            }
            if let Some(Hit::Handle { id, path, index, out }) = hit {
                self.begin("Edit Points");
                self.tool.drag = Some(Drag::Handle { id, path, index, out });
                return Ok(json!({"handle": [path, index]}));
            }
            let st = self.doc()?;
            if let Some(sh) = st.shape(eid)
                && let Some((path, seg, t)) = cmd::points::segment_at(&st.doc, &st.selection, sh, p, tol * 1.6)
            {
                if mods.cmd {
                    return self.execute("shape.addPoint", &json!({"id": eid, "path": path, "segment": seg, "t": t}));
                }
                return Ok(Value::Null);
            }
            self.tool.edit_points = None;
            self.tool.edit_vertex = None;
        }
        let st = self.doc()?;
        match hit {
            Some(Hit::TableColumn { id, col }) => self.begin_table_border(id, true, col, p)?,
            Some(Hit::TableRow { id, row }) => self.begin_table_border(id, false, row, p)?,
            Some(Hit::Resize { id, handle }) => {
                let orig = st.shape(id).map(|s| xfrm_of(&st.doc, &st.selection, s)).unwrap_or_default();
                self.begin("Size");
                self.tool.drag = Some(Drag::Resize { id, handle, orig, start: p });
            }
            Some(Hit::LineEnd { id, end }) => {
                let orig = st.shape(id).map(|s| xfrm_of(&st.doc, &st.selection, s)).unwrap_or_default();
                self.begin("Size");
                self.tool.drag = Some(Drag::LineEnd { id, end, orig });
            }
            Some(Hit::Rotate { id }) => {
                let ids: Vec<ShapeId> = if st.selection.shapes.contains(&id) { st.selection.shapes.clone() } else { vec![id] };
                let origs: Vec<(ShapeId, Xfrm)> = ids.iter().filter_map(|i| st.shape(*i).map(|s| (*i, xfrm_of(&st.doc, &st.selection, s)))).collect();
                let center = st.shape(id).map(|s| xfrm_of(&st.doc, &st.selection, s).center()).unwrap_or(p);
                let start = (p.y - center.y).atan2(p.x - center.x);
                self.begin("Rotate");
                self.tool.drag = Some(Drag::Rotate { ids, center, start, origs });
            }
            // Only reached outside Edit Points (handled above).
            Some(Hit::Vertex { .. } | Hit::Handle { .. }) => {}
            Some(Hit::Adjust { id, index }) => {
                let orig = st.shape(id).map(|s| xfrm_of(&st.doc, &st.selection, s)).unwrap_or_default();
                self.begin("Adjust Shape");
                self.tool.drag = Some(Drag::Adjust { id, index, orig });
            }
            Some(Hit::Frame { id }) => {
                // Like PowerPoint: the box leaves text editing and is selected whole (a click
                // without a drag stops there), and dragging moves it as one undo step.
                let origs: Vec<(ShapeId, Xfrm)> = st.shape(id).map(|s| (id, xfrm_of(&st.doc, &st.selection, s))).into_iter().collect();
                self.select(|_, sel| {
                    sel.shapes = vec![id];
                    sel.text = None;
                    sel.cells = None;
                })?;
                self.tool.drag = Some(Drag::Move { ids: vec![id], start: p, origs, moved: false, duplicate: mods.alt, enter_text: None });
            }
            Some(Hit::Warp { id, index }) => {
                let orig = st.shape(id).map(|s| xfrm_of(&st.doc, &st.selection, s)).unwrap_or_default();
                let found = st.shape(id).and_then(|sh| {
                    let ctx = crate::cmd::text::ctx_for(&st.doc, &st.selection)?;
                    let bp = deckcraft_model::resolve::body(&ctx, sh);
                    bp.warp.map(|n| (n, bp.warp_adj))
                });
                if let Some((name, base)) = found {
                    self.begin("Adjust WordArt");
                    self.tool.drag = Some(Drag::Warp { id, index, name, base, orig });
                }
            }
            Some(Hit::Shape { id, .. })
                if !(mods.shift || mods.cmd)
                    && st.selection.cells.is_some_and(|c| c.0 == id && st.selection.shapes == [id])
                    && in_table_cell(st, id, p) =>
            {
                // A press in a table whose cells are selected starts again in the cell under the
                // pointer: a click puts the caret there and a drag selects another range. A press
                // on the table outside its cells selects and moves the table like any shape.
                let before = self.text_selection();
                if self.edit_text_at(id, p)? {
                    self.select(|_, sel| sel.cells = None)?;
                    let press = self.doc()?.selection.text.clone();
                    self.tool.drag = Some(Drag::TextSelect { shape: id, before, press });
                }
            }
            Some(Hit::Shape { id, text }) => {
                let selected = st.selection.shapes.contains(&id);
                let toggle = mods.shift || mods.cmd;
                let ids: Vec<ShapeId> = if toggle {
                    let mut v = st.selection.shapes.clone();
                    if selected {
                        v.retain(|x| *x != id);
                    } else {
                        v.push(id);
                    }
                    v
                } else if selected {
                    st.selection.shapes.clone()
                } else {
                    vec![id]
                };
                // Locked objects stay put when the selection is dragged.
                let origs: Vec<(ShapeId, Xfrm)> =
                    ids.iter().filter_map(|i| st.shape(*i).filter(|s| !s.locked).map(|s| (*i, xfrm_of(&st.doc, &st.selection, s)))).collect();
                // A click (no drag) on text puts the caret there; unselected text boxes and
                // placeholders take the caret straight away like PowerPoint. A click in a cell of a
                // selected table puts the caret there; a click on its grab band keeps the table selected.
                let table = st.shape(id).is_some_and(|s| matches!(s.kind, ShapeKind::Table(_)));
                // A click on the frame band of a text box selects the box whole, as PowerPoint
                // does, so it can be moved, duplicated or deleted; only the inside takes the caret.
                let on_frame = text
                    && is_text_first(st, id)
                    && st.shape(id).is_some_and(|s| edit_zone(&xfrm_of(&st.doc, &st.selection, s), p, tol) == EditZone::Frame);
                let enter_text =
                    (!toggle && !on_frame && ((text && (selected || is_text_first(st, id))) || (table && selected && in_table_cell(st, id, p))))
                        .then_some((id, p));
                let ids2 = ids.clone();
                self.select(|_, sel| {
                    sel.shapes = ids2;
                    sel.text = None;
                    sel.cells = None;
                })?;
                let duplicate = mods.alt && origs.len() == ids.len();
                self.tool.drag = Some(Drag::Move { ids, start: p, origs, moved: false, duplicate, enter_text });
                // Format Painter applies on click.
                if self.painter.is_some() && !toggle {
                    let _ = self.execute("format.painterApply", &json!({"ids": [id]}));
                }
                // Animation Painter applies on click.
                if self.anim_painter.is_some() && !toggle {
                    let _ = self.execute("animation.painterApply", &json!({"ids": [id]}));
                }
            }
            None if self.guide_at(p, tol).is_some() => {
                // Away from every shape a press on a guide picks the guide up.
                if let Some(index) = self.guide_at(p, tol) {
                    let pos = st.doc.guides.get(index).map(|g| g.pos).unwrap_or(0.0);
                    self.tool.drag = Some(Drag::Guide { index });
                    self.tool.guide_drag = Some((index, pos));
                }
            }
            None => {
                let add = mods.shift || mods.cmd;
                self.select(|_, sel| {
                    sel.text = None;
                    sel.cells = None;
                    if !add {
                        sel.shapes.clear();
                    }
                })?;
                self.tool.drag = Some(Drag::Marquee { start: p, add });
                self.tool.marquee = Some(Rect::from_points(p, p));
            }
        }
        Ok(json!({"hit": serde_json::to_value(&hit).unwrap_or_default()}))
    }

    fn up(&mut self, p: Point, mods: Mods) -> Result<Value> {
        let drag = self.tool.drag.take();
        self.tool.guides.clear();
        self.tool.sites.clear();
        self.tool.glue = None;
        let mut out = Value::Null;
        match drag {
            Some(Drag::Guide { index }) => {
                // Dropped off the slide: the guide is deleted.
                let (_, pos) = self.tool.guide_drag.take().unwrap_or((index, 0.0));
                let size = self.doc()?.doc.slide_size;
                let vertical = self.doc()?.doc.guides.get(index).is_some_and(|g| g.vertical);
                let limit = if vertical { size.width } else { size.height };
                out = if pos < 0.0 || pos > limit {
                    self.execute("guide.delete", &json!({"index": index}))?
                } else {
                    self.execute("guide.move", &json!({"index": index, "pos": pos}))?
                };
            }
            Some(Drag::Move { moved: false, enter_text: Some((id, at)), .. }) => {
                // Plain click on text: caret at the click (in a table, in the clicked cell).
                if self.edit_text_at(id, at)? {
                    out = json!({"editing": id});
                }
            }
            Some(Drag::Marquee { start, add }) => {
                let r = Rect::from_points(start, p);
                self.tool.marquee = None;
                if r.width() > 1.0 || r.height() > 1.0 {
                    let st = self.doc()?;
                    let ids: Vec<ShapeId> = st
                        .shapes()
                        .iter()
                        .filter(|s| !s.hidden && !s.locked)
                        .filter(|s| contains_rect(r, xfrm_of(&st.doc, &st.selection, s).bounds()))
                        .map(|s| s.id)
                        .collect();
                    self.select(|_, sel| {
                        if add {
                            for i in &ids {
                                if !sel.shapes.contains(i) {
                                    sel.shapes.push(*i);
                                }
                            }
                        } else {
                            sel.shapes = ids.clone();
                        }
                    })?;
                    out = json!({"selected": ids});
                }
            }
            Some(Drag::Create { start }) => {
                let kind = self.tool.kind.clone();
                let preview = self.tool.preview.take();
                let dragged = (p - start).hypot() > 2.0;
                let res = match &kind {
                    ToolKind::Shape { preset } if preset == DRAW_TABLE => {
                        // Inside a table: split cells along the line. Elsewhere: a one-cell table.
                        if crate::cmd::table::table_at(self, &Value::Null, (start.x, start.y)).is_some() {
                            self.execute("table.drawLine", &json!({"from": [start.x, start.y], "to": [p.x, p.y]})).unwrap_or(Value::Null)
                        } else if let Some(x) = preview.filter(|x| dragged && x.w >= 8.0 && x.h >= 8.0) {
                            self.execute("insert.table", &json!({"rows": 1, "cols": 1, "rect": [x.x, x.y, x.w, x.h]}))?
                        } else {
                            Value::Null
                        }
                    }
                    ToolKind::Shape { preset } => {
                        let line = deckcraft_geom::preset::is_line_like(preset);
                        let rect = match preview.filter(|_| dragged) {
                            Some(x) => x,
                            None if line => Xfrm::new(start.x, start.y, 72.0, 0.0),
                            None => Xfrm::new(start.x, start.y, 72.0, 72.0),
                        };
                        let r = self.execute("shape.insert", &json!({"preset": preset, "rect": [rect.x, rect.y, rect.w, rect.h], "flip": format!("{}{}", if rect.flip_h { "h" } else { "" }, if rect.flip_v { "v" } else { "" })}))?;
                        if let (Some(id), Some(x)) = (r.get("id").and_then(Value::as_u64), preview.filter(|_| dragged && line)) {
                            let id = ShapeId(id as u32);
                            let _ = self.set_xfrm_cmd(id, x);
                            // Glue the ends that landed on connection sites.
                            let (p0, p1) = crate::connect::endpoints(&x);
                            let (g0, g1) = (self.glue_at(p0, 4.0), self.glue_at(p1, 4.0));
                            if g0.is_some() || g1.is_some() {
                                let _ = self.edit(|doc, sel| {
                                    if let Some(list) = crate::shapes_mut(doc, sel) {
                                        crate::connect::set_glue(list, id, 0, g0.map(|g| (g.0, g.1)));
                                        crate::connect::set_glue(list, id, 1, g1.map(|g| (g.0, g.1)));
                                    }
                                    Ok(())
                                });
                            }
                        }
                        r
                    }
                    ToolKind::TextBox => {
                        let w = preview.filter(|_| dragged).map(|x| x.w).unwrap_or(0.0);
                        let at = preview.filter(|_| dragged).map(|x| Point::new(x.x, x.y)).unwrap_or(start);
                        self.execute("insert.textBox", &json!({"rect": [at.x, at.y, w, 30.0]}))?
                    }
                    _ => Value::Null,
                };
                // The table pencil stays on until you pick another tool, as in PowerPoint.
                if !self.tool.sticky && !matches!(&kind, ToolKind::Shape { preset } if preset == DRAW_TABLE) {
                    self.tool.kind = ToolKind::Select;
                }
                out = res;
            }
            Some(Drag::Freeform { mode, points, .. }) => {
                if mode == "scribble" {
                    // A scribble ending back at its start closes into a filled shape.
                    let closed = points.len() > 3 && points.first().zip(points.last()).is_some_and(|(a, b)| (*a - *b).hypot() <= 6.0);
                    self.tool.drag = Some(Drag::Freeform { mode, points, pressed: false });
                    return self.finish_freeform(closed);
                }
                // Polygon / curve: keep going until double-click, Enter or a click on the start.
                self.tool.drag = Some(Drag::Freeform { mode, points, pressed: false });
                return Ok(Value::Null);
            }
            Some(Drag::Ink { points }) => {
                self.tool.ink_preview.clear();
                if points.len() > 1 {
                    self.begin("Ink");
                    out = self.add_ink(points)?;
                }
            }
            Some(Drag::Resize { id, .. }) => {
                self.commit();
                let _ = cmd::text::fit_text_box(self, id);
                self.refit_after_resize(id)?;
            }
            Some(Drag::TextSelect { before, .. }) => {
                out = self.paint_if_selection_changed(before, out)?;
                self.commit();
            }
            Some(_) => self.commit(),
            None => {}
        }
        // Any open interaction (move etc.) ends here.
        self.commit();
        let _ = mods;
        Ok(out)
    }

    pub(super) fn is_picture(&self, id: ShapeId) -> bool {
        self.active().and_then(|d| d.shape(id)).is_some_and(|s| matches!(s.kind, ShapeKind::Picture { .. } | ShapeKind::Media(_)))
    }
}
