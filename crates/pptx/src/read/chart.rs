//! Chart parts (`c:chartSpace`) → [`Chart`]. The original XML is kept in `Chart.raw`.

use deckcraft_model::chart::{Chart, ChartType, ErrorBars, Series, Trendline, TrendlineType};
use deckcraft_model::style::Fill;

use crate::xml::El;

/// Fills of chart parts never reference images we keep; read colour and pattern fills only.
fn simple_fill(sp: Option<&El>) -> Option<Fill> {
    let sp = sp?;
    for e in sp.elements() {
        match e.local() {
            "noFill" => return Some(Fill::None),
            "solidFill" => return super::dml::color(e).map(|color| Fill::Solid { color }),
            "gradFill" => return Some(Fill::Gradient(super::dml::gradient(e))),
            "pattFill" => return Some(Fill::Pattern(super::dml::pattern(e))),
            _ => {}
        }
    }
    None
}

fn simple_line(sp: Option<&El>) -> Option<deckcraft_model::Line> {
    let ln = sp?.child("ln")?;
    let mut l = deckcraft_model::Line { width: super::dml::pt(ln, "w"), ..Default::default() };
    l.fill = simple_fill(Some(ln));
    Some(l)
}

pub(crate) fn rich_text(t: &El) -> Option<String> {
    let tx = t.child("tx")?;
    if let Some(r) = tx.child("rich") {
        let s = r.children_named("p").map(|p| p.text()).collect::<Vec<_>>().join("\n");
        return Some(s);
    }
    tx.find("v").map(|v| v.text())
}

/// Most category/value/size slots one chart may decode across all its series and caches, so
/// sparse `ptCount`/`idx` declarations can't expand far beyond the XML they came from.
pub(crate) const MAX_CHART_POINTS: usize = 1_000_000;

/// Most slots one cache may hold.
const MAX_CACHE_POINTS: usize = 100_000;

/// String values from `c:cat`/`c:tx` (str or num refs, caches or literals). Every slot returned
/// is taken from `budget`; once it runs out the values are cut short.
fn strings(e: &El, budget: &mut usize) -> Vec<String> {
    let cap = MAX_CACHE_POINTS.min(*budget);
    let out = strings_capped(e, cap);
    *budget = budget.saturating_sub(out.len());
    out
}

fn strings_capped(e: &El, cap: usize) -> Vec<String> {
    let cache = e.find("strCache").or_else(|| e.find("numCache")).or_else(|| e.find("strLit")).or_else(|| e.find("numLit"));
    if let Some(c) = cache {
        let n = (c.child("ptCount").and_then(|p| p.u32("val")).unwrap_or(0) as usize).min(cap);
        let mut out = vec![String::new(); n];
        for pt in c.children_named("pt").take(MAX_CACHE_POINTS) {
            let i = pt.u32("idx").unwrap_or(0) as usize;
            if i >= cap {
                continue;
            }
            let v = pt.child("v").map(|v| v.text()).unwrap_or_default();
            if i >= out.len() {
                out.resize(i + 1, String::new());
            }
            if let Some(s) = out.get_mut(i) {
                *s = v;
            }
        }
        return out;
    }
    if let Some(ml) = e.find("multiLvlStrCache") {
        // Use the innermost level.
        if let Some(l) = ml.child("lvl") {
            return l.children_named("pt").take(cap).map(|p| p.child("v").map(|v| v.text()).unwrap_or_default()).collect();
        }
    }
    if cap > 0
        && let Some(v) = e.child("v")
    {
        return vec![v.text()];
    }
    vec![]
}

fn numbers(e: &El, budget: &mut usize) -> Vec<Option<f64>> {
    strings(e, budget).into_iter().map(|s| s.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect()
}

/// Parse a chart part. Returns `None` for something that isn't a chart we can read.
pub fn read_chart_xml(bytes: &[u8]) -> Option<Chart> {
    let doc = crate::xml::parse(bytes).ok()?;
    let mut root = doc.root;
    super::resolve_mc(&mut root, 0);
    if !root.is("chartSpace") {
        return None;
    }
    let mut c = chart_from(&root)?;
    c.raw = Some(String::from_utf8_lossy(bytes).into_owned());
    Some(c)
}

pub(crate) fn chart_from(root: &El) -> Option<Chart> {
    chart_within(root, MAX_CHART_POINTS)
}

/// [`chart_from`] decoding at most `budget` data slots in all.
fn chart_within(root: &El, mut budget: usize) -> Option<Chart> {
    let ch = root.child("chart")?;
    let pa = ch.child("plotArea")?;
    let mut c = Chart { categories: vec![], series: vec![], title: None, legend: None, gridlines: false, ..Default::default() };
    c.style = root.child_val("style").and_then(|v| v.parse().ok()).unwrap_or(2);
    if let Some(t) = ch.child("title") {
        c.title = Some(rich_text(t).unwrap_or_else(|| "Chart Title".into()));
    }
    if ch.child("autoTitleDeleted").and_then(|a| a.bool("val")) == Some(true) {
        c.title = None;
    }
    c.legend = ch.child("legend").map(|l| l.child_val("legendPos").unwrap_or("r").to_string());
    c.legend_overlay = ch.child("legend").and_then(|l| l.child("overlay")).and_then(|o| o.bool("val")).unwrap_or(false);
    c.chart_fill = simple_fill(root.child("spPr"));
    c.plot_fill = simple_fill(pa.child("spPr"));

    let mut kinds: Vec<ChartType> = vec![];
    // The value axis the first plot group uses is the primary one; a group on another value axis
    // (a combo chart's line on the right) is on the secondary axis.
    let val_map: std::collections::HashMap<&str, bool> =
        pa.children_named("valAx").filter_map(|a| Some((a.child_val("axId")?, matches!(a.child_val("axPos"), Some("l" | "r"))))).collect();
    // A group's value axis: its vertical one when it has two (scatter), else its one.
    let group_val_axis = |g: &El| {
        let ids: Vec<(&str, bool)> =
            g.children_named("axId").filter_map(|a| a.attr("val")).filter_map(|id| val_map.get(id).copied().map(|v| (id, v))).collect();
        ids.iter().find(|(_, vertical)| *vertical).or(ids.first()).map(|(id, _)| id.to_string())
    };
    // The primary value axis is the one on the left (bottom for bar charts); the secondary one
    // sits on the right or top, or crosses the category axis at its maximum.
    let secondary_placed = |a: &El| a.child_val("crosses") == Some("max") || matches!(a.child_val("axPos"), Some("r" | "t"));
    let val_axes_by_id: std::collections::HashMap<&str, &El> = pa.children_named("valAx").filter_map(|a| Some((a.child_val("axId")?, a))).collect();
    let used: Vec<String> = pa.elements().filter(|g| g.local().ends_with("Chart")).filter_map(group_val_axis).collect();
    let primary_val: Option<String> =
        used.iter().find(|id| val_axes_by_id.get(id.as_str()).is_some_and(|a| !secondary_placed(a))).or(used.first()).cloned();
    for g in pa.elements() {
        let name = g.local();
        if !name.ends_with("Chart") {
            continue;
        }
        let grouping = g.child_val("grouping").unwrap_or("clustered");
        let kind = match name {
            "barChart" | "bar3DChart" => {
                let bar = g.child_val("barDir") == Some("bar");
                match (bar, grouping) {
                    (false, "stacked") => ChartType::StackedColumn,
                    (false, "percentStacked") => ChartType::PercentColumn,
                    (false, _) => ChartType::Column,
                    (true, "stacked") => ChartType::StackedBar,
                    (true, "percentStacked") => ChartType::PercentBar,
                    (true, _) => ChartType::Bar,
                }
            }
            "lineChart" | "line3DChart" => {
                let markers = g.child_val("marker").is_some_and(|v| v == "1" || v == "true")
                    && g.children_named("ser").all(|s| s.path(&["marker", "symbol"]).and_then(|m| m.attr("val")) != Some("none"));
                match grouping {
                    "stacked" | "percentStacked" => ChartType::StackedLine,
                    _ if markers => ChartType::LineMarkers,
                    _ => ChartType::Line,
                }
            }
            "areaChart" | "area3DChart" => {
                if matches!(grouping, "stacked" | "percentStacked") {
                    ChartType::StackedArea
                } else {
                    ChartType::Area
                }
            }
            "pieChart" | "pie3DChart" | "ofPieChart" => ChartType::Pie,
            "doughnutChart" => ChartType::Doughnut,
            "scatterChart" => ChartType::Scatter,
            "bubbleChart" => ChartType::Bubble,
            "radarChart" => {
                if g.child_val("radarStyle") == Some("filled") {
                    ChartType::FilledRadar
                } else {
                    ChartType::Radar
                }
            }
            "stockChart" => {
                c.up_down_bars = g.child("upDownBars").is_some();
                ChartType::Stock
            }
            "surfaceChart" | "surface3DChart" => ChartType::Surface,
            _ => continue,
        };
        // A missing <c:varyColors> means "vary by point" (the schema default is true), as PowerPoint draws it.
        c.vary_colors = g.child_val("varyColors").is_none_or(|v| v == "1" || v == "true");
        // A bar group without c:gapWidth / c:overlap takes the schema's defaults (150 %, 0 %),
        // not the 219 % / -27 % PowerPoint writes into its new charts.
        let bar_group = g.is("barChart") || g.is("bar3DChart");
        if let Some(v) = g.child_val("gapWidth").and_then(|v| v.parse::<f64>().ok()) {
            c.gap_width = (v / 100.0).clamp(0.0, 5.0);
        } else if bar_group {
            c.gap_width = 1.5;
        }
        if let Some(v) = g.child_val("overlap").and_then(|v| v.parse::<f64>().ok()) {
            c.overlap = (v / 100.0).clamp(-1.0, 1.0);
        } else if bar_group {
            c.overlap = 0.0;
        }
        if let Some(v) = g.child_val("holeSize").and_then(|v| v.parse::<f64>().ok()) {
            c.hole_size = (v / 100.0).clamp(0.0, 0.9);
        }
        // `<c:showVal/>` without a val attribute means true, as in the schema's default.
        // The setting belongs to this plot group: a group that asks for no labels keeps them
        // off, even when another group of a combo chart asks for them (#868).
        let group_labels = g.path(&["dLbls", "showVal"]).map(|v| v.bool("val").unwrap_or(true));
        if group_labels == Some(true) {
            c.data_labels = true;
        }
        kinds.push(kind);
        let val_axis = group_val_axis(g);
        let secondary = val_axis.is_some() && val_axis != primary_val;
        for s in g.children_named("ser").take(1000) {
            let mut ser = Series {
                name: s.child("tx").map(|t| strings(t, &mut budget)).and_then(|v| v.into_iter().next()).unwrap_or_default(),
                fill: simple_fill(s.child("spPr")),
                line: simple_line(s.child("spPr")),
                smooth: s.child("smooth").and_then(|v| v.bool("val")).unwrap_or(false),
                marker: s.path(&["marker", "symbol"]).and_then(|m| m.attr("val")).filter(|m| *m != "none").map(String::from),
                kind: Some(kind),
                secondary_axis: secondary,
                ..Default::default()
            };
            // `<c:showVal/>` without a val attribute means true, as in the schema's default.
            if let Some(d) = s.child("dLbls").and_then(|d| d.child("showVal")).map(|v| v.bool("val").unwrap_or(true)) {
                ser.data_labels = Some(d);
            } else if let Some(false) = group_labels {
                // The group asks for no labels and the series does not say otherwise.
                ser.data_labels = Some(false);
            }
            for t in s.children_named("trendline").take(16) {
                if let Some(t) = trendline(t) {
                    ser.trendlines.push(t);
                }
            }
            // Data labels' number format: the series' labels', the group's, else the source data's.
            let label_fmt = |d: Option<&El>| {
                d.and_then(|d| d.child("numFmt"))
                    .filter(|n| n.attr("sourceLinked") != Some("1"))
                    .and_then(|n| n.attr("formatCode"))
                    .map(str::to_string)
            };
            let source =
                s.child("val").or_else(|| s.child("yVal")).and_then(|v| v.find("numCache")).and_then(|c| c.child("formatCode")).map(|f| f.text());
            ser.number_format = label_fmt(s.child("dLbls")).or_else(|| label_fmt(g.child("dLbls"))).or(source).filter(|f| !f.is_empty());
            for dp in s.children_named("dPt") {
                if let (Some(i), Some(f)) = (dp.child("idx").and_then(|i| i.u32("val")), simple_fill(dp.child("spPr"))) {
                    ser.point_fills.push((i as usize, f));
                }
            }
            if let Some(cat) = s.child("cat").or_else(|| s.child("xVal")) {
                let cats = strings(cat, &mut budget);
                if matches!(kind, ChartType::Scatter | ChartType::Bubble) {
                    ser.x_values = cats.iter().map(|v| v.trim().parse::<f64>().ok()).collect();
                }
                if c.categories.is_empty() {
                    c.categories = cats;
                }
            }
            if let Some(v) = s.child("val").or_else(|| s.child("yVal")) {
                ser.values = numbers(v, &mut budget);
            }
            // Error bars on the values: a scatter chart's y bars, any other chart's only ones.
            if let Some(e) = s
                .children_named("errBars")
                .find(|e| e.child_val("errDir").is_none_or(|d| d == "y" || !matches!(kind, ChartType::Scatter | ChartType::Bubble)))
            {
                let d = ErrorBars::default();
                ser.error_bars = Some(ErrorBars {
                    bar_type: e.child_val("errBarType").unwrap_or("both").to_string(),
                    val_type: e.child_val("errValType").unwrap_or("fixedVal").to_string(),
                    value: e.child_val("val").and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite()).unwrap_or(d.value),
                    plus: e.child("plus").map(|p| numbers(p, &mut budget)).unwrap_or_default(),
                    minus: e.child("minus").map(|p| numbers(p, &mut budget)).unwrap_or_default(),
                    end_cap: e.child("noEndCap").and_then(|v| v.bool("val")) != Some(true),
                });
            }
            if let Some(b) = s.child("bubbleSize") {
                ser.sizes = numbers(b, &mut budget);
            }
            c.series.push(ser);
        }
    }
    // `autoTitleDeleted val="0"` without `c:title`: PowerPoint and LibreOffice draw the series name as the title of a single-series chart.
    if ch.child("title").is_none() && ch.child("autoTitleDeleted").and_then(|a| a.bool("val")) == Some(false) && c.series.len() == 1 {
        c.title = c.series.first().map(|s| s.name.clone()).filter(|n| !n.is_empty());
    }
    let first = *kinds.first()?;
    c.kind = if kinds.iter().any(|k| *k != first) { ChartType::Combo } else { first };
    if c.kind != ChartType::Combo {
        for s in &mut c.series {
            s.kind = None;
            s.secondary_axis = false;
        }
    }
    // Axes.
    c.lang = root.child_val("lang").map(str::to_string);
    let vertical = |a: &&El| matches!(a.child_val("axPos"), Some("l" | "r"));
    let by_id = |id: &Option<String>| pa.children_named("valAx").find(|a| id.is_some() && a.child_val("axId") == id.as_deref());
    let val_ax = by_id(&primary_val).or_else(|| pa.children_named("valAx").find(vertical)).or_else(|| pa.children_named("valAx").last());
    let num = |a: &El, k: &str| a.child_val(k).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
    // An axis' number format: its own unless linked to the source (`sourceLinked="1"`, or no
    // c:numFmt at all), in which case the data's format of the first series plotted on it.
    let source_format = |ax: &El| {
        let id = ax.child_val("axId");
        pa.elements()
            .filter(|g| g.children_named("axId").any(|a| a.attr("val") == id))
            .flat_map(|g| g.children_named("ser"))
            .find_map(|s| {
                s.child("val").or_else(|| s.child("yVal")).and_then(|v| v.find("numCache")).and_then(|c| c.child("formatCode")).map(|f| f.text())
            })
            .filter(|f| !f.is_empty() && f != "General")
    };
    let format_code = |a: &El| match a.child("numFmt") {
        Some(n) if n.attr("sourceLinked") != Some("1") => n.attr("formatCode").map(str::to_string),
        n => source_format(a).or_else(|| n.and_then(|n| n.attr("formatCode")).map(str::to_string)),
    };
    let deleted = |a: &El| a.child("delete").and_then(|d| d.bool("val")) == Some(true);
    if c.kind == ChartType::Combo && c.series.iter().any(|s| s.secondary_axis) {
        let primary_id = val_ax.and_then(|a| a.child_val("axId"));
        // The other vertical value axis is the secondary one.
        if let Some(a) = pa.children_named("valAx").find(|a| a.child_val("axId") != primary_id && vertical(a)) {
            let sc = a.child("scaling");
            c.y2_min = sc.and_then(|s| num(s, "min"));
            c.y2_max = sc.and_then(|s| num(s, "max"));
            c.y2_major = num(a, "majorUnit");
            c.y2_format = format_code(a);
            c.y2_hidden = deleted(a);
        }
    }
    let cat_ax = pa.child("catAx").or_else(|| pa.child("dateAx")).or_else(|| pa.children_named("valAx").find(|a| !vertical(a)));
    if let Some(v) = val_ax {
        c.gridlines = v.child("majorGridlines").is_some();
        c.minor_gridlines = v.child("minorGridlines").is_some();
        let sc = v.child("scaling");
        c.y_min = sc.and_then(|s| s.child_val("min")).and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite());
        c.y_max = sc.and_then(|s| s.child_val("max")).and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite());
        c.axis_titles.1 = v.child("title").map(|t| rich_text(t).unwrap_or_else(|| "Axis Title".into()));
        c.y_major = num(v, "majorUnit");
        c.y_log_base = sc.and_then(|s| s.child_val("logBase")).and_then(|v| v.parse().ok()).filter(|b: &f64| b.is_finite() && *b >= 2.0);
        c.y_format = format_code(v);
        c.y_hidden = deleted(v);
    }
    if let Some(a) = cat_ax {
        c.cat_reversed = a.child("scaling").and_then(|s| s.child_val("orientation")).is_some_and(|o| o == "maxMin");
        c.axis_titles.0 = a.child("title").map(|t| rich_text(t).unwrap_or_else(|| "Axis Title".into()));
    }
    // Text sizes as the file sets them (PowerPoint draws chart text at these sizes, not scaled to
    // the chart): the axes' and legend's, else the whole chart's; the title's own.
    let chart_size = text_size(root.child("txPr"));
    c.text_size = [val_ax, cat_ax, ch.child("legend")].into_iter().flatten().find_map(|e| text_size(e.child("txPr"))).or(chart_size);
    // Text colour likewise: the axes' and legend's, else the whole chart's.
    c.text_color = [val_ax, cat_ax, ch.child("legend"), Some(root)].into_iter().flatten().find_map(|e| text_color(e.child("txPr")));
    c.title_size = ch.child("title").and_then(|t| text_size(t.child("tx").and_then(|x| x.child("rich"))).or_else(|| text_size(t.child("txPr"))));
    Some(c)
}

fn trend_num(e: &El, k: &str) -> Option<f64> {
    e.child_val(k).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && v.abs() <= 1.0e12)
}

fn trend_name(s: &str) -> Option<String> {
    let cleaned: String =
        s.chars().filter(|c| matches!(c, '\t' | '\n' | '\r') || (*c >= ' ' && *c != '\u{FFFE}' && *c != '\u{FFFF}')).take(255).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() { None } else { Some(cleaned.to_string()) }
}

/// One `c:trendline`. Unknown types are skipped.
fn trendline(e: &El) -> Option<Trendline> {
    let kind = TrendlineType::from_xml(e.child_val("trendlineType")?)?;
    Some(Trendline {
        kind,
        name: e.child("name").map(|n| n.text()).as_deref().and_then(trend_name),
        order: if kind == TrendlineType::Poly { e.child("order").and_then(|o| o.u32("val")).map(|v| v.clamp(2, 6)) } else { None },
        period: if kind == TrendlineType::MovingAvg { e.child("period").and_then(|o| o.u32("val")).map(|v| v.clamp(2, 255)) } else { None },
        forward: trend_num(e, "forward"),
        backward: trend_num(e, "backward"),
        intercept: trend_num(e, "intercept"),
        disp_eq: e.child("dispEq").and_then(|d| d.bool("val")).unwrap_or(false),
        disp_r_sqr: e.child("dispRSqr").and_then(|d| d.bool("val")).unwrap_or(false),
    })
}

/// The colour of the first `a:defRPr` or `a:rPr` with a solid fill under `e`.
fn text_color(e: Option<&El>) -> Option<deckcraft_model::ColorRef> {
    fn rec(e: &El, depth: usize) -> Option<deckcraft_model::ColorRef> {
        if depth > 8 {
            return None;
        }
        for c in e.elements() {
            if (c.is("defRPr") || c.is("rPr"))
                && let Some(Fill::Solid { color }) = simple_fill(Some(c))
            {
                return Some(color);
            }
            if let Some(v) = rec(c, depth + 1) {
                return Some(v);
            }
        }
        None
    }
    rec(e?, 0)
}

const MAX_KEPT_PARTS: usize = 256;
const MAX_KEPT_DEPTH: usize = 8;
pub const MAX_KEPT_TOTAL_BYTES: usize = 32 * 1024 * 1024;

/// Relationships and parts a chart points at, so a later save can write them back.
pub(crate) fn capture_kept(imp: &mut super::Imp, chart_part: &str) -> deckcraft_model::chart::ChartKept {
    use deckcraft_model::chart::{ChartKept, ChartRel};

    let key = crate::opc::norm(chart_part);
    if let Some(cached) = imp.kept_charts.get(&key) {
        return cached.clone();
    }

    let mut kept = ChartKept::default();
    // The chart document itself is rewritten; don't store a second copy of it.
    let mut seen = std::collections::HashMap::from([(key.clone(), true)]);
    for r in imp.pkg.rels(chart_part).items {
        if !safe_rel(&r.id, &r.ty, &r.target_raw) {
            log::warn!("pptx: skipping chart relationship {}", r.id);
            continue;
        }
        if r.external {
            kept.rels.push(ChartRel { id: r.id, rel_type: r.ty, target: r.target_raw, part: String::new(), external: true });
            continue;
        }
        if copy_part(imp.pkg, &r.target, &mut kept.parts, &mut seen, &mut imp.kept_sidecar_bytes, 0) {
            kept.rels.push(ChartRel { id: r.id, rel_type: r.ty, target: r.target_raw, part: r.target, external: false });
        }
    }
    imp.kept_charts.insert(key, kept.clone());
    kept
}

fn safe_rel(id: &str, ty: &str, target: &str) -> bool {
    [id, ty, target].iter().all(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '\'')))
}

/// `true` when `part` is stored (now or from an earlier relationship).
fn copy_part(
    pkg: &crate::opc::Package,
    part: &str,
    out: &mut Vec<deckcraft_model::chart::ChartPart>,
    seen: &mut std::collections::HashMap<String, bool>,
    retained_bytes: &mut usize,
    depth: usize,
) -> bool {
    use deckcraft_model::chart::ChartPart;

    if depth > MAX_KEPT_DEPTH || out.len() >= MAX_KEPT_PARTS {
        log::warn!("pptx: chart part closure stopped at {part}");
        return false;
    }
    let key = crate::opc::norm(part);
    if let Some(ok) = seen.get(&key) {
        return *ok;
    }
    if !crate::opc::sidecar_part(part) {
        log::warn!("pptx: chart relationship target {part} is not kept");
        seen.insert(key, false);
        return false;
    }
    let Some(bytes) = pkg.get(part) else {
        log::warn!("pptx: chart relationship target {part} is missing");
        seen.insert(key, false);
        return false;
    };
    let name = pkg.original_name(part).unwrap_or(part).trim_start_matches('/').to_string();
    if !crate::opc::sidecar_part(&name) {
        log::warn!("pptx: chart part name {name} is not kept");
        seen.insert(key, false);
        return false;
    }
    let content_type = pkg.content_type(part).unwrap_or_default();
    let stored_bytes = if *retained_bytes + bytes.len() <= MAX_KEPT_TOTAL_BYTES {
        *retained_bytes += bytes.len();
        bytes.to_vec()
    } else {
        log::warn!("pptx: chart sidecar retained byte budget exceeded for {part}");
        Vec::new()
    };
    out.push(ChartPart { name, content_type, bytes: stored_bytes, native_entry: None });
    seen.insert(key, true);
    let rels_name = crate::opc::rels_path(part);
    if crate::opc::sidecar_part(&rels_name)
        && seen.get(&crate::opc::norm(&rels_name)).is_none()
        && let Some(rels_bytes) = pkg.get(&rels_name)
    {
        seen.insert(crate::opc::norm(&rels_name), true);
        let name = pkg.original_name(&rels_name).unwrap_or(&rels_name).trim_start_matches('/').to_string();
        let stored_rels = if *retained_bytes + rels_bytes.len() <= MAX_KEPT_TOTAL_BYTES {
            *retained_bytes += rels_bytes.len();
            rels_bytes.to_vec()
        } else {
            Vec::new()
        };
        out.push(ChartPart {
            name,
            content_type: "application/vnd.openxmlformats-package.relationships+xml".into(),
            bytes: stored_rels,
            native_entry: None,
        });
        let targets: Vec<String> = pkg.rels(part).items.into_iter().filter(|r| !r.external).map(|r| r.target).collect();
        for target in targets {
            copy_part(pkg, &target, out, seen, retained_bytes, depth + 1);
        }
    }
    true
}

/// The size in points of the first `a:defRPr` or `a:rPr` with one under `e` (`sz` in hundredths).
fn text_size(e: Option<&El>) -> Option<f64> {
    fn rec(e: &El, depth: usize) -> Option<f64> {
        if depth > 8 {
            return None;
        }
        for c in e.elements() {
            if (c.is("defRPr") || c.is("rPr"))
                && let Some(v) = c.attr("sz").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v > 0.0)
            {
                return Some((v / 100.0).clamp(1.0, 400.0));
            }
            if let Some(v) = rec(c, depth + 1) {
                return Some(v);
            }
        }
        None
    }
    rec(e?, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_caches_share_one_chart_wide_budget() {
        // Every series declares 100000 points but writes one: without a chart-wide budget each
        // would expand to the full count.
        let ser = |n: usize| {
            format!(
                r#"<c:ser><c:idx val="{n}"/><c:val><c:numRef><c:numCache><c:ptCount val="100000"/><c:pt idx="0"><c:v>{n}</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>"#
            )
        };
        let sers: String = (0..5).map(ser).collect();
        let xml = format!(
            r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/>{sers}</c:barChart></c:plotArea></c:chart></c:chartSpace>"#
        );
        let doc = crate::xml::parse(xml.as_bytes()).unwrap();
        let c = chart_within(&doc.root, 250_000).unwrap();
        let total: usize = c.series.iter().map(|s| s.values.len()).sum();
        assert!(total <= 250_000, "{total}");
        assert_eq!(c.series.len(), 5);
        assert_eq!(c.series[0].values.len(), 100_000);
        assert_eq!(c.series[0].values[0], Some(0.0));
        assert!(c.series[4].values.is_empty(), "the budget ran out before the last series");
        // A small chart is untouched by the default budget.
        let c = chart_from(&doc.root).unwrap();
        assert!(c.series.iter().all(|s| s.values.len() == 100_000));
    }

    #[test]
    fn minor_gridlines_are_read_and_written_back() {
        let mut c = Chart::sample(ChartType::Line);
        c.minor_gridlines = true;
        let back = read_chart_xml(&crate::write::chart::chart_xml(&c).0).unwrap();
        assert!(back.gridlines && back.minor_gridlines);
        c.minor_gridlines = false;
        assert!(!read_chart_xml(&crate::write::chart::chart_xml(&c).0).unwrap().minor_gridlines);
    }

    #[test]
    fn series_pattern_fills_are_read_and_written_back() {
        // A pattern-filled series and an empty one with an outline, as PowerPoint writes them.
        let xml = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:spPr><a:pattFill prst="openDmnd"><a:fgClr><a:schemeClr val="accent4"/></a:fgClr><a:bgClr><a:schemeClr val="bg1"/></a:bgClr></a:pattFill></c:spPr><c:val><c:numLit><c:ptCount val="1"/><c:pt idx="0"><c:v>5</c:v></c:pt></c:numLit></c:val></c:ser><c:ser><c:idx val="1"/><c:spPr><a:noFill/><a:ln w="19050"><a:solidFill><a:schemeClr val="accent4"/></a:solidFill></a:ln></c:spPr><c:val><c:numLit><c:ptCount val="1"/><c:pt idx="0"><c:v>3</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = read_chart_xml(xml).unwrap();
        let accent4 = deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent4);
        let pattern = deckcraft_model::style::PatternFill {
            preset: "openDmnd".into(),
            fg: accent4.clone(),
            bg: deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Bg1),
        };
        assert_eq!(c.series[0].fill, Some(Fill::Pattern(pattern.clone())));
        assert_eq!(c.series[1].fill, Some(Fill::None));
        assert_eq!(c.series[1].line.as_ref().and_then(|l| l.fill.clone()), Some(Fill::solid(accent4)));
        // Written out from the model (an edited chart), the pattern survives.
        let mut edited = c.clone();
        edited.raw = None;
        edited.title = Some("Edited".into());
        let back = read_chart_xml(&crate::write::chart::chart_xml(&edited).0).unwrap();
        assert_eq!(back.series[0].fill, Some(Fill::Pattern(pattern)));
        assert_eq!(back.series[1].fill, Some(Fill::None));
    }

    #[test]
    fn log_base_is_read() {
        let xml = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:lineChart><c:axId val="1"/><c:axId val="2"/></c:lineChart><c:catAx><c:axId val="1"/></c:catAx><c:valAx><c:axId val="2"/><c:scaling><c:logBase val="10"/><c:orientation val="minMax"/></c:scaling></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        assert_eq!(read_chart_xml(xml).unwrap().y_log_base, Some(10.0));
    }

    #[test]
    fn error_bars_are_read() {
        let xml = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:errBars><c:errBarType val="plus"/><c:errValType val="percentage"/><c:noEndCap val="1"/><c:val val="20"/></c:errBars><c:val><c:numLit><c:ptCount val="1"/><c:pt idx="0"><c:v>5</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = read_chart_xml(xml).unwrap();
        let e = c.series[0].error_bars.clone().unwrap();
        assert_eq!((e.bar_type.as_str(), e.val_type.as_str(), e.value, e.end_cap), ("plus", "percentage", 20.0, false));
    }

    #[test]
    fn legend_position_and_overlay_are_read() {
        let legend = r#"<c:legend><c:legendPos val="r"/><c:overlay val="1"/></c:legend>"#;
        let xml = format!(
            r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/></c:barChart></c:plotArea>{legend}</c:chart></c:chartSpace>"#
        );
        let c = read_chart_xml(xml.as_bytes()).unwrap();
        assert_eq!(c.legend.as_deref(), Some("r"));
        assert!(c.legend_overlay);
        let xml = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/></c:barChart></c:plotArea><c:legend/></c:chart></c:chartSpace>"#;
        let c = read_chart_xml(xml).unwrap();
        assert_eq!(c.legend.as_deref(), Some("r"));
        assert!(!c.legend_overlay);
    }

    #[test]
    fn combo_chart_with_a_secondary_axis() {
        // Columns on axes 1/2, a line on axes 3/4 whose value axis is on the right.
        let xml = br##"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:lang val="ru-RU"/><c:chart><c:plotArea>
<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:tx><c:v>Spend</c:v></c:tx><c:cat><c:strLit><c:ptCount val="2"/><c:pt idx="0"><c:v>W1</c:v></c:pt><c:pt idx="1"><c:v>W2</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:ptCount val="2"/><c:pt idx="0"><c:v>200000</c:v></c:pt><c:pt idx="1"><c:v>310000</c:v></c:pt></c:numLit></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart>
<c:lineChart><c:grouping val="standard"/><c:ser><c:idx val="1"/><c:tx><c:v>Clicks</c:v></c:tx><c:spPr><a:ln w="28575"><a:solidFill><a:srgbClr val="00B050"/></a:solidFill></a:ln></c:spPr><c:val><c:numLit><c:ptCount val="2"/><c:pt idx="0"><c:v>6000</c:v></c:pt><c:pt idx="1"><c:v>7400</c:v></c:pt></c:numLit></c:val></c:ser><c:marker val="1"/><c:axId val="3"/><c:axId val="4"/></c:lineChart>
<c:catAx><c:axId val="1"/><c:axPos val="b"/><c:crossAx val="2"/></c:catAx><c:valAx><c:axId val="2"/><c:axPos val="l"/><c:numFmt formatCode="#,##0" sourceLinked="1"/><c:crossAx val="1"/><c:majorUnit val="100000"/></c:valAx>
<c:valAx><c:axId val="4"/><c:scaling><c:max val="8000"/></c:scaling><c:axPos val="r"/><c:numFmt formatCode="0.0" sourceLinked="0"/><c:crossAx val="3"/></c:valAx><c:catAx><c:axId val="3"/><c:delete val="1"/><c:axPos val="b"/><c:crossAx val="4"/></c:catAx>
</c:plotArea></c:chart></c:chartSpace>"##;
        let c = read_chart_xml(xml).unwrap();
        assert_eq!(c.kind, ChartType::Combo);
        assert_eq!(c.series.len(), 2);
        assert!(!c.series[0].secondary_axis);
        assert!(c.series[1].secondary_axis);
        assert_eq!(c.series[1].kind, Some(ChartType::LineMarkers));
        assert!(c.series[1].line.as_ref().is_some_and(|l| l.width.is_some_and(|w| (w - 2.25).abs() < 0.01)));
        // Each value axis' number format, bounds and unit; the chart's language.
        assert_eq!(c.y_format.as_deref(), Some("#,##0"));
        assert_eq!((c.y_major, c.y2_max, c.y2_format.as_deref()), (Some(100000.0), Some(8000.0), Some("0.0")));
        assert_eq!(c.lang.as_deref(), Some("ru-RU"));
        // Group order doesn't decide: columns on the right-hand axis are the secondary series even
        // when their group comes first, and the line on the left is primary.
        let swapped = String::from_utf8_lossy(xml)
            .replace(
                "<c:axPos val=\"l\"/><c:numFmt formatCode=\"#,##0\"",
                "<c:axPos val=\"r\"/><c:crosses val=\"max\"/><c:numFmt formatCode=\"#,##0\"",
            )
            .replace("<c:axPos val=\"r\"/><c:numFmt formatCode=\"0.0\"", "<c:axPos val=\"l\"/><c:numFmt formatCode=\"0.0\"");
        let c2 = read_chart_xml(swapped.as_bytes()).unwrap();
        assert!(c2.series[0].secondary_axis && !c2.series[1].secondary_axis);
        assert_eq!((c2.y_format.as_deref(), c2.y2_format.as_deref()), (Some("0.0"), Some("#,##0")));
        // A chart of one type on one axis has no secondary series.
        let single = String::from_utf8_lossy(xml)
            .replace("<c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>", "<c:axId val=\"1\"/><c:axId val=\"2\"/></c:lineChart>");
        let c = read_chart_xml(single.as_bytes()).unwrap();
        assert!(c.series.iter().all(|s| !s.secondary_axis));
    }

    #[test]
    fn linked_axis_formats_come_from_the_data() {
        // No c:numFmt on the value axis, or a source-linked one: the series' own format.
        let xml = |ax_fmt: &str| {
            format!(
                r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:val><c:numRef><c:numCache><c:formatCode>#,##0</c:formatCode><c:ptCount val="1"/><c:pt idx="0"><c:v>1200</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart><c:catAx><c:axId val="1"/></c:catAx><c:valAx><c:axId val="2"/>{ax_fmt}<c:axPos val="l"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#
            )
        };
        let fmt = |x: &str| read_chart_xml(xml(x).as_bytes()).unwrap().y_format;
        assert_eq!(fmt("").as_deref(), Some("#,##0"));
        assert_eq!(fmt(r#"<c:numFmt formatCode="General" sourceLinked="1"/>"#).as_deref(), Some("#,##0"));
        assert_eq!(fmt(r#"<c:numFmt formatCode="0.0" sourceLinked="0"/>"#).as_deref(), Some("0.0"));
    }

    #[test]
    fn text_colour_is_read_from_the_axes() {
        // LibreOffice's templates set light axis text for dark slides on the axes' c:txPr.
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:val><c:numRef><c:numCache><c:ptCount val="1"/><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart><c:catAx><c:axId val="1"/><c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr sz="1000"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:defRPr></a:pPr></a:p></c:txPr></c:catAx><c:valAx><c:axId val="2"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = read_chart_xml(xml.as_bytes()).unwrap();
        assert_eq!(c.text_color, Some(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::rgb(255, 255, 255))));
        assert!(!c.cat_reversed);
        let rev = read_chart_xml(
            xml.replace("<c:catAx><c:axId val=\"1\"/>", "<c:catAx><c:axId val=\"1\"/><c:scaling><c:orientation val=\"maxMin\"/></c:scaling>")
                .as_bytes(),
        )
        .unwrap();
        assert!(rev.cat_reversed, "maxMin reverses the category axis");
        // No c:gapWidth / c:overlap: the schema's 150 % and 0 %.
        assert_eq!((c.gap_width, c.overlap), (1.5, 0.0));
    }

    #[test]
    fn text_sizes_are_read_from_the_file() {
        // PowerPoint's new charts set the axes' and legend's text to 11.97 pt and the title's to
        // 18.62 pt; a chart that only sets its overall text size uses it everywhere.
        let ns =
            r#"xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
        let tx = |sz: u32| format!(r#"<c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr sz="{sz}"/></a:pPr></a:p></c:txPr>"#);
        let xml = format!(
            r#"<c:chartSpace {ns}><c:chart><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:pPr><a:defRPr sz="1862"/></a:pPr><a:r><a:t>T</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:barDir val="col"/><c:axId val="1"/><c:axId val="2"/></c:barChart><c:catAx><c:axId val="1"/>{}</c:catAx><c:valAx><c:axId val="2"/>{}</c:valAx></c:plotArea></c:chart></c:chartSpace>"#,
            tx(1197),
            tx(1197)
        );
        let c = read_chart_xml(xml.as_bytes()).unwrap();
        assert_eq!((c.text_size, c.title_size), (Some(11.97), Some(18.62)));
        let xml = format!(
            r#"<c:chartSpace {ns}><c:chart><c:plotArea><c:barChart><c:barDir val="col"/></c:barChart></c:plotArea></c:chart>{}</c:chartSpace>"#,
            tx(1400)
        );
        let c = read_chart_xml(xml.as_bytes()).unwrap();
        assert_eq!((c.text_size, c.title_size), (Some(14.0), None));
        let xml = format!(
            r#"<c:chartSpace {ns}><c:chart><c:plotArea><c:barChart><c:barDir val="col"/></c:barChart></c:plotArea></c:chart></c:chartSpace>"#
        );
        assert_eq!(read_chart_xml(xml.as_bytes()).unwrap().text_size, None, "no sizes: the renderer picks one");
    }

    #[test]
    fn trendlines_are_read() {
        let xml = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:lineChart><c:grouping val="standard"/><c:ser><c:idx val="0"/><c:tx><c:v>Spend</c:v></c:tx>
<c:trendline><c:name>Fit &amp; forecast</c:name><c:trendlineType val="linear"/><c:forward val="1"/><c:backward val="0.5"/><c:intercept val="0"/><c:dispRSqr val="1"/><c:dispEq val="1"/></c:trendline>
<c:trendline><c:trendlineType val="poly"/><c:order val="9"/></c:trendline>
<c:trendline><c:trendlineType val="movingAvg"/><c:period val="3"/></c:trendline>
<c:trendline><c:trendlineType val="notAType"/></c:trendline>
<c:val><c:numLit><c:ptCount val="1"/><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = read_chart_xml(xml).unwrap();
        let t = &c.series[0].trendlines;
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].name.as_deref(), Some("Fit & forecast"));
        assert_eq!(t[0].kind, TrendlineType::Linear);
        assert_eq!((t[0].forward, t[0].backward, t[0].intercept), (Some(1.0), Some(0.5), Some(0.0)));
        assert!(t[0].disp_eq && t[0].disp_r_sqr);
        assert_eq!((t[1].kind, t[1].order), (TrendlineType::Poly, Some(6)));
        assert_eq!((t[2].kind, t[2].period), (TrendlineType::MovingAvg, Some(3)));
    }

    const NS_C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
    const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

    fn chart(auto_deleted: &str, title: &str, series: usize) -> Chart {
        let val = r#"<c:val><c:numLit><c:ptCount val="1"/><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val>"#;
        let ser: String = (0..series).map(|i| format!(r#"<c:ser><c:idx val="{i}"/><c:tx><c:v>S{i}</c:v></c:tx>{val}</c:ser>"#)).collect();
        let plot = format!(r#"<c:plotArea><c:barChart><c:barDir val="col"/>{ser}</c:barChart></c:plotArea>"#);
        let xml = format!(r#"<c:chartSpace xmlns:c="{NS_C}" xmlns:a="{NS_A}"><c:chart>{title}{auto_deleted}{plot}</c:chart></c:chartSpace>"#);
        read_chart_xml(xml.as_bytes()).unwrap()
    }

    #[test]
    fn single_series_gets_automatic_title() {
        assert_eq!(chart(r#"<c:autoTitleDeleted val="0"/>"#, "", 1).title.as_deref(), Some("S0"));
    }

    #[test]
    fn no_automatic_title_when_deleted_or_multi_series() {
        assert_eq!(chart(r#"<c:autoTitleDeleted val="1"/>"#, "", 1).title, None);
        assert_eq!(chart("", "", 1).title, None);
        assert_eq!(chart(r#"<c:autoTitleDeleted val="0"/>"#, "", 2).title, None);
    }

    #[test]
    fn explicit_title_wins() {
        let t = "<c:title><c:tx><c:rich><a:p><a:r><a:t>Mine</a:t></a:r></a:p></c:rich></c:tx></c:title>";
        assert_eq!(chart(r#"<c:autoTitleDeleted val="0"/>"#, t, 1).title.as_deref(), Some("Mine"));
    }

    const NS_CHART: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";

    fn vary_xml(vary: &str) -> String {
        let ser = r#"<c:ser><c:idx val="0"/><c:order val="0"/></c:ser>"#;
        let bar = format!(r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>{vary}{ser}</c:barChart>"#);
        format!(r#"<c:chartSpace xmlns:c="{NS_CHART}"><c:chart><c:plotArea>{bar}</c:plotArea></c:chart></c:chartSpace>"#)
    }

    #[test]
    fn missing_vary_colors_means_vary_by_point() {
        assert!(read_chart_xml(vary_xml("").as_bytes()).is_some_and(|c| c.vary_colors));
    }

    #[test]
    fn explicit_vary_colors_is_respected() {
        assert!(read_chart_xml(vary_xml(r#"<c:varyColors val="0"/>"#).as_bytes()).is_some_and(|c| !c.vary_colors));
        assert!(read_chart_xml(vary_xml(r#"<c:varyColors val="1"/>"#).as_bytes()).is_some_and(|c| c.vary_colors));
    }
}
