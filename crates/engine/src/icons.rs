//! Insert › Icons: Lucide stroke icons as custom geometry.
//!
//! The paths in `icon_data.rs` are the Lucide 1.56.0 set (24×24 centerlines, fill none). They are
//! converted to DeckCraft's `M`/`L`/`C`/`Q`/`Z` syntax when inserted, so the shape is a stroked
//! outline rather than an SVG picture. A fill paints closed subpaths only.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use deckcraft_geom::{BezPath, PathEl};

/// Design size of every icon, in path units. Lucide's viewBox is `0 0 24 24`.
pub const VIEW: f64 = 24.0;

/// One icon from the Insert gallery.
pub struct Icon {
    pub name: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    /// Design stroke width in viewBox units (Lucide's default is 2).
    pub stroke: f64,
    /// Comma-separated search tags (Lucide tags and category slugs, lower case).
    pub tags: &'static str,
    /// SVG path data. Independent subpaths (separate SVG elements) are separated by ` | `.
    pub d: &'static str,
}

include!("icon_data.rs");

/// Every icon, in gallery order (category, then name).
pub fn icons() -> &'static [Icon] {
    ICONS
}

/// Category titles in gallery order, one each.
pub fn categories() -> Vec<&'static str> {
    let mut out = Vec::new();
    for icon in ICONS {
        if out.last() != Some(&icon.category) {
            out.push(icon.category);
        }
    }
    out
}

/// `name` is the Lucide id (`arrow-right`, `arrow_right`) or the gallery label (`Arrow Right`).
pub fn find(name: &str) -> Option<&'static Icon> {
    let raw = name.trim();
    if raw.is_empty() {
        return None;
    }
    let key = raw.to_ascii_lowercase().replace('_', "-");
    ICONS.iter().find(|i| i.name == key || i.label.eq_ignore_ascii_case(raw))
}

/// Whether a search box should show `icon`. `query` matches the name, label, category or a tag.
pub fn matches(icon: &Icon, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let q = q.to_ascii_lowercase();
    icon.name.contains(&q)
        || icon.label.to_ascii_lowercase().contains(&q)
        || icon.category.to_ascii_lowercase().contains(&q)
        || icon.tags.split(',').any(|t| t.contains(&q))
}

/// The icon's outline in the 24×24 viewBox. Parsed on first use and kept.
pub fn outline(name: &str) -> Option<BezPath> {
    let icon = find(name)?;
    let mut map = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(bp) = map.get(icon.name) {
        return Some(bp.clone());
    }
    let bp = bezpath(icon.d)?;
    map.insert(icon.name, bp.clone());
    Some(bp)
}

/// DeckCraft path syntax for `name`, in the 24×24 viewBox.
pub fn path_data(name: &str) -> Option<String> {
    outline(name).as_ref().and_then(deck_path)
}

/// A custom path written in an icon's 24×24 design space.
pub fn is_icon_box(w: f64, h: f64) -> bool {
    w.is_finite() && h.is_finite() && (w - VIEW).abs() < 1e-3 && (h - VIEW).abs() < 1e-3
}

/// True when every subpath of `geom` lives in the icon design box.
pub fn is_icon_geom(geom: &deckcraft_model::Geom) -> bool {
    match geom {
        deckcraft_model::Geom::Custom { paths } => !paths.is_empty() && paths.iter().all(|p| is_icon_box(p.w, p.h)),
        _ => false,
    }
}

/// Stroke width in points for a shape `w`×`h` points. Lucide's stroke is `stroke` units on a
/// 24-unit box, so a 72 pt icon is a 6 pt line. Resizing the shape later does not change it.
pub fn stroke_width(icon: &Icon, w: f64, h: f64) -> f64 {
    let span = w.min(h).abs().min(4_000.0);
    let design = if icon.stroke.is_finite() && icon.stroke > 0.0 { icon.stroke } else { 2.0 };
    (span * design / VIEW).clamp(0.25, 400.0)
}

/// Split a DeckCraft path on each `M`, keeping the command with its subpath.
pub fn split_deck_path(d: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for tok in d.split_whitespace() {
        if tok == "M" && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(tok);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// True when the subpath explicitly closes (`Z`). An open stroke must not be filled: an implicit
/// close draws a chord across the gap.
pub fn path_is_closed(d: &str) -> bool {
    d.split_whitespace().any(|t| t == "Z")
}

fn cache() -> &'static Mutex<HashMap<&'static str, BezPath>> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, BezPath>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Parse SVG path data, one independent subpath at a time (` | ` resets the current point).
fn bezpath(d: &str) -> Option<BezPath> {
    let mut out = BezPath::new();
    for part in d.split(" | ") {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let bp = BezPath::from_svg(part).ok()?;
        for el in bp.elements() {
            out.push(*el);
        }
    }
    if out.elements().is_empty() { None } else { Some(out) }
}

fn deck_path(bp: &BezPath) -> Option<String> {
    let mut out = String::new();
    let mut started = false;
    for el in bp.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                push(&mut out, &format!("M {} {}", num(p.x), num(p.y)));
                started = true;
            }
            PathEl::LineTo(p) if started => push(&mut out, &format!("L {} {}", num(p.x), num(p.y))),
            PathEl::QuadTo(a, b) if started => {
                push(&mut out, &format!("Q {} {} {} {}", num(a.x), num(a.y), num(b.x), num(b.y)));
            }
            PathEl::CurveTo(a, b, c) if started => {
                push(&mut out, &format!("C {} {} {} {} {} {}", num(a.x), num(a.y), num(b.x), num(b.y), num(c.x), num(c.y)));
            }
            PathEl::ClosePath if started => out.push_str(" Z"),
            _ => {}
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn push(out: &mut String, cmd: &str) {
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(cmd);
}

fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_color::SchemeSlot;
    use deckcraft_geom::preset::FillMode;
    use deckcraft_model::{ColorRef, Fill, Geom, ShapeId, ShapeKind};
    use serde_json::json;

    use crate::Session;

    #[test]
    fn every_icon_converts_and_a_second_subpath_keeps_its_own_origin() {
        assert!(icons().len() >= 1_000, "{}", icons().len());
        assert!(categories().len() >= 20);
        for icon in icons() {
            let bp = outline(icon.name).unwrap_or_else(|| panic!("{} did not parse", icon.name));
            assert!(bp.elements().len() >= 2, "{}", icon.name);
            let d = path_data(icon.name).unwrap_or_else(|| panic!("{}", icon.name));
            assert!(d.starts_with('M'), "{d}");
            let again = deckcraft_render::parse_path(&d);
            assert!(!again.elements().is_empty(), "{}", icon.name);
        }
        // `m19 12…` is a second SVG element, not a continuation of the shaft.
        let down = path_data("arrow-down").unwrap();
        assert!(down.contains("M 19 12"), "{down}");
        // Relative arcs in the star become cubics our renderer understands.
        let star = path_data("star").unwrap();
        assert!(star.contains(" C "), "{star}");
        assert!(path_is_closed(&star));
        assert!(split_deck_path(&path_data("user").unwrap()).len() >= 2);
        assert!(split_deck_path(&path_data("settings").unwrap()).len() >= 2);
        assert!(find("arrow_right").is_some_and(|i| i.name == "arrow-right" && i.label == "Arrow Right"));
        assert!(find("Star").is_some_and(|i| i.name == "star" && i.category == "Shapes"));
        assert!(find("star").is_some_and(|i| i.tags.split(',').any(|t| t == "favorite")));
        assert!(matches(find("star").unwrap(), "favorite"));
        assert!(find("nope").is_none());
    }

    #[test]
    fn insert_icon_strokes_the_outline_and_round_trips_through_pptx() {
        let mut s = Session::with_new();
        assert!(s.execute("insert.icon", &json!({})).is_err());
        assert!(s.execute("insert.icon", &json!({"name": "not-an-icon"})).is_err());
        let listed = s.execute("insert.icons", &json!({})).unwrap();
        let list = listed.as_array().unwrap();
        assert!(list.len() >= 1_000);
        assert!(list.iter().any(|i| i["name"] == "star" && i["category"] == "Shapes" && i["tags"].as_str().unwrap().contains("favorite")));

        let before = s.doc().unwrap().shapes().len();
        let id = s.execute("insert.icon", &json!({"name": "star", "rect": [100.0, 80.0, 72.0, 72.0]})).unwrap()["id"].as_u64().unwrap();
        {
            let st = s.doc().unwrap();
            assert_eq!(st.shapes().len(), before + 1);
            let sh = st.shape(ShapeId(id as u32)).unwrap();
            assert_eq!(sh.name, "Star");
            assert!(sh.fill.as_ref().is_some_and(|f| f.is_none()));
            let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
            assert_eq!(paths.len(), 1);
            assert_eq!(paths[0].fill, FillMode::None);
            assert!(paths[0].stroke);
            assert!(is_icon_box(paths[0].w, paths[0].h));
            assert!(paths[0].d.contains('M') && paths[0].d.contains('C'));
            let width = sh.line.as_ref().and_then(|l| l.width).unwrap();
            assert!((width - 6.0).abs() < 1e-6, "{width}");
            let img = deckcraft_render::render_slide(&st.doc, st.selection.slide, &deckcraft_render::RenderOpts { scale: 2.0, ..Default::default() });
            let ink = ink_in(&img, 100.0, 80.0, 72.0, 72.0, 2.0);
            assert!(ink > 40, "star stroke pixels {ink}");
            let mid = img.pixel((100.0 + 36.0) as u32 * 2, (80.0 + 36.0) as u32 * 2);
            let sum = mid[0] as u16 + mid[1] as u16 + mid[2] as u16;
            assert!(sum > 680, "a stroked star is hollow: {mid:?}");
        }
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().shapes().len(), before);

        s.execute("insert.icon", &json!({"name": "arrow-right", "rect": [200.0, 200.0, 96.0, 96.0]})).unwrap();
        {
            let st = s.doc().unwrap();
            let sh = st.shapes().iter().find(|s| s.name == "Arrow Right").unwrap();
            let width = sh.line.as_ref().and_then(|l| l.width).unwrap();
            assert!((width - 8.0).abs() < 1e-6, "{width}");
            let img = deckcraft_render::render_slide(&st.doc, st.selection.slide, &deckcraft_render::RenderOpts { scale: 2.0, ..Default::default() });
            assert!(ink_in(&img, 200.0, 200.0, 96.0, 96.0, 2.0) > 40);
        }

        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let p = deckcraft_pptx::import(&bytes).unwrap();
        let sh = p.slides[0].shapes.iter().find(|s| s.name == "Arrow Right").unwrap();
        let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
        assert!(paths.iter().any(|path| path.fill == FillMode::None && path.stroke && path.d.contains('M')));
        let width = sh.line.as_ref().and_then(|l| l.width).unwrap();
        assert!((width - 8.0).abs() < 0.05, "{width}");
    }

    #[test]
    fn fill_and_colour_paint_closed_subpaths_only() {
        let mut s = Session::with_new();
        assert!(s.execute("insert.iconStyle", &json!({})).is_err());
        let star =
            s.execute("insert.icon", &json!({"name": "star", "rect": [100.0, 80.0, 72.0, 72.0], "fill": true})).unwrap()["id"].as_u64().unwrap();
        {
            let st = s.doc().unwrap();
            let sh = st.shape(ShapeId(star as u32)).unwrap();
            assert!(sh.fill.as_ref().is_some_and(|f| matches!(f, Fill::Solid { .. })));
            let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
            assert_eq!(paths.len(), 1);
            assert_eq!(paths[0].fill, FillMode::Norm);
            let img = deckcraft_render::render_slide(&st.doc, st.selection.slide, &deckcraft_render::RenderOpts { scale: 2.0, ..Default::default() });
            let mid = img.pixel((100.0 + 36.0) as u32 * 2, (80.0 + 36.0) as u32 * 2);
            let sum = mid[0] as u16 + mid[1] as u16 + mid[2] as u16;
            assert!(sum < 400, "a filled star is solid: {mid:?}");
        }
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().shapes().iter().all(|sh| sh.name != "Star"));

        let id =
            s.execute("insert.icon", &json!({"name": "user", "rect": [40.0, 40.0, 80.0, 80.0], "color": "accent2"})).unwrap()["id"].as_u64().unwrap();
        {
            let sh = s.doc().unwrap().shape(ShapeId(id as u32)).unwrap();
            assert_eq!(sh.line.as_ref().and_then(|l| l.fill.clone()), Some(Fill::solid(ColorRef::scheme(SchemeSlot::Accent2))));
            let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
            assert_eq!(paths.len(), 1, "an unfilled icon stays one path");
            assert!(paths.iter().all(|p| p.fill == FillMode::None));
        }
        assert!(s.execute("insert.iconStyle", &json!({})).is_err());
        assert!(s.execute("insert.iconStyle", &json!({"color": 5})).is_err());
        s.execute("insert.iconStyle", &json!({"fill": true})).unwrap();
        {
            let sh = s.doc().unwrap().shape(ShapeId(id as u32)).unwrap();
            let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
            assert!(paths.len() >= 2, "user is a head plus a body");
            assert!(paths.iter().any(|p| p.fill == FillMode::Norm));
            assert!(paths.iter().any(|p| p.fill == FillMode::None), "the open body stays a stroke");
            assert!(matches!(sh.fill.as_ref(), Some(Fill::Solid { color }) if color == &ColorRef::scheme(SchemeSlot::Accent2)));
        }
        s.execute("insert.iconStyle", &json!({"fill": false})).unwrap();
        {
            let sh = s.doc().unwrap().shape(ShapeId(id as u32)).unwrap();
            assert!(sh.fill.as_ref().is_some_and(|f| f.is_none()));
            let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
            assert!(paths.iter().all(|p| p.fill == FillMode::None));
        }
        s.execute("edit.undo", &json!({})).unwrap();
        {
            let sh = s.doc().unwrap().shape(ShapeId(id as u32)).unwrap();
            assert!(matches!(sh.fill.as_ref(), Some(Fill::Solid { .. })));
        }

        s.execute("shape.insert", &json!({"preset": "rect", "rect": [300.0, 40.0, 40.0, 40.0]})).unwrap();
        assert!(s.execute("insert.iconStyle", &json!({"color": "accent1"})).is_err());
        assert!(s.execute("insert.iconToShapes", &json!({})).is_err());
        assert!(s.doc().unwrap().doc.validate().is_empty());
    }

    #[test]
    fn icon_to_shapes_groups_the_parts_and_undoes_in_one_step() {
        let mut s = Session::with_new();
        let id = s.execute("insert.icon", &json!({"name": "user", "rect": [120.0, 90.0, 96.0, 96.0]})).unwrap()["id"].as_u64().unwrap();
        let before = s.doc().unwrap().shapes().len();
        let v = s.execute("insert.iconToShapes", &json!({})).unwrap();
        assert_eq!(v["grouped"], true);
        {
            let st = s.doc().unwrap();
            assert_eq!(st.shapes().len(), before);
            let sh = st.shapes().iter().find(|s| s.name == "User").unwrap();
            let ShapeKind::Group { children, .. } = &sh.kind else { panic!("{:?}", sh.kind) };
            assert!(children.len() >= 2, "{}", children.len());
            assert!(children.iter().all(|c| is_icon_geom(&c.geom)));
            let img = deckcraft_render::render_slide(&st.doc, st.selection.slide, &deckcraft_render::RenderOpts { scale: 2.0, ..Default::default() });
            let ink = ink_in(&img, 120.0, 90.0, 96.0, 96.0, 2.0);
            assert!(ink > 40, "grouped user still draws: {ink}");
        }
        s.execute("edit.undo", &json!({})).unwrap();
        {
            let st = s.doc().unwrap();
            assert_eq!(st.shapes().len(), before);
            let sh = st.shape(ShapeId(id as u32)).unwrap();
            assert_eq!(sh.name, "User");
            assert!(matches!(sh.kind, ShapeKind::Shape));
            assert!(is_icon_geom(&sh.geom));
        }

        s.execute("insert.icon", &json!({"name": "star", "rect": [20.0, 20.0, 48.0, 48.0]})).unwrap();
        let v = s.execute("insert.iconToShapes", &json!({})).unwrap();
        assert_eq!(v["grouped"], false);
        let sh = s.doc().unwrap().shapes().iter().find(|s| s.name == "Star").unwrap();
        assert!(matches!(sh.kind, ShapeKind::Shape));
        let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
        assert_eq!(paths.len(), 1);
    }

    fn ink_in(img: &deckcraft_render::Image, x: f64, y: f64, w: f64, h: f64, scale: f64) -> usize {
        let x0 = (x * scale).floor().max(0.0) as u32;
        let y0 = (y * scale).floor().max(0.0) as u32;
        let x1 = ((x + w) * scale).ceil().min(img.width as f64) as u32;
        let y1 = ((y + h) * scale).ceil().min(img.height as f64) as u32;
        let mut n = 0;
        for yy in y0..y1 {
            for xx in x0..x1 {
                let p = img.pixel(xx, yy);
                let sum = p[0] as u16 + p[1] as u16 + p[2] as u16;
                if p[3] > 200 && sum < 680 {
                    n += 1;
                }
            }
        }
        n
    }
}
