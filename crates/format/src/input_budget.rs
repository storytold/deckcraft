//! Streaming native JSON preflight before tagged-enum buffering or legacy repair.
use crate::{FormatError, Result};
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

pub(super) const MAX_JSON_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOKEN: usize = 1024 * 1024;
// A decoded byte can use six JSON bytes (\uXXXX); decoded tokens stay capped.
const MAX_ENCODED_TOKEN: usize = 6 * MAX_TOKEN;
const MAX_KEY: usize = 1024;
const MAX_METADATA: usize = 128 * 1024 * 1024;
const MAX_VALUES: usize = 2_000_000;
// Encoded depth includes object/array wrappers, unlike semantic shape/table depth.
// A fixed cap covers 64 group owners plus 16 nested tables and their rich text.
pub(super) const MAX_JSON_DEPTH: usize = 512;

fn damaged(reason: &str) -> FormatError {
    FormatError::Damaged(reason.into())
}

// Bound the JSON parser's unescape scratch before it receives any token. This is
// a lexical length pass; the JSON parser subsequently validates the full grammar.
fn tokens(bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(damaged("native presentation JSON exceeds size limit"));
    }
    let mut at = 0usize;
    let mut depth = 0usize;
    while let Some(byte) = bytes.get(at) {
        if *byte != b'"' {
            if matches!(*byte, b'{' | b'[') {
                depth = depth
                    .checked_add(1)
                    .filter(|depth| *depth <= MAX_JSON_DEPTH)
                    .ok_or_else(|| damaged("native JSON nesting exceeds depth limit"))?;
            } else if matches!(*byte, b'}' | b']') {
                depth = depth.saturating_sub(1);
            }
            at += 1;
            continue;
        }
        at += 1;
        let start = at;
        loop {
            match bytes.get(at) {
                Some(b'"') => break,
                Some(b'\\') => {
                    at = at.checked_add(2).ok_or_else(|| damaged("invalid JSON token"))?;
                }
                Some(_) => at += 1,
                None => return Err(damaged("unterminated JSON string")),
            }
            if at.saturating_sub(start) > MAX_ENCODED_TOKEN {
                return Err(damaged("native JSON string token exceeds size limit"));
            }
        }
        let length = at.saturating_sub(start);
        at += 1;
        let mut after = at;
        while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
            after += 1;
        }
        if bytes.get(after) == Some(&b':') && length > MAX_KEY {
            return Err(damaged("native JSON key exceeds size limit"));
        }
    }
    Ok(())
}

struct Budget {
    bytes: usize,
    limit: usize,
    values: usize,
    allow_repair: bool,
}
impl Budget {
    fn charge<E: de::Error>(&mut self, bytes: usize) -> std::result::Result<(), E> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > self.limit {
            return Err(E::custom("native stored-value metadata exceeds allocation budget"));
        }
        Ok(())
    }
}
struct Key {
    bytes: usize,
    marked: bool,
    stops_repair: bool,
}
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Keys;
        impl Visitor<'_> for Keys {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded JSON key")
            }
            fn visit_str<E: de::Error>(self, key: &str) -> std::result::Result<Key, E> {
                if key.len() > MAX_KEY {
                    return Err(E::custom("native JSON key exceeds size limit"));
                }
                Ok(Key {
                    bytes: key.len(),
                    marked: matches!(
                        key,
                        "data" | "formula" | "display" | "value" | "blocks" | "numberFormat" | "comments" | "authorIdentity" | "attachments" | "task"
                    ),
                    stops_repair: matches!(
                        key,
                        "data" | "formula" | "display" | "blocks" | "numberFormat" | "commentId" | "authorIdentity" | "wireGuid" | "task"
                    ),
                })
            }
        }
        d.deserialize_identifier(Keys)
    }
}
struct Scan<'a> {
    budget: &'a mut Budget,
    depth: usize,
    marked: bool,
}
impl<'de> DeserializeSeed<'de> for Scan<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        if self.depth > MAX_JSON_DEPTH {
            return Err(de::Error::custom("native JSON nesting exceeds limit"));
        }
        self.budget.values = self.budget.values.saturating_add(1);
        if self.budget.values > MAX_VALUES {
            return Err(de::Error::custom("native JSON work exceeds limit"));
        }
        if self.marked {
            self.budget.charge::<D::Error>(128)?;
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Scan<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded native JSON")
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, text: &str) -> std::result::Result<(), E> {
        if text.len() > MAX_TOKEN {
            return Err(E::custom("native decoded JSON string exceeds size limit"));
        }
        if self.marked {
            self.budget.charge::<E>(96usize.saturating_add(text.len().saturating_mul(2)))?;
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        if self.marked {
            self.budget.charge::<A::Error>(128)?;
        }
        while seq.next_element_seed(Scan { budget: self.budget, depth: self.depth + 1, marked: self.marked })?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        if self.marked {
            self.budget.charge::<A::Error>(256)?;
        }
        while let Some(key) = map.next_key::<Key>()? {
            if key.stops_repair {
                self.budget.allow_repair = false;
            }
            let marked = self.marked || key.marked;
            if marked {
                // Include immutable record/Arc overhead at every trigger and Content map
                // entries, owned keys and geometric spare capacity throughout that subtree.
                let trigger = if !self.marked { 256 } else { 0 };
                self.budget.charge::<A::Error>(128usize.saturating_add(trigger).saturating_add(96).saturating_add(key.bytes.saturating_mul(2)))?;
            }
            map.next_value_seed(Scan { budget: self.budget, depth: self.depth + 1, marked })?;
        }
        Ok(())
    }
}

fn check_limit(json: &[u8], limit: usize) -> Result<bool> {
    tokens(json)?;
    let mut budget = Budget { bytes: 0, limit, values: 0, allow_repair: true };
    let mut parser = serde_json::Deserializer::from_slice(json);
    // The nonrecursive lexical pass above enforces MAX_JSON_DEPTH before recursion.
    parser.disable_recursion_limit();
    Scan { budget: &mut budget, depth: 0, marked: false }.deserialize(&mut parser).map_err(|error| damaged(&error.to_string()))?;
    parser.end().map_err(|error| damaged(&error.to_string()))?;
    Ok(budget.allow_repair)
}

/// Cap serialized JSON before growing its buffer, then apply the same input profile so
/// every successfully saved file satisfies the reader's resource checks.
pub(super) fn serialize<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    struct Buffer(Vec<u8>);
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let length = self.0.len().checked_add(bytes.len()).ok_or_else(|| std::io::Error::other("native JSON size overflow"))?;
            if length as u64 > MAX_JSON_BYTES {
                return Err(std::io::Error::other("native presentation JSON exceeds size limit"));
            }
            if length > self.0.capacity() {
                let capacity = self.0.capacity().saturating_mul(2).max(1024).max(length).min(MAX_JSON_BYTES as usize);
                self.0.try_reserve_exact(capacity.saturating_sub(self.0.len())).map_err(|_| std::io::Error::other("cannot allocate native JSON"))?;
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer(Vec::new());
    serde_json::to_writer(&mut buffer, value).map_err(|error| FormatError::Write(error.to_string()))?;
    check(&buffer.0).map_err(|error| FormatError::Write(error.to_string()))?;
    Ok(buffer.0)
}

/// Limits stored constant/formula metadata before any complete model/Value allocation. This
/// conservative estimate includes serde tagged-enum buffering; it is not an RSS limit.
pub(super) fn check(json: &[u8]) -> Result<bool> {
    check_limit(json, MAX_METADATA)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn positional_cells_and_escaped_keys_cannot_bypass_metadata_or_repair_policy() {
        for json in [
            r#"[{"value":{"kind":"boolean","value":true},"display":[]}]"#,
            r#"{"d\u0061ta":{"value":{"kind":"string","value":"stored"},"display":[""]}}"#,
        ] {
            assert!(!check(json.as_bytes()).unwrap());
            assert!(check_limit(json.as_bytes(), 256).is_err());
        }
        assert!(check(br#"{"ordinary":[null,0],"label":"data display value"}"#).unwrap());
        assert!(!check(br#"{"unknown":{"display":null}}"#).unwrap());
    }
    #[test]
    fn formulas_mark_the_complete_subtree_and_disable_numeric_repair() {
        for json in [
            br#"{"formula":{"profile":"exact-decimal-v1","expression":"=1+2","outcome":{"number":null}}}"#.as_slice(),
            br#"[{"f\u006frmula":{"arbitrary":{"nested":["","","",""]}}}]"#.as_slice(),
            br#"{"unknown":{"formula":null}}"#.as_slice(),
        ] {
            assert!(!check(json).unwrap());
            assert!(check_limit(json, 256).is_err());
        }
        assert!(check(br#"{"ordinary":[null],"label":"formula"}"#).unwrap());
    }
    #[test]
    fn one_shared_counter_bounds_duplicates_descendants_and_empty_baseline_overhead() {
        let single = br#"{"display":["","","",""]}"#;
        assert!(check_limit(single, 2000).is_ok());
        assert!(check_limit(br#"{"display":["","","",""],"display":["","","",""]}"#, 2000).is_err());
        assert!(check_limit(br#"{"outer":[{"value":{"nested":{"nested":["","","",""]}}}]}"#, 2000).is_err());
        // A valid zero-paragraph baseline still incurs record/vector allocation.
        assert!(check_limit(br#"{"display":[]}"#, 400).is_err());
    }
    #[test]
    fn raw_tokens_keys_trailing_documents_and_invalid_json_refuse_before_model_parse() {
        let long = format!("\"{}\"", "x".repeat(MAX_TOKEN + 1));
        assert!(check(long.as_bytes()).is_err());
        let key = format!("{{\"{}\":0}}", "x".repeat(MAX_KEY + 1));
        assert!(check(key.as_bytes()).is_err());
        assert!(check(br#"{} {}"#).is_err());
        assert!(check(br#"{"value": [}"#).is_err());
        assert!(check(br#"{"value":"unterminated}"#).is_err());
        let valid = serde_json::to_vec(&"\\\"quoted\n").unwrap();
        assert!(check(&valid).is_ok());
    }
    #[test]
    fn lexical_depth_is_capped_before_recursive_parsers_and_escaped_tokens_keep_decoded_limit() {
        let exact = format!("{}0{}", "[".repeat(MAX_JSON_DEPTH), "]".repeat(MAX_JSON_DEPTH));
        assert!(check(exact.as_bytes()).is_ok());
        let too_deep = format!("{}0{}", "[".repeat(MAX_JSON_DEPTH + 1), "]".repeat(MAX_JSON_DEPTH + 1));
        assert!(check(too_deep.as_bytes()).unwrap_err().to_string().contains("depth limit"));
        let escaped = format!("\"{}\"", r"\u0041".repeat(MAX_TOKEN));
        assert!(check(escaped.as_bytes()).is_ok());
        let too_long = format!("\"{}\"", r"\u0041".repeat(MAX_TOKEN + 1));
        assert!(check(too_long.as_bytes()).is_err());
    }
}
