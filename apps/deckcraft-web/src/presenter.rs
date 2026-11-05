//! Same-origin popup transport. Each window starts its own eframe/WASM runner.
use deckcraft_ui_egui::presenter::{Command, DeltaCache, Host, Snapshot, View};
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::{Rc, Weak},
};
use wasm_bindgen::{JsCast, closure::Closure};

const MAX_MESSAGE: usize = 16 * 1024 * 1024;
const MAX_COMMANDS: usize = 64;
const PREFIX: &str = "#deckcraft-presenter=";
fn error(e: wasm_bindgen::JsValue) -> String {
    format!("{e:?}")
}
fn origin() -> Result<String, String> {
    web_sys::window().ok_or("No browser window")?.location().origin().map_err(error)
}
pub fn token() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let token = hash.strip_prefix(PREFIX)?;
    (!token.is_empty() && token.len() < 128).then(|| token.to_owned())
}
fn same_source(event: &web_sys::MessageEvent, window: &web_sys::Window, expected_origin: &str) -> bool {
    event.origin() == expected_origin && event.source().is_some_and(|source| js_sys::Object::is(source.as_ref(), window.as_ref()))
}
fn send(window: &web_sys::Window, origin: &str, value: serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    if text.len() > MAX_MESSAGE {
        return Err("Presenter message is too large".into());
    }
    window.post_message(&wasm_bindgen::JsValue::from_str(&text), origin).map_err(error)
}
struct State {
    popup: Option<web_sys::Window>,
    token: String,
    commands: VecDeque<Command>,
    ready: bool,
    refresh_images: bool,
}
pub struct BrowserHost {
    state: Rc<RefCell<State>>,
    origin: String,
    listener: Closure<dyn FnMut(web_sys::MessageEvent)>,
    button: Option<(web_sys::HtmlElement, Closure<dyn FnMut(web_sys::Event)>)>,
    images: DeltaCache,
}
impl BrowserHost {
    pub fn new(ctx: egui::Context) -> Result<Self, String> {
        let window = web_sys::window().ok_or("No browser window")?;
        let origin = origin()?;
        let state = Rc::new(RefCell::new(State { popup: None, token: String::new(), commands: VecDeque::new(), ready: false, refresh_images: true }));
        let weak = Rc::downgrade(&state);
        let expected = origin.clone();
        let listener = Closure::new(move |event: web_sys::MessageEvent| {
            let Some(state) = weak.upgrade() else { return };
            let Ok(mut state) = state.try_borrow_mut() else { return };
            let Some(popup) = state.popup.as_ref() else { return };
            if !same_source(&event, popup, &expected) {
                return;
            }
            let Some(text) = event.data().as_string().filter(|s| s.len() <= 8192) else { return };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return };
            if value.get("token").and_then(|v| v.as_str()) != Some(state.token.as_str()) {
                return;
            }
            match value.get("kind").and_then(|v| v.as_str()) {
                Some("ready") => {
                    state.ready = true;
                    state.refresh_images = true;
                }
                Some("command") if state.commands.len() < MAX_COMMANDS => {
                    if let Some(command) = value.get("command").and_then(|v| serde_json::from_value(v.clone()).ok()) {
                        state.commands.push_back(command);
                    }
                }
                _ => {}
            }
            ctx.request_repaint();
        });
        window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref()).map_err(error)?;
        Ok(Self { state, origin, listener, button: None, images: DeltaCache::default() })
    }
    fn remove_button(&mut self) {
        if let Some((button, callback)) = self.button.take() {
            let _ = button.remove_event_listener_with_callback("click", callback.as_ref().unchecked_ref());
            button.remove();
        }
    }
    fn recovery_button(&mut self) {
        if self.button.is_some() {
            return;
        }
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Some(body) = document.body() else { return };
        let Ok(element) = document.create_element("button") else { return };
        let Ok(button) = element.dyn_into::<web_sys::HtmlElement>() else { return };
        button.set_text_content(Some("Open presenter window"));
        let _ = button.set_attribute("style", "position:fixed;top:12px;right:12px;z-index:9999;padding:12px;color:white;background:#333;border:1px solid #aaa;border-radius:4px;cursor:pointer");
        let weak: Weak<RefCell<State>> = Rc::downgrade(&self.state);
        let button_copy = button.clone();
        let callback = Closure::new(move |_event: web_sys::Event| {
            if let Some(state) = weak.upgrade() {
                match open_popup(&state) {
                    Ok(()) => {
                        button_copy.set_text_content(Some("Opening presenter window…"));
                    }
                    Err(_) => {
                        button_copy.set_text_content(Some("Popup blocked. Allow popups, then click to retry."));
                    }
                }
            }
        });
        if button.add_event_listener_with_callback("click", callback.as_ref().unchecked_ref()).is_ok() && body.append_child(&button).is_ok() {
            self.button = Some((button, callback));
        } else {
            let _ = button.remove_event_listener_with_callback("click", callback.as_ref().unchecked_ref());
            button.remove();
        }
    }
}
fn open_popup(state: &Rc<RefCell<State>>) -> Result<(), String> {
    let window = web_sys::window().ok_or("No browser window")?;
    let mut state = state.try_borrow_mut().map_err(|_| "Presenter is busy")?;
    if let Some(popup) = &state.popup
        && !popup.closed().unwrap_or(true)
    {
        let _ = popup.focus();
        return Ok(());
    }
    let token = format!("{}-{}", js_sys::Date::now(), js_sys::Math::random());
    let url = web_sys::Url::new(&window.location().href().map_err(error)?).map_err(error)?;
    url.set_hash(&format!("{PREFIX}{token}"));
    let popup = window
        .open_with_url_and_target_and_features(&url.href(), "_blank", "popup,width=1100,height=700")
        .map_err(error)?
        .ok_or("Presenter popup was blocked. Use Open presenter window to retry.")?;
    if popup.closed().unwrap_or(true) {
        return Err("Browser isolation prevented connecting the presenter window".into());
    }
    state.popup = Some(popup);
    state.token = token;
    state.commands.clear();
    state.ready = false;
    Ok(())
}
impl Host for BrowserHost {
    fn open(&mut self) -> Result<(), String> {
        self.close();
        let result = open_popup(&self.state);
        if result.is_err() {
            self.recovery_button();
        }
        result
    }
    fn is_open(&self) -> bool {
        self.state.borrow().popup.as_ref().is_some_and(|w| !w.closed().unwrap_or(true))
    }
    fn commands(&mut self) -> Vec<Command> {
        self.state.borrow_mut().commands.drain(..).collect()
    }
    fn update(&mut self, snapshot: Snapshot) {
        if !self.is_open() {
            self.recovery_button();
            return;
        }
        self.remove_button();
        let mut state = self.state.borrow_mut();
        if !state.ready {
            return;
        }
        if let Some(popup) = state.popup.clone() {
            let token = state.token.clone();
            let sent = self.images.send(snapshot, &token, state.refresh_images, |delta, current_changed, next_changed| {
                send(
                    &popup,
                    &self.origin,
                    serde_json::json!({"kind":"snapshot", "token":token, "snapshot":delta,
                    "currentChanged":current_changed, "nextChanged":next_changed}),
                )
            });
            if sent.is_ok() {
                state.refresh_images = false;
            }
        }
    }
    fn close(&mut self) {
        self.remove_button();
        let mut state = self.state.borrow_mut();
        if let Some(window) = state.popup.take() {
            let _ = window.close();
        }
        state.commands.clear();
        state.ready = false;
        state.token.clear();
    }
}
impl Drop for BrowserHost {
    fn drop(&mut self) {
        self.close();
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback("message", self.listener.as_ref().unchecked_ref());
        }
    }
}

pub struct Popup {
    token: String,
    origin: String,
    opener: web_sys::Window,
    snapshot: Rc<RefCell<Option<Snapshot>>>,
    last_received: Rc<RefCell<f64>>,
    listener: Closure<dyn FnMut(web_sys::MessageEvent)>,
    view: View,
}
impl Popup {
    pub fn new(token: String, ctx: egui::Context) -> Result<Self, String> {
        let window = web_sys::window().ok_or("No browser window")?;
        // `opener` is a WindowProxy from another realm; instanceof Window can fail across
        // realms. The browser API supplies a WindowProxy or null, and messages still verify
        // exact source identity and origin before any data is accepted.
        let opener = window.opener().map_err(error)?;
        if opener.is_null() || opener.is_undefined() {
            return Err("The audience window is unavailable".into());
        }
        let opener: web_sys::Window = opener.unchecked_into();
        let origin = origin()?;
        let snapshot: Rc<RefCell<Option<Snapshot>>> = Rc::new(RefCell::new(None));
        let last_received = Rc::new(RefCell::new(js_sys::Date::now()));
        let received = last_received.clone();
        let target = snapshot.clone();
        let expected_origin = origin.clone();
        let expected_opener = opener.clone();
        let expected_token = token.clone();
        let listener = Closure::new(move |event: web_sys::MessageEvent| {
            if !same_source(&event, &expected_opener, &expected_origin) {
                return;
            }
            let Some(text) = event.data().as_string().filter(|s| s.len() <= MAX_MESSAGE) else { return };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return };
            if value.get("token").and_then(|v| v.as_str()) != Some(expected_token.as_str())
                || value.get("kind").and_then(|v| v.as_str()) != Some("snapshot")
            {
                return;
            }
            if let Some(mut data) = value.get("snapshot").and_then(|v| serde_json::from_value::<Snapshot>(v.clone()).ok()) {
                if let Some(previous) = target.borrow().as_ref() {
                    if value.get("currentChanged").and_then(|v| v.as_bool()) == Some(false) {
                        data.current = previous.current.clone();
                    }
                    if value.get("nextChanged").and_then(|v| v.as_bool()) == Some(false) {
                        data.next = previous.next.clone();
                    }
                }
                *target.borrow_mut() = Some(data);
                *received.borrow_mut() = js_sys::Date::now();
                ctx.request_repaint();
            }
        });
        window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref()).map_err(error)?;
        if let Some(document) = window.document() {
            document.set_title("Presenter View — DeckCraft");
        }
        let popup = Self { token, origin, opener, snapshot, last_received, listener, view: View::default() };
        send(&popup.opener, &popup.origin, serde_json::json!({"kind":"ready","token":popup.token}))?;
        Ok(popup)
    }
}
impl eframe::App for Popup {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let snapshot = self.snapshot.borrow();
        let notes = snapshot.as_ref().map_or("", |snapshot| snapshot.notes.as_str());
        if !self.view.prepare(ui.ctx(), notes) {
            return;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        if self.opener.closed().unwrap_or(true) {
            ui.heading("The audience window has closed.");
            return;
        }
        if js_sys::Date::now() - *self.last_received.borrow() > 10_000.0 {
            ui.heading("The audience window is not responding.");
            ui.label("Return to the audience window to resume the connection.");
            return;
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(snapshot) = snapshot.as_ref() {
                for command in self.view.ui(ui, snapshot) {
                    let _ = send(&self.opener, &self.origin, serde_json::json!({"kind":"command","token":self.token,"command":command}));
                }
            } else {
                ui.label("Waiting for the audience window…");
            }
        });
    }
}
impl Drop for Popup {
    fn drop(&mut self) {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback("message", self.listener.as_ref().unchecked_ref());
        }
    }
}
