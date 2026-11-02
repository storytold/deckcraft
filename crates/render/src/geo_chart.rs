//! Offline map rendering from document-owned geographic boundaries.
mod scope;
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
    let y = lat.clamp(-90.0, 90.0).to_radians();
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
        // Robinson's 5-degree tabulation (Snyder, USGS PP1395, table 36). Native
        // chart controls approach a 4/3 vertical normalization instead of Snyder's
        // 1.3523 across chart sizes. Native finite-size quantization still differs.
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
            Point::new(0.8487 * x * (X[i] + (X[i + 1] - X[i]) * f), -(4. / 3.) * lat.signum() * (Y[i] + (Y[i + 1] - Y[i]) * f))
        }
        _ => {
            // Web Mercator's finite square-world latitude, atan(sinh(pi)).
            let y = lat.clamp(-85.05112878, 85.05112878).to_radians();
            Point::new(x, -(q + y / 2.0).tan().ln())
        }
    }
}

/// Spherical Albers with secants at one-sixth and five-sixths of the administrative
/// latitude range (Snyder, USGS PP1453). Symmetric secants have a cylindrical limit.
struct RegionalAlbers {
    n: f64,
    c: f64,
    cosine: f64,
}
impl RegionalAlbers {
    fn new(south: f64, north: f64) -> Self {
        let span = north - south;
        let (a, b) = ((south + span / 6.).to_radians(), (north - span / 6.).to_radians());
        let n = (a.sin() + b.sin()) / 2.;
        Self { n, c: a.cos().powi(2) + 2. * n * a.sin(), cosine: a.cos().abs().max(1e-6) }
    }
    fn point(&self, lon: f64, lat: f64) -> Point {
        let (x, y) = (lon.to_radians(), lat.clamp(-90., 90.).to_radians());
        if self.n.abs() < 1e-8 {
            return Point::new(x * self.cosine, -y.sin() / self.cosine);
        }
        let radius = (self.c - 2. * self.n * y.sin()).max(0.).sqrt() / self.n;
        Point::new(radius * (self.n * x).sin(), radius * (self.n * x).cos())
    }
}

fn world_longitude(lon: f64, bounds: Option<[f64; 4]>) -> f64 {
    let Some([west, south, east, north]) = bounds else { return lon };
    if ![west, south, east, north].iter().all(|v| v.is_finite())
        || west.abs() > 180.
        || east.abs() > 180.
        || south < -90.
        || north > 90.
        || south > north
        || west <= east
    {
        return lon;
    }
    // A world's regions retain their own dateline continuity. Do not split a country
    // across the two edges just because its global central meridian is zero.
    let centre = west + (east - west).rem_euclid(360.) / 2.;
    let centre = if centre > 180. { centre - 360. } else { centre };
    (lon - centre + 180.).rem_euclid(360.) - 180. + centre
}

fn world_latitude(lat: f64, projection: &str) -> f64 {
    // Native global cylindrical views crop the southern cap at 79°S; regional
    // views and Robinson retain their full cached latitude range.
    if matches!(projection, "mercator" | "miller") { lat.max(-79.) } else { lat }
}

struct ProjectedMap {
    paths: Vec<BezPath>,
    world: bool,
    global_coverage: bool,
}

fn paths(map: &Geography) -> ProjectedMap {
    let scope = scope::resolve(map);
    let mut longs: Vec<_> = map
        .regions
        .iter()
        // The administrative view owns the projection, even when only data regions paint.
        .zip(&scope.reference)
        .filter(|(_, include)| **include)
        .flat_map(|(r, _)| &r.rings)
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
    let global_coverage = 360.0 - gap > 300.0;
    let world = scope.world || !scope.regional && global_coverage;
    let mut centre = if world { 0.0 } else { start + (360.0 - gap) / 2.0 };
    let (mut south, mut north) = (90f64, -90f64);
    for point in map
        .regions
        .iter()
        .zip(&scope.reference)
        .filter(|(_, include)| **include)
        .flat_map(|(r, _)| r.rings.iter().flatten())
        .take(deckcraft_model::geography::MAX_GEO_POINTS)
    {
        if point[1].is_finite() && point[1].abs() <= 90. {
            south = south.min(point[1]);
            north = north.max(point[1]);
        }
    }
    if let Some([west, low, east, high]) = scope.bounds {
        south = low;
        north = high;
        if !world {
            centre = west + (east - west).rem_euclid(360.) / 2.;
        }
    }
    let projection = if scope.regional && matches!(map.projection.as_str(), "" | "robinson") {
        "albers"
    } else if world && matches!(map.projection.as_str(), "" | "albers") {
        "robinson"
    } else {
        &map.projection
    };
    let regional = (scope.regional && projection == "albers" && north >= south).then(|| RegionalAlbers::new(south, north));
    let mut budget = deckcraft_model::geography::MAX_GEO_POINTS;
    let paths = map
        .regions
        .iter()
        .take(4096)
        .zip(&scope.visible)
        .map(|(region, visible)| {
            let mut path = BezPath::new();
            if !visible {
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
                    // Extent metadata and compressed vertices have different precision. Wrap
                    // around the central meridian, not the west edge: a vertex just west of
                    // a rounded bound must not jump by 360 degrees.
                    let lon = if world { world_longitude(lon, region.bounds) } else { (lon - centre + 180.).rem_euclid(360.) - 180. };
                    let lat = if world { world_latitude(lat, projection) } else { lat };
                    let point = regional.as_ref().map_or_else(|| project(lon, lat, projection), |p| p.point(lon, lat));
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
    ProjectedMap { paths, world, global_coverage }
}

fn fit_bounds(map: &Geography, projected: &ProjectedMap) -> Option<Rect> {
    let mut bounds = projected.paths.iter().filter(|p| !p.elements().is_empty()).map(|p| p.bounding_box()).reduce(|a, b| a.union(b))?;
    // With a complete global cache, fit the actual unwrapped regions. If an authored
    // world view has only local geometry, retain a global frame instead of silently
    // zooming that lone country to fill the chart. Data-only explicitly permits cropping.
    if projected.world && !projected.global_coverage && map.viewed_region != "dataOnly" {
        let kind = if matches!(map.projection.as_str(), "" | "albers") { "robinson" } else { &map.projection };
        for lat in (-90..=90).step_by(5) {
            for lon in (-180..=180).step_by(5) {
                bounds = bounds.union_pt(project(lon as f64, world_latitude(lat as f64, kind), kind));
            }
        }
    }
    Some(bounds)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(ctx: &mut RenderContext, rctx: &Ctx, c: &Chart, m: Affine, area: Rect, base: f64, text_col: Rgba) -> bool {
    let Some(series) = c.series.first() else { return false };
    let Some(map) = series.geography.as_ref().filter(|g| !g.regions.is_empty()) else { return false };
    let projected = paths(map);
    let Some(bounds) = fit_bounds(map, &projected) else {
        return false;
    };
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
    let mut copyrights = Vec::new();
    for (region, path) in map.regions.iter().zip(projected.paths) {
        if path.elements().is_empty() {
            continue;
        }
        for text in region.copyrights.iter().take(64) {
            if !copyrights.contains(text) {
                copyrights.push(text.clone());
            }
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
        let original = paths(&map).paths;
        map.viewed_region = "dataOnly".into();
        let cropped = paths(&map).paths;
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
        let path = &paths(&map).paths[0];
        assert!(path.bounding_box().width() < 0.04);
        for kind in ["mercator", "miller", "albers", "robinson"] {
            assert!(project(180., 90., kind).is_finite());
        }
    }
    #[test]
    fn rounded_extent_does_not_wrap_a_regional_vertex_around_the_globe() {
        use deckcraft_model::geography::{GeoEntity, GeoRegion};
        for (west, east) in [(-65., -35.), (170., -170.)] {
            let map = Geography {
                entities: vec![GeoEntity { id: "local".into(), kind: "countryRegion".into(), ..Default::default() }],
                regions: vec![GeoRegion {
                    entity_id: "local".into(),
                    point: Some(0),
                    bounds: Some([west, -30., east, -10.]),
                    rings: vec![vec![[west - 0.000001, -30.], [east, -30.], [east, -10.], [west, -10.]]],
                    ..Default::default()
                }],
                ..Default::default()
            };
            let projected = paths(&map);
            assert!(!projected.world);
            let bounds = projected.paths[0].bounding_box();
            assert!(bounds.width() < 0.6 && bounds.height() < 0.5, "regional seam: {bounds:?}");
        }
    }
    #[test]
    fn regional_albers_is_finite_at_equator_and_symmetric_between_hemispheres() {
        let north = RegionalAlbers::new(10., 60.);
        let south = RegionalAlbers::new(-60., -10.);
        for lon in [-120., -40., 0., 40., 120.] {
            for lat in [10., 30., 60.] {
                let a = north.point(lon, lat);
                let b = south.point(lon, -lat);
                assert!((a.x - b.x).abs() < 1e-12 && (a.y + b.y).abs() < 1e-12);
            }
        }
        for (low, high) in [(-40., 40.), (-90., 90.), (0., 0.), (90., 90.), (-90., -90.)] {
            let p = RegionalAlbers::new(low, high);
            assert!(p.point(-180., -90.).is_finite() && p.point(180., 90.).is_finite());
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
        let p = paths(&map).paths;
        assert!(p[0].bounding_box().width() < 0.3);
        assert!(p[1].bounding_box().width() < 0.3);
    }
    #[test]
    fn world_view_keeps_a_document_owned_dateline_region_contiguous() {
        use deckcraft_model::geography::GeoRegion;
        let mut map = Geography {
            viewed_region: "world".into(),
            projection: "robinson".into(),
            regions: vec![GeoRegion {
                bounds: Some([175., -10., -175., -5.]),
                rings: vec![vec![[175., -10.], [-175., -10.], [-175., -5.], [175., -5.]]],
                ..Default::default()
            }],
            ..Default::default()
        };
        let p = &paths(&map).paths[0];
        assert!(p.bounding_box().width() < 0.2, "one region must not cross the entire world");
        assert!(p.bounding_box().x1 > project(180., -5., "robinson").x);
        map.regions[0].bounds = Some([f64::NAN, -10., -175., -5.]);
        assert!(paths(&map).paths[0].bounding_box().is_finite());
    }
    #[test]
    fn global_albers_uses_robinson_while_regional_robinson_uses_albers() {
        use deckcraft_model::geography::{GeoEntity, GeoRegion};
        let mut map = Geography {
            viewed_region: "world".into(),
            regions: vec![GeoRegion {
                entity_id: "local".into(),
                point: Some(0),
                bounds: Some([-40., 10., 20., 50.]),
                rings: vec![vec![[-40., 10.], [20., 10.], [20., 50.], [-40., 50.]]],
                ..Default::default()
            }],
            entities: vec![GeoEntity { id: "local".into(), kind: "CountryRegion".into(), ..Default::default() }],
            ..Default::default()
        };
        let world = paths(&map).paths;
        map.projection = "albers".into();
        assert_eq!(paths(&map).paths, world);
        map.viewed_region.clear();
        let region = paths(&map).paths;
        assert_ne!(region, world);
        map.projection = "robinson".into();
        assert_eq!(paths(&map).paths, region);
    }
    #[test]
    fn incomplete_world_cache_keeps_global_frame_but_data_only_can_crop() {
        use deckcraft_model::geography::GeoRegion;
        let mut map = Geography {
            viewed_region: "world".into(),
            projection: "robinson".into(),
            regions: vec![GeoRegion { point: Some(0), rings: vec![vec![[10., 10.], [20., 10.], [20., 20.], [10., 20.]]], ..Default::default() }],
            ..Default::default()
        };
        let projected = paths(&map);
        assert!(projected.world && !projected.global_coverage);
        assert!(fit_bounds(&map, &projected).unwrap().width() > 5.);
        map.viewed_region = "dataOnly".into();
        assert!(fit_bounds(&map, &paths(&map)).unwrap().width() < 0.2);
    }
    #[test]
    fn world_cylindrical_cap_does_not_crop_regional_or_robinson_geometry() {
        use deckcraft_model::geography::{GeoEntity, GeoRegion};
        let mut map = Geography {
            viewed_region: "world".into(),
            entities: vec![GeoEntity { id: "polar".into(), kind: "CountryRegion".into(), ..Default::default() }],
            regions: vec![GeoRegion {
                entity_id: "polar".into(),
                point: Some(0),
                bounds: Some([-30., -85., 30., -70.]),
                rings: vec![vec![[-30., -85.], [30., -85.], [30., -70.], [-30., -70.]]],
                ..Default::default()
            }],
            ..Default::default()
        };
        for kind in ["mercator", "miller"] {
            map.projection = kind.into();
            let global = paths(&map).paths[0].bounding_box();
            map.viewed_region.clear();
            let regional = paths(&map).paths[0].bounding_box();
            assert!(regional.height() > global.height());
            map.viewed_region = "world".into();
        }
        map.projection = "robinson".into();
        assert!((paths(&map).paths[0].bounding_box().y1 - project(0., -85., "robinson").y).abs() < 1e-12);
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
        let p = paths(&map).paths;
        assert!(!p[0].elements().is_empty() && p[1].elements().is_empty());
        assert_eq!(map.regions[1].rings[0].len(), 3);
        let mut world = map.clone();
        world.viewed_region = "world".into();
        assert!(paths(&world).paths.iter().all(|p| !p.elements().is_empty()));
    }
}
