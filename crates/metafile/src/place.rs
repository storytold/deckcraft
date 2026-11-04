//! Placing a picture on a target (a page or a device): the fitted transform, paths as kurbo paths in
//! target units and bitmaps as target rectangles. The rasteriser and the PDF writer only paint the result.

use std::sync::Arc;

use kurbo::{Affine, BezPath, Rect, Shape};

use crate::{Item, Picture, Seg};

/// Largest bitmap side the renderers draw: within the PDF image limit and the rasteriser's 16-bit sides.
pub const MAX_BITMAP_SIDE: u32 = 30_000;
/// Placed coordinates past this (target units) are far off any page; such items are left out.
const MAX_COORD: f64 = 1e6;
/// Smallest placed bitmap side (target units), so the stretch onto it is never singular.
const MIN_BITMAP_SIDE: f64 = 1e-6;
/// Maximum aggregate path elements retained in placed output.
const MAX_PLACED_ELEMENTS: usize = 500_000;

/// One item of a placed picture, in target units.
pub enum PlacedItem {
    /// A filled and/or stroked path. The stroke width is in target units; 0 is a cosmetic hairline that
    /// the renderer draws at its minimum width.
    Path { path: BezPath, fill: Option<[u8; 4]>, stroke: Option<([u8; 4], f64)>, even_odd: bool, region: Option<Arc<BezPath>> },
    /// `Picture::items[index]`, a bitmap, stretched into `rect`.
    Bitmap { index: usize, rect: Rect, region: Option<Arc<BezPath>> },
    /// `Picture::items[index]`, a pattern fill: `path` (target units) filled with the item's tile
    /// repeated; `tile` maps tile pixels to target units.
    Pattern { index: usize, path: BezPath, even_odd: bool, tile: Affine, region: Option<Arc<BezPath>> },
    /// `Picture::items[index]`, a text string: `transform` maps its text space to target units;
    /// `clip` (target units) bounds what it draws. `region` is the device-context clip.
    Text { index: usize, transform: Affine, clip: Option<Rect>, region: Option<Arc<BezPath>> },
}

impl Picture {
    /// Whether the picture has a finite, positive size, so it can be fitted into a frame.
    pub fn has_size(&self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width > 0.0 && self.height > 0.0
    }

    /// The transform that fits the picture into `r` with the crop fractions `crop` (left, top, right,
    /// bottom) removed first: the visible part fills `r`. None for a degenerate size.
    pub fn fit(&self, r: Rect, crop: [f32; 4]) -> Option<Affine> {
        let [cl, ct, cr, cb] = crop.map(|v| if v.is_finite() { f64::from(v.clamp(0.0, 0.95)) } else { 0.0 });
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let sx = r.width() / (w * (1.0 - cl - cr).max(0.05));
        let sy = r.height() / (h * (1.0 - ct - cb).max(0.05));
        (sx.is_finite() && sy.is_finite() && sx > 0.0 && sy > 0.0)
            .then(|| Affine::translate((r.x0 - cl * w * sx, r.y0 - ct * h * sy)) * Affine::scale_non_uniform(sx, sy))
    }

    /// The items placed into `r` with `crop` applied. Items that are not finite or reach past ±1e6
    /// target units are left out (they are far off any page), as are bitmaps with no area. Stroke
    /// widths are scaled to target units (along the mean axis scale, so a non-uniform fit keeps
    /// strokes even).
    pub fn place(&self, r: Rect, crop: [f32; 4]) -> Vec<PlacedItem> {
        let Some(tf) = self.fit(r, crop) else { return Vec::new() };
        let c = tf.as_coeffs();
        let k = (c[0].abs() + c[3].abs()) / 2.0;
        let mut out = Vec::with_capacity(self.items.len());
        let mut total_elements = 0usize;
        let mut cached_region: Option<(*const [f32; 4], usize, Option<Option<Arc<BezPath>>>)> = None;
        let mut get_region = |reg: &Option<Vec<[f32; 4]>>| -> Option<Option<Arc<BezPath>>> {
            let (ptr, len) = match reg {
                None => (std::ptr::null(), 0),
                Some(v) => (v.as_ptr(), v.len()),
            };
            if let Some((c_ptr, c_len, ref res)) = cached_region
                && c_ptr == ptr
                && c_len == len
            {
                return res.clone();
            }
            let res = placed_region(&tf, reg);
            cached_region = Some((ptr, len, res.clone()));
            res
        };

        for (index, item) in self.items.iter().enumerate() {
            if total_elements >= MAX_PLACED_ELEMENTS {
                break;
            }
            match item {
                Item::Path { segs, fill, stroke, even_odd, region } => {
                    let path = tf * bez(segs);
                    let Some(region) = get_region(region) else { continue };
                    if in_range(path.bounding_box()) {
                        total_elements = total_elements.saturating_add(path.elements().len());
                        let stroke = stroke.map(|(c, w)| (c, f64::from(w) * k));
                        out.push(PlacedItem::Path { path, fill: *fill, stroke, even_odd: *even_odd, region });
                    }
                }
                Item::Pattern { segs, even_odd, tile: t, region } => {
                    let path = tf * bez(segs);
                    let Some(region) = get_region(region) else { continue };
                    let tile = tf
                        * Affine::translate((f64::from(t.origin[0]), f64::from(t.origin[1])))
                        * Affine::scale_non_uniform(f64::from(t.cell[0]), f64::from(t.cell[1]));
                    let det = tile.determinant();
                    if in_range(path.bounding_box()) && det.is_finite() && det.abs() > 1e-12 {
                        total_elements = total_elements.saturating_add(path.elements().len());
                        out.push(PlacedItem::Pattern { index, path, even_odd: *even_odd, tile, region });
                    }
                }
                Item::Text(t) => {
                    let transform = tf * Affine::new(t.transform);
                    let Some(region) = get_region(&t.region) else { continue };
                    let clip =
                        t.clip.map(|[x0, y0, x1, y1]| tf.transform_rect_bbox(Rect::new(f64::from(x0), f64::from(y0), f64::from(x1), f64::from(y1))));
                    let c = transform.as_coeffs();
                    let origin = Rect::new(c[4], c[5], c[4], c[5]);
                    if in_range(origin) && c.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) && clip.is_none_or(in_range) {
                        out.push(PlacedItem::Text { index, transform, clip, region });
                    }
                }
                Item::Bitmap { rect: [x, y, w, h], region, .. } => {
                    let (x0, y0) = (f64::from(*x), f64::from(*y));
                    let rect = tf.transform_rect_bbox(Rect::new(x0, y0, x0 + f64::from(*w), y0 + f64::from(*h)));
                    let Some(region) = get_region(region) else { continue };
                    if in_range(rect) && rect.width() > MIN_BITMAP_SIDE && rect.height() > MIN_BITMAP_SIDE {
                        out.push(PlacedItem::Bitmap { index, rect, region });
                    }
                }
            }
        }
        out
    }
}

impl Item {
    /// Takes a bitmap's pixels out of the item (its geometry stays) and returns them as (width, height,
    /// RGBA) when a renderer can draw them: both sides in 1..=[`MAX_BITMAP_SIDE`] and exactly that many
    /// pixels. None for paths and for bitmaps that cannot be drawn.
    pub fn take_pixels(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        let Item::Bitmap { width, height, rgba, .. } = self else { return None };
        let (w, h, px) = (*width, *height, std::mem::take(rgba));
        let sides = (1..=MAX_BITMAP_SIDE).contains(&w) && (1..=MAX_BITMAP_SIDE).contains(&h);
        (sides && px.len() == (w as usize).saturating_mul(h as usize).saturating_mul(4)).then_some((w, h, px))
    }
}

/// The device-context clip in target units. `Some(None)` is the default clip. `None` means the
/// region does not meet the page, so the item draws nothing.
fn placed_region(tf: &Affine, region: &Option<Vec<[f32; 4]>>) -> Option<Option<Arc<BezPath>>> {
    let Some(rects) = region else { return Some(None) };
    let mut path = BezPath::new();
    for [x0, y0, x1, y1] in rects {
        let r = tf.transform_rect_bbox(Rect::new(f64::from(*x0), f64::from(*y0), f64::from(*x1), f64::from(*y1)));
        if in_range(r) && r.width() > 1e-9 && r.height() > 1e-9 {
            path.extend(r.to_path(0.1));
        }
    }
    (!path.is_empty()).then_some(Some(Arc::new(path)))
}

/// Segments as a kurbo path. A segment after a close opens a new subpath at the last start point.
fn bez(segs: &[Seg]) -> BezPath {
    let mut p = BezPath::new();
    let mut start = (0.0, 0.0);
    let mut open = false;
    for s in segs {
        match *s {
            Seg::Move(x, y) => {
                start = (f64::from(x), f64::from(y));
                p.move_to(start);
                open = true;
            }
            Seg::Line(x, y) => {
                if !open {
                    p.move_to(start);
                }
                p.line_to((f64::from(x), f64::from(y)));
                open = true;
            }
            Seg::Cubic(a, b, c, d, e, f) => {
                if !open {
                    p.move_to(start);
                }
                p.curve_to((f64::from(a), f64::from(b)), (f64::from(c), f64::from(d)), (f64::from(e), f64::from(f)));
                open = true;
            }
            Seg::Close => {
                if open {
                    p.close_path();
                }
                open = false;
            }
        }
    }
    p
}

/// Whether a placed box is finite and within ±[`MAX_COORD`].
fn in_range(b: Rect) -> bool {
    [b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite() && v.abs() < MAX_COORD)
}
