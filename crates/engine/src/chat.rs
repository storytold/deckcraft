//! The document chat (craft-collab `docs/chat.md`): the `chat.*` commands.
//!
//! The chat belongs to the presentation ([`deckcraft_model::chat`]): it is saved with the file,
//! shared live in a session (`crate::collab` maps it onto the room's document) and works outside
//! one too, as notes left in the file. Chat changes are not undo steps (Undo never removes a
//! message) but they do make the presentation modified, since it has to be saved for the chat to
//! be kept.

use std::sync::Arc;

use deckcraft_model::Presentation;
use deckcraft_model::chat::{self, Chat, ChatAuthor, ChatMessage};
use serde_json::{Value as Json, json};

use crate::cmd::{CommandSpec, bad, cmd, has_doc, str_param, usize_param};
use crate::{EngineError, Result, Session};

/// Milliseconds since 1970 UTC: the system clock, or the browser's on wasm.
pub fn now_ms() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| i64::try_from(d.as_millis()).unwrap_or(0)).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now().max(0.0) as i64
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "chat.list",
            "Chat Messages",
            [],
            None,
            "{since?: id|ms (messages after that one, or sent after that time), limit?} → {messages: [{id, author: {id, name, color}, at, text?, replyTo?, edited?, deleted?}], clearedAt?, count}, newest last",
            has_doc,
            list
        ),
        cmd!(query "chat.send", "Send Chat Message", [], None, "{text (1–4,000 characters), replyTo?: id} → {id}", can_chat, send),
        cmd!(query "chat.edit", "Edit Chat Message", [], None, "{id, text} (your own messages)", can_chat, edit),
        cmd!(query "chat.delete", "Delete Chat Message", [], None, "{id} (your own messages: \"Message deleted\" stays)", can_chat, delete),
        cmd!(query "chat.clear", "Clear Chat", [], None, "{} → {removed}. Removes every message, for everyone in a live session", can_chat, clear),
        cmd!(query "chat.export", "Export Chat", [], None, "{format?: text|json} → {format, text}", has_doc, export),
        cmd!(query "chat.typing", "Typing in the Chat", [], None, "{typing: bool} → {typing}. Others in a live session see \"Name is typing…\"", has_doc, typing),
    ]
}

/// The chat can change: a presentation is open and, in a live session, the room's document is
/// there (and is DeckCraft's).
fn can_chat(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if let Some(r) = s.active_room() {
        if let Some(why) = &r.refused {
            return Err(why.clone());
        }
        if r.joining {
            return Err("Joining…".into());
        }
    }
    Ok(())
}

/// Who writes: the room's user in a live session, else the app's own.
pub fn me(s: &mut Session) -> ChatAuthor {
    let u = match s.active_room() {
        Some(r) => r.user.clone(),
        None => crate::collab::user_from_prefs(s),
    };
    ChatAuthor { id: u.id, name: u.name, color: u.color }
}

/// The active presentation's chat.
pub fn chat_of(s: &Session) -> Option<&Arc<Chat>> {
    s.active().map(|d| &d.doc.chat)
}

/// Replaces the active presentation's chat: no undo step, but the presentation is modified.
fn set_chat(s: &mut Session, c: Chat) -> Result<()> {
    s.end_composition();
    let st = s.doc_mut()?;
    st.doc = Arc::new(Presentation { chat: Arc::new(c), ..(*st.doc).clone() });
    st.revision += 1;
    Ok(())
}

/// `doc` with `from`'s chat: undo, redo and cancelled compositions restore the slides but never
/// the chat.
pub(crate) fn keep_chat(doc: Arc<Presentation>, from: &Presentation) -> Arc<Presentation> {
    if Arc::ptr_eq(&doc.chat, &from.chat) {
        return doc;
    }
    Arc::new(Presentation { chat: from.chat.clone(), ..(*doc).clone() })
}

/// 8 random lower-case base-36 characters, not used in `c`.
fn new_id(c: &Chat) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut rng = deckcraft_collab::Rng::new(now_ms() as u64 ^ (c.messages.len() as u64).wrapping_mul(0x9E37_79B9));
    loop {
        let id: String = (0..8).map(|_| char::from(DIGITS.get((rng.next_u64() % 36) as usize).copied().unwrap_or(b'0'))).collect();
        if c.get(&id).is_none() {
            return id;
        }
    }
}

fn text_param(p: &Json, cmd: &str) -> Result<String> {
    let t = str_param(p, "text").ok_or_else(|| bad(cmd, "missing `text`"))?;
    if t.trim().is_empty() {
        return Err(bad(cmd, "`text` is empty"));
    }
    if !chat::valid_text(t) {
        return Err(bad(cmd, format!("`text` is longer than {} characters", chat::MAX_TEXT)));
    }
    Ok(t.to_string())
}

fn list(s: &mut Session, p: &Json) -> Result<Json> {
    let c = chat_of(s).cloned().unwrap_or_default();
    let mut from = 0;
    match p.get("since") {
        Some(Json::String(id)) => {
            if let Some(i) = c.index_of(id) {
                from = i + 1;
            }
        }
        Some(v) if v.is_number() => {
            let t = v.as_f64().unwrap_or(0.0);
            from = c.messages.iter().position(|m| m.at as f64 > t).unwrap_or(c.messages.len());
        }
        Some(Json::Null) | None => {}
        Some(_) => return Err(bad("chat.list", "`since` is a message id or a time in milliseconds")),
    }
    let mut msgs: Vec<&ChatMessage> = c.messages.iter().skip(from).collect();
    if let Some(n) = usize_param(p, "limit") {
        let skip = msgs.len().saturating_sub(n);
        msgs.drain(..skip);
    }
    let mut out = json!({"messages": msgs.iter().map(|m| m.to_json()).collect::<Vec<_>>(), "count": c.messages.len()});
    if let (Some(at), Some(o)) = (c.cleared_at, out.as_object_mut()) {
        o.insert("clearedAt".into(), json!(at));
    }
    Ok(out)
}

fn send(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "chat.send";
    let text = text_param(p, ID)?;
    let c = chat_of(s).cloned().unwrap_or_default();
    let reply_to = match p.get("replyTo") {
        None | Some(Json::Null) => None,
        Some(Json::String(r)) if c.get(r).is_some() => Some(r.clone()),
        Some(_) => return Err(bad(ID, "`replyTo` names no message in the chat")),
    };
    if c.messages.len() >= chat::MAX_MESSAGES {
        return Err(EngineError::Other(chat::FULL.into()));
    }
    let id = new_id(&c);
    let author = me(s);
    let mut next = (*c).clone();
    next.messages.push(ChatMessage { id: id.clone(), author, at: now_ms(), text, reply_to, edited: None, deleted: false });
    set_chat(s, next)?;
    Ok(json!({"id": id}))
}

/// The index of `p.id`, a message of this person's.
fn own(s: &mut Session, p: &Json, cmd: &str) -> Result<(Arc<Chat>, usize)> {
    let id = str_param(p, "id").ok_or_else(|| bad(cmd, "missing `id`"))?;
    let c = chat_of(s).cloned().unwrap_or_default();
    let i = c.index_of(id).ok_or_else(|| bad(cmd, format!("no message `{id}` in the chat")))?;
    let mine = me(s).id;
    let m = c.messages.get(i).ok_or_else(|| bad(cmd, "no such message"))?;
    if m.author.id != mine {
        return Err(EngineError::Other("only the author can change a message".into()));
    }
    if m.deleted {
        return Err(EngineError::Other("the message was deleted".into()));
    }
    Ok((c, i))
}

fn edit(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "chat.edit";
    let text = text_param(p, ID)?;
    let (c, i) = own(s, p, ID)?;
    let mut next = (*c).clone();
    if let Some(m) = next.messages.get_mut(i) {
        if m.text == text {
            return Ok(json!({"id": m.id}));
        }
        m.text = text;
        m.edited = Some(now_ms());
    }
    set_chat(s, next)?;
    Ok(json!({"id": p["id"]}))
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let (c, i) = own(s, p, "chat.delete")?;
    let mut next = (*c).clone();
    if let Some(m) = next.messages.get_mut(i) {
        m.deleted = true;
        m.text.clear();
    }
    set_chat(s, next)?;
    Ok(json!({"id": p["id"]}))
}

fn clear(s: &mut Session, _p: &Json) -> Result<Json> {
    let removed = chat_of(s).map(|c| c.messages.len()).unwrap_or(0);
    set_chat(s, Chat { cleared_at: Some(now_ms()), messages: vec![] })?;
    Ok(json!({"removed": removed}))
}

/// `at` as `2026-10-16 14:03 UTC`.
pub fn utc_text(at: i64) -> String {
    let secs = at.div_euclid(1000);
    let (y, m, d) = crate::cmd::design::civil(secs.div_euclid(86_400));
    let day = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", day / 3600, day % 3600 / 60)
}

fn export(s: &mut Session, p: &Json) -> Result<Json> {
    let c = chat_of(s).cloned().unwrap_or_default();
    match str_param(p, "format").unwrap_or("text") {
        "text" => Ok(json!({"format": "text", "text": c.export_text(&utc_text)})),
        "json" => {
            let text = serde_json::to_string_pretty(&c.to_json()).map_err(|e| EngineError::Other(e.to_string()))?;
            Ok(json!({"format": "json", "text": text}))
        }
        _ => Err(bad("chat.export", "`format` is `text` or `json`")),
    }
}

fn typing(s: &mut Session, p: &Json) -> Result<Json> {
    let on = p.get("typing").and_then(Json::as_bool).unwrap_or(false);
    let uid = s.doc()?.uid;
    s.collab.chat_typing = on.then_some(uid);
    Ok(json!({"typing": on}))
}
