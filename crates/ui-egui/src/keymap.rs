//! Custom keyboard shortcuts (File › Settings › Keyboard Shortcuts, #127): the user's own keys for editor
//! commands and slide-show actions.
//!
//! Custom keys are an overlay on the built-in shortcuts (the command registry, the UI's own keys
//! in [`crate::menus::ui_shortcut`] and the slide show's keys): they are checked first. An action
//! with custom keys runs on those keys, and its built-in keys stop doing it. Actions the user never
//! changed keep their built-in keys, so the overlay is empty until someone customises a shortcut.
//!
//! Editor shortcuts need Cmd/Ctrl, Alt or a function key, so a plain letter can never take over
//! typing; slide-show keys can be any key. Everything is saved in `ui.json` with the rest of
//! [`crate::UiState`] and is reachable as commands (`keys.list`, `keys.set`, `keys.reset`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Most keys one action can have.
pub const MAX_KEYS: usize = 8;
/// Most customised actions kept (a hostile `ui.json` can't make every key press slow).
const MAX_ACTIONS: usize = 512;

/// A key combination: `Cmd+Shift+K`. `Cmd` is Ctrl on Windows and Linux.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Combo {
    pub cmd: bool,
    pub alt: bool,
    pub shift: bool,
    /// The key in [`deckcraft_engine::keys::key_token`] spelling: `k`, `f5`, `left`, `]`, `space`.
    pub key: String,
}

/// The token of an egui key, as [`Combo::key`] holds it.
fn token_of(key: egui::Key) -> String {
    deckcraft_engine::keys::key_token(key.name())
}

/// The egui key for a token, when there is one.
fn key_of(token: &str) -> Option<egui::Key> {
    egui::Key::ALL.iter().copied().find(|k| token_of(*k) == token)
}

impl Combo {
    /// Parses `Cmd+Shift+K`, `Ctrl+Alt+F5`, `Space`, `Cmd++`. Modifier names are `Cmd`, `Ctrl`,
    /// `Command`, `Alt`, `Option` and `Shift`, in any order and case; the key must be a key egui
    /// knows.
    pub fn parse(s: &str) -> Option<Combo> {
        let s = s.trim();
        if s.is_empty() || s.len() > 40 {
            return None;
        }
        let parts: Vec<&str> = s.split('+').collect();
        let (key, mods) = match parts.as_slice() {
            [mods @ .., "", ""] => ("+", mods),
            [mods @ .., key] => (*key, mods),
            [] => return None,
        };
        let mut c = Combo { cmd: false, alt: false, shift: false, key: String::new() };
        for m in mods {
            match m.trim().to_ascii_lowercase().as_str() {
                "cmd" | "ctrl" | "control" | "command" => c.cmd = true,
                "alt" | "option" => c.alt = true,
                "shift" => c.shift = true,
                _ => return None,
            }
        }
        let token = deckcraft_engine::keys::key_token(key.trim());
        key_of(&token)?;
        c.key = token;
        Some(c)
    }

    pub fn from_egui(key: egui::Key, m: egui::Modifiers) -> Combo {
        Combo { cmd: m.command || m.ctrl, alt: m.alt, shift: m.shift, key: token_of(key) }
    }

    /// The canonical spelling, as saved: `Cmd+Alt+Shift+K` (key names, never symbols).
    pub fn spec(&self) -> String {
        let key = key_of(&self.key).map(|k| k.name().to_string()).unwrap_or_else(|| self.key.clone());
        self.join("Cmd", "Alt", &key)
    }

    /// As the user reads it: `Ctrl+Shift+K` (`Cmd+Shift+K` on the Mac), arrows and punctuation as
    /// symbols.
    pub fn display(&self) -> String {
        let key = key_of(&self.key).map(|k| k.symbol_or_name().to_string()).unwrap_or_else(|| self.key.clone());
        if cfg!(target_os = "macos") { self.join("Cmd", "Option", &key) } else { self.join("Ctrl", "Alt", &key) }
    }

    fn join(&self, cmd: &str, alt: &str, key: &str) -> String {
        let mut s = String::new();
        for (on, name) in [(self.cmd, cmd), (self.alt, alt), (self.shift, "Shift")] {
            if on {
                s.push_str(name);
                s.push('+');
            }
        }
        s.push_str(key);
        s
    }

    fn is_function_key(&self) -> bool {
        self.key.strip_prefix('f').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    }

    /// Editor shortcuts need Cmd/Ctrl, Alt or a function key, so that typing stays typing.
    pub fn ok_for_editor(&self) -> bool {
        self.cmd || self.alt || self.is_function_key()
    }

    /// The egui key and modifiers this combo is pressed with.
    pub fn to_egui(&self) -> Option<(egui::Key, egui::Modifiers)> {
        let key = key_of(&self.key)?;
        Some((key, egui::Modifiers { alt: self.alt, ctrl: self.cmd, shift: self.shift, mac_cmd: false, command: self.cmd }))
    }
}

/// Where an action's keys work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The editor: slides, ribbon, panes.
    Editor,
    /// A running slide show.
    Show,
}

/// What a key does in a slide show. The ids are `show:` + [`ShowAction::id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowAction {
    Next,
    Previous,
    First,
    Last,
    End,
    Black,
    White,
    Pen,
    Erase,
}

impl ShowAction {
    pub const ALL: [ShowAction; 9] = [
        ShowAction::Next,
        ShowAction::Previous,
        ShowAction::First,
        ShowAction::Last,
        ShowAction::End,
        ShowAction::Black,
        ShowAction::White,
        ShowAction::Pen,
        ShowAction::Erase,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ShowAction::Next => "show:next",
            ShowAction::Previous => "show:previous",
            ShowAction::First => "show:first",
            ShowAction::Last => "show:last",
            ShowAction::End => "show:end",
            ShowAction::Black => "show:black",
            ShowAction::White => "show:white",
            ShowAction::Pen => "show:pen",
            ShowAction::Erase => "show:erase",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ShowAction::Next => "Next animation or slide",
            ShowAction::Previous => "Previous animation or slide",
            ShowAction::First => "First slide",
            ShowAction::Last => "Last slide",
            ShowAction::End => "End the show",
            ShowAction::Black => "Black screen",
            ShowAction::White => "White screen",
            ShowAction::Pen => "Pen on or off",
            ShowAction::Erase => "Erase the ink",
        }
    }

    /// The keys `show.rs` handles for it.
    fn defaults(self) -> &'static [&'static str] {
        match self {
            ShowAction::Next => &["Right", "Down", "Space", "PageDown", "N", "Enter"],
            ShowAction::Previous => &["Left", "Up", "Backspace", "PageUp", "P"],
            ShowAction::First => &["Home"],
            ShowAction::Last => &["End"],
            ShowAction::End => &["Escape", "-"],
            ShowAction::Black => &["B", "."],
            ShowAction::White => &["W", ","],
            ShowAction::Pen => &["Cmd+P"],
            ShowAction::Erase => &["E"],
        }
    }

    /// One of its built-in keys, which the show's key handling understands.
    pub fn canonical(self) -> (egui::Key, egui::Modifiers) {
        use egui::Key::*;
        match self {
            ShowAction::Next => (ArrowRight, egui::Modifiers::NONE),
            ShowAction::Previous => (ArrowLeft, egui::Modifiers::NONE),
            ShowAction::First => (Home, egui::Modifiers::NONE),
            ShowAction::Last => (End, egui::Modifiers::NONE),
            ShowAction::End => (Escape, egui::Modifiers::NONE),
            ShowAction::Black => (B, egui::Modifiers::NONE),
            ShowAction::White => (W, egui::Modifiers::NONE),
            ShowAction::Pen => (P, egui::Modifiers::COMMAND),
            ShowAction::Erase => (E, egui::Modifiers::NONE),
        }
    }

    fn from_id(id: &str) -> Option<ShowAction> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// Keys the UI handles in [`crate::menus::ui_shortcut`] for commands that don't declare them.
const UI_DEFAULTS: &[(&str, &str)] = &[
    ("app.saveAsDialog", "Cmd+Shift+S"),
    ("app.openDialog", "Cmd+O"),
    ("app.closePresentation", "Cmd+W"),
    ("view.fit", "Cmd+0"),
    ("app.backstage", "Alt+F"),
];

/// Something a shortcut can do.
#[derive(Clone, Debug)]
pub struct Action {
    pub id: String,
    pub label: String,
    /// The ribbon tab or menu it lives in (`Home`, `View`, `Slide Show`…).
    pub group: String,
    pub scope: Scope,
    pub defaults: Vec<Combo>,
}

/// Every action a key can be given: each command (the engine's and the UI's) and the slide-show
/// actions.
pub fn actions() -> Vec<Action> {
    let parse = |s: Option<&str>| s.and_then(Combo::parse).into_iter().collect::<Vec<_>>();
    let mut out: Vec<Action> = vec![];
    // The registry can hold an id twice (the first one is the command that runs); list it once.
    for a in deckcraft_engine::command_specs().iter().map(|c| Action {
        id: c.id.to_string(),
        label: c.label.to_string(),
        group: c.menu.first().copied().unwrap_or("Other").to_string(),
        scope: Scope::Editor,
        defaults: parse(c.shortcut),
    }) {
        if !out.iter().any(|o| o.id == a.id) {
            out.push(a);
        }
    }
    for (id, label, sc, _) in crate::UI_COMMANDS {
        if out.iter().any(|a| a.id == *id) || id.starts_with("keys.") {
            continue;
        }
        let mut defaults = parse(*sc);
        for k in UI_DEFAULTS.iter().filter(|(i, _)| i == id).filter_map(|(_, k)| Combo::parse(k)) {
            if !defaults.contains(&k) {
                defaults.push(k);
            }
        }
        let group = match id.split('.').next() {
            Some("view") => "View",
            Some("show") => "Slide Show",
            _ => "DeckCraft",
        };
        out.push(Action {
            id: id.to_string(), label: label.trim_end_matches('…').to_string(), group: group.into(), scope: Scope::Editor, defaults
        });
    }
    out.extend(ShowAction::ALL.iter().map(|a| Action {
        id: a.id().into(),
        label: a.label().into(),
        group: "Slide Show".into(),
        scope: Scope::Show,
        defaults: a.defaults().iter().filter_map(|k| Combo::parse(k)).collect(),
    }));
    out
}

/// What a key press means once custom keys are taken into account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A custom key: run this action.
    Run(String),
    /// A built-in key whose action now has other keys: do nothing.
    Swallow,
    /// Not affected: the built-in handling goes on.
    Pass,
}

/// The user's keys: action id → keys (spelled as [`Combo::spec`]), for the actions they changed.
/// An empty list turns an action's keys off.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Keymap(pub BTreeMap<String, Vec<String>>);

impl Keymap {
    /// The keys `id` has now: its custom keys, or its built-in ones.
    pub fn keys_of(&self, a: &Action) -> Vec<Combo> {
        match self.0.get(&a.id) {
            Some(keys) => keys.iter().filter_map(|k| Combo::parse(k)).collect(),
            None => a.defaults.clone(),
        }
    }

    pub fn is_custom(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    /// What `c` does in `scope`, given the custom keys.
    pub fn hit(&self, scope: Scope, c: &Combo) -> Hit {
        if self.0.is_empty() {
            return Hit::Pass;
        }
        let in_scope = |id: &str| (scope == Scope::Show) == id.starts_with("show:");
        let custom = self.0.iter().take(MAX_ACTIONS).filter(|(id, _)| in_scope(id));
        let mut changed = vec![];
        let mut run = None;
        for (id, keys) in custom {
            if run.is_none() && keys.iter().take(MAX_KEYS).filter_map(|k| Combo::parse(k)).any(|k| k == *c) {
                run = Some(id.clone());
            }
            changed.push(id.as_str());
        }
        if changed.is_empty() {
            return Hit::Pass;
        }
        let all = actions();
        let defaults_of = |id: &str| all.iter().find(|a| a.id == id).map(|a| a.defaults.as_slice()).unwrap_or_default();
        match run {
            // A built-in key the action kept goes on doing what it always did: Delete still deletes a
            // character while typing, not the shape.
            Some(id) if defaults_of(&id).contains(c) => Hit::Pass,
            Some(id) => Hit::Run(id),
            None if changed.iter().any(|id| defaults_of(id).contains(c)) => Hit::Swallow,
            None => Hit::Pass,
        }
    }

    /// The slide-show action for a key press, after custom keys: `Some` with the key and
    /// modifiers the show should act on, or `None` to ignore the press.
    pub fn show_key(&self, key: egui::Key, m: egui::Modifiers) -> Option<(egui::Key, egui::Modifiers)> {
        match self.hit(Scope::Show, &Combo::from_egui(key, m)) {
            Hit::Run(id) => ShowAction::from_id(&id).map(ShowAction::canonical),
            Hit::Swallow => None,
            Hit::Pass => Some((key, m)),
        }
    }

    /// Drops what a hostile or outdated `ui.json` could hold: unknown actions, unparseable keys,
    /// too many entries.
    pub fn sanitize(&mut self) {
        let all = actions();
        let mut kept = BTreeMap::new();
        for (id, keys) in std::mem::take(&mut self.0) {
            let Some(a) = all.iter().find(|a| a.id == id) else { continue };
            if kept.len() >= MAX_ACTIONS {
                break;
            }
            let mut ks: Vec<String> = vec![];
            for k in keys.iter().filter_map(|k| Combo::parse(k)) {
                if (a.scope == Scope::Show || k.ok_for_editor() || a.defaults.contains(&k)) && !ks.contains(&k.spec()) && ks.len() < MAX_KEYS {
                    ks.push(k.spec());
                }
            }
            kept.insert(id, ks);
        }
        self.0 = kept;
    }

    /// `keys.list`: every action with its keys.
    pub fn list(&self) -> Value {
        let rows: Vec<Value> = actions()
            .iter()
            .map(|a| {
                json!({
                    "id": a.id,
                    "label": a.label,
                    "group": a.group,
                    "scope": if a.scope == Scope::Show { "show" } else { "editor" },
                    "keys": self.keys_of(a).iter().map(Combo::spec).collect::<Vec<_>>(),
                    "defaults": a.defaults.iter().map(Combo::spec).collect::<Vec<_>>(),
                    "custom": self.is_custom(&a.id),
                })
            })
            .collect();
        Value::Array(rows)
    }

    /// `keys.set {action, keys, replace?}`: gives `action` exactly `keys`. A key another action
    /// already has is refused, unless `replace` moves it.
    pub fn set(&mut self, p: &Value) -> Result<Value, String> {
        let id = p.get("action").and_then(Value::as_str).ok_or("missing `action`")?;
        let all = actions();
        let a = all.iter().find(|a| a.id == id).ok_or_else(|| format!("unknown action `{id}`"))?;
        let list = p.get("keys").and_then(Value::as_array).ok_or("`keys` must be a list like [\"Cmd+Shift+K\"]")?;
        if list.len() > MAX_KEYS {
            return Err(format!("at most {MAX_KEYS} keys per action"));
        }
        let mut keys: Vec<Combo> = vec![];
        for k in list {
            let s = k.as_str().ok_or("each key must be text like \"Cmd+Shift+K\"")?;
            let c = Combo::parse(s).ok_or_else(|| format!("`{s}` isn't a key combination like \"Cmd+Shift+K\""))?;
            // The action's own built-in keys (plain Delete) may stay; new editor keys need a modifier.
            if a.scope == Scope::Editor && !c.ok_for_editor() && !a.defaults.contains(&c) {
                return Err(format!("{}: editor shortcuts need Ctrl, Alt or a function key", c.display()));
            }
            if !keys.contains(&c) {
                keys.push(c);
            }
        }
        let replace = p.get("replace").and_then(Value::as_bool).unwrap_or(false);
        // Other actions in the same scope that have one of these keys now.
        let clashes: Vec<(&Action, Combo)> = all
            .iter()
            .filter(|o| o.id != a.id && o.scope == a.scope)
            .flat_map(|o| self.keys_of(o).into_iter().filter(|k| keys.contains(k)).map(move |k| (o, k)))
            .collect();
        if let Some((o, k)) = clashes.first()
            && !replace
        {
            return Err(format!("{} is already used by {} ({}); pass replace: true to move it", k.display(), o.label, o.id));
        }
        for (o, k) in &clashes {
            let left: Vec<String> = self.keys_of(o).iter().filter(|x| *x != k).map(Combo::spec).collect();
            self.put(o, left);
        }
        self.put(a, keys.iter().map(Combo::spec).collect());
        Ok(
            json!({"action": a.id, "keys": self.keys_of(a).iter().map(Combo::spec).collect::<Vec<_>>(), "moved": clashes.iter().map(|(o, _)| o.id.clone()).collect::<Vec<_>>()}),
        )
    }

    /// Custom keys for `a`; back to built-in when they are the built-in ones.
    fn put(&mut self, a: &Action, keys: Vec<String>) {
        let defaults: Vec<String> = a.defaults.iter().map(Combo::spec).collect();
        if keys == defaults {
            self.0.remove(&a.id);
        } else {
            self.0.insert(a.id.clone(), keys);
        }
    }

    /// `keys.reset {action?}`: one action, or every one, back to its built-in keys.
    pub fn reset(&mut self, p: &Value) -> Result<Value, String> {
        match p.get("action").and_then(Value::as_str) {
            Some(id) => {
                if !actions().iter().any(|a| a.id == id) {
                    return Err(format!("unknown action `{id}`"));
                }
                self.0.remove(id);
            }
            None => self.0.clear(),
        }
        Ok(json!({"custom": self.0.len()}))
    }

    /// `keys.cheatSheet`: every action that has a key, the editor's then the slide show's, as
    /// plain text and as a page to print.
    pub fn cheat_sheet(&self) -> (String, String) {
        const TITLE: &str = "DeckCraft keyboard shortcuts";
        let mut text = format!("{TITLE}\n");
        let mut html = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>{TITLE}</title><style>\
             body{{font-family:system-ui,sans-serif;margin:32px;color:#222}}h1{{font-size:22px}}\
             h2{{font-size:16px;margin:24px 0 6px}}table{{border-collapse:collapse;width:100%;max-width:780px}}\
             td{{padding:4px 8px;border-bottom:1px solid #ddd;font-size:13px}}td.g{{color:#777}}td.k{{white-space:nowrap;text-align:right}}\
             kbd{{border:1px solid #bbb;border-radius:3px;padding:1px 5px;font-family:inherit;background:#f6f6f6}}\
             @media print{{body{{margin:12mm}}}}</style></head><body><h1>{TITLE}</h1>"
        );
        let mut all = actions();
        all.sort_by(|a, b| a.group.cmp(&b.group).then(a.label.cmp(&b.label)));
        for (scope, title) in [(Scope::Editor, "Editor"), (Scope::Show, "Slide Show")] {
            text.push_str(&format!("\n{title}\n"));
            html.push_str(&format!("<h2>{title}</h2><table>"));
            for a in all.iter().filter(|a| a.scope == scope) {
                let keys: Vec<String> = self.keys_of(a).iter().map(Combo::display).collect();
                if keys.is_empty() {
                    continue;
                }
                text.push_str(&format!("  {} ({}): {}\n", a.label, a.group, keys.join(", ")));
                let kbd: Vec<String> = keys.iter().map(|k| format!("<kbd>{}</kbd>", escape(k))).collect();
                html.push_str(&format!(
                    "<tr><td>{}</td><td class=\"g\">{}</td><td class=\"k\">{}</td></tr>",
                    escape(&a.label),
                    escape(&a.group),
                    kbd.join(" ")
                ));
            }
            html.push_str("</table>");
        }
        html.push_str("</body></html>\n");
        (text, html)
    }
}

/// Text made safe to put in an HTML page.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn c(s: &str) -> Combo {
        Combo::parse(s).unwrap()
    }

    #[test]
    fn combos_parse_and_print() {
        assert_eq!(c("Cmd+Shift+K"), Combo { cmd: true, alt: false, shift: true, key: "k".into() });
        assert_eq!(c("ctrl+k"), c("Cmd+K"), "Ctrl and Cmd are the same key");
        assert_eq!(c("Shift+Cmd+K").spec(), "Cmd+Shift+K", "one order when saved");
        assert_eq!(c("Cmd++").key, "+");
        assert_eq!(c("Alt+F9").spec(), "Alt+F9");
        assert_eq!(c("Cmd+]"), c("Cmd+CloseBracket"));
        assert_eq!(c("Left"), c("ArrowLeft"));
        for bad in ["", "Cmd+", "Hyper+K", "Cmd+NoSuchKey", "Cmd+KK", "é", &"Cmd+".repeat(20)] {
            assert_eq!(Combo::parse(bad), None, "{bad:?}");
        }
        for s in ["Cmd+Alt+Shift+K", "F5", "Space", "Cmd+,", "Alt+Left"] {
            assert_eq!(Combo::parse(&c(s).spec()), Some(c(s)), "{s} round-trips");
        }
    }

    #[test]
    fn editor_keys_must_not_take_over_typing() {
        assert!(c("Cmd+K").ok_for_editor() && c("Alt+K").ok_for_editor() && c("F7").ok_for_editor());
        assert!(!c("K").ok_for_editor() && !c("Shift+K").ok_for_editor() && !c("Space").ok_for_editor());
        let mut m = Keymap::default();
        let r = m.set(&json!({"action": "edit.copy", "keys": ["K"]}));
        assert!(r.unwrap_err().contains("Ctrl, Alt or a function key"));
        // Slide-show keys can be anything.
        m.set(&json!({"action": "show:next", "keys": ["X"]})).unwrap();
    }

    #[test]
    fn custom_keys_run_and_built_in_keys_stop() {
        let mut m = Keymap::default();
        assert_eq!(m.hit(Scope::Editor, &c("Cmd+C")), Hit::Pass, "nothing customised");
        m.set(&json!({"action": "edit.copy", "keys": ["Cmd+Alt+C"]})).unwrap();
        assert_eq!(m.hit(Scope::Editor, &c("Cmd+Alt+C")), Hit::Run("edit.copy".into()));
        assert_eq!(m.hit(Scope::Editor, &c("Cmd+C")), Hit::Swallow, "Ctrl+C no longer copies");
        assert_eq!(m.hit(Scope::Editor, &c("Cmd+V")), Hit::Pass, "other keys are untouched");
        assert_eq!(m.hit(Scope::Show, &c("Cmd+Alt+C")), Hit::Pass, "editor keys don't reach the show");

        m.set(&json!({"action": "show:next", "keys": ["X"]})).unwrap();
        assert_eq!(m.show_key(egui::Key::X, egui::Modifiers::NONE), Some(ShowAction::Next.canonical()));
        assert_eq!(m.show_key(egui::Key::Space, egui::Modifiers::NONE), None, "Space no longer advances");
        assert_eq!(m.show_key(egui::Key::B, egui::Modifiers::NONE), Some((egui::Key::B, egui::Modifiers::NONE)));
    }

    /// Keeping a built-in key while adding another leaves the built-in key's handling alone: plain
    /// Delete goes on deleting a character while typing.
    #[test]
    fn kept_built_in_keys_are_not_taken_over() {
        let mut m = Keymap::default();
        m.set(&json!({"action": "edit.delete", "keys": ["Delete", "Cmd+Alt+Shift+D"]})).unwrap();
        assert_eq!(m.hit(Scope::Editor, &c("Delete")), Hit::Pass);
        assert_eq!(m.hit(Scope::Editor, &c("Cmd+Alt+Shift+D")), Hit::Run("edit.delete".into()));
        m.set(&json!({"action": "show:next", "keys": ["Space", "X"]})).unwrap();
        assert_eq!(m.show_key(egui::Key::Space, egui::Modifiers::NONE), Some((egui::Key::Space, egui::Modifiers::NONE)));
    }

    #[test]
    fn a_used_key_moves_only_when_asked() {
        let mut m = Keymap::default();
        let err = m.set(&json!({"action": "edit.copy", "keys": ["Cmd+V"]})).unwrap_err();
        assert!(err.contains("edit.paste"), "{err}");
        assert!(m.0.is_empty(), "a refused change changes nothing");
        let r = m.set(&json!({"action": "edit.copy", "keys": ["Cmd+V"], "replace": true})).unwrap();
        assert_eq!(r["moved"], json!(["edit.paste"]));
        assert_eq!(m.0.get("edit.paste"), Some(&vec![]), "Paste lost the key");
        assert_eq!(m.hit(Scope::Editor, &c("Cmd+V")), Hit::Run("edit.copy".into()));
        // Setting the built-in keys again is the same as never changing them.
        m.set(&json!({"action": "edit.copy", "keys": ["Cmd+C"]})).unwrap();
        assert!(!m.is_custom("edit.copy"));
        m.reset(&json!({})).unwrap();
        assert!(m.0.is_empty());
    }

    #[test]
    fn bad_input_is_refused() {
        let mut m = Keymap::default();
        for bad in [
            json!({}),
            json!({"action": "no.such", "keys": []}),
            json!({"action": "edit.copy"}),
            json!({"action": "edit.copy", "keys": "Cmd+K"}),
            json!({"action": "edit.copy", "keys": [42]}),
            json!({"action": "edit.copy", "keys": ["Cmd+Nope"]}),
            json!({"action": "edit.copy", "keys": vec!["Cmd+K"; MAX_KEYS + 1]}),
        ] {
            assert!(m.set(&bad).is_err(), "{bad}");
        }
        assert!(m.0.is_empty());
        assert!(m.reset(&json!({"action": "no.such"})).is_err());
    }

    #[test]
    fn a_hostile_ui_json_is_cleaned() {
        let mut m: Keymap = serde_json::from_value(json!({
            "edit.copy": ["Cmd+Alt+C", "K", "garbage", "Cmd+Alt+C"],
            "no.such": ["Cmd+J"],
            "show:next": ["X"],
        }))
        .unwrap();
        m.sanitize();
        assert_eq!(m.0.get("edit.copy"), Some(&vec!["Cmd+Alt+C".to_string()]));
        assert!(!m.0.contains_key("no.such"));
        assert_eq!(m.0.get("show:next"), Some(&vec!["X".to_string()]));
    }

    #[test]
    fn every_action_is_listed_once() {
        let all = actions();
        let mut ids: Vec<&str> = all.iter().map(|a| a.id.as_str()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate action ids");
        assert!(all.iter().any(|a| a.id == "edit.copy" && a.defaults.contains(&c("Cmd+C"))));
        assert!(all.iter().any(|a| a.id == "app.openDialog" && a.defaults.contains(&c("Cmd+O"))));
        assert_eq!(all.iter().filter(|a| a.scope == Scope::Show).count(), ShowAction::ALL.len());
        for a in &all {
            let mut d = a.defaults.clone();
            d.sort();
            d.dedup();
            assert_eq!(d.len(), a.defaults.len(), "{} lists a key twice", a.id);
        }
    }
}
