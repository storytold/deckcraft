//! Label defaults and sparse point overrides, shared by classic charts and chartEx.
use super::chart::{label_run_props, manual_layout, simple_fill, simple_line};
use crate::xml::El;
use deckcraft_model::{
    chart::{DataLabel, Series},
    text::{Align, Paragraph, Run, RunKind, TextBody},
};

pub(super) fn body(e: &El) -> TextBody {
    let mut body = TextBody { body: e.child("bodyPr").map(super::dml::body_pr).unwrap_or_default(), ..Default::default() };
    let list_default =
        e.path(&["lstStyle", "lvl1pPr", "defRPr"]).or_else(|| e.path(&["lstStyle", "defPPr", "defRPr"])).map(label_run_props).unwrap_or_default();
    let mut budget = 4096;
    for p in e.children_named("p").take(256) {
        let mut para = Paragraph::default();
        para.props.align = p.child("pPr").and_then(|e| e.attr("algn")).and_then(Align::from_xml);
        para.def_props = p.path(&["pPr", "defRPr"]).map(label_run_props).unwrap_or_default();
        para.def_props.inherit(&list_default);
        para.end_props = p.child("endParaRPr").map(label_run_props).unwrap_or_default();
        for run in p.elements().filter(|e| matches!(e.local(), "r" | "fld" | "br")).take(budget) {
            let mut props = run.child("rPr").map(label_run_props).unwrap_or_default();
            props.inherit(&para.def_props);
            para.runs.push(Run {
                text: run.child("t").map(El::text).unwrap_or_default(),
                props,
                kind: if run.is("br") { RunKind::Break } else { RunKind::Text },
            });
        }
        if para.runs.is_empty() {
            // Office uses the end mark as the style of generated numeric label text.
            para.def_props.inherit(&para.end_props);
        }
        budget = budget.saturating_sub(para.runs.len());
        body.paragraphs.push(para);
        if budget == 0 {
            break;
        }
    }
    body
}
fn properties(e: &El, cx: bool) -> DataLabel {
    let flag = |classic: &str, extended: &str| {
        if cx { e.child("visibility").and_then(|v| v.bool(extended)) } else { e.child(classic).map(|v| v.bool("val").unwrap_or(true)) }
    };
    DataLabel {
        hidden: e.child("delete").map(|v| v.bool("val").unwrap_or(true)),
        text: e.path(&["tx", "rich"]).map(body).or_else(|| e.child("tx").and_then(|t| t.find("v")).map(|v| TextBody::from_text(&v.text()))),
        style: e.child("txPr").map(body),
        position: if cx { e.attr("pos") } else { e.child_val("dLblPos") }.map(str::to_string),
        layout: manual_layout(Some(e)),
        number_format: e.child("numFmt").and_then(|n| n.attr("formatCode")).map(str::to_string),
        show_value: flag("showVal", "value"),
        show_category: flag("showCatName", "categoryName"),
        show_series: flag("showSerName", "seriesName"),
        show_percent: flag("showPercent", "percent"),
        show_bubble_size: flag("showBubbleSize", "bubbleSize"),
        separator: e.child("separator").map(El::text),
        fill: simple_fill(e.child("spPr")),
        line: simple_line(e.child("spPr")),
    }
}
pub(super) fn read(series: &mut Series, labels: Option<&El>, cx: bool) {
    let Some(labels) = labels else { return };
    let mut defaults = properties(labels, cx);
    defaults.inherit(&series.label_defaults);
    series.label_defaults = defaults;
    for e in labels.children_named(if cx { "dataLabel" } else { "dLbl" }).take(4096) {
        let i = if cx { e.u32("idx") } else { e.child("idx").and_then(|i| i.u32("val")) };
        if let Some(i) = i.filter(|i| *i < 100_000) {
            let mut point = properties(e, cx);
            if let Some(parent) = series.point_labels.get(&(i as usize)) {
                point.inherit(parent);
            }
            series.point_labels.insert(i as usize, point);
        }
    }
    for e in labels.children_named("dataLabelHidden").take(4096) {
        if let Some(i) = e.u32("idx").filter(|i| *i < 100_000) {
            series.point_labels.entry(i as usize).or_default().hidden = Some(true);
        }
    }
}
