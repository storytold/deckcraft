//! Label defaults and sparse point overrides, shared by classic charts and chartEx.
use super::chart::{label_run_props, manual_layout, simple_fill, simple_line};
use crate::xml::El;
use deckcraft_model::{
    chart::{DataLabel, Series},
    text::{Align, ParaProps, Paragraph, Run, RunKind, Spacing, TabStop, TextBody},
};

fn paragraph_props(e: &El) -> ParaProps {
    let pt = |attr| e.f64(attr).filter(|v| v.is_finite()).map(|v| (v / 12700.).clamp(-1e6, 1e6));
    let spacing = |tag| {
        e.child(tag).and_then(|s| {
            s.child("spcPct").and_then(|s| s.f64("val")).filter(|v| v.is_finite()).map(|v| Spacing::Pct((v / 100000.).clamp(0., 100.))).or_else(
                || s.child("spcPts").and_then(|s| s.f64("val")).filter(|v| v.is_finite()).map(|v| Spacing::Pts((v / 100.).clamp(0., 10000.))),
            )
        })
    };
    ParaProps {
        align: e.attr("algn").and_then(Align::from_xml),
        margin_left: pt("marL"),
        margin_right: pt("marR"),
        indent: pt("indent"),
        default_tab: pt("defTabSz"),
        rtl: e.bool("rtl"),
        font_align: e.attr("fontAlgn").map(str::to_string),
        east_asian_line_break: e.bool("eaLnBrk"),
        latin_line_break: e.bool("latinLnBrk"),
        hanging_punct: e.bool("hangingPunct"),
        line_spacing: spacing("lnSpc"),
        space_before: spacing("spcBef"),
        space_after: spacing("spcAft"),
        tabs: e.child("tabLst").map(|l| {
            l.children_named("tab")
                .take(64)
                .filter_map(|t| {
                    let pos = t.f64("pos").filter(|v| v.is_finite())? / 12700.;
                    Some(TabStop { pos: pos.clamp(-1e6, 1e6), align: t.attr("algn").unwrap_or("l").into() })
                })
                .collect()
        }),
        ..Default::default()
    }
}

pub(super) fn body(e: &El) -> TextBody {
    let mut body = TextBody { body: e.child("bodyPr").map(super::dml::body_pr).unwrap_or_default(), ..Default::default() };
    let list = e.child("lstStyle");
    let default = list.and_then(|l| l.child("defPPr"));
    let mut budget = 4096;
    for p in e.children_named("p").take(256) {
        let mut para = Paragraph::default();
        let ppr = p.child("pPr");
        para.level = ppr.and_then(|p| p.u32("lvl")).unwrap_or(0).min(8) as u8;
        let level_tag = format!("lvl{}pPr", para.level + 1);
        let level = list.and_then(|l| l.child(&level_tag));
        para.props = ppr.map(paragraph_props).unwrap_or_default();
        para.def_props = p.path(&["pPr", "defRPr"]).map(label_run_props).unwrap_or_default();
        // List levels are independent. Level two must not inherit level one's size or
        // margins; each level inherits the shared default, below explicit paragraph data.
        for source in [level, default].into_iter().flatten() {
            para.props.inherit(&paragraph_props(source));
            if let Some(run) = source.child("defRPr") {
                para.def_props.inherit(&label_run_props(run));
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_labels_inherit_their_own_list_level_and_shared_defaults() {
        let xml = crate::xml::parse(
            br#"<rich><bodyPr/><lstStyle>
        <defPPr marR="127000" rtl="1"><defRPr sz="1600" i="1"/></defPPr>
        <lvl1pPr marL="254000"><defRPr sz="3000" b="1"/></lvl1pPr>
        <lvl2pPr marL="508000" algn="r"><defRPr sz="2200"/></lvl2pPr>
        </lstStyle>
        <p><r><t>First</t></r></p>
        <p><pPr lvl="1" marR="63500"><defRPr b="0"/></pPr><r><rPr i="0"/><t>Second</t></r></p>
        <p><pPr lvl="2"/><r><t>Third</t></r></p></rich>"#,
        )
        .unwrap();
        let text = body(&xml.root);
        let first = &text.paragraphs[0];
        assert_eq!(first.runs[0].props.size, Some(30.));
        assert_eq!(first.props.margin_left, Some(20.));
        let second = &text.paragraphs[1];
        assert_eq!(second.runs[0].props.size, Some(22.));
        assert_eq!(second.runs[0].props.bold, Some(false));
        assert_eq!(second.runs[0].props.italic, Some(false));
        assert_eq!(second.props.align, Some(Align::Right));
        assert_eq!(second.props.margin_left, Some(40.));
        assert_eq!(second.props.margin_right, Some(5.));
        assert_eq!(second.props.rtl, Some(true));
        let third = &text.paragraphs[2];
        assert_eq!(third.runs[0].props.size, Some(16.));
        assert_eq!(third.runs[0].props.bold, None, "level one is not the fallback for other levels");
        assert_eq!(third.runs[0].props.italic, Some(true));
        assert_eq!(third.props.margin_left, None);
    }
}
