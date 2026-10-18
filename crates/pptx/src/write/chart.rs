//! Chart parts: a `c:chartSpace` with cached categories and values, and the workbook that holds
//! the chart data (what PowerPoint opens for Edit Data).
//!
//! - An unchanged chart keeps its original part, its workbook and the other parts it points at.
//! - An imported chart whose data still fits its original ranges ([`source`]) is written from the
//!   model with its original formulas. Its workbook is kept byte for byte when only the
//!   presentation changed (title, type, legend…); when the data changed, only the cells that
//!   changed are rewritten in it.
//! - Otherwise (a new chart, series or points added or removed…) both are written from the model,
//!   the workbook laid out where the part's formulas point.

use std::collections::BTreeMap;

use deckcraft_model::chart::{Chart, ChartType, Series, Trendline, TrendlineType};
use deckcraft_model::style::Fill;

use super::xlsx::{Edit, Value};
use crate::opc::{NS_A, NS_C, NS_R, PackageOut, RelsOut};

/// The Office 2016 chart namespace.
pub const NS_CX: &str = "http://schemas.microsoft.com/office/drawing/2014/chartex";
use crate::xml::{A, W};

/// Content type of an embedded `.xlsx` workbook.
pub const CT_XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const NS_SSML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const CT_WORKBOOK: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const CT_WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
/// Rows a worksheet can hold, header row excluded.
const MAX_ROWS: usize = 1_048_575;

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

/// Columns (0-based) of one series in the data sheet. Data starts on row 2; the series name sits
/// on row 1 above its values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Cols {
    /// Categories, or X values for scatter and bubble charts.
    x: usize,
    /// Values.
    y: usize,
    /// Bubble sizes.
    size: Option<usize>,
}

/// X values of a scatter or bubble series (1, 2, 3… when it has none).
fn xs(s: &Series) -> Vec<Option<f64>> {
    if s.x_values.is_empty() { (1..=s.values.len()).map(|v| Some(v as f64)).collect() } else { s.x_values.clone() }
}

/// Bubble sizes (all 1 when the series has none).
fn sizes(s: &Series) -> Vec<Option<f64>> {
    if s.sizes.is_empty() { s.values.iter().map(|_| Some(1.0)).collect() } else { s.sizes.clone() }
}

/// Where each series sits in the data sheet: categories (or shared X values) in column A and one
/// column per series, the layout of a new chart's sheet in PowerPoint. Scatter series with
/// different X values, and bubble series, get columns of their own so no two series share cells.
fn layout(c: &Chart) -> Vec<Cols> {
    let n = c.series.len();
    let shared_x = c.series.windows(2).all(|w| match w {
        [a, b] => xs(a) == xs(b),
        _ => true,
    });
    match c.kind {
        ChartType::Scatter if shared_x => (0..n).map(|i| Cols { x: 0, y: i + 1, size: None }).collect(),
        ChartType::Scatter => (0..n).map(|i| Cols { x: 2 * i, y: 2 * i + 1, size: None }).collect(),
        ChartType::Bubble if shared_x => (0..n).map(|i| Cols { x: 0, y: 2 * i + 1, size: Some(2 * i + 2) }).collect(),
        ChartType::Bubble => (0..n).map(|i| Cols { x: 3 * i, y: 3 * i + 1, size: Some(3 * i + 2) }).collect(),
        _ => (0..n).map(|i| Cols { x: 0, y: i + 1, size: None }).collect(),
    }
}

/// `Sheet1!$B$2:$B$5`: rows 2 to `last` of column `c`.
fn range(c: usize, last: usize) -> String {
    format!("Sheet1!${0}$2:${0}${last}", col(c))
}

/// The workbook formulas of one series in the chart part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Refs {
    name: String,
    /// Categories, or X values.
    x: String,
    y: String,
    size: String,
}

/// Formulas into the workbook [`workbook_xlsx`] writes.
fn generated_refs(c: &Chart) -> Vec<Refs> {
    c.series
        .iter()
        .zip(layout(c))
        .map(|(s, k)| {
            let last = c.categories.len().max(s.values.len()) + 1;
            Refs { name: format!("Sheet1!${}$1", col(k.y)), x: range(k.x, last), y: range(k.y, last), size: range(k.size.unwrap_or(k.y + 1), last) }
        })
        .collect()
}

/// An edited chart whose data still fits where its workbook holds it: the formulas of the
/// imported chart part, and the cells to update for the data that changed since (none when only
/// the title, type, legend or other options changed).
pub struct Source {
    pub refs: Vec<Refs>,
    pub edits: Vec<Edit>,
}

/// Cells of `formula` whose number changed.
fn number_edits(edits: &mut Vec<Edit>, formula: &str, now: &[Option<f64>], was: &[Option<f64>]) {
    for (index, (a, b)) in now.iter().zip(was).enumerate() {
        if a != b {
            let value = a.filter(|v| v.is_finite()).map_or(Value::Blank, Value::Number);
            edits.push(Edit { formula: formula.to_string(), index, value });
        }
    }
}

/// [`Source`] of a chart read from a PPTX. `None` when the data no longer fits the original
/// ranges: series or points added or removed, a category chart turned into a scatter or bubble
/// chart (or back), data the chart part held as literals rather than formulas. The chart then
/// gets a workbook of its own, as a new chart does.
pub fn source(c: &Chart) -> Option<Source> {
    let src = c.source.as_ref()?;
    let xy = matches!(c.kind, ChartType::Scatter | ChartType::Bubble);
    if src.xy != xy || src.series.len() != c.series.len() || (!xy && c.categories.len() != src.categories.len()) {
        return None;
    }
    let mut refs = vec![];
    let mut edits = vec![];
    for (s, o) in c.series.iter().zip(&src.series) {
        let r = Refs {
            name: o.name_ref.clone()?,
            x: o.categories_ref.clone()?,
            y: o.values_ref.clone()?,
            size: if c.kind == ChartType::Bubble { o.sizes_ref.clone()? } else { o.sizes_ref.clone().unwrap_or_default() },
        };
        if s.values.len() != o.values.len() {
            return None;
        }
        if s.name != o.name {
            edits.push(Edit { formula: r.name.clone(), index: 0, value: Value::Text(s.name.clone()) });
        }
        number_edits(&mut edits, &r.y, &s.values, &o.values);
        if xy {
            if s.x_values.len() != o.x_values.len() {
                return None;
            }
            number_edits(&mut edits, &r.x, &s.x_values, &o.x_values);
        }
        if c.kind == ChartType::Bubble {
            if s.sizes.len() != o.sizes.len() {
                return None;
            }
            number_edits(&mut edits, &r.size, &s.sizes, &o.sizes);
        }
        refs.push(r);
    }
    if !xy {
        let mut ranges: Vec<&str> = refs.iter().map(|r| r.x.as_str()).collect();
        ranges.sort_unstable();
        ranges.dedup();
        for (index, (a, b)) in c.categories.iter().zip(&src.categories).enumerate() {
            if a != b {
                for f in &ranges {
                    edits.push(Edit { formula: f.to_string(), index, value: Value::Text(a.clone()) });
                }
            }
        }
    }
    Some(Source { refs, edits })
}

fn simple_fill(w: &mut W, f: &Fill) {
    match f {
        Fill::None => w.empty0("a:noFill"),
        Fill::Solid { color } => super::dml::solid(w, color),
        Fill::Gradient(_) | Fill::Picture(_) | Fill::Pattern(_) | Fill::Group | Fill::Background => {
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

fn rich(w: &mut W, text: &str) {
    w.open0("c:tx");
    w.open0("c:rich");
    w.empty0("a:bodyPr");
    w.empty0("a:lstStyle");
    for line in text.split('\n') {
        w.open0("a:p");
        w.open0("a:r");
        w.elt("a:t", line);
        w.close("a:r");
        w.close("a:p");
    }
    w.close("c:rich");
    w.close("c:tx");
}

fn title(w: &mut W, text: &str) {
    w.open0("c:title");
    rich(w, text);
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

fn series(w: &mut W, c: &Chart, s: &Series, i: usize, r: &Refs, kind: ChartType, g: Group) {
    w.open0("c:ser");
    w.val("c:idx", i);
    w.val("c:order", i);
    w.open0("c:tx");
    str_cache(w, &r.name, std::slice::from_ref(&s.name));
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
    if g == Group::Pie || g == Group::Doughnut || g == Group::Bar {
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
    if let Some(d) = s.data_labels {
        data_labels(w, d);
    }
    // After labels, before the category and value caches (ECMA-376 series content order).
    if matches!(g, Group::Bar | Group::Line | Group::Area | Group::Scatter) {
        for t in s.trendlines.iter().take(16) {
            trendline(w, t);
        }
    }
    if let Some(e) = &s.error_bars {
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
    if matches!(g, Group::Scatter | Group::Bubble) {
        w.open0("c:xVal");
        num_cache(w, &r.x, &xs(s));
        w.close("c:xVal");
        w.open0("c:yVal");
        num_cache(w, &r.y, &s.values);
        w.close("c:yVal");
        if g == Group::Bubble {
            w.open0("c:bubbleSize");
            num_cache(w, &r.size, &sizes(s));
            w.close("c:bubbleSize");
            w.val("c:bubble3D", 0);
        } else {
            w.val("c:smooth", if s.smooth { 1 } else { 0 });
        }
    } else {
        w.open0("c:cat");
        str_cache(w, &r.x, &c.categories);
        w.close("c:cat");
        w.open0("c:val");
        num_cache(w, &r.y, &s.values);
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

fn data_labels(w: &mut W, show: bool) {
    w.open0("c:dLbls");
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
            title(w, t);
        }
        w.empty("c:numFmt", A::new().a("formatCode", "General").a("sourceLinked", 1));
        w.val("c:majorTickMark", "out");
        w.val("c:minorTickMark", "none");
        w.val("c:tickLblPos", "nextTo");
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
            title(w, t);
        }
        w.empty("c:numFmt", A::new().a("formatCode", "General").a("sourceLinked", 1));
        w.val("c:majorTickMark", "none");
        w.val("c:minorTickMark", "none");
        w.val("c:tickLblPos", "nextTo");
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
        title(w, t);
    }
    match c.y_format.as_deref().filter(|f| !f.is_empty()) {
        Some(f) => w.empty("c:numFmt", A::new().a("formatCode", f).a("sourceLinked", 0)),
        None => w.empty("c:numFmt", A::new().a("formatCode", if c.kind.percent() { "0%" } else { "General" }).a("sourceLinked", 1)),
    }
    w.val("c:majorTickMark", "none");
    w.val("c:minorTickMark", "none");
    w.val("c:tickLblPos", "nextTo");
    w.val("c:crossAx", AX_CAT);
    w.val("c:crosses", "autoZero");
    w.val("c:crossBetween", if g == Group::Area || g == Group::Scatter { "midCat" } else { "between" });
    w.close("c:valAx");
}

fn plot_group(w: &mut W, c: &Chart, kind: ChartType, list: &[(usize, &Series)], refs: &[Refs]) {
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
        series(w, c, s, *i, refs.get(*i).unwrap_or(&Refs::default()), kind, g);
    }
    if c.data_labels {
        data_labels(w, true);
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
/// A part written from the model points at no workbook.
#[cfg(test)]
pub fn chart_xml(c: &Chart) -> (Vec<u8>, bool) {
    match kept_chart_xml(c) {
        Some(x) => (x, true),
        None => (build_chart_xml(c, None, None), false),
    }
}

/// The original chart part when the chart is unchanged since it was read: its formulas,
/// unmodelled options and kept parts (see [`write_kept`]) all still apply.
pub fn kept_chart_xml(c: &Chart) -> Option<Vec<u8>> {
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
                return Some(w.finish());
            }
        }
    }
    None
}

/// The chart part written from the model. `workbook` is the id of the relationship to the data
/// workbook, when one is written; `refs` are the series formulas into it, those of [`source`] or,
/// when `None`, those of the workbook [`workbook_xlsx`] writes.
pub fn build_chart_xml(c: &Chart, workbook: Option<&str>, refs: Option<&[Refs]>) -> Vec<u8> {
    let refs = refs.map(<[Refs]>::to_vec).unwrap_or_else(|| generated_refs(c));
    let mut w = W::new();
    w.open("c:chartSpace", A::new().a("xmlns:c", NS_C).a("xmlns:a", NS_A).a("xmlns:r", NS_R));
    w.val("c:date1904", 0);
    w.val("c:roundedCorners", 0);
    w.val("c:style", c.style.clamp(1, 48));
    w.open0("c:chart");
    match &c.title {
        Some(t) => {
            title(&mut w, t);
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
            plot_group(&mut w, c, k, &list, &refs);
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
        plot_group(&mut w, c, main, &indexed, &refs);
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
        w.close("c:legend");
    }
    w.val("c:plotVisOnly", 1);
    w.val("c:dispBlanksAs", "gap");
    w.close("c:chart");
    if let Some(f) = &c.chart_fill {
        sp_pr(&mut w, Some(f), None);
    }
    if let Some(rid) = workbook {
        w.open("c:externalData", A::new().a("r:id", rid));
        w.val("c:autoUpdate", 0);
        w.close("c:externalData");
    }
    w.close("c:chartSpace");
    w.finish()
}

enum Cell {
    Text(String),
    Num(f64),
}

fn put_numbers(cells: &mut BTreeMap<(usize, usize), Cell>, column: usize, values: &[Option<f64>]) {
    for (r, v) in values.iter().enumerate().take(MAX_ROWS) {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            cells.insert((r + 2, column), Cell::Num(v));
        }
    }
}

const STYLES_XML: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
    "<fonts count=\"1\"><font><sz val=\"11\"/></font></fonts>",
    "<fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>",
    "<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>",
    "<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>",
    "<cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs>",
    "<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles>",
    "</styleSheet>"
);

/// A workbook holding the chart data on `Sheet1`, where the formulas of [`build_chart_xml`] point (see
/// [`layout`]), so PowerPoint's Edit Data opens the chart's own numbers. `None` for a chart
/// without series.
pub fn workbook_xlsx(c: &Chart) -> Option<Vec<u8>> {
    if c.series.is_empty() {
        return None;
    }
    let xy = matches!(c.kind, ChartType::Scatter | ChartType::Bubble);
    let mut cells: BTreeMap<(usize, usize), Cell> = BTreeMap::new();
    for (s, k) in c.series.iter().zip(layout(c)) {
        if !s.name.is_empty() {
            cells.insert((1, k.y), Cell::Text(s.name.clone()));
        }
        put_numbers(&mut cells, k.y, &s.values);
        if xy {
            put_numbers(&mut cells, k.x, &xs(s));
        }
        if let Some(column) = k.size {
            put_numbers(&mut cells, column, &sizes(s));
        }
    }
    if !xy {
        for (r, cat) in c.categories.iter().enumerate().take(MAX_ROWS) {
            if !cat.is_empty() {
                cells.insert((r + 2, 0), Cell::Text(cat.clone()));
            }
        }
    }

    // Rows in order, cells in column order within a row, as SpreadsheetML requires.
    let mut sheet = W::new();
    sheet.open("worksheet", A::new().a("xmlns", NS_SSML));
    sheet.open0("sheetData");
    let mut row = 0;
    for ((r, column), cell) in &cells {
        if *r != row {
            if row != 0 {
                sheet.close("row");
            }
            sheet.open("row", A::new().a("r", r));
            row = *r;
        }
        let at = format!("{}{r}", col(*column));
        match cell {
            Cell::Text(t) => {
                sheet.open("c", A::new().a("r", at).a("t", "inlineStr"));
                sheet.open0("is");
                sheet.open("t", A::new().a("xml:space", "preserve"));
                sheet.text(t);
                sheet.close("t");
                sheet.close("is");
                sheet.close("c");
            }
            Cell::Num(v) => {
                sheet.open("c", A::new().a("r", at));
                sheet.elt("v", &fmt_num(*v));
                sheet.close("c");
            }
        }
    }
    if row != 0 {
        sheet.close("row");
    }
    sheet.close("sheetData");
    sheet.close("worksheet");

    let mut rels = RelsOut::default();
    let sheet_rid = rels.add("worksheet", "worksheets/sheet1.xml", false);
    rels.add("styles", "styles.xml", false);
    let mut book = W::new();
    book.open("workbook", A::new().a("xmlns", NS_SSML).a("xmlns:r", NS_R));
    book.open0("sheets");
    book.empty("sheet", A::new().a("name", "Sheet1").a("sheetId", 1).a("r:id", sheet_rid));
    book.close("sheets");
    book.close("workbook");

    let mut pkg = PackageOut::default();
    pkg.xml_part("xl/workbook.xml", CT_WORKBOOK, book.finish());
    pkg.rels("xl/workbook.xml", &rels);
    pkg.xml_part("xl/worksheets/sheet1.xml", CT_WORKSHEET, sheet.finish());
    pkg.xml_part("xl/styles.xml", CT_STYLES, STYLES_XML.as_bytes().to_vec());
    let mut root = RelsOut::default();
    root.add("officeDocument", "xl/workbook.xml", false);
    pkg.rels("", &root);
    pkg.finish().ok()
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
    // The kept parts' own relationships, pointed at the new names.
    for p in c.kept.parts.iter().filter(|p| kept_payload(&p.name)) {
        let Some(new_source) = names.get(&norm(&p.name)) else { continue };
        let rp = norm(&rels_path(&p.name));
        let Some(rels_part) = c.kept.parts.iter().find(|q| norm(&q.name) == rp) else { continue };
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
        if s.data_labels.unwrap_or(c.data_labels) {
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
    use deckcraft_model::chart::{Chart, ChartType};

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

#[cfg(test)]
mod workbook_tests {
    use super::*;

    /// Cells of the workbook's first sheet by reference (`B2`), as written (number or text).
    fn sheet_cells(xlsx: &[u8]) -> BTreeMap<String, String> {
        let pkg = crate::opc::Package::open(xlsx).unwrap();
        let book = pkg.rels("").first_of("officeDocument").unwrap().target.clone();
        let sheet = pkg.rels(&book).first_of("worksheet").unwrap().target.clone();
        let doc = pkg.xml(&sheet).unwrap();
        let mut found = vec![];
        doc.root.find_all("c", &mut found);
        found
            .into_iter()
            .map(|c| {
                let v = c.child("v").map(|v| v.text()).or_else(|| c.find("t").map(|t| t.text())).unwrap_or_default();
                (c.attr("r").unwrap().to_string(), v)
            })
            .collect()
    }

    /// The cells a one-column formula covers: `Sheet1!$B$2:$B$5` → B2, B3, B4, B5.
    fn formula_cells(f: &str) -> Vec<String> {
        let refs = f.strip_prefix("Sheet1!").unwrap();
        let cell = |r: &str| {
            let r = r.replace('$', "");
            let split = r.find(|ch: char| ch.is_ascii_digit()).unwrap();
            (r[..split].to_string(), r[split..].parse::<usize>().unwrap())
        };
        let (a, b) = refs.split_once(':').unwrap_or((refs, refs));
        let ((ca, ra), (cb, rb)) = (cell(a), cell(b));
        assert_eq!(ca, cb, "{f}");
        (ra..=rb).map(|r| format!("{ca}{r}")).collect()
    }

    /// Every cached value of the chart part is in the workbook cell its formula points to.
    fn assert_formulas_match(c: &Chart) {
        let xml = build_chart_xml(c, Some("rId1"), None);
        let book = sheet_cells(&workbook_xlsx(c).unwrap());
        let doc = crate::xml::parse(&xml).unwrap();
        assert!(doc.root.child("externalData").and_then(|e| e.attr("r:id")) == Some("rId1"));
        let mut refs = vec![];
        doc.root.find_all("strRef", &mut refs);
        doc.root.find_all("numRef", &mut refs);
        assert!(!refs.is_empty());
        for r in refs {
            let cells = formula_cells(&r.child("f").unwrap().text());
            let mut pts = vec![];
            r.find_all("pt", &mut pts);
            for pt in pts {
                let i = pt.u32("idx").unwrap() as usize;
                let cached = pt.child("v").unwrap().text();
                let at = &cells[i];
                assert_eq!(book.get(at).map(String::as_str).unwrap_or(""), cached, "{:?}: {at}", c.kind);
            }
        }
    }

    #[test]
    fn workbook_holds_the_cached_data() {
        for kind in
            [ChartType::Column, ChartType::StackedBar, ChartType::Line, ChartType::Pie, ChartType::Scatter, ChartType::Bubble, ChartType::Radar]
        {
            assert_formulas_match(&Chart::sample(kind));
        }
        let mut combo = Chart::sample(ChartType::Column);
        combo.kind = ChartType::Combo;
        combo.series[1].kind = Some(ChartType::Line);
        assert_formulas_match(&combo);
        // Scatter series with their own X values, and several bubble series, never share cells.
        let mut scatter = Chart::sample(ChartType::Scatter);
        scatter.series[1].x_values = vec![Some(10.0), Some(20.0), Some(30.0), Some(40.0)];
        assert_formulas_match(&scatter);
        let mut bubbles = Chart::sample(ChartType::Bubble);
        bubbles.series = Chart::sample(ChartType::Column).series;
        bubbles.series[0].sizes = vec![Some(3.0), Some(1.0), Some(2.0), Some(5.0)];
        assert_formulas_match(&bubbles);
        bubbles.series[2].x_values = vec![Some(0.5), Some(1.5), Some(2.5), Some(3.5)];
        assert_formulas_match(&bubbles);
    }

    #[test]
    fn workbook_layout_is_the_familiar_one() {
        let c = Chart::sample(ChartType::Column);
        let book = sheet_cells(&workbook_xlsx(&c).unwrap());
        assert_eq!(book.get("A2").map(String::as_str), Some("Q1"));
        assert_eq!(book.get("B1").map(String::as_str), Some("Series 1"));
        assert_eq!(book.get("D5").map(String::as_str), Some("5.1"));
        assert!(!book.contains_key("A1"));
        assert!(workbook_xlsx(&Chart::default()).is_none());
    }

    #[test]
    fn unchanged_chart_keeps_its_part() {
        let first = build_chart_xml(&Chart::sample(ChartType::Column), Some("rId7"), None);
        // The chart as an import leaves it: read back from the part, which it keeps.
        let doc = crate::xml::parse(&first).unwrap();
        let mut read = crate::read::chart_from_el(&doc.root).unwrap();
        read.raw = Some(String::from_utf8(first.clone()).unwrap());
        assert!(kept_chart_xml(&read).is_some());
        // Edited data: the part is rewritten from the model.
        let mut edited = read.clone();
        edited.series[0].values[0] = Some(99.0);
        assert!(kept_chart_xml(&edited).is_none());
    }
}
