use super::{
    Chart, ChartType, Result, RunKind, Session, Shape, ShapeKind, Value, Xfrm, add_shape, bad, bool_param, color_value, edit_shapes, json,
    rect_param, str_param,
};

pub fn chart_type(name: &str) -> ChartType {
    match name.to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
        "bar" | "clusteredbar" => ChartType::Bar,
        "stackedbar" => ChartType::StackedBar,
        "percentbar" => ChartType::PercentBar,
        "line" => ChartType::Line,
        "linemarkers" | "linewithmarkers" => ChartType::LineMarkers,
        "stackedline" => ChartType::StackedLine,
        "area" => ChartType::Area,
        "stackedarea" => ChartType::StackedArea,
        "pie" => ChartType::Pie,
        "doughnut" | "donut" => ChartType::Doughnut,
        "scatter" | "xy" => ChartType::Scatter,
        "bubble" => ChartType::Bubble,
        "radar" => ChartType::Radar,
        "filledradar" => ChartType::FilledRadar,
        "stackedcolumn" => ChartType::StackedColumn,
        "percentcolumn" => ChartType::PercentColumn,
        "waterfall" => ChartType::Waterfall,
        "funnel" => ChartType::Funnel,
        "histogram" => ChartType::Histogram,
        "pareto" => ChartType::Pareto,
        "treemap" => ChartType::Treemap,
        "sunburst" => ChartType::Sunburst,
        "boxwhisker" | "box" => ChartType::BoxWhisker,
        "stock" => ChartType::Stock,
        "surface" => ChartType::Surface,
        "combo" => ChartType::Combo,
        _ => ChartType::Column,
    }
}

/// The whole-value plan of a chart command, the one source of truth for a single-user command and
/// for the collaboration normaliser (RFC §3.2.1): `insert.chart` builds the sample and applies the
/// typed data, `chart.type`, `chart.data` and `chart.options` change the chart they are handed.
///
/// The normaliser runs this plan on the chart its author sees and sends the whole resulting value,
/// so a replica never merges arguments against a stale (perhaps imported) copy of its own. The
/// plan mutates only after it has parsed everything the command names; a refusal therefore leaves
/// the chart as it was.
pub fn plan(cmd: &str, p: &Value, c: &mut Chart) -> Result<Value> {
    match cmd {
        "insert.chart" => {
            *c = Chart::sample(chart_type(str_param(p, "type").unwrap_or("column")));
            if let Some(cats) = p.get("categories").and_then(Value::as_array) {
                c.categories = cats.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect();
            }
            if let Some(series) = p.get("series").and_then(Value::as_array) {
                c.series = series
                    .iter()
                    .map(|sv| deckcraft_model::chart::Series {
                        name: sv.get("name").and_then(Value::as_str).unwrap_or("Series").to_string(),
                        values: sv.get("values").and_then(Value::as_array).map(|a| a.iter().map(Value::as_f64).collect()).unwrap_or_default(),
                        ..Default::default()
                    })
                    .collect();
            }
            if let Some(t) = p.get("title") {
                c.title = t.as_str().map(String::from);
            }
            Ok(Value::Null)
        }
        "chart.type" => {
            let k = chart_type(str_param(p, "type").unwrap_or("column"));
            c.kind = k;
            c.vary_colors = matches!(
                k,
                ChartType::Pie | ChartType::Doughnut | ChartType::Funnel | ChartType::Treemap | ChartType::Sunburst | ChartType::Waterfall
            ) || c.series.len() == 1 && c.vary_colors;
            if matches!(k, ChartType::Waterfall | ChartType::Histogram | ChartType::Pareto | ChartType::BoxWhisker) {
                c.gridlines = true;
            }
            c.raw = None;
            Ok(Value::Null)
        }
        "chart.data" => {
            let cats: Option<Vec<String>> = p
                .get("categories")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect());
            let series: Option<Vec<deckcraft_model::chart::Series>> = p.get("series").and_then(Value::as_array).map(|a| {
                a.iter()
                    .map(|sv| deckcraft_model::chart::Series {
                        name: sv.get("name").and_then(Value::as_str).unwrap_or("Series").to_string(),
                        values: sv.get("values").and_then(Value::as_array).map(|v| v.iter().map(Value::as_f64).collect()).unwrap_or_default(),
                        ..Default::default()
                    })
                    .collect()
            });
            let levels: Option<Vec<Vec<String>>> = p.get("levels").and_then(Value::as_array).map(|a| {
                a.iter()
                    .take(deckcraft_model::chart::MAX_LEVELS)
                    .map(|l| {
                        l.as_array()
                            .map(|v| v.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect())
                            .unwrap_or_default()
                    })
                    .collect()
            });
            if let Some(cs) = &cats {
                c.categories = cs.clone();
            }
            if let Some(l) = &levels {
                c.category_levels = l.clone();
            }
            if let Some(se) = &series {
                // Keep series formatting by position.
                let old = std::mem::take(&mut c.series);
                c.series = se
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        let mut x = old.get(i).cloned().unwrap_or_default();
                        x.name = n.name.clone();
                        x.values = n.values.clone();
                        x
                    })
                    .collect();
            }
            c.raw = None;
            Ok(Value::Null)
        }
        "chart.options" => {
            // Histogram bins: a count or a width (setting one clears the other; null for automatic).
            let bin_count = p.get("binCount").map(|v| v.as_u64().map(|n| n.clamp(1, deckcraft_model::chart::MAX_BINS as u64) as u32));
            let bin_size = p.get("binSize").map(|v| v.as_f64().filter(|w| w.is_finite() && *w > 0.0));
            if p.get("binSize").is_some_and(|v| !v.is_null()) && bin_size.flatten().is_none() {
                return Err(bad("chart.options", "binSize must be a positive number"));
            }
            // Waterfall total points.
            let subtotals: Option<Vec<usize>> =
                p.get("subtotals").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect());
            // Colour of the chart's text (title, axes, legend, data labels); null goes back to
            // PowerPoint's default (tx1 at lumMod 65 % / lumOff 35 %).
            let text_color = match p.get("textColor") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(v) => Some(Some(
                    color_value(v).ok_or_else(|| bad("chart.options", "`textColor` is a colour: #RRGGBB, accent1…accent6, tx1…, or null"))?,
                )),
            };
            if let Some(t) = &text_color {
                c.text_color = t.clone();
                // A legend entry's own colour would otherwise keep overriding the chart-wide one.
                c.legend_entries.iter_mut().for_each(|e| e.color = None);
            }
            apply_bin_options(c, bin_count, bin_size);
            apply_subtotals_and_bars(c, p, subtotals.as_deref());
            if bin_count.is_some() || bin_size.is_some() || subtotals.is_some() {
                c.raw = None;
            }
            if let Some(t) = p.get("title") {
                c.title = t.as_str().map(String::from);
            }
            if let Some(l) = p.get("legend") {
                c.legend = l.as_str().map(String::from);
            }
            if let Some(v) = bool_param(p, "dataLabels") {
                c.data_labels = v;
                // A series' own setting would otherwise keep overriding the chart-wide switch.
                for series in &mut c.series {
                    series.data_labels = None;
                }
            }
            if let Some(v) = bool_param(p, "gridlines") {
                c.gridlines = v;
            }
            if let Some(v) = str_param(p, "palette") {
                c.palette = v.to_string();
            }
            c.raw = None;
            Ok(Value::Null)
        }
        other => Err(bad(other, "unknown chart plan")),
    }
}

pub(super) fn chart(s: &mut Session, p: &Value) -> Result<Value> {
    // `chart` carries the whole value of a collaborative insertion: the author's normaliser built
    // the chart its command would leave and every replica must create exactly that chart. Without
    // it the ordinary single-user path builds the sample and applies the typed data.
    let c = match p.get("chart") {
        Some(value) => verified_chart(value, "insert.chart")?,
        None => {
            let mut c = Chart::default();
            plan("insert.chart", p, &mut c)?;
            c
        }
    };
    let size = s.doc()?.doc.slide_size;
    let rect = rect_param(p, "rect").unwrap_or(Xfrm::new(size.width * 0.15, size.height * 0.15, size.width * 0.7, size.height * 0.7));
    let id = add_shape(s, "insert.chart", Shape { xfrm: Some(rect), kind: ShapeKind::Chart(Box::new(c)), ..Default::default() }, true)?;
    Ok(json!({"id": id}))
}

#[allow(dead_code)]
fn _kinds() -> RunKind {
    RunKind::Text
}

fn with_chart(s: &mut Session, p: &Value, cmd: &str, f: impl Fn(&mut Chart) -> Result<()>) -> Result<Value> {
    edit_shapes(s, p, cmd, |sh| match &mut sh.kind {
        ShapeKind::Chart(c) => f(c),
        _ => Err(bad(cmd, "not a chart")),
    })
}

pub(super) fn chart_type_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    with_chart(s, p, "chart.type", |c| plan("chart.type", p, c).map(|_| ()))
}

pub(super) fn chart_data(s: &mut Session, p: &Value) -> Result<Value> {
    with_chart(s, p, "chart.data", |c| plan("chart.data", p, c).map(|_| ()))
}

fn apply_bin_options(c: &mut deckcraft_model::chart::Chart, bin_count: Option<Option<u32>>, bin_size: Option<Option<f64>>) {
    if let Some(n) = bin_count {
        c.bin_count = n;
        if n.is_some() {
            c.bin_size = None;
        }
    }
    if let Some(w) = bin_size {
        c.bin_size = w;
        if w.is_some() {
            c.bin_count = None;
        }
    }
}

fn apply_subtotals_and_bars(c: &mut deckcraft_model::chart::Chart, p: &Value, subtotals: Option<&[usize]>) {
    if let Some(t) = subtotals {
        let n = c.series.first().map_or(0, |s| s.values.len());
        let mut t: Vec<usize> = t.iter().copied().filter(|i| *i < n).collect();
        t.sort_unstable();
        t.dedup();
        c.subtotals = t;
    }
    if let Some(b) = p.get("upDownBars").and_then(Value::as_bool) {
        c.up_down_bars = b;
        c.raw = None;
    }
    if let Some(q) = p.get("quartiles").and_then(Value::as_str) {
        c.inclusive_quartiles = q.eq_ignore_ascii_case("inclusive");
        c.raw = None;
    }
}

pub(super) fn chart_options(s: &mut Session, p: &Value) -> Result<Value> {
    with_chart(s, p, "chart.options", |c| plan("chart.options", p, c).map(|_| ()))
}

/// Longest whole chart value the confirmed commands may carry, as JSON bytes. A chart part a file
/// kept (an embedded workbook or a drawing) travels inside the value, so a chart whose kept parts
/// do not fit is refused before the first write instead of splitting an operation.
pub const MAX_CHART_JSON: usize = 192 * 1024;
/// Most categories, series and points one whole chart value may carry.
const MAX_CATEGORIES: usize = 4096;
const MAX_SERIES: usize = 4096;
const MAX_POINTS: usize = 100_000;
/// Most text (category labels, series names and chart captions) one chart value may carry.
const MAX_CHART_TEXT: usize = 64 * 1024;

/// Replace one chart's whole value (`chart.setWhole`).
///
/// This is the confirmed command of every `chart.type`, `chart.data` and `chart.options` edit and
/// the exact value a chart step records (ADR 0010): the normaliser sends the whole `Chart` the
/// author's command produced, so every replica ends with the same type, data and options instead
/// of merging arguments against its own copy. The value is still hostile input: it is
/// size-bounded, deserialised into the model type and checked before the first write.
pub(super) fn set_whole(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "chart.setWhole";
    let value = p.get("chart").ok_or_else(|| bad(CMD, "missing `chart`"))?;
    // The shape is checked before the value is written: a command that names a shape which is not
    // a chart must not replace anything, whatever the value says.
    let chart = verified_chart(value, CMD)?;
    edit_shapes(s, p, CMD, |sh| match &mut sh.kind {
        ShapeKind::Chart(c) => {
            **c = chart.clone();
            Ok(())
        }
        _ => Err(bad(CMD, "not a chart")),
    })
}

/// One whole chart value from the wire: size-bounded, deserialised and checked. Used by
/// `insert.chart` (the author's chart, recreated identically on every replica) and by
/// `chart.setWhole`; both must refuse the same hostile values.
fn verified_chart(value: &Value, cmd: &str) -> Result<Chart> {
    let bytes = serde_json::to_vec(value).map_err(|e| bad(cmd, format!("invalid chart: {e}")))?;
    if bytes.len() > MAX_CHART_JSON {
        return Err(bad(cmd, format!("the chart value is larger than {MAX_CHART_JSON} bytes")));
    }
    let c: Chart = serde_json::from_value(value.clone()).map_err(|e| bad(cmd, format!("invalid chart: {e}")))?;
    check_chart(&c, cmd)?;
    Ok(c)
}

/// Bounds a hostile whole chart value: the model deserialiser already rejects an unknown chart
/// type and a missing required shape, this caps what would otherwise be an unbounded allocation
/// or a chart no renderer can lay out.
fn check_chart(c: &Chart, cmd: &str) -> Result<()> {
    if c.categories.len() > MAX_CATEGORIES {
        return Err(bad(cmd, format!("at most {MAX_CATEGORIES} categories")));
    }
    if c.category_levels.len() > deckcraft_model::chart::MAX_LEVELS {
        return Err(bad(cmd, format!("at most {} category levels", deckcraft_model::chart::MAX_LEVELS)));
    }
    if c.series.len() > MAX_SERIES {
        return Err(bad(cmd, format!("at most {MAX_SERIES} series")));
    }
    let mut text = 0usize;
    for label in c.categories.iter().chain(c.category_levels.iter().flatten()) {
        text += label.len();
    }
    for series in &c.series {
        if series.values.len() > MAX_POINTS || series.x_values.len() > MAX_POINTS || series.sizes.len() > MAX_POINTS {
            return Err(bad(cmd, format!("at most {MAX_POINTS} points in a series")));
        }
        text += series.name.len();
        if series.marker.as_ref().is_some_and(|m| m.len() > 64) {
            return Err(bad(cmd, "a series marker name is too long"));
        }
        if series.number_format.as_ref().is_some_and(|f| f.len() > 1024) {
            return Err(bad(cmd, "a number format is too long"));
        }
    }
    if text > MAX_CHART_TEXT {
        return Err(bad(cmd, "the chart text is too large"));
    }
    let captions = [
        c.title.as_deref(),
        c.legend.as_deref(),
        Some(c.palette.as_str()),
        c.axis_titles.0.as_deref(),
        c.axis_titles.1.as_deref(),
        c.lang.as_deref(),
        c.y_format.as_deref(),
        c.y2_format.as_deref(),
    ];
    if captions.into_iter().flatten().any(|s| s.len() > MAX_CHART_TEXT) {
        return Err(bad(cmd, "a chart caption is too long"));
    }
    Ok(())
}
