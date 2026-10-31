//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! DeckCraft in an `<iframe>`. Same protocol as GridCraft's host bridge, with `deckcraft:`
//! message types:
//!
//! - `deckcraft:ready` (to the parent): `{ version }`, the app is listening; send a deck now.
//! - `deckcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a presentation dropped on the window.
//! - `deckcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file DeckCraft would otherwise download: Save/Save As and the PDF/PNG/GIF exports. The
//!   parent stores it and reports failures itself.
//! - `deckcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `deckcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//! - `deckcraft:cmd` (from the parent): `{ id, params?, requestId? }` runs the engine command `id`
//!   in the open presentation, like a ribbon button. `params` is a JSON string (absent means `{}`)
//!   and `requestId` an optional string that comes back in the response.
//! - `deckcraft:response` (to the parent): `{ id, requestId?, ok, value? }` or
//!   `{ id, requestId?, ok: false, error }` for every `deckcraft:cmd`, once it has run.
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the page
//! starts with an empty presentation instead of the sample deck.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use deckcraft_ui_egui::{Services, SlideApp};
use serde_json::Value;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

use crate::host_cmd;

/// Files saved under these extensions hand the whole deck to the parent, which stores it: the
/// deck counts as saved under that name, as it would after Save on the desktop. Exports (PDF,
/// PNG, GIF) go to the parent too but don't mark the deck saved.
const DECK_EXTS: &[&str] = &["pptx", "deckcraft"];

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// Files the parent sent since the last frame: (name, bytes).
    incoming: Rc<RefCell<Vec<(String, Vec<u8>)>>>,
    /// Names of the decks handed to the parent since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    /// Whether a "no presentations" state was reported (so docClosed fires once).
    closed_reported: Rc<Cell<bool>>,
    /// Commands the parent sent since the last frame: (id, params, request id).
    commands: Rc<RefCell<Vec<(String, Value, Option<String>)>>>,
    /// Length of the `params` text of the commands that haven't run yet.
    queued_bytes: Rc<Cell<usize>>,
    /// Total length of the command identifiers that haven't run yet.
    queued_id_bytes: Rc<Cell<usize>>,
    /// The uid of the presentation the host opened (the session keeps a blank deck around,
    /// so "closed" means the uid vanishing, not an empty document list).
    host_uid: Rc<Cell<Option<u64>>>,
}

impl Bridge {
    /// `None` when the page isn't framed (there is no parent to talk to).
    pub fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        // A sandboxed frame has the opaque origin "null": no sender can be vetted and nothing
        // can be addressed to the parent, so the bridge stays off.
        let origin = window.location().origin().ok().filter(|o| !matches!(o.as_str(), "" | "null" | "*"))?;
        Some(Self {
            window,
            parent,
            origin,
            incoming: Rc::default(),
            saved: Rc::default(),
            reported: Rc::default(),
            closed_reported: Rc::default(),
            commands: Rc::default(),
            queued_bytes: Rc::default(),
            queued_id_bytes: Rc::default(),
            host_uid: Rc::default(),
        })
    }

    /// Remember the presentation the host opened, to notice it closing later.
    pub fn set_host_uid(&self, uid: u64) {
        self.host_uid.set(Some(uid));
        self.closed_reported.set(false);
    }

    /// Send saved and exported files to the parent instead of downloading them.
    pub fn connect(&self, services: &mut Services, ctx: egui::Context) {
        self.listen(ctx);
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| {
            if let Err(e) = bridge.post_save(name, bytes) {
                log::error!("couldn't hand {name} to the page: {e}");
            }
        }));
    }

    /// Files the parent sent since the last frame, oldest first.
    pub fn take_incoming(&self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut *self.incoming.borrow_mut())
    }

    /// Commands the parent sent since the last frame, oldest first: (id, params, request id).
    pub fn take_commands(&self) -> Vec<(String, Value, Option<String>)> {
        self.queued_bytes.set(0);
        self.queued_id_bytes.set(0);
        std::mem::take(&mut *self.commands.borrow_mut())
    }

    pub fn post_ready(&self) {
        self.send(&[("type", "deckcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into())]);
    }

    /// Tell the parent how a command (`deckcraft:cmd`) ended.
    pub fn post_response(&self, id: &str, request_id: Option<&str>, result: Result<Value, String>) {
        let value = host_cmd::response(id, request_id, result);
        let sent = js_sys::JSON::parse(&value.to_string()).ok().is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok());
        if !sent {
            log::error!("couldn't answer command {id} to the page");
        }
    }

    /// Queue the command in a `deckcraft:cmd` message, or answer the parent at once when the
    /// message doesn't fit.
    fn receive_command(&self, data: &JsValue) {
        let id = string(data, "id");
        let request_id = string(data, "requestId");
        // Check the JavaScript string length before allocating the Rust copy, so an
        // oversized `params` can't exhaust iframe memory before host_cmd::parse rejects it.
        let params_value = js_sys::Reflect::get(data, &JsValue::from_str("params")).ok().filter(|v| !v.is_undefined() && !v.is_null());
        let params = match params_value {
            None => Some(None),
            Some(v) => {
                let js_len = v.dyn_ref::<js_sys::JsString>().map(|s| s.length() as usize);
                if js_len.unwrap_or(usize::MAX) > host_cmd::MAX_PARAMS_BYTES {
                    self.post_response(
                        id.as_deref().unwrap_or(""),
                        request_id.as_deref(),
                        Err(format!("params are larger than {} bytes", host_cmd::MAX_PARAMS_BYTES)),
                    );
                    return;
                }
                v.as_string().map(Some)
            }
        };
        let parsed = match params {
            Some(params) => host_cmd::parse(id.clone(), Some(params), request_id.clone()),
            None => Err("params must be a string of valid JSON".to_string()),
        };
        match parsed {
            Ok(c) if !host_cmd::has_room(self.commands.borrow().len(), self.queued_bytes.get(), c.params_bytes) => {
                self.post_response(&c.id, c.request_id.as_deref(), Err("too many commands are waiting; try again".to_string()));
            }
            Ok(c) => {
                let id_bytes = c.id.len() + c.request_id.as_deref().map(str::len).unwrap_or(0);
                if self.queued_id_bytes.get().saturating_add(id_bytes) > host_cmd::MAX_PENDING_PARAMS_BYTES {
                    self.post_response(&c.id, c.request_id.as_deref(), Err("too many commands are waiting; try again".to_string()));
                    return;
                }
                self.queued_bytes.set(self.queued_bytes.get().saturating_add(c.params_bytes));
                self.queued_id_bytes.set(self.queued_id_bytes.get().saturating_add(id_bytes));
                self.commands.borrow_mut().push((c.id, c.params, c.request_id));
            }
            Err(error) => self.post_response(id.as_deref().unwrap_or(""), request_id.as_deref(), Err(error)),
        }
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "deckcraft:failed".into()), ("error", error.into())]);
    }

    /// After each frame: a deck handed to the parent this frame counts as saved, and the parent
    /// hears when unsaved changes appear or go away. When the last presentation closes
    /// (File › Close), the parent hears `docClosed` once.
    pub fn sync(&self, app: &mut SlideApp) {
        let saved = std::mem::take(&mut *self.saved.borrow_mut());
        if saved.iter().any(|name| is_deck(name))
            && let Some(doc) = app.session.active_mut()
        {
            doc.saved_doc = doc.doc.clone();
        }
        let dirty = app.session.documents().iter().any(|d| d.is_dirty());
        if self.reported.get() != Some(dirty) && self.send(&[("type", "deckcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
        // The host's presentation closed (File › Close): the session keeps a blank deck around,
        // so the host's one is gone when its uid no longer appears in the document list.
        if let Some(uid) = self.host_uid.get()
            && !app.session.documents().iter().any(|d| d.uid == uid)
        {
            self.host_uid.set(None);
            self.send(&[("type", "deckcraft:docClosed".into())]);
        }
    }

    fn post_save(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let message = message(&[("type", "deckcraft:save".into()), ("name", name.as_str().into()), ("bytes", buffer.clone().into())])
            .ok_or_else(|| "couldn't build the message".to_string())?;
        self.parent
            .post_message_with_transfer(&message, &self.origin, &js_sys::Array::of1(&buffer))
            .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
        self.saved.borrow_mut().push(name);
        Ok(())
    }

    pub fn listen(&self, ctx: egui::Context) {
        let parent = self.parent.clone();
        let origin = self.origin.clone();
        let incoming = self.incoming.clone();
        let bridge = self.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            match string(&data, "type").as_deref() {
                Some("deckcraft:open") => {
                    let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "deck.pptx".to_string());
                    let Some(bytes) = bytes(&data) else { return };
                    incoming.borrow_mut().push((name, bytes));
                    ctx.request_repaint();
                }
                Some("deckcraft:cmd") => {
                    bridge.receive_command(&data);
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

fn is_deck(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| DECK_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
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
