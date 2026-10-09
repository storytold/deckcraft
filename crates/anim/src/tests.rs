#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use deckcraft_model::anim::{ANIMATIONS, TRANSITIONS, TextBuild};
use deckcraft_model::{
    AnimClass, AnimStart, Animation, ColorRef, CustomShow, Presentation, Rgba, Shape, ShapeId, Slide, SlideId, TextBody, Transition, Xfrm,
};

use crate::*;

const W: f64 = 960.0;
const H: f64 = 540.0;

fn shape(id: u32, name: &str) -> Shape {
    Shape { id: ShapeId(id), name: name.into(), xfrm: Some(Xfrm::new(100.0, 100.0, 200.0, 100.0)), ..Default::default() }
}

fn anim(shape: u32, class: AnimClass, effect: &str, start: AnimStart) -> Animation {
    Animation { shape: ShapeId(shape), class, effect: effect.into(), start, duration_ms: 1000, ..Default::default() }
}

fn slide_with(anims: Vec<Animation>) -> Slide {
    Slide { shapes: (1..=4).map(|i| shape(i, &format!("S{i}"))).collect(), animations: anims, ..Default::default() }
}

fn finite(s: &AnimState) -> bool {
    s.opacity.is_finite()
        && s.offset_x.is_finite()
        && s.offset_y.is_finite()
        && s.scale_x.is_finite()
        && s.scale_y.is_finite()
        && s.rotate.is_finite()
        && s.clip.is_none_or(|c| c.iter().all(|v| v.is_finite()))
}

#[test]
fn default_state_is_identity() {
    let s = AnimState::default();
    assert!(s.visible && s.opacity == 1.0 && s.scale_x == 1.0 && s.scale_y == 1.0);
    assert!(s.is_identity());
}

#[test]
fn on_click_starts_steps() {
    let tl = Timeline::new(
        &slide_with(vec![
            anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick),
            anim(2, AnimClass::Entrance, "fade", AnimStart::OnClick),
            anim(3, AnimClass::Entrance, "fade", AnimStart::OnClick),
        ]),
        W,
        H,
    );
    assert_eq!(tl.steps(), 3);
    assert!(!tl.step_is_auto(0));
    assert!((tl.step_duration(1) - 1.0).abs() < 1e-9);
}

#[test]
fn with_previous_joins_step() {
    let tl = Timeline::new(
        &slide_with(vec![anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick), anim(2, AnimClass::Entrance, "fade", AnimStart::WithPrevious)]),
        W,
        H,
    );
    assert_eq!(tl.steps(), 1);
    let e = tl.effects();
    assert_eq!(e[1].step, 0);
    assert_eq!(e[1].start, 0.0);
    assert!((tl.step_duration(0) - 1.0).abs() < 1e-9);
}

#[test]
fn after_previous_chains() {
    let tl = Timeline::new(
        &slide_with(vec![
            anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick),
            anim(2, AnimClass::Entrance, "fade", AnimStart::AfterPrevious),
            anim(3, AnimClass::Entrance, "fade", AnimStart::AfterPrevious),
        ]),
        W,
        H,
    );
    assert_eq!(tl.steps(), 1);
    let e = tl.effects();
    assert!((e[1].start - 1.0).abs() < 1e-9);
    assert!((e[2].start - 2.0).abs() < 1e-9);
    assert!((tl.step_duration(0) - 3.0).abs() < 1e-9);
}

#[test]
fn delays_shift_start() {
    let mut a = anim(2, AnimClass::Entrance, "fade", AnimStart::WithPrevious);
    a.delay_ms = 250;
    let mut b = anim(3, AnimClass::Entrance, "fade", AnimStart::AfterPrevious);
    b.delay_ms = 500;
    let tl = Timeline::new(&slide_with(vec![anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick), a, b]), W, H);
    let e = tl.effects();
    assert!((e[1].start - 0.25).abs() < 1e-9);
    // After previous: after the latest end (1.25) plus its delay.
    assert!((e[2].start - 1.75).abs() < 1e-9);
    assert!((tl.step_duration(0) - 2.75).abs() < 1e-9);
}

#[test]
fn auto_first_step() {
    let tl = Timeline::new(
        &slide_with(vec![anim(1, AnimClass::Entrance, "fade", AnimStart::AfterPrevious), anim(2, AnimClass::Entrance, "fade", AnimStart::OnClick)]),
        W,
        H,
    );
    assert_eq!(tl.steps(), 2);
    assert!(tl.step_is_auto(0));
    assert!(!tl.step_is_auto(1));
}

#[test]
fn hidden_before_entrance() {
    let tl = Timeline::new(&slide_with(vec![anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick)]), W, H);
    assert!(!tl.state(ShapeId(1), None, 0, 0.0).visible);
    let mid = tl.state(ShapeId(1), None, 0, 0.5);
    assert!(mid.visible && mid.opacity > 0.0 && mid.opacity < 1.0);
    let done = tl.state(ShapeId(1), None, 1, 0.0);
    assert!(done.visible && (done.opacity - 1.0).abs() < 1e-9);
    // Unanimated shapes are untouched.
    assert!(tl.state(ShapeId(2), None, 0, 0.0).is_identity());
}

#[test]
fn hidden_after_exit() {
    let tl = Timeline::new(&slide_with(vec![anim(1, AnimClass::Exit, "fadeOut", AnimStart::OnClick)]), W, H);
    assert!(tl.state(ShapeId(1), None, 0, 0.0).visible);
    let mid = tl.state(ShapeId(1), None, 0, 0.5);
    assert!(mid.visible && mid.opacity < 1.0);
    assert!(!tl.state(ShapeId(1), None, 1, 0.0).visible);
    assert!(!tl.state(ShapeId(1), None, 0, 5.0).visible);
}

#[test]
fn entrance_then_exit() {
    let tl = Timeline::new(
        &slide_with(vec![anim(1, AnimClass::Entrance, "appear", AnimStart::OnClick), anim(1, AnimClass::Exit, "disappear", AnimStart::OnClick)]),
        W,
        H,
    );
    assert!(!tl.state(ShapeId(1), None, 0, 0.0).visible);
    assert!(tl.state(ShapeId(1), None, 1, 0.0).visible);
    assert!(!tl.state(ShapeId(1), None, 2, 0.0).visible);
}

#[test]
fn emphasis_returns_or_persists() {
    let mut spin = anim(1, AnimClass::Emphasis, "spin", AnimStart::OnClick);
    spin.amount = Some(90.0);
    let tl = Timeline::new(&slide_with(vec![anim(2, AnimClass::Emphasis, "pulse", AnimStart::OnClick), spin]), W, H);
    let p = tl.state(ShapeId(2), None, 0, 0.5);
    assert!(p.scale_x > 1.0);
    assert!(tl.state(ShapeId(2), None, 1, 0.0).is_identity());
    assert!((tl.state(ShapeId(1), None, 2, 0.0).rotate - 90.0).abs() < 1e-9);
}

#[test]
fn color_emphasis_tints() {
    let mut a = anim(1, AnimClass::Emphasis, "fillColor", AnimStart::OnClick);
    a.color = Some(ColorRef::rgb(Rgba::rgb(255, 0, 0)));
    let tl = Timeline::new(&slide_with(vec![a]), W, H);
    assert_eq!(tl.state(ShapeId(1), None, 1, 0.0).tint, Some(Rgba::rgb(255, 0, 0)));
}

#[test]
fn every_animation_evaluates_finite() {
    for (id, _, class, _, _, opts) in ANIMATIONS {
        let mut options: Vec<&str> = opts.to_vec();
        options.push("");
        for o in options {
            let mut a = anim(1, *class, id, AnimStart::OnClick);
            a.option = o.into();
            let tl = Timeline::new(&slide_with(vec![a]), W, H);
            for (step, t) in [(0, 0.0), (0, 0.001), (0, 0.25), (0, 0.5), (0, 0.999), (0, 1.0), (0, 2.0), (1, 0.0)] {
                let s = tl.state(ShapeId(1), None, step, t);
                assert!(finite(&s), "{id} {o} {step} {t}: {s:?}");
            }
            let end = tl.state(ShapeId(1), None, 1, 0.0);
            match class {
                AnimClass::Entrance => assert!(end.visible && (end.opacity - 1.0).abs() < 1e-9, "{id}"),
                AnimClass::Exit => assert!(!end.visible, "{id}"),
                _ => {}
            }
        }
    }
}

#[test]
fn fly_starts_off_slide() {
    let mut a = anim(1, AnimClass::Entrance, "fly", AnimStart::OnClick);
    a.option = "l".into();
    let tl = Timeline::new(&slide_with(vec![a]), W, H);
    let s = tl.state(ShapeId(1), None, 0, 1e-6);
    // Shape at x=100, w=200: must start at or beyond the left edge.
    assert!(100.0 + s.offset_x + 200.0 <= 0.01, "{s:?}");
    let s = tl.state(ShapeId(1), None, 0, 0.999_999);
    assert!(s.offset_x.abs() < 1.0);
}

#[test]
fn wipe_clips() {
    let mut a = anim(1, AnimClass::Entrance, "wipe", AnimStart::OnClick);
    a.option = "l".into();
    let tl = Timeline::new(&slide_with(vec![a]), W, H);
    let c = tl.state(ShapeId(1), None, 0, 0.5).clip.unwrap();
    assert!(c[0] == 0.0 && c[2] > 0.0 && c[2] < 1.0);
}

#[test]
fn repeat_and_auto_reverse_extend_duration() {
    let mut a = anim(1, AnimClass::Emphasis, "pulse", AnimStart::OnClick);
    a.repeat = 3;
    a.auto_reverse = true;
    let tl = Timeline::new(&slide_with(vec![a]), W, H);
    assert!((tl.step_duration(0) - 6.0).abs() < 1e-9);
    // Until next click: one iteration counts, keeps playing within the step.
    let mut b = anim(2, AnimClass::Emphasis, "spin", AnimStart::OnClick);
    b.repeat = u32::MAX;
    let tl = Timeline::new(&slide_with(vec![b]), W, H);
    assert!((tl.step_duration(0) - 1.0).abs() < 1e-9);
    let s = tl.state(ShapeId(2), None, 0, 10.5);
    assert!(s.rotate > 0.0 && s.rotate < 360.0);
}

#[test]
fn rewind_reverts_entrance() {
    let mut a = anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick);
    a.rewind = true;
    let tl = Timeline::new(&slide_with(vec![a]), W, H);
    assert!(!tl.state(ShapeId(1), None, 1, 0.0).visible);
}

#[test]
fn after_hide_on_next_click() {
    let mut a = anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick);
    a.after = Some("hideOnNextClick".into());
    let tl = Timeline::new(&slide_with(vec![a, anim(2, AnimClass::Entrance, "fade", AnimStart::OnClick)]), W, H);
    assert!(tl.state(ShapeId(1), None, 0, 5.0).visible);
    assert!(!tl.state(ShapeId(1), None, 1, 0.0).visible);
}

#[test]
fn triggered_sequences() {
    let mut a = anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick);
    a.trigger = Some(ShapeId(3));
    let tl = Timeline::new(&slide_with(vec![anim(2, AnimClass::Entrance, "fade", AnimStart::OnClick), a]), W, H);
    assert_eq!(tl.steps(), 1);
    assert_eq!(tl.trigger_shapes(), vec![ShapeId(3)]);
    assert_eq!(tl.trigger_steps(ShapeId(3)), 1);
    // Not played: hidden even after the main sequence.
    assert!(!tl.state(ShapeId(1), None, 1, 0.0).visible);
    let tp = [TriggerPlay { trigger: ShapeId(3), step: 1, t: 0.0 }];
    assert!(tl.state_with(ShapeId(1), None, 0, 0.0, &tp).visible);
}

#[test]
fn text_build_by_paragraph() {
    let mut s = slide_with(vec![]);
    s.shapes[0].text = Some(TextBody::from_text("one\ntwo\nthree"));
    let mut a = anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick);
    a.text_build = TextBuild::ByParagraph;
    s.animations = vec![a.clone()];
    let tl = Timeline::new(&s, W, H);
    let n = tl.paragraph_targets(ShapeId(1)).len();
    assert_eq!(n, 3);
    // On click: one click per paragraph.
    assert_eq!(tl.steps(), n);
    if n >= 2 {
        assert!(tl.state(ShapeId(1), Some(0), 1, 0.0).visible);
        assert!(!tl.state(ShapeId(1), Some(1), 1, 0.0).visible);
    }
    // With previous: staggered within one step.
    a.start = AnimStart::WithPrevious;
    s.animations = vec![a];
    let tl = Timeline::new(&s, W, H);
    assert_eq!(tl.steps(), 1);
    let e = tl.effects();
    if e.len() >= 2 {
        assert!((e[1].start - e[0].start - 0.5).abs() < 1e-9);
    }
    // The shape itself isn't hidden.
    assert!(tl.state(ShapeId(1), None, 0, 0.0).visible);
}

#[test]
fn by_paragraph_groups_sub_bullets_with_their_parent() {
    let mut s = slide_with(vec![]);
    let mut body = TextBody::from_text("one\nsub a\nsub b\ntwo");
    body.paragraphs[1].level = 1;
    body.paragraphs[2].level = 1;
    s.shapes[0].text = Some(body);
    let mut a = anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick);
    a.text_build = TextBuild::ByParagraph;
    s.animations = vec![a];
    let tl = Timeline::new(&s, W, H);
    assert_eq!(tl.steps(), 2);
    assert_eq!(tl.paragraph_targets(ShapeId(1)), vec![0, 1, 2, 3]);
    assert!(tl.state(ShapeId(1), Some(2), 1, 0.0).visible);
    assert!(!tl.state(ShapeId(1), Some(3), 1, 0.0).visible);
    assert!(!tl.state(ShapeId(1), Some(1), 0, 0.0).visible);
}

#[test]
fn hostile_times_and_durations() {
    let mut a = anim(1, AnimClass::Entrance, "fly", AnimStart::OnClick);
    a.duration_ms = u32::MAX;
    a.delay_ms = u32::MAX;
    a.repeat = u32::MAX - 1;
    a.smooth_start = f64::NAN;
    a.smooth_end = -5.0;
    a.bounce_end = f64::INFINITY;
    a.amount = Some(f64::NAN);
    let mut b = anim(2, AnimClass::Emphasis, "growShrink", AnimStart::AfterPrevious);
    b.amount = Some(1e300);
    b.duration_ms = 0;
    let tl = Timeline::new(&slide_with(vec![a, b, anim(99, AnimClass::Path, "customPath", AnimStart::OnClick)]), f64::NAN, -1.0);
    for t in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1e12, 1e300] {
        for step in [0, 1, 2, 3, usize::MAX] {
            for id in [1, 2, 99] {
                let s = tl.state(ShapeId(id), None, step, t);
                assert!(finite(&s));
                let s = tl.state(ShapeId(id), Some(usize::MAX), step, t);
                assert!(finite(&s));
            }
        }
        let _ = tl.locate(t);
        let _ = tl.state_at(ShapeId(1), None, t);
    }
    assert!(tl.step_duration(usize::MAX) == 0.0);
}

#[test]
fn empty_slide() {
    let tl = Timeline::new(&Slide::default(), W, H);
    assert_eq!(tl.steps(), 0);
    assert!(tl.is_empty());
    assert!(tl.state(ShapeId(1), None, 0, 1.0).is_identity());
    assert_eq!(tl.locate(5.0), (0, 0.0));
}

#[test]
fn locate_play_all_clock() {
    let tl = Timeline::new(
        &slide_with(vec![anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick), anim(2, AnimClass::Entrance, "fade", AnimStart::OnClick)]),
        W,
        H,
    );
    assert_eq!(tl.locate(0.5), (0, 0.5));
    assert_eq!(tl.locate(1.5), (1, 0.5));
    assert_eq!(tl.locate(9.0), (2, 0.0));
    assert!((tl.total_duration() - 2.0).abs() < 1e-9);
}

#[test]
fn motion_path_parse_and_eval() {
    let p = MotionPath::parse("M 0 0 L 0.25 0 L 0.25 0.25 E");
    assert!((p.length() - 0.5).abs() < 1e-9);
    let (x, y) = p.at(0.5);
    assert!((x - 0.25).abs() < 1e-9 && y.abs() < 1e-9);
    let (x, y) = p.at(1.0);
    assert!((x - 0.25).abs() < 1e-9 && (y - 0.25).abs() < 1e-9);
    // Relative commands and curves.
    let q = MotionPath::parse("m 0 0 l 0.1 0 c 0 0.1 0 0.1 0 0.2 z");
    assert!(q.length() > 0.0);
    let c = MotionPath::parse("M0,0C0.1,0,0.2,0.1,0.2,0.2E");
    assert!(c.points.len() > 2);
    let e = MotionPath::parse("M 0 0 L 1e-1 -2.5E-1 E");
    let (x, y) = e.at(1.0);
    assert!((x - 0.1).abs() < 1e-9 && (y + 0.25).abs() < 1e-9);
}

#[test]
fn malformed_paths_dont_panic() {
    for s in [
        "",
        "E",
        "M",
        "M 0",
        "L 1 1",
        "C 1 2 3",
        "M 0 0 L x y",
        "M NaN 1 L inf 2",
        "M 1e400 0 L 0 0",
        "Z Z Z",
        "M 0 0 Q",
        "--..ee",
        "M 0 0 H 1 V 1 h -1 v -1",
        "😀 M 1 1",
        "M 0 0 L 0.5 0.5 X 9 9",
    ] {
        let p = MotionPath::parse(s);
        for t in [f64::NAN, -1.0, 0.0, 0.5, 1.0, 2.0] {
            let (x, y) = p.at(t);
            assert!(x.is_finite() && y.is_finite(), "{s}");
        }
    }
    let big = "L 1 1 ".repeat(50_000);
    let p = MotionPath::parse(&big);
    assert!(p.points.len() <= 20_000);
}

#[test]
fn preset_paths_move() {
    for (id, _, class, _, _, opts) in ANIMATIONS.iter().filter(|a| a.2 == AnimClass::Path && a.0 != "customPath") {
        let _ = class;
        for o in opts.iter().copied().chain([""]) {
            let p = MotionPath::parse(&default_path(id, o));
            assert!(p.length() > 0.0, "{id} {o}");
        }
    }
    let mut a = anim(1, AnimClass::Path, "lines", AnimStart::OnClick);
    a.option = "right".into();
    let tl = Timeline::new(&slide_with(vec![a]), W, H);
    let s = tl.state(ShapeId(1), None, 1, 0.0);
    assert!((s.offset_x - 0.25 * W).abs() < 1e-6 && s.offset_y.abs() < 1e-6);
}

#[test]
fn custom_path_offsets_in_points() {
    let mut a = anim(1, AnimClass::Path, "customPath", AnimStart::OnClick);
    a.path = Some("M 0 0 L 0.5 0.5 E".into());
    let tl = Timeline::new(&slide_with(vec![a]), W, H);
    let s = tl.state(ShapeId(1), None, 1, 0.0);
    assert!((s.offset_x - 480.0).abs() < 1e-6 && (s.offset_y - 270.0).abs() < 1e-6);
}

#[test]
fn easing_endpoints() {
    for (a, d, b) in [(0.0, 0.0, 0.0), (0.5, 0.5, 0.0), (0.3, 0.0, 0.0), (0.0, 0.7, 0.0), (0.9, 0.9, 0.0), (0.0, 0.0, 0.3)] {
        assert!(ease(0.0, a, d, b).abs() < 1e-9, "{a} {d} {b}");
        assert!((ease(1.0, a, d, b) - 1.0).abs() < 1e-9, "{a} {d} {b}");
        assert!(ease(0.5, a, d, b).is_finite());
    }
    assert!(ease(f64::NAN, f64::NAN, f64::NAN, f64::NAN).is_finite());
}

fn covers_at_end(layers: &[Layer]) -> bool {
    layers.iter().any(|l| {
        l.source == Source::New
            && (l.alpha - 1.0).abs() < 1e-9
            && l.quad == [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
            && l.uv == [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
    })
}

#[test]
fn every_transition_finite_and_complete() {
    for (id, _, _, _, opts) in TRANSITIONS {
        let mut options: Vec<&str> = opts.to_vec();
        options.push("");
        for o in options {
            for t in [0.0, 0.01, 0.25, 0.5, 0.75, 0.99, 1.0] {
                let ls = transition_layers(id, o, t);
                assert!(!ls.is_empty(), "{id} {o} {t}");
                assert!(ls.len() < 5000, "{id} {o} {t}");
                for l in &ls {
                    assert!(l.alpha.is_finite() && (0.0..=1.0).contains(&l.alpha));
                    assert!(l.quad.iter().chain(l.uv.iter()).all(|p| p[0].is_finite() && p[1].is_finite()));
                    assert!(l.src.iter().all(|v| v.is_finite()));
                }
            }
            assert!(covers_at_end(&transition_layers(id, o, 1.0)), "{id} {o}");
            let start = transition_layers(id, o, 0.0);
            assert!(start.iter().any(|l| l.source == Source::Old && l.alpha == 1.0), "{id} {o}");
        }
    }
}

#[test]
fn transitions_hostile_t() {
    for (id, ..) in TRANSITIONS {
        for t in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -5.0, 1e12] {
            let ls = transition_layers(id, "bogus-option", t);
            assert!(!ls.is_empty());
        }
    }
    assert!(!transition_layers("no-such-kind", "", 0.5).is_empty());
}

#[test]
fn fade_and_push_shapes() {
    let ls = transition_layers("fade", "smoothly", 0.5);
    assert_eq!(ls.len(), 2);
    assert_eq!(ls[1].source, Source::New);
    assert!((ls[1].alpha - 0.5).abs() < 1e-9);
    let ls = transition_layers("push", "l", 0.5);
    let new = ls.iter().find(|l| l.source == Source::New).unwrap();
    // Pushing left: the new slide comes from the right.
    assert!(new.quad[0][0] > 0.0);
    let ls = transition_layers("fade", "throughBlack", 0.25);
    assert!(ls.iter().all(|l| l.source != Source::New));
}

#[test]
fn mid_transitions_mix_sources() {
    // Most transitions show something of both slides (or black/white) midway.
    for (id, ..) in TRANSITIONS.iter().filter(|t| !matches!(t.0, "none" | "cut" | "flip")) {
        let ls = transition_layers(id, "", 0.5);
        assert!(ls.iter().any(|l| l.source != Source::New) || ls.len() > 1, "{id}");
    }
}

#[test]
fn random_is_deterministic() {
    assert_eq!(transition_layers("random", "x", 0.4), transition_layers("random", "x", 0.4));
    assert_eq!(transition_layers("dissolve", "", 0.4), transition_layers("dissolve", "", 0.4));
}

#[test]
fn morph_pairs_by_name_text_and_geometry() {
    let mut a = Slide::default();
    let mut b = Slide::default();
    a.shapes = vec![shape(1, "!!Logo"), shape(2, "Title 1"), shape(3, "x"), shape(4, "Rect A")];
    b.shapes = vec![shape(14, "Rect B"), shape(13, "y"), shape(12, "Title 1"), shape(11, "!!Logo")];
    a.shapes[2].text = Some(TextBody::from_text("Hello"));
    b.shapes[1].text = Some(TextBody::from_text("Hello"));
    let p = morph_pairs(&a, &b);
    assert!(p.contains(&(ShapeId(1), ShapeId(11))));
    assert!(p.contains(&(ShapeId(2), ShapeId(12))));
    assert!(p.contains(&(ShapeId(3), ShapeId(13))));
    assert!(p.contains(&(ShapeId(4), ShapeId(14))));
    assert_eq!(p.len(), 4);
    assert!(morph_pairs(&Slide::default(), &b).is_empty());
}

#[test]
fn morph_xfrm_interpolates() {
    let a = Xfrm { rot: 350.0, ..Xfrm::new(0.0, 0.0, 100.0, 100.0) };
    let b = Xfrm { rot: 10.0, ..Xfrm::new(100.0, 50.0, 200.0, 100.0) };
    let m = morph_xfrm(a, b, 0.5);
    assert!((m.x - 50.0).abs() < 1e-9 && (m.w - 150.0).abs() < 1e-9);
    assert!(m.rot.abs() < 1e-9 || (m.rot - 360.0).abs() < 1e-9, "{}", m.rot);
    assert_eq!(morph_xfrm(a, b, 1.0), b);
    assert_eq!(morph_xfrm(a, b, f64::NAN), a);
    let n = Xfrm { x: f64::NAN, rot: f64::INFINITY, ..Xfrm::default() };
    let m = morph_xfrm(n, b, 0.3);
    assert!(m.rot.is_finite());
}

fn solid(s: Shape, r: u8) -> Shape {
    Shape { fill: Some(deckcraft_model::Fill::solid(ColorRef::rgb(Rgba { r, g: 0, b: 0, a: 255 }))), ..s }
}

#[test]
fn morph_frame_moves_blends_and_fades() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let mut a = Slide::default();
    let mut b = Slide::default();
    let moved = |id, x| Shape { xfrm: Some(Xfrm::new(x, 100.0, 200.0, 100.0)), ..shape(id, "Box") };
    a.shapes = vec![solid(moved(1, 0.0), 0), shape(2, "Gone")];
    let new = Shape { text: Some(TextBody::from_text("Hi")), ..shape(3, "New") };
    b.shapes = vec![solid(moved(1, 400.0), 200), new, shape(4, "Later")];
    let f = morph_frame(&p, &a, &b, 0.5, &|id| id == ShapeId(4));
    assert!((f.mix - 0.5).abs() < 1e-9);
    // Gone fades out, Box moves halfway and blends red, New fades in, Later stays out.
    let names: Vec<(&str, bool)> = f.shapes.iter().map(|(s, old)| (s.name.as_str(), *old)).collect();
    assert_eq!(names, [("Gone", true), ("Box", false), ("New", false)]);
    let m = &f.shapes[1].0;
    assert!((m.xfrm.unwrap().x - 200.0).abs() < 1e-9);
    assert_eq!(m.fill, Some(deckcraft_model::Fill::solid(ColorRef::rgb(Rgba { r: 100, g: 0, b: 0, a: 255 }))));
    assert_eq!(f.opacity_of(m.id), 1.0);
    assert!((f.opacity_of(f.shapes[0].0.id) - 0.5).abs() < 1e-9);
    assert!((f.opacity_of(f.shapes[2].0.id) - 0.5).abs() < 1e-9);
    // Ids are fresh and unique.
    let mut ids: Vec<_> = f.shapes.iter().map(|(s, _)| s.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|i| i.0 > 4));
    // At the start only the old slide shows.
    let f0 = morph_frame(&p, &a, &b, 0.0, &|id| id == ShapeId(4));
    assert!(f0.shapes.iter().filter(|(s, _)| f0.opacity_of(s.id) > 0.0).all(|(s, _)| s.name != "New"));
    assert_eq!(morph_frame(&p, &a, &b, f64::NAN, &|_| false).mix, 0.0);
}

#[test]
fn morph_frame_cross_fades_changed_content() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let mut a = Slide::default();
    let mut b = Slide::default();
    a.shapes = vec![Shape { text: Some(TextBody::from_text("Before")), ..shape(1, "T") }];
    b.shapes = vec![Shape { text: Some(TextBody::from_text("After")), ..shape(1, "T") }];
    let f = morph_frame(&p, &a, &b, 0.25, &|_| false);
    // The new text under the fading old one, both on the moving box.
    assert_eq!(f.shapes.len(), 2);
    assert_eq!((&f.shapes[0].0.text, f.shapes[0].1), (&b.shapes[0].text, false));
    assert_eq!((&f.shapes[1].0.text, f.shapes[1].1), (&a.shapes[0].text, true));
    let s = f.mix;
    assert!((f.opacity_of(f.shapes[0].0.id) - s).abs() < 1e-9);
    assert!((f.opacity_of(f.shapes[1].0.id) - (1.0 - s)).abs() < 1e-9);
}

#[test]
fn morph_frame_lists_texts_for_words_and_characters() {
    let p = deckcraft_model::defaults::new_presentation(None);
    let mut a = Slide::default();
    let mut b = Slide::default();
    a.shapes = vec![Shape { text: Some(TextBody::from_text("The Science")), ..shape(1, "T") }];
    b.shapes = vec![Shape { text: Some(TextBody::from_text("Science rocks")), ..shape(1, "T") }];
    for (option, want) in [("objects", None), ("words", Some(false)), ("characters", Some(true))] {
        b.transition = Some(Transition { kind: "morph".into(), option: option.into(), ..Default::default() });
        let f = morph_frame(&p, &a, &b, 0.5, &|_| false);
        assert_eq!(f.text.first().map(|m| m.chars), want, "{option}");
        if let Some(m) = f.text.first() {
            assert_eq!((m.new, m.old), (f.shapes[0].0.id, f.shapes[1].0.id));
            assert_eq!(m.from, a.shapes[0].xfrm.unwrap());
        }
    }
    // Same text: one shape moves, nothing to morph.
    b.shapes[0].text = a.shapes[0].text.clone();
    assert!(morph_frame(&p, &a, &b, 0.5, &|_| false).text.is_empty());
}

fn pres(n: usize) -> Presentation {
    Presentation { slides: (0..n).map(|i| Arc::new(Slide { id: SlideId(1000 + i as u32), ..Default::default() })).collect(), ..Default::default() }
}

#[test]
fn show_skips_hidden_slides() {
    let mut p = pres(4);
    Arc::make_mut(&mut p.slides[1]).hidden = true;
    let mut s = ShowState::new(&p);
    assert_eq!(s.slide, 0);
    assert_eq!(s.next(&p), ShowAction::Slide(2));
    assert_eq!(s.prev(&p), ShowAction::Slide(0));
    // Jumping to a hidden slide shows it; next continues after it.
    assert_eq!(s.goto(&p, 1), ShowAction::Slide(1));
    assert_eq!(s.next(&p), ShowAction::Slide(2));
    assert_eq!(s.next(&p), ShowAction::Slide(3));
    assert_eq!(s.next(&p), ShowAction::End);
    assert_eq!(s.next(&p), ShowAction::Exit);
    assert_eq!(s.goto(&p, 99), ShowAction::None);
}

#[test]
fn show_loops() {
    let mut p = pres(3);
    p.show.loop_until_esc = true;
    let mut s = ShowState::new(&p);
    s.next(&p);
    s.next(&p);
    assert_eq!(s.next(&p), ShowAction::Slide(0));
    assert_eq!(s.prev(&p), ShowAction::Slide(2));
}

#[test]
fn show_custom_show_and_range() {
    let mut p = pres(5);
    p.custom_shows = vec![CustomShow { name: "Short".into(), slides: vec![SlideId(1003), SlideId(1001)] }];
    p.show.custom_show = Some("Short".into());
    assert_eq!(show_order(&p), vec![3, 1]);
    let mut s = ShowState::new(&p);
    assert_eq!(s.slide, 3);
    assert_eq!(s.next(&p), ShowAction::Slide(1));
    p.show.custom_show = None;
    p.show.range = Some((2, 4));
    assert_eq!(show_order(&p), vec![1, 2, 3]);
    p.show.range = Some((0, 99));
    assert_eq!(show_order(&p), vec![0, 1, 2, 3, 4]);
}

#[test]
fn show_steps_through_builds() {
    let mut p = pres(2);
    Arc::make_mut(&mut p.slides[0]).shapes = vec![shape(1, "A")];
    Arc::make_mut(&mut p.slides[0]).animations = vec![anim(1, AnimClass::Entrance, "fade", AnimStart::OnClick)];
    let mut s = ShowState::new(&p);
    assert!(!s.playing);
    assert_eq!(s.next(&p), ShowAction::PlayStep(0));
    assert!(s.playing);
    assert_eq!(s.step_done(&p), ShowAction::FinishStep(0));
    assert_eq!(s.step, 1);
    assert_eq!(s.prev(&p), ShowAction::StepBack(0));
    assert_eq!(s.next(&p), ShowAction::PlayStep(0));
    assert_eq!(s.next(&p), ShowAction::FinishStep(0));
    assert_eq!(s.next(&p), ShowAction::Slide(1));
    // Back to a fully built previous slide.
    assert_eq!(s.prev(&p), ShowAction::Slide(0));
    assert_eq!(s.step, 1);
}

#[test]
fn show_auto_advance() {
    let mut p = pres(2);
    Arc::make_mut(&mut p.slides[0]).transition = Some(Transition { advance_after_ms: Some(2000), ..Default::default() });
    let s = ShowState::new(&p);
    assert_eq!(s.advance_after(&p), Some(2.0));
    assert!(!s.auto_advance_due(&p, 1.0));
    assert!(s.auto_advance_due(&p, 2.5));
    assert!(!s.auto_advance_due(&p, f64::NAN));
    p.show.use_timings = false;
    assert!(!s.auto_advance_due(&p, 5.0));
}

#[test]
fn show_empty_and_all_hidden() {
    let p = pres(0);
    let mut s = ShowState::new(&p);
    assert!(s.ended);
    assert_eq!(s.next(&p), ShowAction::Exit);
    let _ = s.prev(&p);
    let mut p = pres(2);
    for sl in &mut p.slides {
        Arc::make_mut(sl).hidden = true;
    }
    let mut s = ShowState::new(&p);
    assert!(s.ended);
    let _ = s.next(&p);
    let _ = s.prev(&p);
}

#[test]
fn compose_and_sanitize() {
    let a = AnimState { opacity: 0.5, clip: Some([0.0, 0.0, 0.5, 1.0]), ..Default::default() };
    let b = AnimState { opacity: 0.5, offset_x: 3.0, clip: Some([0.25, 0.0, 1.0, 1.0]), ..Default::default() };
    let c = a.compose(&b);
    assert!((c.opacity - 0.25).abs() < 1e-9 && c.offset_x == 3.0 && c.clip == Some([0.25, 0.0, 0.5, 1.0]));
    let bad = AnimState { opacity: f64::NAN, rotate: f64::INFINITY, clip: Some([f64::NAN, 2.0, -1.0, 0.5]), ..Default::default() }.sanitized();
    assert!(finite(&bad));
}

#[test]
fn show_follows_links() {
    use deckcraft_model::text::Action;
    let mut p = pres(4);
    Arc::make_mut(&mut p.slides[3]).hidden = true;
    let mut s = ShowState::new(&p);
    assert_eq!(s.follow(&p, &Action::Slide { slide: SlideId(1002) }, None), LinkJump::Show(ShowAction::Slide(2)));
    assert_eq!(s.follow(&p, &Action::PreviousSlide, None), LinkJump::Show(ShowAction::Slide(1)));
    assert_eq!(s.follow(&p, &Action::NextSlide, None), LinkJump::Show(ShowAction::Slide(2)));
    // Last skips the hidden slide; a link to the hidden slide shows it.
    assert_eq!(s.follow(&p, &Action::LastSlide, None), LinkJump::Show(ShowAction::Slide(2)));
    assert_eq!(s.follow(&p, &Action::Slide { slide: SlideId(1003) }, None), LinkJump::Show(ShowAction::Slide(3)));
    assert_eq!(s.follow(&p, &Action::FirstSlide, None), LinkJump::Show(ShowAction::Slide(0)));
    assert_eq!(s.follow(&p, &Action::LastViewed, Some(3)), LinkJump::Show(ShowAction::Slide(3)));
    assert_eq!(s.slide, 3);
    assert_eq!(s.follow(&p, &Action::Url { url: "https://example.org".into() }, None), LinkJump::Open("https://example.org".into()));
    assert_eq!(s.follow(&p, &Action::EndShow, None), LinkJump::Exit);
    assert_eq!(s.follow(&p, &Action::Slide { slide: SlideId(9) }, None), LinkJump::None, "a link to a deleted slide does nothing");
    assert_eq!(s.follow(&p, &Action::LastViewed, None), LinkJump::None);
    assert_eq!(s.slide, 3);
}
