//! The craft-ui runtime, hosted from DeckCraft's own eframe `App` (craft-libs docs/craft-framework.md, section 2.2).
//! The desktop app calls [`install`] once, [`SlideApp::before_input`] in `raw_input_hook`, [`SlideApp::close_path`]
//! last in `logic`, and [`before_ui`] first in `ui`; the browser build and tests install with [`install_headless`].
//! `SlideApp::ui` runs the framework dialogs ([`SlideApp::framework_dialogs`]) after DeckCraft's own UI.
//!
//! The runtime takes Escape, Tab and the clipboard events for the whole window unless one of DeckCraft's own egui
//! widgets has the keyboard. The window's Escape is read with [`escape_pressed`] or [`take_window_cancel`]; the
//! document window is declared to the runtime with [`host_canvas`] each pass that draws it, which keeps the edit chords
//! and the clipboard events, and Tab while it has the focus and Tab has work, in egui's input for DeckCraft's own key
//! routing (`SlideApp::keyboard`). That routing has the keyboard unless [`widget_has_keyboard`].

use std::sync::Arc;

use craft_host::target::ShellInfo;
use craft_host::{AppInfo, Host};
use craft_keymap::{Chord, Key, Mods};
use craft_ui::fonts::{I18nScripts, SystemFonts, UiFonts};
use craft_ui::runtime::env::{RootEnv, Scale};
use craft_ui::runtime::focus::{self, EditSet, FocusTarget, KeepKeys, KeyUse};
use craft_ui::runtime::frame::{FrameInput, FrameOutput};
use craft_ui::runtime::keys::{Intent, with_window_intents};
use craft_ui::shell::embed::{self, CloseDecision};
use craft_window::system::WindowSystem;

use crate::SlideApp;

/// Each pass of the root window, first in `App::ui`: hands the runtime the native window it answers the hit test of
/// (Windows) and fits the window to its monitor once.
#[cfg(not(target_arch = "wasm32"))]
pub use craft_ui::shell::embed::before_ui;

/// The scripts of the interface languages, for the font stack.
static SCRIPTS: I18nScripts = I18nScripts;

/// The runtime's first font stack: egui's bundled faces and no installed system fonts. DeckCraft's own definitions
/// (`theme::install_fonts_with`, in the first UI pass) replace it before anything is drawn.
fn ui_fonts() -> UiFonts<'static> {
    UiFonts::new(&SCRIPTS).system_fonts(SystemFonts::Off)
}

fn install_with(ctx: &egui::Context, shell: ShellInfo, windows: Arc<dyn WindowSystem>) -> Result<(), String> {
    let spec = craft_ui::InstallSpec::new(shell, windows).app_id(crate::os_window::app_id());
    embed::install(ctx, spec, &ui_fonts()).map(drop).map_err(|error| format!("the UI runtime could not be installed: {error}"))
}

/// Installs the runtime on the desktop window's context, once, before the first pass. `forced_x11` when the app
/// opens its window through XWayland. DeckCraft draws its own title bar row (`ribbon::title_bar`) and gives the
/// runtime no frame decision, so the runtime takes no part of the window's move area or hit test.
#[cfg(not(target_arch = "wasm32"))]
pub fn install(ctx: &egui::Context, forced_x11: bool) -> Result<(), String> {
    use craft_host::target::Backend;
    let backend = if forced_x11 { Backend::X11 } else { Backend::from_env() };
    install_with(ctx, ShellInfo::default().backend(backend), Arc::new(craft_window::system::SystemWindows))
}

/// Installs the runtime on a context with no native window of its own: the browser build's canvas and test windows.
pub fn install_headless(ctx: &egui::Context) {
    if let Err(error) = install_with(ctx, ShellInfo::default(), Arc::new(craft_window::system::NoWindows)) {
        log::warn!("{error}");
    }
}

/// Who DeckCraft is to the framework's dialogs (the update dialogs show its name and icon).
pub(crate) fn host() -> Arc<Host> {
    Arc::new(
        Host::builder(AppInfo { id: "deckcraft", name: "DeckCraft", version: env!("CARGO_PKG_VERSION") })
            .icon_png(include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.deckcraft.png"))
            .build(),
    )
}

impl SlideApp {
    /// The framework's part of `App::raw_input_hook`, once per frame before the first pass: takes what the runtime
    /// gathered since the last frame and gives the coming passes the root values (theme mode, interface scale).
    pub fn before_input(&mut self, ctx: &egui::Context) {
        embed::before_input(ctx, |output| self.frame_input(output));
    }

    /// The root values of this frame. DeckCraft registers no commands with the runtime and lets it toggle no chrome,
    /// so the output carries nothing it acts on. The interface scale is DeckCraft's own (`UiState::ui_scale`, which
    /// `ui_setup` applies), so the runtime's zoom and DeckCraft's agree.
    fn frame_input(&mut self, _output: FrameOutput) -> FrameInput {
        let mode = if self.ui.brightness == crate::theme::Brightness::Dark { craft_theme::Mode::Dark } else { craft_theme::Mode::Light };
        // The slide show and the canvas read Ctrl/Cmd+wheel themselves, so a smooth Ctrl+wheel stays a wheel event.
        let root = RootEnv::new().mode(mode).scale(Scale::new(self.ui.ui_scale.clamp(0.5, 3.0))).ctrl_wheel_pinch(false);
        FrameInput::new(root).host(Arc::clone(&self.framework_host))
    }

    /// The framework's part of `App::logic`, after DeckCraft's own: the root window's close path. With unsaved
    /// presentations the window stays open while the quit dialog asks; its answer closes the window again with
    /// `quit_confirmed` set.
    pub fn close_path(&mut self, ctx: &egui::Context) {
        embed::logic(ctx, || {
            if self.quit_confirmed || !self.session.documents().iter().any(|d| d.is_dirty()) {
                return CloseDecision::Close;
            }
            self.show = None;
            if self.dialog.as_ref().is_none_or(|d| d.id != "quit") {
                self.dialog = Some(crate::dialogs::Dialog::new("quit"));
            }
            CloseDecision::Hold
        });
    }
}

/// Alt+1 to Alt+9, the Quick Access Toolbar's positions on Windows and Linux (`menus::ui_shortcut`). Claimed, so
/// the runtime's Alt+numpad code entry leaves them to DeckCraft's shortcuts.
const QUICK_ACCESS_KEYS: [Chord; 9] = [
    Chord::new(Mods::ALT, Key::Num1),
    Chord::new(Mods::ALT, Key::Num2),
    Chord::new(Mods::ALT, Key::Num3),
    Chord::new(Mods::ALT, Key::Num4),
    Chord::new(Mods::ALT, Key::Num5),
    Chord::new(Mods::ALT, Key::Num6),
    Chord::new(Mods::ALT, Key::Num7),
    Chord::new(Mods::ALT, Key::Num8),
    Chord::new(Mods::ALT, Key::Num9),
];

/// The keys DeckCraft's document window keeps while it has the focus or is the window's key target: every clipboard
/// event and edit chord, and the Quick Access Toolbar's Alt+digits, which `SlideApp::keyboard` routes itself.
const DOCUMENT_KEYS: KeyUse = KeyUse::CANVAS.with_edits(EditSet::ALL).with_claims(&QUICK_ACCESS_KEYS);

/// The document window's widget: the slide canvas, the slide sorter or the notes page, whichever the view draws.
pub(crate) fn document_id() -> egui::Id {
    egui::Id::unique("deckcraft_document")
}

/// Declares DeckCraft's document window, drawn in this pass, as the window's canvas. With nothing focused it is the
/// window's key target: the edit chords, the clipboard events and the claimed Alt+digits stay in egui's input for
/// DeckCraft. Tab and Shift+Tab stop at it; with `keeps_tab` the focused document keeps them in egui's input, else they
/// move on from it.
pub(crate) fn host_canvas(ctx: &egui::Context, keeps_tab: bool) {
    let keys = if keeps_tab { DOCUMENT_KEYS.with_keep(KeepKeys::Tab) } else { DOCUMENT_KEYS };
    focus::host_canvas(ctx, document_id(), keys);
}

/// Gives the document window the keyboard focus. It is the runtime's focus, which an Escape does not take away.
pub(crate) fn focus_document(ctx: &egui::Context) {
    focus::request(ctx, FocusTarget::Id(document_id()));
}

/// The widget that has the keyboard, the document window left out: a focused document leaves the keyboard to
/// DeckCraft's own key routing, as with nothing focused.
pub(crate) fn focused_widget(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.memory(|m| m.focused()).filter(|id| *id != document_id())
}

/// Whether a widget other than the document window has the keyboard.
pub(crate) fn widget_has_keyboard(ctx: &egui::Context) -> bool {
    focused_widget(ctx).is_some()
}

/// Whether Escape was pressed in this pass: from egui while one of DeckCraft's own widgets or popups has the keyboard,
/// otherwise the window's cancel, which the runtime routes to no component. Leaves it for later readers.
pub(crate) fn escape_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape)) || window_escape(ctx)
}

/// Whether the window's cancel (an Escape while nothing has framework focus, which the runtime takes out of egui's
/// events) is there and untaken. Key loops that read egui's events add it as an Escape press.
pub(crate) fn window_escape(ctx: &egui::Context) -> bool {
    window_cancel(ctx, false)
}

/// The Escape press [`window_escape`] stands for, for key loops over egui's events.
pub(crate) const ESCAPE_PRESS: egui::Event =
    egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };

/// Takes the window's cancel (an Escape while nothing has framework focus): whether there was one.
pub(crate) fn take_window_cancel(ctx: &egui::Context) -> bool {
    window_cancel(ctx, true)
}

fn window_cancel(ctx: &egui::Context, take: bool) -> bool {
    let mut pending = false;
    with_window_intents(ctx, false, |intents| {
        pending = intents.untaken(&Intent::Cancel);
        take && pending && intents.take(Intent::Cancel)
    });
    pending
}
