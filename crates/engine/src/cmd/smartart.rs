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

mod diagrams;
mod edit;
mod layout;
mod nodes;
mod paint;

use edit::{
    add_node, change_colors, change_layout, change_style, demote, from_text, nodes, promote, remove_node, set_nodes, set_picture, to_shapes, to_text,
};
pub use edit::{insert, selected};
pub use nodes::{add, demote_node, nodes_from_lines, normalize, promote_node, remove};
pub use paint::{BRICK_CAMERA, is_3d, layout};
#[cfg(test)]
use paint::{MIN_SCALE, accent, layout_items, shade};

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
