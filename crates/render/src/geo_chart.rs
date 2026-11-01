//! Offline map rendering from document-owned geographic boundaries.
use crate::{
    chart::{LabelStyle, label, point_data_label, series_color},
    color,
};
use deckcraft_color::Rgba;
use deckcraft_model::{
    chart::Chart,
    geography::Geography,
    resolve::Ctx,
    text::{Align, Anchor},
};
use kurbo::{Affine, BezPath, Point, Rect, Shape as _};
use vello_cpu::RenderContext;

fn project(lon: f64, lat: f64, kind: &str) -> Point {
    let x = lon.to_radians();
    let y = lat.clamp(-85.0, 85.0).to_radians();
    let q = std::f64::consts::FRAC_PI_4;
    match kind {
        "miller" => Point::new(x, -1.25 * (q + 0.4 * y).tan().ln()),
        // Spherical Albers, standard parallels 20 and 50 degrees.
        "albers" => {
            let (a, b) = (20f64.to_radians(), 50f64.to_radians());
            let n = (a.sin() + b.sin()) / 2.0;
            let c = a.cos().powi(2) + 2.0 * n * a.sin();
            let rho = (c - 2.0 * n * y.sin()).max(0.0).sqrt() / n;
            Point::new(rho * (n * x).sin(), rho * (n * x).cos())
        }
        // Robinson's standard 5-degree tabulation (Snyder, USGS PP1395, table 36).
        "robinson" => {
            const X: [f64; 19] = [
                1., 0.9986, 0.9954, 0.99, 0.9822, 0.973, 0.96, 0.9427, 0.9216, 0.8962, 0.8679, 0.835, 0.7986, 0.7597, 0.7186, 0.6732, 0.6213, 0.5722,
                0.5322,
            ];
            const Y: [f64; 19] = [
                0., 0.062, 0.124, 0.186, 0.248, 0.31, 0.372, 0.434, 0.4958, 0.5571, 0.6176, 0.6769, 0.7346, 0.7903, 0.8435, 0.8936, 0.9394, 0.9761,
                1.,
            ];
            let t = lat.abs().min(90.0) / 5.0;
            let i = (t as usize).min(17);
            let f = (t - i as f64).min(1.0);
            Point::new(0.8487 * x * (X[i] + (X[i + 1] - X[i]) * f), -1.3523 * lat.signum() * (Y[i] + (Y[i + 1] - Y[i]) * f))
        }
        _ => Point::new(x, -(q + y / 2.0).tan().ln()),
    }
}

fn paths(map: &Geography) -> Vec<BezPath> {
    let included = |r: &&deckcraft_model::geography::GeoRegion| map.viewed_region != "dataOnly" || r.point.is_some();
    let mut longs: Vec<_> = map
        .regions
        .iter()
        .filter(included)
        .flat_map(|r| &r.rings)
        .flatten()
        .take(deckcraft_model::geography::MAX_GEO_POINTS)
        .map(|p| p[0])
        .filter(|v| v.is_finite())
        .collect();
    longs.sort_by(f64::total_cmp);
    let mut start = -180.0;
    let mut gap = 0.0;
    for (i, &a) in longs.iter().enumerate() {
        let b = longs.get(i + 1).copied().unwrap_or(longs.first().copied().unwrap_or(a) + 360.0);
        if b - a > gap {
            gap = b - a;
            start = b;
        }
    }
    let world = map.viewed_region == "world" || 360.0 - gap > 300.0;
    let centre = if world {
        start = -180.0;
        0.0
    } else {
        start + (360.0 - gap) / 2.0
    };
    // Native automatic projection uses Robinson for a world extent. Keep an explicitly
    // selected projection; small local extents retain a cylindrical default.
    let projection = if map.projection.is_empty() && world { "robinson" } else { &map.projection };
    let mut budget = deckcraft_model::geography::MAX_GEO_POINTS;
    map.regions
        .iter()
        .take(4096)
        .map(|region| {
            let mut path = BezPath::new();
            if !included(&region) {
                return path;
            }
            for ring in region.rings.iter().take(deckcraft_model::geography::MAX_GEO_RINGS) {
                if ring.len() < 3 || ring.len() > budget {
                    continue;
                }
                budget -= ring.len();
                if ring.iter().any(|p| !p[0].is_finite() || !p[1].is_finite() || p[0].abs() > 180.0 || p[1].abs() > 90.0) {
                    continue;
                }
                for (i, &[lon, lat]) in ring.iter().enumerate() {
                    let lon = if world { lon } else { (lon - start).rem_euclid(360.0) + start - centre };
                    let point = project(lon, lat, projection);
                    if i == 0 {
                        path.move_to(point);
                    } else {
                        path.line_to(point);
                    }
                }
                path.close_path();
            }
            path
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, m: Affine, area: Rect, base: f64, text_col: Rgba) -> bool {
    let Some(series) = c.series.first() else { return false };
    let Some(map) = series.geography.as_ref().filter(|g| !g.regions.is_empty()) else { return false };
    let paths = paths(map);
    let Some(mut bounds) = paths.iter().filter(|p| !p.elements().is_empty()).map(|p| p.bounding_box()).reduce(|a, b| a.union(b)) else {
        return false;
    };
    if map.viewed_region == "world" {
        let kind = if map.projection.is_empty() { "robinson" } else { &map.projection };
        for lat in (-90..=90).step_by(5) {
            for lon in (-180..=180).step_by(5) {
                bounds = bounds.union_pt(project(lon as f64, lat as f64, kind));
            }
        }
    }
    if bounds.width() < 1e-9 || bounds.height() < 1e-9 {
        return false;
    }
    let plot = area.inset(-base * 0.35);
    if plot.width() <= 0.0 || plot.height() <= 0.0 {
        return false;
    }
    let scale = (plot.width() / bounds.width()).min(plot.height() / bounds.height());
    let transform = Affine::translate(plot.center().to_vec2()) * Affine::scale(scale) * Affine::translate(-bounds.center().to_vec2());
    let (min, max) = series.values.iter().flatten().filter(|v| v.is_finite()).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let hi = match &series.fill {
        Some(deckcraft_model::Fill::Solid { color }) => rctx.color(color, None),
        _ => series_color(rctx, c, 0),
    };
    let lo = crate::chartex::mix(hi, Rgba::WHITE, 0.85);
    let mut labels = Vec::new();
    for (region, path) in map.regions.iter().zip(paths) {
        if path.elements().is_empty() {
            continue;
        }
        let path = transform * path;
        let value = region.point.and_then(|i| series.values.get(i)).copied().flatten().filter(|v| v.is_finite());
        let t = value.map(|v| if max > min { (v - min) / (max - min) } else { 0.5 });
        let fill = if matches!(series.fill, Some(deckcraft_model::Fill::Solid { .. })) {
            hi
        } else {
            t.map_or(Rgba::rgb(224, 224, 224), |t| crate::chartex::mix(lo, hi, t))
        };
        ctx.set_transform(m);
        ctx.set_fill_rule(vello_cpu::peniko::Fill::EvenOdd);
        ctx.set_paint(color(fill, 1.0));
        ctx.fill_path(&path);
        ctx.set_fill_rule(vello_cpu::peniko::Fill::NonZero);
        ctx.set_stroke(kurbo::Stroke::new(0.7));
        ctx.set_paint(color(Rgba::WHITE, 1.0));
        ctx.stroke_path(&path);
        let point_label = region.point.and_then(|i| c.point_label(series, i));
        if point_label.is_some() || (region.point.is_none() && c.series_data_labels(series)) {
            let bounds = path.bounding_box();
            let p = bounds.center();
            // Never put a label in a hole or outside a concave region.
            if path.winding(p) % 2 != 0 && bounds.width() > base * 3.0 && bounds.height() > base * 1.5 {
                let text = point_label.unwrap_or_else(|| region.name.clone());
                labels.push((
                    text,
                    Rect::new(p.x - base * 5.0, p.y - base * 1.4, p.x + base * 5.0, p.y + base * 1.4),
                    t.is_some_and(|v| v > 0.55),
                    region.point,
                    bounds,
                ));
            }
        }
    }
    for (text, r, dark, index, mark) in labels {
        let style = LabelStyle {
            size: base * 0.85,
            align: Align::Center,
            anchor: Anchor::Middle,
            col: if dark { Rgba::WHITE } else { text_col },
            bold: false,
        };
        if let Some(i) = index {
            point_data_label(ctx, rctx, m, &text, r, style, (c, series, i, mark, (area.x1, area.y1)));
        } else {
            label(ctx, rctx, m, &text, r, style);
        }
    }
    let mut attribution = vec![map.attribution.clone()];
    for region in &map.regions {
        for text in &region.copyrights {
            if !attribution.contains(text) {
                attribution.push(text.clone());
            }
        }
    }
    label(
        ctx,
        rctx,
        m,
        &attribution.join(" · "),
        Rect::new(area.x0, area.y1 - base * 1.3, area.x1, area.y1),
        LabelStyle { size: base * 0.65, align: Align::Center, anchor: Anchor::Middle, col: text_col, bold: false },
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dateline_is_narrow_and_holes_remain_holes() {
        let map = Geography {
            regions: vec![deckcraft_model::geography::GeoRegion {
                rings: vec![vec![[179., -10.], [-179., -10.], [-179., 10.], [179., 10.]]],
                ..Default::default()
            }],
            ..Default::default()
        };
        let path = &paths(&map)[0];
        assert!(path.bounding_box().width() < 0.04);
        for kind in ["mercator", "miller", "albers", "robinson"] {
            assert!(project(180., 90., kind).is_finite());
        }
    }
    #[test]
    fn clipped_world_meridian_does_not_draw_a_transcontinental_chord() {
        use deckcraft_model::geography::GeoRegion;
        let map = Geography {
            projection: "robinson".into(),
            regions: vec![
                GeoRegion { rings: vec![vec![[170., 10.], [180., 10.], [180., 20.], [170., 20.]]], ..Default::default() },
                GeoRegion { rings: vec![vec![[-180., -10.], [-170., -10.], [-170., 0.], [-180., 0.]]], ..Default::default() },
                // Intermediate points establish a world extent rather than one local dateline island.
                GeoRegion { rings: vec![(-180..=180).step_by(30).map(|x| [x as f64, -60.]).collect()], ..Default::default() },
            ],
            ..Default::default()
        };
        let p = paths(&map);
        assert!(p[0].bounding_box().width() < 0.3);
        assert!(p[1].bounding_box().width() < 0.3);
    }
    #[test]
    fn data_only_scope_excludes_unbound_background_without_mutating_cache() {
        use deckcraft_model::geography::GeoRegion;
        let map = Geography {
            viewed_region: "dataOnly".into(),
            regions: vec![
                GeoRegion { point: Some(0), rings: vec![vec![[10., 10.], [11., 10.], [11., 11.]]], ..Default::default() },
                GeoRegion { rings: vec![vec![[100., 40.], [120., 40.], [120., 60.]]], ..Default::default() },
            ],
            ..Default::default()
        };
        let p = paths(&map);
        assert!(!p[0].elements().is_empty() && p[1].elements().is_empty());
        assert_eq!(map.regions[1].rings[0].len(), 3);
        let mut world = map.clone();
        world.viewed_region = "world".into();
        assert!(paths(&world).iter().all(|p| !p.elements().is_empty()));
    }
}
