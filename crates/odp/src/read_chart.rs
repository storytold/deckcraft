//! Editable ODF charts backed by an embedded local data table.
use super::*;
use deckcraft_model::chart::{ChartType, Series};

#[derive(Clone, Default)]
struct Value {
    label: String,
    number: Option<f64>,
}

fn repeat(el: &El, key: &str) -> Result<usize> {
    let n = el.attr(key).unwrap_or("1").parse::<usize>().map_err(|_| OdpError("invalid chart data repeat".into()))?;
    if !(1..=1000).contains(&n) {
        return Err(OdpError("chart data repeat exceeds limit".into()));
    }
    Ok(n)
}

fn cell(s: &str) -> Option<(usize, usize)> {
    let s = s.rsplit('.').next()?.trim_matches('\'').replace('$', "");
    let mut col = 0usize;
    let mut digits = String::new();
    let mut numeric = false;
    for ch in s.chars() {
        if ch.is_ascii_alphabetic() && !numeric {
            col = col.checked_mul(26)?.checked_add((ch.to_ascii_uppercase() as u8 - b'A' + 1) as usize)?;
        } else if ch.is_ascii_digit() {
            numeric = true;
            digits.push(ch);
        } else {
            return None;
        }
    }
    let row = digits.parse::<usize>().ok()?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}

fn address_parts(value: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (offset, ch) in value.char_indices() {
        if ch == '\'' {
            quoted = !quoted;
        }
        if !quoted && if separator == ' ' { ch.is_ascii_whitespace() } else { ch == separator } {
            if let Some(part) = value.get(start..offset).filter(|s| !s.is_empty()) {
                parts.push(part);
            }
            start = offset + ch.len_utf8();
        }
    }
    if let Some(part) = value.get(start..).filter(|s| !s.is_empty()) {
        parts.push(part);
    }
    parts
}
fn chart_cell(value: &str, table_name: &str) -> Result<(usize, usize)> {
    if let Some((table, _)) = value.rsplit_once('.') {
        let name = table.trim_start_matches('$').trim_matches('\'').replace("''", "'");
        if !name.is_empty() && name != table_name {
            return Err(OdpError("chart range references a non-local data table".into()));
        }
    }
    cell(value).ok_or_else(|| OdpError("invalid chart cell range".into()))
}
fn range<'a>(address: &str, grid: &'a [Vec<Value>], table_name: &str) -> Result<Vec<&'a Value>> {
    static EMPTY: Value = Value { label: String::new(), number: None };
    let mut out = Vec::new();
    for address in address_parts(address, ' ') {
        let endpoints = address_parts(address, ':');
        let (a, b) = match endpoints.as_slice() {
            [a] => (*a, *a),
            [a, b] => (*a, *b),
            _ => return Err(OdpError("invalid chart cell range".into())),
        };
        let (r0, c0) = chart_cell(a, table_name)?;
        let (r1, c1) = chart_cell(b, table_name)?;
        let count = r1.saturating_sub(r0).saturating_add(1).saturating_mul(c1.saturating_sub(c0).saturating_add(1));
        if r1 < r0 || c1 < c0 || r1 >= 1000 || c1 >= 1000 || out.len().saturating_add(count) > 20_000 {
            return Err(OdpError("chart range exceeds limit".into()));
        }
        for r in r0..=r1 {
            for c in c0..=c1 {
                out.push(grid.get(r).and_then(|row| row.get(c)).unwrap_or(&EMPTY));
            }
        }
    }
    Ok(out)
}
fn data_members<'a>(el: &'a El, name: &str, out: &mut Vec<&'a El>) {
    for child in el.elements() {
        if child.name == name {
            out.push(child);
        } else if matches!(child.name.as_str(), "table:table-header-rows" | "table:table-rows" | "table:table-row-group") {
            data_members(child, name, out);
        }
    }
}

fn kind(name: &str) -> Option<ChartType> {
    Some(match name {
        "bar" => ChartType::Column,
        "line" => ChartType::Line,
        "area" => ChartType::Area,
        "circle" => ChartType::Pie,
        "ring" => ChartType::Doughnut,
        "scatter" => ChartType::Scatter,
        "bubble" => ChartType::Bubble,
        "radar" => ChartType::Radar,
        "filled-radar" => ChartType::FilledRadar,
        "stock" => ChartType::Stock,
        "surface" => ChartType::Surface,
        _ => return None,
    })
}

fn chart_kind(el: &El) -> Option<ChartType> {
    let (uri, local) = el.resolve_qname(el.attr("chart:class")?)?;
    if uri != Some("urn:oasis:names:tc:opendocument:xmlns:chart:1.0") {
        return None;
    }
    kind(local)
}

pub(super) fn read(doc: &El, parent_styles: &Styles<'_>, total: &mut u64) -> Result<Option<Chart>> {
    let mut elements = Vec::new();
    doc.descendants("chart:chart", &mut elements);
    let Some(el) = elements.first().copied() else { return Ok(None) };
    let Some(base_kind) = chart_kind(el) else { return Ok(None) };
    let mut styles = parent_styles.clone();
    resources::register(doc, &mut styles);
    let mut definitions = Vec::new();
    doc.descendants("style:style", &mut definitions);
    for e in definitions {
        styles.insert((e.attr("style:family").unwrap_or("").into(), e.attr("style:name").unwrap_or("").into()), e);
    }
    let formats = number_formats(doc);
    let mut sampled = 0usize;
    let mut chart = Chart { kind: base_kind, gridlines: false, minor_gridlines: false, ..Default::default() };
    chart.title = el.child("chart:title").map(extras::plain);
    chart.legend = el.child("chart:legend").map(|e| {
        match e.attr("chart:legend-position").unwrap_or("end") {
            "start" => "l",
            "top" => "t",
            "bottom" => "b",
            "top-end" => "tr",
            _ => "r",
        }
        .into()
    });
    let props = properties(&styles, "chart", el.attr("chart:style-name"))?;
    chart.chart_fill = super::fill(&props)?;
    chart.text_size = props.get("fo:font-size").and_then(|v| length(v)).filter(|v| *v > 0.0);
    chart.text_color = props.get("fo:color").and_then(|v| color(v));
    if let Some(title) = el.child("chart:title") {
        let title_props = properties(&styles, "chart", title.attr("chart:style-name"))?;
        chart.title_size = title_props.get("fo:font-size").and_then(|v| length(v)).filter(|v| *v > 0.0);
    }
    let mut grid = Vec::new();
    let mut cells = 0usize;
    let mut expanded = 0usize;
    let mut tables = Vec::new();
    el.descendants("table:table", &mut tables);
    let table_name = tables.first().and_then(|e| e.attr("table:name")).unwrap_or("local-table");
    if let Some(table) = tables.first() {
        let mut rows = Vec::new();
        data_members(table, "table:table-row", &mut rows);
        for row in rows {
            let nr = repeat(row, "table:number-rows-repeated")?;
            if grid.len() + nr > 1000 {
                return Err(OdpError("too many chart data rows".into()));
            }
            let mut out = Vec::new();
            for e in row.elements().filter(|e| matches!(e.name.as_str(), "table:table-cell" | "table:covered-table-cell")) {
                let nc = repeat(e, "table:number-columns-repeated")?;
                cells = cells.saturating_add(nc.saturating_mul(nr));
                if cells > 20_000 || out.len() + nc > 1000 {
                    return Err(OdpError("too many chart data cells".into()));
                }
                let value = Value { label: extras::plain(e), number: e.attr("office:value").and_then(crate::geometry::number) };
                expanded = expanded.saturating_add(value.label.len().saturating_mul(nc).saturating_mul(nr));
                if expanded > 2 * 1024 * 1024 {
                    return Err(OdpError("expanded chart text exceeds size limit".into()));
                }
                out.extend(std::iter::repeat_n(value, nc));
            }
            grid.extend(std::iter::repeat_n(out, nr));
        }
    }
    let Some(plot) = el.child("chart:plot-area") else { return Ok(Some(chart)) };
    let props = properties(&styles, "chart", plot.attr("chart:style-name"))?;
    chart.plot_fill = super::fill(&props)?;
    chart.gap_width =
        props.get("chart:gap-width").and_then(|v| crate::geometry::number(v)).map(|v| (v / 100.0).clamp(0.0, 10.0)).unwrap_or(chart.gap_width);
    chart.overlap =
        props.get("chart:overlap").and_then(|v| crate::geometry::number(v)).map(|v| (v / 100.0).clamp(-1.0, 1.0)).unwrap_or(chart.overlap);
    chart.hole_size = props
        .get("chart:hole-size")
        .and_then(|v| v.strip_suffix('%'))
        .and_then(crate::geometry::number)
        .map(|v| (v / 100.0).clamp(0.0, 1.0))
        .unwrap_or(chart.hole_size);
    chart.data_labels =
        props.get("chart:data-label-number").is_some_and(|v| v != "none") || props.get("chart:data-label-text").is_some_and(|v| v == "true");
    let plot_smooth = props.get("chart:interpolation").is_some_and(|v| matches!(v.as_str(), "cubic-spline" | "b-spline"));
    let horizontal = props.get("chart:vertical").is_some_and(|s| s == "true");
    let stacked = props.get("chart:stacked").is_some_and(|s| s == "true");
    let percent = props.get("chart:percentage").is_some_and(|s| s == "true");
    chart.kind = match (base_kind, horizontal, stacked, percent) {
        (ChartType::Column, true, _, true) => ChartType::PercentBar,
        (ChartType::Column, false, _, true) => ChartType::PercentColumn,
        (ChartType::Column, true, true, _) => ChartType::StackedBar,
        (ChartType::Column, true, false, _) => ChartType::Bar,
        (ChartType::Column, false, true, _) => ChartType::StackedColumn,
        (ChartType::Line, _, true, _) => ChartType::StackedLine,
        (ChartType::Area, _, true, _) => ChartType::StackedArea,
        _ => base_kind,
    };
    let mut categories = Vec::new();
    plot.descendants("chart:categories", &mut categories);
    if let Some(address) = categories.first().and_then(|e| e.attr("table:cell-range-address")) {
        let values = range(address, &grid, table_name)?;
        charge(&values, &mut sampled, total, true)?;
        chart.categories = values.iter().map(|v| v.label.clone()).collect();
    }
    let mut value_axes = Vec::new();
    for axis in plot.elements().filter(|e| e.name == "chart:axis") {
        let properties = properties(&styles, "chart", axis.attr("chart:style-name"))?;
        match axis.attr("chart:dimension") {
            Some("x") => {
                chart.axis_titles.0 = axis.child("chart:title").map(extras::plain);
                chart.cat_reversed = properties.get("chart:reverse-direction").is_some_and(|v| v == "true");
            }
            Some("y") => {
                let primary = value_axes.is_empty();
                let min = number(&properties, "chart:minimum");
                let max = number(&properties, "chart:maximum");
                let major = number(&properties, "chart:interval-major").filter(|v| *v > 0.0);
                let hidden = properties.get("chart:display-label").is_some_and(|v| v == "false");
                if primary {
                    if let Some(value) = number(&properties, "chart:gap-width") {
                        chart.gap_width = (value / 100.0).clamp(0.0, 10.0);
                    }
                    if let Some(value) = number(&properties, "chart:overlap") {
                        chart.overlap = (value / 100.0).clamp(-1.0, 1.0);
                    }
                    chart.y_min = min;
                    chart.y_max = max;
                    chart.y_major = major;
                    chart.y_hidden = hidden;
                    chart.y_format = axis.attr("chart:style-name").and_then(|name| style_format(&styles, name, &formats));
                    chart.y_log_base = properties.get("chart:logarithmic").filter(|v| *v == "true").map(|_| 10.0);
                    chart.axis_titles.1 = axis.child("chart:title").map(extras::plain);
                    chart.gridlines = axis.elements().any(|e| e.name == "chart:grid" && e.attr("chart:class") != Some("minor"));
                    chart.minor_gridlines = axis.elements().any(|e| e.name == "chart:grid" && e.attr("chart:class") == Some("minor"));
                } else {
                    chart.y2_min = min;
                    chart.y2_max = max;
                    chart.y2_major = major;
                    chart.y2_hidden = hidden;
                    chart.y2_format = axis.attr("chart:style-name").and_then(|name| style_format(&styles, name, &formats));
                }
                if value_axes.len() >= 2 {
                    return Err(OdpError("more than two chart value axes are not supported".into()));
                }
                value_axes.push(axis.attr("chart:name").unwrap_or("").to_string());
            }
            _ => {}
        }
    }
    let mut previous_domains: Vec<(ChartType, Vec<Vec<Option<f64>>>)> = Vec::new();
    let mut series = Vec::new();
    plot.descendants("chart:series", &mut series);
    if series.len() > 1000 {
        return Err(OdpError("too many chart series".into()));
    }
    for (i, e) in series.into_iter().enumerate() {
        let props = properties(&styles, "chart", e.attr("chart:style-name"))?;
        let mut out = Series { name: format!("Series {}", i + 1), fill: super::fill(&props)?, ..Default::default() };
        if let Some(address) = e.attr("chart:label-cell-address")
            && let Some(value) = range(address, &grid, table_name)?.first()
        {
            charge(&[value], &mut sampled, total, true)?;
            out.name = value.label.clone();
        }
        if let Some(address) = e.attr("chart:values-cell-range-address") {
            let values = range(address, &grid, table_name)?;
            charge(&values, &mut sampled, total, false)?;
            out.values = values.iter().map(|v| v.number).collect();
        }
        let series_kind = chart_kind(e).unwrap_or(base_kind);
        if series_kind != base_kind {
            out.kind = Some(series_kind);
        }
        out.secondary_axis = e.attr("chart:attached-axis").is_some_and(|name| value_axes.get(1).is_some_and(|axis| axis == name));
        out.line = series_line(&props);
        out.number_format = e.attr("chart:style-name").and_then(|name| style_format(&styles, name, &formats));
        out.data_labels =
            props.get("chart:data-label-number").map(|v| v != "none").or_else(|| props.get("chart:data-label-text").map(|v| v == "true"));
        out.smooth = props.get("chart:interpolation").map(|v| matches!(v.as_str(), "cubic-spline" | "b-spline")).unwrap_or(plot_smooth);
        out.marker = props.get("chart:symbol-type").and_then(|v| match v.as_str() {
            "none" => Some("none".into()),
            "named-symbol" => props.get("chart:symbol-name").map(|s| {
                match s.as_str() {
                    "plus" => "plus",
                    "x" => "x",
                    "diamond" => "diamond",
                    "square" => "square",
                    "star" => "star",
                    _ => "circle",
                }
                .into()
            }),
            "automatic" => Some("circle".into()),
            _ => None,
        });
        let mut domains = Vec::new();
        for domain in e.elements().filter(|e| e.name == "chart:domain") {
            if domains.len() >= 2 {
                return Err(OdpError("too many chart domains".into()));
            }
            if let Some(address) = domain.attr("table:cell-range-address") {
                let values = range(address, &grid, table_name)?;
                charge(&values, &mut sampled, total, false)?;
                domains.push(values.iter().map(|v| v.number).collect::<Vec<_>>());
            }
        }
        if domains.is_empty() {
            if let Some((_, previous)) = previous_domains.iter().find(|(kind, _)| *kind == series_kind) {
                for values in previous {
                    charge_numbers(values.len(), &mut sampled, total)?;
                    domains.push(values.clone());
                }
            }
        } else {
            let count = domains.iter().map(Vec::len).sum::<usize>();
            // Cache domain templates only once per supported class; the output clones above
            // are charged separately to the shared sampled-value budget.
            if count > 40_000 {
                return Err(OdpError("chart domain cache exceeds limit".into()));
            }
            if let Some((_, previous)) = previous_domains.iter_mut().find(|(kind, _)| *kind == series_kind) {
                *previous = domains.clone();
            } else {
                previous_domains.push((series_kind, domains.clone()));
            }
        }
        match series_kind {
            ChartType::Bubble => {
                out.sizes = std::mem::take(&mut out.values);
                if let Some(values) = domains.first() {
                    out.values = values.clone();
                }
                if let Some(values) = domains.get(1) {
                    out.x_values = values.clone();
                }
            }
            ChartType::Scatter => {
                if let Some(values) = domains.first() {
                    out.x_values = values.clone();
                }
            }
            _ => {}
        }
        let mut point = 0usize;
        for item in e.elements().filter(|e| e.name == "chart:data-point") {
            let repeats = item.attr("chart:repeated").unwrap_or("1").parse::<usize>().map_err(|_| OdpError("invalid chart point repeat".into()))?;
            if repeats == 0 || point.saturating_add(repeats) > 20_000 {
                return Err(OdpError("chart point repeat exceeds limit".into()));
            }
            let style = properties(&styles, "chart", item.attr("chart:style-name"))?;
            if let Some(fill) = super::fill(&style)? {
                *total = total.saturating_add(repeats.saturating_mul(std::mem::size_of::<usize>().saturating_add(fill_weight(&fill))) as u64);
                if *total > crate::MAX_TOTAL {
                    return Err(OdpError("chart point styles exceed total size limit".into()));
                }
                for index in point..point + repeats {
                    out.point_fills.push((index, fill.clone()));
                }
            }
            point += repeats;
        }
        for curve in e.elements().filter(|e| e.name == "chart:regression-curve") {
            if out.trendlines.len() >= 16 {
                return Err(OdpError("too many chart regression curves".into()));
            }
            let properties = properties(&styles, "chart", curve.attr("chart:style-name"))?;
            if let Some(trend) = trendline(curve, &properties) {
                out.trendlines.push(trend);
            }
        }
        for indicator in e.elements().filter(|e| e.name == "chart:error-indicator" && e.attr("chart:dimension") != Some("x")) {
            let properties = properties(&styles, "chart", indicator.attr("chart:style-name"))?;
            out.error_bars = error_bars(&properties, &grid, table_name, out.values.len(), &mut sampled, total)?;
        }
        let style_bytes = std::mem::size_of::<Series>()
            .saturating_add(out.name.len())
            .saturating_add(out.fill.as_ref().map(fill_weight).unwrap_or(0))
            .saturating_add(
                out.trendlines
                    .iter()
                    .map(|t| std::mem::size_of::<deckcraft_model::chart::Trendline>().saturating_add(t.name.as_ref().map(String::len).unwrap_or(0)))
                    .sum::<usize>(),
            );
        *total = total.saturating_add(style_bytes as u64);
        if *total > crate::MAX_TOTAL {
            return Err(OdpError("expanded chart style content exceeds size limit".into()));
        }
        chart.series.push(out);
    }
    // A common minimal chart uses the first column for categories and first row for labels.
    if chart.series.is_empty()
        && let Some(first) = grid.first()
    {
        for (c, label) in first.iter().enumerate().skip(1) {
            let values: Vec<_> = grid.iter().skip(1).filter_map(|row| row.get(c)).collect();
            charge(&values, &mut sampled, total, false)?;
            charge(&[label], &mut sampled, total, true)?;
            chart.series.push(Series {
                name: label.label.clone(),
                values: grid.iter().skip(1).map(|row| row.get(c).and_then(|v| v.number)).collect(),
                ..Default::default()
            });
        }
    }
    if chart.categories.is_empty() && categories.is_empty() && matches!(plot.attr("chart:data-source-has-labels"), None | Some("column" | "both")) {
        let values: Vec<_> = grid.iter().skip(1).filter_map(|row| row.first()).collect();
        charge(&values, &mut sampled, total, true)?;
        chart.categories = values.iter().map(|v| v.label.clone()).collect();
    }
    // Rendering dispatches per-series kinds and secondary axes only for combo charts.
    if value_axes.len() > 1 || chart.series.iter().any(|series| series.kind.is_some() || series.secondary_axis) {
        for series in &mut chart.series {
            series.kind.get_or_insert(chart.kind);
        }
        chart.kind = ChartType::Combo;
    }
    let fill_bytes = chart.chart_fill.as_ref().map(fill_weight).unwrap_or(0).saturating_add(chart.plot_fill.as_ref().map(fill_weight).unwrap_or(0));
    *total = total.saturating_add(fill_bytes as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("expanded chart fills exceed size limit".into()));
    }
    Ok(Some(chart))
}

fn charge(values: &[&Value], count: &mut usize, total: &mut u64, text: bool) -> Result<()> {
    *count = count.saturating_add(values.len());
    if *count > 100_000 {
        return Err(OdpError("chart sampled value count exceeds limit".into()));
    }
    let bytes = values
        .iter()
        .fold(0usize, |sum, v| sum.saturating_add(if text { v.label.len().saturating_add(32) } else { std::mem::size_of::<Option<f64>>() }));
    *total = total.saturating_add(bytes as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("expanded chart content exceeds total size limit".into()));
    }
    Ok(())
}

fn number(props: &Props, key: &str) -> Option<f64> {
    props.get(key).and_then(|v| crate::geometry::number(v))
}
fn charge_numbers(count: usize, sampled: &mut usize, total: &mut u64) -> Result<()> {
    *sampled = sampled.saturating_add(count);
    *total = total.saturating_add(count.saturating_mul(std::mem::size_of::<Option<f64>>()) as u64);
    if *sampled > 100_000 || *total > crate::MAX_TOTAL {
        return Err(OdpError("expanded chart sampled values exceed limit".into()));
    }
    Ok(())
}
fn series_line(props: &Props) -> Option<Line> {
    props.get("draw:stroke")?;
    if props.get("draw:stroke").is_some_and(|v| v == "none") {
        return Some(Line::none());
    }
    Some(Line::solid(
        props.get("svg:stroke-color").and_then(|s| color(s)).unwrap_or_else(|| ColorRef::rgb(Rgba::BLACK)),
        props.get("svg:stroke-width").and_then(|s| length(s)).unwrap_or(1.0),
    ))
}
fn trendline(el: &El, props: &Props) -> Option<deckcraft_model::chart::Trendline> {
    use deckcraft_model::chart::{Trendline, TrendlineType};
    let kind = match props.get("chart:regression-type")?.as_str() {
        "linear" => TrendlineType::Linear,
        "logarithmic" => TrendlineType::Log,
        "exponential" => TrendlineType::Exp,
        "power" => TrendlineType::Power,
        "polynomial" => TrendlineType::Poly,
        "moving-average" => TrendlineType::MovingAvg,
        _ => return None,
    };
    let equation = el.child("chart:equation");
    Some(Trendline {
        kind,
        name: props.get("chart:regression-name").cloned(),
        order: props.get("chart:regression-max-degree").and_then(|v| v.parse::<u32>().ok()).map(|v| v.clamp(2, 6)),
        period: props.get("chart:regression-period").and_then(|v| v.parse::<u32>().ok()).map(|v| v.clamp(2, 1000)),
        intercept: props
            .get("chart:regression-force-intercept")
            .filter(|v| *v == "true")
            .and_then(|_| number(props, "chart:regression-intercept-value")),
        disp_eq: equation.is_some_and(|e| e.attr("chart:display-equation") == Some("true")),
        disp_r_sqr: equation.is_some_and(|e| e.attr("chart:display-r-square") == Some("true")),
        ..Default::default()
    })
}
fn error_bars(
    props: &Props,
    grid: &[Vec<Value>],
    table: &str,
    points: usize,
    sampled: &mut usize,
    total: &mut u64,
) -> Result<Option<deckcraft_model::chart::ErrorBars>> {
    use deckcraft_model::chart::ErrorBars;
    let Some(kind) = props.get("chart:error-category") else { return Ok(None) };
    let lower = props.get("chart:error-lower-indicator").is_none_or(|v| v == "true");
    let upper = props.get("chart:error-upper-indicator").is_none_or(|v| v == "true");
    if !lower && !upper {
        return Ok(None);
    }
    let mut bars = ErrorBars {
        bar_type: match (lower, upper) {
            (true, true) => "both",
            (true, false) => "minus",
            _ => "plus",
        }
        .into(),
        ..Default::default()
    };
    match kind.as_str() {
        "constant" => {
            let low = number(props, "chart:error-lower-limit").unwrap_or(0.0);
            let high = number(props, "chart:error-upper-limit").unwrap_or(low);
            if (low - high).abs() > 1e-9 {
                bars.val_type = "cust".into();
                let count = points;
                charge_numbers(count.saturating_mul(2), sampled, total)?;
                bars.minus = vec![Some(low); count];
                bars.plus = vec![Some(high); count];
            } else {
                bars.value = low;
            }
        }
        "percentage" => {
            bars.val_type = "percentage".into();
            bars.value = number(props, "chart:error-percentage").unwrap_or(0.0);
        }
        "standard-deviation" => {
            bars.val_type = "stdDev".into();
            bars.value = 1.0;
        }
        "standard-error" => {
            bars.val_type = "stdErr".into();
            bars.value = 1.0;
        }
        // ODF error margins depend on the largest series value. A fixed amount or
        // per-point percentage would change their meaning when users edit data.
        // The warning pass retains these through source recovery and guards export.
        "error-margin" => return Ok(None),
        "cell-range" => {
            bars.val_type = "cust".into();
            if let Some(address) = props.get("chart:error-lower-range") {
                let values = range(address, grid, table)?;
                charge(&values, sampled, total, false)?;
                bars.minus = values.iter().map(|v| v.number).collect();
            }
            if let Some(address) = props.get("chart:error-upper-range") {
                let values = range(address, grid, table)?;
                charge(&values, sampled, total, false)?;
                bars.plus = values.iter().map(|v| v.number).collect();
            }
        }
        _ => return Ok(None),
    }
    Ok(Some(bars))
}

fn fill_weight(fill: &Fill) -> usize {
    use deckcraft_model::style::GradientShape;
    let extra = match fill {
        Fill::Solid { color } => color.mods.len().saturating_mul(std::mem::size_of::<ColorTransform>()),
        Fill::Gradient(g) => g
            .stops
            .iter()
            .fold(0usize, |sum, stop| {
                sum.saturating_add(std::mem::size_of::<deckcraft_model::style::GradientStop>())
                    .saturating_add(stop.color.mods.len().saturating_mul(std::mem::size_of::<ColorTransform>()))
            })
            .saturating_add(match &g.shape {
                GradientShape::Path { path, .. } => path.len(),
                _ => 0,
            }),
        Fill::Pattern(p) => p.preset.len(),
        _ => 0,
    };
    std::mem::size_of::<Fill>().saturating_add(extra)
}
fn number_formats(doc: &El) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in ["number:number-style", "number:percentage-style", "number:currency-style"] {
        let mut elements = Vec::new();
        doc.descendants(name, &mut elements);
        for el in elements {
            let Some(name) = el.attr("style:name") else { continue };
            let mut format = String::new();
            let mut valid = true;
            for child in el.elements() {
                match child.name.as_str() {
                    "number:number" => {
                        let decimals = child.attr("number:decimal-places").unwrap_or("0").parse::<usize>().ok().filter(|n| *n <= 10);
                        let digits = child.attr("number:min-integer-digits").unwrap_or("1").parse::<usize>().ok().filter(|n| *n <= 20);
                        if let (Some(decimals), Some(digits)) = (decimals, digits) {
                            if child.attr("number:grouping") == Some("true") {
                                format.push_str("#,##");
                            }
                            format.push_str(&"0".repeat(digits.max(1)));
                            if decimals > 0 {
                                format.push('.');
                                format.push_str(&"0".repeat(decimals));
                            }
                        } else {
                            valid = false;
                        }
                    }
                    "number:text" | "number:currency-symbol" => {
                        let text = extras::plain(child);
                        if text.len() > 32 || text.contains(['"', ';', '[', ']']) {
                            valid = false;
                        } else if text.chars().all(|c| matches!(c, '%' | ' ' | ',' | '.' | '$' | '€' | '£' | '¥' | '-' | '+' | '(' | ')')) {
                            format.push_str(&text);
                        } else {
                            format.push('"');
                            format.push_str(&text);
                            format.push('"');
                        }
                    }
                    "style:text-properties" => {}
                    _ => valid = false,
                }
            }
            if valid && !format.is_empty() && out.len() < 1000 {
                out.insert(name.into(), format);
            }
        }
    }
    out
}
fn style_format(styles: &Styles<'_>, name: &str, formats: &BTreeMap<String, String>) -> Option<String> {
    let mut current = Some(name);
    for _ in 0..32 {
        let name = current?;
        let style = styles.get(&("chart".into(), name.into()))?;
        if let Some(format) = style.attr("style:data-style-name").and_then(|name| formats.get(name)) {
            return Some(format.clone());
        }
        current = style.attr("style:parent-style-name");
    }
    None
}

pub(super) fn warnings(doc: &El, parent_styles: &Styles<'_>) -> Result<Vec<&'static str>> {
    fn add(out: &mut Vec<&'static str>, text: &'static str) {
        if !out.contains(&text) {
            out.push(text);
        }
    }
    let mut styles = parent_styles.clone();
    let mut definitions = Vec::new();
    doc.descendants("style:style", &mut definitions);
    for el in definitions {
        styles.insert((el.attr("style:family").unwrap_or("").into(), el.attr("style:name").unwrap_or("").into()), el);
    }
    let formats = number_formats(doc);
    let mut out = Vec::new();
    let mut charts = Vec::new();
    doc.descendants("chart:chart", &mut charts);
    for chart in charts {
        let Some(base) = chart_kind(chart) else {
            add(&mut out, "An ODF chart class namespace or local token is not represented by the editable chart model");
            continue;
        };
        let Some(plot) = chart.child("chart:plot-area") else { continue };
        let mut previous = Vec::new();
        let mut series = Vec::new();
        plot.descendants("chart:series", &mut series);
        for series in series {
            let class = chart_kind(series).unwrap_or(base);
            let required = match class {
                ChartType::Scatter => 1,
                ChartType::Bubble => 2,
                ChartType::Surface => 2,
                _ => 0,
            };
            let domains = series.elements().filter(|e| e.name == "chart:domain").collect::<Vec<_>>();
            if domains.iter().any(|d| d.attr("table:cell-range-address").is_none())
                || !(domains.len() == required || domains.is_empty() && previous.contains(&class))
            {
                add(&mut out, "ODF chart domain cardinality is incomplete or unsupported; source recovery retains the original coordinate ranges");
            } else if domains.len() == required {
                previous.push(class);
            }
            if series.attr("chart:class").is_some() && chart_kind(series).is_none() {
                add(&mut out, "An ODF series chart class is not represented by the editable chart model");
            }
            if let Some(name) = series.attr("chart:attached-axis")
                && !plot.elements().any(|e| e.name == "chart:axis" && e.attr("chart:dimension") == Some("y") && e.attr("chart:name") == Some(name))
            {
                add(&mut out, "ODF chart series names an unresolved value axis; its attachment is retained through source recovery");
            }
        }
    }
    fn walk(el: &El, styles: &Styles<'_>, formats: &BTreeMap<String, String>, out: &mut Vec<&'static str>, y_count: &mut usize) -> Result<()> {
        if el.name == "table:table-cell" && el.attr("table:formula").is_some() {
            add(out, "ODF chart data formulas are retained in the source archive; editable series use cached values without recalculation");
        }
        if el.name == "chart:axis" && el.attr("chart:dimension") == Some("y") {
            *y_count += 1;
        }
        if el.name.starts_with("chart:") {
            let props = properties(styles, "chart", el.attr("chart:style-name"))?;
            if props.get("draw:stroke").is_some_and(|v| !matches!(v.as_str(), "none" | "solid"))
                || ["draw:stroke-dash", "svg:stroke-linecap", "draw:stroke-linejoin", "draw:marker-start", "draw:marker-end"]
                    .iter()
                    .any(|key| props.contains_key(*key))
            {
                add(out, "ODF chart custom outline geometry is retained in the source archive; the editable series exposes simple outlines");
            }
            if props.get("svg:stroke-opacity").is_some_and(|v| v != "100%") {
                add(out, "ODF translucent chart outlines are retained in the source archive; editable chart outline opacity is not mapped yet");
            }
            if props.get("chart:data-label-number").is_some_and(|v| !matches!(v.as_str(), "none" | "value"))
                || props.get("chart:data-label-text").is_some_and(|v| v == "true")
                || props.get("chart:data-label-symbol").is_some_and(|v| v == "true")
                || el.name == "chart:data-label"
            {
                add(out, "ODF category, percentage or custom chart labels have no equivalent in the editable numeric-label model");
            }
            if el.name == "chart:mean-value" || props.get("chart:mean-value").is_some_and(|v| v == "true") {
                add(out, "ODF chart mean-value lines are retained in the source archive; the editable model does not expose them");
            }
            if props.get("chart:treat-empty-cells").is_some_and(|v| v != "ignore") {
                add(out, "ODF chart gap/zero missing-value policies differ from the editable renderer's skip-and-connect policy");
            }
            if props.get("chart:visible").is_some_and(|v| v == "false") {
                add(out, "ODF chart element visibility has no editable model equivalent");
            }
            if el.name == "chart:axis" && el.attr("chart:dimension") == Some("x") && props.get("chart:display-label").is_some_and(|v| v == "false") {
                add(out, "ODF hidden category-axis labels have no editable model equivalent");
            }
            if props.get("chart:interval-minor-divisor").is_some_and(|v| v != "5") {
                add(out, "ODF chart minor-grid intervals differ from the editable renderer's five subdivisions");
            }
            let error_active = props.get("chart:error-lower-indicator").is_none_or(|v| v == "true")
                || props.get("chart:error-upper-indicator").is_none_or(|v| v == "true");
            if props.get("draw:fill").is_some_and(|value| value == "bitmap") {
                add(out, "ODF chart bitmap fills are retained in the source archive; editable chart fill resources are not loaded yet");
            }
            if props.get("chart:three-dimensional").is_some_and(|v| v == "true") || props.contains_key("chart:pie-offset") {
                add(
                    out,
                    "ODF three-dimensional or exploded chart geometry is retained in the source archive; editable chart geometry is approximated",
                );
            }
            if props.get("chart:interpolation").is_some_and(|v| v.starts_with("step-") || v == "b-spline") {
                add(out, "ODF step/B-spline chart interpolation is retained in the source archive; model interpolation differs");
            }
            if el.name == "chart:axis"
                && el.attr("chart:dimension") == Some("x")
                && ["chart:minimum", "chart:maximum", "chart:interval-major", "chart:logarithmic"].iter().any(|k| props.contains_key(*k))
            {
                add(out, "ODF numeric X-axis bounds/scales are retained in the source archive; the chart model currently exposes Y-axis settings");
            }
            if el.name == "chart:axis" && *y_count > 1 && props.get("chart:logarithmic").is_some_and(|v| v == "true") {
                add(out, "ODF secondary-axis logarithmic scaling is retained in the source archive; the model does not expose it yet");
            }
            if el.name == "chart:axis" && el.attr("chart:dimension") == Some("y") && props.get("chart:reverse-direction").is_some_and(|v| v == "true")
            {
                add(out, "ODF reversed value-axis direction is retained in the source archive; the model currently exposes category reversal only");
            }
            if error_active
                && (props.get("chart:error-category").is_some_and(|v| v == "variance")
                    || el.name == "chart:error-indicator" && el.attr("chart:dimension") == Some("x"))
            {
                add(
                    out,
                    "ODF variance or X error bars are retained in the source archive; the editable chart model currently exposes other Y error bars",
                );
            }
            if error_active && props.get("chart:error-category").is_some_and(|v| v == "error-margin") {
                add(
                    out,
                    "ODF error margins depend on the largest series value; this behavior is retained in the source archive and has no editable model equivalent",
                );
            }
            if props.get("chart:regression-moving-type").is_some_and(|v| v != "prior") {
                add(out, "ODF centered or averaged-abscissa moving regressions have no equivalent in the editable model");
            }
            if props.get("chart:regression-max-degree").and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v > 6) {
                add(out, "ODF polynomial chart orders above six are retained in the source archive; editable model order is limited to six");
            }
            if props.get("chart:symbol-type").is_some_and(|v| v == "image")
                || props.get("chart:symbol-name").is_some_and(|v| !matches!(v.as_str(), "square" | "diamond" | "circle" | "star" | "x" | "plus"))
            {
                add(out, "ODF image/custom chart markers are retained in the source archive; the closest model marker is used");
            }
            if let Some(style) = el.attr("chart:style-name").and_then(|name| styles.get(&("chart".into(), name.into())))
                && style.attr("style:data-style-name").is_some_and(|name| !formats.contains_key(name))
            {
                add(
                    out,
                    "Some ODF chart number formats are retained in the source archive; only common numeric/percentage/currency formats are editable",
                );
            }
        }
        for child in el.elements() {
            walk(child, styles, formats, out, y_count)?;
        }
        Ok(())
    }
    walk(doc, &styles, &formats, &mut out, &mut 0)?;
    Ok(out)
}
