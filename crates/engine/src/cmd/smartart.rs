//! SmartArt: a diagram's outline (the Text Pane) and the shapes generated from it.
//!
//! A SmartArt graphic is a group whose `diagram` holds its layout and its nodes (text + outline
//! level). Every edit changes the nodes and regenerates the group's children inside the group's
//! child rectangle, so the diagram keeps its size and place. Text typed straight into a diagram
//! shape is read back into the nodes before each regeneration.

use deckcraft_color::SchemeSlot;
use deckcraft_geom::Xfrm;
use deckcraft_model::style::{Gradient, GradientShape, GradientStop, Shadow};
use deckcraft_model::text::{Align, AutoFit, Paragraph, TextBody};
use deckcraft_model::{
    ColorRef, ColorTransform, Diagram, DiagramNode, Effects, Fill, Geom, Line, Presentation, Shape, ShapeId, ShapeKind, ShapeStyle,
};
use serde_json::{Value, json};

use super::{CommandSpec, NO_ID_SPACE, bad, bool_or, cmd, current_shapes, has_selection, shape_of, str_param, targets, xfrm_of};
use crate::{EngineError, Result, Session};

/// Deepest outline level the Text Pane allows.
pub const MAX_LEVEL: u8 = 8;
/// Most top-level shapes a layout draws.
const MAX_SHAPES: usize = 40;

/// DeckCraft's layouts, with the names PowerPoint's gallery gives them.
pub const LAYOUTS: [(&str, &str); 45] = [
    ("list", "Vertical Bullet List"),
    ("blockList", "Basic Block List"),
    ("hBulletList", "Horizontal Bullet List"),
    ("process", "Basic Process"),
    ("chevron", "Basic Chevron Process"),
    ("verticalProcess", "Vertical Process"),
    ("stepUp", "Step Up Process"),
    ("cycle", "Basic Cycle"),
    ("blockCycle", "Block Cycle"),
    ("radial", "Basic Radial"),
    ("hierarchy", "Hierarchy"),
    ("pyramid", "Basic Pyramid"),
    ("invertedPyramid", "Inverted Pyramid"),
    ("matrix", "Basic Matrix"),
    ("titledMatrix", "Titled Matrix"),
    ("pictureList", "Vertical Picture List"),
    ("pictureCaption", "Picture Caption List"),
    ("pictureProcess", "Picture Accent Process"),
    ("pictureHList", "Horizontal Picture List"),
    ("timeline", "Basic Timeline"),
    ("venn", "Basic Venn"),
    ("target", "Basic Target"),
    ("textCycle", "Text Cycle"),
    ("continuousCycle", "Continuous Cycle"),
    ("horizontalHierarchy", "Horizontal Hierarchy"),
    ("gridMatrix", "Grid Matrix"),
    ("gear", "Gear"),
    ("funnel", "Funnel"),
    ("segmentedCycle", "Segmented Cycle"),
    ("multidirectionalCycle", "Multidirectional Cycle"),
    ("orgChart", "Organization Chart"),
    ("halfCircleOrgChart", "Half Circle Organization Chart"),
    ("cycleMatrix", "Cycle Matrix"),
    ("segmentedPyramid", "Segmented Pyramid"),
    ("pie", "Basic Pie"),
    ("nameTitleOrgChart", "Name and Title Organization Chart"),
    ("verticalBoxList", "Vertical Box List"),
    ("trapezoidList", "Trapezoid List"),
    ("tableList", "Table List"),
    ("accentProcess", "Accent Process"),
    ("continuousBlockProcess", "Continuous Block Process"),
    ("bendingProcess", "Basic Bending Process"),
    ("linearVenn", "Linear Venn"),
    ("stackedVenn", "Stacked Venn"),
    ("nestedTarget", "Nested Target"),
];
/// SmartArt Styles.
pub const STYLES: [(&str, &str); 9] = [
    ("flat", "Simple Fill"),
    ("subtle", "Subtle Effect"),
    ("intense", "Intense Effect"),
    ("outline", "White Outline"),
    ("polished", "Polished"),
    ("inset", "Inset"),
    ("cartoon", "Cartoon"),
    ("powder", "Powder"),
    ("brick", "Brick Scene"),
];
/// Change Colors.
pub const COLORS: [(&str, &str); 17] = [
    ("colorful", "Colorful"),
    ("range23", "Colorful Range - Accent Colors 2 to 3"),
    ("range34", "Colorful Range - Accent Colors 3 to 4"),
    ("range45", "Colorful Range - Accent Colors 4 to 5"),
    ("range56", "Colorful Range - Accent Colors 5 to 6"),
    ("accent1", "Accent 1"),
    ("accent2", "Accent 2"),
    ("accent3", "Accent 3"),
    ("accent4", "Accent 4"),
    ("accent5", "Accent 5"),
    ("accent6", "Accent 6"),
    ("accent1Grad", "Gradient Range - Accent 1"),
    ("accent2Grad", "Gradient Range - Accent 2"),
    ("accent3Grad", "Gradient Range - Accent 3"),
    ("accent4Grad", "Gradient Range - Accent 4"),
    ("accent5Grad", "Gradient Range - Accent 5"),
    ("accent6Grad", "Gradient Range - Accent 6"),
];

/// The gallery category a layout is shown under in Convert to SmartArt and Change Layout.
pub fn category(kind: &str) -> &'static str {
    match kind {
        "pictureList" | "pictureCaption" | "pictureProcess" | "pictureHList" => "picture",
        "process"
        | "chevron"
        | "verticalProcess"
        | "stepUp"
        | "timeline"
        | "gear"
        | "funnel"
        | "accentProcess"
        | "continuousBlockProcess"
        | "bendingProcess" => "process",
        "cycle" | "blockCycle" | "textCycle" | "continuousCycle" | "segmentedCycle" | "multidirectionalCycle" | "pie" => "cycle",
        "hierarchy" | "horizontalHierarchy" | "orgChart" | "halfCircleOrgChart" | "nameTitleOrgChart" => "hierarchy",
        "radial" | "venn" | "target" | "linearVenn" | "stackedVenn" | "nestedTarget" => "relationship",
        "matrix" | "titledMatrix" | "gridMatrix" | "cycleMatrix" => "matrix",
        "pyramid" | "invertedPyramid" | "segmentedPyramid" => "pyramid",
        _ => "list",
    }
}

/// A layout's gallery picture: the graphic drawn with sample nodes (or `nodes`) in a 320 × 200 pt
/// group at the origin, in the style and colours given, with no text in the samples.
pub fn preview(kind: &str, style: &str, colors: &str, nodes: Option<&[DiagramNode]>) -> Shape {
    let sample = |levels: &[u8]| -> Vec<DiagramNode> { levels.iter().map(|l| DiagramNode { level: *l, ..Default::default() }).collect() };
    let nodes = match nodes {
        Some(n) if !n.is_empty() => n.to_vec(),
        _ => match category(kind) {
            "hierarchy" => sample(&[0, 1, 2, 2, 1]),
            "matrix" => sample(&[0, 0, 0, 0, 0]),
            "list" if kind != "blockList" && kind != "tableList" => sample(&[0, 1, 0, 1, 0, 1]),
            _ => sample(&[0, 0, 0, 0]),
        },
    };
    let d = Diagram { kind: kind.into(), nodes, style: style.into(), colors: colors.into(), ..Default::default() };
    let area = Xfrm::new(0.0, 0.0, 320.0, 200.0);
    let mut children = layout(&d, area);
    for (k, c) in children.iter_mut().enumerate() {
        c.id = ShapeId(10_000 + k as u32);
    }
    Shape {
        id: ShapeId(9_999),
        name: "SmartArt preview".into(),
        xfrm: Some(area),
        kind: ShapeKind::Group { children, child: area },
        ..Default::default()
    }
}

pub fn is_kind(k: &str) -> bool {
    LAYOUTS.iter().any(|(id, _)| *id == k)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "smartArt.nodes",
            "Text Pane",
            ["SmartArt Design", "Create Graphic"],
            None,
            "{id?} → {id, kind, nodes: [{text, level}]} (id: the SmartArt group or one of its shapes; default the selection)",
            has_selection,
            nodes
        ),
        cmd!(
            "smartArt.setNodes",
            "Edit SmartArt Text",
            ["SmartArt Design", "Create Graphic"],
            None,
            "{id?, nodes?: [{text, level}], lines?: [string] (leading tabs set the level), index?, text?, title?} (index + text edits one node; title: its Name and Title box)",
            has_selection,
            set_nodes
        ),
        cmd!(
            "smartArt.addNode",
            "Add Shape",
            ["SmartArt Design", "Create Graphic"],
            None,
            "{id?, index?, position?: after|before|below|above|assistant, text?} → {index} (default: after the last node)",
            has_selection,
            add_node
        ),
        cmd!(
            "smartArt.removeNode",
            "Remove Shape",
            ["SmartArt Design", "Create Graphic"],
            None,
            "{id?, index} (its sub-points move up one level)",
            has_selection,
            remove_node
        ),
        cmd!("smartArt.promote", "Promote", ["SmartArt Design", "Create Graphic"], None, "{id?, index}", has_selection, promote),
        cmd!("smartArt.demote", "Demote", ["SmartArt Design", "Create Graphic"], None, "{id?, index}", has_selection, demote),
        cmd!(
            "smartArt.layout",
            "Change Layout",
            ["SmartArt Design", "Layouts"],
            None,
            "{id?, kind: list|blockList|hBulletList|process|chevron|verticalProcess|stepUp|cycle|blockCycle|radial|hierarchy|pyramid|invertedPyramid|matrix|titledMatrix|pictureList|pictureCaption|pictureProcess|pictureHList|timeline|venn|target|textCycle|continuousCycle|horizontalHierarchy|gridMatrix|gear|funnel|segmentedCycle|multidirectionalCycle|orgChart|halfCircleOrgChart|cycleMatrix|segmentedPyramid|pie|nameTitleOrgChart|verticalBoxList|trapezoidList|tableList|accentProcess|continuousBlockProcess|bendingProcess|linearVenn|stackedVenn|nestedTarget}",
            has_selection,
            change_layout
        ),
        cmd!(
            "smartArt.fromText",
            "Convert to SmartArt",
            ["Home", "Paragraph"],
            None,
            "{id?, kind?} → {id} (a text box or placeholder becomes a SmartArt graphic in its place: one node per paragraph, indent levels kept)",
            has_selection,
            from_text
        ),
        cmd!(
            "smartArt.toText",
            "Convert to Text",
            ["SmartArt Design", "Reset"],
            None,
            "{id?} → {id} (the graphic becomes a text box of its outline, bulleted by level)",
            has_selection,
            to_text
        ),
        cmd!(
            "smartArt.toShapes",
            "Convert to Shapes",
            ["SmartArt Design", "Reset"],
            None,
            "{id?} (the graphic becomes a plain group of its shapes; the outline is dropped)",
            has_selection,
            to_shapes
        ),
        cmd!(
            "smartArt.picture",
            "Insert Picture in SmartArt",
            ["SmartArt Design", "Create Graphic"],
            None,
            "{id?, index, data?: base64 | path?: string | reset?: bool} (the node's picture placeholder in picture layouts)",
            has_selection,
            set_picture
        ),
        cmd!(
            "smartArt.style",
            "SmartArt Styles",
            ["SmartArt Design", "SmartArt Styles"],
            None,
            "{id?, style: flat|subtle|intense|outline|polished|inset|cartoon|powder|brick}",
            has_selection,
            change_style
        ),
        cmd!(
            "smartArt.colors",
            "Change Colors",
            ["SmartArt Design", "SmartArt Styles"],
            None,
            "{id?, colors: colorful|range23|range34|range45|range56|accent1|…|accent6|accent1Grad|…|accent6Grad}",
            has_selection,
            change_colors
        ),
    ]
}

/// Nodes from text lines: each leading tab is one outline level.
pub fn nodes_from_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<DiagramNode> {
    lines
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let tabs = l.chars().take_while(|c| *c == '\t').count();
            DiagramNode {
                text: l.trim().to_string(),
                level: u8::try_from(tabs).unwrap_or(MAX_LEVEL).min(MAX_LEVEL),
                picture: None,
                assistant: false,
                title: String::new(),
            }
        })
        .collect()
}

/// Levels made consistent: the first node is top level and no node is more than one level
/// deeper than the node before it.
pub fn normalize(nodes: &mut [DiagramNode]) {
    let mut prev: Option<u8> = None;
    for n in nodes.iter_mut() {
        n.level = match prev {
            None => 0,
            Some(p) => n.level.min(p.saturating_add(1)).min(MAX_LEVEL),
        };
        prev = Some(n.level);
    }
}

/// Index one past the last descendant of node `i`.
fn subtree_end(nodes: &[DiagramNode], i: usize) -> usize {
    let lv = nodes.get(i).map(|n| n.level).unwrap_or(0);
    let mut j = i + 1;
    while nodes.get(j).is_some_and(|n| n.level > lv) {
        j += 1;
    }
    j
}

/// Add a node next to node `index` (PowerPoint's Add Shape After/Before/Below/Above). Returns the
/// new node's index.
pub fn add(nodes: &mut Vec<DiagramNode>, index: Option<usize>, position: &str, text: &str) -> usize {
    let Some(i) = index.filter(|i| *i < nodes.len()) else {
        nodes.push(DiagramNode { text: text.into(), level: 0, picture: None, assistant: false, title: String::new() });
        return nodes.len() - 1;
    };
    let lv = nodes[i].level;
    let (at, level) = match position {
        "before" => (i, lv),
        "below" => (subtree_end(nodes, i), (lv + 1).min(MAX_LEVEL)),
        "above" => {
            let end = subtree_end(nodes, i);
            for n in &mut nodes[i..end] {
                n.level = (n.level + 1).min(MAX_LEVEL);
            }
            (i, lv)
        }
        "assistant" => (i + 1, (lv + 1).min(MAX_LEVEL)),
        _ => (subtree_end(nodes, i), lv),
    };
    nodes.insert(at, DiagramNode { text: text.into(), level, picture: None, assistant: position == "assistant", title: String::new() });
    at
}

/// Remove node `i`; its sub-points move up one level.
pub fn remove(nodes: &mut Vec<DiagramNode>, i: usize) -> bool {
    if i >= nodes.len() {
        return false;
    }
    let end = subtree_end(nodes, i);
    for n in &mut nodes[i + 1..end] {
        n.level = n.level.saturating_sub(1);
    }
    nodes.remove(i);
    normalize(nodes);
    true
}

/// Move node `i` and its sub-points one level up. False when it is already top level.
pub fn promote_node(nodes: &mut [DiagramNode], i: usize) -> bool {
    if nodes.get(i).is_none_or(|n| n.level == 0) {
        return false;
    }
    let end = subtree_end(nodes, i);
    for n in &mut nodes[i..end] {
        n.level -= 1;
    }
    true
}

/// Move node `i` and its sub-points one level down, under the node before it. False when there is
/// nothing to go under.
pub fn demote_node(nodes: &mut [DiagramNode], i: usize) -> bool {
    let (Some(prev), Some(cur)) = (i.checked_sub(1).and_then(|p| nodes.get(p)), nodes.get(i)) else { return false };
    if cur.level > prev.level || cur.level >= MAX_LEVEL {
        return false;
    }
    let end = subtree_end(nodes, i);
    if nodes[i..end].iter().any(|n| n.level >= MAX_LEVEL) {
        return false;
    }
    for n in &mut nodes[i..end] {
        n.level += 1;
    }
    true
}

/// A generated shape and the nodes whose text it shows (none for arrows and lines).
type Item = (Shape, Vec<usize>);

/// Top-level nodes, each with its sub-points.
fn groups(d: &Diagram) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = vec![];
    for (i, n) in d.nodes.iter().enumerate() {
        match out.last_mut() {
            Some(g) if n.level > 0 => g.push(i),
            _ => out.push(vec![i]),
        }
    }
    out.truncate(MAX_SHAPES);
    out
}

/// Read text typed into the diagram's shapes back into its nodes.
fn sync_text(d: &mut Diagram, children: &[Shape]) {
    if d.imported {
        return;
    }
    let map: Vec<Vec<usize>> = layout_items(d, Xfrm::new(0.0, 0.0, 100.0, 100.0)).into_iter().map(|(_, m)| m).collect();
    for (shape, idx) in children.iter().zip(map) {
        let Some(body) = &shape.text else { continue };
        if idx.is_empty() || body.paragraphs.len() != idx.len() {
            continue;
        }
        for (p, i) in body.paragraphs.iter().zip(idx) {
            if let Some(n) = d.nodes.get_mut(i) {
                n.text = p.text();
            }
        }
    }
}

fn slot(i: usize) -> SchemeSlot {
    [SchemeSlot::Accent1, SchemeSlot::Accent2, SchemeSlot::Accent3, SchemeSlot::Accent4, SchemeSlot::Accent5, SchemeSlot::Accent6][i % 6]
}

/// The accent shape `i` is drawn in under Change Colors: `colorful` cycles the accents, `accentN`
/// keeps one.
fn accent(d: &Diagram, i: usize) -> SchemeSlot {
    let digit = |s: &str, k: usize| s.chars().nth(k).and_then(|c| c.to_digit(10)).map(|n| n as usize);
    if let Some(r) = d.colors.strip_prefix("range")
        && let Some(a @ 1..=6) = digit(r, 0)
    {
        // Colorful Range: alternate between two neighbouring accents.
        return slot(a - 1 + i % 2);
    }
    match d.colors.strip_prefix("accent").and_then(|r| digit(r, 0)) {
        Some(n @ 1..=6) => slot(n - 1),
        _ => slot(i),
    }
}

/// Gradient Range: each shape a step darker than the one before (the steps cycle).
fn shade(d: &Diagram, i: usize) -> Option<ColorTransform> {
    d.colors.ends_with("Grad").then(|| ColorTransform::LumMod(100_000 - (i % 4) as i32 * 15_000))
}

/// The 3-D XML (`scene3d` + `sp3d`) for a 3-D SmartArt Style: a top bevel (`w`, `h` in points)
/// lit by the three-point rig.
fn three_d(bevel: &str, w: f64, h: f64, camera: &str) -> String {
    format!(
        "<a:scene3d><a:camera prst=\"{camera}\"/><a:lightRig rig=\"threePt\" dir=\"t\"/></a:scene3d><a:sp3d prstMaterial=\"matte\"><a:bevelT w=\"{}\" h=\"{}\" prst=\"{bevel}\"/></a:sp3d>",
        (w * 12700.0).round() as i64,
        (h * 12700.0).round() as i64
    )
}

/// Brick Scene's camera: the diagram's shapes seen from above and to the side.
pub const BRICK_CAMERA: &str = "isometricOffAxis2Top";

/// Whether `style` is one of the 3-D SmartArt Styles.
pub fn is_3d(style: &str) -> bool {
    matches!(style, "polished" | "inset" | "cartoon" | "powder" | "brick")
}

/// Fill, outline, text colour and effects for the SmartArt style. `light` shapes (the bullet
/// boxes of the bullet lists) are a pale tint of the accent with dark text.
fn paint(d: &Diagram, sh: &mut Shape, i: usize, light: bool) {
    let a = accent(d, i);
    let base = match shade(d, i) {
        Some(t) => ColorRef::scheme(a).with(t),
        None => ColorRef::scheme(a),
    };
    let tint = |pct: i32| ColorRef::scheme(a).with(ColorTransform::LumMod(100_000 - pct * 1000)).with(ColorTransform::LumOff(pct * 1000));
    let dark_text = light || matches!(d.style.as_str(), "outline" | "powder");
    sh.style = Some(ShapeStyle::accent(a));
    match d.style.as_str() {
        _ if light => {
            sh.fill = Some(Fill::solid(tint(80)));
            sh.line = Some(Line::solid(tint(60), 1.0));
        }
        "outline" => {
            sh.fill = Some(Fill::solid(ColorRef::scheme(SchemeSlot::Lt1)));
            sh.line = Some(Line::solid(base.clone(), 2.25));
        }
        "subtle" => {
            sh.fill = Some(Fill::solid(base.clone()));
            sh.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.5));
            sh.effects = Some(Effects { outer_shadow: Some(shadow(4.0, 3.0)), ..Default::default() });
        }
        "polished" | "inset" | "cartoon" | "powder" | "brick" => {
            let (bevel, bw, line, fill) = match d.style.as_str() {
                "polished" => ("circle", 12.0, Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 0.75)), base.clone()),
                "inset" => ("relaxedInset", 10.0, None, base.clone()),
                "cartoon" => ("softRound", 8.0, Some(Line::solid(ColorRef::scheme(SchemeSlot::Tx1), 2.0)), base.clone()),
                "powder" => ("convex", 14.0, None, tint(40)),
                _ => ("slope", 6.0, Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.0)), base.clone()),
            };
            sh.fill = Some(Fill::solid(fill));
            sh.line = Some(line.unwrap_or_else(|| Line { fill: Some(Fill::None), ..Default::default() }));
            sh.effects = Some(Effects {
                outer_shadow: (d.style != "inset").then(|| shadow(6.0, 4.0)),
                bevel: Some(bevel.into()),
                raw3d: Some(three_d(bevel, bw, bw * 0.6, if d.style == "brick" { BRICK_CAMERA } else { "orthographicFront" })),
                ..Default::default()
            });
        }
        "intense" => {
            sh.fill = Some(Fill::Gradient(Gradient {
                stops: vec![
                    GradientStop { pos: 0.0, color: tint(30) },
                    GradientStop { pos: 1.0, color: base.clone().with(ColorTransform::LumMod(75_000)) },
                ],
                shape: GradientShape::Linear { angle: 90.0, scaled: false },
                rotate_with_shape: true,
                tile: [0.0; 4],
            }));
            sh.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.0));
            sh.effects = Some(Effects { outer_shadow: Some(shadow(8.0, 5.0)), ..Default::default() });
        }
        _ => {
            sh.fill = Some(Fill::solid(base.clone()));
            sh.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.5));
        }
    }
    if let Some(body) = &mut sh.text {
        let c = if dark_text { ColorRef::scheme(SchemeSlot::Tx1) } else { ColorRef::scheme(SchemeSlot::Lt1) };
        for p in &mut body.paragraphs {
            for r in &mut p.runs {
                r.props.fill = Some(Fill::solid(c.clone()));
            }
        }
    }
}

fn shadow(blur: f64, dist: f64) -> Shadow {
    Shadow {
        color: ColorRef::rgb(deckcraft_model::Rgba { r: 0, g: 0, b: 0, a: 255 }).with(ColorTransform::Alpha(40_000)),
        blur,
        dist,
        dir: 90.0,
        inner: false,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: String::new(),
        rotate_with_shape: false,
    }
}

/// A shape with text: the first line as the title (centred, larger), the rest as bullets. With
/// `title` false every line is a bullet (the bullet boxes of the bullet lists).
fn text_shape(preset: &str, x: Xfrm, lines: &[&DiagramNode], title: bool) -> Shape {
    let mut body = TextBody::default();
    body.body.autofit = Some(AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 });
    let top = lines.first().map(|n| if title { n.level } else { n.level.saturating_sub(1) }).unwrap_or(0);
    for (k, n) in lines.iter().enumerate() {
        let mut para = Paragraph::new(&n.text);
        let head = title && k == 0;
        if head {
            para.props.align = Some(Align::Center);
        } else {
            para.level = n.level.saturating_sub(top).saturating_sub(1).min(MAX_LEVEL);
            para.props.align = Some(Align::Left);
            para.props.bullet = Some(deckcraft_model::text::Bullet::Char { char: "•".into() });
            para.props.margin_left = Some(12.0 * (para.level as f64 + 1.0));
            para.props.indent = Some(-10.0);
        }
        let size = if head { 20.0 } else { 14.0 };
        for r in &mut para.runs {
            r.props.size = Some(size);
        }
        para.end_props.size = Some(size);
        body.paragraphs.push(para);
    }
    if body.paragraphs.is_empty() {
        body.paragraphs.push(Paragraph::new(""));
    }
    if !title {
        body.body.anchor = Some(deckcraft_model::text::Anchor::Top);
    }
    Shape { name: "SmartArt shape".into(), xfrm: Some(x), geom: Geom::preset(preset), text: Some(body), ..Default::default() }
}

/// A plain shape with no text (arrows between steps).
fn deco(preset: &str, x: Xfrm, rot: f64) -> Shape {
    let mut x = x;
    x.rot = rot;
    Shape { name: "SmartArt arrow".into(), xfrm: Some(x), geom: Geom::preset(preset), ..Default::default() }
}

fn line_shape(a: (f64, f64), b: (f64, f64)) -> Shape {
    let mut x = Xfrm::new(a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs());
    x.flip_h = (b.0 < a.0) != (b.1 < a.1);
    Shape {
        name: "SmartArt connector".into(),
        xfrm: Some(x),
        geom: Geom::preset("line"),
        fill: Some(Fill::None),
        line: Some(Line::solid(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::LumMod(60_000)), 1.25)),
        ..Default::default()
    }
}

/// The diagram's shapes laid out in `area` (ids not yet assigned).
pub fn layout(d: &Diagram, area: Xfrm) -> Vec<Shape> {
    layout_items(d, area).into_iter().map(|(s, _)| s).collect()
}

/// Lay out `d`, painted with its style and colours, each shape with the nodes it shows; text is
/// shrunk per shape so its longest word fits on a line.
fn layout_items(d: &Diagram, area: Xfrm) -> Vec<Item> {
    let mut v = layout_raw(d, area);
    for (sh, _) in &mut v {
        fit_words(sh);
    }
    v
}

/// Rough advance of a character as a fraction of the font size (a sans text face).
const CHAR_W: f64 = 0.6;
/// Smallest font scale shrinking to fit goes to.
const MIN_SCALE: f64 = 0.25;

/// Shrink the shape's text (all paragraphs together, as PowerPoint's per-shape font size does) so
/// no word is wider than its line and would break mid-word. Overflow in height is left to the
/// body's shrink-on-overflow autofit.
fn fit_words(sh: &mut Shape) {
    let Some(w) = sh.xfrm.map(|x| x.w) else { return };
    // Round shapes keep their text in the inscribed rectangle (about 71% of the width).
    let text_w = match sh.geom.preset_name() {
        Some("ellipse" | "gear6" | "gear9") => w * 0.71,
        _ => w,
    };
    let Some(body) = &mut sh.text else { return };
    let avail = text_w - body.body.inset_l.unwrap_or(7.2) - body.body.inset_r.unwrap_or(7.2);
    let mut scale: f64 = 1.0;
    for p in &body.paragraphs {
        let line = avail - p.props.margin_left.unwrap_or(0.0).max(0.0);
        for r in &p.runs {
            let size = r.props.size.unwrap_or(18.0);
            let word = r.text.split_whitespace().map(|t| t.chars().count()).max().unwrap_or(0) as f64;
            let need = word * size * CHAR_W;
            if need > 0.0 {
                scale = scale.min(line.max(0.0) / need);
            }
        }
    }
    let scale = scale.max(MIN_SCALE);
    if scale >= 0.999 {
        return;
    }
    let shrink = |v: &mut Option<f64>| {
        if let Some(x) = v {
            *x = (*x * scale * 2.0).floor() / 2.0;
        }
    };
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            shrink(&mut r.props.size);
        }
        shrink(&mut p.end_props.size);
        shrink(&mut p.props.margin_left);
        if let Some(i) = &mut p.props.indent {
            *i *= scale;
        }
    }
}

/// A picture placeholder for node `node`: its picture when it has one, else a pale box.
fn picture_shape(d: &Diagram, preset: &str, x: Xfrm, i: usize, node: usize) -> Shape {
    let mut sh = Shape { name: "SmartArt picture".into(), xfrm: Some(x), geom: Geom::preset(preset), ..Default::default() };
    paint(d, &mut sh, i, true);
    if let Some(m) = d.nodes.get(node).and_then(|n| n.picture) {
        sh.fill = Some(Fill::Picture(deckcraft_model::style::PictureFill { media: m, ..Default::default() }));
        sh.line = Some(Line::solid(ColorRef::scheme(SchemeSlot::Lt1), 1.5));
    }
    sh
}

fn layout_raw(d: &Diagram, area: Xfrm) -> Vec<Item> {
    let gs = groups(d);
    let nodes = |g: &[usize]| -> Vec<&DiagramNode> { g.iter().filter_map(|i| d.nodes.get(*i)).collect() };
    let n = gs.len().max(1);
    let nf = n as f64;
    let mut out: Vec<Item> = vec![];
    // A titled shape for top-level group `i`, painted in its accent.
    let titled = |out: &mut Vec<Item>, preset: &str, x: Xfrm, i: usize, g: &[usize]| {
        let mut sh = text_shape(preset, x, &nodes(g), true);
        paint(d, &mut sh, i, false);
        out.push((sh, g.to_vec()));
    };
    let arrow = |out: &mut Vec<Item>, x: Xfrm, rot: f64, i: usize| {
        let mut sh = deco("rightArrow", x, rot);
        paint(d, &mut sh, i, true);
        out.push((sh, vec![]));
    };
    match d.kind.as_str() {
        "hierarchy" => return hierarchy(d, area, false),
        "horizontalHierarchy" => return hierarchy(d, area, true),
        "orgChart" | "halfCircleOrgChart" | "nameTitleOrgChart" => return org_chart(d, area),
        "list" | "hBulletList" => {
            // Vertical / Horizontal Bullet List: a coloured title bar, sub-points in a pale box below.
            let vertical = d.kind == "list";
            for (i, g) in gs.iter().enumerate() {
                let (cell, bar_h) = if vertical {
                    (
                        Xfrm::new(area.x, area.y + i as f64 * area.h / nf, area.w, area.h / nf * 0.92),
                        (area.h / nf * 0.92).min(area.h * 0.12).max(18.0),
                    )
                } else {
                    let w = area.w / nf;
                    (Xfrm::new(area.x + i as f64 * w + w * 0.03, area.y, w * 0.94, area.h), area.h * 0.22)
                };
                let has_sub = g.len() > 1;
                let bar_h = bar_h.min(cell.h);
                titled(&mut out, "roundRect", Xfrm::new(cell.x, cell.y, cell.w, bar_h), i, &g[..1]);
                if has_sub {
                    let pad = cell.w * 0.03;
                    let x = Xfrm::new(cell.x + pad, cell.y + bar_h, cell.w - 2.0 * pad, (cell.h - bar_h).max(1.0));
                    let mut sh = text_shape("rect", x, &nodes(&g[1..]), false);
                    paint(d, &mut sh, i, true);
                    out.push((sh, g[1..].to_vec()));
                }
            }
        }
        "blockList" => {
            // Basic Block List: equal blocks in rows, as square as the area allows.
            let cols = ((nf * area.w / area.h.max(1.0) / 1.6).sqrt().ceil() as usize).clamp(1, n);
            let rows = n.div_ceil(cols);
            let (w, h) = (area.w / cols as f64, area.h / rows as f64);
            let (bw, bh) = (w * 0.9, (h * 0.85).min(w * 0.9 * 0.6));
            for (i, g) in gs.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let in_row = if r + 1 == rows { n - r * cols } else { cols };
                let x0 = area.x + (area.w - in_row as f64 * w) / 2.0;
                titled(&mut out, "rect", Xfrm::new(x0 + c as f64 * w + (w - bw) / 2.0, area.y + r as f64 * h + (h - bh) / 2.0, bw, bh), i, g);
            }
        }
        "process" | "chevron" => {
            let chevron = d.kind == "chevron";
            let gap = if chevron { -area.w * 0.02 } else { area.w * 0.06 };
            let w = ((area.w - gap * (nf - 1.0)) / nf).max(1.0);
            let h = (area.h * 0.5).min(w * if chevron { 0.45 } else { 0.65 });
            let y = area.y + (area.h - h) / 2.0;
            for (i, g) in gs.iter().enumerate() {
                let x = area.x + i as f64 * (w + gap);
                titled(&mut out, if chevron { "chevron" } else { "roundRect" }, Xfrm::new(x, y, w, h), i, g);
                if !chevron && i + 1 < n {
                    let aw = gap * 0.7;
                    arrow(&mut out, Xfrm::new(x + w + gap * 0.15, y + h / 2.0 - aw * 0.45, aw, aw * 0.9), 0.0, i);
                }
            }
        }
        "verticalProcess" => {
            let gap = area.h * 0.06;
            let h = ((area.h - gap * (nf - 1.0)) / nf).max(1.0);
            let w = area.w * 0.6;
            let x = area.x + (area.w - w) / 2.0;
            for (i, g) in gs.iter().enumerate() {
                let y = area.y + i as f64 * (h + gap);
                titled(&mut out, "roundRect", Xfrm::new(x, y, w, h), i, g);
                if i + 1 < n {
                    let s = gap * 0.8;
                    // A right arrow turned to point down.
                    arrow(&mut out, Xfrm::new(area.x + area.w / 2.0 - s / 2.0, y + h + gap * 0.1, s, s), 90.0, i);
                }
            }
        }
        "stepUp" => {
            // Step Up Process: each step higher than the one before.
            let w = area.w / nf;
            let h = area.h / (nf + 1.0) * 1.6;
            for (i, g) in gs.iter().enumerate() {
                let y = area.y + area.h - h - i as f64 * (area.h - h) / (nf - 1.0).max(1.0);
                titled(&mut out, "rect", Xfrm::new(area.x + i as f64 * w + w * 0.04, y, w * 0.92, h), i, g);
            }
        }
        "cycle" | "blockCycle" => {
            let block = d.kind == "blockCycle";
            let r = area.h.min(area.w) * 0.36;
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dd = (r * if block { 0.75 } else { 0.8 }).min(2.0 * std::f64::consts::PI * r / nf * 0.62);
            for (i, g) in gs.iter().enumerate() {
                let a = (i as f64 / nf * 360.0 - 90.0).to_radians();
                let (w, h) = if block { (dd * 1.3, dd * 0.75) } else { (dd, dd) };
                titled(
                    &mut out,
                    if block { "roundRect" } else { "ellipse" },
                    Xfrm::new(cx + r * a.cos() - w / 2.0, cy + r * a.sin() - h / 2.0, w, h),
                    i,
                    g,
                );
                if n > 1 {
                    // An arrow halfway to the next node, along the circle.
                    let m = ((i as f64 + 0.5) / nf * 360.0 - 90.0).to_radians();
                    let s = (dd * 0.35).max(6.0);
                    let rot = (i as f64 + 0.5) / nf * 360.0;
                    arrow(&mut out, Xfrm::new(cx + r * m.cos() - s / 2.0, cy + r * m.sin() - s * 0.4, s, s * 0.8), rot, i);
                }
            }
        }
        "radial" => {
            // Basic Radial: the first node in the middle, the others around it with spokes.
            let Some((centre, rest)) = gs.split_first() else { return out };
            let m = rest.len().max(1) as f64;
            let r = area.h.min(area.w) * 0.36;
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dc = r * 0.75;
            let dd = (r * 0.75).min(2.0 * std::f64::consts::PI * r / m * 0.7);
            for (k, _) in rest.iter().enumerate() {
                let a = (k as f64 / m * 360.0 - 90.0).to_radians();
                let (ex, ey) = (cx + (r - dd / 2.0) * a.cos(), cy + (r - dd / 2.0) * a.sin());
                out.push((line_shape((cx + dc / 2.0 * a.cos(), cy + dc / 2.0 * a.sin()), (ex, ey)), vec![]));
            }
            titled(&mut out, "ellipse", Xfrm::new(cx - dc / 2.0, cy - dc / 2.0, dc, dc), 0, centre);
            for (k, g) in rest.iter().enumerate() {
                let a = (k as f64 / m * 360.0 - 90.0).to_radians();
                titled(&mut out, "ellipse", Xfrm::new(cx + r * a.cos() - dd / 2.0, cy + r * a.sin() - dd / 2.0, dd, dd), k + 1, g);
            }
        }
        "pyramid" | "invertedPyramid" => {
            // One triangle cut into bands; inverted, the wide band is on top. Each band is drawn as
            // its own polygon so the sides keep the triangle's slope and the text stays upright.
            let inverted = d.kind == "invertedPyramid";
            let h = area.h / nf;
            let pw = area.h.min(area.w) * 1.15;
            let px = area.x + (area.w - pw) / 2.0;
            for (i, g) in gs.iter().enumerate() {
                let (a, b) = (i as f64 / nf, (i as f64 + 1.0) / nf);
                let (t, bt) = if inverted { (1.0 - a, 1.0 - b) } else { (a, b) };
                let (wt, wb) = (pw * t, pw * bt);
                let w = wt.max(wb).max(1.0);
                let bh = h * 0.96;
                let x = px + (pw - w) / 2.0;
                let (it, ib) = ((w - wt) / 2.0, (w - wb) / 2.0);
                let path = format!("M {it:.2} 0 L {:.2} 0 L {:.2} {bh:.2} L {ib:.2} {bh:.2} Z", w - it, w - ib);
                let mut sh = text_shape("rect", Xfrm::new(x, area.y + i as f64 * h, w, bh), &nodes(g), true);
                sh.geom = Geom::Custom {
                    paths: vec![deckcraft_model::CustomPath { w, h: bh, d: path, fill: deckcraft_geom::preset::FillMode::Norm, stroke: true }],
                };
                // Keep the text inside the sloped sides: inset it to a bit past the band's mid width.
                // The tip band is a triangle: its text sits at the wide end, inset to the width there.
                let tip = wt.min(wb) < 1e-6;
                let inset = if tip { w * 0.22 } else { (it + ib) / 2.0 + (it - ib).abs() / 4.0 + 4.0 };
                if let Some(b) = &mut sh.text {
                    b.body.inset_l = Some(inset);
                    b.body.inset_r = Some(inset);
                    if tip {
                        b.body.anchor = Some(if inverted { deckcraft_model::text::Anchor::Top } else { deckcraft_model::text::Anchor::Bottom });
                    }
                }
                paint(d, &mut sh, i, false);
                out.push((sh, g.clone()));
            }
        }
        "matrix" | "titledMatrix" => {
            let titled_m = d.kind == "titledMatrix";
            let s = area.h.min(area.w);
            let (x0, y0) = (area.x + (area.w - s) / 2.0, area.y + (area.h - s) / 2.0);
            let (quads, title): (&[Vec<usize>], Option<&Vec<usize>>) =
                if titled_m { gs.split_first().map(|(t, r)| (r, Some(t))).unwrap_or((&[], None)) } else { (&gs, None) };
            for (i, g) in quads.iter().take(4).enumerate() {
                let x = Xfrm::new(x0 + (i % 2) as f64 * s / 2.0 + 3.0, y0 + (i / 2) as f64 * s / 2.0 + 3.0, s / 2.0 - 6.0, s / 2.0 - 6.0);
                titled(&mut out, "roundRect", x, i + usize::from(titled_m), g);
            }
            if let Some(t) = title {
                let c = s * 0.42;
                let mut sh = text_shape("roundRect", Xfrm::new(x0 + (s - c) / 2.0, y0 + (s - c * 0.6) / 2.0, c, c * 0.6), &nodes(t), true);
                paint(d, &mut sh, 0, true);
                out.push((sh, t.clone()));
            }
        }
        "pictureList" => {
            // Vertical Picture List: a picture square at the left of each row, the text beside it.
            let h = area.h / nf;
            let bh = h * 0.88;
            for (i, g) in gs.iter().enumerate() {
                let y = area.y + i as f64 * h;
                let s = bh.min(area.w * 0.3);
                out.push((picture_shape(d, "rect", Xfrm::new(area.x, y, s, bh), i, g[0]), vec![]));
                titled(&mut out, "roundRect", Xfrm::new(area.x + s + bh * 0.08, y, (area.w - s - bh * 0.08).max(1.0), bh), i, g);
            }
        }
        "pictureCaption" | "pictureProcess" => {
            // Picture Caption List: a picture over each caption, in rows. Picture Accent Process: one
            // row, arrows between the steps.
            let process = d.kind == "pictureProcess";
            let cols = if process { n } else { ((nf * area.w / area.h.max(1.0) / 1.2).sqrt().ceil() as usize).clamp(1, n) };
            let rows = n.div_ceil(cols);
            let gap = if process { area.w * 0.05 } else { 0.0 };
            let w = (area.w - gap * (cols as f64 - 1.0)) / cols as f64;
            let h = area.h / rows as f64;
            let bw = if process { w } else { w * 0.9 };
            let ch = (h * 0.9).min(bw * 1.1);
            for (i, g) in gs.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let in_row = if r + 1 == rows { n - r * cols } else { cols };
                let x0 = area.x + (area.w - in_row as f64 * w - gap * (in_row as f64 - 1.0)) / 2.0;
                let x = x0 + c as f64 * (w + gap) + (w - bw) / 2.0;
                let y = area.y + r as f64 * h + (h - ch) / 2.0;
                let ph = ch * 0.62;
                out.push((
                    picture_shape(d, if process { "ellipse" } else { "rect" }, Xfrm::new(x + (bw - ph.min(bw)) / 2.0, y, ph.min(bw), ph), i, g[0]),
                    vec![],
                ));
                titled(&mut out, if process { "roundRect" } else { "rect" }, Xfrm::new(x, y + ph + ch * 0.03, bw, ch * 0.35), i, g);
                if process && i + 1 < n {
                    let a = (gap * 0.7).min(ch * 0.2);
                    arrow(&mut out, Xfrm::new(x + bw + (gap - a) / 2.0, y + ph / 2.0 - a / 2.0, a, a), 0.0, i);
                }
            }
        }
        "textCycle" | "continuousCycle" => {
            // Text Cycle: the text alone around a circle, arrows between. Continuous Cycle: boxes on
            // a ring.
            let text_only = d.kind == "textCycle";
            let r = area.h.min(area.w) * 0.36;
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dd = (r * 0.8).min(2.0 * std::f64::consts::PI * r / nf * 0.62);
            if !text_only {
                let ring = r * 2.0 + dd * 0.3;
                let mut sh = deco("donut", Xfrm::new(cx - ring / 2.0, cy - ring / 2.0, ring, ring), 0.0);
                sh.geom = Geom::Preset { name: "donut".into(), adj: vec![7000.0] };
                paint(d, &mut sh, 0, true);
                out.push((sh, vec![]));
            }
            for (i, g) in gs.iter().enumerate() {
                let a = (i as f64 / nf * 360.0 - 90.0).to_radians();
                let (w, h) = (dd * 1.35, dd * 0.7);
                let mut sh = text_shape("roundRect", Xfrm::new(cx + r * a.cos() - w / 2.0, cy + r * a.sin() - h / 2.0, w, h), &nodes(g), true);
                paint(d, &mut sh, i, false);
                if text_only {
                    sh.fill = Some(Fill::None);
                    sh.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                    sh.effects = None;
                    let c = ColorRef::scheme(accent(d, i)).with(ColorTransform::LumMod(75_000));
                    if let Some(b) = &mut sh.text {
                        for p in &mut b.paragraphs {
                            for r in &mut p.runs {
                                r.props.fill = Some(Fill::solid(c.clone()));
                            }
                        }
                    }
                    if n > 1 {
                        let m = ((i as f64 + 0.5) / nf * 360.0 - 90.0).to_radians();
                        let s = (dd * 0.3).max(6.0);
                        arrow(
                            &mut out,
                            Xfrm::new(cx + r * m.cos() - s / 2.0, cy + r * m.sin() - s * 0.4, s, s * 0.8),
                            (i as f64 + 0.5) / nf * 360.0,
                            i,
                        );
                    }
                }
                out.push((sh, g.clone()));
            }
        }
        "segmentedCycle" | "pie" => {
            // Segmented Cycle: a ring cut into one arc per node. Basic Pie: a disc cut into wedges.
            // The text sits in a clear box over each segment's middle.
            let ring = d.kind == "segmentedCycle";
            let s = area.h.min(area.w);
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let gap = if ring { 2.0 } else { 0.0 };
            for (i, g) in gs.iter().enumerate() {
                // Angles clockwise from 3 o'clock, starting at the top.
                let a0 = i as f64 / nf * 360.0 - 90.0 + gap / 2.0;
                let a1 = (i as f64 + 1.0) / nf * 360.0 - 90.0 - gap / 2.0;
                let mut sh = deco(if ring { "blockArc" } else { "pie" }, Xfrm::new(cx - s / 2.0, cy - s / 2.0, s, s), 0.0);
                let ang = |a: f64| (a.rem_euclid(360.0) * 60_000.0).round();
                // blockArc runs from its first angle to its second clockwise; so does pie.
                sh.geom = Geom::Preset {
                    name: if ring { "blockArc" } else { "pie" }.into(),
                    adj: if ring { vec![ang(a0), ang(a1), 25_000.0] } else { vec![ang(a0), ang(a1)] },
                };
                paint(d, &mut sh, i, false);
                out.push((sh, vec![]));
                let m = ((a0 + a1) / 2.0).to_radians();
                let rr = if ring { s * 0.4375 } else { s * 0.3 };
                let tw = (s * if ring { 0.22 } else { 0.3 }).min(std::f64::consts::PI * 2.0 * rr / nf * 0.9);
                let th = s * 0.16;
                let mut t = text_shape("rect", Xfrm::new(cx + rr * m.cos() - tw / 2.0, cy + rr * m.sin() - th / 2.0, tw, th), &nodes(g), true);
                paint(d, &mut t, i, false);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                t.effects = None;
                out.push((t, g.clone()));
            }
        }
        "multidirectionalCycle" => {
            // Multidirectional Cycle: nodes around a circle, double-headed arrows between them.
            let r = area.h.min(area.w) * 0.36;
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dd = (r * 0.75).min(2.0 * std::f64::consts::PI * r / nf * 0.55);
            for (i, g) in gs.iter().enumerate() {
                let a = (i as f64 / nf * 360.0 - 90.0).to_radians();
                titled(&mut out, "roundRect", Xfrm::new(cx + r * a.cos() - dd * 0.65, cy + r * a.sin() - dd * 0.4, dd * 1.3, dd * 0.8), i, g);
                if n > 1 {
                    let m = ((i as f64 + 0.5) / nf * 360.0 - 90.0).to_radians();
                    let s = (dd * 0.5).max(8.0);
                    let mut sh = deco(
                        "leftRightArrow",
                        Xfrm::new(cx + r * m.cos() - s / 2.0, cy + r * m.sin() - s * 0.2, s, s * 0.4),
                        (i as f64 + 0.5) / nf * 360.0,
                    );
                    paint(d, &mut sh, i, true);
                    out.push((sh, vec![]));
                }
            }
        }
        "cycleMatrix" => {
            // Cycle Matrix: four quarters of a circle in the middle with the top-level text, each
            // quarter's sub-points in a box at its corner.
            let s = area.h.min(area.w);
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let c = s * 0.56;
            for (i, g) in gs.iter().take(4).enumerate() {
                let (sx, sy) = ([-1.0, 1.0, 1.0, -1.0][i], [-1.0, -1.0, 1.0, 1.0][i]);
                let b = s * 0.4;
                let bx = Xfrm::new(cx + if sx < 0.0 { -s / 2.0 } else { s / 2.0 - b }, cy + if sy < 0.0 { -s / 2.0 } else { s / 2.0 - b }, b, b);
                if g.len() > 1 {
                    let mut sh = text_shape("roundRect", bx, &nodes(&g[1..]), false);
                    paint(d, &mut sh, i, true);
                    if let Some(body) = &mut sh.text {
                        // Text in the box's outer part, clear of the circle.
                        body.body.inset_l = Some(if sx > 0.0 { b * 0.35 } else { 6.0 });
                        body.body.inset_r = Some(if sx < 0.0 { b * 0.35 } else { 6.0 });
                        body.body.anchor = Some(if sy < 0.0 { deckcraft_model::text::Anchor::Top } else { deckcraft_model::text::Anchor::Bottom });
                    }
                    out.push((sh, g[1..].to_vec()));
                }
            }
            for (i, g) in gs.iter().take(4).enumerate() {
                let (sx, sy) = ([-1.0, 1.0, 1.0, -1.0][i], [-1.0, -1.0, 1.0, 1.0][i]);
                let a0 = [180.0, 270.0, 0.0, 90.0][i] + 1.5;
                let mut sh = deco("pie", Xfrm::new(cx - c / 2.0, cy - c / 2.0, c, c), 0.0);
                sh.geom = Geom::Preset { name: "pie".into(), adj: vec![a0 * 60_000.0, (a0 + 87.0) * 60_000.0] };
                paint(d, &mut sh, i, false);
                out.push((sh, vec![]));
                let q = c * 0.3;
                let mut t = text_shape("rect", Xfrm::new(cx + sx * c * 0.2 - q / 2.0, cy + sy * c * 0.2 - q / 2.0, q, q), &nodes(&g[..1]), true);
                paint(d, &mut t, i, false);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                t.effects = None;
                out.push((t, g[..1].to_vec()));
            }
        }
        "segmentedPyramid" => {
            // Segmented Pyramid: a triangle cut into rows of triangles (1, 3, 5, …), one per node,
            // every other one upside down.
            let rows = (nf.sqrt().ceil() as usize).max(1);
            let s = area.h.min(area.w / 1.15);
            let pw = s * 1.15;
            let (x0, y0) = (area.x + (area.w - pw) / 2.0, area.y + (area.h - s) / 2.0);
            let (tw, th) = (pw / rows as f64, s / rows as f64);
            let mut k = 0;
            'rows: for r in 0..rows {
                let left = x0 + pw / 2.0 - (r as f64 + 1.0) * tw / 2.0;
                for c in 0..(2 * r + 1) {
                    let Some(g) = gs.get(k) else { break 'rows };
                    let x = Xfrm::new(left + c as f64 * tw / 2.0, y0 + r as f64 * th, tw, th);
                    let mut sh = text_shape("rect", x, &nodes(g), true);
                    // Drawn as a polygon (not a flipped preset) so upside-down text stays upright.
                    let path = if c % 2 == 1 {
                        format!("M 0 0 L {tw:.2} 0 L {:.2} {th:.2} Z", tw / 2.0)
                    } else {
                        format!("M {:.2} 0 L {tw:.2} {th:.2} L 0 {th:.2} Z", tw / 2.0)
                    };
                    sh.geom = Geom::Custom {
                        paths: vec![deckcraft_model::CustomPath {
                            w: tw,
                            h: th,
                            d: path,
                            fill: deckcraft_geom::preset::FillMode::Norm,
                            stroke: true,
                        }],
                    };
                    if let Some(b) = &mut sh.text {
                        // The text in the triangle's wide half.
                        b.body.inset_l = Some(tw * 0.22);
                        b.body.inset_r = Some(tw * 0.22);
                        if c % 2 == 1 {
                            b.body.anchor = Some(deckcraft_model::text::Anchor::Top);
                            b.body.inset_b = Some(th * 0.35);
                        } else {
                            b.body.anchor = Some(deckcraft_model::text::Anchor::Bottom);
                            b.body.inset_t = Some(th * 0.35);
                        }
                    }
                    paint(d, &mut sh, k, false);
                    out.push((sh, g.clone()));
                    k += 1;
                }
            }
        }
        "verticalBoxList" => {
            // Vertical Box List: a pale box per node with its sub-points, its title on a tab
            // overlapping the box's top left.
            let h = area.h / nf;
            for (i, g) in gs.iter().enumerate() {
                let y = area.y + i as f64 * h;
                let tab_h = (h * 0.3).min(40.0);
                let mut bx = text_shape("rect", Xfrm::new(area.x, y + tab_h / 2.0, area.w, h * 0.9 - tab_h / 2.0), &nodes(&g[1..]), false);
                paint(d, &mut bx, i, true);
                if let Some(b) = &mut bx.text {
                    b.body.inset_t = Some(tab_h * 0.6);
                }
                out.push((bx, g[1..].to_vec()));
                titled(&mut out, "roundRect", Xfrm::new(area.x + area.w * 0.05, y, area.w * 0.6, tab_h), i, &g[..1]);
            }
        }
        "trapezoidList" | "tableList" => {
            // Trapezoid List: a column per node, each a trapezoid with its sub-points. Table List: the
            // first node as a header bar over a column per other node.
            let table = d.kind == "tableList";
            let (head, cols): (Option<&Vec<usize>>, &[Vec<usize>]) =
                if table { gs.split_first().map(|(h, r)| (Some(h), r)).unwrap_or((None, &[])) } else { (None, &gs) };
            let top = if head.is_some() { area.h * 0.28 } else { 0.0 };
            if let Some(h) = head {
                titled(&mut out, "rect", Xfrm::new(area.x, area.y, area.w, top * 0.9), 0, h);
            }
            let m = cols.len().max(1) as f64;
            let w = area.w / m;
            for (k, g) in cols.iter().enumerate() {
                let i = k + usize::from(table);
                let x = Xfrm::new(area.x + k as f64 * w + w * 0.02, area.y + top, w * 0.96, area.h - top);
                let mut sh = text_shape(if table { "rect" } else { "trapezoid" }, x, &nodes(g), true);
                if let Some(b) = &mut sh.text {
                    b.body.anchor = Some(deckcraft_model::text::Anchor::Top);
                    if !table {
                        b.body.inset_l = Some(x.w * 0.2);
                        b.body.inset_r = Some(x.w * 0.2);
                        b.body.inset_t = Some(x.h * 0.08);
                    }
                }
                paint(d, &mut sh, i, table);
                out.push((sh, g.clone()));
            }
        }
        "accentProcess" => {
            // Accent Process: a title box per step with its sub-points in a pale box below and to the
            // right, arrows between the titles.
            let gap = area.w * 0.05;
            let w = (area.w - gap * (nf - 1.0)) / nf;
            for (i, g) in gs.iter().enumerate() {
                let x = area.x + i as f64 * (w + gap);
                let th = area.h * 0.22;
                if g.len() > 1 {
                    let mut sh = text_shape("rect", Xfrm::new(x + w * 0.12, area.y + th * 0.7, w * 0.88, area.h - th * 0.7), &nodes(&g[1..]), false);
                    paint(d, &mut sh, i, true);
                    if let Some(b) = &mut sh.text {
                        b.body.inset_t = Some(th * 0.45);
                    }
                    out.push((sh, g[1..].to_vec()));
                }
                titled(&mut out, "roundRect", Xfrm::new(x, area.y, w * 0.85, th), i, &g[..1]);
                if i + 1 < n {
                    let a = gap * 0.8;
                    arrow(&mut out, Xfrm::new(x + w + (gap - a) / 2.0 - w * 0.07, area.y + th / 2.0 - a / 2.0, a, a), 0.0, i);
                }
            }
        }
        "continuousBlockProcess" => {
            // Continuous Block Process: one big pale arrow behind a row of boxes.
            let mut big = deco("rightArrow", Xfrm::new(area.x, area.y + area.h * 0.15, area.w, area.h * 0.7), 0.0);
            paint(d, &mut big, 0, true);
            out.push((big, vec![]));
            let w = area.w * 0.85 / nf;
            let h = area.h * 0.32;
            for (i, g) in gs.iter().enumerate() {
                let x = area.x + area.w * 0.02 + i as f64 * w;
                titled(&mut out, "roundRect", Xfrm::new(x + w * 0.05, area.y + (area.h - h) / 2.0, w * 0.9, h), i, g);
            }
        }
        "bendingProcess" => {
            // Basic Bending Process: rows of boxes read left to right, wrapping, arrows between.
            let cols = ((nf * area.w / area.h.max(1.0) / 1.5).sqrt().ceil() as usize).clamp(1, n);
            let rows = n.div_ceil(cols);
            let (cw, rh) = (area.w / cols as f64, area.h / rows as f64);
            let (bw, bh) = (cw * 0.72, (rh * 0.75).min(cw * 0.72 * 0.6));
            for (i, g) in gs.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let x = area.x + c as f64 * cw;
                let y = area.y + r as f64 * rh + (rh - bh) / 2.0;
                titled(&mut out, "rect", Xfrm::new(x, y, bw, bh), i, g);
                if i + 1 < n {
                    let a = (cw - bw) * 0.7;
                    if c + 1 < cols {
                        arrow(&mut out, Xfrm::new(x + bw + (cw - bw - a) / 2.0, y + bh / 2.0 - a / 2.0, a, a), 0.0, i);
                    } else {
                        // The bend: down from the row's last box towards the next row's first.
                        let a = (rh - bh).min(a * 1.5).max(6.0);
                        arrow(&mut out, Xfrm::new(x + bw / 2.0 - a / 2.0, y + bh + (rh - bh - a) / 2.0, a, a), 90.0, i);
                    }
                }
            }
        }
        "linearVenn" | "stackedVenn" => {
            // Linear Venn: overlapping circles in a row. Stacked Venn: circles nested from the bottom,
            // the text near each one's top.
            let linear = d.kind == "linearVenn";
            if linear {
                let dc = (area.w / (nf * 0.75 + 0.25)).min(area.h);
                let step = dc * 0.75;
                let x0 = area.x + (area.w - (step * (nf - 1.0) + dc)) / 2.0;
                for (i, g) in gs.iter().enumerate() {
                    let mut sh = text_shape("ellipse", Xfrm::new(x0 + i as f64 * step, area.y + (area.h - dc) / 2.0, dc, dc), &nodes(g), true);
                    paint(d, &mut sh, i, false);
                    if let Some(Fill::Solid { color: c }) = &mut sh.fill {
                        *c = c.clone().with(ColorTransform::Alpha(55_000));
                    }
                    out.push((sh, g.clone()));
                }
            } else {
                let s = area.h.min(area.w);
                let (cx, bottom) = (area.x + area.w / 2.0, area.y + (area.h + s) / 2.0);
                for (i, _) in gs.iter().enumerate() {
                    let dc = s * (1.0 - i as f64 / nf);
                    let mut sh = deco("ellipse", Xfrm::new(cx - dc / 2.0, bottom - dc, dc, dc), 0.0);
                    paint(d, &mut sh, i, false);
                    out.push((sh, vec![]));
                }
                // Each circle's text in the band between its top and the next circle's.
                let band = s / nf;
                for (i, g) in gs.iter().enumerate() {
                    let dc = s * (1.0 - i as f64 / nf);
                    let top = bottom - dc;
                    let h = if i + 1 < n { band } else { dc };
                    let w = dc * if i + 1 < n { 0.6 } else { 0.7 };
                    let mut t =
                        text_shape("rect", Xfrm::new(cx - w / 2.0, top + if i + 1 < n { band * 0.05 } else { 0.0 }, w, h * 0.9), &nodes(g), true);
                    paint(d, &mut t, i, false);
                    t.fill = Some(Fill::None);
                    t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                    t.effects = None;
                    out.push((t, g.clone()));
                }
            }
        }
        "nestedTarget" => {
            // Nested Target: rounded boxes nested inside each other, the text on each one's top band.
            let band_x = area.w / 2.0 / nf * 0.6;
            let band_y = area.h / nf * 0.9 / 2.0;
            for (i, _) in gs.iter().enumerate() {
                let (dx, dy) = (band_x * i as f64, band_y * i as f64);
                let mut sh = deco("roundRect", Xfrm::new(area.x + dx, area.y + dy, area.w - 2.0 * dx, area.h - 2.0 * dy), 0.0);
                paint(d, &mut sh, i, false);
                out.push((sh, vec![]));
            }
            for (i, g) in gs.iter().enumerate() {
                let (dx, dy) = (band_x * i as f64, band_y * i as f64);
                let h = if i + 1 < n { band_y } else { area.h - 2.0 * dy };
                let mut t = text_shape(
                    "rect",
                    Xfrm::new(area.x + dx + band_x * 0.3, area.y + dy, (area.w - 2.0 * dx - band_x * 0.6).max(1.0), h),
                    &nodes(g),
                    true,
                );
                paint(d, &mut t, i, false);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                t.effects = None;
                out.push((t, g.clone()));
            }
        }
        "gridMatrix" => {
            // Grid Matrix: four quadrants on crossed axes.
            let s = area.h.min(area.w);
            let (x0, y0) = (area.x + (area.w - s) / 2.0, area.y + (area.h - s) / 2.0);
            let t = s * 0.06;
            for (preset, x) in [("leftRightArrow", Xfrm::new(x0, y0 + (s - t) / 2.0, s, t)), ("upDownArrow", Xfrm::new(x0 + (s - t) / 2.0, y0, t, s))]
            {
                let mut sh = deco(preset, x, 0.0);
                paint(d, &mut sh, 0, true);
                out.push((sh, vec![]));
            }
            let q = s / 2.0 - t * 1.2;
            for (i, g) in gs.iter().take(4).enumerate() {
                let (c, r) = ((i % 2) as f64, (i / 2) as f64);
                let x = Xfrm::new(x0 + c * (s / 2.0 + t * 0.7) + t * 0.5 * (1.0 - c), y0 + r * (s / 2.0 + t * 0.7) + t * 0.5 * (1.0 - r), q, q);
                titled(&mut out, "rect", x, i, g);
            }
        }
        "gear" => {
            // Gear: up to three interlocking gears, the first the biggest.
            let s = area.h.min(area.w * 0.62);
            let (x0, y0) = (area.x + (area.w - s / 0.62) / 2.0, area.y + (area.h - s) / 2.0);
            let spots = [(0.0, 0.18, 0.66, "gear9"), (0.5, 0.0, 0.48, "gear6"), (0.56, 0.48, 0.42, "gear6")];
            for (i, g) in gs.iter().take(3).enumerate() {
                let (fx, fy, fs, preset) = spots[i];
                let z = s * fs;
                titled(&mut out, preset, Xfrm::new(x0 + fx * s * 1.3, y0 + fy * s, z, z), i, g);
            }
        }
        "funnel" => {
            // Funnel: the ideas poured in at the top (up to three), the last one coming out below.
            let s = area.h.min(area.w);
            let (cx, y0) = (area.x + area.w / 2.0, area.y);
            let fw = s * 0.8;
            let fh = s * 0.62;
            let mut f = deco("funnel", Xfrm::new(cx - fw / 2.0, y0, fw, fh), 0.0);
            paint(d, &mut f, 0, true);
            out.push((f, vec![]));
            let (inside, last) = match gs.split_last() {
                Some((l, r)) if !r.is_empty() => (r, Some(l)),
                _ => (&gs[..], None),
            };
            let k = inside.len().min(3);
            let dc = fw * 0.27;
            let spots = [(0.0, 0.04), (-0.2, 0.3), (0.2, 0.3)];
            for (i, g) in inside.iter().take(3).enumerate() {
                let (dx, dy) = if k == 1 { (0.0, 0.15) } else { spots[i] };
                titled(&mut out, "ellipse", Xfrm::new(cx + dx * fw - dc / 2.0, y0 + dy * fh + fh * 0.05, dc, dc), i, g);
            }
            if let Some(g) = last {
                let a = s * 0.12;
                let mut ar = deco("downArrow", Xfrm::new(cx - a / 2.0, y0 + fh, a, a * 1.2), 0.0);
                paint(d, &mut ar, k, true);
                out.push((ar, vec![]));
                titled(&mut out, "roundRect", Xfrm::new(cx - fw * 0.35, y0 + fh + a * 1.3, fw * 0.7, (s - fh - a * 1.3).max(10.0)), k, g);
            }
        }
        "pictureHList" => {
            // Horizontal Picture List: a column per node, a big picture over its text.
            let w = area.w / nf;
            for (i, g) in gs.iter().enumerate() {
                let x = area.x + i as f64 * w + w * 0.04;
                let bw = w * 0.92;
                let ph = area.h * 0.55;
                out.push((picture_shape(d, "rect", Xfrm::new(x, area.y, bw, ph), i, g[0]), vec![]));
                let mut sh = text_shape("rect", Xfrm::new(x, area.y + ph, bw, area.h - ph), &nodes(g), true);
                if let Some(b) = &mut sh.text {
                    b.body.anchor = Some(deckcraft_model::text::Anchor::Top);
                }
                paint(d, &mut sh, i, false);
                out.push((sh, g.clone()));
            }
        }
        "timeline" => {
            // Basic Timeline: an arrow across the middle, a dot per node on it, the text above and
            // below in turn.
            let mid = area.y + area.h / 2.0;
            let bar = (area.h * 0.08).max(6.0);
            let mut sh = deco("rightArrow", Xfrm::new(area.x, mid - bar, area.w, bar * 2.0), 0.0);
            paint(d, &mut sh, 0, true);
            out.push((sh, vec![]));
            let w = area.w * 0.92 / nf;
            for (i, g) in gs.iter().enumerate() {
                let cx = area.x + w * (i as f64 + 0.5);
                let dot = bar * 1.4;
                let mut dsh = deco("ellipse", Xfrm::new(cx - dot / 2.0, mid - dot / 2.0, dot, dot), 0.0);
                paint(d, &mut dsh, i, false);
                out.push((dsh, vec![]));
                let th = area.h / 2.0 - bar * 1.6;
                let y = if i % 2 == 0 { area.y } else { mid + bar * 1.6 };
                let mut t = text_shape("rect", Xfrm::new(cx - w * 0.48, y, w * 0.96, th.max(1.0)), &nodes(g), true);
                if let Some(b) = &mut t.text {
                    b.body.anchor = Some(if i % 2 == 0 { deckcraft_model::text::Anchor::Bottom } else { deckcraft_model::text::Anchor::Top });
                }
                paint(d, &mut t, i, true);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                out.push((t, g.clone()));
            }
        }
        "venn" => {
            // Basic Venn: see-through circles overlapping in a ring (two or three in a row/triangle).
            let s = area.h.min(area.w);
            let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
            let dc = if n == 1 { s } else { s * 0.62 };
            let r = if n == 1 { 0.0 } else { (s - dc) / 2.0 };
            for (i, g) in gs.iter().enumerate() {
                let a = (i as f64 / nf * 360.0 + if n == 2 { 180.0 } else { -90.0 }).to_radians();
                let mut sh = text_shape("ellipse", Xfrm::new(cx + r * a.cos() - dc / 2.0, cy + r * a.sin() - dc / 2.0, dc, dc), &nodes(g), true);
                paint(d, &mut sh, i, false);
                if let Some(Fill::Solid { color: c }) = &mut sh.fill {
                    *c = c.clone().with(ColorTransform::Alpha(50_000));
                }
                if let Some(b) = &mut sh.text {
                    // Text towards the circle's outer side, clear of the overlap.
                    let p = dc * 0.36;
                    let (dx, dy) = (a.cos() * p, a.sin() * p);
                    b.body.inset_l = Some((7.2 + dx).max(2.0));
                    b.body.inset_r = Some((7.2 - dx).max(2.0));
                    b.body.inset_t = Some((3.6 + dy).max(2.0));
                    b.body.inset_b = Some((3.6 - dy).max(2.0));
                }
                out.push((sh, g.clone()));
            }
        }
        "target" => {
            // Basic Target: nested rings, the outermost first, each ring's text in a box beside it
            // with a line from the ring.
            let s = area.h.min(area.w * 0.5);
            let (cx, cy) = (area.x + s / 2.0, area.y + area.h / 2.0);
            let band = s / 2.0 / nf;
            for (i, _) in gs.iter().enumerate() {
                let dc = s - 2.0 * band * i as f64;
                let mut sh = deco("ellipse", Xfrm::new(cx - dc / 2.0, cy - dc / 2.0, dc, dc), 0.0);
                paint(d, &mut sh, i, false);
                out.push((sh, vec![]));
            }
            let tx = area.x + s + area.w * 0.06;
            let rh = area.h / nf;
            for (i, g) in gs.iter().enumerate() {
                let ty = area.y + rh * (i as f64 + 0.5);
                // The point on ring i's band level with the text, else its top.
                let ry = cy - s / 2.0 + band * (i as f64 + 0.5);
                out.push((line_shape((cx, ry.min(ty)), (tx, ty)), vec![]));
                let mut t = text_shape("rect", Xfrm::new(tx, ty - rh * 0.45, (area.x + area.w - tx).max(1.0), rh * 0.9), &nodes(g), true);
                paint(d, &mut t, i, true);
                t.fill = Some(Fill::None);
                t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                if let Some(b) = &mut t.text {
                    for p in &mut b.paragraphs {
                        p.props.align = Some(Align::Left);
                    }
                }
                out.push((t, g.clone()));
            }
        }
        _ => {}
    }
    out
}

/// Organization Chart: [`hierarchy`] with elbow connectors, and assistants beside their parent on
/// a row of their own between the parent and its other children. `half_circle` draws each node as
/// its text in an open half ring (Half Circle Organization Chart).
fn org_chart(d: &Diagram, area: Xfrm) -> Vec<Item> {
    let half_circle = d.kind == "halfCircleOrgChart";
    let name_title = d.kind == "nameTitleOrgChart";
    let nodes = &d.nodes[..d.nodes.len().min(MAX_SHAPES)];
    if nodes.is_empty() {
        return vec![];
    }
    let mut parent: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut stack: Vec<usize> = vec![];
    for (i, n) in nodes.iter().enumerate() {
        while stack.last().is_some_and(|&p| nodes[p].level >= n.level) {
            stack.pop();
        }
        parent[i] = stack.last().copied();
        stack.push(i);
    }
    let asst = |i: usize| nodes[i].assistant && parent[i].is_some();
    let kids = |i: Option<usize>| -> Vec<usize> { (0..nodes.len()).filter(|&k| parent[k] == i && !asst(k)).collect() };
    let mut leaves = vec![0usize; nodes.len()];
    for i in (0..nodes.len()).rev() {
        let k = kids(Some(i));
        leaves[i] = if k.is_empty() { 1 } else { k.iter().map(|&c| leaves[c]).sum() };
    }
    // Row slots: a node's level is 2 per depth; assistants take the odd slot under their parent.
    let mut slot = vec![0usize; nodes.len()];
    for i in 0..nodes.len() {
        slot[i] = match parent[i] {
            None => 0,
            Some(p) if asst(i) => slot[p] + 1,
            Some(p) => slot[p] + 2,
        };
    }
    let mut used: Vec<usize> = slot.clone();
    used.sort_unstable();
    used.dedup();
    let row = |i: usize| used.iter().position(|&s| s == slot[i]).unwrap_or(0);
    let rows = used.len().max(1);
    let total: usize = kids(None).iter().map(|&r| leaves[r]).sum::<usize>().max(1);
    let cell_w = area.w / total as f64;
    let row_h = area.h / rows as f64;
    let (bw, bh) = ((cell_w * 0.85).min(area.w * 0.3), row_h * 0.62);
    let mut left = vec![0usize; nodes.len()];
    let mut next_root = 0;
    for i in 0..nodes.len() {
        if asst(i) {
            continue;
        }
        let mut start = match parent[i] {
            None => {
                let s = next_root;
                next_root += leaves[i];
                s
            }
            Some(p) => left[p],
        };
        if let Some(p) = parent[i] {
            for s in kids(Some(p)) {
                if s == i {
                    break;
                }
                start += leaves[s];
            }
        }
        left[i] = start;
    }
    let mut side = vec![0usize; nodes.len()];
    let boxes: Vec<Xfrm> = (0..nodes.len())
        .map(|i| {
            let y = area.y + row(i) as f64 * row_h + (row_h - bh) / 2.0;
            match parent[i] {
                Some(p) if asst(i) => {
                    // Assistants alternate right and left of the parent's trunk.
                    let k = (0..i).filter(|&j| parent[j] == Some(p) && asst(j)).count();
                    side[i] = k;
                    let pc = area.x + (left[p] as f64 + leaves[p] as f64 / 2.0) * cell_w;
                    let off = bw * 0.15 + (k / 2) as f64 * bw * 1.1;
                    let x = if k % 2 == 0 { pc + off } else { pc - off - bw };
                    Xfrm::new(x.clamp(area.x, area.x + area.w - bw), y, bw, bh)
                }
                _ => {
                    let cx = area.x + (left[i] as f64 + leaves[i] as f64 / 2.0) * cell_w;
                    Xfrm::new(cx - bw / 2.0, y, bw, bh)
                }
            }
        })
        .collect();
    let mut out: Vec<Item> = vec![];
    for i in 0..nodes.len() {
        let Some(p) = parent[i] else { continue };
        let (a, b) = (boxes[p], boxes[i]);
        let pc = a.x + a.w / 2.0;
        if asst(i) {
            let my = b.y + b.h / 2.0;
            out.push((line_shape((pc, a.y + a.h), (pc, my)), vec![]));
            let ex = if b.x > pc { b.x } else { b.x + b.w };
            out.push((line_shape((pc, my), (ex, my)), vec![]));
        } else {
            let mid = b.y - (row_h - bh) / 2.0;
            let cc = b.x + b.w / 2.0;
            out.push((line_shape((pc, a.y + a.h), (pc, mid)), vec![]));
            out.push((line_shape((pc.min(cc), mid), (pc.max(cc), mid)), vec![]));
            out.push((line_shape((cc, mid), (cc, b.y)), vec![]));
        }
    }
    let _ = side;
    for (i, n) in nodes.iter().enumerate() {
        let depth = slot[i] / 2;
        if half_circle {
            // Two half rings round the text: the upper left one in the accent, the lower right one
            // in its tint.
            let x = boxes[i];
            for (k, (a0, a1)) in [(140.0, 310.0), (320.0, 130.0)].into_iter().enumerate() {
                let mut arc = deco("blockArc", x, 0.0);
                arc.geom = Geom::Preset { name: "blockArc".into(), adj: vec![a0 * 60_000.0, a1 * 60_000.0, 4_000.0] };
                paint(d, &mut arc, depth, false);
                if k == 1 {
                    let c = ColorRef::scheme(accent(d, depth)).with(ColorTransform::LumMod(60_000)).with(ColorTransform::LumOff(40_000));
                    arc.fill = Some(Fill::solid(c));
                }
                arc.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
                arc.effects = None;
                out.push((arc, vec![]));
            }
            let mut t = text_shape("rect", x, &[n], true);
            paint(d, &mut t, depth, true);
            t.fill = Some(Fill::None);
            t.line = Some(Line { fill: Some(Fill::None), ..Default::default() });
            out.push((t, vec![i]));
        } else {
            let mut x = boxes[i];
            if name_title {
                // The name in the upper part; the title box overlaps the lower edge.
                x.h *= 0.85;
            }
            let mut sh = text_shape("rect", x, &[n], true);
            paint(d, &mut sh, depth, false);
            out.push((sh, vec![i]));
            if name_title {
                let tb = Xfrm::new(x.x + x.w * 0.12, x.y + x.h * 0.72, x.w * 0.8, x.h * 0.42);
                let title = DiagramNode { text: n.title.clone(), ..Default::default() };
                let mut t = text_shape("rect", tb, &[&title], true);
                paint(d, &mut t, depth, true);
                t.fill = Some(Fill::solid(ColorRef::scheme(SchemeSlot::Lt1)));
                if let Some(b) = &mut t.text {
                    for p in &mut b.paragraphs {
                        for r in &mut p.runs {
                            r.props.size = Some(11.0);
                        }
                        p.end_props.size = Some(11.0);
                    }
                }
                out.push((t, vec![]));
            }
        }
    }
    out
}

/// An organisation chart: one box per node, rows by level, each subtree as wide as its leaves.
fn hierarchy(d: &Diagram, area: Xfrm, horizontal: bool) -> Vec<Item> {
    let nodes = &d.nodes[..d.nodes.len().min(MAX_SHAPES)];
    if nodes.is_empty() {
        return vec![];
    }
    let mut parent: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut stack: Vec<usize> = vec![];
    for (i, n) in nodes.iter().enumerate() {
        while stack.last().is_some_and(|&p| nodes[p].level >= n.level) {
            stack.pop();
        }
        parent[i] = stack.last().copied();
        stack.push(i);
    }
    let kids = |i: Option<usize>| -> Vec<usize> { (0..nodes.len()).filter(|&k| parent[k] == i).collect() };
    let mut leaves = vec![0usize; nodes.len()];
    for i in (0..nodes.len()).rev() {
        let k = kids(Some(i));
        leaves[i] = if k.is_empty() { 1 } else { k.iter().map(|&c| leaves[c]).sum() };
    }
    let depth: Vec<usize> = (0..nodes.len())
        .map(|i| {
            let (mut d, mut p) = (0, parent[i]);
            while let Some(q) = p {
                d += 1;
                p = parent[q];
            }
            d
        })
        .collect();
    let rows = depth.iter().max().copied().unwrap_or(0) + 1;
    let total: usize = kids(None).iter().map(|&r| leaves[r]).sum::<usize>().max(1);
    // Horizontal Hierarchy: the same tree turned a quarter, levels from left to right.
    let (span, across) = if horizontal { (area.h, area.w) } else { (area.w, area.h) };
    let cell_w = span / total as f64;
    let row_h = across / rows as f64;
    let (bw, bh) = if horizontal { (cell_w * 0.8, row_h * 0.7) } else { (cell_w * 0.85, row_h * 0.6) };
    // Left edge (in leaves) of each node's span.
    let mut left = vec![0usize; nodes.len()];
    let mut next_root = 0;
    for i in 0..nodes.len() {
        let mut start = match parent[i] {
            None => {
                let s = next_root;
                next_root += leaves[i];
                s
            }
            Some(p) => left[p],
        };
        if let Some(p) = parent[i] {
            for s in kids(Some(p)) {
                if s == i {
                    break;
                }
                start += leaves[s];
            }
        }
        left[i] = start;
    }
    let boxes: Vec<Xfrm> = (0..nodes.len())
        .map(|i| {
            let c = (left[i] as f64 + leaves[i] as f64 / 2.0) * cell_w;
            let r = depth[i] as f64 * row_h + (row_h - bh) / 2.0;
            if horizontal { Xfrm::new(area.x + r, area.y + c - bw / 2.0, bh, bw) } else { Xfrm::new(area.x + c - bw / 2.0, area.y + r, bw, bh) }
        })
        .collect();
    let mut out: Vec<Item> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut sh = text_shape("roundRect", boxes[i], &[n], true);
            paint(d, &mut sh, depth[i], false);
            (sh, vec![i])
        })
        .collect();
    for i in 0..nodes.len() {
        if let Some(p) = parent[i] {
            let (a, b) = (boxes[p], boxes[i]);
            let (p, q) = if horizontal {
                ((a.x + a.w, a.y + a.h / 2.0), (b.x, b.y + b.h / 2.0))
            } else {
                ((a.x + a.w / 2.0, a.y + a.h), (b.x + b.w / 2.0, b.y))
            };
            out.push((line_shape(p, q), vec![]));
        }
    }
    out
}

/// The SmartArt group that `id` is, or is inside.
fn diagram_group(shapes: &[Shape], id: ShapeId) -> Option<&Shape> {
    for sh in shapes {
        if sh.id == id && sh.diagram.is_some() {
            return Some(sh);
        }
        if let ShapeKind::Group { children, .. } = &sh.kind {
            if sh.diagram.is_some() && deckcraft_model::find_shape(children, id).is_some() {
                return Some(sh);
            }
            if let Some(g) = diagram_group(children, id) {
                return Some(g);
            }
        }
    }
    None
}

/// The SmartArt graphic the selection is (or is inside), with its outline as the Text Pane shows it.
pub fn selected(st: &crate::DocState) -> Option<(ShapeId, Diagram)> {
    let shapes = current_shapes(&st.doc, &st.selection)?;
    let g = st.selection.shapes.iter().find_map(|id| diagram_group(shapes, *id))?;
    let mut d = g.diagram.clone()?;
    if let ShapeKind::Group { children, .. } = &g.kind {
        sync_text(&mut d, children);
    }
    Some((g.id, d))
}

fn target_group(s: &Session, p: &Value, cmd: &str) -> Result<ShapeId> {
    let st = s.doc()?;
    let shapes = current_shapes(&st.doc, &st.selection).ok_or_else(|| bad(cmd, "no slide"))?;
    let ids = targets(s, p)?;
    ids.iter().find_map(|id| diagram_group(shapes, *id)).map(|g| g.id).ok_or_else(|| bad(cmd, "select a SmartArt graphic"))
}

/// Children with fresh ids for diagram `d` inside a group whose child space is `area`.
fn regenerate(cmd: &str, doc: &mut Presentation, d: &Diagram, area: Xfrm) -> Result<Vec<Shape>> {
    let mut v = layout(d, area);
    for sh in &mut v {
        sh.id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
    }
    Ok(v)
}

/// Read the diagram (with typed text synced), let `f` change it, and regenerate the group.
fn edit_diagram<T>(s: &mut Session, p: &Value, cmd: &str, f: impl FnOnce(&mut Diagram) -> Result<T>) -> Result<T> {
    let gid = target_group(s, p, cmd)?;
    s.edit(|doc, sel| {
        let g = current_shapes(doc, sel).and_then(|v| deckcraft_model::find_shape(v, gid)).cloned().ok_or_else(|| bad(cmd, "no such shape"))?;
        let (ShapeKind::Group { children, child }, Some(mut d)) = (g.kind, g.diagram) else { return Err(bad(cmd, "not a SmartArt graphic")) };
        sync_text(&mut d, &children);
        let out = f(&mut d)?;
        normalize(&mut d.nodes);
        d.imported = false;
        let kids = regenerate(cmd, doc, &d, child)?;
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
        let sh = deckcraft_model::find_shape_mut(shapes, gid).ok_or_else(|| bad(cmd, "no such shape"))?;
        sh.kind = ShapeKind::Group { children: kids, child };
        sh.diagram = Some(d);
        sel.shapes = vec![gid];
        sel.text = None;
        Ok(out)
    })
}

fn index(p: &Value, cmd: &str) -> Result<usize> {
    p.get("index").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok()).ok_or_else(|| bad(cmd, "index is required"))
}

/// Insert a new SmartArt graphic (`insert.smartArt`).
pub fn insert(s: &mut Session, kind: &str, mut nodes: Vec<DiagramNode>) -> Result<Value> {
    let kind = if is_kind(kind) { kind } else { "process" };
    if nodes.is_empty() {
        nodes = (0..3).map(|_| DiagramNode { text: "Text".into(), level: 0, picture: None, assistant: false, title: String::new() }).collect();
    }
    normalize(&mut nodes);
    let d = Diagram { kind: kind.into(), nodes, ..Default::default() };
    let size = s.doc()?.doc.slide_size;
    let area = Xfrm::new(size.width * 0.1, size.height * 0.28, size.width * 0.8, size.height * 0.6);
    let id = s.edit(|doc, sel| {
        let children = regenerate("insert.smartArt", doc, &d, area)?;
        let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("insert.smartArt", NO_ID_SPACE))?);
        let g = Shape {
            id,
            name: format!("SmartArt ({kind})"),
            xfrm: Some(area),
            kind: ShapeKind::Group { children, child: area },
            diagram: Some(d),
            ..Default::default()
        };
        crate::shapes_mut(doc, sel).ok_or_else(|| EngineError::Other("no slide".into()))?.push(g);
        sel.shapes = vec![id];
        sel.text = None;
        Ok(id)
    })?;
    Ok(json!({"id": id.0}))
}

fn nodes(s: &mut Session, p: &Value) -> Result<Value> {
    let gid = target_group(s, p, "smartArt.nodes")?;
    let g = shape_of(s, gid)?;
    let mut d = g.diagram.unwrap_or_default();
    if let ShapeKind::Group { children, .. } = &g.kind {
        sync_text(&mut d, children);
    }
    Ok(json!({"id": gid.0, "kind": d.kind, "nodes": d.nodes}))
}

fn set_nodes(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.setNodes";
    let given: Option<Vec<DiagramNode>> = match (p.get("nodes"), p.get("lines").and_then(Value::as_array)) {
        (Some(v), _) => Some(serde_json::from_value(v.clone()).map_err(|e| bad(CMD, e.to_string()))?),
        (None, Some(a)) => Some(nodes_from_lines(a.iter().filter_map(Value::as_str))),
        _ => None,
    };
    let one = p.get("index").map(|_| index(p, CMD)).transpose()?;
    let text = str_param(p, "text").map(String::from);
    let title = str_param(p, "title").map(String::from);
    edit_diagram(s, p, CMD, |d| {
        if let Some(n) = given {
            d.nodes = n;
        }
        if let (Some(i), Some(t)) = (one, text) {
            d.nodes.get_mut(i).ok_or_else(|| bad(CMD, "no such node"))?.text = t;
        }
        if let (Some(i), Some(t)) = (one, title) {
            d.nodes.get_mut(i).ok_or_else(|| bad(CMD, "no such node"))?.title = t;
        }
        Ok(Value::Null)
    })
}

fn add_node(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok());
    let pos = str_param(p, "position").unwrap_or("after").to_string();
    let text = str_param(p, "text").unwrap_or("").to_string();
    edit_diagram(s, p, "smartArt.addNode", |d| {
        let at = add(&mut d.nodes, i, &pos, &text);
        Ok(json!({"index": at}))
    })
}

fn remove_node(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.removeNode";
    let i = index(p, CMD)?;
    edit_diagram(s, p, CMD, |d| if remove(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(CMD, "no such node")) })
}

fn promote(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.promote";
    let i = index(p, CMD)?;
    edit_diagram(s, p, CMD, |d| if promote_node(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(CMD, "already at the top level")) })
}

fn demote(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.demote";
    let i = index(p, CMD)?;
    edit_diagram(s, p, CMD, |d| if demote_node(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(CMD, "nothing above to demote under")) })
}

/// Convert to SmartArt: the selected text shape's paragraphs become the nodes of a new graphic in
/// the shape's place.
fn from_text(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.fromText";
    let kind = str_param(p, "kind").filter(|k| is_kind(k)).unwrap_or("list").to_string();
    let id = *targets(s, p)?.first().ok_or_else(|| bad(CMD, "select a text box"))?;
    let (area, nodes) = {
        let st = s.doc()?;
        let shapes = current_shapes(&st.doc, &st.selection).ok_or_else(|| bad(CMD, "no slide"))?;
        let sh = shapes.iter().find(|x| x.id == id).ok_or_else(|| bad(CMD, "select a text box on the slide"))?;
        let body =
            sh.text.as_ref().filter(|b| b.paragraphs.iter().any(|p| !p.text().trim().is_empty())).ok_or_else(|| bad(CMD, "the shape has no text"))?;
        let mut nodes: Vec<DiagramNode> = body
            .paragraphs
            .iter()
            .filter(|p| !p.text().trim().is_empty())
            .map(|p| DiagramNode { text: p.text().trim().to_string(), level: p.level.min(MAX_LEVEL), ..Default::default() })
            .collect();
        normalize(&mut nodes);
        // A small text box grows to a size a diagram can use, keeping its top left on the slide.
        let mut a = xfrm_of(&st.doc, &st.selection, sh);
        let size = st.doc.slide_size;
        a.w = a.w.max(size.width * 0.5).min(size.width);
        a.h = a.h.max(size.height * 0.35).min(size.height);
        a.x = a.x.min(size.width - a.w).max(0.0);
        a.y = a.y.min(size.height - a.h).max(0.0);
        a.rot = 0.0;
        (a, nodes)
    };
    let d = Diagram { kind: kind.clone(), nodes, ..Default::default() };
    let gid = s.edit(|doc, sel| {
        let children = regenerate(CMD, doc, &d, area)?;
        let gid = ShapeId(doc.alloc_id().ok_or_else(|| bad(CMD, NO_ID_SPACE))?);
        let g = Shape {
            id: gid,
            name: format!("SmartArt ({kind})"),
            xfrm: Some(area),
            kind: ShapeKind::Group { children, child: area },
            diagram: Some(d),
            ..Default::default()
        };
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        let at = shapes.iter().position(|x| x.id == id).ok_or_else(|| bad(CMD, "no such shape"))?;
        // In the text shape's place in the z-order.
        shapes[at] = g;
        sel.shapes = vec![gid];
        sel.text = None;
        Ok(gid)
    })?;
    Ok(json!({"id": gid.0}))
}

/// Convert to Text: a text box with the outline, one bulleted paragraph per node.
fn to_text(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.toText";
    let gid = target_group(s, p, CMD)?;
    let (_, d) = selected_for(s, gid).ok_or_else(|| bad(CMD, "select a SmartArt graphic"))?;
    // The sizes the graphic's text shows at: its titles' and its bullets' (shrunk to fit included).
    let (title_pt, bullet_pt) = {
        let st = s.doc()?;
        let shapes = current_shapes(&st.doc, &st.selection).ok_or_else(|| bad(CMD, "no slide"))?;
        let g = diagram_group(shapes, gid).ok_or_else(|| bad(CMD, "select a SmartArt graphic"))?;
        text_sizes(g.children())
    };
    let new = s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        let at = shapes.iter().position(|x| x.id == gid).ok_or_else(|| bad(CMD, "the graphic isn't at the top of the slide"))?;
        let area = shapes[at].xfrm.unwrap_or_default();
        let mut body = TextBody::default();
        for n in &d.nodes {
            let mut para = Paragraph::new(&n.text);
            para.level = n.level.min(MAX_LEVEL);
            let size = if n.level == 0 { title_pt } else { bullet_pt };
            for r in &mut para.runs {
                r.props.size = Some(size);
            }
            para.end_props.size = Some(size);
            para.props.bullet = Some(deckcraft_model::text::Bullet::Char { char: "•".into() });
            para.props.margin_left = Some(18.0 * (para.level as f64 + 1.0));
            para.props.indent = Some(-14.0);
            body.paragraphs.push(para);
        }
        if body.paragraphs.is_empty() {
            body.paragraphs.push(Paragraph::new(""));
        }
        body.body.wrap = Some(true);
        body.body.autofit = Some(AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 });
        let id = ShapeId(doc.alloc_id().ok_or_else(|| bad(CMD, NO_ID_SPACE))?);
        let tb =
            Shape { id, name: "Text".into(), xfrm: Some(area), text_box: true, text: Some(body), geom: Geom::preset("rect"), ..Default::default() };
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        shapes[at] = tb;
        sel.shapes = vec![id];
        sel.text = None;
        Ok(id)
    })?;
    Ok(json!({"id": new.0}))
}

/// Convert to Shapes: keep the drawing as an ordinary group.
fn to_shapes(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.toShapes";
    let gid = target_group(s, p, CMD)?;
    s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        let g = deckcraft_model::find_shape_mut(shapes, gid).ok_or_else(|| bad(CMD, "no such shape"))?;
        g.diagram = None;
        g.name = "Group".into();
        sel.shapes = vec![gid];
        Ok(())
    })?;
    Ok(Value::Null)
}

/// The most common title and bullet font sizes in a diagram's shapes (20 and 14 pt when none).
fn text_sizes(children: &[Shape]) -> (f64, f64) {
    let mut titles: Vec<f64> = vec![];
    let mut bullets: Vec<f64> = vec![];
    for c in children {
        let Some(b) = &c.text else { continue };
        for p in &b.paragraphs {
            let Some(sz) = p.runs.iter().find_map(|r| r.props.size) else { continue };
            if p.props.bullet.is_some() { bullets.push(sz) } else { titles.push(sz) }
        }
    }
    let mode = |v: &mut Vec<f64>, d: f64| {
        v.sort_by(|a, b| a.total_cmp(b));
        let mut best = (d, 0usize);
        let mut k = 0;
        while k < v.len() {
            let j = v[k..].iter().take_while(|x| (**x - v[k]).abs() < 1e-9).count();
            if j > best.1 {
                best = (v[k], j);
            }
            k += j;
        }
        best.0
    };
    (mode(&mut titles, 20.0), mode(&mut bullets, 14.0))
}

/// The graphic `gid` with its outline (typed text synced).
fn selected_for(s: &Session, gid: ShapeId) -> Option<(ShapeId, Diagram)> {
    let st = s.doc().ok()?;
    let shapes = current_shapes(&st.doc, &st.selection)?;
    let g = diagram_group(shapes, gid)?;
    let mut d = g.diagram.clone()?;
    if let ShapeKind::Group { children, .. } = &g.kind {
        sync_text(&mut d, children);
    }
    Some((g.id, d))
}

fn set_picture(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.picture";
    let i = index(p, CMD)?;
    let media = if bool_or(p, "reset", false) {
        None
    } else {
        let (name, bytes) = super::insert::media_bytes(p, CMD)?;
        let ct = super::insert::content_type(&name, &bytes);
        target_group(s, p, CMD)?;
        Some(s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad(CMD, NO_ID_SPACE)))?)
    };
    edit_diagram(s, p, CMD, |d| {
        // A sub-point's picture is its top-level node's: that's the node the placeholder shows.
        let top = (0..=i.min(d.nodes.len().saturating_sub(1))).rev().find(|k| d.nodes.get(*k).is_some_and(|n| n.level == 0)).unwrap_or(i);
        let n = d.nodes.get_mut(top).ok_or_else(|| bad(CMD, "no such node"))?;
        n.picture = media;
        Ok(Value::Null)
    })
}

fn change_style(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.style";
    let v = str_param(p, "style").filter(|k| STYLES.iter().any(|(id, _)| id == k)).ok_or_else(|| bad(CMD, "unknown style"))?.to_string();
    edit_diagram(s, p, CMD, |d| {
        if d.style != v {
            d.style_xml = None;
        }
        d.style = v;
        Ok(Value::Null)
    })
}

fn change_colors(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.colors";
    let v = str_param(p, "colors").filter(|k| COLORS.iter().any(|(id, _)| id == k)).ok_or_else(|| bad(CMD, "unknown colours"))?.to_string();
    edit_diagram(s, p, CMD, |d| {
        if d.colors != v {
            d.colors_xml = None;
        }
        d.colors = v;
        Ok(Value::Null)
    })
}

fn change_layout(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.layout";
    let kind = str_param(p, "kind").filter(|k| is_kind(k)).ok_or_else(|| bad(CMD, "unknown layout"))?.to_string();
    edit_diagram(s, p, CMD, |d| {
        if d.kind != kind {
            // PowerPoint's own layout no longer applies.
            d.layout_id.clear();
            d.layout_xml = None;
        }
        d.kind = kind;
        Ok(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lv(n: &[DiagramNode]) -> Vec<u8> {
        n.iter().map(|x| x.level).collect()
    }
    fn mk(levels: &[u8]) -> Vec<DiagramNode> {
        levels
            .iter()
            .enumerate()
            .map(|(i, l)| DiagramNode { text: format!("n{i}"), level: *l, picture: None, assistant: false, title: String::new() })
            .collect()
    }

    #[test]
    fn outline_edits() {
        let mut n = mk(&[0, 1, 1, 0]);
        assert_eq!(add(&mut n, Some(0), "after", "x"), 3, "after the node's sub-points");
        assert_eq!(lv(&n), [0, 1, 1, 0, 0]);
        assert_eq!(add(&mut n, Some(0), "below", "y"), 3);
        assert_eq!(lv(&n), [0, 1, 1, 1, 0, 0]);
        assert_eq!(add(&mut n, Some(0), "above", "z"), 0);
        assert_eq!(lv(&n), [0, 1, 2, 2, 2, 0, 0]);
        assert!(remove(&mut n, 1));
        assert_eq!(lv(&n), [0, 1, 1, 1, 0, 0]);
        assert!(!promote_node(&mut n, 0));
        assert!(!demote_node(&mut n, 0), "nothing above the first node");
        assert!(!demote_node(&mut n, 2) || lv(&n)[2] == 2);
        let mut m = mk(&[0, 0, 1]);
        assert!(demote_node(&mut m, 1));
        assert_eq!(lv(&m), [0, 1, 2], "sub-points move with their node");
        assert!(promote_node(&mut m, 1));
        assert_eq!(lv(&m), [0, 0, 1]);
        let mut bad = mk(&[2, 4, 0]);
        normalize(&mut bad);
        assert_eq!(lv(&bad), [0, 1, 0]);
        assert_eq!(lv(&nodes_from_lines(["A", "\tB", "\t\tC", "", "D"])), [0, 1, 2, 0]);
    }

    #[test]
    fn layouts_map_nodes_to_shapes() {
        let area = Xfrm::new(0.0, 0.0, 800.0, 400.0);
        let d = Diagram { kind: "blockList".into(), nodes: mk(&[0, 1, 1, 0]), ..Default::default() };
        let v = layout(&d, area);
        assert_eq!(v.len(), 2);
        let t = v[0].text.as_ref().map(|b| b.paragraphs.len());
        assert_eq!(t, Some(3), "sub-points are bullets in their parent's shape");
        let h = Diagram { kind: "hierarchy".into(), nodes: mk(&[0, 1, 2, 1]), ..Default::default() };
        let v = layout(&h, area);
        assert_eq!(v.len(), 4 + 3, "a box per node and a line per parent link");
        for sh in &v {
            let x = sh.xfrm.unwrap_or_default();
            assert!(x.x >= -0.01 && x.x + x.w <= 800.01 && x.y >= -0.01 && x.y + x.h <= 400.01, "{x:?}");
        }
        assert!(layout(&Diagram { kind: "cycle".into(), ..Default::default() }, area).is_empty());
        // Vertical Bullet List: a bar per top node, its sub-points in a box below.
        let v = layout_items(&Diagram { kind: "list".into(), nodes: mk(&[0, 1, 1, 0]), ..Default::default() }, area);
        let maps: Vec<Vec<usize>> = v.into_iter().map(|(_, m)| m).collect();
        assert_eq!(maps, [vec![0], vec![1, 2], vec![3]]);
    }

    #[test]
    fn every_layout_shows_every_node_inside_its_area() {
        let area = Xfrm::new(50.0, 40.0, 800.0, 400.0);
        for (kind, _) in LAYOUTS {
            for style in std::iter::once("").chain(STYLES.iter().map(|(k, _)| *k)) {
                let d = Diagram {
                    kind: kind.into(),
                    nodes: mk(&[0, 1, 0, 0, 1, 0]),
                    style: style.into(),
                    colors: "accent2".into(),
                    ..Default::default()
                };
                let items = layout_items(&d, area);
                let mut shown: Vec<usize> = items.iter().flat_map(|(_, m)| m.clone()).collect();
                shown.sort_unstable();
                // Gear draws three gears, as PowerPoint does: the fourth group isn't shown.
                let want: Vec<usize> = (0..if kind == "gear" { 5 } else { 6 }).collect();
                assert_eq!(shown, want, "{kind}");
                for (sh, _) in &items {
                    let x = sh.xfrm.unwrap_or_default();
                    let (cx, cy) = (x.x + x.w / 2.0, x.y + x.h / 2.0);
                    assert!(x.w.is_finite() && x.h.is_finite() && x.w >= 0.0 && x.h >= 0.0, "{kind} {x:?}");
                    assert!((49.0..=851.0).contains(&cx) && (39.0..=441.0).contains(&cy), "{kind} {x:?}");
                }
            }
        }
    }

    #[test]
    fn long_words_shrink_to_fit_their_shape() {
        let d = Diagram { kind: "pyramid".into(), nodes: nodes_from_lines(["Internationalisation", "Build", "Test"]), ..Default::default() };
        let items = layout_items(&d, Xfrm::new(0.0, 0.0, 600.0, 300.0));
        let size = |k: usize| items[k].0.text.as_ref().and_then(|b| b.paragraphs[0].runs[0].props.size).unwrap_or(0.0);
        assert!(size(0) < 20.0 && size(0) >= 20.0 * MIN_SCALE - 0.5, "the tip's long word shrinks: {}", size(0));
        assert!((size(1) - 20.0).abs() < 1e-9, "short words keep their size");
    }

    #[test]
    fn colour_variants_pick_accents() {
        let d = |c: &str| Diagram { colors: c.into(), ..Default::default() };
        assert_eq!(
            (accent(&d("range45"), 0), accent(&d("range45"), 1), accent(&d("range45"), 2)),
            (SchemeSlot::Accent4, SchemeSlot::Accent5, SchemeSlot::Accent4)
        );
        assert_eq!(accent(&d("accent3Grad"), 4), SchemeSlot::Accent3);
        assert!(shade(&d("accent3Grad"), 1).is_some() && shade(&d("accent3"), 1).is_none());
    }

    #[test]
    fn assistants_sit_beside_their_parent() {
        let mut v = mk(&[0, 1, 1]);
        let a = add(&mut v, Some(0), "assistant", "Aide");
        assert_eq!((a, v[1].level, v[1].assistant), (1, 1, true));
        let d = Diagram { kind: "orgChart".into(), nodes: v, ..Default::default() };
        let items = layout_items(&d, Xfrm::new(0.0, 0.0, 600.0, 400.0));
        let boxed = |k: usize| items.iter().find(|(_, m)| m == &vec![k]).and_then(|(s, _)| s.xfrm).unwrap_or_default();
        let (ceo, aide, kid) = (boxed(0), boxed(1), boxed(2));
        assert!(aide.y > ceo.y && aide.y < kid.y, "the assistant's row is between its parent and the children");
        assert!(aide.x > ceo.x, "beside the parent's trunk");
    }
}
