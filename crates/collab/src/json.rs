//! JSON values ↔ Yjs `Any` (formatting attributes are native Yjs values: `bold: true`,
//! `size: 24`, `font: "Arial"`, objects as maps).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Map as JsonMap, Number, Value as Json};
use yrs::Any;
use yrs::any::Number as YNumber;

/// Deepest nesting converted (deeper parts become `null`): input from peers is hostile.
const MAX_DEPTH: usize = 64;

/// A JSON value as Yjs `Any`. Whole numbers that fit 32 bits become integers (as Yjs writes them),
/// others floats: one value always has one form, so values compare equal however they arrived.
pub fn to_any(v: &Json) -> Any {
    to_any_at(v, 0)
}

fn to_any_at(v: &Json, depth: usize) -> Any {
    if depth > MAX_DEPTH {
        return Any::Null;
    }
    match v {
        Json::Null => Any::Null,
        Json::Bool(b) => Any::Bool(*b),
        Json::Number(n) => number(n.as_i64().map(|i| i as f64).or_else(|| n.as_f64()).unwrap_or(0.0)),
        Json::String(s) => Any::String(Arc::from(s.as_str())),
        Json::Array(a) => Any::Array(a.iter().map(|x| to_any_at(x, depth + 1)).collect::<Vec<_>>().into()),
        Json::Object(o) => {
            let m: HashMap<String, Any> = o.iter().map(|(k, x)| (k.clone(), to_any_at(x, depth + 1))).collect();
            Any::Map(Arc::new(m))
        }
    }
}

fn number(f: f64) -> Any {
    if f.fract() == 0.0 && f.abs() <= 2_147_483_647.0 { Any::Number(YNumber::Int(f as i64)) } else { Any::Number(YNumber::Float(f)) }
}

/// `v` in the one form [`to_any`] gives (numbers as there), so equal values compare equal.
pub fn canon(v: &Any) -> Any {
    to_any(&to_json(v))
}

/// Yjs `Any` as JSON. A whole `Number` (a peer that writes every number as a float) becomes an
/// integer, so it reads into integer fields; bytes become an array of numbers.
pub fn to_json(v: &Any) -> Json {
    to_json_at(v, 0)
}

fn to_json_at(v: &Any, depth: usize) -> Json {
    if depth > MAX_DEPTH {
        return Json::Null;
    }
    match v {
        Any::Null | Any::Undefined => Json::Null,
        Any::Bool(b) => Json::Bool(*b),
        Any::Number(YNumber::Int(i)) => Json::Number(Number::from(*i)),
        Any::Number(YNumber::Float(f)) => {
            if f.fract() == 0.0 && f.abs() < 9.0e15 {
                Json::Number(Number::from(*f as i64))
            } else {
                Number::from_f64(*f).map(Json::Number).unwrap_or(Json::Null)
            }
        }
        Any::String(s) => Json::String(s.to_string()),
        Any::Buffer(b) => Json::Array(b.iter().map(|x| Json::from(*x)).collect()),
        Any::Array(a) => Json::Array(a.iter().map(|x| to_json_at(x, depth + 1)).collect()),
        Any::Map(m) => {
            let mut o = JsonMap::new();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for k in keys {
                if let Some(x) = m.get(k) {
                    o.insert(k.clone(), to_json_at(x, depth + 1));
                }
            }
            Json::Object(o)
        }
    }
}

/// The non-null entries of a JSON object (a model struct's `Option` fields serialize as `null`).
pub fn set_entries(v: &Json) -> Vec<(String, Json)> {
    match v {
        Json::Object(o) => o.iter().filter(|(_, x)| !x.is_null()).map(|(k, x)| (k.clone(), x.clone())).collect(),
        _ => Vec::new(),
    }
}

/// Deserializes `T` from `obj`, keeping what can be kept: when the whole object doesn't read
/// (a peer wrote a field this build can't take), fields are added one by one onto `T`'s default
/// and those that break it are left out.
pub fn lenient<T: serde::de::DeserializeOwned + serde::Serialize + Default>(obj: JsonMap<String, Json>) -> T {
    if let Ok(v) = serde_json::from_value::<T>(Json::Object(obj.clone())) {
        return v;
    }
    let mut base = match serde_json::to_value(T::default()) {
        Ok(Json::Object(o)) => o,
        _ => JsonMap::new(),
    };
    for (k, v) in obj {
        let mut trial = base.clone();
        trial.insert(k.clone(), v);
        if serde_json::from_value::<T>(Json::Object(trial.clone())).is_ok() {
            base = trial;
        } else {
            log::debug!("collab: dropped a property `{k}` this build can't read");
        }
    }
    serde_json::from_value(Json::Object(base)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips() {
        let v = json!({"a": 1, "b": 2.5, "c": [true, null, "x"], "d": {"e": -3}});
        assert_eq!(to_json(&to_any(&v)), v);
        // A peer that writes whole numbers as floats still reads into integer fields.
        assert_eq!(to_json(&Any::Number(YNumber::Float(12.0))), json!(12));
        assert_eq!(to_any(&json!(24.0)), to_any(&json!(24)));
        assert_eq!(canon(&Any::Number(YNumber::Float(24.0))), to_any(&json!(24)));
        let mut deep = json!(1);
        for _ in 0..200 {
            deep = json!([deep]);
        }
        let _ = to_json(&to_any(&deep));
    }
}
