//! Insert › Icons: Lucide stroke icons as custom geometry.
//!
//! The paths in `icon_data.rs` are Lucide 1.56.0 centerlines (24×24, fill none). They are converted
//! to DeckCraft's `M`/`L`/`C`/`Q`/`Z` syntax when inserted, so the shape is a stroked outline rather
//! than a filled silhouette or an SVG picture.

use std::collections::HashMap;
use std::sync::OnceLock;

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
    /// SVG path data. Independent subpaths (separate SVG elements) are separated by ` | `.
    pub d: &'static str,
}

include!("icon_data.rs");

/// Every icon, in gallery order.
pub fn icons() -> &'static [Icon] {
    ICONS
}

/// `name` is the Lucide id (`arrow-right`, `arrow_right`) or the gallery label (`Arrow right`).
pub fn find(name: &str) -> Option<&'static Icon> {
    let raw = name.trim();
    if raw.is_empty() {
        return None;
    }
    let key = raw.to_ascii_lowercase().replace('_', "-");
    ICONS.iter().find(|i| i.name == key || i.label.eq_ignore_ascii_case(raw))
}

/// The icon's outline in the 24×24 viewBox.
pub fn outline(name: &str) -> Option<&'static BezPath> {
    let icon = find(name)?;
    parsed().get(icon.name)
}

/// DeckCraft path syntax for `name`, in the 24×24 viewBox.
pub fn path_data(name: &str) -> Option<String> {
    outline(name).and_then(deck_path)
}

/// Stroke width in points for a shape `w`×`h` points. Lucide's stroke is `stroke` units on a
/// 24-unit box, so a 72 pt icon is a 6 pt line. Resizing the shape later does not change it.
pub fn stroke_width(icon: &Icon, w: f64, h: f64) -> f64 {
    let span = w.min(h).abs().min(4_000.0);
    let design = if icon.stroke.is_finite() && icon.stroke > 0.0 { icon.stroke } else { 2.0 };
    (span * design / VIEW).clamp(0.25, 400.0)
}

fn parsed() -> &'static HashMap<&'static str, BezPath> {
    static CACHE: OnceLock<HashMap<&str, BezPath>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut m = HashMap::new();
        for icon in ICONS {
            if let Some(bp) = bezpath(icon.d) {
                m.insert(icon.name, bp);
            }
        }
        m
    })
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
    use deckcraft_geom::preset::FillMode;
    use deckcraft_model::{Geom, ShapeId};
    use serde_json::json;

    use crate::Session;

    #[test]
    fn every_icon_converts_and_a_second_subpath_keeps_its_own_origin() {
        assert!(icons().len() >= 60);
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
        assert!(path_data("star").unwrap().contains(" C "), "{}", path_data("star").unwrap());
        assert!(find("arrow_right").is_some_and(|i| i.name == "arrow-right"));
        assert!(find("Star").is_some_and(|i| i.name == "star"));
        assert!(find("nope").is_none());
    }

    #[test]
    fn insert_icon_strokes_the_outline_and_round_trips_through_pptx() {
        let mut s = Session::with_new();
        assert!(s.execute("insert.icon", &json!({})).is_err());
        assert!(s.execute("insert.icon", &json!({"name": "not-an-icon"})).is_err());
        let listed = s.execute("insert.icons", &json!({})).unwrap();
        assert!(listed.as_array().unwrap().iter().any(|i| i["name"] == "star" && i["category"] == "Marks"));

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
            assert!((paths[0].w - VIEW).abs() < 1e-9 && (paths[0].h - VIEW).abs() < 1e-9);
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
            let sh = st.shapes().iter().find(|s| s.name == "Arrow right").unwrap();
            let width = sh.line.as_ref().and_then(|l| l.width).unwrap();
            assert!((width - 8.0).abs() < 1e-6, "{width}");
            let img = deckcraft_render::render_slide(&st.doc, st.selection.slide, &deckcraft_render::RenderOpts { scale: 2.0, ..Default::default() });
            assert!(ink_in(&img, 200.0, 200.0, 96.0, 96.0, 2.0) > 40);
        }

        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let p = deckcraft_pptx::import(&bytes).unwrap();
        let sh = p.slides[0].shapes.iter().find(|s| s.name == "Arrow right").unwrap();
        let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
        assert!(paths.iter().any(|path| path.fill == FillMode::None && path.stroke && path.d.contains('M')));
        let width = sh.line.as_ref().and_then(|l| l.width).unwrap();
        assert!((width - 8.0).abs() < 0.05, "{width}");
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
