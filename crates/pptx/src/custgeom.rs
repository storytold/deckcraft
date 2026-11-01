//! DrawingML custom geometry (`a:custGeom`): guide formula evaluation and conversion of
//! `a:pathLst` to our `M L C Q Z` path syntax, and back.
//!
//! Formulas follow the ECMA-376 shape guide semantics (§20.1.9.11): angles are in 1/60 000 degree,
//! and every operand is a guide name, a built-in variable or an integer literal.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::fmt::Write as _;

use deckcraft_geom::preset::FillMode;
use deckcraft_model::CustomPath;

use crate::xml::{A, El, W};

const DEG: f64 = 60_000.0;

/// Guide variables for one shape.
pub struct Guides {
    vars: HashMap<String, f64>,
}

fn to_rad(a: f64) -> f64 {
    a / DEG * PI / 180.0
}

fn to_ang(r: f64) -> f64 {
    r * 180.0 / PI * DEG
}

fn finite(v: f64) -> f64 {
    if v.is_finite() { v.clamp(-1.0e15, 1.0e15) } else { 0.0 }
}

impl Guides {
    /// Built-in variables for a shape `w` × `h` (in the shape's coordinate units).
    pub fn new(w: f64, h: f64) -> Self {
        let (w, h) = (finite(w), finite(h));
        let ss = w.min(h);
        let ls = w.max(h);
        let mut v = HashMap::new();
        let mut set = |k: &str, x: f64| {
            v.insert(k.to_string(), x);
        };
        set("w", w);
        set("h", h);
        set("l", 0.0);
        set("t", 0.0);
        set("r", w);
        set("b", h);
        set("hc", w / 2.0);
        set("vc", h / 2.0);
        set("ss", ss);
        set("ls", ls);
        for d in [2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 32.0] {
            set(&format!("wd{d}"), w / d);
            set(&format!("hd{d}"), h / d);
            set(&format!("ssd{d}"), ss / d);
        }
        set("cd2", 10_800_000.0);
        set("cd4", 5_400_000.0);
        set("cd8", 2_700_000.0);
        set("3cd4", 16_200_000.0);
        set("3cd8", 8_100_000.0);
        set("5cd8", 13_500_000.0);
        set("7cd8", 18_900_000.0);
        Guides { vars: v }
    }
    pub fn get(&self, name: &str) -> f64 {
        let name = name.trim();
        if let Some(v) = self.vars.get(name) {
            return *v;
        }
        name.parse::<f64>().ok().map(finite).unwrap_or(0.0)
    }
    pub fn set(&mut self, name: &str, v: f64) {
        if self.vars.len() < 10_000 {
            self.vars.insert(name.to_string(), finite(v));
        }
    }
    /// Evaluate one formula (`*/ w 1 2`).
    pub fn eval(&self, fmla: &str) -> f64 {
        let mut it = fmla.split_whitespace();
        let op = it.next().unwrap_or("");
        let args: Vec<f64> = it.take(3).map(|a| self.get(a)).collect();
        let a = |i: usize| args.get(i).copied().unwrap_or(0.0);
        let div = |n: f64, d: f64| if d == 0.0 { 0.0 } else { n / d };
        let r = match op {
            "*/" => div(a(0) * a(1), a(2)),
            "+-" => a(0) + a(1) - a(2),
            "+/" => div(a(0) + a(1), a(2)),
            "?:" => {
                if a(0) > 0.0 {
                    a(1)
                } else {
                    a(2)
                }
            }
            "abs" => a(0).abs(),
            "at2" => to_ang(a(1).atan2(a(0))),
            "cat2" => a(0) * a(2).atan2(a(1)).cos(),
            "sat2" => a(0) * a(2).atan2(a(1)).sin(),
            "cos" => a(0) * to_rad(a(1)).cos(),
            "sin" => a(0) * to_rad(a(1)).sin(),
            "tan" => a(0) * to_rad(a(1)).tan(),
            "max" => a(0).max(a(1)),
            "min" => a(0).min(a(1)),
            "mod" => (a(0) * a(0) + a(1) * a(1) + a(2) * a(2)).sqrt(),
            "pin" => {
                if a(1) < a(0) {
                    a(0)
                } else if a(1) > a(2) {
                    a(2)
                } else {
                    a(1)
                }
            }
            "sqrt" => a(0).max(0.0).sqrt(),
            "val" => a(0),
            _ => 0.0,
        };
        finite(r)
    }
    /// Evaluate a list of `a:gd` elements in order.
    pub fn eval_list(&mut self, list: Option<&El>) {
        let Some(list) = list else { return };
        for gd in list.children_named("gd").take(5000) {
            let (Some(name), Some(f)) = (gd.attr("name"), gd.attr("fmla")) else { continue };
            let v = self.eval(f);
            self.set(name, v);
        }
    }
}

fn num(s: &mut String, v: f64) {
    let v = finite(v);
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() {
        let _ = write!(s, "{}", r as i64);
    } else {
        let _ = write!(s, "{r}");
    }
}

fn pt(g: &Guides, e: &El) -> (f64, f64) {
    (g.get(e.attr("x").unwrap_or("0")), g.get(e.attr("y").unwrap_or("0")))
}

/// Convert `a:custGeom` to custom paths. `w`/`h` is the shape size in EMU (used for guides and for
/// paths that don't give their own coordinate space).
pub fn read_cust_geom(cg: &El, w_emu: f64, h_emu: f64) -> Vec<CustomPath> {
    let mut out = vec![];
    let Some(pl) = cg.child("pathLst") else { return out };
    for p in pl.children_named("path").take(1000) {
        let pw = p.f64("w").unwrap_or(0.0);
        let ph = p.f64("h").unwrap_or(0.0);
        // Guides are evaluated in the path's own space when it has one.
        let (gw, gh) = if pw > 0.0 || ph > 0.0 { (pw, ph) } else { (w_emu, h_emu) };
        let mut g = Guides::new(gw, gh);
        g.eval_list(cg.child("avLst"));
        g.eval_list(cg.child("gdLst"));
        let mut d = String::new();
        let mut cur = (0.0, 0.0);
        let mut start = (0.0, 0.0);
        for c in p.elements().take(100_000) {
            match c.local() {
                "moveTo" => {
                    if let Some(q) = c.child("pt") {
                        cur = pt(&g, q);
                        start = cur;
                        d.push_str("M ");
                        num(&mut d, cur.0);
                        d.push(' ');
                        num(&mut d, cur.1);
                        d.push(' ');
                    }
                }
                "lnTo" => {
                    if let Some(q) = c.child("pt") {
                        cur = pt(&g, q);
                        d.push_str("L ");
                        num(&mut d, cur.0);
                        d.push(' ');
                        num(&mut d, cur.1);
                        d.push(' ');
                    }
                }
                "cubicBezTo" => {
                    let pts: Vec<(f64, f64)> = c.children_named("pt").map(|q| pt(&g, q)).collect();
                    if let [a, b, e] = pts.as_slice() {
                        d.push_str("C ");
                        for (x, y) in [a, b, e] {
                            num(&mut d, *x);
                            d.push(' ');
                            num(&mut d, *y);
                            d.push(' ');
                        }
                        cur = *e;
                    }
                }
                "quadBezTo" => {
                    let pts: Vec<(f64, f64)> = c.children_named("pt").map(|q| pt(&g, q)).collect();
                    if let [a, e] = pts.as_slice() {
                        d.push_str("Q ");
                        for (x, y) in [a, e] {
                            num(&mut d, *x);
                            d.push(' ');
                            num(&mut d, *y);
                            d.push(' ');
                        }
                        cur = *e;
                    }
                }
                "arcTo" => {
                    let wr = g.get(c.attr("wR").unwrap_or("0"));
                    let hr = g.get(c.attr("hR").unwrap_or("0"));
                    let st = g.get(c.attr("stAng").unwrap_or("0"));
                    let sw = g.get(c.attr("swAng").unwrap_or("0"));
                    cur = arc_to(&mut d, cur, wr, hr, st, sw);
                }
                "close" => {
                    d.push_str("Z ");
                    cur = start;
                }
                _ => {}
            }
        }
        let fill = match p.attr("fill").unwrap_or("norm") {
            "none" => FillMode::None,
            "lighten" => FillMode::Lighten,
            "lightenLess" => FillMode::LightenLess,
            "darken" => FillMode::Darken,
            "darkenLess" => FillMode::DarkenLess,
            _ => FillMode::Norm,
        };
        let (ow, oh) = if pw > 0.0 || ph > 0.0 { (pw, ph) } else { (w_emu.max(1.0), h_emu.max(1.0)) };
        out.push(CustomPath { w: ow, h: oh, d: d.trim_end().to_string(), fill, stroke: p.bool("stroke").unwrap_or(true) });
    }
    out
}

/// Ellipse parametric angle for a visual angle `a` (radians).
fn param(a: f64, wr: f64, hr: f64) -> f64 {
    (wr * a.sin()).atan2(hr * a.cos())
}

/// Append an elliptical arc as cubic Béziers; returns the end point.
fn arc_to(d: &mut String, cur: (f64, f64), wr: f64, hr: f64, st: f64, sw: f64) -> (f64, f64) {
    if wr.abs() < 1e-9 || hr.abs() < 1e-9 || sw == 0.0 {
        return cur;
    }
    let st_r = to_rad(st);
    let sw_r = to_rad(sw).clamp(-4.0 * PI, 4.0 * PI);
    let t0 = param(st_r, wr, hr);
    let mut t1 = param(st_r + sw_r, wr, hr);
    // Keep the sweep direction and size of the visual angle.
    let full = sw_r.abs() >= 2.0 * PI - 1e-9;
    if full {
        t1 = t0 + sw_r.signum() * 2.0 * PI;
    } else {
        while sw_r > 0.0 && t1 < t0 {
            t1 += 2.0 * PI;
        }
        while sw_r < 0.0 && t1 > t0 {
            t1 -= 2.0 * PI;
        }
    }
    let cx = cur.0 - wr * t0.cos();
    let cy = cur.1 - hr * t0.sin();
    let total = t1 - t0;
    let n = ((total.abs() / (PI / 2.0)).ceil() as usize).clamp(1, 16);
    let step = total / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let mut a = t0;
    let mut end = cur;
    for _ in 0..n {
        let b = a + step;
        let (ca, sa, cb, sb) = (a.cos(), a.sin(), b.cos(), b.sin());
        let p1 = (cx + wr * (ca - k * sa), cy + hr * (sa + k * ca));
        let p2 = (cx + wr * (cb + k * sb), cy + hr * (sb - k * cb));
        end = (cx + wr * cb, cy + hr * sb);
        d.push_str("C ");
        for (x, y) in [p1, p2, end] {
            num(d, x);
            d.push(' ');
            num(d, y);
            d.push(' ');
        }
        a = b;
    }
    end
}

/// Parse our path syntax into commands.
pub fn parse_path(d: &str) -> Vec<(char, Vec<f64>)> {
    let mut out = vec![];
    let mut cmd: Option<char> = None;
    let mut nums: Vec<f64> = vec![];
    let flush = |cmd: Option<char>, nums: &mut Vec<f64>, out: &mut Vec<(char, Vec<f64>)>| {
        if let Some(c) = cmd {
            let n = match c {
                'M' | 'L' => 2,
                'C' => 6,
                'Q' => 4,
                _ => 0,
            };
            if n == 0 {
                out.push((c, vec![]));
            } else {
                for (i, chunk) in nums.chunks(n).enumerate() {
                    if chunk.len() == n && out.len() < 1_000_000 {
                        // Repeated coordinates after M continue as L.
                        let c2 = if c == 'M' && i > 0 { 'L' } else { c };
                        out.push((c2, chunk.to_vec()));
                    }
                }
            }
        }
        nums.clear();
    };
    let mut token = String::new();
    let push_tok = |token: &mut String, nums: &mut Vec<f64>| {
        if !token.is_empty() {
            if let Ok(v) = token.parse::<f64>()
                && v.is_finite()
            {
                nums.push(v);
            }
            token.clear();
        }
    };
    let cs: Vec<char> = d.chars().collect();
    for (i, &ch) in cs.iter().enumerate() {
        match ch {
            // An 'E' inside a number followed by a digit (or a sign and a digit) is an exponent,
            // not the end command.
            'E' if !token.is_empty() && {
                let n1 = cs.get(i + 1).copied().unwrap_or(' ');
                let n2 = cs.get(i + 2).copied().unwrap_or(' ');
                n1.is_ascii_digit() || ((n1 == '-' || n1 == '+') && n2.is_ascii_digit())
            } =>
            {
                token.push(ch)
            }
            'M' | 'L' | 'C' | 'Q' | 'Z' | 'm' | 'l' | 'c' | 'q' | 'z' | 'E' => {
                push_tok(&mut token, &mut nums);
                flush(cmd, &mut nums, &mut out);
                cmd = Some(ch.to_ascii_uppercase());
            }
            ' ' | ',' | '\t' | '\n' => push_tok(&mut token, &mut nums),
            '-' if !token.is_empty() && !token.ends_with(['e', 'E']) => {
                push_tok(&mut token, &mut nums);
                token.push(ch);
            }
            c => token.push(c),
        }
    }
    push_tok(&mut token, &mut nums);
    flush(cmd, &mut nums, &mut out);
    out
}

/// Write custom paths as `a:custGeom`. Coordinates are integers in the file, so small path spaces
/// are scaled up.
pub fn write_cust_geom(w: &mut W, paths: &[CustomPath]) {
    w.open0("a:custGeom");
    w.empty0("a:avLst");
    w.empty0("a:gdLst");
    w.empty0("a:ahLst");
    w.empty0("a:cxnLst");
    w.empty("a:rect", A::new().a("l", "l").a("t", "t").a("r", "r").a("b", "b"));
    w.open0("a:pathLst");
    for p in paths {
        let (pw, ph) = (finite(p.w).abs(), finite(p.h).abs());
        let m = pw.max(ph);
        let k = if m > 0.0 && m < 20_000.0 { 100_000.0 / m } else { 1.0 };
        let r = |v: f64| (finite(v) * k).round().clamp(-2.0e9, 2.0e9) as i64;
        let fill = match p.fill {
            FillMode::Norm => None,
            FillMode::None => Some("none"),
            FillMode::Lighten => Some("lighten"),
            FillMode::LightenLess => Some("lightenLess"),
            FillMode::Darken => Some("darken"),
            FillMode::DarkenLess => Some("darkenLess"),
        };
        let a = A::new().a("w", r(pw).max(0)).a("h", r(ph).max(0)).o("fill", fill).o("stroke", (!p.stroke).then_some("0"));
        w.open("a:path", a);
        for (c, v) in parse_path(&p.d) {
            let pts: Vec<(i64, i64)> = v
                .chunks(2)
                .filter_map(|c| match c {
                    [x, y] => Some((r(*x), r(*y))),
                    _ => None,
                })
                .collect();
            let tag = match c {
                'M' => "a:moveTo",
                'L' => "a:lnTo",
                'C' => "a:cubicBezTo",
                'Q' => "a:quadBezTo",
                'Z' => {
                    w.empty0("a:close");
                    continue;
                }
                _ => continue,
            };
            w.open0(tag);
            for (x, y) in pts {
                w.empty("a:pt", A::new().a("x", x).a("y", y));
            }
            w.close(tag);
        }
        w.close("a:path");
    }
    w.close("a:pathLst");
    w.close("a:custGeom");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    #[test]
    fn formulas() {
        let mut g = Guides::new(1000.0, 500.0);
        assert_eq!(g.eval("*/ w 1 2"), 500.0);
        assert_eq!(g.eval("+- w h 100"), 1400.0);
        assert_eq!(g.eval("+/ w h 3"), 500.0);
        assert_eq!(g.eval("?: -1 5 7"), 7.0);
        assert_eq!(g.eval("?: 1 5 7"), 5.0);
        assert_eq!(g.eval("abs -42"), 42.0);
        assert_eq!(g.eval("max 3 9"), 9.0);
        assert_eq!(g.eval("min 3 9"), 3.0);
        assert_eq!(g.eval("mod 3 4 0"), 5.0);
        assert_eq!(g.eval("pin 0 150 100"), 100.0);
        assert_eq!(g.eval("pin 0 -5 100"), 0.0);
        assert_eq!(g.eval("sqrt 16"), 4.0);
        assert_eq!(g.eval("val ss"), 500.0);
        assert!((g.eval("at2 1 1") - 2_700_000.0).abs() < 1e-6);
        assert!((g.eval("cos 100 5400000")).abs() < 1e-9);
        assert!((g.eval("sin 100 5400000") - 100.0).abs() < 1e-9);
        assert!((g.eval("tan 100 2700000") - 100.0).abs() < 1e-6);
        assert!((g.eval("cat2 10 1 0") - 10.0).abs() < 1e-9);
        assert!((g.eval("sat2 10 0 1") - 10.0).abs() < 1e-9);
        assert_eq!(g.eval("*/ 1 1 0"), 0.0, "division by zero is 0");
        assert_eq!(g.eval("bogus 1 2"), 0.0);
        assert_eq!(g.eval(""), 0.0);
        g.set("x1", 7.0);
        assert_eq!(g.eval("+- x1 0 2"), 5.0);
        assert_eq!(g.get("hc"), 500.0);
        assert_eq!(g.get("cd4"), 5_400_000.0);
    }

    #[test]
    fn custgeom_with_guides_and_arc() {
        let x = br#"<a:custGeom xmlns:a="x"><a:avLst/><a:gdLst><a:gd name="mid" fmla="*/ w 1 2"/></a:gdLst>
            <a:pathLst><a:path w="200" h="100"><a:moveTo><a:pt x="0" y="0"/></a:moveTo><a:lnTo><a:pt x="mid" y="h"/></a:lnTo>
            <a:arcTo wR="50" hR="50" stAng="0" swAng="5400000"/><a:close/></a:path>
            <a:path w="10" h="10" fill="none" stroke="0"><a:moveTo><a:pt x="1" y="1"/></a:moveTo><a:quadBezTo><a:pt x="2" y="2"/><a:pt x="3" y="1"/></a:quadBezTo></a:path></a:pathLst></a:custGeom>"#;
        let d = parse(x).unwrap();
        let paths = read_cust_geom(&d.root, 1000.0, 1000.0);
        assert_eq!(paths.len(), 2);
        assert!(paths[0].d.starts_with("M 0 0 L 100 100 C"), "{}", paths[0].d);
        assert!(paths[0].d.ends_with('Z'));
        // The quarter arc from (100,100) with centre (50,100) ends at (50,150).
        assert!(paths[0].d.contains("50 150"), "{}", paths[0].d);
        assert_eq!(paths[1].fill, FillMode::None);
        assert!(!paths[1].stroke);
        assert_eq!(paths[1].d, "M 1 1 Q 2 2 3 1");
    }

    #[test]
    fn write_then_read_path() {
        let p = CustomPath { w: 100.0, h: 50.0, d: "M 0 0 L 100 0 C 100 10 90 50 50 50 Q 0 50 0 0 Z".into(), fill: FillMode::Norm, stroke: true };
        let mut w = W::frag();
        write_cust_geom(&mut w, std::slice::from_ref(&p));
        let d = parse(w.s.as_bytes()).unwrap();
        let back = read_cust_geom(&d.root, 1.0, 1.0);
        assert_eq!(back.len(), 1);
        // Scaled by 1000 (100 → 100000).
        assert_eq!(back[0].w, 100_000.0);
        assert_eq!(back[0].d, "M 0 0 L 100000 0 C 100000 10000 90000 50000 50000 50000 Q 0 50000 0 0 Z");
    }

    #[test]
    fn parse_path_uppercase_exponent() {
        assert_eq!(parse_path("M 1E-5 2 L 3 4"), vec![('M', vec![1e-5, 2.0]), ('L', vec![3.0, 4.0])]);
        assert_eq!(parse_path("M 1.5E3 2"), vec![('M', vec![1500.0, 2.0])]);
        assert_eq!(parse_path("M 1e-5 2"), vec![('M', vec![1e-5, 2.0])]);
        // An 'E' right after a number with no exponent digits is the end command.
        assert_eq!(parse_path("M 0 0E"), vec![('M', vec![0.0, 0.0]), ('E', vec![])]);
    }

    #[test]
    fn parse_path_tokens() {
        let v = parse_path("M0,0 L10-5 Z");
        assert_eq!(v.len(), 3);
        assert_eq!(v[1], ('L', vec![10.0, -5.0]));
        assert!(parse_path("garbage ### C 1").is_empty() || parse_path("garbage").is_empty());
    }
}
