//! `text.fit`: how each text body fits its shape as DeckCraft draws it. A placeholder that shrinks
//! its text on overflow usually inherits the setting from its layout, so `shape.inspect` shows
//! `autofit: null` while the slide draws the text at half size; this query reports the setting in
//! effect, the scale and line spacing reduction the layout applied, the drawn font sizes and how
//! far the drawn lines reach past the frame's top and bottom edges (its left and right edges for
//! vertical text, whose lines stack from side to side).

use std::borrow::Cow;

use deckcraft_geom::{Affine, Rect};
use deckcraft_model::resolve::Ctx;
use deckcraft_model::text::{AutoFit, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::{Result, Selection, Session, Target, ctx_for, resolve, xfrm_of};
use crate::cmd::{bad, opt_usize_param};

const CMD: &str = "text.fit";

/// One text body's fit, measured on the layout the slide draws.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fit {
    pub id: ShapeId,
    pub name: String,
    /// The autofit in effect after placeholder inheritance: `none`, `shrink` (shrink text on
    /// overflow) or `shape` (resize shape to fit text).
    pub autofit: &'static str,
    /// Where that autofit is set: `shape` (the shape's own), `layout` or `master` (the
    /// placeholder it inherits from) or `default` (none is set: text boxes resize, other shapes
    /// do not fit).
    pub autofit_from: &'static str,
    /// Font scale the layout applied (1 = full size).
    pub font_scale: f64,
    /// Line spacing reduction the layout applied (0 = none).
    pub line_reduction: f64,
    /// Smallest and largest font size of the drawn text, in points, after the scale (bullets are
    /// left out); `None` for a body with no drawn text.
    pub sizes: Option<(f64, f64)>,
    pub lines: usize,
    /// Height of the laid-out text and of the frame it sits in (insets applied), in points on the
    /// slide: a group member's at the size its groups draw it. Vertical text is laid out in its
    /// frame turned a quarter turn and measured across its lines, so `frame_height` is the frame's
    /// width. For text set in columns, `content_height` is its height set in one column, which can
    /// exceed `frame_height` while every column fits.
    pub content_height: f64,
    pub frame_height: f64,
    /// The layout's overflow test: the text is taller than its frame by more than half a point.
    pub overflows: bool,
    /// How far the drawn lines reach past the frame's top and bottom edges together, in points:
    /// past its left and right edges for vertical text. Text set in columns is measured where its
    /// columns draw its lines: each column starts at the top, and the last takes the lines the
    /// others leave.
    pub overflow: f64,
}

impl Fit {
    pub(crate) fn to_json(&self) -> Value {
        let r1 = |v: f64| (v * 10.0).round() / 10.0;
        let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
        json!({
            "id": self.id,
            "name": self.name,
            "autofit": self.autofit,
            "autofitFrom": self.autofit_from,
            "fontScale": r3(self.font_scale),
            "lineReduction": r3(self.line_reduction),
            "minSize": self.sizes.map(|s| r1(s.0)),
            "maxSize": self.sizes.map(|s| r1(s.1)),
            "lines": self.lines,
            "contentHeight": r1(self.content_height),
            "frameHeight": r1(self.frame_height),
            "overflows": self.overflows,
            "overflowPt": r1(self.overflow),
        })
    }
}

/// The fit of `sh`'s text on the page `sel` names, as the slide draws it: `fields` are the field
/// values the page draws (slide number, date, footer), and `parent` maps the coordinates `sh` is
/// placed in onto the slide, `depth` groups down (the identity and 0 for a shape on the slide
/// itself). `None` for a shape with no text body and for tables, whose rows grow to fit their text.
pub(crate) fn fit_of(
    doc: &Presentation,
    sel: &Selection,
    fields: &dyn deckcraft_text::Fields,
    sh: &Shape,
    parent: Affine,
    depth: usize,
) -> Option<Fit> {
    if matches!(sh.kind, ShapeKind::Table(_)) {
        return None;
    }
    let body = sh.text.as_ref()?;
    let ctx = ctx_for(doc, sel)?;
    let bp = resolve::body(&ctx, sh);
    let x = xfrm_of(doc, sel, sh);
    let (w, h) = text_size(&ctx, sh, x.w, x.h, parent * x.affine(), depth);
    let rect = deckcraft_render::shape_geometry(sh, w, h).text_rect;
    let body = drawn_body(body);
    let opts = deckcraft_text::Opts { rect, fields, prompt_color: None, no_shrink: false };
    let l = deckcraft_text::layout(&ctx, sh, &body, &opts);
    let autofit = match bp.autofit {
        None | Some(AutoFit::None) => "none",
        Some(AutoFit::Shrink { .. }) => "shrink",
        Some(AutoFit::Shape) => "shape",
    };
    let set = |s: Option<&Shape>| s.and_then(|s| s.text.as_ref()).is_some_and(|t| t.body.autofit.is_some());
    let (layout_ph, master_ph) = resolve::parents(&ctx, sh);
    let autofit_from = if body.body.autofit.is_some() {
        "shape"
    } else if set(layout_ph) {
        "layout"
    } else if set(master_ph) {
        "master"
    } else {
        "default"
    };
    // Bullets are drawn runs too, but they carry no paragraph characters.
    let sizes = l
        .runs
        .iter()
        .filter(|r| !r.cells.is_empty() && r.size.is_finite())
        .fold(None, |acc: Option<(f64, f64)>, r| Some(acc.map_or((r.size, r.size), |(lo, hi)| (lo.min(r.size), hi.max(r.size)))));
    let inner = l.inner;
    let top = l.lines.iter().map(|li| li.top).fold(f64::INFINITY, f64::min);
    let bottom = l.lines.iter().map(|li| li.bottom).fold(f64::NEG_INFINITY, f64::max);
    let overflow = if l.lines.is_empty() { 0.0 } else { (inner.y0 - top).max(0.0) + (bottom - inner.y1).max(0.0) };
    Some(Fit {
        id: sh.id,
        name: sh.name.clone(),
        autofit,
        autofit_from,
        font_scale: l.font_scale,
        line_reduction: l.line_reduction,
        sizes,
        lines: l.lines.len(),
        content_height: l.content_height,
        frame_height: inner.height(),
        overflows: l.overflow,
        overflow: if overflow.is_finite() { overflow } else { 0.0 },
    })
}

/// The body the slide lays out for `body`. The renderer draws no text for a body whose paragraphs
/// are all empty (the editor shows only a caret or a placeholder prompt there), so such a body is
/// measured as one with no paragraphs: its frame, and no lines or overflow.
fn drawn_body(body: &TextBody) -> Cow<'_, TextBody> {
    if body.is_empty() && !body.paragraphs.is_empty() {
        let mut none = body.clone();
        none.paragraphs.clear();
        Cow::Owned(none)
    } else {
        Cow::Borrowed(body)
    }
}

/// The width and height the renderer sets `sh`'s text in (`Renderer::text_space`): `w` x `h`, the
/// shape's own size, stretched along each of its axes as much as `m`, its transform to the slide,
/// stretches them. A group's child coordinates size and place a member's box but never its type,
/// so a member of a resized group wraps its text at its size on the slide. A shape on the slide
/// itself, a scale within 1e-9 of 1 on both axes and a degenerate transform keep `w` x `h`.
fn text_size(ctx: &Ctx, sh: &Shape, w: f64, h: f64, mut m: Affine, depth: usize) -> (f64, f64) {
    if depth == 0 {
        return (w, h);
    }
    // The renderer measures the transform it draws the shape with, 3-D camera turn included.
    if let Some(turn) = deckcraft_render::camera_turn(ctx, sh, w, h) {
        m *= turn;
    }
    let c = m.as_coeffs();
    let (sx, sy) = (c[0].hypot(c[1]), c[2].hypot(c[3]));
    let ok = |k: f64| k.is_finite() && k > 1e-9;
    if !ok(sx) || !ok(sy) || ((sx - 1.0).abs() < 1e-9 && (sy - 1.0).abs() < 1e-9) {
        return (w, h);
    }
    let (tw, th) = (w * sx, h * sy);
    if tw.is_finite() && th.is_finite() { (tw, th) } else { (w, h) }
}

/// Every text body on slide `index`, in drawing order, group members included: hidden shapes too
/// (measured as the slide would draw them), but none nested deeper than the renderer draws
/// ([`deckcraft_render::MAX_DRAW_DEPTH`] groups down).
pub(crate) fn slide_fits(doc: &Presentation, index: usize) -> Vec<Fit> {
    let Some(slide) = doc.slides.get(index) else { return vec![] };
    let sel = Selection { slide: index, target: Target::Slides, ..Default::default() };
    // The slide number, date and footer text the renderer puts in this slide's fields.
    let fields = deckcraft_render::slide_fields(doc, index);
    let mut out = vec![];
    fits_in(doc, &sel, &fields, &slide.shapes, Affine::IDENTITY, 0, &mut out);
    out
}

/// Appends the fits of `shapes` and of their group members to `out`, depth first in the order
/// `deckcraft_model::walk` visits them, down to the depth the renderer draws. `parent` maps the
/// shapes' coordinates onto the slide; a group's members get the group's child coordinates mapped
/// onto its box, as the renderer draws them.
fn fits_in(
    doc: &Presentation,
    sel: &Selection,
    fields: &dyn deckcraft_text::Fields,
    shapes: &[Shape],
    parent: Affine,
    depth: usize,
    out: &mut Vec<Fit>,
) {
    if depth > deckcraft_render::MAX_DRAW_DEPTH {
        return;
    }
    for sh in shapes {
        if let Some(f) = fit_of(doc, sel, fields, sh, parent, depth) {
            out.push(f);
        }
        if let ShapeKind::Group { children, child } = &sh.kind {
            let ch = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
            let gm = parent * deckcraft_geom::group_child_affine(&xfrm_of(doc, sel, sh), ch);
            fits_in(doc, sel, fields, children, gm, depth + 1, out);
        }
    }
}

/// How many groups deep shape `id` sits in `shapes` (0 for one of `shapes` itself), taking the
/// first match in the order `deckcraft_model::walk` visits shapes; `None` when it is not there.
/// `walk` stops 64 groups down, so a shape nested deeper would read as absent; this follows groups
/// as deep as a file nests them, keeping the groups it is inside on a list rather than on the call
/// stack, so no nesting depth can overflow it.
fn depth_of(shapes: &[Shape], id: ShapeId) -> Option<usize> {
    // The shapes still to visit at each level, outermost first: a shape taken from the last level
    // sits one group deeper than the shapes of the level before it.
    let mut levels = vec![shapes.iter()];
    while let Some(level) = levels.last_mut() {
        let Some(sh) = level.next() else {
            levels.pop();
            continue;
        };
        if sh.id == id {
            return Some(levels.len().saturating_sub(1));
        }
        let members = sh.children();
        if !members.is_empty() {
            levels.push(members.iter());
        }
    }
    None
}

/// `text.fit {index?, ids?}`: the fit of every text body on a slide (the current one by default),
/// or of the shapes `ids` names. The slide answer leaves out shapes nested deeper than the
/// renderer draws, so `ids` refuses them, naming their depth.
pub(super) fn fit(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let index = opt_usize_param(p, "index", CMD)?.unwrap_or(st.selection.slide);
    if index >= st.doc.slides.len() {
        return Err(bad(CMD, format!("no slide {index}")));
    }
    let wanted = match p.get("ids") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => {
            let mut ids = Vec::with_capacity(a.len().min(4096));
            for v in a {
                let id = v.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(CMD, format!("`ids` has an invalid shape id: {v}")))?;
                ids.push(ShapeId(id));
            }
            Some(ids)
        }
        Some(v) => return Err(bad(CMD, format!("`ids` is not a list of shape ids: {v}"))),
    };
    let fits = slide_fits(&st.doc, index);
    let bodies: Vec<Value> = match wanted {
        None => fits.iter().map(Fit::to_json).collect(),
        Some(ids) => {
            let mut v = Vec::with_capacity(ids.len());
            for id in ids {
                let f = fits.iter().find(|f| f.id == id).ok_or_else(|| {
                    let found = st.doc.slides.get(index).and_then(|sl| depth_of(&sl.shapes, id));
                    let max = deckcraft_render::MAX_DRAW_DEPTH;
                    bad(
                        CMD,
                        match found {
                            Some(depth) if depth > max => {
                                format!("shape {id} is nested {depth} groups deep and the slide draws nothing deeper than {max}")
                            }
                            Some(_) => format!("shape {id} has no text body to fit (tables grow their rows instead)"),
                            None => format!("no shape {id} on slide {index}"),
                        },
                    )
                })?;
                v.push(f.to_json());
            }
            v
        }
    };
    Ok(json!({"index": index, "bodies": bodies}))
}
