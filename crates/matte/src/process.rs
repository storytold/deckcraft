//! The model's input and output contract: RGB -> normalised NCHW f32, logits -> 8-bit alpha.
//!
//! The manifest records a 512x512 input. `onnx-community`'s BiRefNet exports and the
//! `studioludens/birefnet-lite-512` conversion both take RGB in 0..1, normalised with ImageNet's
//! mean and standard deviation, and return one channel of logits that a sigmoid turns into
//! coverage. NCHW means channel planes, not interleaved pixels.

use crate::MatteError;
use crate::frame::{Matte, RgbaFrame};

/// ImageNet channel mean, in R, G, B order.
pub const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
/// ImageNet channel standard deviation, in R, G, B order.
pub const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// The input tensor's shape: one image, three channels, `size` square.
pub const fn input_shape(size: u32) -> [usize; 4] {
    [1, 3, size as usize, size as usize]
}

/// Resize `frame` to `size` x `size` and return normalised NCHW RGB planes (3 * size * size f32).
/// Alpha is dropped: the model sees the picture the camera produced, not its transparency.
pub fn preprocess(frame: &RgbaFrame, size: u32) -> Result<Vec<f32>, MatteError> {
    let resized = frame.resize(size, size)?;
    let plane = (size as usize).checked_mul(size as usize).ok_or(MatteError::TooLarge { width: size, height: size, limit: crate::MAX_PIXELS })?;
    let total = plane.checked_mul(3).ok_or(MatteError::TooLarge { width: size, height: size, limit: crate::MAX_PIXELS })?;
    let mut out = Vec::new();
    out.try_reserve_exact(total).map_err(|_| MatteError::TooLarge { width: size, height: size, limit: crate::MAX_PIXELS })?;
    for (c, (&mean, &std)) in IMAGENET_MEAN.iter().zip(&IMAGENET_STD).enumerate() {
        for i in 0..plane {
            let px = resized.pixels().get(i * 4..i * 4 + 4).unwrap_or(&[0, 0, 0, 0]);
            let v = f32::from(px[c]) / 255.0;
            out.push((v - mean) / std);
        }
    }
    Ok(out)
}

/// The logistic function, clamped so an extreme logit cannot produce NaN.
pub fn sigmoid(x: f32) -> f32 {
    if !x.is_finite() {
        return if x.is_sign_positive() { 1.0 } else { 0.0 };
    }
    let x = x.clamp(-60.0, 60.0);
    1.0 / (1.0 + (-x).exp())
}

/// Turn `size` x `size` logits into a matte at the frame's size, then soften the edge by
/// `feather` pixels.
pub fn postprocess(logits: &[f32], size: u32, frame_width: u32, frame_height: u32, feather: u32) -> Result<Matte, MatteError> {
    let expected = (size as usize).checked_mul(size as usize).ok_or(MatteError::BadOutput("output too large".into()))?;
    if logits.len() != expected {
        return Err(MatteError::BadOutput(format!("expected {expected} values for {size}x{size}, got {}", logits.len())));
    }
    let alpha: Vec<u8> = logits.iter().map(|v| (sigmoid(*v) * 255.0).round().clamp(0.0, 255.0) as u8).collect();
    let matte = Matte::new(size, size, alpha)?;
    let matte = matte.resize(frame_width, frame_height)?;
    Ok(if feather == 0 { matte } else { matte.feather(feather, 1) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> RgbaFrame {
        RgbaFrame::solid(w, h, px).expect("frame")
    }

    #[test]
    fn preprocessing_is_nchw_and_normalised() {
        let f = solid(2, 2, [255, 128, 0, 255]);
        let v = preprocess(&f, 2).expect("preprocess");
        assert_eq!(v.len(), 12);
        let plane = 4;
        let white = (1.0 - IMAGENET_MEAN[0]) / IMAGENET_STD[0];
        let mid = (128.0 / 255.0 - IMAGENET_MEAN[1]) / IMAGENET_STD[1];
        let black = (0.0 - IMAGENET_MEAN[2]) / IMAGENET_STD[2];
        assert!((v[0] - white).abs() < 1e-5);
        assert!((v[plane] - mid).abs() < 1e-5);
        assert!((v[2 * plane] - black).abs() < 1e-5);
    }

    #[test]
    fn sigmoid_is_monotone_and_clamped() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(10.0) > 0.999);
        assert!(sigmoid(-10.0) < 0.001);
        assert_eq!(sigmoid(f32::INFINITY), 1.0);
        assert_eq!(sigmoid(f32::NEG_INFINITY), 0.0);
        assert!(sigmoid(f32::NAN).is_finite());
    }

    #[test]
    fn logits_become_an_8_bit_matte_at_the_frame_size() {
        let logits = vec![0.0f32; 4];
        let m = postprocess(&logits, 2, 3, 3, 0).expect("postprocess");
        assert_eq!((m.width(), m.height()), (3, 3));
        assert!(m.alpha().iter().all(|v| *v == 128), "a zero logit is half coverage: {:?}", m.alpha());
        assert!(postprocess(&[0.0; 3], 2, 2, 2, 0).is_err(), "a wrongly sized output is rejected");
    }
}
