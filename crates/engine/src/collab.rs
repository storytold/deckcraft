//! Co-editing (plugin protocol 2): rooms, the `collab.*` commands, and the pump the app's main
//! loop runs between frames ([`Session::collab_poll`]).
//!
//! A room ties one open presentation to a transport plugin. The presentation is mirrored in a
//! Yjs document ([`deckcraft_collab::Replica`]): every committed command (and every finished
//! drag) becomes one transaction sent as `collab/update`, typing as character inserts so that
//! two people typing in one text box both keep their text; remote updates are applied between
//! commands (never in the middle of a drag or an input method's composition) and mapped back into
//! the model slide by slide. While a presentation is in a room, Undo and Redo are the person's own
//! (they revert only their changes). Transports only move bytes; with a room key every payload is
//! encrypted end to end (AES-256-GCM).

use std::sync::Arc;

use deckcraft_collab::awareness::{self, Awareness};
use deckcraft_collab::crypto::{self, Key};
use deckcraft_collab::{CollabError, Replica, TextAt};
use deckcraft_model::{Presentation, ShapeId, SlideId};
use deckcraft_plugin::push::{Incoming, Pushed};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::cmd::{CommandSpec, always, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session, Target, UiRequest};

/// The transport of rooms joined with [`Session::collab_join_in_memory`]: messages wait in the
/// room for the caller ([`Session::collab_take_outgoing`]) instead of going to a plugin. Tests
/// connect sessions with it; a build without plugins can bridge it to a network itself.
pub const IN_MEMORY: &str = "(in-memory)";

/// Longest room id (protocol 2, section 2).
const MAX_ROOM: usize = 256;
/// After this many messages that don't decrypt, the status bar asks about the key.
const WRONG_KEY_AFTER: u32 = 10;
/// Awareness at most this often (20 a second).
const AWARENESS_MS: u64 = 50;
/// A transport that exited is started again after this long.
const RESTART_MS: u64 = 3_000;
/// Others' edits kept for the highlight.
const MAX_REMOTE_EDITS: usize = 2_000;
/// How long others' edits stay highlighted.
pub const HIGHLIGHT_MS: u64 = 2_000;
/// Rooms per transport (protocol 2, section 7).
const MAX_ROOMS_PER_TRANSPORT: usize = 16;
/// Messages kept while a drag or a composition is in progress.
const MAX_DEFERRED: usize = 10_000;
/// Ids handed out from one random block before a new block is chosen.
const ID_BLOCK: u32 = 32_768;

/// A person in a room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub color: String,
}

impl User {
    fn parse(v: &Json) -> Option<User> {
        let s = |k: &str| v.get(k).and_then(Json::as_str).map(str::to_string);
        let u = User { id: s("id")?, name: s("name")?, color: s("color").unwrap_or_else(|| "#808080".into()) };
        u.valid().then_some(u)
    }
    fn valid(&self) -> bool {
        let len = |t: &str| (1..=128).contains(&t.chars().count());
        len(&self.id) && len(&self.name) && valid_color(&self.color)
    }
    /// One or two letters for the person's circle: the initials of the first two words.
    pub fn initials(&self) -> String {
        let mut out: String = self.name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
        if out.is_empty() {
            out = "?".into();
        }
        out.to_uppercase()
    }
}

/// `#RRGGBB`.
pub fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c.chars().skip(1).all(|ch| ch.is_ascii_hexdigit())
}

/// What the transport says about the room.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomState {
    Connecting,
    Online,
    Offline,
    Ended,
}

impl RoomState {
    fn parse(s: &str) -> Option<RoomState> {
        Some(match s {
            "connecting" => RoomState::Connecting,
            "online" => RoomState::Online,
            "offline" => RoomState::Offline,
            "ended" => RoomState::Ended,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            RoomState::Connecting => "connecting",
            RoomState::Online => "online",
            RoomState::Offline => "offline",
            RoomState::Ended => "ended",
        }
    }
}

/// Someone else in the room, as their awareness state says.
#[derive(Clone, Debug, PartialEq)]
pub struct Peer {
    pub client_id: u64,
    pub user: User,
    /// The slide they're on (index in our presentation) and the shapes they selected.
    pub selection: Option<(usize, Vec<ShapeId>)>,
    /// Where they're editing text.
    pub text: Option<TextAt>,
    /// They're typing in the chat (awareness `typing: true`).
    pub typing: bool,
}

/// One shared presentation.
pub struct Room {
    pub room: String,
    pub plugin: String,
    /// The presentation's `DocState::uid`.
    pub doc: u64,
    pub state: RoomState,
    /// Waiting for the room's document (`mode: "join"`): the presentation is read-only.
    pub joining: bool,
    /// `view`: the presentation is read-only.
    pub view_only: bool,
    pub user: User,
    /// Peers the transport listed (`collab/status`).
    pub listed: Vec<User>,
    /// The transport's or the app's message for the status bar.
    pub message: Option<String>,
    /// Another app's or schema's room: nothing syncs.
    pub refused: Option<String>,
    /// The invitation the transport gave when the room was started (`result.link`), to copy.
    pub link: Option<String>,
    key: Option<Key>,
    replica: Replica,
    awareness: Awareness,
    decrypt_failures: u32,
    last_awareness: Option<Json>,
    last_awareness_at: u64,
    /// The transport exited at this time (it's started again after [`RESTART_MS`]).
    exited_at: Option<u64>,
    /// Messages for a transport served in memory.
    outbox: std::cell::RefCell<Vec<Json>>,
    in_memory: bool,
    /// Status-bar messages not shown yet ([`Session::collab_take_notices`]).
    notices: Vec<String>,
    /// Remote changes held while a drag or a composition is in progress.
    deferred: Vec<Pushed>,
    /// The id counter when this person's current block of ids started.
    id_floor: u32,
    /// The text being edited, as relative positions taken before remote changes apply.
    caret: Option<Caret>,
}

impl Room {
    pub fn client_id(&self) -> u64 {
        self.replica.client_id()
    }
    /// The room's title (`meta.title`).
    pub fn title(&self) -> Option<String> {
        self.replica.title()
    }
    pub fn encrypted(&self) -> bool {
        self.key.is_some()
    }
    /// The synced presentation (tests compare peers with it).
    pub fn synced(&self) -> Option<&Arc<Presentation>> {
        self.replica.base()
    }
    /// Others present, from their awareness states.
    pub fn peers(&self) -> Vec<Peer> {
        self.awareness
            .present()
            .filter_map(|(id, st)| {
                let user = User::parse(st.get("user")?)?;
                let sel = st.get("selection").and_then(|s| self.replica.selection_from_ids(s));
                let (selection, text) = match sel {
                    Some((slide, shapes, text)) => (Some((slide, shapes.into_iter().map(ShapeId).collect())), text),
                    None => (None, None),
                };
                let typing = st.get("typing").and_then(Json::as_bool) == Some(true);
                Some(Peer { client_id: id, user, selection, text, typing })
            })
            .collect()
    }
    fn info(&self, s: &Session) -> Json {
        let people: Vec<Json> = self
            .peers()
            .iter()
            .map(|p| json!({"clientId": p.client_id, "user": p.user, "slide": p.selection.as_ref().map(|(i, _)| i), "shapes": p.selection.as_ref().map(|(_, sh)| sh.iter().map(|x| x.0).collect::<Vec<_>>())}))
            .collect();
        json!({
            "room": self.room,
            "plugin": self.plugin,
            "state": if self.joining { "joining" } else { self.state.as_str() },
            "peers": people,
            "listed": self.listed,
            "role": if self.view_only { "view" } else { "edit" },
            "encrypted": self.encrypted(),
            "clientId": self.client_id(),
            "user": self.user,
            "title": self.replica.title(),
            "document": s.docs.iter().find(|d| d.uid == self.doc).map(|d| d.title()),
            "message": self.refused.clone().or_else(|| self.message.clone()),
            "link": self.link,
            "canUndo": self.replica.can_undo(),
            "canRedo": self.replica.can_redo(),
        })
    }
}

/// Co-editing state of a session.
#[derive(Default)]
pub struct Collab {
    pub rooms: Vec<Room>,
    /// Shapes others changed, for the interface to highlight briefly: (document uid, slide id,
    /// shape id, when).
    pub remote_edits: Vec<(u64, u32, u32, u64)>,
    /// Where room states are kept for recovery (the app's data folder); `None`: not kept.
    pub recovery_dir: Option<std::path::PathBuf>,
    /// The main loop's clock (milliseconds), as [`Session::collab_poll`] got it.
    now: u64,
    /// Transport ids the caller serves itself, in memory ([`Session::collab_serve_in_memory`]).
    in_process: Vec<String>,
    /// Answers to their requests (`collab/snapshot`), for [`Session::collab_take_outgoing`].
    answers: Vec<Json>,
    /// The presentation (uid) whose chat this person is typing in (`chat.typing`): awareness says
    /// `typing: true` there.
    pub chat_typing: Option<u64>,
}

impl Collab {
    pub fn room_of(&self, doc: u64) -> Option<&Room> {
        self.rooms.iter().find(|r| r.doc == doc)
    }
    fn room_mut(&mut self, room: &str) -> Option<&mut Room> {
        self.rooms.iter_mut().find(|r| r.room == room)
    }
    /// The clock [`Session::collab_poll`] last got.
    pub fn now(&self) -> u64 {
        self.now
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "collab.share",
            "Share",
            [],
            None,
            "{plugin, room, user?, role?, key?, title?, link?} → the room. Shares the active presentation in a live session through a transport add-in (protocol 2); with no params, the interface asks",
            has_doc,
            share
        ),
        cmd!(
            query "collab.join",
            "Join Live Session",
            [],
            None,
            "{plugin, room, mode: share|join, user?: {id, name, color}, role?: edit|view, key?: base64 of 32 bytes, path?, title?, link?} → {room, clientId, state}. `share` starts a room from the active presentation (or `path`); `join` opens a new presentation (or `path`) filled from the room's document",
            always,
            join
        ),
        cmd!(query "collab.leave", "Leave Live Session", [], None, "{room?} → {left}. The presentation stays open with every edit, as an ordinary presentation", always, leave),
        cmd!(query "collab.status", "Live Sessions", [], None, "{} → {rooms: [{room, plugin, state, peers, role, encrypted, …}], transports: [{id, transport, label}], user}", always, status),
        cmd!(query "collab.setUser", "Live Session Name and Colour", [], None, "{name?, color?: #RRGGBB} → the user. How others see you in live sessions", always, set_user),
    ]
}

/// This person as the preferences say (a stable random id is made the first time).
pub(crate) fn user_from_prefs(s: &mut Session) -> User {
    if s.prefs.collab_user_id.is_empty() {
        let mut b = [0u8; 8];
        let v = if deckcraft_collab::random_bytes(&mut b).is_ok() { u64::from_le_bytes(b) } else { deckcraft_collab::new_client_id() };
        s.prefs.collab_user_id = format!("u-{v:016x}");
    }
    let palette = ["#3B7DD8", "#D8553B", "#2E9E5B", "#9B4FD0", "#D89B1F", "#1F9BB0", "#C23B7A", "#6B7F2A"];
    if !valid_color(&s.prefs.collab_color) {
        let i = s.prefs.collab_user_id.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % palette.len();
        s.prefs.collab_color = palette.get(i).copied().unwrap_or("#3B7DD8").to_string();
    }
    let name: String = s.prefs.author.trim().chars().take(128).collect();
    User {
        id: s.prefs.collab_user_id.clone(),
        name: if name.is_empty() { "DeckCraft User".into() } else { name },
        color: s.prefs.collab_color.clone(),
    }
}

fn share(s: &mut Session, p: &Json) -> Result<Json> {
    if p.as_object().is_none_or(|o| o.is_empty()) {
        s.ui_requests.push(UiRequest::Dialog { id: "collabShare".into(), params: json!({"mode": "share"}) });
        return Ok(json!({}));
    }
    let mut p = p.clone();
    if let Some(o) = p.as_object_mut() {
        o.insert("mode".into(), json!("share"));
    }
    join(s, &p)
}

fn join(s: &mut Session, p: &Json) -> Result<Json> {
    if p.as_object().is_none_or(|o| o.is_empty()) {
        s.ui_requests.push(UiRequest::Dialog { id: "collabShare".into(), params: json!({"mode": "join"}) });
        return Ok(json!({}));
    }
    join_with(s, p, false)
}

/// Replaces a presentation's model, keeping a clean one clean.
fn replace_doc(d: &mut crate::DocState, p: Arc<Presentation>) {
    let clean = !d.is_dirty();
    d.doc = p.clone();
    if clean {
        d.saved_doc = p;
    }
    d.revision += 1;
}

fn join_with(s: &mut Session, p: &Json, in_memory: bool) -> Result<Json> {
    const ID: &str = "collab.join";
    let plugin_id = match str_param(p, "plugin") {
        Some(id) => id.to_string(),
        None if in_memory => IN_MEMORY.to_string(),
        None => return Err(bad(ID, "missing `plugin` (the transport)")),
    };
    let in_memory = in_memory || plugin_id == IN_MEMORY || s.collab.in_process.contains(&plugin_id);
    let room = str_param(p, "room").ok_or_else(|| bad(ID, "missing `room`"))?.to_string();
    if room.is_empty() || room.chars().count() > MAX_ROOM || room.chars().any(char::is_control) {
        return Err(bad(ID, "`room` needs 1–256 characters"));
    }
    let mode = str_param(p, "mode").unwrap_or("join");
    if mode != "share" && mode != "join" {
        return Err(bad(ID, "`mode` is `share` or `join`"));
    }
    let view_only = match str_param(p, "role").unwrap_or("edit") {
        "edit" => false,
        "view" => true,
        _ => return Err(bad(ID, "`role` is `edit` or `view`")),
    };
    let key = match str_param(p, "key").filter(|k| !k.trim().is_empty()) {
        Some(k) => Some(Key::from_base64(k).map_err(|e| bad(ID, format!("`key`: {e}")))?),
        None => None,
    };
    let mut user = user_from_prefs(s);
    if let Some(u) = p.get("user").filter(|u| !u.is_null()) {
        user = User::parse(u).ok_or_else(|| bad(ID, "`user` needs `id` and `name` (1–128 characters) and `color` (#RRGGBB)"))?;
    }
    let title = str_param(p, "title").map(|t| t.chars().take(256).collect::<String>());
    let path = str_param(p, "path").filter(|x| !x.trim().is_empty()).map(str::to_string);
    let link = str_param(p, "link").filter(|x| !x.trim().is_empty()).map(|l| l.chars().take(2048).collect::<String>());

    let plugin = if in_memory {
        None
    } else {
        let plugin = deckcraft_plugin::get(&plugin_id).ok_or_else(|| bad(ID, format!("no add-in `{plugin_id}` is loaded")))?;
        if !plugin.enabled() {
            return Err(EngineError::Other(format!("add-in {plugin_id} is turned off (Add-ins window)")));
        }
        if let Some(b) = plugin.blocked_by().first() {
            return Err(EngineError::Other(format!("add-in {plugin_id} is blocked: {}", b.message)));
        }
        if !plugin.is_transport() {
            return Err(EngineError::Other(format!("add-in {plugin_id} isn't a co-editing transport (its manifest has no `collab`)")));
        }
        Some(plugin)
    };
    if s.collab.rooms.iter().any(|r| r.room == room) {
        return Err(EngineError::Other(format!("already in room {room}")));
    }
    if s.collab.rooms.iter().filter(|r| r.plugin == plugin_id).count() >= MAX_ROOMS_PER_TRANSPORT {
        return Err(EngineError::Other(format!("{plugin_id} already has {MAX_ROOMS_PER_TRANSPORT} rooms")));
    }
    if key.is_some() {
        // Encryption needs random nonces: refuse now rather than at the first edit.
        deckcraft_collab::random_bytes(&mut [0u8; 4]).map_err(|e| EngineError::Other(e.to_string()))?;
    }
    // The presentation: the active one (or `path`) to share; a new one (or `path`) to join.
    match (&path, mode) {
        (Some(path), _) => {
            s.execute("file.open", &json!({"path": path}))?;
        }
        (None, "join") => {
            s.execute("file.new", &json!({}))?;
        }
        (None, _) => {}
    }
    let doc = s.doc()?.uid;
    if s.collab.rooms.iter().any(|r| r.doc == doc) {
        return Err(EngineError::Other("this presentation is already in a live session".into()));
    }
    if let Some(plugin) = &plugin {
        plugin.ensure_running().map_err(|e| EngineError::Other(format!("can't start {plugin_id}: {e}")))?;
    }
    s.end_composition();
    let client_id = deckcraft_collab::new_client_id();
    let mut replica = Replica::new(client_id);
    let mut joining = mode == "join";
    if mode == "share" {
        let d = s.doc()?;
        // The title: as given, else the file's name; an unsaved presentation has none.
        let title = title.clone().unwrap_or_else(|| if d.path.is_some() { d.title() } else { String::new() });
        let p = d.doc.clone();
        replica.seed(p, &title).map_err(|e| EngineError::Other(e.to_string()))?;
        // From now on new ids come from a random block (two people adding at once).
        if let Some(b) = replica.base().cloned() {
            replace_doc(s.doc_mut()?, b);
        }
    } else if let Some(state) = read_recovery(s, &room) {
        // Our own edits from before a crash: merged with the room's when it answers.
        if replica.apply_update(&state).is_ok() {
            let cur = s.doc()?.doc.clone();
            if let Ok(Some(r)) = replica.refresh(&cur) {
                let p = Arc::new(r.doc);
                replica.rebase(p.clone());
                replace_doc(s.doc_mut()?, p);
                joining = false;
            }
        }
    }
    let encrypted = key.is_some();
    let id_floor = replica.base().map(|b| b.next_id).unwrap_or(0);
    let mut room_obj = Room {
        room: room.clone(),
        plugin: plugin_id.clone(),
        doc,
        state: RoomState::Connecting,
        joining,
        view_only,
        user: user.clone(),
        listed: vec![],
        message: None,
        refused: None,
        link,
        key,
        awareness: Awareness::new(client_id),
        replica,
        decrypt_failures: 0,
        last_awareness: None,
        last_awareness_at: 0,
        exited_at: None,
        outbox: Default::default(),
        in_memory,
        notices: vec![],
        deferred: vec![],
        id_floor,
        caret: None,
    };
    // A room's undo is the person's own: the presentation's history starts here.
    if let Some(d) = s.docs.iter_mut().find(|d| d.uid == doc) {
        d.history.undo.clear();
        d.history.redo.clear();
        if joining {
            d.selection = crate::Selection::default();
        }
    }
    announce(&mut room_obj).map_err(EngineError::Other)?;
    let out = json!({"room": room, "clientId": client_id, "mode": mode, "state": if room_obj.joining { "joining" } else { "connecting" }, "encrypted": encrypted});
    s.collab.rooms.push(room_obj);
    send_awareness(s, true);
    Ok(out)
}

/// `collab/joined`, then sync step 1.
fn announce(r: &mut Room) -> std::result::Result<(), String> {
    let joined = json!({"room": r.room, "clientId": r.client_id(), "user": r.user, "app": deckcraft_collab::APP, "schema": deckcraft_collab::SCHEMA, "encrypted": r.encrypted()});
    let sync1 = json!({"room": r.room, "stateVector": deckcraft_collab::b64_encode(&r.replica.state_vector())});
    if r.in_memory {
        notify(r, "collab/joined", joined);
        notify(r, "collab/sync1", sync1);
    } else {
        let plugin = deckcraft_plugin::get(&r.plugin).ok_or("the transport was unloaded")?;
        plugin.notify("collab/joined", joined)?;
        plugin.notify("collab/sync1", sync1)?;
    }
    Ok(())
}

fn seal(r: &Room, method: &str, bytes: &[u8]) -> std::result::Result<String, CollabError> {
    let data = match &r.key {
        Some(k) => crypto::encrypt(k, &r.room, method, bytes)?,
        None => bytes.to_vec(),
    };
    Ok(deckcraft_collab::b64_encode(&data))
}

fn unseal(r: &mut Room, method: &str, text: &str) -> std::result::Result<Vec<u8>, CollabError> {
    let data = deckcraft_collab::b64_decode(text)?;
    match &r.key {
        Some(k) => crypto::decrypt(k, &r.room, method, &data).inspect_err(|_| {
            r.decrypt_failures += 1;
            if r.decrypt_failures >= WRONG_KEY_AFTER {
                r.message = Some("Wrong room key?".into());
                if r.decrypt_failures == WRONG_KEY_AFTER {
                    r.notices.push("Wrong room key?".into());
                }
            }
        }),
        None => Ok(data),
    }
}

fn notify(r: &Room, method: &str, params: Json) {
    if r.in_memory {
        r.outbox.borrow_mut().push(json!({"jsonrpc": "2.0", "method": method, "params": params}));
        return;
    }
    if let Some(p) = deckcraft_plugin::get(&r.plugin)
        && let Err(e) = p.notify(method, params)
    {
        log::warn!("collab {}: {method}: {e}", r.room);
    }
}

fn leave(s: &mut Session, p: &Json) -> Result<Json> {
    let room = match str_param(p, "room") {
        Some(r) => r.to_string(),
        None => {
            let uid = s.doc()?.uid;
            s.collab.room_of(uid).map(|r| r.room.clone()).ok_or_else(|| EngineError::Other("this presentation isn't in a live session".into()))?
        }
    };
    if !s.collab.rooms.iter().any(|r| r.room == room) {
        return Err(EngineError::Other(format!("not in room {room}")));
    }
    leave_room(s, &room, None, true);
    Ok(json!({"left": room}))
}

/// Leaves `room`: tells the transport (unless it asked), keeps the presentation as it is.
fn leave_room(s: &mut Session, room: &str, reason: Option<&str>, tell: bool) {
    let Some(i) = s.collab.rooms.iter().position(|r| r.room == room) else { return };
    let mut r = s.collab.rooms.remove(i);
    flush(s, &mut r);
    if tell {
        let bye = r.awareness.set_local(None);
        if let Ok(u) = seal(&r, "collab/awareness", &bye) {
            notify(&r, "collab/awareness", json!({"room": r.room, "update": u}));
        }
        notify(&r, "collab/leave", json!({"room": r.room, "reason": reason.unwrap_or("left")}));
    }
    s.collab.answers.extend(std::mem::take(&mut *r.outbox.borrow_mut()));
    remove_recovery(s, room);
    if let Some(d) = s.docs.iter_mut().find(|d| d.uid == r.doc) {
        // Back to an ordinary presentation: its own undo starts afresh.
        d.history.undo.clear();
        d.history.redo.clear();
    }
    s.collab.remote_edits.retain(|e| e.0 != r.doc);
    if let Some(why) = reason
        && !tell
    {
        s.ui_requests.push(UiRequest::Message { text: format!("The live session ended: {why}") });
    }
}

fn status(s: &mut Session, _p: &Json) -> Result<Json> {
    let rooms: Vec<Json> = s.collab.rooms.iter().map(|r| r.info(s)).collect();
    let transports: Vec<Json> = deckcraft_plugin::transports()
        .iter()
        .map(|p| {
            let c = p.manifest.collab.clone().unwrap_or_default();
            json!({"id": p.manifest.id, "transport": c.transport, "label": if c.label.is_empty() { p.manifest.name.clone() } else { c.label }})
        })
        .collect();
    Ok(json!({"rooms": rooms, "transports": transports, "user": user_from_prefs(s)}))
}

fn set_user(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "collab.setUser";
    if let Some(n) = str_param(p, "name") {
        let n = n.trim();
        if n.is_empty() || n.chars().count() > 128 {
            return Err(bad(ID, "`name` needs 1–128 characters"));
        }
        s.prefs.author = n.to_string();
    }
    if let Some(c) = str_param(p, "color") {
        if !valid_color(c) {
            return Err(bad(ID, "`color` is #RRGGBB"));
        }
        s.prefs.collab_color = c.to_ascii_uppercase();
    }
    let user = user_from_prefs(s);
    for r in s.collab.rooms.iter_mut() {
        r.user = User { id: r.user.id.clone(), ..user.clone() };
        r.last_awareness = None;
    }
    send_awareness(s, true);
    Ok(serde_json::to_value(&user).unwrap_or(Json::Null))
}

// ---------------------------------------------------------------- the engine's side

/// Why the active presentation can't be edited now (in a room while joining, as a viewer, or in
/// a room for another app).
pub fn read_only(s: &Session) -> Option<String> {
    let uid = s.active()?.uid;
    let r = s.collab.room_of(uid)?;
    if let Some(why) = &r.refused {
        return Some(why.clone());
    }
    if r.joining {
        return Some("Joining…".into());
    }
    r.view_only.then(|| "You can view this presentation (live session, view only)".into())
}

/// `edit.undo` and `edit.redo` in a room: the person's own changes. `None`: not in a room.
pub(crate) fn intercept(s: &mut Session, id: &str, p: &Json) -> Option<Result<Json>> {
    let undo = match id {
        "edit.undo" => true,
        "edit.redo" => false,
        _ => return None,
    };
    let uid = s.active()?.uid;
    let i = s.collab.rooms.iter().position(|r| r.doc == uid)?;
    if let Some(why) = read_only(s) {
        return Some(Err(EngineError::Disabled(id.to_string(), why)));
    }
    s.end_composition();
    let steps = p.get("steps").and_then(Json::as_u64).unwrap_or(1).clamp(1, 100);
    // Anything not yet sent goes first (it becomes the step being undone).
    let mut r = s.collab.rooms.remove(i);
    flush(s, &mut r);
    r.caret = caret_of(s, &r);
    let mut did = 0;
    let mut result = Ok(());
    for _ in 0..steps {
        let step = if undo { r.replica.undo() } else { r.replica.redo() };
        match step {
            Ok(Some(update)) => {
                did += 1;
                send_update(&r, "collab/update", &update);
            }
            Ok(None) => break,
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    let cur = s.docs.iter().find(|d| d.uid == uid).map(|d| d.doc.clone()).unwrap_or_default();
    match r.replica.refresh(&cur) {
        Ok(Some(out)) => apply_refreshed(s, uid, out, &mut r),
        Ok(None) => {}
        Err(e) => result = Err(e),
    }
    s.collab.rooms.insert(i, r);
    Some(match result {
        Err(e) => Err(EngineError::Other(e.to_string())),
        Ok(()) if did == 0 => Err(EngineError::Disabled(id.to_string(), if undo { "nothing to undo".into() } else { "nothing to redo".into() })),
        Ok(()) => {
            let mut out = json!({"perUser": true});
            if let Some(o) = out.as_object_mut() {
                o.insert(if undo { "undone" } else { "redone" }.into(), json!(did));
            }
            Ok(out)
        }
    })
}

/// Whether Undo / Redo have something to do in the active presentation's room. `None`: not in
/// one.
pub fn room_undo(s: &Session, redo: bool) -> Option<bool> {
    let uid = s.active()?.uid;
    let r = s.collab.room_of(uid)?;
    Some(r.replica.is_ready() && if redo { r.replica.can_redo() } else { r.replica.can_undo() })
}

fn send_update(r: &Room, method: &str, update: &[u8]) {
    if update.len() > deckcraft_collab::MAX_UPDATE {
        log::warn!("collab {}: an update of {} bytes is larger than 16 MiB; peers will fetch it with a sync", r.room, update.len());
        notify(r, "collab/sync1", json!({"room": r.room, "stateVector": deckcraft_collab::b64_encode(&r.replica.state_vector())}));
        return;
    }
    match seal(r, method, update) {
        Ok(u) => notify(r, method, json!({"room": r.room, "update": u})),
        Err(e) => log::warn!("collab {}: {e}", r.room),
    }
}

/// Whether the presentation is in the middle of a drag or an input method's composition (its
/// change is shared when that ends).
fn busy(s: &Session, uid: u64) -> bool {
    s.docs.iter().find(|d| d.uid == uid).is_some_and(|d| d.interaction.is_some() || d.composition.is_some())
}

/// Sends the change since the last sync of `r`'s presentation, if any.
fn flush(s: &mut Session, r: &mut Room) {
    if !r.replica.is_ready() || r.refused.is_some() || busy(s, r.doc) {
        return;
    }
    let Some(d) = s.docs.iter_mut().find(|d| d.uid == r.doc) else { return };
    if r.replica.base().is_some_and(|b| Arc::ptr_eq(b, &d.doc)) {
        return;
    }
    if r.view_only || r.joining {
        // A viewer chats; their other edits never leave: back to the room's state.
        if r.view_only && !r.joining && r.replica.base().is_some_and(|b| !Arc::ptr_eq(&b.chat, &d.doc.chat)) {
            let chat = d.doc.chat.clone();
            match r.replica.commit_chat(&chat) {
                Ok(Some(update)) => send_update(r, "collab/update", &update),
                Ok(None) => {}
                Err(e) => log::warn!("collab {}: {e}", r.room),
            }
        }
        if let Some(b) = r.replica.base() {
            d.doc = b.clone();
            d.revision += 1;
        }
        return;
    }
    let cur = d.doc.clone();
    let rep = &mut r.replica;
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rep.commit(&cur)));
    // The room keeps the person's own undo: the local history isn't used.
    d.history.undo.clear();
    d.history.redo.clear();
    match res {
        Ok(Ok(Some(update))) => send_update(r, "collab/update", &update),
        Ok(Ok(None)) => {}
        Ok(Err(e)) => {
            log::warn!("collab {}: {e}", r.room);
            r.message = Some(e.to_string());
        }
        Err(_) => {
            log::warn!("collab {}: a change couldn't be shared", r.room);
            r.message = Some("A change couldn't be shared".into());
        }
    }
    // A new block of ids once this one is well used.
    if cur.next_id.saturating_sub(r.id_floor) > ID_BLOCK || cur.next_id < r.id_floor {
        let mut p = (*cur).clone();
        let mut rng = deckcraft_collab::Rng::new(r.client_id());
        deckcraft_collab::reseed_ids(&mut p, &mut rng);
        r.id_floor = p.next_id;
        let p = Arc::new(p);
        if Arc::ptr_eq(&d.saved_doc, &cur) {
            d.saved_doc = p.clone();
        }
        d.doc = p.clone();
        r.replica.rebase(p);
    }
}

/// After every top-level command: each room's presentation change becomes one update.
pub(crate) fn after_command(s: &mut Session) {
    if s.collab.rooms.is_empty() {
        return;
    }
    // Rooms whose presentation was closed end.
    let gone: Vec<String> = s.collab.rooms.iter().filter(|r| !s.docs.iter().any(|d| d.uid == r.doc)).map(|r| r.room.clone()).collect();
    for room in gone {
        leave_room(s, &room, Some("the presentation was closed"), true);
    }
    let mut rooms = std::mem::take(&mut s.collab.rooms);
    for r in rooms.iter_mut() {
        flush(s, r);
    }
    rooms.append(&mut s.collab.rooms);
    s.collab.rooms = rooms;
    send_awareness(s, false);
}

/// The text being edited in a presentation, as relative positions in the room's document, so
/// the caret follows others' typing: (slide, shape or notes, anchor, caret).
type Caret = (u32, Option<u32>, String, String);

fn caret_of(s: &Session, r: &Room) -> Option<Caret> {
    let d = s.docs.iter().find(|d| d.uid == r.doc)?;
    let t = d.selection.text.as_ref()?;
    if d.selection.target != Target::Slides || t.cell.is_some() {
        return None;
    }
    let slide = d.current_slide()?.id.0;
    let shape = if t.notes { None } else { Some(t.shape.0) };
    Some((slide, shape, r.replica.sticky(slide, shape, t.anchor)?, r.replica.sticky(slide, shape, t.caret)?))
}

/// Puts a refreshed presentation (remote changes, an undo) into its document.
fn apply_refreshed(s: &mut Session, uid: u64, out: deckcraft_collab::Refreshed, room: &mut Room) {
    let caret = room.caret.take();
    let Some(d) = s.docs.iter_mut().find(|d| d.uid == uid) else { return };
    let current = d.current_slide().map(|sl| sl.id);
    let p = Arc::new(out.doc);
    let dirty = d.is_dirty();
    d.doc = p.clone();
    // Others' chat makes the presentation modified: it has to be saved for the chat to be kept.
    if !dirty && out.changed.is_empty() && !out.structural && !out.chat {
        d.saved_doc = p.clone();
    }
    d.revision += 1;
    room.replica.rebase(p);
    // The caret stays where it was in the text, wherever others typed.
    if let (Some((slide, shape, a, c)), Some(t)) = (caret, d.selection.text.as_mut()) {
        let anchor = room.replica.resolve(slide, shape, &a);
        let head = room.replica.resolve(slide, shape, &c);
        if let (Some(a), Some(h)) = (anchor, head) {
            t.anchor = a;
            t.caret = h;
            t.line_end = None;
        }
    }
    // The current slide stays the same slide when others add or remove slides before it.
    if let Some(i) = current.and_then(|id| d.doc.slide_index(id)) {
        d.selection.slide = i;
    }
    crate::clamp_selection(d);
    let now = s.collab.now;
    for (slide, shapes) in out.changed {
        for shape in shapes {
            if s.collab.remote_edits.len() >= MAX_REMOTE_EDITS {
                s.collab.remote_edits.remove(0);
            }
            s.collab.remote_edits.push((uid, slide, shape, now));
        }
    }
}

impl Session {
    /// Runs co-editing between frames: takes what transports pushed, applies remote changes,
    /// answers snapshots and sends this person's selection. `now`: the caller's clock in
    /// milliseconds (the engine reads no clock: it would trap in the browser). Returns whether
    /// anything changed that the interface should show.
    pub fn collab_poll(&mut self, now: u64) -> bool {
        let pushed = deckcraft_plugin::push::take();
        self.collab_poll_pushed(now, pushed)
    }

    /// [`Session::collab_poll`] with the messages given (several sessions in one process share
    /// the plugins' queue: the caller splits it; [`IN_MEMORY`] rooms get theirs this way).
    pub fn collab_poll_pushed(&mut self, now: u64, pushed: Vec<Pushed>) -> bool {
        self.collab.now = now;
        // Our own changes first (a finished drag), so remote ones apply on top of them.
        let mut rooms = std::mem::take(&mut self.collab.rooms);
        let mut all = Vec::new();
        for r in rooms.iter_mut() {
            flush(self, r);
            if !busy(self, r.doc) {
                all.append(&mut r.deferred);
            }
            // Where the caret is, before others' changes move the text under it.
            r.caret = caret_of(self, r);
        }
        rooms.append(&mut self.collab.rooms);
        self.collab.rooms = rooms;
        all.extend(pushed);
        let mut changed = false;
        for p in all {
            changed |= handle(self, p);
        }
        // Remote changes into the presentations.
        let mut rooms = std::mem::take(&mut self.collab.rooms);
        for r in rooms.iter_mut() {
            if r.refused.is_some() || !r.replica.has_changes() || busy(self, r.doc) {
                continue;
            }
            let Some(cur) = self.docs.iter().find(|d| d.uid == r.doc).map(|d| d.doc.clone()) else { continue };
            let rep = &mut r.replica;
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rep.refresh(&cur)));
            match res {
                Ok(Ok(Some(out))) => {
                    let was_joining = r.joining;
                    r.joining = false;
                    let uid = r.doc;
                    apply_refreshed(self, uid, out, r);
                    if was_joining {
                        r.id_floor = r.replica.base().map(|b| b.next_id).unwrap_or(0);
                        if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                            d.saved_doc = d.doc.clone();
                            d.selection = crate::Selection::default();
                            if let Some(t) = r.replica.title().filter(|t| !t.is_empty()) {
                                d.title = t;
                            }
                        }
                        r.last_awareness = None;
                    }
                    changed = true;
                }
                Ok(Ok(None)) => {}
                Ok(Err(e @ CollabError::Mismatch { .. })) => {
                    r.notices.push(e.to_string());
                    r.refused = Some(e.to_string());
                    changed = true;
                }
                Ok(Err(e)) => {
                    r.message = Some(e.to_string());
                    changed = true;
                }
                Err(_) => {
                    r.message = Some("A change from the room couldn't be applied".into());
                    changed = true;
                }
            }
            if r.replica.has_gaps() && r.state == RoomState::Online {
                notify(r, "collab/sync1", json!({"room": r.room, "stateVector": deckcraft_collab::b64_encode(&r.replica.state_vector())}));
            }
        }
        rooms.append(&mut self.collab.rooms);
        self.collab.rooms = rooms;
        // Transports that exited: started again, then the rooms rejoin.
        for i in 0..self.collab.rooms.len() {
            let Some(r) = self.collab.rooms.get_mut(i) else { continue };
            let Some(at) = r.exited_at else { continue };
            if now.saturating_sub(at) < RESTART_MS {
                continue;
            }
            r.exited_at = None;
            let Some(plugin) = deckcraft_plugin::get(&r.plugin) else { continue };
            match plugin.ensure_running() {
                Ok(()) => {
                    r.state = RoomState::Connecting;
                    r.message = None;
                    if let Err(e) = announce(r) {
                        r.message = Some(e);
                    }
                }
                Err(e) => {
                    r.message = Some(format!("{}: {e}", r.plugin));
                    if !e.starts_with("stopped after") {
                        r.exited_at = Some(now);
                    }
                }
            }
            changed = true;
        }
        let pruned = self.collab.rooms.iter_mut().fold(false, |acc, r| r.awareness.prune(now) | acc);
        let before = self.collab.remote_edits.len();
        self.collab.remote_edits.retain(|e| now.saturating_sub(e.3) < HIGHLIGHT_MS || now < e.3);
        send_awareness(self, false);
        changed | pruned | (before != self.collab.remote_edits.len())
    }

    /// Serves transport `id` in memory: rooms joined through it (`collab.join` with `plugin: id`)
    /// work like [`Session::collab_join_in_memory`]'s. A conformance replayer stands in for a
    /// transport this way.
    pub fn collab_serve_in_memory(&mut self, id: &str) {
        if !self.collab.in_process.iter().any(|x| x == id) {
            self.collab.in_process.push(id.to_string());
        }
    }

    /// Joins a room whose transport is the caller ([`IN_MEMORY`]): `collab.join`'s params
    /// without `plugin`. Its messages are taken with [`Session::collab_take_outgoing`]; what
    /// arrives is handed to [`Session::collab_poll_pushed`] as coming from [`IN_MEMORY`].
    pub fn collab_join_in_memory(&mut self, params: Json) -> Result<Json> {
        join_with(self, &params, true)
    }

    /// Messages for the status bar since the last call ("This room is for wordcraft 1",
    /// "Wrong room key?").
    pub fn collab_take_notices(&mut self) -> Vec<String> {
        self.collab.rooms.iter_mut().flat_map(|r| std::mem::take(&mut r.notices)).collect()
    }

    /// Messages [`IN_MEMORY`] rooms sent, oldest first (JSON-RPC notifications).
    pub fn collab_take_outgoing(&mut self) -> Vec<Json> {
        let mut out: Vec<Json> = std::mem::take(&mut self.collab.answers);
        out.extend(self.collab.rooms.iter().flat_map(|r| std::mem::take(&mut *r.outbox.borrow_mut())));
        out
    }

    /// The co-editing room of the active presentation.
    pub fn active_room(&self) -> Option<&Room> {
        self.collab.room_of(self.active()?.uid)
    }

    /// Every room's whole state, for recovery: `(room, plugin, update)`.
    pub fn collab_recovery(&self) -> Vec<(String, String, Vec<u8>)> {
        self.collab.rooms.iter().filter(|r| r.replica.is_ready()).map(|r| (r.room.clone(), r.plugin.clone(), r.replica.full_state())).collect()
    }

    /// Writes [`Session::collab_recovery`] into the recovery folder (the app does this with its
    /// AutoRecover timer).
    pub fn save_collab_recovery(&self) {
        let Some(dir) = &self.collab.recovery_dir else { return };
        for (room, plugin, state) in self.collab_recovery() {
            let body = json!({"room": room, "plugin": plugin, "update": deckcraft_collab::b64_encode(&state)});
            if let Err(e) = crate::recovery::write_room(dir, &recovery_name(&room), &body.to_string()) {
                log::warn!("collab: can't keep the room's state for recovery: {e}");
            }
        }
    }
}

/// A file name for a room (its id may hold any character): FNV-1a of the room, twice.
fn recovery_name(room: &str) -> String {
    let fnv = |seed: u64| room.bytes().fold(seed, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("room-{:016x}{:016x}.json", fnv(0xcbf2_9ce4_8422_2325), fnv(0x8422_2325_cbf2_9ce4))
}

fn read_recovery(s: &Session, room: &str) -> Option<Vec<u8>> {
    let dir = s.collab.recovery_dir.as_ref()?;
    let text = crate::recovery::read_room(dir, &recovery_name(room))?;
    let v: Json = serde_json::from_str(&text).ok()?;
    if v.get("room").and_then(Json::as_str) != Some(room) {
        return None;
    }
    deckcraft_collab::b64_decode(v.get("update")?.as_str()?).ok()
}

fn remove_recovery(s: &Session, room: &str) {
    if let Some(dir) = &s.collab.recovery_dir {
        crate::recovery::remove_room(dir, &recovery_name(room));
    }
}

/// This person's selection to the rooms (at most 20 a second, renewed every 15 s).
fn send_awareness(s: &mut Session, force: bool) {
    let now = s.collab.now;
    let mut sends = Vec::new();
    for (i, r) in s.collab.rooms.iter().enumerate() {
        if !r.replica.is_ready() || r.refused.is_some() {
            continue;
        }
        let Some(d) = s.docs.iter().find(|d| d.uid == r.doc) else { continue };
        let mut state = json!({"user": r.user});
        if s.collab.chat_typing == Some(r.doc)
            && let Some(o) = state.as_object_mut()
        {
            o.insert("typing".into(), json!(true));
        }
        if d.selection.target == Target::Slides
            && let Some(slide) = d.current_slide()
        {
            let shapes: Vec<u32> = d.selection.shapes.iter().map(|x| x.0).collect();
            let text = d.selection.text.as_ref().filter(|t| t.cell.is_none()).map(|t| TextAt {
                shape: (!t.notes).then_some(t.shape.0),
                notes: t.notes,
                anchor: t.anchor,
                head: t.caret,
            });
            if let Some(o) = state.as_object_mut() {
                o.insert("selection".into(), r.replica.selection_to_ids(slide.id.0, &shapes, text.as_ref()));
            }
        }
        let same = r.last_awareness.as_ref() == Some(&state);
        let due = now.saturating_sub(r.last_awareness_at) >= awareness::RENEW_MS;
        let allowed = force || now.saturating_sub(r.last_awareness_at) >= AWARENESS_MS || now < r.last_awareness_at;
        if (!same || due) && allowed {
            sends.push((i, state));
        }
    }
    for (i, state) in sends {
        let Some(r) = s.collab.rooms.get_mut(i) else { continue };
        let update = r.awareness.set_local(Some(state.clone()));
        r.last_awareness = Some(state);
        r.last_awareness_at = now;
        if let Ok(u) = seal(r, "collab/awareness", &update) {
            notify(r, "collab/awareness", json!({"room": r.room, "update": u}));
        }
    }
}

/// Handles one pushed message. Returns whether the interface should repaint.
fn handle(s: &mut Session, p: Pushed) -> bool {
    let msg = match &p.incoming {
        Incoming::Message(m) => m.clone(),
        Incoming::Exited(why) => {
            let mut any = false;
            let now = s.collab.now;
            for r in s.collab.rooms.iter_mut().filter(|r| r.plugin == p.plugin) {
                r.state = RoomState::Offline;
                r.message = Some(format!("Offline: {why}"));
                r.exited_at = Some(now);
                any = true;
            }
            if let Some(plugin) = deckcraft_plugin::get(&p.plugin) {
                plugin.exited();
                if any {
                    plugin.count_failure(why);
                }
            }
            return any;
        }
        Incoming::Overlong => {
            if let Some(plugin) = deckcraft_plugin::get(&p.plugin) {
                plugin.count_failure("a message larger than 64 MiB was dropped");
            }
            return false;
        }
        Incoming::Flood => {
            if let Some(plugin) = deckcraft_plugin::get(&p.plugin) {
                plugin.fail("stopped: it sent more than 1,000 messages a second");
            }
            for r in s.collab.rooms.iter_mut().filter(|r| r.plugin == p.plugin) {
                r.state = RoomState::Offline;
                r.message = Some(format!("{} was stopped: it flooded the app", p.plugin));
            }
            return true;
        }
    };
    let method = msg.get("method").and_then(Json::as_str).unwrap_or("").to_string();
    let params = msg.get("params").cloned().unwrap_or(Json::Null);
    let id = msg.get("id").cloned();
    // Requests: answered now, in any order.
    if let Some(id) = id {
        let answer = match method.as_str() {
            "host/log" => {
                let text: String = params.get("text").and_then(Json::as_str).unwrap_or("").chars().take(4000).collect();
                log::info!("add-in {}: {text}", p.plugin);
                Ok(json!({}))
            }
            "collab/snapshot" => snapshot(s, &p.plugin, &params),
            m if m.starts_with("host/") => Err(deckcraft_plugin::HostError {
                code: -32000,
                message: format!("{m} is only available while the add-in runs a command"),
                data: json!({"code": "NotNow"}),
            }),
            _ => Err(deckcraft_plugin::HostError::unknown()),
        };
        if p.plugin == IN_MEMORY || s.collab.in_process.contains(&p.plugin) {
            let msg = match answer {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": e.code, "message": e.message, "data": e.data}}),
            };
            s.collab.answers.push(msg);
        } else if let Some(plugin) = deckcraft_plugin::get(&p.plugin)
            && let Err(e) = plugin.respond(&id, answer)
        {
            log::warn!("add-in {}: answering {method}: {e}", p.plugin);
        }
        return false;
    }
    let Some(room) = params.get("room").and_then(Json::as_str).map(str::to_string) else {
        if !method.is_empty() && !method.starts_with("host/") {
            log::debug!("add-in {}: ignored notification {method}", p.plugin);
        }
        return false;
    };
    let now = s.collab.now;
    let Some(i) = s.collab.rooms.iter().position(|r| r.room == room) else {
        log::debug!("add-in {}: {method} for a room we're not in", p.plugin);
        return false;
    };
    let uid = s.collab.rooms.get(i).map(|r| r.doc).unwrap_or(0);
    let in_progress = busy(s, uid);
    let Some(r) = s.collab.room_mut(&room) else { return false };
    if r.plugin != p.plugin {
        log::warn!("add-in {}: {method} for room {room}, which belongs to {}", p.plugin, r.plugin);
        return false;
    }
    match method.as_str() {
        "collab/sync1" => {
            let Some(sv) = params.get("stateVector").and_then(Json::as_str).and_then(|t| deckcraft_collab::b64_decode(t).ok()) else {
                return false;
            };
            if r.refused.is_some() {
                return false;
            }
            match r.replica.diff_for(&sv) {
                Ok(diff) => match seal(r, "collab/sync2", &diff) {
                    Ok(u) => {
                        let mut out = json!({"room": r.room, "update": u});
                        if let Some(to) = params.get("from").or_else(|| params.get("clientId")).filter(|c| c.is_u64())
                            && let Some(o) = out.as_object_mut()
                        {
                            o.insert("to".into(), to.clone());
                        }
                        notify(r, "collab/sync2", out);
                        // The asker has changes we lack: ask back.
                        if r.replica.lacks(&sv) {
                            notify(
                                r,
                                "collab/sync1",
                                json!({"room": r.room, "stateVector": deckcraft_collab::b64_encode(&r.replica.state_vector())}),
                            );
                        }
                    }
                    Err(e) => log::warn!("collab {room}: {e}"),
                },
                Err(e) => log::warn!("collab {room}: sync1: {e}"),
            }
            false
        }
        "collab/sync2" | "collab/update" => {
            if let Some(to) = params.get("to").and_then(Json::as_u64)
                && to != r.client_id()
            {
                return false;
            }
            if r.refused.is_some() {
                return false;
            }
            // Never in the middle of a drag or a composition: it waits for it to end.
            if in_progress {
                if r.deferred.len() < MAX_DEFERRED {
                    r.deferred.push(p);
                }
                return false;
            }
            let Some(text) = params.get("update").and_then(Json::as_str) else { return false };
            // A stored snapshot (the room's whole state) is sealed as `collab/snapshot`.
            let sealed_as = if method == "collab/sync2" && params.get("snapshot").and_then(Json::as_bool) == Some(true) {
                "collab/snapshot"
            } else {
                method.as_str()
            };
            match unseal(r, sealed_as, text).and_then(|u| r.replica.apply_update(&u)) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("collab {room}: {method}: {e}");
                    match e {
                        CollabError::Decrypt => {}
                        CollabError::Mismatch { .. } => {
                            r.message = Some(e.to_string());
                            r.notices.push(e.to_string());
                        }
                        e => r.message = Some(e.to_string()),
                    }
                    true
                }
            }
        }
        "collab/awareness" => {
            let Some(text) = params.get("update").and_then(Json::as_str) else { return false };
            match unseal(r, "collab/awareness", text).and_then(|u| r.awareness.apply(&u, now)) {
                Ok(changed) => changed,
                Err(e) => {
                    log::debug!("collab {room}: awareness: {e}");
                    false
                }
            }
        }
        "collab/status" => {
            if let Some(st) = params.get("state").and_then(Json::as_str).and_then(RoomState::parse) {
                let back_online = st == RoomState::Online && r.state != RoomState::Online;
                r.state = st;
                if back_online {
                    notify(r, "collab/sync1", json!({"room": r.room, "stateVector": deckcraft_collab::b64_encode(&r.replica.state_vector())}));
                    r.last_awareness = None;
                    r.last_awareness_at = 0;
                }
            }
            if let Some(peers) = params.get("peers").and_then(Json::as_array) {
                r.listed = peers.iter().take(1000).filter_map(|p| p.get("user").and_then(User::parse)).collect();
            }
            r.message = params.get("message").and_then(Json::as_str).map(|m| m.chars().take(300).collect());
            if let Some(m) = &r.message {
                r.notices.push(m.clone());
            }
            if r.state == RoomState::Ended {
                let why = r.message.clone().unwrap_or_else(|| "the room was closed".into());
                leave_room(s, &room, Some(&why), false);
            }
            true
        }
        "collab/leave" => {
            let why = params.get("reason").and_then(Json::as_str).unwrap_or("the transport closed the room").to_string();
            leave_room(s, &room, Some(&why), false);
            true
        }
        _ => {
            log::debug!("add-in {}: ignored {method}", p.plugin);
            false
        }
    }
}

/// `collab/snapshot`: the room's whole state, and the presentation's file when asked (`file:
/// true`; in the `format` given when DeckCraft writes it, else `.pptx`; not in an encrypted room:
/// the transport would see the content).
fn snapshot(s: &mut Session, plugin: &str, params: &Json) -> std::result::Result<Json, deckcraft_plugin::HostError> {
    let err = |m: String| deckcraft_plugin::HostError { code: -32000, message: m, data: Json::Null };
    let room = params.get("room").and_then(Json::as_str).ok_or_else(|| err("missing `room`".into()))?.to_string();
    let i = s.collab.rooms.iter().position(|r| r.room == room && r.plugin == plugin).ok_or_else(|| err(format!("not in room {room}")))?;
    let mut r = s.collab.rooms.remove(i);
    flush(s, &mut r);
    let out = (|| {
        if !r.replica.is_ready() {
            return Err(err("the room's document hasn't arrived yet".into()));
        }
        let update = seal(&r, "collab/snapshot", &r.replica.full_state()).map_err(|e| err(e.to_string()))?;
        let mut out = json!({"update": update});
        if params.get("file").and_then(Json::as_bool) == Some(true)
            && let Some(o) = out.as_object_mut()
        {
            if r.key.is_some() {
                o.insert("fileRefused".into(), json!("the room is encrypted: the presentation stays with the people in it"));
            } else if let Some(d) = s.docs.iter().find(|d| d.uid == r.doc) {
                // The `format` hint (craft-live's nextcloud-live: the file's own extension) when
                // DeckCraft writes it; `.pptx` otherwise, and the transport keeps it as a copy.
                let asked = params.get("format").and_then(Json::as_str).unwrap_or("pptx");
                let (fmt, _) = crate::live::save_target(asked, "");
                let name = {
                    let t = d.title();
                    let ext = format!(".{fmt}");
                    if t.to_ascii_lowercase().ends_with(&ext) { t } else { format!("{t}{ext}") }
                };
                let bytes = crate::cmd::file::save_bytes(&d.doc, fmt).map_err(|e| err(e.to_string()))?;
                o.insert("file".into(), json!(deckcraft_collab::b64_encode(&bytes)));
                o.insert("name".into(), json!(name));
                o.insert("format".into(), json!(fmt));
            }
        }
        Ok(out)
    })();
    s.collab.rooms.insert(i, r);
    out
}

/// A presentation's shared content, for comparing peers (tests, diagnostics).
pub fn fingerprint(p: &Presentation) -> String {
    deckcraft_collab::fingerprint(p)
}

/// The slide a peer is on and the shapes they selected, by id (the people circles' click).
pub fn peer_target(s: &Session, client_id: u64) -> Option<(usize, Vec<ShapeId>)> {
    let r = s.active_room()?;
    r.peers().into_iter().find(|p| p.client_id == client_id)?.selection
}

/// The slide id of a slide index (others' highlights name slides by id).
pub fn slide_id_at(p: &Presentation, index: usize) -> Option<SlideId> {
    p.slides.get(index).map(|s| s.id)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn the_host_announces_what_the_core_speaks() {
        assert_eq!(deckcraft_plugin::COLLAB_FORMAT, deckcraft_collab::FORMAT);
        assert_eq!(deckcraft_plugin::COLLAB_SCHEMA, deckcraft_collab::SCHEMA);
        assert_eq!(deckcraft_plugin::HOST_ID, deckcraft_collab::APP);
    }

    #[test]
    fn commands_refuse_bad_params_and_programmatic_calls_open_nothing_but_the_share_window() {
        let mut s = Session::with_new();
        // No params: the interface's Share window, nothing joined.
        s.execute("collab.share", &json!({})).unwrap();
        s.execute("collab.join", &json!({})).unwrap();
        assert_eq!(s.ui_requests.iter().filter(|r| matches!(r, crate::UiRequest::Dialog { id, .. } if id == "collabShare")).count(), 2);
        assert!(s.collab.rooms.is_empty());
        for bad in [
            json!({"room": "r"}),
            json!({"plugin": "nope", "room": "r"}),
            json!({"plugin": "nope", "room": ""}),
            json!({"plugin": "nope", "room": "r", "mode": "fly"}),
            json!({"plugin": "nope", "room": "r", "role": "god"}),
            json!({"plugin": "nope", "room": "r", "key": "short"}),
            json!({"plugin": "nope", "room": "r", "user": {"id": "", "name": "x", "color": "red"}}),
        ] {
            assert!(s.execute("collab.join", &bad).is_err(), "{bad}");
        }
        assert!(s.execute("collab.leave", &json!({})).is_err(), "not in a session");
        assert!(s.execute("collab.setUser", &json!({"color": "blue"})).is_err());
        let u = s.execute("collab.setUser", &json!({"name": "Ada", "color": "#00aa00"})).unwrap();
        assert_eq!((u["name"].as_str(), u["color"].as_str()), (Some("Ada"), Some("#00AA00")));
        let st = s.execute("collab.status", &json!({})).unwrap();
        assert_eq!(st["rooms"], json!([]));
        // One room per presentation; the same room twice is refused.
        s.collab_join_in_memory(json!({"room": "r1", "mode": "share"})).unwrap();
        assert!(s.collab_join_in_memory(json!({"room": "r2", "mode": "share"})).is_err());
        assert!(s.collab_join_in_memory(json!({"room": "r1", "mode": "join"})).is_err());
        assert_eq!(s.execute("collab.status", &json!({})).unwrap()["rooms"][0]["room"], "r1");
        s.execute("collab.leave", &json!({})).unwrap();
        assert!(s.collab.rooms.is_empty());
    }
}
