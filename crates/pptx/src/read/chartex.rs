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
    let mut geo_budget = deckcraft_model::geography::MAX_GEO_POINTS;
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

fn geography(g: &El, d: &Data, budget: &mut usize) -> deckcraft_model::geography::Geography {
    use deckcraft_model::geography::{GeoRegion, Geography, decode_ring};
    let mut map = Geography {
        projection: g.attr("projectionType").unwrap_or("").into(),
        viewed_region: g.attr("viewedRegionType").unwrap_or("").into(),
        attribution: g.attr("attribution").unwrap_or("").into(),
        regions: vec![],
    };
    if *budget < 3 {
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
    let Some(results) = clear.child("geoDataEntityQueryResults") else { return map };
    for result in results.children_named("geoDataEntityQueryResult").take(4096) {
        let Some(data) = result.child("geoData") else { continue };
        let mut region = GeoRegion {
            entity_id: data.attr("entityId").unwrap_or("").into(),
            name: data.attr("entityName").unwrap_or("").into(),
            ..Default::default()
        };
        region.point = bindings.get(&region.entity_id).copied();
        if let Some(polys) = data.child("geoPolygons") {
            for poly in polys.children_named("geoPolygon").take(deckcraft_model::geography::MAX_GEO_RINGS) {
                let mut encoded = poly.attr("pcaRings").unwrap_or("").split(',');
                if !matches!(encoded.next(), Some("0" | "1")) {
                    continue;
                }
                let mut rings = Vec::new();
                let mut valid = true;
                for ring in encoded.take(4096) {
                    if let Some(points) = decode_ring(ring, budget) {
                        rings.push(points);
                    } else {
                        valid = false;
                        break;
                    }
                }
                let count: usize = rings.iter().map(Vec::len).sum();
                if valid && poly.u32("numPoints").is_none_or(|n| n as usize == count) {
                    region.rings.extend(rings);
                }
                if *budget == 0 {
                    break;
                }
            }
        }
        if let Some(copyrights) = data.child("copyrights") {
            region.copyrights = copyrights.children_named("copyright").take(64).map(El::text).collect();
        }
        if !region.rings.is_empty() {
            map.regions.push(region);
        }
        if *budget == 0 {
            break;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::geography::ValueColorPosition as Position;

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
