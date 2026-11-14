//! Picture facts for agents: the pixel size of a picture's image, its resolution as drawn and how
//! far its box stretches it, read from the image header when asked (`slide.inspect`,
//! `shape.inspect`, `review.pictures`), and the box and crop `insert.picture` gives a picture for
//! its `fit`.
//!
//! Nothing here is stored. A media item keeps only its bytes, picture compression rewrites them,
//! and the decoder stops after the header, so each request reads the header again.

use deckcraft_geom::{Affine, Point, Rect, Xfrm};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::style::{PictureFill, PictureMode};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};
use serde_json::{Map, Value, json};

use super::{CommandSpec, bad, cmd, has_doc};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        query "review.pictures",
        "Check Pictures",
        [],
        None,
        "{minPpi?: lowest image pixels per inch as drawn (0 to 1200, default 96; 0 turns the check off), maxDistortion?: largest max(r, 1/r) - 1 where r is the shown aspect over the source aspect (0 to 100, default 0.02)} → [{slide, shape, name, kind: stretched|lowResolution, ppi: [across, down], distortion, fitRect: [x, y, w, h]}] (pictures on slides, group members included; a picture that is both gets one entry of each kind; fitRect is the largest box inside the picture's box that draws it undistorted, centred, in the coordinates shape.setBounds takes; pictures whose image states no pixel size, such as SVG, are left out)",
        has_doc,
        check
    )]
}

/// Lowest resolution `review.pictures` accepts by default, in image pixels per inch as drawn: the
/// lowest target `picture.compress` offers. A 960 pt (13.33 in) wide slide shown on a 1280 px wide
/// screen gets 96 screen pixels per inch, so a picture below it shows fewer image pixels than
/// screen pixels even there; 144 ppi matches a 1920 px wide screen.
const MIN_PPI: f64 = 96.0;
/// Largest distortion `review.pictures` accepts by default: twice the error that rounding a
/// fitted 100 pt box to whole points can add (100.5 / 99.5 - 1, about 1%).
const MAX_DISTORTION: f64 = 0.02;
/// Upper bounds of the two thresholds: the highest `ppi` `picture.compress` accepts, and a
/// 101-fold stretch.
const MIN_PPI_LIMIT: f64 = 1200.0;
const MAX_DISTORTION_LIMIT: f64 = 100.0;
/// Deepest group nesting walked, as in the model's other walks.
const MAX_DEPTH: usize = 64;

/// Width and height in pixels from an image's header; `None` for SVG, metafiles and anything else
/// the decoder can't read, and for a zero side.
pub(crate) fn pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok().filter(|&(w, h)| w > 0 && h > 0)
}

/// Width and height on the slide, in points, of a box `x` whose coordinates `to_slide` maps to the
/// slide: the lengths of its top and left edges there, so rotation leaves them as they are and a
/// scaled group scales them.
pub(crate) fn displayed_size(to_slide: Affine, x: &Xfrm) -> (f64, f64) {
    let local_to_slide = to_slide * x.affine();
    let origin = local_to_slide * Point::ZERO;
    let across = local_to_slide * Point::new(x.w, 0.0);
    let down = local_to_slide * Point::new(0.0, x.h);
    ((across - origin).hypot(), (down - origin).hypot())
}

/// The map from a group's child coordinates to the slide, for a group whose box `group` is drawn
/// through `to_slide` and whose child coordinate space is `child`.
pub(crate) fn child_space_to_slide(to_slide: Affine, group: &Xfrm, child: &Xfrm) -> Affine {
    let child_rect = Rect::new(child.x, child.y, child.x + child.w, child.y + child.h);
    to_slide * deckcraft_geom::group_child_affine(group, child_rect)
}

/// `v` when it is positive and finite.
fn positive(v: f64) -> Option<f64> {
    (v > 0.0 && v.is_finite()).then_some(v)
}

/// `a / b` when both are positive and the quotient is positive and finite.
fn ratio(a: f64, b: f64) -> Option<f64> {
    if a > 0.0 && b > 0.0 { positive(a / b) } else { None }
}

/// `v` rounded to `places` decimals, or as it is where the scaling would overflow.
fn round(v: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    let r = (v * k).round() / k;
    if r.is_finite() { r } else { v }
}

/// What `slide.inspect`, `shape.inspect` and `review.pictures` say about one picture. A field is
/// `None` when it can't be known: there is no pixel size for SVG or unreadable bytes, and no drawn
/// size for a tiled picture or an empty box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Facts {
    /// The image's width and height in pixels, from its header.
    pub pixels: Option<(u32, u32)>,
    /// Image pixels per inch of the picture as drawn, across and down, after the crop.
    pub ppi: Option<(f64, f64)>,
    /// Width over height of the part of the image the crop shows, in pixels (padding from a
    /// negative crop counts).
    pub source: Option<f64>,
    /// Width over height of that part as drawn on the slide.
    pub shown: Option<f64>,
}

impl Facts {
    /// How far the box stretches the picture: `max(r, 1/r) - 1` for `r` = shown aspect over source
    /// aspect. 0 is undistorted; 0.1 means one direction is drawn 10% longer, relative to the other,
    /// than the image has it.
    pub(crate) fn distortion(self) -> Option<f64> {
        let r = ratio(self.shown?, self.source?)?;
        let d = r.max(1.0 / r) - 1.0;
        d.is_finite().then_some(d)
    }

    /// The largest box inside `x`, the picture's box in the coordinates it is stored in, that draws
    /// the picture undistorted, centred on `x`. Drawn sizes scale with stored ones along each
    /// axis, so the stored aspect that turns `shown` into `source` is `source × (x.w / x.h) / shown`.
    pub(crate) fn fit_rect(self, x: &Xfrm) -> Option<Xfrm> {
        let want = ratio(self.source? * ratio(x.w, x.h)?, self.shown?)?;
        Some(contain(x, want))
    }

    /// The `pixels`, `ppi` and `aspect` fields of an inspected picture.
    fn fields(self) -> [(&'static str, Value); 3] {
        [
            ("pixels", json!(self.pixels.map(|(w, h)| [w, h]))),
            ("ppi", json!(self.ppi.map(|(across, down)| [round(across, 2), round(down, 2)]))),
            ("aspect", json!({"source": self.source.map(|a| round(a, 3)), "shown": self.shown.map(|a| round(a, 3))})),
        ]
    }

    /// Add the fields to a shape summary.
    pub(crate) fn insert_into(self, o: &mut Map<String, Value>) {
        for (key, v) in self.fields() {
            o.insert(key.into(), v);
        }
    }

    /// The fields as one object (`shape.inspect`'s `picture`).
    pub(crate) fn json(self) -> Value {
        Value::Object(self.fields().into_iter().map(|(key, v)| (key.to_string(), v)).collect())
    }
}

/// Facts for a picture filled with `fill` whose box `x` is drawn through `to_slide`.
pub(crate) fn facts(pres: &Presentation, fill: &PictureFill, x: &Xfrm, to_slide: Affine) -> Facts {
    let pixels = pres.media(fill.media).and_then(|m| pixel_size(&m.data));
    // A tiled picture repeats the image at its own scale, so the box doesn't stretch it.
    let PictureMode::Stretch { fill_rect: [l, t, r, b] } = fill.mode else {
        return Facts { pixels, source: pixels.and_then(|(w, h)| ratio(f64::from(w), f64::from(h))), ..Facts::default() };
    };
    // The image fills the box less the stretch insets, and the crop picks the part of the image
    // that area shows. `crop_dest` is the renderer's reading of the crop (clamped; negative values
    // pad): for a unit box it gives the whole image's size, 1 over the share the crop shows.
    let (w, h) = displayed_size(to_slide, x);
    let (dw, dh) = (w * (1.0 - l - r), h * (1.0 - t - b));
    let whole = deckcraft_render::crop_dest(Rect::new(0.0, 0.0, 1.0, 1.0), fill.crop);
    let shown = ratio(dw, dh);
    let Some((pw, ph)) = pixels else {
        return Facts { shown, ..Facts::default() };
    };
    let (sx, sy) = (f64::from(pw) / whole.width(), f64::from(ph) / whole.height());
    let ppi = ratio(sx * 72.0, dw).zip(ratio(sy * 72.0, dh));
    Facts { pixels, ppi, source: ratio(sx, sy), shown }
}

/// What a shape summary needs to report its pictures' facts: the presentation that holds their
/// images and the map from the coordinates of the shape's box to the slide. Passed into groups.
#[derive(Clone, Copy)]
pub(crate) struct Drawn<'a> {
    pres: &'a Presentation,
    to_slide: Affine,
}

impl<'a> Drawn<'a> {
    /// For a shape placed directly on its slide, layout or master.
    pub(crate) fn on_slide(pres: &'a Presentation) -> Self {
        Drawn { pres, to_slide: Affine::IDENTITY }
    }

    /// For the members of a group whose box is `x` and whose child coordinate space is `child`.
    pub(crate) fn inside(self, x: &Xfrm, child: &Xfrm) -> Self {
        Drawn { to_slide: child_space_to_slide(self.to_slide, x, child), ..self }
    }

    /// Facts for a picture with fill `fill` and box `x`.
    pub(crate) fn picture(self, fill: &PictureFill, x: &Xfrm) -> Facts {
        facts(self.pres, fill, x, self.to_slide)
    }
}

/// `shape.inspect`'s `picture` object for `sh` when it is a picture: `x` is its box, and `shapes`
/// the list it was found in, searched into groups for the map from its box to the slide.
pub(crate) fn for_shape(pres: &Presentation, shapes: &[Shape], sh: &Shape, x: &Xfrm) -> Option<Value> {
    let ShapeKind::Picture { fill } = &sh.kind else { return None };
    let to_slide = to_slide_of(shapes, sh.id, Affine::IDENTITY, 0).unwrap_or(Affine::IDENTITY);
    Some(facts(pres, fill, x, to_slide).json())
}

/// The map from the coordinates of shape `id`'s box to the slide, for a shape among `shapes`
/// (whose boxes are drawn through `to_slide`) or inside one of their groups.
fn to_slide_of(shapes: &[Shape], id: ShapeId, to_slide: Affine, depth: usize) -> Option<Affine> {
    if depth > MAX_DEPTH {
        return None;
    }
    for sh in shapes {
        if sh.id == id {
            return Some(to_slide);
        }
        if let ShapeKind::Group { children, child } = &sh.kind
            && let Some(found) = to_slide_of(children, id, child_space_to_slide(to_slide, &sh.xfrm.unwrap_or_default(), child), depth + 1)
        {
            return Some(found);
        }
    }
    None
}

/// A picture found on a slide: the shape, its fill, its box and the map from the coordinates of
/// that box to the slide.
type Placed<'a> = (&'a Shape, &'a PictureFill, Xfrm, Affine);

/// Every picture among `shapes` and inside their groups, with its fill, its box and the map from
/// the coordinates of that box to the slide (`to_slide` for `shapes` themselves). `ctx` gives
/// top-level placeholders their inherited boxes, as `slide.inspect` reports them.
fn collect<'a>(ctx: Option<&Ctx>, shapes: &'a [Shape], to_slide: Affine, depth: usize, out: &mut Vec<Placed<'a>>) {
    if depth > MAX_DEPTH {
        return;
    }
    for sh in shapes {
        let x = ctx.map(|c| resolve::xfrm(c, sh)).or(sh.xfrm).unwrap_or_default();
        match &sh.kind {
            ShapeKind::Picture { fill } => out.push((sh, fill, x, to_slide)),
            ShapeKind::Group { children, child } => collect(None, children, child_space_to_slide(to_slide, &x, child), depth + 1, out),
            _ => {}
        }
    }
}

/// A threshold `key` from `p`: `default` when it is absent or null, else a number from 0 to
/// `max`; anything else (a string, a negative or larger number) is refused with the key named.
fn threshold(p: &Value, key: &str, default: f64, max: f64) -> Result<f64> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|n| (0.0..=max).contains(n))
            .ok_or_else(|| bad("review.pictures", format!("`{key}` must be a number from 0 to {max}, not {v}"))),
    }
}

fn check(s: &mut Session, p: &Value) -> Result<Value> {
    let min_ppi = threshold(p, "minPpi", MIN_PPI, MIN_PPI_LIMIT)?;
    let max_distortion = threshold(p, "maxDistortion", MAX_DISTORTION, MAX_DISTORTION_LIMIT)?;
    let st = s.doc()?;
    let mut out = vec![];
    for (i, sl) in st.doc.slides.iter().enumerate() {
        let ctx = Ctx::for_slide(&st.doc, sl);
        let mut pictures = vec![];
        collect(ctx.as_ref(), &sl.shapes, Affine::IDENTITY, 0, &mut pictures);
        for (sh, fill, x, to_slide) in pictures {
            let f = facts(&st.doc, fill, &x, to_slide);
            out.extend(findings(i, sh, &x, f, min_ppi, max_distortion));
        }
    }
    Ok(Value::Array(out))
}

/// `review.pictures` entries for one picture on slide `slide`: `stretched` when its distortion is
/// over `max_distortion`, `lowResolution` when its ppi across or down is under `min_ppi`, one of
/// each when both hold (they are fixed apart: a new box, a bigger image). Both are compared as
/// reported, rounded, so a picture listed with distortion 0 is never stretched.
fn findings(slide: usize, sh: &Shape, x: &Xfrm, f: Facts, min_ppi: f64, max_distortion: f64) -> Vec<Value> {
    let distortion = f.distortion().map(|d| round(d, 3));
    let ppi = f.ppi.map(|(across, down)| [round(across, 2), round(down, 2)]);
    let mut kinds = vec![];
    if distortion.is_some_and(|d| d > max_distortion) {
        kinds.push("stretched");
    }
    if ppi.is_some_and(|[across, down]| across.min(down) < min_ppi) {
        kinds.push("lowResolution");
    }
    let fit = f.fit_rect(x).map(|r| [round(r.x, 2), round(r.y, 2), round(r.w, 2), round(r.h, 2)]);
    kinds
        .into_iter()
        .map(|kind| {
            json!({
                "slide": slide,
                "shape": sh.id,
                "name": sh.name,
                "kind": kind,
                "ppi": ppi,
                "distortion": distortion,
                "fitRect": fit,
            })
        })
        .collect()
}

/// How `insert.picture` fits a picture to its box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Fit {
    /// Fill the box, stretching the picture to it.
    Stretch,
    /// Shrink the box to the picture's aspect, centred in the box asked for.
    Contain,
    /// Keep the box and crop the picture to the box's aspect, keeping its middle.
    Cover,
}

impl Fit {
    /// `fit` from `p`: `None` when it is absent or null. Anything but stretch, contain or cover is
    /// refused with `fit` named, and so are contain and cover for a picture whose header states no
    /// pixel size (SVG), since they keep an aspect the picture doesn't give.
    pub(crate) fn param(p: &Value, cmd: &str, bytes: &[u8]) -> Result<Option<Fit>> {
        let fit = match p.get("fit") {
            None | Some(Value::Null) => return Ok(None),
            Some(v) => match v.as_str() {
                Some("stretch") => Fit::Stretch,
                Some("contain") => Fit::Contain,
                Some("cover") => Fit::Cover,
                _ => return Err(bad(cmd, format!("`fit` must be stretch, contain or cover, not {v}"))),
            },
        };
        if fit != Fit::Stretch && pixel_size(bytes).is_none() {
            return Err(bad(
                cmd,
                "`fit` contain and cover keep the picture's aspect, and this picture states no pixel size (SVG has none); use stretch",
            ));
        }
        Ok(Some(fit))
    }

    /// The box and crop of a picture of aspect `aspect` (width over height) fitted to `frame`.
    pub(crate) fn apply(self, frame: Xfrm, aspect: f64) -> (Xfrm, [f64; 4]) {
        match self {
            Fit::Stretch => (frame, [0.0; 4]),
            Fit::Contain => (contain(&frame, aspect), [0.0; 4]),
            Fit::Cover => (frame, cover_crop(aspect, frame.w / frame.h.max(1e-9))),
        }
    }
}

/// The largest box of aspect `aspect` (width over height) inside `x`, centred on it; `x` itself
/// when the aspects already match or either can't be used.
pub(crate) fn contain(x: &Xfrm, aspect: f64) -> Xfrm {
    let (Some(have), Some(want)) = (ratio(x.w, x.h), positive(aspect)) else { return *x };
    if (want / have - 1.0).abs() < 1e-9 {
        return *x;
    }
    let (w, h) = if want > have { (x.w, x.w / want) } else { (x.h * want, x.h) };
    Xfrm { x: x.x + (x.w - w) / 2.0, y: x.y + (x.h - h) / 2.0, w, h, ..*x }
}

/// The crop `[l, t, r, b]` that fills a box of aspect `frame` with the middle of an image of
/// aspect `image` (both width over height) without stretching it: the sides that overhang are cut
/// away, equally on both. A picture put into a placeholder without a box gets this crop.
pub(crate) fn cover_crop(image: f64, frame: f64) -> [f64; 4] {
    let crop = if image > frame {
        let c = (1.0 - frame / image) / 2.0;
        [c, 0.0, c, 0.0]
    } else {
        let c = (1.0 - image / frame) / 2.0;
        [0.0, c, 0.0, c]
    };
    if crop.iter().all(|v| v.is_finite()) { crop } else { [0.0; 4] }
}
