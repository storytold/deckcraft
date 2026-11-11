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

pub(super) fn chart(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = chart_type(str_param(p, "type").unwrap_or("column"));
    let mut c = Chart::sample(kind);
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
    let k = chart_type(str_param(p, "type").unwrap_or("column"));
    with_chart(s, p, "chart.type", |c| {
        c.kind = k;
        c.vary_colors =
            matches!(k, ChartType::Pie | ChartType::Doughnut | ChartType::Funnel | ChartType::Treemap | ChartType::Sunburst | ChartType::Waterfall)
                || c.series.len() == 1 && c.vary_colors;
        if matches!(k, ChartType::Waterfall | ChartType::Histogram | ChartType::Pareto | ChartType::BoxWhisker) {
            c.gridlines = true;
        }
        c.raw = None;
        Ok(())
    })
}

pub(super) fn chart_data(s: &mut Session, p: &Value) -> Result<Value> {
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
                l.as_array().map(|v| v.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect()).unwrap_or_default()
            })
            .collect()
    });
    with_chart(s, p, "chart.data", |c| {
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
            if c.auto_title == Some(true) {
                c.title = if c.series.len() == 1 { c.series.first().map(|s| s.name.clone()).filter(|n| !n.is_empty()) } else { None };
            }
        }
        c.raw = None;
        Ok(())
    })
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
    // Histogram bins: a count or a width (setting one clears the other; null for automatic).
    let bin_count = p.get("binCount").map(|v| v.as_u64().map(|n| n.clamp(1, deckcraft_model::chart::MAX_BINS as u64) as u32));
    let bin_size = p.get("binSize").map(|v| v.as_f64().filter(|w| w.is_finite() && *w > 0.0));
    if p.get("binSize").is_some_and(|v| !v.is_null()) && bin_size.flatten().is_none() {
        return Err(bad("chart.options", "binSize must be a positive number"));
    }
    // Waterfall total points.
    let subtotals: Option<Vec<usize>> =
        p.get("subtotals").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect());
    // Colour of the chart's text (title, axes, legend, data labels); null goes back to PowerPoint's
    // default (tx1 at lumMod 65 % / lumOff 35 %).
    let text_color = match p.get("textColor") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => {
            Some(Some(color_value(v).ok_or_else(|| bad("chart.options", "`textColor` is a colour: #RRGGBB, accent1…accent6, tx1…, or null"))?))
        }
    };
    with_chart(s, p, "chart.options", |c| {
        if let Some(t) = &text_color {
            c.text_color = t.clone();
        }
        apply_bin_options(c, bin_count, bin_size);
        apply_subtotals_and_bars(c, p, subtotals.as_deref());
        if bin_count.is_some() || bin_size.is_some() || subtotals.is_some() {
            c.raw = None;
        }
        if let Some(t) = p.get("title") {
            c.title = t.as_str().map(String::from);
            c.auto_title = Some(false);
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
        Ok(())
    })
}
