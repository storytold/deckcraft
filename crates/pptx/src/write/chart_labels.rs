//! Rebuild editable data labels rather than flattening their rich text into numbers.
use super::chart::{chart_rpr, layout, sp_pr_tag};
use crate::xml::{A, W};
use deckcraft_model::{
    chart::{Chart, DataLabel, Series},
    text::{RunKind, TextBody},
};
fn body(w: &mut W, tag: &str, body: &TextBody) {
    w.open0(tag);
    super::dml::body_pr(w, &body.body);
    w.empty0("a:lstStyle");
    for p in body.paragraphs.iter().take(256) {
        w.open0("a:p");
        w.open("a:pPr", A::new().o("algn", p.props.align.map(|a| a.xml())));
        chart_rpr(w, "a:defRPr", &p.def_props);
        w.close("a:pPr");
        for run in p.runs.iter().take(4096) {
            let tag = if run.kind == RunKind::Break { "a:br" } else { "a:r" };
            w.open0(tag);
            chart_rpr(w, "a:rPr", &run.props);
            if tag == "a:r" {
                w.elt("a:t", &run.text);
            }
            w.close(tag);
        }
        w.close("a:p");
    }
    w.close(tag);
}
fn properties(w: &mut W, l: &DataLabel, cx: bool, point: bool) {
    let prefix = if cx { "cx" } else { "c" };
    if !cx {
        if let Some(hidden) = l.hidden {
            w.val("c:delete", u8::from(hidden));
            if hidden {
                return;
            }
        }
        if point {
            layout(w, l.layout.as_ref(), false);
            if let Some(text) = &l.text {
                w.open0("c:tx");
                body(w, "c:rich", text);
                w.close("c:tx");
            }
        }
    }
    if let Some(code) = &l.number_format {
        w.empty(&format!("{prefix}:numFmt"), A::new().a("formatCode", code).a("sourceLinked", 0));
    }
    sp_pr_tag(w, l.fill.as_ref(), l.line.as_ref(), &format!("{prefix}:spPr"));
    if let Some(style) = l.style.as_ref().or(l.text.as_ref()) {
        body(w, &format!("{prefix}:txPr"), style);
    }
    if cx {
        w.empty(
            "cx:visibility",
            A::new()
                .o("seriesName", l.show_series.map(u8::from))
                .o("categoryName", l.show_category.map(u8::from))
                .o("value", l.show_value.map(u8::from)),
        );
    } else {
        if let Some(pos) = &l.position {
            w.val("c:dLblPos", pos);
        }
        w.val("c:showLegendKey", 0);
        for (name, val) in [
            ("showVal", l.show_value),
            ("showCatName", l.show_category),
            ("showSerName", l.show_series),
            ("showPercent", l.show_percent),
            ("showBubbleSize", l.show_bubble_size),
        ] {
            if let Some(val) = val {
                w.val(&format!("c:{name}"), u8::from(val));
            }
        }
    }
    if let Some(separator) = &l.separator {
        w.elt(&format!("{prefix}:separator"), separator);
    }
}
pub(super) fn write(w: &mut W, c: &Chart, s: &Series, cx: bool) {
    let mut defaults = s.label_defaults.clone();
    defaults.show_value.get_or_insert(c.series_data_labels(s));
    defaults.show_category.get_or_insert(false);
    defaults.show_series.get_or_insert(false);
    defaults.show_percent.get_or_insert(false);
    defaults.show_bubble_size.get_or_insert(false);
    defaults.number_format.get_or_insert_with(|| s.number_format.clone().unwrap_or_else(|| "General".into()));
    if cx {
        w.open("cx:dataLabels", A::new().o("pos", defaults.position.as_deref()));
        properties(w, &defaults, true, false);
        for (&i, l) in s.point_labels.iter().take(4096).filter(|(_, l)| l.hidden != Some(true)) {
            w.open("cx:dataLabel", A::new().a("idx", i).o("pos", l.position.as_deref()));
            properties(w, l, true, true);
            w.close("cx:dataLabel");
        }
        for (&i, _) in s.point_labels.iter().take(4096).filter(|(_, l)| l.hidden == Some(true)) {
            w.empty("cx:dataLabelHidden", A::new().a("idx", i));
        }
        w.close("cx:dataLabels");
    } else {
        w.open0("c:dLbls");
        for (&i, l) in s.point_labels.iter().take(4096) {
            w.open0("c:dLbl");
            w.val("c:idx", i);
            let mut effective = l.clone();
            effective.inherit(&defaults);
            properties(w, &effective, false, true);
            w.close("c:dLbl");
        }
        properties(w, &defaults, false, false);
        w.close("c:dLbls");
    }
}
