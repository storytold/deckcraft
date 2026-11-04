//! DeckCraft metafiles (ported from WordCraft's `crates/metafile`): reads Windows Metafiles (WMF)
//! and Enhanced Metafiles (EMF) into a neutral vector model of filled and stroked paths, pattern
//! fills, RGBA bitmaps and text strings, in a picture frame with y down.
//!
//! Text stays font-agnostic: a [`Text`] item names the font the file asks for and carries the
//! string, its advances and alignment; the renderer resolves the font and draws the glyphs.
//!
//! The reader is pure Rust over byte slices (no file system), so it builds for wasm. Every read is
//! bounds-checked and every count is capped; hostile input yields a shorter picture or an [`Error`],
//! never a panic.
//!
//! Records follow MS-WMF and MS-EMF. Rectangle clip regions (`INTERSECTCLIPRECT`,
//! `EXCLUDECLIPRECT`, `EXTSELECTCLIPRGN`) limit what later records draw. Records outside the
//! supported set are skipped silently.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod bytes;
mod canvas;
mod clip;
mod cliprect;
mod dib;
mod emf;
mod emfplus;
mod geom;
mod place;
mod text;
mod wmf;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use canvas::Raw;
use geom::{Op, P};
pub use place::{MAX_BITMAP_SIDE, PlacedItem};
pub use text::{Font, HAlign, SYMBOL_CHARSET, Text, VAlign};

/// Items kept from one file. Past the cap the parse stops and keeps what it has.
pub(crate) const MAX_ITEMS: usize = 200_000;
/// Object table slots (WMF handles, EMF object indices).
pub(crate) const MAX_OBJECTS: usize = 4096;
/// Largest bitmap in pixels (about 40 megapixels, 160 MB of RGBA).
pub(crate) const MAX_PIXELS: usize = 40_000_000;
/// DC save-stack depth.
pub(crate) const MAX_STACK: usize = 1024;
/// Points in one open path bracket.
pub(crate) const MAX_PATH_OPS: usize = 2_000_000;
/// Bytes of path ops and pixels kept from one file (about 256 MB).
pub(crate) const MAX_BYTES: usize = 256 * 1024 * 1024;

/// A parsed metafile. Items use coordinates in `0..width` and `0..height`, y down.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// Size of the drawing in its own units: points for placeable WMF and EMF (from the frame), device
    /// or logical units otherwise.
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A filled and/or stroked path, in picture coordinates.
    Path {
        segs: Vec<Seg>,
        fill: Option<[u8; 4]>,
        /// Colour and width in picture units.
        stroke: Option<([u8; 4], f32)>,
        even_odd: bool,
        /// Clip region in picture units, each rectangle `[x0, y0, x1, y1]`. None is the default clip.
        region: Option<Vec<[f32; 4]>>,
    },
    /// A bitmap stretched into `rect` = `[x, y, w, h]` (picture units), RGBA8 rows top-down.
    Bitmap {
        rect: [f32; 4],
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        /// Clip region in picture units. None is the default clip.
        region: Option<Vec<[f32; 4]>>,
    },
    /// A path filled with a repeating tile (pattern and hatch brushes). Items filled with the same
    /// brush share its tile.
    Pattern {
        segs: Vec<Seg>,
        even_odd: bool,
        tile: Arc<Tile>,
        /// Clip region in picture units. None is the default clip.
        region: Option<Vec<[f32; 4]>>,
    },
    /// A text string.
    Text(Box<Text>),
}

/// A brush tile: RGBA8 rows top-down (transparent where the brush leaves the background), each
/// pixel `cell` picture units, with a tile's top-left corner at `origin`.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub cell: [f32; 2],
    pub origin: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("not a WMF or EMF file")]
    NotMetafile,
    #[error("metafile header is truncated or invalid")]
    BadHeader,
    #[error("metafile has no readable records")]
    NoRecords,
}

/// The picture frame in device units (`rect` = `[x, y, w, h]`) and, when the file states one, its
/// size in picture units.
pub(crate) struct Frame {
    pub rect: [f64; 4],
    pub size: Option<(f64, f64)>,
}

/// Parses a WMF (placeable or plain) or EMF file.
pub fn parse(bytes: &[u8]) -> Result<Picture, Error> {
    let b = Bytes(bytes);
    let (cv, frame) = if emf::is_emf(b) {
        emf::parse(b)?
    } else if wmf::is_wmf(b) {
        // A WMF converted from an EMF can carry the original in MFCOMMENT escapes; PowerPoint draws
        // the WMF's own records (its dx rounding shows), so the copy is not used.
        wmf::parse(b)?
    } else {
        return Err(Error::NotMetafile);
    };
    Ok(build(cv.items, frame))
}

/// True when the bytes start like a WMF (placeable or plain header) or an EMF.
pub fn is_metafile(bytes: &[u8]) -> bool {
    let b = Bytes(bytes);
    emf::is_emf(b) || wmf::is_wmf(b)
}

/// Maps device-space output into the picture frame. Without a usable frame the frame is the bounding
/// box of the drawing, so the origin is still the top-left of what was drawn.
fn build(raw: Vec<Raw>, frame: Option<Frame>) -> Picture {
    let frame = frame.filter(|f| f.rect.iter().all(|v| v.is_finite()) && f.rect[2] > 0.0 && f.rect[3] > 0.0);
    let (rect, size) = match frame {
        Some(f) => (f.rect, f.size.unwrap_or((f.rect[2], f.rect[3]))),
        None => match bbox(&raw) {
            Some([x0, y0, x1, y1]) => ([x0, y0, x1 - x0, y1 - y0], (x1 - x0, y1 - y0)),
            None => return Picture { width: 0.0, height: 0.0, items: Vec::new() },
        },
    };
    let sx = if rect[2] > 0.0 { size.0 / rect[2] } else { 1.0 };
    let sy = if rect[3] > 0.0 { size.1 / rect[3] } else { 1.0 };
    let tf = move |p: P| ((p.0 - rect[0]) * sx, (p.1 - rect[1]) * sy);
    let stroke_scale = (sx + sy) / 2.0;
    // No stroke is wider than the picture's diagonal: anything wider is a mapping artefact.
    let diag = size.0.hypot(size.1);
    // Tiles shared by several fills stay shared.
    let mut tiles: HashMap<*const canvas::RawTile, (Arc<canvas::RawTile>, Arc<Tile>)> = HashMap::new();
    let items = raw.into_iter().filter_map(|r| match r {
        Raw::Path { ops, fill, stroke, even_odd, region } => {
            let segs = ops.iter().map(|op| seg(op, tf)).collect::<Option<Vec<Seg>>>()?;
            if segs.len() < 2 {
                return None;
            }
            let stroke = stroke.and_then(|(c, w)| Some((c, f((w * stroke_scale).min(diag))?)));
            Some(Item::Path { segs, fill, stroke, even_odd, region: map_region(region, &tf)? })
        }
        Raw::Bitmap { rect: r, img, region } => {
            let (x, y) = tf((r[0], r[1]));
            let rect = [f(x)?, f(y)?, f(r[2] * sx)?, f(r[3] * sy)?];
            Some(Item::Bitmap { rect, width: img.w as u32, height: img.h as u32, rgba: img.px, region: map_region(region, &tf)? })
        }
        Raw::Pattern { ops, even_odd, tile, region } => {
            let segs = ops.iter().map(|op| seg(op, tf)).collect::<Option<Vec<Seg>>>()?;
            if segs.len() < 2 {
                return None;
            }
            let key = Arc::as_ptr(&tile);
            let tile = match tiles.get(&key) {
                Some((_, t)) => t.clone(),
                None => {
                    let (ox, oy) = tf((0.0, 0.0));
                    let t = Arc::new(Tile {
                        width: u32::try_from(tile.w).ok()?,
                        height: u32::try_from(tile.h).ok()?,
                        rgba: tile.px.clone(),
                        cell: [f(tile.cell * sx)?, f(tile.cell * sy)?],
                        origin: [f(ox)?, f(oy)?],
                    });
                    // The raw tile is kept with its key, so the address is never reused by another.
                    tiles.insert(key, (tile.clone(), t.clone()));
                    t
                }
            };
            Some(Item::Pattern { segs, even_odd, tile, region: map_region(region, &tf)? })
        }
        Raw::Text { mut text, region } => {
            // Device to picture: scale by (sx, sy) after moving the frame's corner to the origin.
            let [a, b, c, d, e, g] = text.transform;
            let (e, g) = tf((e, g));
            text.transform = [a * sx, b * sy, c * sx, d * sy, e, g];
            if !text.transform.iter().all(|v| v.is_finite()) {
                return None;
            }
            if let Some([x0, y0, x1, y1]) = text.clip {
                let (p, q) = (tf((f64::from(x0), f64::from(y0))), tf((f64::from(x1), f64::from(y1))));
                text.clip = Some([f(p.0.min(q.0))?, f(p.1.min(q.1))?, f(p.0.max(q.0))?, f(p.1.max(q.1))?]);
            }
            text.region = map_region(region, &tf)?;
            Some(Item::Text(text))
        }
    });
    let items = items.collect();
    Picture { width: f(size.0).unwrap_or(0.0), height: f(size.1).unwrap_or(0.0), items }
}

fn map_region(region: Option<Vec<[f64; 4]>>, tf: &impl Fn(P) -> P) -> Option<Option<Vec<[f32; 4]>>> {
    let Some(rects) = region else { return Some(None) };
    let mut out = Vec::with_capacity(rects.len());
    for [x0, y0, x1, y1] in rects {
        let (p, q) = (tf((x0, y0)), tf((x1, y1)));
        out.push([f(p.0.min(q.0))?, f(p.1.min(q.1))?, f(p.0.max(q.0))?, f(p.1.max(q.1))?]);
    }
    Some(Some(out))
}

fn f(v: f64) -> Option<f32> {
    let v = v as f32;
    v.is_finite().then_some(v)
}

fn seg(op: &Op, tf: impl Fn(P) -> P) -> Option<Seg> {
    let pt = |p: P| {
        let (x, y) = tf(p);
        Some((f(x)?, f(y)?))
    };
    Some(match *op {
        Op::M(p) => {
            let (x, y) = pt(p)?;
            Seg::Move(x, y)
        }
        Op::L(p) => {
            let (x, y) = pt(p)?;
            Seg::Line(x, y)
        }
        Op::C(a, b, c) => {
            let (x1, y1) = pt(a)?;
            let (x2, y2) = pt(b)?;
            let (x3, y3) = pt(c)?;
            Seg::Cubic(x1, y1, x2, y2, x3, y3)
        }
        Op::Z => Seg::Close,
    })
}

/// Bounding box `[x0, y0, x1, y1]` over the finite points of the output.
fn bbox(raw: &[Raw]) -> Option<[f64; 4]> {
    let mut b: Option<[f64; 4]> = None;
    let mut grow = |p: P| {
        if p.0.is_finite() && p.1.is_finite() {
            b = Some(match b {
                None => [p.0, p.1, p.0, p.1],
                Some([x0, y0, x1, y1]) => [x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1)],
            });
        }
    };
    for r in raw {
        match r {
            Raw::Path { ops, .. } | Raw::Pattern { ops, .. } => {
                for op in ops {
                    match *op {
                        Op::M(p) | Op::L(p) => grow(p),
                        Op::C(a, c, d) => {
                            grow(a);
                            grow(c);
                            grow(d);
                        }
                        Op::Z => {}
                    }
                }
            }
            Raw::Bitmap { rect, .. } => {
                grow((rect[0], rect[1]));
                grow((rect[0] + rect[2], rect[1] + rect[3]));
            }
            Raw::Text { text: t, .. } => grow((t.transform[4], t.transform[5])),
        }
    }
    b
}
