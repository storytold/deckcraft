use super::nodes::{add, demote_node, nodes_from_lines, normalize, promote_node, remove, sync_text};
use super::paint::layout;
use super::{
    AutoFit, COLORS, Diagram, DiagramNode, EngineError, Geom, MAX_LEVEL, NO_ID_SPACE, Paragraph, Presentation, Result, STYLES, Session, Shape,
    ShapeId, ShapeKind, TextBody, Value, Xfrm, bad, bool_or, current_shapes, is_kind, json, shape_of, str_param, targets, xfrm_of,
};

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

pub(super) fn nodes(s: &mut Session, p: &Value) -> Result<Value> {
    let gid = target_group(s, p, "smartArt.nodes")?;
    let g = shape_of(s, gid)?;
    let mut d = g.diagram.unwrap_or_default();
    if let ShapeKind::Group { children, .. } = &g.kind {
        sync_text(&mut d, children);
    }
    Ok(json!({"id": gid.0, "kind": d.kind, "nodes": d.nodes}))
}

pub(super) fn set_nodes(s: &mut Session, p: &Value) -> Result<Value> {
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

pub(super) fn add_node(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok());
    let pos = str_param(p, "position").unwrap_or("after").to_string();
    let text = str_param(p, "text").unwrap_or("").to_string();
    edit_diagram(s, p, "smartArt.addNode", |d| {
        let at = add(&mut d.nodes, i, &pos, &text);
        Ok(json!({"index": at}))
    })
}

pub(super) fn remove_node(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.removeNode";
    let i = index(p, CMD)?;
    edit_diagram(s, p, CMD, |d| if remove(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(CMD, "no such node")) })
}

pub(super) fn promote(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.promote";
    let i = index(p, CMD)?;
    edit_diagram(s, p, CMD, |d| if promote_node(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(CMD, "already at the top level")) })
}

pub(super) fn demote(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.demote";
    let i = index(p, CMD)?;
    edit_diagram(s, p, CMD, |d| if demote_node(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(CMD, "nothing above to demote under")) })
}

/// Convert to SmartArt: the selected text shape's paragraphs become the nodes of a new graphic in
/// the shape's place.
pub(super) fn from_text(s: &mut Session, p: &Value) -> Result<Value> {
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
pub(super) fn to_text(s: &mut Session, p: &Value) -> Result<Value> {
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
pub(super) fn to_shapes(s: &mut Session, p: &Value) -> Result<Value> {
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

pub(super) fn set_picture(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.picture";
    let i = index(p, CMD)?;
    let media = if bool_or(p, "reset", false) {
        None
    } else {
        let (name, bytes) = crate::cmd::insert::media_bytes(p, CMD)?;
        let ct = crate::cmd::insert::content_type(&name, &bytes);
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

pub(super) fn change_style(s: &mut Session, p: &Value) -> Result<Value> {
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

pub(super) fn change_colors(s: &mut Session, p: &Value) -> Result<Value> {
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

pub(super) fn change_layout(s: &mut Session, p: &Value) -> Result<Value> {
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
