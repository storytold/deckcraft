//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! DeckCraft in an `<iframe>` (such as a file manager). It follows GridCraft's and WordCraft's:
//!
//! - `deckcraft:ready` (to the parent): `{ version }`, the app is listening; send a file now.
//! - `deckcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a presentation dropped on the window.
//! - `deckcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file DeckCraft would otherwise download: Save, Save As, the exports and Print's PDF. Save
//!   writes `<title>.deckcraft`; posting a `.deckcraft` file marks the presentation saved.
//! - `deckcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `deckcraft:failed` (to the parent): `{ error }` when the app couldn't start, or a
//!   `deckcraft:open` was refused (over `MAX_OPEN_BYTES`, 512 MiB).
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the page
//! starts on a blank presentation instead of the sample deck. `?author=` names the user
//! (comments and the author of new presentations).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use deckcraft_ui_egui::{Services, SlideApp};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

/// The longest user name taken from `?author=` (the preference's own limit).
const MAX_AUTHOR_CHARS: usize = 256;
/// Largest document taken from the parent page, checked before its bytes are copied: the same
/// 512 MiB as the largest single part DeckCraft's PPTX reader decompresses (`deckcraft_pptx::opc::MAX_PART`).
const MAX_OPEN_BYTES: u64 = 512 * 1024 * 1024;

/// Why `len` bytes from the parent can't be opened, or `None` if they can.
fn oversize(len: u64) -> Option<String> {
    (len > MAX_OPEN_BYTES).then(|| format!("document is {len} bytes; the largest DeckCraft opens from the host is {MAX_OPEN_BYTES} bytes (512 MiB)"))
}

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    /// Names of the files posted to the parent since the last frame.
    posted: Rc<RefCell<Vec<String>>>,
    /// Files the parent sent, waiting for the next frame.
    received: Rc<RefCell<Vec<(String, Vec<u8>)>>>,
}

impl Bridge {
    /// `None` when the page isn't framed (there is no parent to talk to).
    pub fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let origin = window.location().origin().ok()?;
        Some(Self { window, parent, origin, reported: Rc::default(), posted: Rc::default(), received: Rc::default() })
    }

    /// Send saved and exported files to the parent instead of downloading them, and hear files
    /// from it.
    pub fn connect(&self, services: &mut Services, ctx: egui::Context) {
        self.listen(ctx);
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| match bridge.post_save(name, bytes) {
            Ok(name) => bridge.posted.borrow_mut().push(name),
            Err(e) => log::error!("couldn't hand {name} to the host: {e}"),
        }));
    }

    /// `?author=`: the host's name for the user.
    pub fn set_author(app: &mut SlideApp, author: &str) {
        let author: String = author.trim().chars().take(MAX_AUTHOR_CHARS).collect();
        if !author.is_empty() {
            app.session.prefs.author = author;
        }
    }

    /// Once a frame: open what the parent sent, mark a natively saved presentation clean, and
    /// report unsaved changes.
    pub fn sync(&self, app: &mut SlideApp) {
        for (name, bytes) in std::mem::take(&mut *self.received.borrow_mut()) {
            app.open_or_insert_bytes(&name, &bytes);
        }
        let posted = std::mem::take(&mut *self.posted.borrow_mut());
        if posted.iter().any(|n| n.to_ascii_lowercase().ends_with(".deckcraft"))
            && let Some(d) = app.session.active_mut()
        {
            d.saved_doc = d.doc.clone();
        }
        let dirty = app.session.active().is_some_and(|d| d.is_dirty());
        if self.reported.get() != Some(dirty) && self.send(&[("type", "deckcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
    }

    pub fn post_ready(&self) {
        self.send(&[("type", "deckcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into())]);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "deckcraft:failed".into()), ("error", error.into())]);
    }

    /// Post a file to the parent; the name it went under.
    fn post_save(&self, path: &str, bytes: &[u8]) -> Result<String, String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let message = message(&[("type", "deckcraft:save".into()), ("name", name.as_str().into()), ("bytes", buffer.clone().into())])
            .ok_or_else(|| "couldn't build the message".to_string())?;
        self.parent
            .post_message_with_transfer(&message, &self.origin, &js_sys::Array::of1(&buffer))
            .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
        Ok(name)
    }

    fn listen(&self, ctx: egui::Context) {
        let parent = self.parent.clone();
        let origin = self.origin.clone();
        let received = self.received.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            if string(&data, "type").as_deref() != Some("deckcraft:open") {
                return;
            }
            let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Presentation.deckcraft".to_string());
            let Some(array) = byte_array(&data) else { return };
            if let Some(error) = oversize(u64::from(array.byte_length())) {
                if let Some(m) = message(&[("type", "deckcraft:failed".into()), ("error", error.as_str().into())]) {
                    let _ = parent.post_message(&m, &origin);
                }
                return;
            }
            let bytes = array.to_vec();
            received.borrow_mut().push((name, bytes));
            ctx.request_repaint();
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

/// `bytes` (an `ArrayBuffer` or `Uint8Array`) as a view, not yet copied.
fn byte_array(data: &JsValue) -> Option<js_sys::Uint8Array> {
    let bytes = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
    if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
        Some(array.clone())
    } else if bytes.is_instance_of::<js_sys::ArrayBuffer>() {
        Some(js_sys::Uint8Array::new(&bytes))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_cap() {
        assert!(oversize(MAX_OPEN_BYTES).is_none());
        assert!(oversize(MAX_OPEN_BYTES + 1).is_some_and(|e| e.contains("512 MiB")));
    }
}
