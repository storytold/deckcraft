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
        if dim.is("strDim") || (dim.is("numDim") && ty == "cat") {
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
        if let Some(st) = s.child("layoutPr").and_then(|l| l.child("statistics")) {
            c.inclusive_quartiles = st.attr("quartileMethod") == Some("inclusive");
        }
        if let Some(b) = s.child("layoutPr").and_then(|l| l.child("binning")) {
            c.bin_count = b.child("binCount").and_then(|n| n.u32("val")).filter(|n| *n > 0);
            c.bin_size = b.child("binSize").and_then(|n| n.attr("val")).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v > 0.0);
        }
        if let Some(st) = s.child("layoutPr").and_then(|l| l.child("subtotals")) {
            c.subtotals = st.children_named("idx").filter_map(|i| i.u32("val")).map(|i| i as usize).take(MAX_POINTS).collect();
        }
        let name = s.child("tx").and_then(|t| t.find("v")).map(|v| v.text()).unwrap_or_default();
        c.series.push(Series {
            name,
            values,
            data_labels: s.child("dataLabels").map(|l| l.bool("hidden") != Some(true)),
            number_format: format,
            ..Default::default()
        });
    }
    c.kind = kind?;
    c.data_labels = c.series.iter().any(|s| s.data_labels == Some(true));
    c.vary_colors = matches!(c.kind, ChartType::Funnel | ChartType::Treemap | ChartType::Sunburst | ChartType::Waterfall);
    // Waterfall, histogram and box charts have a value axis with gridlines by default.
    c.gridlines = matches!(c.kind, ChartType::Waterfall | ChartType::Histogram | ChartType::Pareto | ChartType::BoxWhisker);
    Some(c)
}
