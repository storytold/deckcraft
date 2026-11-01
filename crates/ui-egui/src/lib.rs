//! DeckCraft's egui frontend: a PowerPoint-style UI over `deckcraft-engine`.
//!
//! The UI is thin: every action goes through [`SlideApp::run`], which handles UI commands
//! (views, panes, zoom) here and sends everything else to the engine. The ribbon, menus,
//! shortcuts, the command palette and the control channel all share that entry point.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// DeckCraft as craft-update knows it, with its 128 px icon and accent colour: the update
/// prompt shows both, and an AppImage's menu entry gets the icon.
#[cfg(not(target_arch = "wasm32"))]
pub const UPDATE_APP: craft_update::app::App = craft_update::app::App {
    icon_png: Some(include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.deckcraft.png")),
    accent: Some([0xf0, 0x7a, 0x1e]),
    ..craft_update::suite::DECKCRAFT
};

pub mod backstage;
pub mod canvas;
pub mod capture;
pub mod control;
pub mod dialogs;
pub mod doc_window;
pub mod eyedropper;
pub mod icons;
pub mod media;
pub mod menus;
pub mod opening;
pub mod ribbon;
pub mod settings;
pub mod show;
pub mod sorter;
pub mod status;
pub mod textures;
pub mod theme;
pub mod toast;
pub mod uifonts;
pub mod widgets;

use std::collections::HashMap;
use std::sync::mpsc::Receiver;

use deckcraft_engine::{Mods, Session, Target, UiRequest};
use deckcraft_model::{LayoutId, MasterId, SlideId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse};

mod app;
mod clock;
mod ime_hooks;
pub mod panels;
mod services;
mod state;

pub use app::key_name;
#[cfg(test)]
use app::{OBJECTS_MARKER, system_clipboard_text, web_image_files};
use canvas::preview;
pub use canvas::squiggles;
pub use clock::{monotonic_secs, now_ms};
pub use dialogs::credits;
#[cfg(not(target_arch = "wasm32"))]
use dialogs::updates_ui;
pub use ime_hooks::install_ime_pass_hooks;
pub use panels as panes;
pub use panels::animtimeline;
pub use panels::fillui;
pub use panels::notes;
pub use panels::selpane;
pub use panels::thumbs;
pub use ribbon::qat;
pub use services::{DownloadFn, Inbox, InboxEvent, InboxFile, OpenAsyncFn, PickFn, ReadFn, Services, WriteFn};
use state::{FilePlace, FilePurpose, ImeDescriptorKey, ImeDestination, ImeTerminalStamp};
pub use state::{RECENT_MAX, UiState, ViewMode, ZOOM_RANGE};

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
    (
        "show.presenter",
        "Presenter View",
        None,
        "{on?: bool} → {on, window} (presenter view, with notes, a clock and the next slide, appears in its own separate window beside the show window when the show starts; window is false in the browser build, which draws it inside the page)",
    ),
    ("show.keepCursor", "Keep Mouse Cursor Visible", None, "{on?: bool}"),
    ("show.end", "End Show", None, "{}"),
    ("animation.preview", "Preview Animations", None, "{on?: bool, from?: effect index}"),
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
    ("app.checkUpdates", "Check for Updates…", None, "{}"),
    ("app.palette", "Command Palette", Some("Cmd+Shift+P"), "{}"),
    ("app.preferences", "Preferences…", Some("Cmd+,"), "{}"),
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
    /// Where on the play-everything clock playback began (Play From the selected effect).
    from: f64,
    elapsed: f64,
    timeline: deckcraft_anim::Timeline,
}

/// The app state a queued synthetic control-channel input is meant for.
///
/// A batch is injected only while this identity is unchanged. eframe carries a raw-input batch
/// from a logic-only (hidden) pass into the next full pass, so the hook also keeps the exact event
/// list it appended and retracts it when the identity no longer matches: the stale click never
/// reaches the new document, slide or canvas layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InputTarget {
    documents: usize,
    active: Option<u64>,
    slide: Option<usize>,
    view: ViewMode,
    zoom: Option<u32>,
    canvas: Option<[u32; 4]>,
    canvas_scale: u32,
    /// Dialog, palette or Backstage view over the canvas. A modal transition changes what a
    /// queued window-point click means even when the document and canvas layout are unchanged.
    dialog: Option<egui::Id>,
    /// Command palette query *and* highlighted entry: the keyboard target moves with the
    /// selection even when the query does not.
    palette: Option<egui::Id>,
    backstage: Option<egui::Id>,
    /// Active ribbon tab, whether the ribbon is collapsed, and its horizontal scroll offset.
    /// A window-point click means a different button when any of them changes.
    ribbon_tab: egui::Id,
    ribbon_collapsed: bool,
    ribbon_scroll: u32,
    /// Full-screen slide show: the show instance, slide, step and the layout/mode state that
    /// changes what a click at the same window point does.
    show: Option<show::ShowInputState>,
    /// Background-open progress window, identified by the file *and* the attempt.
    opening: Option<(egui::Id, u64)>,
}

pub struct SlideApp {
    /// Shape Fill's last colour (a command colour parameter): the button applies it again. Accent 1 until one is picked.
    pub last_fill: serde_json::Value,
    /// Shape Outline's last colour, likewise. Text 1 until one is picked.
    pub last_outline: serde_json::Value,
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
    /// Where the Reuse Slides picker inserts: the presentation and slide current when it opened.
    reuse_target: Option<FileTarget>,
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
    pub(crate) synthetic: Vec<egui::Event>,
    /// A synthetic batch has been injected but the UI pass has not acknowledged it. While this
    /// is set no second batch is injected, so a hidden/discarded frame cannot replay an event.
    synthetic_pending: bool,
    /// Events queued while an older batch is still pending. They belong to the target at the time
    /// they were queued, so a later document change cannot make them fall in behind the old batch
    /// and get cleared with it.
    synthetic_next: Vec<egui::Event>,
    synthetic_next_target: Option<InputTarget>,
    /// The exact events appended to the pending `RawInput`; retracted on a target change.
    synthetic_injected: Vec<egui::Event>,
    /// Where `synthetic_injected` was appended in `RawInput::events`. Retraction uses this
    /// index so an identical host event earlier in the batch is never removed instead.
    synthetic_injected_at: Option<usize>,
    /// Identity of the last injected synthetic batch (for diagnostics).
    synthetic_generation: u64,
    /// Document/slide/canvas identity the queued/injected batch belongs to.
    synthetic_target: Option<InputTarget>,
    /// Focus identity the pending batch was injected with; a focus change retracts it.
    synthetic_focus: Option<egui::Id>,
    /// `RawInput::time` of the pass the batch was injected into. A logic-only callback leaves
    /// `InputState` from the previous pass; comparing this value keeps retraction from draining
    /// an unrelated batch out of that stale input.
    synthetic_injected_time: Option<f64>,
    /// Horizontal scroll offset (as `f32` bits) of each ribbon tab, recorded for
    /// [`InputTarget`]. Scroll state is per tab in egui, so the identity is too.
    ribbon_scroll: HashMap<String, u32>,
    /// Monotonic identity for a new slide show instance (`start_show`).
    show_instance: u64,
    /// Monotonic identity for a new background-open attempt.
    opening_generation: u64,
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
    /// The window title last sent (`ViewportCommand::Title`), so it is sent only when it changes.
    sent_title: String,
    /// The File tab's full-window view (Backstage). Over a presentation it replaces the ribbon,
    /// slides and status bar until Esc or Back; with nothing open it is the start screen. Opening,
    /// creating or closing a presentation leaves it.
    pub backstage: Option<backstage::Backstage>,
    pending_urls: Vec<String>,
    print_previews: app::files::PrintPreviews,
    #[cfg(not(target_arch = "wasm32"))]
    updates_rt: updates_ui::Runtime,
    /// Text for the system clipboard from a Copy or Cut command, handed over on the next frame.
    pending_copy: Option<String>,
    /// Bounded screenshot completion slots for the control channel (egui 0.37 callbacks).
    capture: capture::Capture,
    styled: bool,
    restyle: bool,
    fonts_ready: bool,
    last_time: f64,
    /// When AutoRecover data was last written (monotonic seconds, not wall time).
    pub last_recovery: f64,
    /// Quit was confirmed (changes saved or discarded): let the window close.
    pub quit_confirmed: bool,
    pub(crate) notes_buf: (u64, usize, notes::Draft),
    /// Faces borrowed from the font database for text the interface fonts can't draw (#101).
    pub ui_fonts: uifonts::UiFonts,
    /// Slide drag in the thumbnail pane or the Slide Sorter: the dragged slides and the drop gap.
    pub(crate) thumb_drag: Option<thumbs::SlideDrag>,
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
    /// Owner/generation/cause bookkeeping for the canvas text IME producer.
    ime_tracker: egui::ime_context::ImeContextTracker,
    /// Canvas IME geometry for this pass: the caret while editing, or the selected shape.
    ime_pending: Option<egui::Rect>,
    /// The viewport this UI pass belongs to (not the last immediate child viewport drawn).
    ime_frame_viewport: Option<egui::ViewportId>,
    /// Viewport whose tracker state is current. Closing the editing window or the last
    /// presentation releases it (`ImeContextTracker::end_viewport`), so a later owner in that
    /// viewport starts from a fresh context.
    pub(crate) ime_viewport: Option<egui::ViewportId>,
    /// The platform composition we know about: the owner its pre-edit began in and whether the
    /// application has since settled it. See `ime_event`.
    ime_composition: Option<ImeComposition>,
    /// Empty preedit cleared visible model text, but its terminal mutation may arrive later.
    ime_terminal_pending: Option<ImeTerminalPending>,
    /// A composition generation the application settled. Events queued for it in the settling
    /// input batch stay stale until the pass that publishes its platform interrupt (or an accepted
    /// pass with no descriptor, which resets the backend); the tombstone is retired there so later
    /// commit-only input is not lost (see [`ImeCancelled`]).
    ime_cancelled: Option<ImeCancelled>,
    /// Monotonic composition generation; every accepted pre-edit starts a new one.
    ime_generation: u64,
    /// The last [`Session::composition_settles`] value seen.
    ime_settles_seen: u64,
    /// A composition the application settled has not been announced to the platform yet.
    ime_interrupt_pending: bool,
    /// Token-bound reset lifetime for the owning root viewport's custom canvas editor. The map is
    /// keyed by viewport so a request or completion stamped for another viewport is never bound;
    /// render-only child viewports are rejected before they can create or overwrite an entry.
    ime_reset: HashMap<egui::ViewportId, ImeResetPhase>,
}

/// The platform's in-progress IME composition, as far as this editor tracks it.
#[derive(Clone, Copy, Debug)]
struct ImeComposition {
    /// The target whose text the input method started composing in.
    owner: egui::Id,
    /// Generation of this composition; a new pre-edit after a settled generation starts the next.
    generation: u64,
    /// The application settled the composition itself (a click, a key, a command). The platform
    /// may still hold it and must be interrupted rather than believed.
    settled: bool,
}

/// A settled composition generation whose platform copy may still deliver the rest of the
/// input batch it was queued in.
///
/// The tombstone lives only until the pass that publishes this generation's
/// `should_interrupt_composition` in its platform output, because that is the point at which the
/// backend resets (disables and re-enables) the platform composition. Events earlier in the same
/// batch still belong to the old composition and are rejected; from the next batch on egui's
/// events carry no generation, so an event the OS queued across the reset cannot be distinguished
/// from a legitimate new composition (see `ime_event`).
#[derive(Clone, Copy, Debug)]
struct ImeCancelled {
    owner: egui::Id,
    generation: u64,
}

/// A cleared preedit still owns its separately delivered terminal delete/commit. Application
/// work invalidates this snapshot; publishing an interrupt is not delivery of that terminal.
/// Keep a cancelled record until the terminal commit (or platform enable/disable) is observed.
struct ImeTerminalPending {
    owner: egui::Id,
    revision: u64,
    anchor: (usize, usize),
    caret: (usize, usize),
    cancelled: bool,
    /// An interruption has been observed since this guard was recorded. This stays distinct from
    /// the origin snapshot even when there is no active destination to stamp: `interrupted_at ==
    /// None` then means "interrupted with no target", not "not interrupted yet".
    interrupted: bool,
    /// The destination at which this terminal guard was last interrupted. Kept separately from
    /// the retired origin above: repeated passes at the same interrupted destination must not
    /// request another reset, while a genuinely newer target/selection/revision change still
    /// supersedes the current fence. When `interrupted` is false, `None` means the origin snapshot
    /// is still the reference; once interrupted, it means the interruption left no destination.
    interrupted_at: Option<ImeTerminalStamp>,
}

/// Explicit lifetime of the canvas's token-bound IME reset fence.
///
/// This is separate from the composition provenance/tombstone state above. Only a matching
/// ordered `ImeResetCompleted` clears `Awaiting`; `Enabled`, `Disabled`, empty preedits, idle
/// passes and timers never do. `Suspended` retains no acceptable old id and may carry one newly
/// requested id that a later eligible pass can bind.
#[derive(Clone, Copy, Debug)]
enum ImeResetPhase {
    /// No interrupted composition guard.
    Clear,
    /// The exact newest request has been published for this destination/descriptor.
    Awaiting { id: egui::ImeResetId, viewport: egui::ViewportId, destination: ImeDestination, output_context: ImeDescriptorKey },
    /// Guarded but no old id is acceptable; `request` is a new, not-yet-bound fence.
    Suspended { viewport: egui::ViewportId, needs_rearm: bool, request: Option<egui::ImeResetId> },
    /// No fence could be allocated; remain fail-closed without retrying on idle output.
    AllocationFailed { viewport: egui::ViewportId, destination: ImeDestination, output_context: ImeDescriptorKey },
}

impl ImeResetPhase {
    /// Only `Clear` admits custom-canvas input; awaiting, suspended, and allocation-failed phases
    /// remain fail-closed.
    fn blocks_custom_input(self) -> bool {
        !matches!(self, Self::Clear)
    }
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
mod field_frame_tests;
#[cfg(test)]
mod find_dialog_tests;
#[cfg(test)]
mod gpu_lock;
#[cfg(test)]
mod huge_deck_tests;
#[cfg(test)]
mod ime_tests;
#[cfg(test)]
mod last_color_tests;
#[cfg(test)]
mod presenter_tests;
#[cfg(test)]
mod reflection_pane_tests;
#[cfg(test)]
mod replacefonts_tests;
#[cfg(test)]
mod reuse_picker_tests;
#[cfg(test)]
mod slide_size_tests;
#[cfg(test)]
mod split_button_tests;
#[cfg(test)]
mod texture_limit_tests;
#[cfg(test)]
mod ui_soak_tests;
#[cfg(test)]
mod units_tests;

#[cfg(test)]
#[path = "datetime_tests.rs"]
mod datetime_tests;

#[cfg(test)]
mod inbox_tests;

#[cfg(test)]
mod tests;
