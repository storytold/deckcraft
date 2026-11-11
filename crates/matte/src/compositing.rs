//! The four PowerPoint camera-background modes, applied to one RGBA frame.
//!
//! Normal returns the frame untouched. Removed keeps the subject and makes the rest transparent.
//! Blur keeps the subject and blurs the rest. Custom keeps the subject and draws a replacement
//! picture (or a solid colour) behind it. Every mode is a lerp between the source and a background
//! using the matte as the weight; the result never grows past the frame's own allocation.

use deckcraft_model::CameraBackgroundMode;

use crate::MatteError;
use crate::frame::{Matte, RgbaFrame, box_blur, lerp};

/// How a frame should be composited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositeOptions {
    pub mode: CameraBackgroundMode,
    /// Custom's solid colour, used when no replacement picture is supplied.
    pub color: Option<[u8; 3]>,
    /// Blur radius in pixels; `None` picks one from the frame size (1/24 of the shorter side,
    /// clamped to 2..=24).
    pub blur_radius: Option<u32>,
    /// Soft-edge radius applied to the matte before compositing.
    pub feather: u32,
}

impl Default for CompositeOptions {
    fn default() -> Self {
        CompositeOptions { mode: CameraBackgroundMode::Normal, color: None, blur_radius: None, feather: 2 }
    }
}

impl CompositeOptions {
    pub fn new(mode: CameraBackgroundMode) -> Self {
        CompositeOptions { mode, ..Default::default() }
    }

    /// The radius a blur will use for this frame.
    pub fn radius_for(&self, width: u32, height: u32) -> u32 {
        self.blur_radius.unwrap_or_else(|| (width.min(height) / 24).clamp(2, 24))
    }
}

/// Composite one frame: resize and feather the matte, resize the replacement, then composite.
/// The per-call work here is what [`prepare_matte`] and [`composite_prepared`] split out for a
/// caller that reuses the same matte for many frames ([`crate::MatteWorker`] does).
pub fn composite(
    frame: &RgbaFrame,
    matte: Option<&Matte>,
    options: &CompositeOptions,
    replacement: Option<&RgbaFrame>,
) -> Result<RgbaFrame, MatteError> {
    if options.mode == CameraBackgroundMode::Normal {
        return Ok(frame.clone());
    }
    let matte = matte.ok_or_else(|| MatteError::InvalidImage("this background needs a matte".into()))?;
    let prepared = prepare_matte(matte, frame.width(), frame.height(), options.feather)?;
    let replacement = replacement.map(|r| r.resize(frame.width(), frame.height())).transpose()?;
    composite_prepared(frame, &prepared, options, replacement.as_ref())
}

/// Resize a matte to the frame and apply the feather once, so a caller that composites several
/// frames with the same matte does not re-feather (two separable passes over the whole frame) on
/// every call. Borrows the matte and clones only when a resize or feather is actually needed.
pub fn prepare_matte(matte: &Matte, width: u32, height: u32, feather: u32) -> Result<Matte, MatteError> {
    if (matte.width(), matte.height()) == (width, height) && feather == 0 {
        return Ok(matte.clone());
    }
    let matte = if (matte.width(), matte.height()) == (width, height) { matte.clone() } else { matte.resize(width, height)? };
    Ok(if feather == 0 { matte } else { matte.feather(feather, 1) })
}

/// Composite with a matte already resized and feathered for this frame ([`prepare_matte`]); the
/// replacement, when present, must already be this frame's size. The matte is borrowed, not
/// cloned, and `options.feather` is not applied again.
pub fn composite_prepared(
    frame: &RgbaFrame,
    matte: &Matte,
    options: &CompositeOptions,
    replacement: Option<&RgbaFrame>,
) -> Result<RgbaFrame, MatteError> {
    match options.mode {
        CameraBackgroundMode::Normal => Ok(frame.clone()),
        CameraBackgroundMode::Removed => removed(frame, matte),
        CameraBackgroundMode::Blur => blur(frame, matte, options.radius_for(frame.width(), frame.height())),
        CameraBackgroundMode::Custom => {
            let replacement = match replacement {
                Some(r) if (r.width(), r.height()) == (frame.width(), frame.height()) => Some(r),
                // A caller that hands us another size gets the documented resize, not a panic and
                // not a wrong-sized read.
                Some(r) => {
                    let resized = r.resize(frame.width(), frame.height())?;
                    return custom(frame, matte, Some(&resized), options.color.unwrap_or([0, 0, 0]));
                }
                None => None,
            };
            custom(frame, matte, replacement, options.color.unwrap_or([0, 0, 0]))
        }
    }
}

fn removed(frame: &RgbaFrame, matte: &Matte) -> Result<RgbaFrame, MatteError> {
    let mut out = frame.clone();
    for y in 0..out.height() {
        for x in 0..out.width() {
            let m = f64::from(matte.coverage(x, y)) / 255.0;
            let px = frame.at(x, y);
            let a = (f64::from(px[3]) * m).round().clamp(0.0, 255.0) as u8;
            out.set(x, y, [px[0], px[1], px[2], a]);
        }
    }
    Ok(out)
}

fn blur(frame: &RgbaFrame, matte: &Matte, radius: u32) -> Result<RgbaFrame, MatteError> {
    let blurred = blur_frame(frame, radius)?;
    let mut out = frame.clone();
    for y in 0..out.height() {
        for x in 0..out.width() {
            let m = f64::from(matte.coverage(x, y)) / 255.0;
            let src = frame.at(x, y);
            let bg = blurred.at(x, y);
            let mut px = [0u8; 4];
            for ((dst, b), s) in px.iter_mut().zip(bg).zip(src) {
                *dst = lerp(f64::from(b), f64::from(s), m).round().clamp(0.0, 255.0) as u8;
            }
            px[3] = src[3];
            out.set(x, y, px);
        }
    }
    Ok(out)
}

fn custom(frame: &RgbaFrame, matte: &Matte, replacement: Option<&RgbaFrame>, color: [u8; 3]) -> Result<RgbaFrame, MatteError> {
    let mut out = frame.clone();
    for y in 0..out.height() {
        for x in 0..out.width() {
            let m = f64::from(matte.coverage(x, y)) / 255.0;
            let src = frame.at(x, y);
            let bg = replacement.map(|r| r.at(x, y)).unwrap_or([color[0], color[1], color[2], 255]);
            let mut px = [0u8; 4];
            for ((dst, b), s) in px.iter_mut().zip(bg).zip(src) {
                *dst = lerp(f64::from(b), f64::from(s), m).round().clamp(0.0, 255.0) as u8;
            }
            // A replacement background is opaque even where the camera had alpha.
            px[3] = 255;
            out.set(x, y, px);
        }
    }
    Ok(out)
}

/// A separable box blur, run twice to approximate a Gaussian. Alpha is left alone. Two iterations
/// (12 full-frame plane passes for a 3-channel blur) are visually indistinguishable from three here
/// and a third of the pass cost cheaper; the radius is the dominant term.
fn blur_frame(frame: &RgbaFrame, radius: u32) -> Result<RgbaFrame, MatteError> {
    let radius = radius.min(64);
    if radius == 0 {
        return Ok(frame.clone());
    }
    let (w, h) = (frame.width(), frame.height());
    let mut planes: [Vec<u8>; 4] = Default::default();
    for (i, plane) in planes.iter_mut().enumerate() {
        let mut v = Vec::new();
        v.try_reserve_exact(w as usize * h as usize).map_err(|_| MatteError::TooLarge { width: w, height: h, limit: crate::MAX_PIXELS })?;
        for y in 0..h {
            for x in 0..w {
                v.push(frame.at(x, y)[i]);
            }
        }
        *plane = v;
    }
    let mut scratch = vec![0u8; w as usize * h as usize];
    for plane in &mut planes[..3] {
        for _ in 0..2 {
            box_blur(plane, &mut scratch, w, h, radius, true);
            box_blur(&scratch, plane, w, h, radius, false);
        }
    }
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(w as usize * h as usize * 4).map_err(|_| MatteError::TooLarge { width: w, height: h, limit: crate::MAX_PIXELS })?;
    for y in 0..h {
        for x in 0..w {
            let i = y as usize * w as usize + x as usize;
            pixels.push(planes[0].get(i).copied().unwrap_or(0));
            pixels.push(planes[1].get(i).copied().unwrap_or(0));
            pixels.push(planes[2].get(i).copied().unwrap_or(0));
            pixels.push(planes[3].get(i).copied().unwrap_or(255));
        }
    }
    RgbaFrame::new(w, h, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, left: [u8; 4], right: [u8; 4]) -> RgbaFrame {
        let mut px = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                px.extend_from_slice(if x < w / 2 { &left } else { &right });
            }
        }
        RgbaFrame::new(w, h, px).expect("frame")
    }

    fn half_matte(w: u32, h: u32) -> Matte {
        let mut a = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                a.push(if x < w / 2 { 255 } else { 0 });
            }
        }
        Matte::new(w, h, a).expect("matte")
    }

    #[test]
    fn normal_ignores_the_matte_and_returns_the_frame() {
        let f = frame(4, 2, [10, 20, 30, 255], [40, 50, 60, 255]);
        let m = half_matte(4, 2);
        let out = composite(&f, Some(&m), &CompositeOptions::new(CameraBackgroundMode::Normal), None).expect("normal");
        assert_eq!(out, f);
    }

    #[test]
    fn removed_makes_the_background_transparent_and_keeps_the_subject() {
        let f = frame(4, 2, [10, 20, 30, 255], [40, 50, 60, 255]);
        let m = half_matte(4, 2);
        let out =
            composite(&f, Some(&m), &CompositeOptions { feather: 0, ..CompositeOptions::new(CameraBackgroundMode::Removed) }, None).expect("removed");
        assert_eq!(out.at(0, 0), [10, 20, 30, 255], "subject stays");
        assert_eq!(out.at(3, 0), [40, 50, 60, 0], "background is transparent");
    }

    #[test]
    fn blur_keeps_the_subject_and_blurs_the_background() {
        let f = frame(8, 8, [0, 0, 0, 255], [255, 255, 255, 255]);
        let m = half_matte(8, 8);
        let out = composite(
            &f,
            Some(&m),
            &CompositeOptions { feather: 0, blur_radius: Some(2), ..CompositeOptions::new(CameraBackgroundMode::Blur) },
            None,
        )
        .expect("blur");
        assert_eq!(out.at(1, 4), [0, 0, 0, 255], "the subject centre is untouched");
        let bg = out.at(7, 4);
        assert!(bg[0] > 0 && bg[0] < 255, "the far background is a mix: {bg:?}");
        assert_eq!(out.at(3, 4), f.at(3, 4), "the subject edge column stays source");
    }

    #[test]
    fn custom_draws_the_replacement_behind_the_subject() {
        let f = frame(4, 2, [10, 20, 30, 255], [40, 50, 60, 255]);
        let m = half_matte(4, 2);
        let red = RgbaFrame::solid(4, 2, [200, 0, 0, 255]).expect("red");
        let out = composite(&f, Some(&m), &CompositeOptions { feather: 0, ..CompositeOptions::new(CameraBackgroundMode::Custom) }, Some(&red))
            .expect("custom");
        assert_eq!(out.at(0, 0), [10, 20, 30, 255]);
        assert_eq!(out.at(3, 0), [200, 0, 0, 255]);
        // Without a picture, the solid colour is the background.
        let out = composite(
            &f,
            Some(&m),
            &CompositeOptions { feather: 0, color: Some([0, 0, 200]), ..CompositeOptions::new(CameraBackgroundMode::Custom) },
            None,
        )
        .expect("colour");
        assert_eq!(out.at(3, 0), [0, 0, 200, 255]);
    }

    #[test]
    fn a_mode_that_needs_a_matte_rejects_a_missing_one() {
        let f = frame(2, 2, [1, 2, 3, 255], [4, 5, 6, 255]);
        for mode in [CameraBackgroundMode::Removed, CameraBackgroundMode::Blur, CameraBackgroundMode::Custom] {
            assert!(composite(&f, None, &CompositeOptions::new(mode), None).is_err(), "{mode:?}");
        }
    }

    #[test]
    fn a_matte_at_another_size_is_resampled() {
        let f = frame(4, 4, [0, 0, 0, 255], [255, 255, 255, 255]);
        let small = Matte::new(2, 2, vec![255, 0, 255, 0]).expect("matte");
        let out = composite(&f, Some(&small), &CompositeOptions { feather: 0, ..CompositeOptions::new(CameraBackgroundMode::Custom) }, None)
            .expect("custom");
        assert_eq!(out.at(3, 0), [0, 0, 0, 255], "the transparent side takes the replacement");
    }
}
