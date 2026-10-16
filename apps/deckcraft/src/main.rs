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

struct App {
    ui: SlideApp,
    #[cfg(target_os = "macos")]
    os_events: fmv_macos_events::Inbox,
    #[cfg(target_os = "macos")]
    pending_open: std::collections::VecDeque<std::path::PathBuf>,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        for event in self.os_events.drain() {
            match event {
                fmv_macos_events::Event::Open(paths) => {
                    self.pending_open.extend(paths);
                }
                fmv_macos_events::Event::Quit => {
                    if let Err(e) = self.ui.run("app.quit", serde_json::json!({})) {
                        log::error!("quit: {e}");
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        if self.ui.opening.is_none()
            && let Some(path) = self.pending_open.pop_front()
        {
            self.ui.open_path_in_background(&path.to_string_lossy());
        }
        self.ui.logic(ctx);
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.ui.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui.ui(ui);
    }
    fn on_exit(&mut self) {
        save_prefs(&self.ui);
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

/// Read an opened regular file without trusting its size to remain unchanged.
fn read_opened_bounded(file: std::fs::File, limit: usize) -> Result<Vec<u8>, String> {
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let limit_u64 = u64::try_from(limit).map_err(|_| "File byte limit is too large".to_string())?;
    if !metadata.is_file() {
        return Err("Choose a regular file for the attachment".into());
    }
    if metadata.len() > limit_u64 {
        return Err("Attachment exceeds 32 MiB limit".into());
    }
    read_stream_bounded(file, limit).map_err(|error| error.to_string())
}

fn read_stream_bounded(mut reader: impl std::io::Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let remaining = limit.saturating_sub(bytes.len());
        // At the exact limit, probe one byte on the stack to detect growth or an
        // inaccurate initial size without ever growing the result past its cap.
        let wanted = if remaining == 0 { 1 } else { remaining.min(chunk.len()) };
        let read = match reader.read(chunk.get_mut(..wanted).ok_or_else(|| std::io::Error::other("invalid bounded read size"))?) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 {
            return Ok(bytes);
        }
        if remaining == 0 || read > remaining {
            return Err(std::io::Error::other("Attachment exceeds 32 MiB limit"));
        }
        let source = chunk.get(..read).ok_or_else(|| std::io::Error::other("invalid bounded read result"))?;
        let length = bytes.len().checked_add(read).ok_or_else(|| std::io::Error::other("attachment size overflow"))?;
        if length > bytes.capacity() {
            let capacity = bytes.capacity().saturating_mul(2).max(4096).max(length).min(limit);
            bytes.try_reserve_exact(capacity.saturating_sub(bytes.len())).map_err(|_| std::io::Error::other("cannot allocate attachment bytes"))?;
        }
        bytes.extend_from_slice(source);
    }
}

fn read_attachment_bounded(path: &str, limit: usize) -> Result<Vec<u8>, String> {
    // Keep this one handle for metadata and reading: replacing the pathname cannot
    // redirect the read after the size check. The stream cap also handles growth.
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A selected path can be replaced with a FIFO before open. NONBLOCK
        // lets the opened-handle type check reject it without waiting for a writer.
        let flags = i32::try_from(rustix::fs::OFlags::NONBLOCK.bits()).map_err(|_| "Unsupported nonblocking file flags".to_string())?;
        options.custom_flags(flags);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    read_opened_bounded(file, limit)
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
                "commentAttachment" => d,
                "theme" => d.add_filter("Themes and presentations", &["thmx", "pptx", "potx", "ppsx"]).add_filter("Office Theme", &["thmx"]),
                _ => d
                    .add_filter("Presentations", &["deckcraft", "pptx", "potx", "ppsx", "odp"])
                    .add_filter("DeckCraft", &["deckcraft"])
                    .add_filter("PowerPoint", &["pptx", "potx", "ppsx"])
                    .add_filter("OpenDocument", &["odp"])
                    .add_filter("Outline", &["txt", "md"]),
            };
            d.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| rfd::FileDialog::new().set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string()))),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        read_bounded: Some(Box::new(read_attachment_bounded)),
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
    // Register before the event loop starts: Finder can send open-document events during launch.
    #[cfg(target_os = "macos")]
    let (_open_events, os_events) = fmv_macos_events::Registration::install();
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
            #[cfg(target_os = "macos")]
            {
                let ctx = cc.egui_ctx.clone();
                os_events.set_wake(move || ctx.request_repaint());
            }
            Ok(Box::new(App {
                ui: app,
                #[cfg(target_os = "macos")]
                os_events,
                #[cfg(target_os = "macos")]
                pending_open: Default::default(),
            }))
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
            let mut app = App {
                ui: SlideApp::new(Session::new(), Services::default()),
                #[cfg(target_os = "macos")]
                os_events: Default::default(),
                #[cfg(target_os = "macos")]
                pending_open: Default::default(),
            };
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

#[cfg(test)]
mod attachment_read_tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!("deckcraft-attachment-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> std::path::PathBuf {
            self.0.join("selected.txt")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn bounded_stream_accepts_exact_limit_and_empty_input() {
        assert_eq!(read_stream_bounded(std::io::Cursor::new(b"abcd"), 4).unwrap(), b"abcd");
        assert_eq!(read_stream_bounded(std::io::Cursor::new(b""), 0).unwrap(), b"");
        assert!(read_stream_bounded(std::io::Cursor::new(b"a"), 0).is_err());
    }
    #[test]
    fn overflow_reads_only_one_extra_stack_byte_and_never_grows_the_result() {
        struct Counting {
            input: std::io::Cursor<Vec<u8>>,
            consumed: usize,
        }
        impl Read for Counting {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let n = self.input.read(out)?;
                self.consumed += n;
                Ok(n)
            }
        }
        let mut reader = Counting { input: std::io::Cursor::new(vec![7; 100_000]), consumed: 0 };
        assert!(read_stream_bounded(&mut reader, 4).unwrap_err().to_string().contains("limit"));
        assert_eq!(reader.consumed, 5);
    }
    #[test]
    fn sparse_oversized_file_rejects_from_opened_metadata() {
        let fixture = Fixture::new();
        let path = fixture.path();
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(deckcraft_ui_egui::MAX_ATTACHMENT_BYTES as u64 + 1).unwrap();
        assert!(read_attachment_bounded(path.to_str().unwrap(), deckcraft_ui_egui::MAX_ATTACHMENT_BYTES).unwrap_err().contains("limit"));
    }
    #[test]
    fn replacing_the_path_does_not_redirect_an_already_opened_attachment() {
        let fixture = Fixture::new();
        let path = fixture.path();
        std::fs::write(&path, b"old").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        std::fs::rename(&path, fixture.0.join("original.txt")).unwrap();
        std::fs::write(&path, b"oversized replacement").unwrap();
        assert_eq!(read_opened_bounded(file, 4).unwrap(), b"old");
    }
    #[test]
    fn growth_after_size_observation_is_still_rejected_by_the_stream_cap() {
        let fixture = Fixture::new();
        let path = fixture.path();
        std::fs::write(&path, b"abcd").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 4);
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"extra").unwrap();
        assert!(read_stream_bounded(file, 4).unwrap_err().to_string().contains("limit"));
    }
    #[test]
    fn interrupted_reads_retry_and_io_errors_are_returned() {
        struct Failing {
            calls: usize,
        }
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                self.calls += 1;
                if self.calls == 1 { Err(std::io::ErrorKind::Interrupted.into()) } else { Err(std::io::Error::other("synthetic read error")) }
            }
        }
        let mut reader = Failing { calls: 0 };
        assert!(read_stream_bounded(&mut reader, 4).unwrap_err().to_string().contains("synthetic read error"));
        assert_eq!(reader.calls, 2);
    }
    #[test]
    fn a_directory_cannot_be_read_as_an_attachment() {
        let fixture = Fixture::new();
        assert!(read_attachment_bounded(fixture.0.to_str().unwrap(), 4).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn fifo_refuses_without_waiting_for_a_writer() {
        if let Some(path) = std::env::var_os("DECKCRAFT_TEST_ATTACHMENT_FIFO") {
            assert!(read_attachment_bounded(&path.to_string_lossy(), 4).is_err());
            return;
        }
        let fixture = Fixture::new();
        let path = fixture.path();
        assert!(std::process::Command::new("mkfifo").arg(&path).status().unwrap().success());
        // A regression to blocking open must fail this test, not hang the whole suite.
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "attachment_read_tests::fifo_refuses_without_waiting_for_a_writer", "--nocapture"])
            .env("DECKCRAFT_TEST_ATTACHMENT_FIFO", &path)
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if started.elapsed() > std::time::Duration::from_secs(2) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("opening the selected FIFO waited for a writer");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
