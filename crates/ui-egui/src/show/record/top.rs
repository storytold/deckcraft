//! The floating command bar of the Record view: Edit, the capture clock, Reset/Undo,
//! Record/Stop, Pause/Resume, camera, microphone, screen recording, More and Export.
//!
//! Every button acts on [`RecordView`] or [`Show`] itself. Device controls call the
//! [`super::devices`] hooks, and Export leaves an explicit [`RecordCommand`] for the section that
//! eventually owns video export: nothing here reports success it did not have.
//!
//! The bar also owns the Record view's shortcuts. They are chosen so they cannot shadow the
//! show's own navigation: Ctrl/⌘R records, Ctrl/⌘Shift+P pauses, Ctrl/⌘Z undoes a Reset,
//! Ctrl/⌘E asks for Export, and Escape closes the view (asking first while a take is running).

mod edit;
mod export;
mod more;
mod pause;
mod record;
mod reset;
mod timer;

use egui::{Align, Color32, CornerRadius, Layout as EguiLayout, Rect, RichText, Ui, UiBuilder, vec2};

use super::devices;
use super::{Layout, RecordStatus, RecordView};
use crate::show::Show;
use crate::{SlideApp, theme};

pub(super) struct Out {
    pub close: bool,
}

const FG: Color32 = Color32::from_gray(226);
const DIM: Color32 = Color32::from_gray(150);
const RED: Color32 = Color32::from_rgb(224, 62, 62);

/// What a top-bar shortcut asks for. Kept apart from the widgets so the mapping is testable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shortcut {
    Record,
    Pause,
    UndoReset,
    Close,
    Export,
}

/// The shortcut a key press means in the Record view, if any.
///
/// `command` is Ctrl on Windows/Linux and ⌘ on macOS. Plain keys are deliberately not taken:
/// they stay with the show (Space/arrows advance, R resets the show's own timer).
fn shortcut(key: egui::Key, m: egui::Modifiers, status: RecordStatus) -> Option<Shortcut> {
    let command = m.command && !m.alt && !m.shift;
    let command_shift = m.command && !m.alt && m.shift;
    match key {
        egui::Key::Escape if !m.command && !m.ctrl && !m.alt && !m.shift => Some(Shortcut::Close),
        egui::Key::R if command => Some(Shortcut::Record),
        egui::Key::P if command_shift => (status != RecordStatus::Idle).then_some(Shortcut::Pause),
        egui::Key::Z if command => Some(Shortcut::UndoReset),
        egui::Key::E if command => Some(Shortcut::Export),
        _ => None,
    }
}

/// Whether the Record shortcuts are read this frame. The notes editor owns the keyboard while it
/// is focused; a focused button must not swallow Ctrl+R, Ctrl+Shift+P or Ctrl+Z.
fn reads_shortcuts(rec: &RecordView, ctx: &egui::Context) -> bool {
    !rec.close_confirm && !super::notes::has_focus(ctx)
}

/// Draw the top bar. Returns `close` when Edit (or Escape, confirmed) asked to leave.
pub(super) fn ui(app: &mut SlideApp, ui: &mut Ui, show: &mut Show, rec: &mut RecordView, now: f64, layout: &Layout) -> Out {
    let rect = layout.top;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(10), Color32::from_rgb(30, 30, 34));
    painter.rect_stroke(rect, CornerRadius::same(10), egui::Stroke::new(1.0, Color32::from_gray(64)), egui::StrokeKind::Inside);

    let mut out = Out { close: false };
    // Shortcuts are read before the widgets so a focused button does not also act on the key.
    // While the close confirmation is up the modal owns Escape.
    let mut close_requested = false;
    // A focused button must not swallow Ctrl+R / Ctrl+Shift+P / Ctrl+Z; only the notes editor
    // owns the keyboard.
    if reads_shortcuts(rec, ui.ctx()) {
        let presses: Vec<(egui::Key, egui::Modifiers)> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, modifiers) in presses {
            let Some(action) = shortcut(key, modifiers, rec.status) else { continue };
            ui.ctx().input_mut(|i| i.consume_key(modifiers, key));
            match action {
                Shortcut::Record => rec.toggle_record(show, now),
                Shortcut::Pause => match rec.status {
                    RecordStatus::Recording => rec.pause(show, now),
                    RecordStatus::Paused => rec.resume(show, now),
                    RecordStatus::Idle => {}
                },
                Shortcut::UndoReset => {
                    rec.undo(now);
                }
                Shortcut::Close => close_requested = true,
                Shortcut::Export => {
                    export::request(rec);
                    app.set_status("Export is waiting for the recording pipeline.");
                }
            }
        }
    }

    let inner = rect.shrink2(vec2(10.0, 6.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(EguiLayout::left_to_right(Align::Center)));
    child.spacing_mut().item_spacing.x = 6.0;
    child.visuals_mut().override_text_color = Some(FG);

    let edit_clicked = edit::ui(&mut child, rec);
    if (edit_clicked || close_requested) && close_now(show, rec, now) {
        out.close = true;
    }

    // The clock sits in the middle of the bar, whatever the groups on either side.
    timer::paint(&painter, rect, rec, now);

    // Right-hand group, laid out from the right edge inward.
    let right = Rect::from_min_max(egui::pos2(rect.center().x + 40.0, inner.min.y), inner.max);
    let mut right_ui = ui.new_child(UiBuilder::new().max_rect(right).layout(EguiLayout::right_to_left(Align::Center)));
    right_ui.spacing_mut().item_spacing.x = 6.0;
    right_ui.visuals_mut().override_text_color = Some(FG);

    if export::ui(&mut right_ui, rec) {
        app.set_status("Export is waiting for the recording pipeline.");
    }
    more::ui(&mut right_ui, rec, now);
    if devices::screen_button(&mut right_ui, rec, now) {
        // Screen capture is a device hook with no backend yet: say so instead of queueing the
        // video-export request, which belongs to the export section.
        app.set_status("Screen recording is not available yet.");
    }
    // A refused microphone that the user asks for again gets a new start attempt. Read the
    // failure first: the click clears it.
    let mic_failed = rec.devices.mic.failed;
    if devices::mic_button(&mut right_ui, rec, now) && mic_failed {
        show.mic_tried = false;
    }
    if devices::camera_button(&mut right_ui, rec, now) {
        rec.camera_bubble = rec.devices.camera.is_on();
    }
    pause::ui(&mut right_ui, show, rec, now);
    record::ui(&mut right_ui, show, rec, now);
    reset::ui(&mut right_ui, rec, now);

    painter.text(
        egui::pos2(inner.max.x - 2.0, inner.max.y - 8.0),
        egui::Align2::RIGHT_BOTTOM,
        status_label(rec.status),
        theme::font(10.0),
        timer::dot_color(rec.status),
    );
    // The show never draws the editor's status bar, so a microphone that stopped mid-take says why
    // here: the switch alone only reads unavailable.
    if let Some(e) = &show.mic_error {
        painter.text(egui::pos2(inner.min.x + 2.0, inner.max.y - 8.0), egui::Align2::LEFT_BOTTOM, format!("Microphone: {e}"), theme::font(10.0), RED);
    }

    if let Some(verdict) = edit::confirm(ui, rec, now)
        && verdict == edit::Verdict::End
    {
        end_and_close(show, rec, now);
        out.close = true;
    }
    out
}

/// Handle Edit/Escape. Returns whether the view can close now; otherwise it raises the
/// confirmation for a running or paused take.
fn close_now(show: &mut Show, rec: &mut RecordView, now: f64) -> bool {
    match edit::intent(rec) {
        edit::Closing::Now => {
            end_and_close(show, rec, now);
            true
        }
        edit::Closing::Confirm => {
            rec.close_confirm = true;
            false
        }
    }
}

/// Stop the take (freezing the clock) and hand the save decision back to the show: only a take
/// that actually captured something is kept.
fn end_and_close(show: &mut Show, rec: &mut RecordView, now: f64) {
    rec.stop(show, now);
    rec.close_confirm = false;
    show.record = rec.captured;
}

fn status_label(status: RecordStatus) -> &'static str {
    match status {
        RecordStatus::Idle => "Ready",
        RecordStatus::Recording => "Recording",
        RecordStatus::Paused => "Paused",
    }
}

fn button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).size(13.0)).frame(false))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn plain() -> egui::Modifiers {
        egui::Modifiers::NONE
    }

    fn command() -> egui::Modifiers {
        egui::Modifiers { command: true, ..egui::Modifiers::NONE }
    }

    fn command_shift() -> egui::Modifiers {
        egui::Modifiers { command: true, shift: true, ..egui::Modifiers::NONE }
    }

    fn show() -> Show {
        let session = deckcraft_engine::Session::with_new();
        let doc = session.active().unwrap().doc.clone();
        Show::new(&doc, 0, false, false)
    }

    /// A focused button does not stop the Record shortcuts; the notes editor does.
    #[test]
    fn a_focused_button_keeps_the_record_shortcuts() {
        let ctx = egui::Context::default();
        let rec = RecordView::default();
        let button = egui::Id::unique("a-record-button");
        ctx.memory_mut(|m| m.request_focus(button));
        assert!(reads_shortcuts(&rec, &ctx), "a focused button is not the script");
        // The script editor's id, reached through the module that owns it.
        let editor = super::super::notes::editor_id_for_tests();
        ctx.memory_mut(|m| m.request_focus(editor));
        assert!(!reads_shortcuts(&rec, &ctx), "the script editor owns the keyboard");
        ctx.memory_mut(|m| m.surrender_focus(editor));
        let confirming = RecordView { close_confirm: true, ..Default::default() };
        assert!(!reads_shortcuts(&confirming, &ctx), "the confirmation owns the keyboard");
    }

    /// Ctrl/⌘R is the only Record shortcut; plain R stays with the show's timer reset.
    #[test]
    fn record_shortcuts_do_not_shadow_the_show_keys() {
        assert_eq!(shortcut(egui::Key::R, command(), RecordStatus::Idle), Some(Shortcut::Record));
        assert_eq!(shortcut(egui::Key::R, plain(), RecordStatus::Idle), None);
        assert_eq!(shortcut(egui::Key::P, command_shift(), RecordStatus::Recording), Some(Shortcut::Pause));
        assert_eq!(shortcut(egui::Key::P, command_shift(), RecordStatus::Idle), None, "nothing to pause");
        assert_eq!(shortcut(egui::Key::P, command(), RecordStatus::Recording), None, "Ctrl+P is the show's pen");
        assert_eq!(shortcut(egui::Key::Z, command(), RecordStatus::Idle), Some(Shortcut::UndoReset));
        assert_eq!(shortcut(egui::Key::Z, plain(), RecordStatus::Idle), None, "plain Z is the show's zoom");
        assert_eq!(shortcut(egui::Key::E, command(), RecordStatus::Idle), Some(Shortcut::Export));
        assert_eq!(shortcut(egui::Key::Escape, plain(), RecordStatus::Recording), Some(Shortcut::Close));
        assert_eq!(shortcut(egui::Key::Escape, command(), RecordStatus::Recording), None);
        assert_eq!(shortcut(egui::Key::Space, plain(), RecordStatus::Recording), None);
    }

    /// Edit/Escape closes at once when idle, and raises the confirmation while a take runs.
    #[test]
    fn close_asks_first_only_for_a_running_take() {
        let mut idle = show();
        let mut rec = RecordView::default();
        assert!(close_now(&mut idle, &mut rec, 1.0));
        assert!(!rec.captured, "an idle view never claims a recording");

        let mut show = show();
        let mut rec = RecordView::default();
        rec.start(&mut show, 0.0);
        assert!(!close_now(&mut show, &mut rec, 2.0), "a running take asks first");
        assert!(rec.close_confirm);
        assert_eq!(rec.status, RecordStatus::Recording);
    }

    /// Ending from the confirmation stops the clock and closes; the take is marked captured.
    #[test]
    fn ending_a_confirmed_take_stops_and_closes() {
        let mut show = show();
        let mut rec = RecordView::default();
        rec.start(&mut show, 0.0);
        rec.close_confirm = true;
        end_and_close(&mut show, &mut rec, 5.0);
        assert_eq!(rec.status, RecordStatus::Idle);
        assert!(!rec.close_confirm);
        assert!(rec.captured);
        assert!(show.record, "a captured take is handed to the save path");
    }
}
