//! The browser shell: web `Services`, drag-and-drop, and the eframe web runner.

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Inbox, InboxEvent, InboxFile, Services, SlideApp};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;

const PRESENTATION_EXTS: &[&str] = &["deckcraft", "pptx", "potx", "ppsx", "txt", "md"];
const PICTURE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];
const AUDIO_EXTS: &[&str] = &["wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "aif", "aiff"];
const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "mov", "webm"];
const CANVAS_ID: &str = "deckcraft_canvas";
const LOADING_ID: &str = "deckcraft_loading";

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
        let mut options = eframe::WebOptions::default();
        if query().contains("webgl")
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
                    let inbox: Inbox = Inbox::default();
                    let mut app = SlideApp::new(Session::new(), services(inbox.clone(), cc.egui_ctx.clone()));
                    let q = query();
                    if !q.contains("blank") {
                        if let Err(e) = deckcraft_engine::sample::open_sample(&mut app.session) {
                            log::error!("sample deck: {e}");
                        }
                    } else if let Err(e) = app.run("file.new", serde_json::json!({})) {
                        log::error!("new presentation: {e}");
                    }
                    if q.contains("show") {
                        app.start_show(0, false);
                    }
                    Ok(Box::new(WebShell { app, inbox }))
                }),
            )
            .await;
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

/// Wraps the app to read dropped files asynchronously (browsers can't read them synchronously)
/// and feed them through the inbox.
struct WebShell {
    app: SlideApp,
    inbox: Inbox,
}

impl eframe::App for WebShell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        for f in dropped {
            // Capture the document and slide now. The bytes arrive later, after the user may
            // have switched slides.
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
        self.app.logic(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}

static JS_FONT_ARRAY: std::sync::OnceLock<js_sys::Array> = std::sync::OnceLock::new();
static FETCHING_FAMILIES: std::sync::OnceLock<std::sync::RwLock<std::collections::HashSet<String>>> = std::sync::OnceLock::new();

fn fetching_set() -> &'static std::sync::RwLock<std::collections::HashSet<String>> {
    FETCHING_FAMILIES.get_or_init(|| std::sync::RwLock::new(std::collections::HashSet::new()))
}

fn fetch_family_blobs(family: &str, ctx: &egui::Context) {
    let Some(arr) = JS_FONT_ARRAY.get() else { return };
    let family_lower = family.to_ascii_lowercase();

    // Avoid duplicate concurrent fetches for the same family:
    {
        let mut set = fetching_set().write().unwrap_or_else(|e| e.into_inner());
        if set.contains(&family_lower) || deckcraft_fonts::FontDb::global().is_loaded(family) {
            return;
        }
        set.insert(family_lower);
    }

    let arr_len = arr.length();
    for i in 0..arr_len {
        let item = arr.get(i);
        let item_family = js_sys::Reflect::get(&item, &JsValue::from_str("family")).ok().and_then(|v| v.as_string()).unwrap_or_default();
        if !item_family.eq_ignore_ascii_case(family) {
            continue;
        }
        let blob_fn: Option<js_sys::Function> = js_sys::Reflect::get(&item, &JsValue::from_str("blob")).ok().and_then(|v| v.dyn_into().ok());
        let Some(blob_fn) = blob_fn else { continue };
        let Ok(blob_prom) = blob_fn.call0(&item).and_then(|v| v.dyn_into::<js_sys::Promise>()) else { continue };

        let ctx = ctx.clone();
        let family_for_log = family.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            let Ok(blob_val) = wasm_bindgen_futures::JsFuture::from(blob_prom).await else { return };
            let Ok(blob) = blob_val.dyn_into::<web_sys::Blob>() else { return };
            let Ok(buf) = wasm_bindgen_futures::JsFuture::from(blob.array_buffer()).await else { return };
            let bytes = js_sys::Uint8Array::new(&buf).to_vec();
            let added = deckcraft_fonts::FontDb::global().add_font(bytes);
            if added > 0 {
                log::info!("Loaded system font '{family_for_log}' ({added} face(s))");
                ctx.request_repaint();
            }
        });
    }
}

fn setup_system_fonts(ctx: egui::Context) -> (Option<Box<dyn Fn()>>, Option<Box<dyn Fn(&str)>>) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return (None, None),
    };
    let query_prop = js_sys::Reflect::get(&window, &JsValue::from_str("queryLocalFonts")).ok();
    if !query_prop.as_ref().is_some_and(|p| p.is_function()) {
        return (None, None);
    }

    let fetch_ctx = ctx.clone();
    let fetch_action: Box<dyn Fn(&str)> = Box::new(move |family: &str| {
        fetch_family_blobs(family, &fetch_ctx);
    });

    let load_action: Box<dyn Fn()> = Box::new(move || {
        let Some(window) = web_sys::window() else { return };
        let Ok(query_val) = js_sys::Reflect::get(&window, &JsValue::from_str("queryLocalFonts")) else { return };
        let Ok(query_fn) = query_val.dyn_into::<js_sys::Function>() else { return };
        let Ok(prom_val) = query_fn.call0(&window) else { return };
        let Ok(prom) = prom_val.dyn_into::<js_sys::Promise>() else { return };

        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Ok(result) = wasm_bindgen_futures::JsFuture::from(prom).await else {
                log::info!("queryLocalFonts was not granted");
                return;
            };
            let Ok(arr) = result.dyn_into::<js_sys::Array>() else { return };
            log::info!("queryLocalFonts granted with {} fonts", arr.length());

            let mut families: Vec<String> = Vec::with_capacity(arr.length() as usize);
            for i in 0..arr.length() {
                let item = arr.get(i);
                let family = js_sys::Reflect::get(&item, &JsValue::from_str("family")).ok().and_then(|v| v.as_string()).unwrap_or_default();
                if !family.is_empty() && !families.iter().any(|f| f.eq_ignore_ascii_case(&family)) {
                    families.push(family);
                }
            }
            let _ = JS_FONT_ARRAY.set(arr);
            deckcraft_fonts::FontDb::global().register_web_families(families);
            ctx.request_repaint();
        });
    });

    (Some(load_action), Some(fetch_action))
}

/// Web services: no file paths, so `pick_open`/`pick_save`/`read` stay `None` and the UI falls
/// back to `open_async` (browser picker → inbox) and `download`.
fn services(inbox: Inbox, ctx: egui::Context) -> Services {
    let open_inbox = inbox.clone();
    let (load_system_fonts, fetch_font) = setup_system_fonts(ctx.clone());
    Services {
        open_async: Some(Box::new(move |purpose: &str, request: u64| {
            let inbox = open_inbox.clone();
            let ctx = ctx.clone();
            let dialog = match purpose {
                "picture" => rfd::AsyncFileDialog::new().add_filter("Pictures", PICTURE_EXTS),
                "audio" => rfd::AsyncFileDialog::new().add_filter("Audio", AUDIO_EXTS),
                "video" => rfd::AsyncFileDialog::new().add_filter("Video", VIDEO_EXTS),
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
        load_system_fonts,
        fetch_font,
        ..Default::default()
    }
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
