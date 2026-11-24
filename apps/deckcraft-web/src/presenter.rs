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
/// The audience window opened by the presenter tab (the tab running DeckCraft is the presenter
/// view, this window shows the presentation).
const AUDIENCE_PREFIX: &str = "#deckcraft-audience=";
/// The biggest deck handed to the audience window.
const MAX_DECK: usize = 512 * 1024 * 1024;
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
pub fn audience_token() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let token = hash.strip_prefix(AUDIENCE_PREFIX)?;
    (!token.is_empty() && token.len() < 128).then(|| token.to_owned())
}
/// `window.opener` of a window the presenter tab opened. It is a WindowProxy from another realm,
/// so messages still verify its identity and the origin before any data is accepted.
fn opener_window() -> Result<web_sys::Window, String> {
    let opener = web_sys::window().ok_or("No browser window")?.opener().map_err(error)?;
    if opener.is_null() || opener.is_undefined() {
        return Err("The presenter window is unavailable".into());
    }
    Ok(opener.unchecked_into())
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
    /// This is the audience window opened by the presenter tab: its peer is the opener, which it
    /// never opens or closes, and when the show ends it closes itself.
    audience: bool,
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
        Ok(Self { state, origin, listener, button: None, images: DeltaCache::default(), audience: false })
    }
    /// The host of the audience window: the peer is the presenter tab that opened this window.
    pub fn for_opener(ctx: egui::Context, token: String) -> Result<Self, String> {
        let mut host = Self::new(ctx)?;
        {
            let mut state = host.state.borrow_mut();
            state.popup = Some(opener_window()?);
            state.token = token;
        }
        host.audience = true;
        Ok(host)
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
        if self.audience {
            return Ok(());
        }
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
            if !self.audience {
                self.recovery_button();
            }
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
        if self.audience {
            // The show is over: tell the presenter tab, then close this window.
            let state = self.state.borrow();
            if let Some(peer) = state.popup.as_ref() {
                let _ = send(peer, &self.origin, serde_json::json!({"kind":"ended","token":state.token}));
            }
            drop(state);
            if let Some(window) = web_sys::window() {
                let _ = window.close();
            }
            return;
        }
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
    /// The other window: the audience (when this is the presenter) or its opener.
    opener: web_sys::Window,
    snapshot: Rc<RefCell<Option<Snapshot>>>,
    last_received: Rc<RefCell<f64>>,
    listener: Closure<dyn FnMut(web_sys::MessageEvent)>,
    view: View,
    /// The audience window has loaded and asked for the deck.
    hello: Rc<std::cell::Cell<bool>>,
    /// The audience window said the show is over.
    ended: Rc<std::cell::Cell<bool>>,
    opened_at: f64,
}
impl Popup {
    pub fn new(token: String, ctx: egui::Context) -> Result<Self, String> {
        let window = web_sys::window().ok_or("No browser window")?;
        let popup = Self::attach(opener_window()?, token, ctx, View::default())?;
        if let Some(document) = window.document() {
            document.set_title("Presenter View — DeckCraft");
        }
        send(&popup.opener, &popup.origin, serde_json::json!({"kind":"ready","token":popup.token}))?;
        Ok(popup)
    }

    /// This tab becomes the presenter view: open the audience window, which shows the
    /// presentation. It asks for the deck (`take_hello`, `start`) once it has loaded.
    pub fn open_audience(ctx: egui::Context) -> Result<Self, String> {
        let window = web_sys::window().ok_or("No browser window")?;
        let token = format!("{}-{}", js_sys::Date::now(), js_sys::Math::random());
        let url = web_sys::Url::new(&window.location().href().map_err(error)?).map_err(error)?;
        url.set_hash(&format!("{AUDIENCE_PREFIX}{token}"));
        let peer = window
            .open_with_url_and_target_and_features(&url.href(), "_blank", "popup,width=1280,height=720")
            .map_err(error)?
            .ok_or("The presentation window was blocked. Allow popups for this site.")?;
        if peer.closed().unwrap_or(true) {
            return Err("Browser isolation prevented connecting the presentation window".into());
        }
        Self::attach(peer, token, ctx, View::embedded())
    }

    /// The audience window asked for the deck (once).
    pub fn take_hello(&self) -> bool {
        self.hello.replace(false)
    }

    /// Give the audience window the deck (a `.deckcraft` file) and the slide to start on, then
    /// tell it the presenter is ready for snapshots.
    pub fn start(&self, name: &str, from: usize, bytes: &[u8]) -> Result<(), String> {
        let message = js_sys::Object::new();
        let set = |key: &str, value: wasm_bindgen::JsValue| js_sys::Reflect::set(&message, &key.into(), &value).map(|_| ()).map_err(error);
        set("kind", "deck".into())?;
        set("token", self.token.as_str().into())?;
        set("name", name.into())?;
        set("from", (from as f64).into())?;
        let array = js_sys::Uint8Array::from(bytes);
        set("bytes", array.clone().into())?;
        let transfer = js_sys::Array::of1(&array.buffer());
        self.opener.post_message_with_transfer(&message, &self.origin, &transfer).map_err(error)?;
        send(&self.opener, &self.origin, serde_json::json!({"kind":"ready","token":self.token}))
    }

    /// The show is over: the audience window said so, or it was closed.
    pub fn ended(&self) -> bool {
        self.ended.get() || self.opener.closed().unwrap_or(true)
    }

    /// Seconds since this tab became the presenter view with no word from the audience window.
    pub fn waited(&self) -> f64 {
        (js_sys::Date::now() - self.opened_at) / 1000.0
    }

    /// The slide the show was last on.
    pub fn last_slide(&self) -> usize {
        self.snapshot.borrow().as_ref().map_or(0, |s| s.slide.min(s.total.saturating_sub(1)))
    }

    /// Whether the audience window has sent its first snapshot.
    pub fn connected(&self) -> bool {
        self.snapshot.borrow().is_some()
    }

    /// Ask the audience window to end the show and close.
    pub fn end(&self) {
        let _ = send(&self.opener, &self.origin, serde_json::json!({"kind":"command","token":self.token,"command":Command::End}));
    }

    fn attach(opener: web_sys::Window, token: String, ctx: egui::Context, view: View) -> Result<Self, String> {
        let window = web_sys::window().ok_or("No browser window")?;
        let origin = origin()?;
        let hello = Rc::new(std::cell::Cell::new(false));
        let ended = Rc::new(std::cell::Cell::new(false));
        let (hello_flag, ended_flag) = (hello.clone(), ended.clone());
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
            if value.get("token").and_then(|v| v.as_str()) != Some(expected_token.as_str()) {
                return;
            }
            match value.get("kind").and_then(|v| v.as_str()) {
                Some("hello") => {
                    hello_flag.set(true);
                    ctx.request_repaint();
                    return;
                }
                Some("ended") => {
                    ended_flag.set(true);
                    ctx.request_repaint();
                    return;
                }
                Some("snapshot") => {}
                _ => return,
            }
            if let Some(mut data) = value.get("snapshot").and_then(|v| serde_json::from_value::<Snapshot>(v.clone()).ok()) {
                if let Some(previous) = target.borrow().as_ref() {
                    if value.get("currentChanged").and_then(|v| v.as_bool()) == Some(false) {
                        data.current = previous.current.clone();
                    }
                    if value.get("nextChanged").and_then(|v| v.as_bool()) == Some(false) {
                        data.next = previous.next.clone();
                    }
                    if data.slides.is_none() {
                        data.slides = previous.slides.clone();
                    }
                }
                *target.borrow_mut() = Some(data);
                *received.borrow_mut() = js_sys::Date::now();
                ctx.request_repaint();
            }
        });
        window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref()).map_err(error)?;
        Ok(Self { token, origin, opener, snapshot, last_received, listener, view, hello, ended, opened_at: js_sys::Date::now() })
    }

    /// Draw the presenter view into `ui`.
    pub fn show(&mut self, ui: &mut egui::Ui) {
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
        if snapshot.is_some() && js_sys::Date::now() - *self.last_received.borrow() > 10_000.0 {
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
                ui.label("Opening the presentation window…");
            }
        });
    }
}
impl eframe::App for Popup {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// The audience window waiting for the deck from the presenter tab that opened it.
pub struct AudienceLink {
    deck: Rc<RefCell<Option<Deck>>>,
    listener: Closure<dyn FnMut(web_sys::MessageEvent)>,
}
/// The deck the presenter tab handed over, and the slide to start on.
pub struct Deck {
    pub name: String,
    pub from: usize,
    pub bytes: Vec<u8>,
}
impl AudienceLink {
    pub fn new(token: String, ctx: egui::Context) -> Result<Self, String> {
        let window = web_sys::window().ok_or("No browser window")?;
        let opener = opener_window()?;
        let origin = origin()?;
        let deck: Rc<RefCell<Option<Deck>>> = Rc::new(RefCell::new(None));
        let slot = deck.clone();
        let (expected_opener, expected_origin, expected_token) = (opener.clone(), origin.clone(), token.clone());
        let listener = Closure::new(move |event: web_sys::MessageEvent| {
            if !same_source(&event, &expected_opener, &expected_origin) {
                return;
            }
            let data = event.data();
            let field = |key: &str| js_sys::Reflect::get(&data, &key.into()).ok();
            if !data.is_object()
                || field("kind").and_then(|v| v.as_string()).as_deref() != Some("deck")
                || field("token").and_then(|v| v.as_string()).as_deref() != Some(expected_token.as_str())
            {
                return;
            }
            let (Some(name), Some(from), Some(bytes)) =
                (field("name").and_then(|v| v.as_string()), field("from").and_then(|v| v.as_f64()), field("bytes"))
            else {
                return;
            };
            let bytes = js_sys::Uint8Array::new(&bytes);
            if bytes.length() as usize > MAX_DECK || !(0.0..1e6).contains(&from) {
                return;
            }
            *slot.borrow_mut() = Some(Deck { name: name.chars().take(80).collect(), from: from as usize, bytes: bytes.to_vec() });
            ctx.request_repaint();
        });
        window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref()).map_err(error)?;
        send(&opener, &origin, serde_json::json!({"kind":"hello","token":token}))?;
        Ok(Self { deck, listener })
    }

    /// The deck, once it has arrived.
    pub fn take(&self) -> Option<Deck> {
        self.deck.borrow_mut().take()
    }
}
impl Drop for AudienceLink {
    fn drop(&mut self) {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback("message", self.listener.as_ref().unchecked_ref());
        }
    }
}
impl Drop for Popup {
    fn drop(&mut self) {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback("message", self.listener.as_ref().unchecked_ref());
        }
    }
}
