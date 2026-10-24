//! The pure side of the host bridge's commands (`deckcraft:cmd`): checking what the parent
//! sent and shaping the answer, apart from the wasm-only code that posts messages, so both can be
//! tested natively.

use serde_json::{Map, Value, json};

/// Largest `params` string accepted from the host; a command's parameters are small, and the
/// string is parsed into memory.
pub const MAX_PARAMS_BYTES: usize = 1 << 20;

/// Most commands waiting for the next frame. The queue is drained once per frame, so a throttled
/// tab or a parent that sends faster than frames run would otherwise grow it without bound; the
/// size limit of `params` applies per command, not to the queue.
pub const MAX_PENDING_COMMANDS: usize = 256;

/// Most `params` text waiting for the next frame, all commands together. A count alone would let
/// 256 large commands queue a quarter of a gigabyte of text, and parsed values take more room
/// than their text.
pub const MAX_PENDING_PARAMS_BYTES: usize = 8 << 20;

pub const MAX_ID_BYTES: usize = 1024;

/// Whether a command whose `params` text is `incoming_bytes` long fits in a queue that already
/// holds `pending` commands with `pending_bytes` of `params` text.
pub fn has_room(pending: usize, pending_bytes: usize, incoming_bytes: usize) -> bool {
    pending < MAX_PENDING_COMMANDS && pending_bytes.saturating_add(incoming_bytes) <= MAX_PENDING_PARAMS_BYTES
}

/// What the parent asked for: the engine command, its parameters and the id to echo back.
#[derive(Debug, PartialEq)]
pub struct Command {
    pub id: String,
    pub params: Value,
    pub request_id: Option<String>,
    /// Length of the `params` text the host sent, for the queue's byte budget.
    pub params_bytes: usize,
}

/// Reads a `deckcraft:cmd` message's fields. `params` is a JSON string, as in WordCraft's bridge;
/// a missing or `null` one means `{}`. Nothing is run for a message that doesn't fit.
pub fn parse(id: Option<String>, params: Option<Option<String>>, request_id: Option<String>) -> Result<Command, String> {
    let id = id.filter(|id| !id.trim().is_empty()).ok_or("a command needs an id")?;
    if id.len() > MAX_ID_BYTES {
        return Err(format!("id is larger than {MAX_ID_BYTES} bytes"));
    }
    let request_id = match request_id {
        Some(r) if r.len() > MAX_ID_BYTES => return Err(format!("requestId is larger than {MAX_ID_BYTES} bytes")),
        other => other,
    };
    let (params, params_bytes) = match params {
        None | Some(None) => (json!({}), 0),
        Some(Some(text)) => {
            if text.len() > MAX_PARAMS_BYTES {
                return Err(format!("params are larger than {MAX_PARAMS_BYTES} bytes"));
            }
            let value = serde_json::from_str(&text).map_err(|_| "params must be a string of valid JSON".to_string())?;
            (value, text.len())
        }
    };
    Ok(Command { id, params, request_id, params_bytes })
}

/// The `deckcraft:response` fields: the command's `id`, the host's `requestId` when it sent one,
/// `ok`, and the result in `value` or the failure in `error`.
pub fn response(id: &str, request_id: Option<&str>, result: Result<Value, String>) -> Value {
    let mut fields = Map::new();
    fields.insert("type".into(), json!("deckcraft:response"));
    fields.insert("id".into(), json!(id));
    if let Some(request_id) = request_id {
        fields.insert("requestId".into(), json!(request_id));
    }
    match result {
        Ok(value) => {
            fields.insert("ok".into(), json!(true));
            fields.insert("value".into(), value);
        }
        Err(error) => {
            fields.insert("ok".into(), json!(false));
            fields.insert("error".into(), json!(error));
        }
    }
    Value::Object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_without_params_runs_with_an_empty_object() -> Result<(), String> {
        let c = parse(Some("home.bold".into()), None, None)?;
        assert_eq!(c, Command { id: "home.bold".into(), params: json!({}), request_id: None, params_bytes: 0 });
        let c = parse(Some("home.bold".into()), Some(None), Some("r1".into()))?;
        assert_eq!((c.params, c.request_id.as_deref()), (json!({}), Some("r1")));
        Ok(())
    }

    #[test]
    fn params_are_a_json_string() -> Result<(), String> {
        let c = parse(Some("cell.set".into()), Some(Some(r#"{"cell":"B2","input":"=1+1"}"#.into())), None)?;
        assert_eq!(c.params, json!({"cell": "B2", "input": "=1+1"}));
        Ok(())
    }

    #[test]
    fn malformed_params_are_refused_not_run_as_empty() {
        assert!(parse(Some("cell.set".into()), Some(Some("{nope".into())), None).is_err());
    }

    #[test]
    fn oversized_params_are_refused() {
        let big = "1".repeat(MAX_PARAMS_BYTES + 1);
        assert!(parse(Some("cell.set".into()), Some(Some(big)), None).is_err());
    }

    #[test]
    fn the_queue_holds_a_fixed_number_of_commands() {
        assert!(has_room(0, 0, 0));
        assert!(has_room(MAX_PENDING_COMMANDS - 1, 0, 0));
        assert!(!has_room(MAX_PENDING_COMMANDS, 0, 0));
    }

    #[test]
    fn the_queue_holds_a_fixed_amount_of_params_text() {
        assert!(has_room(1, MAX_PENDING_PARAMS_BYTES - 10, 10));
        assert!(!has_room(1, MAX_PENDING_PARAMS_BYTES - 10, 11));
        assert!(!has_room(1, usize::MAX, 1));
    }

    #[test]
    fn a_missing_or_blank_id_is_refused() {
        assert!(parse(None, None, None).is_err());
        assert!(parse(Some("  ".into()), None, None).is_err());
    }

    #[test]
    fn a_response_echoes_the_ids_and_carries_value_or_error() {
        let ok = response("cell.set", Some("r1"), Ok(json!({"cell": "B2"})));
        assert_eq!(ok, json!({"type": "deckcraft:response", "id": "cell.set", "requestId": "r1", "ok": true, "value": {"cell": "B2"}}));
        let failed = response("cell.set", None, Err("no such cell".into()));
        assert_eq!(failed, json!({"type": "deckcraft:response", "id": "cell.set", "ok": false, "error": "no such cell"}));
    }
}
