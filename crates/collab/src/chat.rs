//! The document chat in the shared document (craft-collab `docs/chat.md`): the root array `chat`
//! of messages (each a map `{id, author, at, text, replyTo?, edited?, deleted?}`, `author` the
//! JSON text of `{id, name, color}`) and the root map `chatMeta` (`clearedAt`).
//!
//! Messages are only appended; their order is their place in the array (the same on every peer).
//! Clearing deletes every entry in one transaction and sets `clearedAt`. Neither root is in the
//! undo manager's scope: Undo never removes a message.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use deckcraft_model::chat::{Chat, ChatMessage};
use serde_json::{Map as JsonMap, Value as Json};
use yrs::{Any, Array, ArrayRef, Map, MapPrelim, MapRef, Out, ReadTxn, TransactionMut, WriteTxn};

/// Root names.
pub(crate) const CHAT: &str = "chat";
pub(crate) const CHAT_META: &str = "chatMeta";

fn any_str(s: &str) -> yrs::In {
    yrs::In::Any(Any::from(s))
}

fn prelim(m: &ChatMessage) -> MapPrelim {
    let mut fields: Vec<(&str, yrs::In)> =
        vec![("id", any_str(&m.id)), ("author", any_str(&m.author.to_json().to_string())), ("at", yrs::In::Any(Any::from(m.at)))];
    if !m.deleted {
        fields.push(("text", any_str(&m.text)));
    }
    if let Some(r) = &m.reply_to {
        fields.push(("replyTo", any_str(r)));
    }
    if let Some(e) = m.edited {
        fields.push(("edited", yrs::In::Any(Any::from(e))));
    }
    if m.deleted {
        fields.push(("deleted", yrs::In::Any(Any::Bool(true))));
    }
    MapPrelim::from_iter(fields)
}

/// The id of an entry of the `chat` array.
fn entry_id<R: ReadTxn>(v: &Out, txn: &R) -> Option<Arc<str>> {
    match v {
        Out::YMap(m) => match m.get(txn, "id") {
            Some(Out::Any(Any::String(s))) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

/// The entry (map) of the first message with `id`.
fn entry<R: ReadTxn>(arr: &ArrayRef, txn: &R, id: &str) -> Option<MapRef> {
    arr.iter(txn).find_map(|v| match (&v, entry_id(&v, txn)) {
        (Out::YMap(m), Some(i)) if &*i == id => Some(m.clone()),
        _ => None,
    })
}

/// Writes the change from `old` (what the document holds; `None`: nothing yet) to `new`:
/// messages appended, the author's edits and deletions set on their entries, a clear as every
/// entry removed and `clearedAt` set.
pub(crate) fn write(txn: &mut TransactionMut, old: Option<&Arc<Chat>>, new: &Arc<Chat>) {
    if old.is_some_and(|o| Arc::ptr_eq(o, new) || **o == **new) {
        return;
    }
    if old.is_none() && new.is_empty() && new.cleared_at.is_none() {
        return;
    }
    let empty = Chat::default();
    let before: &Chat = old.map(|o| &**o).unwrap_or(&empty);
    let arr = txn.get_or_insert_array(CHAT);
    let cleared = new.cleared_at.is_some() && new.cleared_at != before.cleared_at;
    if cleared {
        let n = arr.len(txn);
        if n > 0 {
            arr.remove_range(txn, 0, n);
        }
        let meta = txn.get_or_insert_map(CHAT_META);
        meta.insert(txn, "clearedAt", Any::from(new.cleared_at.unwrap_or(0)));
    }
    let had: HashMap<&str, &ChatMessage> = if cleared { HashMap::new() } else { before.messages.iter().map(|m| (m.id.as_str(), m)).collect() };
    // Messages that went (only a clear removes them; anything else is a peer's oddity).
    let keep: HashSet<&str> = new.messages.iter().map(|m| m.id.as_str()).collect();
    if had.keys().any(|id| !keep.contains(id)) {
        let gone: Vec<u32> = arr
            .iter(txn)
            .enumerate()
            .filter(|(_, v)| entry_id(v, txn).is_some_and(|id| had.contains_key(&*id) && !keep.contains(&*id)))
            .map(|(i, _)| i as u32)
            .collect();
        for i in gone.into_iter().rev() {
            arr.remove(txn, i);
        }
    }
    for m in &new.messages {
        match had.get(m.id.as_str()) {
            None => {
                arr.push_back(txn, prelim(m));
            }
            Some(o) if *o != m => {
                let Some(e) = entry(&arr, txn, &m.id) else { continue };
                if m.deleted {
                    if !o.deleted {
                        e.remove(txn, "text");
                        e.insert(txn, "deleted", true);
                    }
                } else if o.text != m.text {
                    e.insert(txn, "text", m.text.as_str());
                }
                if m.edited != o.edited {
                    match m.edited {
                        Some(at) => {
                            e.insert(txn, "edited", Any::from(at));
                        }
                        None => {
                            e.remove(txn, "edited");
                        }
                    }
                }
            }
            Some(_) => {}
        }
    }
}

/// The chat the document holds, validated as a file's (at most 10,000 entries looked at,
/// malformed ones and repeated ids skipped).
pub(crate) fn read<R: ReadTxn>(txn: &R) -> Chat {
    let cleared_at = txn.get_map(CHAT_META).and_then(|m| match m.get(txn, "clearedAt") {
        Some(Out::Any(a)) => {
            let j = crate::json::to_json(&a);
            j.as_i64().filter(|v| *v >= 0)
        }
        _ => None,
    });
    let Some(arr) = txn.get_array(CHAT) else { return Chat { cleared_at, messages: vec![] } };
    let entries: Vec<Json> = arr
        .iter(txn)
        .take(deckcraft_model::chat::MAX_MESSAGES)
        .map(|v| match v {
            Out::YMap(m) => {
                let mut o = JsonMap::new();
                for (k, v) in m.iter(txn) {
                    if let Out::Any(a) = v {
                        o.insert(k.to_string(), crate::json::to_json(&a));
                    }
                }
                Json::Object(o)
            }
            _ => Json::Null,
        })
        .collect();
    Chat::from_entries(&entries, cleared_at)
}
