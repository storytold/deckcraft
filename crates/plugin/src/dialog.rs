//! Protocol 1.2 command results: `open` (a local file the app opens after the step) and `dialog`
//! (a dialog the app draws itself, whose buttons run more of the plugin's commands). Both come from
//! a plugin, so they are hostile input: every list and text is bounded (anything longer is cut),
//! and anything malformed is an error for the command, never a crash.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

/// Most items a list dialog shows.
pub const MAX_ITEMS: usize = 10_000;
/// Longest item id.
pub const MAX_ITEM_ID: usize = 4_096;
/// Longest item label and detail.
pub const MAX_ITEM_TEXT: usize = 256;
/// Most fields a form shows.
pub const MAX_FIELDS: usize = 16;
/// Most extra buttons, shown left of OK.
pub const MAX_BUTTONS: usize = 4;
/// Longest title (and `open.label`).
pub const MAX_TITLE: usize = 200;
/// Longest prompt (a message dialog's text).
pub const MAX_PROMPT: usize = 4_000;
/// Longest button, field and option label, and a field's text value.
const MAX_LABEL: usize = 200;
/// Most options a choice field offers.
const MAX_OPTIONS: usize = 1_000;
/// Longest path `open` names.
const MAX_PATH: usize = 4_096;
/// A list dialog filters by label above this many items.
pub const FILTER_ABOVE: usize = 20;

/// `open`: a local file the app opens after the command's step, as File › Open would.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub path: String,
    /// Shown where the app shows the document's location (at most 200 characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DialogKind {
    #[default]
    List,
    Form,
    Message,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub detail: String,
    /// One of the app's icon names (as for ribbon buttons); unknown names get a generic file icon.
    #[serde(default)]
    pub icon: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    #[default]
    Text,
    Secret,
    Number,
    Checkbox,
    Choice,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogField {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: FieldKind,
    /// The starting value: text for text, secret and number fields (a number as typed), `true` or
    /// `false` for a checkbox, an option id for a choice.
    #[serde(default)]
    pub value: Json,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ChoiceOption>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogButton {
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub params: Map<String, Json>,
}

/// `dialog`: what the app shows after the step. What the user does runs `command` (or a button's)
/// with `params` plus the selection or the form's fields.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialog {
    pub title: String,
    pub kind: DialogKind,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<DialogItem>,
    /// A list allows several items (`ids` instead of `id`).
    #[serde(default)]
    pub multi: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<DialogField>,
    /// What OK (or a double-click on an item) runs; a message may have none (OK just closes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub params: Map<String, Json>,
    pub ok_label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<DialogButton>,
}

/// `s` cut to `max` characters.
fn cut(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// An optional string field: absent or null is `None`, a string is cut to `max`, anything else is
/// malformed.
fn opt_str(o: &Map<String, Json>, key: &str, max: usize, what: &str) -> Result<Option<String>, String> {
    match o.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(cut(s, max))),
        Some(_) => Err(format!("{what}: `{key}` must be text")),
    }
}

fn req_str(o: &Map<String, Json>, key: &str, max: usize, what: &str) -> Result<String, String> {
    opt_str(o, key, max, what)?.ok_or_else(|| format!("{what}: `{key}` is missing"))
}

/// `params`: absent or null is empty, an object is kept, anything else is malformed.
fn params(o: &Map<String, Json>, what: &str) -> Result<Map<String, Json>, String> {
    match o.get("params") {
        None | Some(Json::Null) => Ok(Map::new()),
        Some(Json::Object(m)) => Ok(m.clone()),
        Some(_) => Err(format!("{what}: `params` must be an object")),
    }
}

fn object<'a>(v: &'a Json, what: &str) -> Result<&'a Map<String, Json>, String> {
    v.as_object().ok_or_else(|| format!("{what} must be an object"))
}

/// A command id the dialog runs (one of the plugin's: checked by the caller, who knows them).
fn command_id(o: &Map<String, Json>, what: &str) -> Result<Option<String>, String> {
    let c = opt_str(o, "command", 128, what)?;
    if let Some(c) = &c
        && !crate::valid_id(c)
    {
        return Err(format!("{what}: `command` `{c}` isn't a command id"));
    }
    Ok(c)
}

impl OpenRequest {
    /// Reads a reply's `open`. The file itself (absolute, existing, a regular file the app opens)
    /// is checked by the app.
    pub fn parse(v: &Json) -> Result<OpenRequest, String> {
        const WHAT: &str = "the add-in's `open`";
        let o = object(v, WHAT)?;
        let path = req_str(o, "path", MAX_PATH + 1, WHAT)?;
        if path.chars().count() > MAX_PATH || path.trim().is_empty() || path.contains('\0') {
            return Err(format!("{WHAT}: `path` must name a file"));
        }
        let read_only = match o.get("readOnly") {
            None | Some(Json::Null) => false,
            Some(Json::Bool(b)) => *b,
            Some(_) => return Err(format!("{WHAT}: `readOnly` must be true or false")),
        };
        Ok(OpenRequest { path, label: opt_str(o, "label", MAX_TITLE, WHAT)?.filter(|l| !l.trim().is_empty()), read_only })
    }
}

impl Dialog {
    /// Reads and bounds a reply's `dialog`.
    pub fn parse(v: &Json) -> Result<Dialog, String> {
        const WHAT: &str = "the add-in's dialog";
        let o = object(v, WHAT)?;
        let kind = match o.get("kind").and_then(Json::as_str) {
            Some("list") => DialogKind::List,
            Some("form") => DialogKind::Form,
            Some("message") => DialogKind::Message,
            Some(other) => return Err(format!("{WHAT}: `kind` `{}` isn't list, form or message", cut(other, 40))),
            None => return Err(format!("{WHAT}: `kind` (list, form or message) is missing")),
        };
        let command = command_id(o, WHAT)?;
        if command.is_none() && kind != DialogKind::Message {
            return Err(format!("{WHAT}: a {} needs a `command`", if kind == DialogKind::List { "list" } else { "form" }));
        }
        let mut d = Dialog {
            title: opt_str(o, "title", MAX_TITLE, WHAT)?.unwrap_or_default(),
            kind,
            prompt: opt_str(o, "prompt", MAX_PROMPT, WHAT)?.unwrap_or_default(),
            multi: matches!(o.get("multi"), Some(Json::Bool(true))),
            command,
            params: params(o, WHAT)?,
            ok_label: opt_str(o, "okLabel", MAX_LABEL, WHAT)?.filter(|l| !l.trim().is_empty()).unwrap_or_else(|| "OK".into()),
            ..Default::default()
        };
        match o.get("buttons") {
            None | Some(Json::Null) => {}
            Some(Json::Array(list)) => {
                for b in list.iter().take(MAX_BUTTONS) {
                    let bo = object(b, "a dialog button")?;
                    let command = command_id(bo, "a dialog button")?.ok_or("a dialog button needs a `command`")?;
                    d.buttons.push(DialogButton {
                        label: req_str(bo, "label", MAX_LABEL, "a dialog button")?,
                        command,
                        params: params(bo, "a dialog button")?,
                    });
                }
            }
            Some(_) => return Err(format!("{WHAT}: `buttons` must be a list")),
        }
        match kind {
            DialogKind::List => {
                let list = o.get("items").and_then(Json::as_array).ok_or_else(|| format!("{WHAT}: a list needs `items`"))?;
                d.items.reserve(list.len().min(MAX_ITEMS));
                for item in list.iter().take(MAX_ITEMS) {
                    let io = object(item, "a dialog item")?;
                    let id = match io.get("id") {
                        Some(Json::String(s)) => cut(s, MAX_ITEM_ID),
                        Some(Json::Number(n)) => n.to_string(),
                        _ => return Err("a dialog item needs an `id`".into()),
                    };
                    let label = opt_str(io, "label", MAX_ITEM_TEXT, "a dialog item")?.unwrap_or_else(|| cut(&id, MAX_ITEM_TEXT));
                    d.items.push(DialogItem {
                        id,
                        label,
                        detail: opt_str(io, "detail", MAX_ITEM_TEXT, "a dialog item")?.unwrap_or_default(),
                        icon: opt_str(io, "icon", 64, "a dialog item")?.unwrap_or_default(),
                    });
                }
            }
            DialogKind::Form => {
                let list = o.get("fields").and_then(Json::as_array).ok_or_else(|| format!("{WHAT}: a form needs `fields`"))?;
                for f in list.iter().take(MAX_FIELDS) {
                    d.fields.push(field(f)?);
                }
            }
            DialogKind::Message => {}
        }
        Ok(d)
    }

    /// The command ids the dialog can run (OK's and the buttons').
    pub fn commands(&self) -> impl Iterator<Item = &str> {
        self.command.as_deref().into_iter().chain(self.buttons.iter().map(|b| b.command.as_str()))
    }
}

fn field(f: &Json) -> Result<DialogField, String> {
    const WHAT: &str = "a form field";
    let o = object(f, WHAT)?;
    let id = req_str(o, "id", MAX_LABEL, WHAT)?;
    let label = opt_str(o, "label", MAX_LABEL, WHAT)?.unwrap_or_else(|| id.clone());
    let kind = match o.get("kind").and_then(Json::as_str) {
        None => FieldKind::Text,
        Some("text") => FieldKind::Text,
        Some("secret") => FieldKind::Secret,
        Some("number") => FieldKind::Number,
        Some("checkbox") => FieldKind::Checkbox,
        Some("choice") => FieldKind::Choice,
        Some(other) => return Err(format!("{WHAT} `{id}`: `kind` `{}` isn't text, secret, number, checkbox or choice", cut(other, 40))),
    };
    let mut options = Vec::new();
    if kind == FieldKind::Choice {
        let list = o.get("options").and_then(Json::as_array).ok_or_else(|| format!("{WHAT} `{id}`: a choice needs `options`"))?;
        for opt in list.iter().take(MAX_OPTIONS) {
            let oo = object(opt, "a choice option")?;
            let oid = req_str(oo, "id", MAX_LABEL, "a choice option")?;
            let olabel = opt_str(oo, "label", MAX_LABEL, "a choice option")?.unwrap_or_else(|| oid.clone());
            options.push(ChoiceOption { id: oid, label: olabel });
        }
    }
    let raw = o.get("value").unwrap_or(&Json::Null);
    let value = match kind {
        FieldKind::Checkbox => Json::Bool(match raw {
            Json::Bool(b) => *b,
            Json::Null => false,
            _ => return Err(format!("{WHAT} `{id}`: a checkbox's `value` is true or false")),
        }),
        FieldKind::Choice => {
            let want = raw.as_str();
            let chosen = options.iter().find(|op| Some(op.id.as_str()) == want).or(options.first());
            chosen.map(|op| Json::String(op.id.clone())).unwrap_or(Json::Null)
        }
        FieldKind::Number => match raw {
            Json::Null => Json::String(String::new()),
            Json::Number(n) => Json::String(n.to_string()),
            Json::String(s) => Json::String(cut(s, 64)),
            _ => return Err(format!("{WHAT} `{id}`: a number's `value` is a number")),
        },
        FieldKind::Text | FieldKind::Secret => match raw {
            Json::Null => Json::String(String::new()),
            Json::String(s) => Json::String(cut(s, MAX_PROMPT)),
            Json::Number(n) => Json::String(n.to_string()),
            Json::Bool(b) => Json::String(b.to_string()),
            _ => return Err(format!("{WHAT} `{id}`: `value` must be text")),
        },
    };
    Ok(DialogField { id, label, kind, value, options })
}

/// A typed number field's value as the plugin gets it: a JSON number, or `None` when the text
/// isn't one (empty is `null`).
pub fn number_value(text: &str) -> Option<Json> {
    let t = text.trim();
    if t.is_empty() {
        return Some(Json::Null);
    }
    let n: f64 = t.parse().ok().filter(|n: &f64| n.is_finite())?;
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        // A whole number goes as an integer (`3`, not `3.0`).
        return Some(Json::from(n as i64));
    }
    serde_json::Number::from_f64(n).map(Json::Number)
}

/// The params a dialog action runs its command with: the dialog's (or the button's) `params` plus,
/// for a list, the selection (`id`, or `ids` with `multi`; nothing when nothing is selected) and,
/// for a form, `fields`.
pub fn action_params(base: &Map<String, Json>, dialog: &Dialog, selected: &[String], fields: Option<Map<String, Json>>) -> Json {
    let mut p = base.clone();
    match dialog.kind {
        DialogKind::List if dialog.multi && !selected.is_empty() => {
            p.insert("ids".into(), Json::Array(selected.iter().cloned().map(Json::String).collect()));
        }
        DialogKind::List => {
            if let Some(id) = selected.first() {
                p.insert("id".into(), Json::String(id.clone()));
            }
        }
        DialogKind::Form => {
            p.insert("fields".into(), Json::Object(fields.unwrap_or_default()));
        }
        DialogKind::Message => {}
    }
    Json::Object(p)
}
