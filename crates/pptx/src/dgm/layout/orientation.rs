//! Resolve rotation inside the already constrained layout footprint. Text algorithms own
//! their orientation independently; composite shapes can borrow that text frame unchanged.
use super::{MAX_WORK, Placed};
use deckcraft_geom::{Rect, Xfrm, preset};
use std::collections::HashMap;

/// Maximise w*h subject to c*w+s*h <= width and s*w+c*h <= height.
/// The maximum is either the intersection or a stationary point on one active edge.
fn inscribed(frame: Xfrm) -> Xfrm {
    let angle = frame.rot.rem_euclid(360.).to_radians();
    let (s, c) = (angle.sin().abs(), angle.cos().abs());
    let (w, h) = if s < 1e-10 {
        (frame.w, frame.h)
    } else if c < 1e-10 {
        (frame.h, frame.w)
    } else {
        let mut best = (0., 0.);
        let mut consider = |w: f64, h: f64| {
            if w.is_finite()
                && h.is_finite()
                && w >= 0.
                && h >= 0.
                && c * w + s * h <= frame.w + 1e-7
                && s * w + c * h <= frame.h + 1e-7
                && w * h > best.0 * best.1
            {
                best = (w, h);
            }
        };
        consider(frame.w / (2. * c), frame.w / (2. * s));
        consider(frame.h / (2. * s), frame.h / (2. * c));
        let det = c * c - s * s;
        if det.abs() > 1e-10 {
            consider((c * frame.w - s * frame.h) / det, (c * frame.h - s * frame.w) / det);
        }
        best
    };
    Xfrm { x: frame.x + (frame.w - w) / 2., y: frame.y + (frame.h - h) / 2., w, h, ..frame }
}

fn counter_rotation(angle: f64, mode: &str) -> f64 {
    let a = angle.rem_euclid(360.);
    match mode {
        "none" => 0.,
        "grav" => {
            if a > 90. && a < 270. {
                180.
            } else {
                0.
            }
        }
        _ => {
            if a > 45. && a < 135. {
                -90.
            } else if (135. ..=225.).contains(&a) {
                180.
            } else if a > 225. && a < 315. {
                90.
            } else {
                0.
            }
        }
    }
}

fn text_box(p: &Placed<'_>) -> Xfrm {
    let r = p.rect;
    let name = p.shape.and_then(|s| s.attr("type")).unwrap_or("rect");
    let mut adj = preset::info(name).map(|i| i.defaults.to_vec()).unwrap_or_default();
    if let Some(list) = p.shape.and_then(|s| s.child("adjLst")) {
        for a in list.children_named("adj").take(64) {
            if let (Some(i), Some(v)) = (a.u32("idx"), a.f64("val"))
                && (1..=64).contains(&i)
                && v.is_finite()
                && v.abs() <= 1e6
            {
                adj.resize(adj.len().max(i as usize), 0.);
                if let Some(value) = adj.get_mut(i as usize - 1) {
                    *value = v * 100000.;
                }
            }
        }
    }
    let area = preset::build(name, r.w, r.h, &adj).map(|g| g.text_rect).unwrap_or_else(|| Rect::new(0., 0., r.w, r.h));
    let centre = r.affine() * area.center();
    let counter = counter_rotation(r.rot, p.params.get("autoTxRot").map_or("upr", String::as_str));
    let (w, h) = if counter.abs() == 90. { (area.height(), area.width()) } else { (area.width(), area.height()) };
    Xfrm { rot: (r.rot + counter + p.text_rotation).rem_euclid(360.), ..Xfrm::new(centre.x - w / 2., centre.y - h / 2., w, h) }
}

pub(super) fn apply(out: &mut [Placed<'_>], work: &mut usize) -> Result<(), &'static str> {
    *work = work.saturating_add(out.len().saturating_mul(3));
    if *work > MAX_WORK {
        return Err("diagram orientation budget exceeded");
    }
    let mut text = HashMap::new();
    for p in out.iter_mut().filter(|p| !p.connector) {
        p.rect = inscribed(p.rect);
        if p.text_enabled && p.text_source.is_none() {
            *work = work.saturating_add(p.shape.and_then(|s| s.child("adjLst")).map_or(0, |l| l.children_named("adj").take(64).count()));
            if *work > MAX_WORK {
                return Err("diagram orientation budget exceeded");
            }
            let t = text_box(p);
            p.text_rect = Some(t);
            text.insert(p.node_key, t);
        }
    }
    for p in out {
        if let Some(source) = p.text_source {
            p.text_rect = text.get(&source).copied();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotated_rectangles_fit_their_footprint_at_maximal_area() {
        for (w, h) in [(20., 40.), (80., 70.), (40., 20.), (10., 10.)] {
            for degrees in [-405., -90., -30., 0., 30., 45., 46., 90., 135., 180., 225., 270., 315., 390.] {
                let frame = Xfrm { rot: degrees, ..Xfrm::new(13., 21., w, h) };
                let r = inscribed(frame);
                assert!(r.is_finite());
                assert_eq!(r.center(), frame.center());
                let bounds = r.bounds();
                assert!(bounds.width() <= w + 1e-6 && bounds.height() <= h + 1e-6);
                // Independent feasible grid samples may not beat the continuous optimum.
                for x in 1..=50 {
                    for y in 1..=50 {
                        let trial = Xfrm { w: x as f64 * w.max(h) / 50., h: y as f64 * w.max(h) / 50., ..r };
                        if trial.bounds().width() <= w && trial.bounds().height() <= h {
                            assert!(trial.w * trial.h <= r.w * r.h + 1e-6);
                        }
                    }
                }
            }
        }
        let r = inscribed(Xfrm { rot: 30., ..Xfrm::new(0., 0., 20., 40.) });
        assert!((r.w - 11.547005).abs() < 1e-5 && (r.h - 20.).abs() < 1e-6);
        let r = inscribed(Xfrm { rot: 30., ..Xfrm::new(0., 0., 80., 70.) });
        assert!((r.w - 68.564065).abs() < 1e-5 && (r.h - 41.243557).abs() < 1e-5);
    }

    #[test]
    fn text_modes_have_independent_boundary_and_wrap_semantics() {
        for (a, upright, gravity) in [
            (-45., 0., 0.),
            (30., 0., 0.),
            (45., 0., 0.),
            (46., -90., 0.),
            (90., -90., 0.),
            (135., 180., 180.),
            (180., 180., 180.),
            (225., 180., 180.),
            (270., 90., 0.),
            (315., 0., 0.),
            (450., -90., 0.),
        ] {
            assert_eq!(counter_rotation(a, "upr"), upright);
            assert_eq!(counter_rotation(a, "grav"), gravity);
            assert_eq!(counter_rotation(a, "none"), 0.);
        }
    }
}
