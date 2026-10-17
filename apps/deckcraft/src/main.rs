//! DeckCraft desktop app.
//!
//! Usage: `deckcraft [--control <port>] [--sample] [--show] [files…]`
//! `deckcraft --help` prints the flags and exits. `deckcraft --version` prints the version and exits.
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

mod audio;
mod control_server;
#[cfg(all(unix, not(target_os = "macos")))]
mod display;
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod logging;
#[cfg(not(target_arch = "wasm32"))]
mod pasteboard;

use deckcraft_engine::Session;
use deckcraft_ui_egui::{Services, SlideApp};

struct App(SlideApp);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
        // macOS (#279): end the process here, before eframe tears the window down. AccessKit's
        // macOS adapter subclasses the window's content view and restores its class on teardown;
        // AppKit's Touch Bar support observes that view (KVO), so in the next display cycle
        // `-[_NSTouchBarFinderObservation invalidate]` throws "not registered" and the quit ends
        // in a crash report. eframe calls `on_exit` on every way of quitting (⌘Q, the close
        // button, `app.quit`), once the unsaved-changes prompt has let the window close, and
        // nothing after it is needed: the preferences are saved and the log file is unbuffered.
        // `on_exit` only runs once the app exists, never on a failed start. Same as GridCraft #117.
        #[cfg(target_os = "macos")]
        std::process::exit(0);
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

fn load_prefs(app: &mut SlideApp) {
    if std::env::var_os("DECKCRAFT_NO_PREFS").is_some() {
        return;
    }
    let Some(dir) = config_dir() else { return };
    let (ui, prefs) = deckcraft_ui_egui::settings::load(&dir);
    if let Some(ui) = ui {
        app.ui = ui;
    }
    if let Some(prefs) = prefs {
        app.session.prefs = prefs;
    }
}

fn save_prefs(app: &SlideApp) {
    if std::env::var_os("DECKCRAFT_NO_PREFS").is_some() {
        return;
    }
    if let Some(dir) = config_dir() {
        deckcraft_ui_egui::settings::save(&dir, &app.ui, &app.session.prefs);
    }
}

fn services() -> Services {
    Services {
        system_locale: sys_locale::get_locale(),
        pick_open: Some(Box::new(|purpose: &str| {
            let d = rfd::FileDialog::new();
            let d = match purpose {
                "picture" => d.add_filter(deckcraft_ui_egui::i18n::tr("Pictures"), &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"]),
                "audio" => d.add_filter(
                    deckcraft_ui_egui::i18n::tr("Audio"),
                    &["wav", "mp3", "m4a", "m4b", "aac", "flac", "ogg", "oga", "opus", "aif", "aiff", "aifc", "caf", "wma", "weba", "mka"],
                ),
                "video" => d.add_filter(deckcraft_ui_egui::i18n::tr("Video"), &["mp4", "m4v", "mov", "webm", "mkv", "wmv"]),
                "theme" => d
                    .add_filter(deckcraft_ui_egui::i18n::tr("Themes and presentations"), &["thmx", "pptx", "potx", "ppsx"])
                    .add_filter(deckcraft_ui_egui::i18n::tr("Office Theme"), &["thmx"]),
                _ => d
                    .add_filter(deckcraft_ui_egui::i18n::tr("Presentations"), &["deckcraft", "pptx", "potx", "ppsx"])
                    .add_filter("DeckCraft", &["deckcraft"])
                    .add_filter("PowerPoint", &["pptx", "potx", "ppsx"])
                    .add_filter(deckcraft_ui_egui::i18n::tr("Outline"), &["txt", "md"]),
            };
            d.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| rfd::FileDialog::new().set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string()))),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        clipboard_image: Some(Box::new(pasteboard::image_png)),
        clipboard_html: Some(Box::new(pasteboard::html)),
        clipboard_set_html: Some(Box::new(pasteboard::set_html)),
        // Media is mixed and clocked by the output device; `DECKCRAFT_NO_AUDIO` plays silently.
        audio_out: std::env::var_os("DECKCRAFT_NO_AUDIO")
            .is_none()
            .then(|| Box::new(audio::CpalOut::default()) as Box<dyn deckcraft_media::AudioOut>),
        ..Default::default()
    }
}

const APP_ID: &str = "ai.storyteller.deckcraft";

const USAGE: &str = "\
Usage: deckcraft [--control PORT] [--sample] [--show] [FILE...]

  --control PORT  JSON control server on 127.0.0.1:PORT
  --sample        open the sample presentation
  --show          start the slide show
  --help, -h      print this help and exit
  --version, -V   print the version and exit
";

enum Startup {
    Help,
    Version,
    Run { control_port: Option<u16>, files: Vec<String>, sample: bool, show: bool },
}

/// `--help` and `--version` leave before the window and the log file.
fn startup(mut control_port: Option<u16>, args: impl IntoIterator<Item = String>) -> Startup {
    let mut files = Vec::new();
    let mut sample = false;
    let mut show = false;
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = args.next().and_then(|p| p.parse().ok()),
            "--sample" => sample = true,
            "--show" => show = true,
            "--version" | "-V" => return Startup::Version,
            "--help" | "-h" => return Startup::Help,
            _ => files.push(a),
        }
    }
    Startup::Run { control_port, files, sample, show }
}

fn app_icon() -> Option<egui::IconData> {
    let png: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.deckcraft.png");
    eframe::icon_data::from_png_bytes(png).map_err(|e| log::warn!("app icon: {e}")).ok()
}

fn main() -> eframe::Result {
    // First, so every start-up record (and the engine's panic hook, installed with the first
    // Session) is captured; see `logging`.
    let logger = logging::install();
    let (control_port, files, sample, show) =
        match startup(std::env::var("DECKCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok()), std::env::args().skip(1)) {
            Startup::Help => {
                print!("{USAGE}");
                return Ok(());
            }
            Startup::Version => {
                println!("deckcraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Startup::Run { control_port, files, sample, show } => (control_port, files, sample, show),
        };
    log::info!("DeckCraft {} ({} {})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    // The log file lives under the settings directory; opened after the arguments, so `--help` and
    // `--version` leave no file behind. Records logged until now are written to it first.
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
    let x11 = match display::choose_from_env() {
        display::Choice::X11 => true,
        display::Choice::Default => false,
        display::Choice::None => {
            // winit's own error says little to someone who started DeckCraft from a console.
            log::error!("no display: DISPLAY and WAYLAND_DISPLAY are not set");
            eprintln!("DeckCraft needs a graphical session: neither DISPLAY nor WAYLAND_DISPLAY is set.");
            false
        }
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    if x11 {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    // Before eframe creates the wgpu instance: default Windows to DirectX 12 with FXC (see graphics.rs).
    #[cfg(target_os = "windows")]
    graphics::configure(&mut options, eframe::wgpu::Backends::from_env(), eframe::wgpu::Dx12Compiler::from_env());
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
                    deckcraft_ui_egui::i18n::set_current(app.ui.interface_language.resolve(app.services.system_locale.as_deref()));
                    app.set_status(deckcraft_ui_egui::i18n::recovered(n));
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
            Ok(Box::new(App(app)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #279: on macOS the process ends inside `on_exit`, before eframe would tear the window down.
    /// The check runs in a child (this test binary again), since on macOS it ends the process.
    #[test]
    fn on_exit_ends_the_process_on_macos() {
        if std::env::var_os("DECKCRAFT_TEST_ON_EXIT").is_some() {
            let mut app = App(SlideApp::new(Session::new(), Services::default()));
            println!("calling on_exit");
            eframe::App::on_exit(&mut app);
            println!("on_exit returned");
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::on_exit_ends_the_process_on_macos", "--nocapture"])
            .env("DECKCRAFT_TEST_ON_EXIT", "1")
            .env("DECKCRAFT_NO_PREFS", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{stdout}");
        assert!(stdout.contains("calling on_exit"), "the child didn't run the test: {stdout}");
        assert_eq!(stdout.contains("on_exit returned"), !cfg!(target_os = "macos"), "{stdout}");
    }

    #[test]
    fn help_exits_instead_of_opening_a_file() {
        assert!(matches!(startup(None, ["--help".to_string()]), Startup::Help));
        assert!(matches!(startup(None, ["-h".to_string()]), Startup::Help));
        assert!(matches!(startup(None, ["talk.pptx".to_string(), "--help".to_string()]), Startup::Help));
        assert!(matches!(startup(None, ["--version".to_string()]), Startup::Version));
        let launch = startup(Some(9), ["--sample".to_string(), "--control".to_string(), "4242".to_string(), "talk.pptx".to_string()]);
        assert!(matches!(launch, Startup::Run { .. }));
        if let Startup::Run { control_port, files, sample, show } = launch {
            assert_eq!(control_port, Some(4242));
            assert_eq!(files, ["talk.pptx".to_string()]);
            assert!(sample);
            assert!(!show);
        }
    }
}
