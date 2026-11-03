//! Preserve imported diagram model IDs, presentation points and their connected package parts.
use crate::{
    opc,
    read::{Imp, Part},
    xml::El,
};
use deckcraft_model::chart::{ChartKept, ChartPart, ChartRel};
use deckcraft_model::{DiagramKept, Shape};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const MAX_BYTES: usize = 64 << 20;
const MAX_PARTS: usize = 512;

/// Apply only text/title changes to an imported diagram, preserving shape geometry and model IDs.
/// An ambiguous binding returns an error rather than silently replacing its layout.
pub fn edit_text(source: &Shape, updated: &deckcraft_model::Diagram) -> Result<Option<Shape>, String> {
    let Some(original) = source.diagram.as_ref().filter(|d| d.imported && d.kept.is_some()) else {
        return Ok(None);
    };
    let stripped = |d: &deckcraft_model::Diagram| {
        let mut d = d.clone();
        d.kept = None;
        for node in &mut d.nodes {
            node.text.clear();
            node.title.clear();
        }
        d
    };
    if stripped(original) != stripped(updated) {
        return Ok(None);
    }
    if original.nodes == updated.nodes {
        return Ok(Some(source.clone()));
    }
    edit_text_inner(source, updated).map(Some).ok_or_else(||
        "Imported SmartArt text cannot be mapped safely to its cached shapes. The diagram was left unchanged; convert it to shapes for independent editing or explicitly choose a new layout.".into())
}

fn replace_text(body: &mut El, text: &str) -> Option<()> {
    if text.contains(['\n', '\r']) {
        return None;
    }
    fn visit(el: &mut El, text: &str, first: &mut bool, depth: usize) {
        if depth > crate::xml::MAX_DEPTH {
            return;
        }
        if el.is("t") {
            el.children = vec![crate::xml::Node::Text(if *first { String::new() } else { text.into() })];
            *first = true;
            return;
        }
        for c in el.elements_mut() {
            visit(c, text, first, depth + 1);
        }
    }
    let mut seen = false;
    // The caller supplies a diagram text body, whose root is also called t; visit its paragraphs.
    for c in body.elements_mut() {
        visit(c, text, &mut seen, 0);
    }
    seen.then_some(())
}

fn edit_text_inner(source: &Shape, updated: &deckcraft_model::Diagram) -> Option<Shape> {
    let original = source.diagram.as_ref()?;
    let old = original.kept.as_ref()?;
    if fingerprint(source)? != old.fingerprint {
        return None;
    }
    let mut out = source.clone();
    let mut kept = old.clone();
    let index = |role: &str| {
        let root = kept.package.rels.iter().find(|r| r.id == role)?;
        kept.package.parts.iter().position(|p| opc::norm(&p.name) == opc::norm(&root.part))
    };
    let data_index = index("dm")?;
    let drawing_index = index("drawing")?;
    let mut data = crate::xml::parse(&kept.package.parts.get(data_index)?.bytes).ok()?.root;
    let mut drawing = crate::xml::parse(&kept.package.parts.get(drawing_index)?.bytes).ok()?.root;
    let nodes = super::outline_ids(&data);
    if nodes.len() != updated.nodes.len() {
        return None;
    }
    let mut changes = HashMap::new();
    let mut titles = HashMap::new();
    let mut changed_nodes = HashSet::new();
    for (i, (id, _)) in nodes.iter().enumerate() {
        let before = original.nodes.get(i)?;
        let after = updated.nodes.get(i)?;
        if before.text != after.text {
            changes.insert(id.clone(), after.text.clone());
            changed_nodes.insert(id.clone());
        }
        if before.title != after.title {
            titles.insert(id.clone(), after.title.clone());
        }
    }
    let mut owners = HashMap::new();
    for pt in data.child("ptLst")?.children_named("pt") {
        let Some(pr) = pt.child("prSet") else {
            continue;
        };
        let Some(assoc) = pr.attr("presAssocID") else {
            continue;
        };
        let id = pt.attr("modelId")?;
        let is_title = pr.attr("presName").is_some_and(|n| n.starts_with("titleText"));
        if let Some(text) = if is_title { titles.get(assoc) } else { changes.get(assoc) } {
            owners.insert(id.to_string(), (assoc.to_string(), is_title));
            changes.insert(id.to_string(), text.clone());
        }
    }
    if let Some(cxns) = data.child("cxnLst") {
        for c in cxns.children_named("cxn").filter(|c| c.attr("type") == Some("presOf")) {
            let (Some(src), Some(dst)) = (c.attr("srcId"), c.attr("destId")) else {
                continue;
            };
            if let Some(text) = changes.get(src).cloned() {
                changes.entry(dst.to_string()).or_insert(text);
                owners.entry(dst.into()).or_insert((src.into(), false));
            }
        }
    }
    for pt in data.elements_mut().find(|e| e.is("ptLst"))?.elements_mut().filter(|e| e.is("pt")) {
        if let Some(text) = pt.attr("modelId").and_then(|id| changes.get(id))
            && let Some(body) = pt.elements_mut().find(|e| e.is("t"))
        {
            replace_text(body, text)?;
        }
    }
    fn drawing_text(el: &mut El, changes: &HashMap<String, String>, found: &mut HashSet<String>, depth: usize) -> Option<()> {
        if depth > crate::xml::MAX_DEPTH {
            return None;
        }
        if el.is("sp")
            && let Some(id) = el.attr("modelId").map(str::to_string)
            && let Some(text) = changes.get(&id)
            && let Some(body) = el.elements_mut().find(|e| e.is("txBody"))
        {
            replace_text(body, text)?;
            found.insert(id);
        }
        for c in el.elements_mut() {
            drawing_text(c, changes, found, depth + 1)?;
        }
        Some(())
    }
    let mut found = HashSet::new();
    drawing_text(&mut drawing, &changes, &mut found, 0)?;
    for (id, title) in changed_nodes.iter().map(|id| (id, false)).chain(titles.keys().map(|id| (id, true))) {
        if !found.iter().any(|f| (!title && f == id) || owners.get(f).is_some_and(|(owner, is_title)| owner == id && *is_title == title)) {
            return None;
        }
    }
    fn shapes(items: &mut [Shape], changes: &HashMap<String, String>, found: &mut HashSet<String>, depth: usize) -> Option<()> {
        if depth > 32 {
            return None;
        }
        for s in items {
            if let Some(id) = &s.diagram_node
                && let Some(text) = changes.get(id)
                && let Some(body) = &mut s.text
            {
                let mut first = false;
                for p in &mut body.paragraphs {
                    for r in &mut p.runs {
                        if !matches!(r.kind, deckcraft_model::text::RunKind::Text) {
                            return None;
                        }
                        r.text = if first { String::new() } else { text.clone() };
                        first = true;
                    }
                }
                if first {
                    found.insert(id.clone());
                }
            }
            if let Some(children) = s.children_mut() {
                shapes(children, changes, found, depth + 1)?;
            }
        }
        Some(())
    }
    let mut rendered = HashSet::new();
    shapes(out.children_mut()?, &changes, &mut rendered, 0)?;
    if !found.is_subset(&rendered) {
        return None;
    }
    kept.package.parts.get_mut(data_index)?.bytes = data.to_xml().into_bytes();
    kept.package.parts.get_mut(drawing_index)?.bytes = drawing.to_xml().into_bytes();
    out.diagram = Some(updated.clone());
    out.diagram.as_mut()?.kept = Some(kept);
    let hash = fingerprint(&out)?;
    out.diagram.as_mut()?.kept.as_mut()?.fingerprint = hash;
    Some(out)
}

pub(crate) fn fingerprint(s: &Shape) -> Option<String> {
    let d = s.diagram.as_ref()?;
    // The outer frame may move/rotate without changing the cached drawing's coordinate space.
    let value =
        serde_json::to_value((&s.kind, &d.kind, &d.nodes, &d.style, &d.colors, &d.layout_id, &d.layout_xml, &d.style_xml, &d.colors_xml)).ok()?;
    let bytes = serde_json::to_vec(&value).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

/// Import normalizes text defaults after reading the individual drawing parts.
/// Establish the comparison point only after those semantics-preserving adjustments.
pub(crate) fn seal(shapes: &mut [Shape], depth: usize) {
    if depth > 32 {
        return;
    }
    for s in shapes {
        if let Some(children) = s.children_mut() {
            seal(children, depth + 1);
        }
        if s.diagram.as_ref().is_some_and(|d| d.kept.is_some()) {
            let hash = fingerprint(s);
            if let Some(kept) = s.diagram.as_mut().and_then(|d| d.kept.as_mut()) {
                kept.fingerprint = hash.unwrap_or_default();
            }
        }
    }
}

fn allowed(name: &str) -> bool {
    let n = opc::norm(name);
    let parts: Vec<_> = n.split('/').collect();
    parts.len() >= 3
        && parts.first() == Some(&"ppt")
        && matches!(parts.get(1), Some(&"diagrams" | &"media"))
        && !parts.iter().any(|p| matches!(*p, "" | "." | ".."))
}

pub(crate) fn capture(imp: &mut Imp, part: &Part, ids: &El, drawing: &str, shape: &Shape) -> Option<DiagramKept> {
    let fingerprint = fingerprint(shape)?;
    let mut package = ChartKept::default();
    for role in ["dm", "lo", "qs", "cs"] {
        let rel = part.rels.get(ids.attr(&format!("r:{role}"))?)?;
        if rel.external {
            return None;
        }
        package.rels.push(ChartRel {
            id: role.into(),
            rel_type: rel.ty.clone(),
            target: rel.target_raw.clone(),
            part: rel.target.clone(),
            external: false,
        });
    }
    if !drawing.is_empty() {
        package.rels.push(ChartRel {
            id: "drawing".into(),
            rel_type: super::RT_DRAWING.into(),
            target: drawing.into(),
            part: drawing.into(),
            external: false,
        });
    }
    let mut pending: Vec<_> = package.rels.iter().map(|r| r.part.clone()).collect();
    let mut seen = HashSet::new();
    let mut bytes = 0usize;
    while let Some(name) = pending.pop() {
        let key = opc::norm(&name);
        if !seen.insert(key) {
            continue;
        }
        if !allowed(&name) || package.parts.len() >= MAX_PARTS {
            return None;
        }
        let data = imp.pkg.get(&name)?;
        bytes = bytes.checked_add(data.len())?;
        if bytes > MAX_BYTES || imp.kept_sidecar_bytes.saturating_add(bytes) > MAX_BYTES {
            return None;
        }
        package.parts.push(ChartPart {
            name: name.clone(),
            content_type: imp.pkg.content_type(&name).unwrap_or_default(),
            bytes: data.to_vec(),
            native_entry: None,
        });
        if name.ends_with(".rels") {
            continue;
        }
        let rels_name = opc::rels_path(&name);
        if imp.pkg.get(&rels_name).is_some() {
            pending.push(rels_name);
        }
        for rel in imp.pkg.rels(&name).items {
            if !rel.external {
                pending.push(rel.target);
            }
        }
    }
    imp.kept_sidecar_bytes = imp.kept_sidecar_bytes.saturating_add(bytes);
    Some(DiagramKept { fingerprint, package })
}

fn replace_drawing_id(el: &mut El, id: &str, depth: usize) {
    if depth > crate::xml::MAX_DEPTH {
        return;
    }
    if el.is("dataModelExt") {
        el.set_attr("relId", id);
    }
    for c in el.elements_mut() {
        replace_drawing_id(c, id, depth + 1);
    }
}

fn remove_drawing_reference(el: &mut El, depth: usize) {
    if depth > crate::xml::MAX_DEPTH {
        return;
    }
    el.children.retain(|n| !matches!(n,crate::xml::Node::El(e) if e.is("dataModelExt") || (e.is("ext") && e.child("dataModelExt").is_some())));
    for c in el.elements_mut() {
        remove_drawing_reference(c, depth + 1);
    }
}

#[cfg(test)]
#[test]
fn removing_missing_drawing_keeps_other_extensions_without_empty_wrappers() {
    let mut root = crate::xml::parse(
        br#"<dataModel><extLst><ext uri="drawing"><dataModelExt relId="absent"/></ext>
        <ext uri="other"><recolorImg val="1"/></ext></extLst></dataModel>"#,
    )
    .unwrap()
    .root;
    remove_drawing_reference(&mut root, 0);
    let remaining: Vec<_> = root.child("extLst").unwrap().children_named("ext").collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].attr("uri"), Some("other"));
    assert!(remaining[0].child("recolorImg").is_some());
}

/// Validate first, then publish under fresh names; no retained part can overwrite a slide.
pub(crate) fn write(x: &mut crate::write::Exp, out: &mut crate::write::Out, s: &Shape) -> Option<HashMap<String, String>> {
    let kept = s.diagram.as_ref()?.kept.as_ref()?;
    if fingerprint(s)? != kept.fingerprint || kept.package.parts.len() > MAX_PARTS {
        return None;
    }
    let mut originals = HashMap::new();
    let mut bytes = 0usize;
    for p in &kept.package.parts {
        bytes = bytes.checked_add(p.bytes.len())?;
        if !allowed(&p.name) || p.bytes.is_empty() || bytes > MAX_BYTES || originals.insert(opc::norm(&p.name), p).is_some() {
            return None;
        }
    }
    let mut roots = HashMap::new();
    let roles: Vec<_> = ["dm", "lo", "qs", "cs", "drawing"]
        .into_iter()
        .filter(|role| *role != "drawing" || kept.package.rels.iter().any(|r| r.id == "drawing"))
        .collect();
    for &role in &roles {
        let rel = kept.package.rels.iter().find(|r| r.id == role && !r.external)?;
        let root = originals.get(&opc::norm(&rel.part))?;
        if root.name.ends_with(".rels") {
            return None;
        }
        roots.insert(role, rel);
    }
    let data_part = originals.get(&opc::norm(&roots.get("dm")?.part))?;
    let mut data = crate::xml::parse(&data_part.bytes).ok()?.root;
    let mut edges = HashMap::new();
    for p in kept.package.parts.iter().filter(|p| !p.name.ends_with(".rels")) {
        let rels = originals.get(&opc::norm(&opc::rels_path(&p.name))).map(|r| opc::rels_from_bytes(&r.bytes, &p.name)).unwrap_or_default();
        for r in &rels.items {
            if !r.external && originals.get(&opc::norm(&r.target))?.name.ends_with(".rels") {
                return None;
            }
        }
        edges.insert(opc::norm(&p.name), rels);
    }
    let n = x.next_diagram();
    let mut names = HashMap::new();
    for (i, p) in kept.package.parts.iter().filter(|p| !p.name.ends_with(".rels")).enumerate() {
        let ext = p.name.rsplit('.').next().filter(|e| e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("bin");
        names.insert(opc::norm(&p.name), format!("ppt/diagrams/preserved{n}_{i}.{ext}"));
    }
    let mut ids = HashMap::new();
    for &role in &roles {
        let rel = roots.get(role)?;
        let target = names.get(&opc::norm(&rel.part))?;
        ids.insert(role.to_string(), out.rels.add_new(&rel.rel_type, &opc::relative(&out.name, target), false));
    }
    if let Some(id) = ids.get("drawing") {
        replace_drawing_id(&mut data, id, 0);
    } else {
        remove_drawing_reference(&mut data, 0);
    }
    for p in kept.package.parts.iter().filter(|p| !p.name.ends_with(".rels")) {
        let key = opc::norm(&p.name);
        let name = names.get(&key)?;
        let bytes = if key == opc::norm(&data_part.name) { data.to_xml().into_bytes() } else { p.bytes.clone() };
        x.pkg.xml_part(name, &p.content_type, bytes);
        let mut rels = opc::RelsOut::default();
        for r in &edges.get(&key)?.items {
            let target = if r.external { r.target_raw.clone() } else { opc::relative(name, names.get(&opc::norm(&r.target))?) };
            rels.items.push((r.id.clone(), r.ty.clone(), target, r.external));
        }
        x.pkg.rels(name, &rels);
    }
    Some(ids)
}
