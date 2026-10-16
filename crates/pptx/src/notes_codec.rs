//! Bounded transparent XML values for the closed, typed notes interchange records.
//! This is not an original-package or arbitrary XML preservation channel.
use std::collections::HashMap;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    PptxError,
    xml::{A, El, Node, W},
};

pub(crate) const NS: &str = "urn:deckcraft:pptx:notes:1";
pub(crate) const URI: &str = "urn:deckcraft:pptx:notes:1";
pub(crate) const MAX_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_NODES: usize = 200_000;
pub(crate) const MAX_DEPTH: usize = 512;

pub(crate) fn invalid(message: impl std::fmt::Display) -> PptxError {
    PptxError::Unsupported(format!("notes interchange: {message}"))
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    use std::fmt::Write as _;
    for byte in hash {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Hash actual core XML, including owner-local shape/relationship ids and all text.
/// Ignore only the one correctly namespaced DeckCraft extension, never other data.
pub(crate) fn projection(root: &El) -> Result<String, PptxError> {
    let mut root = root.clone();
    strip(&mut root, &HashMap::new(), 0)?;
    Ok(digest(root.to_xml().as_bytes()))
}

fn namespace_map(el: &El, parent: &HashMap<String, String>) -> Result<HashMap<String, String>, PptxError> {
    let mut ns = parent.clone();
    let mut seen = std::collections::HashSet::new();
    for (key, value) in &el.attrs {
        if !seen.insert(key) {
            return Err(invalid("duplicate XML attribute"));
        }
        if key == "xmlns" {
            ns.insert(String::new(), value.clone());
        }
        if let Some(prefix) = key.strip_prefix("xmlns:") {
            ns.insert(prefix.to_string(), value.clone());
        }
    }
    Ok(ns)
}

fn is(el: &El, name: &str, ns: &HashMap<String, String>, uri: &str) -> bool {
    el.local() == name && ns.get(el.prefix()).is_some_and(|bound| bound == uri)
}

fn strip(el: &mut El, parent: &HashMap<String, String>, depth: usize) -> Result<(), PptxError> {
    if depth > MAX_DEPTH {
        return Err(invalid("projection nesting limit"));
    }
    let ns = namespace_map(el, parent)?;
    let mut kept = Vec::with_capacity(el.children.len());
    for mut child in std::mem::take(&mut el.children) {
        if let Node::El(e) = &mut child {
            let child_ns = namespace_map(e, &ns)?;
            let own = is(e, "ext", &child_ns, crate::opc::NS_P) && exact_attr(e, "uri") == Some(URI);
            if own {
                continue;
            }
            strip(e, &ns, depth + 1)?;
            // An extension-list created only to hold our record is absent in core.
            if is(e, "extLst", &child_ns, crate::opc::NS_P) && e.children.is_empty() && e.attrs.is_empty() {
                continue;
            }
        }
        kept.push(child);
    }
    el.children = kept;
    Ok(())
}

pub(crate) fn exact_attr<'a>(el: &'a El, name: &str) -> Option<&'a str> {
    el.attrs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}

pub(crate) fn write_record<T: Serialize + DeserializeOwned + PartialEq>(w: &mut W, kind: &str, record: &T) -> Result<(), PptxError> {
    crate::notes_budget::Counter::new(MAX_BYTES).value(record)?;
    let mut value = serde_json::to_value(record).map_err(invalid)?;
    let decoded: T = serde_json::from_value(value.clone()).map_err(invalid)?;
    if decoded != *record {
        return Err(invalid("notes metadata would repair a nonfinite or invalid native value"));
    }
    omit_null_members(&mut value, 0)?;
    let mut budget = Budget::default();
    validate_value(&value, 0, &mut budget)?;
    w.open0("p:extLst");
    w.open("p:ext", A::new().a("uri", URI));
    w.open("dc:notes", A::new().a("xmlns:dc", NS).a("profile", "1").a("record", kind));
    write_value(w, &value);
    w.close("dc:notes");
    w.close("p:ext");
    w.close("p:extLst");
    Ok(())
}

/// Absent optional properties have one canonical spelling. Pruning only null
/// object members keeps rich default styles compact without hiding unknown input:
/// decoder equality compares the complete supplied wire object to this canonical
/// typed spelling, so even an unknown explicit null remains an error.
fn omit_null_members(value: &mut Value, depth: usize) -> Result<(), PptxError> {
    if depth > MAX_DEPTH {
        return Err(invalid("metadata nesting limit"));
    }
    match value {
        Value::Object(members) => {
            members.retain(|_, value| !value.is_null());
            for value in members.values_mut() {
                omit_null_members(value, depth + 1)?;
            }
        }
        Value::Array(items) => {
            for value in items {
                omit_null_members(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn write_value(w: &mut W, value: &Value) {
    // Breadth-first contiguous ids make every child a strictly forward reference.
    // validate_value has already bounded all values and allocation work.
    let mut nodes = vec![value];
    let mut index = 0usize;
    w.open("dc:graph", A::new().a("root", "0"));
    while let Some(value) = nodes.get(index).copied() {
        let kind = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        w.open("dc:node", A::new().a("id", index).a("kind", kind));
        match value {
            Value::Null => {}
            Value::Bool(value) => w.text(if *value { "true" } else { "false" }),
            Value::Number(value) => w.text(&value.to_string()),
            Value::String(value) => w.text(value),
            Value::Array(items) => {
                for item in items {
                    let child = nodes.len();
                    nodes.push(item);
                    w.empty("dc:item", A::new().a("ref", child));
                }
            }
            Value::Object(items) => {
                for (name, value) in items {
                    let child = nodes.len();
                    nodes.push(value);
                    w.empty("dc:member", A::new().a("name", name).a("ref", child));
                }
            }
        }
        w.close("dc:node");
        index += 1;
    }
    w.close("dc:graph");
}

#[derive(Default)]
struct Budget {
    bytes: usize,
    nodes: usize,
    edges: usize,
}
impl Budget {
    /// A tree of at most MAX_NODES values has at most MAX_NODES-1 edges.
    /// Charge before allocating/cloning an edge, including anonymous array items.
    fn edge(&mut self) -> Result<(), PptxError> {
        self.edges = self.edges.checked_add(1).filter(|count| *count < MAX_NODES).ok_or_else(|| invalid("metadata graph edge limit"))?;
        self.bytes =
            self.bytes.checked_add(32).filter(|bytes| *bytes <= MAX_BYTES).ok_or_else(|| invalid("metadata graph edge byte budget exceeded"))?;
        Ok(())
    }
    fn add(&mut self, bytes: usize) -> Result<(), PptxError> {
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(|| invalid("metadata size overflow"))?;
        self.nodes = self.nodes.checked_add(1).ok_or_else(|| invalid("metadata node overflow"))?;
        if self.bytes > MAX_BYTES || self.nodes > MAX_NODES {
            return Err(invalid("metadata budget exceeded"));
        }
        Ok(())
    }
}

fn validate_value(value: &Value, depth: usize, budget: &mut Budget) -> Result<(), PptxError> {
    if depth > MAX_DEPTH {
        return Err(invalid("metadata nesting limit"));
    }
    budget.add(64)?;
    match value {
        Value::String(value) => {
            budget.add(value.len())?;
            if !value.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
            {
                return Err(invalid("notes metadata contains unsupported XML control characters"));
            }
        }
        Value::Number(value) if value.as_f64().is_some_and(|n| !n.is_finite()) => return Err(invalid("nonfinite metadata number")),
        Value::Array(items) => {
            for item in items {
                budget.edge()?;
                validate_value(item, depth + 1, budget)?;
            }
        }
        Value::Object(items) => {
            for (name, value) in items {
                budget.edge()?;
                budget.add(name.len())?;
                // Raw model fragments do not belong in this typed preservation channel.
                if matches!(name.as_str(), "rawExt" | "rawExtra" | "raw3d" | "layoutXml" | "styleXml" | "colorXml" | "omml" | "xml")
                    && !value.is_null()
                    && value.as_str() != Some("")
                    && !(name == "rawExtra" && value.as_str().is_some_and(empty_theme_extra))
                {
                    return Err(invalid("unclassified XML in notes metadata"));
                }
                validate_value(value, depth + 1, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The normal PPTX theme writer emits these two empty standard containers. Their
/// import retention is classified here without admitting arbitrary raw theme XML.
fn empty_theme_extra(raw: &str) -> bool {
    if raw.len() > 8192 {
        return false;
    }
    let xml = format!("<root xmlns:a=\"{}\">{raw}</root>", crate::opc::NS_A);
    let Ok(doc) = crate::xml::parse(xml.as_bytes()) else { return false };
    let mut seen = std::collections::HashSet::new();
    let mut count = 0usize;
    for child in doc.root.elements() {
        let Ok(ns) = namespace_map(child, &HashMap::from([("a".into(), crate::opc::NS_A.into())])) else { return false };
        if !matches!(child.local(), "objectDefaults" | "extraClrSchemeLst")
            || !seen.insert(child.local())
            || ns.get(child.prefix()).map(String::as_str) != Some(crate::opc::NS_A)
            || child.elements().next().is_some()
            || !child.text().trim().is_empty()
            || check_attrs(child, &[]).is_err()
        {
            return false;
        }
        count += 1;
    }
    count > 0 && count <= 2
}

/// Read one closed typed record. Re-serialization equality rejects ignored fields in
/// legacy model decoders, repaired null numerics and partially understood variants.
pub(crate) fn read_record<T: DeserializeOwned + Serialize>(root: &El, kind: &str) -> Result<Option<T>, PptxError> {
    let ns = namespace_map(root, &HashMap::new())?;
    let mut found = None;
    for list in root.elements() {
        let list_ns = namespace_map(list, &ns)?;
        if !is(list, "extLst", &list_ns, crate::opc::NS_P) {
            continue;
        }
        for ext in list.elements() {
            let ext_ns = namespace_map(ext, &list_ns)?;
            if !is(ext, "ext", &ext_ns, crate::opc::NS_P) || exact_attr(ext, "uri") != Some(URI) {
                continue;
            }
            check_attrs(ext, &["uri"])?;
            if found.is_some() {
                return Err(invalid("duplicate notes extension"));
            }
            let payload = one_child(ext)?;
            let payload_ns = namespace_map(payload, &ext_ns)?;
            check_attrs(payload, &["profile", "record"])?;
            if !is(payload, "notes", &payload_ns, NS) || exact_attr(payload, "profile") != Some("1") || exact_attr(payload, "record") != Some(kind) {
                return Err(invalid("unknown notes extension profile or record"));
            }
            let mut budget = Budget::default();
            let value = read_value(one_child(payload)?, &payload_ns, 0, &mut budget)?;
            validate_value(&value, 0, &mut Budget::default())?;
            let decoded: T = serde_json::from_value(value.clone()).map_err(invalid)?;
            let mut canonical = serde_json::to_value(&decoded).map_err(invalid)?;
            omit_null_members(&mut canonical, 0)?;
            if canonical != value {
                return Err(invalid("unknown or repaired notes metadata fields"));
            }
            found = Some(decoded);
        }
    }
    Ok(found)
}

fn one_child(el: &El) -> Result<&El, PptxError> {
    let mut children = el.elements();
    let child = children.next().ok_or_else(|| invalid("missing metadata value"))?;
    if children.next().is_some() || el.children.iter().any(|node| matches!(node, Node::Text(text) if !text.trim().is_empty())) {
        return Err(invalid("multiple metadata values"));
    }
    Ok(child)
}

fn read_value(el: &El, parent: &HashMap<String, String>, _depth: usize, budget: &mut Budget) -> Result<Value, PptxError> {
    struct Flat {
        value: Value,
        edges: Vec<(Option<String>, usize)>,
    }
    let ns = namespace_map(el, parent)?;
    if !is(el, "graph", &ns, NS) {
        return Err(invalid("missing flat metadata graph"));
    }
    check_attrs(el, &["root"])?;
    if exact_attr(el, "root") != Some("0") {
        return Err(invalid("metadata root identity is not canonical"));
    }
    whitespace_only(el)?;
    let mut nodes = Vec::new();
    for node in el.elements() {
        if nodes.len() >= MAX_NODES {
            return Err(invalid("metadata graph node limit"));
        }
        let node_ns = namespace_map(node, &ns)?;
        if !is(node, "node", &node_ns, NS) {
            return Err(invalid("unknown graph node element"));
        }
        check_attrs(node, &["id", "kind"])?;
        if exact_attr(node, "id") != Some(nodes.len().to_string().as_str()) {
            return Err(invalid("metadata graph identities are not contiguous"));
        }
        budget.add(64)?;
        let kind = exact_attr(node, "kind").ok_or_else(|| invalid("metadata graph node kind is missing"))?;
        let scalar = || -> Result<String, PptxError> {
            if node.elements().next().is_some() {
                return Err(invalid("element inside scalar metadata node"));
            }
            Ok(node.text())
        };
        let mut edges = Vec::new();
        let value = match kind {
            "null" => {
                if !node.children.is_empty() {
                    return Err(invalid("metadata null node has content"));
                }
                Value::Null
            }
            "boolean" => match scalar()?.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => return Err(invalid("invalid metadata Boolean")),
            },
            "number" => {
                let text = scalar()?;
                budget.add(text.len())?;
                if text.len() > 128 {
                    return Err(invalid("metadata number lexical limit"));
                }
                let value: Value = serde_json::from_str(&text).map_err(invalid)?;
                if !value.is_number() {
                    return Err(invalid("invalid metadata number"));
                }
                value
            }
            "string" => {
                let text = scalar()?;
                budget.add(text.len())?;
                Value::String(text)
            }
            "array" | "object" => {
                whitespace_only(node)?;
                let mut names = std::collections::HashSet::new();
                for edge in node.elements() {
                    budget.edge()?;
                    let edge_ns = namespace_map(edge, &node_ns)?;
                    let member = kind == "object";
                    if !is(edge, if member { "member" } else { "item" }, &edge_ns, NS) {
                        return Err(invalid("invalid metadata graph edge"));
                    }
                    check_attrs(edge, if member { &["name", "ref"] } else { &["ref"] })?;
                    if !edge.children.is_empty() {
                        return Err(invalid("metadata graph edge contains a value"));
                    }
                    let name = if member {
                        let name = exact_attr(edge, "name").ok_or_else(|| invalid("metadata member name is missing"))?;
                        budget.add(name.len())?;
                        if !names.insert(name) {
                            return Err(invalid("duplicate metadata member"));
                        }
                        Some(name.to_string())
                    } else {
                        None
                    };
                    let target = exact_attr(edge, "ref").ok_or_else(|| invalid("metadata graph edge target is missing"))?;
                    let reference = target.parse::<usize>().map_err(invalid)?;
                    if target != reference.to_string() || reference <= nodes.len() {
                        return Err(invalid("metadata graph has a cycle or noncanonical reference"));
                    }
                    edges.push((name, reference));
                }
                if kind == "object" { Value::Object(Default::default()) } else { Value::Array(Vec::new()) }
            }
            _ => return Err(invalid("unknown metadata graph node kind")),
        };
        nodes.push(Flat { value, edges });
    }
    if nodes.is_empty() {
        return Err(invalid("empty metadata graph"));
    }
    let mut owners = vec![0usize; nodes.len()];
    let mut depths = vec![0usize; nodes.len()];
    for (parent, node) in nodes.iter().enumerate() {
        let depth = depths.get(parent).copied().ok_or_else(|| invalid("metadata graph depth state is missing"))?;
        for (_, child) in &node.edges {
            let owner = owners.get_mut(*child).ok_or_else(|| invalid("metadata graph reference is outside its ledger"))?;
            *owner = owner.checked_add(1).filter(|count| *count == 1).ok_or_else(|| invalid("metadata graph child has multiple owners"))?;
            let next = depth.checked_add(1).filter(|depth| *depth <= MAX_DEPTH).ok_or_else(|| invalid("decoded metadata depth limit"))?;
            let child_depth = depths.get_mut(*child).ok_or_else(|| invalid("metadata graph depth state is missing"))?;
            *child_depth = next;
        }
    }
    if owners.first() != Some(&0) || owners.iter().skip(1).any(|owners| *owners != 1) {
        return Err(invalid("metadata graph has detached or unowned values"));
    }
    let mut values: Vec<Option<Value>> = (0..nodes.len()).map(|_| None).collect();
    for (index, mut node) in nodes.into_iter().enumerate().rev() {
        for (name, child) in node.edges {
            let value = values.get_mut(child).and_then(Option::take).ok_or_else(|| invalid("metadata graph child was already consumed"))?;
            match (&mut node.value, name) {
                (Value::Object(members), Some(name)) => {
                    members.insert(name, value);
                }
                (Value::Array(items), None) => items.push(value),
                _ => return Err(invalid("metadata graph edge kind mismatch")),
            }
        }
        let slot = values.get_mut(index).ok_or_else(|| invalid("metadata graph output state is missing"))?;
        *slot = Some(node.value);
    }
    values.first_mut().and_then(Option::take).ok_or_else(|| invalid("metadata graph root is missing"))
}

fn whitespace_only(el: &El) -> Result<(), PptxError> {
    if el.children.iter().any(|node| matches!(node, Node::Text(text) if !text.trim().is_empty())) {
        return Err(invalid("unexpected metadata text"));
    }
    Ok(())
}

fn check_attrs(el: &El, allowed: &[&str]) -> Result<(), PptxError> {
    if el.attrs.iter().any(|(key, _)| key != "xmlns" && !key.starts_with("xmlns:") && !allowed.contains(&key.as_str())) {
        return Err(invalid("unknown metadata attribute"));
    }
    Ok(())
}

#[cfg(test)]
mod independent_edge_budget_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    #[test]
    fn graph_edges_are_bounded_before_collection_even_without_member_names() {
        let mut budget = Budget::default();
        for _ in 0..MAX_NODES - 1 {
            budget.edge().unwrap();
        }
        assert_eq!(budget.edges, MAX_NODES - 1);
        assert!(budget.edge().unwrap_err().to_string().contains("edge limit"));
    }
    #[test]
    fn edge_bytes_share_the_existing_metadata_byte_budget() {
        let mut budget = Budget { bytes: MAX_BYTES - 31, ..Default::default() };
        assert!(budget.edge().unwrap_err().to_string().contains("edge byte budget"));
    }
}

#[cfg(test)]
mod exact_whitespace_roundtrip_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn flat_record_preserves_crlf_lone_cr_lf_tabs_unicode_and_member_names() {
        let text = " \tfirst\r\nsecond\rlast\nÉ漢🦀<&> ";
        let record = serde_json::json!({
            "lexical": text,
            "nested": [{"value":text}, [text, true, null]],
            "member\r\n\tÉ": "\r\r\n\n\t",
        });
        let mut writer = W::new();
        writer.open("p:sp", A::new().a("xmlns:p", crate::opc::NS_P));
        write_record(&mut writer, "whitespace-unit", &record).unwrap();
        writer.close("p:sp");
        assert!(writer.s.contains("&#13;"));
        assert!(!writer.s.contains('\r'), "literal CR would be normalized by XML 1.0");
        let doc = crate::xml::parse(writer.s.as_bytes()).unwrap();
        let actual: Option<serde_json::Value> = read_record(&doc.root, "whitespace-unit").unwrap();
        assert_eq!(actual, Some(record.clone()));
        let serialized = doc.root.to_xml();
        let doc = crate::xml::parse(serialized.as_bytes()).unwrap();
        let repeated: Option<serde_json::Value> = read_record(&doc.root, "whitespace-unit").unwrap();
        assert_eq!(repeated, Some(record));
    }
}
