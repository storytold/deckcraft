use super::paint::layout_items;
use super::{Diagram, DiagramNode, MAX_LEVEL, MAX_SHAPES, Shape, Xfrm};

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
pub(super) type Item = (Shape, Vec<usize>);

/// Top-level nodes, each with its sub-points.
pub(super) fn groups(d: &Diagram) -> Vec<Vec<usize>> {
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
pub(super) fn sync_text(d: &mut Diagram, children: &[Shape]) {
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
