//! craft-live mode: the web build as Nextcloud Office's live editor.
//!
//! craft-live loads this build as `index.html#craft-live=1&ws=…` inside its shell page
//! (craft-live's `docs/design.md` §9). [`deckcraft_engine::live::LiveClient`] does the work
//! without any I/O; this module carries out its effects with the browser: the room's
//! WebSocket (binary frames), `fetch` for the file, `prepare` and the upload, `postMessage` to
//! the shell, and the status bar or a toast for messages. Browser callbacks only queue events;
//! the app's frame handles them, so the session is only touched from the frame.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use deckcraft_engine::live::{Config, Effect, HttpRequest, HttpResponse, LiveClient};
use deckcraft_ui_egui::{Embed, SlideApp};
use serde_json::Value as Json;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;

/// Something the browser reported, tagged with the WebSocket it came from (`generation`), so a
/// socket that was replaced can't speak for the new one.
enum Event {
    Open(u64),
    Frame(u64, Vec<u8>),
    Close(u64, u16, String),
    Http(u64, HttpResponse),
    Shell(Json),
}

type Events = Rc<RefCell<VecDeque<Event>>>;
type Callback = Closure<dyn FnMut(JsValue)>;

/// Events kept while the frame doesn't run (a hidden tab): beyond this the oldest frames go,
/// and the reconnect's sync brings the room back.
const MAX_EVENTS: usize = 100_000;

/// The configuration in the page's fragment: `None` outside craft-live.
pub fn detect() -> Option<Result<Config, String>> {
    let hash = web_sys::window()?.location().hash().ok()?;
    Config::from_fragment(&hash)
}

pub struct LiveShell {
    client: LiveClient,
    /// The fragment's configuration, used at the first frame (or its error, reported then).
    pending: Option<Result<Config, String>>,
    events: Events,
    ws: Option<web_sys::WebSocket>,
    generation: u64,
    /// The current socket's callbacks (dropped with it).
    ws_callbacks: Vec<Callback>,
    /// The `message` listener for the shell's messages (kept for the page's life).
    _listener: Option<Callback>,
    ctx: egui::Context,
}

fn now_ms() -> u64 {
    js_sys::Date::now().max(0.0) as u64
}

fn push(events: &Events, ctx: &egui::Context, e: Event) {
    let mut q = events.borrow_mut();
    if q.len() >= MAX_EVENTS {
        q.pop_front();
    }
    q.push_back(e);
    ctx.request_repaint();
}

fn js_to_json(v: &JsValue) -> Option<Json> {
    let text: String = js_sys::JSON::stringify(v).ok()?.into();
    serde_json::from_str(&text).ok()
}

fn origin() -> String {
    web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default()
}

impl LiveShell {
    /// Starts listening to the shell and says hello to it (as soon as the app runs).
    pub fn new(config: Result<Config, String>, ctx: egui::Context) -> LiveShell {
        let events: Events = Rc::default();
        let listener = {
            let events = events.clone();
            let ctx = ctx.clone();
            let cb: Callback = Closure::new(move |e: JsValue| {
                let Ok(e) = e.dyn_into::<web_sys::MessageEvent>() else { return };
                // Only the shell around this frame, on this origin, speaks the embed contract.
                if e.origin() != origin() {
                    return;
                }
                let parent = web_sys::window().and_then(|w| w.parent().ok().flatten());
                let from_parent = match (e.source(), parent) {
                    (Some(src), Some(p)) => js_sys::Object::is(&src, p.as_ref()),
                    _ => false,
                };
                if !from_parent {
                    return;
                }
                if let Some(msg) = js_to_json(&e.data()) {
                    push(&events, &ctx, Event::Shell(msg));
                }
            });
            match web_sys::window() {
                Some(w) => {
                    if let Err(e) = w.add_event_listener_with_callback("message", cb.as_ref().unchecked_ref()) {
                        log::error!("craft-live: can't listen to the shell: {e:?}");
                    }
                    Some(cb)
                }
                None => None,
            }
        };
        let shell = LiveShell {
            client: LiveClient::new(),
            pending: Some(config),
            events,
            ws: None,
            generation: 0,
            ws_callbacks: Vec::new(),
            _listener: listener,
            ctx,
        };
        post(&serde_json::json!({"type": "craft-live:hello"}));
        shell
    }

    /// Prepares the app: Save and Close go to the shell; a placeholder shows while loading.
    pub fn prepare(app: &mut SlideApp) {
        app.embed = Some(Embed { host: "Nextcloud".into(), ..Default::default() });
        if let Err(e) = app.run("file.new", serde_json::json!({})) {
            log::error!("craft-live: placeholder: {e}");
        }
        app.set_status("Opening the presentation…");
    }

    /// Runs each frame, before the app's own logic.
    pub fn tick(&mut self, app: &mut SlideApp) {
        let now = now_ms();
        if let Some(cfg) = self.pending.take() {
            let effects = match cfg {
                Ok(cfg) => self.client.start(&mut app.session, cfg, now),
                Err(e) => vec![Effect::Alert(e.clone()), Effect::Post(serde_json::json!({"type": "craft-live:error", "message": e}))],
            };
            self.run(app, effects);
        }
        let events: Vec<Event> = self.events.borrow_mut().drain(..).collect();
        for e in events {
            let effects = match e {
                Event::Open(g) if g == self.generation => self.client.on_open(&mut app.session, now),
                Event::Frame(g, data) if g == self.generation => self.client.on_frame(&mut app.session, &data, now),
                Event::Close(g, code, reason) if g == self.generation => {
                    self.drop_socket();
                    self.client.on_close(&mut app.session, code, &reason, now)
                }
                Event::Http(id, r) => self.client.on_http(&mut app.session, id, r, now),
                Event::Shell(msg) => self.client.on_shell(&mut app.session, &msg, now),
                _ => Vec::new(),
            };
            self.run(app, effects);
        }
        if let Some(e) = app.embed.as_mut() {
            let (save, close) = (std::mem::take(&mut e.save_requested), std::mem::take(&mut e.close_requested));
            if save {
                let effects = self.client.save_now(false);
                self.run(app, effects);
            }
            if close {
                let effects = self.client.close(&mut app.session);
                self.run(app, effects);
            }
        }
        let effects = self.client.pump(&mut app.session, now);
        self.run(app, effects);
        // Reconnect timers and the room's messages need frames even when nothing is touched.
        self.ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    fn run(&mut self, app: &mut SlideApp, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Connect(url) => self.connect(&url),
                Effect::Send(data) => {
                    if let Some(ws) = &self.ws
                        && let Err(e) = ws.send_with_u8_array(&data)
                    {
                        log::warn!("craft-live: send failed: {e:?}");
                    }
                }
                Effect::Disconnect => {
                    if let Some(ws) = &self.ws {
                        let _ = ws.close_with_code(1000);
                    }
                    self.drop_socket();
                }
                Effect::Http(req) => self.fetch(req),
                Effect::Post(msg) => post(&msg),
                Effect::Status(text) => app.set_status(text),
                Effect::Alert(text) => {
                    app.set_status(text.clone());
                    app.notify(text);
                }
            }
        }
    }

    /// Forgets the current socket: its late events are ignored.
    fn drop_socket(&mut self) {
        if let Some(ws) = self.ws.take() {
            ws.set_onopen(None);
            ws.set_onmessage(None);
            ws.set_onclose(None);
            ws.set_onerror(None);
        }
        self.ws_callbacks.clear();
        self.generation = self.generation.wrapping_add(1);
    }

    fn connect(&mut self, url: &str) {
        if let Some(ws) = &self.ws {
            let _ = ws.close_with_code(1000);
        }
        self.drop_socket();
        let g = self.generation;
        let ws = match web_sys::WebSocket::new(url) {
            Ok(ws) => ws,
            Err(e) => {
                push(&self.events, &self.ctx, Event::Close(g, 1006, format!("{e:?}")));
                return;
            }
        };
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);
        let (events, ctx) = (self.events.clone(), self.ctx.clone());
        let on_open: Callback = Closure::new(move |_: JsValue| push(&events, &ctx, Event::Open(g)));
        let (events, ctx) = (self.events.clone(), self.ctx.clone());
        let on_message: Callback = Closure::new(move |e: JsValue| {
            let Ok(e) = e.dyn_into::<web_sys::MessageEvent>() else { return };
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                push(&events, &ctx, Event::Frame(g, bytes));
            }
        });
        let (events, ctx) = (self.events.clone(), self.ctx.clone());
        let on_close: Callback = Closure::new(move |e: JsValue| {
            let (code, reason) = match e.dyn_into::<web_sys::CloseEvent>() {
                Ok(c) => (c.code(), c.reason()),
                Err(_) => (1006, String::new()),
            };
            push(&events, &ctx, Event::Close(g, code, reason));
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        self.ws_callbacks = vec![on_open, on_message, on_close];
        self.ws = Some(ws);
    }

    fn fetch(&self, req: HttpRequest) {
        let (events, ctx) = (self.events.clone(), self.ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let id = req.id;
            let r = fetch(req).await.unwrap_or_else(|e| HttpResponse::failed(&e));
            push(&events, &ctx, Event::Http(id, r));
        });
    }
}

async fn fetch(req: HttpRequest) -> Result<HttpResponse, String> {
    let js = |e: JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let init = web_sys::RequestInit::new();
    init.set_method(req.method);
    let headers = web_sys::Headers::new().map_err(js)?;
    for (k, v) in &req.headers {
        headers.set(k, v).map_err(js)?;
    }
    init.set_headers(&headers);
    if let Some(body) = &req.body {
        let arr = js_sys::Uint8Array::from(body.as_slice());
        init.set_body(&arr);
    }
    let request = web_sys::Request::new_with_str_and_init(&req.url, &init).map_err(js)?;
    let resp =
        wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&request)).await.map_err(|e| format!("the network failed ({})", js(e)))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "not a response")?;
    let status = resp.status();
    let buf = wasm_bindgen_futures::JsFuture::from(resp.array_buffer().map_err(js)?).await.map_err(js)?;
    let body = js_sys::Uint8Array::new(&buf).to_vec();
    Ok(HttpResponse { status, body })
}

/// A message to the shell: `window.parent.postMessage(msg, location.origin)`.
fn post(msg: &Json) {
    let Some(window) = web_sys::window() else { return };
    let Ok(Some(parent)) = window.parent() else { return };
    let Ok(value) = js_sys::JSON::parse(&msg.to_string()) else { return };
    if let Err(e) = parent.post_message(&value, &origin()) {
        log::warn!("craft-live: postMessage failed: {e:?}");
    }
}
