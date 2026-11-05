//! EMF+ (MS-EMFPLUS): GDI+ drawing records carried in EMF comment records.
//!
//! Most EMF+ files are *dual*: the same picture is also drawn with plain EMF records, which
//! [`crate::emf`] already plays, so this reader ignores their EMF+ part. An *EMF+ only* file has
//! nothing else to draw; for those this reader plays the records:
//! - brushes (solid, hatch, texture, linear and path gradients), pens, paths, images, fonts,
//!   string formats and regions, including objects split across several records;
//! - rectangles, polygons, lines, ellipses, paths and Béziers;
//! - bitmaps (raw pixel formats, and PNG/JPEG/GIF/TIFF/BMP payloads) and strings (DrawString,
//!   DrawDriverString with character codes);
//! - world and page transforms, save/restore and containers;
//! - arcs and pies;
//! - clipping to rectangles and convex clip paths and regions (others: their bounding rectangle);
//! - nested metafile images, and glyph-index driver strings in the Windows core fonts;
//! - cardinal splines (DrawCurve, DrawClosedCurve, FillClosedCurve), FillRegion, and Clear
//!   (over the picture frame).
//!
//! Every drawing record of MS-EMFPLUS §2.3.4 is played. Not drawn: image attributes, glyph
//! indices of other fonts, and region combinations other than intersect and union (XOR, exclude
//! and complement keep the left area). Deliberately ignored: rendering hints (antialias, text
//! hint, interpolation, pixel offset, compositing; the renderer has its own), SetRenderingOrigin
//! (hatch phase), SerializableObject image effects, the terminal-server records SetTSGraphics and
//! SetTSClip, and StrokeFillPath (0x4037), which the specification lists but gives no layout for
//! and GDI+ doesn't write.

use std::sync::Arc;

use crate::bytes::Bytes;
use crate::canvas::Raw;
use crate::canvas::{BS_NULL, BS_SOLID, Brush, Canvas, LogFont, PS_NULL, Pen, RawTile, Xform, hatch_tile, mul};
use crate::cliprect::{clip_polygon, convex, flatten, ops_bbox, pts_bbox, rect_poly};
use crate::dib::Rgba;
use crate::geom::{Op, P};
use crate::text::{ETO_CLIPPED, Font};
use crate::{Item, Picture, Seg};

/// "EMF+" as the comment identifier of an EMR_COMMENT holding EMF+ records.
pub(crate) const EMF_PLUS: u32 = 0x2B46_4D45;
const IDENTITY: Xform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
/// GDI+ allows 64 objects; ids are a byte.
const MAX_OBJECTS: usize = 64;
const MAX_STACK: usize = 64;
/// Points per path or point list read, as the plain EMF reader caps paths.
const MAX_POINTS: usize = 100_000;
/// Gradient stops read from a brush.
const MAX_STOPS: usize = 256;
/// Characters per string.
const MAX_CHARS: usize = 32_768;
/// Bytes gathered for one object split across records.
const MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;
/// Nesting depth of region nodes.
const MAX_REGION_DEPTH: usize = 32;
/// Region nodes and path points parsed together, including nodes in sibling branches.
const MAX_REGION_WORK: usize = MAX_POINTS;

const TA_RIGHT: u32 = 2;
const TA_CENTER: u32 = 6;
const TA_BOTTOM: u32 = 8;
const TA_BASELINE: u32 = 24;

/// What a brush paints. Coordinates are in brush space, which `xf` maps to world space.
#[derive(Clone)]
enum Fill {
    Solid([u8; 4]),
    Hatch { style: u32, fg: [u8; 4], bg: [u8; 4] },
    Linear { rect: [f64; 4], stops: Vec<(f64, [u8; 4])>, wrap: u32, xf: Xform },
    Radial { center: P, radius: P, stops: Vec<(f64, [u8; 4])>, xf: Xform },
    Texture { img: Arc<Rgba>, wrap: u32, xf: Xform },
}

impl Fill {
    /// One colour standing for the brush (pens and text draw solid).
    fn color(&self) -> [u8; 4] {
        match self {
            Fill::Solid(c) | Fill::Hatch { fg: c, .. } => *c,
            Fill::Linear { stops, .. } | Fill::Radial { stops, .. } => ramp(stops, 0.5),
            Fill::Texture { img, .. } => img.px.get(..4).and_then(|c| c.try_into().ok()).unwrap_or([0, 0, 0, 255]),
        }
    }

    /// The colour at a brush-space point.
    fn sample(&self, p: P) -> [u8; 4] {
        match self {
            Fill::Solid(c) | Fill::Hatch { fg: c, .. } => *c,
            Fill::Linear { rect, stops, wrap, .. } => {
                let t = if rect[2] == 0.0 { 0.0 } else { (p.0 - rect[0]) / rect[2] };
                ramp(stops, wrap_t(t, *wrap))
            }
            Fill::Radial { center, radius, stops, .. } => {
                let (dx, dy) = ((p.0 - center.0) / radius.0.max(1e-9), (p.1 - center.1) / radius.1.max(1e-9));
                ramp(stops, dx.hypot(dy).min(1.0))
            }
            Fill::Texture { img, wrap, .. } => {
                let (w, h) = (img.w as f64, img.h as f64);
                if w == 0.0 || h == 0.0 {
                    return [0, 0, 0, 0];
                }
                if *wrap == 4 && !((0.0..w).contains(&p.0) && (0.0..h).contains(&p.1)) {
                    return [0, 0, 0, 0];
                }
                let x = (p.0.rem_euclid(w) as usize).min(img.w - 1);
                let y = (p.1.rem_euclid(h) as usize).min(img.h - 1);
                let i = (y * img.w + x) * 4;
                img.px.get(i..i + 4).and_then(|c| c.try_into().ok()).unwrap_or([0, 0, 0, 0])
            }
        }
    }

    fn xf(&self) -> Xform {
        match self {
            Fill::Linear { xf, .. } | Fill::Radial { xf, .. } | Fill::Texture { xf, .. } => *xf,
            _ => IDENTITY,
        }
    }
}

/// Where `t` lands under a wrap mode: tiled, mirrored (flip modes) or clamped.
fn wrap_t(t: f64, wrap: u32) -> f64 {
    if !t.is_finite() {
        return 0.0;
    }
    match wrap {
        1 | 3 => {
            let m = t.rem_euclid(2.0);
            if m > 1.0 { 2.0 - m } else { m }
        }
        4 => t.clamp(0.0, 1.0),
        _ => t.rem_euclid(1.0),
    }
}

/// The colour at `t` along sorted stops.
fn ramp(stops: &[(f64, [u8; 4])], t: f64) -> [u8; 4] {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else { return [0, 0, 0, 0] };
    if t <= first.0 {
        return first.1;
    }
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t <= b {
            let k = if b > a { (t - a) / (b - a) } else { 0.0 };
            return lerp(ca, cb, k);
        }
    }
    last.1
}

fn lerp(a: [u8; 4], b: [u8; 4], k: f64) -> [u8; 4] {
    let mut c = [0u8; 4];
    for i in 0..4 {
        c[i] = (f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * k).round().clamp(0.0, 255.0) as u8;
    }
    c
}

#[derive(Clone)]
struct FontObj {
    em: f64,
    unit: u32,
    style: u32,
    family: String,
}

#[derive(Clone, Copy, Debug, Default)]
struct Format {
    flags: u32,
    align: u32,
    line_align: u32,
}

/// A clip area: a convex polygon (empty clips everything), or None for infinite.
type Area = Option<Vec<P>>;

enum ImageData {
    Bitmap(Rgba),
    /// A nested WMF or EMF (EMF+) picture.
    Meta(Picture),
}

thread_local! {
    /// Nesting depth of metafile images being parsed, so a file of nested metafiles cannot run
    /// the stack out.
    static NESTING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// Metafile images inside metafile images, at most.
const MAX_NESTING: usize = 3;

#[derive(Clone)]
enum Obj {
    Brush(Fill),
    Pen {
        color: [u8; 4],
        width: f64,
    },
    Path(Vec<Op>),
    Image(Arc<ImageData>),
    Font(FontObj),
    Format(Format),
    /// A region's bounding rectangle `[l, t, r, b]` in world units; None is infinite.
    Region(Area),
}

/// An object arriving in several records: its id, type, total size and the bytes so far.
struct Partial {
    id: usize,
    kind: u16,
    total: usize,
    data: Vec<u8>,
}

struct Saved {
    id: u32,
    world: Xform,
    page: f64,
    clip: Area,
}

pub(crate) struct Plus {
    /// An EMF+ header said the file is EMF+ only, so these records are the picture.
    active: bool,
    dpi: f64,
    world: Xform,
    page: f64,
    /// Convex device clip polygon; None is unclipped.
    clip: Area,
    objects: Vec<Option<Obj>>,
    object_bytes: usize,
    stack: Vec<Saved>,
    partial: Option<Partial>,
    /// The picture frame in device units (`[x, y, w, h]`), what Clear paints.
    pub frame: Option<[f64; 4]>,
}

impl Plus {
    pub fn new() -> Self {
        Plus {
            active: false,
            dpi: 96.0,
            world: IDENTITY,
            page: 1.0,
            clip: None,
            objects: vec![None; MAX_OBJECTS],
            object_bytes: 0,
            stack: Vec::new(),
            partial: None,
            frame: None,
        }
    }

    /// Plays the EMF+ records of one EMR_COMMENT payload (after the identifier).
    pub fn comment(&mut self, cv: &mut Canvas, data: Bytes) {
        let mut pos = 0usize;
        while !cv.full() {
            let Some(tail) = data.0.get(pos..) else { break };
            let tail = Bytes(tail);
            let (Some(typ), Some(flags), Some(size), Some(dsize)) = (tail.u16(0), tail.u16(2), tail.u32(4), tail.u32(8)) else {
                break;
            };
            let (size, dsize) = (size as usize, dsize as usize);
            if size < 12 || dsize > size - 12 {
                break;
            }
            let Some(rec) = tail.slice(12, dsize) else { break };
            let _ = self.record(cv, typ, flags, Bytes(rec));
            let Some(next) = pos.checked_add(size) else { break };
            pos = next;
        }
    }

    /// World units to device pixels before the canvas's own mapping.
    fn xf(&self) -> Xform {
        mul(self.world, [self.page, 0.0, 0.0, self.page, 0.0, 0.0])
    }

    fn painter(&self) -> Painter {
        Painter { xf: self.xf(), clip: self.clip.clone() }
    }

    fn record(&mut self, cv: &mut Canvas, typ: u16, flags: u16, r: Bytes) -> Option<()> {
        if typ == 0x4001 {
            // Header: dual files also draw with EMF records, which win.
            self.active = flags & 1 == 0;
            if let Some(d) = r.u32(8).map(f64::from).filter(|d| *d > 0.0 && *d < 100_000.0) {
                self.dpi = d;
            }
            return Some(());
        }
        if !self.active {
            return None;
        }
        match typ {
            0x4008 => self.object(flags, r),
            0x4009 | 0x4013 | 0x401A..=0x401C | 0x4036 => self.record_image_text_region(cv, typ, flags, r),
            0x4025..=0x4035 => self.record_state(cv, typ, flags, r),
            0x4010..=0x4012 | 0x4016..=0x4019 => self.record_curve(cv, typ, flags, r),
            _ => self.record_shape(cv, typ, flags, r),
        }
    }

    /// Rectangle, polygon, ellipse and path records.
    fn record_shape(&mut self, cv: &mut Canvas, typ: u16, flags: u16, r: Bytes) -> Option<()> {
        let compressed = flags & 0x4000 != 0;
        let id = usize::from(flags & 0xFF);
        match typ {
            0x400A => {
                let fill = self.brush_of(flags, r.u32(0)?)?;
                let p = self.painter();
                for rc in rects(r, 8, r.u32(4)? as usize, compressed)? {
                    if cv.full() {
                        break;
                    }
                    p.fill(cv, &rect_ops(rc), &fill);
                }
                Some(())
            }
            0x400B => {
                let pen = self.pen(id)?;
                let p = self.painter();
                for rc in rects(r, 4, r.u32(0)? as usize, compressed)? {
                    if cv.full() {
                        break;
                    }
                    p.stroke(cv, &rect_ops(rc), pen);
                }
                Some(())
            }
            0x400C => {
                let fill = self.brush_of(flags, r.u32(0)?)?;
                let pts = points(r, 8, r.u32(4)? as usize, compressed)?;
                self.painter().fill(cv, &poly_ops(&pts, true), &fill);
                Some(())
            }
            0x400D => {
                let pen = self.pen(id)?;
                let pts = points(r, 4, r.u32(0)? as usize, compressed)?;
                self.painter().stroke(cv, &poly_ops(&pts, flags & 0x2000 != 0), pen);
                Some(())
            }
            0x400E => {
                let fill = self.brush_of(flags, r.u32(0)?)?;
                let rect = *rects(r, 4, 1, compressed)?.first()?;
                self.painter().fill(cv, &ellipse_ops(rect), &fill);
                Some(())
            }
            0x400F => {
                let pen = self.pen(id)?;
                let rect = *rects(r, 0, 1, compressed)?.first()?;
                self.painter().stroke(cv, &ellipse_ops(rect), pen);
                Some(())
            }
            0x4014 => {
                let fill = self.brush_of(flags, r.u32(0)?)?;
                let ops = self.path(id)?;
                self.painter().fill(cv, &ops, &fill);
                Some(())
            }
            0x4015 => {
                let ops = self.path(id)?;
                let pen = self.pen(r.u32(0)? as usize)?;
                self.painter().stroke(cv, &ops, pen);
                Some(())
            }
            _ => None,
        }
    }

    /// Curve, bezier and arc records.
    fn record_curve(&mut self, cv: &mut Canvas, typ: u16, flags: u16, r: Bytes) -> Option<()> {
        let compressed = flags & 0x4000 != 0;
        let id = usize::from(flags & 0xFF);
        match typ {
            0x4019 => {
                let pen = self.pen(id)?;
                let pts = points(r, 4, r.u32(0)? as usize, compressed)?;
                let (first, rest) = pts.split_first()?;
                let mut ops = vec![Op::M(*first)];
                ops.extend(rest.as_chunks::<3>().0.iter().map(|c| Op::C(c[0], c[1], c[2])));
                self.painter().stroke(cv, &ops, pen);
                Some(())
            }
            // Cardinal splines: FillClosedCurve, DrawClosedCurve, DrawCurve.
            0x4016 => {
                let fill = self.brush_of(flags, r.u32(0)?)?;
                let tension = finite(r.f32(4)?)?;
                let pts = points(r, 12, r.u32(8)? as usize, compressed)?;
                self.painter().fill(cv, &curve_ops(&pts, tension, true, 0, usize::MAX), &fill);
                Some(())
            }
            0x4017 => {
                let pen = self.pen(id)?;
                let tension = finite(r.f32(0)?)?;
                let pts = points(r, 8, r.u32(4)? as usize, compressed)?;
                self.painter().stroke(cv, &curve_ops(&pts, tension, true, 0, usize::MAX), pen);
                Some(())
            }
            0x4018 => {
                let pen = self.pen(id)?;
                let tension = finite(r.f32(0)?)?;
                let (offset, segments) = (r.u32(4)? as usize, r.u32(8)? as usize);
                let pts = points(r, 16, r.u32(12)? as usize, compressed)?;
                self.painter().stroke(cv, &curve_ops(&pts, tension, false, offset, segments), pen);
                Some(())
            }
            0x4010 => {
                let fill = self.brush_of(flags, r.u32(0)?)?;
                let [start, sweep] = floats::<2>(r, 4)?;
                let rect = *rects(r, 12, 1, compressed)?.first()?;
                self.painter().fill(cv, &arc_ops(rect, start, sweep, true), &fill);
                Some(())
            }
            0x4011 | 0x4012 => {
                let pen = self.pen(id)?;
                let [start, sweep] = floats::<2>(r, 0)?;
                let rect = *rects(r, 8, 1, compressed)?.first()?;
                self.painter().stroke(cv, &arc_ops(rect, start, sweep, typ == 0x4011), pen);
                Some(())
            }
            _ => None,
        }
    }

    /// Clear, region fill, image and text records.
    fn record_image_text_region(&mut self, cv: &mut Canvas, typ: u16, flags: u16, r: Bytes) -> Option<()> {
        let compressed = flags & 0x4000 != 0;
        let id = usize::from(flags & 0xFF);
        match typ {
            // Clear: the whole picture in one colour, whatever the clip (a transparent clear
            // leaves nothing to draw).
            0x4009 => {
                let c = argb(r.u32(0)?);
                let [x, y, w, h] = self.frame?;
                if c[3] == 0 || !(w > 0.0 && h > 0.0) {
                    return Some(());
                }
                let to_device = invert(mul(cv.world(), cv.device_affine()))?;
                let p = Painter { xf: to_device, clip: None };
                p.fill(cv, &rect_ops([x, y, x + w, y + h]), &Fill::Solid(c));
                Some(())
            }
            // FillRegion: the region's area (as the clip reads it).
            0x4013 => {
                let fill = self.brush_of(flags, r.u32(0)?)?;
                let Some(Some(Obj::Region(Some(area)))) = self.objects.get(id).cloned() else { return None };
                if area.len() < 3 {
                    return None;
                }
                self.painter().fill(cv, &poly_ops(&area, true), &fill);
                Some(())
            }
            0x401A => {
                let img = self.image(id)?;
                let dst = *rects(r, 24, 1, compressed)?.first()?;
                self.draw_image(cv, &img, r, [dst[0], dst[1], dst[2] - dst[0], dst[3] - dst[1]])
            }
            0x401B => {
                let img = self.image(id)?;
                let pts = points(r, 28, 3, compressed)?;
                let (p0, p1, p2) = (*pts.first()?, *pts.get(1)?, *pts.get(2)?);
                // The destination parallelogram's top-left, top-right and bottom-left corners; a
                // sheared or rotated one is drawn into its bounding rectangle's axes.
                let dst = [p0.0, p0.1, p1.0 - p0.0 + (p2.0 - p0.0), p2.1 - p0.1 + (p1.1 - p0.1)];
                self.draw_image(cv, &img, r, dst)
            }
            0x401C => self.draw_string(cv, flags, r),
            0x4036 => self.draw_driver_string(cv, flags, r),
            _ => None,
        }
    }

    /// Save / restore, transform and clip records.
    fn record_state(&mut self, cv: &mut Canvas, typ: u16, flags: u16, r: Bytes) -> Option<()> {
        let id = usize::from(flags & 0xFF);
        match typ {
            0x4025 | 0x4028 => r.u32(0).map(|i| self.push(i)),
            0x4027 => r.u32(24).map(|i| self.push(i)),
            0x4026 | 0x4029 => r.u32(0).map(|i| self.pop(i)),
            0x402A => matrix(r, 0).map(|m| self.world = m),
            0x402B => {
                self.world = IDENTITY;
                Some(())
            }
            0x402C => matrix(r, 0).and_then(|m| self.apply(m, flags)),
            0x402D => self.apply([1.0, 0.0, 0.0, 1.0, f64::from(r.f32(0)?), f64::from(r.f32(4)?)], flags),
            0x402E => self.apply([f64::from(r.f32(0)?), 0.0, 0.0, f64::from(r.f32(4)?), 0.0, 0.0], flags),
            0x402F => {
                let a = f64::from(r.f32(0)?).to_radians();
                self.apply([a.cos(), a.sin(), -a.sin(), a.cos(), 0.0, 0.0], flags)
            }
            0x4030 => {
                let page = f64::from(r.f32(0)?) * unit_px(u32::from(flags & 0xFF), self.dpi);
                if page.is_finite() && page > 0.0 && page < 1.0e6 {
                    self.page = page;
                }
                Some(())
            }
            0x4031 => {
                self.clip = None;
                Some(())
            }
            0x4032 => {
                let [x, y, w, h] = floats::<4>(r, 0)?;
                let area = map_area(Some(rect_poly([x, y, x + w, y + h])), mul(self.xf(), cv.device_affine()));
                self.combine(flags, area)
            }
            0x4033 => {
                let area = map_area(Some(area_of_ops(&self.path(id)?)), mul(self.xf(), cv.device_affine()));
                self.combine(flags, area)
            }
            0x4034 => {
                let Some(Some(Obj::Region(reg))) = self.objects.get(id).cloned() else { return None };
                let area = map_area(reg, mul(self.xf(), cv.device_affine()));
                self.combine(flags, area)
            }
            0x4035 => {
                let [dx, dy] = floats::<2>(r, 0)?;
                let a = mul(self.xf(), cv.device_affine());
                let (ox, oy) = (dx * a[0] + dy * a[2], dx * a[1] + dy * a[3]);
                if let Some(c) = self.clip.as_mut() {
                    for p in c.iter_mut() {
                        *p = (p.0 + ox, p.1 + oy);
                    }
                }
                Some(())
            }
            _ => None,
        }
    }

    /// Without the `A` flag a transform is applied before the world transform (prepended).
    fn apply(&mut self, m: Xform, flags: u16) -> Option<()> {
        self.world = if flags & 0x2000 != 0 { mul(self.world, m) } else { mul(m, self.world) };
        Some(())
    }

    fn push(&mut self, id: u32) {
        if self.stack.len() < MAX_STACK {
            self.stack.push(Saved { id, world: self.world, page: self.page, clip: self.clip.clone() });
        }
    }

    fn pop(&mut self, id: u32) {
        if let Some(i) = self.stack.iter().rposition(|s| s.id == id) {
            let s = self.stack.swap_remove(i);
            (self.world, self.page, self.clip) = (s.world, s.page, s.clip);
            self.stack.truncate(i);
        }
    }

    /// Combines a new clip area with the current one under the record's combine mode: replace,
    /// intersect and union (as the rectangle around both). XOR, exclude and complement leave the
    /// clip as it was.
    fn combine(&mut self, flags: u16, new: Area) -> Option<()> {
        self.clip = match (flags >> 8) & 0xF {
            0 => new,
            1 => area_intersect(self.clip.take(), new),
            2 => area_union(self.clip.take(), new),
            _ => self.clip.take(),
        };
        Some(())
    }

    fn object(&mut self, flags: u16, r: Bytes) -> Option<()> {
        let id = usize::from(flags & 0xFF);
        let kind = (flags >> 8) & 0x7F;
        if flags & 0x8000 != 0 {
            // A continued object: the total size, then this record's share of the bytes.
            let total = (r.u32(0)? as usize).min(MAX_OBJECT_BYTES);
            let chunk = r.tail(4)?.0;
            let p = match self.partial.take() {
                Some(p) if p.id == id && p.kind == kind && p.total == total => p,
                _ => Partial { id, kind, total, data: Vec::new() },
            };
            let mut p = p;
            let room = p.total.saturating_sub(p.data.len());
            p.data.extend_from_slice(&chunk[..chunk.len().min(room)]);
            if p.data.len() >= p.total {
                let obj = parse_object(p.kind, Bytes(&p.data));
                self.store(p.id, obj);
            } else {
                self.partial = Some(p);
            }
            return Some(());
        }
        // The last piece of a continued object comes without the continue flag.
        if let Some(mut p) = self.partial.take()
            && p.id == id
            && p.kind == kind
        {
            let room = p.total.saturating_sub(p.data.len());
            p.data.extend_from_slice(&r.0[..r.0.len().min(room)]);
            let obj = parse_object(kind, Bytes(&p.data));
            self.store(id, obj);
            return Some(());
        }
        let obj = parse_object(kind, r);
        self.store(id, obj);
        Some(())
    }

    fn store(&mut self, id: usize, obj: Option<Obj>) {
        let Some(slot) = self.objects.get_mut(id) else { return };
        if let Some(old) = slot.take() {
            self.object_bytes = self.object_bytes.saturating_sub(Self::obj_cost(&old));
        }
        if let Some(new_obj) = obj {
            let cost = Self::obj_cost(&new_obj);
            if self.object_bytes.saturating_add(cost) <= crate::MAX_BYTES {
                self.object_bytes += cost;
                *slot = Some(new_obj);
            }
        }
    }

    fn obj_cost(obj: &Obj) -> usize {
        match obj {
            Obj::Image(img) => match img.as_ref() {
                ImageData::Bitmap(rgba) => rgba.px.len(),
                ImageData::Meta(pic) => pic.items.len().saturating_mul(128),
            },
            Obj::Brush(Fill::Texture { img, .. }) => img.px.len(),
            Obj::Path(ops) => ops.len().saturating_mul(std::mem::size_of::<Op>()),
            _ => 0,
        }
    }

    fn path(&self, id: usize) -> Option<Vec<Op>> {
        match self.objects.get(id)? {
            Some(Obj::Path(ops)) => Some(ops.clone()),
            _ => None,
        }
    }

    fn pen(&self, id: usize) -> Option<([u8; 4], f64)> {
        match self.objects.get(id)? {
            Some(Obj::Pen { color, width }) => Some((*color, *width)),
            _ => None,
        }
    }

    fn image(&self, id: usize) -> Option<Arc<ImageData>> {
        match self.objects.get(id)? {
            Some(Obj::Image(img)) => Some(img.clone()),
            _ => None,
        }
    }

    /// The brush of a fill record: an ARGB value with the `S` flag, else a brush object.
    fn brush_of(&self, flags: u16, v: u32) -> Option<Fill> {
        if flags & 0x8000 != 0 {
            return Some(Fill::Solid(argb(v)));
        }
        match self.objects.get(v as usize)? {
            Some(Obj::Brush(f)) => Some(f.clone()),
            _ => None,
        }
    }

    /// DrawImage and DrawImagePoints: the source rectangle (pixels) cut from the image, drawn
    /// into world rectangle `dst` = `[x, y, w, h]`.
    fn draw_image(&self, cv: &mut Canvas, img: &ImageData, r: Bytes, dst: [f64; 4]) -> Option<()> {
        let img = match img {
            ImageData::Bitmap(b) => b,
            ImageData::Meta(pic) => {
                self.painter().picture(cv, dst, pic);
                return Some(());
            }
        };
        let [sx, sy, sw, sh] = floats::<4>(r, 8)?;
        let src = if r.u32(4)? == 2 { [sx, sy, sw, sh].map(|v| v.round().clamp(-1.0e9, 1.0e9) as i64) } else { [0; 4] };
        let (part, dst) = crate::dib::clip_blit(img, src, dst)?;
        self.painter().image(cv, dst, part);
        Some(())
    }

    fn font(&self, id: usize) -> Option<FontObj> {
        match self.objects.get(id)? {
            Some(Obj::Font(f)) => Some(f.clone()),
            _ => None,
        }
    }

    /// The font as a LOGFONT whose height is in world units.
    fn log_font(&self, f: &FontObj) -> LogFont {
        let em = if f.unit == 0 { f.em } else { f.em * unit_px(f.unit, self.dpi) / self.page };
        let font = Font {
            family: f.family.clone(),
            height: -(em.clamp(0.0, 1.0e6) as f32),
            weight: if f.style & 1 != 0 { 700 } else { 400 },
            italic: f.style & 2 != 0,
            underline: f.style & 4 != 0,
            strike: f.style & 8 != 0,
            ..Font::default()
        };
        LogFont { font, escapement: 0.0 }
    }

    /// DrawString: lines of text laid out in a rectangle by the string format's alignments.
    fn draw_string(&self, cv: &mut Canvas, flags: u16, r: Bytes) -> Option<()> {
        let font = self.font(usize::from(flags & 0xFF))?;
        let color = self.brush_of(flags, r.u32(0)?)?.color();
        let fmt = match self.objects.get(r.u32(4)? as usize) {
            Some(Some(Obj::Format(f))) => *f,
            _ => Format::default(),
        };
        let len = (r.u32(8)? as usize).min(MAX_CHARS);
        let [x, y, w, h] = floats::<4>(r, 12)?;
        let units: Vec<u16> = (0..len).map(|i| r.u16(28 + i * 2)).collect::<Option<_>>()?;
        let lf = self.log_font(&font);
        let em = f64::from(-lf.font.height);
        let wrap = w > 0.0 && fmt.flags & 0x1000 == 0;
        let lines: Vec<Vec<u16>> = units
            .split(|u| *u == 0x0A)
            .flat_map(|l| {
                let l: Vec<u16> = l.iter().copied().filter(|u| *u != 0x0D).collect();
                if wrap { wrap_line(&l, w, em) } else { vec![l] }
            })
            .collect();
        let line_h = em * 1.2;
        let (ax, ta_h) = match fmt.align {
            1 => (x + w / 2.0, TA_CENTER),
            2 => (x + w, TA_RIGHT),
            _ => (x, 0),
        };
        let block = line_h * lines.len() as f64;
        let top = match fmt.line_align {
            1 => y + (h - block) / 2.0,
            2 => y + h - block,
            _ => y,
        };
        let clip = (w > 0.0 && h > 0.0 && fmt.flags & 0x4000 == 0).then_some([x, y, x + w, y + h]);
        let p = self.painter();
        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let (text, dx) = crate::text::decode_utf16(line, &[]);
            let at = (ax, top + line_h * i as f64);
            let (at, ta) = if fmt.line_align == 2 && lines.len() == 1 { ((ax, y + h), ta_h | TA_BOTTOM) } else { (at, ta_h) };
            p.text(cv, at, text, dx, TextStyle { color, font: lf.clone(), align: ta, clip }, IDENTITY);
        }
        Some(())
    }

    /// DrawDriverString with character codes (CmapLookup): positioned glyphs on a baseline.
    fn draw_driver_string(&self, cv: &mut Canvas, flags: u16, r: Bytes) -> Option<()> {
        let font = self.font(usize::from(flags & 0xFF))?;
        let color = self.brush_of(flags, r.u32(0)?)?.color();
        let opts = r.u32(4)?;
        let n = (r.u32(12)? as usize).min(MAX_CHARS);
        let mut units: Vec<u16> = (0..n).map(|i| r.u16(16 + i * 2)).collect::<Option<_>>()?;
        if opts & 1 == 0 {
            // Glyph indices into the font. The crate knows no fonts, but the classic Windows core
            // fonts keep the standard Macintosh glyph order for ASCII, so those map back.
            units = glyphs_to_chars(&font.family, &units)?;
        }
        let pos = points(r, 16 + n * 2, n, false)?;
        let m = if r.u32(8)? != 0 { matrix(r, 16 + n * 10)? } else { IDENTITY };
        let lf = self.log_font(&font);
        let first = *pos.first()?;
        let dx: Vec<f32> = if opts & 4 != 0 {
            Vec::new()
        } else {
            let mut d: Vec<f32> = pos.windows(2).map(|w| (w[1].0 - w[0].0) as f32).collect();
            d.push(d.last().copied().unwrap_or(-lf.font.height * 0.5));
            d
        };
        let (text, dx) = crate::text::decode_utf16(&units, &dx);
        self.painter().text(cv, first, text, dx, TextStyle { color, font: lf, align: TA_BASELINE, clip: None }, m);
        Some(())
    }
}

/// Page unit to device pixels: world, display and pixel are 1; point, inch, document, mm.
fn unit_px(unit: u32, dpi: f64) -> f64 {
    match unit {
        3 => dpi / 72.0,
        4 => dpi,
        5 => dpi / 300.0,
        6 => dpi / 25.4,
        _ => 1.0,
    }
}

struct TextStyle {
    color: [u8; 4],
    font: LogFont,
    align: u32,
    /// World clip rectangle `[l, t, r, b]`.
    clip: Option<[f64; 4]>,
}

/// Draws through the canvas with EMF+'s transform, clip and its own pen or brush, then puts the
/// canvas's GDI state back.
struct Painter {
    xf: Xform,
    clip: Area,
}

impl Painter {
    fn begin(&self, cv: &mut Canvas, pre: Xform) {
        let world = cv.world();
        cv.save_dc();
        cv.modify_world(mul(mul(pre, self.xf), world), 4);
        cv.set_clip(self.clip.clone());
    }

    fn end(&self, cv: &mut Canvas) {
        cv.restore_dc(-1);
        cv.set_clip(None);
    }

    fn no_pen(cv: &mut Canvas) {
        cv.set_pen(Pen { style: PS_NULL, width: 0.0, color: [0, 0, 0, 0] });
    }

    fn fill(&self, cv: &mut Canvas, ops: &[Op], fill: &Fill) {
        self.begin(cv, IDENTITY);
        Self::no_pen(cv);
        match fill {
            Fill::Solid(c) => {
                cv.set_brush(Brush::new(BS_SOLID, *c));
                cv.shape(ops, true, false);
            }
            Fill::Hatch { style, fg, bg } => match hatch_tile(*style, *fg, Some(*bg)) {
                Some(px) => cv.tile_shape(ops, Arc::new(RawTile { w: 8, h: 8, px, cell: cv.pattern_px })),
                // GDI+ has many more hatch styles than GDI: those fill with the blend of both colours.
                None => {
                    cv.set_brush(Brush::new(BS_SOLID, lerp(*fg, *bg, 0.5)));
                    cv.shape(ops, true, false);
                }
            },
            _ => {
                // Device point to brush space: invert brush space → world → device.
                if let Some(inv) = invert(mul(fill.xf(), cv.device_affine())) {
                    cv.shade_shape(ops, &|p| fill.sample(apply(inv, p)));
                }
            }
        }
        self.end(cv);
    }

    fn stroke(&self, cv: &mut Canvas, ops: &[Op], (color, width): ([u8; 4], f64)) {
        self.begin(cv, IDENTITY);
        cv.set_pen(Pen { style: 0, width, color });
        cv.set_brush(Brush::new(BS_NULL, [0, 0, 0, 0]));
        cv.shape(ops, false, true);
        self.end(cv);
    }

    fn image(&self, cv: &mut Canvas, dst: [f64; 4], img: Rgba) {
        self.begin(cv, IDENTITY);
        cv.bitmap(dst, img);
        self.end(cv);
    }

    /// A nested metafile's picture stretched into world rectangle `dst` = `[x, y, w, h]`.
    fn picture(&self, cv: &mut Canvas, [x, y, w, h]: [f64; 4], pic: &Picture) {
        self.begin(cv, IDENTITY);
        let (sx, sy) = (w / f64::from(pic.width), h / f64::from(pic.height));
        let m = mul([sx, 0.0, 0.0, sy, x, y], cv.device_affine());
        let scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
        let pt = |a: f32, b: f32| apply(m, (f64::from(a), f64::from(b)));
        let ops = |segs: &[Seg]| -> Vec<Op> {
            segs.iter()
                .map(|s| match *s {
                    Seg::Move(a, b) => Op::M(pt(a, b)),
                    Seg::Line(a, b) => Op::L(pt(a, b)),
                    Seg::Cubic(a, b, c, d, e, f) => Op::C(pt(a, b), pt(c, d), pt(e, f)),
                    Seg::Close => Op::Z,
                })
                .collect()
        };
        // The nested picture's own clip regions are not carried over: each item takes the
        // canvas's current clip.
        for item in &pic.items {
            if cv.full() {
                break;
            }
            let raw = match item {
                Item::Path { segs, fill, stroke, even_odd, .. } => Raw::Path {
                    ops: ops(segs),
                    fill: *fill,
                    stroke: stroke.map(|(c, w)| (c, f64::from(w) * scale)),
                    even_odd: *even_odd,
                    region: None,
                },
                Item::Pattern { segs, even_odd, tile, .. } => Raw::Pattern {
                    ops: ops(segs),
                    even_odd: *even_odd,
                    tile: Arc::new(RawTile {
                        w: tile.width as usize,
                        h: tile.height as usize,
                        px: tile.rgba.clone(),
                        cell: f64::from(tile.cell[0]) * scale,
                    }),
                    region: None,
                },
                Item::Bitmap { rect, width, height, rgba, .. } => {
                    let (a, b) = (pt(rect[0], rect[1]), pt(rect[0] + rect[2], rect[1] + rect[3]));
                    let mut img = Rgba { w: *width as usize, h: *height as usize, bottom_up: false, px: rgba.clone() };
                    crate::dib::flip(&mut img, a.0 > b.0, a.1 > b.1);
                    Raw::Bitmap { rect: [a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs()], img, region: None }
                }
                Item::Text(t) => {
                    let mut t = t.clone();
                    t.transform = mul(t.transform, m);
                    if let Some([x0, y0, x1, y1]) = t.clip {
                        let (p, q) = (pt(x0, y0), pt(x1, y1));
                        t.clip = Some([p.0.min(q.0) as f32, p.1.min(q.1) as f32, p.0.max(q.0) as f32, p.1.max(q.1) as f32]);
                    }
                    Raw::Text { text: t, region: None }
                }
            };
            cv.push_device(raw);
        }
        self.end(cv);
    }

    fn text(&self, cv: &mut Canvas, at: P, text: String, dx: Vec<f32>, style: TextStyle, pre: Xform) {
        self.begin(cv, pre);
        cv.set_text_style(style.color, style.font, style.align);
        let (opts, rect) = match style.clip {
            Some(c) => (ETO_CLIPPED, Some(c)),
            None => (0, None),
        };
        cv.text(at, text, dx, opts, rect);
        self.end(cv);
    }
}

fn apply(m: Xform, p: P) -> P {
    (p.0 * m[0] + p.1 * m[2] + m[4], p.0 * m[1] + p.1 * m[3] + m[5])
}

fn invert(m: Xform) -> Option<Xform> {
    let det = m[0] * m[3] - m[1] * m[2];
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(m[4] * a + m[5] * c), -(m[4] * b + m[5] * d)])
}

/// ARGB (0xAARRGGBB) to RGBA.
fn argb(v: u32) -> [u8; 4] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8, (v >> 24) as u8]
}

fn floats<const N: usize>(r: Bytes, off: usize) -> Option<[f64; N]> {
    let mut v = [0.0; N];
    for (i, x) in v.iter_mut().enumerate() {
        *x = f64::from(r.f32(off + i * 4)?);
    }
    v.iter().all(|x| x.is_finite()).then_some(v)
}

fn matrix(r: Bytes, off: usize) -> Option<Xform> {
    floats::<6>(r, off)
}

fn parse_object(kind: u16, r: Bytes) -> Option<Obj> {
    match kind {
        1 => brush(r, 0).map(Obj::Brush),
        2 => pen(r),
        3 => path(r).map(Obj::Path),
        4 => region(r),
        5 => image(r).map(|i| Obj::Image(Arc::new(i))),
        6 => font(r),
        7 => Some(Obj::Format(Format { flags: r.u32(4)?, align: r.u32(12)?, line_align: r.u32(16)? })),
        _ => None,
    }
}

/// A brush object at `off`: version, type, then the type's data.
fn brush(r: Bytes, off: usize) -> Option<Fill> {
    let d = r.tail(off + 8)?;
    match r.u32(off + 4)? {
        0 => Some(Fill::Solid(argb(d.u32(0)?))),
        1 => Some(Fill::Hatch { style: d.u32(0)?, fg: argb(d.u32(4)?), bg: argb(d.u32(8)?) }),
        2 => {
            let bflags = d.u32(0)?;
            let wrap = d.u32(4)?;
            let (xf, off) = if bflags & 0x2 != 0 { (matrix(d, 8)?, 32) } else { (IDENTITY, 8) };
            let ImageData::Bitmap(img) = image(d.tail(off)?)? else { return None };
            Some(Fill::Texture { img: Arc::new(img), wrap, xf })
        }
        3 => path_gradient(d),
        4 => linear_gradient(d),
        _ => None,
    }
}

fn linear_gradient(d: Bytes) -> Option<Fill> {
    let bflags = d.u32(0)?;
    let wrap = d.u32(4)?;
    let rect = floats::<4>(d, 8)?;
    let (c0, c1) = (argb(d.u32(24)?), argb(d.u32(28)?));
    let mut off = 40;
    let mut xf = IDENTITY;
    if bflags & 0x2 != 0 {
        xf = matrix(d, off)?;
        off += 24;
    }
    let mut stops = vec![(0.0, c0), (1.0, c1)];
    if bflags & 0x4 != 0 {
        // Preset colours: positions, then one colour each.
        let n = (d.u32(off)? as usize).min(MAX_STOPS);
        let pos: Vec<f64> = (0..n).map(|i| d.f32(off + 4 + i * 4).map(f64::from)).collect::<Option<_>>()?;
        let cols: Vec<[u8; 4]> = (0..n).map(|i| d.u32(off + 4 + n * 4 + i * 4).map(argb)).collect::<Option<_>>()?;
        stops = pos.into_iter().zip(cols).collect();
    } else if bflags & 0x8 != 0 {
        // Blend factors: positions, then how far from the start colour to the end colour.
        let n = (d.u32(off)? as usize).min(MAX_STOPS);
        let pos: Vec<f64> = (0..n).map(|i| d.f32(off + 4 + i * 4).map(f64::from)).collect::<Option<_>>()?;
        let fac: Vec<f64> = (0..n).map(|i| d.f32(off + 4 + n * 4 + i * 4).map(f64::from)).collect::<Option<_>>()?;
        stops = pos.into_iter().zip(fac).map(|(p, k)| (p, lerp(c0, c1, k.clamp(0.0, 1.0)))).collect();
    }
    stops.retain(|s| s.0.is_finite());
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    (!stops.is_empty()).then_some(Fill::Linear { rect, stops, wrap, xf })
}

/// A path gradient: from the centre colour to the (first) surrounding colour at the boundary,
/// taken as the ellipse in the boundary's bounding rectangle.
fn path_gradient(d: Bytes) -> Option<Fill> {
    let bflags = d.u32(0)?;
    let center_c = argb(d.u32(8)?);
    let [cx, cy] = floats::<2>(d, 12)?;
    let nc = (d.u32(20)? as usize).min(MAX_STOPS);
    let surround = if nc > 0 { argb(d.u32(24)?) } else { center_c };
    let mut off = 24 + nc * 4;
    let bbox = if bflags & 0x1 != 0 {
        let size = d.u32(off)? as usize;
        let b = ops_bbox(&path(d.tail(off + 4)?)?)?;
        off = off.checked_add(4 + size)?;
        b
    } else {
        let n = d.u32(off)? as usize;
        let pts = points(d, off + 4, n, false)?;
        off = off.checked_add(4 + 8 * n.min(MAX_POINTS))?;
        ops_bbox(&poly_ops(&pts, true))?
    };
    let xf = if bflags & 0x2 != 0 { matrix(d, off)? } else { IDENTITY };
    let radius = ((bbox[2] - bbox[0]) / 2.0, (bbox[3] - bbox[1]) / 2.0);
    Some(Fill::Radial { center: (cx, cy), radius, stops: vec![(0.0, center_c), (1.0, surround)], xf })
}

/// A pen object: version, type, then pen data (flags, unit, width, optional fields), then a brush.
fn pen(r: Bytes) -> Option<Obj> {
    let pflags = r.u32(8)?;
    let width = f64::from(r.f32(16)?);
    let mut off = 20usize;
    for (bit, len) in [(0x1, 24), (0x2, 4), (0x4, 4), (0x8, 4), (0x10, 4), (0x20, 4), (0x40, 4), (0x80, 4)] {
        if pflags & bit != 0 {
            off += len;
        }
    }
    for bit in [0x100, 0x200, 0x400] {
        if pflags & bit != 0 {
            off += if bit == 0x200 { 4 } else { 4 + 4 * (r.u32(off)? as usize).min(MAX_POINTS) };
        }
    }
    for bit in [0x800, 0x1000] {
        if pflags & bit != 0 {
            off += 4 + (r.u32(off)? as usize).min(1 << 20);
        }
    }
    let color = brush(r, off)?.color();
    Some(Obj::Pen { color, width: if width.is_finite() { width.clamp(0.0, 1.0e5) } else { 0.0 } })
}

/// A path object: points, then one type byte per point (start, line, Bézier; 0x80 closes).
fn path(r: Bytes) -> Option<Vec<Op>> {
    let n = (r.u32(4)? as usize).min(MAX_POINTS);
    let pflags = r.u32(8)?;
    if pflags & 0x1800 != 0 {
        // Relative (0x0800) and run-length encoded (0x1000) points: not read yet.
        return None;
    }
    let compressed = pflags & 0x4000 != 0;
    let pts = points(r, 12, n, compressed)?;
    let toff = 12 + n * if compressed { 4 } else { 8 };
    let types = r.slice(toff, n)?;
    let mut ops = Vec::with_capacity(n + 4);
    let mut i = 0;
    while i < n {
        let (t, p) = (types[i], pts[i]);
        match t & 0x07 {
            0 => ops.push(Op::M(p)),
            3 if i + 2 < n => {
                ops.push(Op::C(p, pts[i + 1], pts[i + 2]));
                if types[i + 2] & 0x80 != 0 {
                    ops.push(Op::Z);
                }
                i += 3;
                continue;
            }
            _ => ops.push(Op::L(p)),
        }
        if t & 0x80 != 0 {
            ops.push(Op::Z);
        }
        i += 1;
    }
    Some(ops)
}

/// A region object as a convex area in world units (non-convex parts become their bounding
/// rectangle).
fn region(r: Bytes) -> Option<Obj> {
    let mut work = 0;
    region_node(r, 8, 0, &mut work).map(|(a, _)| Obj::Region(a))
}

/// One region node at `off`: its area and its length in bytes.
fn region_node(r: Bytes, off: usize, depth: usize, work: &mut usize) -> Option<(Area, usize)> {
    *work = work.checked_add(1)?;
    if depth > MAX_REGION_DEPTH || *work > MAX_REGION_WORK {
        return None;
    }
    match r.u32(off)? {
        0x1000_0000 => {
            let [x, y, w, h] = floats::<4>(r, off + 4)?;
            Some((Some(rect_poly([x.min(x + w), y.min(y + h), x.max(x + w), y.max(y + h)])), 20))
        }
        0x1000_0001 => {
            let size = r.u32(off + 4)? as usize;
            if size < 12 {
                return None;
            }
            let data = Bytes(r.slice(off + 8, size)?);
            let points = (data.u32(4)? as usize).min(MAX_POINTS);
            *work = work.checked_add(points)?;
            if *work > MAX_REGION_WORK {
                return None;
            }
            let area = area_of_ops(&path(data)?);
            Some((Some(area), 8usize.checked_add(size)?))
        }
        0x1000_0002 => Some((Some(Vec::new()), 4)),
        0x1000_0003 => Some((None, 4)),
        mode @ 0..=5 => {
            let (a, la) = region_node(r, off + 4, depth + 1, work)?;
            let (b, lb) = region_node(r, off.checked_add(4 + la)?, depth + 1, work)?;
            let out = match mode {
                0 => b,
                1 => area_intersect(a, b),
                2 => area_union(a, b),
                // XOR, exclude, complement: the left area bounds the result closely enough.
                _ => a,
            };
            Some((out, (4 + la).checked_add(lb)?))
        }
        _ => None,
    }
}

/// The area a path encloses: its outline when that is one convex figure, else the rectangle
/// around it.
fn area_of_ops(ops: &[Op]) -> Vec<P> {
    let subs = flatten(ops);
    if let [(pts, _)] = subs.as_slice()
        && let Some(c) = convex(pts)
    {
        return c;
    }
    ops_bbox(ops).map(rect_poly).unwrap_or_default()
}

fn area_intersect(a: Area, b: Area) -> Area {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) if a.len() >= 3 && b.len() >= 3 => Some(convex(&clip_polygon(&a, &b)).unwrap_or_default()),
        _ => Some(Vec::new()),
    }
}

fn area_union(a: Area, b: Area) -> Area {
    match (a, b) {
        (Some(a), Some(b)) => {
            if a.len() < 3 {
                return Some(b);
            }
            if b.len() < 3 {
                return Some(a);
            }
            let all: Vec<P> = a.into_iter().chain(b).collect();
            Some(pts_bbox(&all).map(rect_poly).unwrap_or_default())
        }
        _ => None,
    }
}

/// An area through an affine map (which keeps it convex; the orientation is put right again).
fn map_area(a: Area, m: Xform) -> Area {
    a.map(|pts| {
        let mapped: Vec<P> = pts.iter().map(|p| apply(m, *p)).collect();
        if mapped.len() < 3 { Vec::new() } else { convex(&mapped).unwrap_or_default() }
    })
}

/// An elliptical arc in `rect` from `start` sweeping `sweep` degrees (clockwise on screen), as
/// cubics; a pie adds the lines to and from the centre.
fn arc_ops([l, t, r, b]: [f64; 4], start: f64, sweep: f64, pie: bool) -> Vec<Op> {
    let (cx, cy, rx, ry) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) / 2.0, (b - t) / 2.0);
    let sweep = sweep.clamp(-360.0, 360.0);
    // GDI+ angles are along rays from the centre; turn them into the ellipse's parameter.
    let param = |deg: f64| {
        let a = deg.to_radians();
        (rx * a.sin()).atan2(ry * a.cos())
    };
    let t0 = param(start);
    let mut dt = if sweep.abs() >= 360.0 { sweep.signum() * std::f64::consts::TAU } else { param(start + sweep) - t0 };
    if sweep.abs() < 360.0 {
        while sweep > 0.0 && dt < 0.0 {
            dt += std::f64::consts::TAU;
        }
        while sweep < 0.0 && dt > 0.0 {
            dt -= std::f64::consts::TAU;
        }
    }
    let at = |t: f64| (cx + rx * t.cos(), cy + ry * t.sin());
    let mut ops = Vec::with_capacity(8);
    if pie {
        ops.push(Op::M((cx, cy)));
        ops.push(Op::L(at(t0)));
    } else {
        ops.push(Op::M(at(t0)));
    }
    let n = ((dt.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).clamp(1, 4);
    let step = dt / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    for i in 0..n {
        let (a, e) = (t0 + step * i as f64, t0 + step * (i + 1) as f64);
        let (p0, p3) = (at(a), at(e));
        let c1 = (p0.0 - k * rx * a.sin(), p0.1 + k * ry * a.cos());
        let c2 = (p3.0 + k * rx * e.sin(), p3.1 - k * ry * e.cos());
        ops.push(Op::C(c1, c2, p3));
    }
    if pie {
        ops.push(Op::Z);
    }
    ops
}

/// A font object: em size, unit, style flags, then the family name in UTF-16.
fn font(r: Bytes) -> Option<Obj> {
    let em = f64::from(r.f32(4)?);
    let n = (r.u32(20)? as usize).min(64);
    let units: Vec<u16> = (0..n).map(|i| r.u16(24 + i * 2)).collect::<Option<_>>()?;
    let family = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
    em.is_finite().then_some(Obj::Font(FontObj { em, unit: r.u32(8)?, style: r.u32(12)?, family }))
}

/// An image object: a bitmap of raw pixels or a compressed (PNG, JPEG, GIF…) file, or a nested
/// metafile.
fn image(r: Bytes) -> Option<ImageData> {
    match r.u32(4)? {
        1 => bitmap(r).map(ImageData::Bitmap),
        2 => metafile(r).map(ImageData::Meta),
        _ => None,
    }
}

/// A metafile image: type, size, then a WMF (placeable or not) or EMF file, parsed as a picture.
fn metafile(r: Bytes) -> Option<Picture> {
    let size = r.u32(12)? as usize;
    let data = r.slice(16, size).or_else(|| r.tail(16).map(|t| t.0))?;
    let depth = NESTING.with(std::cell::Cell::get);
    if depth >= MAX_NESTING {
        return None;
    }
    NESTING.with(|n| n.set(depth + 1));
    let pic = crate::parse(data).ok();
    NESTING.with(|n| n.set(depth));
    pic.filter(|p| p.width > 0.0 && p.height > 0.0)
}

fn bitmap(r: Bytes) -> Option<Rgba> {
    let (w, h, stride) = (r.u32(8)? as i32, r.u32(12)? as i32, r.u32(16)? as i32);
    let format = r.u32(20)?;
    let data = r.tail(28)?;
    if r.u32(24)? == 1 {
        return decode_compressed(data.0);
    }
    let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
    if w == 0 || h == 0 || w.checked_mul(h)? > crate::MAX_PIXELS {
        return None;
    }
    let bpp = ((format >> 8) & 0xFF) as usize;
    let (pal, pixels) = if format & 0x0001_0000 != 0 {
        // Indexed formats carry a palette first: flags, count, ARGB entries.
        let n = (data.u32(4)? as usize).min(256);
        let pal: Vec<[u8; 4]> = (0..n).map(|i| data.u32(8 + i * 4).map(argb)).collect::<Option<_>>()?;
        (pal, data.tail(8 + n * 4)?)
    } else {
        (Vec::new(), data)
    };
    let row = (w.checked_mul(bpp)?).div_ceil(8);
    let stride_abs = (stride.unsigned_abs() as usize).max(row);
    let mut px = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let line = pixels.slice(y.checked_mul(stride_abs)?, row)?;
        for x in 0..w {
            let c = match (format, bpp) {
                (_, 1 | 4 | 8) if !pal.is_empty() => {
                    let bit = x * bpp;
                    let byte = *line.get(bit / 8)?;
                    let i = (byte >> (8 - bpp - bit % 8)) & ((1u16 << bpp) - 1) as u8;
                    pal.get(usize::from(i)).copied().unwrap_or([0, 0, 0, 255])
                }
                (_, 24) => {
                    let p = line.get(x * 3..x * 3 + 3)?;
                    [p[2], p[1], p[0], 255]
                }
                (0x0002_2009, 32) => {
                    let p = line.get(x * 4..x * 4 + 4)?;
                    [p[2], p[1], p[0], 255]
                }
                (0x000E_200B, 32) => {
                    let p = line.get(x * 4..x * 4 + 4)?;
                    let a = u32::from(p[3]);
                    let un = |v: u8| (u32::from(v) * 255 + a / 2).checked_div(a).map_or(v, |c| c.min(255) as u8);
                    [un(p[2]), un(p[1]), un(p[0]), p[3]]
                }
                (_, 32) => {
                    let p = line.get(x * 4..x * 4 + 4)?;
                    [p[2], p[1], p[0], p[3]]
                }
                (0x0002_1006, 16) => {
                    let v = u16::from_le_bytes([*line.get(x * 2)?, *line.get(x * 2 + 1)?]);
                    let s = |v: u16, bits: u32| ((u32::from(v) * 255) / ((1 << bits) - 1)) as u8;
                    [s(v >> 11, 5), s((v >> 5) & 0x3F, 6), s(v & 0x1F, 5), 255]
                }
                (_, 16) => {
                    let v = u16::from_le_bytes([*line.get(x * 2)?, *line.get(x * 2 + 1)?]);
                    let s = |v: u16| ((u32::from(v & 0x1F) * 255) / 31) as u8;
                    [s(v >> 10), s(v >> 5), s(v), 255]
                }
                _ => return None,
            };
            px.extend_from_slice(&c);
        }
    }
    // A negative stride stores the rows bottom-up.
    Some(top_down(Rgba { w, h, bottom_up: stride < 0, px }))
}

fn top_down(mut img: Rgba) -> Rgba {
    if img.bottom_up {
        crate::dib::flip(&mut img, false, true);
        img.bottom_up = false;
    }
    img
}

fn decode_compressed(bytes: &[u8]) -> Option<Rgba> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1 << 15);
    limits.max_image_height = Some(1 << 15);
    limits.max_alloc = Some((crate::MAX_PIXELS * 4) as u64);
    reader.limits(limits);
    let (w, h) = reader.into_dimensions().ok()?;
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || w.checked_mul(h)? > crate::MAX_PIXELS {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1 << 15);
    limits.max_image_height = Some(1 << 15);
    limits.max_alloc = Some((crate::MAX_PIXELS * 4) as u64);
    reader.limits(limits);
    let img = reader.decode().ok()?.to_rgba8();
    Some(Rgba { w, h, bottom_up: false, px: img.into_raw() })
}

/// `n` points at `off`: 16-bit integers when compressed, else 32-bit floats.
fn points(r: Bytes, off: usize, n: usize, compressed: bool) -> Option<Vec<P>> {
    let n = n.min(MAX_POINTS);
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        v.push(if compressed {
            (f64::from(r.i16(off + i * 4)?), f64::from(r.i16(off + i * 4 + 2)?))
        } else {
            let (x, y) = (f64::from(r.f32(off + i * 8)?), f64::from(r.f32(off + i * 8 + 4)?));
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            (x, y)
        });
    }
    Some(v)
}

/// `n` rectangles (x, y, w, h) at `off`, as `[l, t, r, b]`.
fn rects(r: Bytes, off: usize, n: usize, compressed: bool) -> Option<Vec<[f64; 4]>> {
    let pts = points(r, off, n.min(MAX_POINTS / 2).checked_mul(2)?, compressed)?;
    Some(pts.as_chunks::<2>().0.iter().map(|c| [c[0].0, c[0].1, c[0].0 + c[1].0, c[0].1 + c[1].1]).collect())
}

fn rect_ops([l, t, r, b]: [f64; 4]) -> Vec<Op> {
    crate::geom::rect(l, t, r, b)
}

fn finite(v: f32) -> Option<f64> {
    v.is_finite().then_some(f64::from(v))
}

/// A GDI+ cardinal spline through `pts` as cubic Béziers: each segment's control points lie a
/// third of `tension` (GDI+'s default 0.5) along the neighbouring chords. `closed` joins the
/// last point back to the first; an open curve draws `segments` segments from point `offset`.
fn curve_ops(pts: &[P], tension: f64, closed: bool, offset: usize, segments: usize) -> Vec<Op> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let k = tension.clamp(0.0, 10.0) / 3.0;
    let at = |i: isize| -> P {
        let i = if closed { i.rem_euclid(n as isize) as usize } else { i.clamp(0, n as isize - 1) as usize };
        pts.get(i).copied().unwrap_or((0.0, 0.0))
    };
    let (first, count) = if closed { (0, n) } else { (offset.min(n - 1), segments.min(n - 1 - offset.min(n - 1))) };
    let mut ops = vec![Op::M(at(first as isize))];
    for s in first..first + count {
        let i = s as isize;
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = (p1.0 + (p2.0 - p0.0) * k, p1.1 + (p2.1 - p0.1) * k);
        let c2 = (p2.0 - (p3.0 - p1.0) * k, p2.1 - (p3.1 - p1.1) * k);
        ops.push(Op::C(c1, c2, p2));
    }
    if closed {
        ops.push(Op::Z);
    }
    ops
}

fn poly_ops(pts: &[P], close: bool) -> Vec<Op> {
    let mut ops: Vec<Op> = pts.iter().enumerate().map(|(i, p)| if i == 0 { Op::M(*p) } else { Op::L(*p) }).collect();
    if close {
        ops.push(Op::Z);
    }
    ops
}

/// An ellipse in its bounding box, as four cubic arcs.
fn ellipse_ops([l, t, r, b]: [f64; 4]) -> Vec<Op> {
    let (cx, cy, rx, ry) = ((l + r) / 2.0, (t + b) / 2.0, (r - l) / 2.0, (b - t) / 2.0);
    let k = 0.552_284_75;
    let (ox, oy) = (rx * k, ry * k);
    vec![
        Op::M((cx + rx, cy)),
        Op::C((cx + rx, cy + oy), (cx + ox, cy + ry), (cx, cy + ry)),
        Op::C((cx - ox, cy + ry), (cx - rx, cy + oy), (cx - rx, cy)),
        Op::C((cx - rx, cy - oy), (cx - ox, cy - ry), (cx, cy - ry)),
        Op::C((cx + ox, cy - ry), (cx + rx, cy - oy), (cx + rx, cy)),
        Op::Z,
    ]
}

/// Core fonts whose first glyphs follow the standard Macintosh order (glyph 3 is the space,
/// glyphs 3..=97 are U+0020..=U+007E).
const MAC_ORDER_FONTS: &[&str] =
    &["arial", "arial black", "times new roman", "courier new", "verdana", "georgia", "tahoma", "trebuchet ms", "comic sans ms", "impact"];

/// Glyph indices of a known font back to ASCII; None when the font is unknown or a glyph is
/// outside the ASCII range.
fn glyphs_to_chars(family: &str, glyphs: &[u16]) -> Option<Vec<u16>> {
    let family = family.trim().to_ascii_lowercase();
    if !MAC_ORDER_FONTS.contains(&family.as_str()) {
        return None;
    }
    glyphs.iter().map(|g| (3..=97).contains(g).then_some(g + 29)).collect()
}

/// A rough advance for a character in ems: the metafile crate has no fonts, so line breaks for
/// wrapped strings use typical proportional widths.
fn char_em(u: u16) -> f64 {
    match char::from_u32(u32::from(u)).unwrap_or('x') {
        ' ' | 'i' | 'l' | 'j' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' | 'I' | 'f' | 't' | 'r' => 0.3,
        'm' | 'w' | 'M' | 'W' => 0.85,
        c if c.is_ascii_uppercase() || c.is_ascii_digit() => 0.62,
        c if (c as u32) >= 0x2E80 => 1.0,
        _ => 0.52,
    }
}

/// Greedy word wrap of one line to `width` world units at em size `em`; a word longer than the
/// line breaks between characters.
fn wrap_line(line: &[u16], width: f64, em: f64) -> Vec<Vec<u16>> {
    if em <= 0.0 || !width.is_finite() {
        return vec![line.to_vec()];
    }
    let trimmed = |v: &[u16]| v.iter().rposition(|u| *u != 0x20).map_or(0, |i| i + 1);
    let measure = |v: &[u16]| v.iter().map(|u| char_em(*u) * em).sum::<f64>();

    let mut out: Vec<Vec<u16>> = Vec::new();
    let mut cur: Vec<u16> = Vec::new();
    let mut cur_total_w = 0.0;

    for word in line.split_inclusive(|u| *u == 0x20) {
        let n_non_space = trimmed(word);
        let word_non_space_w = measure(&word[..n_non_space]);
        let word_total_w = measure(word);

        if cur.is_empty() {
            if word_non_space_w <= width {
                cur.extend_from_slice(word);
                cur_total_w = word_total_w;
                continue;
            }
        } else if cur_total_w + word_non_space_w <= width {
            cur.extend_from_slice(word);
            cur_total_w += word_total_w;
            continue;
        }

        if !cur.is_empty() {
            let n = trimmed(&cur);
            cur.truncate(n);
            out.push(std::mem::take(&mut cur));
        }

        if word_non_space_w <= width {
            cur.extend_from_slice(word);
            cur_total_w = word_total_w;
            continue;
        }

        // A word wider than the line on its own: break it between characters.
        let mut cur_w = 0.0;
        for &u in word {
            let u_w = char_em(u) * em;
            if cur.is_empty() {
                cur.push(u);
                cur_w = u_w;
            } else if cur_w + u_w <= width {
                cur.push(u);
                cur_w += u_w;
            } else {
                let n = trimmed(&cur);
                cur.truncate(n);
                out.push(std::mem::take(&mut cur));
                cur.push(u);
                cur_w = u_w;
            }
        }
        cur_total_w = cur_w;
    }
    let n = cur.iter().rposition(|u| *u != 0x20).map_or(0, |i| i + 1);
    cur.truncate(n);
    out.push(cur);
    out
}
