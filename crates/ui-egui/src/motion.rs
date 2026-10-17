//! Animation effects: Preferences ▸ Animation effects and the `view.animationEffects` command.
//!
//! The window's own motion is what this switches:
//! - a wheel notch scrolls the slide, the thumbnails and every list eased in over a few frames,
//!   and Ctrl/⌘+wheel zooms the slide and the Slide Sorter the same way (egui's smoothing, which
//!   has no switch of its own);
//! - egui fades menus, tooltips and dialogs in and glides scroll areas to a target.
//!
//! With effects off a notch scrolls and zooms by the whole notch at once and nothing fades.
//! [`AnimationEffects::System`], the default, follows the system's setting, which the host reports
//! through [`crate::Services::animations_enabled`]: Windows' Animation effects, macOS' Reduce
//! motion, the Linux/BSD desktop's animations setting, a browser's `prefers-reduced-motion`.
//! Where the system says nothing, animations stay on.
//!
//! What a presentation plays (transitions, animations, animated GIFs, video) is its content, not
//! the window's motion, and is not affected: Set Up Slide Show ▸ Show without animation is the
//! switch for that.

use egui::{Event, InputOptions, InputState, MouseWheelUnit, TouchPhase, Vec2, style::ScrollAnimation};
use serde::{Deserialize, Serialize};

/// The setting's tooltip.
pub const HINT: &str = "Smooth scrolling and zooming, and menus, tooltips and dialogs that fade in";

/// The Animation effects preference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationEffects {
    /// Follow the system's animation setting.
    #[default]
    System,
    On,
    Off,
}

impl AnimationEffects {
    /// In the order Preferences lists them.
    pub const ALL: [Self; 3] = [Self::System, Self::On, Self::Off];

    /// The name saved in `ui.json` and taken by `view.animationEffects {mode}`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    /// The mode a [`code`](Self::code) names, ignoring case and surrounding spaces.
    pub fn parse(code: &str) -> Option<Self> {
        let code = code.trim();
        Self::ALL.into_iter().find(|mode| mode.code().eq_ignore_ascii_case(code))
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "Use system setting",
            Self::On => "On",
            Self::Off => "Off",
        }
    }

    /// Whether animations run, given what the system says (`None`: it says nothing, so they do).
    pub fn enabled(self, system: Option<bool>) -> bool {
        match self {
            Self::System => system.unwrap_or(true),
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// Reads the saved preference, treating anything unreadable as the default instead of failing the
/// whole [`crate::UiState`] (which would lose every other saved preference with it).
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<AnimationEffects, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Code(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Saved::deserialize(d)? {
        Saved::Code(code) => AnimationEffects::parse(&code).unwrap_or_default(),
        Saved::Other(_) => AnimationEffects::default(),
    })
}

/// Makes egui follow Animation effects. Called once a frame, before the window is laid out.
///
/// Off: egui's own fades and gliding scroll areas stop, and from the next pass on, wheel scrolling
/// arrives whole in every window. On: egui's settings are put back as they were. A context that
/// never animated (the headless test harness) is left that way.
pub fn apply(ctx: &egui::Context, on: bool) {
    let saved = ctx.data(|d| d.get_temp::<State>(State::id()));
    if saved.is_none() {
        ctx.add_plugin(WholeWheelScroll);
    }
    let mut state = saved.unwrap_or_default();
    state.off = !on;
    if on {
        if let Some(was) = state.held.take() {
            was.set(ctx);
        }
    } else {
        let now = EguiMotion::of(ctx);
        if now != EguiMotion::none() {
            // Only the settings from before the first Off are kept; any seen later are egui's
            // style being reset while effects are off.
            state.held.get_or_insert(now);
            EguiMotion::none().set(ctx);
        }
    }
    ctx.data_mut(|d| d.insert_temp(State::id(), state));
}

/// This frame's zoom factor from Ctrl/⌘+wheel and pinches (1.0: none), for the canvas and the
/// Slide Sorter. egui eases a wheel notch's zoom in over a few frames; with animation effects off
/// the whole notch is applied at once.
pub fn zoom_delta(ctx: &egui::Context, animate: bool) -> f32 {
    if animate {
        return ctx.input(|i| i.zoom_delta());
    }
    let options = ctx.options(|o| o.input_options);
    ctx.input(|i| unsmoothed_zoom(i, &options))
}

/// What [`apply`] keeps in the egui context from one frame to the next.
#[derive(Clone, Copy, Debug, Default)]
struct State {
    /// Animation effects were off at the last [`apply`].
    off: bool,
    /// egui's own animation settings, kept while they are switched off.
    held: Option<EguiMotion>,
}

impl State {
    fn id() -> egui::Id {
        egui::Id::new("deckcraft_ui_egui::motion::State")
    }
}

/// While animation effects are off, replaces egui's eased wheel scrolling with the whole notch.
/// egui calls it at the start of each pass of each viewport (the main window, presenter view, New
/// Window), before any widget reads the scrolling.
struct WholeWheelScroll;

impl egui::plugin::Plugin for WholeWheelScroll {
    fn debug_name(&self) -> &'static str {
        "deckcraft whole wheel scroll"
    }

    fn on_begin_pass(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx();
        if ctx.data(|d| d.get_temp::<State>(State::id())).is_some_and(|state| state.off) {
            let options = ctx.options(|o| o.input_options);
            ctx.input_mut(|i| i.smooth_scroll_delta = unsmoothed_scroll(i, &options));
        }
    }
}

/// egui's own animation settings: how long fades take and how scroll areas glide.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EguiMotion {
    animation_time: f32,
    scroll_animation: ScrollAnimation,
}

impl EguiMotion {
    fn none() -> Self {
        EguiMotion { animation_time: 0.0, scroll_animation: ScrollAnimation::none() }
    }

    fn of(ctx: &egui::Context) -> Self {
        let style = ctx.global_style();
        EguiMotion { animation_time: style.animation_time, scroll_animation: style.scroll_animation }
    }

    /// Sets both the light and the dark style.
    fn set(self, ctx: &egui::Context) {
        ctx.all_styles_mut(|style| {
            style.animation_time = self.animation_time;
            style.scroll_animation = self.scroll_animation;
        });
        ctx.request_repaint();
    }
}

/// A wheel event's delta in points, counted as egui counts it: a line is `line_scroll_speed`
/// points and a page is the window's height.
fn wheel_points(unit: MouseWheelUnit, delta: Vec2, input: &InputState, options: &InputOptions) -> Vec2 {
    let points = match unit {
        MouseWheelUnit::Point => delta,
        MouseWheelUnit::Line => delta * options.line_scroll_speed,
        MouseWheelUnit::Page => delta * input.viewport_rect().height(),
    };
    if points.is_finite() { points } else { Vec2::ZERO }
}

/// This frame's wheel scrolling with nothing held back for later frames, in place of
/// [`InputState::smooth_scroll_delta`], which eases each notch in. Ctrl/⌘+wheel is left out, as
/// egui leaves it out: it zooms.
fn unsmoothed_scroll(input: &InputState, options: &InputOptions) -> Vec2 {
    let mut total = Vec2::ZERO;
    for event in &input.events {
        if let Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers } = event
            && !modifiers.matches_any(options.zoom_modifier)
        {
            let points = wheel_points(*unit, *delta, input, options);
            // Shift scrolls sideways and Alt vertically, as in egui.
            let sideways = modifiers.matches_any(options.horizontal_scroll_modifier);
            let vertical = modifiers.matches_any(options.vertical_scroll_modifier);
            total += match (sideways, vertical) {
                (true, false) => Vec2::new(points.x + points.y, 0.0),
                (false, true) => Vec2::new(0.0, points.x + points.y),
                _ => points,
            };
        }
    }
    if total.is_finite() { total } else { Vec2::ZERO }
}

/// This frame's zoom factor with each Ctrl/⌘+wheel notch applied whole, in place of
/// [`InputState::zoom_delta`]. Fingers on a touch screen are measured, never eased, so a pinch
/// counts as egui measures it.
fn unsmoothed_zoom(input: &InputState, options: &InputOptions) -> f32 {
    if let Some(touch) = input.multi_touch() {
        return if touch.zoom_delta.is_finite() && touch.zoom_delta > 0.0 { touch.zoom_delta } else { 1.0 };
    }
    let mut zoom = 1.0_f32;
    for event in &input.events {
        match event {
            Event::Zoom(factor) => zoom *= factor,
            Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers } if modifiers.matches_any(options.zoom_modifier) => {
                let points = wheel_points(*unit, *delta, input, options);
                zoom *= (options.scroll_zoom_speed * (points.x + points.y)).exp();
            }
            _ => {}
        }
    }
    if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Modifiers, vec2};

    fn wheel(unit: MouseWheelUnit, delta: Vec2, modifiers: Modifiers) -> Event {
        Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers }
    }

    /// What the two readers make of one frame's events. The events are handed over as they
    /// are: egui's own smoothing, which a debug build stops at a NaN, isn't run on them.
    fn read(events: Vec<Event>) -> (Vec2, f32) {
        let mut input = InputState::default();
        input.events = events;
        let options = InputOptions::default();
        (unsmoothed_scroll(&input, &options), unsmoothed_zoom(&input, &options))
    }

    #[test]
    fn the_setting_reads_writes_and_follows_the_system() {
        assert_eq!(AnimationEffects::default(), AnimationEffects::System);
        for mode in AnimationEffects::ALL {
            assert_eq!(AnimationEffects::parse(mode.code()), Some(mode));
            assert_eq!(serde_json::to_value(mode).ok(), Some(serde_json::json!(mode.code())));
        }
        assert_eq!(AnimationEffects::parse(" OFF "), Some(AnimationEffects::Off));
        assert_eq!(AnimationEffects::parse("reduced"), None);
        assert_eq!(AnimationEffects::parse(""), None);
        // Use system setting follows the system, and animates when the system says nothing.
        assert!(AnimationEffects::System.enabled(None) && AnimationEffects::System.enabled(Some(true)));
        assert!(!AnimationEffects::System.enabled(Some(false)));
        // On and Off don't ask.
        assert!(AnimationEffects::On.enabled(Some(false)) && AnimationEffects::On.enabled(None));
        assert!(!AnimationEffects::Off.enabled(Some(true)) && !AnimationEffects::Off.enabled(None));
    }

    #[test]
    fn an_unreadable_saved_value_is_the_default() {
        #[derive(Deserialize)]
        struct Saved {
            #[serde(deserialize_with = "lenient")]
            mode: AnimationEffects,
        }
        let read = |json: &str| serde_json::from_str::<Saved>(json).map(|s| s.mode).ok();
        assert_eq!(read(r#"{"mode": "off"}"#), Some(AnimationEffects::Off));
        assert_eq!(read(r#"{"mode": "On"}"#), Some(AnimationEffects::On));
        for unreadable in [r#""reduced""#, "true", "3", "null", "[]", "{}"] {
            assert_eq!(read(&format!(r#"{{"mode": {unreadable}}}"#)), Some(AnimationEffects::System), "{unreadable}");
        }
    }

    #[test]
    fn a_notch_scrolls_whole_and_keeps_eguis_axes() {
        let line = InputOptions::default().line_scroll_speed;
        assert_eq!(read(vec![]), (Vec2::ZERO, 1.0));
        // One notch down, then two more in the same frame: all of it, now.
        assert_eq!(read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::NONE)]).0, vec2(0.0, -line));
        let three =
            vec![wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::NONE), wheel(MouseWheelUnit::Line, vec2(0.0, -2.0), Modifiers::NONE)];
        assert_eq!(read(three).0, vec2(0.0, -3.0 * line));
        // Points come as they are, a page is the window's height.
        assert_eq!(read(vec![wheel(MouseWheelUnit::Point, vec2(3.0, -7.5), Modifiers::NONE)]).0, vec2(3.0, -7.5));
        let page = InputState::default().viewport_rect().height();
        assert_eq!(read(vec![wheel(MouseWheelUnit::Page, vec2(0.0, 1.0), Modifiers::NONE)]).0, vec2(0.0, page));
        // Shift turns the wheel sideways.
        assert_eq!(read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::SHIFT)]).0, vec2(-line, 0.0));
        // A gesture's start and end carry no scrolling.
        let ended = Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, 50.0), phase: TouchPhase::End, modifiers: Modifiers::NONE };
        assert_eq!(read(vec![ended]).0, Vec2::ZERO);
    }

    #[test]
    fn ctrl_wheel_zooms_whole_and_never_scrolls() {
        let options = InputOptions::default();
        let notch = (options.scroll_zoom_speed * options.line_scroll_speed).exp();
        let (scroll, zoom) = read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, 1.0), Modifiers::COMMAND)]);
        assert_eq!(scroll, Vec2::ZERO);
        assert!((zoom - notch).abs() < 1e-6 && zoom > 1.1, "{zoom}");
        let (_, out) = read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::COMMAND)]);
        assert!((out * notch - 1.0).abs() < 1e-5, "a notch back undoes it: {out}");
        // A pinch the system reports as one follows the fingers.
        assert_eq!(read(vec![Event::Zoom(1.25)]).1, 1.25);
        // Wheel and pinch without Ctrl: one scrolls, the other zooms.
        let (scroll, zoom) = read(vec![wheel(MouseWheelUnit::Line, vec2(0.0, 1.0), Modifiers::NONE), Event::Zoom(0.5)]);
        assert!(scroll.y > 0.0 && zoom == 0.5);
    }

    #[test]
    fn hostile_wheel_numbers_scroll_and_zoom_nothing() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN] {
            for unit in [MouseWheelUnit::Point, MouseWheelUnit::Line, MouseWheelUnit::Page] {
                for modifiers in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::COMMAND] {
                    let (scroll, zoom) = read(vec![wheel(unit, vec2(bad, bad), modifiers), wheel(unit, vec2(0.0, bad), modifiers)]);
                    assert!(scroll.is_finite(), "{bad} {unit:?}: {scroll:?}");
                    assert!(zoom.is_finite() && zoom > 0.0, "{bad} {unit:?}: {zoom}");
                }
            }
            let zoom = read(vec![Event::Zoom(bad), Event::Zoom(0.0), Event::Zoom(-2.0)]).1;
            assert_eq!(zoom, 1.0, "{bad}");
        }
    }

    #[test]
    fn eguis_own_motion_goes_off_and_comes_back_as_it_was() {
        let ctx = egui::Context::default();
        let before = EguiMotion::of(&ctx);
        assert!(before.animation_time > 0.0);
        // On from the start: nothing changes.
        apply(&ctx, true);
        assert_eq!(EguiMotion::of(&ctx), before);
        // Off: both styles (light and dark) stand still, every frame.
        apply(&ctx, false);
        apply(&ctx, false);
        assert_eq!(EguiMotion::of(&ctx), EguiMotion::none());
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            assert_eq!(ctx.style_of(theme).animation_time, 0.0);
        }
        // Something sets a style's animations again while they are off: off they stay.
        ctx.all_styles_mut(|style| style.animation_time = 0.5);
        apply(&ctx, false);
        assert_eq!(EguiMotion::of(&ctx), EguiMotion::none());
        // On again: as they were before the first Off.
        apply(&ctx, true);
        assert_eq!(EguiMotion::of(&ctx), before);
    }

    /// The scrolling is settled at the start of every pass of every window, so an egui scroll area
    /// (the thumbnails, a list, presenter view's notes) takes a whole notch at once too.
    #[test]
    fn with_effects_off_a_scroll_area_takes_a_whole_notch_at_once() {
        let scrolled_after_a_notch = |on: bool| {
            let mut h = egui_kittest::Harness::builder().with_size(vec2(300.0, 200.0)).with_step_dt(1.0 / 60.0).build_ui_state(
                |ui, offset: &mut f32| {
                    apply(ui.ctx(), on);
                    let area = egui::ScrollArea::vertical().show(ui, |ui| ui.allocate_space(vec2(100.0, 2000.0)));
                    *offset = area.state.offset.y;
                },
                0.0,
            );
            h.run_steps(2);
            h.input_mut().events.push(Event::PointerMoved(egui::pos2(50.0, 50.0)));
            h.step();
            h.input_mut().events.push(wheel(MouseWheelUnit::Line, vec2(0.0, -1.0), Modifiers::NONE));
            h.run_steps(2);
            let early = *h.state();
            h.run_steps(60);
            (early, *h.state())
        };
        let line = InputOptions::default().line_scroll_speed;
        let (early, settled) = scrolled_after_a_notch(true);
        assert!(early < settled * 0.9 && (settled - line).abs() < 1.0, "on: eased in ({early}, then {settled})");
        let (early, settled) = scrolled_after_a_notch(false);
        assert_eq!((early, settled), (line, line), "off: the whole notch at once, and nothing trails in");
    }

    #[test]
    fn a_context_that_never_animated_is_left_alone() {
        let ctx = egui::Context::default();
        EguiMotion::none().set(&ctx);
        for on in [false, true, false, true] {
            apply(&ctx, on);
            assert_eq!(EguiMotion::of(&ctx), EguiMotion::none(), "on: {on}");
        }
    }
}
