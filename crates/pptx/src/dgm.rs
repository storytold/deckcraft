//! SmartArt (DrawingML diagrams): the outline ⇄ the data model, and the layout, quick style and
//! colour definitions DeckCraft writes for its own layouts.
//!
//! A diagram's data part holds its points (`dgm:pt`: the document, one per node, and the
//! transitions PowerPoint pairs with each link) and its links (`dgm:cxn` of type `parOf`, parent →
//! child, ordered by `srcOrd`). The outline is that tree flattened depth first.

use std::collections::HashMap;

use deckcraft_model::{Diagram, DiagramNode};

use crate::xml::{A, El, W};

pub const NS_DGM: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
pub const NS_DSP: &str = "http://schemas.microsoft.com/office/drawing/2008/diagram";
pub const CT_DATA: &str = "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml";
pub const CT_LAYOUT: &str = "application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml";
pub const CT_STYLE: &str = "application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml";
pub const CT_COLORS: &str = "application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml";
pub const CT_DRAWING: &str = "application/vnd.ms-office.drawingml.diagramDrawing+xml";
pub const RT_DRAWING: &str = "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing";
const QS_BASE: &str = "urn:microsoft.com/office/officeart/2005/8/quickstyle/";
const CS_BASE: &str = "urn:microsoft.com/office/officeart/2005/8/colors/";

/// PowerPoint's quick style for a SmartArt Style.
pub fn qs_id(style: &str) -> String {
    let s = match style {
        "subtle" => "simple3",
        "intense" => "simple5",
        "outline" => "simple4",
        "polished" => "3d1",
        "inset" => "3d2",
        "cartoon" => "3d3",
        "powder" => "3d4",
        "brick" => "3d5",
        _ => "simple1",
    };
    format!("{QS_BASE}{s}")
}

/// The SmartArt Style closest to a quick style id.
pub fn style_for(qs: &str) -> &'static str {
    match qs.rsplit('/').next().unwrap_or("") {
        "simple1" | "" => "",
        "simple4" => "outline",
        "3d1" => "polished",
        "3d2" => "inset",
        "3d3" => "cartoon",
        "3d4" => "powder",
        "3d5" => "brick",
        "simple2" | "simple3" => "subtle",
        _ => "intense",
    }
}

/// PowerPoint's colour definition for Change Colors.
pub fn cs_id(colors: &str) -> String {
    if let Some(a @ 2..=5) = colors.strip_prefix("range").and_then(|r| r.get(..1)).and_then(|n| n.parse::<u8>().ok()) {
        return format!("{CS_BASE}colorful{a}");
    }
    let grad = colors.ends_with("Grad");
    match colors.strip_prefix("accent").and_then(|r| r.get(..1)).and_then(|n| n.parse::<u8>().ok()) {
        Some(n @ 1..=6) => format!("{CS_BASE}accent{n}_{}", if grad { 3 } else { 2 }),
        _ => format!("{CS_BASE}colorful1"),
    }
}

/// The accents a Change Colors choice fills shapes with, in turn.
fn fill_accents(colors: &str) -> Vec<String> {
    if let Some(a @ 2..=5) = colors.strip_prefix("range").and_then(|r| r.get(..1)).and_then(|n| n.parse::<u8>().ok()) {
        return vec![format!("accent{a}"), format!("accent{}", a + 1)];
    }
    match colors.strip_prefix("accent").and_then(|r| r.get(..1)).and_then(|n| n.parse::<u8>().ok()) {
        Some(n @ 1..=6) => vec![format!("accent{n}")],
        _ => (1..=6).map(|k| format!("accent{k}")).collect(),
    }
}

/// Change Colors from a colour definition id: `accentN_…` keeps accent N, anything else is colourful.
pub fn colors_for(cs: &str) -> String {
    let last = cs.rsplit('/').next().unwrap_or("");
    if let Some(a @ 2..=5) = last.strip_prefix("colorful").and_then(|n| n.parse::<u8>().ok()) {
        return format!("range{a}{}", a + 1);
    }
    let mut parts = last.strip_prefix("accent").unwrap_or("").split('_');
    match (parts.next().and_then(|n| n.parse::<u8>().ok()), parts.next()) {
        (Some(n @ 1..=6), Some("3" | "4" | "5")) => format!("accent{n}Grad"),
        (Some(n @ 1..=6), _) => format!("accent{n}"),
        _ => String::new(),
    }
}
/// Most nodes read from one diagram.
const MAX_NODES: usize = 2000;

/// PowerPoint's built-in layout DeckCraft's layout `kind` is written as.
pub fn layout_id(kind: &str) -> &'static str {
    match kind {
        "blockList" => "urn:microsoft.com/office/officeart/2005/8/layout/default",
        "hBulletList" => "urn:microsoft.com/office/officeart/2005/8/layout/hList1",
        "process" => "urn:microsoft.com/office/officeart/2005/8/layout/process1",
        "chevron" => "urn:microsoft.com/office/officeart/2005/8/layout/chevron1",
        "verticalProcess" => "urn:microsoft.com/office/officeart/2005/8/layout/process2",
        "stepUp" => "urn:microsoft.com/office/officeart/2009/3/layout/StepUpProcess",
        "cycle" => "urn:microsoft.com/office/officeart/2005/8/layout/cycle2",
        "blockCycle" => "urn:microsoft.com/office/officeart/2005/8/layout/cycle5",
        "radial" => "urn:microsoft.com/office/officeart/2005/8/layout/radial1",
        "hierarchy" => "urn:microsoft.com/office/officeart/2005/8/layout/hierarchy1",
        "pyramid" => "urn:microsoft.com/office/officeart/2005/8/layout/pyramid1",
        "invertedPyramid" => "urn:microsoft.com/office/officeart/2005/8/layout/pyramid3",
        "matrix" => "urn:microsoft.com/office/officeart/2005/8/layout/matrix3",
        "titledMatrix" => "urn:microsoft.com/office/officeart/2005/8/layout/matrix1",
        "pictureList" => "urn:microsoft.com/office/officeart/2005/8/layout/vList4",
        "pictureCaption" => "urn:microsoft.com/office/officeart/2005/8/layout/pList1",
        "pictureProcess" => "urn:microsoft.com/office/officeart/2005/8/layout/hProcess10",
        "pictureHList" => "urn:microsoft.com/office/officeart/2005/8/layout/pList2",
        "timeline" => "urn:microsoft.com/office/officeart/2005/8/layout/hProcess11",
        "venn" => "urn:microsoft.com/office/officeart/2005/8/layout/venn1",
        "target" => "urn:microsoft.com/office/officeart/2005/8/layout/target1",
        "textCycle" => "urn:microsoft.com/office/officeart/2005/8/layout/cycle1",
        "continuousCycle" => "urn:microsoft.com/office/officeart/2005/8/layout/cycle3",
        "horizontalHierarchy" => "urn:microsoft.com/office/officeart/2005/8/layout/hierarchy2",
        "gridMatrix" => "urn:microsoft.com/office/officeart/2005/8/layout/matrix2",
        "gear" => "urn:microsoft.com/office/officeart/2005/8/layout/gear1",
        "funnel" => "urn:microsoft.com/office/officeart/2005/8/layout/funnel1",
        "segmentedCycle" => "urn:microsoft.com/office/officeart/2005/8/layout/cycle8",
        "multidirectionalCycle" => "urn:microsoft.com/office/officeart/2005/8/layout/cycle7",
        "orgChart" => "urn:microsoft.com/office/officeart/2005/8/layout/orgChart1",
        "halfCircleOrgChart" => "urn:microsoft.com/office/officeart/2008/layout/HalfCircleOrganizationChart",
        "nameTitleOrgChart" => "urn:microsoft.com/office/officeart/2008/layout/NameandTitleOrganizationalChart",
        "verticalBoxList" => "urn:microsoft.com/office/officeart/2005/8/layout/list1",
        "trapezoidList" => "urn:microsoft.com/office/officeart/2005/8/layout/hList6",
        "tableList" => "urn:microsoft.com/office/officeart/2005/8/layout/hList3",
        "accentProcess" => "urn:microsoft.com/office/officeart/2005/8/layout/process3",
        "continuousBlockProcess" => "urn:microsoft.com/office/officeart/2005/8/layout/hProcess9",
        "bendingProcess" => "urn:microsoft.com/office/officeart/2005/8/layout/process5",
        "linearVenn" => "urn:microsoft.com/office/officeart/2005/8/layout/venn3",
        "stackedVenn" => "urn:microsoft.com/office/officeart/2005/8/layout/venn2",
        "nestedTarget" => "urn:microsoft.com/office/officeart/2005/8/layout/target2",
        "cycleMatrix" => "urn:microsoft.com/office/officeart/2005/8/layout/cycle4",
        "segmentedPyramid" => "urn:microsoft.com/office/officeart/2005/8/layout/pyramid4",
        "pie" => "urn:microsoft.com/office/officeart/2005/8/layout/chart3",
        _ => "urn:microsoft.com/office/officeart/2005/8/layout/vList2",
    }
}

/// The DeckCraft layout closest to a PowerPoint layout (by its id and category).
pub fn kind_for(layout_id: &str, category: &str) -> &'static str {
    let last = layout_id.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    // DeckCraft before Picture Accent Process got its own id wrote it as pList2 (Horizontal
    // Picture List) with its own layout part in the process category; PowerPoint's pList2 is a list.
    if last == "plist2" && category.eq_ignore_ascii_case("process") {
        return "pictureProcess";
    }
    // Exact ids first: the layouts DeckCraft writes, and their closest siblings.
    for k in [
        "blockList",
        "hBulletList",
        "process",
        "chevron",
        "verticalProcess",
        "stepUp",
        "cycle",
        "blockCycle",
        "radial",
        "hierarchy",
        "pyramid",
        "invertedPyramid",
        "matrix",
        "titledMatrix",
        "pictureList",
        "pictureCaption",
        "pictureProcess",
        "pictureHList",
        "timeline",
        "venn",
        "target",
        "textCycle",
        "continuousCycle",
        "horizontalHierarchy",
        "gridMatrix",
        "gear",
        "funnel",
        "segmentedCycle",
        "multidirectionalCycle",
        "orgChart",
        "halfCircleOrgChart",
        "nameTitleOrgChart",
        "verticalBoxList",
        "trapezoidList",
        "tableList",
        "accentProcess",
        "continuousBlockProcess",
        "bendingProcess",
        "linearVenn",
        "stackedVenn",
        "nestedTarget",
        "cycleMatrix",
        "segmentedPyramid",
        "pie",
        "list",
    ] {
        if layout_id == layout_id_of(k) {
            return k;
        }
    }
    let s = format!("{last} {}", category.to_ascii_lowercase());
    if s.contains("gear") {
        "gear"
    } else if s.contains("funnel") {
        "funnel"
    } else if s.contains("venn") {
        "venn"
    } else if s.contains("target") {
        "target"
    } else if s.contains("timeline") {
        "timeline"
    } else if s.contains("nameandtitle") {
        "nameTitleOrgChart"
    } else if s.contains("orgchart") || s.contains("organizationchart") || s.contains("organizationalchart") {
        "orgChart"
    } else if s.contains("hierarchy") {
        "hierarchy"
    } else if s.contains("radial") {
        "radial"
    } else if s.contains("cycle") {
        "cycle"
    } else if s.contains("pyramid") {
        "pyramid"
    } else if s.contains("matrix") {
        "matrix"
    } else if s.contains("chevron") {
        "chevron"
    } else if s.contains("process") || s.contains("arrow") {
        "process"
    } else if last == "default" {
        "blockList"
    } else if last.starts_with("hlist") {
        "hBulletList"
    } else {
        "list"
    }
}

fn layout_id_of(kind: &str) -> &'static str {
    layout_id(kind)
}

fn category(kind: &str) -> &'static str {
    match kind {
        "process"
        | "chevron"
        | "verticalProcess"
        | "stepUp"
        | "pictureProcess"
        | "timeline"
        | "gear"
        | "funnel"
        | "accentProcess"
        | "continuousBlockProcess"
        | "bendingProcess" => "process",
        "venn" | "target" | "linearVenn" | "stackedVenn" | "nestedTarget" => "relationship",
        "cycle" | "blockCycle" | "radial" | "textCycle" | "continuousCycle" | "segmentedCycle" | "multidirectionalCycle" | "cycleMatrix" | "pie" => {
            "cycle"
        }
        "hierarchy" | "horizontalHierarchy" | "orgChart" | "halfCircleOrgChart" | "nameTitleOrgChart" => "hierarchy",
        "pyramid" | "invertedPyramid" | "segmentedPyramid" => "pyramid",
        "matrix" | "titledMatrix" | "gridMatrix" => "matrix",
        _ => "list",
    }
}

/// A model id, stable for a given diagram (`seed`) and point number.
fn guid(seed: u32, n: usize) -> String {
    format!("{{{seed:08X}-D1A6-4C0A-9E5B-{n:012X}}}")
}

fn text(w: &mut W, t: &str) {
    w.open0("dgm:t");
    w.empty0("a:bodyPr");
    w.empty0("a:lstStyle");
    w.open0("a:p");
    if t.is_empty() {
        w.empty("a:endParaRPr", A::new().a("lang", "en-US"));
    } else {
        w.open0("a:r");
        w.empty("a:rPr", A::new().a("lang", "en-US"));
        w.elt("a:t", t);
        w.close("a:r");
    }
    w.close("a:p");
    w.close("dgm:t");
}

/// `ppt/diagrams/dataN.xml` for the outline. `drawing_rel` is the slide's relationship id of the
/// cached drawing; `pics` the data part's relationship ids of node pictures, by node index.
pub fn data_xml(d: &Diagram, seed: u32, drawing_rel: &str, pics: &[(usize, String)]) -> Vec<u8> {
    let lo = if d.layout_id.is_empty() { layout_id(&d.kind).to_string() } else { d.layout_id.clone() };
    let mut w = W::new();
    w.open("dgm:dataModel", A::new().a("xmlns:dgm", NS_DGM).a("xmlns:a", crate::opc::NS_A).a("xmlns:r", crate::opc::NS_R));
    w.open0("dgm:ptLst");
    let doc = guid(seed, 0);
    w.open("dgm:pt", A::new().a("modelId", &doc).a("type", "doc"));
    w.empty(
        "dgm:prSet",
        A::new()
            .a("loTypeId", &lo)
            .a("loCatId", category(&d.kind))
            .a("qsTypeId", qs_id(&d.style))
            .a("qsCatId", "simple")
            .a("csTypeId", cs_id(&d.colors))
            .a("csCatId", if d.colors.starts_with("accent") { d.colors.as_str() } else { "colorful" })
            .a("phldr", "1"),
    );
    w.empty0("dgm:spPr");
    text(&mut w, "");
    w.close("dgm:pt");
    // Node k: model id 1 + 3k; its link's parent and sibling transitions 2 + 3k and 3 + 3k.
    let node = |k: usize| guid(seed, 1 + 3 * k);
    for (k, n) in d.nodes.iter().enumerate() {
        let mut a = A::new().a("modelId", node(k));
        if n.assistant {
            a = a.a("type", "asst");
        }
        w.open("dgm:pt", a);
        w.empty("dgm:prSet", A::new().a("phldrT", "[Text]"));
        w.empty0("dgm:spPr");
        text(&mut w, &n.text);
        w.close("dgm:pt");
        let cxn = guid(seed, 0x10_0000 + k);
        for (i, ty) in [(2, "parTrans"), (3, "sibTrans")] {
            w.open("dgm:pt", A::new().a("modelId", guid(seed, i + 3 * k)).a("type", ty).a("cxnId", &cxn));
            w.empty0("dgm:prSet");
            w.empty0("dgm:spPr");
            text(&mut w, "");
            w.close("dgm:pt");
        }
    }
    // A node's picture sits on the presentation point of its picture placeholder, as PowerPoint
    // stores it (`presAssocID` names the node, the blip is a relationship of the data part).
    // Name and Title: a node's title is the text of its title box's presentation point.
    for (k, n) in d.nodes.iter().enumerate() {
        if n.title.is_empty() {
            continue;
        }
        w.open("dgm:pt", A::new().a("modelId", guid(seed, 0x30_0000 + k)).a("type", "pres"));
        w.empty("dgm:prSet", A::new().a("presAssocID", node(k)).a("presName", "titleText1").a("presStyleLbl", "fgAcc1").a("custT", "1"));
        w.empty0("dgm:spPr");
        text(&mut w, &n.title);
        w.close("dgm:pt");
    }
    for (k, rid) in pics {
        if *k >= d.nodes.len() {
            continue;
        }
        w.open("dgm:pt", A::new().a("modelId", guid(seed, 0x20_0000 + k)).a("type", "pres"));
        w.empty("dgm:prSet", A::new().a("presAssocID", node(*k)).a("presName", "Image").a("presStyleLbl", "fgImgPlace1"));
        w.open0("dgm:spPr");
        w.open0("a:blipFill");
        w.empty("a:blip", A::new().a("r:embed", rid.as_str()));
        w.open0("a:stretch");
        w.empty0("a:fillRect");
        w.close("a:stretch");
        w.close("a:blipFill");
        w.close("dgm:spPr");
        w.close("dgm:pt");
    }
    w.close("dgm:ptLst");
    w.open0("dgm:cxnLst");
    let mut stack: Vec<(u8, String)> = vec![];
    let mut ords: HashMap<String, usize> = HashMap::new();
    for (k, n) in d.nodes.iter().enumerate() {
        while stack.last().is_some_and(|(l, _)| *l >= n.level) {
            stack.pop();
        }
        let parent = stack.last().map(|(_, id)| id.clone()).unwrap_or_else(|| doc.clone());
        let ord = ords.entry(parent.clone()).or_insert(0);
        w.empty(
            "dgm:cxn",
            A::new()
                .a("modelId", guid(seed, 0x10_0000 + k))
                .a("srcId", &parent)
                .a("destId", node(k))
                .a("srcOrd", *ord)
                .a("destOrd", 0)
                .a("parTransId", guid(seed, 2 + 3 * k))
                .a("sibTransId", guid(seed, 3 + 3 * k)),
        );
        *ord += 1;
        stack.push((n.level, node(k)));
    }
    w.close("dgm:cxnLst");
    w.empty0("dgm:bg");
    w.empty0("dgm:whole");
    w.open0("dgm:extLst");
    w.open("a:ext", A::new().a("uri", "http://schemas.microsoft.com/office/drawing/2008/diagram"));
    w.empty(
        "dsp:dataModelExt",
        A::new().a("xmlns:dsp", NS_DSP).a("relId", drawing_rel).a("minVer", "http://schemas.openxmlformats.org/drawingml/2006/diagram"),
    );
    w.close("a:ext");
    w.close("dgm:extLst");
    w.close("dgm:dataModel");
    w.finish()
}

/// The outline of a data part: the `parOf` tree under the document point, depth first.
pub fn outline(data: &El) -> Vec<DiagramNode> {
    outline_ids(data).into_iter().map(|(_, n)| n).collect()
}

/// The relationship ids (of the data part) of node pictures, by node index in [`outline`]: the
/// blip fill of a presentation point associated with the node.
pub fn node_pictures(data: &El) -> Vec<(usize, String)> {
    let ids: HashMap<String, usize> = outline_ids(data).into_iter().enumerate().map(|(k, (id, _))| (id, k)).collect();
    let Some(pts) = data.child("ptLst") else { return vec![] };
    let mut out: Vec<(usize, String)> = vec![];
    for pt in pts.children_named("pt").take(MAX_NODES * 8) {
        if pt.attr("type") != Some("pres") {
            continue;
        }
        let Some(k) = pt.child("prSet").and_then(|p| p.attr("presAssocID")).and_then(|a| ids.get(a)) else { continue };
        let Some(rid) = pt.child("spPr").and_then(|s| s.child("blipFill")).and_then(|b| b.child("blip")).and_then(|b| b.attr("r:embed")) else {
            continue;
        };
        if !out.iter().any(|(j, _)| j == k) {
            out.push((*k, rid.to_string()));
        }
    }
    out
}

fn outline_ids(data: &El) -> Vec<(String, DiagramNode)> {
    let Some(pts) = data.child("ptLst") else { return vec![] };
    let mut texts: HashMap<&str, String> = HashMap::new();
    let mut assistants: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut titles: HashMap<&str, String> = HashMap::new();
    let mut doc: Option<&str> = None;
    for pt in pts.children_named("pt").take(MAX_NODES * 4) {
        let Some(id) = pt.attr("modelId") else { continue };
        match pt.attr("type").unwrap_or("node") {
            "doc" => doc = doc.or(Some(id)),
            ty @ ("node" | "asst") => {
                let t = pt.child("t").map(paragraphs).unwrap_or_default();
                texts.insert(id, t);
                if ty == "asst" {
                    assistants.insert(id);
                }
            }
            "pres" => {
                // Name and Title: the title box's own text, on its presentation point.
                let pr = pt.child("prSet");
                if pr.and_then(|p| p.attr("presName")).is_some_and(|n| n.starts_with("titleText"))
                    && let Some(node) = pr.and_then(|p| p.attr("presAssocID"))
                    && let Some(t) = pt.child("t").map(paragraphs).filter(|t| !t.is_empty())
                {
                    titles.insert(node, t);
                }
            }
            _ => {}
        }
    }
    let Some(doc) = doc else { return vec![] };
    let mut kids: HashMap<&str, Vec<(i64, &str)>> = HashMap::new();
    if let Some(cx) = data.child("cxnLst") {
        for c in cx.children_named("cxn").take(MAX_NODES * 4) {
            if c.attr("type").unwrap_or("parOf") != "parOf" {
                continue;
            }
            let (Some(src), Some(dst)) = (c.attr("srcId"), c.attr("destId")) else { continue };
            if texts.contains_key(dst) {
                kids.entry(src).or_default().push((c.i64("srcOrd").unwrap_or(0), dst));
            }
        }
    }
    for v in kids.values_mut() {
        v.sort_by_key(|(o, _)| *o);
    }
    let mut out = vec![];
    let mut seen = std::collections::HashSet::new();
    // Depth first, iteratively; a node reached twice (a cycle in a hostile file) is skipped.
    let mut stack: Vec<(&str, u8)> = kids.get(doc).map(|v| v.iter().rev().map(|(_, id)| (*id, 0u8)).collect()).unwrap_or_default();
    while let Some((id, level)) = stack.pop() {
        if out.len() >= MAX_NODES || !seen.insert(id) {
            continue;
        }
        out.push((
            id.to_string(),
            DiagramNode {
                text: texts.get(id).cloned().unwrap_or_default(),
                level: level.min(8),
                picture: None,
                assistant: assistants.contains(id),
                title: titles.get(id).cloned().unwrap_or_default(),
            },
        ));
        if let Some(v) = kids.get(id) {
            stack.extend(v.iter().rev().map(|(_, c)| (*c, level.saturating_add(1))));
        }
    }
    out
}

/// A text body's paragraphs joined by spaces.
fn paragraphs(t: &El) -> String {
    t.children_named("p")
        .map(|p| {
            let mut v = vec![];
            p.find_all("t", &mut v);
            v.iter().map(|e| e.text()).collect::<String>()
        })
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The layout algorithm and node shape of a DeckCraft layout.
fn alg(kind: &str) -> (&'static str, Vec<(&'static str, &'static str)>, &'static str) {
    match kind {
        "process" | "stepUp" | "pictureProcess" | "pictureCaption" | "pictureHList" | "timeline" => {
            ("lin", vec![("linDir", "fromL"), ("nodeVertAlign", "mid")], "roundRect")
        }
        "chevron" => ("lin", vec![("linDir", "fromL"), ("nodeVertAlign", "mid")], "chevron"),
        "verticalProcess" => ("lin", vec![("linDir", "fromT"), ("nodeHorzAlign", "ctr")], "roundRect"),
        "cycle"
        | "radial"
        | "venn"
        | "target"
        | "textCycle"
        | "continuousCycle"
        | "gear"
        | "segmentedCycle"
        | "multidirectionalCycle"
        | "cycleMatrix"
        | "pie" => ("cycle", vec![("stAng", "0"), ("spanAng", "360")], "ellipse"),
        "blockCycle" => ("cycle", vec![("stAng", "0"), ("spanAng", "360")], "roundRect"),
        "pyramid" | "invertedPyramid" | "segmentedPyramid" => {
            ("pyra", vec![("linDir", if kind == "pyramid" { "fromT" } else { "fromB" }), ("txDir", "fromT"), ("pyraAcctPos", "bef")], "trapezoid")
        }
        "matrix" | "titledMatrix" | "gridMatrix" | "blockList" => (
            "snake",
            vec![("grDir", "tL"), ("flowDir", "row"), ("contDir", "sameDir"), ("off", "ctr")],
            if kind == "blockList" { "rect" } else { "roundRect" },
        ),
        "hBulletList" => ("lin", vec![("linDir", "fromL"), ("nodeVertAlign", "t")], "roundRect"),
        _ => ("lin", vec![("linDir", "fromT"), ("vertAlign", "mid")], "roundRect"),
    }
}

fn node_layout(w: &mut W, shape: &str, bullets: bool) {
    w.open("dgm:layoutNode", A::new().a("name", "node"));
    w.open0("dgm:varLst");
    w.empty("dgm:bulletEnabled", A::new().a("val", if bullets { "1" } else { "0" }));
    w.close("dgm:varLst");
    w.empty("dgm:alg", A::new().a("type", "tx"));
    w.open("dgm:shape", A::new().a("type", shape).a("r:blip", ""));
    w.empty0("dgm:adjLst");
    w.close("dgm:shape");
    w.empty("dgm:presOf", A::new().a("axis", if bullets { "desOrSelf" } else { "self" }).a("ptType", "node"));
    w.open0("dgm:constrLst");
    for side in ["tMarg", "bMarg", "lMarg", "rMarg"] {
        w.empty("dgm:constr", A::new().a("type", side).a("refType", "primFontSz").a("fact", "0.3"));
    }
    w.close("dgm:constrLst");
    w.open0("dgm:ruleLst");
    w.empty("dgm:rule", A::new().a("type", "primFontSz").a("val", "5").a("fact", "NaN").a("max", "NaN"));
    w.close("dgm:ruleLst");
    w.close("dgm:layoutNode");
}

fn space(w: &mut W) {
    w.open("dgm:forEach", A::new().a("name", "sibling").a("axis", "followSib").a("ptType", "sibTrans").a("cnt", "1"));
    w.open("dgm:layoutNode", A::new().a("name", "space"));
    w.empty("dgm:alg", A::new().a("type", "sp"));
    w.open("dgm:shape", A::new().a("r:blip", ""));
    w.empty0("dgm:adjLst");
    w.close("dgm:shape");
    w.empty0("dgm:presOf");
    w.empty0("dgm:constrLst");
    w.empty0("dgm:ruleLst");
    w.close("dgm:layoutNode");
    w.close("dgm:forEach");
}

/// `ppt/diagrams/layoutN.xml`: a layout definition for DeckCraft's layout `kind`, under the id of
/// the closest built-in PowerPoint layout.
pub fn layout_xml(kind: &str) -> Vec<u8> {
    let mut w = W::new();
    w.open(
        "dgm:layoutDef",
        A::new().a("xmlns:dgm", NS_DGM).a("xmlns:a", crate::opc::NS_A).a("xmlns:r", crate::opc::NS_R).a("uniqueId", layout_id(kind)),
    );
    w.empty("dgm:title", A::new().a("val", ""));
    w.empty("dgm:desc", A::new().a("val", ""));
    w.open0("dgm:catLst");
    w.empty("dgm:cat", A::new().a("type", category(kind)).a("pri", "1000"));
    w.close("dgm:catLst");
    w.open("dgm:layoutNode", A::new().a("name", "diagram"));
    w.open0("dgm:varLst");
    w.empty0("dgm:dir");
    w.empty("dgm:resizeHandles", A::new().a("val", "exact"));
    w.close("dgm:varLst");
    if matches!(kind, "hierarchy" | "horizontalHierarchy" | "orgChart" | "halfCircleOrgChart" | "nameTitleOrgChart") {
        // Each node over the row of its children, recursively.
        w.empty("dgm:alg", A::new().a("type", "hierChild"));
        w.open("dgm:shape", A::new().a("r:blip", ""));
        w.empty0("dgm:adjLst");
        w.close("dgm:shape");
        w.empty0("dgm:presOf");
        w.empty0("dgm:constrLst");
        w.empty0("dgm:ruleLst");
        w.open("dgm:forEach", A::new().a("name", "level").a("axis", "ch").a("ptType", "node"));
        w.open("dgm:layoutNode", A::new().a("name", "hierRoot"));
        w.empty("dgm:alg", A::new().a("type", "hierRoot"));
        w.open("dgm:shape", A::new().a("r:blip", ""));
        w.empty0("dgm:adjLst");
        w.close("dgm:shape");
        w.empty0("dgm:presOf");
        w.empty0("dgm:constrLst");
        w.empty0("dgm:ruleLst");
        node_layout(&mut w, "roundRect", false);
        w.open("dgm:layoutNode", A::new().a("name", "hierChild"));
        w.empty("dgm:alg", A::new().a("type", "hierChild"));
        w.open("dgm:shape", A::new().a("r:blip", ""));
        w.empty0("dgm:adjLst");
        w.close("dgm:shape");
        w.empty0("dgm:presOf");
        w.empty0("dgm:constrLst");
        w.empty0("dgm:ruleLst");
        w.empty("dgm:forEach", A::new().a("name", "repeat").a("ref", "level"));
        w.close("dgm:layoutNode");
        w.close("dgm:layoutNode");
        w.close("dgm:forEach");
    } else {
        let (ty, params, shape) = alg(kind);
        w.open("dgm:alg", A::new().a("type", ty));
        for (k, v) in params {
            w.empty("dgm:param", A::new().a("type", k).a("val", v));
        }
        w.close("dgm:alg");
        w.open("dgm:shape", A::new().a("r:blip", ""));
        w.empty0("dgm:adjLst");
        w.close("dgm:shape");
        w.empty0("dgm:presOf");
        w.open0("dgm:constrLst");
        w.empty("dgm:constr", A::new().a("type", "w").a("for", "ch").a("forName", "node").a("refType", "w"));
        w.empty(
            "dgm:constr",
            A::new().a("type", "h").a("for", "ch").a("forName", "node").a("refType", "w").a("refFor", "ch").a("refForName", "node").a("fact", "0.6"),
        );
        w.empty(
            "dgm:constr",
            A::new().a("type", "w").a("for", "ch").a("forName", "space").a("refType", "w").a("refFor", "ch").a("refForName", "node").a("fact", "0.1"),
        );
        w.empty("dgm:constr", A::new().a("type", "primFontSz").a("for", "ch").a("forName", "node").a("op", "equ").a("val", "65"));
        w.close("dgm:constrLst");
        w.empty0("dgm:ruleLst");
        w.open("dgm:forEach", A::new().a("name", "nodes").a("axis", "ch").a("ptType", "node"));
        node_layout(&mut w, shape, true);
        if !matches!(kind, "cycle" | "blockCycle" | "radial") {
            space(&mut w);
        }
        w.close("dgm:forEach");
    }
    w.close("dgm:layoutNode");
    w.close("dgm:layoutDef");
    w.finish()
}

/// The top bevel (preset, width in EMU) of a 3-D SmartArt Style.
fn bevel(style: &str) -> Option<(&'static str, i64)> {
    Some(match style {
        "polished" => ("circle", 76200),
        "inset" => ("relaxedInset", 63500),
        "cartoon" => ("softRound", 50800),
        "powder" => ("convex", 101600),
        "brick" => ("slope", 50800),
        _ => return None,
    })
}

/// `ppt/diagrams/quickStyleN.xml`: flat shapes drawn from the theme's first styles.
pub fn style_xml(style: &str) -> Vec<u8> {
    // Brick Scene looks at the diagram from above and to the side.
    let camera = if style == "brick" { "isometricOffAxis2Top" } else { "orthographicFront" };
    let mut w = W::new();
    w.open("dgm:styleDef", A::new().a("xmlns:dgm", NS_DGM).a("xmlns:a", crate::opc::NS_A).a("uniqueId", qs_id(style)));
    w.empty("dgm:title", A::new().a("val", ""));
    w.empty("dgm:desc", A::new().a("val", ""));
    w.open0("dgm:catLst");
    w.empty("dgm:cat", A::new().a("type", if bevel(style).is_some() { "3D" } else { "simple" }).a("pri", "10100"));
    w.close("dgm:catLst");
    w.open0("dgm:scene3d");
    w.empty("a:camera", A::new().a("prst", camera));
    w.empty("a:lightRig", A::new().a("rig", "threePt").a("dir", "t"));
    w.close("dgm:scene3d");
    w.open("dgm:styleLbl", A::new().a("name", "node1"));
    w.open0("dgm:scene3d");
    w.empty("a:camera", A::new().a("prst", camera));
    w.empty("a:lightRig", A::new().a("rig", "threePt").a("dir", "t"));
    w.close("dgm:scene3d");
    match bevel(style) {
        Some((prst, w_emu)) => {
            w.open0("dgm:sp3d");
            w.empty("a:bevelT", A::new().a("w", w_emu.to_string()).a("h", (w_emu * 3 / 5).to_string()).a("prst", prst));
            w.close("dgm:sp3d");
        }
        None => w.empty0("dgm:sp3d"),
    }
    w.empty("dgm:txPr", A::new());
    w.open0("dgm:style");
    for (tag, idx) in [("a:lnRef", "2"), ("a:fillRef", "1"), ("a:effectRef", "0")] {
        w.open(tag, A::new().a("idx", idx));
        w.empty("a:scrgbClr", A::new().a("r", "0").a("g", "0").a("b", "0"));
        w.close(tag);
    }
    w.open("a:fontRef", A::new().a("idx", "minor"));
    w.empty("a:schemeClr", A::new().a("val", "lt1"));
    w.close("a:fontRef");
    w.close("dgm:style");
    w.close("dgm:styleLbl");
    w.close("dgm:styleDef");
    w.finish()
}

/// `ppt/diagrams/colorsN.xml`: accent 1 fills with light text.
pub fn colors_xml(colors: &str) -> Vec<u8> {
    let mut w = W::new();
    w.open("dgm:colorsDef", A::new().a("xmlns:dgm", NS_DGM).a("xmlns:a", crate::opc::NS_A).a("uniqueId", cs_id(colors)));
    w.empty("dgm:title", A::new().a("val", ""));
    w.empty("dgm:desc", A::new().a("val", ""));
    let accents = fill_accents(colors);
    let single = accents.len() == 1;
    let cat = if single { accents.first().map(String::as_str).unwrap_or("accent1") } else { "colorful" };
    w.open0("dgm:catLst");
    w.empty("dgm:cat", A::new().a("type", cat).a("pri", "11200"));
    w.close("dgm:catLst");
    w.open("dgm:styleLbl", A::new().a("name", "node1"));
    if colors.ends_with("Grad") {
        // Gradient Range: the accent shaded darker across the shapes.
        w.open("dgm:fillClrLst", A::new().a("meth", "repeat"));
        for k in 0..4 {
            w.open("a:schemeClr", A::new().a("val", cat));
            w.empty("a:lumMod", A::new().a("val", (100_000 - k * 15_000).to_string()));
            w.close("a:schemeClr");
        }
        w.close("dgm:fillClrLst");
    } else {
        w.open("dgm:fillClrLst", A::new().a("meth", "repeat"));
        for a in &accents {
            w.empty("a:schemeClr", A::new().a("val", a.as_str()));
        }
        w.close("dgm:fillClrLst");
    }
    w.open("dgm:linClrLst", A::new().a("meth", "repeat"));
    w.empty("a:schemeClr", A::new().a("val", "lt1"));
    w.close("dgm:linClrLst");
    w.empty0("dgm:effectClrLst");
    w.empty0("dgm:txLinClrLst");
    w.empty0("dgm:txFillClrLst");
    w.empty0("dgm:txEffectClrLst");
    w.close("dgm:styleLbl");
    w.close("dgm:colorsDef");
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(v: &[(&str, u8)]) -> Vec<DiagramNode> {
        v.iter().map(|(t, l)| DiagramNode { text: (*t).into(), level: *l, picture: None, assistant: false, title: String::new() }).collect()
    }

    #[test]
    fn outline_survives_the_data_model() {
        let d =
            Diagram { kind: "hierarchy".into(), nodes: nodes(&[("CEO", 0), ("CTO", 1), ("Dev", 2), ("CFO", 1), ("Board", 0)]), ..Default::default() };
        let xml = data_xml(&d, 7, "rId9", &[]);
        let doc = crate::xml::parse(&xml).unwrap();
        assert_eq!(outline(&doc.root), d.nodes);
        let s = String::from_utf8(xml).unwrap();
        assert!(s.contains("hierarchy1") && s.contains("relId=\"rId9\""));
        for x in [
            layout_xml("hierarchy"),
            layout_xml("cycle"),
            style_xml("subtle"),
            colors_xml(""),
            colors_xml("accent3"),
            style_xml("polished"),
            colors_xml("range23"),
            colors_xml("accent6Grad"),
            layout_xml("pictureList"),
        ] {
            assert!(crate::xml::parse(&x).is_ok());
        }
    }

    #[test]
    fn assistants_survive_the_data_model() {
        let mut v = nodes(&[("CEO", 0), ("Aide", 1), ("CTO", 1)]);
        v[1].assistant = true;
        v[0].title = "Chief Executive".into();
        let d = Diagram { kind: "orgChart".into(), nodes: v, ..Default::default() };
        let xml = data_xml(&d, 5, "rId1", &[]);
        assert!(String::from_utf8(xml.clone()).unwrap().contains("type=\"asst\""));
        let doc = crate::xml::parse(&xml).unwrap();
        assert_eq!(outline(&doc.root), d.nodes);
    }

    #[test]
    fn node_pictures_come_from_presentation_points() {
        let d = Diagram { kind: "pictureList".into(), nodes: nodes(&[("A", 0), ("a", 1), ("B", 0)]), ..Default::default() };
        let xml = data_xml(&d, 3, "rId1", &[(2, "rId7".into()), (9, "rId8".into())]);
        let doc = crate::xml::parse(&xml).unwrap();
        assert_eq!(node_pictures(&doc.root), vec![(2, "rId7".to_string())]);
    }

    #[test]
    fn style_and_colour_ids_round_trip() {
        for st in ["", "subtle", "intense", "outline", "polished", "inset", "cartoon", "powder", "brick"] {
            assert_eq!(style_for(&qs_id(st)), st);
        }
        for c in ["", "range23", "range56", "accent1", "accent4Grad"] {
            assert_eq!(colors_for(&cs_id(c)), c);
        }
        assert!(String::from_utf8(style_xml("cartoon")).unwrap().contains("bevelT"));
        assert!(layout_id("pictureProcess").ends_with("/hProcess10"), "Picture Accent Process");
        assert!(layout_id("pictureHList").ends_with("/pList2"), "Horizontal Picture List");
        // Picture Accent Process as an earlier DeckCraft wrote it (pList2, process category).
        assert_eq!(kind_for("urn:microsoft.com/office/officeart/2005/8/layout/pList2", "process"), "pictureProcess");
        assert_eq!(kind_for("urn:microsoft.com/office/officeart/2005/8/layout/pList2", "list"), "pictureHList");
        for k in [
            "pictureList",
            "pictureCaption",
            "pictureProcess",
            "pictureHList",
            "timeline",
            "venn",
            "target",
            "textCycle",
            "continuousCycle",
            "horizontalHierarchy",
            "gridMatrix",
            "gear",
            "funnel",
            "segmentedCycle",
            "multidirectionalCycle",
            "orgChart",
            "halfCircleOrgChart",
            "cycleMatrix",
            "segmentedPyramid",
            "pie",
            "nameTitleOrgChart",
            "verticalBoxList",
            "trapezoidList",
            "tableList",
            "accentProcess",
            "continuousBlockProcess",
            "bendingProcess",
            "linearVenn",
            "stackedVenn",
            "nestedTarget",
        ] {
            assert_eq!(kind_for(layout_id(k), ""), k);
        }
    }

    #[test]
    fn hostile_links_are_bounded() {
        let xml = br#"<dgm:dataModel xmlns:dgm="d"><dgm:ptLst><dgm:pt modelId="0" type="doc"/><dgm:pt modelId="1"><dgm:t><a:p xmlns:a="a"><a:r><a:t>A</a:t></a:r></a:p></dgm:t></dgm:pt><dgm:pt modelId="2"/></dgm:ptLst><dgm:cxnLst><dgm:cxn srcId="0" destId="1"/><dgm:cxn srcId="1" destId="2"/><dgm:cxn srcId="2" destId="1"/><dgm:cxn type="presOf" srcId="0" destId="2"/></dgm:cxnLst></dgm:dataModel>"#;
        let doc = crate::xml::parse(xml).unwrap();
        assert_eq!(outline(&doc.root), nodes(&[("A", 0), ("", 1)]));
        assert_eq!(kind_for("urn:microsoft.com/office/officeart/2005/8/layout/orgChart1", ""), "orgChart");
        assert_eq!(kind_for("urn:x/layout/NameandTitleOrganizationalChart", ""), "nameTitleOrgChart");
        assert_eq!(kind_for("urn:x/layout/chevron2", "process"), "chevron");
        assert_eq!(kind_for("urn:x/layout/hProcess9", "process"), "process");
        assert_eq!(kind_for("urn:x/layout/default", "list"), "blockList");
        assert_eq!(colors_for(&cs_id("accent4")), "accent4");
        assert_eq!(colors_for(&cs_id("")), "");
        assert_eq!(style_for(&qs_id("outline")), "outline");
    }
}
