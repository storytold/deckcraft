//! A Harvey ball's shapes: a background disc, a `pie` wedge from 12 o'clock and a ring.

use deckcraft_geom::Xfrm;
use deckcraft_model::{Fill, Geom, HarveyBall, Line, Shape};

/// 12 o'clock in DrawingML angles: 60000ths of a degree, clockwise from 3 o'clock.
pub const TWELVE: u32 = 16_200_000;
/// A full turn in DrawingML angles.
pub const TURN: u32 = 21_600_000;

/// The wedge's end angle (`pie` preset `adj2`) for `value` parts of `of`, from 12 o'clock
/// clockwise: `(TWELVE + TURN·value/of) mod TURN`. Integer maths, so thirds, eighths and tenths are
/// exact; a share that isn't a whole number of 60000ths of a degree rounds to the nearest.
/// `None` when `of` is 0.
pub fn wedge_end(value: u16, of: u16) -> Option<u32> {
    let (value, of) = (u64::from(value), u64::from(of));
    if of == 0 {
        return None;
    }
    let sweep = (u64::from(TURN) * value * 2 + of) / (2 * of);
    u32::try_from((u64::from(TWELVE) + sweep) % u64::from(TURN)).ok()
}

/// The ball's shapes inside square `sq` (child space), in z-order, with id 0: background, wedge
/// (left out at 0, a full disc at `of`), ring (left out without an outline).
pub(super) fn shapes(b: &HarveyBall, sq: Xfrm) -> Result<Vec<Shape>, String> {
    b.check()?;
    let disc = |name: &str, geom: Geom, fill: Fill, line: Line| Shape {
        name: name.into(),
        xfrm: Some(sq),
        geom,
        fill: Some(fill),
        line: Some(line),
        ..Default::default()
    };
    let mut v = Vec::with_capacity(3);
    v.push(disc("Background", Geom::preset("ellipse"), b.background.clone().map_or(Fill::None, Fill::solid), Line::none()));
    if b.value == b.of {
        v.push(disc("Wedge", Geom::preset("ellipse"), Fill::solid(b.fill.clone()), Line::none()));
    } else if b.value > 0 {
        let end = wedge_end(b.value, b.of).ok_or("`of` must be at least 1")?;
        let geom = Geom::Preset { name: "pie".into(), adj: vec![f64::from(TWELVE), f64::from(end)] };
        v.push(disc("Wedge", geom, Fill::solid(b.fill.clone()), Line::none()));
    }
    if let Some(c) = &b.outline {
        v.push(disc("Outline", Geom::preset("ellipse"), Fill::None, Line::solid(c.clone(), b.outline_width)));
    }
    Ok(v)
}
