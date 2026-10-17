//! Preferences ▸ Animation effects: Use system setting, On, Off. Off (chosen, or the system's own
//! setting while the preference follows it) stops the window's own motion: wheel scrolling and
//! zooming land at once and egui's fades are off.

use std::cell::Cell;
use std::rc::Rc;

use deckcraft_engine::Session;
use deckcraft_ui_egui::motion::AnimationEffects;
use deckcraft_ui_egui::{Services, SlideApp, UiState, ViewMode};
use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SlideApp>;

/// What the system says about its animation effects, switched by the test.
type System = Rc<Cell<Option<bool>>>;

/// A window on a new presentation whose host reports `system`. The harness has egui's own
/// animations off; `animated` puts them on, as a real window has them.
fn harness(system: &System, animated: bool) -> Harness {
    let asked = system.clone();
    let services = Services { animations_enabled: Some(Box::new(move || asked.get())), ..Default::default() };
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), services),
    );
    // The app keeps requesting repaints, so step frames instead of run().
    h.run_steps(4);
    if animated {
        h.ctx.all_styles_mut(|s| s.animation_time = 0.1);
        h.step();
    }
    h
}

fn set(h: &mut Harness, mode: AnimationEffects) {
    h.state_mut().run("view.animationEffects", json!({"mode": mode.code()})).unwrap();
    h.step();
}

/// Turns the wheel over the middle of the canvas: `lines` notches (negative scrolls down).
fn wheel(h: &mut Harness, lines: f32, modifiers: Modifiers) {
    let at = h.state().canvas_rect.unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(at));
    h.step();
    h.input_mut().events.push(Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, lines), phase: TouchPhase::Move, modifiers });
    h.step();
}

/// How far the slide has been scrolled down in the canvas.
fn scrolled(h: &Harness, top: f32) -> f32 {
    top - h.state().slide_rect.unwrap().min.y
}

fn zoom(h: &Harness) -> f32 {
    h.state().ui.zoom.unwrap()
}

/// One wheel notch of zoom: e^(40 points / 200).
const NOTCH: f32 = 1.221_402_8;

#[test]
fn the_default_follows_the_system_and_on_and_off_overrule_it() {
    let system = System::default();
    let mut h = harness(&system, true);
    assert_eq!(h.state().ui.animation_effects, AnimationEffects::System);
    // The system says nothing (a platform that can't be asked, or no answer yet): on.
    assert!(h.state().animations_on() && h.state().system_animations().is_none());
    assert_eq!(h.ctx.global_style().animation_time, 0.1);
    // Animation effects are switched off in the system's settings while the app runs.
    system.set(Some(false));
    h.step();
    assert_eq!(h.state().system_animations(), Some(false));
    assert!(!h.state().animations_on());
    assert_eq!(h.state().ui.animation_effects, AnimationEffects::System, "the preference itself is untouched");
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let style = h.ctx.style_of(theme);
        assert_eq!(style.animation_time, 0.0, "off: menus, tooltips and dialogs don't fade in");
        assert_eq!(style.scroll_animation, egui::style::ScrollAnimation::none());
    }
    // ...and back on: egui's animations are as they were.
    system.set(Some(true));
    h.step();
    assert!(h.state().animations_on());
    assert_eq!(h.ctx.global_style().animation_time, 0.1);
    // On and Off don't ask the system.
    system.set(Some(false));
    set(&mut h, AnimationEffects::On);
    assert!(h.state().animations_on());
    assert_eq!(h.ctx.global_style().animation_time, 0.1);
    system.set(Some(true));
    set(&mut h, AnimationEffects::Off);
    assert!(!h.state().animations_on());
    assert_eq!(h.ctx.global_style().animation_time, 0.0);
    // Dark Mode restyles the window; the fades stay off.
    h.state_mut().run("view.dark", json!({"on": true})).unwrap();
    h.run_steps(2);
    assert_eq!(h.ctx.global_style().animation_time, 0.0);
    // Back to following it.
    set(&mut h, AnimationEffects::System);
    assert!(h.state().animations_on());
    assert_eq!(h.ctx.global_style().animation_time, 0.1);
}

#[test]
fn a_headless_window_never_gains_animations_from_the_setting() {
    // The harness (and an offscreen render) has egui's animations off; On leaves that alone.
    let system = System::default();
    let mut h = harness(&system, false);
    for mode in [AnimationEffects::On, AnimationEffects::Off, AnimationEffects::On, AnimationEffects::System] {
        set(&mut h, mode);
        h.step();
        assert_eq!(h.ctx.global_style().animation_time, 0.0, "{mode:?}");
    }
}

#[test]
fn the_command_takes_the_three_modes_and_refuses_others() {
    let system = System::default();
    let mut h = harness(&system, false);
    system.set(Some(false));
    let app = h.state_mut();
    // The answer says what was set, what it comes to, and what the system says.
    assert_eq!(app.run("view.animationEffects", json!({"mode": "system"})), Ok(json!({"mode": "system", "on": false, "system": false})));
    assert_eq!(app.run("view.animationEffects", json!({"mode": "on"})), Ok(json!({"mode": "on", "on": true, "system": false})));
    assert_eq!(app.run("view.animationEffects", json!({"mode": " OFF "})), Ok(json!({"mode": "off", "on": false, "system": false})));
    for bad in [json!("reduced"), json!(""), json!(1), json!([]), json!({"mode": "on"})] {
        assert!(app.run("view.animationEffects", json!({"mode": bad})).is_err(), "{bad}");
        assert_eq!(app.ui.animation_effects, AnimationEffects::Off, "a refused mode changes nothing");
    }
    // A `mode` of null is no mode (clients that write every optional field).
    assert_eq!(app.run("view.animationEffects", json!({"mode": null, "on": true})), Ok(json!({"mode": "on", "on": true, "system": false})));
    assert_eq!(app.run("view.animationEffects", json!({"on": false})), Ok(json!({"mode": "off", "on": false, "system": false})));
    // Without parameters it toggles what shows: following a system that has them off, to On.
    app.run("view.animationEffects", json!({"mode": "system"})).unwrap();
    assert_eq!(app.run("view.animationEffects", json!({})), Ok(json!({"mode": "on", "on": true, "system": false})));
    assert!(app.dialog.is_none());
    // It is listed with the other commands, and `ui.inspect` says what it comes to.
    let commands = deckcraft_ui_egui::control::all_commands(h.state());
    assert!(commands.as_array().unwrap().iter().any(|c| c["id"] == "view.animationEffects"));
    h.state_mut().run("view.animationEffects", json!({"mode": "system"})).unwrap();
    let seen = deckcraft_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!((&seen["animationsOn"], &seen["systemAnimations"], &seen["ui"]["animationEffects"]), (&json!(false), &json!(false), &json!("system")));
}

/// The control channel's `ui.set` takes a mode or a switch, and reports a value it can't use.
#[test]
fn ui_set_takes_animation_effects() {
    use deckcraft_ui_egui::control::{self, ControlRequest, Outcome};
    let system = System::default();
    let mut h = harness(&system, false);
    let mut ui_set = |effects: serde_json::Value| {
        let (request, _reply) = ControlRequest::new("ui.set", json!({"animationEffects": effects}));
        let ctx = h.ctx.clone();
        match control::handle(h.state_mut(), &ctx, &request) {
            Outcome::Done(reply) => reply["ok"] == json!(true),
            Outcome::Screenshot { .. } => panic!("ui.set doesn't take a screenshot"),
        }
    };
    assert!(ui_set(json!("off")));
    assert!(ui_set(json!(true)));
    assert!(!ui_set(json!("reduced")) && !ui_set(json!(3)), "a value it can't use is an error");
    assert_eq!(h.state().ui.animation_effects, AnimationEffects::On, "and changes nothing");
}

#[test]
fn the_choice_is_saved_and_an_older_or_damaged_file_still_loads() {
    let ui = UiState { animation_effects: AnimationEffects::Off, ..Default::default() };
    let saved = serde_json::to_value(&ui).unwrap();
    assert_eq!(saved["animationEffects"], "off");
    assert_eq!(serde_json::from_value::<UiState>(saved).unwrap().animation_effects, AnimationEffects::Off);
    // A `ui.json` from before the setting: Use system setting, and nothing else is lost.
    let older: UiState = serde_json::from_value(json!({"tab": "Insert", "notes": false})).unwrap();
    assert_eq!((older.animation_effects, older.tab.as_str(), older.notes), (AnimationEffects::System, "Insert", false));
    // A value this version can't read doesn't cost the other preferences.
    for unreadable in [json!("reduced"), json!(true), json!(7), json!(null), json!({"mode": "off"})] {
        let ui: UiState = serde_json::from_value(json!({"tab": "Design", "animationEffects": unreadable})).unwrap();
        assert_eq!((ui.animation_effects, ui.tab.as_str()), (AnimationEffects::System, "Design"));
    }
}

#[test]
fn off_scrolls_a_wheel_notch_at_once_and_on_eases_it_in() {
    let system = System::default();
    // Zoomed in far enough that the slide scrolls both ways.
    let zoomed = |system: &System| {
        let mut h = harness(system, true);
        h.state_mut().ui.zoom = Some(4.0);
        h.run_steps(2);
        let top = h.state().slide_rect.unwrap().min.y;
        (h, top)
    };
    let (mut eased, top) = zoomed(&system);
    wheel(&mut eased, -1.0, Modifiers::NONE);
    let first = scrolled(&eased, top);
    eased.run_steps(90);
    let whole = scrolled(&eased, top);
    assert!(first > 0.0 && first < whole * 0.6, "on: the notch arrives over several frames ({first} of {whole})");

    let (mut h, top) = zoomed(&system);
    set(&mut h, AnimationEffects::Off);
    wheel(&mut h, -1.0, Modifiers::NONE);
    let at_once = scrolled(&h, top);
    assert!((at_once - 40.0).abs() < 0.01, "off: the whole notch in the frame the wheel turned ({at_once})");
    // To the pixel: the canvas puts the slide on whole device pixels (#184).
    assert!((at_once - whole).abs() <= 1.0, "the same distance as when it is eased ({at_once}, {whole})");
    h.run_steps(30);
    assert_eq!(scrolled(&h, top), at_once, "and nothing trails in afterwards");
    // The same when it is the system that has animations off.
    set(&mut h, AnimationEffects::System);
    system.set(Some(false));
    wheel(&mut h, -1.0, Modifiers::NONE);
    assert!((scrolled(&h, top) - 80.0).abs() < 0.01, "{}", scrolled(&h, top));
    // Shift turns the wheel sideways, whole as well.
    let left = h.state().slide_rect.unwrap().min.x;
    wheel(&mut h, -1.0, Modifiers::SHIFT);
    let sideways = left - h.state().slide_rect.unwrap().min.x;
    assert!((sideways - 40.0).abs() < 0.01, "{sideways}");
    h.run_steps(30);
    assert_eq!(left - h.state().slide_rect.unwrap().min.x, sideways);
    assert!((scrolled(&h, top) - 80.0).abs() < 0.01, "Shift+wheel doesn't scroll down");
}

#[test]
fn off_zooms_a_ctrl_wheel_notch_at_once_and_on_eases_it_in() {
    let system = System::default();
    let at_100 = |system: &System| {
        let mut h = harness(system, true);
        h.state_mut().ui.zoom = Some(1.0);
        h.run_steps(2);
        h
    };
    let mut eased = at_100(&system);
    wheel(&mut eased, 1.0, Modifiers::COMMAND);
    let first = zoom(&eased);
    eased.run_steps(90);
    assert!(first > 1.0 && first < zoom(&eased), "on: the zoom arrives over several frames ({first}, then {})", zoom(&eased));

    let mut h = at_100(&system);
    let top = h.state().slide_rect.unwrap().min.y;
    set(&mut h, AnimationEffects::Off);
    wheel(&mut h, 1.0, Modifiers::COMMAND);
    assert!((zoom(&h) - NOTCH).abs() < 1e-4, "off: the whole notch in the frame the wheel turned ({})", zoom(&h));
    h.run_steps(30);
    assert!((zoom(&h) - NOTCH).abs() < 1e-4, "and nothing trails in afterwards ({})", zoom(&h));
    wheel(&mut h, -1.0, Modifiers::COMMAND);
    assert!((zoom(&h) - 1.0).abs() < 1e-4, "a notch back undoes it ({})", zoom(&h));
    // The slide is laid out at the new zoom in the next frame.
    h.step();
    assert!((h.state().slide_rect.unwrap().min.y - top).abs() < 0.01, "Ctrl+wheel zooms; it doesn't scroll");

    // The Slide Sorter's thumbnails zoom the same way.
    h.state_mut().ui.view = ViewMode::Sorter;
    h.run_steps(2);
    assert_eq!(h.state().ui.sorter_zoom, 1.0);
    h.input_mut().events.push(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: vec2(0.0, 1.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::COMMAND,
    });
    h.step();
    assert!((h.state().ui.sorter_zoom - NOTCH).abs() < 1e-4, "{}", h.state().ui.sorter_zoom);
    h.run_steps(30);
    assert!((h.state().ui.sorter_zoom - NOTCH).abs() < 1e-4, "{}", h.state().ui.sorter_zoom);
}

#[test]
fn preferences_offers_use_system_setting_on_and_off() {
    let system = System::default();
    let mut h = harness(&system, false);
    h.state_mut().run("app.preferences", json!({})).unwrap();
    h.run_steps(3);
    h.get_by_label("Animation effects:");
    // What the box shows: the only combo box in Preferences.
    let shown = |h: &Harness| h.get_by_role(egui::accesskit::Role::ComboBox).accesskit_node().value().unwrap_or_default();
    assert_eq!(shown(&h), "Use system setting", "what it reads until something is chosen");
    h.get_by_label("Your system doesn't report an animation setting, so effects stay on.");
    system.set(Some(false));
    h.run_steps(2);
    h.get_by_label("Animation effects are off in your system's settings.");
    for (label, mode) in [("Off", AnimationEffects::Off), ("On", AnimationEffects::On), ("Use system setting", AnimationEffects::System)] {
        h.get_by_role(egui::accesskit::Role::ComboBox).click();
        h.run_steps(3);
        h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
        h.run_steps(3);
        assert_eq!(h.state().ui.animation_effects, mode, "{label}");
        assert_eq!(shown(&h), label);
        assert!(h.state().dialog.as_ref().is_some_and(|d| d.id == "preferences"), "choosing keeps Preferences open");
    }
    assert!(!h.state().animations_on(), "following a system that has them off");
}

/// Preferences applies a change as it is made; Cancel puts the choice back, Done keeps it.
#[test]
fn preferences_cancel_restores_the_choice_and_done_keeps_it() {
    let system = System::default();
    let mut h = harness(&system, false);
    let choose_off_then = |h: &mut Harness, button: &str| {
        h.state_mut().run("app.preferences", json!({})).unwrap();
        h.run_steps(3);
        h.get_by_role(egui::accesskit::Role::ComboBox).click();
        h.run_steps(3);
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Off").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.animation_effects, AnimationEffects::Off, "the choice applies at once");
        h.get_by_role_and_label(egui::accesskit::Role::Button, button).click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none(), "{button} closes Preferences");
    };
    choose_off_then(&mut h, "Cancel");
    assert_eq!(h.state().ui.animation_effects, AnimationEffects::System, "Cancel puts the choice back");
    choose_off_then(&mut h, "Done");
    assert_eq!(h.state().ui.animation_effects, AnimationEffects::Off, "Done keeps it");
}
