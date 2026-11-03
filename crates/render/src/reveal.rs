//! The mask of a soft edged reveal: Box, Circle, Diamond, Plus, Wipe and Split entrance effects.

use deckcraft_anim::Reveal;
use kurbo::Rect;
use vello_cpu::Mask;

/// Most mask pixels one shape gets; a bigger canvas fades the shape by the reveal at its middle.
const MAX_PIXELS: u64 = 1 << 24;

/// The mask for a canvas of `w` x `h` pixels: how much of the shape shows at each pixel, with
/// `bounds` (device pixels, on the slide's axes) the box the reveal is measured on. `None` when the
/// canvas is too big for a mask.
pub(crate) fn mask(w: u16, h: u16, bounds: Rect, reveal: &Reveal) -> Option<Mask> {
    if u64::from(w) * u64::from(h) > MAX_PIXELS || !bounds.is_finite() {
        return None;
    }
    // A box thinner than a pixel (a line) is widened around its middle.
    let (bw, bh) = (bounds.width().max(1.0), bounds.height().max(1.0));
    let (x0, y0) = ((bounds.x0 + bounds.x1) / 2.0 - bw / 2.0, (bounds.y0 + bounds.y1) / 2.0 - bh / 2.0);
    let mut data = Vec::with_capacity(usize::from(w) * usize::from(h));
    for py in 0..h {
        let v = (f64::from(py) + 0.5 - y0) / bh;
        data.extend((0..w).map(|px| {
            let u = (f64::from(px) + 0.5 - x0) / bw;
            (reveal.alpha(u, v) * 255.0).round() as u8
        }));
    }
    Some(Mask::from_parts(data, w, h))
}
