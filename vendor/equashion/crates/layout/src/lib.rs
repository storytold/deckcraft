//! equaSHion layout: TeX-style layout of equations into a backend-neutral [`Scene`], plus the
//! caret geometry an editor needs ([`EqLayout`]).
//!
//! Fonts come in through the [`MathFonts`] trait (implemented for
//! [`equashion_font::FontDb`]), never from a process-wide database, so a host chooses which
//! fonts it offers and tests stay deterministic with [`FontDb::bundled_only`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod math;
mod scene;

use std::sync::Arc;

pub use equashion_font::{DEFAULT_FAMILY, FaceId, FontDb, FontFace};
pub use kurbo::{BezPath, Rect};
pub use math::{
    Cached, EqLayout, LayoutCache, MathBox, SeqGeo, Src, emit, hit_geo, layout_equation, layout_equation_wrapped, layout_math, layout_math_wrapped,
    layout_src,
};
pub use scene::{Prim, Rgb, Scene};

/// The fonts equation layout reads: faces by family and style, a fallback face for characters
/// a face lacks, and glyph outlines (in font units, y down). Backends also resolve the faces a
/// [`Scene`] names with [`MathFonts::face_by_id`].
pub trait MathFonts {
    /// The face for `family` and `style` ("Regular", "Italic", "Bold", "Bold Italic"), or the
    /// closest one the implementation has.
    fn face(&self, family: &str, style: &str) -> Arc<FontFace>;
    /// A face that covers `c`, other than the face with id `exclude`.
    fn fallback_for(&self, c: char, exclude: u32) -> Option<Arc<FontFace>>;
    /// The outline of glyph `gid` of `f`, in font units, y down.
    fn outline(&self, f: &FontFace, gid: u32) -> Arc<BezPath>;
    /// The face a [`Prim::Glyphs`] names.
    fn face_by_id(&self, id: FaceId) -> Option<Arc<FontFace>>;
}

impl MathFonts for FontDb {
    fn face(&self, family: &str, style: &str) -> Arc<FontFace> {
        FontDb::face(self, family, style)
    }
    fn fallback_for(&self, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        FontDb::fallback_for(self, c, exclude)
    }
    fn outline(&self, f: &FontFace, gid: u32) -> Arc<BezPath> {
        FontDb::outline(self, f, gid)
    }
    fn face_by_id(&self, id: FaceId) -> Option<Arc<FontFace>> {
        FontDb::face_by_id(self, id)
    }
}

#[cfg(test)]
mod tests;
