use super::nodes::{add, demote_node, nodes_from_lines, normalize, promote_node, remove, sync_text};
use super::paint::layout;
use super::{
    AutoFit, COLORS, Diagram, DiagramNode, EngineError, Geom, MAX_LEVEL, NO_ID_SPACE, Paragraph, Presentation, Result, STYLES, Session, Shape,
    ShapeId, ShapeKind, TextBody, Value, Xfrm, bad, bool_or, current_shapes, is_kind, json, shape_of, str_param, targets, xfrm_of,
};
use deckcraft_model::PicturePlace;

/// The SmartArt group that `id` is, or is inside.
pub(super) fn diagram_group(shapes: &[Shape], id: ShapeId) -> Option<&Shape> {
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

/// The diagram a single-user `insert.smartArt` builds from its parameters: `kind`, `nodes` or
/// `items` (leading tabs are sub-points), normalised as the command has always done. Public so the
/// collaboration normaliser can send the exact value the command would create (RFC §3.2.1).
pub fn insert_plan(p: &Value) -> Result<Diagram> {
    let asked = str_param(p, "kind").unwrap_or("process");
    let kind = if is_kind(asked) { asked } else { "process" };
    let mut nodes: Vec<DiagramNode> = match p.get("nodes") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad("insert.smartArt", e.to_string()))?,
        None => nodes_from_lines(p.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str)),
    };
    if nodes.is_empty() {
        nodes = (0..3)
            .map(|_| DiagramNode { text: "Text".into(), level: 0, picture: None, picture_place: None, assistant: false, title: String::new() })
            .collect();
    }
    normalize(&mut nodes);
    Ok(Diagram { kind: kind.into(), nodes, ..Default::default() })
}

/// Insert a SmartArt graphic whose whole value is already decided (`insert.smartArt`). The
/// collaboration adapter sends the value its normaliser built with [`insert_plan`], so every
/// replica regenerates the same drawing instead of running its own sample. `rect` is the final
/// box; without it the command's default box applies.
pub fn insert_value(s: &mut Session, d: Diagram, rect: Option<Xfrm>) -> Result<Value> {
    let size = s.doc()?.doc.slide_size;
    let area = rect.unwrap_or_else(|| Xfrm::new(size.width * 0.1, size.height * 0.28, size.width * 0.8, size.height * 0.6));
    let kind = d.kind.clone();
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
    let d = synced(&g).unwrap_or_default();
    Ok(json!({"id": gid.0, "kind": d.kind, "nodes": d.nodes}))
}

/// The value an outline, layout, style or colour command leaves, applied to a `Diagram` value.
///
/// The single-user commands below and the collaboration normaliser both call this, so a replica's
/// final value is exactly the one the author's command produced: the normaliser sends the whole
/// resulting diagram instead of the command's arguments (RFC §15).
pub fn plan(cmd: &str, p: &Value, d: &mut Diagram) -> Result<Value> {
    match cmd {
        "smartArt.setNodes" => {
            let given: Option<Vec<DiagramNode>> = match (p.get("nodes"), p.get("lines").and_then(Value::as_array)) {
                (Some(v), _) => Some(serde_json::from_value(v.clone()).map_err(|e| bad(cmd, e.to_string()))?),
                (None, Some(a)) => Some(nodes_from_lines(a.iter().filter_map(Value::as_str))),
                _ => None,
            };
            let one = p.get("index").map(|_| index(p, cmd)).transpose()?;
            let text = str_param(p, "text").map(String::from);
            let title = str_param(p, "title").map(String::from);
            if let Some(n) = given {
                d.nodes = n;
            }
            if let (Some(i), Some(t)) = (one, text) {
                d.nodes.get_mut(i).ok_or_else(|| bad(cmd, "no such node"))?.text = t;
            }
            if let (Some(i), Some(t)) = (one, title) {
                d.nodes.get_mut(i).ok_or_else(|| bad(cmd, "no such node"))?.title = t;
            }
            Ok(Value::Null)
        }
        "smartArt.addNode" => {
            let i = p.get("index").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok());
            let pos = str_param(p, "position").unwrap_or("after").to_string();
            let text = str_param(p, "text").unwrap_or("").to_string();
            let at = add(&mut d.nodes, i, &pos, &text);
            Ok(json!({"index": at}))
        }
        "smartArt.removeNode" => {
            let i = index(p, cmd)?;
            if remove(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(cmd, "no such node")) }
        }
        "smartArt.promote" => {
            let i = index(p, cmd)?;
            if promote_node(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(cmd, "already at the top level")) }
        }
        "smartArt.demote" => {
            let i = index(p, cmd)?;
            if demote_node(&mut d.nodes, i) { Ok(Value::Null) } else { Err(bad(cmd, "nothing above to demote under")) }
        }
        "smartArt.layout" => {
            let kind = str_param(p, "kind").filter(|k| is_kind(k)).ok_or_else(|| bad(cmd, "unknown layout"))?.to_string();
            if d.kind != kind {
                // PowerPoint's own layout no longer applies.
                d.layout_id.clear();
                d.layout_xml = None;
            }
            d.kind = kind;
            Ok(Value::Null)
        }
        "smartArt.style" => {
            let v = str_param(p, "style").filter(|k| STYLES.iter().any(|(id, _)| id == k)).ok_or_else(|| bad(cmd, "unknown style"))?.to_string();
            if d.style != v {
                d.style_xml = None;
            }
            d.style = v;
            Ok(Value::Null)
        }
        "smartArt.colors" => {
            let v = str_param(p, "colors").filter(|k| COLORS.iter().any(|(id, _)| id == k)).ok_or_else(|| bad(cmd, "unknown colours"))?.to_string();
            if d.colors != v {
                d.colors_xml = None;
            }
            d.colors = v;
            Ok(Value::Null)
        }
        other => Err(bad(other, "not a SmartArt value command")),
    }
}

/// The diagram of a group with the text typed into its shapes synced back, as every outline edit
/// (and the collaboration normaliser) sees it.
pub fn synced(group: &Shape) -> Option<Diagram> {
    let mut d = group.diagram.clone()?;
    if let ShapeKind::Group { children, .. } = &group.kind {
        sync_text(&mut d, children);
    }
    Some(d)
}

pub(super) fn set_nodes(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.setNodes";
    edit_diagram(s, p, CMD, |d| plan(CMD, p, d))
}

pub(super) fn add_node(s: &mut Session, p: &Value) -> Result<Value> {
    edit_diagram(s, p, "smartArt.addNode", |d| plan("smartArt.addNode", p, d))
}

pub(super) fn remove_node(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.removeNode";
    edit_diagram(s, p, CMD, |d| plan(CMD, p, d))
}

pub(super) fn promote(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.promote";
    edit_diagram(s, p, CMD, |d| plan(CMD, p, d))
}

pub(super) fn demote(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.demote";
    edit_diagram(s, p, CMD, |d| plan(CMD, p, d))
}

/// Most nodes one `smartArt.setDiagram` may carry, and the most text they may hold.
const MAX_NODES: usize = 4096;
const MAX_NODE_TEXT: usize = 64 * 1024;
/// Longest diagram value a confirmed or inserted SmartArt command may carry, as JSON bytes.
pub const MAX_DIAGRAM_JSON: usize = 128 * 1024;
/// Longest text body a collaborative `smartArt.toText` may carry, and the most paragraphs and
/// runs it may hold — the same bounds `shape.setField` puts on a restored body.
pub const MAX_CONVERSION_TEXT_JSON: usize = 192 * 1024;
const MAX_CONVERSION_PARAGRAPHS: usize = 10_000;
const MAX_CONVERSION_RUNS: usize = 50_000;

/// Most bytes one node picture may carry: the media channel's picture budget (8 MiB), checked by
/// the engine too so a node picture is refused before the first write whatever path built it.
pub const MAX_NODE_PICTURE_BYTES: usize = 8 * 1024 * 1024;

/// The top-level node a picture step names: the node itself when it is top level, else the
/// nearest top-level node above it (a sub-point's picture is its top-level node's, as the
/// placeholder shows). `None` when the index is past the outline.
pub fn top_node(d: &Diagram, index: usize) -> Option<usize> {
    if index >= d.nodes.len() {
        return None;
    }
    (0..=index).rev().find(|k| d.nodes[*k].level == 0)
}

/// Whether a place leaves a positive visible area and a usable opacity. A crop side is a fraction
/// of the image (negative pads), so the visible part must be more than nothing.
fn place_ok(place: &PicturePlace) -> bool {
    place.crop.iter().all(|v| v.is_finite())
        && place.crop[0] + place.crop[2] < 1.0
        && place.crop[1] + place.crop[3] < 1.0
        && place.alpha.is_none_or(|a| a.is_finite() && (0.0..=1.0).contains(&a))
}

/// A node picture's place from `place` (`crop: [l, t, r, b]`, `alpha?`), refused when it leaves
/// no visible area or names a field the place does not have. Absent or `null` = the whole image.
pub fn place_param(p: &Value, cmd: &str) -> Result<Option<PicturePlace>> {
    let Some(value) = p.get("place") else { return Ok(None) };
    if value.is_null() {
        return Ok(None);
    }
    let obj = value.as_object().ok_or_else(|| bad(cmd, "`place` must be an object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "crop" | "alpha") {
            return Err(bad(cmd, format!("`place` has no field `{key}`")));
        }
    }
    let mut place = PicturePlace::default();
    if let Some(crop) = obj.get("crop") {
        let a = crop.as_array().filter(|a| a.len() == 4).ok_or_else(|| bad(cmd, "`place.crop` must be [left, top, right, bottom]"))?;
        for (i, v) in a.iter().enumerate() {
            place.crop[i] = v.as_f64().ok_or_else(|| bad(cmd, "`place.crop` values must be numbers"))?;
        }
    }
    if let Some(alpha) = obj.get("alpha").filter(|v| !v.is_null()) {
        place.alpha = Some(alpha.as_f64().ok_or_else(|| bad(cmd, "`place.alpha` must be a number"))?);
    }
    if !place_ok(&place) {
        return Err(bad(cmd, "the picture's place leaves no visible area"));
    }
    Ok(Some(place))
}

/// The final box a conversion carries. The sender computes it against the shape it sees and every
/// replica uses it unchanged, so the replaced shape keeps the same place and size. The value is
/// hostile input like every other field: four finite numbers, a positive size, and a sane bound.
fn conversion_rect(p: &Value, cmd: &str) -> Result<Xfrm> {
    let a = p.get("rect").and_then(Value::as_array).filter(|a| a.len() == 4).ok_or_else(|| bad(cmd, "missing `rect` ([x, y, w, h])"))?;
    let f = |i: usize| a[i].as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, "the rect is not four finite numbers"));
    let (x, y, w, h) = (f(0)?, f(1)?, f(2)?.abs(), f(3)?.abs());
    if w <= 0.0 || h <= 0.0 || w > 1_000_000.0 || h > 1_000_000.0 {
        return Err(bad(cmd, "the rect has an invalid size"));
    }
    Ok(Xfrm::new(x, y, w, h))
}

/// Replace a text shape with a SmartArt graphic whose whole value is already decided
/// (`smartArt.fromText`). The confirmed command of the conversion: the operation carries the
/// diagram the author's command planned and the final box, so every replica builds the same
/// graphic in the source shape's place in the z-order. Everything is checked before the first
/// write: the value's layout, nodes and text, the box, and that the source is a top-level shape.
pub(super) fn replace_with_diagram(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.replaceWithDiagram";
    let value = p.get("diagram").ok_or_else(|| bad(CMD, "missing `diagram`"))?;
    let d = verified_diagram(value, CMD)?;
    let area = conversion_rect(p, CMD)?;
    let id = *planned_ids(p, &s.doc()?.selection, CMD)?.first().ok_or_else(|| bad(CMD, "select a text box"))?;
    let gid = s.edit(|doc, sel| {
        let at = {
            let shapes = current_shapes(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
            shapes.iter().position(|x| x.id == id).ok_or_else(|| bad(CMD, "the shape isn't at the top of the slide"))?
        };
        let kind = d.kind.clone();
        let children = regenerate(CMD, doc, &d, area)?;
        let gid = ShapeId(doc.alloc_id().ok_or_else(|| bad(CMD, NO_ID_SPACE))?);
        let g = Shape {
            id: gid,
            name: format!("SmartArt ({kind})"),
            xfrm: Some(area),
            kind: ShapeKind::Group { children, child: area },
            diagram: Some(d.clone()),
            ..Default::default()
        };
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        *shapes.get_mut(at).ok_or_else(|| bad(CMD, "no such shape"))? = g;
        sel.shapes = vec![gid];
        sel.text = None;
        Ok(gid)
    })?;
    Ok(json!({"id": gid.0}))
}

/// Replace a SmartArt graphic with a text box whose whole body is already decided
/// (`smartArt.toText`). The body and box are the author's; the value is size-bounded and
/// deserialised before the first write, and the target must still be (inside) a SmartArt graphic.
pub(super) fn replace_with_text(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.replaceWithText";
    let value = p.get("body").ok_or_else(|| bad(CMD, "missing `body`"))?;
    let bytes = serde_json::to_vec(value).map_err(|e| bad(CMD, format!("invalid text body: {e}")))?;
    if bytes.len() > MAX_CONVERSION_TEXT_JSON {
        return Err(bad(CMD, "the text body is too large"));
    }
    let body: TextBody = serde_json::from_value(value.clone()).map_err(|e| bad(CMD, format!("invalid text body: {e}")))?;
    if body.paragraphs.len() > MAX_CONVERSION_PARAGRAPHS || body.paragraphs.iter().map(|q| q.runs.len()).sum::<usize>() > MAX_CONVERSION_RUNS {
        return Err(bad(CMD, "the text body has too many paragraphs or runs"));
    }
    let area = conversion_rect(p, CMD)?;
    let gid = *planned_ids(p, &s.doc()?.selection, CMD)?.first().ok_or_else(|| bad(CMD, "select a SmartArt graphic"))?;
    let new = s.edit(|doc, sel| {
        let at = {
            let shapes = current_shapes(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
            let g = diagram_group(shapes, gid).ok_or_else(|| bad(CMD, "the shape is not a SmartArt graphic"))?;
            shapes.iter().position(|x| x.id == g.id).ok_or_else(|| bad(CMD, "the graphic isn't at the top of the slide"))?
        };
        let id = ShapeId(doc.alloc_id().ok_or_else(|| bad(CMD, NO_ID_SPACE))?);
        let tb = Shape {
            id,
            name: "Text".into(),
            xfrm: Some(area),
            text_box: true,
            text: Some(body.clone()),
            geom: Geom::preset("rect"),
            ..Default::default()
        };
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        *shapes.get_mut(at).ok_or_else(|| bad(CMD, "no such shape"))? = tb;
        sel.shapes = vec![id];
        sel.text = None;
        Ok(id)
    })?;
    Ok(json!({"id": new.0}))
}

/// Detach a SmartArt graphic as an ordinary group (`smartArt.toShapes`). The drawing is kept as
/// it is; only the outline is dropped, so the step is deterministic from the authoritative state
/// and carries no value. The target is checked before the first write.
pub(super) fn detach(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.detach";
    let gid = *planned_ids(p, &s.doc()?.selection, CMD)?.first().ok_or_else(|| bad(CMD, "select a SmartArt graphic"))?;
    s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        let g = diagram_group(shapes, gid).ok_or_else(|| bad(CMD, "the shape is not a SmartArt graphic"))?;
        let gid = g.id;
        let g = deckcraft_model::find_shape_mut(shapes, gid).ok_or_else(|| bad(CMD, "no such shape"))?;
        g.diagram = None;
        g.name = "Group".into();
        sel.shapes = vec![gid];
        Ok(())
    })?;
    Ok(Value::Null)
}

/// Check a whole diagram value before it is written: it must deserialise into the model type, its
/// layout, style and colours must be known, and it must stay inside the node, text and JSON bounds.
/// Both `smartArt.setDiagram` and a collaborative `insert.smartArt` run this, so hostile input is
/// refused before the first write.
pub fn verified_diagram(value: &Value, cmd: &str) -> Result<Diagram> {
    let bytes = serde_json::to_vec(value).map_err(|e| bad(cmd, format!("invalid diagram: {e}")))?;
    if bytes.len() > MAX_DIAGRAM_JSON {
        return Err(bad(cmd, "the diagram is too large"));
    }
    let d: Diagram = serde_json::from_value(value.clone()).map_err(|e| bad(cmd, format!("invalid diagram: {e}")))?;
    if !is_kind(&d.kind) {
        return Err(bad(cmd, "unknown layout"));
    }
    if d.nodes.len() > MAX_NODES {
        return Err(bad(cmd, format!("at most {MAX_NODES} nodes")));
    }
    if d.nodes.iter().map(|n| n.text.len() + n.title.len()).sum::<usize>() > MAX_NODE_TEXT {
        return Err(bad(cmd, "the outline is too large"));
    }
    if d.nodes.iter().filter_map(|n| n.picture_place.as_ref()).any(|p| !place_ok(p)) {
        return Err(bad(cmd, "a node picture's place leaves no visible area"));
    }
    if !d.style.is_empty() && !STYLES.iter().any(|(id, _)| id == &d.style) {
        return Err(bad(cmd, "unknown style"));
    }
    if !d.colors.is_empty() && !COLORS.iter().any(|(id, _)| id == &d.colors) {
        return Err(bad(cmd, "unknown colours"));
    }
    Ok(d)
}

/// Replace a graphic's whole diagram value and regenerate its shapes (`smartArt.setDiagram`).
///
/// This is the confirmed command of every outline, layout, style and colour edit and the exact
/// restore of a step the collaboration adapter records (ADR 0010): the normaliser sends the whole
/// `Diagram` the author's command produced, so every replica regenerates from the same final value
/// instead of merging arguments against its own copy. The value is still hostile input: it is
/// size-bounded, deserialised into the model type and its layout, style and colours are checked
/// before the first write.
pub(super) fn set_diagram(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.setDiagram";
    let value = p.get("diagram").ok_or_else(|| bad(CMD, "missing `diagram`"))?;
    let d = verified_diagram(value, CMD)?;
    edit_diagram(s, p, CMD, |cur| {
        *cur = d.clone();
        Ok(Value::Null)
    })
}

/// The shapes a conversion plan addresses: `ids`/`id` first, then the selection, exactly as the
/// single-user command's `targets` resolves them. The collaboration normaliser calls the plan
/// with its own `DocState`, so the value it sends is the one the command would leave.
fn planned_ids(p: &Value, sel: &crate::Selection, cmd: &str) -> Result<Vec<ShapeId>> {
    if let Some(a) = p.get("ids").and_then(Value::as_array) {
        let mut ids = Vec::with_capacity(a.len());
        for item in a {
            let id = item.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(cmd, format!("`ids` has an invalid shape id: {item}")))?;
            ids.push(ShapeId(id));
        }
        if !ids.is_empty() {
            return Ok(ids);
        }
    }
    if let Some(item) = p.get("id") {
        let id = item.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(cmd, format!("`id` is not a shape id: {item}")))?;
        return Ok(vec![ShapeId(id)]);
    }
    Ok(sel.shapes.clone())
}

/// Everything a `smartArt.fromText` step needs, decided against the sender's document: which
/// shape is replaced, the whole diagram its paragraphs become, and the final box. The engine
/// command and the collaboration normaliser both call it, so a replica regenerates from the
/// same value and a refusal happens before anything is written (RFC §15).
#[derive(Clone, Debug)]
pub struct FromTextPlan {
    pub id: ShapeId,
    pub diagram: Diagram,
    pub rect: Xfrm,
}

pub fn from_text_plan(doc: &Presentation, sel: &crate::Selection, p: &Value) -> Result<FromTextPlan> {
    const CMD: &str = "smartArt.fromText";
    let kind = str_param(p, "kind").filter(|k| is_kind(k)).unwrap_or("list").to_string();
    let id = *planned_ids(p, sel, CMD)?.first().ok_or_else(|| bad(CMD, "select a text box"))?;
    let shapes = current_shapes(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
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
    let mut a = xfrm_of(doc, sel, sh);
    let size = doc.slide_size;
    a.w = a.w.max(size.width * 0.5).min(size.width);
    a.h = a.h.max(size.height * 0.35).min(size.height);
    a.x = a.x.min(size.width - a.w).max(0.0);
    a.y = a.y.min(size.height - a.h).max(0.0);
    a.rot = 0.0;
    let diagram = Diagram { kind, nodes, ..Default::default() };
    // The value is what the whole step would carry, so its node/text/JSON bounds are checked here
    // as well: an outline too large to regenerate is refused before any replica writes.
    let value = serde_json::to_value(&diagram).map_err(|e| bad(CMD, format!("invalid diagram: {e}")))?;
    verified_diagram(&value, CMD)?;
    Ok(FromTextPlan { id, diagram, rect: a })
}

/// Convert to SmartArt: the selected text shape's paragraphs become the nodes of a new graphic in
/// the shape's place.
pub(super) fn from_text(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.fromText";
    let plan = {
        let st = s.doc()?;
        from_text_plan(&st.doc, &st.selection, p)?
    };
    let gid = s.edit(|doc, sel| {
        let (id, d, area) = (plan.id, plan.diagram.clone(), plan.rect);
        let children = regenerate(CMD, doc, &d, area)?;
        let gid = ShapeId(doc.alloc_id().ok_or_else(|| bad(CMD, NO_ID_SPACE))?);
        let kind = d.kind.clone();
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

/// Everything a `smartArt.toText` step needs, decided against the sender's document: the graphic,
/// the whole text body its outline becomes, and the final box. The text sizes are the ones the
/// graphic shows at, so the body carries the same runs and ends the single-user command builds.
#[derive(Clone, Debug)]
pub struct ToTextPlan {
    pub id: ShapeId,
    pub body: TextBody,
    pub rect: Xfrm,
}

pub fn to_text_plan(doc: &Presentation, sel: &crate::Selection, p: &Value) -> Result<ToTextPlan> {
    const CMD: &str = "smartArt.toText";
    let id = *planned_ids(p, sel, CMD)?.first().ok_or_else(|| bad(CMD, "select a SmartArt graphic"))?;
    let shapes = current_shapes(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
    let g = diagram_group(shapes, id).ok_or_else(|| bad(CMD, "select a SmartArt graphic"))?;
    let d = synced(g).ok_or_else(|| bad(CMD, "select a SmartArt graphic"))?;
    // The sizes the graphic's text shows at: its titles' and its bullets' (shrunk to fit included).
    let (title_pt, bullet_pt) = text_sizes(g.children());
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
    // Hostile input check before any write: the whole body travels in the operation, so its JSON
    // size and paragraph/run counts are bounded here as they are in `shape.setField`.
    let value = serde_json::to_value(&body).map_err(|e| bad(CMD, format!("invalid text body: {e}")))?;
    let bytes = serde_json::to_vec(&value).map_err(|e| bad(CMD, format!("invalid text body: {e}")))?;
    if bytes.len() > MAX_CONVERSION_TEXT_JSON {
        return Err(bad(CMD, "the converted text is too large"));
    }
    if body.paragraphs.len() > MAX_CONVERSION_PARAGRAPHS || body.paragraphs.iter().map(|q| q.runs.len()).sum::<usize>() > MAX_CONVERSION_RUNS {
        return Err(bad(CMD, "the converted text has too many paragraphs or runs"));
    }
    Ok(ToTextPlan { id: g.id, body, rect: g.xfrm.unwrap_or_default() })
}

/// Convert to Text: a text box with the outline, one bulleted paragraph per node.
pub(super) fn to_text(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.toText";
    let plan = {
        let st = s.doc()?;
        to_text_plan(&st.doc, &st.selection, p)?
    };
    let new = s.edit(|doc, sel| {
        let (gid, body, area) = (plan.id, plan.body.clone(), plan.rect);
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        let at = shapes.iter().position(|x| x.id == gid).ok_or_else(|| bad(CMD, "the graphic isn't at the top of the slide"))?;
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

/// The graphic a `smartArt.toShapes` step detaches: the id checked before the first write, so the
/// collaboration normaliser can refuse a shape that is not a SmartArt graphic here.
pub fn to_shapes_plan(doc: &Presentation, sel: &crate::Selection, p: &Value) -> Result<ShapeId> {
    const CMD: &str = "smartArt.toShapes";
    let id = *planned_ids(p, sel, CMD)?.first().ok_or_else(|| bad(CMD, "select a SmartArt graphic"))?;
    let shapes = current_shapes(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
    diagram_group(shapes, id).map(|g| g.id).ok_or_else(|| bad(CMD, "select a SmartArt graphic"))
}

/// Convert to Shapes: keep the drawing as an ordinary group.
pub(super) fn to_shapes(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.toShapes";
    let gid = {
        let st = s.doc()?;
        to_shapes_plan(&st.doc, &st.selection, p)?
    };
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
pub(super) fn set_picture(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.picture";
    let i = index(p, CMD)?;
    let place = place_param(p, CMD)?;
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
        let top = top_node(d, i).ok_or_else(|| bad(CMD, "no such node"))?;
        let n = &mut d.nodes[top];
        n.picture = media;
        n.picture_place = if media.is_some() { place.clone() } else { None };
        Ok(Value::Null)
    })
}

/// Changes one node's picture as a final value (`smartArt.setPicture`), the confirmed command of
/// `smartArt.picture`: the picture bytes come from the media channel (the operation carried only
/// their digest) and the place is a final value. The graphic must be a top-level SmartArt group,
/// the node must exist, the layout must not change, and the picture must be one this document
/// accepts — all checked before the first write, so a hostile operation regenerates nothing.
pub fn set_picture_value(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.setPicture";
    let i = index(p, CMD)?;
    let place = place_param(p, CMD)?;
    let reset = bool_or(p, "reset", false);
    // Plan against the authoritative document before the first write.
    let gid = target_group(s, p, CMD)?;
    let top = {
        let st = s.doc()?;
        let shapes = current_shapes(&st.doc, &st.selection).ok_or_else(|| bad(CMD, "no slide"))?;
        if !shapes.iter().any(|x| x.id == gid) {
            return Err(bad(CMD, "the graphic is not at the top of the slide"));
        }
        let d = synced(&shape_of(s, gid)?).ok_or_else(|| bad(CMD, "not a SmartArt graphic"))?;
        top_node(&d, i).ok_or_else(|| bad(CMD, "no such node"))?
    };
    let media = if reset {
        None
    } else {
        let (name, bytes) = crate::cmd::insert::media_bytes(p, CMD)?;
        if bytes.len() > MAX_NODE_PICTURE_BYTES {
            return Err(bad(CMD, "the picture is too large"));
        }
        let ct = crate::cmd::insert::content_type(&name, &bytes);
        if !ct.starts_with("image/") {
            return Err(bad(CMD, format!("{name} is not a picture")));
        }
        if ct != "image/svg+xml" && deckcraft_render::decode_image(&bytes).is_none() {
            return Err(bad(CMD, format!("{name}: the picture can't be read")));
        }
        Some(s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad(CMD, NO_ID_SPACE)))?)
    };
    edit_diagram(s, p, CMD, |d| {
        let node = d.nodes.get_mut(top).ok_or_else(|| bad(CMD, "no such node"))?;
        node.picture = media;
        node.picture_place = if media.is_some() { place.clone() } else { None };
        Ok(Value::Null)
    })
}

pub(super) fn change_style(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.style";
    edit_diagram(s, p, CMD, |d| plan(CMD, p, d))
}

pub(super) fn change_colors(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.colors";
    edit_diagram(s, p, CMD, |d| plan(CMD, p, d))
}

pub(super) fn change_layout(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "smartArt.layout";
    edit_diagram(s, p, CMD, |d| plan(CMD, p, d))
}
