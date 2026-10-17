//! DeckCraft's plugin host (add-ins).
//!
//! A plugin is a separate program, written in any language, described by a `plugin.json`
//! manifest. DeckCraft starts it on first use and talks JSON-RPC 2.0 with it over its standard
//! input and output, one message per line. A plugin contributes **commands** run through
//! `plugins.run`: the plugin answers with engine commands, which the engine executes as one
//! undoable step, and may ask the host for the presentation meanwhile (protocol 1.1, `host/*`).
//! A plugin that declares `macros` runs the presentation's macros (the Developer tab). Office
//! add-ins (`manifest.xml`) load as plugins whose commands open task panes ([`office`]).
//!
//! The manifest and protocol are the craft apps' shared ones (GridCraft, WordCraft): one plugin can
//! serve several apps. Spreadsheet functions in a shared manifest are accepted and ignored here.
//!
//! Protocol 2 adds the push channel ([`push`]): a running plugin's output is read all the time,
//! so a co-editing transport (manifest `collab`) can send whenever it likes, and the app sends it
//! notifications ([`Plugin::notify`]) between its requests.
//!
//! Manifests 1.3 add `requires` ([`requires`]): the app versions, protocol, host features and
//! other add-ins (with version ranges, [`version`]) an add-in needs. Every loaded add-in is
//! resolved together ([`refresh`]); one whose needs aren't met is blocked, with the reasons.
//!
//! A plugin runs in its own process, so a crash, a hang or a bad answer from it costs a command an
//! error, never the presentation. Calls time out; a plugin that keeps failing is stopped.
//! The protocol is documented in `docs/plugins.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod dialog;
pub mod install;
pub mod office;
mod process;
pub mod push;
pub mod requires;
pub mod version;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

pub use dialog::{Dialog, OpenRequest};
pub use process::{Connection, HostError, HostHandler, Notifier, RequestError};

/// The protocol version this host speaks (sent in `initialize`).
pub const PROTOCOL_VERSION: u32 = 1;
/// The plugin protocol this host speaks, as manifests' `requires.protocol` is checked against
/// (`initialize`'s `host.protocol`): protocol 2 (the push channel), with manifests 1.3.
pub const PROTOCOL: &str = "2.0";
/// What this host does beyond protocol 1.1, sent as `initialize`'s `host.features`: host-drawn
/// dialogs, opening files, document events and `context.document` (protocol 1.2); the push channel
/// and co-editing (protocol 2).
pub const HOST_FEATURES: &[&str] = &["dialogs", "open", "events", "document", "push", "collab"];
/// Co-editing's wire format, sent in `host.collab.formats` (protocol 2). The app's collab core
/// has the same constants; a test checks they agree.
pub const COLLAB_FORMAT: &str = "yjs-v1";
/// The shared document's schema (`host.collab.schema`).
pub const COLLAB_SCHEMA: u32 = 1;
/// The document events a manifest may ask for (`"events": [...]`, protocol 1.2).
pub const EVENTS: &[&str] = &["documentOpened", "documentSaved", "documentClosed"];
/// How long `events/documentOpened` and `events/documentSaved` may take.
pub const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `events/documentClosed` may take (the app doesn't wait longer while quitting).
pub const CLOSE_EVENT_TIMEOUT: Duration = Duration::from_secs(5);
/// This app's id in a manifest's `apps` and `uiFor`, and in `initialize`'s `host.id`. The
/// manifest and protocol are shared with the other craft apps (GridCraft, WordCraft), so one plugin can
/// serve several.
pub const HOST_ID: &str = "deckcraft";
/// The manifest's file name inside a plugin folder.
pub const MANIFEST: &str = "plugin.json";
/// Environment variable listing extra plugin folders (the platform's path-list separator).
pub const PATH_ENV: &str = "DECKCRAFT_PLUGIN_PATH";

const MAX_MANIFEST_BYTES: u64 = 1 << 20;
const MAX_DECLARATIONS: usize = 1_000;
/// How long a command or macro may take.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const INIT_TIMEOUT: Duration = Duration::from_secs(10);
/// A plugin that crashes or times out this many times is not started again until reloaded.
const MAX_FAILURES: u32 = 3;

// ---------------------------------------------------------------- manifest

/// `plugin.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Unique id, e.g. `org.example.finance` (letters, digits, `.`, `-`, `_`).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// The plugin's web page (documentation, support): an `https://` or `http://` URL.
    #[serde(default)]
    pub homepage: String,
    #[serde(default)]
    pub author: String,
    /// The apps the plugin is made for (`gridcraft`, `wordcraft`, …); empty: any.
    #[serde(default)]
    pub apps: Vec<String>,
    /// The program to start; empty for an Office add-in, whose commands open its task pane.
    #[serde(default)]
    pub run: RunSpec,
    /// Spreadsheet functions (GridCraft's): validated, not used by DeckCraft.
    #[serde(default)]
    pub functions: Vec<FunctionDecl>,
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
    /// Ribbon groups and menu items that run the plugin's commands: `uiFor.deckcraft` when the
    /// manifest has one, else `ui`.
    #[serde(default)]
    pub ui: UiDecl,
    /// Per-app ribbon groups and menu items (`{"gridcraft": {…}, "wordcraft": {…}}`), as written.
    #[serde(default)]
    pub ui_for: std::collections::BTreeMap<String, Json>,
    /// A scripting language the plugin runs macros in (protocol 1.1: `macros/*`), e.g. VBA.
    #[serde(default)]
    pub macros: Option<MacrosDecl>,
    /// The document events the plugin hears (protocol 1.2): `documentOpened`, `documentSaved`,
    /// `documentClosed`. Names this app doesn't send are kept and ignored.
    #[serde(default)]
    pub events: Vec<String>,
    /// A co-editing transport (protocol 2): `{"transport": "mqtt", "label": "Live (MQTT)"}`.
    /// Only such a plugin may join rooms.
    #[serde(default)]
    pub collab: Option<CollabDecl>,
    /// What the add-in needs (manifest 1.3): app versions, protocol, host features, other add-ins.
    #[serde(default, skip_serializing_if = "requires::Requires::is_empty")]
    pub requires: requires::Requires,
    /// An Office add-in (read from its `manifest.xml`): its task panes and permissions.
    #[serde(skip)]
    pub office: Option<office::OfficeInfo>,
}

/// A transport plugin's declaration (protocol 2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CollabDecl {
    /// The kind of transport (`mqtt`, `loopback`, `y-websocket`…).
    pub transport: String,
    /// What the Share menus call it.
    #[serde(default)]
    pub label: String,
}

/// A plugin that runs macros (`"macros": {"language": "VBA"}`): the Developer tab's Macros and
/// Visual Basic use it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MacrosDecl {
    pub language: String,
}

/// How to start the plugin. A `command` containing a path separator is relative to the plugin
/// folder; a bare name is looked up on `PATH`. The plugin runs in its folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDecl {
    /// The name formulas use, e.g. `ACME.PRICE`. Must not be a built-in function.
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Shown in the function list, e.g. `ACME.PRICE(ticker, [date])`.
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub min_args: usize,
    #[serde(default = "max_args_default")]
    pub max_args: usize,
    /// Recalculated on every recalculation (results are not cached).
    #[serde(default)]
    pub volatile: bool,
    /// How long a call may take (default 2 s, at most 30 s).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn max_args_default() -> usize {
    255
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDecl {
    /// Unique within the plugin.
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// Params documentation, like the engine's commands.
    #[serde(default)]
    pub params: String,
}

/// Where the plugin's commands appear in the app.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiDecl {
    /// Groups of buttons on a ribbon tab: an existing tab (`Home`, `Data`, …) or a new one.
    #[serde(default)]
    pub ribbon: Vec<RibbonGroupDecl>,
    /// Items added to a right-click menu.
    #[serde(default)]
    pub menus: Vec<MenuItemDecl>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RibbonGroupDecl {
    /// A built-in tab's name (English, as in [`RIBBON_TABS`]) adds the group at the end of that
    /// tab; any other name makes a new tab after View.
    pub tab: String,
    pub group: String,
    pub buttons: Vec<ButtonDecl>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonDecl {
    /// One of the plugin's `commands[].id`.
    pub command: String,
    /// Default: the command's label.
    #[serde(default)]
    pub label: Option<String>,
    /// Default: the command's description.
    #[serde(default)]
    pub tooltip: Option<String>,
    /// An icon name from the app's icon set (see `docs/plugins.md`); unknown names get a default.
    #[serde(default)]
    pub icon: Option<String>,
    /// `large` (default) or `small` (stacked three high).
    #[serde(default)]
    pub size: ButtonSize,
    /// Passed to the command.
    #[serde(default)]
    pub params: Json,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ButtonSize {
    #[default]
    Large,
    Small,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItemDecl {
    /// Which right-click menu: `shape` (default), `slide` (a thumbnail), `text` (selected text).
    #[serde(default)]
    pub menu: MenuKind,
    pub command: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub params: Json,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MenuKind {
    #[default]
    Shape,
    Slide,
    Text,
}

/// The built-in ribbon tabs a plugin can add groups to.
pub const RIBBON_TABS: &[&str] = &["Home", "Insert", "Draw", "Design", "Transitions", "Animations", "Slide Show", "Record", "Review", "View"];
/// Longest tab, group and label text.
const MAX_LABEL: usize = 64;

fn valid_label(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= MAX_LABEL
}

pub(crate) fn valid_id(s: &str) -> bool {
    // Starts with a letter or digit: an id is also the folder an installed plugin goes in.
    s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.len() <= 128
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn valid_function_name(s: &str) -> bool {
    s.len() <= 255
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'))
        && !s.ends_with('.')
}

impl Manifest {
    /// Parses and validates a manifest.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        Manifest::parse_with(text, false)
    }

    /// [`Manifest::parse`]; an Office add-in (`office`) has no program to run.
    pub(crate) fn parse_with(text: &str, office: bool) -> Result<Manifest, String> {
        let mut raw: Json = serde_json::from_str(text).map_err(|e| format!("{MANIFEST}: {e}"))?;
        // This app's own ribbon and menus replace `ui`; another app's `ui` (WordCraft's text menu)
        // would mean nothing here.
        if let Some(obj) = raw.as_object_mut()
            && let Some(own) = obj.get("uiFor").and_then(|u| u.get(HOST_ID)).cloned()
        {
            obj.insert("ui".into(), own);
        }
        let mut m: Manifest = serde_json::from_value(raw).map_err(|e| format!("{MANIFEST}: {e}"))?;
        if !m.for_this_app() {
            return Err(format!("{MANIFEST}: plugin {} is made for {}, not DeckCraft", m.id, m.apps.join(", ")));
        }
        m.validate(office)?;
        for f in &mut m.functions {
            f.name = f.name.to_ascii_uppercase();
        }
        Ok(m)
    }

    /// Whether `apps` names this app (or nothing).
    pub fn for_this_app(&self) -> bool {
        self.apps.is_empty() || self.apps.iter().any(|a| a.eq_ignore_ascii_case(HOST_ID))
    }

    fn validate(&self, office: bool) -> Result<(), String> {
        if !valid_id(&self.id) {
            return Err(format!("{MANIFEST}: id `{}` must be 1–128 letters, digits, `.`, `-` or `_`", self.id));
        }
        if self.name.trim().is_empty() {
            return Err(format!("{MANIFEST}: `name` is empty"));
        }
        let homepage = self.homepage.trim();
        if !homepage.is_empty()
            && (!(homepage.starts_with("https://") || homepage.starts_with("http://"))
                || homepage.len() > 2048
                || homepage.contains(char::is_whitespace))
        {
            return Err(format!("{MANIFEST}: `homepage` must be an http(s) URL"));
        }
        if !office && self.run.command.trim().is_empty() {
            return Err(format!("{MANIFEST}: `run.command` is empty"));
        }
        if self.functions.len() > MAX_DECLARATIONS || self.commands.len() > MAX_DECLARATIONS {
            return Err(format!("{MANIFEST}: at most {MAX_DECLARATIONS} functions and {MAX_DECLARATIONS} commands"));
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.functions {
            if !valid_function_name(&f.name) {
                return Err(format!("{MANIFEST}: function name `{}` must start with a letter and hold letters, digits, `.` or `_`", f.name));
            }
            if !seen.insert(f.name.to_ascii_uppercase()) {
                return Err(format!("{MANIFEST}: function `{}` is declared twice", f.name));
            }
            if f.min_args > f.max_args || f.max_args > 255 {
                return Err(format!("{MANIFEST}: function `{}` needs minArgs ≤ maxArgs ≤ 255", f.name));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for c in &self.commands {
            if !valid_id(&c.id) {
                return Err(format!("{MANIFEST}: command id `{}` must be letters, digits, `.`, `-` or `_`", c.id));
            }
            if !seen.insert(c.id.as_str()) {
                return Err(format!("{MANIFEST}: command `{}` is declared twice", c.id));
            }
        }
        if let Some(c) = &self.collab
            && (c.transport.trim().is_empty() || c.transport.len() > 64 || c.label.chars().count() > MAX_LABEL)
        {
            return Err(format!("{MANIFEST}: `collab.transport` needs 1–64 characters and `collab.label` at most {MAX_LABEL}"));
        }
        self.requires.validate(&self.id)?;
        if self.events.len() > 64 || self.events.iter().any(|e| e.is_empty() || e.len() > 64) {
            return Err(format!("{MANIFEST}: `events` holds at most 64 event names"));
        }
        let buttons: usize = self.ui.ribbon.iter().map(|g| g.buttons.len()).sum();
        if self.ui.ribbon.len() > MAX_DECLARATIONS || buttons > MAX_DECLARATIONS || self.ui.menus.len() > MAX_DECLARATIONS {
            return Err(format!("{MANIFEST}: at most {MAX_DECLARATIONS} ribbon groups, buttons and menu items"));
        }
        let known = |id: &str, what: &str| {
            if seen.contains(id) { Ok(()) } else { Err(format!("{MANIFEST}: {what} runs `{id}`, which isn't one of the plugin's commands")) }
        };
        for g in &self.ui.ribbon {
            if !valid_label(&g.tab) || !valid_label(&g.group) {
                return Err(format!("{MANIFEST}: ribbon `tab` and `group` need 1–{MAX_LABEL} characters"));
            }
            if g.buttons.is_empty() {
                return Err(format!("{MANIFEST}: ribbon group `{}` has no buttons", g.group));
            }
            for b in &g.buttons {
                known(&b.command, &format!("a button in ribbon group `{}`", g.group))?;
                if b.label.as_deref().is_some_and(|l| !valid_label(l)) {
                    return Err(format!("{MANIFEST}: button labels need 1–{MAX_LABEL} characters"));
                }
            }
        }
        for item in &self.ui.menus {
            known(&item.command, "a menu item")?;
            if item.label.as_deref().is_some_and(|l| !valid_label(l)) {
                return Err(format!("{MANIFEST}: menu item labels need 1–{MAX_LABEL} characters"));
            }
        }
        Ok(())
    }

    /// The command a button or menu item runs.
    pub fn command(&self, id: &str) -> Option<&CommandDecl> {
        self.commands.iter().find(|c| c.id == id)
    }
}

// ---------------------------------------------------------------- plugins

enum State {
    /// Not started yet, or stopped after a failure (started again on the next call).
    Idle,
    Running(Connection),
    /// Failed too often; not started again until the plugin is reloaded.
    Failed(String),
}

/// A loaded plugin.
pub struct Plugin {
    pub manifest: Manifest,
    pub dir: PathBuf,
    state: Mutex<(State, u32)>,
    /// Off: listed, but its commands and ribbon and menu items are gone.
    enabled: std::sync::atomic::AtomicBool,
    /// Where it was installed from (`.craft-source.json`), for plugins installed from GitHub.
    pub source: Option<Json>,
    /// Sends to the running process without its lock (protocol 2's notifications).
    notifier: Mutex<Option<Notifier>>,
    /// Why it can't run (manifest 1.3): empty unless blocked. Set by [`refresh`].
    blocked: Mutex<Vec<requires::Blocker>>,
    /// Stopped after failing too often (mirrors `State::Failed`, readable without the state lock).
    failed: std::sync::atomic::AtomicBool,
}

thread_local! {
    /// Plugins this thread is waiting for (see [`Plugin::request_with`]).
    static BUSY: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Plugin {
    fn new(manifest: Manifest, dir: PathBuf) -> Plugin {
        Plugin {
            manifest,
            state: Mutex::new((State::Idle, 0)),
            enabled: std::sync::atomic::AtomicBool::new(true),
            source: install::read_source(&dir),
            dir,
            notifier: Mutex::new(None),
            blocked: Mutex::new(Vec::new()),
            failed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Records that the plugin failed too often (its state is `Failed`).
    fn mark_failed(&self) {
        self.failed.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Stopped after failing too often, until reloaded or turned off and on.
    pub fn failed(&self) -> bool {
        self.failed.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Why it's blocked (manifest 1.3); empty when it isn't.
    pub fn blocked_by(&self) -> Vec<requires::Blocker> {
        lock(&self.blocked).clone()
    }

    pub fn is_blocked(&self) -> bool {
        !lock(&self.blocked).is_empty()
    }

    /// Turned on and not blocked: its commands, ribbon and menu items are there.
    pub fn active(&self) -> bool {
        self.enabled() && !self.is_blocked()
    }

    /// Refuses a blocked plugin, naming the first reason.
    fn check_blocked(&self) -> Result<(), String> {
        match lock(&self.blocked).first() {
            Some(b) => Err(format!("plugin {} is blocked: {}", self.manifest.id, b.message)),
            None => Ok(()),
        }
    }

    /// Starts the required add-ins that aren't running yet, before this one starts (they start
    /// first). Nothing to do when this one is already running or busy.
    fn start_requirements(&self) -> Result<(), String> {
        if self.manifest.requires.addins.is_empty() || !self.enabled() {
            return Ok(());
        }
        let idle = match self.state.try_lock() {
            Ok(g) => !matches!(g.0, State::Running(_)),
            Err(std::sync::TryLockError::Poisoned(g)) => !matches!(g.into_inner().0, State::Running(_)),
            Err(std::sync::TryLockError::WouldBlock) => false,
        };
        if !idle {
            return Ok(());
        }
        for r in self.manifest.requires.required() {
            let Some(dep) = get(&r.id) else { continue };
            if dep.manifest.office.is_some() || BUSY.with(|b| b.borrow().contains(&dep.manifest.id)) {
                continue;
            }
            dep.ensure_running().map_err(|e| format!("{} needs {}, which couldn't start: {e}", self.manifest.id, dep.manifest.name))?;
        }
        Ok(())
    }

    /// [`Plugin::request`], answering the plugin's `host/…` requests with `host` meanwhile.
    fn request_with(&self, method: &str, params: Json, timeout: Duration, host: &mut HostHandler<'_>) -> Result<Json, String> {
        // A host request of this plugin's can run a command that calls the plugin again: that
        // waits for nobody, it fails.
        if BUSY.with(|b| b.borrow().contains(&self.manifest.id)) {
            return Err(format!("plugin {} is busy", self.manifest.id));
        }
        self.check_blocked()?;
        self.start_requirements()?;
        let was_failed = self.failed();
        BUSY.with(|b| b.borrow_mut().push(self.manifest.id.clone()));
        let r = self.request_inner(method, params, timeout, host);
        BUSY.with(|b| {
            let mut b = b.borrow_mut();
            if let Some(i) = b.iter().rposition(|x| *x == self.manifest.id) {
                b.remove(i);
            }
        });
        // Failing for good blocks the add-ins that need this one.
        if self.failed() != was_failed {
            refresh();
        }
        r
    }

    fn request_inner(&self, method: &str, params: Json, timeout: Duration, host: &mut HostHandler<'_>) -> Result<Json, String> {
        if self.manifest.office.is_some() {
            return Err(format!("{} is an Office add-in: it runs in its task pane", self.manifest.id));
        }
        let mut guard = lock(&self.state);
        // Checked under the lock: turning off stores the flag, then stops the process under it.
        if !self.enabled() {
            return Err(format!("plugin {} is turned off", self.manifest.id));
        }
        self.check_blocked()?;
        let (state, failures) = &mut *guard;
        if let State::Failed(why) = state {
            return Err(why.clone());
        }
        if !matches!(state, State::Running(_)) {
            match self.start(failures) {
                Ok(conn) => {
                    *lock(&self.notifier) = conn.sender();
                    *state = State::Running(conn);
                }
                Err(e) => {
                    *state = if *failures >= MAX_FAILURES { State::Failed(format!("stopped after {failures} failures: {e}")) } else { State::Idle };
                    if *failures >= MAX_FAILURES {
                        self.mark_failed();
                    }
                    return Err(e);
                }
            }
        }
        let State::Running(conn) = state else { return Err("plugin not running".into()) };
        match conn.request_with(method, params, timeout, host) {
            Ok(v) => Ok(v),
            Err(e) if e.fatal => {
                // The connection is gone or out of step: stop the process and start afresh next time.
                *failures += 1;
                log::warn!("plugin {}: {} (failure {failures} of {MAX_FAILURES})", self.manifest.id, e.message);
                *state =
                    if *failures >= MAX_FAILURES { State::Failed(format!("stopped after {failures} failures: {}", e.message)) } else { State::Idle };
                if *failures >= MAX_FAILURES {
                    self.mark_failed();
                }
                *lock(&self.notifier) = None;
                Err(e.message)
            }
            Err(e) => Err(e.message),
        }
    }

    /// Starts the process and sends `initialize`. A failure counts.
    fn start(&self, failures: &mut u32) -> Result<Connection, String> {
        let mut conn = Connection::start(&self.manifest, &self.dir)?;
        let mut init = initialize_params(&self.manifest.id, &self.dir);
        // Manifest 1.3: every installed add-in, so this one can adapt to optional ones.
        init["addins"] = Json::Array(
            list().iter().map(|p| json!({"id": p.manifest.id, "version": p.manifest.version, "enabled": p.active() && !p.failed()})).collect(),
        );
        if let Err(e) = conn.request("initialize", init, INIT_TIMEOUT) {
            *failures += 1;
            return Err(e.message);
        }
        Ok(conn)
    }

    /// Starts the process when it isn't running (protocol 2: a transport runs while it's in a
    /// room, not only during calls).
    pub fn ensure_running(&self) -> Result<(), String> {
        if self.manifest.office.is_some() {
            return Err(format!("{} is an Office add-in: it runs in its task pane", self.manifest.id));
        }
        self.check_blocked()?;
        self.start_requirements()?;
        let was_failed = self.failed();
        let r = self.ensure_running_inner();
        if self.failed() != was_failed {
            refresh();
        }
        r
    }

    fn ensure_running_inner(&self) -> Result<(), String> {
        let mut guard = lock(&self.state);
        if !self.enabled() {
            return Err(format!("plugin {} is turned off", self.manifest.id));
        }
        let (state, failures) = &mut *guard;
        match state {
            State::Running(_) => Ok(()),
            State::Failed(why) => Err(why.clone()),
            State::Idle => match self.start(failures) {
                Ok(conn) => {
                    *lock(&self.notifier) = conn.sender();
                    *state = State::Running(conn);
                    Ok(())
                }
                Err(e) => {
                    if *failures >= MAX_FAILURES {
                        *state = State::Failed(format!("stopped after {failures} failures: {e}"));
                        self.mark_failed();
                    }
                    Err(e)
                }
            },
        }
    }

    /// Sends a notification (protocol 2), without waiting for a request in progress.
    pub fn notify(&self, method: &str, params: Json) -> Result<(), String> {
        let n = lock(&self.notifier).clone().ok_or_else(|| format!("plugin {} isn't running", self.manifest.id))?;
        n.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// Answers one of the plugin's own requests (protocol 2's *anytime* requests).
    pub fn respond(&self, id: &Json, answer: Result<Json, HostError>) -> Result<(), String> {
        let n = lock(&self.notifier).clone().ok_or_else(|| format!("plugin {} isn't running", self.manifest.id))?;
        let msg = match answer {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": e.code, "message": e.message, "data": e.data}}),
        };
        n.send(&msg)
    }

    /// Records a failure that happened outside a request (the process exited, a message was too
    /// long). The next use starts it again, until it failed too often.
    pub fn count_failure(&self, why: &str) {
        let was_failed = self.failed();
        self.count_failure_inner(why);
        if self.failed() != was_failed {
            refresh();
        }
    }

    fn count_failure_inner(&self, why: &str) {
        let mut guard = lock(&self.state);
        let (state, failures) = &mut *guard;
        *failures += 1;
        log::warn!("plugin {}: {why} (failure {failures} of {MAX_FAILURES})", self.manifest.id);
        if *failures >= MAX_FAILURES {
            self.mark_failed();
            if let State::Running(conn) = std::mem::replace(state, State::Failed(format!("stopped after {failures} failures: {why}"))) {
                conn.shutdown();
            }
            *lock(&self.notifier) = None;
        }
    }

    /// Stops the plugin and marks it failed (it flooded the app).
    pub fn fail(&self, why: &str) {
        let was_failed = self.failed();
        {
            let mut guard = lock(&self.state);
            if let State::Running(conn) = std::mem::replace(&mut guard.0, State::Failed(why.to_string())) {
                conn.shutdown();
            }
            self.mark_failed();
            *lock(&self.notifier) = None;
        }
        log::warn!("plugin {}: {why}", self.manifest.id);
        if !was_failed {
            refresh();
        }
    }

    /// The process exited on its own: it's started again on next use.
    pub fn exited(&self) {
        let mut guard = lock(&self.state);
        if matches!(guard.0, State::Running(_)) {
            guard.0 = State::Idle;
        }
        *lock(&self.notifier) = None;
    }

    /// A co-editing transport (its manifest has `collab`).
    pub fn is_transport(&self) -> bool {
        self.manifest.collab.is_some() && self.manifest.office.is_none()
    }

    /// Whether the plugin declared `event` in its manifest's `events`.
    pub fn hears(&self, event: &str) -> bool {
        self.manifest.office.is_none() && self.manifest.events.iter().any(|e| e == event)
    }

    /// False once turned off with [`set_enabled`].
    pub fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// `ready` (not started), `running`, or `failed: …`.
    pub fn status(&self) -> String {
        if !self.enabled() {
            return "off".into();
        }
        if let Some(b) = lock(&self.blocked).first() {
            return format!("blocked: {}", b.message);
        }
        match &lock(&self.state).0 {
            State::Idle => "ready".into(),
            State::Running(_) => "running".into(),
            State::Failed(why) => format!("failed: {why}"),
        }
    }

    /// `disabled` (turned off), `blocked` (manifest 1.3), `failed`, `running` or `loaded` (not
    /// started yet).
    pub fn state(&self) -> &'static str {
        if !self.enabled() {
            "disabled"
        } else if self.is_blocked() {
            "blocked"
        } else if self.failed() {
            "failed"
        } else {
            match self.state.try_lock() {
                Ok(g) if !matches!(g.0, State::Running(_)) => "loaded",
                Err(std::sync::TryLockError::Poisoned(g)) if !matches!(g.get_ref().0, State::Running(_)) => "loaded",
                // Busy in a request: running.
                _ => "running",
            }
        }
    }

    /// The plugin as `plugins.list` reports it.
    pub fn info(&self) -> Json {
        let m = &self.manifest;
        json!({
            "state": self.state(),
            "blockedBy": self.blocked_by(),
            "requires": m.requires,
            "requiredBy": required_by(&m.id),
            "id": m.id,
            "name": m.name,
            "version": m.version,
            "description": m.description,
            "homepage": m.homepage,
            "author": m.author,
            "apps": m.apps,
            "enabled": self.enabled(),
            "dir": self.dir.display().to_string(),
            "source": self.source.clone().unwrap_or(Json::Null),
            "status": self.status(),
            "commands": m.commands.iter().map(|c| json!({
                "id": c.id, "label": c.label, "description": c.description, "params": c.params,
            })).collect::<Vec<_>>(),
            "ui": m.ui,
            "events": m.events,
            "collab": m.collab,
            "kind": if m.office.is_some() { "office" } else { "native" },
            "office": m.office.as_ref().map(|o| json!({
                "id": o.office_id, "permissions": o.permissions, "sourceLocation": o.source_location,
                "requirements": o.requirements, "methods": o.methods, "iconUrl": o.icon_url,
            })),
        })
    }

    fn stop(&self) {
        *lock(&self.notifier) = None;
        let mut guard = lock(&self.state);
        if let State::Running(conn) = std::mem::replace(&mut guard.0, State::Idle) {
            conn.shutdown();
        }
    }
}

/// `initialize`'s params: the protocol version, the host (with its `protocol`, `features`, UI
/// `language` and, for protocol 2, what co-editing it speaks) and the plugin. Starting a plugin
/// adds `addins` (manifest 1.3).
pub fn initialize_params(plugin_id: &str, dir: &Path) -> Json {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "host": {
            "id": HOST_ID, "name": "DeckCraft", "version": env!("CARGO_PKG_VERSION"), "protocol": PROTOCOL,
            "features": HOST_FEATURES, "language": host_language(),
            "collab": {"formats": [COLLAB_FORMAT], "app": HOST_ID, "schema": COLLAB_SCHEMA, "encryption": ["aes-256-gcm"]},
        },
        "plugin": {"id": plugin_id, "dir": dir.display().to_string()},
    })
}

fn language_slot() -> &'static Mutex<String> {
    static LANGUAGE: OnceLock<Mutex<String>> = OnceLock::new();
    LANGUAGE.get_or_init(|| Mutex::new("en".into()))
}

/// The UI language sent as `host.language` (a BCP 47 tag; DeckCraft's interface is English: `en`).
pub fn host_language() -> String {
    lock(language_slot()).clone()
}

/// Sets the UI language plugins started from now on are told (`host.language`). Anything that
/// isn't a plausible BCP 47 tag is ignored.
pub fn set_host_language(tag: &str) {
    let ok = !tag.is_empty() && tag.len() <= 35 && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        *lock(language_slot()) = tag.to_string();
    }
}

// ---------------------------------------------------------------- the registry

/// Plugins are application-wide, like PowerPoint's add-ins: one set per process.
fn loaded() -> &'static Mutex<Vec<Arc<Plugin>>> {
    static LOADED: OnceLock<Mutex<Vec<Arc<Plugin>>>> = OnceLock::new();
    LOADED.get_or_init(|| Mutex::new(Vec::new()))
}

/// The plugin with this id.
pub fn get(id: &str) -> Option<Arc<Plugin>> {
    lock(loaded()).iter().find(|p| p.manifest.id == id).cloned()
}

/// Every loaded plugin, in load order.
pub fn list() -> Vec<Arc<Plugin>> {
    lock(loaded()).clone()
}

/// Loads the plugin in `path` (a folder holding `plugin.json`, or the manifest itself). A plugin
/// with the same id is unloaded first. The process starts on first use.
pub fn load(path: &Path) -> Result<Arc<Plugin>, String> {
    let (manifest, dir) = read_manifest(path)?;
    install(manifest, dir)
}

/// Reads the manifest in `path` (a folder holding `plugin.json` or `manifest.xml`, or the
/// manifest itself) without loading it. Returns it and the plugin's folder.
pub fn read_manifest(path: &Path) -> Result<(Manifest, PathBuf), String> {
    let (dir, file) = if path.is_dir() {
        // An Office add-in's folder holds `manifest.xml` instead of `plugin.json`.
        let office = path.join(office::MANIFEST_FILE);
        let file = if !path.join(MANIFEST).is_file() && office.is_file() { office } else { path.join(MANIFEST) };
        (path.to_path_buf(), file)
    } else {
        (path.parent().unwrap_or(Path::new(".")).to_path_buf(), path.to_path_buf())
    };
    let len = std::fs::metadata(&file).map_err(|e| format!("{}: {e}", file.display()))?.len();
    if len > MAX_MANIFEST_BYTES {
        return Err(format!("{}: manifest larger than 1 MiB", file.display()));
    }
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let is_office = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml"));
    let manifest = if is_office { office::parse_manifest(&text, HOST_ID)? } else { Manifest::parse(&text)? };
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    Ok((manifest, dir))
}

/// Installs a parsed manifest (tests and [`load`]).
pub fn install(manifest: Manifest, dir: PathBuf) -> Result<Arc<Plugin>, String> {
    if get(&manifest.id).is_some() {
        remove(&manifest.id);
    }
    let plugin = Arc::new(Plugin::new(manifest, dir));
    lock(loaded()).push(plugin.clone());
    log::info!("plugin {} {} loaded from {}", plugin.manifest.id, plugin.manifest.version, plugin.dir.display());
    // Its requirements, and those of the add-ins that need it, are resolved again.
    refresh();
    Ok(plugin)
}

// ---------------------------------------------------------------- requirements (manifest 1.3)

fn resolution() -> &'static Mutex<requires::Resolution> {
    static RESOLUTION: OnceLock<Mutex<requires::Resolution>> = OnceLock::new();
    RESOLUTION.get_or_init(|| Mutex::new(requires::Resolution::default()))
}

/// This app as requirements see it: its version, protocol, features and the Office.js API sets.
pub fn host_info() -> requires::HostInfo {
    requires::HostInfo {
        id: HOST_ID.into(),
        name: "DeckCraft".into(),
        version: version::Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(version::Version::new(0, 0, 0)),
        protocol: version::Version::parse(PROTOCOL).unwrap_or(version::Version::new(1, 0, 0)),
        features: HOST_FEATURES.iter().map(|f| f.to_string()).collect(),
        api_sets: office::API_SETS.iter().map(|(n, (major, minor))| (n.to_string(), version::Version::new(*major, *minor, 0))).collect(),
        methods: office::API_METHODS.iter().map(|m| m.to_string()).collect(),
    }
}

/// The loaded plugins as [`requires::resolve`] sees them.
pub fn nodes() -> Vec<requires::Node> {
    list().iter().map(|p| node_of(p)).collect()
}

fn node_of(p: &Plugin) -> requires::Node {
    requires::Node {
        id: p.manifest.id.clone(),
        name: p.manifest.name.clone(),
        version: p.manifest.version.clone(),
        enabled: p.enabled(),
        failed: p.failed(),
        requires: p.manifest.requires.clone(),
        office: p.manifest.office.as_ref().map(|o| requires::OfficeNeeds { sets: o.requirements.clone(), methods: o.methods.clone() }),
    }
}

/// Resolves every loaded plugin's requirements together and applies the outcome: blocked
/// plugins lose their commands, ribbon and menu items (and their process stops), unblocked ones
/// get them back. Runs whenever plugins arrive, leave or turn on or off, and when one fails for
/// good.
pub fn refresh() -> requires::Resolution {
    // One resolution at a time, from snapshot to applying it: two threads refreshing at once would
    // otherwise let the older snapshot win.
    static REFRESHING: Mutex<()> = Mutex::new(());
    let _one = lock(&REFRESHING);
    let plugins = list();
    let nodes: Vec<requires::Node> = plugins.iter().map(|p| node_of(p)).collect();
    let r = requires::resolve(&nodes, &host_info());
    for p in &plugins {
        let now = r.blocked.get(&p.manifest.id).cloned().unwrap_or_default();
        let was_blocked = {
            let mut b = lock(&p.blocked);
            let was = !b.is_empty();
            *b = now;
            was
        };
        let blocked = p.is_blocked();
        if blocked && !was_blocked {
            log::info!("plugin {} is blocked: {}", p.manifest.id, p.blocked_by().first().map(|b| b.message.as_str()).unwrap_or(""));
        }
        if blocked {
            // Not while it's answering (this may run from inside one of its requests).
            if let Ok(mut guard) = p.state.try_lock() {
                *lock(&p.notifier) = None;
                if let State::Running(conn) = std::mem::replace(&mut guard.0, State::Idle) {
                    conn.shutdown();
                }
            }
        }
    }
    *lock(resolution()) = r.clone();
    r
}

/// The loaded plugins that require `id` (not optionally).
pub fn required_by(id: &str) -> Vec<String> {
    lock(resolution()).required_by.get(id).cloned().unwrap_or_default()
}

/// The plugins in start order: required ones before those that need them.
pub fn start_order() -> Vec<String> {
    lock(resolution()).order.clone()
}

/// The turned-on plugins that need `id`, directly or through others (what turning it off or
/// removing it would block), in start order.
pub fn dependents(id: &str) -> Vec<String> {
    let r = lock(resolution()).clone();
    let mut out: Vec<String> = Vec::new();
    let mut todo = vec![id.to_string()];
    while let Some(x) = todo.pop() {
        for u in r.required_by.get(&x).into_iter().flatten() {
            if u != id && !out.contains(u) && get(u).is_some_and(|p| p.enabled()) {
                out.push(u.clone());
                todo.push(u.clone());
            }
        }
        if out.len() > 10_000 {
            break;
        }
    }
    out.sort_by_key(|x| r.order.iter().position(|o| o == x));
    out
}

/// Installing `manifest` would put it out of range for these turned-on plugins that require it.
pub fn conflicts(manifest: &Manifest) -> Vec<requires::Conflict> {
    requires::conflicts(&nodes(), &manifest.id, &manifest.name, &manifest.version)
}

/// `id`'s requirements that are missing or out of range (optional ones too).
pub fn missing(id: &str) -> Vec<requires::Missing> {
    let all = nodes();
    all.iter().find(|n| n.id == id).map(|n| requires::missing(n, &all)).unwrap_or_default()
}

/// `plugins.requirements`: whether `id`'s requirements are met, and the tree of the add-ins it
/// needs (`{id, need, have, state, optional, children}`, `state` `missing` for one that isn't
/// installed). Bounded by [`requires::MAX_DEPTH`] and cycles.
pub fn requirements(id: &str) -> Result<Json, String> {
    let p = get(id).ok_or_else(|| format!("no plugin `{id}` is loaded"))?;
    fn tree(p: &Plugin, seen: &mut Vec<String>, depth: usize) -> Vec<Json> {
        p.manifest
            .requires
            .addins
            .iter()
            .map(|r| {
                let dep = get(&r.id);
                let mut node = json!({
                    "id": r.id, "name": dep.as_ref().map(|d| d.manifest.name.clone()).unwrap_or_else(|| r.label()),
                    "need": r.need(), "have": dep.as_ref().map(|d| d.manifest.version.clone()),
                    "state": dep.as_ref().map_or("missing", |d| d.state()), "optional": r.optional, "why": r.why,
                    "github": r.github, "children": [],
                });
                if let Some(d) = dep
                    && depth < requires::MAX_DEPTH
                    && !seen.contains(&r.id)
                {
                    seen.push(r.id.clone());
                    node["children"] = Json::Array(tree(&d, seen, depth + 1));
                    seen.pop();
                }
                node
            })
            .collect()
    }
    let mut seen = vec![id.to_string()];
    Ok(json!({
        "id": id,
        "satisfied": !p.is_blocked(),
        "blockedBy": p.blocked_by(),
        "tree": tree(&p, &mut seen, 0),
    }))
}

/// Turns a loaded plugin off or back on. Off, it stays listed but its commands are refused and its
/// process is stopped. Returns whether anything changed.
pub fn set_enabled(id: &str, on: bool) -> Result<bool, String> {
    let plugin = get(id).ok_or_else(|| format!("no plugin `{id}` is loaded"))?;
    if plugin.enabled() == on {
        return Ok(false);
    }
    if on {
        // A plugin that failed too often gets a fresh start.
        *lock(&plugin.state) = (State::Idle, 0);
        plugin.failed.store(false, std::sync::atomic::Ordering::Relaxed);
        plugin.enabled.store(true, std::sync::atomic::Ordering::Relaxed);
    } else {
        plugin.enabled.store(false, std::sync::atomic::Ordering::Relaxed);
        plugin.stop();
    }
    log::info!("plugin {id} turned {}", if on { "on" } else { "off" });
    // Blocks or unblocks the plugins that need it.
    refresh();
    Ok(true)
}

/// Unloads a plugin: its commands disappear and its process is asked to exit. True when it was loaded.
pub fn unload(id: &str) -> bool {
    let removed = remove(id);
    if removed {
        // The plugins that needed it are blocked now.
        refresh();
    }
    removed
}

/// Takes a plugin out of the list and stops it (no resolution).
fn remove(id: &str) -> bool {
    let plugin = {
        let mut all = lock(loaded());
        let Some(i) = all.iter().position(|p| p.manifest.id == id) else { return false };
        all.remove(i)
    };
    plugin.stop();
    true
}

/// Plugin folders to load at startup: each folder in [`PATH_ENV`], then `extra`. A folder holding
/// `plugin.json` is one plugin; otherwise each of its subfolders holding one is.
pub fn discover(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = std::env::var_os(PATH_ENV).map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
    roots.extend(extra.iter().cloned());
    let mut out = Vec::new();
    for root in roots {
        let is_plugin = |p: &Path| p.join(MANIFEST).is_file() || p.join(office::MANIFEST_FILE).is_file();
        if is_plugin(&root) {
            out.push(root);
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        let mut dirs: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| is_plugin(p)).collect();
        dirs.sort();
        out.extend(dirs);
    }
    out
}

/// Loads every plugin [`discover`] finds; failures are returned (and logged), not fatal.
pub fn autoload(extra: &[PathBuf]) -> Vec<(PathBuf, Result<String, String>)> {
    discover(extra)
        .into_iter()
        .map(|dir| {
            let r = load(&dir).map(|p| p.manifest.id.clone());
            if let Err(e) = &r {
                log::warn!("plugin {}: {e}", dir.display());
            }
            (dir, r)
        })
        .collect()
}

// ---------------------------------------------------------------- commands

/// What a plugin command asks the engine to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandReply {
    /// Engine commands to run, in order: `(id, params)`.
    pub commands: Vec<(String, Json)>,
    /// A message to show the user.
    pub message: Option<String>,
    /// Anything else the plugin returned (handed back to the caller).
    pub result: Json,
    /// An Office add-in's command: what the app opens or calls (no process is involved).
    pub office: Option<office::OfficeCommand>,
    /// A local file to open after the step (protocol 1.2).
    pub open: Option<OpenRequest>,
    /// A dialog to show after the step (protocol 1.2).
    pub dialog: Option<Dialog>,
    /// `collab.join`: a room to join or start (protocol 2), as the plugin gave it.
    pub collab_join: Option<Json>,
}

/// Most engine commands one plugin command may run.
pub const MAX_REPLY_COMMANDS: usize = 10_000;

/// Runs one of a plugin's commands. `context` describes the presentation (slide, selection, text).
pub fn run_command(plugin_id: &str, command: &str, params: Json, context: Json) -> Result<CommandReply, String> {
    run_command_with(plugin_id, command, params, context, &mut |_, _| Err(HostError::unknown()))
}

/// [`run_command`], answering the plugin's `host/…` requests with `host` while it runs.
pub fn run_command_with(plugin_id: &str, command: &str, params: Json, context: Json, host: &mut HostHandler<'_>) -> Result<CommandReply, String> {
    let plugin = get(plugin_id).ok_or_else(|| format!("no plugin `{plugin_id}` is loaded"))?;
    if !plugin.enabled() {
        return Err(format!("plugin {plugin_id} is turned off (File › Options › Add-ins)"));
    }
    plugin.check_blocked()?;
    if !plugin.manifest.commands.iter().any(|c| c.id == command) {
        return Err(format!("plugin {plugin_id} has no command `{command}`"));
    }
    if let Some(o) = &plugin.manifest.office {
        let what = o.commands.get(command).cloned().ok_or_else(|| format!("plugin {plugin_id} has no command `{command}`"))?;
        return Ok(CommandReply { office: Some(what), ..Default::default() });
    }
    let r = plugin.request_with("commands/run", json!({"command": command, "params": params, "context": context}), COMMAND_TIMEOUT, host)?;
    parse_reply(r)
}

/// Sends a macro plugin one of its `macros/…` requests (protocol 1.1), answering its `host/…`
/// requests with `host` meanwhile.
pub fn call_macros(plugin_id: &str, method: &str, params: Json, host: &mut HostHandler<'_>) -> Result<Json, String> {
    let plugin = get(plugin_id).ok_or_else(|| format!("no plugin `{plugin_id}` is loaded"))?;
    if !plugin.enabled() {
        return Err(format!("plugin {plugin_id} is turned off (File › Options › Add-ins)"));
    }
    plugin.check_blocked()?;
    if plugin.manifest.macros.is_none() || !method.starts_with("macros/") {
        return Err(format!("plugin {plugin_id} doesn't run macros"));
    }
    plugin.request_with(method, params, COMMAND_TIMEOUT, host)
}

/// The active (on, not blocked) co-editing transports (protocol 2), in load order.
pub fn transports() -> Vec<Arc<Plugin>> {
    list().into_iter().filter(|p| p.active() && p.is_transport()).collect()
}

/// The active plugins that run macros, with their language.
pub fn macro_plugins() -> Vec<(String, String)> {
    list()
        .into_iter()
        .filter(|p| p.active())
        .filter_map(|p| p.manifest.macros.as_ref().map(|m| (p.manifest.id.clone(), m.language.clone())))
        .collect()
}

fn parse_reply(r: Json) -> Result<CommandReply, String> {
    let mut reply = CommandReply {
        message: r.get("message").and_then(Json::as_str).map(str::to_string),
        result: r.get("result").cloned().unwrap_or(Json::Null),
        ..Default::default()
    };
    if let Some(open) = r.get("open").filter(|v| !v.is_null()) {
        reply.open = Some(OpenRequest::parse(open)?);
    }
    if let Some(dialog) = r.get("dialog").filter(|v| !v.is_null()) {
        reply.dialog = Some(Dialog::parse(dialog)?);
    }
    if let Some(j) = r.get("collab").and_then(|c| c.get("join")).filter(|j| !j.is_null()) {
        if !j.is_object() {
            return Err("`collab.join` must be an object".into());
        }
        reply.collab_join = Some(j.clone());
    }
    if let Some(list) = r.get("commands") {
        let list = list.as_array().ok_or("`commands` must be an array")?;
        if list.len() > MAX_REPLY_COMMANDS {
            return Err(format!("a plugin command may run at most {MAX_REPLY_COMMANDS} commands"));
        }
        for c in list {
            let id = c.get("command").and_then(Json::as_str).ok_or("each entry of `commands` needs a `command` id")?;
            reply.commands.push((id.to_string(), c.get("params").cloned().unwrap_or(Json::Null)));
        }
    }
    Ok(reply)
}

// ---------------------------------------------------------------- document events

/// The loaded, active (on, not blocked) plugins that declared `event` (protocol 1.2), in load order.
pub fn listeners(event: &str) -> Vec<String> {
    list().into_iter().filter(|p| p.active() && p.hears(event)).map(|p| p.manifest.id.clone()).collect()
}

/// Sends `events/<event>` to one plugin, answering its `host/…` requests with `host` meanwhile:
/// `Ok(message)` (the answer's `message`, if any) or the error. `documentClosed` times out after
/// [`CLOSE_EVENT_TIMEOUT`], the others after [`EVENT_TIMEOUT`]; a timeout counts as a failure, as
/// for a command.
pub fn send_event(plugin_id: &str, event: &str, params: Json, host: &mut HostHandler<'_>) -> Result<Option<String>, String> {
    let plugin = get(plugin_id).ok_or_else(|| format!("no plugin `{plugin_id}` is loaded"))?;
    if !plugin.active() || !plugin.hears(event) {
        return Err(format!("plugin {plugin_id} doesn't hear {event}"));
    }
    let timeout = if event == "documentClosed" { CLOSE_EVENT_TIMEOUT } else { EVENT_TIMEOUT };
    let r = plugin.request_with(&format!("events/{event}"), params, timeout, host)?;
    Ok(r.get("message").and_then(Json::as_str).filter(|m| !m.trim().is_empty()).map(|m| m.chars().take(4000).collect()))
}

#[cfg(test)]
mod tests;
