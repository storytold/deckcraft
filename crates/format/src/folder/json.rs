//! Canonical JSON for the folder format (R1, R2).
//!
//! [`J`] is a JSON value whose objects keep their key order: the order the model writes, so files
//! read like the model. (`serde_json::Value` sorts keys unless the crate-wide `preserve_order`
//! feature is on, which would change the key order of every JSON DeckCraft writes.)

use std::collections::HashSet;

use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A JSON value with ordered object keys.
#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            J::Num(n) => n.as_u64(),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, J::Null)
    }
    /// The numeric `id` of an object.
    pub fn id(&self) -> Option<u64> {
        self.get("id").and_then(J::as_u64)
    }
    /// Remove `key` from an object, returning its value.
    pub fn remove(&mut self, key: &str) -> Option<J> {
        match self {
            J::Obj(m) => m.iter().position(|(k, _)| k == key).map(|i| m.remove(i).1),
            _ => None,
        }
    }
    /// Set `key` in an object (in place if present, else at the end).
    pub fn set(&mut self, key: &str, v: J) {
        if let J::Obj(m) = self {
            match m.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = v,
                None => m.push((key.to_string(), v)),
            }
        }
    }
}

impl Serialize for J {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            J::Null => s.serialize_unit(),
            J::Bool(b) => s.serialize_bool(*b),
            J::Num(n) => n.serialize(s),
            J::Str(x) => s.serialize_str(x),
            J::Arr(a) => {
                let mut seq = s.serialize_seq(Some(a.len()))?;
                for x in a {
                    seq.serialize_element(x)?;
                }
                seq.end()
            }
            J::Obj(m) => {
                let mut map = s.serialize_map(Some(m.len()))?;
                for (k, v) in m {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

struct JVisitor;

impl<'de> Visitor<'de> for JVisitor {
    type Value = J;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_bool<E>(self, b: bool) -> Result<J, E> {
        Ok(J::Bool(b))
    }
    fn visit_i64<E>(self, n: i64) -> Result<J, E> {
        Ok(J::Num(n.into()))
    }
    fn visit_u64<E>(self, n: u64) -> Result<J, E> {
        Ok(J::Num(n.into()))
    }
    fn visit_f64<E>(self, n: f64) -> Result<J, E> {
        Ok(serde_json::Number::from_f64(n).map_or(J::Null, J::Num))
    }
    fn visit_str<E>(self, s: &str) -> Result<J, E> {
        Ok(J::Str(s.to_string()))
    }
    fn visit_string<E>(self, s: String) -> Result<J, E> {
        Ok(J::Str(s))
    }
    fn visit_unit<E>(self) -> Result<J, E> {
        Ok(J::Null)
    }
    fn visit_none<E>(self) -> Result<J, E> {
        Ok(J::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<J, D::Error> {
        J::deserialize(d)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<J, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(x) = seq.next_element::<J>()? {
            out.push(x);
        }
        Ok(J::Arr(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<J, A::Error> {
        let mut out: Vec<(String, J)> = Vec::new();
        while let Some((k, v)) = map.next_entry::<String, J>()? {
            out.push((k, v));
        }
        // A repeated key keeps its first place and its last value, like serde_json.
        let mut seen = HashSet::with_capacity(out.len());
        if out.iter().all(|(k, _)| seen.insert(k.as_str())) {
            return Ok(J::Obj(out));
        }
        let mut dedup: Vec<(String, J)> = Vec::with_capacity(out.len());
        for (k, v) in out {
            match dedup.iter_mut().find(|(x, _)| *x == k) {
                Some(slot) => slot.1 = v,
                None => dedup.push((k, v)),
            }
        }
        Ok(J::Obj(dedup))
    }
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<J, D::Error> {
        d.deserialize_any(JVisitor)
    }
}

/// `t` as written by the model, keys in model order.
pub fn to_j<T: Serialize>(t: &T) -> Result<J, String> {
    let s = serde_json::to_string(t).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

/// Read `j` into a model type.
pub fn from_j<T: DeserializeOwned>(j: &J) -> Result<T, String> {
    let v = serde_json::to_value(j).map_err(|e| e.to_string())?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}

pub fn parse(bytes: &[u8]) -> Result<J, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

/// R1: 2-space indentation, LF line ends, final newline, unescaped UTF-8, keys in source order.
pub fn pretty(j: &J) -> Vec<u8> {
    let mut out = Vec::with_capacity(4096);
    if serde_json::to_writer_pretty(&mut out, j).is_err() {
        out.clear();
        out.extend_from_slice(b"null");
    }
    out.push(b'\n');
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Index(usize),
}

pub type Path = Vec<Seg>;

/// `v` without its `key: null` pairs, except at the paths in `keep` and under those in `unknown`.
/// Nulls inside arrays stay: they are positional.
fn strip_nulls(v: &J, keep: &[Path], unknown: &[Path]) -> J {
    fn rec(v: &J, path: &mut Path, keep: &[Path], unknown: &[Path]) -> J {
        match v {
            J::Obj(m) => {
                let mut out = Vec::with_capacity(m.len());
                for (k, x) in m {
                    path.push(Seg::Key(k.clone()));
                    let protected = keep.iter().any(|p| p == path) || unknown.iter().any(|p| path.starts_with(p));
                    if !x.is_null() || protected {
                        out.push((k.clone(), rec(x, path, keep, unknown)));
                    }
                    path.pop();
                }
                J::Obj(out)
            }
            J::Arr(a) => {
                let mut out = Vec::with_capacity(a.len());
                for (i, x) in a.iter().enumerate() {
                    path.push(Seg::Index(i));
                    out.push(rec(x, path, keep, unknown));
                    path.pop();
                }
                J::Arr(out)
            }
            other => other.clone(),
        }
    }
    rec(v, &mut Vec::new(), keep, unknown)
}

/// Paths where `a` is `null` and `b` holds something else (or nothing).
fn null_diffs(a: &J, b: &J, path: &mut Path, out: &mut Vec<Path>) {
    match (a, b) {
        (J::Obj(ma), J::Obj(_)) => {
            for (k, xa) in ma {
                path.push(Seg::Key(k.clone()));
                match b.get(k) {
                    Some(xb) if xa.is_null() && !xb.is_null() => out.push(path.clone()),
                    Some(xb) => null_diffs(xa, xb, path, out),
                    None if xa.is_null() => out.push(path.clone()),
                    None => {}
                }
                path.pop();
            }
        }
        (J::Arr(aa), J::Arr(ab)) => {
            for (i, (xa, xb)) in aa.iter().zip(ab).enumerate() {
                path.push(Seg::Index(i));
                null_diffs(xa, xb, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Copy into `new` the keys of `prev` that the model does not know (absent from `view`, the
/// model's reading of `prev`), each right after the key that precedes it in `prev`. Objects are
/// aligned by key, array items by `id` when they have one, else by index when the lengths match.
/// Records the inserted paths (in `new`) in `unknown`.
fn merge_unknown(new: &mut J, prev: &J, view: &J, path: &mut Path, unknown: &mut Vec<Path>, depth: usize) {
    if depth > 256 {
        return;
    }
    match (new, prev, view) {
        (J::Obj(n), J::Obj(p), J::Obj(_)) => {
            for (k, x) in n.iter_mut() {
                if let (Some(px), Some(vx)) = (prev.get(k), view.get(k)) {
                    path.push(Seg::Key(k.clone()));
                    merge_unknown(x, px, vx, path, unknown, depth + 1);
                    path.pop();
                }
            }
            for (i, (k, x)) in p.iter().enumerate() {
                if view.get(k).is_some() || n.iter().any(|(nk, _)| nk == k) {
                    continue;
                }
                // After the nearest preceding key of `prev` that `new` has (unknown keys already
                // inserted count, so several unknown keys keep their order).
                let pos = p.get(..i).unwrap_or(&[]).iter().rev().find_map(|(pk, _)| n.iter().position(|(nk, _)| nk == pk)).map_or(0, |j| j + 1);
                n.insert(pos.min(n.len()), (k.clone(), x.clone()));
                path.push(Seg::Key(k.clone()));
                unknown.push(path.clone());
                path.pop();
            }
        }
        (J::Arr(n), J::Arr(p), J::Arr(v)) if p.len() == v.len() => {
            let same_len = n.len() == p.len();
            for (i, (px, vx)) in p.iter().zip(v).enumerate() {
                let target = match px.id() {
                    Some(id) => n.iter().position(|x| x.id() == Some(id)),
                    None => same_len.then_some(i),
                };
                if let Some(t) = target
                    && let Some(nx) = n.get_mut(t)
                {
                    path.push(Seg::Index(t));
                    merge_unknown(nx, px, vx, path, unknown, depth + 1);
                    path.pop();
                }
            }
        }
        _ => {}
    }
}

/// R2: strip nulls, reload, and put back those whose absence changes the model. `m0` is the
/// model `v` loads as.
fn strip_verified<T: DeserializeOwned + Serialize + PartialEq>(v: &J, m0: &T, unknown: &[Path]) -> J {
    let s1 = strip_nulls(v, &[], unknown);
    let Ok(m1) = from_j::<T>(&s1) else { return v.clone() };
    if m1 == *m0 {
        return s1;
    }
    let Ok(c1) = to_j(&m1) else { return v.clone() };
    let mut kept = vec![];
    null_diffs(v, &c1, &mut Vec::new(), &mut kept);
    let s2 = strip_nulls(v, &kept, unknown);
    match from_j::<T>(&s2) {
        Ok(m2) if m2 == *m0 => s2,
        _ => v.clone(),
    }
}

/// The canonical JSON of `t` (R1, R2): model key order, nulls dropped where that changes nothing,
/// and the keys of `prev` (the same file before this save) that this model doesn't know kept in
/// place, so a folder written by a newer DeckCraft keeps what this build can't read.
pub fn canonical<T: DeserializeOwned + Serialize + PartialEq>(t: &T, prev: Option<&J>) -> Result<J, String> {
    let full = to_j(t)?;
    // What loading the same JSON from a `.deckcraft` gives: the reference for every check.
    let Ok(m0) = from_j::<T>(&full) else { return Ok(full) };
    let mut v = full;
    let mut unknown = vec![];
    if let Some(prev) = prev
        && let Ok(view) = from_j::<T>(prev).and_then(|m| to_j(&m))
    {
        let mut merged = v.clone();
        merge_unknown(&mut merged, prev, &view, &mut Vec::new(), &mut unknown, 0);
        if !unknown.is_empty() {
            if from_j::<T>(&merged).is_ok_and(|m| m == m0) {
                v = merged;
            } else {
                unknown.clear();
            }
        }
    }
    Ok(strip_verified(&v, &m0, &unknown))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(s: &str) -> J {
        parse(s.as_bytes()).unwrap()
    }

    #[test]
    fn keeps_key_order_and_numbers() {
        let v = j(r#"{"b": 1.0, "a": "é", "n": null, "x": [1, null, -2, 0.5]}"#);
        let s = String::from_utf8(pretty(&v)).unwrap();
        assert_eq!(s, "{\n  \"b\": 1.0,\n  \"a\": \"é\",\n  \"n\": null,\n  \"x\": [\n    1,\n    null,\n    -2,\n    0.5\n  ]\n}\n");
    }

    #[test]
    fn repeated_keys_keep_last_value() {
        assert_eq!(j(r#"{"a": 1, "b": 2, "a": 3}"#), j(r#"{"a": 3, "b": 2}"#));
    }

    #[test]
    fn strip_keeps_array_nulls_and_kept_paths() {
        let v = j(r#"{"a": null, "b": {"c": null, "d": 1}, "e": [null, {"f": null}]}"#);
        let s = strip_nulls(&v, &[vec![Seg::Key("b".into()), Seg::Key("c".into())]], &[]);
        assert_eq!(s, j(r#"{"b": {"c": null, "d": 1}, "e": [null, {}]}"#));
    }

    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    #[serde(default)]
    struct T {
        a: Option<u32>,
        title: Option<String>,
        b: u32,
        items: Vec<Item>,
    }
    impl Default for T {
        fn default() -> Self {
            T { a: None, title: Some("Chart Title".into()), b: 0, items: vec![] }
        }
    }
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug, Default)]
    #[serde(default)]
    struct Item {
        id: u32,
        x: Option<u32>,
    }

    #[test]
    fn nulls_whose_absence_matters_are_kept() {
        let t = T { a: None, title: None, b: 2, items: vec![Item { id: 7, x: None }] };
        let v = canonical(&t, None).unwrap();
        assert_eq!(v, j(r#"{"title": null, "b": 2, "items": [{"id": 7}]}"#));
        assert_eq!(from_j::<T>(&v).unwrap(), t);
    }

    #[test]
    fn unknown_keys_of_the_previous_file_stay_in_place() {
        let prev = j(r#"{"a": 1, "future": {"k": null}, "title": "T", "b": 2, "items": [{"id": 7, "later": true}, {"id": 8}], "end": 0}"#);
        // Items reordered and one changed: unknown keys follow their item by id.
        let t = T { a: Some(1), title: Some("T".into()), b: 3, items: vec![Item { id: 8, x: None }, Item { id: 7, x: Some(1) }] };
        let v = canonical(&t, Some(&prev)).unwrap();
        assert_eq!(
            v,
            j(r#"{"a": 1, "future": {"k": null}, "title": "T", "b": 3, "items": [{"id": 8}, {"id": 7, "later": true, "x": 1}], "end": 0}"#)
        );
        // Stable on its own output.
        assert_eq!(canonical(&t, Some(&v)).unwrap(), v);
    }
}
