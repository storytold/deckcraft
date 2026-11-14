//! Contributor-original, runtime-generated deck; no external media, fonts or sample files.
//! Real browser API/publisher/application logic coverage, without a WebRunner or GPU renderer.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_ui_egui::file_providers::Host;
use deckcraft_ui_egui::{Services, SlideApp};
use js_sys::{Array, Function, Object, RangeError, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

const API: &str = "deckcraftFileProviders";
const EVENT: &str = "deckcraft-file-provider";

/// Restores the fixture's own change on every returned success or error.
struct CopyOverride {
    constructor: Object,
    property: JsValue,
    descriptor: Option<Object>,
}

impl CopyOverride {
    fn restore(&self) -> Result<bool, JsValue> {
        match &self.descriptor {
            Some(descriptor) => Reflect::define_property(&self.constructor, &self.property, descriptor),
            None => Reflect::delete_property(&self.constructor, &self.property),
        }
    }
}

impl Drop for CopyOverride {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn with_copy_failure(action: impl FnOnce()) -> Result<u32, JsValue> {
    let constructor = Reflect::get(&js_sys::global(), &"Uint8Array".into())?.dyn_into::<Object>()?;
    let property = JsValue::from_str("from");
    let original = Reflect::get(&constructor, &property)?;
    // Uint8Array typically inherits from %TypedArray%; remove the shadow on cleanup.
    let descriptor = Reflect::get_own_property_descriptor(&constructor, &property)?;
    let descriptor = if descriptor.is_undefined() { None } else { Some(descriptor.dyn_into::<Object>()?) };
    let restore = CopyOverride { constructor, property, descriptor };
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let fail = Closure::<dyn Fn() -> Result<JsValue, JsValue>>::new(move || {
        count.set(count.get() + 1);
        Err(RangeError::new("synthetic Deck provider copy failure").into())
    });
    if !Reflect::set(&restore.constructor, &restore.property, fail.as_ref())? {
        return Err(JsValue::from_str("could not install provider copy fault"));
    }
    action();
    let restored = restore.restore()?;
    if !restored || !Object::is(&Reflect::get(&restore.constructor, &restore.property)?, &original) {
        return Err(JsValue::from_str("provider fixture did not restore Uint8Array.from"));
    }
    Ok(calls.get())
}

fn field(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &key.into()).unwrap()
}

fn text(value: &JsValue, key: &str) -> String {
    field(value, key).as_string().unwrap()
}

struct Page {
    window: web_sys::Window,
    host: Host,
    ctx: egui::Context,
    api: JsValue,
    events: Rc<RefCell<Vec<JsValue>>>,
    listener: Closure<dyn FnMut(web_sys::Event)>,
}

impl Page {
    fn new() -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("fixture requires a Window"))?;
        assert!(!Reflect::has(&window, &API.into())?, "fixture requires its own fresh page");
        let host = Host::default();
        let ctx = egui::Context::default();
        let events = Rc::new(RefCell::new(Vec::new()));
        let seen = events.clone();
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            seen.borrow_mut().push(event.dyn_into::<web_sys::CustomEvent>().unwrap().detail());
        });
        let mut page = Self { window, host, ctx, api: JsValue::UNDEFINED, events, listener };
        // Own teardown before install: ready dispatch can fail after publishing the API.
        let installed = crate::file_providers::install(page.host.clone(), page.ctx.clone());
        page.api = Reflect::get(&page.window, &API.into())?;
        installed?;
        page.window.add_event_listener_with_callback(EVENT, page.listener.as_ref().unchecked_ref())?;
        Ok(page)
    }

    fn call(&self, method: &str, arguments: &[JsValue]) -> Result<JsValue, JsValue> {
        let function = field(&self.api, method).dyn_into::<Function>()?;
        let array = Array::new();
        for argument in arguments {
            array.push(argument);
        }
        function.apply(&self.api, &array)
    }

    fn latest(&self, kind: &str) -> JsValue {
        self.events.borrow().iter().rev().find(|event| text(event, "type") == kind).unwrap().clone()
    }

    fn document_provider_bound(&self, document: u64) -> bool {
        let snapshot = self.host.snapshot();
        let document = document.to_string();
        let documents = snapshot["documents"].as_array().unwrap();
        let entry = documents.iter().find(|entry| entry["id"].as_str() == Some(document.as_str())).unwrap();
        entry["provider"].as_bool().unwrap()
    }

    fn tick(&self, app: &mut SlideApp) {
        self.ctx.run_ui(Default::default(), |ui| app.logic(ui.ctx())).drop_without_applying_deltas();
    }

    fn capture_save(&self, app: &mut SlideApp) -> String {
        app.save();
        crate::file_providers::publish(&self.host, &self.ctx);
        let request = text(&self.latest("saveDialog"), "requestId");
        self.call("resolveSaveDialog", &[request.clone().into(), "provider".into(), "Synthetic.deckcraft".into()]).unwrap();
        self.tick(app);
        request
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        let _ = self.window.remove_event_listener_with_callback(EVENT, self.listener.as_ref().unchecked_ref());
        if self.api.is_object() && Reflect::get(&self.window, &API.into()).is_ok_and(|current| Object::is(&current, &self.api)) {
            let _ = Reflect::delete_property(self.window.unchecked_ref::<Object>(), &API.into());
        }
    }
}

#[wasm_bindgen_test]
fn production_publisher_recovers_copy_failure_without_acknowledging_deck() -> Result<(), JsValue> {
    let page = Page::new()?;
    let connection = page.call("registerProvider", &["fixture".into(), "Fixture storage".into(), "account-one".into()]).unwrap();
    let resource = Object::new();
    Reflect::set(&resource, &"id".into(), &"one".into()).unwrap();
    Reflect::set(&resource, &"name".into(), &"Synthetic.deckcraft".into()).unwrap();
    let resources = Array::new();
    resources.push(&resource);
    page.call("setResources", &[connection.clone(), resources.into()]).unwrap();

    let services = Services { file_providers: Some(page.host.clone()), ..Default::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.session.execute("slide.new", &serde_json::json!({})).unwrap();
    let document = app.session.active().unwrap().uid;
    let revision = app.session.active().unwrap().revision;
    let checkpoint = app.session.active().unwrap().saved_doc.clone();
    assert!(app.session.active().unwrap().is_dirty());
    let failed = page.capture_save(&mut app);
    let later_read = page.call("requestOpen", &["one".into()]).unwrap();
    let throws = with_copy_failure(|| crate::file_providers::publish(&page.host, &page.ctx))?;
    assert_eq!(throws, 1, "the production catch-enabled copy import must actually throw once");
    assert!(!page.events.borrow().iter().any(|event| text(event, "type") == "save"));
    let later = page.latest("openRequest");
    assert_eq!(field(&later, "requestId"), later_read);
    assert_eq!(field(&later, "connectionId"), connection);
    assert_eq!(text(&later, "resourceId"), "one");
    assert!(page.call("saveResult", &[failed.clone().into(), "saved".into(), "one".into()]).is_err());
    page.call("cancelOpen", &[later_read, "fixture complete".into()]).unwrap();
    page.tick(&mut app);
    crate::file_providers::publish(&page.host, &page.ctx);
    assert!(!page.host.busy() && !page.document_provider_bound(document));
    assert!(app.fresh_status().unwrap().contains("Provider save failed"), "real application logic must consume the failure");
    let state = app.session.active().unwrap();
    assert_eq!((state.uid, state.revision), (document, revision));
    assert!(state.is_dirty() && Arc::ptr_eq(&state.saved_doc, &checkpoint));

    let retry = page.capture_save(&mut app);
    assert_ne!(retry, failed);
    crate::file_providers::publish(&page.host, &page.ctx);
    let saved = page.latest("save");
    assert_eq!(text(&saved, "requestId"), retry);
    assert_eq!(text(&saved, "documentId"), document.to_string());
    assert_eq!(field(&saved, "connectionId"), connection);
    assert_eq!(text(&saved, "namespace"), "account-one");
    let bytes = field(&saved, "bytes").dyn_into::<Uint8Array>().unwrap();
    assert!(bytes.length() > 0, "retry must deliver actual encoded native deck bytes through the window event");
    let (restored, damage) = deckcraft_engine::cmd::file::open_checked("Synthetic.deckcraft", &bytes.to_vec()).unwrap();
    assert!(damage.is_empty(), "the actual retry payload must decode without lost resources");
    assert_eq!(&restored, app.session.active().unwrap().doc.as_ref(), "the retry must round-trip the captured presentation");
    assert!(page.host.pending_for(document) && app.session.active().unwrap().is_dirty());
    assert!(Arc::ptr_eq(&app.session.active().unwrap().saved_doc, &checkpoint));
    app.session.execute("slide.new", &serde_json::json!({})).unwrap();
    let later_revision = app.session.active().unwrap().revision;
    page.call("saveResult", &[retry.into(), "saved".into(), "one".into()]).unwrap();
    page.tick(&mut app);
    let state = app.session.active().unwrap();
    assert_eq!((state.uid, state.revision), (document, later_revision));
    assert!(state.is_dirty() && Arc::ptr_eq(&state.saved_doc, &checkpoint), "an older ACK cannot acknowledge later edits");
    assert!(page.document_provider_bound(document) && !page.host.busy());
    page.call("unregisterProvider", &[]).unwrap();
    Ok(())
}
