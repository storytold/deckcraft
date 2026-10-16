//! Native subdocuments and document metadata (OASIS OpenDocument 1.3).
use super::*;

pub(super) fn metadata(p: &Properties) -> Result<String> {
    if !p.created.is_empty() {
        if p.created.len() > crate::annotation_metadata::MAX_DATE {
            return Err(OdpError("document creation-date scalar exceeds size limit".into()));
        }
        if !crate::annotation_metadata::valid_xml_text(&p.created) || !crate::annotation_metadata::valid_date(&p.created) {
            return Err(OdpError("document creation-date outside the supported XML Schema dateTime profile".into()));
        }
    }
    let mut out = format!("{}<office:meta><meta:generator>DeckCraft</meta:generator>", header("office:document-meta"));
    if !p.created.is_empty() {
        // Literal CR is normalized by XML readers; preserve the accepted lexical scalar.
        out.push_str(&format!("<meta:creation-date>{}</meta:creation-date>", esc(&p.created).replace('\r', "&#13;")));
    }
    for (key, value) in [
        ("dc:title", &p.title),
        ("dc:subject", &p.subject),
        ("meta:initial-creator", &p.author),
        ("dc:creator", &p.last_modified_by),
        ("meta:keyword", &p.keywords),
        ("dc:description", &p.comments),
    ] {
        if !value.is_empty() {
            out.push_str(&format!("<{key}>{}</{key}>", esc(value)));
        }
    }
    // Modification dates remain opaque strings until an edited-export policy exists.
    for (name, value) in [("Category", &p.category), ("Company", &p.company), ("Modified", &p.modified)] {
        if !value.is_empty() {
            out.push_str(&format!("<meta:user-defined meta:name=\"{name}\" meta:value-type=\"string\">{}</meta:user-defined>", esc(value)));
        }
    }
    out.push_str(&format!("<meta:editing-cycles>{}</meta:editing-cycles></office:meta></office:document-meta>", p.revision.max(1)));
    Ok(out)
}
fn column(mut n: usize) -> String {
    let mut chars = Vec::new();
    loop {
        chars.push(char::from(b'A' + (n % 26) as u8));
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    chars.into_iter().rev().collect()
}
fn chart_color(c: &Chart, i: usize) -> ColorRef {
    let mono = c.palette.starts_with("monochrome");
    let slot = if mono {
        SchemeSlot::Accent1
    } else {
        match i % 6 {
            0 => SchemeSlot::Accent1,
            1 => SchemeSlot::Accent2,
            2 => SchemeSlot::Accent3,
            3 => SchemeSlot::Accent4,
            4 => SchemeSlot::Accent5,
            _ => SchemeSlot::Accent6,
        }
    };
    let color = ColorRef::scheme(slot);
    let round = if mono { i } else { i / 6 };
    match round {
        0 => color,
        1 => color.with(ColorTransform::Shade(60000)),
        2 => color.with(ColorTransform::Tint(60000)),
        n => color.with(ColorTransform::LumMod(100000 - (n as i32 % 5) * 12000)),
    }
}
// Chart subdocuments deliberately use only self-contained solid paints. Resource-backed
// fills would otherwise point at definitions owned by the outer presentation document.
fn chart_fill_supported(fill: Option<&Fill>) -> Result<()> {
    if fill.is_some_and(|f| !matches!(f, Fill::Solid { .. } | Fill::None)) {
        return Err(OdpError("chart resource-backed fills cannot be exported faithfully yet".into()));
    }
    Ok(())
}
fn chart_class(kind: deckcraft_model::chart::ChartType) -> Result<&'static str> {
    use deckcraft_model::chart::ChartType as T;
    Ok(match kind {
        T::Column | T::StackedColumn | T::PercentColumn | T::Bar | T::StackedBar | T::PercentBar => "bar",
        T::Line | T::LineMarkers | T::StackedLine => "line",
        T::Area | T::StackedArea => "area",
        T::Pie => "circle",
        T::Doughnut => "ring",
        T::Scatter => "scatter",
        T::Bubble => "bubble",
        T::Radar => "radar",
        T::FilledRadar => "filled-radar",
        T::Stock => "stock",
        _ => return Err(OdpError(format!("{} charts cannot be represented faithfully in ODP yet", kind.label()))),
    })
}
struct ChartColumn {
    label: String,
    values: Vec<Option<f64>>,
}
struct ChartSeriesData {
    values: usize,
    domains: Vec<usize>,
    errors: (Option<usize>, Option<usize>),
}
fn chart_column(columns: &mut Vec<ChartColumn>, label: String, values: &[Option<f64>]) -> Result<usize> {
    let rows = columns.iter().map(|c| c.values.len()).max().unwrap_or(0).max(values.len());
    if values.len() > 999 || columns.len() >= 1000 || rows.saturating_add(1).saturating_mul(columns.len().saturating_add(1)) > 20_000 {
        return Err(OdpError("chart exceeds editable table export limits".into()));
    }
    if values.iter().flatten().any(|v| !v.is_finite()) {
        return Err(OdpError("non-finite chart data".into()));
    }
    let index = columns.len();
    columns.push(ChartColumn { label, values: values.to_vec() });
    Ok(index)
}
fn chart_range(columns: &[ChartColumn], index: usize) -> String {
    let n = columns.get(index).map(|c| c.values.len()).unwrap_or(0);
    if n == 0 {
        String::new()
    } else {
        let col = column(index);
        format!("local-table.{col}2:local-table.{col}{}", n + 1)
    }
}
fn chart_number_style(name: &str, format: &str) -> Result<String> {
    // Only formats whose semantics the chart reader independently models. Never
    // reinterpret arbitrary Excel sections, dates, colours or scaling commas.
    let (currency, rest) = match format.chars().next() {
        Some(c @ ('$' | '€' | '£' | '¥')) => (format.get(..c.len_utf8()).unwrap_or(""), format.get(c.len_utf8()..).unwrap_or(format)),
        _ => ("", format),
    };
    let (rest, percent) = rest.strip_suffix('%').map(|s| (s, true)).unwrap_or((rest, false));
    let (rest, grouping) = rest.strip_prefix("#,##").map(|s| (s, true)).unwrap_or((rest, false));
    let (integer, fractional) = rest.split_once('.').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
    if integer.is_empty()
        || integer.len() > 20
        || !integer.bytes().all(|c| c == b'0')
        || fractional.is_some_and(|f| f.is_empty() || f.len() > 10 || !f.bytes().all(|c| c == b'0'))
        || percent && !currency.is_empty()
    {
        return Err(OdpError("unsupported chart number format".into()));
    }
    let family = if percent {
        "percentage"
    } else if !currency.is_empty() {
        "currency"
    } else {
        "number"
    };
    let prefix = if currency.is_empty() { String::new() } else { format!("<number:currency-symbol>{}</number:currency-symbol>", esc(currency)) };
    Ok(format!(
        "<number:{family}-style style:name=\"{name}\">{prefix}<number:number number:min-integer-digits=\"{}\" number:decimal-places=\"{}\" number:grouping=\"{grouping}\"/>{}</number:{family}-style>",
        integer.len(),
        fractional.map(str::len).unwrap_or(0),
        if percent { "<number:text>%</number:text>" } else { "" }
    ))
}
#[allow(clippy::too_many_arguments)]
fn chart_axis_style(
    name: &str,
    min: Option<f64>,
    max: Option<f64>,
    major: Option<f64>,
    hidden: bool,
    log: bool,
    format: Option<&str>,
    geometry: &str,
) -> Result<String> {
    let mut props = String::new();
    if min.zip(max).is_some_and(|(a, b)| a >= b) {
        return Err(OdpError("chart axis minimum must precede maximum".into()));
    }
    for (key, v) in [("chart:minimum", min), ("chart:maximum", max), ("chart:interval-major", major)] {
        if let Some(v) = v {
            if !v.is_finite() || (key == "chart:interval-major" || log) && v <= 0.0 {
                return Err(OdpError("invalid chart axis".into()));
            }
            props.push_str(&format!(" {key}=\"{v}\""));
        }
    }
    let (number, attr) = if let Some(f) = format {
        (chart_number_style(&format!("{name}Number"), f)?, format!(" style:data-style-name=\"{name}Number\""))
    } else {
        (String::new(), String::new())
    };
    Ok(format!(
        "{number}<style:style style:name=\"{name}\" style:family=\"chart\"{attr}><style:chart-properties chart:display-label=\"{}\" chart:logarithmic=\"{log}\"{props} {geometry}/></style:style>",
        !hidden
    ))
}
fn chart_regression(i: usize, j: usize, t: &deckcraft_model::chart::Trendline) -> Result<(String, String)> {
    use deckcraft_model::chart::TrendlineType as T;
    if t.forward.is_some()
        || t.backward.is_some()
        || t.order.is_some_and(|v| !(2..=6).contains(&v))
        || t.period.is_some_and(|v| !(2..=255).contains(&v))
        || t.order.is_some() && t.kind != T::Poly
        || t.period.is_some() && t.kind != T::MovingAvg
        || t.intercept.is_some_and(|v| !v.is_finite())
        || t.intercept.is_some() && !matches!(t.kind, T::Linear | T::Poly | T::Exp)
        || t.kind == T::MovingAvg && (t.disp_eq || t.disp_r_sqr)
    {
        return Err(OdpError("chart regression options cannot be represented faithfully in ODP".into()));
    }
    let kind = match t.kind {
        T::Linear => "linear",
        T::Log => "logarithmic",
        T::Exp => "exponential",
        T::Power => "power",
        T::Poly => "polynomial",
        T::MovingAvg => "moving-average",
    };
    let mut attrs = format!("chart:regression-type=\"{kind}\"");
    if let Some(name) = &t.name {
        attrs.push_str(&format!(" chart:regression-name=\"{}\"", esc(name)));
    }
    if let Some(v) = t.order {
        attrs.push_str(&format!(" chart:regression-max-degree=\"{v}\""));
    }
    if let Some(v) = t.period {
        attrs.push_str(&format!(" chart:regression-period=\"{v}\" chart:regression-moving-type=\"prior\""));
    }
    if let Some(v) = t.intercept {
        attrs.push_str(&format!(" chart:regression-force-intercept=\"true\" chart:regression-intercept-value=\"{v}\""));
    }
    let style = format!("<style:style style:name=\"Trend{i}_{j}\" style:family=\"chart\"><style:chart-properties {attrs}/></style:style>");
    let child = format!(
        "<chart:regression-curve chart:style-name=\"Trend{i}_{j}\"><chart:equation chart:display-equation=\"{}\" chart:display-r-square=\"{}\"/></chart:regression-curve>",
        t.disp_eq, t.disp_r_sqr
    );
    Ok((style, child))
}
impl Writer<'_> {
    fn chart_fill_attrs(&mut self, ctx: &Ctx<'_>, fill: Option<&Fill>, ph: Option<Rgba>, dimensions: (f64, f64)) -> Result<String> {
        Ok(self.fill_attrs(ctx, fill, ph, dimensions)?.replace(" draw:opacity=\"100%\"", ""))
    }
    pub(super) fn chart(&mut self, ctx: &Ctx<'_>, c: &Chart, width: f64, height: f64) -> Result<String> {
        use deckcraft_model::chart::ChartType as T;
        let base_kind = if c.kind == T::Combo { c.series.first().map(|s| c.series_kind(s)).unwrap_or(T::Column) } else { c.kind };
        let class = chart_class(base_kind)?;
        let numeric = matches!(base_kind, T::Scatter | T::Bubble);
        if c.series.len() > 999 || c.categories.len() > 999 {
            return Err(OdpError("chart exceeds editable export limits".into()));
        }
        if c.kind == T::Combo && c.series.is_empty() {
            return Err(OdpError("empty Combo charts have no representable effective class".into()));
        }
        if numeric && c.cat_reversed {
            return Err(OdpError("numeric X-axis reversal is not represented by the chart model".into()));
        }
        for series in &c.series {
            let kind = c.series_kind(series);
            chart_class(kind)?;
            if c.kind != T::Combo && (series.secondary_axis || series.kind.is_some_and(|k| k != c.kind))
                || c.kind == T::Combo
                    && (kind.stacked()
                        || kind.horizontal() != base_kind.horizontal()
                        || matches!(kind, T::Scatter | T::Bubble) != numeric
                        || !matches!(kind, T::Column | T::Bar | T::Line | T::LineMarkers | T::Area | T::Scatter | T::Bubble))
                || series.secondary_axis && base_kind.horizontal()
            {
                return Err(OdpError("incompatible mixed chart classes, stacking or secondary horizontal axes".into()));
            }
        }
        if matches!(base_kind, T::Pie | T::Doughnut) && c.series.len() > 1 {
            return Err(OdpError("multi-series circular chart geometry is not represented by the renderer".into()));
        }
        let has_secondary = c.series.iter().any(|s| c.on_secondary(s))
            || c.y2_min.is_some()
            || c.y2_max.is_some()
            || c.y2_major.is_some()
            || c.y2_format.is_some()
            || c.y2_hidden;
        if has_secondary && c.kind != T::Combo {
            return Err(OdpError("secondary-axis settings require an effective Combo chart".into()));
        }
        if (c.gap_width * 100.0 - (c.gap_width * 100.0).round()).abs() > 1e-7 || (c.overlap * 100.0 - (c.overlap * 100.0).round()).abs() > 1e-7 {
            return Err(OdpError("fractional-percent chart gap or overlap cannot be represented exactly".into()));
        }
        if c.series.len() > 999 || c.categories.len() > 999 {
            return Err(OdpError("chart exceeds editable export limits".into()));
        }
        let labels = c
            .categories
            .iter()
            .map(String::len)
            .chain(c.series.iter().map(|s| s.name.len()))
            .chain(c.title.iter().map(String::len))
            .chain(c.axis_titles.0.iter().chain(&c.axis_titles.1).map(String::len))
            .chain(c.series.iter().flat_map(|s| s.trendlines.iter()).filter_map(|t| t.name.as_ref()).map(String::len))
            .fold(0usize, usize::saturating_add);
        if labels > 2 * 1024 * 1024 {
            return Err(OdpError("chart text exceeds export budget".into()));
        }
        if c.series.iter().filter(|s| s.number_format.is_some()).count() + usize::from(c.y_format.is_some()) + usize::from(c.y2_format.is_some())
            > 1000
        {
            return Err(OdpError("chart number styles exceed editable export limit".into()));
        }
        if !c.gap_width.is_finite()
            || !c.overlap.is_finite()
            || !c.hole_size.is_finite()
            || c.gap_width < 0.0
            || c.gap_width > 10.0
            || c.overlap.abs() > 1.0
            || !(0.0..=1.0).contains(&c.hole_size)
        {
            return Err(OdpError("invalid chart geometry".into()));
        }
        if c.raw.as_ref().is_some_and(|s| !s.is_empty()) || !c.kept.parts.is_empty() || !c.kept.rels.is_empty() {
            return Err(OdpError("unclassified original chart XML or dependent parts cannot be discarded during ODP conversion".into()));
        }
        if c.legend_overlay
            || c.up_down_bars
            || !c.category_levels.is_empty()
            || c.lang.is_some()
            || !c.subtotals.is_empty()
            || c.inclusive_quartiles
            || c.bin_count.is_some()
            || c.bin_size.is_some()
        {
            return Err(OdpError("chart overlays, stock bars, locale or extended settings cannot be exported faithfully yet".into()));
        }
        if c.y_log_base.is_some_and(|v| v != 10.0) {
            return Err(OdpError("only base-ten logarithmic chart axes are representable in ODP".into()));
        }
        if c.y_log_base.is_some() && c.y_major.is_some() {
            return Err(OdpError("manual logarithmic major units are not honored by the chart renderer".into()));
        }
        let mut columns = vec![ChartColumn { label: String::new(), values: vec![None; c.categories.len()] }];
        let mut data = Vec::with_capacity(c.series.len());
        for series in &c.series {
            let kind = c.series_kind(series);
            if kind == T::Bubble && series.sizes.iter().flatten().any(|v| *v < 0.0) {
                return Err(OdpError("negative bubble areas cannot be represented faithfully".into()));
            }
            if !numeric && (!series.x_values.is_empty() || !series.sizes.is_empty()) || kind != T::Bubble && !series.sizes.is_empty() {
                return Err(OdpError("chart data domains do not match its effective type".into()));
            }
            if series.smooth != c.series.first().is_some_and(|s| s.smooth) || series.smooth && kind == T::Bubble {
                return Err(OdpError("mixed per-series interpolation cannot be represented in a global ODF plot style".into()));
            }
            if series.trendlines.len() > 8 {
                return Err(OdpError("chart regression count exceeds renderer limit".into()));
            }
            if numeric && !series.trendlines.is_empty() && !series.x_values.iter().flatten().any(|v| v.is_finite()) {
                return Err(OdpError("numeric regressions require explicit X values in the chart model".into()));
            }
            if (!series.trendlines.is_empty() || series.error_bars.is_some())
                && !matches!(
                    kind,
                    T::Column
                        | T::StackedColumn
                        | T::PercentColumn
                        | T::Bar
                        | T::StackedBar
                        | T::PercentBar
                        | T::Line
                        | T::LineMarkers
                        | T::StackedLine
                        | T::Area
                        | T::StackedArea
                        | T::Scatter
                        | T::Bubble
                )
            {
                return Err(OdpError("chart regressions or errors require a supported Cartesian renderer".into()));
            }
            if c.y_log_base.is_some() && !c.on_secondary(series) && series.values.iter().flatten().any(|v| *v <= 0.0) {
                return Err(OdpError("logarithmic chart series require positive values".into()));
            }
            let values = chart_column(&mut columns, series.name.clone(), if kind == T::Bubble { &series.sizes } else { &series.values })?;
            let mut domains = Vec::new();
            if kind == T::Bubble {
                domains.push(chart_column(&mut columns, String::new(), &series.values)?);
            }
            if numeric {
                // The renderer's implicit X is 1,2,... only when no X value exists.
                // Materialise it rather than accidentally sharing an earlier domain.
                if series.x_values.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(OdpError("non-finite chart X data".into()));
                }
                let implicit;
                let xs = if series.x_values.iter().flatten().next().is_none() {
                    let count = series.values.len().max(series.x_values.len());
                    if count > 999 {
                        return Err(OdpError("chart domain exceeds export limits".into()));
                    }
                    implicit = (1..=count).map(|i| Some(i as f64)).collect::<Vec<_>>();
                    &implicit
                } else {
                    &series.x_values
                };
                domains.push(chart_column(&mut columns, String::new(), xs)?);
            }
            let mut errors = (None, None);
            if let Some(e) = &series.error_bars {
                if !matches!(e.bar_type.as_str(), "both" | "plus" | "minus")
                    || !e.end_cap
                    || !e.value.is_finite()
                    || e.value < 0.0
                    || !matches!(e.val_type.as_str(), "fixedVal" | "percentage" | "cust")
                {
                    return Err(OdpError("unsupported chart error-bar type, cap or statistical semantics".into()));
                }
                if e.val_type == "cust" {
                    if e.plus.iter().chain(&e.minus).flatten().any(|v| *v < 0.0) {
                        return Err(OdpError("negative chart error amount".into()));
                    }
                    errors.0 = Some(chart_column(&mut columns, String::new(), &e.minus)?);
                    errors.1 = Some(chart_column(&mut columns, String::new(), &e.plus)?);
                } else if !e.plus.is_empty() || !e.minus.is_empty() {
                    return Err(OdpError("unused custom error data cannot be discarded".into()));
                }
            }
            data.push(ChartSeriesData { values, domains, errors });
        }
        let rows = columns.iter().map(|c| c.values.len()).max().unwrap_or(0);
        if c.title.is_none() && c.title_size.is_some() {
            return Err(OdpError("chart title formatting requires a title to preserve its editable scope".into()));
        }
        // The importer charges each displayed cell, including numeric text. Count
        // precisely the finite values we serialize so a successful export can reopen.
        let table_text_bytes = columns
            .iter()
            .map(|column| column.label.len())
            .chain(c.categories.iter().take(rows).map(String::len))
            .chain(columns.iter().skip(1).flat_map(|column| column.values.iter().flatten()).map(|value| value.to_string().len()))
            .fold(0usize, usize::saturating_add);
        if table_text_bytes > 2 * 1024 * 1024 {
            return Err(OdpError("expanded chart text exceeds export budget".into()));
        }
        let mut styles = String::new();
        let mut root_text = String::new();
        for (key, value) in [("fo:font-size", c.text_size)] {
            if let Some(v) = value {
                if !v.is_finite() || v <= 0.0 || v > 10000.0 {
                    return Err(OdpError("invalid chart text size".into()));
                }
                root_text.push_str(&format!(" {key}=\"{v}pt\""));
            }
        }
        if let Some(color) = &c.text_color {
            let color = ctx.color(color, None);
            if color.a != 255 {
                return Err(OdpError("transparent chart text is not representable yet".into()));
            }
            root_text.push_str(&format!(" fo:color=\"#{}\"", color.hex()));
        }
        chart_fill_supported(c.chart_fill.as_ref())?;
        chart_fill_supported(c.plot_fill.as_ref())?;
        let root_fill = self.chart_fill_attrs(ctx, c.chart_fill.as_ref(), None, (width, height))?;
        styles.push_str(&format!("<style:style style:name=\"Chart\" style:family=\"chart\"><style:graphic-properties {root_fill}/><style:text-properties {root_text}/></style:style>"));
        let plot_fill = self.chart_fill_attrs(ctx, c.plot_fill.as_ref(), None, (width, height))?;
        styles.push_str(&format!("<style:style style:name=\"Plot\" style:family=\"chart\"><style:chart-properties chart:treat-empty-cells=\"ignore\" chart:vertical=\"{}\" chart:stacked=\"{}\" chart:percentage=\"{}\" chart:gap-width=\"{}\" chart:overlap=\"{}\" chart:hole-size=\"{}%\" chart:data-label-number=\"{}\" chart:interpolation=\"{}\"/><style:graphic-properties {plot_fill}/></style:style>",base_kind.horizontal(),base_kind.stacked(),base_kind.percent(),(c.gap_width*100.0).round(),(c.overlap*100.0).round(),c.hole_size*100.0,if c.data_labels {"value"}else{"none"},if c.series.first().is_some_and(|s|s.smooth){"cubic-spline"}else{"none"}));
        let geometry = format!(
            "chart:gap-width=\"{}\" chart:overlap=\"{}\" chart:interval-minor-divisor=\"5\"",
            (c.gap_width * 100.0).round(),
            (c.overlap * 100.0).round()
        );
        styles.push_str(&chart_axis_style(
            "ValueAxis",
            c.y_min,
            c.y_max,
            c.y_major,
            c.y_hidden,
            c.y_log_base.is_some(),
            c.y_format.as_deref(),
            &geometry,
        )?);
        if has_secondary {
            styles.push_str(&chart_axis_style("ValueAxis2", c.y2_min, c.y2_max, c.y2_major, c.y2_hidden, false, c.y2_format.as_deref(), &geometry)?);
        }
        styles.push_str(&format!(
            "<style:style style:name=\"CategoryAxis\" style:family=\"chart\"><style:chart-properties chart:reverse-direction=\"{}\"/></style:style>",
            c.cat_reversed
        ));
        if let Some(size) = c.title_size {
            if !size.is_finite() || size <= 0.0 || size > 10000.0 {
                return Err(OdpError("invalid chart title size".into()));
            }
            styles.push_str(&format!(
                "<style:style style:name=\"Title\" style:family=\"chart\"><style:text-properties fo:font-size=\"{size}pt\"/></style:style>"
            ));
        }
        for (i, s) in c.series.iter().enumerate() {
            let kind = c.series_kind(s);
            chart_fill_supported(s.fill.as_ref())?;
            if s.line.as_ref().is_some_and(|l| {
                l.dash.is_some() || l.head.is_some() || l.tail.is_some() || l.cap.is_some() || l.join.is_some() || l.compound.is_some()
            }) {
                return Err(OdpError("custom chart outlines cannot be exported faithfully yet".into()));
            }
            let default = Fill::solid(chart_color(c, i));
            let fill = self.chart_fill_attrs(ctx, s.fill.as_ref().or(Some(&default)), None, (width, height))?;
            let default_line = if kind == T::Scatter || s.fill.as_ref().is_some_and(Fill::is_none) {
                Line::none()
            } else {
                let color = match &s.fill {
                    Some(Fill::Solid { color }) => color.clone(),
                    _ => chart_color(c, i),
                };
                Line::solid(color, 2.25)
            };
            let effective_line = s.line.as_ref().unwrap_or(&default_line);
            if effective_line.fill.as_ref().is_some_and(|f| matches!(f,Fill::Solid{color} if ctx.color(color,None).a!=255)) {
                return Err(OdpError("transparent chart outlines are not represented yet".into()));
            }
            let line = self.line_attrs(ctx, effective_line, None)?;
            let marker = s.marker.as_deref().unwrap_or(if matches!(kind, T::LineMarkers | T::Scatter) { "circle" } else { "none" });
            if !matches!(marker, "none" | "circle" | "square" | "diamond" | "star" | "x" | "plus") {
                return Err(OdpError("unsupported chart marker".into()));
            }
            let marker = if marker == "none" {
                "chart:symbol-type=\"none\"".into()
            } else {
                format!("chart:symbol-type=\"named-symbol\" chart:symbol-name=\"{marker}\"")
            };
            let labels = s.data_labels.map(|v| format!(" chart:data-label-number=\"{}\"", if v { "value" } else { "none" })).unwrap_or_default();
            let format_attr = if let Some(f) = &s.number_format {
                styles.push_str(&chart_number_style(&format!("SeriesNumber{i}"), f)?);
                format!(" style:data-style-name=\"SeriesNumber{i}\"")
            } else {
                String::new()
            };
            styles.push_str(&format!("<style:style style:name=\"Series{i}\" style:family=\"chart\"{format_attr}><style:chart-properties {marker}{labels}/><style:graphic-properties {fill} {line}/></style:style>"));
            for (j, t) in s.trendlines.iter().enumerate() {
                styles.push_str(&chart_regression(i, j, t)?.0);
            }
            if let Some(e) = &s.error_bars {
                let mut attrs =
                    format!("chart:error-lower-indicator=\"{}\" chart:error-upper-indicator=\"{}\"", e.bar_type != "plus", e.bar_type != "minus");
                match e.val_type.as_str() {
                    "fixedVal" => attrs.push_str(&format!(
                        " chart:error-category=\"constant\" chart:error-lower-limit=\"{}\" chart:error-upper-limit=\"{}\"",
                        e.value, e.value
                    )),
                    "percentage" => attrs.push_str(&format!(" chart:error-category=\"percentage\" chart:error-percentage=\"{}\"", e.value)),
                    "cust" => {
                        attrs.push_str(" chart:error-category=\"cell-range\"");
                        if let Some(d) = data.get(i) {
                            for (key, index) in [("chart:error-lower-range", d.errors.0), ("chart:error-upper-range", d.errors.1)] {
                                if let Some(index) = index {
                                    attrs.push_str(&format!(" {key}=\"{}\"", chart_range(&columns, index)));
                                }
                            }
                        }
                    }
                    _ => return Err(OdpError("unsupported chart error bars".into())),
                }
                styles.push_str(&format!(
                    "<style:style style:name=\"Error{i}\" style:family=\"chart\"><style:chart-properties {attrs}/></style:style>"
                ));
            }
            let mut used = std::collections::BTreeSet::new();
            for (index, fill) in &s.point_fills {
                if *index >= s.values.len() || !used.insert(*index) {
                    return Err(OdpError("chart point style index is invalid or duplicated".into()));
                }
                chart_fill_supported(Some(fill))?;
            }
            for point in 0..s.values.len() {
                let palette = Fill::solid(chart_color(c, point));
                let vary = matches!(c.kind, T::Pie | T::Doughnut) || (c.vary_colors && c.series.len() == 1 && s.fill.is_none());
                let fill = s.point_fills.iter().find(|(k, _)| *k == point).map(|(_, f)| f).or(if vary { Some(&palette) } else { None });
                if let Some(fill) = fill {
                    let attrs = self.chart_fill_attrs(ctx, Some(fill), None, (width, height))?;
                    styles.push_str(&format!(
                        "<style:style style:name=\"Point{i}_{point}\" style:family=\"chart\"><style:graphic-properties {attrs}/></style:style>"
                    ));
                }
            }
        }
        let mut out = header("office:document-content");
        out.push_str(&format!("<office:automatic-styles>{styles}</office:automatic-styles><office:body><office:chart><chart:chart chart:class=\"chart:{class}\" chart:style-name=\"Chart\" svg:width=\"{width}pt\" svg:height=\"{height}pt\" xlink:href=\"..\" xlink:type=\"simple\">"));
        if let Some(title) = &c.title {
            let style = if c.title_size.is_some() { " chart:style-name=\"Title\"" } else { "" };
            out.push_str(&format!("<chart:title{style}><text:p>{}</text:p></chart:title>", text(title)));
        }
        if let Some(pos) = &c.legend {
            let pos = match pos.as_str() {
                "l" => "start",
                "r" => "end",
                "t" => "top",
                "b" => "bottom",
                "tr" => "top-end",
                _ => return Err(OdpError("unsupported chart legend position".into())),
            };
            out.push_str(&format!("<chart:legend chart:legend-position=\"{pos}\"/>"));
        }
        out.push_str("<chart:plot-area chart:style-name=\"Plot\" chart:data-source-has-labels=\"row\"><chart:axis chart:dimension=\"x\" chart:name=\"primary-x\" chart:style-name=\"CategoryAxis\">");
        if let Some(title) = &c.axis_titles.0 {
            out.push_str(&format!("<chart:title><text:p>{}</text:p></chart:title>", text(title)));
        }
        if !c.categories.is_empty() {
            out.push_str(&format!("<chart:categories table:cell-range-address=\"{}\"/>", chart_range(&columns, 0)));
        }
        out.push_str("</chart:axis><chart:axis chart:dimension=\"y\" chart:name=\"primary-y\" chart:style-name=\"ValueAxis\">");
        if let Some(title) = &c.axis_titles.1 {
            out.push_str(&format!("<chart:title><text:p>{}</text:p></chart:title>", text(title)));
        }
        if c.gridlines {
            out.push_str("<chart:grid chart:class=\"major\"/>");
        }
        if c.minor_gridlines {
            out.push_str("<chart:grid chart:class=\"minor\"/>");
        }
        out.push_str("</chart:axis>");
        if has_secondary {
            out.push_str("<chart:axis chart:dimension=\"y\" chart:name=\"secondary-y\" chart:style-name=\"ValueAxis2\"/>");
        }
        for (i, (s, d)) in c.series.iter().zip(&data).enumerate() {
            let class = chart_class(c.series_kind(s))?;
            let attached = if c.on_secondary(s) { "secondary-y" } else { "primary-y" };
            let label = column(d.values);
            out.push_str(&format!("<chart:series chart:class=\"chart:{class}\" chart:attached-axis=\"{attached}\" chart:style-name=\"Series{i}\" chart:label-cell-address=\"local-table.{label}1\" chart:values-cell-range-address=\"{}\">",chart_range(&columns,d.values)));
            for index in &d.domains {
                out.push_str(&format!("<chart:domain table:cell-range-address=\"{}\"/>", chart_range(&columns, *index)));
            }
            for (j, t) in s.trendlines.iter().enumerate() {
                out.push_str(&chart_regression(i, j, t)?.1);
            }
            if s.error_bars.is_some() {
                out.push_str(&format!("<chart:error-indicator chart:dimension=\"y\" chart:style-name=\"Error{i}\"/>"));
            }
            for point in 0..s.values.len() {
                let vary = matches!(c.kind, T::Pie | T::Doughnut) || (c.vary_colors && c.series.len() == 1 && s.fill.is_none());
                if vary || s.point_fills.iter().any(|(k, _)| *k == point) {
                    out.push_str(&format!("<chart:data-point chart:style-name=\"Point{i}_{point}\"/>"));
                } else {
                    out.push_str("<chart:data-point/>");
                }
            }
            out.push_str("</chart:series>");
        }
        out.push_str(&format!("</chart:plot-area><table:table table:name=\"local-table\"><table:table-column table:number-columns-repeated=\"{}\"/><table:table-header-rows><table:table-row>",columns.len()));
        for column in &columns {
            out.push_str(&format!("<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", text(&column.label)));
        }
        out.push_str("</table:table-row></table:table-header-rows>");
        for row in 0..rows {
            out.push_str("<table:table-row>");
            for (i, column) in columns.iter().enumerate() {
                if i == 0 {
                    if let Some(label) = c.categories.get(row) {
                        out.push_str(&format!("<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", text(label)));
                    } else {
                        out.push_str("<table:table-cell/>");
                    }
                } else if let Some(v) = column.values.get(row).copied().flatten() {
                    out.push_str(&format!(
                        "<table:table-cell office:value-type=\"float\" office:value=\"{v}\"><text:p>{v}</text:p></table:table-cell>"
                    ));
                } else {
                    out.push_str("<table:table-cell/>");
                }
            }
            out.push_str("</table:table-row>");
        }
        out.push_str("</table:table></chart:chart></office:chart></office:body></office:document-content>");
        Ok(out)
    }
    pub(super) fn media(&mut self, c: &MediaClip, out: &mut String) -> Result<()> {
        if !c.volume.is_finite() {
            return Err(OdpError("non-finite media volume".into()));
        }
        if c.trim_start_ms != 0
            || c.trim_end_ms != 0
            || c.fade_in_ms != 0
            || c.fade_out_ms != 0
            || c.play_across_slides
            || c.full_screen
            || !c.bookmarks.is_empty()
        {
            return Err(OdpError("media trimming, fades, bookmarks or cross-slide playback cannot be represented faithfully in ODP yet".into()));
        }
        let path = self.image_path(c.media)?;
        let media = self.pres.media.iter().find(|m| m.id == c.media).ok_or_else(|| OdpError("missing media".into()))?;
        out.push_str(&format!("<draw:plugin xlink:href=\"{path}\" xlink:type=\"simple\" draw:mime-type=\"{}\"><draw:param draw:name=\"autoplay\" draw:value=\"{}\"/><draw:param draw:name=\"loop\" draw:value=\"{}\"/><draw:param draw:name=\"volume\" draw:value=\"{}\"/></draw:plugin>",esc(&media.content_type),c.autoplay,c.loop_play,(c.volume.clamp(0.0,1.0)*100.0).round()));
        if let Some(poster) = c.poster {
            let path = self.image_path(poster)?;
            out.push_str(&format!("<draw:image xlink:href=\"{path}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>"));
        }
        Ok(())
    }
}

pub(super) fn link_href(p: &Presentation, l: &deckcraft_model::text::Hyperlink) -> Result<String> {
    use deckcraft_model::text::Action;
    if l.sound.is_some() || l.sound_name.is_some() || l.stop_sound || l.zoom_return.is_some() {
        return Err(OdpError("hyperlink sound and return behavior cannot be exported faithfully in ODP yet".into()));
    }
    match &l.action {
        Action::Url { url } => Ok(url.clone()),
        Action::Slide { slide } => p
            .slides
            .iter()
            .enumerate()
            .find(|(_, s)| s.id == *slide)
            .map(|(i, s)| format!("#{}", if s.name.is_empty() { format!("Slide {}", i + 1) } else { s.name.clone() }))
            .ok_or_else(|| OdpError("hyperlink target slide is missing".into())),
        _ => Err(OdpError("this hyperlink action cannot be exported faithfully in ODP yet".into())),
    }
}
pub(super) fn transition_attrs(t: Option<&Transition>) -> Result<String> {
    let Some(t) = t else { return Ok(String::new()) };
    if !t.advance_on_click && t.advance_after_ms.is_none() {
        return Err(OdpError("disabled slide advance without a timer has no native ODF page setting".into()));
    }
    let (kind, sub, dir) = super::timing::transition_type(t)?;
    let mut out = format!(
        "smil:type=\"{kind}\" smil:subtype=\"{sub}\" smil:direction=\"{}\" presentation:transition-type=\"{}\"",
        dir,
        if t.advance_after_ms.is_some() { if t.advance_on_click { "semi-automatic" } else { "automatic" } } else { "manual" }
    );
    if let Some(ms) = t.advance_after_ms {
        out.push_str(&format!(" presentation:duration=\"PT{}S\"", f64::from(ms) / 1000.0));
    }
    Ok(out)
}
