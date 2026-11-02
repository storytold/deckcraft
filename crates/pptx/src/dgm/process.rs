//! Export a flat process using its actual canvas dimensions. Using a full-parent width for each
//! node and then asking the linear algorithm to shrink it leaves the height unscaled in PowerPoint.

use crate::xml::{A, El, Node, W};
use deckcraft_geom::Xfrm;
use deckcraft_model::{Diagram, Shape};

pub(crate) struct Parts {
    pub layout: Vec<u8>,
    pub style: Vec<u8>,
    pub colors: Vec<u8>,
}

/// Imported/custom layouts and hierarchical processes keep their existing export path.
pub(crate) fn parts(d: &Diagram, children: &[Shape], child: &Xfrm, frame: &Xfrm) -> Option<Parts> {
    if d.kind != "process"
        || !d.style.is_empty()
        || d.layout_xml.is_some()
        || d.style_xml.is_some()
        || d.colors_xml.is_some()
        || d.nodes.is_empty()
        || d.nodes.iter().any(|n| n.level != 0)
        || [child.w, child.h, frame.w, frame.h].iter().any(|v| !v.is_finite() || *v <= 0.0)
    {
        return None;
    }
    let nodes: Vec<_> = children.iter().filter(|s| s.text.is_some()).collect();
    if nodes.len() != d.nodes.len() {
        return None;
    }
    let first = nodes.first()?;
    let rect = first.xfrm?;
    let font = nodes
        .iter()
        .filter_map(|s| s.text.as_ref())
        .flat_map(|t| &t.paragraphs)
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.props.size)
        .filter(|v| v.is_finite() && *v > 0.0)
        .reduce(f64::min)?;
    let gap = nodes.get(1).and_then(|s| s.xfrm).map_or(0.0, |x| x.x - rect.x - rect.w);
    let arrow = children.iter().find(|s| s.text.is_none()).and_then(|s| s.xfrm);
    let (aw, ah) = arrow.map_or((0.0, 0.0), |a| (a.w, a.h));
    if [rect.w, rect.h, gap, aw, ah].iter().any(|v| !v.is_finite() || *v < 0.0) || aw > gap {
        return None;
    }
    let mut root = crate::xml::parse(&super::layout_xml("process")).ok()?.root;
    let diagram = root.elements_mut().find(|e| e.is("layoutNode"))?;
    let constraints = diagram.elements_mut().find(|e| e.is("constrLst"))?;
    constraints.children.clear();
    for (typ, name, size) in [
        ("w", "node", rect.w),
        ("h", "node", rect.h),
        ("w", "space", aw),
        ("h", "space", ah),
        ("w", "gapBefore", (gap - aw) / 2.0),
        ("w", "gapAfter", (gap - aw) / 2.0),
    ] {
        let mut w = W::frag();
        // Every constraint references parent width; heights must use the vertical frame scale.
        let fact = if typ == "h" { size / child.h * frame.h / frame.w } else { size / child.w };
        if !fact.is_finite() {
            return None;
        }
        w.empty("dgm:constr", A::new().a("type", typ).a("for", "ch").a("forName", name).a("refType", "w").a("fact", fact));
        constraints.children.push(Node::El(crate::xml::parse(&w.finish()).ok()?.root));
    }
    let mut w = W::frag();
    w.empty("dgm:constr", A::new().a("type", "primFontSz").a("for", "ch").a("forName", "node").a("op", "equ").a("val", font));
    constraints.children.push(Node::El(crate::xml::parse(&w.finish()).ok()?.root));
    let each = diagram.elements_mut().find(|e| e.is("forEach"))?;
    let siblings = each.elements_mut().find(|e| e.is("forEach"))?;
    let mut space = siblings.child("layoutNode")?.clone();
    space.set_attr("styleLbl", "processArrow");
    space.elements_mut().find(|e| e.is("shape"))?.set_attr("type", "rightArrow");
    let blank = |name: &str| {
        let mut e = space.clone();
        e.set_attr("name", name);
        e.attrs.retain(|(k, _)| k != "styleLbl");
        if let Some(shape) = e.elements_mut().find(|e| e.is("shape")) {
            shape.attrs.retain(|(k, _)| k != "type");
        }
        e
    };
    siblings.children = vec![Node::El(blank("gapBefore")), Node::El(space.clone()), Node::El(blank("gapAfter"))];
    let style = arrow_style(&super::style_xml(&d.style), false)?;
    let colors = arrow_style(&super::colors_xml(&d.colors), true)?;
    Some(Parts { layout: root.to_xml().into_bytes(), style, colors })
}

/// A separate style label makes arrows use the same sequence of accents as nodes without
/// consuming every second node colour. The pale arrows match the canvas' light decoration.
fn arrow_style(xml: &[u8], colors: bool) -> Option<Vec<u8>> {
    let mut root = crate::xml::parse(xml).ok()?.root;
    let mut style = root.child("styleLbl")?.clone();
    style.set_attr("name", "processArrow");
    if colors {
        for e in style.elements_mut().filter(|e| e.is("fillClrLst")) {
            for color in e.elements_mut() {
                color.children.clear();
                for (tag, value) in [("a:lumMod", "20000"), ("a:lumOff", "80000")] {
                    color.children.push(Node::El(El { name: tag.into(), attrs: vec![("val".into(), value.into())], children: vec![] }));
                }
            }
        }
    }
    root.children.push(Node::El(style));
    Some(root.to_xml().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::{DiagramNode, Geom, text::TextBody};

    #[test]
    fn process_geometry_and_font_come_from_the_canvas_not_a_fixed_template() {
        for (count, width, height, size) in [(2, 600.0, 240.0, 16.0), (4, 800.0, 300.0, 20.0), (6, 900.0, 200.0, 12.0)] {
            let d = Diagram {
                kind: "process".into(),
                nodes: (0..count).map(|_| DiagramNode { text: "Node".into(), ..Default::default() }).collect(),
                ..Default::default()
            };
            let child = Xfrm::new(30.0, 40.0, width, height);
            let gap = width * 0.06;
            let nw = (width - gap * (count - 1) as f64) / count as f64;
            let nh = (height * 0.5).min(nw * 0.65);
            let mut children = vec![];
            for i in 0..count {
                let mut body = TextBody::from_text("Node");
                body.paragraphs[0].runs[0].props.size = Some(size);
                let x = child.x + i as f64 * (nw + gap);
                children.push(Shape { xfrm: Some(Xfrm::new(x, child.y, nw, nh)), text: Some(body), ..Default::default() });
                if i + 1 < count {
                    children.push(Shape {
                        xfrm: Some(Xfrm::new(x + nw, child.y, gap * 0.7, gap * 0.63)),
                        geom: Geom::preset("rightArrow"),
                        ..Default::default()
                    });
                }
            }
            let p = parts(&d, &children, &child, &child).expect("process");
            let root = crate::xml::parse(&p.layout).expect("layout").root;
            let constraints = root.child("layoutNode").unwrap().child("constrLst").unwrap();
            let h = constraints.elements().find(|e| e.attr("type") == Some("h") && e.attr("forName") == Some("node")).unwrap();
            assert_eq!(h.attr("refType"), Some("w"));
            assert!((h.f64("fact").unwrap() - nh / width).abs() < 1e-9);
            let font = constraints.elements().find(|e| e.attr("type") == Some("primFontSz")).unwrap();
            assert_eq!(font.f64("val"), Some(size));
            assert!(String::from_utf8(p.layout).unwrap().contains("rightArrow"));
            assert!(String::from_utf8(p.colors).unwrap().contains("processArrow"));
            let stretched = Xfrm::new(0.0, 0.0, width * 0.8, height * 1.5);
            let p = parts(&d, &children, &child, &stretched).unwrap();
            let root = crate::xml::parse(&p.layout).unwrap().root;
            let constraints = root.child("layoutNode").unwrap().child("constrLst").unwrap();
            for (kind, expected) in [("w", nw * 0.8), ("h", nh * 1.5)] {
                let constraint = constraints.elements().find(|e| e.attr("type") == Some(kind) && e.attr("forName") == Some("node")).unwrap();
                assert!((constraint.f64("fact").unwrap() * stretched.w - expected).abs() < 1e-9);
            }
            let custom = Diagram { layout_xml: Some("custom".into()), ..d };
            assert!(parts(&custom, &children, &child, &child).is_none(), "never rewrite an imported layout");
        }
    }
}
