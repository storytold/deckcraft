//! Per-keystroke cost: parse + layout + SVG for typical rows (native; wasm is a small multiple).
use equashion_layout::{DEFAULT_FAMILY, FontDb, Rgb, layout_equation};
use std::time::Instant;

fn main() {
    let fonts = FontDb::bundled_only();
    let rows = [r"y=x^2", r"y=\frac{x^2-1}{x+1}", r"a=\frac{-b\pm\sqrt{b^{2}-4ac}}{2a}", r"\sum_{n=1}^{5}\left(n^2+\frac{1}{n}\right)"];
    for src in rows {
        let n = 300;
        let t = Instant::now();
        let mut bytes = 0;
        for _ in 0..n {
            let m = equashion_math::from_linear(src);
            let eq = layout_equation(&fonts, &m, DEFAULT_FAMILY, 24.0, true, Rgb::BLACK, 4.0);
            bytes = equashion_render::scene_to_svg(&eq.scene, &fonts, DEFAULT_FAMILY).len();
        }
        let per = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("{per:6.2} ms  {bytes:6} B svg  {src}");
    }
}
