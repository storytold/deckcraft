//! Pointer tools. Gestures arrive in slide coordinates (points) through [`Session::pointer`] from
//! the UI, the control channel or MCP alike; each finished gesture is one undo step.

use std::sync::Arc;

use deckcraft_geom::{Affine, Point, Rect, Vec2, Xfrm};
use deckcraft_model::{Shape, ShapeId, ShapeKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cmd::{self, xfrm_of};
use crate::{HistoryEntry, Result, Session, TextSel};

mod click;
mod drag;
mod geometry;
mod hit;
mod pointer;

pub use hit::{line_xfrm, resize_box, table_rows, text_pos};

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
