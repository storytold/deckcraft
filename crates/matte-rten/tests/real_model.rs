//! The real model, run only when the user points `DECKCRAFT_BIREFNET_MODEL` at the installed
//! `onnx/model.onnx`. Ignored by default: CI has no 192 MB weights.

use std::path::Path;
use std::time::Instant;

use deckcraft_matte::{RgbaFrame, Segmenter};
use deckcraft_matte_rten::RtenSegmenter;

#[test]
#[ignore = "needs DECKCRAFT_BIREFNET_MODEL pointing at the installed model"]
fn birefnet_lite_512_produces_a_plausible_matte_and_reports_its_speed() {
    let path = std::env::var("DECKCRAFT_BIREFNET_MODEL").expect("DECKCRAFT_BIREFNET_MODEL");
    let mut segmenter = RtenSegmenter::load(Path::new(&path)).expect("load the model");
    // A 640x480 frame with a bright oval in the middle over a dark background: a real image is
    // better, but this proves the tensor path end to end without committing one.
    let (w, h) = (640u32, 480u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 - 320.0) / 140.0;
            let dy = (y as f32 - 240.0) / 170.0;
            let inside = dx * dx + dy * dy < 1.0;
            let c = if inside { [235, 190, 160, 255] } else { [18, 20, 26, 255] };
            px.extend_from_slice(&c);
        }
    }
    let frame = RgbaFrame::new(w, h, px).expect("frame");
    let start = Instant::now();
    let matte = segmenter.matte(&frame).expect("matte");
    let elapsed = start.elapsed();
    let mean = matte.alpha().iter().map(|v| f64::from(*v) / 255.0).sum::<f64>() / matte.alpha().len() as f64;
    assert_eq!((matte.width(), matte.height()), (w, h));
    assert!(matte.coverage(320, 240) > 100, "the subject is covered");
    assert!(mean > 0.02 && mean < 0.98, "not an empty or full mask: {mean}");
    println!("birefnet-lite-512: {:.0} ms/frame at {w}x{h}", elapsed.as_secs_f64() * 1000.0);
}
