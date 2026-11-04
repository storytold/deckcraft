//! The Edit button (leave the Record view) and the confirmation a running take asks for.
//!
//! Leaving is immediate while nothing is being captured, but a running or paused take is real
//! work: the shell asks first, and only the answer lets the view close. The confirmation is a
//! foreground window anchored to the screen, so it stays readable even when the top bar itself
//! is squeezed.

use egui::Ui;

use super::super::{RecordStatus, RecordView};
use super::button;

/// What pressing Edit (or Escape) does right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Closing {
    /// Leave at once: nothing is being captured.
    Now,
    /// Ask first: the clock is running or paused.
    Confirm,
}

/// The answer to the close confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Stop the take and leave the view.
    End,
    /// Stay in the view and keep capturing.
    Keep,
}

/// What pressing Edit (or Escape) does right now.
pub(super) fn intent(rec: &RecordView) -> Closing {
    if rec.status == RecordStatus::Idle { Closing::Now } else { Closing::Confirm }
}

/// Draw the Edit button. Returns true when it was clicked; the view is left untouched.
pub(super) fn ui(ui: &mut Ui, _rec: &RecordView) -> bool {
    button(ui, "‹ Edit", true).on_hover_text("Leave the Record view").clicked()
}

/// Draw the close confirmation while `rec.close_confirm` is set.
///
/// Returns the answer, if one was given this frame: Keep was picked (or Escape was pressed),
/// or End was picked. Keep clears the flag here; End leaves the flag to the caller, which owns
/// the decision to stop the take and close the view.
///
/// The clock keeps running while the question is up, so `now` is not needed to draw it.
pub(super) fn confirm(ui: &mut Ui, rec: &mut RecordView, _now: f64) -> Option<Verdict> {
    if !rec.close_confirm {
        return None;
    }
    let ctx = ui.ctx().clone();
    // Escape means "keep recording". Consume it so the shell does not also read it as a second
    // close request while the confirmation is up.
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        apply_verdict(rec, Verdict::Keep);
        return Some(Verdict::Keep);
    }

    let mut verdict = None;
    // Anchored to the screen rather than laid out in the top bar: the question must be drawn
    // at full size even when the bar has no room left for it.
    let _window = egui::Window::new("End recording?")
        .id(ui.scope_id().with("record_close_confirm"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .show(&ctx, |ui| {
            ui.label("The recording so far will be kept.");
            ui.horizontal(|ui| {
                if button(ui, "Keep recording", true).clicked() {
                    verdict = Some(Verdict::Keep);
                }
                if button(ui, "End recording", true).clicked() {
                    verdict = Some(Verdict::End);
                }
            });
        });

    let verdict = verdict?;
    apply_verdict(rec, verdict);
    Some(verdict)
}

/// Apply the state change a verdict implies. End is the caller's to act on, so it changes
/// nothing here.
fn apply_verdict(rec: &mut RecordView, verdict: Verdict) {
    if verdict == Verdict::Keep {
        rec.close_confirm = false;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn intent_is_immediate_only_when_nothing_is_being_captured() {
        let mut rec = RecordView::default();
        assert_eq!(intent(&rec), Closing::Now);

        rec.status = RecordStatus::Recording;
        assert_eq!(intent(&rec), Closing::Confirm);

        rec.status = RecordStatus::Paused;
        assert_eq!(intent(&rec), Closing::Confirm);
    }

    #[test]
    fn a_finished_take_does_not_ask_again() {
        let rec = RecordView { captured: true, ..Default::default() };
        assert_eq!(intent(&rec), Closing::Now, "a take that already stopped needs no confirmation");
    }

    #[test]
    fn confirm_returns_none_without_the_flag_and_draws_no_window() {
        let ctx = egui::Context::default();
        let mut rec = RecordView { status: RecordStatus::Recording, ..Default::default() };
        assert!(!rec.close_confirm);

        let mut verdict = Some(Verdict::End);
        let mut opened = true;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let before = ui.ctx().memory(|m| m.areas().visible_layer_ids().len());
            verdict = confirm(ui, &mut rec, 3.0);
            opened = ui.ctx().memory(|m| m.areas().visible_layer_ids().len()) > before;
        });
        out.textures_delta.clear();

        assert_eq!(verdict, None);
        assert!(!opened, "a clear flag must not draw the confirmation");
        assert!(!rec.close_confirm);
        assert_eq!(rec.status, RecordStatus::Recording);
    }

    #[test]
    fn confirm_draws_the_window_until_it_is_answered() {
        let ctx = egui::Context::default();
        let mut rec = RecordView { status: RecordStatus::Recording, ..Default::default() };
        rec.close_confirm = true;

        let mut verdict = Some(Verdict::End);
        let mut opened = false;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let before = ui.ctx().memory(|m| m.areas().visible_layer_ids().len());
            verdict = confirm(ui, &mut rec, 4.0);
            opened = ui.ctx().memory(|m| m.areas().visible_layer_ids().len()) > before;
        });
        out.textures_delta.clear();

        assert_eq!(verdict, None, "nothing was clicked");
        assert!(opened, "the confirmation window is drawn");
        assert!(rec.close_confirm, "the question stays up until it is answered");
    }

    #[test]
    fn keep_clears_the_flag_while_end_leaves_it_to_the_caller() {
        let mut rec = RecordView { status: RecordStatus::Recording, ..Default::default() };
        rec.close_confirm = true;

        apply_verdict(&mut rec, Verdict::Keep);
        assert!(!rec.close_confirm);

        rec.close_confirm = true;
        apply_verdict(&mut rec, Verdict::End);
        assert!(rec.close_confirm, "End is the caller's decision to stop and close");
    }
}
