//! Draw › Ink to Shape: recognise a hand-drawn stroke as a line, rectangle, ellipse or triangle.
//!
//! Pure geometry so it is unit-tested with synthetic strokes. A stroke is closed when its ends
//! meet; a closed stroke is classified by how much of its bounding box it covers (rectangle ≈ 1,
//! ellipse ≈ π/4, triangle ≈ ½) and, for ellipses, by how evenly it keeps its distance from the
//! centre.

/// What a stroke was recognised as, in its bounding box `(x, y, w, h)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Recognized {
    /// From `(x0, y0)` to `(x1, y1)`.
    Line {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    },
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    Ellipse {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// Apex at the top (`flip_v` false) or at the bottom.
    Triangle {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        flip_v: bool,
    },
}

/// Strokes smaller than this (points) stay ink.
const MIN_SIZE: f64 = 6.0;
const MAX_POINTS: usize = 100_000;

pub fn recognize(points: &[(f64, f64)]) -> Option<Recognized> {
    let pts: Vec<(f64, f64)> = points.iter().copied().filter(|(x, y)| x.is_finite() && y.is_finite()).take(MAX_POINTS).collect();
    if pts.len() < 2 {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in &pts {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let size = w.max(h);
    if !size.is_finite() || size < MIN_SIZE {
        return None;
    }
    let length: f64 = pts.windows(2).map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1)).sum();
    let (first, last) = (pts[0], pts[pts.len() - 1]);
    let gap = (last.0 - first.0).hypot(last.1 - first.1);
    if length <= 0.0 {
        return None;
    }
    // Open stroke: a line if nearly straight, otherwise not a shape.
    if gap > 0.25 * size {
        return (gap / length > 0.92).then_some(Recognized::Line { x0: first.0, y0: first.1, x1: last.0, y1: last.1 });
    }
    // Closed but flat: a line drawn back and forth isn't a shape.
    if w.min(h) < 0.15 * size {
        return None;
    }
    // Covered area (shoelace, the gap closed) over the bounding box.
    let mut a2 = 0.0;
    for k in 0..pts.len() {
        let (p, q) = (pts[k], pts[(k + 1) % pts.len()]);
        a2 += p.0 * q.1 - q.0 * p.1;
    }
    let cover = (a2.abs() / 2.0) / (w * h);
    // Ellipse fit: distance from the centre in box-normalised units should stay near 1.
    let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
    let radial: Vec<f64> = pts.iter().map(|(x, y)| ((x - cx) / (w / 2.0)).hypot((y - cy) / (h / 2.0))).collect();
    let spread = radial.iter().map(|r| (r - 1.0).abs()).sum::<f64>() / radial.len() as f64;
    if cover > 0.86 {
        Some(Recognized::Rect { x: x0, y: y0, w, h })
    } else if cover > 0.66 && spread < 0.12 {
        Some(Recognized::Ellipse { x: x0, y: y0, w, h })
    } else if (0.35..0.66).contains(&cover) {
        // Apex: the side of the box with fewer points near it is where the triangle narrows.
        let near_top = pts.iter().filter(|(_, y)| *y < y0 + h * 0.15).count();
        let near_bottom = pts.iter().filter(|(_, y)| *y > y1 - h * 0.15).count();
        Some(Recognized::Triangle { x: x0, y: y0, w, h, flip_v: near_bottom < near_top })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poly(corners: &[(f64, f64)], per_side: usize) -> Vec<(f64, f64)> {
        let mut v = vec![];
        for k in 0..corners.len() {
            let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
            for i in 0..per_side {
                let t = i as f64 / per_side as f64;
                // A little hand wobble.
                let wob = ((i * 7 + k * 3) % 5) as f64 * 0.4 - 0.8;
                v.push((a.0 + (b.0 - a.0) * t + wob, a.1 + (b.1 - a.1) * t - wob));
            }
        }
        v.push((corners[0].0 + 2.0, corners[0].1 + 1.5));
        v
    }

    #[test]
    fn rectangles_ellipses_triangles_and_lines() {
        let r = recognize(&poly(&[(100.0, 100.0), (300.0, 100.0), (300.0, 200.0), (100.0, 200.0)], 30)).unwrap();
        assert!(matches!(r, Recognized::Rect { w, h, .. } if (w - 200.0).abs() < 4.0 && (h - 100.0).abs() < 4.0), "{r:?}");
        let ell: Vec<(f64, f64)> = (0..=80)
            .map(|i| {
                let a = i as f64 / 80.0 * std::f64::consts::TAU;
                (200.0 + 120.0 * a.cos() + (i % 3) as f64 * 0.5, 150.0 + 60.0 * a.sin())
            })
            .collect();
        assert!(matches!(recognize(&ell), Some(Recognized::Ellipse { .. })), "{:?}", recognize(&ell));
        let up = recognize(&poly(&[(200.0, 50.0), (300.0, 250.0), (100.0, 250.0)], 30)).unwrap();
        assert!(matches!(up, Recognized::Triangle { flip_v: false, .. }), "{up:?}");
        let down = recognize(&poly(&[(100.0, 50.0), (300.0, 50.0), (200.0, 250.0)], 30)).unwrap();
        assert!(matches!(down, Recognized::Triangle { flip_v: true, .. }), "{down:?}");
        let line: Vec<(f64, f64)> = (0..=40).map(|i| (10.0 + i as f64 * 5.0, 20.0 + i as f64 * 2.0 + (i % 2) as f64 * 0.6)).collect();
        assert!(matches!(recognize(&line), Some(Recognized::Line { x0, x1, .. }) if x0 < x1), "{:?}", recognize(&line));
    }

    #[test]
    fn scribbles_dots_and_garbage_stay_ink() {
        assert_eq!(recognize(&[]), None);
        assert_eq!(recognize(&[(1.0, 1.0)]), None);
        assert_eq!(recognize(&[(0.0, 0.0), (2.0, 2.0), (1.0, 3.0)]), None, "too small");
        let zig: Vec<(f64, f64)> = (0..40).map(|i| (i as f64 * 5.0, if i % 2 == 0 { 0.0 } else { 60.0 })).collect();
        assert_eq!(recognize(&zig), None, "an open zigzag is no line");
        assert_eq!(recognize(&[(f64::NAN, 0.0), (f64::INFINITY, 1.0)]), None);
        let there_and_back: Vec<(f64, f64)> = (0..=20).chain((0..=20).rev()).map(|i| (i as f64 * 10.0, 5.0)).collect();
        assert_eq!(recognize(&there_and_back), None);
    }

    #[test]
    fn ink_to_shape_command_converts_and_undoes() {
        use deckcraft_model::{InkStroke, Shape, ShapeId, ShapeKind};
        use serde_json::json;
        let mut s = crate::Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let rect = poly(&[(100.0, 100.0), (300.0, 100.0), (300.0, 200.0), (100.0, 200.0)], 30);
        let zig: Vec<(f64, f64)> = (0..40).map(|i| (i as f64 * 5.0, if i % 2 == 0 { 300.0 } else { 360.0 })).collect();
        let mk = |v: &[(f64, f64)]| InkStroke {
            points: v.iter().map(|(x, y)| (*x, *y, 0.5)).collect(),
            color: deckcraft_color::Rgba::rgb(200, 0, 0),
            width: 3.0,
            highlighter: false,
        };
        let strokes = vec![mk(&rect), mk(&zig)];
        s.edit(|doc, sel| {
            let id = ShapeId(doc.alloc_id().expect("id"));
            crate::shapes_mut(doc, sel).unwrap().push(Shape {
                id,
                name: "Ink".into(),
                xfrm: Some(deckcraft_geom::Xfrm::new(0.0, 0.0, 960.0, 540.0)),
                kind: ShapeKind::Ink { strokes },
                ..Default::default()
            });
            Ok(serde_json::Value::Null)
        })
        .unwrap();
        let n0 = s.doc().unwrap().shapes().len();
        let r = s.execute("draw.inkToShape", &json!({})).unwrap();
        assert_eq!(r["kept"], 1);
        let sh = s.doc().unwrap().shapes();
        assert_eq!(sh.len(), n0 + 1, "ink keeps the zigzag, one rectangle added");
        let made = sh.last().unwrap();
        assert!(matches!(&made.geom, deckcraft_model::Geom::Preset { name, .. } if name == "rect"));
        assert_eq!(made.line.as_ref().and_then(|l| l.width), Some(3.0));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().shapes().len(), n0);
        // Nothing recognisable: a no-op, not an error. Empty slide too.
        s.execute("draw.inkToShape", &json!({})).unwrap();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        assert_eq!(s.execute("draw.inkToShape", &json!({})).unwrap()["kept"], 0);
    }
}
