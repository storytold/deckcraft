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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub cmd: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PointerEvent {
    pub kind: PointerKind,
    pub x: f64,
    pub y: f64,
    pub mods: Mods,
    /// Hit tolerance in points (≈ 4 screen px at the current zoom).
    pub tol: f64,
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
    /// Line end 0/1 of a line-like shape.
    LineEnd {
        id: ShapeId,
        end: u8,
    },
    Shape {
        id: ShapeId,
        text: bool,
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
    Marquee {
        start: Point,
        add: bool,
    },
    Create {
        start: Point,
    },
    TextSelect {
        shape: ShapeId,
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
}

impl ToolState {
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
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
        // Handles of selected shapes first.
        if st.selection.text.is_none() || st.selection.shapes.len() == 1 {
            for id in st.selection.shapes.iter().rev() {
                let Some(sh) = st.shape(*id) else { continue };
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
        // Shapes, topmost first.
        for sh in st.shapes().iter().rev() {
            if sh.hidden || sh.locked {
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
        if !(p.x.is_finite() && p.y.is_finite()) {
            return Ok(Value::Null);
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
            PointerKind::DoubleClick => self.double_click(p, tol),
            PointerKind::TripleClick => {
                if self.active().is_some_and(|d| d.selection.text.is_some()) {
                    self.execute("text.selectParagraph", &json!({}))
                } else {
                    Ok(Value::Null)
                }
            }
        }
    }

    fn begin(&mut self, label: &str) {
        if let Some(st) = self.active_mut()
            && st.interaction.is_none()
        {
            st.interaction = Some(crate::Interaction { label: label.into(), doc: st.doc.clone(), selection: st.selection.clone() });
        }
    }

    fn commit(&mut self) {
        if let Some(st) = self.active_mut()
            && let Some(i) = st.interaction.take()
            && !Arc::ptr_eq(&i.doc, &st.doc)
        {
            crate::push_undo(st, HistoryEntry { label: i.label, doc: i.doc, selection: i.selection });
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
            ToolKind::Shape { .. } | ToolKind::TextBox => {
                self.tool.drag = Some(Drag::Create { start: p });
                self.tool.preview = Some(Xfrm::new(p.x, p.y, 0.0, 0.0));
                return Ok(Value::Null);
            }
            ToolKind::Ink { mode, .. } => {
                if mode == "eraser" {
                    return self.erase_at(p, tol);
                }
                self.tool.drag = Some(Drag::Ink { points: vec![(p.x, p.y, 0.5)] });
                self.tool.ink_preview = vec![(p.x, p.y, 0.5)];
                return Ok(Value::Null);
            }
            ToolKind::Select => {}
        }
        let st = self.doc()?;
        // Clicking inside the text being edited moves the caret / starts a text selection.
        if let Some(t) = st.selection.text.clone()
            && !t.notes
            && t.cell.is_none()
            && let Some(sh) = st.shape(t.shape)
        {
            let x = xfrm_of(&st.doc, &st.selection, sh);
            if x.bounds().inflate(tol, tol).contains(p)
                && !matches!(self.hit_test(p, tol), Some(Hit::Resize { .. } | Hit::Rotate { .. } | Hit::Adjust { .. }))
            {
                let pos = text_pos(st, sh, p);
                let shift = mods.shift;
                self.select(|_, sel| {
                    if let Some(tt) = sel.text.as_mut() {
                        tt.caret = pos;
                        if !shift {
                            tt.anchor = pos;
                        }
                    }
                })?;
                self.tool.drag = Some(Drag::TextSelect { shape: t.shape });
                return Ok(json!({"caret": [pos.0, pos.1]}));
            }
        }
        let hit = self.hit_test(p, tol);
        let st = self.doc()?;
        match hit {
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
            Some(Hit::Adjust { id, index }) => {
                let orig = st.shape(id).map(|s| xfrm_of(&st.doc, &st.selection, s)).unwrap_or_default();
                self.begin("Adjust Shape");
                self.tool.drag = Some(Drag::Adjust { id, index, orig });
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
                let origs: Vec<(ShapeId, Xfrm)> = ids.iter().filter_map(|i| st.shape(*i).map(|s| (*i, xfrm_of(&st.doc, &st.selection, s)))).collect();
                // A click (no drag) on text puts the caret there; unselected text boxes and
                // placeholders take the caret straight away like PowerPoint.
                let enter_text = (text && !toggle && (selected || is_text_first(st, id))).then_some((id, p));
                let ids2 = ids.clone();
                self.select(|_, sel| {
                    sel.shapes = ids2;
                    sel.text = None;
                    sel.cells = None;
                })?;
                self.tool.drag = Some(Drag::Move { ids, start: p, origs, moved: false, duplicate: mods.alt, enter_text });
                // Format Painter applies on click.
                if self.painter.is_some() && !toggle {
                    let _ = self.execute("format.painterApply", &json!({"ids": [id]}));
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
            Drag::Move { ids, start, origs, moved, duplicate, enter_text } => {
                let mut d = p - start;
                if !moved && d.hypot() < tol.max(1.0) {
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
            Drag::TextSelect { shape } => {
                let st = self.doc()?;
                if let Some(sh) = st.shape(shape) {
                    let pos = text_pos(st, sh, p);
                    self.select(|_, sel| {
                        if let Some(t) = sel.text.as_mut() {
                            t.caret = pos;
                        }
                    })?;
                }
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
                points.push((p.x, p.y, 0.5));
                self.tool.ink_preview = points.clone();
                self.tool.drag = Some(Drag::Ink { points });
            }
        }
        Ok(Value::Null)
    }

    fn up(&mut self, p: Point, mods: Mods) -> Result<Value> {
        let drag = self.tool.drag.take();
        self.tool.guides.clear();
        self.tool.sites.clear();
        self.tool.glue = None;
        let mut out = Value::Null;
        match drag {
            Some(Drag::Move { moved: false, enter_text: Some((id, at)), .. }) => {
                // Plain click on text: caret at the click.
                let st = self.doc()?;
                if let Some(sh) = st.shape(id) {
                    let pos = text_pos(st, sh, at);
                    self.execute("text.edit", &json!({"id": id, "at": [pos.0, pos.1]}))?;
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
                if !self.tool.sticky {
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
        let st = self.doc()?;
        if st.selection.text.is_some() {
            return self.execute("text.selectWord", &json!({}));
        }
        match self.hit_test(p, tol) {
            Some(Hit::Shape { id, .. }) => {
                let st = self.doc()?;
                let Some(sh) = st.shape(id).cloned() else { return Ok(Value::Null) };
                match &sh.kind {
                    ShapeKind::Table(_) => {
                        let x = xfrm_of(&st.doc, &st.selection, &sh);
                        let local = x.affine().inverse() * p;
                        if let ShapeKind::Table(t) = &sh.kind {
                            let cell = deckcraft_render_cell(t, &table_rows(st, t), local);
                            return self.execute("text.edit", &json!({"id": id, "cell": [cell.0, cell.1]}));
                        }
                        Ok(Value::Null)
                    }
                    ShapeKind::Shape | ShapeKind::Connector { .. } if !sh.is_line() => {
                        let pos = text_pos(st, &sh, p);
                        self.execute("text.edit", &json!({"id": id, "at": [pos.0, pos.1]}))?;
                        self.execute("text.selectWord", &json!({}))
                    }
                    ShapeKind::Group { .. } => Ok(Value::Null),
                    _ => {
                        self.ui_requests.push(crate::UiRequest::Pane { id: "format".into() });
                        Ok(Value::Null)
                    }
                }
            }
            _ => Ok(Value::Null),
        }
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
            cmd::edit::reid(doc, &mut copies);
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

    /// Smart guides: snap `r` edges/centres to the slide and other shapes. Returns the offset to
    /// apply and the guides to draw.
    fn snap_rect(&self, r: Rect, exclude: &[ShapeId], tol: f64, enabled: bool) -> (Vec2, Vec<Guide>) {
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
        for edge in [r.x0, r.center().x, r.x1] {
            for (x, b) in &xs {
                let d = x - edge;
                if d.abs() <= tol && best_x.is_none_or(|(bd, _, _)| d.abs() < bd.abs()) {
                    best_x = Some((d, *x, *b));
                }
            }
        }
        let mut best_y: Option<(f64, f64, Rect)> = None;
        for edge in [r.y0, r.center().y, r.y1] {
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
        } else if self.prefs.snap_to_grid {
            off.x = deckcraft_geom::snap(r.x0, self.prefs.grid_spacing) - r.x0;
        }
        if let Some((d, y, b)) = best_y {
            off.y = d;
            let rr = r + Vec2::new(0.0, d);
            guides.push(Guide { vertical: false, pos: y, from: b.x0.min(rr.x0), to: b.x1.max(rr.x1) });
        } else if self.prefs.snap_to_grid {
            off.y = deckcraft_geom::snap(r.y0, self.prefs.grid_spacing) - r.y0;
        }
        (off, guides)
    }

    fn snap_resize(&mut self, x: Xfrm, _orig: &Xfrm, handle: u8, id: ShapeId, tol: f64) -> Xfrm {
        if x.rot != 0.0 {
            self.tool.guides.clear();
            return x;
        }
        let r = x.rect();
        let (off, guides) = self.snap_rect(r, &[id], tol * 1.5, true);
        let mut nx = x;
        // Only the dragged edges move.
        let moves_left = matches!(handle, 0 | 6 | 7);
        let moves_right = matches!(handle, 2..=4);
        let moves_top = matches!(handle, 0..=2);
        let moves_bottom = matches!(handle, 4..=6);
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

    fn add_ink(&mut self, points: Vec<(f64, f64, f32)>) -> Result<Value> {
        let ToolKind::Ink { mode, color, width } = self.tool.kind.clone() else { return Ok(Value::Null) };
        let stroke = deckcraft_model::InkStroke { points, color, width, highlighter: mode == "highlighter" };
        // Strokes join the slide's ink layer (one ink shape per slide).
        let existing = self.doc()?.shapes().iter().rev().find(|s| matches!(s.kind, ShapeKind::Ink { .. })).map(|s| s.id);
        self.edit(|doc, sel| {
            let id = match existing {
                Some(id) => id,
                None => {
                    let id = ShapeId(doc.alloc_id());
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
        if let Some(st) = self.active_mut() {
            crate::push_undo(st, HistoryEntry { label: "Erase".into(), doc: before, selection: sel_before });
        }
        Ok(Value::Null)
    }
}

fn is_text_first(st: &crate::DocState, id: ShapeId) -> bool {
    st.shape(id).is_some_and(|s| s.ph.is_some() || s.text_box)
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

fn hit_shape(st: &crate::DocState, sh: &Shape, p: Point, tol: f64) -> Option<Hit> {
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let inv = x.affine().inverse();
    let local = inv * p;
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
                    ShapeKind::Picture { .. } | ShapeKind::Table(_) | ShapeKind::Chart(_) | ShapeKind::Media(_) | ShapeKind::Opaque { .. }
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
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let local = x.affine().inverse() * p;
    let t = TextSel { shape: sh.id, ..Default::default() };
    match cmd::text::layout_for(st, &t) {
        Some(l) => {
            let h = l.hit(local);
            (h.para, h.ch)
        }
        None => (0, 0),
    }
}

/// Row heights of table `t` as drawn: rows grow to fit their text.
pub fn table_rows(st: &crate::DocState, t: &deckcraft_model::Table) -> Vec<f64> {
    cmd::table_drawn_heights(&st.doc, &st.selection, t)
}

/// Which table cell contains a table-local point (`heights` from [`table_rows`]).
fn deckcraft_render_cell(t: &deckcraft_model::Table, heights: &[f64], p: Point) -> (usize, usize) {
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
