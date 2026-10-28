//! Chart parts: a `c:chartSpace` with cached categories and values.
//! An unchanged chart also keeps the embedded workbook and the other parts it points at.

use deckcraft_model::chart::{Chart, ChartType, ErrorBars, Series, Trendline, TrendlineType};
use deckcraft_model::style::Fill;

use crate::opc::{NS_A, NS_C, NS_R};

/// The Office 2016 chart namespace.
pub const NS_CX: &str = "http://schemas.microsoft.com/office/drawing/2014/chartex";
use crate::xml::{A, W};

/// Column letter for a 0-based index (A, B, … Z, AA…).
fn col(i: usize) -> String {
    let mut n = i + 1;
    let mut s = String::new();
    while n > 0 && s.len() < 4 {
        let r = (n - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

fn fmt_num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

fn simple_fill(w: &mut W, f: &Fill) {
    match f {
        Fill::None => w.empty0("a:noFill"),
        Fill::Solid { color } => super::dml::solid(w, color),
        Fill::Pattern(p) => {
            w.open("a:pattFill", A::new().a("prst", if p.preset.is_empty() { "pct5" } else { &p.preset }));
            w.open0("a:fgClr");
            super::dml::color(w, &p.fg);
            w.close("a:fgClr");
            w.open0("a:bgClr");
            super::dml::color(w, &p.bg);
            w.close("a:bgClr");
            w.close("a:pattFill");
        }
        Fill::Gradient(_) | Fill::Picture(_) | Fill::Group | Fill::Background => {
            if let Fill::Gradient(g) = f
                && let Some(s) = g.stops.first()
            {
                super::dml::solid(w, &s.color);
            }
        }
    }
}

fn sp_pr(w: &mut W, fill: Option<&Fill>, line: Option<&deckcraft_model::Line>) {
    if fill.is_none() && line.is_none() {
        return;
    }
    w.open0("c:spPr");
    if let Some(f) = fill {
        simple_fill(w, f);
    }
    if let Some(l) = line {
        w.open("a:ln", A::new().o("w", l.width.map(super::dml::emu_pos)));
        if let Some(f) = &l.fill {
            simple_fill(w, f);
        }
        w.close("a:ln");
    }
    w.close("c:spPr");
}

/// `a:defRPr` carrying the chart's text colour, where PowerPoint keeps it: the paragraph
/// defaults of a title's rich text and of every `c:txPr`.
fn def_rpr(w: &mut W, color: Option<&deckcraft_model::ColorRef>) {
    match color {
        Some(c) => {
            w.open0("a:defRPr");
            super::dml::solid(w, c);
            w.close("a:defRPr");
        }
        None => w.empty0("a:defRPr"),
    }
}

/// `c:txPr` with the chart's text colour (`Chart::text_color`) as the default run properties, on
/// the axes, the legend, the data labels and the chart itself, as PowerPoint writes it. Nothing
/// without a colour: the theme's default then applies on reading.
fn tx_pr(w: &mut W, c: &Chart) {
    let Some(color) = &c.text_color else { return };
    w.open0("c:txPr");
    w.empty0("a:bodyPr");
    w.empty0("a:lstStyle");
    w.open0("a:p");
    w.open0("a:pPr");
    def_rpr(w, Some(color));
    w.close("a:pPr");
    w.empty("a:endParaRPr", A::new().a("lang", "en-US"));
    w.close("a:p");
    w.close("c:txPr");
}

fn rich(w: &mut W, text: &str, color: Option<&deckcraft_model::ColorRef>) {
    w.open0("c:tx");
    w.open0("c:rich");
    w.empty0("a:bodyPr");
    w.empty0("a:lstStyle");
    for line in text.split('\n') {
        w.open0("a:p");
        if color.is_some() {
            w.open0("a:pPr");
            def_rpr(w, color);
            w.close("a:pPr");
        }
        w.open0("a:r");
        if let Some(c) = color {
            w.open("a:rPr", A::new().a("lang", "en-US"));
            super::dml::solid(w, c);
            w.close("a:rPr");
        }
        w.elt("a:t", line);
        w.close("a:r");
        w.close("a:p");
    }
    w.close("c:rich");
    w.close("c:tx");
}

/// A title (the chart's or an axis's) in the chart's text colour.
fn title(w: &mut W, text: &str, c: &Chart) {
    w.open0("c:title");
    rich(w, text, c.text_color.as_ref());
    w.val("c:overlay", 0);
    w.close("c:title");
}

fn str_cache(w: &mut W, f: &str, vals: &[String]) {
    w.open0("c:strRef");
    w.elt("c:f", f);
    w.open0("c:strCache");
    w.val("c:ptCount", vals.len());
    for (i, v) in vals.iter().enumerate() {
        w.open("c:pt", A::new().a("idx", i));
        w.elt("c:v", v);
        w.close("c:pt");
    }
    w.close("c:strCache");
    w.close("c:strRef");
}

fn num_cache(w: &mut W, f: &str, vals: &[Option<f64>]) {
    w.open0("c:numRef");
    w.elt("c:f", f);
    w.open0("c:numCache");
    w.elt("c:formatCode", "General");
    w.val("c:ptCount", vals.len());
    for (i, v) in vals.iter().enumerate() {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            w.open("c:pt", A::new().a("idx", i));
            w.elt("c:v", &fmt_num(v));
            w.close("c:pt");
        }
    }
    w.close("c:numCache");
    w.close("c:numRef");
}

/// Numbers written into the chart itself (`c:numLit`), with no worksheet range behind them.
fn num_lit(w: &mut W, vals: &[Option<f64>]) {
    w.open0("c:numLit");
    w.val("c:ptCount", vals.len());
    for (i, v) in vals.iter().enumerate() {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            w.open("c:pt", A::new().a("idx", i));
            w.elt("c:v", &fmt_num(v));
            w.close("c:pt");
        }
    }
    w.close("c:numLit");
}

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Bar,
    Line,
    Area,
    Pie,
    Doughnut,
    Scatter,
    Radar,
    Bubble,
    Stock,
}

fn group_of(k: ChartType) -> Group {
    match k {
        ChartType::Line | ChartType::LineMarkers | ChartType::StackedLine => Group::Line,
        ChartType::Stock => Group::Stock,
        ChartType::Area | ChartType::StackedArea | ChartType::Surface => Group::Area,
        ChartType::Pie => Group::Pie,
        ChartType::Doughnut | ChartType::Sunburst => Group::Doughnut,
        ChartType::Scatter => Group::Scatter,
        ChartType::Bubble => Group::Bubble,
        ChartType::Radar | ChartType::FilledRadar => Group::Radar,
        _ => Group::Bar,
    }
}

fn error_bars(w: &mut W, e: &ErrorBars, g: Group) {
    let word = |v: &str, ok: &[&'static str], d: &'static str| ok.iter().copied().find(|o| *o == v).unwrap_or(d);
    w.open0("c:errBars");
    if matches!(g, Group::Scatter | Group::Bubble) {
        w.val("c:errDir", "y");
    }
    w.val("c:errBarType", word(&e.bar_type, &["both", "plus", "minus"], "both"));
    let vt = word(&e.val_type, &["fixedVal", "percentage", "stdDev", "stdErr", "cust"], "fixedVal");
    w.val("c:errValType", vt);
    w.val("c:noEndCap", if e.end_cap { 0 } else { 1 });
    if vt == "cust" {
        w.open0("c:plus");
        num_lit(w, &e.plus);
        w.close("c:plus");
        w.open0("c:minus");
        num_lit(w, &e.minus);
        w.close("c:minus");
    } else if e.value.is_finite() {
        w.val("c:val", fmt_num(e.value));
    }
    w.close("c:errBars");
}

/// Per-point fills of pie, doughnut and bar series.
fn point_fills(w: &mut W, s: &Series, g: Group) {
    if !matches!(g, Group::Pie | Group::Doughnut | Group::Bar) {
        return;
    }
    for (idx, f) in s.point_fills.iter().take(1000) {
        w.open0("c:dPt");
        w.val("c:idx", idx);
        if g == Group::Bar {
            w.val("c:invertIfNegative", 0);
        }
        if g != Group::Bar {
            w.val("c:bubble3D", 0);
        }
        sp_pr(w, Some(f), None);
        w.close("c:dPt");
    }
}

fn series(w: &mut W, c: &Chart, s: &Series, i: usize, kind: ChartType, g: Group) {
    w.open0("c:ser");
    w.val("c:idx", i);
    w.val("c:order", i);
    w.open0("c:tx");
    str_cache(w, &format!("Sheet1!${}$1", col(i + 1)), std::slice::from_ref(&s.name));
    w.close("c:tx");
    if g == Group::Stock && s.line.is_none() {
        // A stock chart's series are points joined by the high-low lines, not lines of their own.
        w.open0("c:spPr");
        w.open("a:ln", A::new().a("w", 19050));
        w.empty0("a:noFill");
        w.close("a:ln");
        w.close("c:spPr");
    } else {
        sp_pr(w, s.fill.as_ref(), s.line.as_ref());
    }
    if g == Group::Stock {
        // High-low-close: the close is a dash on the high-low line; with up/down bars (or a set
        // marker) the series keep their own.
        let close = !c.up_down_bars && i + 1 == c.series.len() && c.series.len() == 3;
        let sym = s.marker.as_deref().filter(|m| !m.is_empty() && m.chars().all(|ch| ch.is_ascii_alphanumeric())).unwrap_or(if close {
            "dash"
        } else {
            "none"
        });
        w.open0("c:marker");
        w.val("c:symbol", sym);
        if sym != "none" {
            w.val("c:size", 7);
        }
        w.close("c:marker");
    }
    if g == Group::Bar {
        w.val("c:invertIfNegative", 0);
    }
    if matches!(g, Group::Line | Group::Scatter | Group::Radar) {
        let sym = match (&s.marker, kind) {
            (Some(m), _) if !m.is_empty() => Some(m.as_str()),
            (_, ChartType::LineMarkers) => None,
            (_, ChartType::Scatter) => None,
            _ => Some("none"),
        };
        if let Some(sym) = sym.filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric())) {
            w.open0("c:marker");
            w.val("c:symbol", sym);
            w.close("c:marker");
        }
    }
    point_fills(w, s, g);
    if let Some(d) = s.data_labels {
        data_labels(w, c, d);
    }
    // After labels, before the category and value caches (ECMA-376 series content order).
    if matches!(g, Group::Bar | Group::Line | Group::Area | Group::Scatter) {
        for t in s.trendlines.iter().take(16) {
            trendline(w, t);
        }
    }
    let n = c.categories.len().max(s.values.len());
    let last = n + 1;
    if let Some(e) = &s.error_bars {
        error_bars(w, e, g);
    }
    if matches!(g, Group::Scatter | Group::Bubble) {
        let xs: Vec<Option<f64>> = if s.x_values.is_empty() { (1..=s.values.len()).map(|v| Some(v as f64)).collect() } else { s.x_values.clone() };
        w.open0("c:xVal");
        num_cache(w, &format!("Sheet1!$A$2:$A${last}"), &xs);
        w.close("c:xVal");
        w.open0("c:yVal");
        num_cache(w, &format!("Sheet1!${0}$2:${0}${last}", col(i + 1)), &s.values);
        w.close("c:yVal");
        if g == Group::Bubble {
            let sizes: Vec<Option<f64>> = if s.sizes.is_empty() { s.values.iter().map(|_| Some(1.0)).collect() } else { s.sizes.clone() };
            w.open0("c:bubbleSize");
            num_cache(w, &format!("Sheet1!${0}$2:${0}${last}", col(i + 2)), &sizes);
            w.close("c:bubbleSize");
            w.val("c:bubble3D", 0);
        } else {
            w.val("c:smooth", if s.smooth { 1 } else { 0 });
        }
    } else {
        w.open0("c:cat");
        str_cache(w, &format!("Sheet1!$A$2:$A${last}"), &c.categories);
        w.close("c:cat");
        w.open0("c:val");
        num_cache(w, &format!("Sheet1!${0}$2:${0}${last}", col(i + 1)), &s.values);
        w.close("c:val");
        if g == Group::Line {
            w.val("c:smooth", if s.smooth { 1 } else { 0 });
        }
    }
    w.close("c:ser");
}

fn trend_num(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite() && v.abs() <= 1.0e12)
}

fn trend_name(s: &str) -> Option<String> {
    let cleaned: String =
        s.chars().filter(|c| matches!(c, '\t' | '\n' | '\r') || (*c >= ' ' && *c != '\u{FFFE}' && *c != '\u{FFFF}')).take(255).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() { None } else { Some(cleaned.to_string()) }
}

fn trendline(w: &mut W, t: &Trendline) {
    w.open0("c:trendline");
    if let Some(name) = t.name.as_deref().and_then(trend_name) {
        w.elt("c:name", &name);
    }
    w.val("c:trendlineType", t.kind.xml());
    if t.kind == TrendlineType::Poly
        && let Some(o) = t.order
    {
        w.val("c:order", o.clamp(2, 6));
    }
    if t.kind == TrendlineType::MovingAvg
        && let Some(p) = t.period
    {
        w.val("c:period", p.clamp(2, 255));
    }
    if let Some(v) = trend_num(t.forward) {
        w.val("c:forward", fmt_num(v));
    }
    if let Some(v) = trend_num(t.backward) {
        w.val("c:backward", fmt_num(v));
    }
    if let Some(v) = trend_num(t.intercept) {
        w.val("c:intercept", fmt_num(v));
    }
    if t.disp_r_sqr {
        w.val("c:dispRSqr", 1);
    }
    if t.disp_eq {
        w.val("c:dispEq", 1);
    }
    w.close("c:trendline");
}

fn data_labels(w: &mut W, c: &Chart, show: bool) {
    w.open0("c:dLbls");
    tx_pr(w, c);
    for (n, v) in [
        ("c:showLegendKey", false),
        ("c:showVal", show),
        ("c:showCatName", false),
        ("c:showSerName", false),
        ("c:showPercent", false),
        ("c:showBubbleSize", false),
    ] {
        w.val(n, if v { 1 } else { 0 });
    }
    w.close("c:dLbls");
}

const AX_CAT: u32 = 50_001;
const AX_VAL: u32 = 50_002;

fn axes(w: &mut W, c: &Chart, g: Group) {
    let horizontal = c.kind.horizontal() && g == Group::Bar;
    let (cat_pos, val_pos) = if horizontal { ("l", "b") } else { ("b", "l") };
    if g == Group::Scatter || g == Group::Bubble {
        w.open0("c:valAx");
        w.val("c:axId", AX_CAT);
        w.open0("c:scaling");
        w.val("c:orientation", "minMax");
        w.close("c:scaling");
        w.val("c:delete", 0);
        w.val("c:axPos", "b");
        if let Some(t) = &c.axis_titles.0 {
            title(w, t, c);
        }
        w.empty("c:numFmt", A::new().a("formatCode", "General").a("sourceLinked", 1));
        w.val("c:majorTickMark", "out");
        w.val("c:minorTickMark", "none");
        w.val("c:tickLblPos", "nextTo");
        tx_pr(w, c);
        w.val("c:crossAx", AX_VAL);
        w.val("c:crosses", "autoZero");
        w.val("c:crossBetween", "midCat");
        w.close("c:valAx");
    } else {
        w.open0("c:catAx");
        w.val("c:axId", AX_CAT);
        w.open0("c:scaling");
        w.val("c:orientation", if c.cat_reversed { "maxMin" } else { "minMax" });
        w.close("c:scaling");
        w.val("c:delete", 0);
        w.val("c:axPos", cat_pos);
        if let Some(t) = &c.axis_titles.0 {
            title(w, t, c);
        }
        w.empty("c:numFmt", A::new().a("formatCode", "General").a("sourceLinked", 1));
        w.val("c:majorTickMark", "none");
        w.val("c:minorTickMark", "none");
        w.val("c:tickLblPos", "nextTo");
        tx_pr(w, c);
        w.val("c:crossAx", AX_VAL);
        w.val("c:crosses", "autoZero");
        w.val("c:auto", 1);
        w.val("c:lblAlgn", "ctr");
        w.val("c:lblOffset", 100);
        w.val("c:noMultiLvlLbl", 0);
        w.close("c:catAx");
    }
    w.open0("c:valAx");
    w.val("c:axId", AX_VAL);
    w.open0("c:scaling");
    if let Some(b) = c.y_log_base {
        w.val("c:logBase", fmt_num(b));
    }
    w.val("c:orientation", "minMax");
    if let Some(m) = c.y_max.filter(|v| v.is_finite()) {
        w.val("c:max", fmt_num(m));
    }
    if let Some(m) = c.y_min.filter(|v| v.is_finite()) {
        w.val("c:min", fmt_num(m));
    }
    w.close("c:scaling");
    w.val("c:delete", 0);
    w.val("c:axPos", val_pos);
    if c.gridlines {
        w.empty0("c:majorGridlines");
    }
    if c.minor_gridlines {
        w.empty0("c:minorGridlines");
    }
    if let Some(t) = &c.axis_titles.1 {
        title(w, t, c);
    }
    match c.y_format.as_deref().filter(|f| !f.is_empty()) {
        Some(f) => w.empty("c:numFmt", A::new().a("formatCode", f).a("sourceLinked", 0)),
        None => w.empty("c:numFmt", A::new().a("formatCode", if c.kind.percent() { "0%" } else { "General" }).a("sourceLinked", 1)),
    }
    w.val("c:majorTickMark", "none");
    w.val("c:minorTickMark", "none");
    w.val("c:tickLblPos", "nextTo");
    tx_pr(w, c);
    w.val("c:crossAx", AX_CAT);
    w.val("c:crosses", "autoZero");
    w.val("c:crossBetween", if g == Group::Area || g == Group::Scatter { "midCat" } else { "between" });
    w.close("c:valAx");
}

fn plot_group(w: &mut W, c: &Chart, kind: ChartType, list: &[(usize, &Series)]) {
    let g = group_of(kind);
    let grouping = if kind.percent() {
        "percentStacked"
    } else if kind.stacked() {
        "stacked"
    } else if matches!(g, Group::Bar) {
        "clustered"
    } else {
        "standard"
    };
    let vary = if c.vary_colors { 1 } else { 0 };
    let tag = match g {
        Group::Bar => "c:barChart",
        Group::Line => "c:lineChart",
        Group::Area => "c:areaChart",
        Group::Pie => "c:pieChart",
        Group::Doughnut => "c:doughnutChart",
        Group::Scatter => "c:scatterChart",
        Group::Radar => "c:radarChart",
        Group::Bubble => "c:bubbleChart",
        Group::Stock => "c:stockChart",
    };
    w.open0(tag);
    match g {
        Group::Bar => {
            w.val("c:barDir", if kind.horizontal() { "bar" } else { "col" });
            w.val("c:grouping", grouping);
        }
        Group::Line | Group::Area => w.val("c:grouping", grouping),
        Group::Scatter => w.val("c:scatterStyle", "lineMarker"),
        Group::Radar => w.val("c:radarStyle", if kind == ChartType::FilledRadar { "filled" } else { "marker" }),
        _ => {}
    }
    // `c:stockChart` has no `c:varyColors`.
    if g != Group::Stock {
        w.val("c:varyColors", vary);
    }
    for (i, s) in list {
        series(w, c, s, *i, kind, g);
    }
    if c.data_labels {
        data_labels(w, c, true);
    }
    match g {
        Group::Bar => {
            w.val("c:gapWidth", (c.gap_width.clamp(0.0, 5.0) * 100.0).round() as i64);
            let ov = if kind.stacked() { 100 } else { (c.overlap.clamp(-1.0, 1.0) * 100.0).round() as i64 };
            if ov != 0 {
                w.val("c:overlap", ov);
            }
        }
        Group::Line => w.val("c:marker", 1),
        Group::Stock => {
            w.open0("c:hiLowLines");
            w.open0("c:spPr");
            w.open("a:ln", A::new().a("w", 9525));
            w.open0("a:solidFill");
            w.empty("a:schemeClr", A::new().a("val", "tx1"));
            w.close("a:solidFill");
            w.close("a:ln");
            w.close("c:spPr");
            w.close("c:hiLowLines");
            if c.up_down_bars {
                w.open0("c:upDownBars");
                w.val("c:gapWidth", 150);
                w.empty0("c:upBars");
                w.empty0("c:downBars");
                w.close("c:upDownBars");
            }
        }
        Group::Pie => w.val("c:firstSliceAng", 0),
        Group::Doughnut => {
            w.val("c:firstSliceAng", 0);
            w.val("c:holeSize", (c.hole_size.clamp(0.1, 0.9) * 100.0).round() as i64);
        }
        _ => {}
    }
    if !matches!(g, Group::Pie | Group::Doughnut) {
        w.val("c:axId", AX_CAT);
        w.val("c:axId", AX_VAL);
    }
    w.close(tag);
}

/// The `chartEx` part of an Office 2016 chart (waterfall, funnel…): the kept part when the chart
/// is unchanged since it was read, else one written from the model. `None` for other chart types.
#[cfg(test)]
pub fn chartex_xml(c: &Chart) -> Option<Vec<u8>> {
    chartex_part(c).map(|(x, _)| x)
}

/// [`chartex_xml`], and whether it is the original part (so its kept relationships still apply).
pub fn chartex_part(c: &Chart) -> Option<(Vec<u8>, bool)> {
    kept_chartex(c).map(|x| (x, true)).or_else(|| c.kind.is_chartex().then(|| (build_chartex(c), false)))
}

fn kept_chartex(c: &Chart) -> Option<Vec<u8>> {
    let raw = c.raw.as_ref()?;
    let doc = crate::xml::parse(raw.as_bytes()).ok()?;
    let mut root = doc.root.clone();
    crate::read::resolve_mc(&mut root, 0);
    let mut back = crate::read::chartex_from(&root)?;
    back.raw = c.raw.clone();
    // The sidecar is not modelled chart state.
    back.kept = c.kept.clone();
    if &back != c {
        return None;
    }
    let mut keep = doc.root;
    // The workbook link stays only when its part is written back.
    drop_unkept_refs(&mut keep, c);
    let mut w = W::new();
    w.raw(&keep.to_xml());
    Some(w.finish())
}

/// The chart part XML, and whether it is the original part (so its relationships still apply).
pub fn chart_xml(c: &Chart) -> (Vec<u8>, bool) {
    // Reuse the kept part when the chart is unchanged since it was read.
    if let Some(raw) = &c.raw
        && !raw.contains(NS_CX)
        && let Ok(doc) = crate::xml::parse(raw.as_bytes())
        && doc.root.is("chartSpace")
    {
        let mut root = doc.root.clone();
        crate::read::resolve_mc(&mut root, 0);
        if let Some(back) = crate::read::chart_from_el(&root) {
            let mut me = c.clone();
            me.raw = None;
            // The sidecar is not modelled chart state. Comparing it would throw away an unchanged chart.
            me.kept = Default::default();
            if back == me {
                let mut keep = doc.root;
                // Without the parts those elements point at, PowerPoint repairs the file.
                drop_unkept_refs(&mut keep, c);
                let mut w = W::new();
                w.raw(&keep.to_xml());
                return (w.finish(), true);
            }
        }
    }
    let mut w = W::new();
    w.open("c:chartSpace", A::new().a("xmlns:c", NS_C).a("xmlns:a", NS_A).a("xmlns:r", NS_R));
    w.val("c:date1904", 0);
    w.val("c:roundedCorners", 0);
    w.val("c:style", c.style.clamp(1, 48));
    w.open0("c:chart");
    match &c.title {
        Some(t) => {
            title(&mut w, t, c);
            w.val("c:autoTitleDeleted", 0);
        }
        None => w.val("c:autoTitleDeleted", 1),
    }
    w.open0("c:plotArea");
    w.empty0("c:layout");
    let indexed: Vec<(usize, &Series)> = c.series.iter().enumerate().collect();
    let main = match c.kind {
        ChartType::Combo => c.series.first().and_then(|s| s.kind).unwrap_or(ChartType::Column),
        k => k,
    };
    if c.kind == ChartType::Combo {
        // One plot group per series type, sharing the axes.
        let mut kinds: Vec<ChartType> = vec![];
        for s in &c.series {
            let k = s.kind.unwrap_or(main);
            let k = if matches!(group_of(k), Group::Pie | Group::Doughnut | Group::Scatter | Group::Bubble) { ChartType::Column } else { k };
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
        for k in kinds {
            let list: Vec<(usize, &Series)> = indexed
                .iter()
                .filter(|(_, s)| {
                    let sk = s.kind.unwrap_or(main);
                    let sk =
                        if matches!(group_of(sk), Group::Pie | Group::Doughnut | Group::Scatter | Group::Bubble) { ChartType::Column } else { sk };
                    sk == k
                })
                .copied()
                .collect();
            plot_group(&mut w, c, k, &list);
        }
        axes(&mut w, c, Group::Bar);
    } else {
        if !matches!(
            c.kind,
            ChartType::Column
                | ChartType::StackedColumn
                | ChartType::PercentColumn
                | ChartType::Bar
                | ChartType::StackedBar
                | ChartType::PercentBar
                | ChartType::Line
                | ChartType::LineMarkers
                | ChartType::StackedLine
                | ChartType::Area
                | ChartType::StackedArea
                | ChartType::Pie
                | ChartType::Doughnut
                | ChartType::Scatter
                | ChartType::Bubble
                | ChartType::Radar
                | ChartType::FilledRadar
                | ChartType::Stock
        ) {
            log::warn!("pptx: {} chart written as a column chart", c.kind.label());
        }
        plot_group(&mut w, c, main, &indexed);
        let g = group_of(main);
        if !matches!(g, Group::Pie | Group::Doughnut) {
            axes(&mut w, c, g);
        }
    }
    if let Some(f) = &c.plot_fill {
        sp_pr(&mut w, Some(f), None);
    }
    w.close("c:plotArea");
    if let Some(pos) = &c.legend {
        let p = match pos.as_str() {
            "b" | "t" | "l" | "r" | "tr" => pos.as_str(),
            _ => "r",
        };
        w.open0("c:legend");
        w.val("c:legendPos", p);
        w.val("c:overlay", u8::from(c.legend_overlay));
        tx_pr(&mut w, c);
        w.close("c:legend");
    }
    w.val("c:plotVisOnly", 1);
    w.val("c:dispBlanksAs", "gap");
    w.close("c:chart");
    if let Some(f) = &c.chart_fill {
        sp_pr(&mut w, Some(f), None);
    }
    tx_pr(&mut w, c);
    w.close("c:chartSpace");
    (w.finish(), false)
}

/// The relationship ids of an unchanged chart that [`write_kept`] writes back: external ones, and
/// internal ones whose part was kept.
fn kept_rel_ids(c: &Chart) -> std::collections::HashSet<&str> {
    let parts: std::collections::HashSet<String> = c.kept.parts.iter().filter(|p| kept_payload(&p.name)).map(|p| crate::opc::norm(&p.name)).collect();
    c.kept.rels.iter().filter(|r| r.external || parts.contains(&crate::opc::norm(&r.part))).map(|r| r.id.as_str()).collect()
}

/// A kept part that is written as a part of its own (rels parts are rewritten instead).
fn kept_payload(name: &str) -> bool {
    !name.to_ascii_lowercase().ends_with(".rels") && crate::opc::sidecar_part(name)
}

/// Drop `c:externalData` / `c:userShapes` (and the `cx:` ones) that point at a relationship that
/// is not written back: without the part PowerPoint repairs the file.
fn drop_unkept_refs(root: &mut crate::xml::El, c: &Chart) {
    let ids = kept_rel_ids(c);
    fn rec(e: &mut crate::xml::El, ids: &std::collections::HashSet<&str>, depth: usize) {
        if depth > crate::xml::MAX_DEPTH {
            return;
        }
        e.children.retain(|n| match n {
            crate::xml::Node::El(x) if x.is("externalData") || x.is("userShapes") => x.attr("r:id").is_some_and(|id| ids.contains(id)),
            _ => true,
        });
        for ch in e.elements_mut() {
            rec(ch, ids, depth + 1);
        }
    }
    rec(root, &ids, 0);
}

/// A fresh name for a kept part: `ppt/<folder>/keptN.<ext>`. The writer generates no `kept…`
/// names and `n` is unique within the package, so a kept part never lands on (or duplicates) a
/// part the writer makes, whatever the source file called it.
fn kept_name(original: &str, n: u32) -> String {
    let norm = crate::opc::norm(original);
    let area = norm.split('/').nth(1).unwrap_or("embeddings");
    let area = if matches!(area, "embeddings" | "charts" | "drawings" | "media") { area } else { "embeddings" };
    let file = crate::opc::file_of(&norm);
    let ext = file
        .rsplit_once('.')
        .map(|(_, e)| e)
        .filter(|e| !e.is_empty() && e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    format!("ppt/{area}/kept{n}.{ext}")
}

/// Write an unchanged chart's own relationships and the parts they reach. Each kept part is
/// written under a fresh name ([`kept_name`]) and the relationships between kept parts are
/// rewritten to those names; a relationship to anything that was not kept is dropped.
/// `n_kept` numbers kept parts across the package.
pub fn write_kept(pkg: &mut crate::opc::PackageOut, chart_name: &str, c: &Chart, n_kept: &mut u32) {
    use std::collections::HashMap;

    use crate::opc::{RelsOut, norm, relative, rels_path};

    if c.kept.rels.is_empty() {
        return;
    }
    // Original part name (normalized) → the name written.
    let mut names: HashMap<String, String> = HashMap::new();
    for p in c.kept.parts.iter().filter(|p| kept_payload(&p.name)) {
        let key = norm(&p.name);
        if names.contains_key(&key) {
            continue;
        }
        *n_kept += 1;
        let new = kept_name(&p.name, *n_kept);
        let ct = if !p.content_type.is_empty() { p.content_type.clone() } else { crate::mime_for(&p.name).to_string() };
        // An override per part: a kept part never sets the package-wide default for its extension.
        pkg.xml_part(&new, &ct, p.bytes.clone());
        names.insert(key, new);
    }
    let mut rels = RelsOut::default();
    for r in &c.kept.rels {
        if r.external {
            rels.items.push((r.id.clone(), r.rel_type.clone(), r.target.clone(), true));
        } else if let Some(new) = names.get(&norm(&r.part)) {
            rels.items.push((r.id.clone(), r.rel_type.clone(), relative(chart_name, new), false));
        } else {
            log::warn!("pptx: chart relationship {} has no kept part; dropped", r.id);
        }
    }
    pkg.rels(chart_name, &rels);
    let mut parts_by_norm = std::collections::HashMap::new();
    for p in &c.kept.parts {
        parts_by_norm.entry(norm(&p.name)).or_insert(p);
    }
    let mut seen_payloads = std::collections::HashSet::new();
    // The kept parts' own relationships, pointed at the new names.
    for p in c.kept.parts.iter().filter(|p| kept_payload(&p.name)) {
        let norm_name = norm(&p.name);
        if !seen_payloads.insert(norm_name.clone()) {
            continue;
        }
        let Some(new_source) = names.get(&norm_name) else { continue };
        let rp = norm(&rels_path(&p.name));
        let Some(rels_part) = parts_by_norm.get(&rp) else { continue };
        let mut out = RelsOut::default();
        for r in crate::opc::rels_from_bytes(&rels_part.bytes, &p.name).items {
            if r.external {
                out.items.push((r.id, r.ty, r.target_raw, true));
            } else if let Some(new) = names.get(&norm(&r.target)) {
                out.items.push((r.id, r.ty, relative(new_source, new), false));
            } else {
                log::warn!("pptx: relationship {} of kept chart part {} has no kept target; dropped", r.id, p.name);
            }
        }
        pkg.rels(new_source, &out);
    }
}

/// A `cx:tx` holding plain text.
fn cx_text(w: &mut W, t: &str) {
    w.open0("cx:tx");
    w.open0("cx:txData");
    w.open0("cx:v");
    w.text(t);
    w.close("cx:v");
    w.close("cx:txData");
    w.close("cx:tx");
}

/// A `cx:lvl` of points.
fn cx_level(w: &mut W, pts: &[String], format: Option<&str>) {
    let mut a = A::new().a("ptCount", pts.len());
    if let Some(f) = format {
        a = a.a("formatCode", f);
    }
    w.open("cx:lvl", a);
    for (i, p) in pts.iter().enumerate() {
        w.open("cx:pt", A::new().a("idx", i));
        w.text(p);
        w.close("cx:pt");
    }
    w.close("cx:lvl");
}

/// The `cx:layoutPr` of the region map series in a kept `chartEx` part.
fn kept_region_layout(raw: &str) -> Option<String> {
    let doc = crate::xml::parse(raw.as_bytes()).ok()?;
    let mut root = doc.root;
    crate::read::resolve_mc(&mut root, 0);
    let region = root.child("chart")?.child("plotArea")?.child("plotAreaRegion")?;
    let s = region.children_named("series").find(|s| s.attr("layoutId") == Some("regionMap"))?;
    let lp = s.child("layoutPr")?;
    // Written under our `cx` prefix; a part that bound chartex to another prefix is not copied.
    lp.name.starts_with("cx:").then(|| lp.to_xml())
}

/// Writes an Office 2016 chart from the model, as PowerPoint lays the part out: one `cx:data`
/// per series (categories and values), then the plot area with each series' layout, the axes
/// the layout uses, the title and the legend.
pub fn build_chartex(c: &Chart) -> Vec<u8> {
    let mut w = W::new();
    w.open("cx:chartSpace", A::new().a("xmlns:a", NS_A).a("xmlns:r", NS_R).a("xmlns:cx", NS_CX));
    w.open0("cx:chartData");
    for (i, s) in c.series.iter().enumerate() {
        w.open("cx:data", A::new().a("id", i));
        if !c.categories.is_empty() {
            w.open("cx:strDim", A::new().a("type", "cat"));
            cx_level(&mut w, &c.categories, None);
            for lv in &c.category_levels {
                cx_level(&mut w, lv, None);
            }
            w.close("cx:strDim");
        }
        let ty = match c.kind {
            ChartType::Treemap | ChartType::Sunburst => "size",
            ChartType::RegionMap => "colorVal",
            _ => "val",
        };
        w.open("cx:numDim", A::new().a("type", ty));
        let vals: Vec<String> = s.values.iter().map(|v| v.map(fmt_num).unwrap_or_default()).collect();
        cx_level(&mut w, &vals, Some(s.number_format.as_deref().unwrap_or("General")));
        w.close("cx:numDim");
        w.close("cx:data");
    }
    w.close("cx:chartData");
    w.open0("cx:chart");
    if let Some(t) = &c.title {
        w.open("cx:title", A::new().a("pos", "t").a("align", "ctr").a("overlay", "0"));
        cx_text(&mut w, t);
        w.close("cx:title");
    }
    w.open0("cx:plotArea");
    w.open0("cx:plotAreaRegion");
    let layout = match c.kind {
        ChartType::Waterfall => "waterfall",
        ChartType::Funnel => "funnel",
        ChartType::Treemap => "treemap",
        ChartType::Sunburst => "sunburst",
        ChartType::BoxWhisker => "boxWhisker",
        ChartType::RegionMap => "regionMap",
        _ => "clusteredColumn",
    };
    let one = !matches!(c.kind, ChartType::BoxWhisker);
    for (i, s) in c.series.iter().enumerate().take(if one { 1 } else { usize::MAX }) {
        w.open("cx:series", A::new().a("layoutId", layout).a("uniqueId", format!("{{DC00000{i}-0000-4000-8000-000000000000}}")));
        cx_text(&mut w, &s.name);
        if c.series_data_labels(s) {
            w.open("cx:dataLabels", A::new().a("pos", if c.kind == ChartType::Funnel { "ctr" } else { "outEnd" }));
            w.empty("cx:visibility", A::new().a("seriesName", "0").a("categoryName", "0").a("value", "1"));
            w.close("cx:dataLabels");
        }
        w.empty("cx:dataId", A::new().a("val", i));
        match c.kind {
            ChartType::Waterfall if !c.subtotals.is_empty() => {
                w.open0("cx:layoutPr");
                w.open0("cx:subtotals");
                for t in &c.subtotals {
                    w.empty("cx:idx", A::new().a("val", t));
                }
                w.close("cx:subtotals");
                w.close("cx:layoutPr");
            }
            ChartType::Histogram | ChartType::Pareto => {
                w.open0("cx:layoutPr");
                w.open("cx:binning", A::new().a("intervalClosed", "r"));
                if let Some(size) = c.bin_size {
                    w.empty("cx:binSize", A::new().a("val", fmt_num(size)));
                } else if let Some(n) = c.bin_count {
                    w.empty("cx:binCount", A::new().a("val", n));
                }
                w.close("cx:binning");
                w.close("cx:layoutPr");
            }
            ChartType::BoxWhisker => {
                w.open0("cx:layoutPr");
                w.empty("cx:visibility", A::new().a("meanLine", "0").a("meanMarker", "1").a("nonoutliers", "0").a("outliers", "1"));
                w.empty("cx:statistics", A::new().a("quartileMethod", if c.inclusive_quartiles { "inclusive" } else { "exclusive" }));
                w.close("cx:layoutPr");
            }
            ChartType::RegionMap => {
                // The map's geography (projection, region level and the cached boundaries
                // PowerPoint downloaded) is not modelled: carry it over from the part we read.
                if let Some(lp) = c.raw.as_deref().and_then(kept_region_layout) {
                    w.raw(&lp);
                }
            }
            _ => {}
        }
        w.close("cx:series");
    }
    if c.kind == ChartType::Pareto {
        w.open("cx:series", A::new().a("layoutId", "paretoLine").a("ownerIdx", "0").a("uniqueId", "{DC0000FF-0000-4000-8000-000000000000}"));
        w.empty("cx:axisId", A::new().a("val", "2"));
        w.close("cx:series");
    }
    w.close("cx:plotAreaRegion");
    if !matches!(c.kind, ChartType::Treemap | ChartType::Sunburst | ChartType::RegionMap) {
        w.open("cx:axis", A::new().a("id", "0"));
        w.empty("cx:catScaling", A::new().a("gapWidth", if matches!(c.kind, ChartType::Histogram | ChartType::Pareto) { "0" } else { "0.5" }));
        w.empty0("cx:tickLabels");
        w.close("cx:axis");
        if c.kind != ChartType::Funnel {
            w.open("cx:axis", A::new().a("id", "1"));
            w.empty0("cx:valScaling");
            if c.gridlines {
                w.empty0("cx:majorGridlines");
            }
            w.empty0("cx:tickLabels");
            w.close("cx:axis");
        }
        if c.kind == ChartType::Pareto {
            w.open("cx:axis", A::new().a("id", "2"));
            w.empty("cx:valScaling", A::new().a("max", "1").a("min", "0"));
            w.empty0("cx:units");
            w.empty0("cx:tickLabels");
            w.close("cx:axis");
        }
    }
    w.close("cx:plotArea");
    if let Some(pos) = &c.legend {
        w.empty("cx:legend", A::new().a("pos", pos.as_str()).a("align", "ctr").a("overlay", "0"));
    }
    w.close("cx:chart");
    w.close("cx:chartSpace");
    w.finish()
}

#[cfg(test)]
mod region_map_tests {
    use super::*;

    /// A region map part as PowerPoint writes it (our own numbers; the cached geography is a
    /// stand-in blob).
    const MAP: &str = r#"<cx:chartSpace xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"><cx:chartData><cx:data id="0"><cx:strDim type="cat"><cx:f>Sheet1!$A$2:$A$4</cx:f><cx:lvl ptCount="3"><cx:pt idx="0">Minsk</cx:pt><cx:pt idx="1">Brest</cx:pt><cx:pt idx="2">Gomel</cx:pt></cx:lvl></cx:strDim><cx:numDim type="colorVal"><cx:f>Sheet1!$B$2:$B$4</cx:f><cx:lvl ptCount="3" formatCode="General"><cx:pt idx="0">12</cx:pt><cx:pt idx="1">5</cx:pt><cx:pt idx="2">8</cx:pt></cx:lvl></cx:numDim></cx:data></cx:chartData><cx:chart><cx:title pos="t" align="ctr" overlay="0"/><cx:plotArea><cx:plotAreaRegion><cx:series layoutId="regionMap" uniqueId="{00000000-0000-4000-8000-000000000001}"><cx:tx><cx:txData><cx:f>Sheet1!$B$1</cx:f><cx:v>Sales</cx:v></cx:txData></cx:tx><cx:dataId val="0"/><cx:layoutPr><cx:geography cultureLanguage="en-US" cultureRegion="BY" attribution="Powered by Bing"><cx:geoCache provider="{E9337A44-BEBE-4D9F-B70C-5C5E7DAFC167}"><cx:binary>ZGVja2NyYWZ0LXRlc3Q=</cx:binary></cx:geoCache></cx:geography></cx:layoutPr></cx:series></cx:plotAreaRegion></cx:plotArea><cx:legend pos="r" align="min" overlay="0"/></cx:chart></cx:chartSpace>"#;

    #[test]
    fn region_maps_keep_their_geography_through_save_and_edit() {
        let c = crate::read::read_chartex_xml(MAP.as_bytes()).expect("a region map is read");
        assert_eq!(c.kind, ChartType::RegionMap);
        assert_eq!(c.categories, ["Minsk", "Brest", "Gomel"]);
        assert_eq!(c.series.first().map(|s| s.values.clone()), Some(vec![Some(12.0), Some(5.0), Some(8.0)]));
        // Unchanged: written back as read.
        let kept = String::from_utf8(chartex_xml(&c).unwrap_or_default()).unwrap_or_default();
        assert!(kept.contains("ZGVja2NyYWZ0LXRlc3Q=") && kept.contains("cultureRegion=\"BY\""), "{kept}");
        // Edited: rebuilt from the model, with the geography carried over.
        let mut e = c.clone();
        if let Some(s) = e.series.first_mut() {
            s.values[1] = Some(20.0);
        }
        let built = String::from_utf8(chartex_xml(&e).unwrap_or_default()).unwrap_or_default();
        assert!(built.contains("<cx:geoCache") && built.contains("ZGVja2NyYWZ0LXRlc3Q=") && built.contains("colorVal"), "{built}");
        assert!(!built.contains("<cx:axis"), "{built}");
        let back = crate::read::read_chartex_xml(built.as_bytes()).expect("the rebuilt part reads");
        assert_eq!(back.kind, ChartType::RegionMap);
        assert_eq!(back.series.first().and_then(|s| s.values.get(1).copied().flatten()), Some(20.0));
    }
}

#[cfg(test)]
mod tests {
    use deckcraft_color::{ColorTransform, Rgba};
    use deckcraft_model::ColorRef;
    use deckcraft_model::chart::{Chart, ChartType};

    /// The chart's text colour is written where PowerPoint keeps it (`c:txPr` on the axes, legend,
    /// data labels and chart, the title's run defaults) and read back as the same colour.
    #[test]
    fn chart_text_colour_round_trips() {
        let mut c = Chart::sample(ChartType::Column);
        c.title = Some("Revenue".into());
        c.legend = Some("b".into());
        c.data_labels = true;
        c.axis_titles = (Some("Quarter".into()), Some("EUR".into()));
        let tinted = ColorRef::scheme(deckcraft_model::SchemeSlot::Accent2).with(ColorTransform::LumMod(75000));
        c.text_color = Some(tinted.clone());
        let xml = String::from_utf8(super::chart_xml(&c).0).unwrap();
        // Axes (2), legend, data labels and the chart root; a title keeps it in its rich text.
        assert!(xml.matches("<c:txPr>").count() >= 5, "{xml}");
        assert!(xml.contains(r#"<a:schemeClr val="accent2"><a:lumMod val="75000"/></a:schemeClr>"#), "{xml}");
        assert!(xml.contains("<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr><a:solidFill>"), "{xml}");
        assert_eq!(crate::read::chart::read_chart_xml(xml.as_bytes()).unwrap().text_color, Some(tinted));
        // An RGB colour and no colour at all round-trip too.
        c.text_color = Some(ColorRef::rgb(Rgba::rgb(255, 255, 255)));
        let xml = super::chart_xml(&c).0;
        assert_eq!(crate::read::chart::read_chart_xml(&xml).unwrap().text_color, c.text_color);
        c.text_color = None;
        let xml = String::from_utf8(super::chart_xml(&c).0).unwrap();
        assert!(!xml.contains("<c:txPr>"), "{xml}");
        assert_eq!(crate::read::chart::read_chart_xml(xml.as_bytes()).unwrap().text_color, None);
    }

    /// An imported chart's `c:txPr` colour survives an edit that rebuilds the part from the model
    /// (the colour used to be read but never written, so a save after any edit lost it).
    #[test]
    fn imported_text_colour_survives_an_edit_and_a_save() {
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:val><c:numRef><c:numCache><c:ptCount val="1"/><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart><c:catAx><c:axId val="1"/><c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr><a:solidFill><a:schemeClr val="tx1"><a:lumMod val="65000"/><a:lumOff val="35000"/></a:schemeClr></a:solidFill></a:defRPr></a:pPr></a:p></c:txPr><c:crossAx val="2"/></c:catAx><c:valAx><c:axId val="2"/><c:crossAx val="1"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = crate::read::chart::read_chart_xml(xml.as_bytes()).unwrap();
        let grey = ColorRef::scheme(deckcraft_model::SchemeSlot::Tx1).with(ColorTransform::LumMod(65000)).with(ColorTransform::LumOff(35000));
        assert_eq!(c.text_color, Some(grey.clone()));
        // Unchanged: the part is written back as read.
        assert!(super::chart_xml(&c).1);
        // Edited (a title; the editor drops `raw` on any edit): rebuilt with the colour.
        let mut e = c.clone();
        e.title = Some("Units".into());
        e.raw = None;
        let (out, kept) = super::chart_xml(&e);
        assert!(!kept);
        let back = crate::read::chart::read_chart_xml(&out).unwrap();
        assert_eq!(back.text_color, Some(grey));
        assert_eq!(back.title.as_deref(), Some("Units"));
    }

    #[test]
    fn reversed_category_axis_round_trips() {
        let mut c = Chart::sample(ChartType::Bar);
        c.cat_reversed = true;
        let xml = super::chart_xml(&c).0;
        assert!(String::from_utf8_lossy(&xml).contains(r#"<c:orientation val="maxMin"/>"#));
        assert!(crate::read::chart::read_chart_xml(&xml).unwrap().cat_reversed);
        c.cat_reversed = false;
        assert!(!crate::read::chart::read_chart_xml(&super::chart_xml(&c).0).unwrap().cat_reversed);
    }
}
