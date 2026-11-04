//! Which screen plays the slides, and which screen shows Presenter View.
//!
//! Indices are positions in the host's monitor list, the same order as
//! `egui::ViewportCommand::SetMonitor`.

use crate::show::Show;
use crate::{SlideApp, UiState};

/// One screen the host can show a window on.
///
/// `x` and `y` are the monitor's top-left corner in physical pixels, in the same
/// space as `width` and `height`. Presenter View turns those into points with the
/// window's own scale, so a 125% display still fills the monitor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenInfo {
    pub index: usize,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// A monitor rectangle in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScreenFrame {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl ScreenInfo {
    pub(crate) fn frame(&self) -> ScreenFrame {
        ScreenFrame { x: self.x, y: self.y, width: self.width, height: self.height }
    }
}

/// The same rectangle in egui points for a window whose scale is `pixels_per_point`.
///
/// Multiplying the result by that scale returns the physical pixels, so the window
/// lands on the monitor instead of on a smaller point size.
pub(crate) fn frame_in_points(frame: ScreenFrame, pixels_per_point: f32) -> Option<(egui::Pos2, egui::Vec2)> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return None;
    }
    let x = frame.x as f32 / pixels_per_point;
    let y = frame.y as f32 / pixels_per_point;
    let w = frame.width as f32 / pixels_per_point;
    let h = frame.height as f32 / pixels_per_point;
    if ![x, y, w, h].into_iter().all(f32::is_finite) || w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some((egui::pos2(x, y), egui::vec2(w, h)))
}

/// Move and size this viewport so its physical pixels cover `frame`.
///
/// A point size such as 1100×700 is turned into physical pixels by the window scale.
/// At 125% that is only 1375×875 on a 1920×1200 monitor, so the notes window never
/// fills the screen. Correcting from the monitor's pixels, with this window's own
/// scale, makes the physical size match the monitor.
pub(crate) fn cover_monitor(ctx: &egui::Context, frame: ScreenFrame) {
    let Some((pos, size)) = frame_in_points(frame, ctx.pixels_per_point()) else { return };
    let (inner, outer) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().outer_rect));
    let Some(inner) = inner else { return };
    let apart = |a: f32, b: f32| (a - b).abs() > 1.5;
    let size_off = apart(inner.width(), size.x) || apart(inner.height(), size.y);
    let pos_off = outer.is_none_or(|r| apart(r.min.x, pos.x) || apart(r.min.y, pos.y));
    if !size_off && !pos_off {
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
    if pos_off {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
    if size_off {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }
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
        show.notes_frame = presenter_monitor(audience, notes).and_then(|index| self.screens.iter().find(|s| s.index == index).map(ScreenInfo::frame));
    }
}

/// The index named by a Screen Settings field, when it is one of `screens`.
pub(crate) fn chosen(raw: &str, screens: &[ScreenInfo]) -> Option<usize> {
    let index = raw.parse::<usize>().ok()?;
    screens.iter().any(|s| s.index == index).then_some(index)
}

/// Where the editing window was before the slides took it over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EditorPlace {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

/// The root window's position and size, before a show moves it onto another screen.
pub(crate) fn take_editor_place(ctx: &egui::Context) -> Option<EditorPlace> {
    ctx.input(|i| {
        let v = i.viewport();
        let outer = v.outer_rect?;
        let inner = v.inner_rect?;
        let (x, y, w, h) = (outer.min.x, outer.min.y, inner.width(), inner.height());
        if ![x, y, w, h].into_iter().all(f32::is_finite) || w < 1.0 || h < 1.0 {
            return None;
        }
        Some(EditorPlace { x, y, width: w, height: h, maximized: v.maximized.unwrap_or(false) })
    })
}

/// Leave fullscreen and put the editing window back at `place`.
pub(crate) fn restore_editor_place(ctx: &egui::Context, place: EditorPlace) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(place.x, place.y)));
    if place.maximized {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
    } else {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(place.width, place.height)));
    }
}

/// True when the window is out of fullscreen and sits on the saved screen again.
pub(crate) fn editor_place_matches(
    place: EditorPlace,
    outer_min: egui::Pos2,
    inner: egui::Vec2,
    maximized: Option<bool>,
    fullscreen: Option<bool>,
) -> bool {
    if fullscreen == Some(true) {
        return false;
    }
    let near = |a: f32, b: f32| (a - b).abs() < 24.0;
    if !near(outer_min.x, place.x) || !near(outer_min.y, place.y) {
        return false;
    }
    if place.maximized { maximized == Some(true) } else { maximized != Some(true) && near(inner.x, place.width) && near(inner.y, place.height) }
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
        ScreenInfo { index, name: format!(r"\\.\DISPLAY{}", index + 1), x: (index as i32) * 1920, y: 0, width: 1920, height: 1080, primary }
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
        let named = ScreenInfo { index: 1, name: "Dell U2720Q".into(), x: 1920, y: 0, width: 2560, height: 1440, primary: false };
        assert_eq!(named.label(), "Screen 2: Dell U2720Q (Extended, 2560×1440)");
    }

    #[test]
    fn the_editor_is_back_only_when_it_sits_where_it_started() {
        let windowed = EditorPlace { x: 40.0, y: 20.0, width: 1200.0, height: 800.0, maximized: false };
        assert!(editor_place_matches(windowed, egui::pos2(40.0, 20.0), egui::vec2(1200.0, 800.0), Some(false), Some(false)));
        assert!(!editor_place_matches(windowed, egui::pos2(2000.0, 20.0), egui::vec2(1200.0, 800.0), Some(false), Some(false)));
        assert!(!editor_place_matches(windowed, egui::pos2(40.0, 20.0), egui::vec2(1200.0, 800.0), Some(false), Some(true)));
        let maximized = EditorPlace { maximized: true, ..windowed };
        assert!(editor_place_matches(maximized, egui::pos2(40.0, 20.0), egui::vec2(1920.0, 1080.0), Some(true), Some(false)));
        assert!(!editor_place_matches(maximized, egui::pos2(40.0, 20.0), egui::vec2(1920.0, 1080.0), Some(false), Some(false)));
    }

    #[test]
    fn a_saved_screen_past_the_list_is_dropped() {
        let ui = UiState { show_screen: Some(99), notes_screen: Some(1), ..UiState::default() }.sanitized();
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
        assert_eq!(show.map(|s| s.notes_frame), Some(Some(ScreenFrame { x: 1920, y: 0, width: 1920, height: 1080 })));
    }

    #[test]
    fn notes_on_the_slides_screen_stay_in_a_window() {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.set_screens(vec![screen(0, true), screen(1, false)]);
        app.ui.show_screen = Some(0);
        app.ui.notes_screen = Some(0);
        app.ui.presenter_view = true;
        app.start_show(0, false);
        assert_eq!(app.show.as_ref().map(|s| s.notes_frame), Some(None));
    }

    #[test]
    fn a_scaled_monitor_is_covered_in_physical_pixels() {
        let frame = ScreenFrame { x: 0, y: 0, width: 1920, height: 1200 };
        let scaled = frame_in_points(frame, 1.25);
        let (pos, size) = scaled.unwrap_or((egui::pos2(0.0, 0.0), egui::vec2(0.0, 0.0)));
        assert!(scaled.is_some());
        assert_eq!(pos, egui::pos2(0.0, 0.0));
        assert!((size.x - 1536.0).abs() < 0.01 && (size.y - 960.0).abs() < 0.01);
        assert!((size.x * 1.25 - 1920.0).abs() < 0.01 && (size.y * 1.25 - 1200.0).abs() < 0.01);

        let beside = ScreenFrame { x: 1920, y: 0, width: 1920, height: 1200 };
        let (pos, _) = frame_in_points(beside, 1.25).unwrap_or((egui::pos2(0.0, 0.0), egui::vec2(0.0, 0.0)));
        assert!((pos.x * 1.25 - 1920.0).abs() < 0.01);
        assert!(frame_in_points(frame, 0.0).is_none());
    }
}
