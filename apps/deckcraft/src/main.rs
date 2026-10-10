//! DeckCraft desktop app.
//!
//! Usage: `deckcraft [--control <port>] [--sample] [--show] [files…]`
//!
//! `--control <port>` (or `DECKCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `deckcraft_ui_egui::control` for the methods.
//!
//! `log` records go to standard error and `<settings dir>/logs/deckcraft.log` (`RUST_LOG` sets the
//! levels); see [`logging`].
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(target_os = "macos")]
mod apple_events;
mod audio;
mod control_server;
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod logging;

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Services, SlideApp};

struct App(SlideApp, #[cfg(target_os = "macos")] fmv_macos_events::Inbox);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        apple_events::poll(&self.1, &mut self.0, ctx);
        self.0.logic(ctx);
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
    fn on_exit(&mut self) {
        save_prefs(&self.0);
    }
}

/// The per-user settings directory: `ui.json`, `prefs.json` and `logs/` live here.
fn config_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support/DeckCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| std::path::PathBuf::from(a).join("DeckCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
            .map(|c| c.join("deckcraft"))
    }
}

fn prefs_path() -> Option<std::path::PathBuf> {
    config_dir().map(|b| b.join("ui.json"))
}

fn load_prefs(app: &mut SlideApp) {
    if std::env::var_os("DECKCRAFT_NO_PREFS").is_some() {
        return;
    }
    if let Some(p) = prefs_path()
        && let Ok(bytes) = std::fs::read(&p)
        && let Ok(ui) = serde_json::from_slice::<deckcraft_ui_egui::UiState>(&bytes)
    {
        app.ui = ui;
    }
    if let Some(p) = prefs_path().map(|p| p.with_file_name("prefs.json"))
        && let Ok(bytes) = std::fs::read(&p)
        && let Ok(prefs) = serde_json::from_slice::<deckcraft_engine::Prefs>(&bytes)
    {
        app.session.prefs = prefs;
    }
}

fn save_prefs(app: &SlideApp) {
    if std::env::var_os("DECKCRAFT_NO_PREFS").is_some() {
        return;
    }
    if let Some(p) = prefs_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(std::path::Path::new(".")));
        if let Ok(bytes) = serde_json::to_vec_pretty(&app.ui) {
            let _ = std::fs::write(&p, bytes);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&app.session.prefs) {
            let _ = std::fs::write(p.with_file_name("prefs.json"), bytes);
        }
    }
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|purpose: &str| {
            let d = rfd::FileDialog::new();
            let d = match purpose {
                "picture" => d.add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"]),
                "audio" => d.add_filter(
                    "Audio",
                    &["wav", "mp3", "m4a", "m4b", "aac", "flac", "ogg", "oga", "opus", "aif", "aiff", "aifc", "caf", "wma", "weba", "mka"],
                ),
                "video" => d.add_filter("Video", &["mp4", "m4v", "mov", "webm", "mkv", "wmv"]),
                _ => d
                    .add_filter("Presentations", &["deckcraft", "pptx", "potx", "ppsx"])
                    .add_filter("DeckCraft", &["deckcraft"])
                    .add_filter("PowerPoint", &["pptx", "potx", "ppsx"])
                    .add_filter("Outline", &["txt", "md"]),
            };
            d.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| rfd::FileDialog::new().set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string()))),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        clipboard_image: Some(Box::new(|| {
            let mut cb = arboard::Clipboard::new().ok()?;
            let img = cb.get_image().ok()?;
            let rgba = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())?;
            let mut out = Vec::new();
            rgba.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
            Some(out)
        })),
        // Media is mixed and clocked by the output device; `DECKCRAFT_NO_AUDIO` plays silently.
        audio_out: std::env::var_os("DECKCRAFT_NO_AUDIO")
            .is_none()
            .then(|| Box::new(audio::CpalOut::default()) as Box<dyn deckcraft_media::AudioOut>),
        ..Default::default()
    }
}

const APP_ID: &str = "ai.storyteller.deckcraft";

fn app_icon() -> Option<egui::IconData> {
    let png: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.deckcraft.png");
    eframe::icon_data::from_png_bytes(png).map_err(|e| log::warn!("app icon: {e}")).ok()
}

fn main() -> eframe::Result {
    // First, so every start-up record (and the engine's panic hook, installed with the first
    // Session) is captured; see `logging`.
    let logger = logging::install();
    let mut control_port: Option<u16> = std::env::var("DECKCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut files = Vec::new();
    let mut sample = false;
    let mut show = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = args.next().and_then(|p| p.parse().ok()),
            "--sample" => sample = true,
            "--show" => show = true,
            "--version" => {
                println!("deckcraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => files.push(a),
        }
    }
    log::info!("DeckCraft {} ({} {})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let Some(logger) = logger {
        match config_dir().map(|d| logger.attach_dir(&d.join(logging::LOG_DIR))) {
            Some(Ok(path)) => log::info!("log file {}", path.display()),
            Some(Err(e)) => log::warn!("no log file: {e}"),
            None => {
                logger.stderr_only();
                log::warn!("no log file: no settings directory (HOME, XDG_CONFIG_HOME or APPDATA is not set)");
            }
        }
    }
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DeckCraft")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    options.viewport = options.viewport.with_app_id(APP_ID);
    if let Some(icon) = app_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("DECKCRAFT_WAYLAND").is_none() {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    // Before eframe creates the wgpu instance: default Windows to DirectX 12 only (see graphics.rs).
    #[cfg(target_os = "windows")]
    graphics::configure(&mut options, eframe::wgpu::Backends::from_env());
    // Registered before the event loop starts, so it catches the Finder event that launched us as
    // well as later ones. Lives until the event loop returns; the app creator only borrows it.
    #[cfg(target_os = "macos")]
    let apple_events = apple_events::AppleEvents::install();
    #[cfg(target_os = "macos")]
    let apple_events = &apple_events;
    eframe::run_native(
        "DeckCraft",
        options,
        Box::new(move |cc| {
            let mut app = SlideApp::new(Session::new(), services());
            load_prefs(&mut app);
            // AutoRecover: reopen what a previous run left unsaved, then keep it current.
            if std::env::var_os("DECKCRAFT_NO_RECOVERY").is_none() {
                app.session.recovery_dir = deckcraft_engine::recovery::default_dir();
                if let Ok(v) = app.session.execute("file.recovery.open", &serde_json::json!({}))
                    && let Some(n) = v.get("opened").and_then(|o| o.as_array()).map(Vec::len).filter(|n| *n > 0)
                {
                    app.set_status(format!("Recovered {n} unsaved presentation{} from the last session", if n == 1 { "" } else { "s" }));
                }
            }
            app.integrated_titlebar = cfg!(target_os = "macos");
            if let Some(port) = control_port {
                let rx = control_server::start(port, cc.egui_ctx.clone());
                app = app.with_control(rx);
            }
            if sample && let Err(e) = deckcraft_engine::sample::open_sample(&mut app.session) {
                log::error!("sample: {e}");
            }
            for f in files {
                if let Err(e) = app.open_path(&f) {
                    log::error!("{f}: {e}");
                }
            }
            if show {
                app.start_show(0, false);
            }
            Ok(Box::new(App(
                app,
                #[cfg(target_os = "macos")]
                apple_events.connect(&cc.egui_ctx),
            )))
        }),
    )
}
