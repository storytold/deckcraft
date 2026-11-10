//! Collaboration UI: the feature flag, the sync indicator, the participants pane, remote
//! presence on the canvas and the viewer gate (RFC §10, ADR 0006 and 0008).
//!
//! Nothing here talks to a socket. [`CollabUi`] is fed by the transport layer
//! (`deckcraft-collab`'s `ClientSession`, presence messages and commit results) and the UI reads
//! it. With the flag off every function here is a no-op and the single-user application behaves
//! exactly as before.

use std::collections::VecDeque;

use deckcraft_collab::{ClientSession, CollabDoc, KeyIssuer, KeyMap, Link, Participant, PresenceSet, Transport};
use deckcraft_collab_proto::{ClientMsg, ServerMsg};
use deckcraft_collab_proto::{PresenceMsg, Role, ShapeKey, SlideKey, TextCursor};
use deckcraft_engine::{DocState, Target, TextSel, cmd::xfrm_of};
use deckcraft_geom::Point;
use deckcraft_model::{Presentation, Shape, ShapeId, SlideId};
use egui::{Align2, Color32, CornerRadius, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::SlideApp;
use crate::canvas::Xf;
use crate::theme::{self, Tokens};

/// The task pane the participants list lives in.
pub const PANE: &str = "collab";

/// How the connection looks to the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    /// The `collab` feature flag is off.
    Off,
    Connecting,
    Synced,
    Syncing,
    /// The link is down; queued operations are kept for the reconnect.
    Offline,
    /// A fatal protocol or session error; the session needs attention.
    Error,
}

impl SyncStatus {
    pub fn label(self) -> &'static str {
        match self {
            SyncStatus::Off => "Off",
            SyncStatus::Connecting => "Connecting…",
            SyncStatus::Synced => "Synced",
            SyncStatus::Syncing => "Syncing…",
            SyncStatus::Offline => "Offline",
            SyncStatus::Error => "Sync error",
        }
    }

    fn color(self, t: &Tokens) -> Color32 {
        match self {
            SyncStatus::Synced => Color32::from_rgb(0x16, 0xa3, 0x4a),
            SyncStatus::Connecting | SyncStatus::Syncing => Color32::from_rgb(0xd9, 0x77, 0x06),
            SyncStatus::Error => Color32::from_rgb(0xdc, 0x26, 0x26),
            SyncStatus::Offline | SyncStatus::Off => t.text_dim,
        }
    }
}

/// How the last Save to the cabinet ended (ADR 0009).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitState {
    /// No commit has been asked for in this session.
    Idle,
    /// The server is building the file and the cabinet has not answered yet.
    Committing,
    /// The cabinet took the file; the number is the document version it represents.
    Committed(u64),
    /// The cabinet refused the commit (a `409` after an external change, a build failure). The
    /// document stays dirty and the user is offered a copy, never an overwrite (ADR 0009 §4).
    Refused(String),
}

impl CommitState {
    /// The status-bar line for this state, if it has one worth showing.
    pub fn label(&self) -> Option<String> {
        match self {
            CommitState::Idle => None,
            CommitState::Committing => Some("Committing…".into()),
            CommitState::Committed(version) => Some(format!("Committed v{version}")),
            CommitState::Refused(reason) => Some(format!("Commit refused: {reason}")),
        }
    }
}

/// Collaboration state the UI draws. Disabled until the host turns the feature flag on.
pub struct CollabUi {
    enabled: bool,
    actor_id: Option<String>,
    role: Option<Role>,
    link: Link,
    version: u64,
    queued: usize,
    in_flight: usize,
    error: Option<String>,
    presence: PresenceSet,
    keys: KeyMap,
    issuer: KeyIssuer,
    /// Document and revision the key map was last synced with.
    keys_for: Option<(u64, u64)>,
    /// What the last commit could not keep (ADR 0009); shown until dismissed.
    warnings: Vec<String>,
    warnings_hidden: bool,
    /// The connected client, when a host attached one.
    doc: Option<CollabDoc>,
    transport: Option<Box<dyn Transport>>,
    /// Messages popped from the client while the socket was down.
    pending: VecDeque<ClientMsg>,
    /// Presence we sent last, and when, so an idle session does not spam the room.
    sent_presence: Option<PresenceMsg>,
    sent_presence_ms: u64,
    /// A local operation id prefix: actor plus a monotonic counter.
    op_seq: u64,
    /// The last committed file, for the host to store (sha256, bytes, warnings).
    commit: Option<(String, Vec<u8>, Vec<String>)>,
    /// How the last Save to the cabinet ended, and the line the status bar shows once.
    commit_state: CommitState,
    commit_notice: Option<String>,
    /// The document and revision the commit was asked for. The answer clears the dirty mark only
    /// while the document is still that revision: edits typed while the file was being built are
    /// not in the committed bytes.
    commit_mark: Option<(u64, u64)>,
    /// Show local edits before the room confirms them (RFC §3). Off by default; forwarded to a
    /// client attached later.
    preview: bool,
}

impl Default for CollabUi {
    fn default() -> Self {
        CollabUi {
            enabled: false,
            actor_id: None,
            role: None,
            link: Link::Down,
            version: 0,
            queued: 0,
            in_flight: 0,
            error: None,
            presence: PresenceSet::default(),
            keys: KeyMap::new(),
            issuer: KeyIssuer::new("local"),
            keys_for: None,
            warnings: Vec::new(),
            warnings_hidden: false,
            doc: None,
            transport: None,
            pending: VecDeque::new(),
            sent_presence: None,
            sent_presence_ms: 0,
            op_seq: 0,
            commit: None,
            commit_state: CommitState::Idle,
            commit_notice: None,
            commit_mark: None,
            preview: false,
        }
    }
}

impl CollabUi {
    /// A disabled state. `prefix` feeds the local key issuer and must match the session's on the
    /// other replicas (the transport replaces the map with the server's on join).
    pub fn new(prefix: impl Into<String>) -> Self {
        CollabUi { issuer: KeyIssuer::new(prefix), ..Self::default() }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Turn the feature on: the UI now shows a session and starts in `Connecting` until the
    /// transport reports the handshake.
    pub fn enable(&mut self) {
        self.enabled = true;
        self.link = Link::Connecting;
        self.error = None;
    }

    /// Attach a connected client and its transport (a desktop socket, the browser link, or a
    /// test pipe). Without one the flag is on but edits are refused: they would diverge.
    pub fn attach(&mut self, mut doc: CollabDoc, transport: Box<dyn Transport>) {
        self.enabled = true;
        self.link = Link::Connecting;
        doc.set_preview(self.preview);
        self.doc = Some(doc);
        self.transport = Some(transport);
    }

    /// Whether a local edit is visible before the room confirms it (RFC §3).
    pub fn preview(&self) -> bool {
        self.preview
    }

    /// Turn optimistic preview on or off for this session. The flag is a host setting
    /// (`DECKCRAFT_COLLAB_PREVIEW` on the desktop); it is off unless a host turns it on, and a
    /// client attached later inherits it.
    pub fn set_preview(&mut self, on: bool) {
        self.preview = on;
        if let Some(doc) = &mut self.doc {
            doc.set_preview(on);
        }
    }

    /// Whether the last local edit is shown and not yet confirmed.
    pub fn is_previewing(&self) -> bool {
        self.doc.as_ref().is_some_and(CollabDoc::is_previewing)
    }

    /// How many local commands wait behind that edit (their payloads must not include it).
    pub fn deferred(&self) -> usize {
        self.doc.as_ref().map_or(0, CollabDoc::deferred)
    }

    /// Start the desktop WebSocket link for `file_id`. The host supplies the token from the
    /// cabinet session; only the first frame carries it (ADR 0008).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_native(&mut self, session: &deckcraft_engine::Session, file_id: &str, url: &str, jwt: &str) -> Result<(), String> {
        let info = deckcraft_collab_proto::ClientInfo { name: "DeckCraft".into(), ver: env!("CARGO_PKG_VERSION").into() };
        let mut doc = CollabDoc::new(file_id);
        let hello = doc.connect(session, jwt, info);
        if let Some(fetch) = deckcraft_collab::WsTransport::media_fetcher(url, jwt) {
            doc.set_media_fetcher(fetch);
        }
        let transport = deckcraft_collab::WsTransport::start(url, hello);
        self.attach(doc, Box::new(transport));
        Ok(())
    }

    pub fn is_attached(&self) -> bool {
        self.doc.is_some()
    }

    /// Normalise and queue one local command. Without preview the document changes only when the
    /// server answers; with it the document shows the edit at once and the answer settles it
    /// (RFC §3).
    pub fn submit(&mut self, session: &mut deckcraft_engine::Session, cmd: &str, params: &serde_json::Value) -> Result<usize, String> {
        let doc = self.doc.as_mut().ok_or_else(|| "collaboration is not connected".to_string())?;
        self.op_seq = self.op_seq.saturating_add(1);
        let actor = self.actor_id.clone().unwrap_or_else(|| "local".into());
        doc.submit(session, cmd, params, &format!("{actor}:{}", self.op_seq)).map_err(|e| e.to_string())
    }

    /// Ask the server to undo/redo this actor's latest step.
    pub fn undo(&mut self) -> Result<(), String> {
        self.doc.as_mut().ok_or_else(|| "collaboration is not connected".to_string())?.undo().map_err(|e| e.to_string())
    }

    pub fn redo(&mut self) -> Result<(), String> {
        self.doc.as_mut().ok_or_else(|| "collaboration is not connected".to_string())?.redo().map_err(|e| e.to_string())
    }

    /// The committed file, if a `commit-result` arrived and the host has not taken it yet.
    pub fn take_commit(&mut self) -> Option<(String, Vec<u8>, Vec<String>)> {
        self.commit.take()
    }

    /// Save in a live session: ask the server to build the file and commit it as a new cabinet
    /// version (ADR 0009). The local file is not written; Save As and Export still are.
    ///
    /// Only a writer may ask — a viewer (and a commenter, which is a viewer in this protocol) is
    /// refused before anything is sent. A second Save while the commit is on its way is the same
    /// request, so it creates no second file version.
    pub fn commit(&mut self, session: &deckcraft_engine::Session) -> Result<(), String> {
        if self.doc.is_none() {
            return Err("collaboration is not connected".into());
        }
        if self.link != Link::Live {
            return Err("the session is offline: Save a copy keeps your work".into());
        }
        if !self.may_write() {
            return Err("View only: you cannot commit the presentation".into());
        }
        let expected = self.doc.as_ref().and_then(|doc| doc.file_version()).map(str::to_string);
        self.doc.as_mut().ok_or("collaboration is not connected")?.commit(expected).map_err(|e| e.to_string())?;
        self.commit_state = CommitState::Committing;
        self.commit_notice = Some("Committing…".into());
        self.commit_mark = session.active().map(|d| (d.uid, d.revision));
        Ok(())
    }

    /// Whether Save belongs to the cabinet: a live native session in which this client writes.
    pub fn owns_save(&self) -> bool {
        self.enabled && self.doc.is_some() && self.link == Link::Live
    }

    /// How the last Save to the cabinet ended.
    pub fn commit_state(&self) -> &CommitState {
        &self.commit_state
    }

    /// The status line for the last commit transition, for the host to show once.
    pub fn take_commit_notice(&mut self) -> Option<String> {
        self.commit_notice.take()
    }

    /// Pump the link: send what the client queued, apply what the server sent, and keep presence
    /// fresh. Called once per frame before the UI is laid out.
    pub fn pump(&mut self, session: &mut deckcraft_engine::Session, now_ms: u64) {
        if !self.enabled {
            return;
        }
        let mut incoming = Vec::new();
        if let Some(transport) = &mut self.transport {
            while let Some(msg) = transport.next() {
                incoming.push(msg);
            }
        }
        for msg in incoming {
            self.handle_server_msg(session, msg, now_ms);
        }
        for (cmd, params) in session.take_pending_commands() {
            let result = match cmd.as_str() {
                "edit.undo" => self.undo(),
                "edit.redo" => self.redo(),
                _ => match self.submit(session, &cmd, &params) {
                    Ok(_) => {
                        // The painter clipboard is a session value and the confirmed command is
                        // `slide.animations.set`, so the single-user `painterApply` never runs
                        // here; disarm a one-shot pick as that command would.
                        if cmd == "animation.painterApply" && session.anim_painter.as_ref().is_some_and(|(_, sticky)| !sticky) {
                            session.anim_painter = None;
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                },
            };
            if let Err(e) = result {
                self.error = Some(e);
            }
        }
        let uploads = self.doc.as_mut().map(CollabDoc::take_uploads).unwrap_or_default();
        for (sha, bytes) in uploads {
            match self.transport.as_mut() {
                Some(transport) => {
                    if let Err(e) = transport.put_media(&sha, &bytes) {
                        self.error = Some(format!("media upload: {e}"));
                    }
                }
                None => self.error = Some("media upload: collaboration is not connected".into()),
            }
        }
        while let Some(msg) = self.doc.as_mut().and_then(CollabDoc::next_outgoing) {
            if !self.send_or_queue(msg) {
                break;
            }
        }
        self.tick_presence(session, now_ms);
        let _ = self.tick_expire(now_ms);
    }

    fn send_or_queue(&mut self, msg: ClientMsg) -> bool {
        match &mut self.transport {
            Some(transport) => match transport.send(msg) {
                Ok(()) => true,
                Err(e) => {
                    self.error = Some(e);
                    false
                }
            },
            None => {
                self.pending.push_back(msg);
                false
            }
        }
    }

    fn handle_server_msg(&mut self, session: &mut deckcraft_engine::Session, msg: ServerMsg, now_ms: u64) {
        {
            let echo = msg.clone();
            if let Some(doc) = &mut self.doc {
                let _ = doc.on_msg(session, msg);
                if let Some(err) = doc.last_apply_error() {
                    self.error = Some(err.to_string());
                }
            }
            self.refresh_client();
            match echo {
                deckcraft_collab_proto::ServerMsg::Welcome(w) => {
                    self.welcome(w.actor_id.clone(), w.role, w.version);
                    self.presence.set_role(&w.actor_id, w.role);
                }
                deckcraft_collab_proto::ServerMsg::Rejected(r) => {
                    self.error = Some(r.message.clone());
                    if r.op_id == "commit" {
                        self.on_commit_refused(r.message.clone());
                    }
                    if r.retryable {
                        self.link = Link::Connecting;
                    }
                }
                deckcraft_collab_proto::ServerMsg::CommitResult(r) => self.on_commit_result(session, r),
                deckcraft_collab_proto::ServerMsg::Error(e) => {
                    self.error = Some(e.message.clone());
                    self.link = Link::Down;
                    self.pending.clear();
                    if matches!(self.commit_state, CommitState::Committing) {
                        self.on_commit_refused("the session dropped before the cabinet answered".into());
                    }
                    if let Some(doc) = &mut self.doc {
                        doc.disconnected();
                    }
                }
                deckcraft_collab_proto::ServerMsg::Presence(p) => self.on_presence(p, now_ms),
                _ => {}
            }
        }
    }

    fn tick_presence(&mut self, session: &deckcraft_engine::Session, now_ms: u64) {
        let Some(st) = session.active() else { return };
        let Some(presence) = self.local_presence(st) else { return };
        let changed = self.sent_presence.as_ref() != Some(&presence);
        if changed && now_ms.saturating_sub(self.sent_presence_ms) >= 50 {
            self.sent_presence = Some(presence.clone());
            self.sent_presence_ms = now_ms;
            if let Some(transport) = &mut self.transport
                && transport.is_open()
            {
                let _ = transport.send(ClientMsg::Presence(presence));
            }
        }
    }

    fn tick_expire(&mut self, now_ms: u64) -> bool {
        self.presence.expire(now_ms) > 0
    }

    fn on_commit_result(&mut self, session: &mut deckcraft_engine::Session, r: deckcraft_collab_proto::CommitResult) {
        self.warnings_hidden = false;
        self.warnings = r.warnings.clone();
        if let Some(encoded) = r.bytes_base64.as_deref() {
            use base64::Engine as _;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                self.commit = Some((r.sha256, bytes, r.warnings));
            }
        }
        // The cabinet took the file: only now is the document saved. Edits typed while the file
        // was being built are not in those bytes, so a changed revision stays dirty.
        let unchanged = self.commit_mark.is_some() && self.commit_mark == session.active().map(|d| (d.uid, d.revision));
        self.commit_mark = None;
        self.commit_state = CommitState::Committed(r.version);
        let saved = unchanged && deckcraft_engine::cmd::file::mark_saved(session, None).is_ok();
        self.commit_notice = Some(match (r.version, unchanged) {
            (version, true) if saved => format!("Committed v{version}"),
            (version, _) => format!("Committed v{version} — newer edits are not saved"),
        });
    }

    /// The cabinet (or the server) refused the commit. The document stays dirty: the user's
    /// edits are intact and Save As keeps them (ADR 0009 §4: never overwrite a version that moved
    /// on). A banner says so until dismissed.
    fn on_commit_refused(&mut self, reason: String) {
        self.commit_mark = None;
        self.commit_state = CommitState::Refused(reason.clone());
        self.commit_notice = Some(format!("Commit refused: {reason}"));
        self.warnings_hidden = false;
    }

    fn keys(&self) -> &KeyMap {
        self.doc.as_ref().map_or(&self.keys, |doc| doc.keys())
    }

    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    pub fn role(&self) -> Option<Role> {
        self.role
    }

    /// An unjoined session is treated as editable: the server decides the real role in `welcome`.
    pub fn may_write(&self) -> bool {
        self.role.is_none_or(Role::may_write)
    }

    pub fn link(&self) -> Link {
        self.link
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// The server's `welcome`: who we are, what we may do and where the document is.
    pub fn welcome(&mut self, actor_id: impl Into<String>, role: Role, version: u64) {
        self.enabled = true;
        self.actor_id = Some(actor_id.into());
        self.role = Some(role);
        self.version = version;
        self.link = Link::Live;
        self.error = None;
    }

    pub fn set_role(&mut self, role: Option<Role>) {
        self.role = role;
    }

    pub fn set_link(&mut self, link: Link) {
        self.link = link;
    }

    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    /// Copy the client's state without holding a borrow of it: the UI fields are what the
    /// status bar and the gate read.
    fn refresh_client(&mut self) {
        let snapshot = self.doc.as_ref().map(|doc| {
            let c = doc.client();
            (c.actor_id().map(str::to_string), c.role(), c.link(), c.version(), c.queued(), c.in_flight(), c.last_error().map(str::to_string))
        });
        if let Some((actor, role, link, version, queued, in_flight, error)) = snapshot {
            self.enabled = true;
            self.actor_id = actor;
            self.role = role;
            self.link = link;
            self.version = version;
            self.queued = queued;
            self.in_flight = in_flight;
            self.error = error;
        }
    }

    /// Mirror the state machine after the transport moved a message. Keeps the UI from
    /// duplicating the client's queue and version bookkeeping.
    pub fn sync_from(&mut self, client: &ClientSession) {
        self.enabled = true;
        self.actor_id = client.actor_id().map(str::to_string);
        self.role = client.role();
        self.link = client.link();
        self.version = client.version();
        self.queued = client.queued();
        self.in_flight = client.in_flight();
        self.error = client.last_error().map(str::to_string);
    }

    pub fn status(&self) -> SyncStatus {
        if !self.enabled {
            return SyncStatus::Off;
        }
        if self.error.is_some() {
            return SyncStatus::Error;
        }
        match self.link {
            Link::Connecting => SyncStatus::Connecting,
            Link::Down => SyncStatus::Offline,
            Link::Live if self.queued + self.in_flight > 0 => SyncStatus::Syncing,
            Link::Live => SyncStatus::Synced,
        }
    }

    pub fn presence(&self) -> &PresenceSet {
        &self.presence
    }

    /// Record the latest presence of one participant (ADR 0006: latest state wins).
    pub fn on_presence(&mut self, msg: PresenceMsg, now_ms: u64) {
        self.presence.update(msg, now_ms);
    }

    pub fn set_participant_role(&mut self, actor_id: &str, role: Role) {
        self.presence.set_role(actor_id, role);
    }

    /// Drop participants whose last message is older than the TTL. `true` when someone left.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.presence.expire(now_ms) > 0
    }

    pub fn participants(&self) -> Vec<&Participant> {
        self.presence.participants()
    }

    /// Keep the session-scoped key map in step with the document. Cheap when nothing changed:
    /// the walk only runs when the document's revision moved.
    pub fn sync_keys(&mut self, uid: u64, revision: u64, doc: &Presentation) {
        if self.doc.is_some() || self.keys_for == Some((uid, revision)) {
            return;
        }
        self.keys.sync(doc, &mut self.issuer);
        self.keys_for = Some((uid, revision));
    }

    pub fn slide_key(&self, id: SlideId) -> Option<&SlideKey> {
        self.keys().slide_key(id)
    }

    pub fn slide_id(&self, key: &SlideKey) -> Option<SlideId> {
        self.keys().slide_id(key)
    }

    pub fn shape_id(&self, slide: SlideId, key: &ShapeKey) -> Option<ShapeId> {
        self.keys().shape_id(slide, key)
    }

    /// Adopt a key map issued elsewhere (the transport seeds it from the server's snapshot).
    pub fn set_keys(&mut self, keys: KeyMap, uid: u64, revision: u64) {
        self.keys = keys;
        self.keys_for = Some((uid, revision));
    }

    /// Commit warnings from ADR 0009, shown above the ribbon until dismissed.
    pub fn set_warnings(&mut self, warnings: Vec<String>) {
        self.warnings_hidden = false;
        self.warnings = warnings;
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn dismiss_banner(&mut self) {
        self.warnings_hidden = true;
        if matches!(self.commit_state, CommitState::Refused(_)) {
            self.commit_state = CommitState::Idle;
        }
    }

    pub fn banner_text(&self) -> Option<String> {
        if self.warnings_hidden {
            return None;
        }
        // A refused commit outranks warnings: the file was not saved, and the way out is a copy.
        if let CommitState::Refused(reason) = &self.commit_state {
            return Some(format!("Commit refused: {reason}. Save a copy (File › Save As) keeps your edits."));
        }
        if self.warnings.is_empty() {
            return None;
        }
        let n = self.warnings.len();
        let shown: Vec<&str> = self.warnings.iter().take(2).map(String::as_str).collect();
        let rest = if n > shown.len() { "; …" } else { "" };
        let plural = if n == 1 { "" } else { "s" };
        Some(format!("Saved with {n} warning{plural}: {}{rest}", shown.join("; ")))
    }

    /// What to send as this participant's presence: active slide, selected shapes and the text
    /// caret, all in session keys. `None` when collaboration is off, there is no slide, or the
    /// handshake has not named us yet.
    pub fn local_presence(&self, st: &DocState) -> Option<PresenceMsg> {
        if !self.enabled || st.selection.target != Target::Slides {
            return None;
        }
        let actor_id = self.actor_id.clone()?;
        let current = st.doc.slides.get(st.selection.slide)?;
        let slide = self.keys().slide_key(current.id)?.clone();
        let shapes = st.selection.shapes.iter().filter_map(|shape| self.keys().shape_key(current.id, *shape).cloned()).collect();
        let text = st.selection.text.as_ref().filter(|t| !t.notes).map(|t| TextCursor {
            cell: t.cell.map(|(r, c)| (r as u32, c as u32)),
            anchor: Some((t.anchor.0 as u32, t.anchor.1 as u32)),
            caret: Some((t.caret.0 as u32, t.caret.1 as u32)),
        });
        Some(PresenceMsg { actor_id: actor_id.clone(), slide, shapes, text, viewport: None, color: Some(deckcraft_collab::color_for(&actor_id)) })
    }

    /// Remote selections and carets for the current slide, in slide coordinates. Drawing and
    /// tests share this: it is the whole geometry, with no painter involved.
    pub(crate) fn marks(&self, st: &DocState) -> Vec<RemoteMark> {
        let mut out = Vec::new();
        if !self.enabled || st.selection.target != Target::Slides {
            return out;
        }
        let Some(slide) = st.doc.slides.get(st.selection.slide) else { return out };
        let Some(slide_key) = self.keys().slide_key(slide.id) else { return out };
        for p in self.presence.on_slide(slide_key) {
            if self.actor_id.as_deref() == Some(p.actor_id.as_str()) {
                continue;
            }
            let color = parse_color(&p.color).unwrap_or_else(|| Color32::from_rgb(0x3b, 0x82, 0xf6));
            let label = display_name(&p.actor_id);
            let mut first = true;
            let mut caret_done = false;
            for key in &p.shapes {
                let Some(id) = self.keys().shape_id(slide.id, key) else { continue };
                let Some(shape) = st.shape(id) else { continue };
                let x = xfrm_of(&st.doc, &st.selection, shape);
                let a = x.affine();
                let corners = [Point::new(0.0, 0.0), Point::new(x.w, 0.0), Point::new(x.w, x.h), Point::new(0.0, x.h)].map(|q| a * q);
                let caret = if caret_done { None } else { caret_of(st, shape, p.text.as_ref()) };
                caret_done |= caret.is_some();
                out.push(RemoteMark { corners, caret, color, label: first.then(|| label.clone()) });
                first = false;
            }
        }
        out
    }
}

/// One remote selection frame (and, for the first shape, a possible caret and name label).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RemoteMark {
    pub(crate) corners: [Point; 4],
    pub(crate) caret: Option<(Point, Point)>,
    pub(crate) color: Color32,
    /// The participant's name, on the first mark only.
    pub(crate) label: Option<String>,
}

/// The caret of a remote text cursor on `shape`, in slide coordinates. A cursor with no range
/// is a caret; a range could also be shaded, but presence only needs the caret to be useful.
fn caret_of(st: &DocState, shape: &Shape, cursor: Option<&TextCursor>) -> Option<(Point, Point)> {
    let cursor = cursor?;
    let caret = cursor.caret?;
    let anchor = cursor.anchor.unwrap_or(caret);
    let ts = TextSel {
        shape: shape.id,
        anchor: (anchor.0 as usize, anchor.1 as usize),
        caret: (caret.0 as usize, caret.1 as usize),
        cell: cursor.cell.map(|(r, c)| (r as usize, c as usize)),
        notes: false,
        line_end: None,
    };
    deckcraft_engine::cmd::text::caret_line(st, &ts)
}

/// A name short enough for a canvas label and a participants list: the local part of an email,
/// cut at 24 characters.
pub fn display_name(actor_id: &str) -> String {
    let name = actor_id.split('@').next().unwrap_or(actor_id);
    if name.chars().count() > 24 {
        let mut short: String = name.chars().take(23).collect();
        short.push('…');
        short
    } else {
        name.to_string()
    }
}

fn parse_color(hex: &str) -> Option<Color32> {
    let hex = hex.strip_prefix('#')?;
    let bytes = hex.as_bytes();
    if bytes.len() != 6 {
        return None;
    }
    // Byte slices, not string slicing: a remote colour is untrusted input and must not panic on
    // a multi-byte character at an offset.
    let channel = |i: usize| std::str::from_utf8(&bytes[i..i + 2]).ok().and_then(|s| u8::from_str_radix(s, 16).ok());
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
}

/// The feature flag on the desktop: `DECKCRAFT_COLLAB=1`. Absent or `0` leaves collaboration off.
#[cfg(not(target_arch = "wasm32"))]
pub fn enabled_from_env() -> bool {
    std::env::var("DECKCRAFT_COLLAB").is_ok_and(|v| !matches!(v.as_str(), "" | "0" | "false" | "no" | "off"))
}

/// Optimistic preview on the desktop: `DECKCRAFT_COLLAB_PREVIEW=1`. Off unless the host sets it;
/// the protocol does not change for a client that leaves it off (RFC §3).
#[cfg(not(target_arch = "wasm32"))]
pub fn preview_from_env() -> bool {
    std::env::var("DECKCRAFT_COLLAB_PREVIEW").is_ok_and(|v| !matches!(v.as_str(), "" | "0" | "false" | "no" | "off"))
}

/// The browser has no process environment: a page host asks for preview the way it asks for the
/// session (`CollabUi::set_preview`), so an unattended build leaves it off.
#[cfg(target_arch = "wasm32")]
pub fn preview_from_env() -> bool {
    false
}

impl SlideApp {
    /// Turn collaboration on for this application (host feature flag). The viewer gate follows
    /// the role as soon as `welcome` arrives.
    pub fn enable_collab(&mut self) {
        self.collab.enable();
        self.collab.set_preview(preview_from_env());
        self.sync_collab_gate();
        if let Some(st) = self.session.active() {
            self.collab.sync_keys(st.uid, st.revision, &st.doc);
        }
    }

    /// Connect the desktop app to a session. The token comes from the cabinet; it is only sent in
    /// the first WebSocket frame.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_collab_native(&mut self, file_id: &str, url: &str, jwt: &str) -> Result<(), String> {
        self.collab.start_native(&self.session, file_id, url, jwt)?;
        self.sync_collab_gate();
        Ok(())
    }

    /// Mirror the collaboration role into the engine's read-only gate. Called every frame and
    /// before every command, so a role change can never leave one mutating path open.
    pub fn sync_collab_gate(&mut self) {
        // A viewer is read-only. An editor whose session is on but not attached is also stopped:
        // a local edit would have nowhere to go.
        let attached = self.collab.is_attached();
        self.session.set_read_only(!self.collab.may_write() || (self.collab.is_enabled() && !attached));
        self.session.set_collab_queue(self.collab.is_enabled() && attached);
        // A participants pane remembered from a run with the flag on must not linger when the
        // host starts without collaboration.
        if !self.collab.is_enabled() && self.ui.pane.as_deref() == Some(PANE) {
            self.ui.pane = None;
        }
    }
}

/// The status-bar indicator (right side): a coloured dot and the sync state. A click toggles the
/// participants pane; the tooltip carries the version and the viewer note.
pub fn indicator(app: &mut SlideApp, ui: &mut Ui) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let status = app.collab.status();
    let color = status.color(&t);
    let text = format!("● {}", status.label());
    let width = ui.fonts_mut(|f| f.layout_no_wrap(text.clone(), theme::font(12.0), color).size().x) + 12.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, &text, theme::font(12.0), color);
    resp.widget_info(|| egui::WidgetInfo {
        current_text_value: Some(status.label().into()),
        ..egui::WidgetInfo::labeled(egui::Role::Button, true, "Collaboration")
    });
    let mut tip = format!("{} · version {}{}", status.label(), app.collab.version(), participants_suffix(app.collab.participants().len() + 1));
    if !app.collab.may_write() {
        tip.push_str("\nView only: you cannot change the presentation");
    }
    let resp = resp.on_hover_text(tip);
    if resp.clicked() {
        app.ui.pane = if app.ui.pane.as_deref() == Some(PANE) { None } else { Some(PANE.into()) };
    }
    resp
}

fn participants_suffix(n: usize) -> String {
    if n == 0 { String::new() } else { format!(" · {n} participant{}", if n == 1 { "" } else { "s" }) }
}

/// A row per participant: the local user first, then everyone the presence set knows, in the
/// set's stable order.
struct Row {
    name: String,
    color: Color32,
    role: Option<Role>,
    /// 1-based slide number, when it can be resolved in this document.
    slide: Option<usize>,
    me: bool,
}

fn rows(app: &SlideApp) -> Vec<Row> {
    let mut out = Vec::new();
    let Some(st) = app.session.active() else { return out };
    let doc = &st.doc;
    let slide_no = |key: &SlideKey| app.collab.slide_id(key).and_then(|id| doc.slides.iter().position(|s| s.id == id)).map(|i| i + 1);
    let self_name = app.collab.actor_id().map(display_name);
    out.push(Row {
        name: self_name.unwrap_or_else(|| "You".into()),
        color: parse_color(&deckcraft_collab::color_for(app.collab.actor_id().unwrap_or("you")))
            .unwrap_or_else(|| Color32::from_rgb(0x3b, 0x82, 0xf6)),
        role: app.collab.role(),
        slide: Some(st.selection.slide + 1),
        me: true,
    });
    for p in app.collab.participants() {
        if app.collab.actor_id() == Some(p.actor_id.as_str()) {
            continue;
        }
        out.push(Row {
            name: display_name(&p.actor_id),
            color: parse_color(&p.color).unwrap_or_else(|| Color32::from_rgb(0x3b, 0x82, 0xf6)),
            role: p.role,
            slide: slide_no(&p.slide),
            me: false,
        });
    }
    out
}

fn role_label(role: Option<Role>) -> &'static str {
    match role {
        Some(Role::Owner) => "Owner",
        Some(Role::Editor) => "Editor",
        Some(Role::Viewer) => "Viewer",
        None => "Participant",
    }
}

/// The participants task pane.
pub fn participants_pane(app: &SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let status = app.collab.status();
    ui.horizontal(|ui| {
        ui.label(RichText::new("●").color(status.color(&t)).size(12.0));
        ui.label(RichText::new(status.label()).size(12.0).color(t.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("v{}", app.collab.version())).size(11.0).color(t.text_faint));
        });
    });
    if !app.collab.may_write() {
        ui.add_space(4.0);
        ui.label(RichText::new("View only: editing is disabled.").size(12.0).color(t.text_dim));
    }
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    let all = rows(app);
    for row in &all {
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.5, row.color);
            let name = if row.me { format!("{} (you)", row.name) } else { row.name.clone() };
            ui.label(RichText::new(name).size(13.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(role_label(row.role)).size(11.0).color(t.text_dim));
            });
        });
        if let Some(n) = row.slide {
            ui.label(RichText::new(format!("Slide {n}")).size(11.0).color(t.text_faint));
        }
    }
    if all.len() == 1 {
        ui.add_space(6.0);
        let note = match status {
            SyncStatus::Offline | SyncStatus::Error => "Nobody else is connected.",
            SyncStatus::Connecting => "Waiting for the session…",
            _ => "No one else has joined yet.",
        };
        ui.label(RichText::new(note).size(12.0).color(t.text_faint));
    }
}

/// Draw remote selections and carets over the slide. Called from the canvas overlays; a no-op
/// when the flag is off.
pub(crate) fn paint_presence(app: &mut SlideApp, painter: &egui::Painter, xf: Xf) {
    if !app.collab.is_enabled() {
        return;
    }
    if let Some(st) = app.session.active() {
        let (uid, revision) = (st.uid, st.revision);
        app.collab.sync_keys(uid, revision, &st.doc);
    }
    let Some(st) = app.session.active() else { return };
    for mark in app.collab.marks(st) {
        let pts: Vec<egui::Pos2> = mark.corners.iter().map(|q| xf.to_screen(*q)).collect();
        painter.add(egui::Shape::closed_line(pts.clone(), Stroke::new(1.5, mark.color)));
        if let Some((a, b)) = mark.caret {
            painter.line_segment([xf.to_screen(a), xf.to_screen(b)], Stroke::new(2.0, mark.color));
        }
        if let Some(label) = mark.label {
            let galley = painter.layout_no_wrap(label, theme::font(11.0), Color32::WHITE);
            let top = pts.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
            let left = pts.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
            let at = pos2(left, top - galley.size().y - 6.0);
            let r = Rect::from_min_size(at, galley.size()).expand2(vec2(4.0, 2.0));
            painter.rect_filled(r, CornerRadius::same(3), mark.color);
            painter.galley(r.min + vec2(4.0, 2.0), galley, Color32::WHITE);
        }
    }
}

/// The commit warnings banner (ADR 0009): what the saved `.pptx` could not keep, above the
/// ribbon until dismissed.
pub fn warnings_banner(app: &mut SlideApp, ui: &mut Ui) {
    let Some(text) = app.collab.banner_text() else { return };
    let t = Tokens::get(ui.ctx());
    let (bg, fg) = if t.dark {
        (Color32::from_rgb(0x4a, 0x3b, 0x0f), Color32::from_rgb(0xfd, 0xe6, 0x8a))
    } else {
        (Color32::from_rgb(0xff, 0xf4, 0xce), Color32::from_rgb(0x66, 0x4d, 0x00))
    };
    egui::Panel::top("collab_warnings").frame(egui::Frame::NONE.fill(bg).inner_margin(egui::Margin::symmetric(12, 4))).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(text).color(fg).size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Dismiss").clicked() {
                    app.collab.dismiss_banner();
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_collab_proto::{ShapeKey, SlideKey, Viewport};
    use deckcraft_engine::Session;
    use deckcraft_model::Presentation;
    use serde_json::json;

    fn presence(actor: &str, slide: &str, shapes: Vec<&str>, caret: Option<(u32, u32)>) -> PresenceMsg {
        PresenceMsg {
            actor_id: actor.into(),
            slide: SlideKey(slide.into()),
            shapes: shapes.into_iter().map(|s| ShapeKey(s.into())).collect(),
            text: caret.map(|(p, c)| TextCursor { cell: None, anchor: Some((p, c)), caret: Some((p, c)) }),
            viewport: Some(Viewport { zoom: 1.0, x: 0.0, y: 0.0 }),
            color: None,
        }
    }

    fn deck() -> (Session, Presentation) {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("rect");
        s.apply_remote("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 120, 100]})).expect("ellipse");
        let doc = s.doc().expect("doc").doc.as_ref().clone();
        (s, doc)
    }

    #[test]
    fn the_flag_is_off_until_the_host_turns_it_on() {
        let ui = CollabUi::default();
        assert!(!ui.is_enabled());
        assert_eq!(ui.status(), SyncStatus::Off);
        assert!(ui.may_write(), "off means the ordinary single-user editor");
    }

    #[test]
    fn the_viewer_gate_covers_every_command_that_changes_the_document() {
        let mutating = [
            "edit.undo",
            "edit.redo",
            "file.new",
            "file.open",
            "file.openBytes",
            "file.adopt",
            "file.revert",
            "file.recovery.open",
            "text.compose",
            "text.commit",
            "text.deleteSurrounding",
            "shape.insert",
            "slide.delete",
        ];
        for id in mutating {
            let spec = deckcraft_engine::find_command(id).unwrap_or_else(|| panic!("{id} exists"));
            assert!(spec.changes_document(), "{id} must be refused to a viewer");
        }
        let allowed = ["slide.go", "slide.next", "edit.copy", "edit.select", "file.save", "file.export", "file.saveBytes", "text.move", "text.edit"];
        for id in allowed {
            let spec = deckcraft_engine::find_command(id).unwrap_or_else(|| panic!("{id} exists"));
            assert!(!spec.changes_document(), "{id} must stay available to a viewer");
        }
    }

    #[test]
    fn with_the_flag_off_the_app_is_an_ordinary_editor() {
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        assert!(!app.collab.is_enabled());
        assert!(!app.session.is_read_only());
        app.ui.pane = Some(PANE.into());
        app.sync_collab_gate();
        assert!(app.ui.pane.is_none(), "a remembered participants pane closes when the flag is off");
        app.run("slide.new", json!({"layout": "blank"})).expect("editing works");
    }

    #[test]
    fn status_follows_the_link_and_the_queue() {
        let mut ui = CollabUi::default();
        ui.enable();
        assert_eq!(ui.status(), SyncStatus::Connecting);
        ui.set_link(Link::Live);
        assert_eq!(ui.status(), SyncStatus::Synced);
        ui.queued = 2;
        assert_eq!(ui.status(), SyncStatus::Syncing);
        ui.queued = 0;
        ui.set_error(Some("bad token".into()));
        assert_eq!(ui.status(), SyncStatus::Error);
        ui.set_error(None);
        ui.set_link(Link::Down);
        assert_eq!(ui.status(), SyncStatus::Offline);
    }

    #[test]
    fn only_a_viewer_is_read_only() {
        let mut ui = CollabUi::default();
        assert!(ui.may_write());
        ui.welcome("alice@example.com", Role::Editor, 3);
        assert!(ui.may_write());
        ui.set_role(Some(Role::Viewer));
        assert!(!ui.may_write());
        ui.set_role(Some(Role::Owner));
        assert!(ui.may_write());
    }

    #[test]
    fn seen_through_the_session_a_viewer_cannot_change_the_document() {
        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        app.enable_collab();
        app.collab.welcome("victor@example.com", Role::Viewer, 1);
        app.sync_collab_gate();
        let before = app.session.doc().expect("doc").doc.slides.len();
        assert!(app.run("slide.new", json!({"layout": "blank"})).is_err(), "a viewer's insert is refused");
        assert_eq!(app.session.doc().expect("doc").doc.slides.len(), before);

        // With collaboration off, the ordinary engine path is unchanged.
        app.collab = CollabUi::default();
        app.sync_collab_gate();
        app.run("slide.new", json!({"layout": "blank"})).expect("an editor may insert");
    }

    #[test]
    fn the_engine_gate_refuses_mutations_and_lets_queries_and_remote_operations_through() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).expect("one undo step to protect");
        let before = s.doc().expect("doc").doc.slides.len();
        s.set_read_only(true);
        assert!(s.execute("slide.new", &json!({"layout": "blank"})).is_err());
        assert!(s.execute("edit.undo", &json!({})).is_err(), "a viewer cannot rewind the document");
        assert_eq!(s.doc().expect("doc").doc.slides.len(), before);
        assert!(s.execute("file.info", &json!({})).is_ok(), "queries still work");
        assert!(s.execute("slide.next", &json!({})).is_ok(), "navigation still works");
        assert!(s.execute("file.saveBytes", &json!({})).is_ok(), "a viewer can save a copy");
        s.apply_remote("slide.new", &json!({"layout": "blank"})).expect("remote edits land");
        assert_eq!(s.doc().expect("doc").doc.slides.len(), before + 1);
        s.set_read_only(false);
        s.execute("slide.new", &json!({"layout": "blank"})).expect("editing is back");
    }

    #[test]
    fn presence_expires_and_keeps_the_latest_state() {
        let mut ui = CollabUi::default();
        ui.enable();
        ui.on_presence(presence("bob@example.com", "s1", vec!["h1"], None), 1_000);
        ui.on_presence(presence("bob@example.com", "s2", vec!["h2"], None), 2_000);
        assert_eq!(ui.participants().len(), 1);
        assert_eq!(ui.participants()[0].slide.0, "s2");
        assert!(!ui.tick(30_000), "still inside the 30 s window");
        assert!(ui.tick(32_000), "expired after it");
        assert!(ui.participants().is_empty());
    }

    #[test]
    fn the_key_map_resolves_presence_back_to_shapes() {
        let (_s, doc) = deck();
        let mut ui = CollabUi::new("test");
        ui.enable();
        ui.sync_keys(1, 1, &doc);
        let slide = doc.slides.first().expect("slide");
        let slide_key = ui.slide_key(slide.id).expect("slide key").clone();
        let shape = slide.shapes.first().expect("shape").id;
        let shape_key = ui.keys.shape_key(slide.id, shape).expect("shape key").clone();
        assert_eq!(ui.shape_id(slide.id, &shape_key), Some(shape));
        assert_eq!(ui.slide_id(&slide_key), Some(slide.id));
    }

    #[test]
    fn remote_marks_name_the_participant_once_and_carry_the_caret() {
        let (s, doc) = deck();
        let mut ui = CollabUi::new("test");
        ui.enable();
        ui.welcome("alice@example.com", Role::Editor, 1);
        ui.sync_keys(1, 1, &doc);
        let st = s.doc().expect("doc");
        let slide = doc.slides.first().expect("slide");
        let slide_key = ui.slide_key(slide.id).expect("key").clone();
        let a = slide.shapes[0].id;
        let b = slide.shapes[1].id;
        let ka = ui.keys.shape_key(slide.id, a).expect("a").clone();
        let kb = ui.keys.shape_key(slide.id, b).expect("b").clone();
        ui.on_presence(presence("bob@example.com", &slide_key.0, vec![&ka.0, &kb.0], Some((0, 0))), 1_000);
        let marks = ui.marks(st);
        assert_eq!(marks.len(), 2, "one frame per selected shape");
        assert_eq!(marks[0].label.as_deref(), Some("bob"), "the name is not repeated");
        assert!(marks[1].label.is_none());
        assert!(marks.iter().any(|m| m.caret.is_some()), "the caret rides the first shape");
        let expected = parse_color(&deckcraft_collab::color_for("bob@example.com")).expect("colour");
        assert!(marks.iter().all(|m| m.color == expected));
    }

    #[test]
    fn our_presence_names_the_selected_shape_in_session_keys() {
        let (mut s, doc) = deck();
        let mut ui = CollabUi::new("test");
        ui.enable();
        ui.welcome("alice@example.com", Role::Editor, 1);
        ui.sync_keys(1, 1, &doc);
        let shape = doc.slides[0].shapes[0].id;
        s.execute("edit.select", &json!({"ids": [shape]})).expect("select");
        let msg = ui.local_presence(s.doc().expect("doc")).expect("presence");
        let key = ui.keys.shape_key(doc.slides[0].id, shape).expect("key").clone();
        assert_eq!(msg.shapes, vec![key]);
        assert_eq!(msg.slide, ui.keys.slide_key(doc.slides[0].id).expect("slide key").clone());
        assert_eq!(msg.actor_id, "alice@example.com");

        let mut unnamed = CollabUi::new("test");
        unnamed.enable();
        assert!(unnamed.local_presence(s.doc().expect("doc")).is_none(), "no actor, nothing to send");
    }

    #[test]
    fn our_own_presence_is_not_drawn() {
        let (s, doc) = deck();
        let mut ui = CollabUi::new("test");
        ui.enable();
        ui.welcome("alice@example.com", Role::Editor, 1);
        ui.sync_keys(1, 1, &doc);
        let slide = doc.slides.first().expect("slide");
        let key = ui.slide_key(slide.id).expect("key").clone();
        let shape = ui.keys.shape_key(slide.id, slide.shapes[0].id).expect("shape").clone();
        ui.on_presence(presence("alice@example.com", &key.0, vec![&shape.0], None), 1_000);
        assert!(ui.marks(s.doc().expect("doc")).is_empty());
    }

    #[test]
    fn the_participants_rows_start_with_us_and_keep_roles_and_slides() {
        let (s, doc) = deck();
        let mut app = SlideApp::new(s, crate::Services::default());
        app.enable_collab();
        app.collab.welcome("alice@example.com", Role::Owner, 2);
        app.collab.sync_keys(1, 1, &doc);
        let slide = doc.slides.first().expect("slide");
        let key = app.collab.slide_key(slide.id).expect("key").clone();
        app.collab.on_presence(presence("bob@example.com", &key.0, vec![], None), 1_000);
        app.collab.set_participant_role("bob@example.com", Role::Viewer);
        let rows = rows(&app);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].me);
        assert_eq!(rows[0].name, "alice");
        assert_eq!(rows[0].role, Some(Role::Owner));
        assert_eq!(rows[0].slide, Some(1));
        assert!(!rows[1].me);
        assert_eq!(rows[1].name, "bob");
        assert_eq!(rows[1].role, Some(Role::Viewer));
        assert_eq!(rows[1].slide, Some(1), "the slide key resolves to its number");
    }

    #[test]
    fn the_indicator_opens_the_participants_pane() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        app.enable_collab();
        app.collab.welcome("alice@example.com", Role::Editor, 4);
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        let indicator = h.get_by_role_and_label(egui::Role::Button, "Collaboration");
        assert_eq!(indicator.value().as_deref(), Some("Synced"));
        indicator.click();
        h.run_steps(3);
        assert_eq!(h.state().ui.pane.as_deref(), Some(PANE));
    }

    #[test]
    fn a_command_goes_through_the_link_and_changes_the_document_only_on_applied() {
        use deckcraft_collab::{CollabDoc, PipeTransport};
        use deckcraft_collab_proto::{Applied, ClientMsg, ConfirmedCommand, OpKind, Payload, ServerMsg};

        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        let mut doc = CollabDoc::new("d");
        let (transport, from_ui, to_ui) = PipeTransport::pair();
        let _ = doc.connect(&app.session, "jwt", deckcraft_collab_proto::ClientInfo { name: "t".into(), ver: "1".into() });
        app.collab.attach(doc, Box::new(transport));
        app.sync_collab_gate();
        to_ui
            .send(ServerMsg::Welcome(deckcraft_collab_proto::Welcome {
                v: deckcraft_collab_proto::PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role: Role::Editor,
                lease: None,
                version: 0,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: deckcraft_collab_proto::SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: deckcraft_collab_proto::Capabilities {
                    kinds: vec![OpKind::Command, OpKind::Structural],
                    limits: deckcraft_collab_proto::Limits::default(),
                    commands: vec![],
                },
            }))
            .expect("welcome");
        app.collab.pump(&mut app.session, 1_000);
        assert!(app.session.is_collab_queue());

        let before = app.session.doc().expect("doc").doc.slides[0].shapes.len();
        app.run("shape.insert", json!({"preset": "rect", "rect": [40, 40, 80, 60]})).expect("queued");
        assert_eq!(app.session.doc().expect("doc").doc.slides[0].shapes.len(), before, "nothing local before applied");
        app.collab.pump(&mut app.session, 1_060);
        let mut op = None;
        while let Ok(msg) = from_ui.try_recv() {
            if let ClientMsg::Op(envelope) = msg {
                op = Some(envelope);
            }
        }
        let op = op.expect("the operation reached the link");
        let payload = op.payload.clone();
        let applied = ServerMsg::Applied(Applied {
            op_id: op.op_id.clone(),
            version: 1,
            actor_id: "alice".into(),
            command: Some(ConfirmedCommand { cmd: payload.command().into(), target: op.target.clone(), payload }),
            delta: Default::default(),
        });
        to_ui.send(applied).expect("applied");
        app.collab.pump(&mut app.session, 1_100);
        assert_eq!(app.session.doc().expect("doc").doc.slides[0].shapes.len(), before + 1, "the server's command created the shape");
        let _ = Payload::ShapeInsert { preset: "rect".into(), rect: [0.0; 4], text: None };
    }

    #[test]
    fn with_preview_on_a_move_is_drawn_before_the_room_answers_and_settled_in_place() {
        use deckcraft_collab::{CollabDoc, PipeTransport};
        use deckcraft_collab_proto::{Applied, ClientMsg, ConfirmedCommand, OpKind, ServerMsg};

        let (session, _) = deck();
        let mut app = SlideApp::new(session, crate::Services::default());
        let mut doc = CollabDoc::new("d");
        let (transport, from_ui, to_ui) = PipeTransport::pair();
        let _ = doc.connect(&app.session, "jwt", deckcraft_collab_proto::ClientInfo { name: "t".into(), ver: "1".into() });
        // The host flag (`DECKCRAFT_COLLAB_PREVIEW` on the desktop) is set before the client is
        // attached; `attach` forwards it.
        app.collab.set_preview(true);
        app.collab.attach(doc, Box::new(transport));
        app.sync_collab_gate();
        to_ui
            .send(ServerMsg::Welcome(deckcraft_collab_proto::Welcome {
                v: deckcraft_collab_proto::PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role: Role::Editor,
                lease: None,
                version: 0,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: deckcraft_collab_proto::SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: deckcraft_collab_proto::Capabilities {
                    kinds: vec![OpKind::Command, OpKind::Structural],
                    limits: deckcraft_collab_proto::Limits::default(),
                    commands: vec![],
                },
            }))
            .expect("welcome");
        app.collab.pump(&mut app.session, 1_000);
        assert!(app.collab.preview(), "the flag follows the host setting");

        let x = |s: &Session| s.doc().expect("doc").doc.slides[0].shapes[0].xfrm.expect("box").x;
        let start = x(&app.session);
        let shape = app.session.doc().expect("doc").doc.slides[0].shapes[0].id;
        app.run("shape.move", json!({"ids": [shape.0], "x": 300.0, "y": 110.0})).expect("queued");
        assert_eq!(x(&app.session), start, "the engine queue holds it until the pump");

        app.collab.pump(&mut app.session, 1_060);
        assert_eq!(x(&app.session), 300.0, "the canvas draws the edit before the room answers");
        assert!(app.collab.is_previewing());
        let mut op = None;
        while let Ok(msg) = from_ui.try_recv() {
            if let ClientMsg::Op(envelope) = msg {
                op = Some(envelope);
            }
        }
        let op = op.expect("the operation reached the link");
        assert_eq!(op.preview.as_deref(), Some(op.op_id.as_str()), "the wire marks the preview with its local id");
        let payload = op.payload.clone();
        to_ui
            .send(ServerMsg::Applied(Applied {
                op_id: op.op_id.clone(),
                version: 1,
                actor_id: "alice".into(),
                command: Some(ConfirmedCommand { cmd: payload.command().into(), target: op.target.clone(), payload }),
                delta: Default::default(),
            }))
            .expect("applied");
        app.collab.pump(&mut app.session, 1_100);
        assert!(!app.collab.is_previewing(), "the answer settles the preview");
        assert_eq!(x(&app.session), 300.0, "and the move is not applied a second time");
        assert_eq!(app.collab.version(), 1);
    }

    #[test]
    fn applying_the_animation_painter_over_the_link_sends_the_sequence_and_disarms_a_one_shot_pick() {
        use deckcraft_collab::{CollabDoc, PipeTransport};
        use deckcraft_collab_proto::{ClientMsg, Payload, ServerMsg};

        let (session, _) = deck();
        let mut app = SlideApp::new(session, crate::Services::default());
        let mut doc = CollabDoc::new("d");
        let (transport, from_ui, to_ui) = PipeTransport::pair();
        let _ = doc.connect(&app.session, "jwt", deckcraft_collab_proto::ClientInfo { name: "t".into(), ver: "1".into() });
        app.collab.attach(doc, Box::new(transport));
        app.sync_collab_gate();
        to_ui
            .send(ServerMsg::Welcome(deckcraft_collab_proto::Welcome {
                v: deckcraft_collab_proto::PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role: Role::Editor,
                lease: None,
                version: 0,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: deckcraft_collab_proto::SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: deckcraft_collab_proto::Capabilities {
                    kinds: vec![deckcraft_collab_proto::OpKind::Command, deckcraft_collab_proto::OpKind::Structural],
                    limits: deckcraft_collab_proto::Limits::default(),
                    commands: vec![],
                },
            }))
            .expect("welcome");
        app.collab.pump(&mut app.session, 1_000);

        // A step on the first shape, picked into the painter (the pick is a session value, not a
        // document change, so it runs even while the queue is on).
        let shapes: Vec<u64> = app.session.doc().expect("doc").doc.slides[0].shapes.iter().map(|s| s.id.0 as u64).collect();
        app.session.apply_remote("animation.add", &json!({"effect": "fade", "ids": [shapes[0]]})).expect("seed a step");
        app.session.execute("animation.painter", &json!({"id": shapes[0]})).expect("pick");
        assert!(app.session.anim_painter.is_some());

        // The paint is queued, the link carries the slide's whole resulting sequence, and the
        // one-shot pick is disarmed as the single-user command would disarm it.
        app.run("animation.painterApply", json!({"ids": [shapes[1]]})).expect("queued");
        app.collab.pump(&mut app.session, 1_060);
        let mut op = None;
        while let Ok(msg) = from_ui.try_recv() {
            if let ClientMsg::Op(envelope) = msg {
                op = Some(envelope);
            }
        }
        let op = op.expect("the painted sequence reached the link");
        assert!(matches!(&op.payload, Payload::SlideAnimations { animations } if animations.len() == 2), "{:?}", op.payload);
        assert!(app.session.anim_painter.is_none(), "a one-shot pick is disarmed once it is sent");
    }

    #[test]
    fn a_text_range_format_goes_through_the_link_with_its_character_offsets() {
        use deckcraft_collab::{CollabDoc, PipeTransport};
        use deckcraft_collab_proto::{Applied, ClientMsg, ConfirmedCommand, OpKind, Payload, ServerMsg, TextRange};

        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        app.run("shape.insert", json!({"preset": "rect", "rect": [40, 40, 200, 60], "text": "Hello world"})).expect("shape");
        let id = app.session.doc().expect("doc").doc.slides[0].shapes.last().expect("shape").id;
        app.session.doc_mut().expect("doc").selection.shapes = vec![id];
        app.session.doc_mut().expect("doc").selection.text =
            Some(deckcraft_engine::TextSel { shape: id, anchor: (0, 0), caret: (0, 5), cell: None, notes: false, line_end: None });

        let mut doc = CollabDoc::new("d");
        let (transport, from_ui, to_ui) = PipeTransport::pair();
        let _ = doc.connect(&app.session, "jwt", deckcraft_collab_proto::ClientInfo { name: "t".into(), ver: "1".into() });
        app.collab.attach(doc, Box::new(transport));
        app.sync_collab_gate();
        to_ui
            .send(ServerMsg::Welcome(deckcraft_collab_proto::Welcome {
                v: deckcraft_collab_proto::PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role: Role::Editor,
                lease: None,
                version: 0,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: deckcraft_collab_proto::SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: deckcraft_collab_proto::Capabilities {
                    kinds: vec![OpKind::Command, OpKind::Structural],
                    limits: deckcraft_collab_proto::Limits::default(),
                    commands: vec![],
                },
            }))
            .expect("welcome");
        app.collab.pump(&mut app.session, 1_000);
        assert!(app.session.is_collab_queue());

        app.run("format.bold", json!({"on": true})).expect("queued");
        let bold = |app: &SlideApp| -> Vec<Option<bool>> {
            app.session.doc().expect("doc").doc.slides[0]
                .shapes
                .last()
                .expect("shape")
                .text
                .as_ref()
                .expect("text")
                .paragraphs
                .iter()
                .flat_map(|p| p.runs.iter())
                .map(|r| r.props.bold)
                .collect()
        };
        assert!(bold(&app).iter().all(Option::is_none), "nothing local before applied");
        app.collab.pump(&mut app.session, 1_060);
        let mut op = None;
        while let Ok(msg) = from_ui.try_recv() {
            if let ClientMsg::Op(envelope) = msg {
                op = Some(envelope);
            }
        }
        let op = op.expect("the operation reached the link");
        assert_eq!(
            op.payload,
            Payload::FormatSet {
                format: deckcraft_collab_proto::FormatCmd::Bold,
                args: json!({"on": true}),
                addresses: vec![deckcraft_collab_proto::FormatAddress { cell: None, range: Some(TextRange { from: 0, to: 5 }), caret: None }]
            },
            "the queued format carries the selected character range, not the whole body"
        );
        let payload = op.payload.clone();
        let applied = ServerMsg::Applied(Applied {
            op_id: op.op_id.clone(),
            version: 1,
            actor_id: "alice".into(),
            command: Some(ConfirmedCommand { cmd: payload.command().into(), target: op.target.clone(), payload }),
            delta: Default::default(),
        });
        to_ui.send(applied).expect("applied");
        app.collab.pump(&mut app.session, 1_100);
        assert_eq!(bold(&app), vec![Some(true), None], "the server's command bolded exactly the range");
    }

    #[test]
    fn a_caret_in_a_shape_body_goes_through_the_link_with_its_offset() {
        use deckcraft_collab::{CollabDoc, PipeTransport};
        use deckcraft_collab_proto::{Applied, ClientMsg, ConfirmedCommand, FormatAddress, OpKind, Payload, ServerMsg};

        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        app.run("shape.insert", json!({"preset": "rect", "rect": [40, 40, 200, 60], "text": "Hello world"})).expect("shape");
        let id = app.session.doc().expect("doc").doc.slides[0].shapes.last().expect("shape").id;
        app.session.doc_mut().expect("doc").selection.shapes = vec![id];
        app.session.doc_mut().expect("doc").selection.text =
            Some(deckcraft_engine::TextSel { shape: id, anchor: (0, 8), caret: (0, 8), cell: None, notes: false, line_end: None });

        let mut doc = CollabDoc::new("d");
        let (transport, from_ui, to_ui) = PipeTransport::pair();
        let _ = doc.connect(&app.session, "jwt", deckcraft_collab_proto::ClientInfo { name: "t".into(), ver: "1".into() });
        app.collab.attach(doc, Box::new(transport));
        app.sync_collab_gate();
        to_ui
            .send(ServerMsg::Welcome(deckcraft_collab_proto::Welcome {
                v: deckcraft_collab_proto::PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role: Role::Editor,
                lease: None,
                version: 0,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: deckcraft_collab_proto::SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: deckcraft_collab_proto::Capabilities {
                    kinds: vec![OpKind::Command, OpKind::Structural],
                    limits: deckcraft_collab_proto::Limits::default(),
                    commands: vec![],
                },
            }))
            .expect("welcome");
        app.collab.pump(&mut app.session, 1_000);
        assert!(app.session.is_collab_queue());

        app.run("format.bold", json!({"on": true})).expect("queued");
        app.collab.pump(&mut app.session, 1_060);
        let mut op = None;
        while let Ok(msg) = from_ui.try_recv() {
            if let ClientMsg::Op(envelope) = msg {
                op = Some(envelope);
            }
        }
        let op = op.expect("the operation reached the link");
        assert_eq!(
            op.payload,
            Payload::FormatSet {
                format: deckcraft_collab_proto::FormatCmd::Bold,
                args: json!({"on": true}),
                addresses: vec![FormatAddress { cell: None, range: None, caret: Some(8) }]
            },
            "a caret is its own address, not a word and not the whole body"
        );
        let payload = op.payload.clone();
        let applied = ServerMsg::Applied(Applied {
            op_id: op.op_id.clone(),
            version: 1,
            actor_id: "alice".into(),
            command: Some(ConfirmedCommand { cmd: payload.command().into(), target: op.target.clone(), payload }),
            delta: Default::default(),
        });
        to_ui.send(applied).expect("applied");
        app.collab.pump(&mut app.session, 1_100);
        let bold: Vec<Option<bool>> = app.session.doc().expect("doc").doc.slides[0]
            .shapes
            .last()
            .expect("shape")
            .text
            .as_ref()
            .expect("text")
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .map(|r| r.props.bold)
            .collect();
        assert_eq!(bold, vec![None, Some(true)], "the server's command bolded exactly the word at the caret");
    }

    #[test]
    fn commit_warnings_show_once_until_dismissed() {
        let mut ui = CollabUi::default();
        assert!(ui.banner_text().is_none());
        ui.set_warnings(vec!["slide 2: Opaque part".into(), "slide 4: 3-D".into(), "media: linked".into()]);
        let text = ui.banner_text().expect("banner");
        assert!(text.starts_with("Saved with 3 warnings:"));
        assert!(text.contains("…"));
        ui.dismiss_banner();
        assert!(ui.banner_text().is_none());
        ui.set_warnings(vec!["one".into()]);
        assert_eq!(ui.banner_text().as_deref(), Some("Saved with 1 warning: one"));
    }

    #[test]
    fn names_and_colours_are_safe_for_presence() {
        assert_eq!(display_name("alice@example.com"), "alice");
        assert_eq!(display_name("a".repeat(40).as_str()).chars().count(), 24);
        assert_eq!(parse_color("#3b82f6"), Some(Color32::from_rgb(0x3b, 0x82, 0xf6)));
        assert_eq!(parse_color("nope"), None);
        // Six bytes of multi-byte text: a bad colour must be ignored, never panic.
        assert_eq!(parse_color("#ééé"), None);
    }

    // ------------------------------------------------------------ Save becomes a commit

    /// A session joined over a test pipe, with a local edit the cabinet has not seen yet.
    fn live(role: Role) -> (SlideApp, std::sync::mpsc::Receiver<ClientMsg>, std::sync::mpsc::Sender<ServerMsg>) {
        use deckcraft_collab::{CollabDoc, PipeTransport};

        let mut app = SlideApp::new(Session::with_new(), crate::Services::default());
        app.run("shape.insert", json!({"preset": "rect", "rect": [40, 40, 80, 60]})).expect("a local edit");
        let mut doc = CollabDoc::new("d");
        let (transport, from_ui, to_ui) = PipeTransport::pair();
        let _ = doc.connect(&app.session, "jwt", deckcraft_collab_proto::ClientInfo { name: "t".into(), ver: "1".into() });
        app.collab.attach(doc, Box::new(transport));
        app.sync_collab_gate();
        to_ui
            .send(ServerMsg::Welcome(deckcraft_collab_proto::Welcome {
                v: deckcraft_collab_proto::PROTO_VERSION,
                session_id: "d".into(),
                actor_id: "alice".into(),
                role,
                lease: None,
                version: 0,
                snapshot: deckcraft_collab_proto::SnapshotRef { mode: deckcraft_collab_proto::SnapshotMode::BaseAndTail, cursor: "tail@0".into() },
                capabilities: deckcraft_collab_proto::Capabilities {
                    kinds: vec![deckcraft_collab_proto::OpKind::Command, deckcraft_collab_proto::OpKind::Structural],
                    limits: deckcraft_collab_proto::Limits::default(),
                    commands: vec![],
                },
            }))
            .expect("welcome");
        app.collab.pump(&mut app.session, 1_000);
        (app, from_ui, to_ui)
    }

    /// Every commit that reached the link, with the version it expected.
    fn commits(from_ui: &std::sync::mpsc::Receiver<ClientMsg>) -> Vec<Option<String>> {
        let mut out = Vec::new();
        while let Ok(msg) = from_ui.try_recv() {
            if let ClientMsg::Commit { expected_file_version } = msg {
                out.push(expected_file_version);
            }
        }
        out
    }

    fn commit_result(version: u64, file_version: &str) -> ServerMsg {
        ServerMsg::CommitResult(deckcraft_collab_proto::CommitResult {
            sha256: "ab".repeat(32),
            version,
            file_version: Some(file_version.into()),
            bytes_base64: None,
            blob: None,
            warnings: vec![],
        })
    }

    #[test]
    fn save_in_a_live_session_sends_one_commit_and_the_document_stays_dirty_until_the_answer() {
        let (mut app, from_ui, to_ui) = live(Role::Editor);
        assert!(app.session.active().expect("doc").is_dirty(), "there are unsaved edits");

        app.save();
        app.collab.pump(&mut app.session, 1_060);
        assert_eq!(commits(&from_ui), vec![None], "one commit, expecting the version the room was opened from");
        assert_eq!(app.collab.commit_state(), &CommitState::Committing);

        // A second Save while the first is on the wire is the same request, not a second version.
        app.save();
        app.collab.pump(&mut app.session, 1_100);
        assert!(commits(&from_ui).is_empty(), "no second commit in flight");
        assert!(app.session.active().expect("doc").is_dirty(), "dirty until the cabinet answers");

        to_ui.send(commit_result(7, "etag-1")).expect("commit result");
        app.collab.pump(&mut app.session, 1_120);
        assert!(!app.session.active().expect("doc").is_dirty(), "the cabinet has the file: now it is saved");
        assert_eq!(app.collab.commit_state(), &CommitState::Committed(7));
        assert_eq!(app.collab.take_commit_notice().as_deref(), Some("Committed v7"));

        // The next Save expects the version the cabinet accepted.
        app.save();
        app.collab.pump(&mut app.session, 1_160);
        assert_eq!(commits(&from_ui), vec![Some("etag-1".into())]);
    }

    #[test]
    fn a_refused_commit_keeps_the_edits_dirty_and_offers_a_copy() {
        let (mut app, from_ui, to_ui) = live(Role::Editor);
        let before = app.session.doc().expect("doc").doc.slides[0].shapes.len();
        app.save();
        app.collab.pump(&mut app.session, 1_060);
        assert_eq!(commits(&from_ui), vec![None]);

        to_ui
            .send(ServerMsg::Rejected(deckcraft_collab_proto::Rejected {
                op_id: "commit".into(),
                code: deckcraft_collab_proto::RejectCode::Conflict,
                message: "the file changed outside the session: expected etag-0, current etag-ext".into(),
                retryable: false,
                hint: None,
            }))
            .expect("refusal");
        app.collab.pump(&mut app.session, 1_100);
        assert!(app.session.active().expect("doc").is_dirty(), "nothing was saved");
        assert_eq!(app.session.doc().expect("doc").doc.slides[0].shapes.len(), before, "the edits are intact");
        assert!(matches!(app.collab.commit_state(), CommitState::Refused(_)));
        assert!(app.collab.take_commit_notice().expect("notice").starts_with("Commit refused:"));
        let banner = app.collab.banner_text().expect("banner");
        assert!(banner.contains("Save a copy"), "{banner}");

        // The request is not stuck: Save asks again, and there is still no accepted version to
        // expect, so the server compares against the version it restored for the room.
        app.save();
        app.collab.pump(&mut app.session, 1_140);
        assert_eq!(commits(&from_ui), vec![None]);
    }

    #[test]
    fn a_viewer_save_never_reaches_the_cabinet() {
        let (mut app, from_ui, _to_ui) = live(Role::Viewer);
        app.save();
        app.collab.pump(&mut app.session, 1_060);
        assert!(commits(&from_ui).is_empty(), "a viewer may not commit");
        assert!(app.session.active().expect("doc").is_dirty(), "the edits are left as they were");
        assert_eq!(app.collab.commit_state(), &CommitState::Idle);
    }
}
