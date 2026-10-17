//! The document's chat (craft-collab `docs/chat.md`, version 1.0): the messages the people
//! editing a presentation leave beside it. It belongs to the presentation: saved in the file (the
//! `.pptx` part `/craft/chat.json`, the `chat` entry of the native format), shared live in a
//! session, and kept until someone clears it.
//!
//! Everything read here (a file's part, a peer's entries) is validated the same way: entries
//! beyond [`MAX_MESSAGES`], with a malformed `author`, `at` or `text`, or repeating an earlier
//! `id`, are skipped; nothing here fails.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// Most messages a chat holds; at the limit new ones are refused ([`FULL`]).
pub const MAX_MESSAGES: usize = 10_000;
/// Longest message, in UTF-16 code units.
pub const MAX_TEXT: usize = 4_000;
/// The JSON's `version`.
pub const VERSION: u64 = 1;
/// What the app says when the chat is at [`MAX_MESSAGES`].
pub const FULL: &str = "The chat is full: clear it to go on";
/// Largest `at` read (2^53, the largest integer every peer's numbers hold exactly).
const MAX_AT: i64 = 1 << 53;

/// A message id: 8 characters of `0-9a-z`.
pub fn valid_id(s: &str) -> bool {
    s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

/// Who wrote a message: the `user` of the room's join (outside a session, the app's own user).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAuthor {
    pub id: String,
    pub name: String,
    /// `#RRGGBB`.
    pub color: String,
}

impl ChatAuthor {
    /// An author from JSON (an object, or JSON text of one as the shared document holds it):
    /// non-empty `id` and `name` and a string `color` (shown grey when it isn't `#RRGGBB`).
    pub fn from_json(v: &Value) -> Option<ChatAuthor> {
        let parsed;
        let v = match v {
            Value::String(s) => {
                parsed = serde_json::from_str::<Value>(s).ok()?;
                &parsed
            }
            v => v,
        };
        let o = v.as_object()?;
        let field = |k: &str| o.get(k).and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string);
        let color = o.get("color").and_then(Value::as_str)?.to_string();
        Some(ChatAuthor { id: field("id")?, name: field("name")?, color })
    }
    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "name": self.name, "color": self.color})
    }
    /// One or two letters for the author's circle.
    pub fn initials(&self) -> String {
        let out: String = self.name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
        if out.is_empty() { "?".into() } else { out.to_uppercase() }
    }
}

/// `#RRGGBB`.
pub fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c.chars().skip(1).all(|ch| ch.is_ascii_hexdigit())
}

/// Length in UTF-16 code units (the unit of the 4,000 limit).
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// A message's text: 1–4,000 UTF-16 code units.
pub fn valid_text(s: &str) -> bool {
    (1..=MAX_TEXT).contains(&utf16_len(s))
}

/// One message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatMessage {
    /// 8 random lower-case base-36 characters, unique in the chat.
    pub id: String,
    pub author: ChatAuthor,
    /// The sender's clock when sent: milliseconds since 1970 UTC.
    pub at: i64,
    /// The message (empty once deleted).
    pub text: String,
    /// The id of the message it answers.
    pub reply_to: Option<String>,
    /// When the author last edited it.
    pub edited: Option<i64>,
    /// The author deleted it: the entry stays ("Message deleted") so replies still point somewhere.
    pub deleted: bool,
}

/// An integer from 0 to 2^53 (a whole float too, as some writers store numbers).
fn int(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    if let Some(i) = v.as_i64() {
        return (0..=MAX_AT).contains(&i).then_some(i);
    }
    let f = v.as_f64()?;
    (f.is_finite() && f.fract() == 0.0 && (0.0..=MAX_AT as f64).contains(&f)).then_some(f as i64)
}

impl ChatMessage {
    /// A message from its JSON (`author` an object or JSON text); `None` when it's malformed.
    pub fn from_json(v: &Value) -> Option<ChatMessage> {
        let o = v.as_object()?;
        let id = o.get("id").and_then(Value::as_str).filter(|s| valid_id(s))?;
        let author = ChatAuthor::from_json(o.get("author")?)?;
        let at = int(o.get("at"))?;
        let deleted = o.get("deleted").and_then(Value::as_bool) == Some(true);
        let text = if deleted { String::new() } else { o.get("text").and_then(Value::as_str).filter(|t| valid_text(t))?.to_string() };
        let reply_to = o.get("replyTo").and_then(Value::as_str).filter(|s| valid_id(s)).map(str::to_string);
        let edited = int(o.get("edited"));
        Some(ChatMessage { id: id.to_string(), author, at, text, reply_to, edited, deleted })
    }

    /// The message as the file part holds it (`author` an object).
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("id".into(), json!(self.id));
        o.insert("author".into(), self.author.to_json());
        o.insert("at".into(), json!(self.at));
        if !self.deleted {
            o.insert("text".into(), json!(self.text));
        }
        if let Some(r) = &self.reply_to {
            o.insert("replyTo".into(), json!(r));
        }
        if let Some(e) = self.edited {
            o.insert("edited".into(), json!(e));
        }
        if self.deleted {
            o.insert("deleted".into(), json!(true));
        }
        Value::Object(o)
    }
}

/// The chat: messages in order, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chat {
    /// When someone last cleared it.
    pub cleared_at: Option<i64>,
    pub messages: Vec<ChatMessage>,
}

impl Chat {
    /// No messages: nothing is written to the file.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Builds a chat from entries as they come (a file's `messages`, a room's array): malformed
    /// ones and repeated ids are skipped, and the first [`MAX_MESSAGES`] left are kept.
    pub fn from_entries<'a>(entries: impl IntoIterator<Item = &'a Value>, cleared_at: Option<i64>) -> Chat {
        let mut seen = std::collections::HashSet::new();
        let mut messages = Vec::new();
        for e in entries {
            if messages.len() >= MAX_MESSAGES {
                break;
            }
            if let Some(m) = ChatMessage::from_json(e)
                && seen.insert(m.id.clone())
            {
                messages.push(m);
            }
        }
        Chat { cleared_at, messages }
    }

    /// The chat from its JSON (`{version, clearedAt?, messages}`); `None` when it isn't one.
    pub fn from_json(v: &Value) -> Option<Chat> {
        let o = v.as_object()?;
        if o.get("version").is_some_and(|v| v.as_u64() != Some(VERSION)) {
            return None;
        }
        let messages = o.get("messages")?.as_array()?;
        Some(Chat::from_entries(messages, int(o.get("clearedAt"))))
    }

    /// The JSON the file part (and the native format) holds.
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("version".into(), json!(VERSION));
        if let Some(c) = self.cleared_at {
            o.insert("clearedAt".into(), json!(c));
        }
        o.insert("messages".into(), Value::Array(self.messages.iter().map(ChatMessage::to_json).collect()));
        Value::Object(o)
    }

    pub fn get(&self, id: &str) -> Option<&ChatMessage> {
        self.messages.iter().find(|m| m.id == id)
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.messages.iter().position(|m| m.id == id)
    }

    /// The chat as plain text, one message a paragraph: `[time] Name: text`, replies and edits
    /// noted. `time` formats a message's `at`.
    pub fn export_text(&self, time: &dyn Fn(i64) -> String) -> String {
        let mut out = String::new();
        for m in &self.messages {
            let mut line = format!("[{}] {}", time(m.at), m.author.name);
            if let Some(r) = m.reply_to.as_deref().and_then(|r| self.get(r)) {
                line.push_str(&format!(" (replying to {})", r.author.name));
            }
            line.push_str(": ");
            if m.deleted {
                line.push_str("Message deleted");
            } else {
                line.push_str(&m.text);
                if m.edited.is_some() {
                    line.push_str(" (edited)");
                }
            }
            out.push_str(&line);
            out.push('\n');
        }
        out
    }
}

impl Serialize for Chat {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Chat {
    /// Never fails: a chat that doesn't read is left out, like a malformed file part.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(Chat::from_json(&v).unwrap_or_default())
    }
}

/// For `skip_serializing_if` on the presentation's `chat`.
pub fn is_empty(c: &std::sync::Arc<Chat>) -> bool {
    c.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, text: &str) -> Value {
        json!({"id": id, "author": {"id": "u-1", "name": "Ann", "color": "#3B7DD8"}, "at": 1_700_000_000_000i64, "text": text})
    }

    #[test]
    fn reads_validates_and_round_trips() {
        let long = "x".repeat(MAX_TEXT + 1);
        let v = json!({"version": 1, "clearedAt": 5, "messages": [
            msg("aaaaaaaa", "hello"),
            msg("aaaaaaaa", "duplicate id"),
            msg("bbbbbbbb", ""),
            msg("cccccccc", &long),
            {"id": "dddddddd", "author": "not json", "at": 1, "text": "bad author"},
            {"id": "eeeeeeee", "author": {"id": "u", "name": "B", "color": "#000000"}, "at": "soon", "text": "bad at"},
            {"id": "eeeeeee2", "author": {"id": "u", "name": "B"}, "at": 1, "text": "no colour"},
            {"id": "EEEEEEEE", "author": {"id": "u", "name": "B", "color": "#000000"}, "at": 1, "text": "bad id"},
            {"id": "eeeeeee3", "author": {"id": "u", "name": "B", "color": "#000000"}, "at": 9007199254740993i64, "text": "beyond 2^53"},
            {"id": "ffffffff", "author": "{\"id\":\"u-2\",\"name\":\"Bo\",\"color\":\"#000000\"}", "at": 2.0, "text": "author as JSON text", "replyTo": "aaaaaaaa", "edited": 3},
            {"id": "gggggggg", "author": {"id": "u-1", "name": "Ann", "color": "#3B7DD8"}, "at": 4, "deleted": true, "text": "gone"},
            7,
        ]});
        let c = Chat::from_json(&v).unwrap();
        assert_eq!(c.cleared_at, Some(5));
        let ids: Vec<&str> = c.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["aaaaaaaa", "ffffffff", "gggggggg"]);
        assert_eq!(c.messages[1].author.name, "Bo");
        assert_eq!(c.messages[1].reply_to.as_deref(), Some("aaaaaaaa"));
        assert!(c.messages[2].deleted && c.messages[2].text.is_empty());
        let back = Chat::from_json(&c.to_json()).unwrap();
        assert_eq!(back, c);
        assert!(back.to_json()["messages"][2].get("text").is_none(), "a deleted message keeps no text");
        // Not a chat.
        assert!(Chat::from_json(&json!({"version": 2, "messages": []})).is_none());
        assert!(Chat::from_json(&json!([1, 2])).is_none());
        // Serde never fails on a bad chat.
        let c: Chat = serde_json::from_value(json!("junk")).unwrap();
        assert!(c.is_empty());
        // Emoji count as two UTF-16 units.
        assert!(valid_text(&"😀".repeat(MAX_TEXT / 2)));
        assert!(!valid_text(&"😀".repeat(MAX_TEXT / 2 + 1)));
    }

    #[test]
    fn the_first_ten_thousand_valid_messages_are_read() {
        let mut entries: Vec<Value> = vec![json!("junk"), msg("BADID", "x")];
        entries.extend((0..MAX_MESSAGES + 5).map(|i| msg(&format!("m{i:07}"), "hi")));
        let c = Chat::from_entries(&entries, None);
        assert_eq!(c.messages.len(), MAX_MESSAGES);
        assert_eq!(c.messages[0].id, "m0000000");
        assert_eq!(c.messages[MAX_MESSAGES - 1].id, format!("m{:07}", MAX_MESSAGES - 1));
    }

    #[test]
    fn exports_text() {
        let c = Chat::from_json(&json!({"messages": [msg("aaaaaaaa", "hello"), {"id": "bbbbbbbb", "author": {"id": "u-2", "name": "Bo", "color": "#000000"}, "at": 1, "text": "hi", "replyTo": "aaaaaaaa", "edited": 2}]})).unwrap();
        let t = c.export_text(&|at| at.to_string());
        assert_eq!(t, "[1700000000000] Ann: hello\n[1] Bo (replying to Ann): hi (edited)\n");
    }
}
