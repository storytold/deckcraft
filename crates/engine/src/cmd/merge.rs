//! Merge Shapes (Shape Format ▸ Insert Shapes): union, combine, fragment, intersect and subtract
//! the outlines of the selected shapes. The result takes the formatting of the first selected
//! shape, like PowerPoint.

use deckcraft_geom::{PathEl, Rect, Xfrm};
use deckcraft_model::{CustomPath, Geom, Shape, ShapeId, ShapeKind};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde_json::{Value, json};

use super::{CommandSpec, NO_ID_SPACE, bad, cmd, has_two, str_param, targets, xfrm_of};
use crate::{Result, Session};

/// Closed polygons (slide points) describing a filled area (non-zero winding).
type Area = Vec<Vec<[f64; 2]>>;

pub const OPS: [(&str, &str); 5] =
    [("union", "Union"), ("combine", "Combine"), ("fragment", "Fragment"), ("intersect", "Intersect"), ("subtract", "Subtract")];

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "shape.merge",
        "Merge Shapes",
        ["Shape Format", "Insert Shapes", "Merge Shapes"],
        None,
        "{op: union|combine|fragment|intersect|subtract, ids?: shapes in selection order (the first one's formatting is kept)} → {ids}",
        has_two,
        merge
    )]
}

/// The filled outline of `s` in slide points (curves flattened). A text box with no fill and no
/// outline is its letters, as in PowerPoint (merging text with a picture fills the letters).
fn area_of(doc: &deckcraft_model::Presentation, sel: &crate::Selection, s: &Shape) -> Area {
    if let Some((letters, _)) = letters_of(doc, sel, s) {
        return letters;
    }
    let x = xfrm_of(doc, sel, s);
    let geo = deckcraft_render::shape_geometry(s, x.w, x.h);
    let a = x.affine();
    let mut out: Area = vec![];
    for sp in geo.paths.iter().filter(|p| p.fill != deckcraft_geom::preset::FillMode::None) {
        flatten(&sp.path, a, &mut out);
    }
    out
}

/// The letters of a text box with no fill and no outline, in slide points, and their colour.
fn letters_of(doc: &deckcraft_model::Presentation, sel: &crate::Selection, s: &Shape) -> Option<(Area, Option<deckcraft_color::Rgba>)> {
    use deckcraft_model::resolve;
    use deckcraft_model::style::Fill;
    if !matches!(s.kind, ShapeKind::Shape) || s.text.as_ref().is_none_or(|t| t.is_empty()) {
        return None;
    }
    let ctx = super::text::ctx_for(doc, sel)?;
    let none = |f: Option<&Fill>| f.is_none_or(Fill::is_none);
    if !none(resolve::fill(&ctx, s).0.as_ref()) || !none(resolve::line(&ctx, s).0.fill.as_ref()) {
        return None;
    }
    let (path, colour) = deckcraft_render::text_outline(&ctx, s, xfrm_of(doc, sel, s).affine());
    let mut out: Area = vec![];
    flatten(&path, deckcraft_geom::Affine::IDENTITY, &mut out);
    (!out.is_empty()).then_some((out, colour))
}

/// Append the closed polygons of `path` (mapped by `a`) to `out`.
fn flatten(path: &deckcraft_geom::BezPath, a: deckcraft_geom::Affine, out: &mut Area) {
    let mut cur: Vec<[f64; 2]> = vec![];
    let flush = |cur: &mut Vec<[f64; 2]>, out: &mut Area| {
        if cur.len() > 2 {
            out.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    deckcraft_geom::preset_flatten(path, &mut |el| match el {
        PathEl::MoveTo(p) => {
            flush(&mut cur, out);
            let q = a * p;
            cur.push([q.x, q.y]);
        }
        PathEl::LineTo(p) => {
            let q = a * p;
            cur.push([q.x, q.y]);
        }
        PathEl::ClosePath => flush(&mut cur, out),
        _ => {}
    });
    flush(&mut cur, out);
}

/// Keep a picture where it was on the slide when its box becomes `to`: crop it to the new box.
/// Only for upright, unflipped pictures stretched over their box.
fn recrop(fill: &mut deckcraft_model::style::PictureFill, from: &Xfrm, to: &Xfrm) {
    use deckcraft_model::style::PictureMode;
    if from.rot != 0.0 || from.flip_h || from.flip_v || !matches!(fill.mode, PictureMode::Stretch { fill_rect } if fill_rect == [0.0; 4]) {
        return;
    }
    let [l, t, r, b] = fill.crop;
    let (fw, fh) = (1.0 - l - r, 1.0 - t - b);
    if !(fw > 1e-9 && fh > 1e-9 && from.w > 0.0 && from.h > 0.0) {
        return;
    }
    // The whole image in slide points.
    let (iw, ih) = (from.w / fw, from.h / fh);
    let (ix, iy) = (from.x - l * iw, from.y - t * ih);
    let c = [(to.x - ix) / iw, (to.y - iy) / ih, (ix + iw - to.x - to.w) / iw, (iy + ih - to.y - to.h) / ih];
    if c.iter().all(|v| v.is_finite()) {
        fill.crop = c;
    }
}

fn overlay(a: &Area, b: &Area, rule: OverlayRule) -> Area {
    if a.is_empty() && matches!(rule, OverlayRule::Union | OverlayRule::Xor) {
        return b.clone();
    }
    if b.is_empty() {
        return if matches!(rule, OverlayRule::Intersect) { vec![] } else { a.clone() };
    }
    a.overlay(b, rule, FillRule::NonZero).into_iter().flatten().filter(|c| c.len() > 2).collect()
}

fn area_size(a: &Area) -> f64 {
    a.iter()
        .map(|c| {
            let n = c.len();
            (0..n).map(|i| c[i][0] * c[(i + 1) % n][1] - c[(i + 1) % n][0] * c[i][1]).sum::<f64>() / 2.0
        })
        .sum::<f64>()
        .abs()
}

/// The merged areas for `op` (one per resulting shape).
pub fn merge_areas(op: &str, areas: &[Area]) -> Vec<Area> {
    let Some((first, rest)) = areas.split_first() else { return vec![] };
    let fold = |rule| rest.iter().fold(first.clone(), |acc, b| overlay(&acc, b, rule));
    let out = match op {
        "union" => vec![fold(OverlayRule::Union)],
        "combine" => vec![fold(OverlayRule::Xor)],
        "intersect" => vec![fold(OverlayRule::Intersect)],
        "subtract" => {
            let others = rest.iter().fold(Area::new(), |acc, b| overlay(&acc, b, OverlayRule::Union));
            vec![overlay(first, &others, OverlayRule::Difference)]
        }
        _ => {
            // Fragment: every region of the arrangement becomes its own shape.
            let mut pieces: Vec<Area> = vec![first.clone()];
            let mut covered = first.clone();
            for b in rest {
                let mut next = vec![];
                for p in &pieces {
                    next.push(overlay(p, b, OverlayRule::Intersect));
                    next.push(overlay(p, b, OverlayRule::Difference));
                }
                next.push(overlay(b, &covered, OverlayRule::Difference));
                covered = overlay(&covered, b, OverlayRule::Union);
                pieces = next;
            }
            // Split pieces into connected outer contours with their holes is not needed for
            // rendering; keep each piece as one shape.
            pieces
        }
    };
    out.into_iter().filter(|a| area_size(a) > 0.01).collect()
}

/// A custom-geometry box for `area`: its bounds and path data relative to them.
fn area_geom(area: &Area) -> Option<(Xfrm, Geom)> {
    let mut b: Option<Rect> = None;
    for p in area.iter().flatten() {
        let r = Rect::new(p[0], p[1], p[0], p[1]);
        b = Some(b.map_or(r, |x| x.union(r)));
    }
    let b = b?;
    let (w, h) = (b.width().max(0.01), b.height().max(0.01));
    let f = |v: f64| {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let mut d = String::new();
    for c in area {
        for (i, p) in c.iter().enumerate() {
            d += &format!("{}{} {} ", if i == 0 { "M " } else { "L " }, f(p[0] - b.x0), f(p[1] - b.y0));
        }
        d += "Z ";
    }
    let geom = Geom::Custom { paths: vec![CustomPath { w, h, d: d.trim_end().to_string(), ..Default::default() }] };
    Some((Xfrm::new(b.x0, b.y0, w, h), geom))
}

fn merge(s: &mut Session, p: &Value) -> Result<Value> {
    let op = str_param(p, "op").unwrap_or("union").to_ascii_lowercase();
    if !OPS.iter().any(|o| o.0 == op) {
        return Err(bad("shape.merge", format!("unknown op `{op}` (union, combine, fragment, intersect, subtract)")));
    }
    let ids = targets(s, p)?;
    let st = s.doc()?;
    let shapes: Vec<Shape> = ids.iter().filter_map(|id| st.shape(*id).cloned()).collect();
    let mergeable = |x: &Shape| matches!(x.kind, ShapeKind::Shape | ShapeKind::Picture { .. }) && !x.is_line();
    if shapes.len() < 2 || !shapes.iter().all(mergeable) {
        return Err(bad("shape.merge", "select two or more shapes (not lines, groups, tables or charts)"));
    }
    let areas: Vec<Area> = shapes.iter().map(|x| area_of(&st.doc, &st.selection, x)).collect();
    let results = merge_areas(&op, &areas);
    let mut template = shapes[0].clone();
    let template_box = xfrm_of(&st.doc, &st.selection, &template);
    // Letters merged become shapes filled with the text's colour.
    let letters = letters_of(&st.doc, &st.selection, &template);
    if let Some((_, colour)) = &letters {
        use deckcraft_model::style::{ColorRef, Fill, Line};
        template.fill = Some(Fill::solid(ColorRef::rgb(colour.unwrap_or(deckcraft_color::Rgba::BLACK))));
        template.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
        template.text_box = false;
    }
    let drop_text = letters.is_some() || shapes.iter().skip(1).any(|x| letters_of(&st.doc, &st.selection, x).is_some());
    let mut new_ids = vec![];
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.merge", "no slide"))?;
        let at = list.iter().position(|x| x.id == template.id).unwrap_or(list.len());
        let mut made = vec![];
        for (k, area) in results.iter().enumerate() {
            let Some((x, geom)) = area_geom(area) else { continue };
            let mut sh = template.clone();
            sh.id = ShapeId(0);
            sh.ph = None;
            sh.xfrm = Some(x);
            sh.geom = geom;
            sh.name = format!("Freeform {}", k + 1);
            if let ShapeKind::Picture { fill } = &mut sh.kind {
                recrop(fill, &template_box, &x);
            }
            // Text stays with the first piece only, and not at all once it became outlines.
            if k > 0 || drop_text {
                sh.text = None;
            }
            made.push(sh);
        }
        for sh in &mut made {
            sh.id = ShapeId(doc.alloc_id().ok_or_else(|| bad("shape.merge", NO_ID_SPACE))?);
            new_ids.push(sh.id);
        }
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("shape.merge", "no slide"))?;
        let at = at.min(list.len());
        for (k, sh) in made.into_iter().enumerate() {
            list.insert(at + k, sh);
        }
        list.retain(|x| !ids.contains(&x.id));
        sel.shapes = new_ids.clone();
        let _ = list;
        Ok(())
    })?;
    Ok(json!({"ids": new_ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
}

#[cfg(test)]
mod tests {
    use deckcraft_model::{ShapeId, ShapeKind};
    use serde_json::json;

    use super::{Area, area_of, area_size, letters_of, merge_areas};
    use crate::Session;
    use crate::cmd::base64_encode;

    fn square(x: f64, y: f64, s: f64) -> Area {
        vec![vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]]
    }

    #[test]
    fn boolean_ops_have_the_expected_areas() {
        let a = square(0.0, 0.0, 10.0);
        let b = square(5.0, 5.0, 10.0);
        let areas = [a, b];
        let size = |op| merge_areas(op, &areas).iter().map(area_size).sum::<f64>();
        assert!((size("union") - 175.0).abs() < 1e-6);
        assert!((size("intersect") - 25.0).abs() < 1e-6);
        assert!((size("subtract") - 75.0).abs() < 1e-6);
        assert!((size("combine") - 150.0).abs() < 1e-6);
        let frags = merge_areas("fragment", &areas);
        assert_eq!(frags.len(), 3);
        assert!((frags.iter().map(area_size).sum::<f64>() - 175.0).abs() < 1e-6);
        // Disjoint intersect is empty.
        assert!(merge_areas("intersect", &[square(0.0, 0.0, 1.0), square(5.0, 5.0, 1.0)]).is_empty());
    }

    fn picture_and_text(s: &mut Session) -> (ShapeId, ShapeId) {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(40, 20, image::Rgba([0, 128, 255, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("png");
        let _ = s.execute("slide.new", &json!({"layout": "blank"}));
        let r = s
            .execute("insert.picture", &json!({"name": "a.png", "data": base64_encode(&png), "rect": [100.0, 100.0, 400.0, 200.0]}))
            .expect("picture");
        let pic = ShapeId(r["id"].as_u64().expect("id") as u32);
        let r = s.execute("insert.textBox", &json!({"rect": [150.0, 120.0, 300.0, 100.0], "text": "HI"})).expect("text box");
        let text = ShapeId(r["id"].as_u64().expect("id") as u32);
        (pic, text)
    }

    #[test]
    fn a_picture_intersected_with_text_fills_the_letters() {
        // #653: the letters didn't change and the picture vanished (the text box counted as its
        // whole box, and the merge took the text's outline but kept the box's formatting).
        let mut s = Session::with_new();
        let (pic, text) = picture_and_text(&mut s);
        let st = s.doc().expect("doc");
        let letters = letters_of(&st.doc, &st.selection, st.shape(text).expect("text")).expect("the text box is its letters").0;
        let box_area = 300.0 * 100.0;
        assert!(area_size(&letters) > 10.0 && area_size(&letters) < box_area * 0.5, "{}", area_size(&letters));
        let r = s.execute("shape.merge", &json!({"op": "intersect", "ids": [pic.0, text.0]})).expect("merge");
        let ids = r["ids"].as_array().expect("ids");
        assert_eq!(ids.len(), 1);
        let st = s.doc().expect("doc");
        let out = st.shape(ShapeId(ids[0].as_u64().expect("id") as u32)).expect("result").clone();
        let ShapeKind::Picture { fill } = &out.kind else { panic!("the picture fills the letters: {:?}", out.kind) };
        assert!(out.text.is_none(), "the letters are outlines now");
        // The result is the letters' outline, well inside the text box.
        let x = out.xfrm.expect("box");
        assert!(x.x >= 150.0 && x.y >= 120.0 && x.x + x.w <= 450.0 && x.y + x.h <= 220.0, "{x:?}");
        let merged = area_of(&st.doc, &st.selection, &out);
        assert!((area_size(&merged) - area_size(&letters)).abs() < area_size(&letters) * 0.05, "{} vs {}", area_size(&merged), area_size(&letters));
        // The picture stays where it was on the slide: cropped to the new box.
        let [l, t, rr, b] = fill.crop;
        let iw = x.w / (1.0 - l - rr);
        let ih = x.h / (1.0 - t - b);
        assert!((x.x - l * iw - 100.0).abs() < 1e-6 && (iw - 400.0).abs() < 1e-6, "{:?}", fill.crop);
        assert!((x.y - t * ih - 100.0).abs() < 1e-6 && (ih - 200.0).abs() < 1e-6, "{:?}", fill.crop);
    }

    #[test]
    fn text_merged_first_becomes_shapes_in_its_colour() {
        let mut s = Session::with_new();
        let (pic, text) = picture_and_text(&mut s);
        let r = s.execute("shape.merge", &json!({"op": "union", "ids": [text.0, pic.0]})).expect("merge");
        let st = s.doc().expect("doc");
        let out = st.shape(ShapeId(r["ids"][0].as_u64().expect("id") as u32)).expect("result");
        assert!(matches!(out.kind, ShapeKind::Shape) && out.text.is_none());
        assert!(matches!(out.fill, Some(deckcraft_model::style::Fill::Solid { .. })), "{:?}", out.fill);
    }
}
