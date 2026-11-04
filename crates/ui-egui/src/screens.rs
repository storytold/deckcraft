//! Which screen plays the slides, and which screen shows Presenter View.
//!
//! Indices are positions in the host's monitor list, the same order as
//! `egui::ViewportCommand::SetMonitor`.

use crate::show::Show;
use crate::{SlideApp, UiState};

/// One screen the host can show a window on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

impl ScreenInfo {
    /// Text for the Screen Settings menu.
    pub fn label(&self) -> String {
        let role = if self.primary { "Primary" } else { "Extended" };
        format!("{} ({role}, {}×{})", friendly_name(&self.name, self.index), self.width, self.height)
    }
}

fn friendly_name(raw: &str, index: usize) -> String {
    let trimmed = raw.trim().trim_start_matches(r"\\.\");
    let generic = trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") || trimmed.to_ascii_uppercase().starts_with("DISPLAY");
    if generic { format!("Screen {}", index + 1) } else { format!("Screen {}: {trimmed}", index + 1) }
}

fn known(screens: &[ScreenInfo], choice: Option<usize>) -> Option<usize> {
    choice.filter(|i| screens.iter().any(|s| s.index == *i))
}

/// Screens for a show.
///
/// With Presenter View on and no saved choice, the slides use a non-primary
/// screen and the notes stay on the primary. Without Presenter View, the
/// slides move only when a presentation screen was saved and is still connected.
pub fn assign(screens: &[ScreenInfo], presentation: Option<usize>, notes: Option<usize>, presenter: bool) -> (Option<usize>, Option<usize>) {
    if !presenter {
        return (known(screens, presentation), None);
    }
    let Some(primary) = screens.iter().find(|s| s.primary).map(|s| s.index).or_else(|| screens.first().map(|s| s.index)) else {
        return (None, None);
    };
    let other = screens.iter().find(|s| s.index != primary).map(|s| s.index);
    let notes = known(screens, notes).unwrap_or(primary);
    let presentation = known(screens, presentation).or(other).unwrap_or(primary);
    (Some(presentation), Some(notes))
}

/// Presenter View fills its own screen when that screen is not already showing the slides.
/// On the same screen the notes stay in a window, so they are not covered by the slides.
pub fn presenter_monitor(audience: Option<usize>, notes: Option<usize>) -> Option<usize> {
    match (audience, notes) {
        (Some(audience), Some(notes)) if audience != notes => Some(notes),
        (None, Some(notes)) => Some(notes),
        _ => None,
    }
}

impl SlideApp {
    /// Replace the screen list when the host's monitors have changed.
    pub fn set_screens(&mut self, screens: Vec<ScreenInfo>) {
        if self.screens != screens {
            self.screens = screens;
        }
    }

    /// Copy the saved screen choice onto a show that has not started yet.
    pub(crate) fn place_show(&self, show: &mut Show) {
        let (audience, notes) = assign(&self.screens, self.ui.show_screen, self.ui.notes_screen, show.presenter && !show.reading);
        show.audience_screen = audience;
        show.notes_screen = notes;
    }
}

/// The index named by a Screen Settings field, when it is one of `screens`.
pub(crate) fn chosen(raw: &str, screens: &[ScreenInfo]) -> Option<usize> {
    let index = raw.parse::<usize>().ok()?;
    screens.iter().any(|s| s.index == index).then_some(index)
}

/// Drop a saved index the settings file could not have meant.
pub(crate) fn sanitize(ui: &mut UiState) {
    ui.show_screen = ui.show_screen.filter(|i| *i < 64);
    ui.notes_screen = ui.notes_screen.filter(|i| *i < 64);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(index: usize, primary: bool) -> ScreenInfo {
        ScreenInfo { index, name: format!(r"\\.\DISPLAY{}", index + 1), width: 1920, height: 1080, primary }
    }

    #[test]
    fn presenter_view_puts_the_slides_on_the_other_screen() {
        let screens = vec![screen(0, true), screen(1, false)];
        assert_eq!(assign(&screens, None, None, true), (Some(1), Some(0)));
        assert_eq!(assign(&screens, Some(0), Some(1), true), (Some(0), Some(1)));
        assert_eq!(assign(&screens, Some(9), Some(8), true), (Some(1), Some(0)), "a missing screen falls back");
        assert_eq!(presenter_monitor(Some(1), Some(0)), Some(0));
        assert_eq!(presenter_monitor(Some(0), Some(0)), None, "the same screen keeps notes in a window");
    }

    #[test]
    fn a_show_without_presenter_view_stays_put_until_a_screen_is_saved() {
        let screens = vec![screen(0, true), screen(1, false)];
        assert_eq!(assign(&screens, None, Some(1), false), (None, None));
        assert_eq!(assign(&screens, Some(1), None, false), (Some(1), None));
        assert_eq!(assign(&[], None, None, true), (None, None));
    }

    #[test]
    fn names_hide_generic_display_paths() {
        assert_eq!(screen(0, true).label(), "Screen 1 (Primary, 1920×1080)");
        let named = ScreenInfo { index: 1, name: "Dell U2720Q".into(), width: 2560, height: 1440, primary: false };
        assert_eq!(named.label(), "Screen 2: Dell U2720Q (Extended, 2560×1440)");
    }

    #[test]
    fn a_saved_screen_past_the_list_is_dropped() {
        let mut ui = UiState::default();
        ui.show_screen = Some(99);
        ui.notes_screen = Some(1);
        let ui = ui.sanitized();
        assert_eq!(ui.show_screen, None);
        assert_eq!(ui.notes_screen, Some(1));
    }

    #[test]
    fn screen_settings_saves_the_screens_it_shows() {
        use egui_kittest::kittest::Queryable;
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.set_screens(vec![screen(0, true), screen(1, false)]);
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        let _ = harness.state_mut().run("app.dialog", serde_json::json!({"id": "screenSettings"}));
        harness.run_steps(6);
        harness.get_by_label("OK").click();
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none());
        assert_eq!(harness.state().ui.show_screen, Some(1));
        assert_eq!(harness.state().ui.notes_screen, Some(0));
    }

    #[test]
    fn starting_a_show_keeps_the_chosen_screens() {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.set_screens(vec![screen(0, true), screen(1, false)]);
        app.ui.show_screen = Some(0);
        app.ui.notes_screen = Some(1);
        app.ui.presenter_view = true;
        app.start_show(0, false);
        let show = app.show.as_ref();
        assert_eq!(show.map(|s| s.audience_screen), Some(Some(0)));
        assert_eq!(show.map(|s| s.notes_screen), Some(Some(1)));
    }
}
