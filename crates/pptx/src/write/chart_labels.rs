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
        let pp = &p.props;
        let emu = |v: f64| (v.clamp(-1e6, 1e6) * 12700.).round() as i64;
        w.open(
            "a:pPr",
            A::new()
                .o("algn", pp.align.map(|a| a.xml()))
                .o("lvl", (p.level > 0).then_some(p.level))
                .o("marL", pp.margin_left.map(emu))
                .o("marR", pp.margin_right.map(emu))
                .o("indent", pp.indent.map(emu))
                .o("defTabSz", pp.default_tab.map(emu))
                .o("rtl", pp.rtl.map(u8::from))
                .o("fontAlgn", pp.font_align.as_deref())
                .o("eaLnBrk", pp.east_asian_line_break.map(u8::from))
                .o("latinLnBrk", pp.latin_line_break.map(u8::from))
                .o("hangingPunct", pp.hanging_punct.map(u8::from)),
        );
        for (tag, spacing) in [("a:lnSpc", pp.line_spacing), ("a:spcBef", pp.space_before), ("a:spcAft", pp.space_after)] {
            if let Some(spacing) = spacing {
                let (kind, value) = match spacing {
                    deckcraft_model::text::Spacing::Pct(v) => ("a:spcPct", v.clamp(0., 100.) * 100000.),
                    deckcraft_model::text::Spacing::Pts(v) => ("a:spcPts", v.clamp(0., 10000.) * 100.),
                };
                if value.is_finite() {
                    w.open0(tag);
                    w.val(kind, value.round() as i64);
                    w.close(tag);
                }
            }
        }
        if let Some(tabs) = &pp.tabs {
            w.open0("a:tabLst");
            for tab in tabs.iter().take(64) {
                w.empty("a:tab", A::new().a("pos", emu(tab.pos)).a("algn", &tab.align));
            }
            w.close("a:tabLst");
        }
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
        // Native generated chart labels inherit their end-mark properties, even when
        // defRPr is present. Materialize them rather than losing size/fill in PowerPoint.
        let mut end = p.end_props.clone();
        end.inherit(&p.def_props);
        chart_rpr(w, "a:endParaRPr", &end);
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
            let mut effective = l.clone();
            effective.inherit(&defaults);
            properties(w, &effective, true, true);
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
