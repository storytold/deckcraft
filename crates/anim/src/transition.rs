//! Slide transitions as layers of textured quads.
//!
//! A frame is drawn over black: each [`Layer`] maps part of a source image (old slide, new slide,
//! or a solid colour) onto a quad in slide-unit space (0..1, y down). Fake 3-D comes from
//! trapezoids; wedges are triangles (4th corner repeats the 3rd).

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::clampf;
use crate::easing::smooth;

/// What a layer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    /// The outgoing slide.
    Old,
    /// The incoming slide.
    New,
    Black,
    White,
}

/// One textured quad of a transition frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub source: Source,
    /// Source sub-rectangle in uv space (0..1): l, t, r, b (bounds of `uv`).
    pub src: [f64; 4],
    /// Destination corners in slide-unit space: tl, tr, br, bl.
    pub quad: [[f64; 2]; 4],
    /// Texture coordinate of each destination corner (tl, tr, br, bl). For plain rectangles these
    /// are the corners of `src`; triangles and mirrored flaps carry their own. Draw with these.
    pub uv: [[f64; 2]; 4],
    pub alpha: f64,
}

type R = [f64; 4];
const FULL: R = [0.0, 0.0, 1.0, 1.0];

fn corners(r: R) -> [[f64; 2]; 4] {
    [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
}

impl Layer {
    /// The whole source over the whole slide.
    pub fn full(source: Source, alpha: f64) -> Layer {
        Layer::rect(source, FULL, FULL, alpha)
    }
    /// `src` rectangle of the source drawn into the `dst` rectangle.
    pub fn rect(source: Source, src: R, dst: R, alpha: f64) -> Layer {
        Layer { source, src, quad: corners(dst), uv: corners(src), alpha }
    }
    /// `src` rectangle of the source drawn onto a quad.
    pub fn quad(source: Source, src: R, quad: [[f64; 2]; 4], alpha: f64) -> Layer {
        Layer { source, src, quad, uv: corners(src), alpha }
    }
    /// The part of the source under `dst` drawn in place.
    fn inplace(source: Source, dst: R, alpha: f64) -> Layer {
        Layer::rect(source, dst, dst, alpha)
    }
    /// A triangle of the source drawn in place.
    fn tri(source: Source, a: [f64; 2], b: [f64; 2], c: [f64; 2], alpha: f64) -> Layer {
        let xs = [a[0], b[0], c[0]];
        let ys = [a[1], b[1], c[1]];
        let mn = |v: [f64; 3]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let mx = |v: [f64; 3]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Layer { source, src: [mn(xs), mn(ys), mx(xs), mx(ys)], quad: [a, b, c, c], uv: [a, b, c, c], alpha }
    }
    fn sanitized(mut self) -> Layer {
        let f = |v: f64| if v.is_finite() { v.clamp(-1.0e6, 1.0e6) } else { 0.0 };
        for v in self.src.iter_mut() {
            *v = f(*v).clamp(0.0, 1.0);
        }
        for c in self.quad.iter_mut().chain(self.uv.iter_mut()) {
            c[0] = f(c[0]);
            c[1] = f(c[1]);
        }
        for c in self.uv.iter_mut() {
            c[0] = c[0].clamp(0.0, 1.0);
            c[1] = c[1].clamp(0.0, 1.0);
        }
        self.alpha = clampf(self.alpha, 0.0, 1.0, 0.0);
        self
    }
}

// ---------- geometry helpers ----------

fn offset(r: R, dx: f64, dy: f64) -> R {
    [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]
}

fn moved(source: Source, dx: f64, dy: f64, alpha: f64) -> Layer {
    Layer::rect(source, FULL, offset(FULL, dx, dy), alpha)
}

/// The whole source scaled about (cx, cy) and moved.
fn scaled(source: Source, s: f64, cx: f64, cy: f64, dx: f64, dy: f64, alpha: f64) -> Layer {
    let r = [cx + (0.0 - cx) * s + dx, cy + (0.0 - cy) * s + dy, cx + (1.0 - cx) * s + dx, cy + (1.0 - cy) * s + dy];
    Layer::rect(source, FULL, r, alpha)
}

/// Rotate a point about a pivot (degrees).
fn rot(p: [f64; 2], c: [f64; 2], deg: f64) -> [f64; 2] {
    let (s, co) = deg.to_radians().sin_cos();
    let (x, y) = (p[0] - c[0], p[1] - c[1]);
    [c[0] + x * co - y * s, c[1] + x * s + y * co]
}

fn rot_quad(q: [[f64; 2]; 4], c: [f64; 2], deg: f64) -> [[f64; 2]; 4] {
    [rot(q[0], c, deg), rot(q[1], c, deg), rot(q[2], c, deg), rot(q[3], c, deg)]
}

/// Horizontal perspective: left/right edges shortened by `li`/`ri` (fraction of height, each end).
fn persp_h(r: R, li: f64, ri: f64) -> [[f64; 2]; 4] {
    let h = r[3] - r[1];
    [[r[0], r[1] + li * h], [r[2], r[1] + ri * h], [r[2], r[3] - ri * h], [r[0], r[3] - li * h]]
}

/// Vertical perspective: top/bottom edges shortened by `ti`/`bi` (fraction of width, each end).
fn persp_v(r: R, ti: f64, bi: f64) -> [[f64; 2]; 4] {
    let w = r[2] - r[0];
    [[r[0] + ti * w, r[1]], [r[2] - ti * w, r[1]], [r[2] - bi * w, r[3]], [r[0] + bi * w, r[3]]]
}

/// Mirror a rectangle horizontally and/or vertically, or swap its axes (canonical → direction).
fn map_rect(r: R, mx: bool, my: bool, swap: bool) -> R {
    let mut r = if swap { [r[1], r[0], r[3], r[2]] } else { r };
    if mx {
        r = [1.0 - r[2], r[1], 1.0 - r[0], r[3]];
    }
    if my {
        r = [r[0], 1.0 - r[3], r[2], 1.0 - r[1]];
    }
    r
}

/// Motion direction of an option: l, r, u, d and diagonals.
fn dir(opt: &str, default: (f64, f64)) -> (f64, f64) {
    let mut d = (0.0, 0.0);
    if opt.contains('l') {
        d.0 = -1.0;
    }
    if opt.contains('r') {
        d.0 = 1.0;
    }
    if opt.contains('u') {
        d.1 = -1.0;
    }
    if opt.contains('d') {
        d.1 = 1.0;
    }
    if d == (0.0, 0.0) { default } else { d }
}

fn hash64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Deterministic pseudo-random value in [0, 1).
fn rnd(i: u64) -> f64 {
    (hash64(i) >> 11) as f64 / (1u64 << 53) as f64
}

fn ramp(t: f64, order: f64, window: f64) -> f64 {
    let w = window.clamp(0.01, 1.0);
    ((t - order * (1.0 - w)) / w).clamp(0.0, 1.0)
}

fn grid(cols: usize, rows: usize) -> Vec<(usize, usize, R)> {
    let mut v = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            let cell = [c as f64 / cols as f64, r as f64 / rows as f64, (c + 1) as f64 / cols as f64, (r + 1) as f64 / rows as f64];
            v.push((c, r, cell));
        }
    }
    v
}

fn center(r: R) -> [f64; 2] {
    [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0]
}

fn scale_rect(r: R, s: f64) -> R {
    let c = center(r);
    let hw = (r[2] - r[0]) / 2.0 * s;
    let hh = (r[3] - r[1]) / 2.0 * s;
    [c[0] - hw, c[1] - hh, c[0] + hw, c[1] + hh]
}

// ---------- effects ----------

/// Layers of the transition `kind` (a [`deckcraft_model::anim::TRANSITIONS`] id) with `option` at
/// progress `t` (0..1). Draw in order over black. At `t >= 1` the result is the new slide alone;
/// at `t <= 0` (or NaN) the old slide alone. Morph returns a plain crossfade (the UI renders morph
/// frames with [`crate::morph_frame`]).
pub fn transition_layers(kind: &str, option: &str, t: f64) -> Vec<Layer> {
    let t = clampf(t, 0.0, 1.0, 0.0);
    let v = if t >= 1.0 {
        vec![Layer::full(Source::New, 1.0)]
    } else if t <= 0.0 {
        vec![Layer::full(Source::Old, 1.0)]
    } else {
        layers(kind, option, t, 0)
    };
    v.into_iter().map(Layer::sanitized).filter(|l| l.alpha > 0.0).collect()
}

fn crossfade(t: f64) -> Vec<Layer> {
    vec![Layer::full(Source::Old, 1.0), Layer::full(Source::New, t)]
}

const RANDOM_KINDS: &[&str] = &[
    "fade",
    "push",
    "wipe",
    "split",
    "reveal",
    "randomBar",
    "shape",
    "uncover",
    "cover",
    "fallOver",
    "drape",
    "curtains",
    "wind",
    "prestige",
    "fracture",
    "crush",
    "peelOff",
    "pageCurlDouble",
    "airplane",
    "origami",
    "dissolve",
    "checker",
    "blinds",
    "clock",
    "ripple",
    "honeycomb",
    "glitter",
    "vortex",
    "shred",
    "switch",
    "flip",
    "gallery",
    "cube",
    "doors",
    "box",
    "comb",
    "zoom",
    "pan",
    "ferris",
    "conveyor",
    "rotate",
    "window",
    "orbit",
    "flythrough",
];

fn layers(kind: &str, option: &str, t: f64, depth: u32) -> Vec<Layer> {
    use Source::{Black, New, Old, White};
    let o = option;
    let s = smooth(t);
    match kind {
        "none" => vec![Layer::full(New, 1.0)],
        "cut" => {
            if o == "throughBlack" && t < 0.5 {
                vec![Layer::full(Black, 1.0)]
            } else {
                vec![Layer::full(New, 1.0)]
            }
        }
        "fade" if o == "throughBlack" => {
            if t < 0.5 {
                vec![Layer::full(Black, 1.0), Layer::full(Old, 1.0 - 2.0 * t)]
            } else {
                vec![Layer::full(Black, 1.0), Layer::full(New, 2.0 * t - 1.0)]
            }
        }
        "fade" | "morph" => crossfade(t),
        "push" | "pan" => {
            let (dx, dy) = dir(o, (0.0, -1.0));
            vec![moved(Old, dx * s, dy * s, 1.0), moved(New, dx * (s - 1.0), dy * (s - 1.0), 1.0)]
        }
        "uncover" => {
            let (dx, dy) = dir(o, (-1.0, 0.0));
            vec![Layer::full(New, 1.0), moved(Old, dx * s, dy * s, 1.0)]
        }
        "cover" => {
            let (dx, dy) = dir(o, (-1.0, 0.0));
            vec![Layer::full(Old, 1.0), moved(New, dx * (s - 1.0), dy * (s - 1.0), 1.0)]
        }
        "wipe" => wipe(o, t),
        "split" => {
            let horz = o.starts_with("horz");
            let mut v = vec![Layer::full(Old, 1.0)];
            let rects: Vec<R> = if o.ends_with("In") {
                vec![[0.0, 0.0, s / 2.0, 1.0], [1.0 - s / 2.0, 0.0, 1.0, 1.0]]
            } else {
                vec![[0.5 - s / 2.0, 0.0, 0.5 + s / 2.0, 1.0]]
            };
            for r in rects {
                v.push(Layer::inplace(New, map_rect(r, false, false, horz), 1.0));
            }
            v
        }
        "reveal" => {
            let dx = if o.ends_with("Right") { 1.0 } else { -1.0 };
            if o.starts_with("black") {
                if t < 0.5 {
                    vec![Layer::full(Black, 1.0), moved(Old, dx * t * 0.4, 0.0, 1.0 - 2.0 * t)]
                } else {
                    vec![Layer::full(Black, 1.0), moved(New, -dx * (1.0 - t) * 0.2, 0.0, 2.0 * t - 1.0)]
                }
            } else {
                vec![Layer::full(New, 1.0), moved(Old, dx * s * 0.6, 0.0, 1.0 - t)]
            }
        }
        "randomBar" => {
            let horz = o == "horz";
            let n = 48;
            let mut v = vec![Layer::full(Old, 1.0)];
            for i in 0..n {
                if t > rnd(i as u64 + 17) * 0.98 {
                    let r = [i as f64 / n as f64, 0.0, (i + 1) as f64 / n as f64, 1.0];
                    v.push(Layer::inplace(New, map_rect(r, false, false, horz), 1.0));
                }
            }
            v
        }
        "shape" => {
            let form = match o {
                "circle" | "diamond" | "plus" => o,
                _ => "box",
            };
            let inward = o == "in";
            if inward {
                let mut v = vec![Layer::full(New, 1.0)];
                v.extend(shape_form(Old, form, 1.0 - s));
                v
            } else {
                let mut v = vec![Layer::full(Old, 1.0)];
                v.extend(shape_form(New, form, s));
                v
            }
        }
        "flash" => {
            if t < 0.5 {
                vec![Layer::full(Old, 1.0), Layer::full(White, (2.0 * t).min(1.0))]
            } else {
                vec![Layer::full(New, 1.0), Layer::full(White, 2.0 - 2.0 * t)]
            }
        }
        "fallOver" => {
            let sg = if o == "r" { 1.0 } else { -1.0 };
            let a = t * t * PI / 2.0;
            let top = 1.0 - a.cos();
            let ins = 0.12 * a.sin();
            let sh = sg * 0.15 * a.sin();
            let q = [[ins + sh, top], [1.0 - ins + sh, top], [1.0, 1.0], [0.0, 1.0]];
            vec![Layer::full(New, 1.0), Layer::quad(Old, FULL, q, 1.0 - t.powi(3))]
        }
        "drape" => {
            let k = t * t;
            let q = [[0.0, 0.0], [1.0 - 0.3 * k, k], [1.0 - 0.3 * k, 1.0 + k], [0.0, 1.0]];
            let q = if o == "r" { mirror_q(q) } else { q };
            vec![Layer::full(New, 1.0), Layer::quad(Old, FULL, q, 1.0 - k)]
        }
        "curtains" => {
            let w = 0.5 * (1.0 - s);
            let lq = [[0.0, 0.0], [w, 0.12 * s], [w, 1.0 - 0.12 * s], [0.0, 1.0]];
            let rq = [[1.0 - w, 0.12 * s], [1.0, 0.0], [1.0, 1.0], [1.0 - w, 1.0 - 0.12 * s]];
            vec![
                scaled(New, 0.9 + 0.1 * s, 0.5, 0.5, 0.0, 0.0, 1.0),
                Layer::quad(Old, [0.0, 0.0, 0.5, 1.0], lq, 1.0),
                Layer::quad(Old, [0.5, 0.0, 1.0, 1.0], rq, 1.0),
            ]
        }
        "wind" => {
            let d = if o == "r" { 1.0 } else { -1.0 };
            let r = offset(FULL, d * 1.2 * s, -0.1 * s);
            let q = [[r[0] + d * 0.2 * s, r[1]], [r[2] + d * 0.2 * s, r[1]], [r[2], r[3]], [r[0], r[3]]];
            vec![moved(New, -d * 0.3 * (1.0 - s), 0.0, smooth((t * 1.5).min(1.0))), Layer::quad(Old, FULL, q, 1.0 - t)]
        }
        "prestige" => vec![scaled(Old, 1.0 - 0.3 * s, 0.5, 0.5, 0.0, 0.2 * s, 1.0 - t), scaled(New, 0.7 + 0.3 * s, 0.5, 0.5, 0.0, 0.0, s)],
        "fracture" => {
            let mut v = vec![Layer::full(New, 1.0)];
            for (c, r, cell) in grid(8, 6) {
                let i = (r * 8 + c) as u64;
                let q = ramp(t, rnd(i + 101) * 0.5, 0.5);
                let cc = center(cell);
                let (dx, dy) = ((cc[0] - 0.5) * 1.5 * q, (cc[1] - 0.5) * 1.5 * q + 0.5 * q * q);
                let quad = rot_quad(corners(offset(cell, dx, dy)), [cc[0] + dx, cc[1] + dy], (rnd(i + 7) - 0.5) * 180.0 * q);
                v.push(Layer::quad(Old, cell, quad, 1.0 - q));
            }
            v
        }
        "crush" => {
            let mut v = vec![Layer::full(New, 1.0)];
            for (c, r, cell) in grid(4, 4) {
                let i = (r * 4 + c) as u64;
                let cc = center(cell);
                let k = 1.0 - s;
                let pos = [0.5 + (cc[0] - 0.5) * k, 0.5 + (cc[1] - 0.5) * k];
                let half = scale_rect(cell, k * (0.8 + 0.2 * rnd(i + 3)));
                let half = offset(half, pos[0] - cc[0], pos[1] - cc[1]);
                let quad = rot_quad(corners(half), pos, (rnd(i + 5) - 0.5) * 60.0 * s);
                v.push(Layer::quad(Old, cell, quad, 1.0 - t * t));
            }
            v
        }
        "peelOff" => {
            let mut v = vec![Layer::full(New, 1.0)];
            v.extend(peel(Old, [0.0, 0.0, 1.0, 1.0], s, o == "r"));
            v
        }
        "pageCurlDouble" => {
            let mut v = vec![Layer::full(New, 1.0)];
            let left_first = o != "r";
            v.extend(peel(Old, [0.0, 0.0, 0.5, 1.0], s, !left_first));
            v.extend(peel(Old, [0.5, 0.0, 1.0, 1.0], s, left_first));
            v
        }
        "airplane" => {
            let d = if o == "r" { 1.0 } else { -1.0 };
            let k = 1.0 - 0.8 * s;
            let r = offset(scale_rect(FULL, k), d * 1.2 * s * s, -0.5 * s * s);
            let q = persp_h(r, if d > 0.0 { 0.3 * s } else { 0.0 }, if d > 0.0 { 0.0 } else { 0.3 * s });
            vec![Layer::full(New, 1.0), Layer::quad(Old, FULL, q, 1.0 - t * t)]
        }
        "origami" => {
            let d = if o == "r" { 1.0 } else { -1.0 };
            let mut v = vec![Layer::full(New, 1.0)];
            if t < 0.5 {
                let f = t * 2.0;
                let w = 0.5 * (1.0 - 0.8 * f);
                v.push(Layer::quad(Old, [0.0, 0.0, 0.5, 1.0], persp_h([0.5 - w, 0.0, 0.5, 1.0], 0.15 * f, 0.0), 1.0));
                v.push(Layer::quad(Old, [0.5, 0.0, 1.0, 1.0], persp_h([0.5, 0.0, 0.5 + w, 1.0], 0.0, 0.15 * f), 1.0));
            } else {
                let f = (t - 0.5) * 2.0;
                let r = offset(scale_rect([0.4, 0.0, 0.6, 1.0], 1.0 - 0.6 * f), d * f * 0.8, -0.3 * f);
                v.push(Layer::quad(Old, FULL, persp_v(r, 0.1, 0.0), 1.0 - f));
            }
            v
        }
        "dissolve" => {
            let mut v = vec![Layer::full(Old, 1.0)];
            for (c, r, cell) in grid(32, 18) {
                if t > rnd((r * 32 + c) as u64 + 991) * 0.98 {
                    v.push(Layer::inplace(New, cell, 1.0));
                }
            }
            v
        }
        "checker" => {
            let vert = o == "vert";
            let (n, m) = (10usize, 8usize);
            let mut v = vec![Layer::full(Old, 1.0)];
            for (c, r, cell) in grid(n, m) {
                let across = if vert { c } else { r };
                let f = (t * 2.0 - (across % 2) as f64 * 0.5).clamp(0.0, 1.0);
                if f <= 0.0 {
                    continue;
                }
                let rc = if vert {
                    [cell[0], cell[1], cell[2], cell[1] + (cell[3] - cell[1]) * f]
                } else {
                    [cell[0], cell[1], cell[0] + (cell[2] - cell[0]) * f, cell[3]]
                };
                v.push(Layer::inplace(New, rc, 1.0));
            }
            v
        }
        "blinds" => {
            let horz = o == "horz";
            let n = 8;
            let mut v = vec![Layer::full(Old, 1.0)];
            for i in 0..n {
                let x0 = i as f64 / n as f64;
                let r = [x0, 0.0, x0 + s / n as f64, 1.0];
                v.push(Layer::inplace(New, map_rect(r, false, false, horz), 1.0));
            }
            v
        }
        "clock" => clock(o, s),
        "ripple" => {
            let origin = match o {
                "lu" => [0.0, 0.0],
                "ru" => [1.0, 0.0],
                "ld" => [0.0, 1.0],
                "rd" => [1.0, 1.0],
                _ => [0.5, 0.5],
            };
            let maxd = if origin == [0.5, 0.5] { 0.71 } else { 1.42 };
            let mut v = vec![Layer::full(Old, 1.0)];
            for (_, _, cell) in grid(24, 14) {
                let cc = center(cell);
                let d = ((cc[0] - origin[0]).powi(2) + (cc[1] - origin[1]).powi(2)).sqrt() / maxd;
                let a = ramp(t, d.min(1.0), 0.3);
                if a > 0.0 {
                    let sc = 1.0 + 0.15 * (a * PI).sin();
                    v.push(Layer::rect(New, cell, scale_rect(cell, sc), a));
                }
            }
            v
        }
        "honeycomb" => {
            let (cols, rows) = (12usize, 8usize);
            let mut v = vec![Layer::full(Old, 1.0)];
            for c in 0..cols {
                let odd = c % 2 == 1;
                let extra = if odd { 1 } else { 0 };
                for r in 0..rows + extra {
                    let y0 = (r as f64 - if odd { 0.5 } else { 0.0 }) / rows as f64;
                    let cell = [c as f64 / cols as f64, y0.max(0.0), (c + 1) as f64 / cols as f64, (y0 + 1.0 / rows as f64).min(1.0)];
                    let a = ramp(t, rnd((c * 64 + r) as u64 + 4242), 0.35);
                    if a > 0.0 {
                        v.push(Layer::rect(New, cell, scale_rect(cell, 0.3 + 0.7 * a), a));
                    }
                }
            }
            v
        }
        "glitter" => {
            let (dx, dy) = dir(o, (-1.0, 0.0));
            let mut v = vec![Layer::full(Old, 1.0)];
            for (c, r, cell) in grid(20, 12) {
                let cc = center(cell);
                let grad = if dx < 0.0 {
                    1.0 - cc[0]
                } else if dx > 0.0 {
                    cc[0]
                } else if dy < 0.0 {
                    1.0 - cc[1]
                } else {
                    cc[1]
                };
                let order = 0.6 * grad + 0.4 * rnd((r * 20 + c) as u64 + 77);
                let a = ramp(t, order, 0.25);
                if a > 0.0 {
                    let quad = rot_quad(corners(cell), cc, (1.0 - a) * 90.0);
                    v.push(Layer::quad(New, cell, quad, a));
                }
            }
            v
        }
        "vortex" => {
            let (dx, dy) = dir(o, (-1.0, 0.0));
            let mut v = vec![Layer::full(New, 1.0)];
            for (c, r, cell) in grid(16, 9) {
                let i = (r * 16 + c) as u64;
                let cc = center(cell);
                let grad = 0.5 + 0.5 * ((cc[0] - 0.5) * -dx + (cc[1] - 0.5) * -dy);
                let q = ramp(t, 0.5 * grad + 0.5 * rnd(i + 31), 0.4);
                let ang = q * 2.0 * PI + rnd(i) * PI;
                let rad = q * 1.2;
                let (ox, oy) = (dx * q * 0.8 + rad * ang.cos() * 0.3, dy * q * 0.8 + rad * ang.sin() * 0.3);
                let pos = [cc[0] + ox, cc[1] + oy];
                let quad = rot_quad(corners(offset(scale_rect(cell, 1.0 - 0.5 * q), ox, oy)), pos, q * 360.0);
                v.push(Layer::quad(Old, cell, quad, 1.0 - q));
            }
            v
        }
        "shred" => {
            let mut v = vec![Layer::full(New, 1.0)];
            if o == "particles" {
                for (c, r, cell) in grid(24, 14) {
                    let i = (r * 24 + c) as u64;
                    let q = ramp(t, rnd(i + 555) * 0.6 + 0.4 * (1.0 - center(cell)[1]), 0.4);
                    let dy = q * q * 1.2;
                    let dx = (rnd(i + 9) - 0.5) * 0.3 * q;
                    v.push(Layer::rect(Old, cell, offset(scale_rect(cell, 1.0 - 0.6 * q), dx, dy), 1.0 - q));
                }
            } else {
                let n = 16;
                for i in 0..n {
                    let x0 = i as f64 / n as f64;
                    let x1 = (i + 1) as f64 / n as f64;
                    let q = ramp(t, i as f64 / n as f64 * 0.3 + 0.1 * rnd(i as u64), 0.7);
                    let dy = if i % 2 == 0 { 1.1 * q * q } else { -1.1 * q * q };
                    v.push(Layer::rect(Old, [x0, 0.0, x1, 1.0], [x0, dy, x1, 1.0 + dy], 1.0));
                }
            }
            v
        }
        "switch" => {
            let d = if o == "r" { 1.0 } else { -1.0 };
            let k = (t * PI).sin();
            let old = Layer::quad(
                Old,
                FULL,
                persp_h(
                    offset(scale_rect(FULL, 1.0 - 0.25 * t), d * 0.45 * k, 0.0),
                    if d > 0.0 { 0.0 } else { 0.08 * k },
                    if d > 0.0 { 0.08 * k } else { 0.0 },
                ),
                1.0,
            );
            let new = Layer::quad(
                New,
                FULL,
                persp_h(
                    offset(scale_rect(FULL, 0.75 + 0.25 * t), -d * 0.45 * k, 0.0),
                    if d > 0.0 { 0.08 * k } else { 0.0 },
                    if d > 0.0 { 0.0 } else { 0.08 * k },
                ),
                1.0,
            );
            if t < 0.5 { vec![new, old] } else { vec![old, new] }
        }
        "flip" => {
            let a = PI * s;
            let w = a.cos().abs();
            let k = 0.1 * a.sin();
            let r = [0.5 - w / 2.0, 0.0, 0.5 + w / 2.0, 1.0];
            let right = o != "l";
            if t < 0.5 {
                let q = if right { persp_h(r, -k / 2.0, k) } else { persp_h(r, k, -k / 2.0) };
                vec![Layer::quad(Old, FULL, q, 1.0)]
            } else {
                let q = if right { persp_h(r, k, -k / 2.0) } else { persp_h(r, -k / 2.0, k) };
                vec![Layer::quad(New, FULL, q, 1.0)]
            }
        }
        "gallery" | "conveyor" => {
            let d = if o == "r" { 1.0 } else { -1.0 };
            let k = (s * PI).sin();
            let sc = 1.0 - if kind == "gallery" { 0.25 } else { 0.1 } * k;
            let gap = if kind == "gallery" { 1.15 } else { 1.05 };
            let old = offset(scale_rect(FULL, sc), d * s * gap, 0.0);
            let new = offset(scale_rect(FULL, sc), d * (s - 1.0) * gap, 0.0);
            let p = 0.08 * k;
            vec![
                Layer::quad(Old, FULL, if d > 0.0 { persp_h(old, 0.0, p) } else { persp_h(old, p, 0.0) }, 1.0),
                Layer::quad(New, FULL, if d > 0.0 { persp_h(new, p, 0.0) } else { persp_h(new, 0.0, p) }, 1.0),
            ]
        }
        "cube" | "box" | "rotate" => {
            let (dx, dy) = dir(o, (-1.0, 0.0));
            let vertical = dy != 0.0 && dx == 0.0;
            let neg = if vertical { dy < 0.0 } else { dx < 0.0 };
            let k = (s * PI).sin();
            let (far_old, far_new, shared) = match kind {
                "cube" => (0.18 * s, 0.18 * (1.0 - s), 0.05 * k),
                "box" => (0.0, 0.0, 0.18 * k),
                _ => (0.25 * s, 0.25 * (1.0 - s), 0.0),
            };
            // Canonical: moving toward -x; old on the left [0, 1-s], new on the right [1-s, 1].
            let split = 1.0 - s;
            let (old_q, new_q) = if kind == "rotate" {
                if t < 0.5 {
                    (Some(canon_q([0.0, 0.0, (1.0 - 2.0 * t).max(0.0), 1.0], far_old, 0.0)), None)
                } else {
                    (None, Some(canon_q([1.0 - (2.0 * t - 1.0), 0.0, 1.0, 1.0], 0.0, far_new)))
                }
            } else {
                (Some(canon_q([0.0, 0.0, split, 1.0], far_old, shared)), Some(canon_q([split, 0.0, 1.0, 1.0], shared, far_new)))
            };
            let fix = |q: [[f64; 2]; 4]| -> [[f64; 2]; 4] {
                let q = if neg { q } else { mirror_q(q) };
                if vertical { swap_q(q) } else { q }
            };
            let mut v = vec![Layer::full(Black, 1.0)];
            if let Some(q) = old_q {
                v.push(Layer::quad(Old, FULL, fix(q), 1.0));
            }
            if let Some(q) = new_q {
                v.push(Layer::quad(New, FULL, fix(q), 1.0));
            }
            v
        }
        "doors" | "window" => {
            let horz = o == "horz";
            let (zoom0, persp) = if kind == "doors" { (0.85, 0.1) } else { (0.6, 0.2) };
            let w = 0.5 * (1.0 - s);
            let k = persp * s;
            let left = [[0.0, 0.0], [w, -k], [w, 1.0 + k], [0.0, 1.0]];
            let right = [[1.0 - w, -k], [1.0, 0.0], [1.0, 1.0], [1.0 - w, 1.0 + k]];
            let (l, r) = if horz { (swap_q(left), swap_q(right)) } else { (left, right) };
            let (sl, sr) = if horz { ([0.0, 0.0, 1.0, 0.5], [0.0, 0.5, 1.0, 1.0]) } else { ([0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]) };
            vec![scaled(New, zoom0 + (1.0 - zoom0) * s, 0.5, 0.5, 0.0, 0.0, s.max(0.2)), Layer::quad(Old, sl, l, 1.0), Layer::quad(Old, sr, r, 1.0)]
        }
        "comb" => {
            let vert = o == "vert";
            let n = 8;
            let mut v = Vec::new();
            for i in 0..n {
                let a = i as f64 / n as f64;
                let b = (i + 1) as f64 / n as f64;
                let d = if i % 2 == 0 { 1.0 } else { -1.0 };
                let band = if vert { [a, 0.0, b, 1.0] } else { [0.0, a, 1.0, b] };
                let (ox, oy) = if vert { (0.0, d * s) } else { (d * s, 0.0) };
                let (nx, ny) = if vert { (0.0, -d * (1.0 - s)) } else { (-d * (1.0 - s), 0.0) };
                v.push(Layer::rect(Old, band, offset(band, ox, oy), 1.0));
                v.push(Layer::rect(New, band, offset(band, nx, ny), 1.0));
            }
            v
        }
        "zoom" => {
            if o == "out" {
                vec![scaled(New, 1.5 - 0.5 * s, 0.5, 0.5, 0.0, 0.0, s), scaled(Old, 1.0 - 0.7 * s, 0.5, 0.5, 0.0, 0.0, 1.0 - t)]
            } else {
                vec![scaled(Old, 1.0 + s, 0.5, 0.5, 0.0, 0.0, 1.0 - t), scaled(New, s.max(0.01), 0.5, 0.5, 0.0, 0.0, s)]
            }
        }
        "orbit" => {
            let (dx, dy) = dir(o, (-1.0, 0.0));
            let k = (s * PI).sin();
            let old = scaled(Old, 1.0 - 0.4 * k, 0.5, 0.5, dx * s + dy.abs() * 0.15 * k, dy * s + dx.abs() * 0.15 * k, 1.0 - 0.5 * s);
            let new = scaled(New, 1.0 - 0.4 * k, 0.5, 0.5, dx * (s - 1.0) - dy.abs() * 0.15 * k, dy * (s - 1.0) - dx.abs() * 0.15 * k, 0.5 + 0.5 * s);
            if t < 0.5 { vec![new, old] } else { vec![old, new] }
        }
        "flythrough" => {
            let bounce = o.ends_with("Bounce");
            let ns = |base: f64| if bounce { base + 0.08 * (s * PI).sin() } else { base };
            if o.starts_with("out") {
                vec![scaled(Old, 1.0 - 0.5 * s, 0.5, 0.5, 0.0, 0.0, 1.0 - t), scaled(New, ns(1.5 - 0.5 * s), 0.5, 0.5, 0.0, 0.0, s)]
            } else {
                vec![scaled(New, ns(0.5 + 0.5 * s), 0.5, 0.5, 0.0, 0.0, s), scaled(Old, 1.0 + 1.5 * s, 0.5, 0.5, 0.0, 0.0, 1.0 - t)]
            }
        }
        "ferris" => {
            let d = if o == "r" { 1.0 } else { -1.0 };
            let pivot = [0.5, 2.5];
            let a = 50.0 * s * d;
            let oq = rot_quad(corners(FULL), pivot, a);
            let nq = rot_quad(corners(FULL), pivot, a - 50.0 * d);
            vec![Layer::quad(Old, FULL, oq, 1.0), Layer::quad(New, FULL, nq, 1.0)]
        }
        "random" if depth == 0 => {
            let h = option.bytes().fold(0x51ED_u64, |h, b| hash64(h ^ b as u64));
            let k = RANDOM_KINDS.get((h % RANDOM_KINDS.len() as u64) as usize).copied().unwrap_or("fade");
            let opt = deckcraft_model::anim::TRANSITIONS.iter().find(|x| x.0 == k).and_then(|x| x.4.first().copied()).unwrap_or("");
            layers(k, opt, t, depth + 1)
        }
        _ => crossfade(t),
    }
}

fn mirror_q(q: [[f64; 2]; 4]) -> [[f64; 2]; 4] {
    // Mirror x and keep the tl, tr, br, bl order (swap left/right corners).
    let m = |p: [f64; 2]| [1.0 - p[0], p[1]];
    [m(q[1]), m(q[0]), m(q[3]), m(q[2])]
}

fn swap_q(q: [[f64; 2]; 4]) -> [[f64; 2]; 4] {
    // Transpose x/y; corners reordered so tl, tr, br, bl still map to the source's corners.
    let s = |p: [f64; 2]| [p[1], p[0]];
    [s(q[0]), s(q[3]), s(q[2]), s(q[1])]
}

fn canon_q(r: R, left_inset: f64, right_inset: f64) -> [[f64; 2]; 4] {
    persp_h(r, left_inset, right_inset)
}

fn wipe(o: &str, t: f64) -> Vec<Layer> {
    let mut v = vec![Layer::full(Source::Old, 1.0)];
    let diag = o.len() == 2;
    if diag {
        // Diagonal: columns revealed by a slanted edge.
        let (mx, my) = (o.contains('l'), o.contains('u'));
        let n = 32;
        for i in 0..n {
            let x0 = i as f64 / n as f64;
            let x1 = (i + 1) as f64 / n as f64;
            let h = (2.0 * t - (x0 + x1) / 2.0).clamp(0.0, 1.0);
            if h > 0.0 {
                v.push(Layer::inplace(Source::New, map_rect([x0, 0.0, x1, h], mx, my, false), 1.0));
            }
        }
        return v;
    }
    let (mx, swap) = match o {
        "l" => (true, false),
        "d" => (false, true),
        "u" => (true, true),
        _ => (false, false),
    };
    let soft = 0.15;
    let e = t * (1.0 + soft);
    let solid = (e - soft).clamp(0.0, 1.0);
    if solid > 0.0 {
        v.push(Layer::inplace(Source::New, map_rect([0.0, 0.0, solid, 1.0], mx, false, swap), 1.0));
    }
    let n = 6;
    for k in 0..n {
        let a = e - soft + soft * k as f64 / n as f64;
        let b = e - soft + soft * (k + 1) as f64 / n as f64;
        let (a, b) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
        if b > a {
            let alpha = 1.0 - (k as f64 + 0.5) / n as f64;
            v.push(Layer::inplace(Source::New, map_rect([a, 0.0, b, 1.0], mx, false, swap), alpha));
        }
    }
    v
}

/// A centred shape of the source at size `f` (0 = nothing, 1 = covers the slide), as strips.
fn shape_form(src: Source, form: &str, f: f64) -> Vec<Layer> {
    let f = f.clamp(0.0, 1.0);
    if f <= 0.0 {
        return vec![];
    }
    match form {
        "plus" => {
            let h = f / 2.0;
            vec![Layer::inplace(src, [0.0, 0.5 - h, 1.0, 0.5 + h], 1.0), Layer::inplace(src, [0.5 - h, 0.0, 0.5 + h, 1.0], 1.0)]
        }
        "circle" | "diamond" => {
            let n = 40;
            let r = if form == "circle" { f * 0.75 } else { f * 1.02 };
            let mut v = Vec::new();
            for i in 0..n {
                let y0 = i as f64 / n as f64;
                let y1 = (i + 1) as f64 / n as f64;
                let dy = ((y0 + y1) / 2.0 - 0.5).abs();
                let half = if form == "circle" { if r > dy { (r * r - dy * dy).sqrt() } else { 0.0 } } else { r - dy };
                if half > 0.0 {
                    let half = half.min(0.5);
                    v.push(Layer::inplace(src, [0.5 - half, y0, 0.5 + half, y1], 1.0));
                }
            }
            v
        }
        _ => vec![Layer::inplace(src, scale_rect(FULL, f), 1.0)],
    }
}

/// A peeling page: the part of `area` not yet peeled stays, the peeled part shows as a mirrored flap.
fn peel(src: Source, area: R, s: f64, toward_right: bool) -> Vec<Layer> {
    let w = area[2] - area[0];
    let p = s * w;
    if toward_right {
        let edge = area[0] + p;
        let stay = [edge, area[1], area[2], area[3]];
        let flap_src = [area[0], area[1], edge, area[3]];
        let x2 = edge + p;
        let flap = Layer {
            source: src,
            src: flap_src,
            quad: [[x2, area[1]], [edge, area[1]], [edge, area[3]], [x2, area[3]]],
            uv: corners(flap_src),
            alpha: 0.85,
        };
        vec![Layer::inplace(src, stay, 1.0), flap]
    } else {
        let edge = area[2] - p;
        let stay = [area[0], area[1], edge, area[3]];
        let flap_src = [edge, area[1], area[2], area[3]];
        let x2 = edge - p;
        let flap = Layer {
            source: src,
            src: flap_src,
            quad: [[edge, area[1]], [x2, area[1]], [x2, area[3]], [edge, area[3]]],
            uv: corners(flap_src),
            alpha: 0.85,
        };
        vec![Layer::inplace(src, stay, 1.0), flap]
    }
}

/// Point on the unit square's boundary seen from the centre at angle `a` (radians, clockwise from
/// 12 o'clock).
fn boundary(a: f64) -> [f64; 2] {
    let (dx, dy) = (a.sin(), -a.cos());
    let m = dx.abs().max(dy.abs()).max(1e-9);
    [0.5 + 0.5 * dx / m, 0.5 + 0.5 * dy / m]
}

fn clock(o: &str, s: f64) -> Vec<Layer> {
    let mut v = vec![Layer::full(Source::Old, 1.0)];
    let c = [0.5, 0.5];
    let sweep = s * 2.0 * PI;
    let (a0, a1) = match o {
        "counterClockwise" => (-sweep, 0.0),
        "wedge" => (-sweep / 2.0, sweep / 2.0),
        _ => (0.0, sweep),
    };
    let span = a1 - a0;
    if span <= 0.0 {
        return v;
    }
    let n = ((span / (PI / 24.0)).ceil() as usize).clamp(1, 96);
    let mut angles: Vec<f64> = (0..=n).map(|k| a0 + span * k as f64 / n as f64).collect();
    // Square corners inside the sweep, so the fan reaches them.
    for k in -8i32..=8 {
        let a = PI / 4.0 + k as f64 * PI / 2.0;
        if a > a0 && a < a1 {
            angles.push(a);
        }
    }
    angles.sort_by(f64::total_cmp);
    for w in angles.windows(2) {
        if let [x, y] = w
            && y > x
        {
            v.push(Layer::tri(Source::New, c, boundary(*x), boundary(*y), 1.0));
        }
    }
    v
}
