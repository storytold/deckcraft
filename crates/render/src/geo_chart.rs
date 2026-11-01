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

fn scale_position(position: Option<deckcraft_model::geography::ValueColorPosition>, range: (f64, f64), fallback: f64) -> f64 {
    use deckcraft_model::geography::ValueColorPosition;
    let value = match position {
        Some(ValueColorPosition::Number(v)) => v,
        Some(ValueColorPosition::Percent(v)) => range.0 * (1. - v / 100.) + range.1 * (v / 100.),
        _ => fallback,
    };
    if value.is_finite() { value } else { fallback }
}
fn ramp(value: f64, stops: &[(f64, Rgba)]) -> Rgba {
    let Some(&(low, first)) = stops.first() else { return Rgba::BLACK };
    if let Some(&(high, last)) = stops.last()
        && value == low
        && low == high
    {
        // Native maps resolve a constant range at the midpoint of the final interval:
        // low/high for two colours, middle/high for three (also for zero/negative data).
        let previous = stops.get(stops.len().saturating_sub(2)).map_or(first, |s| s.1);
        return crate::chartex::mix(previous, last, 0.5);
    }
    if value <= low {
        return first;
    }
    for pair in stops.windows(2) {
        let [(a, ca), (b, cb)] = pair else { continue };
        if value <= *b {
            // Normalise first to avoid overflow for large, finite chart numbers.
            let scale = a.abs().max(b.abs()).max(value.abs()).max(1.);
            let t = if b > a { ((value / scale - a / scale) / (b / scale - a / scale)).clamp(0., 1.) } else { 1. };
            return crate::chartex::mix(*ca, *cb, t);
        }
    }
    stops.last().map_or(first, |(_, color)| *color)
}
fn value_stops(rctx: &Ctx, c: &Chart, series: &deckcraft_model::chart::Series, range: (f64, f64)) -> Vec<(f64, Rgba)> {
    use deckcraft_color::ColorTransform::{LumMod, LumOff};
    let base = series_color(rctx, c, 0);
    let lo = deckcraft_color::apply(base, &[LumMod(20000), LumOff(80000)]);
    let hi = deckcraft_color::apply(base, &[LumMod(75000)]);
    let mid = (range.0 / 2. + range.1 / 2., base);
    let Some(scale) = &series.value_colors else { return vec![(range.0, lo), (range.1, hi)] };
    let resolve = |stop: &deckcraft_model::geography::ValueColorStop, fallback: (f64, Rgba)| {
        (scale_position(stop.position, range, fallback.0), stop.color.as_ref().map_or(fallback.1, |color| rctx.color(color, None)))
    };
    let mut stops = vec![resolve(&scale.minimum, (range.0, lo))];
    if scale.three_color {
        stops.push(resolve(&scale.middle, mid));
    }
    stops.push(resolve(&scale.maximum, (range.1, hi)));
    // Malformed/reversed stops must not make interpolation divide by zero or escape bounds.
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    stops
}

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

fn paths(map: &Geography) -> (Vec<BezPath>, bool) {
    let included = |r: &&deckcraft_model::geography::GeoRegion| map.viewed_region != "dataOnly" || r.point.is_some();
    let mut longs: Vec<_> = map
        .regions
        .iter()
        // Projection and central meridian belong to the cached geographic extent. A
        // dataOnly viewport crops that map; it must not reproject its remaining regions.
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
    let paths = map
        .regions
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
        .collect();
    (paths, world)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, m: Affine, area: Rect, base: f64, text_col: Rgba) -> bool {
    let Some(series) = c.series.first() else { return false };
    let Some(map) = series.geography.as_ref().filter(|g| !g.regions.is_empty()) else { return false };
    let (paths, world) = paths(map);
    let Some(mut bounds) = paths.iter().filter(|p| !p.elements().is_empty()).map(|p| p.bounding_box()).reduce(|a, b| a.union(b)) else {
        return false;
    };
    if world && map.viewed_region != "dataOnly" {
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
    let plot = area;
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
    let stops = value_stops(rctx, c, series, (min, max));
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
            value.map_or(Rgba::rgb(224, 224, 224), |v| ramp(v, &stops))
        };
        ctx.set_transform(m);
        ctx.set_fill_rule(vello_cpu::peniko::Fill::EvenOdd);
        ctx.set_paint(color(fill, 1.0));
        ctx.fill_path(&path);
        ctx.set_fill_rule(vello_cpu::peniko::Fill::NonZero);
        if let Some(line) = &series.line {
            crate::stroke::stroke(ctx, rctx, line, None, &path, None, m, line.width.unwrap_or(0.75).clamp(0., 2000.), 1.);
        }
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
    let mut copyrights = Vec::new();
    for region in map.regions.iter().filter(|r| map.viewed_region != "dataOnly" || r.point.is_some()) {
        for text in &region.copyrights {
            if !copyrights.contains(text) {
                copyrights.push(text.clone());
            }
        }
    }
    copyrights.sort();
    let mut attribution = map.attribution.clone();
    if !copyrights.is_empty() {
        if !attribution.is_empty() {
            attribution.push('\n');
        }
        attribution.push_str("© ");
        attribution.push_str(&copyrights.join(", "));
    }
    label(
        ctx,
        rctx,
        m,
        &attribution,
        Rect::new(area.x0, area.y1 - 14.4, area.x1, area.y1),
        LabelStyle { size: 6., align: Align::Right, anchor: Anchor::Bottom, col: text_col, bold: false },
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_colour_ramp_clamps_interpolates_and_handles_large_values() {
        use deckcraft_model::geography::ValueColorPosition as P;
        let a = Rgba::rgba(240, 20, 20, 100);
        let b = Rgba::rgba(20, 220, 20, 200);
        let stops = [(0., a), (10., b)];
        assert_eq!(ramp(-1., &stops), a);
        assert_eq!(ramp(11., &stops), b);
        assert_eq!(ramp(5., &stops), Rgba::rgba(130, 120, 20, 150));
        assert_eq!(ramp(0., &[(-1e308, a), (1e308, b)]), Rgba::rgba(130, 120, 20, 150));
        assert_eq!(scale_position(Some(P::Percent(25.)), (-20., 60.), 0.), 0.);
        assert_eq!(scale_position(Some(P::Number(f64::NAN)), (0., 1.), 0.5), 0.5);
    }
    #[test]
    fn constant_maps_use_the_native_final_interval_midpoint() {
        let low = Rgba::rgb(240, 20, 20);
        let mid = Rgba::rgb(240, 220, 20);
        let high = Rgba::rgb(20, 220, 20);
        for value in [0., 7., -12.] {
            assert_eq!(ramp(value, &[(value, low), (value, high)]), Rgba::rgb(130, 120, 20));
            assert_eq!(ramp(value, &[(value, low), (value, mid), (value, high)]), Rgba::rgb(130, 220, 20));
        }
        assert_eq!(ramp(6., &[(7., low), (7., high)]), low);
        assert_eq!(ramp(8., &[(7., low), (7., high)]), high);
    }
    #[test]
    fn data_only_crops_world_projection_without_reprojecting_selected_regions() {
        use deckcraft_model::geography::GeoRegion;
        let mut map = Geography {
            regions: vec![
                GeoRegion { point: Some(0), rings: vec![vec![[-110., 45.], [-90., 45.], [-90., 70.]]], ..Default::default() },
                GeoRegion { rings: vec![(-180..=180).step_by(30).map(|x| [x as f64, -60.]).collect()], ..Default::default() },
            ],
            ..Default::default()
        };
        let original = paths(&map).0;
        map.viewed_region = "dataOnly".into();
        let cropped = paths(&map).0;
        assert_eq!(original[0], cropped[0]);
        assert!(cropped[1].elements().is_empty());
    }
    #[test]
    fn dateline_is_narrow_and_holes_remain_holes() {
        let map = Geography {
            regions: vec![deckcraft_model::geography::GeoRegion {
                rings: vec![vec![[179., -10.], [-179., -10.], [-179., 10.], [179., 10.]]],
                ..Default::default()
            }],
            ..Default::default()
        };
        let path = &paths(&map).0[0];
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
        let p = paths(&map).0;
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
        let p = paths(&map).0;
        assert!(!p[0].elements().is_empty() && p[1].elements().is_empty());
        assert_eq!(map.regions[1].rings[0].len(), 3);
        let mut world = map.clone();
        world.viewed_region = "world".into();
        assert!(paths(&world).0.iter().all(|p| !p.elements().is_empty()));
    }
}
