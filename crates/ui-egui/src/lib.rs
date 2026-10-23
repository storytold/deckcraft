//! DeckCraft's egui frontend: a PowerPoint-style UI over `deckcraft-engine`.
//!
//! The UI is thin: every action goes through [`SlideApp::run`], which handles UI commands
//! (views, panes, zoom) here and sends everything else to the engine. The ribbon, menus,
//! shortcuts, the command palette and the control channel all share that entry point.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod animtimeline;
pub mod backstage;
pub mod canvas;
pub mod control;
pub mod credits;
pub mod dialogs;
pub mod doc_window;
pub mod eyedropper;
pub mod fillui;
pub mod i18n;
pub mod icons;
pub mod media;
pub mod menus;
pub mod notes;
pub mod opening;
pub mod panes;
mod preview;
pub mod qat;
pub mod ribbon;
pub mod selpane;
pub mod settings;
pub mod show;
pub mod sorter;
pub mod squiggles;
pub mod status;
pub mod textures;
pub mod theme;
pub mod thumbs;
pub mod toast;
pub mod uifonts;
pub mod widgets;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use deckcraft_engine::{Mods, Session, Target, UiRequest};
use deckcraft_model::{LayoutId, MasterId, SlideId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse};

pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;
pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;
pub type PickFn = Box<dyn FnMut(&str) -> Option<String>>;
/// `(purpose, request id)`. The host shows a picker and later pushes an [`InboxEvent`]
/// carrying that same id. Completing a cancelled or unknown id does nothing: the file is
/// not applied to whichever document happens to be active.
pub type OpenAsyncFn = Box<dyn FnMut(&str, u64)>;
pub type DownloadFn = Box<dyn FnMut(&str, &[u8])>;

/// A file the host finished reading for the request `id` from [`SlideApp::begin_external_file`].
#[derive(Clone, Debug)]
pub struct InboxFile {
    /// Cancellation identity. A file whose id was cancelled, or never started, is ignored.
    pub id: u64,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// What the host delivers through [`Inbox`] after an asynchronous read.
#[derive(Clone, Debug)]
pub enum InboxEvent {
    File(InboxFile),
    /// The picker was dismissed or the file could not be read. `id` will not be applied.
    Cancelled {
        id: u64,
    },
}

/// Files delivered asynchronously by the host (web file picker, dropped files).
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<InboxEvent>>>;

/// The operation the user asked for. Kept with the request so a late completion does not
/// guess a command from the file name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FilePurpose {
    Open,
    Picture,
    Audio,
    Video,
    /// Change Picture: the picture replaces the selected one (`picture.change`).
    ChangePicture,
    /// A drop, or any other purpose: decide from the name and bytes.
    Infer,
}

/// Where in a presentation a file should land. Ids, not indices: slides can move while
/// the browser is still reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FilePlace {
    Slide(SlideId),
    Master(MasterId),
    Layout {
        master: MasterId,
        layout: LayoutId,
    },
    /// The handout or notes master (one per presentation, so no id is needed).
    NotesPage(Target),
    /// The document was open but had no current slide, master or layout.
    Unspecified,
}

/// The presentation (and place in it) captured when the request started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileTarget {
    /// Nothing was open. An insert makes its own document rather than using one opened
    /// while the file was being read.
    Absent,
    Document {
        doc: u64,
        place: FilePlace,
    },
}

struct PendingFile {
    id: u64,
    purpose: FilePurpose,
    target: FileTarget,
}

struct LocatedPlace {
    target: Target,
    slide: Option<usize>,
}

fn purpose_of(purpose: &str) -> FilePurpose {
    match purpose {
        "open" => FilePurpose::Open,
        "picture" => FilePurpose::Picture,
        "audio" => FilePurpose::Audio,
        "video" => FilePurpose::Video,
        _ => FilePurpose::Infer,
    }
}

fn is_presentation_name(lower: &str) -> bool {
    lower.ends_with(".deckcraft")
        || lower.ends_with(".slidecraft")
        || lower.ends_with(".pptx")
        || lower.ends_with(".potx")
        || lower.ends_with(".ppsx")
}

/// File → Open keeps the requested operation: a presentation opens, an outline becomes slides.
fn open_command(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if is_presentation_name(&lower) {
        Some("file.openBytes")
    } else if lower.ends_with(".txt") || lower.ends_with(".md") {
        Some("slide.fromOutline")
    } else {
        None
    }
}

/// A drop has no requested operation, so the name and bytes decide: a presentation opens,
/// then a recognised picture, sound or video is inserted, then an outline becomes slides.
fn infer_command(name: &str, bytes: &[u8]) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if is_presentation_name(&lower) {
        return Some("file.openBytes");
    }
    let ct = deckcraft_engine::cmd::insert::content_type(name, bytes);
    if ct.starts_with("image/") {
        Some("insert.picture")
    } else if ct.starts_with("audio/") {
        Some("insert.audio")
    } else if ct.starts_with("video/") {
        Some("insert.video")
    } else if lower.ends_with(".txt") || lower.ends_with(".md") {
        Some("slide.fromOutline")
    } else {
        None
    }
}

fn command_for(purpose: FilePurpose, name: &str, bytes: &[u8]) -> Result<&'static str, String> {
    let cmd = match purpose {
        FilePurpose::Picture => Some("insert.picture"),
        FilePurpose::Audio => Some("insert.audio"),
        FilePurpose::Video => Some("insert.video"),
        FilePurpose::ChangePicture => deckcraft_engine::cmd::insert::content_type(name, bytes).starts_with("image/").then_some("picture.change"),
        FilePurpose::Open => open_command(name),
        FilePurpose::Infer => infer_command(name, bytes),
    };
    cmd.ok_or_else(|| crate::i18n::format("{name}: DeckCraft can't open this kind of file", &[name.to_string()]))
}

/// How long a transient status message stays visible.
const STATUS_LINGER_MS: f64 = 6000.0;

/// Platform services injected by the host (desktop or web).
#[derive(Default)]
pub struct Services {
    /// Host locale (OS on desktop, navigator.language on the web).
    pub system_locale: Option<String>,
    /// Open-file dialog for a purpose (`open`, `picture`, `audio`, `video`) → path.
    pub pick_open: Option<PickFn>,
    /// Save dialog with a suggested name → path.
    pub pick_save: Option<PickFn>,
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
    /// Asynchronous open dialog (web). The callback receives the purpose and the request id
    /// from [`SlideApp::begin_external_file`]; the chosen file arrives through `inbox`.
    pub open_async: Option<OpenAsyncFn>,
    /// Hand bytes to the user as a named file (browser download).
    pub download: Option<DownloadFn>,
    pub inbox: Option<Inbox>,
    /// Image on the system clipboard (PNG bytes), when the host can read one.
    pub clipboard_image: Option<Box<dyn FnMut() -> Option<Vec<u8>>>>,
    /// HTML on the system clipboard (Pages, Keynote, and other apps that write `public.html`).
    pub clipboard_html: Option<Box<dyn FnMut() -> Option<String>>>,
    /// Put HTML and its plain-text alternative on the system clipboard. `true` when it landed.
    pub clipboard_set_html: Option<Box<dyn FnMut(&str, &str) -> bool>>,
    /// Audio output for media playback (cpal on desktop); without one media plays silently.
    pub audio_out: Option<Box<dyn deckcraft_media::AudioOut>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewMode {
    #[default]
    Normal,
    Outline,
    Sorter,
    NotesPage,
    Reading,
}

/// Persisted UI state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub interface_language: i18n::Language,
    pub brightness: theme::Brightness,
    pub tab: String,
    pub ribbon_collapsed: bool,
    pub view: ViewMode,
    pub notes: bool,
    pub notes_height: f32,
    pub thumbs_width: f32,
    /// Open task pane: `format`, `animation`, `selection`, `comments`, `designer`, `background`,
    /// `smartart` (Text Pane).
    pub pane: Option<String>,
    pub pane_width: f32,
    pub ruler: bool,
    pub gridlines: bool,
    /// Slides per page previewed in Handout Master view (1, 2, 3, 4, 6 or 9).
    pub handout_per_page: u8,
    pub guides: bool,
    pub smart_guides: bool,
    /// None = fit to window.
    pub zoom: Option<f32>,
    pub sorter_zoom: f32,
    pub recent: Vec<String>,
    pub presenter_view: bool,
    pub ui_scale: f32,
    /// Format Shape pane: tab (`fill`, `effects`, `size`, `picture`, `text`).
    pub format_tab: String,
    /// The Text Pane's current SmartArt line (Add Shape, Promote and Demote act on it).
    pub smartart_row: usize,
    /// A Text Pane line to focus once it is drawn (after Enter, Tab or Backspace rebuilt the list).
    pub smartart_focus: Option<usize>,
    /// The Text Pane's line drafts, for the SmartArt group they were read from.
    pub smartart_drafts: (u32, Vec<String>),
    pub grayscale: bool,
    /// Comment the Review Previous/Next buttons last landed on: `(slide, index)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_focus: Option<(usize, usize)>,
    /// Quick Access Toolbar buttons, left to right.
    pub qat: Vec<qat::QatItem>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            interface_language: i18n::Language::System,
            brightness: theme::Brightness::Light,
            tab: "Home".into(),
            ribbon_collapsed: false,
            view: ViewMode::Normal,
            notes: true,
            notes_height: 52.0,
            thumbs_width: 236.0,
            pane: None,
            pane_width: 300.0,
            ruler: false,
            gridlines: false,
            handout_per_page: 6,
            guides: false,
            smart_guides: true,
            zoom: None,
            sorter_zoom: 1.0,
            recent: vec![],
            presenter_view: true,
            ui_scale: 1.0,
            format_tab: "fill".into(),
            smartart_row: 0,
            smartart_focus: None,
            smartart_drafts: (0, vec![]),
            grayscale: false,
            comment_focus: None,
            qat: qat::defaults(),
        }
    }
}

/// Text put on the system clipboard when the copied objects (a rectangle, a picture, slides…)
/// carry no text. egui-winit drops Ctrl+V when the system clipboard is empty, so without it the
/// shortcut never reached the session and only the context menu could paste objects (#182). An
/// invisible word joiner, so a paste into another app adds nothing visible.
const OBJECTS_MARKER: &str = "\u{2060}";

/// What a copy/cut leaves on the system clipboard for the session's plain-text rendering `t`.
fn system_clipboard_text(t: &str) -> String {
    if t.is_empty() { OBJECTS_MARKER.to_string() } else { t.to_string() }
}

/// The editor's zoom range (1 = 100 %).
pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.1..=4.0;

impl UiState {
    /// The state with every value usable: `ui.json` is a file on disk, and a damaged or
    /// hand-edited one must not stop the window from opening. A zoom of 10,000 % asked the
    /// renderer for a texture larger than memory at start-up; panel sizes, scales and names are
    /// brought into range the same way, and the recent list is cut to its usual length.
    pub fn sanitized(mut self) -> UiState {
        let d = UiState::default();
        let within = |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        self.zoom = self.zoom.filter(|z| z.is_finite() && *z > 0.0).map(|z| z.clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end()));
        self.sorter_zoom = within(self.sorter_zoom, 0.5, 3.0, d.sorter_zoom);
        self.ui_scale = within(self.ui_scale, 0.5, 3.0, d.ui_scale);
        self.notes_height = within(self.notes_height, 0.0, 2000.0, d.notes_height);
        self.thumbs_width = within(self.thumbs_width, 60.0, 1000.0, d.thumbs_width);
        self.pane_width = within(self.pane_width, 240.0, 520.0, d.pane_width);
        let short = |s: &String| s.len() <= 64;
        if !short(&self.tab) {
            self.tab = d.tab;
        }
        if !short(&self.format_tab) {
            self.format_tab = d.format_tab;
        }
        self.pane = self.pane.filter(short);
        self.recent.retain(|p| !p.is_empty() && p.len() <= 4096 && !p.contains('\0'));
        self.recent.dedup();
        self.recent.truncate(RECENT_MAX);
        self
    }
}

/// How many recent files File › Open and Home list.
pub const RECENT_MAX: usize = 12;

/// UI-only commands (id, label, shortcut, params).
pub const UI_COMMANDS: &[(&str, &str, Option<&str>, &str)] = &[
    ("view.normal", "Normal", None, "{}"),
    ("view.outline", "Outline View", None, "{}"),
    ("view.sorter", "Slide Sorter", None, "{}"),
    ("view.notesPage", "Notes Page", None, "{}"),
    ("view.reading", "Reading View", None, "{}"),
    ("view.notes", "Notes", None, "{on?: bool}"),
    ("view.ruler", "Ruler", None, "{on?: bool}"),
    ("view.gridlines", "Gridlines", Some("Shift+F9"), "{on?: bool}"),
    ("view.guides", "Guides", Some("Alt+F9"), "{on?: bool}"),
    ("view.gridOptions", "Grid Options…", None, "{spacing?: pt, snap?: bool, smartGuides?: bool, gridlines?: bool, guides?: bool}"),
    ("view.zoomIn", "Zoom In", Some("Cmd+="), "{}"),
    ("view.zoomOut", "Zoom Out", Some("Cmd+-"), "{}"),
    ("view.zoom", "Set Zoom", None, "{percent}"),
    ("view.zoomDialog", "Zoom…", None, "{}"),
    ("view.fit", "Fit to Window", None, "{}"),
    ("view.pane", "Task Pane", None, "{pane: format|animation|selection|comments|designer|background|smartart|null}"),
    ("review.thesaurusPane", "Thesaurus", Some("Shift+F7"), "{word?}: open the Thesaurus pane on the selected word"),
    ("view.tab", "Ribbon Tab", None, "{tab}"),
    ("view.collapseRibbon", "Collapse the Ribbon", Some("Cmd+F1"), "{}"),
    ("view.dark", "Dark Mode", None, "{on?: bool}"),
    ("view.grayscale", "Grayscale", None, "{on?: bool}"),
    ("view.newWindow", "New Window", None, "{on?: bool, slide?: index, delta?: int}"),
    ("show.start", "Slide Show", Some("F5"), "{from?: index, custom?: name}"),
    ("show.presenter", "Presenter View", None, "{on?: bool}"),
    ("show.end", "End Show", None, "{}"),
    ("animation.preview", "Preview Animations", None, "{on?: bool}"),
    ("app.home", "Home", None, "{on?}"),
    ("app.backstage", "File", Some("Alt+F"), "{page?: home|new|open|info|saveAs|print|export|account, on?: bool}"),
    ("app.closePresentation", "Close", Some("Cmd+W"), "{}"),
    (
        "app.print",
        "Print",
        Some("Cmd+P"),
        "{layout?, perPage?, slides?, includeHidden?, frame?, paper?: letter|a4|legal|a3, orientation?: portrait|landscape}",
    ),
    ("app.about", "About DeckCraft", None, "{}"),
    ("app.palette", "Command Palette", Some("Cmd+Shift+P"), "{}"),
    ("app.preferences", "Preferences…", Some("Cmd+,"), "{}"),
    ("app.language", "Interface Language", None, "{language: system|en|uk|pl}"),
    ("app.openDialog", "Open…", None, "{purpose?}"),
    ("app.insertPictureDialog", "Picture from File…", None, "{}"),
    ("app.smartArtPictureDialog", "Insert Picture in SmartArt…", None, "{id?, index}"),
    ("app.changePictureDialog", "Change Picture…", None, "{}"),
    ("app.insertAudioDialog", "Audio from File…", None, "{}"),
    ("app.reuseSlidesDialog", "Reuse Slides…", None, "{keepFormatting?: bool}"),
    ("app.insertVideoDialog", "Video from File…", None, "{}"),
    ("app.saveAsDialog", "Save As…", None, "{}"),
    ("app.exportDialog", "Export…", None, "{}"),
    ("app.browseThemeDialog", "Browse for Themes…", None, "{}"),
    ("app.compareDialog", "Compare…", None, "{}"),
    ("app.dialog", "Open Dialog", None, "{id}"),
    ("app.pasteSpecial", "Paste Special…", Some("Cmd+Alt+V"), "{}"),
    ("app.qat", "Quick Access Toolbar", None, "{add: command id, params?, label?, icon?} | {remove: id, params?} | {reset: true} → [buttons]"),
    ("app.find", "Find…", Some("Cmd+F"), "{}"),
    ("app.replace", "Replace…", Some("Cmd+H"), "{}"),
    ("app.links", "Community Links", None, "{}"),
];

pub struct Perf {
    pub frame_ms: f64,
    pub render_ms: f64,
    pub fps: f64,
}

/// Playhead for the in-canvas animation preview.
struct AnimPreview {
    uid: u64,
    slide: usize,
    /// `Arc` address of the slide when preview started. An edit replaces the slide and stops playback.
    slide_ptr: usize,
    /// egui time when the first frame latched the clock (`None` until then).
    started: Option<f64>,
    elapsed: f64,
    timeline: deckcraft_anim::Timeline,
}

pub struct SlideApp {
    /// Table Design's border pen (`color`, `width`, `dash` for `table.borders`).
    pub table_pen: serde_json::Value,
    pub session: Session,
    pub services: Services,
    pub ui: UiState,
    pub textures: textures::Textures,
    pub control_rx: Option<Receiver<ControlRequest>>,
    /// Spelling squiggles, checked on a worker thread.
    pub squiggles: squiggles::Squiggles,
    pub dialog: Option<dialogs::Dialog>,
    /// The word looked up in the Thesaurus pane.
    pub thesaurus_query: String,
    pub show: Option<show::Show>,
    /// Current slide's animations playing on the editor canvas (not the full-screen show).
    anim_preview: Option<AnimPreview>,
    /// Audio/video playback (editor control bar, slide show, `media.*` commands).
    pub media: media::MediaHost,
    pub status: Option<(String, f64)>,
    /// A presentation being read in the background, with its progress window ([`opening`]).
    pub opening: Option<opening::Opening>,
    /// A brief notice over the bottom of the window (missing fonts on opening a file).
    pub toast: Option<toast::Toast>,
    /// Documents (by uid) whose missing fonts have been mentioned.
    fonts_noted: std::collections::HashSet<u64>,
    pub perf: Perf,
    /// Synthetic input events (control channel `ui.click` etc.), injected one per frame.
    pub synthetic: Vec<egui::Event>,
    /// Canvas screen rect and points-per-slide-point scale (for control & tests).
    pub canvas_rect: Option<egui::Rect>,
    /// The Eyedropper is waiting for a click on the slide.
    pub eyedropper: Option<eyedropper::State>,
    pub canvas_scale: f32,
    pub slide_rect: Option<egui::Rect>,
    /// Where the canvas texture (the slide and the pasteboard drawn around it) is on screen.
    pub canvas_region: Option<egui::Rect>,
    pub palette: Option<(String, usize)>,
    /// Integrated title bar (macOS traffic lights over our chrome).
    pub integrated_titlebar: bool,
    /// The File tab's full-window view (Backstage). Over a presentation it replaces the ribbon,
    /// slides and status bar until Esc or Back; with nothing open it is the start screen. Opening,
    /// creating or closing a presentation leaves it.
    pub backstage: Option<backstage::Backstage>,
    pending_urls: Vec<String>,
    /// Text for the system clipboard from a Copy or Cut command, handed over on the next frame.
    pending_copy: Option<String>,
    shot_token: u64,
    queued_shots: Vec<(u64, f64, u32)>,
    pending_shots: Vec<(u64, Option<String>, Sender<ControlResponse>, f64)>,
    styled: bool,
    restyle: bool,
    fonts_ready: bool,
    last_time: f64,
    /// When AutoRecover data was last written (seconds, egui time).
    pub last_recovery: f64,
    /// Quit was confirmed (changes saved or discarded): let the window close.
    pub quit_confirmed: bool,
    pub(crate) notes_buf: (u64, usize, notes::Draft),
    /// Faces borrowed from the font database for text the interface fonts can't draw (#101).
    pub ui_fonts: uifonts::UiFonts,
    /// Thumbnail pane drag (slide index, current drop index).
    pub(crate) thumb_drag: Option<(usize, usize)>,
    /// Selection pane: folded groups, renaming, drag and drop.
    pub selpane: selpane::State,
    /// Arrange ▸ Align: Align to Slide (`true`) or Align Selected Objects, ticked for that selection.
    pub(crate) align_to: Option<(Vec<deckcraft_model::ShapeId>, bool)>,
    /// View › New Window: a second window on one presentation. `None` when it is closed.
    pub doc_window: Option<doc_window::DocWindow>,
    /// Slide Sorter: folded sections, inline rename, dragging a whole section.
    pub sorter: sorter::State,
    /// The slide thumbnail pane was the last thing clicked, so ⌘A selects slides, not shapes.
    pub(crate) slides_focused: bool,
    /// A V key press or a Paste event arrived since the last V release (see `keyboard`).
    v_seen: bool,
    /// Which recent paths exist on disk, probed once per path per session (a dead network share
    /// must not stall every frame). Not persisted.
    recent_checked: HashMap<String, bool>,
    /// A trackpad twist is in progress (one undo step when it ends).
    twisting: bool,
    /// Next cancellation identity for a browser pick or a dropped file.
    next_file_id: u64,
    /// Picks and drops whose bytes have not arrived yet.
    pending_files: Vec<PendingFile>,
}

fn grid_options_touched(p: &Value) -> bool {
    ["spacing", "snap", "smartGuides", "gridlines", "guides"].iter().any(|k| p.get(*k).is_some())
}

fn grid_flag(p: &Value, key: &str) -> Result<Option<bool>, String> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("`{key}` must be true or false")),
    }
}

impl SlideApp {
    pub fn new(session: Session, mut services: Services) -> Self {
        let audio_out = services.audio_out.take();
        SlideApp {
            table_pen: serde_json::json!({"width": 1.0}),
            session,
            services,
            ui: UiState::default(),
            textures: textures::Textures::default(),
            control_rx: None,
            squiggles: Default::default(),
            dialog: None,
            thesaurus_query: String::new(),
            show: None,
            anim_preview: None,
            media: media::MediaHost::new(audio_out),
            status: None,
            opening: None,
            toast: None,
            fonts_noted: Default::default(),
            perf: Perf { frame_ms: 0.0, render_ms: 0.0, fps: 0.0 },
            synthetic: vec![],
            canvas_rect: None,
            eyedropper: None,
            canvas_scale: 1.0,
            slide_rect: None,
            canvas_region: None,
            palette: None,
            integrated_titlebar: false,
            backstage: None,
            pending_urls: vec![],
            pending_copy: None,
            shot_token: 0,
            queued_shots: vec![],
            pending_shots: vec![],
            styled: false,
            restyle: true,
            fonts_ready: false,
            last_time: 0.0,
            last_recovery: 0.0,
            quit_confirmed: false,
            notes_buf: (0, usize::MAX, notes::Draft::default()),
            ui_fonts: uifonts::UiFonts::default(),
            thumb_drag: None,
            selpane: selpane::State::default(),
            align_to: None,
            doc_window: None,
            sorter: sorter::State::default(),
            slides_focused: false,
            v_seen: false,
            recent_checked: HashMap::new(),
            twisting: false,
            next_file_id: 1,
            pending_files: Vec::new(),
        }
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        i18n::set_current(self.ui.interface_language.resolve(self.services.system_locale.as_deref()));
        let msg = msg.into();
        self.status = Some((i18n::tr(&msg).to_string(), now_ms()));
    }

    /// A message that must be seen wherever the user is: a toast, which also shows over the start
    /// screen and the File tab, where there is no status bar.
    pub fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some(toast::Toast { text: msg.into(), at: now_ms() });
    }

    /// The transient status message while it is fresh; clears it once stale. Both the status bar
    /// and the start screen draw it — the start screen has no status bar, and errors raised there
    /// (a missing recent file, a failed sample) used to vanish (issue #172).
    pub fn fresh_status(&mut self) -> Option<String> {
        let (msg, at) = self.status.clone()?;
        if now_ms() - at < STATUS_LINGER_MS {
            Some(msg)
        } else {
            self.status = None;
            None
        }
    }

    /// Does this recent path exist on disk? Probed once per path per session and only on
    /// desktop (the web build has no file system to ask): a stat on a dead network share can
    /// block for seconds, so it must not repeat on every frame. A stale answer is harmless —
    /// opening still tries and reports, and a successful open re-probes.
    pub fn recent_exists(&mut self, path: &str) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            true
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(ok) = self.recent_checked.get(path) {
                return *ok;
            }
            // A stat on a dead network location (UNC share, URL) can block for the OS timeout,
            // so remote-looking paths are never probed: they stay unmarked and are judged when
            // opened. A disconnected mapped drive would still stall once — accepted.
            if path.starts_with("\\\\") || path.contains("://") {
                return true;
            }
            let ok = std::path::Path::new(path).is_file();
            if self.recent_checked.len() > 64 {
                let live: std::collections::HashSet<&String> = self.ui.recent.iter().collect();
                self.recent_checked.retain(|p, _| live.contains(p));
            }
            self.recent_checked.insert(path.to_string(), ok);
            ok
        }
    }

    /// Drop a path from the Recent list (its file is gone or the user asked for it).
    pub fn forget_recent(&mut self, path: &str) {
        self.ui.recent.retain(|p| p != path);
        self.recent_checked.remove(path);
    }

    /// Run a command by id (UI commands here, everything else in the engine). Errors show in the
    /// status bar and are returned.
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        i18n::set_current(self.ui.interface_language.resolve(self.services.system_locale.as_deref()));
        let r = if UI_COMMANDS.iter().any(|c| c.0 == id) {
            self.run_ui(id, &params)
        } else {
            self.session.execute(id, &params).map_err(|e| e.to_string())
        };
        match &r {
            // Copy and Cut from the ribbon, menus, the slide's context menu or the control channel
            // put the text on the system clipboard, as Ctrl+C and Ctrl+X do (#431).
            Ok(v) if matches!(id, "edit.copy" | "edit.cut") => {
                if let Some(t) = v.get("text").and_then(Value::as_str) {
                    self.pending_copy = Some(t.to_string());
                }
            }
            Ok(_) => {}
            Err(e) => self.set_status(e.clone()),
        }
        self.drain_requests();
        r
    }

    pub(crate) fn docs_key(&self) -> (usize, Option<usize>) {
        (self.session.documents().len(), self.session.active_index())
    }

    /// The File tab's view is open over a presentation (not just the start screen with nothing
    /// open).
    pub fn backstage_open(&self) -> bool {
        self.backstage.is_some() && self.session.active().is_some()
    }

    /// Opens the File tab's view on `page` (keeping its state when it is already open).
    pub fn open_backstage(&mut self, page: backstage::Page) {
        let key = self.docs_key();
        match &mut self.backstage {
            Some(b) => b.go(page),
            None => self.backstage = Some(backstage::Backstage::new(page, key)),
        }
    }

    /// Back to the presentation (no effect on the start screen, where nothing is open).
    pub fn close_backstage(&mut self) {
        if self.session.active().is_some() {
            self.backstage = None;
        }
    }

    /// Keeps the File tab's view in step with the open presentations: it closes when one is
    /// opened, created or closed (the result shows), and it is the start screen when none is open.
    fn sync_backstage(&mut self) {
        let key = self.docs_key();
        let has_doc = self.session.active().is_some();
        let stale = self.backstage.as_ref().is_some_and(|b| b.docs != key);
        if has_doc && stale {
            self.backstage = None;
        } else if !has_doc && (stale || self.backstage.is_none()) {
            self.backstage = Some(backstage::Backstage::new(backstage::Page::Home, key));
        }
    }

    /// File › Close and Ctrl+W: closes the active presentation, asking first when it has unsaved
    /// changes (Save, Don't Save, Cancel), as PowerPoint does.
    pub fn close_presentation(&mut self) {
        match self.session.active() {
            None => {}
            Some(d) if d.is_dirty() => self.dialog = Some(dialogs::Dialog::new("close")),
            Some(_) => {
                let _ = self.run("file.close", json!({}));
            }
        }
    }

    /// The file name a save or export suggests: the presentation's title with `ext`.
    fn suggested_name(&self, ext: &str) -> String {
        let name = self.session.active().map(|d| d.title()).unwrap_or_else(|| "Presentation".into());
        format!("{name}.{ext}")
    }

    /// Exports the active presentation as `fmt` (`pdf`, `png`, `pngAll`, `jpeg`, `pptx`,
    /// `deckcraft`, `outline`) through `file.export`: the save picker on the desktop, a download
    /// on the web. `extra` adds `file.export` params (the PDF's layout, slides…). The
    /// presentation keeps its own file. Returns whether a file was written or handed over.
    pub fn export_as(&mut self, fmt: &str, extra: Value) -> bool {
        let Some(slide) = self.session.active().map(|d| d.selection.slide) else { return false };
        let (ext, format, all) = match fmt {
            "pdf" => ("pdf", "pdf", false),
            "png" => ("png", "png", false),
            "pngAll" => ("png", "png", true),
            "jpeg" | "jpg" => ("jpg", "jpeg", false),
            "pptx" => ("pptx", "pptx", false),
            "outline" | "txt" => ("txt", "outline", false),
            "deckcraft" => ("deckcraft", "deckcraft", false),
            "gif" => ("gif", "gif", false),
            other => {
                self.set_status(format!("DeckCraft can't export “{other}”"));
                return false;
            }
        };
        let suggested = self.suggested_name(ext);
        if let Some(pick) = self.services.pick_save.as_mut() {
            let Some(path) = pick(&suggested) else { return false };
            let mut params = json!({"path": path, "format": format, "all": all});
            if let (Some(o), Some(x)) = (params.as_object_mut(), extra.as_object()) {
                o.extend(x.clone());
            }
            return match self.run("file.export", params) {
                Ok(_) => {
                    self.set_status(crate::i18n::format("Exported {path}", std::slice::from_ref(&path)));
                    true
                }
                Err(_) => false,
            };
        }
        let Some(doc) = self.session.active().map(|d| d.doc.clone()) else { return false };
        let bytes = match format {
            "png" | "jpeg" => {
                // Every slide for "all slides", one download each, named like the desktop export (#64).
                let name = self.session.active().map(|d| d.title()).unwrap_or_else(|| "Presentation".into());
                let mut any = false;
                let transparent = extra.get("transparentBackground").and_then(Value::as_bool).unwrap_or(false);
                for (file, bytes) in web_image_files(&doc, slide, all, &name, ext, transparent) {
                    any = self.download(&file, Ok(bytes)) || any;
                }
                return any;
            }
            "pdf" => deckcraft_engine::cmd::file::pdf_bytes(&doc, &extra),
            "gif" => deckcraft_engine::cmd::file::gif_bytes(&doc, &extra),
            other => deckcraft_engine::cmd::file::save_bytes(&doc, other),
        };
        self.download(&suggested, bytes)
    }

    /// Hands `bytes` to the browser as a download (web); false (with the reason in the status
    /// bar) when there are no bytes or no way to download.
    fn download(&mut self, name: &str, bytes: deckcraft_engine::Result<Vec<u8>>) -> bool {
        match (bytes, self.services.download.as_mut()) {
            (Ok(b), Some(dl)) => {
                dl(name, &b);
                true
            }
            (Err(e), _) => {
                self.set_status(e.to_string());
                false
            }
            (Ok(_), None) => {
                self.set_status("Saving a copy needs a file picker, which this window doesn't have.");
                false
            }
        }
    }

    /// File › Print: DeckCraft prints through a PDF. On the desktop the PDF (with the Print page's
    /// layout, slides and options) is written to the temporary folder and opened in the system's
    /// PDF viewer, which has the printers; on the web it downloads.
    pub fn print_pdf(&mut self, params: Value) -> bool {
        let Some((title, doc)) = self.session.active().map(|d| (d.title(), d.doc.clone())) else { return false };
        let mut params = if params.is_object() { params } else { json!({}) };
        if self.services.pick_save.is_none() {
            // Web: the browser hands the PDF to the user.
            let bytes = deckcraft_engine::cmd::file::pdf_bytes(&doc, &params);
            return self.download(&format!("{title}.pdf"), bytes);
        }
        let safe: String = title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
        let path = std::env::temp_dir().join(format!("{safe} (print).pdf")).to_string_lossy().to_string();
        if let Some(o) = params.as_object_mut() {
            o.insert("path".into(), json!(path));
            o.insert("format".into(), json!("pdf"));
        }
        match self.run("file.export", params) {
            Ok(_) => {
                self.open_url(&format!("file:///{}", path.replace('\\', "/").trim_start_matches('/')));
                self.set_status("Opened the presentation as a PDF to print");
                true
            }
            Err(_) => false,
        }
    }

    fn flag(&mut self, params: &Value, cur: bool) -> bool {
        params.get("on").and_then(Value::as_bool).unwrap_or(!cur)
    }

    fn run_ui(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        match id {
            "view.normal" => self.ui.view = ViewMode::Normal,
            "view.outline" => self.ui.view = ViewMode::Outline,
            "view.sorter" => self.ui.view = ViewMode::Sorter,
            "view.notesPage" => self.ui.view = ViewMode::NotesPage,
            "view.reading" => self.start_show(self.session.active().map(|d| d.selection.slide).unwrap_or(0), true),
            "view.notes" => self.ui.notes = self.flag(p, self.ui.notes),
            "view.ruler" => self.ui.ruler = self.flag(p, self.ui.ruler),
            "view.gridlines" => self.ui.gridlines = self.flag(p, self.ui.gridlines),
            "view.guides" => {
                self.ui.guides = self.flag(p, self.ui.guides);
                // A presentation without guides gets PowerPoint's two centre guides.
                if self.ui.guides && self.session.active().is_some_and(|d| d.doc.guides.is_empty()) {
                    let _ = self.session.execute("guide.add", &json!({"defaults": true}));
                }
            }
            "view.gridOptions" => {
                if grid_options_touched(p) {
                    let spacing = match p.get("spacing") {
                        None => None,
                        Some(raw) => {
                            let n = raw.as_f64().filter(|v| v.is_finite()).ok_or("`spacing` must be a finite number of points")?;
                            if !(1.0..=288.0).contains(&n) {
                                return Err("`spacing` must be from 1 to 288 points".into());
                            }
                            Some(n)
                        }
                    };
                    let snap = grid_flag(p, "snap")?;
                    let smart = grid_flag(p, "smartGuides")?;
                    let gridlines = grid_flag(p, "gridlines")?;
                    let guides = grid_flag(p, "guides")?;
                    if let Some(n) = spacing {
                        self.session.prefs.grid_spacing = n;
                    }
                    if let Some(b) = snap {
                        self.session.prefs.snap_to_grid = b;
                    }
                    if let Some(b) = smart {
                        self.session.prefs.smart_guides = b;
                        self.ui.smart_guides = b;
                    }
                    if let Some(b) = gridlines {
                        self.ui.gridlines = b;
                    }
                    if let Some(b) = guides {
                        self.ui.guides = b;
                        // As View › Guides: a presentation without guides gets the two centre guides.
                        if b && self.session.active().is_some_and(|d| d.doc.guides.is_empty()) {
                            let _ = self.session.execute("guide.add", &json!({"defaults": true}));
                        }
                    }
                } else {
                    self.dialog = Some(dialogs::Dialog::new("gridOptions"));
                }
                return Ok(json!({
                    "spacing": self.session.prefs.grid_spacing,
                    "snap": self.session.prefs.snap_to_grid,
                    "smartGuides": self.session.prefs.smart_guides,
                    "gridlines": self.ui.gridlines,
                    "guides": self.ui.guides,
                }));
            }
            "view.zoomIn" | "view.zoomOut" => {
                let cur = self.ui.zoom.unwrap_or(self.canvas_scale);
                let steps = [0.1, 0.25, 0.33, 0.5, 0.66, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
                let next = if id == "view.zoomIn" {
                    steps.iter().copied().find(|s| *s > cur + 0.01).unwrap_or(4.0)
                } else {
                    steps.iter().rev().copied().find(|s| *s < cur - 0.01).unwrap_or(0.1)
                };
                self.ui.zoom = Some(next);
            }
            "view.zoom" => {
                let pct = p.get("percent").and_then(Value::as_f64).ok_or("missing `percent`")?;
                self.ui.zoom = Some((pct as f32 / 100.0).clamp(0.1, 4.0));
            }
            "view.zoomDialog" => self.dialog = Some(dialogs::Dialog::new("zoom")),
            "view.fit" => self.ui.zoom = None,
            "review.thesaurusPane" => {
                let word = match p.get("word").and_then(Value::as_str) {
                    Some(w) => w.to_string(),
                    None => self
                        .session
                        .execute("review.thesaurus", &json!({}))
                        .ok()
                        .and_then(|r| r["word"].as_str().map(String::from))
                        .unwrap_or_default(),
                };
                self.thesaurus_query = word;
                self.ui.pane = Some("thesaurus".into());
            }
            "view.pane" => {
                let pane = p.get("pane").and_then(Value::as_str).map(String::from);
                self.ui.pane =
                    if pane.is_some() && pane == self.ui.pane && p.get("toggle").and_then(Value::as_bool).unwrap_or(false) { None } else { pane };
                if let Some(tab) = p.get("tab").and_then(Value::as_str) {
                    self.ui.format_tab = tab.to_string();
                }
            }
            "view.tab" => {
                let tab = p.get("tab").and_then(Value::as_str).ok_or("missing `tab`")?;
                self.ui.tab = tab.to_string();
                self.ui.ribbon_collapsed = false;
            }
            "view.collapseRibbon" => self.ui.ribbon_collapsed = !self.ui.ribbon_collapsed,
            "view.dark" => {
                let on = self.flag(p, self.ui.brightness == theme::Brightness::Dark);
                self.ui.brightness = if on { theme::Brightness::Dark } else { theme::Brightness::Light };
                self.restyle = true;
            }
            "view.grayscale" => self.ui.grayscale = self.flag(p, self.ui.grayscale),
            "view.newWindow" => return doc_window::command(self, p),
            "show.start" => {
                if let Some(name) = p.get("custom").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    self.start_custom_show(name, false);
                } else if p.get("from").is_some() {
                    let from = p.get("from").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(0);
                    self.start_show(from, false);
                } else {
                    self.start_show(self.show_start_index(), false);
                }
            }
            "show.presenter" => self.ui.presenter_view = self.flag(p, self.ui.presenter_view),
            "show.end" => {
                self.show = None;
                self.media.stop_owned(media::Owner::Show);
            }
            "animation.preview" => {
                if p.get("on").and_then(Value::as_bool) == Some(false) {
                    self.anim_preview = None;
                    return Ok(json!({"playing": false}));
                }
                return self.start_anim_preview();
            }
            "app.home" => {
                let on = self.flag(p, self.backstage_open());
                if on {
                    self.open_backstage(backstage::Page::Home);
                } else {
                    self.close_backstage();
                }
            }
            "app.backstage" => {
                if !p.get("on").and_then(Value::as_bool).unwrap_or(true) {
                    self.close_backstage();
                    return Ok(json!({"page": self.backstage.as_ref().map(|b| b.page.id())}));
                }
                let page = match p.get("page").and_then(Value::as_str) {
                    Some(id) => backstage::Page::parse(id).ok_or_else(|| format!("unknown File page `{id}`"))?,
                    None => backstage::Page::Home,
                };
                if page.needs_doc() && self.session.active().is_none() {
                    return Err(format!("File › {} needs an open presentation", page.label()));
                }
                self.open_backstage(page);
                return Ok(json!({"page": page.id()}));
            }
            "app.closePresentation" => self.close_presentation(),
            "app.print" => {
                if self.session.active().is_none() {
                    return Err("nothing to print: no presentation is open".into());
                }
                return Ok(json!({"printed": self.print_pdf(p.clone())}));
            }
            "app.about" => self.dialog = Some(dialogs::Dialog::new("about")),
            "app.palette" => self.palette = Some((String::new(), 0)),
            "app.language" => {
                let language = p.get("language").and_then(Value::as_str).unwrap_or("system");
                self.ui.interface_language = match language {
                    "system" => i18n::Language::System,
                    "en" => i18n::Language::En,
                    "uk" => i18n::Language::Uk,
                    "pl" => i18n::Language::Pl,
                    _ => return Err("unknown interface language".into()),
                };
                i18n::set_current(self.ui.interface_language.resolve(self.services.system_locale.as_deref()));
            }
            "app.preferences" => self.dialog = Some(dialogs::Dialog::new("preferences")),
            "app.openDialog" => self.pick_and_open(),
            "app.insertPictureDialog" => self.pick_and_insert("picture", "insert.picture"),
            "app.smartArtPictureDialog" => {
                if self.services.pick_open.is_none() {
                    return Err("picking a SmartArt picture needs the desktop app".into());
                }
                self.pick_and_run("picture", "smartArt.picture", p);
            }
            "app.changePictureDialog" => self.pick_and_insert("picture", "picture.change"),
            "app.insertAudioDialog" => self.pick_and_insert("audio", "insert.audio"),
            "app.reuseSlidesDialog" => self.pick_and_reuse(p.get("keepFormatting").and_then(Value::as_bool).unwrap_or(false)),
            "app.insertVideoDialog" => self.pick_and_insert("video", "insert.video"),
            "app.saveAsDialog" => self.save_as_dialog("deckcraft"),
            "app.exportDialog" => self.dialog = Some(dialogs::Dialog::new("export")),
            "app.pasteSpecial" => self.dialog = Some(dialogs::Dialog::new("pasteSpecial")),
            "app.browseThemeDialog" => {
                if self.session.active().is_none() {
                    return Err("open a presentation first".into());
                }
                if self.services.pick_open.is_some() {
                    self.pick_and_insert("theme", "design.browseTheme");
                } else {
                    // The browser's asynchronous picker would open the file as a new document.
                    self.set_status("Browse for Themes needs the desktop app for now".to_string());
                }
            }
            "app.qat" => return qat::command(self, p),
            "app.find" | "app.replace" if self.session.active().is_some() => {
                let mut d = dialogs::Dialog::new("find");
                d.fields.insert("replace".into(), if id == "app.replace" { "1" } else { "0" }.into());
                self.dialog = Some(d);
            }
            "app.find" | "app.replace" => self.palette = Some((String::new(), 0)),
            "app.compareDialog" => {
                if self.session.active().is_none() {
                    return Err("open the presentation to compare first".into());
                }
                if self.services.pick_open.is_some() {
                    self.pick_and_insert("open", "review.compare");
                } else {
                    // The browser's asynchronous picker would open the file as a new document.
                    self.set_status("Compare needs the desktop app for now".to_string());
                }
            }
            "app.dialog" => {
                let d = p.get("id").and_then(Value::as_str).ok_or("missing `id`")?;
                self.dialog = Some(dialogs::Dialog::new(d));
            }
            "app.links" => {
                use deckcraft_engine::links::*;
                return Ok(json!({"discord": DISCORD, "website": WEBSITE, "apps": APPS, "appPage": APP_PAGE, "github": GITHUB}));
            }
            _ => return Err(format!("unknown UI command `{id}`")),
        }
        Ok(Value::Null)
    }

    /// Whether ⌘A should act on the slide list: always in the sorter, in Normal view once the
    /// thumbnail pane has been clicked.
    pub(crate) fn slides_list_focused(&self) -> bool {
        self.ui.view == ViewMode::Sorter || (self.ui.view == ViewMode::Normal && self.slides_focused)
    }

    /// First slide Play from Start should open: the custom show or range, skipping hidden slides.
    pub fn show_start_index(&self) -> usize {
        self.session.active().and_then(|d| deckcraft_anim::show_order(&d.doc).first().copied()).unwrap_or(0)
    }

    pub fn start_show(&mut self, from: usize, reading: bool) {
        self.anim_preview = None;
        self.media.stop_owned(media::Owner::Editor);
        if let Some(d) = self.session.active() {
            self.show = Some(show::Show::new(&d.doc, from, reading, self.ui.presenter_view && !reading));
        }
    }

    /// Play custom show `name` from its first slide. Set Up Slide Show is left as it was.
    pub fn start_custom_show(&mut self, name: &str, reading: bool) {
        let state = self.session.active().and_then(|d| deckcraft_anim::ShowState::play_custom(&d.doc, name));
        let Some(state) = state else { return };
        let from = state.slide;
        self.start_show(from, reading);
        if let Some(show) = self.show.as_mut() {
            show.state = state;
        }
    }

    /// The current slide's animations are playing on the editor canvas.
    pub fn anim_preview_active(&self) -> bool {
        self.anim_preview.is_some()
    }

    /// `(slide, seconds played, sequence length)` while a preview is running.
    pub fn anim_preview_report(&self) -> Option<(usize, f64, f64)> {
        self.anim_preview.as_ref().map(|p| (p.slide, p.elapsed, p.timeline.total_duration()))
    }

    pub fn stop_anim_preview(&mut self) {
        self.anim_preview = None;
    }

    /// Timeline and playhead for this frame's canvas raster. `None` when the editor should draw the slide at rest.
    pub fn anim_preview_draw(&self) -> Option<(deckcraft_anim::Timeline, f64)> {
        self.anim_preview.as_ref().map(|p| (p.timeline.clone(), p.elapsed))
    }

    /// Advance the preview clock. Stops when the slide changes, the slide is edited, or the sequence
    /// (plus a short hold) has finished.
    pub fn tick_anim_preview(&mut self, now: f64) -> bool {
        let Some(p) = self.anim_preview.as_ref() else { return false };
        let (uid, slide, ptr) = (p.uid, p.slide, p.slide_ptr);
        let alive = self.show.is_none()
            && self.session.active().is_some_and(|d| {
                d.uid == uid
                    && d.selection.target == deckcraft_engine::Target::Slides
                    && d.selection.slide == slide
                    && d.doc.slides.get(slide).is_some_and(|s| std::sync::Arc::as_ptr(s) as usize == ptr)
            });
        if !alive {
            self.anim_preview = None;
            return false;
        }
        let Some(p) = self.anim_preview.as_mut() else { return false };
        let now = if now.is_finite() { now } else { p.started.unwrap_or(0.0) };
        let start = *p.started.get_or_insert(now);
        let elapsed = (now - start).max(0.0);
        if !elapsed.is_finite() || elapsed > p.timeline.total_duration() + preview::HOLD_SECS {
            self.anim_preview = None;
            return false;
        }
        p.elapsed = elapsed;
        true
    }

    fn start_anim_preview(&mut self) -> Result<Value, String> {
        let (uid, slide, ptr, timeline) = {
            let Some(d) = self.session.active() else {
                return Err("no presentation is open".into());
            };
            if d.selection.target != deckcraft_engine::Target::Slides {
                return Err("animation preview plays the current slide".into());
            }
            let slide = d.selection.slide;
            let Some(slide_arc) = d.doc.slides.get(slide) else {
                return Err("no slide to preview".into());
            };
            let ptr = std::sync::Arc::as_ptr(slide_arc) as usize;
            let timeline = deckcraft_anim::ShowState::timeline(&d.doc, slide);
            (d.uid, slide, ptr, timeline)
        };
        if timeline.is_empty() {
            self.anim_preview = None;
            self.set_status("This slide has no animations");
            return Ok(json!({"playing": false}));
        }
        if matches!(self.ui.view, ViewMode::Sorter | ViewMode::NotesPage) {
            self.ui.view = ViewMode::Normal;
        }
        self.close_backstage();
        let duration = timeline.total_duration();
        self.anim_preview = Some(AnimPreview { uid, slide, slide_ptr: ptr, started: None, elapsed: 0.0, timeline });
        self.set_status("Previewing animations");
        Ok(json!({"playing": true, "slide": slide, "duration": duration}))
    }

    fn pick_and_open(&mut self) {
        if let Some(pick) = self.services.pick_open.as_mut() {
            if let Some(path) = pick("open") {
                self.open_path_in_background(&path);
            }
        } else if self.services.open_async.is_some() {
            let id = self.begin_external_file("open");
            if let Some(f) = self.services.open_async.as_mut() {
                f("open", id);
            }
        }
    }

    pub fn open_path(&mut self, path: &str) -> Result<(), String> {
        self.session.execute("file.open", &json!({"path": path})).map_err(|e| e.to_string())?;
        self.note_recent(path);
        Ok(())
    }

    /// Files the system hands over (the command line, a double-click through the file
    /// association), opened in order. When the last one is a PowerPoint Show (`.ppsx`), its slide
    /// show starts at once from where Play from Start begins, as PowerPoint does; anything else
    /// opens for editing, and File › Open opens a `.ppsx` for editing too. Returns the files that
    /// couldn't be opened, with the reason.
    pub fn open_from_system(&mut self, paths: &[String]) -> Vec<(String, String)> {
        let mut failed = Vec::new();
        let mut last = None;
        for p in paths {
            match self.open_path(p) {
                Ok(()) => last = Some(p),
                Err(e) => failed.push((p.clone(), e)),
            }
        }
        if last.is_some_and(|p| p.to_ascii_lowercase().ends_with(".ppsx")) {
            self.start_show(self.show_start_index(), false);
        }
        failed
    }

    /// A presentation was opened from `path`: first in the recent list, shown in Normal view.
    pub(crate) fn note_recent(&mut self, path: &str) {
        self.status = None; // an older error (a missing file just before this one) must not linger
        self.recent_checked.remove(path);
        self.ui.recent.retain(|p| p != path);
        self.ui.recent.insert(0, path.to_string());
        self.ui.recent.truncate(RECENT_MAX);
        self.ui.view = ViewMode::Normal;
    }

    /// Transitions › Sound › Other Sound…: pick an audio file for the selected slides.
    pub(crate) fn pick_transition_sound(&mut self) {
        if self.services.pick_open.is_some() {
            self.pick_and_insert("audio", "transition.sound");
        }
    }

    /// Home › New Slide › Reuse Slides…: pick a presentation and insert all its slides after the
    /// current one (desktop only: the browser picker opens what it picks as a document).
    fn pick_and_reuse(&mut self, keep: bool) {
        let Some(pick) = self.services.pick_open.as_mut() else {
            self.set_status("Reuse Slides needs the desktop app".to_string());
            return;
        };
        let Some(path) = pick("open") else { return };
        match self.session.execute("slide.reuse", &json!({"path": path, "keepFormatting": keep})) {
            Ok(r) => self.set_status(format!("Inserted {} slides from {path}", r["slides"])),
            Err(e) => self.set_status(e.to_string()),
        }
    }

    fn pick_and_insert(&mut self, purpose: &str, cmd: &str) {
        self.pick_and_run(purpose, cmd, &json!({}));
    }

    /// Pick a file and run `cmd` with it (`name` + `data`, or `path`) added to `extra`.
    fn pick_and_run(&mut self, purpose: &str, cmd: &str, extra: &Value) {
        if let Some(pick) = self.services.pick_open.as_mut() {
            if let Some(path) = pick(purpose) {
                let mut args = if extra.is_object() { extra.clone() } else { json!({}) };
                let r = match self.services.read.as_ref().map(|r| r(&path)) {
                    Some(Ok(bytes)) => {
                        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        args["name"] = json!(name);
                        args["data"] = json!(deckcraft_engine::cmd::base64_encode(&bytes));
                        self.session.execute(cmd, &args)
                    }
                    _ => {
                        args["path"] = json!(path);
                        self.session.execute(cmd, &args)
                    }
                };
                if let Err(e) = r {
                    self.set_status(e.to_string());
                }
            }
        } else if self.services.open_async.is_some() {
            let id = self.begin_external_file(purpose);
            // Change Picture: the picture that arrives replaces the selected one (#115).
            if cmd == "picture.change"
                && let Some(r) = self.pending_files.iter_mut().find(|r| r.id == id)
            {
                r.purpose = FilePurpose::ChangePicture;
            }
            if let Some(f) = self.services.open_async.as_mut() {
                f(purpose, id);
            }
        }
    }

    pub fn save_as_dialog(&mut self, format: &str) {
        let name = self.session.active().map(|d| d.title()).unwrap_or_else(|| "Presentation".into());
        let ext = if format == "pptx" { "pptx" } else { "deckcraft" };
        let suggested = format!("{name}.{ext}");
        if let Some(pick) = self.services.pick_save.as_mut() {
            if let Some(path) = pick(&suggested) {
                match self.session.execute("file.saveAs", &json!({"path": path})) {
                    Ok(_) => {
                        self.set_status(crate::i18n::format("Saved {path}", std::slice::from_ref(&path)));
                        self.ui.recent.retain(|p| *p != path);
                        self.ui.recent.insert(0, path);
                    }
                    Err(e) => self.set_status(e.to_string()),
                }
            }
        } else if let Some(dl) = self.services.download.as_mut()
            && let Some(d) = self.session.active()
        {
            match deckcraft_engine::cmd::file::save_bytes(&d.doc, ext) {
                Ok(bytes) => dl(&suggested, &bytes),
                Err(e) => self.set_status(e.to_string()),
            }
        }
    }

    /// Save every presentation with unsaved changes (asking for a location where needed).
    /// Returns false if one wasn't saved (cancelled or failed).
    pub fn save_all(&mut self) -> bool {
        let dirty: Vec<usize> = self.session.documents().iter().enumerate().filter(|(_, d)| d.is_dirty()).map(|(i, _)| i).collect();
        for i in dirty {
            self.session.set_active(i);
            self.save();
            if self.session.documents().get(i).is_some_and(|d| d.is_dirty()) {
                return false;
            }
        }
        true
    }

    pub fn save(&mut self) {
        if self.session.active().and_then(|d| d.path.clone()).is_some() {
            match self.session.execute("file.save", &json!({})) {
                Ok(_) => self.set_status("Saved"),
                Err(e) => self.set_status(e.to_string()),
            }
        } else {
            self.save_as_dialog("deckcraft");
        }
    }

    /// Act on requests commands left for the UI.
    fn drain_requests(&mut self) {
        for r in std::mem::take(&mut self.session.ui_requests) {
            match r {
                UiRequest::Dialog { id, params } => {
                    let mut d = dialogs::Dialog::new(&id);
                    d.params = params;
                    self.dialog = Some(d);
                }
                UiRequest::Pane { id } => self.ui.pane = Some(id),
                UiRequest::StartShow { from } => self.start_show(from, false),
                UiRequest::Message { text } => self.set_status(text),
                UiRequest::PickFile { purpose } => {
                    if purpose == "saveAs" {
                        self.save_as_dialog("deckcraft");
                    }
                }
                UiRequest::Media { action, shape, ms } => {
                    let owner = if self.show.is_some() { media::Owner::Show } else { media::Owner::Editor };
                    if let Some((uid, doc)) = self.session.active().map(|d| (d.uid, d.doc.clone())) {
                        self.media.request(uid, &doc, &action, shape, ms, owner);
                    }
                }
            }
        }
    }

    fn drain_inbox(&mut self) {
        let Some(inbox) = self.services.inbox.clone() else { return };
        let events = match inbox.lock() {
            Ok(mut v) => std::mem::take(&mut *v),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };
        for event in events {
            match event {
                InboxEvent::Cancelled { id } => self.pending_files.retain(|r| r.id != id),
                InboxEvent::File(file) => self.apply_external_file(file.id, &file.name, &file.bytes),
            }
        }
    }

    /// Remember the open presentation, the slide (or master/layout) and the operation for a
    /// file the host will read asynchronously. Returns the cancellation identity the host must
    /// echo on the [`InboxEvent`]. Call this when the pick or the drop starts, not when the
    /// bytes arrive.
    pub fn begin_external_file(&mut self, purpose: &str) -> u64 {
        let id = self.next_file_id;
        self.next_file_id = self.next_file_id.wrapping_add(1).max(1);
        self.pending_files.push(PendingFile { id, purpose: purpose_of(purpose), target: self.capture_file_target() });
        id
    }

    /// A dropped or picked file read synchronously: presentations open, media is inserted on
    /// the slide that is current now.
    pub fn open_or_insert_bytes(&mut self, name: &str, bytes: &[u8]) {
        let id = self.begin_external_file("drop");
        self.apply_external_file(id, name, bytes);
    }

    fn capture_file_target(&self) -> FileTarget {
        let Some(d) = self.session.active() else { return FileTarget::Absent };
        let place = match d.selection.target {
            Target::Slides => match d.current_slide() {
                Some(s) => FilePlace::Slide(s.id),
                None => FilePlace::Unspecified,
            },
            Target::Master { master } => match d.doc.masters.get(master) {
                Some(m) => FilePlace::Master(m.id),
                None => FilePlace::Unspecified,
            },
            Target::Layout { master, layout } => match d.doc.masters.get(master) {
                Some(m) => match m.layouts.get(layout) {
                    Some(l) => FilePlace::Layout { master: m.id, layout: l.id },
                    None => FilePlace::Unspecified,
                },
                None => FilePlace::Unspecified,
            },
            t @ (Target::Handout | Target::NotesMaster) => FilePlace::NotesPage(t),
        };
        FileTarget::Document { doc: d.uid, place }
    }

    /// True when the user is still looking at the place this request captured.
    fn viewing_target(&self, target: &FileTarget) -> bool {
        let FileTarget::Document { doc, place } = target else { return false };
        let Some(d) = self.session.active() else { return false };
        if d.uid != *doc {
            return false;
        }
        match place {
            FilePlace::Slide(id) => d.selection.target == Target::Slides && d.current_slide().is_some_and(|s| s.id == *id),
            FilePlace::Master(id) => {
                matches!(d.selection.target, Target::Master { master } if d.doc.masters.get(master).is_some_and(|m| m.id == *id))
            }
            FilePlace::Layout { master, layout } => {
                matches!(d.selection.target, Target::Layout { master: mi, layout: li }
                    if d.doc.masters.get(mi).is_some_and(|m| m.id == *master && m.layouts.get(li).is_some_and(|l| l.id == *layout)))
            }
            FilePlace::NotesPage(t) => d.selection.target == *t,
            FilePlace::Unspecified => false,
        }
    }

    fn apply_external_file(&mut self, id: u64, name: &str, bytes: &[u8]) {
        let Some(pos) = self.pending_files.iter().position(|r| r.id == id) else { return };
        let req = self.pending_files.remove(pos);
        let cmd = match command_for(req.purpose, name, bytes) {
            Ok(cmd) => cmd,
            Err(e) => {
                self.set_status(e);
                return;
            }
        };
        let params = if cmd == "slide.fromOutline" {
            json!({"text": String::from_utf8_lossy(bytes), "markdown": name.to_ascii_lowercase().ends_with(".md")})
        } else {
            json!({"name": name, "data": deckcraft_engine::cmd::base64_encode(bytes)})
        };
        let result = if cmd == "file.openBytes" || self.viewing_target(&req.target) {
            self.session.execute(cmd, &params).map(|_| ()).map_err(|e| e.to_string())
        } else {
            self.execute_on_target(&req.target, name, cmd, &params)
        };
        match result {
            Err(e) if cmd == "file.openBytes" => self.notify(e),
            Err(e) => self.set_status(e),
            Ok(()) => {}
        }
        self.drain_requests();
    }

    /// Run `cmd` on the document and slide captured for the request, then put the view back
    /// if the user has since moved. A missing document or slide cancels the file.
    fn execute_on_target(&mut self, target: &FileTarget, name: &str, cmd: &str, params: &Value) -> Result<(), String> {
        let FileTarget::Document { doc, place } = target else {
            self.session.execute("file.new", &json!({})).map_err(|e| e.to_string())?;
            return self.session.execute(cmd, params).map(|_| ()).map_err(|e| e.to_string());
        };
        let Some(idx) = self.session.documents().iter().position(|d| d.uid == *doc) else {
            return Err(format!("{name} was not applied: the presentation it was chosen for is closed"));
        };
        let view = self.session.active_index();
        let saved = self.session.documents().get(idx).map(|d| d.selection.clone());
        self.session.set_active(idx);
        let located = match self.locate_place(place) {
            Ok(located) => located,
            Err(e) => {
                self.restore_file_view(view);
                return Err(format!("{name} was not applied: {e}"));
            }
        };
        if let Some(st) = self.session.active_mut() {
            st.selection.target = located.target;
            if let Some(slide) = located.slide {
                st.selection.slide = slide;
                st.selection.slides.clear();
            }
            st.selection.shapes.clear();
            st.selection.text = None;
            st.selection.cells = None;
        }
        let result = self.session.execute(cmd, params).map(|_| ()).map_err(|e| e.to_string());
        if view == Some(idx) {
            if let (Some(sel), Some(st)) = (saved, self.session.active_mut()) {
                st.selection = sel;
            }
        } else {
            self.restore_file_view(view);
        }
        result
    }

    fn restore_file_view(&mut self, view: Option<usize>) {
        if let Some(i) = view
            && i < self.session.documents().len()
        {
            self.session.set_active(i);
        }
    }

    fn locate_place(&self, place: &FilePlace) -> Result<LocatedPlace, String> {
        let d = self.session.doc().map_err(|e| e.to_string())?;
        match place {
            FilePlace::Slide(id) => {
                let slide = d.doc.slide_index(*id).ok_or_else(|| "the slide it was chosen for was deleted".to_string())?;
                Ok(LocatedPlace { target: Target::Slides, slide: Some(slide) })
            }
            FilePlace::Master(id) => {
                let master =
                    d.doc.masters.iter().position(|m| m.id == *id).ok_or_else(|| "the slide master it was chosen for was deleted".to_string())?;
                Ok(LocatedPlace { target: Target::Master { master }, slide: None })
            }
            FilePlace::Layout { master, layout } => {
                let mi =
                    d.doc.masters.iter().position(|m| m.id == *master).ok_or_else(|| "the slide master it was chosen for was deleted".to_string())?;
                let li = d
                    .doc
                    .masters
                    .get(mi)
                    .and_then(|m| m.layouts.iter().position(|l| l.id == *layout))
                    .ok_or_else(|| "the layout it was chosen for was deleted".to_string())?;
                Ok(LocatedPlace { target: Target::Layout { master: mi, layout: li }, slide: None })
            }
            FilePlace::NotesPage(t) => Ok(LocatedPlace { target: *t, slide: None }),
            FilePlace::Unspecified => Err("there was no slide to put it on".into()),
        }
    }

    /// Per-frame logic before layout: styles, control requests, screenshots, shortcuts.
    pub fn logic(&mut self, ctx: &egui::Context) {
        i18n::set_current(self.ui.interface_language.resolve(self.services.system_locale.as_deref()));
        let scale = self.ui.ui_scale.clamp(0.5, 3.0);
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        if !self.styled {
            theme::install_fonts_with(ctx, &self.ui_fonts.faces);
            self.styled = true;
            self.restyle = true;
        } else {
            self.fonts_ready = true;
        }
        if self.restyle {
            theme::apply(ctx, &theme::Tokens::for_brightness(self.ui.brightness));
            self.restyle = false;
        }
        let now = ctx.input(|i| i.time);
        let dt = now - self.last_time;
        if dt > 0.0 {
            self.perf.fps = self.perf.fps * 0.9 + (1.0 / dt).min(240.0) * 0.1;
        }
        self.last_time = now;
        self.drain_control(ctx);
        self.poll_opening();
        if self.fonts_ready {
            self.drain_inbox();
        }
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
        if self.fonts_ready {
            self.note_ui_text(ctx);
        }
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        // Closing the window with unsaved changes asks first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed && self.session.documents().iter().any(|d| d.is_dirty()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show = None;
            if self.dialog.as_ref().is_none_or(|d| d.id != "quit") {
                self.dialog = Some(dialogs::Dialog::new("quit"));
            }
        }
        // AutoRecover: unsaved presentations are written to the recovery folder periodically.
        if self.session.recovery_dir.is_some() && now - self.last_recovery > (self.session.prefs.recovery_minutes * 60.0).max(10.0) {
            self.last_recovery = now;
            if self.session.documents().iter().any(|d| d.is_dirty())
                && let Err(e) = self.session.execute("file.recovery.save", &json!({}))
            {
                self.set_status(crate::i18n::format("Couldn't save AutoRecover information: {e}", &[(e).to_string()]));
            }
        }
        // On the web, dropped files can only be read asynchronously: the host reads them and
        // delivers them through `Services::inbox`, tagged with the request captured at the drop.
        #[cfg(not(target_arch = "wasm32"))]
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let name = f.path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped file".into());
            match f.bytes() {
                Ok(bytes) => self.open_or_insert_bytes(&name, &bytes),
                Err(e) => self.set_status(e),
            }
        }
        canvas::trackpad_twist(self, ctx);
    }

    /// Borrow faces for interface text in scripts the interface fonts lack (#101): the open
    /// presentations' names and text, recent files, and what is typed or composed this frame.
    fn note_ui_text(&mut self, ctx: &egui::Context) {
        let typed: String = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) | egui::Event::Paste(t) => Some(t.as_str()),
                    egui::Event::Ime(egui::ImeEvent::Preedit { text, .. } | egui::ImeEvent::Commit(text)) => Some(text.as_str()),
                    _ => None,
                })
                .collect()
        });
        self.ui_fonts.note(ctx, &typed);
        for d in self.session.documents() {
            self.ui_fonts.note(ctx, &d.title());
        }
        for r in &self.ui.recent {
            self.ui_fonts.note(ctx, r);
        }
        if let Some(st) = self.session.active() {
            self.ui_fonts.note_doc(ctx, st);
        }
        self.ui_fonts.apply(ctx);
    }

    /// An input method's event for the slide's text (#101): the preedit is shown in the text and
    /// replaced on each update; the commit replaces it as one undo step.
    fn ime_event(&mut self, e: &egui::ImeEvent) {
        let r = match e {
            egui::ImeEvent::Preedit { text, active_range_chars } => {
                self.session.compose_text(text, active_range_chars.as_ref().map(|r| (r.start, r.end)))
            }
            egui::ImeEvent::Commit(text) => self.session.commit_text(text),
            #[allow(deprecated)]
            egui::ImeEvent::Disabled => {
                self.session.end_composition();
                Ok(Value::Null)
            }
            _ => Ok(Value::Null),
        };
        if let Err(e) = r {
            self.set_status(e.to_string());
        }
    }

    /// Inject synthetic events (one pointer event per frame).
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        if self.synthetic.is_empty() {
            return;
        }
        let n = match self.synthetic.first() {
            Some(egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }) => 1,
            _ => self.synthetic.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(self.synthetic.len(), |i| i + 1),
        };
        if let Some(egui::Event::PointerMoved(p) | egui::Event::PointerButton { pos: p, .. }) = self.synthetic.first() {
            raw.events.push(egui::Event::PointerMoved(*p));
        }
        let n = n.min(self.synthetic.len());
        raw.events.extend(self.synthetic.drain(..n));
    }

    /// Keyboard and clipboard for the slide (when no text field has focus).
    fn keyboard(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        // Ctrl+V with only an image on the clipboard: egui-winit sends no Paste event and swallows the
        // key press, so the release of a V press never seen is the paste. Tracked even while a text
        // field has focus, so that a V typed there is not taken for one.
        let mut image_paste = false;
        for e in &events {
            match e {
                egui::Event::Paste(_) | egui::Event::Key { key: egui::Key::V, pressed: true, .. } => self.v_seen = true,
                egui::Event::Key { key: egui::Key::V, pressed: false, modifiers, .. } => {
                    // Only a release with Command/Ctrl held is a paste: a bare V release is not.
                    let seen = std::mem::take(&mut self.v_seen);
                    image_paste = !seen && (modifiers.command || modifiers.ctrl);
                }
                _ => {}
            }
        }
        if ctx.memory(|m| m.focused().is_some()) || self.dialog.as_ref().is_some_and(|d| d.modal()) || self.palette.is_some() {
            return;
        }
        if self.backstage.is_some() {
            // The File tab: its own keys (Esc goes back, arrows walk the side menu) and app
            // shortcuts (New, Open, Command Palette…); typing, pasting or deleting would edit the
            // hidden presentation.
            for e in events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                    let mods = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command || modifiers.ctrl };
                    if backstage::key(self, key, mods) || !(mods.cmd || mods.alt) || menus::ui_shortcut(self, key, mods) {
                        continue;
                    }
                    // Engine shortcuts that don't touch the hidden slide (Ctrl+N).
                    if mods.cmd
                        && matches!(key, egui::Key::N)
                        && let Err(e) = self.session.key(key_name(key), mods)
                    {
                        self.set_status(e.to_string());
                    }
                }
            }
            self.drain_requests();
            return;
        }
        let img = if image_paste { self.services.clipboard_image.as_mut().and_then(|f| f()) } else { None };
        if let Some(png) = img {
            let data = deckcraft_engine::cmd::base64_encode(&png);
            if let Err(e) = self.session.execute("insert.picture", &json!({"name": "Pasted image.png", "data": data})) {
                self.set_status(e.to_string());
            }
        }
        for e in events {
            match e {
                egui::Event::Text(t) => {
                    if self.session.active().is_some_and(|d| d.selection.text.is_some() || d.selection.shapes.len() == 1)
                        && let Err(e) = self.session.type_text(&t)
                    {
                        self.set_status(e.to_string());
                    }
                }
                egui::Event::Ime(ime) => self.ime_event(&ime),
                egui::Event::Copy => self.publish_clipboard(ctx, false),
                egui::Event::Cut => self.publish_clipboard(ctx, true),
                egui::Event::Paste(text) => {
                    // Our own marker for objects without text: paste what the session copied.
                    let text = if text == OBJECTS_MARKER { String::new() } else { text };
                    // A picture (TIFF from Keynote, a screenshot) lands when the paste has no text.
                    // Text keeps its HTML (bold, italic, underline) when the host can read it.
                    let html = self.services.clipboard_html.as_mut().and_then(|f| f()).filter(|h| !h.is_empty());
                    let img = if text.is_empty() { self.services.clipboard_image.as_mut().and_then(|f| f()) } else { None };
                    let r = match img {
                        Some(png) => self
                            .session
                            .execute("insert.picture", &json!({"name": "Pasted image.png", "data": deckcraft_engine::cmd::base64_encode(&png)})),
                        None => {
                            let mut params = json!({"text": text});
                            if let Some(h) = html {
                                params["html"] = Value::String(h);
                            }
                            self.session.execute("edit.paste", &params)
                        }
                    };
                    if let Err(e) = r {
                        self.set_status(e.to_string());
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if key == egui::Key::Tab {
                        // The slide has the keyboard, so Tab is ours (indent, tab character, next
                        // shape). Left to egui it would also focus the first ribbon widget, and
                        // every key after it would go there instead of the slide (#78).
                        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
                    }
                    if key == egui::Key::Escape && self.anim_preview.is_some() && !modifiers.command && !modifiers.ctrl && !modifiers.alt {
                        self.anim_preview = None;
                        continue;
                    }
                    let mods = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command || modifiers.ctrl };
                    if !self.session.tool.drawing_freeform() && menus::ui_shortcut(self, key, mods) {
                        continue;
                    }
                    let name = key_name(key);
                    match self.session.key(name, mods) {
                        Ok(_) => {}
                        Err(e) => self.set_status(e.to_string()),
                    }
                }
                _ => {}
            }
        }
        self.drain_requests();
    }

    /// Offer the copy or cut on the system clipboard.
    ///
    /// Slides go out as plain text. Text goes out as HTML plus the same plain words, so a paste
    /// back into DeckCraft still uses the internal clipboard. A textless shape leaves the
    /// invisible [`OBJECTS_MARKER`] rather than a picture: with only an image on the clipboard
    /// egui-winit drops Ctrl+V, and the shortcut could no longer paste the shape here (#182).
    fn publish_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        let cmd = if cut { "edit.cut" } else { "edit.copy" };
        let Ok(v) = self.session.execute(cmd, &json!({})) else { return };
        let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let html = v.get("html").and_then(Value::as_str).unwrap_or("").to_string();
        if self.session.clipboard.slides.is_empty() && !html.is_empty() && self.services.clipboard_set_html.as_mut().is_some_and(|f| f(&html, &text))
        {
            return;
        }
        ctx.copy_text(system_clipboard_text(&text));
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        i18n::set_current(self.ui.interface_language.resolve(self.services.system_locale.as_deref()));
        let ctx = ui.ctx().clone();
        if !self.fonts_ready {
            ctx.request_repaint();
            return;
        }
        let t0 = now_ms();
        let t = theme::Tokens::get(&ctx);
        self.tick_media(&ctx);
        eyedropper::tick(self, &ctx);
        self.opening_ui(&ctx);
        if self.show.is_some() {
            self.anim_preview = None;
            show::ui(self, ui);
            doc_window::show(self, &ctx);
            self.perf.frame_ms = now_ms() - t0;
            return;
        }
        self.sync_backstage();
        self.keyboard(&ctx);
        self.sync_backstage();
        ribbon::title_bar(self, ui);
        if self.backstage.is_none() && self.session.active().is_some() {
            ribbon::show(self, ui);
            status::status_bar(self, ui);
        }
        let now = ui.input(|i| i.time);
        if self.tick_anim_preview(now) {
            ctx.request_repaint();
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
            if self.backstage.is_some() {
                // File tab: the Backstage view replaces everything below the title bar.
                backstage::show(self, ui);
                return;
            }
            if self.session.active().is_none() {
                return;
            }
            match self.ui.view {
                ViewMode::Sorter => sorter::show(self, ui),
                ViewMode::NotesPage => sorter::notes_page(self, ui),
                ViewMode::Outline | ViewMode::Normal | ViewMode::Reading => {
                    if self.ui.pane.is_some() {
                        panes::show(self, ui);
                    }
                    if self.ui.view == ViewMode::Outline {
                        sorter::outline_pane(self, ui);
                    } else {
                        thumbs::show(self, ui);
                    }
                    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(ui, |ui| {
                        if self.ui.notes {
                            status::notes_pane(self, ui);
                        }
                        canvas::show(self, ui);
                    });
                }
            }
        });
        toast::note_missing_fonts(self);
        toast::show(self, &ctx);
        dialogs::show(self, &ctx);
        menus::palette(self, &ctx);
        for url in std::mem::take(&mut self.pending_urls) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if let Some(t) = self.pending_copy.take() {
            ctx.copy_text(t);
        }
        self.drain_requests();
        doc_window::show(self, &ctx);
        self.perf.frame_ms = now_ms() - t0;
    }

    /// Media clocks and video textures. Voices belong to the open presentation: closing the last
    /// one, or switching decks, stops them. The editor's media also stops when its slide is left.
    fn tick_media(&mut self, ctx: &egui::Context) {
        let uid = self.session.active().map(|d| d.uid);
        self.media.set_document(uid);
        if !self.media.any_active() {
            self.session.media_status.clear();
            return;
        }
        if self.show.is_none()
            && let Some(d) = self.session.active()
        {
            let here: Vec<deckcraft_model::ShapeId> = if d.selection.target == deckcraft_engine::Target::Slides {
                media::slide_media(&d.doc, d.selection.slide).into_iter().map(|m| m.0).collect()
            } else {
                vec![]
            };
            let gone: Vec<_> =
                self.media.ids().into_iter().filter(|id| !here.contains(id) && self.media.owner(*id) == Some(media::Owner::Editor)).collect();
            for id in gone {
                self.media.stop(id);
            }
        }
        self.media.tick(ctx, &mut self.session);
    }

    pub fn open_url(&mut self, url: &str) {
        self.pending_urls.push(url.to_string());
    }

    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        while let Ok(req) = rx.try_recv() {
            // A request whose sender gave up waiting (reply timeout) must not run late (#719).
            if !req.take() {
                continue;
            }
            let reply = req.reply.clone();
            match control::handle(self, ctx, &req) {
                control::Outcome::Done(v) => {
                    let _ = reply.send(v);
                }
                control::Outcome::Screenshot { path } => {
                    self.shot_token += 1;
                    let token = self.shot_token;
                    self.queued_shots.push((token, now_ms() + 120.0, 0));
                    self.pending_shots.push((token, path, reply, now_ms() + 8000.0));
                }
            }
        }
        self.control_rx = Some(rx);
    }

    fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        self.queued_shots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(*token)));
                false
            } else {
                true
            }
        });
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let events: Vec<_> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            if let Some(i) = self.pending_shots.iter().position(|(t, ..)| *t == token) {
                let (_, path, reply, _) = self.pending_shots.remove(i);
                let _ = reply.send(control::save_screenshot(self, &image, path.as_deref()));
            }
        }
        let now = now_ms();
        self.pending_shots.retain(|(_, _, reply, deadline)| {
            if now < *deadline {
                return true;
            }
            let _ = reply.send(json!({"ok": false, "error": "no frame was presented (screen locked or window hidden); use ui.render"}));
            false
        });
    }
}

/// Engine key name for an egui key.
pub fn key_name(key: egui::Key) -> &'static str {
    use egui::Key::*;
    match key {
        Enter => "Enter",
        Escape => "Escape",
        Backspace => "Backspace",
        Delete => "Delete",
        Tab => "Tab",
        ArrowLeft => "Left",
        ArrowRight => "Right",
        ArrowUp => "Up",
        ArrowDown => "Down",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        Space => "Space",
        F2 => "F2",
        F5 => "F5",
        F7 => "F7",
        other => other.name(),
    }
}

pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_now()
    }
}

/// Images for the browser download: the current slide, or every slide (named `<name>1.png`,
/// `<name>2.png`... like the native export) when `all`. `transparent` leaves the slide
/// background out (PNG only, #784).
fn web_image_files(
    doc: &deckcraft_model::Presentation,
    current: usize,
    all: bool,
    name: &str,
    ext: &str,
    transparent: bool,
) -> Vec<(String, Vec<u8>)> {
    let slides: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![current] };
    let single = slides.len() == 1;
    let scale = 2.0;
    let dpi = deckcraft_render::dpi_for_scale(scale);
    slides
        .into_iter()
        .enumerate()
        .map(|(k, i)| {
            let opts = deckcraft_render::RenderOpts { scale, transparent_background: transparent && ext == "png", ..Default::default() };
            let img = deckcraft_render::render_slide(doc, i, &opts);
            let bytes = if ext == "png" { img.to_png_at(dpi) } else { img.to_jpeg_at(92, Some(dpi)) };
            let file = if single { format!("{name}.{ext}") } else { format!("{name}{}.{ext}", k + 1) };
            (file, bytes)
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
fn js_now() -> f64 {
    js_sys::Date::now()
}

#[cfg(test)]
mod animpane_tests;
#[cfg(test)]
mod browsetheme_tests;
#[cfg(test)]
mod compare_tests;
#[cfg(test)]
mod eyedropper_tests;
#[cfg(test)]
mod huge_deck_tests;
#[cfg(test)]
mod ime_tests;
#[cfg(test)]
mod replacefonts_tests;
#[cfg(test)]
mod slide_size_tests;
#[cfg(test)]
mod texture_limit_tests;
#[cfg(test)]
mod ui_soak_tests;

#[cfg(test)]
#[path = "datetime_tests.rs"]
mod datetime_tests;

#[cfg(test)]
mod inbox_tests;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use egui_kittest::kittest::Queryable;

    fn shape_rot(app: &SlideApp) -> f64 {
        let d = app.session.active().unwrap();
        let id = d.selection.shapes[0];
        d.shape(id).unwrap().xfrm.unwrap().rot
    }

    /// A two-finger twist is egui's Rotate event. Over the canvas it turns the selection;
    /// letting go is one undo step. Off the canvas it does nothing.
    #[test]
    fn a_trackpad_twist_rotates_the_shape_under_the_canvas() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        let id = harness.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 100.0, 60.0]})).unwrap();
        let id = id.get("id").and_then(|v| v.as_u64()).unwrap();
        harness.state_mut().session.execute("edit.select", &json!({"ids": [id]})).unwrap();
        harness.run_steps(1);
        let canvas = harness.state().canvas_rect.expect("canvas");
        let outside = egui::pos2(1.0, 1.0);
        assert!(!canvas.contains(outside), "the ribbon corner is not the canvas: {canvas:?}");

        harness.input_mut().events.push(egui::Event::PointerMoved(outside));
        harness.input_mut().events.push(egui::Event::Rotate(20.0_f32.to_radians()));
        harness.run_steps(1);
        assert!((shape_rot(harness.state()) - 0.0).abs() < 0.01, "off the canvas, still {}", shape_rot(harness.state()));

        harness.input_mut().events.push(egui::Event::PointerMoved(canvas.center()));
        harness.input_mut().events.push(egui::Event::Rotate(20.0_f32.to_radians()));
        harness.run_steps(1);
        harness.input_mut().events.push(egui::Event::PointerMoved(canvas.center()));
        harness.input_mut().events.push(egui::Event::Rotate(10.0_f32.to_radians()));
        harness.run_steps(1);
        assert!((shape_rot(harness.state()) - 30.0).abs() < 0.05, "twist landed on {}", shape_rot(harness.state()));
        harness.run_steps(1);
        assert!(harness.state().session.active().unwrap().interaction.is_none());
        harness.state_mut().session.execute("edit.undo", &json!({})).unwrap();
        assert!((shape_rot(harness.state()) - 0.0).abs() < 0.01, "one undo, got {}", shape_rot(harness.state()));
    }

    /// The browser "PNG images (all slides)" export yields one image per slide, not just the selected one (#64).
    #[test]
    fn web_export_all_slides_gives_one_file_per_slide() {
        let mut doc = deckcraft_model::Presentation::default();
        if let Some(first) = doc.slides.first().cloned() {
            doc.slides.push(first.clone());
            doc.slides.push(first);
        }
        assert_eq!(doc.slides.len(), 3);
        let all = web_image_files(&doc, 1, true, "Deck", "png", false);
        let names: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Deck1.png", "Deck2.png", "Deck3.png"]);
        let one = web_image_files(&doc, 1, false, "Deck", "png", false);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].0, "Deck.png");
    }

    /// The web PNG download honours "Transparent background" (#784): the slide background is
    /// left out, so the corner is transparent; a JPEG download stays opaque.
    #[test]
    fn web_png_export_can_leave_the_background_transparent() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let got: Rc<RefCell<Vec<(String, Vec<u8>)>>> = Rc::default();
        let log = got.clone();
        let services = Services {
            download: Some(Box::new(move |name: &str, b: &[u8]| log.borrow_mut().push((name.to_string(), b.to_vec())))),
            ..Default::default()
        };
        let mut app = SlideApp::new(Session::with_new(), services);
        let corner_alpha = |bytes: &[u8]| deckcraft_render::decode_image(bytes).unwrap().get_pixel(2, 2).0[3];
        assert!(app.export_as("png", json!({})));
        assert!(app.export_as("png", json!({"transparentBackground": true})));
        assert!(app.export_as("jpeg", json!({"transparentBackground": true})));
        let got = got.borrow();
        assert_eq!(got.len(), 3);
        assert_eq!(corner_alpha(&got[0].1), 255, "opaque by default");
        assert_eq!(corner_alpha(&got[1].1), 0, "transparent background");
        assert_eq!(corner_alpha(&got[2].1), 255, "JPEG has no transparency");
    }

    fn title_text(app: &SlideApp) -> String {
        let d = app.session.active().unwrap();
        let sh = &d.doc.slides[0].shapes[0];
        sh.text.as_ref().map(|t| t.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
    }

    #[test]
    fn quick_access_toolbar_adds_removes_resets_and_persists() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        let ids = |app: &SlideApp| app.ui.qat.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(harness.state()), ["app.home", "file.save", "app.exportDialog", "edit.undo", "edit.redo", "show.fromStart"]);
        let app = harness.state_mut();
        app.run("app.qat", json!({"add": "insert.table", "label": "Table", "icon": "table"})).unwrap();
        app.run("app.qat", json!({"add": "insert.table"})).unwrap();
        app.run("app.qat", json!({"add": "view.pane", "params": {"pane": "selection"}, "label": "Selection Pane"})).unwrap();
        assert!(app.run("app.qat", json!({"add": "no.such"})).is_err());
        app.run("app.qat", json!({"remove": "edit.redo"})).unwrap();
        assert_eq!(ids(app), ["app.home", "file.save", "app.exportDialog", "edit.undo", "show.fromStart", "insert.table", "view.pane"]);
        assert_eq!(app.ui.qat[5].label, "Table");
        // Saved with the rest of the UI state and read back.
        let back: UiState = serde_json::from_value(serde_json::to_value(&app.ui).unwrap()).unwrap();
        assert_eq!(back.qat, app.ui.qat);
        harness.run_steps(2);
        let app = harness.state_mut();
        app.run("app.qat", json!({"remove": "view.pane", "params": {"pane": "selection"}})).unwrap();
        assert_eq!(app.ui.qat.len(), 6);
        app.run("app.qat", json!({"reset": true})).unwrap();
        assert_eq!(ids(app), ["app.home", "file.save", "app.exportDialog", "edit.undo", "edit.redo", "show.fromStart"]);
        // An older settings file without the toolbar gets the defaults.
        let old: UiState = serde_json::from_value(json!({"tab": "Home"})).unwrap();
        assert_eq!(old.qat.len(), 6);
    }

    #[test]
    fn opening_a_file_with_missing_fonts_shows_a_toast_once() {
        use egui_kittest::kittest::Queryable;
        let mut p = deckcraft_model::Presentation::default();
        let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
        let mut body = deckcraft_model::text::TextBody::from_text("Hello");
        body.paragraphs[0].runs[0].props.font = Some("Qqq Nonexistent Sans".into());
        slide.shapes[0].text = Some(body);
        let data = deckcraft_engine::cmd::base64_encode(&deckcraft_engine::cmd::file::save_bytes(&p, "pptx").unwrap());
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        assert!(harness.state().toast.is_none(), "a new presentation has nothing to say");
        harness.state_mut().session.execute("file.openBytes", &json!({"name": "a.pptx", "data": data})).unwrap();
        harness.run_steps(2);
        let text = harness.state().toast.as_ref().map(|t| t.text.clone()).unwrap_or_default();
        assert!(text.starts_with("Qqq Nonexistent Sans isn't installed"), "{text}");
        assert!(harness.query_by_label_contains("Qqq Nonexistent Sans isn't installed").is_some());
        // Closed, it stays closed for that document.
        harness.get_by_label("Close").click();
        harness.run_steps(2);
        assert!(harness.state().toast.is_none());
        harness.run_steps(2);
        assert!(harness.state().toast.is_none());
    }

    /// #115: Change Picture replaced nothing — it inserted a second picture.
    #[test]
    fn change_picture_replaces_the_selected_picture() {
        let svg = |c: &str| {
            format!("<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'><rect width='4' height='4' fill='{c}'/></svg>").into_bytes()
        };
        let media = |app: &SlideApp| {
            let d = app.session.active().unwrap();
            let pics: Vec<_> = d.doc.slides[0]
                .shapes
                .iter()
                .filter_map(|s| if let deckcraft_model::ShapeKind::Picture { fill } = &s.kind { Some(fill.media) } else { None })
                .collect();
            assert_eq!(pics.len(), 1, "one picture, replaced in place");
            pics[0]
        };
        let insert = |app: &mut SlideApp| {
            let id =
                app.session.execute("insert.picture", &json!({"name": "a.svg", "data": deckcraft_engine::cmd::base64_encode(&svg("red"))})).unwrap()
                    ["id"]
                    .clone();
            app.session.execute("edit.select", &json!({"ids": [id]})).unwrap();
            media(app)
        };

        // Desktop: the file dialog returns a path.
        let blue = svg("blue");
        let services = Services {
            pick_open: Some(Box::new(|_: &str| Some("b.svg".into()))),
            read: Some(Box::new(move |_: &str| Ok(blue.clone()))),
            ..Default::default()
        };
        let mut app = SlideApp::new(Session::with_new(), services);
        let before = insert(&mut app);
        app.run("app.changePictureDialog", json!({})).unwrap();
        assert_ne!(media(&app), before);

        // Web: the browser picker delivers the file through the inbox later.
        let inbox = Inbox::default();
        let started = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = started.clone();
        let services = Services {
            open_async: Some(Box::new(move |_: &str, id| record.lock().unwrap().push(id))),
            inbox: Some(inbox.clone()),
            ..Default::default()
        };
        let mut app = SlideApp::new(Session::with_new(), services);
        let before = insert(&mut app);
        app.run("app.changePictureDialog", json!({})).unwrap();
        let id = started.lock().unwrap()[0];
        inbox.lock().unwrap().push(InboxEvent::File(InboxFile { id, name: "b.svg".into(), bytes: svg("blue") }));
        app.drain_inbox();
        assert_ne!(media(&app), before);
    }

    /// #17: Home created a new blank presentation instead of going to the start screen.
    #[test]
    fn home_shows_the_start_screen_over_the_open_presentation() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        // The app keeps requesting repaints, so step frames instead of run().
        harness.run_steps(2);
        let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
        harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();

        harness.state_mut().run("app.home", json!({})).unwrap();
        harness.run_steps(2);
        assert!(harness.state().backstage_open());
        assert_eq!(harness.state().session.documents().len(), 1, "Home doesn't create a presentation");

        // Typing on the start screen doesn't reach the presentation behind it; Escape goes back.
        harness.event(egui::Event::Text("a".into()));
        harness.key_press(egui::Key::Backspace);
        harness.run_steps(2);
        assert_eq!(title_text(harness.state()), "");
        assert!(!harness.state().session.active().unwrap().is_dirty());
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        assert!(!harness.state().backstage_open());

        // Home again toggles; picking a theme on the start screen (a new presentation) leaves it.
        harness.state_mut().run("app.home", json!({})).unwrap();
        harness.run_steps(2);
        assert!(harness.state().backstage_open());
        harness.state_mut().run("app.home", json!({})).unwrap();
        assert!(!harness.state().backstage_open());
        harness.state_mut().run("app.home", json!({"on": true})).unwrap();
        harness.state_mut().run("file.new", json!({})).unwrap();
        harness.run_steps(2);
        assert!(!harness.state().backstage_open(), "a new presentation leaves Home");
        assert_eq!(harness.state().session.documents().len(), 2);
    }

    /// #268: Ctrl+Backspace, Ctrl+Delete, Ctrl+arrows and Ctrl+Shift+arrows in text, through real
    /// egui key events (Windows keys).
    #[test]
    fn ctrl_word_keys_edit_text() {
        use egui::{Key, Modifiers};
        let mut session = Session::with_new();
        session.mac_keys = false;
        session.prefs.autocorrect = false;
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(session, Services::default()),
        );
        harness.run_steps(2);
        let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
        harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
        harness.event(egui::Event::Text("Hello, world.  Foo bar".into()));
        harness.run_steps(1);
        assert_eq!(title_text(harness.state()), "Hello, world.  Foo bar");
        let caret = |h: &egui_kittest::Harness<'_, SlideApp>| h.state().session.active().unwrap().selection.text.clone().unwrap();
        let ctrl = Modifiers::COMMAND;
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        // Ctrl+Left stops at every word and punctuation run, as in PowerPoint.
        let mut stops = vec![];
        for _ in 0..6 {
            harness.key_press_modifiers(ctrl, Key::ArrowLeft);
            harness.run_steps(1);
            stops.push(caret(&harness).caret.1);
        }
        assert_eq!(stops, vec![19, 15, 12, 7, 5, 0]);
        harness.key_press_modifiers(ctrl, Key::ArrowRight);
        harness.run_steps(1);
        assert_eq!(caret(&harness).caret, (0, 5));

        // Ctrl+Shift+Right selects to the next stops.
        harness.key_press_modifiers(ctrl_shift, Key::ArrowRight);
        harness.key_press_modifiers(ctrl_shift, Key::ArrowRight);
        harness.run_steps(1);
        let t = caret(&harness);
        assert_eq!((t.anchor, t.caret), ((0, 5), (0, 12)));

        // Ctrl+Delete deletes the next word from the selection's start; Ctrl+Backspace the one
        // before the caret.
        harness.key_press_modifiers(ctrl, Key::Delete);
        harness.run_steps(1);
        assert_eq!(title_text(harness.state()), "Helloworld.  Foo bar");
        harness.key_press(Key::End);
        harness.key_press_modifiers(ctrl, Key::Backspace);
        harness.run_steps(1);
        assert_eq!(title_text(harness.state()), "Helloworld.  Foo ");

        // Ctrl+Up/Down go by paragraph, Ctrl+Home/End to the text's ends.
        harness.key_press(Key::Enter);
        harness.event(egui::Event::Text("Next".into()));
        harness.key_press_modifiers(ctrl, Key::ArrowUp);
        harness.run_steps(1);
        assert_eq!(caret(&harness).caret, (1, 0));
        harness.key_press_modifiers(ctrl, Key::ArrowUp);
        harness.run_steps(1);
        assert_eq!(caret(&harness).caret, (0, 0));
        harness.key_press_modifiers(ctrl, Key::ArrowDown);
        harness.run_steps(1);
        assert_eq!(caret(&harness).caret, (1, 0));
        harness.key_press_modifiers(ctrl_shift, Key::Home);
        harness.run_steps(1);
        let t = caret(&harness);
        assert_eq!((t.anchor, t.caret), ((1, 0), (0, 0)));

        // Ctrl+Shift+> (egui reports the Period key) grows the selected text's font.
        let size = |h: &mut egui_kittest::Harness<'_, SlideApp>| h.state_mut().session.execute("format.state", &json!({})).unwrap()["size"].as_f64();
        let before = size(&mut harness);
        harness.key_press_modifiers(ctrl_shift, Key::Period);
        harness.run_steps(1);
        let after = size(&mut harness).unwrap();
        assert!(after > before.unwrap_or(0.0), "{before:?} → {after}");
        harness.key_press_modifiers(ctrl, Key::OpenBracket);
        harness.run_steps(1);
        assert!(size(&mut harness).unwrap_or(f64::MAX) < after, "Ctrl+[ shrinks");
    }

    /// Export… had no visible control: only the command palette reached it.
    #[test]
    fn title_bar_export_opens_the_export_dialog() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none());
        // Quick access: Home, Save, Export, each 24 points wide with 2 between, from x = 10.
        let pos = egui::pos2(10.0 + 2.0 * 26.0 + 12.0, ribbon::TITLE_H / 2.0);
        harness.hover_at(pos);
        harness.run_steps(1);
        harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        harness.run_steps(2);
        assert_eq!(harness.state().dialog.as_ref().map(|d| d.id.as_str()), Some("export"));
    }

    /// #53: Ctrl+V did nothing for an image-only clipboard: egui-winit sends no Paste event for it and
    /// swallows the key press, so only the V release reaches the app.
    #[test]
    fn ctrl_v_pastes_an_image_when_no_paste_event_arrives() {
        let session = Session::with_new();
        let png = {
            let d = session.active().unwrap();
            deckcraft_engine::cmd::file::render_png(&d.doc, 0, 0.1, false).0
        };
        assert!(!png.is_empty());
        let services = Services { clipboard_image: Some(Box::new(move || Some(png.clone()))), ..Default::default() };
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(session, services),
        );
        harness.run_steps(2);
        let pictures = |app: &SlideApp| {
            let d = app.session.active().unwrap();
            d.doc.slides[0].shapes.iter().filter(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })).count()
        };
        let v = |pressed| egui::Event::Key { key: egui::Key::V, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::COMMAND };
        // A text paste (Paste event on the press) does not also paste the image on release.
        harness.event(egui::Event::Paste("hello".into()));
        harness.run_steps(2);
        harness.event(v(false));
        harness.run_steps(2);
        assert_eq!(pictures(harness.state()), 0);
        // A V release without Command/Ctrl is not a paste.
        harness.event(egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE });
        harness.run_steps(2);
        assert_eq!(pictures(harness.state()), 0);
        // What egui-winit sends for Ctrl+V with only an image on the clipboard: the release alone.
        harness.event(v(false));
        harness.run_steps(2);
        assert_eq!(pictures(harness.state()), 1);
    }

    /// #78: after Tab in a text box, typing and deleting no longer reached the text.
    #[test]
    fn typing_goes_on_after_tab_in_a_text_box() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            {
                let mut session = Session::with_new();
                session.prefs.autocorrect = false;
                SlideApp::new(session, Services::default())
            },
        );
        harness.run_steps(2);
        let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
        harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
        harness.event(egui::Event::Text("ab".into()));
        harness.run_steps(2);
        harness.key_press(egui::Key::Tab);
        harness.run_steps(2);
        harness.event(egui::Event::Text("c".into()));
        harness.run_steps(2);
        assert_eq!(title_text(harness.state()), "ab\tc");
        harness.key_press(egui::Key::Backspace);
        harness.run_steps(2);
        assert_eq!(title_text(harness.state()), "ab\t");
        assert!(harness.state().session.active().unwrap().selection.text.is_some(), "still editing");
        // Tab at the start of a paragraph indents it, and typing goes on there too (#93).
        harness.key_press(egui::Key::Enter);
        harness.run_steps(2);
        harness.key_press(egui::Key::Tab);
        harness.run_steps(2);
        harness.event(egui::Event::Text("d".into()));
        harness.run_steps(2);
        assert_eq!(title_text(harness.state()), "ab\t\nd");
        let level = |h: &egui_kittest::Harness<SlideApp>| {
            h.state().session.active().unwrap().doc.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[1].level
        };
        assert_eq!(level(&harness), 1);
    }

    #[test]
    fn home_needs_an_open_presentation() {
        let mut app = SlideApp::new(Session::new(), Services::default());
        app.run("app.home", json!({})).unwrap();
        assert!(!app.backstage_open(), "with nothing open the start screen shows anyway");
    }

    /// View › New Window is a second window on the open presentation. Its slide is its own;
    /// the editor's selection stays where it was. Closing the window leaves the document open.
    #[test]
    fn new_window_shows_the_same_document_on_its_own_slide() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        app.session.execute("slide.new", &json!({})).unwrap();
        app.session.execute("slide.go", &json!({"index": 1})).unwrap();
        let err = {
            let mut empty = SlideApp::new(Session::new(), Services::default());
            empty.run("view.newWindow", json!({})).unwrap_err()
        };
        assert!(err.contains("presentation"), "{err}");

        let name = app.session.active().unwrap().title();
        let opened = app.run("view.newWindow", json!({})).unwrap();
        assert_eq!(opened["open"], true);
        assert_eq!(opened["window"], 2);
        assert_eq!(opened["slide"], 1);
        assert_eq!(opened["title"], format!("{name}:2"));
        assert_eq!(app.session.active().unwrap().selection.slide, 1);

        let moved = app.run("view.newWindow", json!({"delta": -1})).unwrap();
        assert_eq!(moved["slide"], 0);
        assert_eq!(app.doc_window.as_ref().unwrap().slide, 0);
        assert_eq!(app.session.active().unwrap().selection.slide, 1, "the editor stays on its slide");

        let again = app.run("view.newWindow", json!({})).unwrap();
        assert_eq!(again["window"], 2);
        assert_eq!(again["slide"], 0, "opening again keeps the window's slide");

        app.session.execute("file.close", &json!({})).unwrap();
        doc_window::show(&mut app, &egui::Context::default());
        assert!(app.doc_window.is_none(), "closing the presentation closes the extra window");
        assert!(app.session.active().is_none());
    }

    #[test]
    fn grid_options_sets_spacing_and_display_or_opens_the_dialog() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        assert!((app.session.prefs.grid_spacing - 6.0).abs() < 1e-9);
        let v =
            app.run("view.gridOptions", json!({"spacing": 72.0, "snap": false, "smartGuides": false, "gridlines": true, "guides": true})).unwrap();
        assert_eq!(v["spacing"], 72.0);
        assert_eq!(v["snap"], false);
        assert_eq!(v["smartGuides"], false);
        assert_eq!(v["gridlines"], true);
        assert_eq!(v["guides"], true);
        assert!(app.dialog.is_none());
        assert!(app.ui.gridlines && app.ui.guides);
        assert!(!app.session.prefs.snap_to_grid && !app.ui.smart_guides);

        app.run("view.gridOptions", json!({"snap": true})).unwrap();
        assert!(app.session.prefs.snap_to_grid);
        assert!((app.session.prefs.grid_spacing - 72.0).abs() < 1e-9);
        assert!(app.dialog.is_none());

        for bad in [json!({"spacing": 0}), json!({"spacing": 289}), json!({"spacing": "72"}), json!({"spacing": null}), json!({"snap": 1})] {
            assert!(app.run("view.gridOptions", bad).is_err(), "rejected a bad grid option");
        }
        assert!((app.session.prefs.grid_spacing - 72.0).abs() < 1e-9);
        assert!(app.session.prefs.snap_to_grid);

        app.run("view.gridOptions", json!({})).unwrap();
        assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some("gridOptions"));
        assert!((app.session.prefs.grid_spacing - 72.0).abs() < 1e-9);
    }

    #[test]
    fn grid_options_dialog_lists_spacing_and_display() {
        use egui_kittest::kittest::Queryable;
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        harness.state_mut().run("view.gridOptions", json!({})).unwrap();
        harness.run_steps(2);
        assert!(harness.query_by_label_contains("Display grid on screen").is_some());
        assert!(harness.query_by_label_contains("Snap objects to grid").is_some());
        assert!(harness.query_by_label_contains("Spacing (pt)").is_some());
        let show_grid = harness
            .query_all_by_role_and_label(egui::accesskit::Role::CheckBox, "Display grid on screen")
            .next()
            .unwrap_or_else(|| panic!("no grid checkbox"));
        show_grid.click_accesskit();
        harness.run_steps(2);
        harness.query_all_by_role_and_label(egui::accesskit::Role::Button, "OK").next().unwrap_or_else(|| panic!("no OK button")).click_accesskit();
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none(), "OK closes Grid Options");
        assert!(harness.state().ui.gridlines, "OK applies the grid checkbox");
        assert!((harness.state().session.prefs.grid_spacing - 6.0).abs() < 1e-9);

        harness.state_mut().run("view.gridOptions", json!({})).unwrap();
        harness.run_steps(2);
        harness
            .query_all_by_role_and_label(egui::accesskit::Role::Button, "Cancel")
            .next()
            .unwrap_or_else(|| panic!("no Cancel button"))
            .click_accesskit();
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none(), "Cancel closes Grid Options");
        assert!(harness.state().ui.gridlines, "Cancel leaves the grid on");
        assert!((harness.state().session.prefs.grid_spacing - 6.0).abs() < 1e-9);
    }

    /// Preferences (#752): Cancel, Esc and the window's close button put back what the dialog
    /// changed while it was open; Done keeps it.
    #[test]
    fn preferences_cancel_restores_the_interface_language() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        let before = harness.state().ui.interface_language;
        harness.state_mut().run("app.preferences", json!({})).unwrap();
        harness.run_steps(2);
        // The language list applies a choice at once, as the other Preferences controls do.
        harness.state_mut().run("app.language", json!({"language": "pl"})).unwrap();
        harness.run_steps(2);
        assert_eq!(harness.state().ui.interface_language, i18n::Language::Pl);
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        let s = harness.state_mut();
        assert!(s.dialog.is_none(), "Esc closes Preferences");
        assert_eq!(s.ui.interface_language, before, "Esc restores the interface language");
        let _ = s.run("app.language", json!({"language": "en"}));
    }

    #[test]
    fn preferences_cancel_restores_the_settings_done_keeps_them() {
        use egui_kittest::kittest::Queryable;
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        let before = harness.state().session.prefs.clone();
        let dark_before = harness.state().ui.brightness;
        let change = |harness: &mut egui_kittest::Harness<'static, SlideApp>| {
            harness.state_mut().run("app.preferences", json!({})).unwrap();
            harness.run_steps(2);
            for label in ["Dark appearance", "Show smart guides", "Snap objects to grid"] {
                harness
                    .query_all_by_role_and_label(egui::accesskit::Role::CheckBox, label)
                    .next()
                    .unwrap_or_else(|| panic!("no {label} checkbox"))
                    .click_accesskit();
                harness.run_steps(2);
            }
            assert_ne!(harness.state().ui.brightness, dark_before, "the dialog previews the appearance");
            assert_ne!(harness.state().session.prefs.smart_guides, before.smart_guides);
        };
        let unchanged = |harness: &egui_kittest::Harness<'static, SlideApp>, how: &str| {
            let s = harness.state();
            assert!(s.dialog.is_none(), "{how} closes Preferences");
            assert_eq!(s.ui.brightness, dark_before, "{how} restores the appearance");
            assert_eq!(s.session.prefs.smart_guides, before.smart_guides, "{how} restores smart guides");
            assert_eq!(s.session.prefs.snap_to_grid, before.snap_to_grid, "{how} restores snap to grid");
        };

        change(&mut harness);
        harness
            .query_all_by_role_and_label(egui::accesskit::Role::Button, "Cancel")
            .next()
            .unwrap_or_else(|| panic!("no Cancel button"))
            .click_accesskit();
        harness.run_steps(2);
        unchanged(&harness, "Cancel");

        change(&mut harness);
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
        unchanged(&harness, "Esc");

        change(&mut harness);
        harness
            .query_all_by_role_and_label(egui::accesskit::Role::Button, "Done")
            .next()
            .unwrap_or_else(|| panic!("no Done button"))
            .click_accesskit();
        harness.run_steps(2);
        let s = harness.state();
        assert!(s.dialog.is_none(), "Done closes Preferences");
        assert_ne!(s.ui.brightness, dark_before, "Done keeps the appearance");
        assert_ne!(s.session.prefs.smart_guides, before.smart_guides, "Done keeps smart guides");
        assert_ne!(s.session.prefs.snap_to_grid, before.snap_to_grid, "Done keeps snap to grid");
    }

    fn canvas_harness() -> egui_kittest::Harness<'static, SlideApp> {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        let app = harness.state_mut();
        // Zoomed out, the canvas shows plenty of pasteboard around the slide.
        app.ui.zoom = Some(0.4);
        app.session.prefs.snap_to_grid = false;
        app.session.prefs.smart_guides = false;
        harness
    }

    fn selected(app: &SlideApp) -> Vec<u64> {
        app.session.active().unwrap().selection.shapes.iter().map(|i| i.0 as u64).collect()
    }

    fn shape_x(app: &SlideApp, id: u64) -> f64 {
        let st = app.session.active().unwrap();
        st.shape(deckcraft_model::ShapeId(id as u32)).and_then(|s| s.xfrm).unwrap().x
    }

    /// #270: PowerPoint's Normal view draws objects off the slide on the area around it, where
    /// they can be selected and moved.
    #[test]
    fn off_slide_objects_show_on_the_pasteboard_and_can_be_selected_and_moved() {
        let mut harness = canvas_harness();
        let off =
            harness.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [1000.0, 100.0, 120.0, 80.0]})).unwrap()["id"]
                .as_u64()
                .unwrap();
        harness.state_mut().session.execute("edit.select", &json!({"ids": []})).unwrap();
        harness.run_steps(3);

        let app = harness.state();
        let (slide, region, k) = (app.slide_rect.unwrap(), app.canvas_region.unwrap(), app.canvas_scale);
        // The texture reaches past the slide's right edge to the shape's, and no further left/up.
        assert!(region.max.x >= slide.min.x + 1120.0 * k - 1.0, "{region:?} {slide:?}");
        assert!((region.min.x - slide.min.x).abs() < 1.0 && (region.min.y - slide.min.y).abs() < 1.0, "{region:?} {slide:?}");
        let centre = slide.min + egui::vec2(1060.0 * k, 140.0 * k);
        assert!(app.canvas_rect.unwrap().contains(centre));

        // Drawn at full strength on the pasteboard (when a GPU renderer is available here).
        if let Ok(img) = harness.render() {
            let px = |p: egui::Pos2| img.get_pixel(p.x as u32, p.y as u32).0;
            let shape = px(centre);
            let board = px(slide.min + egui::vec2(1060.0 * k, 400.0 * k));
            let doc = &harness.state().session.active().unwrap().doc;
            let accent = doc.masters[0].theme.colors.get(deckcraft_color::SchemeSlot::Accent1);
            let close = |a: u8, b: u8| (a as i32 - b as i32).abs() <= 4;
            assert!(close(shape[0], accent.r) && close(shape[1], accent.g) && close(shape[2], accent.b), "{shape:?} vs {accent:?}");
            assert_ne!(shape, board);
        }

        // Click selects it.
        harness.hover_at(centre);
        harness.run_steps(1);
        harness.drag_at(centre);
        harness.run_steps(1);
        harness.drop_at(centre);
        harness.run_steps(2);
        assert_eq!(selected(harness.state()), vec![off]);

        // Drag moves it, further off the slide.
        let to = centre + egui::vec2(40.0, 0.0);
        harness.hover_at(centre);
        harness.run_steps(1);
        harness.drag_at(centre);
        harness.run_steps(1);
        for i in 1..=4 {
            harness.hover_at(centre + egui::vec2(10.0 * i as f32, 0.0));
            harness.run_steps(1);
        }
        harness.drop_at(to);
        harness.run_steps(2);
        let x = shape_x(harness.state(), off);
        assert!((x - (1000.0 + 40.0 / k as f64)).abs() < 1.5, "{x}");
        // The pasteboard followed it.
        let (slide, region) = (harness.state().slide_rect.unwrap(), harness.state().canvas_region.unwrap());
        assert!(region.max.x >= slide.min.x + (x as f32 + 120.0) * k - 1.0, "{region:?}");

        // Idle frames don't render the canvas again.
        let n = harness.state().textures.canvas_renders;
        harness.run_steps(3);
        assert_eq!(harness.state().textures.canvas_renders, n);
    }

    /// A slide with nothing off it renders just the slide, as before.
    #[test]
    fn the_canvas_renders_only_the_slide_when_nothing_is_off_it() {
        let mut harness = canvas_harness();
        harness.run_steps(3);
        let app = harness.state();
        let (slide, region) = (app.slide_rect.unwrap(), app.canvas_region.unwrap());
        assert!((region.min - slide.min).length() < 0.5 && (region.max - slide.max).length() < 1.0, "{region:?} {slide:?}");
        let n = app.textures.canvas_renders;
        harness.run_steps(3);
        assert_eq!(harness.state().textures.canvas_renders, n, "an unchanged slide isn't rendered again");
    }

    /// #431: the ribbon's Copy and Cut, the slide's context menu, the menus and the control
    /// channel all run the commands; they put the text on the system clipboard, as Ctrl+C and
    /// Ctrl+X do.
    #[test]
    fn copy_and_cut_commands_reach_the_system_clipboard() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        let copied = |h: &egui_kittest::Harness<'_, SlideApp>| {
            h.output().platform_output.commands.iter().find_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
        };
        let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
        harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
        harness.state_mut().session.type_text("Aurora").unwrap();
        harness.state_mut().session.key("Escape", Mods::default()).unwrap();
        harness.run_steps(1);
        assert_eq!(copied(&harness), None);

        harness.state_mut().run("edit.copy", json!({})).unwrap();
        harness.run_steps(1);
        assert_eq!(copied(&harness).as_deref(), Some("Aurora"), "Copy puts the text on the clipboard");
        harness.run_steps(1);
        assert_eq!(copied(&harness), None, "only once");

        harness.state_mut().run("edit.cut", json!({})).unwrap();
        harness.run_steps(1);
        assert_eq!(copied(&harness).as_deref(), Some("Aurora"), "Cut does too");
        assert!(harness.state().session.active().unwrap().doc.slides[0].shapes.iter().all(|s| s.id.0 != title), "and removes the shape");
    }

    /// #182: Ctrl+C then Ctrl+V did nothing for objects without text (a rectangle), because
    /// egui-winit drops Ctrl+V when the system clipboard is empty; the context menu worked.
    #[test]
    fn copy_then_paste_shortcut_duplicates_an_object_without_text() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        harness.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap();
        let count = |h: &egui_kittest::Harness<'_, SlideApp>| h.state().session.doc().unwrap().current_slide().unwrap().shapes.len();
        let before = count(&harness);

        // What the copy leaves on the system clipboard is what egui-winit hands back on Ctrl+V.
        assert_eq!(system_clipboard_text(""), OBJECTS_MARKER);
        assert_eq!(system_clipboard_text("hello"), "hello");
        harness.event(egui::Event::Copy);
        harness.run_steps(2);
        harness.event(egui::Event::Paste(OBJECTS_MARKER.to_string()));
        harness.run_steps(2);
        assert_eq!(count(&harness), before + 1, "the shortcut pastes a copy of the rectangle");
        let d = harness.state().session.active().unwrap();
        let pasted = d.doc.slides[0].shapes.last().unwrap();
        assert!(pasted.text.as_ref().is_none_or(|t| t.text().is_empty()), "a copy of the rectangle, not a text box holding the marker");
    }

    /// #188: Ctrl/Cmd+A with the slide thumbnail pane focused selects every slide; with the
    /// canvas focused it still selects shapes.
    #[test]
    fn select_all_follows_the_focused_pane() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        for _ in 0..2 {
            harness.state_mut().run("slide.new", json!({})).unwrap();
        }
        harness.state_mut().run("slide.go", json!({"index": 0})).unwrap();
        harness.run_steps(3);
        let click = |h: &mut egui_kittest::Harness<'_, SlideApp>, pos: egui::Pos2| {
            h.event(egui::Event::PointerMoved(pos));
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
            h.run_steps(2);
        };
        // Press in the thumbnail pane, then Cmd+A.
        let w = harness.state().ui.thumbs_width;
        click(&mut harness, egui::pos2(w / 2.0, 300.0));
        assert!(harness.state().slides_focused);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run_steps(2);
        let st = harness.state().session.active().unwrap();
        assert_eq!(st.selection.slides.len(), 3, "every slide is selected");

        // A press on the canvas moves focus away: Cmd+A selects shapes again.
        click(&mut harness, egui::pos2(700.0, 400.0));
        assert!(!harness.state().slides_focused);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run_steps(2);
        let st = harness.state().session.active().unwrap();
        assert!(!st.selection.shapes.is_empty(), "canvas select all picks the shapes");
    }

    fn harness() -> egui_kittest::Harness<'static, SlideApp> {
        egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::new(), Services::default()),
        )
    }

    /// #172: a recent file that no longer exists is marked on File › Home, and clicking it draws
    /// the open error right there (the status bar is hidden while the backstage is open, so the
    /// message used to vanish).
    #[test]
    fn missing_recent_is_marked_and_its_error_shows() {
        let mut harness = harness();
        let missing = "/deckcraft-tests/172/missing.deckcraft";
        harness.state_mut().ui.recent = vec![missing.to_string()];
        harness.run_steps(2);
        // The row is labelled with the file's name. Clicking it raises the open error, and the
        // drawn message is a second node that mentions the name (the OS text after it varies).
        harness.get_by_label("missing.deckcraft").click();
        harness.run_steps(2);
        // The file is read in the background (#350): wait for that read to finish.
        for _ in 0..500 {
            if harness.state().opening.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            harness.run_steps(1);
        }
        harness.run_steps(2);
        assert!(harness.state().toast.is_some(), "the open error was raised");
        assert!(harness.get_all_by_label_contains("missing.deckcraft").count() >= 2, "the open error is drawn on the page, next to the entry");
    }

    /// #172: a Recent entry can be removed from the list.
    #[test]
    fn recent_entry_can_be_removed() {
        let mut harness = harness();
        harness.state_mut().ui.recent = vec!["/deckcraft-tests/172/a.deckcraft".into(), "/deckcraft-tests/172/b.deckcraft".into()];
        harness.run_steps(2);
        let removes = harness.get_all_by_label("Remove from Recent").collect::<Vec<_>>();
        assert_eq!(removes.len(), 2);
        // The remove button must sit inside the visible part of the row even when the page's
        // content is wider than the window.
        assert!(removes[1].rect().right() < 1200.0, "reachable in the 1200-wide window: {:?}", removes[1].rect());
        removes[1].click();
        harness.run_steps(1);
        assert_eq!(harness.state().ui.recent, vec!["/deckcraft-tests/172/a.deckcraft"]);
    }

    /// The existence probe tells present files from missing ones and never touches remote paths.
    #[test]
    fn recent_exists_tells_missing_from_present() {
        let mut app = SlideApp::new(Session::new(), Services::default());
        let mut path = std::env::temp_dir();
        path.push(format!("deckcraft-test-172-{}.deckcraft", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        assert!(app.recent_exists(&path.to_string_lossy()), "the file exists");
        assert!(!app.recent_exists("/deckcraft-tests/172/missing.deckcraft"));
        assert!(app.recent_exists("\\\\server/share/deck.deckcraft"), "UNC paths are never probed");
        std::fs::remove_file(&path).ok();
    }

    /// #339: clicking a language in Review > Language did nothing, and OK didn't mark the text.
    #[test]
    fn the_language_dialog_marks_the_selected_text() {
        use egui_kittest::kittest::Queryable;
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
        harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
        harness.state_mut().session.execute("text.insert", &json!({"text": "Bonjour"})).unwrap();
        harness.state_mut().session.execute("text.selectAll", &json!({})).unwrap();
        harness.state_mut().run("app.dialog", json!({"id": "language"})).unwrap();
        // The anchored window settles over its first frames; click once it stays put.
        harness.run_steps(4);
        harness.get_by_label("Français (France)").click();
        harness.run_steps(2);
        harness.get_by_label("OK").click();
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none());
        let d = harness.state().session.active().unwrap();
        let body = d.doc.slides[0].shapes[0].text.as_ref().unwrap();
        assert_eq!(body.paragraphs[0].runs[0].props.lang.as_deref(), Some("fr-FR"));
    }

    /// Speaker notes keep their formatting when edited in the notes pane: typing into a bold
    /// heading, Enter in a bulleted point, then saving as .pptx and reopening.
    #[test]
    fn editing_notes_in_the_pane_keeps_their_formatting() {
        use deckcraft_model::text::{Bullet, ParaProps, Paragraph, Run, RunProps, TextBody};
        let point = |runs: Vec<Run>| Paragraph {
            level: 1,
            props: ParaProps {
                margin_left: Some(22.5),
                indent: Some(-22.5),
                bullet: Some(Bullet::Char { char: "§".into() }),
                bullet_font: Some("Wingdings".into()),
                ..Default::default()
            },
            runs,
            ..Default::default()
        };
        let bold = RunProps { bold: Some(true), ..Default::default() };
        let notes = TextBody {
            paragraphs: vec![
                Paragraph { runs: vec![Run::with("Metodický prehľad", bold.clone())], ..Default::default() },
                Paragraph::new("Žiaci pracujú vo dvojiciach."),
                point(vec![Run::with("Cieľ: ", bold.clone()), Run::new("vysvetliť cyklus")]),
                point(vec![Run::new("Reflexia")]),
                Paragraph::new("Pomôcky: počítač"),
            ],
            ..Default::default()
        };
        let mut session = Session::with_new();
        session.execute("slide.notes", &json!({"index": 0, "body": notes})).unwrap();
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(session, Services::default()),
        );
        harness.run_steps(2);
        let id = egui::Id::new("notes_edit");
        let caret = |h: &mut egui_kittest::Harness<'_, SlideApp>, at: usize| {
            let mut st = egui::TextEdit::load_state(&h.ctx, id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
            egui::TextEdit::store_state(&h.ctx, id, st);
        };
        let chars = |s: &str, of: &str| s.find(of).map(|b| s[..b].chars().count() + of.chars().count()).unwrap();
        let text = notes.text();
        harness.ctx.memory_mut(|m| m.request_focus(id));
        harness.run_steps(1);
        // Type into the bold heading.
        caret(&mut harness, chars(&text, "prehľad"));
        harness.run_steps(1);
        harness.event(egui::Event::Text(" hodiny".into()));
        harness.run_steps(1);
        // Enter after a bulleted point, and type the next one.
        let text = text.replacen("prehľad", "prehľad hodiny", 1);
        caret(&mut harness, chars(&text, "cyklus"));
        harness.run_steps(1);
        harness.key_press(egui::Key::Enter);
        harness.event(egui::Event::Text("Nový bod".into()));
        harness.run_steps(1);
        // Leave the field: the notes are saved.
        harness.ctx.memory_mut(|m| m.surrender_focus(id));
        harness.run_steps(2);
        let check = |n: &TextBody| {
            assert_eq!(
                n.text(),
                "Metodický prehľad hodiny\nŽiaci pracujú vo dvojiciach.\nCieľ: vysvetliť cyklus\nNový bod\nReflexia\nPomôcky: počítač"
            );
            assert_eq!(n.paragraphs[0].runs.len(), 1, "{n:?}");
            assert_eq!(n.paragraphs[0].runs[0].props.bold, Some(true));
            assert_eq!(n.paragraphs[1].props, ParaProps::default());
            assert_eq!(n.paragraphs[2].runs[0].props.bold, Some(true), "the bold label");
            assert_eq!(n.paragraphs[2].runs[1].props.bold, None);
            for i in [2, 3, 4] {
                assert_eq!((n.paragraphs[i].level, &n.paragraphs[i].props), (1, &notes.paragraphs[2].props), "point {i}");
            }
            assert_eq!((n.paragraphs[5].level, &n.paragraphs[5].props), (0, &ParaProps::default()));
        };
        check(&harness.state().session.active().unwrap().doc.slides[0].notes);
        // Saved as .pptx and reopened.
        let s = &mut harness.state_mut().session;
        let data = s.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].clone();
        s.execute("file.openBytes", &json!({"name": "notes.pptx", "data": data})).unwrap();
        check(&s.active().unwrap().doc.slides[0].notes);
    }

    fn step_app(services: Services) -> egui_kittest::Harness<'static, SlideApp> {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), services),
        );
        h.run_steps(2);
        h
    }

    /// Copy of a bold selection offers HTML; paste of Pages-like HTML keeps the bold.
    #[test]
    fn copy_offers_html_and_paste_keeps_bold() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let seen = Rc::new(RefCell::new(None));
        let record = seen.clone();
        let services = Services {
            clipboard_set_html: Some(Box::new(move |html, plain| {
                *record.borrow_mut() = Some((html.to_string(), plain.to_string()));
                true
            })),
            ..Default::default()
        };
        let mut h = step_app(services);
        let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
        h.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
        h.event(egui::Event::Text("Hi".into()));
        h.run_steps(1);
        h.state_mut().session.execute("text.selectAll", &json!({})).unwrap();
        h.state_mut().session.execute("format.bold", &json!({"on": true})).unwrap();
        h.event(egui::Event::Copy);
        h.run_steps(1);
        let (html, plain) = seen.borrow().clone().expect("html offered");
        assert!(html.contains("<b>"), "{html}");
        assert_eq!(plain, "Hi");
        assert!(!h.output().platform_output.commands.iter().any(|c| matches!(c, egui::output::OutputCommand::CopyText(_))));

        let services = Services { clipboard_html: Some(Box::new(|| Some("<p><b>Hi</b></p>".into()))), ..Default::default() };
        let mut h = step_app(services);
        h.event(egui::Event::Paste("Hi".into()));
        h.run_steps(1);
        let st = h.state().session.active().unwrap();
        let body = st
            .current_slide()
            .unwrap()
            .shapes
            .iter()
            .find(|sh| sh.text.as_ref().is_some_and(|t| t.text() == "Hi"))
            .and_then(|sh| sh.text.clone())
            .expect("pasted text");
        assert_eq!(body.paragraphs[0].runs[0].props.bold, Some(true), "{body:?}");
    }

    /// A shape with no text copies out as the objects marker (not a picture), so Ctrl+V can
    /// still paste it here (#182).
    #[test]
    fn a_textless_shape_copies_as_the_objects_marker() {
        let mut h = step_app(Services::default());
        h.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 120.0, 80.0]})).unwrap();
        h.event(egui::Event::Copy);
        h.run_steps(1);
        let cmds = &h.output().platform_output.commands;
        assert!(cmds.iter().any(|c| matches!(c, egui::output::OutputCommand::CopyText(t) if t == OBJECTS_MARKER)), "{cmds:?}");
    }

    fn redder(px: [u8; 4]) -> i32 {
        px[0] as i32 - px[1].max(px[2]) as i32
    }

    /// A fade entrance is hidden at the start of the preview clock and opaque once it has played.
    #[test]
    fn preview_hides_an_entrance_until_it_plays() {
        let mut session = Session::with_new();
        let text = session.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 220.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
        session.execute("text.set", &json!({"id": text, "text": "Alpha\nBeta"})).unwrap();
        session.execute("animation.set", &json!({"effect": "fade", "duration": 1000})).unwrap();
        session.execute("animation.options", &json!({"textBuild": "byParagraph"})).unwrap();
        let doc = session.active().unwrap().doc.clone();
        let tl = deckcraft_anim::ShowState::timeline(&doc, 0);
        let shape = deckcraft_model::ShapeId(text as u32);
        let para = |time: f64, i: usize| preview::shape_state_at(&tl, shape, time).paras.into_iter().find(|(p, _)| *p == i).map(|(_, s)| s).unwrap();
        assert!(!para(0.0, 0).visible && !para(0.0, 1).visible, "entrances have not started");
        let mid = para(0.25, 0);
        assert!(mid.visible && mid.opacity > 0.05 && mid.opacity < 0.95, "{mid:?}");
        assert!(!para(0.25, 1).visible, "the next paragraph waits for its step");

        session.execute("animation.clear", &json!({})).unwrap();
        let id = session.execute("shape.insert", &json!({"preset": "rect", "rect": [400.0, 300.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
        session.execute("shape.fill", &json!({"id": id, "color": "#CC2200"})).unwrap();
        session.execute("animation.set", &json!({"effect": "fade", "duration": 1000})).unwrap();
        let doc = session.active().unwrap().doc.clone();
        let tl = deckcraft_anim::ShowState::timeline(&doc, 0);
        let (w, h) = (doc.slide_size.width.round().max(1.0) as u32, doc.slide_size.height.round().max(1.0) as u32);
        let paint = |time: Option<f64>| {
            let f = |sid: deckcraft_model::ShapeId| preview::shape_state_at(&tl, sid, time.unwrap_or(0.0));
            let state: Option<&dyn Fn(deckcraft_model::ShapeId) -> deckcraft_render::ShapeState> = if time.is_some() { Some(&f) } else { None };
            let opts = deckcraft_render::RenderOpts { scale: 1.0, size: Some((w, h)), state, ..Default::default() };
            deckcraft_render::render_slide(&doc, 0, &opts).pixel(460, 340)
        };
        let rest = paint(None);
        let start = paint(Some(0.0));
        let end = paint(Some(tl.total_duration() + 0.05));
        assert!(redder(rest) > 40, "the shape is filled at rest {rest:?}");
        assert!(redder(start) < redder(rest) / 2, "hidden at the first preview frame {start:?} vs {rest:?}");
        assert!(redder(end) > 40, "visible again once the sequence has played {end:?}");
    }

    #[test]
    fn animation_preview_plays_on_the_canvas_then_stops() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(0.05).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        let none = harness.state_mut().run("animation.preview", json!({})).unwrap();
        assert_eq!(none["playing"], false);
        assert!(harness.state().status.as_ref().is_some_and(|s| s.0.contains("no animations")));
        assert!(harness.state().show.is_none());

        let id = harness.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [200.0, 280.0, 180.0, 80.0]})).unwrap()["id"]
            .as_u64()
            .unwrap();
        harness.state_mut().session.execute("animation.set", &json!({"effect": "fade", "duration": 400})).unwrap();
        let started = harness.state_mut().run("animation.preview", json!({})).unwrap();
        assert_eq!(started["playing"], true);
        assert!(harness.state().show.is_none(), "preview stays on the editor canvas");
        harness.run_steps(1);
        let elapsed = harness.state().anim_preview.as_ref().map(|p| p.elapsed).unwrap();
        assert!(elapsed < 0.02, "the clock latches on the first frame, got {elapsed}");
        let renders = harness.state().textures.canvas_renders;
        harness.run_steps(6);
        let elapsed = harness.state().anim_preview.as_ref().map(|p| p.elapsed).unwrap();
        assert!((0.2..0.5).contains(&elapsed), "{elapsed}");
        assert!(harness.state().textures.canvas_renders > renders, "each preview frame redraws the slide");
        assert!(harness.state().show.is_none());

        harness.key_press(egui::Key::Escape);
        harness.run_steps(1);
        assert!(harness.state().anim_preview.is_none(), "Escape stops the preview");

        harness.state_mut().run("animation.preview", json!({})).unwrap();
        harness.run_steps(1);
        assert!(harness.state().anim_preview.is_some());
        harness.state_mut().run("animation.preview", json!({"on": false})).unwrap();
        assert!(harness.state().anim_preview.is_none());

        harness.state_mut().run("animation.preview", json!({})).unwrap();
        harness.run_steps(1);
        let centre = harness.state().slide_rect.unwrap().center();
        harness.hover_at(centre);
        harness.run_steps(1);
        harness.drag_at(centre);
        harness.run_steps(2);
        assert!(harness.state().anim_preview.is_none(), "a click on the slide stops the preview");

        harness.state_mut().run("animation.preview", json!({})).unwrap();
        harness.run_steps(1);
        harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
        harness.run_steps(1);
        assert!(harness.state().anim_preview.is_none(), "leaving the slide stops the preview");

        harness.state_mut().session.execute("slide.previous", &json!({})).unwrap();
        harness.state_mut().session.execute("edit.select", &json!({"ids": [id]})).unwrap();
        harness.state_mut().run("animation.preview", json!({})).unwrap();
        harness.run_steps(1);
        assert!(harness.state().anim_preview.is_some());
        // 400 ms effect plus the hold.
        harness.run_steps(30);
        assert!(harness.state().anim_preview.is_none(), "the preview ends after the sequence");
        assert!(harness.state().show.is_none());
    }

    fn flick_harness() -> egui_kittest::Harness<'static, SlideApp> {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        // Fitted: the slide is smaller than the window, so a flick turns the page instead of panning.
        harness.state_mut().ui.zoom = Some(0.4);
        harness
    }

    fn slide_at(app: &SlideApp) -> usize {
        app.session.active().unwrap().selection.slide
    }

    /// One frame of a trackpad flick: four short samples, each under egui's cutoff for smoothing a
    /// mouse notch. The harness runs one frame per queued event, so they have to share a frame or
    /// none of them reaches the page threshold.
    fn flick(harness: &mut egui_kittest::Harness<'static, SlideApp>, delta: egui::Vec2) {
        let centre = harness.state().canvas_rect.unwrap_or(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))).center();
        let input = harness.input_mut();
        input.events.push(egui::Event::PointerMoved(centre));
        let step = delta / 4.0;
        for _ in 0..4 {
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: step,
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run_steps(1);
    }

    #[test]
    fn page_flick_uses_the_stronger_axis() {
        assert_eq!(crate::canvas::page_flick(egui::vec2(-24.0, -4.0)), Some(true));
        assert_eq!(crate::canvas::page_flick(egui::vec2(24.0, 4.0)), Some(false));
        assert_eq!(crate::canvas::page_flick(egui::vec2(-4.0, -24.0)), Some(true));
        assert_eq!(crate::canvas::page_flick(egui::vec2(4.0, 10.0)), None);
    }

    /// A sideways swipe on a fitted slide turns the page, like PowerPoint on a Mac trackpad.
    /// The frames of one gesture do not skip ahead.
    #[test]
    fn a_sideways_trackpad_swipe_turns_the_page_and_a_second_flick_waits() {
        let mut harness = flick_harness();
        harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
        harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
        harness.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
        harness.run_steps(1);
        assert_eq!(slide_at(harness.state()), 0);

        flick(&mut harness, egui::vec2(-24.0, 0.0));
        assert_eq!(slide_at(harness.state()), 1, "swipe left goes to the next slide");

        flick(&mut harness, egui::vec2(-24.0, 0.0));
        assert_eq!(slide_at(harness.state()), 1, "the next frame of the same gesture does not skip a slide");

        harness.run_steps(30);
        flick(&mut harness, egui::vec2(24.0, 0.0));
        assert_eq!(slide_at(harness.state()), 0, "swipe right goes back");
    }

    #[test]
    fn a_vertical_wheel_still_turns_the_page_and_a_zoomed_slide_pans_instead() {
        let mut harness = flick_harness();
        harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
        harness.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
        harness.run_steps(1);
        flick(&mut harness, egui::vec2(-2.0, -24.0));
        assert_eq!(slide_at(harness.state()), 1, "a vertical wheel still goes to the next slide");

        harness.state_mut().ui.zoom = Some(3.0);
        harness.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
        harness.run_steps(2);
        flick(&mut harness, egui::vec2(-24.0, 0.0));
        assert_eq!(slide_at(harness.state()), 0, "zoomed in, a sideways swipe pans instead of turning the page");
    }

    #[test]
    fn a_swipe_during_the_show_turns_the_page() {
        let mut harness = flick_harness();
        harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
        harness.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
        harness.state_mut().start_show(0, true);
        harness.run_steps(2);
        flick(&mut harness, egui::vec2(-24.0, 0.0));
        assert_eq!(harness.state().show.as_ref().map(|s| s.state.slide), Some(1));
        harness.run_steps(30);
        flick(&mut harness, egui::vec2(24.0, 0.0));
        assert_eq!(harness.state().show.as_ref().map(|s| s.state.slide), Some(0));
    }

    /// The pen is drawing. A flick must not turn the page out from under the stroke.
    #[test]
    fn the_pen_ignores_a_swipe_during_the_show() {
        let mut harness = flick_harness();
        harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
        harness.state_mut().start_show(0, true);
        harness.run_steps(2);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::P);
        harness.run_steps(1);
        flick(&mut harness, egui::vec2(-24.0, 0.0));
        assert_eq!(harness.state().show.as_ref().map(|s| s.state.slide), Some(0));
    }

    /// Preferences › Interface size (#767): dragging the slider leaves the interface size alone
    /// until the pointer lets go, so the slider doesn't grow under the pointer mid-drag.
    #[test]
    fn interface_size_applies_when_the_slider_is_let_go() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(2);
        harness.state_mut().run("app.preferences", json!({})).unwrap();
        harness.run_steps(4);
        let before = harness.state().ui.ui_scale;
        let rect =
            harness.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Interface size").next().expect("the Interface size slider").rect();
        let (from, to) = (rect.left_center() + egui::vec2(4.0, 0.0), rect.right_center() - egui::vec2(4.0, 0.0));
        harness.hover_at(from);
        harness.run_steps(1);
        harness.drag_at(from);
        harness.run_steps(1);
        for k in 1..=8 {
            harness.hover_at(from.lerp(to, k as f32 / 8.0));
            harness.run_steps(1);
        }
        assert_eq!(harness.state().ui.ui_scale, before, "nothing rescales while the slider is held");
        harness.drop_at(to);
        harness.run_steps(2);
        let after = harness.state().ui.ui_scale;
        assert!(after > before + 0.3, "letting go applies the size dragged to: {before} -> {after}");
        assert!(harness.state().dialog.is_some(), "Preferences stays open");
    }
}
