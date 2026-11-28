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
    Some(Mask::from_parts(alphas(w, h, bounds, reveal), w, h))
}

/// The mask's bytes, row by row. The reveal is evaluated only on the pixels inside `bounds`; what
/// lies outside (the shape's shadow or glow, say) shows as the nearest point of the box does, so a
/// small shape on a big canvas costs its own area and not the canvas's.
fn alphas(w: u16, h: u16, bounds: Rect, reveal: &Reveal) -> Vec<u8> {
    let (w, h) = (usize::from(w), usize::from(h));
    // A box thinner than a pixel (a line) is widened around its middle.
    let (bw, bh) = (bounds.width().max(1.0), bounds.height().max(1.0));
    let (x0, y0) = ((bounds.x0 + bounds.x1) / 2.0 - bw / 2.0, (bounds.y0 + bounds.y1) / 2.0 - bh / 2.0);
    // The columns c0..c1 whose middle is inside the box.
    let c0 = ((x0 - 0.5).floor() + 1.0).clamp(0.0, w as f64) as usize;
    let c1 = ((x0 + bw - 0.5).ceil()).clamp(c0 as f64, w as f64) as usize;
    let us: Vec<f64> = (c0..c1).map(|px| (px as f64 + 0.5 - x0) / bw).collect();
    let mut data = vec![0u8; w * h];
    // The row last computed, with its v: the rows above and below the box share the box's edge's.
    let mut last: Option<(f64, usize)> = None;
    for py in 0..h {
        let v = ((py as f64 + 0.5 - y0) / bh).clamp(0.0, 1.0);
        let start = py * w;
        if let Some((lv, ls)) = last
            && lv == v
        {
            data.copy_within(ls..ls + w, start);
            continue;
        }
        let row = &mut data[start..start + w];
        let (mut left, mut right) = ([0u8], [0u8]);
        reveal.row_alpha(v, &[0.0], &mut left);
        reveal.row_alpha(v, &[1.0], &mut right);
        row[..c0].fill(left[0]);
        row[c1..].fill(right[0]);
        reveal.row_alpha(v, &us, &mut row[c0..c1]);
        last = Some((v, start));
    }
    data
}
