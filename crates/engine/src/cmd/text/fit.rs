//! `text.fit`: how each text body fits its shape as DeckCraft draws it. A placeholder that shrinks
//! its text on overflow usually inherits the setting from its layout, so `shape.inspect` shows
//! `autofit: null` while the slide draws the text at half size; this query reports the setting in
//! effect, the scale and line spacing reduction the layout applied, the drawn font sizes and how
//! far the text runs past its frame.

use deckcraft_model::text::AutoFit;
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
    /// Height of the laid-out text and of the frame it sits in (insets applied), in points.
    pub content_height: f64,
    pub frame_height: f64,
    /// The layout's overflow test: the text is taller than its frame by more than half a point.
    pub overflows: bool,
    /// How far the drawn lines reach past the frame's top and bottom edges together, in points.
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
/// values the page draws (slide number, date, footer). `None` for a shape with no text body and for
/// tables, whose rows grow to fit their text.
pub(crate) fn fit_of(doc: &Presentation, sel: &Selection, fields: &dyn deckcraft_text::Fields, sh: &Shape) -> Option<Fit> {
    if matches!(sh.kind, ShapeKind::Table(_)) {
        return None;
    }
    let body = sh.text.as_ref()?;
    let ctx = ctx_for(doc, sel)?;
    let bp = resolve::body(&ctx, sh);
    let x = xfrm_of(doc, sel, sh);
    let rect = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
    let opts = deckcraft_text::Opts { rect, fields, prompt_color: None, no_shrink: false };
    let l = deckcraft_text::layout(&ctx, sh, body, &opts);
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

/// Every text body on slide `index`, in drawing order, group members included.
pub(crate) fn slide_fits(doc: &Presentation, index: usize) -> Vec<Fit> {
    let Some(slide) = doc.slides.get(index) else { return vec![] };
    let sel = Selection { slide: index, target: Target::Slides, ..Default::default() };
    // The slide number, date and footer text the renderer puts in this slide's fields.
    let fields = deckcraft_render::slide_fields(doc, index);
    let mut out = vec![];
    deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
        if let Some(f) = fit_of(doc, &sel, &fields, sh) {
            out.push(f);
        }
    });
    out
}

/// `text.fit {index?, ids?}`: the fit of every text body on a slide (the current one by default),
/// or of the shapes `ids` names.
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
                    let on_slide = st.doc.slides.get(index).and_then(|sl| deckcraft_model::find_shape(&sl.shapes, id)).is_some();
                    bad(
                        CMD,
                        if on_slide {
                            format!("shape {id} has no text body to fit (tables grow their rows instead)")
                        } else {
                            format!("no shape {id} on slide {index}")
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
