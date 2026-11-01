//! Bounded alpha masks for picture glows. A max filter grows the coverage without
//! adding translucent samples together; the renderer applies the existing blur afterwards.

use std::collections::VecDeque;
use std::sync::Arc;

use deckcraft_color::Rgba;
use kurbo::{Affine, Rect};
use vello_cpu::Pixmap;

use crate::raster::Image;

/// Raster coverage retained by one frame (64 MiB of final RGBA masks).
pub(crate) const FRAME_PIXELS: usize = 16 * 1024 * 1024;
pub(crate) const MIN_PIXELS: usize = 25;
const MAX_SIDE: usize = 2048;
const PICTURE_PIXELS: usize = 4 * 1024 * 1024;
// Match the backend's maximum Gaussian sigma, converted to the glow's radius.
const MAX_DEVICE_RADIUS: f64 = 500.0 / 0.17;

pub(crate) struct Plan {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) to_mask: Affine,
    pub(crate) to_device: Affine,
    pub(crate) blur_sigma: f64,
    axis: usize,
    diagonal: usize,
}

impl Plan {
    /// Grow coverage in shape space, then apply the original affine. This keeps the glow's
    /// solid reach under nonuniform group scaling, while its blur remains in device pixels.
    pub(crate) fn new(m: Affine, bounds: Rect, viewport: Rect, radius: f64, remaining: usize) -> Option<Self> {
        let positive = |r: Rect| r.is_finite() && r.width() > 0.0 && r.height() > 0.0;
        if !m.is_finite()
            || !m.inverse().is_finite()
            || !positive(bounds)
            || !positive(viewport)
            || !radius.is_finite()
            || radius <= 0.0
            || remaining < MIN_PIXELS
        {
            return None;
        }
        let c = m.as_coeffs();
        let pixel_scale = c[0].hypot(c[1]).max(c[2].hypot(c[3]));
        let k = m.determinant().abs().sqrt();
        if !pixel_scale.is_finite() || pixel_scale <= 0.0 || !k.is_finite() || k <= 0.0 {
            return None;
        }
        let radius = radius.min(MAX_DEVICE_RADIUS / pixel_scale);
        let grow = radius * 0.475;
        let blur_sigma = (radius * k * 0.17).min(500.0);
        let device = m.transform_rect_bbox(bounds);
        if !device.is_finite() {
            return None;
        }
        // Twice the nominal growth also covers octagonal/rounded pixel support. The entire
        // picture is rasterized (with a hard size cap), so no distant alpha edge is cropped.
        let reach = 2.0 * grow * pixel_scale + 3.0 * blur_sigma + 2.0;
        if !positive(device.inflate(reach, reach).intersect(viewport)) {
            return None;
        }
        let (w, h) = (bounds.width(), bounds.height());
        let (total_w, total_h) = (w + 2.0 * grow, h + 2.0 * grow);
        if !total_w.is_finite() || !total_h.is_finite() {
            return None;
        }
        let pixels = remaining.min(PICTURE_PIXELS);
        let mut scale = pixel_scale.min((MAX_SIDE - 4) as f64 / total_w.max(total_h)).min((pixels as f64 / (total_w * total_h)).sqrt());
        for _ in 0..12 {
            if !scale.is_finite() || scale <= 0.0 {
                return None;
            }
            let grown = grow * scale;
            let (cw, ch) = ((w * scale).ceil().max(1.0), (h * scale).ceil().max(1.0));
            if !grown.is_finite() || grown > MAX_SIDE as f64 || !cw.is_finite() || !ch.is_finite() || cw > MAX_SIDE as f64 || ch > MAX_SIDE as f64 {
                return None;
            }
            // All float-to-integer conversions below are finite, nonnegative and bounded by
            // MAX_SIDE. Integer padding follows the *rounded* support, even after downsampling.
            let axis = (grown * (std::f64::consts::SQRT_2 - 1.0)).round() as usize;
            let diagonal = (grown * (1.0 - std::f64::consts::FRAC_1_SQRT_2)).round() as usize;
            let pad = axis.checked_add(diagonal.checked_mul(2)?)?.checked_add(2)?;
            let width = (cw as usize).checked_add(pad.checked_mul(2)?)?;
            let height = (ch as usize).checked_add(pad.checked_mul(2)?)?;
            let count = width.checked_mul(height)?;
            if width <= MAX_SIDE && height <= MAX_SIDE && count <= pixels {
                let origin = bounds.origin().to_vec2() - kurbo::Vec2::new(pad as f64 / scale, pad as f64 / scale);
                let to_mask = Affine::scale(scale) * Affine::translate(-origin);
                let to_device = m * Affine::translate(origin) * Affine::scale(1.0 / scale);
                if !to_mask.is_finite() || !to_device.is_finite() || !(to_mask * m.inverse()).is_finite() {
                    return None;
                }
                return Some(Self { width: width as u16, height: height as u16, to_mask, to_device, blur_sigma, axis, diagonal });
            }
            let fit = (MAX_SIDE as f64 / width.max(height) as f64).min((pixels as f64 / count as f64).sqrt());
            scale *= fit.min(0.95) * 0.95;
        }
        None
    }

    pub(crate) fn pixels(&self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }

    pub(crate) fn tint(&self, image: Image, color: Rgba) -> Option<Arc<Pixmap>> {
        let count = self.pixels();
        if image.width != u32::from(self.width) || image.height != u32::from(self.height) || image.pixels.len() != count.checked_mul(4)? {
            return None;
        }
        let mut alpha: Vec<u8> = image.pixels.as_chunks::<4>().0.iter().map(|p| p.get(3).copied().unwrap_or(0)).collect();
        if alpha.iter().all(|a| *a == 0) {
            return None;
        }
        let mut out = vec![0; count];
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        for direction in 0..4 {
            let radius = if direction < 2 { self.axis } else { self.diagonal };
            if radius != 0 {
                pass(&alpha, &mut out, w, h, direction, radius)?;
                std::mem::swap(&mut alpha, &mut out);
            }
        }
        let mut pixmap = Pixmap::new(self.width, self.height);
        for (pixel, a) in pixmap.data_as_u8_slice_mut().as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
            let premul = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
            pixel.copy_from_slice(&[premul(color.r), premul(color.g), premul(color.b), a]);
        }
        Some(Arc::new(pixmap))
    }
}

/// Horizontal, vertical, and the two diagonal sets of disjoint lines cover each pixel once.
fn pass(input: &[u8], output: &mut [u8], w: usize, h: usize, direction: usize, radius: usize) -> Option<()> {
    match direction {
        0 => {
            for y in 0..h {
                line(input, output, y.checked_mul(w)?, 1, w, radius)?;
            }
        }
        1 => {
            for x in 0..w {
                line(input, output, x, w, h, radius)?;
            }
        }
        2 => {
            for x in 0..w {
                line(input, output, x, w.checked_add(1)?, (w - x).min(h), radius)?;
            }
            for y in 1..h {
                line(input, output, y.checked_mul(w)?, w.checked_add(1)?, w.min(h - y), radius)?;
            }
        }
        _ => {
            for x in 0..w {
                line(input, output, x, w.checked_sub(1)?, (x + 1).min(h), radius)?;
            }
            for y in 1..h {
                line(input, output, y.checked_mul(w)?.checked_add(w.checked_sub(1)?)?, w.checked_sub(1)?, w.min(h - y), radius)?;
            }
        }
    }
    Some(())
}

/// A monotonic deque computes one line's moving maximum with each sample queued once.
fn line(input: &[u8], output: &mut [u8], start: usize, step: usize, len: usize, radius: usize) -> Option<()> {
    let mut deque = VecDeque::<(usize, u8)>::new();
    let mut next = 0;
    for at in 0..len {
        let end = at.saturating_add(radius).min(len.saturating_sub(1));
        while next <= end {
            let index = start.checked_add(next.checked_mul(step)?)?;
            let value = *input.get(index)?;
            while deque.back().is_some_and(|(_, v)| *v <= value) {
                deque.pop_back();
            }
            deque.push_back((next, value));
            next += 1;
        }
        let begin = at.saturating_sub(radius);
        while deque.front().is_some_and(|(i, _)| *i < begin) {
            deque.pop_front();
        }
        let index = start.checked_add(at.checked_mul(step)?)?;
        *output.get_mut(index)? = deque.front()?.1;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(m: Affine, bounds: Rect, radius: f64, budget: usize) -> Option<Plan> {
        Plan::new(m, bounds, Rect::new(0.0, 0.0, 960.0, 540.0), radius, budget)
    }

    #[test]
    fn picture_glow_rejects_invalid_inputs_without_allocating() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        for radius in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert!(plan(Affine::IDENTITY, bounds, radius, FRAME_PIXELS).is_none());
        }
        for m in [
            Affine::scale(0.0),
            Affine::scale(f64::NAN),
            Affine::scale(f64::INFINITY),
            Affine::scale(1e200),
            Affine::scale_non_uniform(1e-308, 100.0),
        ] {
            assert!(plan(m, bounds, 10.0, FRAME_PIXELS).is_none());
        }
        for r in [Rect::ZERO, Rect::new(0.0, 0.0, -1.0, 1.0), Rect::new(0.0, 0.0, f64::NAN, 1.0), Rect::new(0.0, 0.0, f64::INFINITY, 1.0)] {
            assert!(plan(Affine::IDENTITY, r, 10.0, FRAME_PIXELS).is_none());
        }
        assert!(Plan::new(Affine::IDENTITY, bounds, Rect::ZERO, 10.0, FRAME_PIXELS).is_none());
        assert!(plan(Affine::translate((1e10, 0.0)), bounds, 10.0, FRAME_PIXELS).is_none());
        for remaining in [0, 1, MIN_PIXELS - 1] {
            assert!(plan(Affine::IDENTITY, bounds, 10.0, remaining).is_none());
        }
    }

    #[test]
    fn picture_glow_caps_large_inputs_and_retains_rounded_support() {
        for (bounds, radius, remaining) in [
            (Rect::new(0.0, 0.0, 1e9, 1e9), 10.0, FRAME_PIXELS),
            (Rect::new(0.0, 0.0, 100.0, 100.0), f64::MAX, 100),
            (Rect::new(0.0, 0.0, 100.0, 100.0), 10.0, MIN_PIXELS),
        ] {
            let p = plan(Affine::IDENTITY, bounds, radius, remaining).unwrap();
            assert!(usize::from(p.width) <= MAX_SIDE && usize::from(p.height) <= MAX_SIDE);
            assert!(p.pixels() <= remaining.min(PICTURE_PIXELS));
            assert!(p.blur_sigma.is_finite() && p.blur_sigma <= 500.0);
            let support = (p.axis + 2 * p.diagonal) as f64;
            let b = p.to_mask.transform_rect_bbox(bounds);
            assert!(b.x0 >= support && b.y0 >= support);
            assert!(b.x1 + support < f64::from(p.width) && b.y1 + support < f64::from(p.height));
        }
    }

    #[test]
    fn picture_glow_max_filter_matches_direct_coverage_on_thin_masks() {
        for (w, h) in [(1, 1), (1, 7), (7, 1), (5, 6)] {
            let input: Vec<u8> = (0..w * h).map(|i| (i * 47 % 251) as u8).collect();
            for (direction, (dx, dy)) in [(1, 0), (0, 1), (1, 1), (-1, 1)].into_iter().enumerate() {
                let mut out = vec![0; input.len()];
                pass(&input, &mut out, w, h, direction, 2).unwrap();
                for y in 0..h {
                    for x in 0..w {
                        let expected = (-2..=2)
                            .filter_map(|offset| {
                                let (nx, ny) = (x as i32 + dx * offset, y as i32 + dy * offset);
                                (nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32).then(|| input[ny as usize * w + nx as usize])
                            })
                            .max()
                            .unwrap();
                        assert_eq!(out[y * w + x], expected, "{w}x{h}, direction {direction}, {x},{y}");
                    }
                }
            }
        }
    }

    #[test]
    fn picture_glow_growth_does_not_accumulate_fractional_alpha() {
        let p = plan(Affine::IDENTITY, Rect::new(0.0, 0.0, 10.0, 10.0), 10.0, FRAME_PIXELS).unwrap();
        let image = Image { width: p.width.into(), height: p.height.into(), pixels: [0, 0, 0, 128].repeat(p.pixels()) };
        let tinted = p.tint(image, Rgba::rgb(200, 100, 50).with_alpha(10)).unwrap();
        assert!(tinted.data_as_u8_slice().as_chunks::<4>().0.iter().all(|p| p == &[100, 50, 25, 128]));
        let clear = Image { width: p.width.into(), height: p.height.into(), pixels: vec![0; p.pixels() * 4] };
        assert!(p.tint(clear, Rgba::WHITE).is_none());
        assert!(p.tint(Image::default(), Rgba::WHITE).is_none());
    }
}
