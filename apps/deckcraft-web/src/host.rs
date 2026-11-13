//! The host bridge, for pages that embed DeckCraft and keep its files (a Nextcloud app, a personal
//! file server). The messages and the two transports are `craft-web-host`'s
//! (storytold/craft-libs), the same contract as the other Craft apps' bridges, with the
//! `deckcraft:` prefix. This module is DeckCraft's part of it.
//!
//! - `?host=parent` (an `<iframe>`) talks to the same-origin parent over `postMessage`;
//!   `data-host="page"` on the canvas (the page's own document, no iframe) uses DOM
//!   `CustomEvent`s on the canvas instead. `?host=parent` wins when both are present. With no
//!   parent, or a parent on another origin, there is no bridge: the normal app with downloads.
//! - `deckcraft:ready` (to the host): sent once the app is running and listening; send a file now. DeckCraft
//!   reads bytes, so it says `file: false` and a host sends `bytes`.
//! - `deckcraft:open` (from the host): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`, name a bare file name. It opens like a file picked with File › Open on the
//!   desktop: a presentation sent with its name (.pptx, .potx, .ppsx, .deckcraft) has that name
//!   as its file, so Save goes back to it; an outline (.txt, .md, .rtf), a file without a name and
//!   a damaged file open untitled.
//! - `deckcraft:save` (to the host): `{ name, saveAs, bytes }` (with the bytes as a transferred `ArrayBuffer` over `postMessage`) for
//!   every file DeckCraft would otherwise download. The host stores it and reports failures
//!   itself. `saveAs` says whether the file becomes the presentation's file:
//!   - Save: `false`. It writes the opened file under its own name and format (a .pptx stays a
//!     .pptx), as the desktop app does. A name the parent never sent (an untitled presentation, an
//!     outline, a file sent without a name, a repaired copy of a damaged file, a file the user
//!     picked or dropped) is a new file: `true`, as for Save As, named `<title>.deckcraft` as
//!     Save suggests on the desktop.
//!   - Save As (File › Save As as .pptx or .deckcraft, Ctrl+Shift+S): `true`. The presentation is
//!     now that file; the next Save sends its name with `false`.
//!   - Export (PDF, pictures, outline, GIF, handouts, a Save As type that isn't a presentation)
//!     and Print: `false`. They are copies; the presentation's file stays the same.
//! - `deckcraft:dirty` whenever unsaved changes appear or go away, `deckcraft:failed` when the
//!   app couldn't start or refused a file of 2 GiB or larger.
//!
//! Collaboration goes through the same bridge (`INTEGRATION.md` §6, ADR-0016 §3):
//! - `deckcraft:session` (from the host): `{ sessionId, wsUrl, token, role }`. It is the only thing
//!   that makes the app dial a sync server; without it the app stays single-user.
//! - `deckcraft:session-state` (to the host): `{ status, participants }` whenever the sync status or
//!   the participant list changes. `status` is `synced`, `pending`, `offline` or `error`.
//! - `deckcraft:presence` (to the host): `{ participants }` at the room's own pace.
//! - `deckcraft:commit` (to the host): `{ sessionId }` when Save is pressed in a live session: the
//!   cabinet owns the file, so it stores a new version rather than the page taking a copy.
//! - `deckcraft:committed` (from the host): `{ version, etag }`, the cabinet's answer.
//!
//! In host mode the app starts with an empty presentation instead of the sample, which gives way to
//! the first file the host sends; the host guards leaving the page, and `?author=` names the user
//! for comments.

use std::cell::RefCell;
use std::rc::Rc;

use craft_web_host::Participant as HostPerson;
use craft_web_host::web::{FileData, Host, Message};
use deckcraft_ui_egui::{Services, SlideApp};

use crate::host_session::{self, HostInbox};
use deckcraft_collab_web::{HostEvent, HostParticipant};

/// The app id in the host messages (`deckcraft:ready`).
const APP: &str = "deckcraft";

/// The name `craft-web-host` gives an open request that carries none (it cleans a host's name and
/// falls back to this): no name is no file, so the presentation opens untitled.
const NAMELESS: &str = "document";

/// The largest file a host may send. File › Open on the desktop refuses anything over 2 GiB.
/// On wasm32 a `Vec` cannot hold 2 GiB (`isize::MAX` is one byte less), so a file of exactly
/// 2 GiB is refused too, before its bytes are copied.
const MAX_OPEN_BYTES: f64 = 2_147_483_648.0;

/// The longest user name taken from `?author=` (the preferences' own limit).
const MAX_AUTHOR_CHARS: usize = 256;

#[derive(Clone)]
pub struct Bridge {
    host: Rc<Host>,
    /// Files the host sent (the name, if it sent one), waiting for the next frame.
    opened: Rc<RefCell<Vec<(Option<String>, Vec<u8>)>>>,
}

impl Bridge {
    /// The bridge for this page, or `None` when DeckCraft isn't hosted: no `?host=parent` and no
    /// `data-host="page"` on the canvas, an unframed `?host=parent` page (it stays the normal app
    /// with downloads), or a parent on another origin.
    pub fn detect(canvas: &web_sys::HtmlCanvasElement) -> Option<Self> {
        let host = Host::connect(APP, canvas)?;
        // An opaque origin (a file:// page in some browsers) can't be a postMessage target.
        if host.mode() == craft_web_host::Mode::Parent && web_sys::window()?.location().origin().ok()? == "null" {
            return None;
        }
        Some(Self { host: Rc::new(host), opened: Rc::default() })
    }

    /// Hand saved and exported files to the host instead of downloading them, hear the files it
    /// sends, and route a collaboration session.
    pub fn connect(&self, services: &mut Services, ctx: egui::Context, events: HostInbox) {
        self.listen(ctx, events);
        let bridge = self.clone();
        services.host_save = Some(Box::new(move |name: &str, bytes: &[u8], save_as: bool| bridge.post_save(name, bytes, save_as)));
        // Exports and Print are copies.
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| {
            if let Err(e) = bridge.post_save(name, bytes, false) {
                log::error!("couldn't hand {name} to the page: {e}");
            }
        }));
    }

    /// `?author=`: the host's name for the user signs new comments.
    pub fn set_author(app: &mut SlideApp, author: &str) {
        let author: String = author.trim().chars().take(MAX_AUTHOR_CHARS).collect();
        if !author.is_empty() {
            app.session.prefs.author = author;
        }
    }

    pub fn post_ready(&self) {
        let _ = self.host.post_ready(env!("CARGO_PKG_VERSION"), false);
    }

    /// The app couldn't start (no WebGPU or WebGL2, for example): let the host say so.
    pub fn post_failed(&self, error: &str) {
        let _ = self.host.post_failed(error, None);
    }

    /// Before each frame: open the files the host sent.
    pub fn open_files(&self, app: &mut SlideApp) {
        let opened = std::mem::take(&mut *self.opened.borrow_mut());
        for (name, bytes) in opened {
            app.open_from_host(name.as_deref(), &bytes);
        }
    }

    /// After each frame: tell the host when unsaved changes appear or go away (the crate does not
    /// repeat a state it already reported).
    pub fn report_dirty(&self, app: &SlideApp) {
        self.host.post_dirty(app.session.documents().iter().any(|d| d.is_dirty()));
    }

    /// `deckcraft:session-state`: the cabinet's sync indicator and who is here.
    pub(crate) fn post_session_state(&self, status: &str, participants: &[HostParticipant]) -> Result<(), String> {
        self.host.post_session_state(status, &host_people(participants))
    }

    /// `deckcraft:presence`: the participant list at the sync server's pace.
    pub(crate) fn post_presence(&self, participants: &[HostParticipant]) -> Result<(), String> {
        self.host.post_presence(&host_people(participants))
    }

    /// `deckcraft:commit`: the cabinet stores the session (it owns the file). The bytes stay with
    /// the sync server, which builds the file; the cabinet fetches or assembles it itself.
    pub(crate) fn post_commit(&self, session_id: &str) -> Result<(), String> {
        self.host.post_commit(session_id, None)
    }

    fn post_save(&self, path: &str, bytes: &[u8], save_as: bool) -> Result<(), String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        self.host.post_save(&name, bytes, None, save_as)
    }

    /// Hear `deckcraft:open`, `deckcraft:session` and `deckcraft:committed`.
    fn listen(&self, ctx: egui::Context, events: HostInbox) {
        let opened = self.opened.clone();
        let host = self.host.clone();
        self.host.listen(move |message| match message {
            // The collaboration messages never touch the document here: the shell applies them on
            // the next frame, and only then is a socket dialled.
            Message::Session { session_id, ws_url, token, role } => {
                host_session::deliver(&events, HostEvent::Session(host_session::HostSession::new(session_id, ws_url, token, role)));
                ctx.request_repaint();
            }
            Message::Committed { version, etag } => {
                host_session::deliver(&events, HostEvent::Committed { version, etag });
                ctx.request_repaint();
            }
            Message::Open { name, data } => {
                if data.size() >= MAX_OPEN_BYTES {
                    log::warn!("host file {name} is 2 GiB or larger; not opened");
                    let _ = host.post_failed(&format!("{name} is 2 GiB or larger, which DeckCraft's web build cannot open"), None);
                    return;
                }
                let FileData::Bytes(array) = data else {
                    log::warn!("host file {name} came as a File; DeckCraft reads bytes only; not opened");
                    return;
                };
                // No name is no file: the presentation opens untitled, never under an invented name.
                let name = (name != NAMELESS).then_some(name);
                opened.borrow_mut().push((name, array.to_vec()));
                ctx.request_repaint();
            }
            Message::Host { .. } | Message::Saved { .. } | Message::HandoffDone { .. } => {}
        });
    }
}

/// The crate's participant shape, from the session's.
fn host_people(participants: &[HostParticipant]) -> Vec<HostPerson> {
    participants
        .iter()
        .map(|participant| HostPerson { client_id: participant.client_id.clone(), role: participant.role.clone(), color: participant.color.clone() })
        .collect()
}
