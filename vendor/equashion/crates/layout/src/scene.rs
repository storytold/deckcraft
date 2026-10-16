//! The backend-neutral output of equation layout: a list of filled primitives in points, y down,
//! origin at the top-left corner. `equashion-render` turns it into pixels or SVG; a web or egui
//! host can draw it directly.

use equashion_font::FaceId;

/// An sRGB colour, 8 bits per channel (the layout's colour; WordCraft took it from its
/// document model).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(255, 255, 255);
}

/// A laid-out equation (or anything else drawn from these primitives).
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub items: Vec<Prim>,
    /// Canvas size, points.
    pub width: f64,
    pub height: f64,
}

/// One filled primitive.
#[derive(Clone, Debug)]
pub enum Prim {
    /// Glyphs of one face at one size. Each glyph is `(glyph id, x, baseline y)` in points.
    /// `synth_bold` asks the backend to embolden (stroke the outline as well as filling it),
    /// `synth_italic` to slant it (the face has no real bold or italic).
    Glyphs { font: FaceId, size: f32, glyphs: Vec<(u32, f32, f32)>, color: Rgb, synth_bold: bool, synth_italic: bool },
    /// A filled rectangle: fraction bars, overbars, box borders, bracket strokes.
    Rule(kurbo::Rect, Rgb),
    /// A filled shape (non-zero winding): radical signs, stretched delimiters, big operators.
    Path(kurbo::BezPath, Rgb),
}
