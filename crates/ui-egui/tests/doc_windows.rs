//! One native window per presentation: each window keeps its own dialog, closing a window closes
//! its presentation (asking first when it has unsaved changes), and quitting asks in each window.
//!
//! The windows are drawn through a test backend that, like eframe, runs each immediate viewport
//! as a nested pass with its own input, so a close button can be pressed on one window.
#![cfg(not(target_arch = "wasm32"))]

use std::cell::RefCell;

use deckcraft_engine::Session;
use deckcraft_ui_egui::doc_windows::DocWindows;
use deckcraft_ui_egui::{Services, SlideApp};
use egui::{Context, RawInput, ViewportCommand, ViewportEvent, ViewportId, ViewportInfo};
use serde_json::json;

thread_local! {
    /// Each viewport's input for the next frame (the close button).
    static INFOS: RefCell<egui::ViewportIdMap<ViewportInfo>> = RefCell::new(egui::ViewportIdMap::default());
    /// The viewport commands the app sent this frame.
    static SENT: RefCell<Vec<(ViewportId, ViewportCommand)>> = const { RefCell::new(Vec::new()) };
    /// Each window's builder as last shown.
    static BUILDERS: RefCell<egui::ViewportIdMap<egui::ViewportBuilder>> = RefCell::new(egui::ViewportIdMap::default());
}

fn record(mut out: egui::FullOutput) {
    out.textures_delta.clear(); // not painted
    SENT.with(|s| {
        let mut s = s.borrow_mut();
        for (id, v) in &out.viewport_output {
            s.extend(v.commands.iter().map(|c| (*id, c.clone())));
        }
    });
}

struct Desk {
    ctx: Context,
    app: SlideApp,
    windows: DocWindows,
}

impl Desk {
    /// The app with `n` new presentations, each in its own window.
    fn new(n: usize) -> Desk {
        Desk::with_base(n, egui::ViewportBuilder::default())
    }

    /// The app with `n` new presentations, its windows built from `base`.
    fn with_base(n: usize, base: egui::ViewportBuilder) -> Desk {
        INFOS.with(|i| i.borrow_mut().clear());
        let ctx = Context::default();
        ctx.set_embed_viewports(false);
        Context::set_immediate_viewport_renderer(|ctx, viewport| {
            let id = viewport.ids.this;
            BUILDERS.with(|b| b.borrow_mut().insert(id, viewport.builder.clone()));
            let mut input = RawInput { viewport_id: id, ..RawInput::default() };
            input.viewports = INFOS.with(|i| i.borrow().clone());
            input.viewports.entry(id).or_default().parent = Some(viewport.ids.parent);
            let mut cb = viewport.viewport_ui_cb;
            record(ctx.run_ui(input, |ui| cb(ui)));
        });
        let mut app = SlideApp::new(Session::new(), Services::default());
        for _ in 0..n {
            app.run("file.new", json!({})).unwrap();
        }
        let windows = DocWindows::new(&mut app, base);
        let mut desk = Desk { ctx, app, windows };
        desk.frames(3);
        desk
    }

    /// One frame as eframe runs it; `act` runs first, inside the frame. Returns the viewport
    /// commands sent.
    fn frame_with(&mut self, act: impl FnOnce(&mut SlideApp, &Context)) -> Vec<(ViewportId, ViewportCommand)> {
        SENT.with(|s| s.borrow_mut().clear());
        let mut input = RawInput { viewport_id: ViewportId::ROOT, ..RawInput::default() };
        input.viewports = INFOS.with(|i| i.borrow().clone());
        input.viewports.entry(ViewportId::ROOT).or_default();
        let Desk { ctx, app, windows } = self;
        let mut act = Some(act);
        let out = ctx.clone().run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            if let Some(act) = act.take() {
                act(app, &ctx);
            }
            app.logic(&ctx);
            windows.show(app, ui);
        });
        record(out);
        // Close buttons are one-shot events.
        INFOS.with(|i| i.borrow_mut().values_mut().for_each(|v| v.events.clear()));
        SENT.with(|s| s.borrow().clone())
    }

    fn frames(&mut self, n: usize) -> Vec<(ViewportId, ViewportCommand)> {
        (0..n).flat_map(|_| self.frame_with(|_, _| {})).collect()
    }

    fn uid(&self, doc: usize) -> u64 {
        self.app.session.documents()[doc].uid
    }

    fn viewport(&self, uid: u64) -> ViewportId {
        self.windows.viewport_of(uid).unwrap_or_else(|| panic!("presentation {uid} has no window"))
    }

    /// The user presses presentation `uid`'s close button.
    fn close(&mut self, uid: u64) -> Vec<(ViewportId, ViewportCommand)> {
        let vp = self.viewport(uid);
        INFOS.with(|i| i.borrow_mut().entry(vp).or_default().events.push(ViewportEvent::Close));
        let mut sent = self.frame_with(|_, _| {});
        sent.extend(self.frames(2));
        sent
    }

    /// The dialog showing in presentation `uid`'s window.
    fn dialog_in(&mut self, uid: u64) -> Option<String> {
        self.app.enter_deck(uid);
        self.app.dialog.as_ref().map(|d| d.id.clone())
    }

    /// Makes presentation `uid` unsaved.
    fn edit(&mut self, uid: u64) {
        self.frame_with(|app, _| {
            app.enter_deck(uid);
            app.run("slide.new", json!({})).unwrap();
        });
    }
}

fn sent_to(sent: &[(ViewportId, ViewportCommand)], vp: ViewportId, cmd: &ViewportCommand) -> bool {
    sent.iter().any(|(id, c)| *id == vp && c == cmd)
}

#[test]
fn each_presentation_has_a_window_and_new_opens_another() {
    let mut desk = Desk::new(1);
    let first = desk.uid(0);
    assert_eq!(desk.viewport(first), ViewportId::ROOT);
    desk.frame_with(|app, _| {
        let _ = app.run("file.new", json!({}));
    });
    desk.frames(2);
    let second = desk.uid(1);
    assert_ne!(desk.viewport(second), ViewportId::ROOT, "the new presentation opens in a window of its own");
    assert_eq!(desk.app.session.active().map(|d| d.uid), Some(second));
    assert_eq!(desk.windows.decks(), [second, first]);
}

#[test]
fn a_new_window_opens_below_and_right_of_a_screen_sized_one() {
    // The app's window size fills this 1440 × 900 screen, as on a 13" MacBook Air.
    let base = egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0]);
    let mut desk = Desk::with_base(1, base);
    let root = egui::Rect::from_min_size(egui::pos2(0.0, 31.0), egui::vec2(1440.0, 838.0));
    INFOS.with(|i| {
        let mut i = i.borrow_mut();
        let info = i.entry(ViewportId::ROOT).or_default();
        info.monitor_size = Some(egui::vec2(1440.0, 900.0));
        info.outer_rect = Some(root);
        info.inner_rect = Some(egui::Rect::from_min_max(root.min + egui::vec2(0.0, 33.0), root.max));
    });
    desk.frames(2);
    desk.frame_with(|app, _| {
        let _ = app.run("file.new", json!({}));
    });
    desk.frames(2);
    let window = BUILDERS.with(|b| b.borrow()[&desk.viewport(desk.uid(1))].clone());
    let at = window.position.expect("a cascaded position");
    assert!(at.x > root.min.x && at.y > root.min.y, "{at:?} is not on top of {root:?}");
    let size = window.inner_size.expect("a size");
    assert!(at.x + size.x <= 1440.0 && at.y + size.y + 33.0 <= 900.0, "{at:?} {size:?} fits the screen");
}

#[test]
fn closing_a_window_closes_only_its_presentation() {
    let mut desk = Desk::new(2);
    let (first, second) = (desk.uid(0), desk.uid(1));
    desk.close(second);
    assert_eq!(desk.app.session.documents().len(), 1);
    assert_eq!(desk.uid(0), first);
    assert_eq!(desk.viewport(first), ViewportId::ROOT);
}

#[test]
fn closing_the_root_window_keeps_the_app_on_the_other_presentation() {
    let mut desk = Desk::new(2);
    let (mut root, mut other) = (desk.uid(0), desk.uid(1));
    if desk.viewport(root) != ViewportId::ROOT {
        std::mem::swap(&mut root, &mut other);
    }
    assert_eq!(desk.viewport(root), ViewportId::ROOT);
    let sent = desk.close(root);
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::CancelClose), "{sent:?}");
    assert_eq!(desk.app.session.documents().len(), 1);
    assert_eq!(desk.viewport(other), ViewportId::ROOT, "the root window now shows the other presentation");
}

#[test]
fn an_unsaved_presentation_asks_in_its_own_window() {
    let mut desk = Desk::new(2);
    let (first, second) = (desk.uid(0), desk.uid(1));
    desk.edit(second);
    desk.close(second);
    assert_eq!(desk.app.session.documents().len(), 2, "nothing closes before the answer");
    assert_eq!(desk.dialog_in(second).as_deref(), Some("close"));
    assert_eq!(desk.dialog_in(first), None, "the other window shows no question");
}

#[test]
fn quitting_asks_each_unsaved_presentation_in_turn() {
    let mut desk = Desk::new(3);
    let (first, second, third) = (desk.uid(0), desk.uid(1), desk.uid(2));
    desk.edit(first);
    desk.edit(third);
    desk.frame_with(|app, ctx| app.request_quit(ctx));
    desk.frames(2);
    assert_eq!(desk.dialog_in(first).as_deref(), Some("close"));
    assert_eq!(desk.dialog_in(third), None, "one question at a time");
    // Don't Save, as its button does.
    desk.frame_with(|app, _| {
        app.enter_deck(first);
        app.dialog = None;
        app.run("file.close", json!({})).unwrap();
    });
    desk.frames(2);
    assert_eq!(desk.dialog_in(third).as_deref(), Some("close"));
    // Don't Save again: nothing unsaved is left, so the app quits.
    let mut sent = desk.frame_with(|app, _| {
        app.enter_deck(third);
        app.dialog = None;
        app.run("file.close", json!({})).unwrap();
    });
    sent.extend(desk.frames(2));
    assert!(desk.app.quit_confirmed);
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "{sent:?}");
    assert_eq!(desk.app.session.documents().iter().map(|d| d.uid).collect::<Vec<_>>(), [second]);
}

#[test]
fn cancel_stops_the_quit() {
    let mut desk = Desk::new(2);
    let first = desk.uid(0);
    desk.edit(first);
    desk.frame_with(|app, ctx| app.request_quit(ctx));
    desk.frames(2);
    assert_eq!(desk.dialog_in(first).as_deref(), Some("close"));
    let mut sent = desk.frame_with(|app, _| {
        app.enter_deck(first);
        app.dialog = None;
    });
    sent.extend(desk.frames(3));
    assert!(!desk.app.quit_confirmed);
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "{sent:?}");
    assert_eq!(desk.app.session.documents().len(), 2);
    assert_eq!(desk.dialog_in(first), None, "Cancel isn't asked again");
}

/// The host's sheets (`Services::save_prompt`): the window titles asked about, and the answers to give.
type Sheets = std::rc::Rc<RefCell<Vec<(String, std::sync::mpsc::Sender<String>)>>>;

fn with_sheets(desk: &mut Desk) -> Sheets {
    let sheets = Sheets::default();
    let asked = sheets.clone();
    desk.app.services.save_prompt = Some(Box::new(move |window, _question, _detail| {
        let (answer, rx) = std::sync::mpsc::channel();
        asked.borrow_mut().push((window.to_string(), answer));
        Some(rx)
    }));
    sheets
}

#[test]
fn quitting_asks_on_each_presentations_own_sheet() {
    let mut desk = Desk::new(2);
    let (first, second) = (desk.uid(0), desk.uid(1));
    desk.edit(first);
    desk.edit(second);
    let sheets = with_sheets(&mut desk);
    desk.frame_with(|app, ctx| app.request_quit(ctx));
    desk.frames(3);
    let title = |desk: &Desk, uid| desk.app.session.documents().iter().find(|d| d.uid == uid).map(|d| format!("{} — Edited", d.title()));
    let asked = sheets.borrow().iter().map(|(w, _)| w.clone()).collect::<Vec<_>>();
    assert_eq!(asked, [title(&desk, first).unwrap()], "one sheet, on the first window, asked once");
    assert_eq!(desk.dialog_in(first).as_deref(), Some("close"), "the quit waits on the sheet");
    sheets.borrow()[0].1.send("dontSave".into()).unwrap();
    desk.frames(3);
    assert_eq!(desk.app.session.documents().iter().map(|d| d.uid).collect::<Vec<_>>(), [second]);
    assert_eq!(sheets.borrow().get(1).map(|(w, _)| w.clone()), title(&desk, second), "then the next window's sheet");
    sheets.borrow()[1].1.send("cancel".into()).unwrap();
    let sent = desk.frames(3);
    assert!(!desk.app.quit_confirmed && !sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "{sent:?}");
    assert_eq!(desk.dialog_in(second), None);
    assert_eq!(sheets.borrow().len(), 2, "Cancel isn't asked again");
}

#[test]
fn without_a_sheet_the_built_in_dialog_asks() {
    let mut desk = Desk::new(1);
    let first = desk.uid(0);
    desk.edit(first);
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let counted = calls.clone();
    desk.app.services.save_prompt = Some(Box::new(move |_, _, _| {
        counted.set(counted.get() + 1);
        None
    }));
    desk.frame_with(|app, _| {
        app.enter_deck(first);
        app.close_presentation();
    });
    desk.frames(3);
    assert_eq!(desk.dialog_in(first).as_deref(), Some("close"));
    assert_eq!(calls.get(), 1, "asked the host once, then the built-in dialog");
}
