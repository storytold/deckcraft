//! Patch data-only edits in an imported chart, retaining unmodelled formatting.
//! Structural/type/style edits still use the normal chart writer.

use crate::xml::{A, El, Node, W};
use deckcraft_model::chart::Chart;

pub(super) fn patched(c: &Chart) -> Option<Chart> {
    let mut root = crate::xml::parse(c.raw.as_ref()?.as_bytes()).ok()?.root;
    // Do not silently modify only one arm of AlternateContent.
    if root.find("AlternateContent").is_some() || root.find("multiLvlStrRef").is_some() {
        return None;
    }
    let mut back = crate::read::chart_from_el(&root)?;
    let mut current = c.clone();
    current.raw = None;
    current.kept = Default::default();
    if back == current || back.series.len() != c.series.len() {
        return None;
    }
    back.categories = c.categories.clone();
    for (old, new) in back.series.iter_mut().zip(&c.series) {
        old.name = new.name.clone();
        old.values = new.values.clone();
    }
    if back != current {
        return None;
    }
    let chart = root.elements_mut().find(|e| e.is("chart"))?;
    let plot = chart.elements_mut().find(|e| e.is("plotArea"))?;
    let mut index = 0;
    for group in plot.elements_mut().filter(|g| g.local().ends_with("Chart")) {
        for ser in group.elements_mut().filter(|s| s.is("ser")) {
            let s = c.series.get(index)?;
            // Categorical series only: x/y/bubble data need their own coupled update.
            if ser.child("val").is_none() || ser.child("cat").is_none() {
                return None;
            }
            let mut w = W::frag();
            w.open0("c:tx");
            w.elt("c:v", &s.name);
            w.close("c:tx");
            replace(ser, "tx", crate::xml::parse(w.finish().as_slice()).ok()?.root)?;
            let mut w = W::frag();
            w.open0("c:cat");
            w.open0("c:strLit");
            w.val("c:ptCount", c.categories.len());
            for (i, text) in c.categories.iter().enumerate() {
                w.open("c:pt", A::new().a("idx", i));
                w.elt("c:v", text);
                w.close("c:pt");
            }
            w.close("c:strLit");
            w.close("c:cat");
            replace(ser, "cat", crate::xml::parse(w.finish().as_slice()).ok()?.root)?;
            let format = ser.child("val").and_then(|v| v.find("formatCode")).map(El::text).unwrap_or_else(|| "General".into());
            let mut w = W::frag();
            w.open0("c:val");
            w.open0("c:numLit");
            w.elt("c:formatCode", &format);
            w.val("c:ptCount", s.values.len());
            for (i, value) in s.values.iter().enumerate() {
                if let Some(value) = value.filter(|v| v.is_finite()) {
                    w.open("c:pt", A::new().a("idx", i));
                    w.elt("c:v", &value.to_string());
                    w.close("c:pt");
                }
            }
            w.close("c:numLit");
            w.close("c:val");
            replace(ser, "val", crate::xml::parse(w.finish().as_slice()).ok()?.root)?;
            index += 1;
        }
    }
    if index != c.series.len() {
        return None;
    }
    let removed: Vec<String> = root
        .children
        .iter()
        .filter_map(|n| match n {
            Node::El(e) if e.is("externalData") => e.attr("r:id").map(str::to_string),
            _ => None,
        })
        .collect();
    root.children.retain(|n| !matches!(n, Node::El(e) if e.is("externalData")));
    // Verify that the patched part actually expresses the requested model state.
    if crate::read::chart_from_el(&root)? != current {
        return None;
    }
    let mut out = c.clone();
    out.raw = Some(root.to_xml());
    out.kept.rels.retain(|r| !removed.contains(&r.id));
    prune_parts(&mut out);
    Some(out)
}

fn replace(parent: &mut El, name: &str, replacement: El) -> Option<()> {
    let node = parent.children.iter_mut().find(|n| matches!(n, Node::El(e) if e.is(name)))?;
    *node = Node::El(replacement);
    Some(())
}

/// Keep styles, user drawings and their dependencies, but not a now-stale workbook.
fn prune_parts(c: &mut Chart) {
    use crate::opc::{norm, rels_from_bytes, rels_path};
    use std::collections::{HashMap, HashSet};
    let parts: HashMap<_, _> = c.kept.parts.iter().map(|p| (norm(&p.name), p)).collect();
    let mut pending: Vec<_> = c.kept.rels.iter().filter(|r| !r.external).map(|r| norm(&r.part)).collect();
    let mut seen = HashSet::new();
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let rels = norm(&rels_path(&name));
        if let Some(p) = parts.get(&rels) {
            seen.insert(rels);
            pending.extend(rels_from_bytes(&p.bytes, &name).items.into_iter().filter(|r| !r.external).map(|r| norm(&r.target)));
        }
    }
    c.kept.parts.retain(|p| seen.contains(&norm(&p.name)));
}
