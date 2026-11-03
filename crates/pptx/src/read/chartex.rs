//! Office 2016 charts (`cx:chartSpace`, the `chartEx` part): waterfall, funnel, treemap,
//! sunburst, histogram, Pareto and box & whisker. Their data sits in `cx:chartData` as levels of
//! points, and each series names its layout (`layoutId`).

use super::chart::rich_text;
use crate::xml::El;
use deckcraft_model::chart::{Chart, ChartType, Series};
use std::collections::HashMap;

/// Points per level and levels per dimension we keep (guards against hostile parts).
const MAX_POINTS: usize = 100_000;
const MAX_CHARTEX_POINTS: usize = 1_000_000;

/// One `cx:data`: its categories (the innermost level) and named numeric dimensions.
#[derive(Default)]
struct Data {
    categories: Vec<String>,
    /// Outer category levels, innermost first.
    levels: Vec<Vec<String>>,
    entities: Vec<String>,
    numbers: HashMap<String, (Vec<Option<f64>>, Option<String>)>,
}

/// A level's points, by `idx`.
fn points(lvl: &El, budget: &mut usize) -> Vec<String> {
    if *budget == 0 {
        return Vec::new();
    }
    let cap = MAX_POINTS.min(*budget);
    let mut pts = Vec::new();
    let mut max_idx = 0;
    for pt in lvl.children_named("pt").take(cap) {
        let i = pt.u32("idx").unwrap_or(0) as usize;
        if i >= cap {
            continue;
        }
        max_idx = max_idx.max(i + 1);
        pts.push((i, pt.text()));
    }
    if pts.is_empty() {
        return Vec::new();
    }
    let n = max_idx.min(cap);
    let mut out = vec![String::new(); n];
    for (i, s) in pts {
        if let Some(target) = out.get_mut(i) {
            *target = s;
        }
    }
    *budget = budget.saturating_sub(out.len());
    out
}

fn data(d: &El, budget: &mut usize) -> Data {
    let mut out = Data::default();
    for dim in d.elements().take(64) {
        if *budget == 0 {
            break;
        }
        // The first level is the innermost: a treemap's leaves, a waterfall's categories.
        let Some(lvl) = dim.child("lvl") else { continue };
        let ty = dim.attr("type").unwrap_or("");
        if dim.is("strDim") && ty == "entityId" {
            out.entities = points(lvl, budget);
        } else if dim.is("strDim") || (dim.is("numDim") && ty == "cat") {
            if ty == "cat" || out.categories.is_empty() {
                out.categories = points(lvl, budget);
                // Further levels are the hierarchy's parents, innermost first.
                out.levels = dim.children_named("lvl").skip(1).take(deckcraft_model::chart::MAX_LEVELS).map(|l| points(l, budget)).collect();
            }
        } else if dim.is("numDim") {
            let vals = points(lvl, budget).into_iter().map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect();
            out.numbers.insert(ty.to_string(), (vals, lvl.attr("formatCode").map(str::to_string)));
        }
    }
    out
}

fn kind_of(layout: &str) -> Option<ChartType> {
    Some(match layout {
        "waterfall" => ChartType::Waterfall,
        "funnel" => ChartType::Funnel,
        "treemap" => ChartType::Treemap,
        "sunburst" => ChartType::Sunburst,
        "clusteredColumn" => ChartType::Histogram,
        "boxWhisker" => ChartType::BoxWhisker,
        "regionMap" => ChartType::RegionMap,
        _ => return None,
    })
}

/// Parse a `chartEx` part. `None` for anything else, and for layouts DeckCraft has no type for.
pub fn read_chartex_xml(bytes: &[u8]) -> Option<Chart> {
    let doc = crate::xml::parse(bytes).ok()?;
    let mut root = doc.root;
    super::resolve_mc(&mut root, 0);
    let mut c = chartex_from(&root)?;
    c.raw = Some(String::from_utf8_lossy(bytes).into_owned());
    Some(c)
}

pub(crate) fn chartex_from(root: &El) -> Option<Chart> {
    if !root.is("chartSpace") {
        return None;
    }
    let mut decode_budget = MAX_CHARTEX_POINTS;
    let datas: HashMap<String, Data> = root
        .child("chartData")?
        .children_named("data")
        .take(64)
        .map(|d| (d.attr("id").unwrap_or("0").to_string(), data(d, &mut decode_budget)))
        .collect();
    let ch = root.child("chart")?;
    let region = ch.child("plotArea")?.child("plotAreaRegion")?;
    let mut c = Chart { categories: vec![], series: vec![], title: None, legend: None, gridlines: false, ..Default::default() };
    c.title = ch.child("title").map(|t| rich_text(t).filter(|s| !s.is_empty()).unwrap_or_else(|| "Chart Title".into()));
    c.legend = ch.child("legend").map(|l| l.attr("pos").unwrap_or("t").to_string());
    let mut kind = None;
    let mut output_budget = MAX_CHARTEX_POINTS;
    let mut geo_budget = GeoBudget::default();
    let mut categories_set = false;
    for s in region.children_named("series").take(1000) {
        let layout = s.attr("layoutId").unwrap_or("");
        if layout == "paretoLine" {
            // The cumulative line PowerPoint adds to a Pareto histogram.
            kind = Some(ChartType::Pareto);
            continue;
        }
        let k = kind_of(layout)?;
        if s.bool("hidden") == Some(true) {
            continue;
        }
        kind = Some(match kind {
            Some(ChartType::Pareto) => ChartType::Pareto,
            _ => k,
        });
        let d = datas.get(s.child("dataId").and_then(|i| i.attr("val")).unwrap_or("0"))?;
        let (values_src, format) = ["val", "size", "colorVal"]
            .iter()
            .find_map(|t| d.numbers.get(*t))
            .or_else(|| d.numbers.values().next())
            .map(|(v, f)| (v.as_slice(), f.clone()))
            .unwrap_or((&[], None));
        let take_len = values_src.len().min(output_budget);
        let values = values_src[..take_len].to_vec();
        output_budget = output_budget.saturating_sub(values.len());
        if !categories_set && (!d.categories.is_empty() || !d.levels.is_empty()) {
            categories_set = true;
            c.categories = d.categories.clone();
            c.category_levels = d.levels.clone();
            output_budget = output_budget.saturating_sub(c.categories.len());
            for lvl in &c.category_levels {
                output_budget = output_budget.saturating_sub(lvl.len());
            }
        }
        let mut options = deckcraft_model::chart::ExtendedChartOptions::default();
        let mut inclusive_quartiles = None;
        if let Some(st) = s.child("layoutPr").and_then(|l| l.child("statistics")) {
            inclusive_quartiles = Some(st.attr("quartileMethod") == Some("inclusive"));
        }
        if let Some(b) = s.child("layoutPr").and_then(|l| l.child("binning")) {
            options.bin_closed_left = b.attr("intervalClosed") == Some("l");
            options.underflow = b.f64("underflow");
            options.overflow = b.f64("overflow");
            c.bin_count = b.child("binCount").and_then(|n| n.text().trim().parse().ok().or_else(|| n.u32("val"))).filter(|n| *n > 0);
            c.bin_size =
                b.child("binSize").and_then(|n| n.text().trim().parse::<f64>().ok().or_else(|| n.f64("val"))).filter(|v| v.is_finite() && *v > 0.0);
        }
        if let Some(lp) = s.child("layoutPr") {
            options.aggregation |= lp.child("aggregation").is_some();
            options.parent_labels = lp.child("parentLabelLayout").and_then(|e| e.attr("val")).map(str::to_string);
            if let Some(v) = lp.child("visibility") {
                options.mean_marker = v.bool("meanMarker").unwrap_or(options.mean_marker);
                options.mean_line = v.bool("meanLine").unwrap_or(options.mean_line);
                options.inner_points = v.bool("nonoutliers").unwrap_or(options.inner_points);
                options.outliers = v.bool("outliers").unwrap_or(options.outliers);
                options.connector_lines = v.bool("connectorLines").unwrap_or(options.connector_lines);
            }
        }
        if c.series.is_empty() {
            c.extended = options.clone();
            c.inclusive_quartiles = inclusive_quartiles.unwrap_or(false);
        }
        if let Some(st) = s.child("layoutPr").and_then(|l| l.child("subtotals")) {
            c.subtotals = st
                .children_named("idx")
                .filter_map(|i| i.text().trim().parse::<u32>().ok().or_else(|| i.u32("val")))
                .map(|i| i as usize)
                .take(MAX_POINTS)
                .collect();
        }
        let name = s.child("tx").and_then(|t| t.find("v")).map(|v| v.text()).unwrap_or_default();
        let mut series = Series {
            name,
            value_colors: value_colors(s),
            geography: s.path(&["layoutPr", "geography"]).map(|g| geography(g, d, &mut geo_budget)),
            values,
            extended: s.child("layoutPr").map(|_| options),
            inclusive_quartiles,
            data_labels: s.child("dataLabels").map(|l| l.bool("hidden") != Some(true)),
            number_format: format,
            fill: super::chart::simple_fill(s.child("spPr")),
            line: super::chart::simple_line(s.child("spPr")),
            ..Default::default()
        };
        super::chart_labels::read(&mut series, s.child("dataLabels"), true);
        c.series.push(series);
    }
    c.kind = kind?;
    c.data_labels = c.series.iter().any(|s| s.data_labels == Some(true));
    c.vary_colors = matches!(c.kind, ChartType::Funnel | ChartType::Treemap | ChartType::Sunburst | ChartType::Waterfall);
    // Waterfall, histogram and box charts have a value axis with gridlines by default.
    c.gridlines = matches!(c.kind, ChartType::Waterfall | ChartType::Histogram | ChartType::Pareto | ChartType::BoxWhisker);
    let pa = ch.child("plotArea")?;
    let cat = pa.children_named("axis").find(|a| a.child("catScaling").is_some());
    let val = pa.children_named("axis").find(|a| a.child("valScaling").is_some());
    if let Some(a) = val {
        let sc = a.child("valScaling");
        c.y_min = sc.and_then(|s| s.f64("min"));
        c.y_max = sc.and_then(|s| s.f64("max"));
        c.y_major = sc.and_then(|s| s.f64("majorUnit")).filter(|v| *v > 0.0);
        c.y_hidden = a.bool("hidden") == Some(true);
        c.y_format = a.child("numFmt").and_then(|e| e.attr("formatCode")).map(str::to_string);
        c.gridlines = a.child("majorGridlines").is_some();
        c.minor_gridlines = a.child("minorGridlines").is_some();
    }
    c.axis_titles = (cat.and_then(|a| a.child("title")).and_then(rich_text), val.and_then(|a| a.child("title")).and_then(rich_text));
    c.chart_fill = super::chart::simple_fill(root.child("spPr"));
    c.plot_fill = super::chart::simple_fill(pa.child("spPr"));
    super::chart::read_text_styles(&mut c, root, ch, val, cat);
    super::chart::read_format(&mut c, ch, pa, val, cat);
    c.legend_overlay = ch.child("legend").and_then(|e| e.bool("overlay")) == Some(true);
    c.format.title_overlay = ch.child("title").and_then(|e| e.bool("overlay")) == Some(true);
    Some(c)
}

fn value_colors(series: &El) -> Option<deckcraft_model::geography::ValueColorScale> {
    use deckcraft_model::geography::{ValueColorPosition, ValueColorScale, ValueColorStop};
    let colors = series.child("valueColors");
    let positions = series.child("valueColorPositions");
    if colors.is_none() && positions.is_none() {
        return None;
    }
    let stop = |name: &str| {
        // Native Office writes minPosition/midPosition/maxPosition, whereas the
        // published CT_ValueColorPositions schema uses the shorter names. Read both.
        let position = positions.and_then(|p| p.child(&format!("{name}Position")).or_else(|| p.child(name))).and_then(|p| {
            if p.child("extremeValue").is_some() {
                Some(ValueColorPosition::Extreme)
            } else if let Some(n) = p.child("number").and_then(|p| p.f64("val")).filter(|n| n.is_finite()) {
                Some(ValueColorPosition::Number(n))
            } else {
                p.child("percent").and_then(|p| p.f64("val")).filter(|n| n.is_finite()).map(ValueColorPosition::Percent)
            }
        });
        ValueColorStop { color: colors.and_then(|c| c.child(&format!("{name}Color"))).and_then(super::dml::color), position }
    };
    Some(ValueColorScale {
        three_color: positions.and_then(|p| p.u32("count")) == Some(3),
        minimum: stop("min"),
        middle: stop("mid"),
        maximum: stop("max"),
    })
}

struct GeoBudget {
    points: usize,
    rings: usize,
    polygons: usize,
}
impl Default for GeoBudget {
    fn default() -> Self {
        use deckcraft_model::geography::{MAX_GEO_POINTS, MAX_GEO_RINGS};
        Self { points: MAX_GEO_POINTS, rings: MAX_GEO_RINGS, polygons: MAX_GEO_RINGS }
    }
}
impl GeoBudget {
    fn exhausted(&self) -> bool {
        self.points == 0 || self.rings == 0 || self.polygons == 0
    }
}

fn geo_polygon(poly: &El, budget: &mut GeoBudget) -> Option<Vec<Vec<[f64; 2]>>> {
    budget.polygons = budget.polygons.checked_sub(1)?;
    let mut encoded = poly.attr("pcaRings").unwrap_or("").split(',');
    if !matches!(encoded.next(), Some("0" | "1")) {
        return None;
    }
    let mut rings = Vec::new();
    for ring in encoded {
        budget.rings = budget.rings.checked_sub(1)?;
        if rings.len() == deckcraft_model::geography::MAX_GEO_POLYGON_RINGS {
            return None;
        }
        rings.push(deckcraft_model::geography::decode_ring(ring, &mut budget.points)?);
    }
    // Native caches can declare a different numPoints, including zero. The decoded
    // payload and shared work budgets determine validity, not that advisory count.
    Some(rings)
}

fn geography(g: &El, d: &Data, budget: &mut GeoBudget) -> deckcraft_model::geography::Geography {
    use deckcraft_model::geography::{GeoRegion, Geography};
    let mut map = Geography {
        provider: g
            .child("geoCache")
            .and_then(|cache| cache.attr("provider"))
            .filter(|s| s.len() <= deckcraft_model::geography::MAX_GEO_PROVIDER_BYTES && !s.chars().any(char::is_control))
            .unwrap_or("")
            .into(),
        projection: g.attr("projectionType").unwrap_or("").into(),
        viewed_region: g.attr("viewedRegionType").unwrap_or("").into(),
        attribution: g.attr("attribution").unwrap_or("").into(),
        regions: vec![],
        entities: vec![],
        child_queries: vec![],
    };
    if budget.points < 3 || budget.exhausted() {
        return map;
    }
    let decoded;
    let clear = if let Some(clear) = g.path(&["geoCache", "clear"]) {
        clear
    } else {
        let Some(binary) = g.path(&["geoCache", "binary"]) else { return map };
        decoded = super::geo_cache::decode(&binary.text());
        let Some(doc) = decoded.as_ref() else { return map };
        &doc.root
    };
    let bindings = super::geo_binding::resolve(clear, &d.categories, &d.levels, &d.entities);
    map.entities = super::geo_hierarchy::read(clear).unwrap_or_else(|| {
        log::warn!("pptx: geographic hierarchy exceeds the work budget");
        vec![]
    });
    map.child_queries = child_queries(clear, &map.entities);
    let Some(results) = clear.child("geoDataEntityQueryResults") else { return map };
    for result in results.children_named("geoDataEntityQueryResult").take(4096) {
        let Some(data) = result.child("geoData") else { continue };
        let mut region = GeoRegion {
            entity_id: data.attr("entityId").unwrap_or("").into(),
            name: data.attr("entityName").unwrap_or("").into(),
            ..Default::default()
        };
        region.point = bindings.get(&region.entity_id).copied();
        region.bounds = super::geo_hierarchy::bounds(data);
        if let Some(polys) = data.child("geoPolygons") {
            for poly in polys.children_named("geoPolygon") {
                if let Some(rings) = geo_polygon(poly, budget) {
                    region.rings.extend(rings);
                }
                if budget.exhausted() {
                    break;
                }
            }
        }
        if let Some(copyrights) = data.child("copyrights") {
            region.copyrights = copyrights.children_named("copyright").take(64).map(El::text).collect();
        }
        if !region.rings.is_empty() || region.bounds.is_some() {
            map.regions.push(region);
        }
        if budget.exhausted() {
            break;
        }
    }
    map
}

fn child_queries(clear: &El, entities: &[deckcraft_model::geography::GeoEntity]) -> Vec<deckcraft_model::geography::GeoChildQuery> {
    use deckcraft_model::geography::{GeoChildQuery, MAX_GEO_CHILD_QUERIES, MAX_GEO_CHILD_REFERENCES, MAX_GEO_QUERY_BYTES};
    let Some(group) = clear.child("geoChildEntitiesQueryResults") else { return vec![] };
    let known: std::collections::HashSet<_> = entities.iter().take(4096).map(|e| e.id.as_str()).collect();
    let (mut references, mut bytes) = (MAX_GEO_CHILD_REFERENCES, MAX_GEO_QUERY_BYTES);
    let mut result = Vec::new();
    for row in group.children_named("geoChildEntitiesQueryResult").take(MAX_GEO_CHILD_QUERIES) {
        let Some(query) = row.child("geoChildEntitiesQuery") else { continue };
        let Some(id) = query.attr("entityId").filter(|s| !s.is_empty()) else { continue };
        let Some(entity_id) = child_query_string(id, 2048, &mut bytes) else { continue };
        let mut valid = true;
        let child_types = query.child("geoChildTypes").map(|types| {
            let mut values = Vec::new();
            for node in types.children_named("entityType").take(65) {
                if values.len() == 64 {
                    valid = false;
                    break;
                }
                let Some(value) = child_query_string(&node.text(), 64, &mut bytes) else {
                    valid = false;
                    break;
                };
                values.push(value);
            }
            values
        });
        if !valid {
            continue;
        }
        let children = row.child("geoChildEntities").map(|children| {
            let mut ids = Vec::new();
            for child in children.children_named("geoHierarchyEntity").take(MAX_GEO_CHILD_REFERENCES + 1) {
                if references == 0 {
                    valid = false;
                    break;
                }
                references -= 1;
                let Some(id) = child.attr("entityId").filter(|id| known.contains(id)) else { continue };
                let Some(id) = child_query_string(id, 2048, &mut bytes) else {
                    valid = false;
                    break;
                };
                ids.push(id);
            }
            ids
        });
        if valid {
            result.push(GeoChildQuery { entity_id, child_types, children });
        }
    }
    result
}

fn child_query_string(value: &str, limit: usize, budget: &mut usize) -> Option<String> {
    if value.len() > limit || value.len() > *budget || value.chars().any(char::is_control) {
        return None;
    }
    *budget -= value.len();
    Some(value.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::geography::ValueColorPosition as Position;

    fn original_rings() -> Vec<Vec<[f64; 2]>> {
        vec![
            vec![[-26., -22.], [23., -22.], [23., 26.], [-26., 26.], [-26., -22.]],
            vec![[-13., -10.], [-13., 12.], [11., 12.], [11., -10.], [-13., -10.]],
        ]
    }

    fn polygon_xml(rings: &[Vec<[f64; 2]>], declared: u32) -> String {
        let encoded = rings.iter().map(|r| deckcraft_model::geography::encode_ring(r).unwrap()).collect::<Vec<_>>().join(",");
        format!(r#"<geoPolygon numPoints="{declared}" pcaRings="0,{encoded}"/>"#)
    }

    #[test]
    fn geographic_declared_point_count_does_not_discard_closed_rings_or_holes() {
        let rings = original_rings();
        for declared in [0, 1, 4, 5, 9, 10, 12, 47, u32::MAX] {
            for prefix in ["0,", "1,"] {
                let xml = polygon_xml(&rings, declared).replace("0,", prefix);
                let doc = crate::xml::parse(xml.as_bytes()).unwrap();
                let mut budget = GeoBudget::default();
                assert_eq!(geo_polygon(&doc.root, &mut budget), Some(rings.clone()));
                assert_eq!(budget.points, deckcraft_model::geography::MAX_GEO_POINTS - 10);
                assert_eq!(budget.rings, deckcraft_model::geography::MAX_GEO_RINGS - 2);
            }
        }
    }

    #[test]
    fn geographic_actual_point_and_ring_limits_reject_whole_polygons() {
        let rings = original_rings();
        let xml = polygon_xml(&rings, 0);
        let doc = crate::xml::parse(xml.as_bytes()).unwrap();
        for (points, count) in [(9, 2), (10, 1)] {
            let mut budget = GeoBudget { points, rings: count, polygons: 1 };
            assert!(geo_polygon(&doc.root, &mut budget).is_none());
            assert!(budget.exhausted());
        }
        let mut budget = GeoBudget { points: 10, rings: 2, polygons: 1 };
        assert_eq!(geo_polygon(&doc.root, &mut budget), Some(rings));
        assert_eq!((budget.points, budget.rings, budget.polygons), (0, 0, 0));
    }

    #[test]
    fn geographic_truncated_and_oversized_polygons_never_keep_a_valid_prefix() {
        let ring = original_rings().remove(0);
        let encoded = deckcraft_model::geography::encode_ring(&ring).unwrap();
        for payload in [format!("0,{encoded},_"), format!("0,{encoded},AA"), format!("0,{encoded},!")] {
            let xml = format!(r#"<geoPolygon numPoints="0" pcaRings="{payload}"/>"#);
            let doc = crate::xml::parse(xml.as_bytes()).unwrap();
            assert!(geo_polygon(&doc.root, &mut GeoBudget::default()).is_none());
        }
        let limit = deckcraft_model::geography::MAX_GEO_POLYGON_RINGS;
        for (count, valid) in [(limit, true), (limit + 1, false)] {
            let xml = polygon_xml(&vec![ring.clone(); count], 0);
            let doc = crate::xml::parse(xml.as_bytes()).unwrap();
            let mut budget = GeoBudget::default();
            let parsed = geo_polygon(&doc.root, &mut budget);
            assert_eq!(parsed.is_some(), valid);
            assert_eq!(budget.points, deckcraft_model::geography::MAX_GEO_POINTS - limit * ring.len());
            assert_eq!(budget.rings, deckcraft_model::geography::MAX_GEO_RINGS - count);
        }
    }

    #[test]
    fn geographic_work_budgets_span_regions_and_repeated_cache_reads() {
        let ring = original_rings().remove(0);
        let polygon = polygon_xml(std::slice::from_ref(&ring), u32::MAX);
        let entry = |id: &str, polygons: &str| {
            format!("<geoDataEntityQueryResult><geoData entityId='{id}'><geoPolygons>{polygons}</geoPolygons></geoData></geoDataEntityQueryResult>")
        };
        let geography_xml = |entries: String| {
            format!("<geography><geoCache><clear><geoDataEntityQueryResults>{entries}</geoDataEntityQueryResults></clear></geoCache></geography>")
        };
        let xml = geography_xml(entry("a", &polygon) + &entry("b", &polygon));
        let doc = crate::xml::parse(xml.as_bytes()).unwrap();
        for mut budget in [
            GeoBudget { points: 5, rings: 10, polygons: 10 },
            GeoBudget { points: 100, rings: 1, polygons: 10 },
            GeoBudget { points: 100, rings: 10, polygons: 1 },
        ] {
            let map = geography(&doc.root, &Data::default(), &mut budget);
            assert_eq!(map.regions.len(), 1);
            assert_eq!(map.regions[0].rings, vec![ring.clone()]);
            assert!(geography(&doc.root, &Data::default(), &mut budget).regions.is_empty());
        }
        // Invalid prefixes and empty polygons still consume the shared visit budget.
        for invalid in ["<geoPolygon pcaRings='invalid'/>", "<geoPolygon pcaRings='1'/>"] {
            let xml = geography_xml(entry("bad", invalid) + &entry("good", &polygon));
            let doc = crate::xml::parse(xml.as_bytes()).unwrap();
            let mut budget = GeoBudget { polygons: 1, ..Default::default() };
            assert!(geography(&doc.root, &Data::default(), &mut budget).regions.is_empty());
        }
    }

    #[test]
    fn geographic_provider_identity_is_document_owned_and_bounded() {
        let limit = deckcraft_model::geography::MAX_GEO_PROVIDER_BYTES;
        for (provider, kept) in [("original-cache".to_string(), true), ("p".repeat(limit), true), ("p".repeat(limit + 1), false)] {
            let xml = format!(r#"<geography><geoCache provider="{provider}"><clear/></geoCache></geography>"#);
            let doc = crate::xml::parse(xml.as_bytes()).unwrap();
            let map = geography(&doc.root, &Data::default(), &mut GeoBudget::default());
            assert_eq!(map.provider, if kept { provider.as_str() } else { "" });
        }
        let doc = crate::xml::parse(br#"<geography><geoCache provider="bad&#10;identity"><clear/></geoCache></geography>"#).unwrap();
        assert!(geography(&doc.root, &Data::default(), &mut GeoBudget::default()).provider.is_empty());
    }

    #[test]
    fn child_query_keys_empty_results_and_global_reference_budget_are_preserved() {
        use deckcraft_model::geography::{GeoEntity, MAX_GEO_CHILD_QUERIES, MAX_GEO_CHILD_REFERENCES};
        let entity = GeoEntity { id: "sparse".into(), ..Default::default() };
        let doc = crate::xml::parse(br#"<clear><geoChildEntitiesQueryResults>
          <geoChildEntitiesQueryResult><geoChildEntitiesQuery entityId="parent"><geoChildTypes><entityType>countryRegion</entityType><entityType>CountryRegion</entityType><entityType>countryRegion</entityType></geoChildTypes></geoChildEntitiesQuery><geoChildEntities><geoHierarchyEntity entityId="sparse"/></geoChildEntities></geoChildEntitiesQueryResult>
          <geoChildEntitiesQueryResult><geoChildEntitiesQuery entityId="parent"/><geoChildEntities/></geoChildEntitiesQueryResult>
          <geoChildEntitiesQueryResult><geoChildEntitiesQuery entityId="parent"><geoChildTypes/></geoChildEntitiesQuery></geoChildEntitiesQueryResult>
        </geoChildEntitiesQueryResults></clear>"#).unwrap();
        let rows = child_queries(&doc.root, std::slice::from_ref(&entity));
        assert_eq!(rows[0].child_types.as_ref().unwrap(), &["countryRegion", "CountryRegion", "countryRegion"]);
        assert_eq!(rows[0].children.as_ref().unwrap(), &["sparse"]);
        assert_eq!((rows[1].child_types.as_ref(), rows[1].children.as_deref()), (None, Some([].as_slice())));
        assert_eq!((rows[2].child_types.as_deref(), rows[2].children.as_ref()), (Some([].as_slice()), None));
        let row = |n: usize| {
            format!(
                "<geoChildEntitiesQueryResult><geoChildEntitiesQuery entityId='parent'/><geoChildEntities>{}</geoChildEntities></geoChildEntitiesQueryResult>",
                "<geoHierarchyEntity entityId='sparse'/>".repeat(n)
            )
        };
        let xml = format!(
            "<clear><geoChildEntitiesQueryResults>{}{}{}</geoChildEntitiesQueryResults></clear>",
            row(MAX_GEO_CHILD_REFERENCES),
            row(1),
            row(0)
        );
        let doc = crate::xml::parse(xml.as_bytes()).unwrap();
        let rows = child_queries(&doc.root, std::slice::from_ref(&entity));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].children.as_ref().unwrap().len(), MAX_GEO_CHILD_REFERENCES);
        assert_eq!(rows[1].children, Some(vec![]));
        let xml = format!("<clear><geoChildEntitiesQueryResults>{}</geoChildEntitiesQueryResults></clear>", row(0).repeat(MAX_GEO_CHILD_QUERIES + 1));
        let doc = crate::xml::parse(xml.as_bytes()).unwrap();
        assert_eq!(child_queries(&doc.root, &[]).len(), MAX_GEO_CHILD_QUERIES);
    }

    #[test]
    fn map_stops_accept_native_names_and_public_schema_aliases() {
        for suffix in ["Position", ""] {
            let xml = crate::xml::parse(
                format!(
                    r#"<series><valueColorPositions count="3">
                <min{suffix}><number val="-12.5"/></min{suffix}>
                <mid{suffix}><percent val="35"/></mid{suffix}>
                <max{suffix}><extremeValue/></max{suffix}>
                </valueColorPositions></series>"#
                )
                .as_bytes(),
            )
            .unwrap();
            let scale = value_colors(&xml.root).unwrap();
            assert!(scale.three_color);
            assert_eq!(scale.minimum.position, Some(Position::Number(-12.5)));
            assert_eq!(scale.middle.position, Some(Position::Percent(35.)));
            assert_eq!(scale.maximum.position, Some(Position::Extreme));
        }
        let xml = crate::xml::parse(
            br#"<series><valueColorPositions>
            <min><number val="99"/></min><minPosition><number val="7"/></minPosition>
            <maxPosition><number val="NaN"/></maxPosition>
            </valueColorPositions></series>"#,
        )
        .unwrap();
        let scale = value_colors(&xml.root).unwrap();
        assert_eq!(scale.minimum.position, Some(Position::Number(7.)), "native name takes precedence");
        assert_eq!(scale.maximum.position, None);
    }
}
