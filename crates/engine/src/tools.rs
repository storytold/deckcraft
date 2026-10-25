//! Pointer tools. Gestures arrive in slide coordinates (points) through [`Session::pointer`] from
//! the UI, the control channel or MCP alike; each finished gesture is one undo step.

use std::sync::Arc;

use deckcraft_geom::{Affine, Point, Rect, Vec2, Xfrm};
use deckcraft_model::{Shape, ShapeId, ShapeKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cmd::{self, xfrm_of};
use crate::{HistoryEntry, Result, Session, TextSel};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PointerKind {
    #[default]
    Down,
    Drag,
    Up,
    Move,
    DoubleClick,
    TripleClick,
    /// Secondary-button press (right click), before the context menu opens: selects what's under
    /// the pointer the way PowerPoint does, without editing text or starting a drag.
    ContextClick,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub cmd: bool,
}

/// A text selection's shape, table cell, notes flag, anchor and caret.
type TextTarget = (ShapeId, Option<(usize, usize)>, bool, (usize, usize), (usize, usize));

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PointerEvent {
    pub kind: PointerKind,
    pub x: f64,
    pub y: f64,
    pub mods: Mods,
    /// Hit tolerance in points (≈ 4 screen px at the current zoom).
    pub tol: f64,
    /// Pen pressure `0..=1` when the device reported one. `None` is a mouse: the stroke
    /// is stored at full pen width.
    #[serde(default)]
    pub pressure: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ToolKind {
    #[default]
    Select,
    /// Draw a preset shape.
    Shape {
        preset: String,
    },
    TextBox,
    /// Ink: pen (`pen`, `highlighter`) or `eraser`.
    Ink {
        mode: String,
        color: deckcraft_color::Rgba,
        width: f64,
    },
}

/// What a point hits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Hit {
    /// Resize handle 0..8: NW, N, NE, E, SE, S, SW, W.
    Resize {
        id: ShapeId,
        handle: u8,
    },
    Rotate {
        id: ShapeId,
    },
    Adjust {
        id: ShapeId,
        index: usize,
    },
    /// A control handle (`out`: the outgoing one) of the selected vertex in Edit Points.
    Handle {
        id: ShapeId,
        path: usize,
        index: usize,
        out: bool,
    },
    /// A vertex of a shape whose points are being edited.
    Vertex {
        id: ShapeId,
        path: usize,
        index: usize,
    },
    /// WordArt warp adjust (yellow handle). `index` is `adj` / `adjN`.
    Warp {
        id: ShapeId,
        index: usize,
    },
    /// Line end 0/1 of a line-like shape.
    LineEnd {
        id: ShapeId,
        end: u8,
    },
    Shape {
        id: ShapeId,
        text: bool,
    },
    /// The frame of the box whose text is being edited (a band on either side of its edge):
    /// pressing there leaves text editing and drags the box, like PowerPoint.
    Frame {
        id: ShapeId,
    },
    /// The right edge of table column `col` (dragging it resizes the column).
    TableColumn {
        id: ShapeId,
        col: usize,
    },
    /// The bottom edge of table row `row` (dragging it resizes the row).
    TableRow {
        id: ShapeId,
        row: usize,
    },
}

#[derive(Clone, Debug)]
enum Drag {
    Move {
        ids: Vec<ShapeId>,
        start: Point,
        origs: Vec<(ShapeId, Xfrm)>,
        moved: bool,
        duplicate: bool,
        enter_text: Option<(ShapeId, Point)>,
    },
    Resize {
        id: ShapeId,
        handle: u8,
        orig: Xfrm,
        start: Point,
    },
    LineEnd {
        id: ShapeId,
        end: u8,
        orig: Xfrm,
    },
    Rotate {
        ids: Vec<ShapeId>,
        center: Point,
        start: f64,
        origs: Vec<(ShapeId, Xfrm)>,
    },
    Adjust {
        id: ShapeId,
        index: usize,
        orig: Xfrm,
    },
    Vertex {
        id: ShapeId,
        path: usize,
        index: usize,
    },
    Handle {
        id: ShapeId,
        path: usize,
        index: usize,
        out: bool,
    },
    /// WordArt: `base` is the adjust list at mouse-down (empty means the preset defaults).
    Warp {
        id: ShapeId,
        index: usize,
        name: String,
        base: Vec<f64>,
        orig: Xfrm,
    },
    Marquee {
        start: Point,
        add: bool,
    },
    /// Dragging drawing guide `index`.
    Guide {
        index: usize,
    },
    Create {
        start: Point,
    },
    TextSelect {
        shape: ShapeId,
        /// The text selection before the press, to tell whether the drag changed it.
        before: Option<TextTarget>,
        /// The text selection the press made. In a table, a drag that leaves its cell selects
        /// whole cells, and coming back to the cell restores this.
        press: Option<TextSel>,
    },
    /// Dragging a table border: the right edge of column `index`, or the bottom edge of row
    /// `index`. `start` is the table-local press point, `size` the column's width or the row's
    /// drawn height then, `frame` where the table was drawn.
    TableBorder {
        id: ShapeId,
        column: bool,
        index: usize,
        start: Point,
        size: f64,
        frame: Affine,
        moved: bool,
    },
    Ink {
        points: Vec<(f64, f64, f32)>,
    },
    /// Freeform drawing (`freeform` polygon, `curve`, `scribble`): placed points; `pressed` while
    /// the button is down (dragging adds scribbled points).
    Freeform {
        mode: String,
        points: Vec<Point>,
        pressed: bool,
    },
}

/// Shape-gallery names that draw a freeform path instead of a preset.
pub const FREEFORM_TOOLS: [(&str, &str); 3] = [("curve", "Curve"), ("freeform", "Freeform: Shape"), ("scribble", "Freeform: Scribble")];

/// Draw Table's pencil and eraser, as `ToolKind::Shape` presets (Insert › Table › Draw Table).
pub const DRAW_TABLE: &str = "drawTable";
pub const TABLE_ERASER: &str = "tableEraser";

pub fn is_freeform_tool(name: &str) -> bool {
    FREEFORM_TOOLS.iter().any(|f| f.0 == name)
}

/// A snapping guide line to draw (slide coordinates): vertical (x) or horizontal (y).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    pub vertical: bool,
    pub pos: f64,
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Debug, Default)]
pub struct ToolState {
    pub kind: ToolKind,
    /// Keep the tool after one use (double-click on a shape in the gallery: lock drawing mode).
    pub sticky: bool,
    drag: Option<Drag>,
    pub marquee: Option<Rect>,
    pub preview: Option<Xfrm>,
    pub guides: Vec<Guide>,
    pub hover: Option<Hit>,
    pub ink_preview: Vec<(f64, f64, f32)>,
    /// Connection sites to show (slide coordinates) while drawing or dragging a line end, and the
    /// site the end would glue to.
    pub sites: Vec<Point>,
    pub glue: Option<Point>,
    /// Freeform being drawn: points so far (plus the rubber-band point), smoothed, and whether
    /// the pointer is over the start (closing the shape).
    pub path_preview: Vec<Point>,
    pub path_smooth: bool,
    pub path_closing: bool,
    /// The shape in Edit Points: its vertices are handles instead of the box's.
    pub edit_points: Option<ShapeId>,
    /// The vertex (path, index) whose control handles show in Edit Points.
    pub edit_vertex: Option<(usize, usize)>,
    /// The segment (path, index of the vertex ending it) last right-clicked in Edit Points, for
    /// the segment menu.
    pub edit_segment: Option<(usize, usize)>,
    /// Where on that segment the right click was (0..1), where Add Point puts the new point.
    pub edit_segment_t: Option<f64>,
    /// The point type last chosen for a vertex (shape, path, index, type): handle drags follow it.
    pub vertex_kind: Option<(ShapeId, usize, usize, String)>,
    /// Drawing guides are shown (View › Guides): they can be dragged and shapes snap to them.
    pub guides_visible: bool,
    /// The guide being dragged and where it is now (slide points); `None` once dropped.
    pub guide_drag: Option<(usize, f64)>,
    /// The drawing guide under the pointer, for the cursor.
    pub guide_hover: Option<usize>,
    /// Pressure from the pointer event being handled, if the device reported one.
    pressure: Option<f32>,
}

impl ToolState {
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The shape in Edit Points, while it is still the one selected shape.
    pub fn editing_points(&self, st: &crate::DocState) -> Option<ShapeId> {
        // Only a freeform has points: after Undo takes the conversion back, Edit Points is over.
        self.edit_points.filter(|id| {
            st.selection.shapes.as_slice() == [*id]
                && st.selection.text.is_none()
                && st.shape(*id).is_some_and(|sh| matches!(sh.geom, deckcraft_model::Geom::Custom { .. }))
        })
    }

    /// A freeform path is being drawn (Enter/Escape/double-click finish it).
    pub fn drawing_freeform(&self) -> bool {
        matches!(self.drag, Some(Drag::Freeform { .. }))
    }
}

/// Handle positions in slide coordinates for a box (8 resize + rotate).
pub fn box_handles(x: &Xfrm) -> ([Point; 8], Point) {
    let a = x.affine();
    let (w, h) = (x.w, x.h);
    let local = [(0.0, 0.0), (w / 2.0, 0.0), (w, 0.0), (w, h / 2.0), (w, h), (w / 2.0, h), (0.0, h), (0.0, h / 2.0)];
    let pts = local.map(|(px, py)| a * Point::new(px, py));
    // The rotation handle sits above the top edge (in the shape's rotated frame).
    let up = if x.flip_v { 1.0 } else { -1.0 };
    let rot =
        Affine::translate(Vec2::new(x.x + w / 2.0, x.y + h / 2.0)) * Affine::rotate(x.rot.to_radians()) * Point::new(0.0, up * (h / 2.0 + 20.0));
    (pts, rot)
}

/// WordArt adjust handles for `sh`, in shape-local points (the text rectangle). Empty when the
/// resolved body has no warp DeckCraft draws, or the shape has no text to store one on.
pub fn wordart_handles(
    doc: &deckcraft_model::Presentation,
    sel: &crate::Selection,
    sh: &Shape,
    w: f64,
    h: f64,
) -> Vec<deckcraft_geom::warp::WarpHandle> {
    if sh.text.is_none() {
        return Vec::new();
    }
    let Some(ctx) = crate::cmd::text::ctx_for(doc, sel) else { return Vec::new() };
    let bp = deckcraft_model::resolve::body(&ctx, sh);
    let Some(name) = bp.warp.as_deref().filter(|n| deckcraft_geom::warp::is_supported(n)) else { return Vec::new() };
    let tr = deckcraft_render::shape_geometry(sh, w, h).text_rect;
    deckcraft_geom::warp::handles(name, &bp.warp_adj, tr)
}

/// Write one warp adjust, keeping the others from `base` (empty = the preset defaults).
fn set_warp_adj(sh: &mut Shape, name: &str, base: &[f64], index: usize, value: f64) {
    let Some(text) = sh.text.as_mut() else { return };
    if !value.is_finite() || index > 7 {
        return;
    }
    let defaults = deckcraft_geom::warp::adjust_defaults(name);
    let n = defaults.len().max(index + 1).min(8);
    if n == 0 {
        return;
    }
    let mut adj = Vec::with_capacity(n);
    for i in 0..n {
        let v = if i == index { value } else { base.get(i).copied().filter(|v| v.is_finite()).or_else(|| defaults.get(i).copied()).unwrap_or(0.0) };
        adj.push(v);
    }
    text.body.warp = Some(name.to_string());
    text.body.warp_adj = adj;
}

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

    fn drag(&mut self, p: Point, mods: Mods, tol: f64) -> Result<Value> {
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

    fn double_click(&mut self, p: Point, tol: f64) -> Result<Value> {
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
    fn context_click(&mut self, p: Point, tol: f64) -> Result<Value> {
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
    fn place_caret(&mut self, p: Point, tol: f64, extend: bool) -> Result<Option<(ShapeId, (usize, usize))>> {
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
    fn edit_text_at(&mut self, id: ShapeId, p: Point) -> Result<bool> {
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
    fn begin_table_border(&mut self, id: ShapeId, column: bool, index: usize, p: Point) -> Result<()> {
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

    fn is_picture(&self, id: ShapeId) -> bool {
        self.active().and_then(|d| d.shape(id)).is_some_and(|s| matches!(s.kind, ShapeKind::Picture { .. } | ShapeKind::Media(_)))
    }

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
    fn glue_at(&self, p: Point, tol: f64) -> Option<(ShapeId, u32, Point)> {
        let st = self.active()?;
        crate::connect::nearest_site(&st.doc, &st.selection, st.shapes(), p, (tol * 2.0).max(6.0), None)
    }

    /// Show the sites of the shape near `p` and return the site an end at `p` would glue to.
    fn update_sites(&mut self, p: Point, tol: f64, exclude: Option<ShapeId>) -> Option<(ShapeId, u32, Point)> {
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

    fn set_xfrm(&mut self, id: ShapeId, x: Xfrm) -> Result<()> {
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

    fn set_xfrm_cmd(&mut self, id: ShapeId, x: Xfrm) -> Result<Value> {
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

    fn refit_after_resize(&mut self, id: ShapeId) -> Result<()> {
        // A text box resized by hand keeps wrapping at its new width.
        let st = self.doc()?;
        if st.shape(id).is_some_and(|s| s.text_box) {
            let _ = cmd::text::fit_text_box(self, id);
        }
        Ok(())
    }

    fn execute_dup_in_place(&mut self, ids: &[ShapeId]) -> Result<()> {
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
    fn snap_rect(&self, r: Rect, exclude: &[ShapeId], tol: f64, enabled: bool) -> (Vec2, Vec<Guide>) {
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
    fn snap_resize(&mut self, x: Xfrm, _orig: &Xfrm, handle: u8, id: ShapeId, tol: f64) -> Xfrm {
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
    fn ink_sample(&self, p: Point) -> (f64, f64, f32) {
        let pressure = match self.tool.pressure {
            Some(v) if v.is_finite() => v.clamp(0.0, 1.0),
            _ => 1.0,
        };
        (p.x, p.y, pressure)
    }

    fn add_ink(&mut self, points: Vec<(f64, f64, f32)>) -> Result<Value> {
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

    fn erase_at(&mut self, p: Point, tol: f64) -> Result<Value> {
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

fn is_text_first(st: &crate::DocState, id: ShapeId) -> bool {
    st.shape(id).is_some_and(|s| s.ph.is_some() || s.text_box)
}

/// The topmost text-holding shape under `p` inside group `sh`, recursing into nested groups.
/// Group children live in the group's child space (`ShapeKind::Group::child`), so the point is
/// mapped through `group_child_affine`, exactly as `hit_shape` does for group hit-testing.
fn text_member_at(st: &crate::DocState, sh: &Shape, p: Point, tol: f64) -> Option<ShapeId> {
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
struct EditedBox {
    id: ShapeId,
    xfrm: Xfrm,
    /// Its frame can be grabbed to move it: not a line (no frame to grab) and not locked.
    movable: bool,
}

/// Whether a visible shape drawn in front of `target` is under `p`: the shapes above it in its
/// list, topmost first, and inside a group that holds it, the members above it.
fn shape_in_front(st: &crate::DocState, shapes: &[Shape], target: ShapeId, p: Point, tol: f64, depth: usize) -> bool {
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

fn edited_box(st: &crate::DocState) -> Option<EditedBox> {
    let t = st.selection.text.as_ref()?;
    if t.notes || t.cell.is_some() {
        return None;
    }
    let sh = st.shape(t.shape)?;
    Some(EditedBox { id: t.shape, xfrm: xfrm_of(&st.doc, &st.selection, sh), movable: !sh.is_line() && !sh.locked })
}

/// Where a point falls against the box of the text being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditZone {
    /// Well inside: the caret goes there.
    Inside,
    /// On the frame: a band `tol` wide on either side of the box edge.
    Frame,
    Outside,
}

/// Classify slide point `p` against box `x`, in the box's own (rotated, flipped) frame. Inside
/// small boxes the band narrows to a quarter of the box so their text stays reachable.
fn edit_zone(x: &Xfrm, p: Point, tol: f64) -> EditZone {
    let l = x.affine().inverse() * p;
    let (w, h) = (x.w.max(0.0), x.h.max(0.0));
    let tol = if tol.is_finite() { tol.max(0.0) } else { 0.0 };
    if !(l.x.is_finite() && l.y.is_finite()) || l.x < -tol || l.y < -tol || l.x > w + tol || l.y > h + tol {
        return EditZone::Outside;
    }
    let inner = tol.min(w / 4.0).min(h / 4.0);
    if l.x > inner && l.y > inner && l.x < w - inner && l.y < h - inner { EditZone::Inside } else { EditZone::Frame }
}

fn contains_rect(outer: Rect, inner: Rect) -> bool {
    inner.x0 >= outer.x0 && inner.y0 >= outer.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

fn constrain_45(a: Point, b: Point) -> Point {
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
fn in_table_cell(st: &crate::DocState, id: ShapeId, p: Point) -> bool {
    st.shape(id).and_then(|sh| cmd::table::table_frame(st, sh)).is_some_and(|f| {
        let q = f.local(p);
        (0.0..=f.width()).contains(&q.x) && (0.0..=f.height()).contains(&q.y) && f.cell_at(q).is_some()
    })
}

fn hit_shape(st: &crate::DocState, sh: &Shape, p: Point, tol: f64) -> Option<Hit> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_corner_and_flip() {
        let o = Xfrm::new(100.0, 100.0, 100.0, 50.0);
        let n = resize_box(&o, 4, Vec2::new(20.0, 10.0), false, false);
        assert!((n.x - 100.0).abs() < 1e-9 && (n.w - 120.0).abs() < 1e-9 && (n.h - 60.0).abs() < 1e-9);
        let n = resize_box(&o, 0, Vec2::new(10.0, 10.0), false, false);
        assert!((n.x - 110.0).abs() < 1e-9 && (n.w - 90.0).abs() < 1e-9);
        let n = resize_box(&o, 3, Vec2::new(-150.0, 0.0), false, false);
        assert!(n.flip_h && (n.w - 50.0).abs() < 1e-9 && (n.x - 50.0).abs() < 1e-9);
        let n = resize_box(&o, 4, Vec2::new(100.0, 0.0), true, false);
        assert!((n.w / n.h - 2.0).abs() < 1e-9);
        let n = resize_box(&o, 3, Vec2::new(10.0, 0.0), false, true);
        assert!((n.w - 120.0).abs() < 1e-9 && (n.x - 90.0).abs() < 1e-9);
    }

    #[test]
    fn resize_rotated_keeps_opposite_corner() {
        let o = Xfrm { x: 100.0, y: 100.0, w: 100.0, h: 50.0, rot: 90.0, ..Default::default() };
        let anchor = o.affine() * Point::new(0.0, 0.0);
        let n = resize_box(&o, 4, Vec2::new(-10.0, 30.0), false, false);
        let anchor2 = n.affine() * Point::new(0.0, 0.0);
        assert!((anchor - anchor2).hypot() < 1e-6, "{anchor:?} {anchor2:?}");
    }
}
