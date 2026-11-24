//! The browser shell: web `Services`, drag-and-drop, and the eframe web runner.

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Inbox, InboxEvent, InboxFile, Services, SlideApp};
use wasm_bindgen::JsCast as _;

pub(crate) mod settings;

const PRESENTATION_EXTS: &[&str] = &["deckcraft", "pptx", "potx", "ppsx", "ppt", "pot", "pps", "txt", "md"];
const PICTURE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];
const AUDIO_EXTS: &[&str] = &["wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "aif", "aiff"];
const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "mov", "webm"];
const CANVAS_ID: &str = "deckcraft_canvas";
const LOADING_ID: &str = "deckcraft_loading";

/// Start the browser shell: install logging, locate the canvas, launch the eframe runner,
/// build the sample or blank deck, and replace the loading element when launch finishes.
pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"{CANVAS_ID}\">");
            return;
        };
        // Host modes (`craft-web-host`): `?host=parent`, a same-origin page framing DeckCraft,
        // opens presentations in it and stores what it saves (`host`); `data-host="page"` on the
        // canvas opts a page in that runs DeckCraft in its own document. The presenter popup is
        // never the framed page.
        let hosted = crate::presenter::token().is_none();
        let bridge = if hosted { crate::host::Bridge::detect(&canvas) } else { None };
        if hosted && bridge.is_none() && query_param("host").as_deref() == Some("parent") {
            log::warn!("?host=parent needs a framing page on the same origin; saves download instead");
        }
        let app_bridge = bridge.clone();
        let mut options = eframe::WebOptions::default();
        if query_has("webgl")
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
        {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(move |cc| {
                    if let Some(rs) = &cc.wgpu_render_state {
                        log::info!("deckcraft-web {}: wgpu backend {:?}", env!("CARGO_PKG_VERSION"), rs.adapter.get_info().backend);
                    }
                    deckcraft_ui_egui::install_ime_pass_hooks(&cc.egui_ctx);
                    if let Some(token) = crate::presenter::token() {
                        let presenter = crate::presenter::Popup::new(token, cc.egui_ctx.clone()).map_err(std::io::Error::other)?;
                        return Ok(Box::new(presenter) as Box<dyn eframe::App>);
                    }
                    let inbox: Inbox = Inbox::default();
                    // A window the presenter tab opened to show the presentation (see `Windowed`).
                    let audience_token = crate::presenter::audience_token();
                    let mut services = services(inbox.clone(), cc.egui_ctx.clone(), audience_token.clone());
                    if let Some(bridge) = &app_bridge {
                        bridge.connect(&mut services, cc.egui_ctx.clone());
                    }
                    let mut app = SlideApp::new(Session::new(), services);
                    let mut settings = settings::Settings::load(&mut app);
                    crate::school::apply(&mut app);
                    settings.save_now(&app);
                    let exam_ready = std::rc::Rc::new(std::cell::Cell::new(false));
                    if crate::school::exam_library() {
                        load_exam_library(exam_ready.clone(), cc.egui_ctx.clone());
                    }
                    let live_bank = std::rc::Rc::new(std::cell::RefCell::new(None));
                    if crate::school::private() {
                        load_live_bank(live_bank.clone(), cc.egui_ctx.clone());
                    }
                    let q = query();
                    // A teacher opens the app to make or open a deck, so it starts on the Home
                    // screen with an empty presentation behind it; `?sample` loads the demo deck.
                    // In host mode the parent sends the presentation to edit; no sample meanwhile.
                    if app_bridge.is_some() {
                        if let Err(e) = app.start_for_host() {
                            log::error!("new presentation: {e}");
                        }
                    } else if q.contains("sample") {
                        if let Err(e) = deckcraft_engine::sample::open_sample(&mut app.session) {
                            log::error!("sample deck: {e}");
                        }
                    } else if let Err(e) = app.run("file.new", serde_json::json!({})) {
                        log::error!("new presentation: {e}");
                    } else if audience_token.is_none() && !q.contains("show") {
                        let _ = app.run("app.home", serde_json::json!({"on": true}));
                    }
                    if query_has("show") {
                        app.start_show(0, false);
                    }
                    // Start-up check of the presenter tab and its presentation window.
                    if q.contains("presenttest") && audience_token.is_none() {
                        app.ui.presenter_view = true;
                        app.start_show(0, false);
                    }
                    let smoke = q.contains("smoke").then(crate::smoke::Smoke::default);
                    let audience = match audience_token {
                        Some(token) => match crate::presenter::AudienceLink::new(token, cc.egui_ctx.clone()) {
                            Ok(link) => Some(link),
                            Err(e) => {
                                log::error!("presentation window: {e}");
                                None
                            }
                        },
                        None => None,
                    };
                    if app_bridge.is_some()
                        && let Some(author) = query_param("author")
                    {
                        crate::host::Bridge::set_author(&mut app, &author);
                    }
                    crate::api::install(&mut app, cc.egui_ctx.clone());
                    Ok(Box::new(WebShell {
                        app,
                        inbox,
                        settings,
                        exam_ready,
                        live_bank,
                        audience,
                        remote: None,
                        connected_logged: false,
                        pending: None,
                        smoke,
                        bridge: app_bridge,
                    }))
                }),
            )
            .await;
        // Ready only once the runner is up: starting can still fail after the app is made.
        if let Some(bridge) = &bridge {
            match &result {
                Ok(()) => bridge.post_ready(),
                Err(e) => bridge.post_failed(&format!("{e:?}")),
            }
        }
        if let Some(el) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => el.remove(),
                Err(e) => el.set_inner_html(&format!("<p>DeckCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>")),
            }
        }
    });
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

fn query_param(key: &str) -> Option<String> {
    web_sys::UrlSearchParams::new_with_str(&query()).ok()?.get(key)
}

/// Whether the URL has the flag `key` (`?show`, `&blank=1`). A whole parameter, never a substring:
/// `?author=` is free text.
fn query_has(key: &str) -> bool {
    web_sys::UrlSearchParams::new_with_str(&query()).is_ok_and(|q| q.has(key))
}

/// Wraps the app to read dropped files asynchronously (browsers can't read them synchronously)
/// and feed them through the inbox.
struct WebShell {
    app: SlideApp,
    inbox: Inbox,
    /// Saved to local storage as they change.
    settings: settings::Settings,
    /// The published exam question library has arrived: point the Question Library at it.
    exam_ready: std::rc::Rc<std::cell::Cell<bool>>,
    /// The live school question bank has arrived (`load_live_bank`): apply it.
    live_bank: std::rc::Rc<std::cell::RefCell<Option<serde_json::Value>>>,
    /// This window only shows the presentation: the presenter tab that opened it sends the deck.
    audience: Option<crate::presenter::AudienceLink>,
    /// This tab is the presenter view while the presentation plays in the window it opened.
    remote: Option<crate::presenter::Popup>,
    /// "Presenter view connected" has been logged for this show (the start-up check looks for it).
    connected_logged: bool,
    /// The deck for that window, until it asks for it: the slide to start on and the file.
    pending: Option<(usize, Vec<u8>)>,
    /// `?smoke`: the start-up check a headless browser runs (`smoke`).
    smoke: Option<crate::smoke::Smoke>,
    /// The parent page that keeps the files (`?host=parent`).
    bridge: Option<crate::host::Bridge>,
}

impl eframe::App for WebShell {
    /// Delegate one eframe logic pass to the wrapped application, opening the files the parent
    /// page sent first and telling it about unsaved changes after.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.exam_ready.take() {
            self.app.ui.maths.exam_library_dir = crate::school::EXAM_DIR.to_string();
        }
        if let Some(live) = self.live_bank.borrow_mut().take() {
            crate::school::apply_live(&mut self.app, &live);
        }
        if let Some(bridge) = &self.bridge {
            bridge.open_files(&mut self.app);
        }
        self.windowed(ctx);
        fetch_wanted(ctx);
        if let Some(s) = self.smoke.as_mut() {
            s.tick(&mut self.app, ctx);
        }
        if self.remote.is_none() {
            self.app.logic(ctx);
        }
        // (The presentation window closes itself when its show ends, and must wait for the deck.)
        if let Some(bridge) = &self.bridge {
            bridge.report_dirty(&self.app);
        }
        if self.app.show.is_none()
            && self.audience.is_none()
            && let Some(host) = self.app.services.presenter.as_mut()
        {
            host.close();
        }
        self.settings.save(&self.app, ctx);
    }

    /// Consume browser drops with fresh input, capture the identity before the asynchronous read,
    /// and deliver the id, name, and bytes through the inbox.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Browsers cannot read dropped files synchronously. Consume this pass's drops here, with
        // fresh input, and capture the document/slide before the asynchronous read finishes.
        for f in std::mem::take(&mut raw.dropped_files) {
            let id = self.app.begin_external_file("drop");
            let inbox = self.inbox.clone();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let name = f.path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped".into());
                match f.bytes_async().await {
                    Ok(bytes) => {
                        push_inbox(&inbox, InboxEvent::File(InboxFile { id, name, bytes }));
                        ctx.request_repaint();
                    }
                    Err(e) => {
                        log::error!("couldn't read dropped file {name}: {e}");
                        push_inbox(&inbox, InboxEvent::Cancelled { id });
                        ctx.request_repaint();
                    }
                }
            });
        }
        self.app.raw_input_hook(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match self.remote.as_mut() {
            Some(presenter) => presenter.show(ui),
            None => self.app.ui(ui),
        }
    }
}

impl WebShell {
    /// Presenter View on: the tab the teacher is working in becomes the presenter view (notes,
    /// current and next slide, timer) and the presentation plays in a second window to drag onto
    /// the projector. That window is this same app, started with the deck and told to play it.
    /// If the browser blocks the window, the show plays in this tab as it does without Presenter View.
    fn windowed(&mut self, ctx: &egui::Context) {
        if let Some(link) = &self.audience
            && let Some(deck) = link.take()
        {
            let data = deckcraft_engine::cmd::base64_encode(&deck.bytes);
            match self.app.run("file.openBytes", serde_json::json!({"name": deck.name, "data": data})) {
                Ok(_) => {
                    self.app.ui.presenter_view = true;
                    self.app.start_show(deck.from, false);
                    log::info!("presentation window: show started at slide {}", deck.from + 1);
                }
                Err(e) => log::error!("presentation window: the deck could not be opened: {e}"),
            }
        }
        if self.audience.is_none()
            && self.remote.is_none()
            && let Some((from, bytes)) = self.app.take_show_for_window()
        {
            log::info!("presenter view: opening the presentation window");
            match crate::presenter::Popup::open_audience(ctx.clone()) {
                Ok(presenter) => {
                    self.remote = Some(presenter);
                    self.pending = Some((from, bytes));
                }
                Err(e) => {
                    log::warn!("presenter view: the presentation window could not be opened: {e}");
                    self.app.ui.presenter_view = false;
                    self.app.start_show(from, false);
                    self.app.set_status(format!("{e} The show plays in this tab; allow popups, then turn Presenter View on again."));
                }
            }
        }
        let Some(presenter) = self.remote.as_mut() else {
            self.connected_logged = false;
            return;
        };
        if presenter.connected() && !self.connected_logged {
            self.connected_logged = true;
            log::info!("presenter view: connected to the presentation window");
        }
        if presenter.take_hello()
            && let Some((from, bytes)) = self.pending.take()
            && let Err(e) = presenter.start("show.deckcraft", from, &bytes)
        {
            log::error!("presentation window: {e}");
            self.app.set_status("The presentation window could not be started.");
            presenter.end();
        }
        // The window never answered, or the show is over: back to the editor.
        let silent = self.pending.is_some() && presenter.waited() > 30.0;
        if presenter.ended() || silent {
            let slide = presenter.last_slide();
            if silent {
                self.app.set_status("The presentation window did not open.");
            }
            self.remote = None;
            self.pending = None;
            let _ = self.app.run("slide.go", serde_json::json!({"index": slide}));
            ctx.request_repaint();
        }
    }
}

/// Web services: no file paths, so `pick_open`/`pick_save`/`read` stay `None` and the UI falls
/// back to `open_async` (browser picker → inbox) and `download`.
fn services(inbox: Inbox, ctx: egui::Context, audience_token: Option<String>) -> Services {
    let open_inbox = inbox.clone();
    let host = match audience_token {
        Some(token) => crate::presenter::BrowserHost::for_opener(ctx.clone(), token),
        None => crate::presenter::BrowserHost::new(ctx.clone()),
    };
    let presenter = match host {
        Ok(host) => Some(Box::new(host) as Box<dyn deckcraft_ui_egui::presenter::Host>),
        Err(error) => {
            log::error!("Presenter window: {error}");
            None
        }
    };
    Services {
        presenter,
        open_async: Some(Box::new(move |purpose: &str, request: u64| {
            let inbox = open_inbox.clone();
            let ctx = ctx.clone();
            let dialog = match purpose {
                "picture" => rfd::AsyncFileDialog::new().add_filter("Pictures", PICTURE_EXTS),
                "audio" => rfd::AsyncFileDialog::new().add_filter("Audio", AUDIO_EXTS),
                "video" => rfd::AsyncFileDialog::new().add_filter("Video", VIDEO_EXTS),
                "curriculum" => rfd::AsyncFileDialog::new().add_filter("Curriculum file", &["json"]),
                "questions" => rfd::AsyncFileDialog::new().add_filter("Question file", &["json", "txt"]),
                _ => rfd::AsyncFileDialog::new().add_filter("Presentations", PRESENTATION_EXTS),
            };
            wasm_bindgen_futures::spawn_local(async move {
                let Some(file) = dialog.pick_file().await else {
                    push_inbox(&inbox, InboxEvent::Cancelled { id: request });
                    ctx.request_repaint();
                    return;
                };
                let bytes = file.read().await;
                push_inbox(&inbox, InboxEvent::File(InboxFile { id: request, name: file.file_name(), bytes }));
                ctx.request_repaint();
            });
        })),
        write: Some(Box::new(|name: &str, bytes: &[u8]| download(name, bytes))),
        download: Some(Box::new(|name: &str, bytes: &[u8]| {
            if let Err(e) = download(name, bytes) {
                log::error!("download of {name} failed: {e}");
            }
        })),
        inbox: Some(inbox),
        ..Default::default()
    }
}

/// The exam question library beside a private site: its questions now, each picture when it is
/// first shown (they are too many to download at once). Only counts are logged, never contents.
fn load_exam_library(ready: std::rc::Rc<std::cell::Cell<bool>>, ctx: egui::Context) {
    use deckcraft_engine::web_files;
    wasm_bindgen_futures::spawn_local(async move {
        let dir = crate::school::EXAM_DIR;
        let items = format!("{dir}/items/exam/library.json");
        let crops = format!("{dir}/derived/exam_crops");
        let names =
            fetch_bytes(&format!("{dir}/crops.json")).await.and_then(|b| serde_json::from_slice::<Vec<String>>(&b).map_err(|e| e.to_string()));
        match (fetch_bytes(&items).await, names) {
            (Ok(bytes), Ok(names)) => {
                log::info!("exam question library: {} pictures", names.len());
                web_files::provide(&items, bytes);
                web_files::list(&crops, names);
                web_files::fetch_on_demand(&crops);
                ready.set(true);
                ctx.request_repaint();
            }
            (Err(e), _) | (_, Err(e)) => log::error!("the exam question library could not be loaded: {e}"),
        }
    });
}

/// The school's live question bank (`bank/api/live.json` on a private site, edited by the staff
/// question-bank page). Absent on the public site, which is fine: nothing is logged as an error.
fn load_live_bank(slot: std::rc::Rc<std::cell::RefCell<Option<serde_json::Value>>>, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        match fetch_bytes("bank/api/live.json").await.and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| e.to_string())) {
            Ok(v) => {
                *slot.borrow_mut() = Some(v);
                ctx.request_repaint();
            }
            Err(e) => log::info!("no live school question bank: {e}"),
        }
    });
}

/// Fetch the files something asked the browser's file store for (`deckcraft_engine::web_files`).
fn fetch_wanted(ctx: &egui::Context) {
    use deckcraft_engine::web_files;
    for path in web_files::take_wanted() {
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match fetch_bytes(&path).await {
                Ok(bytes) => web_files::provide(&path, bytes),
                Err(e) => {
                    log::warn!("{e}");
                    web_files::failed(&path);
                }
            }
            ctx.request_repaint();
        });
    }
}

/// The bytes at `url`, relative to the page (so a private site's password goes with the request).
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    use wasm_bindgen_futures::JsFuture;
    let js = |e: wasm_bindgen::JsValue| format!("{url}: {e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let resp: web_sys::Response =
        JsFuture::from(window.fetch_with_str(url)).await.map_err(js)?.dyn_into().map_err(|_| format!("{url}: not a response"))?;
    if !resp.ok() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(js)?).await.map_err(js)?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

fn push_inbox(inbox: &Inbox, event: InboxEvent) {
    inbox.lock().unwrap_or_else(|e| e.into_inner()).push(event);
}

/// Trigger a browser download of `bytes` named after the last component of `path`.
fn download(path: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "deckcraft".into());
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime_for(&name));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let a: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.dyn_into().map_err(|_| "not an anchor")?;
    a.set_href(&url);
    a.set_download(&name);
    a.style().set_property("display", "none").map_err(js)?;
    let body = document.body().ok_or("no body")?;
    body.append_child(&a).map_err(js)?;
    a.click();
    a.remove();
    // Revoke after the click has been dispatched; the download keeps its own reference.
    let revoke = wasm_bindgen::closure::Closure::once_into_js(move || {
        web_sys::Url::revoke_object_url(&url).ok();
    });
    window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000).map_err(js)?;
    Ok(())
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("pdf") => "application/pdf",
        Some("gif") => "image/gif",
        Some("txt") => "text/plain",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("deckcraft") => "application/zip",
        _ => "application/octet-stream",
    }
}
