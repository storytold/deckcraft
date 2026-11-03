//! Original theme-relative defaults for newly built chartEx parts.
//! MS-ODRAWXML §2.8.3 requires a style entry for each chart role. PowerPoint for Mac
//! can hang opening chartEx without a complete style part, even when all paints are explicit.
//! These defaults are generated from the public schema, not a vendor style/template.
use crate::{
    opc::{self, NS_A, PackageOut, RelsOut},
    xml::{A, W},
};

const NS_CS: &str = "http://schemas.microsoft.com/office/drawing/2012/chartStyle";

fn style() -> Vec<u8> {
    let mut w = W::new();
    w.open("cs:chartStyle", A::new().a("xmlns:cs", NS_CS).a("xmlns:a", NS_A).a("id", "501"));
    for role in [
        "axisTitle",
        "categoryAxis",
        "chartArea",
        "dataLabel",
        "dataPoint",
        "dataPoint3D",
        "dataPointLine",
        "dataPointMarker",
        "dataPointWireframe",
        "dataTable",
        "downBar",
        "dropLine",
        "errorBar",
        "floor",
        "gridlineMajor",
        "gridlineMinor",
        "hiLoLine",
        "leaderLine",
        "legend",
        "plotArea",
        "plotArea3D",
        "seriesAxis",
        "seriesLine",
        "title",
        "trendline",
        "trendlineLabel",
        "upBar",
        "valueAxis",
        "wall",
    ] {
        let tag = format!("cs:{role}");
        let filled = matches!(role, "dataPoint" | "dataPoint3D" | "dataPointMarker" | "upBar" | "downBar");
        let line = matches!(
            role,
            "categoryAxis"
                | "valueAxis"
                | "seriesAxis"
                | "dataPointLine"
                | "dataPointWireframe"
                | "gridlineMajor"
                | "gridlineMinor"
                | "errorBar"
                | "dropLine"
                | "hiLoLine"
                | "leaderLine"
                | "seriesLine"
                | "trendline"
        );
        w.open0(&tag);
        for (name, index, color) in [
            ("lnRef", if line { "1" } else { "0" }, "tx1"),
            ("fillRef", if filled { "1" } else { "0" }, "accent1"),
            ("effectRef", "0", "accent1"),
            ("fontRef", "minor", "tx1"),
        ] {
            let reference = format!("cs:{name}");
            w.open(&reference, A::new().a("idx", index));
            if name == "fillRef" && filled {
                w.empty("cs:styleClr", A::new().a("val", "auto"));
            } else {
                w.empty("a:schemeClr", A::new().a("val", color));
            }
            w.close(&reference);
        }
        w.empty("cs:defRPr", A::new().a("sz", if role == "title" { "1800" } else { "1200" }));
        w.close(&tag);
    }
    w.close("cs:chartStyle");
    w.finish()
}

fn colors() -> Vec<u8> {
    let mut w = W::new();
    w.open("cs:colorStyle", A::new().a("xmlns:cs", NS_CS).a("xmlns:a", NS_A).a("meth", "cycle").a("id", "501"));
    for n in 1..=6 {
        w.empty("a:schemeClr", A::new().a("val", format!("accent{n}")));
    }
    w.empty0("cs:variation");
    w.close("cs:colorStyle");
    w.finish()
}

pub(super) fn write(pkg: &mut PackageOut, chart: &str, number: u32) {
    let mut rels = RelsOut::default();
    for (kind, bytes, content_type, relation) in [
        ("Style", style(), "application/vnd.ms-office.chartstyle+xml", "chartStyle"),
        ("Colors", colors(), "application/vnd.ms-office.chartcolorstyle+xml", "chartColorStyle"),
    ] {
        let path = format!("ppt/charts/chart{kind}{number}.xml");
        pkg.xml_part(&path, content_type, bytes);
        rels.add_new(&format!("http://schemas.microsoft.com/office/2011/relationships/{relation}"), &opc::relative(chart, &path), false);
    }
    pkg.rels(chart, &rels);
}
