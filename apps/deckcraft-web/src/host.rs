//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! DeckCraft in an `<iframe>` (such as a Nextcloud app). Messages are plain objects with a `type`:
//!
//! - `deckcraft:ready` (to the parent): `{ version }`, the app is listening; send a file now.
//! - `deckcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It arrives like a file picked with File › Open or dropped on the window:
//!   presentations open, pictures, audio and video are inserted.
//! - `deckcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file DeckCraft would otherwise download: Save, Save As and Export. The parent stores it and
//!   reports failures itself.
//! - `deckcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `deckcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the page
//! starts without the sample deck, and the parent guards leaving the page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use deckcraft_ui_egui::{Services, SlideApp};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

/// Saving one of these hands the whole presentation to the parent, which stores it: the
/// presentation counts as saved, as it would after Save on the desktop.
const PRESENTATION_EXTS: &[&str] = &["deckcraft", "pptx", "potx", "ppsx"];

/// The longest author name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// Names of the files handed to the parent since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    /// Files the parent sent, waiting for the next frame: (name, bytes).
    opened: Rc<RefCell<Vec<(String, Vec<u8>)>>>,
    /// The app's context, for a frame after a file opens.
    ctx: Rc<RefCell<Option<egui::Context>>>,
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
        Some(Self { window, parent, origin, saved: Rc::default(), reported: Rc::default(), opened: Rc::default(), ctx: Rc::default() })
    }

    /// Send saved and exported files to the parent instead of downloading them, and hear files
    /// from it.
    pub fn connect(&self, services: &mut Services, ctx: egui::Context) {
        self.listen(ctx);
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| {
            if let Err(e) = bridge.post_save(name, bytes) {
                log::error!("couldn't hand {name} to the page: {e}");
            }
        }));
        let bridge = self.clone();
        services.write = Some(Box::new(move |name: &str, bytes: &[u8]| bridge.post_save(name, bytes)));
    }

    /// `?author=`: the host's name for the user signs comments.
    pub fn set_author(app: &mut SlideApp, author: &str) {
        let author: String = author.trim().chars().take(MAX_AUTHOR_CHARS).collect();
        if !author.is_empty() {
            app.session.prefs.author = author;
        }
    }

    pub fn post_ready(&self) {
        self.send(&[("type", "deckcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into())]);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "deckcraft:failed".into()), ("error", error.into())]);
    }

    /// After each frame: files the parent sent open (or are inserted) like dropped files, a
    /// presentation handed to the parent this frame counts as saved, and the parent hears when
    /// unsaved changes appear or go away.
    pub fn sync(&self, app: &mut SlideApp) {
        let opened = std::mem::take(&mut *self.opened.borrow_mut());
        if !opened.is_empty() {
            for (name, bytes) in opened {
                app.open_or_insert_bytes(&name, &bytes);
            }
            // Show it: this runs after the frame was drawn.
            if let Some(ctx) = &*self.ctx.borrow() {
                ctx.request_repaint();
            }
        }
        let saved = std::mem::take(&mut *self.saved.borrow_mut());
        if saved.iter().any(|name| is_presentation(name))
            && let Some(doc) = app.session.active_mut()
        {
            doc.saved_doc = doc.doc.clone();
            doc.revision = doc.revision.saturating_add(1);
        }
        let dirty = app.session.documents().iter().any(|d| d.is_dirty());
        if self.reported.get() != Some(dirty) && self.send(&[("type", "deckcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
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

    fn listen(&self, ctx: egui::Context) {
        *self.ctx.borrow_mut() = Some(ctx.clone());
        let parent = self.parent.clone();
        let origin = self.origin.clone();
        let opened = self.opened.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            if string(&data, "type").as_deref() != Some("deckcraft:open") {
                return;
            }
            let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Presentation.pptx".to_string());
            let Some(bytes) = bytes(&data) else { return };
            opened.borrow_mut().push((name, bytes));
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

fn is_presentation(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| PRESENTATION_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
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
