//! Decode recursive drawing/table edges one owner at a time.
//!
//! Internally tagged serde enums buffer their complete subtree. Recursively
//! decoding a group of groups therefore keeps many copies and large serde stack
//! frames alive at once. Capture a bounded, duplicate-free JSON tree, detach the
//! recursive edges, and assemble the typed owners in reverse order instead.
//! The ordinary derives still validate every field of each shallow owner.
use std::fmt;

use serde::{
    Deserializer,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};

use crate::{
    Shape, ShapeKind, Table,
    table::{CellBlock, MAX_NESTED_TABLE_DEPTH},
};

const MAX_DEPTH: usize = 512;
const MAX_NODES: usize = 2_000_000;
const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_STRING: usize = 1024 * 1024;
const MAX_SHAPES: usize = 100_000;
const MAX_TABLES: usize = 1_000_000;
const MAX_GROUP_DEPTH: usize = 64;

#[derive(Default)]
struct Budget {
    nodes: usize,
    bytes: usize,
}
impl Budget {
    fn charge<E: de::Error>(&mut self, bytes: usize) -> Result<(), E> {
        self.bytes =
            self.bytes.checked_add(bytes).filter(|v| *v <= MAX_BYTES).ok_or_else(|| E::custom("recursive model JSON exceeds allocation budget"))?;
        Ok(())
    }
}
struct Capture<'a> {
    budget: &'a mut Budget,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Capture<'_> {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        if self.depth > MAX_DEPTH {
            return Err(de::Error::custom("recursive model JSON exceeds depth limit"));
        }
        self.budget.nodes = self
            .budget
            .nodes
            .checked_add(1)
            .filter(|v| *v <= MAX_NODES)
            .ok_or_else(|| de::Error::custom("recursive model JSON exceeds work limit"))?;
        self.budget.charge::<D::Error>(128)?;
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Capture<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded model JSON")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(value).map(Value::Number).ok_or_else(|| E::custom("nonfinite model JSON number"))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        if value.len() > MAX_STRING {
            return Err(E::custom("recursive model JSON string exceeds size limit"));
        }
        self.budget.charge::<E>(value.len().saturating_mul(2))?;
        Ok(Value::String(value.into()))
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        if value.len() > MAX_STRING {
            return Err(E::custom("recursive model JSON string exceeds size limit"));
        }
        self.budget.charge::<E>(value.len().saturating_mul(2))?;
        Ok(Value::String(value))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(Capture { budget: self.budget, depth: self.depth + 1 })? {
            values.try_reserve(1).map_err(|_| de::Error::custom("cannot allocate model JSON sequence"))?;
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = map.next_key_seed(Key { budget: self.budget })? {
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate model JSON field"));
            }
            let value = map.next_value_seed(Capture { budget: self.budget, depth: self.depth + 1 })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}
struct Key<'a> {
    budget: &'a mut Budget,
}
impl<'de> DeserializeSeed<'de> for Key<'_> {
    type Value = String;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        struct KeyVisitor<'a>(&'a mut Budget);
        impl Visitor<'_> for KeyVisitor<'_> {
            type Value = String;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded model JSON key")
            }
            fn visit_str<E: de::Error>(self, key: &str) -> Result<String, E> {
                if key.len() > 1024 {
                    return Err(E::custom("model JSON key exceeds size limit"));
                }
                self.0.charge::<E>(128usize.saturating_add(key.len().saturating_mul(2)))?;
                Ok(key.into())
            }
        }
        d.deserialize_identifier(KeyVisitor(self.budget))
    }
}

pub(crate) fn captured<'de, D: Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
    Capture { budget: &mut Budget::default(), depth: 0 }.deserialize(d)
}

fn invalid(message: &str) -> String {
    message.into()
}

// Serde structs also accept their historical sequence representation. Inspect
// both forms without rewriting malformed scalars into valid empty owners.
pub(crate) fn field_mut<'a>(value: &'a mut Value, name: &str, index: usize) -> Option<&'a mut Value> {
    match value {
        Value::Object(fields) => fields.get_mut(name),
        Value::Array(fields) => fields.get_mut(index),
        _ => None,
    }
}

// A malformed/non-array children field remains untouched for the typed derive
// to reject. Only a real group edge is detached; unknown fields are never used
// as authority or converted into trusted owners.
fn take_children(kind: &mut Value) -> Vec<Value> {
    if kind.get("type").and_then(Value::as_str) != Some("group") {
        return Vec::new();
    }
    let Some(Value::Array(children)) = kind.get_mut("children") else {
        return Vec::new();
    };
    std::mem::take(children)
}

pub(crate) fn shape_kind<'de, D: Deserializer<'de>>(d: D) -> Result<ShapeKind, D::Error> {
    decode_kind(captured(d)?).map_err(de::Error::custom)
}

fn decode_kind(mut value: Value) -> Result<ShapeKind, String> {
    struct Node {
        shape: Shape,
        children: Vec<usize>,
    }
    let children = take_children(&mut value);
    let mut root: ShapeKind = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if children.is_empty() {
        return Ok(root);
    }
    let mut pending = Vec::new();
    let mut roots = Vec::new();
    for value in children {
        roots.push(pending.len());
        pending.push((value, 1usize));
    }
    let mut nodes = Vec::new();
    let mut at = 0usize;
    while at < pending.len() {
        if pending.len() > MAX_SHAPES {
            return Err(invalid("model shape count exceeds limit"));
        }
        let (mut value, depth) =
            pending.get_mut(at).map(|(value, depth)| (value.take(), *depth)).ok_or_else(|| invalid("invalid model shape traversal"))?;
        if depth > MAX_GROUP_DEPTH {
            return Err(invalid("model group nesting exceeds limit"));
        }
        let children = field_mut(&mut value, "kind", 3).map(take_children).unwrap_or_default();
        let shape = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let mut indices = Vec::new();
        for child in children {
            if pending.len() >= MAX_SHAPES {
                return Err(invalid("model shape count exceeds limit"));
            }
            indices.push(pending.len());
            pending.push((child, depth + 1));
        }
        nodes.push(Some(Node { shape, children: indices }));
        at += 1;
    }
    let mut ready: Vec<Option<Shape>> = (0..nodes.len()).map(|_| None).collect();
    for index in (0..nodes.len()).rev() {
        let mut node = nodes.get_mut(index).and_then(Option::take).ok_or_else(|| invalid("missing model shape owner"))?;
        if let ShapeKind::Group { children, .. } = &mut node.shape.kind {
            for child in node.children {
                children.push(ready.get_mut(child).and_then(Option::take).ok_or_else(|| invalid("missing model group child"))?);
            }
        } else if !node.children.is_empty() {
            return Err(invalid("invalid model group owner"));
        }
        *ready.get_mut(index).ok_or_else(|| invalid("missing model shape slot"))? = Some(node.shape);
    }
    if let ShapeKind::Group { children, .. } = &mut root {
        for child in roots {
            children.push(ready.get_mut(child).and_then(Option::take).ok_or_else(|| invalid("missing model root child"))?);
        }
    } else {
        return Err(invalid("invalid model root group"));
    }
    Ok(root)
}

pub(crate) fn cell_blocks<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<CellBlock>, D::Error> {
    decode_blocks(captured(d)?).map_err(de::Error::custom)
}

// Each placeholder is a valid empty Table, so all remaining strict CellBlock
// and NestedTable fields still pass through their ordinary serde derives.
fn detach_blocks(value: &mut Value, pending: &mut Vec<(Value, usize)>, depth: usize) -> Result<Vec<(usize, usize)>, String> {
    let mut edges = Vec::new();
    let Some(blocks) = value.as_array_mut() else {
        return Ok(edges);
    };
    for (block, value) in blocks.iter_mut().enumerate() {
        let tag = match value {
            Value::Object(fields) => fields.get("kind"),
            Value::Array(fields) => fields.first(),
            _ => None,
        };
        if tag.and_then(Value::as_str) != Some("table") {
            continue;
        }
        let Some(table) = field_mut(value, "content", 1).and_then(|content| field_mut(content, "table", 1)) else {
            continue;
        };
        if depth >= MAX_NESTED_TABLE_DEPTH {
            return Err(invalid("nested-table depth exceeds limit"));
        }
        if pending.len() >= MAX_TABLES {
            return Err(invalid("nested-table owner count exceeds limit"));
        }
        let child = pending.len();
        let raw = std::mem::replace(table, Value::Object(Map::new()));
        pending.push((raw, depth + 1));
        edges.push((block, child));
    }
    Ok(edges)
}

fn restore_blocks(blocks: &mut [CellBlock], edges: Vec<(usize, usize)>, ready: &mut [Option<Table>]) -> Result<(), String> {
    for (block, child) in edges {
        let table = ready.get_mut(child).and_then(Option::take).ok_or_else(|| invalid("missing nested-table child"))?;
        let Some(CellBlock::Table(owner)) = blocks.get_mut(block) else {
            return Err(invalid("invalid nested-table owner"));
        };
        *owner.table = table;
    }
    Ok(())
}

fn decode_blocks(mut value: Value) -> Result<Vec<CellBlock>, String> {
    struct Node {
        table: Table,
        edges: Vec<(usize, usize, Vec<(usize, usize)>)>,
    }
    let mut pending = Vec::new();
    let roots = detach_blocks(&mut value, &mut pending, 0)?;
    let mut blocks: Vec<CellBlock> = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let mut nodes = Vec::new();
    let mut at = 0usize;
    while at < pending.len() {
        let (mut value, depth) =
            pending.get_mut(at).map(|(value, depth)| (value.take(), *depth)).ok_or_else(|| invalid("invalid nested-table traversal"))?;
        let mut edges = Vec::new();
        if let Some(rows) = field_mut(&mut value, "rows", 1).and_then(Value::as_array_mut) {
            for (r, row) in rows.iter_mut().enumerate() {
                if let Some(cells) = field_mut(row, "cells", 1).and_then(Value::as_array_mut) {
                    for (c, cell) in cells.iter_mut().enumerate() {
                        if let Some(blocks) = field_mut(cell, "blocks", 14) {
                            let children = detach_blocks(blocks, &mut pending, depth)?;
                            if !children.is_empty() {
                                edges.push((r, c, children));
                            }
                        }
                    }
                }
            }
        }
        let table = serde_json::from_value(value).map_err(|e| e.to_string())?;
        nodes.push(Some(Node { table, edges }));
        at += 1;
    }
    let mut ready: Vec<Option<Table>> = (0..nodes.len()).map(|_| None).collect();
    for index in (0..nodes.len()).rev() {
        let mut node = nodes.get_mut(index).and_then(Option::take).ok_or_else(|| invalid("missing nested-table owner"))?;
        for (r, c, children) in node.edges {
            let cell = node.table.cell_mut(r, c).ok_or_else(|| invalid("missing nested-table cell"))?;
            restore_blocks(&mut cell.blocks, children, &mut ready)?;
        }
        *ready.get_mut(index).ok_or_else(|| invalid("missing nested-table slot"))? = Some(node.table);
    }
    restore_blocks(&mut blocks, roots, &mut ready)?;
    Ok(blocks)
}
