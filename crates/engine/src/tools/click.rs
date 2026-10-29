use super::hit::{EditZone, deckcraft_render_cell, edit_zone, edited_box, equation_dialog, math_run_at, shape_in_front, text_member_at, text_pos};
use super::{Drag, Hit, Point, Result, Session, ShapeId, ShapeKind, TextSel, ToolKind, Value, cmd, json, table_rows, xfrm_of};
use deckcraft_model::text::RunKind;

impl Session {
    pub(super) fn double_click(&mut self, p: Point, tol: f64) -> Result<Value> {
        if let Some(Drag::Freeform { mode, mut points, .. }) = self.tool.drag.clone() {
            // The double-click's own click added a duplicate vertex.
            while points.len() > 2 && points.last().zip(points.get(points.len() - 2)).is_some_and(|(a, b)| (*a - *b).hypot() <= tol) {
                points.pop();
            }
            self.tool.drag = Some(Drag::Freeform { mode, points, pressed: false });
            return self.finish_freeform(false);
        }
        if !matches!(self.tool.kind, ToolKind::Select) {
            // Double-click while drawing: lock drawing mode off.
            self.tool.sticky = false;
            return Ok(Value::Null);
        }
        // The double-click's second press started a click (caret on release): this replaces it.
        if matches!(self.tool.drag, Some(Drag::Move { moved: false, .. })) {
            self.tool.drag = None;
        }
        {
            let st = self.doc()?;
            if let Some(t) = st.selection.text.clone()
                && !t.notes
                && t.cell.is_none()
                && let Some(sh) = st.shape(t.shape).cloned()
                && let Some((pi, ri)) = math_run_at(st, &sh, p)
            {
                self.ui_requests.push(equation_dialog(sh.id, None, pi, ri));
                return Ok(Value::Null);
            }
        }
        // In the text being edited: select the word double-clicked.
        if self.place_caret(p, tol, false)?.is_some() {
            return self.execute("text.selectWord", &json!({}));
        }
        match self.hit_test(p, tol) {
            Some(Hit::Shape { id, .. }) => {
                let st = self.doc()?;
                let Some(sh) = st.shape(id).cloned() else { return Ok(Value::Null) };
                match &sh.kind {
                    ShapeKind::Table(_) | ShapeKind::Shape | ShapeKind::Connector { .. } if !sh.is_line() => {
                        // A cell holding nothing but one equation, or an equation under the
                        // pointer, opens the equation editor.
                        if let ShapeKind::Table(t) = &sh.kind {
                            let x = xfrm_of(&st.doc, &st.selection, &sh);
                            let local = x.affine().inverse() * p;
                            let cell = deckcraft_render_cell(t, &table_rows(st, t), local);
                            if let Some(body) = t.cell(cell.0, cell.1).map(|c| &c.text)
                                && let [para] = body.paragraphs.as_slice()
                                && let [run] = para.runs.as_slice()
                                && matches!(run.kind, RunKind::Math { .. })
                            {
                                self.ui_requests.push(equation_dialog(id, Some(cell), 0, 0));
                                return Ok(Value::Null);
                            }
                        } else if let Some((pi, ri)) = math_run_at(st, &sh, p) {
                            self.ui_requests.push(equation_dialog(id, None, pi, ri));
                            return Ok(Value::Null);
                        }
                        // Caret at the double-click (in a table, in the clicked cell), then select
                        // the word there.
                        if !self.edit_text_at(id, p)? {
                            return Ok(Value::Null);
                        }
                        self.execute("text.selectWord", &json!({}))
                    }
                    ShapeKind::Group { .. } => {
                        // A group is a single selection unit, but its text stays editable:
                        // double-click reaches the text-holding member under the cursor.
                        match text_member_at(st, &sh, p, tol).and_then(|mid| st.shape(mid).cloned().map(|m| (mid, m))) {
                            Some((mid, msh)) if !msh.is_line() => {
                                let pos = text_pos(st, &msh, p);
                                self.execute("text.edit", &json!({"id": mid, "at": [pos.0, pos.1]}))?;
                                self.execute("text.selectWord", &json!({}))
                            }
                            _ => Ok(Value::Null),
                        }
                    }
                    _ => {
                        self.ui_requests.push(crate::UiRequest::Pane { id: "format".into() });
                        Ok(Value::Null)
                    }
                }
            }
            _ => Ok(Value::Null),
        }
    }

    /// Right-button press: the selection the context menu will act on, as in PowerPoint. In the
    /// box being edited, the caret and text selection stay (the menu then acts on the text). On a
    /// selected shape (or its handles) the selection stays, multiple selection included; on
    /// another shape, that shape alone is selected; on empty slide, nothing is. Never edits text,
    /// starts a drag or adds an undo step.
    pub(super) fn context_click(&mut self, p: Point, tol: f64) -> Result<Value> {
        if !matches!(self.tool.kind, ToolKind::Select) || self.tool.drag.is_some() {
            return Ok(Value::Null);
        }
        let st = self.doc()?;
        // The same zone in which a left press places the caret (`down`).
        if let Some(t) = &st.selection.text
            && !t.notes
            && t.cell.is_none()
            && let Some(sh) = st.shape(t.shape)
            && xfrm_of(&st.doc, &st.selection, sh).bounds().inflate(tol, tol).contains(p)
        {
            return Ok(json!({"hit": {"kind": "text", "id": t.shape}, "selected": [t.shape], "editing": true}));
        }
        let hit = self.hit_test(p, tol);
        let id = match &hit {
            Some(
                Hit::Shape { id, .. }
                | Hit::Resize { id, .. }
                | Hit::Rotate { id }
                | Hit::Adjust { id, .. }
                | Hit::LineEnd { id, .. }
                | Hit::Handle { id, .. }
                | Hit::Vertex { id, .. }
                | Hit::Frame { id }
                | Hit::TableColumn { id, .. }
                | Hit::TableRow { id, .. }
                | Hit::Warp { id, .. },
            ) => Some(*id),
            None => None,
        };
        // Edit Points: the point or else the segment under the pointer picks the menu.
        let picked = self.tool.editing_points(st).and_then(|eid| st.shape(eid)).map(|sh| match &hit {
            Some(Hit::Vertex { path, index, .. }) => (Some((*path, *index)), None),
            Some(Hit::Handle { .. }) => (None, None),
            _ => (None, cmd::points::segment_at(&st.doc, &st.selection, sh, p, tol * 1.6)),
        });
        if !id.is_some_and(|id| st.selection.shapes.contains(&id)) {
            self.select(|_, sel| {
                sel.text = None;
                sel.cells = None;
                sel.shapes = id.into_iter().collect();
            })?;
        }
        if let Some((vertex, segment)) = picked {
            // A control handle belongs to the picked point: its menu stays.
            if !matches!(hit, Some(Hit::Handle { .. })) {
                self.tool.edit_vertex = vertex;
            }
            self.tool.edit_segment = segment.map(|(path, seg, _)| (path, seg));
            self.tool.edit_segment_t = segment.map(|(_, _, t)| t);
        }
        let st = self.doc()?;
        Ok(json!({
            "hit": serde_json::to_value(&hit).unwrap_or_default(),
            "selected": st.selection.shapes,
            "editing": st.selection.text.is_some(),
        }))
    }

    /// A click at `p` inside the text being edited: the caret goes there (`extend`: the selection
    /// grows to it). In a table being edited, a click in another cell moves the caret into that
    /// cell. Returns the edited shape and the caret, or `None` when `p` isn't in the edited text
    /// (or is on a handle or a table border).
    pub(super) fn place_caret(&mut self, p: Point, tol: f64, extend: bool) -> Result<Option<(ShapeId, (usize, usize))>> {
        let st = self.doc()?;
        let Some(mut t) = st.selection.text.clone().filter(|t| !t.notes) else { return Ok(None) };
        let Some(sh) = st.shape(t.shape) else { return Ok(None) };
        // A text box: in its own (rotated) frame, short of the band that moves it (`Hit::Frame`).
        // A table cell: anywhere over the table.
        let inside = match t.cell {
            None => edited_box(st).is_some_and(|e| edit_zone(&e.xfrm, p, tol) != EditZone::Outside),
            Some(_) => xfrm_of(&st.doc, &st.selection, sh).bounds().inflate(tol, tol).contains(p),
        };
        // A shape in front of the edited text takes the click, as in PowerPoint (#931).
        let in_front = shape_in_front(st, st.shapes(), t.shape, p, tol, 0);
        if !inside
            || in_front
            || matches!(
                self.hit_test(p, tol),
                Some(
                    Hit::Resize { .. }
                        | Hit::Rotate { .. }
                        | Hit::Adjust { .. }
                        | Hit::Warp { .. }
                        | Hit::Frame { .. }
                        | Hit::TableColumn { .. }
                        | Hit::TableRow { .. }
                )
            )
        {
            return Ok(None);
        }
        let old_cell = t.cell;
        if old_cell.is_some()
            && let Some(f) = cmd::table::table_frame(st, sh)
        {
            t.cell = f.cell_at(f.local(p)).or(old_cell);
        }
        let pos = cmd::text::text_pos_at(st, &t, p);
        // A selection can't span cells: a click in another cell starts afresh.
        let extend = extend && t.cell == old_cell;
        let cell = t.cell;
        self.select(|_, sel| {
            if let Some(tt) = sel.text.as_mut() {
                tt.cell = cell;
                tt.caret = pos;
                if !extend {
                    tt.anchor = pos;
                }
            }
        })?;
        Ok(Some((t.shape, pos)))
    }

    /// Start editing shape `id`'s text with the caret at slide point `p`; for a table, in the cell
    /// at `p`. Returns whether editing started.
    pub(super) fn edit_text_at(&mut self, id: ShapeId, p: Point) -> Result<bool> {
        let st = self.doc()?;
        let Some(sh) = st.shape(id) else { return Ok(false) };
        let params = match cmd::table::table_frame(st, sh) {
            Some(f) => {
                let Some((r, c)) = f.cell_at(f.local(p)) else { return Ok(false) };
                let pos = cmd::text::text_pos_at(st, &TextSel { shape: id, cell: Some((r, c)), ..Default::default() }, p);
                json!({"id": id, "cell": [r, c], "at": [pos.0, pos.1]})
            }
            None => {
                let pos = text_pos(st, sh, p);
                json!({"id": id, "at": [pos.0, pos.1]})
            }
        };
        self.execute("text.edit", &params)?;
        Ok(true)
    }

    /// Press on a table border: dragging it resizes the column or row. Editing a cell of the
    /// table goes on; otherwise the table becomes the selection.
    pub(super) fn begin_table_border(&mut self, id: ShapeId, column: bool, index: usize, p: Point) -> Result<()> {
        let st = self.doc()?;
        let Some(f) = st.shape(id).and_then(|sh| cmd::table::table_frame(st, sh)) else { return Ok(()) };
        let size = if column { f.cols.get(index) } else { f.heights.get(index) }.copied().unwrap_or(0.0);
        let (start, frame) = (f.local(p), f.affine);
        if !st.selection.text.as_ref().is_some_and(|t| t.shape == id) {
            self.select(|_, sel| {
                sel.shapes = vec![id];
                sel.text = None;
                sel.cells = None;
            })?;
        }
        self.tool.drag = Some(Drag::TableBorder { id, column, index, start, size, frame, moved: false });
        Ok(())
    }
}
