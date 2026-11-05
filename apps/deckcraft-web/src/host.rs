//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! DeckCraft in an `<iframe>` and keep its files (a Nextcloud app, a personal file server). The
//! same messages as GridCraft's, WordCraft's and PdfCraft's bridges; each is a plain object with a
//! `type`:
//!
//! - `deckcraft:ready` (to the parent): `{ version }`, sent once the app is running and
//!   listening; send a file now.
//! - `deckcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`, name a bare file name. It opens like a file picked with File › Open on the
//!   desktop: a presentation sent with its name (.pptx, .potx, .ppsx, .deckcraft) has that name
//!   as its file, so Save goes back to it; an outline (.txt, .md, .rtf), a file without a name and
//!   a damaged file open untitled.
//! - `deckcraft:save` (to the parent): `{ name, saveAs, bytes }` (a transferred `ArrayBuffer`) for
//!   every file DeckCraft would otherwise download. The parent stores it and reports failures
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
//! - `deckcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `deckcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the app
//! starts with an empty presentation instead of the sample, which gives way to the first file the
//! parent sends; the parent guards leaving the page, and `?author=` names the user for comments.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use deckcraft_ui_egui::{Services, SlideApp};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

/// The longest user name taken from `?author=` (the preferences' own limit).
const MAX_AUTHOR_CHARS: usize = 256;

/// Largest `id`/`requestId` accepted from the host.
const MAX_ID_BYTES: usize = 1024;

/// Largest `params` string accepted from the host.
const MAX_PARAMS_BYTES: usize = 1 << 20;

/// Most commands waiting for the next frame.
const MAX_PENDING_COMMANDS: usize = 256;

/// Most `params` text waiting for the next frame, all commands together.
const MAX_PENDING_PARAMS_BYTES: usize = 8 << 20;

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// Files the parent sent (the name, if it sent one), waiting for the next frame.
    opened: Rc<RefCell<Vec<(Option<String>, Vec<u8>)>>>,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    /// Commands the parent sent since the last frame: (id, params JSON, request id).
    commands: Rc<RefCell<Vec<(String, String, Option<String>)>>>,
    /// Total length of the `params` text of the commands that haven't run yet.
    queued_bytes: Rc<Cell<usize>>,
    /// Total length of the command identifiers that haven't run yet.
    queued_id_bytes: Rc<Cell<usize>>,
    /// The uid of the presentation the host opened (the session keeps a blank deck around,
    /// so "closed" means the uid vanishing, not an empty document list).
    host_uid: Rc<Cell<Option<u64>>>,
    /// Whether a "no presentations" state was reported (so docClosed fires once).
    closed_reported: Rc<Cell<bool>>,
}

impl Bridge {
    /// `None` when the page isn't framed (there is no parent to talk to), or when the parent is
    /// on another origin: messages only go to this origin, so the browser would drop every save
    /// without an error while the presentation showed as saved.
    pub fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let origin = window.location().origin().ok()?;
        // An opaque origin (a `file://` page in some browsers) can't be a `postMessage` target.
        if origin == "null" {
            return None;
        }
        // Reading another origin's location throws, which is `None` too.
        if parent.location().origin().ok()? != origin {
            return None;
        }
        Some(Self {
            window,
            parent,
            origin,
            opened: Rc::default(),
            reported: Rc::default(),
            commands: Rc::default(),
            queued_bytes: Rc::default(),
            queued_id_bytes: Rc::default(),
            host_uid: Rc::default(),
            closed_reported: Rc::default(),
        })
    }

    /// Hand saved and exported files to the parent instead of downloading them, and hear the
    /// files it sends.
    pub fn connect(&self, services: &mut Services, ctx: egui::Context) {
        self.listen(ctx);
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
        self.send(&[("type", "deckcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into())]);
    }

    /// The app couldn't start (no WebGPU or WebGL2, for example): let the parent say so.
    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "deckcraft:failed".into()), ("error", error.into())]);
    }

    /// Before each frame: open the files the parent sent.
    pub fn open_files(&self, app: &mut SlideApp) {
        let opened = std::mem::take(&mut *self.opened.borrow_mut());
        for (name, bytes) in opened {
            app.open_from_host(name.as_deref(), &bytes);
            if name
                .as_deref()
                .and_then(|n| n.rsplit('.').next())
                .is_some_and(|ext| ["pptx", "potx", "ppsx", "deckcraft"].iter().any(|e| ext.eq_ignore_ascii_case(e)))
                && let Some(d) = app.session.active()
            {
                self.set_host_uid(d.uid);
            }
        }
    }

    /// After each frame: tell the parent when unsaved changes appear or go away.
    pub fn report_dirty(&self, app: &SlideApp) {
        let dirty = app.session.documents().iter().any(|d| d.is_dirty());
        if self.reported.get() != Some(dirty) && self.send(&[("type", "deckcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
    }

    /// Remember the presentation the host opened (docClosed tracking) and keep
    /// saving in the format the deck came in as.
    pub fn set_host_uid(&self, uid: u64) {
        self.host_uid.set(Some(uid));
        self.closed_reported.set(false);
    }

    /// Commands the parent sent since the last frame, oldest first: (id, params JSON, request id).
    pub fn take_commands(&self) -> Vec<(String, String, Option<String>)> {
        self.queued_bytes.set(0);
        self.queued_id_bytes.set(0);
        std::mem::take(&mut *self.commands.borrow_mut())
    }

    /// Tell the parent how a command (`deckcraft:cmd`) ended.
    pub fn post_response(&self, id: &str, request_id: Option<&str>, result: Result<serde_json::Value, String>) {
        let value = crate::host_cmd::response(id, request_id, result);
        let sent = js_sys::JSON::parse(&value.to_string()).ok().is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok());
        if !sent {
            log::error!("couldn't answer command {id} to the page");
        }
    }

    /// Tell the parent the presentation it opened has closed.
    pub fn post_doc_closed(&self) {
        self.send(&[("type", "deckcraft:docClosed".into())]);
    }

    /// After each frame: tell the parent when unsaved changes appear or go away, and post
    /// `docClosed` once the host's presentation has vanished (File › Close).
    pub fn sync(&self, app: &mut SlideApp) {
        let dirty = app.session.documents().iter().any(|d| d.is_dirty());
        if self.reported.get() != Some(dirty) && self.send(&[("type", "deckcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
        if let Some(uid) = self.host_uid.get()
            && !app.session.documents().iter().any(|d| d.uid == uid)
        {
            self.host_uid.set(None);
            if !self.closed_reported.get() {
                self.closed_reported.set(true);
                self.send(&[("type", "deckcraft:docClosed".into())]);
            }
        }
    }

    fn post_save(&self, path: &str, bytes: &[u8], save_as: bool) -> Result<(), String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let message = message(&[
            ("type", "deckcraft:save".into()),
            ("name", name.as_str().into()),
            ("saveAs", save_as.into()),
            ("bytes", buffer.clone().into()),
        ])
        .ok_or_else(|| "couldn't build the message".to_string())?;
        self.parent
            .post_message_with_transfer(&message, &self.origin, &js_sys::Array::of1(&buffer))
            .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
    }

    /// Hear `deckcraft:open` from the parent and queue the file for the next frame.
    fn listen(&self, ctx: egui::Context) {
        let parent = self.parent.clone();
        let origin = self.origin.clone();
        let bridge = self.clone();
        let opened = self.opened.clone();
        let commands = self.commands.clone();
        let queued_bytes = self.queued_bytes.clone();
        let queued_id_bytes = self.queued_id_bytes.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            match string(&data, "type").as_deref() {
                Some("deckcraft:open") => {
                    // No name is no file: the presentation opens untitled, never under an invented name.
                    let name = string(&data, "name").filter(|n| !n.trim().is_empty());
                    let Some(bytes) = bytes(&data) else { return };
                    opened.borrow_mut().push((name, bytes));
                    ctx.request_repaint();
                }
                Some("deckcraft:cmd") => {
                    let id_raw = string(&data, "id");
                    let request_id = string(&data, "requestId").filter(|r| !r.is_empty());
                    if id_raw.as_ref().map(|s| s.len()).unwrap_or(0) > crate::host_cmd::MAX_ID_BYTES {
                        bridge.post_response("", request_id.as_deref(), Err(format!("id is larger than {} bytes", crate::host_cmd::MAX_ID_BYTES)));
                        ctx.request_repaint();
                        return;
                    }
                    let id = match id_raw.filter(|i| !i.trim().is_empty()) {
                        Some(id) => id,
                        None => {
                            bridge.post_response("", request_id.as_deref(), Err("a command needs an id".to_string()));
                            ctx.request_repaint();
                            return;
                        }
                    };
                    if request_id.as_ref().map(|s| s.len()).unwrap_or(0) > crate::host_cmd::MAX_ID_BYTES {
                        bridge.post_response(
                            &id,
                            request_id.as_deref(),
                            Err(format!("requestId is larger than {} bytes", crate::host_cmd::MAX_ID_BYTES)),
                        );
                        ctx.request_repaint();
                        return;
                    }
                    let params_value = js_sys::Reflect::get(&data, &JsValue::from_str("params")).ok().filter(|v| !v.is_undefined() && !v.is_null());
                    let params = match params_value {
                        None => "{}".to_string(),
                        Some(v) => {
                            let js_len = v.dyn_ref::<js_sys::JsString>().map(|s| s.length() as usize);
                            if js_len.unwrap_or(usize::MAX) > crate::host_cmd::MAX_PARAMS_BYTES {
                                bridge.post_response(
                                    &id,
                                    request_id.as_deref(),
                                    Err(format!("params are larger than {} bytes", crate::host_cmd::MAX_PARAMS_BYTES)),
                                );
                                ctx.request_repaint();
                                return;
                            }
                            match v.as_string() {
                                Some(text) if text.len() <= crate::host_cmd::MAX_PARAMS_BYTES => text,
                                Some(_) => {
                                    bridge.post_response(
                                        &id,
                                        request_id.as_deref(),
                                        Err(format!("params are larger than {} bytes", crate::host_cmd::MAX_PARAMS_BYTES)),
                                    );
                                    ctx.request_repaint();
                                    return;
                                }
                                None => {
                                    bridge.post_response(&id, request_id.as_deref(), Err("params must be a string of valid JSON".to_string()));
                                    ctx.request_repaint();
                                    return;
                                }
                            }
                        }
                    };
                    let incoming_id_bytes = id.len() + request_id.as_deref().map(str::len).unwrap_or(0);
                    if commands.borrow().len() >= crate::host_cmd::MAX_PENDING_COMMANDS
                        || queued_bytes.get().saturating_add(params.len()) > crate::host_cmd::MAX_PENDING_PARAMS_BYTES
                        || queued_id_bytes.get().saturating_add(incoming_id_bytes) > crate::host_cmd::MAX_PENDING_PARAMS_BYTES
                    {
                        bridge.post_response(&id, request_id.as_deref(), Err("too many commands are waiting; try again".to_string()));
                        ctx.request_repaint();
                        return;
                    }
                    queued_bytes.set(queued_bytes.get().saturating_add(params.len()));
                    queued_id_bytes.set(queued_id_bytes.get().saturating_add(incoming_id_bytes));
                    commands.borrow_mut().push((id, params, request_id));
                    ctx.request_repaint();
                }
                _ => {}
            }
        });
        if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_message.forget();
        }
    }

    /// Post a message to the parent; false if it couldn't be sent.
    fn send(&self, fields: &[(&str, JsValue)]) -> bool {
        message(fields).is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok())
    }
}

fn message(fields: &[(&str, JsValue)]) -> Option<js_sys::Object> {
    let m = js_sys::Object::new();
    for (key, value) in fields {
        js_sys::Reflect::set(&m, &JsValue::from_str(key), value).ok()?;
    }
    Some(m)
}

fn string(data: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_string()
}

/// `bytes` as an `ArrayBuffer` or `Uint8Array`.
fn bytes(data: &JsValue) -> Option<Vec<u8>> {
    let bytes = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
    if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
        Some(array.to_vec())
    } else if bytes.is_instance_of::<js_sys::ArrayBuffer>() {
        Some(js_sys::Uint8Array::new(&bytes).to_vec())
    } else {
        None
    }
}
