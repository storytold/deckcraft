//! Drawing state shared by the WMF and EMF readers: window/viewport mapping and the EMF world
//! transform, pen and brush selection through an object table, the DC save stack and path brackets.
//! Output is device-space items; `lib.rs` maps them into the picture frame.

use std::sync::{Arc, OnceLock};

use crate::clip::{self, MAX_CLIP_RECTS};
use crate::dib::Rgba;
use crate::geom::{Op, P};
use crate::text::{ETO_CLIPPED, ETO_OPAQUE, Font, TA_UPDATECP, Text, align};
use crate::{MAX_BYTES, MAX_ITEMS, MAX_OBJECTS, MAX_PATH_OPS, MAX_STACK};

pub(crate) const MM_TEXT: u32 = 1;
const MM_ISOTROPIC: u32 = 7;
const MM_ANISOTROPIC: u32 = 8;
const STOCK: u32 = 0x8000_0000;
pub(crate) const PS_NULL: u32 = 5;
/// Pen style bits; the rest of the style word holds end-cap, join and type flags.
pub(crate) const PS_STYLE_MASK: u32 = 0x0F;
pub(crate) const BS_SOLID: u32 = 0;
/// Cells on the longer side of a computed (gradient or texture) fill tile.
const SHADE_RES: usize = 128;
pub(crate) const BS_NULL: u32 = 1;
pub(crate) const BS_HATCHED: u32 = 2;
/// BS_PATTERN, BS_DIBPATTERN and BS_DIBPATTERNPT: the brush paints a bitmap tile.
const BS_PATTERN: u32 = 3;
const BS_DIBPATTERN: u32 = 5;
const BS_DIBPATTERNPT: u32 = 6;
const WINDING: u32 = 2;
/// SETROP2 mix modes. R2_NOP leaves the destination alone: outlines and fills draw nothing.
/// R2_XORPEN and R2_MASKPEN draw the classic see-through shadow (XOR a colour, AND a pattern, XOR
/// the colour again); the others draw as R2_COPYPEN.
const R2_XORPEN: u32 = 7;
const R2_MASKPEN: u32 = 9;
const R2_NOP: u32 = 11;
const R2_COPYPEN: u32 = 13;
/// SETBKMODE OPAQUE: text cells and hatch gaps are filled with the background colour.
const OPAQUE: u32 = 2;
const PATCOPY: u32 = 0x00F0_0021;
const BLACKNESS: u32 = 0x0000_0042;
const WHITENESS: u32 = 0x00FF_0062;
/// Device pixels per millimetre assumed for WMF (96 dpi), and for EMF files without a device size.
const DEFAULT_PX_PER_MM: f64 = 96.0 / 25.4;

/// Affine transform `[m11, m12, m21, m22, dx, dy]` in EMF XFORM order: `x' = x·m11 + y·m21 + dx`.
pub(crate) type Xform = [f64; 6];
const IDENTITY: Xform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a` then `b` in EMF row-vector convention: the matrix product `a × b`.
pub(crate) fn mul(a: Xform, b: Xform) -> Xform {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Pen {
    pub style: u32,
    /// Logical width; 0 means a cosmetic one-pixel hairline.
    pub width: f64,
    pub color: [u8; 4],
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Brush {
    pub style: u32,
    pub color: [u8; 4],
    /// HS_* hatch style of a BS_HATCHED brush.
    pub hatch: u32,
    /// The tile of a pattern brush.
    pub pattern: Option<Arc<Pattern>>,
}

impl Brush {
    pub fn new(style: u32, color: [u8; 4]) -> Self {
        Brush { style, color, hatch: 0, pattern: None }
    }

    /// A pattern brush painting `img`. A monochrome brush paints its dark pixels in the text
    /// colour and its light ones in the background colour. None for an empty or oversized tile.
    pub fn pattern(img: Rgba, mono: bool) -> Option<Self> {
        if img.w == 0 || img.h == 0 || img.w > MAX_TILE_SIDE || img.h > MAX_TILE_SIDE || img.px.len() != img.w * img.h * 4 {
            return None;
        }
        let pattern = Arc::new(Pattern { w: img.w, h: img.h, px: img.px, mono, plain: OnceLock::new() });
        Some(Brush { style: BS_DIBPATTERNPT, color: [0, 0, 0, 255], hatch: 0, pattern: Some(pattern) })
    }
}

/// A pattern brush's tile, top-down RGBA.
#[derive(PartialEq, Debug)]
pub(crate) struct Pattern {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    pub mono: bool,
    /// The tile as drawn when nothing recolours it, shared by every fill with this brush.
    pub plain: OnceLock<Arc<RawTile>>,
}

/// Largest pattern tile side kept (GDI brushes are typically 8×8).
pub(crate) const MAX_TILE_SIDE: usize = 256;

#[derive(Clone, Debug)]
pub(crate) enum Obj {
    Pen(Pen),
    Brush(Brush),
    Font(Arc<LogFont>),
    /// Palettes and regions: they take a table slot but draw nothing.
    Other,
}

/// A font object: the neutral [`Font`] and its escapement (tenths of a degree, counter-clockwise),
/// which the text transform takes.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct LogFont {
    pub font: Font,
    pub escapement: f64,
}

/// A brush tile in device units: `cell` device units per tile pixel, laid from the device origin.
#[derive(Debug, PartialEq)]
pub(crate) struct RawTile {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    pub cell: f64,
}

/// What a fill paints.
enum Paint {
    Solid([u8; 4]),
    Tile(Arc<RawTile>),
}

/// Converts a COLORREF (0x00BBGGRR) to opaque RGBA.
pub(crate) fn colorref(c: u32) -> [u8; 4] {
    [(c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8, 255]
}

/// Millimetres per logical unit of the metric map modes (LOMETRIC, HIMETRIC, LOENGLISH, HIENGLISH, TWIPS).
fn metric_mm(mode: u32) -> Option<f64> {
    match mode {
        2 => Some(0.1),
        3 => Some(0.01),
        4 => Some(25.4 / 100.0),
        5 => Some(25.4 / 1000.0),
        6 => Some(25.4 / 1440.0),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Mapping {
    wo: P,
    we: P,
    vo: P,
    ve: P,
    mode: u32,
    world: Xform,
    /// Device pixels per millimetre on each axis (EMF: from szlDevice and szlMillimeters).
    px_mm: P,
}

impl Mapping {
    fn identity() -> Self {
        Mapping {
            wo: (0.0, 0.0),
            we: (1.0, 1.0),
            vo: (0.0, 0.0),
            ve: (1.0, 1.0),
            mode: MM_TEXT,
            world: IDENTITY,
            px_mm: (DEFAULT_PX_PER_MM, DEFAULT_PX_PER_MM),
        }
    }

    /// Logical point to device point: world transform, then window to viewport.
    pub fn device(&self, p: P) -> P {
        let w = self.world;
        let (x, y) = (p.0 * w[0] + p.1 * w[2] + w[4], p.0 * w[1] + p.1 * w[3] + w[5]);
        let (sx, sy) = self.ratios();
        ((x - self.wo.0) * sx + self.vo.0, (y - self.wo.1) * sy + self.vo.1)
    }

    /// Window-to-viewport scale per axis. MM_TEXT fixes the scale at 1; the metric modes use a fixed
    /// scale with y up; MM_ISOTROPIC uses one scale on both axes (the smaller magnitude, each axis's
    /// sign kept); anisotropic and unknown modes use the extents as given.
    fn ratios(&self) -> (f64, f64) {
        if self.mode == MM_TEXT {
            return (1.0, 1.0);
        }
        if let Some(mm) = metric_mm(self.mode) {
            return (mm * self.px_mm.0, -mm * self.px_mm.1);
        }
        let (rx, ry) = (ratio(self.ve.0, self.we.0), ratio(self.ve.1, self.we.1));
        if self.mode == MM_ISOTROPIC {
            let s = rx.abs().min(ry.abs());
            return (if rx < 0.0 { -s } else { s }, if ry < 0.0 { -s } else { s });
        }
        (rx, ry)
    }

    /// Approximate device length per logical length, for pen widths.
    fn scale(&self) -> f64 {
        let w = self.world;
        let det = (w[0] * w[3] - w[1] * w[2]).abs().sqrt();
        let (sx, sy) = self.ratios();
        det * (sx.abs() + sy.abs()) / 2.0
    }

    /// True when the window-to-viewport extents set the scale (the isotropic and anisotropic modes).
    pub fn uses_extents(&self) -> bool {
        matches!(self.mode, MM_ISOTROPIC | MM_ANISOTROPIC)
    }

    pub fn window_ext(&self) -> P {
        self.we
    }

    pub fn viewport_ext(&self) -> P {
        self.ve
    }

    /// The window in device units as `[x, y, w, h]` with positive extents, used as the picture frame
    /// of a plain WMF.
    pub fn window_rect(&self) -> [f64; 4] {
        let a = self.device(self.wo);
        let b = self.device((self.wo.0 + self.we.0, self.wo.1 + self.we.1));
        [a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs()]
    }
}

fn ratio(num: f64, den: f64) -> f64 {
    if den == 0.0 { 1.0 } else { num / den }
}

/// Output of drawing, in device units. Path points are already mapped; `lib.rs` normalises them.
pub(crate) enum Raw {
    Path {
        ops: Vec<Op>,
        fill: Option<[u8; 4]>,
        stroke: Option<([u8; 4], f64)>,
        even_odd: bool,
        /// Device-space clip region. None is the default clip.
        region: Option<Vec<[f64; 4]>>,
    },
    /// A path filled with a repeating tile.
    Pattern {
        ops: Vec<Op>,
        even_odd: bool,
        tile: Arc<RawTile>,
        region: Option<Vec<[f64; 4]>>,
    },
    Bitmap {
        rect: [f64; 4],
        img: Rgba,
        region: Option<Vec<[f64; 4]>>,
    },
    /// Text whose transform and clip are in device units. `region` is the device clip.
    Text {
        text: Box<Text>,
        region: Option<Vec<[f64; 4]>>,
    },
}

impl Raw {
    fn set_region(&mut self, region: Option<Vec<[f64; 4]>>) {
        match self {
            Raw::Path { region: slot, .. }
            | Raw::Pattern { region: slot, .. }
            | Raw::Bitmap { region: slot, .. }
            | Raw::Text { region: slot, .. } => {
                *slot = region;
            }
        }
    }
}

/// Text drawing state: colours, background mode, alignment and the selected font.
#[derive(Clone)]
struct TextState {
    color: [u8; 4],
    bk_color: [u8; 4],
    bk_opaque: bool,
    align: u32,
    font: Option<Arc<LogFont>>,
}

#[derive(Clone)]
struct Saved {
    map: Mapping,
    pen: Pen,
    brush: Brush,
    even_odd: bool,
    rop2: u32,
    text: TextState,
    clip: Option<Vec<[f64; 4]>>,
}

pub(crate) struct Canvas {
    pub items: Vec<Raw>,
    pub map: Mapping,
    /// True once SETWINDOWEXT was seen, so a plain WMF has a frame to use.
    pub window_set: bool,
    /// True once the file set the viewport extent itself (not the player, see [`Canvas::play_into`]).
    pub viewport_set: bool,
    saved: Vec<Saved>,
    /// Bytes held by the items so far (path ops and pixels), capped at [`MAX_BYTES`].
    used: usize,
    objects: Vec<Option<Obj>>,
    pen: Pen,
    brush: Brush,
    even_odd: bool,
    /// SETROP2 mix mode.
    rop2: u32,
    /// The colour of the last fill drawn with R2_XORPEN, for the see-through shadow idiom.
    xor_color: Option<[u8; 4]>,
    text: TextState,
    /// Device units per pattern-brush pixel: brushes are laid out in 96-dpi pixels.
    pub pattern_px: f64,
    pos: P,
    in_path: bool,
    path: Vec<Op>,
    full: bool,
    /// Current clip in device units. None is the default (the whole surface); Some([]) draws nothing.
    clip: Option<Vec<[f64; 4]>>,
    /// Device bounds of the picture, so excluding from the default clip has a surface to cut.
    surface: Option<[f64; 4]>,
    /// EMF+ convex device clip polygon: drawing is cut to it geometrically.
    plus_clip: Option<Vec<P>>,
}

impl Canvas {
    pub fn new() -> Self {
        Canvas {
            items: Vec::new(),
            map: Mapping::identity(),
            window_set: false,
            viewport_set: false,
            saved: Vec::new(),
            used: 0,
            objects: Vec::new(),
            pen: Pen { style: 0, width: 0.0, color: [0, 0, 0, 255] },
            brush: Brush::new(BS_SOLID, [255, 255, 255, 255]),
            even_odd: true,
            rop2: R2_COPYPEN,
            xor_color: None,
            text: TextState { color: [0, 0, 0, 255], bk_color: [255, 255, 255, 255], bk_opaque: true, align: 0, font: None },
            pattern_px: 1.0,
            pos: (0.0, 0.0),
            in_path: false,
            path: Vec::new(),
            full: false,
            clip: None,
            surface: None,
            plus_clip: None,
        }
    }

    /// Device bounds `[x0, y0, x1, y1]` of the picture. Used when a clip record cuts the default clip.
    pub fn set_clip_surface(&mut self, rect: [f64; 4]) {
        self.surface = clip::normalize(rect);
    }

    /// True once the item cap is hit; the readers stop there and keep what they have.
    pub fn full(&self) -> bool {
        self.full
    }

    fn push(&mut self, raw: Raw) {
        match self.plus_clip.take() {
            None => self.push_raw(raw),
            Some(c) => {
                for r in crate::cliprect::clip_raw(raw, &c) {
                    self.push_raw(r);
                }
                self.plus_clip = Some(c);
            }
        }
    }

    fn push_raw(&mut self, mut raw: Raw) {
        if self.clip.as_ref().is_some_and(Vec::is_empty) {
            return;
        }
        raw.set_region(self.clip.clone());
        let region_cost = self.clip.as_ref().map_or(0, |c| c.len() * std::mem::size_of::<[f64; 4]>());
        let cost = match &raw {
            Raw::Path { ops, .. } => ops.len() * std::mem::size_of::<Op>(),
            // A tile shared with its brush is counted once (by the brush); one made for this fill
            // counts here.
            Raw::Pattern { ops, tile, .. } => {
                let own = if Arc::strong_count(tile) == 1 { tile.px.len() } else { 0 };
                ops.len() * std::mem::size_of::<Op>() + 64 + own
            }
            Raw::Bitmap { img, .. } => img.px.len(),
            Raw::Text { text: t, .. } => t.text.len() + t.dx.len() * 4 + t.font.family.len() + 128,
        } + region_cost;
        if self.items.len() >= MAX_ITEMS || self.used.saturating_add(cost) > MAX_BYTES {
            self.full = true;
        } else {
            self.used += cost;
            self.items.push(raw);
        }
    }

    pub fn set_window_org(&mut self, p: P) {
        self.map.wo = p;
    }

    pub fn set_window_ext(&mut self, e: P) {
        self.map.we = e;
        self.window_set = true;
    }

    pub fn set_viewport_org(&mut self, p: P) {
        self.map.vo = p;
    }

    pub fn set_viewport_ext(&mut self, e: P) {
        self.map.ve = e;
        self.viewport_set = true;
    }

    /// Sets up the mapping the way a player does before it plays a WMF into a target: anisotropic,
    /// `window` (`[x, y, w, h]` in logical units) onto a viewport of `size` at the origin. The file's
    /// own window records then replace the window; the viewport stays unless the file sets one.
    /// Without a window (a plain WMF) only the viewport extent is set: the map mode and viewport
    /// origin stay the file's.
    pub fn play_into(&mut self, window: Option<[f64; 4]>, size: P) {
        if let Some([x, y, w, h]) = window {
            self.map.mode = MM_ANISOTROPIC;
            self.map.wo = (x, y);
            self.map.we = (w, h);
            self.map.vo = (0.0, 0.0);
        }
        self.map.ve = size;
    }

    pub fn set_map_mode(&mut self, mode: u32) {
        self.map.mode = mode;
    }

    /// Device pixels per millimetre (x, y) for the metric map modes. Ignored unless both are positive.
    pub fn set_device_mm(&mut self, px_mm: P) {
        if px_mm.0.is_finite() && px_mm.1.is_finite() && px_mm.0 > 0.0 && px_mm.1 > 0.0 {
            self.map.px_mm = px_mm;
        }
    }

    pub fn set_polyfill(&mut self, mode: u32) {
        self.even_odd = mode != WINDING;
    }

    /// SETROP2. R2_NOP draws nothing; R2_XORPEN and R2_MASKPEN are read as the see-through shadow
    /// idiom (see [`Canvas::fill_paint`]); the other mix modes draw as copy.
    pub fn set_rop2(&mut self, mode: u32) {
        self.rop2 = mode;
    }

    pub fn set_text_color(&mut self, c: u32) {
        self.text.color = colorref(c);
    }

    pub fn set_bk_color(&mut self, c: u32) {
        self.text.bk_color = colorref(c);
    }

    pub fn set_bk_mode(&mut self, mode: u32) {
        self.text.bk_opaque = mode == OPAQUE;
    }

    pub fn set_text_align(&mut self, ta: u32) {
        self.text.align = ta;
    }

    /// The character set of the selected font (ANSI without one).
    pub fn font_charset(&self) -> u8 {
        self.text.font.as_ref().map_or(0, |f| f.font.charset)
    }

    /// EMF SETWORLDTRANSFORM (`mode` 4) and MODIFYWORLDTRANSFORM (1 identity, 2 left, 3 right, 4 set).
    /// An unknown mode leaves the world transform as it was.
    pub fn modify_world(&mut self, x: Xform, mode: u32) {
        match mode {
            1 => self.map.world = IDENTITY,
            2 => self.map.world = mul(x, self.map.world),
            3 => self.map.world = mul(self.map.world, x),
            4 => self.map.world = x,
            _ => {}
        }
    }

    /// The current world transform (EMF+ draws through it).
    pub fn world(&self) -> Xform {
        self.map.world
    }

    /// Sets the pen and brush directly (EMF+ records name their own pen and brush).
    pub fn set_pen(&mut self, pen: Pen) {
        self.pen = pen;
    }

    pub fn set_brush(&mut self, brush: Brush) {
        self.brush = brush;
    }

    /// Sets the EMF+ convex device clip polygon (None: unclipped).
    pub fn set_clip(&mut self, clip: Option<Vec<P>>) {
        self.plus_clip = clip;
    }

    /// Logical to device as one affine transform (the mapping is affine).
    pub fn device_affine(&self) -> Xform {
        let o = self.map.device((0.0, 0.0));
        let x = self.map.device((1.0, 0.0));
        let y = self.map.device((0.0, 1.0));
        [x.0 - o.0, x.1 - o.1, y.0 - o.0, y.1 - o.1, o.0, o.1]
    }

    /// Text colour, font and alignment set directly (EMF+ strings name their own font and brush);
    /// the background is transparent.
    pub fn set_text_style(&mut self, color: [u8; 4], font: LogFont, align: u32) {
        self.text.color = color;
        self.text.bk_opaque = false;
        self.text.align = align;
        self.text.font = Some(Arc::new(font));
    }

    /// Pushes an item already in device units (a nested metafile's drawing).
    pub fn push_device(&mut self, raw: Raw) {
        self.push(raw);
    }

    /// Fills logical `ops` with a ready-made tile, laid from the device origin.
    pub fn tile_shape(&mut self, ops: &[Op], tile: Arc<RawTile>) {
        let dev: Vec<Op> = ops.iter().map(|o| self.map_op(*o)).collect();
        if dev.len() >= 2 {
            let even_odd = self.even_odd;
            self.push(Raw::Pattern { ops: dev, even_odd, tile, region: None });
        }
    }

    /// Fills logical `ops` with a colour computed per device point (gradients, textures): a tile
    /// of at most [`SHADE_RES`] cells on the longer side, laid out so that its one repeat covers
    /// the shape.
    pub fn shade_shape(&mut self, ops: &[Op], shade: &dyn Fn(P) -> [u8; 4]) {
        let dev: Vec<Op> = ops.iter().map(|o| self.map_op(*o)).collect();
        let Some([l, t, r, b]) = crate::cliprect::ops_bbox(&dev) else { return };
        let side = (r - l).max(b - t);
        if dev.len() < 2 || !(side.is_finite() && side > 0.0) {
            return;
        }
        let cell = side / SHADE_RES as f64;
        let w = (((r - l) / cell).ceil() as usize + 1).clamp(1, SHADE_RES + 1);
        let h = (((b - t) / cell).ceil() as usize + 1).clamp(1, SHADE_RES + 1);
        let (pw, ph) = (w as f64 * cell, h as f64 * cell);
        let mut px = Vec::with_capacity(w * h * 4);
        for j in 0..h {
            // The device position this tile row stands for inside the shape's box.
            let y = t + (j as f64 * cell - t).rem_euclid(ph) + cell / 2.0;
            for i in 0..w {
                let x = l + (i as f64 * cell - l).rem_euclid(pw) + cell / 2.0;
                px.extend_from_slice(&shade((x, y)));
            }
        }
        let even_odd = self.even_odd;
        self.push(Raw::Pattern { ops: dev, even_odd, tile: Arc::new(RawTile { w, h, px, cell }), region: None });
    }

    /// Pushes the drawing state. Past [`MAX_STACK`] levels the parse stops, since later restores
    /// could no longer match the file.
    pub fn save_dc(&mut self) {
        if self.saved.len() < MAX_STACK {
            self.saved.push(Saved {
                map: self.map,
                pen: self.pen,
                brush: self.brush.clone(),
                even_odd: self.even_odd,
                rop2: self.rop2,
                text: self.text.clone(),
                clip: self.clip.clone(),
            });
        } else {
            self.full = true;
        }
    }

    /// Negative `n` pops `-n` levels; positive `n` restores absolute level `n`.
    pub fn restore_dc(&mut self, n: i32) {
        let idx = if n < 0 { self.saved.len().checked_sub(n.unsigned_abs() as usize) } else { (n as usize).checked_sub(1) };
        let Some(idx) = idx else { return };
        let Some(s) = self.saved.get(idx).cloned() else { return };
        self.saved.truncate(idx);
        self.map = s.map;
        self.pen = s.pen;
        self.brush = s.brush;
        self.even_odd = s.even_odd;
        self.rop2 = s.rop2;
        self.text = s.text;
        self.clip = s.clip;
    }

    /// The logical rectangle as a device rectangle. A scale or translation keeps the rectangle; a
    /// rotation stores the bounds of the rotated corners.
    fn device_rect(&self, [l, t, r, b]: [f64; 4]) -> Option<[f64; 4]> {
        let corners = [(l, t), (r, t), (l, b), (r, b)].map(|p| self.map.device(p));
        if !corners.iter().all(|p| p.0.is_finite() && p.1.is_finite()) {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in corners {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        clip::normalize([x0, y0, x1, y1])
    }

    fn map_rects(&self, logical: &[[f64; 4]]) -> Vec<[f64; 4]> {
        let mut out = Vec::new();
        for r in logical.iter().take(MAX_CLIP_RECTS) {
            if let Some(d) = self.device_rect(*r) {
                out.push(d);
            }
            if out.len() >= MAX_CLIP_RECTS {
                break;
            }
        }
        out
    }

    /// INTERSECTCLIPRECT: later drawing is limited to this logical rectangle as well.
    pub fn intersect_clip_rect(&mut self, logical: [f64; 4]) {
        self.clip = match self.device_rect(logical) {
            Some(rect) => clip::intersect(self.clip.clone(), rect),
            None => Some(Vec::new()),
        };
    }

    /// EXCLUDECLIPRECT: this logical rectangle is removed from the clip.
    pub fn exclude_clip_rect(&mut self, logical: [f64; 4]) {
        let Some(rect) = self.device_rect(logical) else { return };
        self.clip = clip::exclude(self.clip.clone(), rect, self.surface);
    }

    /// EXTSELECTCLIPRGN: combine these logical rectangles with the clip in a `RGN_*` mode.
    pub fn select_clip_rects(&mut self, logical: &[[f64; 4]], mode: u32) {
        self.clip = clip::combine(self.clip.clone(), self.map_rects(logical), mode, self.surface);
    }

    /// The default clip: later records are not limited by a region.
    pub fn reset_clip(&mut self) {
        self.clip = None;
    }

    /// Stores an object in `slot` (EMF index), or in the first free slot (WMF handle table).
    pub fn add_object(&mut self, slot: Option<usize>, obj: Obj) {
        // A pattern brush's pixels count against the byte budget like drawn items.
        if let Obj::Brush(Brush { pattern: Some(p), .. }) = &obj {
            let cost = p.px.len();
            if self.used.saturating_add(cost) > MAX_BYTES {
                self.full = true;
                return;
            }
            self.used += cost;
        }
        match slot {
            Some(i) if i < MAX_OBJECTS => {
                if self.objects.len() <= i {
                    self.objects.resize(i + 1, None);
                }
                if let Some(s) = self.objects.get_mut(i) {
                    *s = Some(obj);
                }
            }
            Some(_) => {}
            None => match self.objects.iter().position(Option::is_none) {
                Some(i) => {
                    if let Some(s) = self.objects.get_mut(i) {
                        *s = Some(obj);
                    }
                }
                None if self.objects.len() < MAX_OBJECTS => self.objects.push(Some(obj)),
                None => {}
            },
        }
    }

    pub fn delete_object(&mut self, i: u32) {
        if let Some(s) = self.objects.get_mut(i as usize) {
            *s = None;
        }
    }

    /// Selects an object by index; indices with the high bit set are stock objects.
    pub fn select(&mut self, i: u32) {
        if i & STOCK != 0 {
            return self.select_stock(i & !STOCK);
        }
        match self.objects.get(i as usize).cloned().flatten() {
            Some(Obj::Pen(p)) => self.pen = p,
            Some(Obj::Brush(b)) => self.brush = b,
            Some(Obj::Font(f)) => self.text.font = Some(f),
            Some(Obj::Other) | None => {}
        }
    }

    fn select_stock(&mut self, i: u32) {
        let white = [255, 255, 255, 255];
        let black = [0, 0, 0, 255];
        let gray = |v: u8| Brush::new(BS_SOLID, [v, v, v, 255]);
        match i {
            0 | 18 => self.brush = Brush::new(BS_SOLID, white),
            1 => self.brush = gray(192),
            2 => self.brush = gray(128),
            3 => self.brush = gray(64),
            4 => self.brush = Brush::new(BS_SOLID, black),
            5 => self.brush = Brush::new(BS_NULL, black),
            6 => self.pen = Pen { style: 0, width: 0.0, color: white },
            7 | 19 => self.pen = Pen { style: 0, width: 0.0, color: black },
            8 => self.pen = Pen { style: PS_NULL, width: 0.0, color: black },
            // The stock fonts (OEM_FIXED_FONT … DEFAULT_GUI_FONT): drawn in the default face.
            10..=17 => self.text.font = None,
            _ => {}
        }
    }

    fn map_op(&self, op: Op) -> Op {
        match op {
            Op::M(p) => Op::M(self.map.device(p)),
            Op::L(p) => Op::L(self.map.device(p)),
            Op::C(a, b, c) => Op::C(self.map.device(a), self.map.device(b), self.map.device(c)),
            Op::Z => Op::Z,
        }
    }

    /// The current brush's paint. Hatch and monochrome brushes take the background colour (when
    /// the background mode is opaque) and the text colour. Under R2_MASKPEN (destination AND
    /// brush) a tile's white pixels keep what is below, and its black ones take the colour of the
    /// XOR fill before it: XOR, AND, XOR again leaves that colour where the tile is black.
    fn fill_paint(&self) -> Option<Paint> {
        let b = &self.brush;
        match b.style {
            BS_SOLID => Some(Paint::Solid(b.color)),
            BS_HATCHED => {
                let bg = self.text.bk_opaque.then_some(self.text.bk_color);
                let px = hatch_tile(b.hatch, b.color, bg)?;
                Some(Paint::Tile(Arc::new(RawTile { w: 8, h: 8, px, cell: self.pattern_px })))
            }
            BS_PATTERN | BS_DIBPATTERN | BS_DIBPATTERNPT => {
                let p = b.pattern.as_ref()?;
                if !p.mono && self.rop2 != R2_MASKPEN {
                    let cell = self.pattern_px;
                    return Some(Paint::Tile(p.plain.get_or_init(|| Arc::new(RawTile { w: p.w, h: p.h, px: p.px.clone(), cell })).clone()));
                }
                let mut px = p.px.clone();
                for c in px.as_chunks_mut::<4>().0 {
                    if p.mono {
                        let light = u16::from(c[0]) + u16::from(c[1]) + u16::from(c[2]) >= 384;
                        let v = if light { self.text.bk_color } else { self.text.color };
                        *c = [v[0], v[1], v[2], 255];
                    }
                    if self.rop2 == R2_MASKPEN {
                        if c[..3] == [255, 255, 255] {
                            c[3] = 0;
                        } else if c[..3] == [0, 0, 0]
                            && let Some(x) = self.xor_color
                        {
                            *c = x;
                        }
                    }
                }
                Some(Paint::Tile(Arc::new(RawTile { w: p.w, h: p.h, px, cell: self.pattern_px })))
            }
            _ => None,
        }
    }

    /// Pushes device ops filled with `fill` and stroked with `stroke`: a tile fill and its outline
    /// are two items.
    fn push_shape(&mut self, ops: Vec<Op>, fill: Option<Paint>, stroke: Option<([u8; 4], f64)>) {
        let even_odd = self.even_odd;
        match fill {
            Some(Paint::Tile(tile)) => {
                if stroke.is_some() {
                    self.push(Raw::Pattern { ops: ops.clone(), even_odd, tile, region: None });
                    self.push(Raw::Path { ops, fill: None, stroke, even_odd, region: None });
                } else {
                    self.push(Raw::Pattern { ops, even_odd, tile, region: None });
                }
            }
            Some(Paint::Solid(c)) => self.push(Raw::Path { ops, fill: Some(c), stroke, even_odd, region: None }),
            None if stroke.is_some() => self.push(Raw::Path { ops, fill: None, stroke, even_odd, region: None }),
            None => {}
        }
    }

    /// Whether the mix mode suppresses drawing: R2_NOP draws nothing, and R2_XORPEN shapes only
    /// remember their fill colour (an XOR pair cancels; the see-through shadow idiom draws its
    /// pattern in that colour).
    fn mix_skips(&mut self, fill: bool) -> bool {
        match self.rop2 {
            R2_NOP => true,
            R2_XORPEN => {
                if fill && self.brush.style == BS_SOLID {
                    self.xor_color = Some(self.brush.color);
                }
                true
            }
            _ => false,
        }
    }

    fn stroke_pen(&self) -> Option<([u8; 4], f64)> {
        if (self.pen.style & PS_STYLE_MASK) == PS_NULL {
            return None;
        }
        // A cosmetic pen (width 0) stays 0: the renderers draw that as a hairline.
        let w = if self.pen.width == 0.0 { 0.0 } else { self.pen.width.abs() * self.map.scale() };
        Some((self.pen.color, w))
    }

    fn push_path_ops(&mut self, ops: impl IntoIterator<Item = Op>) {
        for op in ops {
            if self.path.len() >= MAX_PATH_OPS {
                return;
            }
            self.path.push(op);
        }
    }

    /// Draws logical-space ops with the current pen (`stroke`) and brush (`fill`), or appends them to
    /// the open path bracket, where fill and stroke wait for FILLPATH/STROKEPATH.
    pub fn shape(&mut self, ops: &[Op], fill: bool, stroke: bool) {
        let dev: Vec<Op> = ops.iter().map(|o| self.map_op(*o)).collect();
        if self.in_path {
            return self.push_path_ops(dev);
        }
        if self.mix_skips(fill) || dev.len() < 2 {
            return;
        }
        let fill = if fill { self.fill_paint() } else { None };
        let stroke = if stroke { self.stroke_pen() } else { None };
        self.push_shape(dev, fill, stroke);
    }

    pub fn move_to(&mut self, p: P) {
        if self.in_path {
            self.push_path_ops([Op::M(self.map.device(p))]);
        }
        self.pos = p;
    }

    /// LINETO from the current point, then moves the current point to `p`.
    pub fn line_to(&mut self, p: P) {
        self.line_chain(&[p], false, true);
    }

    /// Lines from the current point through `pts` (POLYLINETO), or a polyline from the current point.
    pub fn line_chain(&mut self, pts: &[P], fill: bool, stroke: bool) {
        // Inside an open bracket the path already ends at the current point, so no MoveTo is needed.
        let mut ops = if self.in_path && !self.path.is_empty() { Vec::new() } else { vec![Op::M(self.pos)] };
        ops.extend(pts.iter().map(|p| Op::L(*p)));
        if let Some(last) = pts.last() {
            self.pos = *last;
        }
        self.shape(&ops, fill, stroke);
    }

    /// POLYBEZIER: cubic segments that start a new figure at `from`. The current point is unchanged.
    pub fn bezier(&mut self, from: P, pts: &[P]) {
        let mut ops = vec![Op::M(from)];
        ops.extend(cubics(pts));
        self.shape(&ops, false, true);
    }

    /// POLYBEZIERTO: cubic segments from the current point through groups of three control or end
    /// points; the current point moves to the last point. Inside an open bracket the path already
    /// ends at the current point, so no MoveTo is needed.
    pub fn bezier_to(&mut self, pts: &[P]) {
        let mut ops = if self.in_path && !self.path.is_empty() { Vec::new() } else { vec![Op::M(self.pos)] };
        ops.extend(cubics(pts));
        if let Some(last) = pts.last() {
            self.pos = *last;
        }
        self.shape(&ops, false, true);
    }

    pub fn poly(&mut self, pts: &[P], close: bool, fill: bool, stroke: bool) {
        let Some(first) = pts.first() else { return };
        let mut ops = vec![Op::M(*first)];
        ops.extend(pts.iter().skip(1).map(|p| Op::L(*p)));
        if close {
            ops.push(Op::Z);
        }
        self.shape(&ops, fill, stroke);
    }

    pub fn begin_path(&mut self) {
        self.in_path = true;
        self.path.clear();
    }

    pub fn end_path(&mut self) {
        self.in_path = false;
    }

    pub fn close_figure(&mut self) {
        if self.in_path {
            self.push_path_ops([Op::Z]);
        }
    }

    /// FILLPATH, STROKEPATH and STROKEANDFILLPATH: paints the bracketed path and empties it.
    pub fn paint_path(&mut self, fill: bool, stroke: bool) {
        let ops = std::mem::take(&mut self.path);
        if self.mix_skips(fill) || ops.len() < 2 {
            return;
        }
        let fill = if fill { self.fill_paint() } else { None };
        let stroke = if stroke { self.stroke_pen() } else { None };
        self.push_shape(ops, fill, stroke);
    }

    /// Fills a logical rectangle `[l, t, r, b]` with the current brush (PATBLT, PATCOPY blits).
    pub fn fill_brush_rect(&mut self, [l, t, r, b]: [f64; 4]) {
        if self.mix_skips(true) {
            return;
        }
        let fill = self.fill_paint();
        let dev: Vec<Op> = crate::geom::rect(l, t, r, b).iter().map(|o| self.map_op(*o)).collect();
        self.push_shape(dev, fill, None);
    }

    /// Fills a logical rectangle `[l, t, r, b]` with a solid colour (ROP fills such as BLACKNESS).
    pub fn fill_rect_with(&mut self, [l, t, r, b]: [f64; 4], color: [u8; 4]) {
        let dev: Vec<Op> = crate::geom::rect(l, t, r, b).iter().map(|o| self.map_op(*o)).collect();
        self.push(Raw::Path { ops: dev, fill: Some(color), stroke: None, even_odd: true, region: None });
    }

    /// Whether a raster operation draws without a source bitmap (PATCOPY, BLACKNESS, WHITENESS).
    pub fn sourceless(rop: u32) -> bool {
        matches!(rop, PATCOPY | BLACKNESS | WHITENESS)
    }

    /// Fills a logical rectangle `[x, y, w, h]` for a source-less raster operation. PATCOPY uses the
    /// brush (when solid); the others fill black or white. Other operations are not handled here.
    pub fn fill_rop(&mut self, [x, y, w, h]: [f64; 4], rop: u32) {
        let rect = [x, y, x + w, y + h];
        match rop {
            PATCOPY => self.fill_brush_rect(rect),
            BLACKNESS => self.fill_rect_with(rect, [0, 0, 0, 255]),
            WHITENESS => self.fill_rect_with(rect, [255, 255, 255, 255]),
            _ => {}
        }
    }

    /// Draws `text` (one `dx` advance per character, or none) with the current font, colours and
    /// alignment. The reference point is `at`, or the current position under TA_UPDATECP (which
    /// then moves past the string). `rect` (logical `[l, t, r, b]`) is filled with the background
    /// colour under ETO_OPAQUE and clips the text under ETO_CLIPPED. Text ignores the mix mode, as
    /// GDI's does.
    pub fn text(&mut self, at: P, text: String, dx: Vec<f32>, options: u32, rect: Option<[f64; 4]>) {
        if let (true, Some(r)) = (options & ETO_OPAQUE != 0, rect) {
            self.fill_rect_with(r, self.text.bk_color);
        }
        let lf = self.text.font.as_deref().cloned().unwrap_or_default();
        let update = self.text.align & TA_UPDATECP != 0;
        let origin = if update { self.pos } else { at };
        let (h_align, v_align) = align(self.text.align);
        let o = self.map.device(origin);
        let ux = self.map.device((origin.0 + 1.0, origin.1));
        let uy = self.map.device((origin.0, origin.1 + 1.0));
        let (ux, uy) = ((ux.0 - o.0, ux.1 - o.1), (uy.0 - o.0, uy.1 - o.1));
        if update {
            // The current position moves along the baseline past the string (back past it when
            // right-aligned). Without a dx array the advance is estimated from the height.
            let adv: f64 = if dx.is_empty() {
                f64::from(lf.font.height.abs().max(1.0)) * 0.5 * text.chars().count() as f64
            } else {
                dx.iter().map(|d| f64::from(*d)).sum()
            };
            let adv = match h_align {
                crate::HAlign::Left => adv,
                crate::HAlign::Right => -adv,
                crate::HAlign::Center => 0.0,
            };
            let theta = (lf.escapement / 10.0).to_radians();
            let y_sign = if uy.1 < 0.0 { 1.0 } else { -1.0 };
            let p = (origin.0 + adv * theta.cos(), origin.1 + adv * theta.sin() * y_sign);
            if p.0.is_finite() && p.1.is_finite() {
                self.pos = p;
            }
        }
        let (sx, sy) = (ux.0.hypot(ux.1), uy.0.hypot(uy.1));
        let drawable = !text.is_empty() && [sx, sy, o.0, o.1].iter().all(|v| v.is_finite()) && sx > 0.0 && sy > 0.0;
        if !drawable {
            return;
        }
        // The baseline runs along the device direction of the logical x axis (rotated by a world
        // transform; a mirroring mapping does not mirror glyphs), turned counter-clockwise by the
        // escapement.
        let det = ux.0 * uy.1 - ux.1 * uy.0;
        let phi = if det > 0.0 { ux.1.atan2(ux.0) } else { ux.1.atan2(ux.0.abs()) };
        let a = phi - (lf.escapement / 10.0).to_radians();
        let (c, s) = (a.cos(), a.sin());
        let clip = match (options & ETO_CLIPPED != 0, rect) {
            (true, Some([l, t, r, b])) => {
                let (p, q) = (self.map.device((l, t)), self.map.device((r, b)));
                Some([p.0.min(q.0) as f32, p.1.min(q.1) as f32, p.0.max(q.0) as f32, p.1.max(q.1) as f32])
            }
            _ => None,
        };
        self.push(Raw::Text {
            text: Box::new(Text {
                transform: [sx * c, sx * s, -sy * s, sy * c, o.0, o.1],
                text,
                dx,
                font: lf.font,
                color: self.text.color,
                background: self.text.bk_opaque.then_some(self.text.bk_color),
                h_align,
                v_align,
                clip,
                region: None,
            }),
            region: None,
        });
    }

    /// A bitmap stretched into a logical rectangle `[x, y, w, h]`. A negative extent mirrors the
    /// bitmap, as the mapping does.
    pub fn bitmap(&mut self, rect: [f64; 4], mut img: Rgba) {
        let [x, y, w, h] = rect;
        let a = self.map.device((x, y));
        let b = self.map.device((x + w, y + h));
        crate::dib::flip(&mut img, a.0 > b.0, a.1 > b.1);
        let dev = [a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs()];
        self.push(Raw::Bitmap { rect: dev, img, region: None });
    }
}

/// An 8×8 hatch tile (HS_HORIZONTAL, HS_VERTICAL, HS_FDIAGONAL, HS_BDIAGONAL, HS_CROSS,
/// HS_DIAGCROSS): one-pixel lines in `fg` over `bg`, or over nothing when the background is
/// transparent. None for an unknown style.
pub(crate) fn hatch_tile(style: u32, fg: [u8; 4], bg: Option<[u8; 4]>) -> Option<Vec<u8>> {
    if style > 5 {
        return None;
    }
    let mut px = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8u32 {
        for x in 0..8u32 {
            let (h, v, f, b) = (y == 7, x == 7, x == y, x + y == 7);
            let on = match style {
                0 => h,
                1 => v,
                2 => f,
                3 => b,
                4 => h || v,
                _ => f || b,
            };
            px.extend(if on { fg } else { bg.unwrap_or([0, 0, 0, 0]) });
        }
    }
    Some(px)
}

/// Cubic segments from groups of three points (control, control, end). A trailing partial group is
/// dropped.
fn cubics(pts: &[P]) -> Vec<Op> {
    let mut ops = Vec::with_capacity(pts.len() / 3);
    let mut it = pts.iter().copied();
    while let (Some(a), Some(b), Some(e)) = (it.next(), it.next(), it.next()) {
        ops.push(Op::C(a, b, e));
    }
    ops
}
