use super::*;

pub(super) fn sites(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id", "shape.sites")?.ok_or_else(|| bad("shape.sites", "missing `id`"))?;
    let st = s.doc()?;
    let sh = st.shape(id).ok_or_else(|| bad("shape.sites", "no such shape"))?;
    Ok(json!(crate::connect::sites(&st.doc, &st.selection, sh).iter().map(|q| [q.x, q.y]).collect::<Vec<_>>()))
}

pub(super) fn connect(s: &mut Session, p: &Value) -> Result<Value> {
    let (Some(from), Some(to)) = (id_param(p, "from", "shape.connect")?, id_param(p, "to", "shape.connect")?) else {
        return Err(bad("shape.connect", "missing `from` / `to`"));
    };
    let st = s.doc()?;
    let site_list = |id| st.shape(id).map(|sh| crate::connect::sites(&st.doc, &st.selection, sh)).unwrap_or_default();
    let (a, b) = (site_list(from), site_list(to));
    if a.is_empty() || b.is_empty() {
        return Err(bad("shape.connect", "both shapes need connection sites (lines and groups have none)"));
    }
    let pick = |v: &[deckcraft_geom::Point], key: &str| usize_param(p, key).filter(|i| *i < v.len());
    let (ia, ib) = match (pick(&a, "fromSite"), pick(&b, "toSite")) {
        (Some(i), Some(j)) => (i, j),
        (fi, fj) => {
            let mut best = (f64::MAX, 0, 0);
            for (i, pa) in a.iter().enumerate().filter(|(i, _)| fi.is_none_or(|f| f == *i)) {
                for (j, pb) in b.iter().enumerate().filter(|(j, _)| fj.is_none_or(|f| f == *j)) {
                    let d = (*pa - *pb).hypot();
                    if d < best.0 {
                        best = (d, i, j);
                    }
                }
            }
            (best.1, best.2)
        }
    };
    let (p0, p1) = (a[ia], b[ib]);
    let id = match id_param(p, "id", "shape.connect")? {
        Some(id) => id,
        None => {
            let preset = str_param(p, "preset").unwrap_or("straightConnector1");
            let r = s.execute("shape.insert", &json!({"preset": preset, "rect": [p0.x, p0.y, 1.0, 1.0]}))?;
            deckcraft_model::ShapeId(r.get("id").and_then(Value::as_u64).ok_or_else(|| bad("shape.connect", "insert failed"))? as u32)
        }
    };
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.connect", "no slide"))?;
        let sh = deckcraft_model::find_shape_mut(list, id).ok_or_else(|| bad("shape.connect", "no such line"))?;
        if !sh.is_line() {
            return Err(bad("shape.connect", "`id` is not a line or connector"));
        }
        sh.xfrm = Some(crate::tools::line_xfrm(p0, p1, 0.0));
        sh.kind = deckcraft_model::ShapeKind::Connector { start: Some((from, ia as u32)), end: Some((to, ib as u32)) };
        Ok(())
    })?;
    Ok(json!({"id": id.0, "fromSite": ia, "toSite": ib}))
}

/// Path data through `pts` (shape-local): straight segments, or a Catmull-Rom curve as cubics.
pub fn freeform_path(pts: &[deckcraft_geom::Point], closed: bool, smooth: bool) -> String {
    let f = |v: f64| {
        let s = format!("{:.2}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let mut d = format!("M {} {}", f(pts[0].x), f(pts[0].y));
    let n = pts.len();
    if smooth && n > 2 {
        let at = |i: isize| -> deckcraft_geom::Point {
            if closed { pts[i.rem_euclid(n as isize) as usize] } else { pts[i.clamp(0, n as isize - 1) as usize] }
        };
        let segs = if closed { n } else { n - 1 };
        for i in 0..segs as isize {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let c1 = p1 + (p2 - p0) / 6.0;
            let c2 = p2 - (p3 - p1) / 6.0;
            d += &format!(" C {} {} {} {} {} {}", f(c1.x), f(c1.y), f(c2.x), f(c2.y), f(p2.x), f(p2.y));
        }
    } else {
        for q in &pts[1..] {
            d += &format!(" L {} {}", f(q.x), f(q.y));
        }
    }
    if closed {
        d += " Z";
    }
    d
}

pub(super) fn freeform(s: &mut Session, p: &Value) -> Result<Value> {
    let pts: Vec<deckcraft_geom::Point> = p
        .get("points")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| Some(deckcraft_geom::Point::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?)))
                .filter(|q| q.x.is_finite() && q.y.is_finite())
                .take(20_000)
                .collect()
        })
        .unwrap_or_default();
    if pts.len() < 2 {
        return Err(bad("shape.freeform", "need at least two points"));
    }
    let closed = bool_or(p, "closed", false) && pts.len() > 2;
    let smooth = bool_or(p, "smooth", false);
    let mut bb = deckcraft_geom::Rect::from_points(pts[0], pts[0]);
    for q in &pts {
        bb = bb.union_pt(*q);
    }
    let (w, h) = (bb.width().max(1.0), bb.height().max(1.0));
    let local: Vec<deckcraft_geom::Point> = pts.iter().map(|q| deckcraft_geom::Point::new(q.x - bb.x0, q.y - bb.y0)).collect();
    let d = freeform_path(&local, closed, smooth);
    // Closed shapes take the default shape style, open paths the line style.
    let r = s.execute("shape.insert", &json!({"preset": if closed { "rect" } else { "line" }, "rect": [bb.x0, bb.y0, w, h]}))?;
    let id = deckcraft_model::ShapeId(r.get("id").and_then(Value::as_u64).ok_or_else(|| bad("shape.freeform", "insert failed"))? as u32);
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.freeform", "no slide"))?;
        let sh = deckcraft_model::find_shape_mut(list, id).ok_or_else(|| bad("shape.freeform", "insert failed"))?;
        sh.geom = Geom::Custom {
            paths: vec![deckcraft_model::CustomPath {
                w,
                h,
                d: d.clone(),
                fill: if closed { deckcraft_geom::preset::FillMode::Norm } else { deckcraft_geom::preset::FillMode::None },
                stroke: true,
            }],
        };
        sh.xfrm = Some(deckcraft_geom::Xfrm::new(bb.x0, bb.y0, w, h));
        sh.kind = deckcraft_model::ShapeKind::Shape;
        sh.name = format!("Freeform {}", id.0);
        Ok(())
    })?;
    Ok(json!({"id": id.0}))
}

/// Draw › Ink to Shape: each recognisable stroke of the slide's ink becomes a preset shape
/// outlined in the stroke's colour and width; the rest stays ink. One undo step.
pub(super) fn ink_to_shape(s: &mut Session, _p: &Value) -> Result<Value> {
    use crate::inkshape::{Recognized, recognize};
    use deckcraft_model::{Shape, ShapeId, ShapeKind};
    let shapes = s.doc()?.shapes();
    let Some(ink) = shapes.iter().find(|sh| matches!(sh.kind, ShapeKind::Ink { .. })) else {
        return Ok(json!({"shapes": [], "kept": 0}));
    };
    let (ink_id, off) = (ink.id, ink.xfrm.map(|x| (x.x, x.y)).unwrap_or((0.0, 0.0)));
    let ShapeKind::Ink { strokes } = &ink.kind else { return Ok(json!({"shapes": [], "kept": 0})) };
    let found: Vec<Option<Recognized>> = strokes
        .iter()
        .map(|st| if st.highlighter { None } else { recognize(&st.points.iter().map(|(x, y, _)| (x + off.0, y + off.1)).collect::<Vec<_>>()) })
        .collect();
    if found.iter().all(Option::is_none) {
        return Ok(json!({"shapes": [], "kept": strokes.len()}));
    }
    s.edit(|doc, sel| {
        let mut new = vec![];
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("draw.inkToShape", "no slide"))?;
        let Some(pos) = list.iter().position(|sh| sh.id == ink_id) else { return Err(bad("draw.inkToShape", "ink gone")) };
        let ShapeKind::Ink { strokes } = &mut list[pos].kind else { return Err(bad("draw.inkToShape", "not ink")) };
        let all = std::mem::take(strokes);
        let mut kept = vec![];
        let mut made = vec![];
        for (st, r) in all.into_iter().zip(found.iter()) {
            let Some(r) = r else {
                kept.push(st);
                continue;
            };
            let (preset, xfrm) = match *r {
                Recognized::Line { x0, y0, x1, y1 } => {
                    ("line", crate::tools::line_xfrm(deckcraft_geom::Point::new(x0, y0), deckcraft_geom::Point::new(x1, y1), 0.0))
                }
                Recognized::Rect { x, y, w, h } => ("rect", deckcraft_geom::Xfrm::new(x, y, w, h)),
                Recognized::Ellipse { x, y, w, h } => ("ellipse", deckcraft_geom::Xfrm::new(x, y, w, h)),
                Recognized::Triangle { x, y, w, h, flip_v } => ("triangle", deckcraft_geom::Xfrm { flip_v, ..deckcraft_geom::Xfrm::new(x, y, w, h) }),
            };
            made.push((preset, xfrm, st.color, st.width));
        }
        let remaining = kept.len();
        *strokes = kept;
        if remaining == 0 {
            list.remove(pos);
        }
        for (preset, xfrm, color, width) in made {
            let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("draw.inkToShape", NO_ID_SPACE))?);
            let name = format!(
                "{} {}",
                match preset {
                    "line" => "Straight Connector",
                    "rect" => "Rectangle",
                    "ellipse" => "Oval",
                    _ => "Isosceles Triangle",
                },
                id.0
            );
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("draw.inkToShape", "no slide"))?;
            list.push(Shape {
                id,
                name,
                xfrm: Some(xfrm),
                geom: Geom::preset(preset),
                fill: Some(Fill::None),
                line: Some(Line::solid(ColorRef::rgb(color), width.clamp(0.25, 100.0))),
                kind: ShapeKind::Shape,
                ..Default::default()
            });
            new.push(id);
        }
        sel.shapes = new.clone();
        Ok(json!({"shapes": new.iter().map(|i| i.0).collect::<Vec<_>>(), "kept": remaining}))
    })
}
