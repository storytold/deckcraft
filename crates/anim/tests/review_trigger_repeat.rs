//! Independent, contributor-original public-API review oracles for native trigger holds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use deckcraft_anim::{Click, HitShape, Sequence, ShowState, Timeline, TriggerPlay};
use deckcraft_model::anim::TextBuild;
use deckcraft_model::text::TextBody;
use deckcraft_model::{AnimClass, AnimStart, Animation, Presentation, Shape, ShapeId, Slide, SlideId, Transition};

const A: ShapeId = ShapeId(u32::MAX);
const B: ShapeId = ShapeId(91);
const TARGET: ShapeId = ShapeId(32);

fn effect(owner: Option<ShapeId>, target: ShapeId) -> Animation {
    Animation {
        shape: target,
        trigger: owner,
        class: AnimClass::Emphasis,
        effect: "spin".into(),
        duration_ms: 1000,
        amount: Some(360.0),
        ..Default::default()
    }
}

fn original(animations: Vec<Animation>) -> Presentation {
    let shapes = [A, B, TARGET, ShapeId(33), ShapeId(34), ShapeId(35)]
        .into_iter()
        .map(|id| Shape { id, text: Some(TextBody::from_text("North\nSouth\nEast")), ..Default::default() })
        .collect();
    Presentation {
        slides: vec![Arc::new(Slide {
            id: SlideId(73),
            shapes,
            animations,
            transition: Some(Transition { advance_on_click: false, ..Default::default() }),
            ..Default::default()
        })],
        ..Default::default()
    }
}

fn click(show: &mut ShowState, pres: &Presentation, tl: &Timeline, owner: ShapeId, now: f64) {
    let hit = HitShape { id: owner, parent: None, is_group: false, link: None, under_pointer: true };
    assert_eq!(show.click(pres, tl, &[hit], &|_| true, now, None), Click::Animated);
}

fn play(show: &ShowState, owner: ShapeId) -> TriggerPlay {
    show.trigger_plays().into_iter().find(|p| p.trigger == owner).expect("clicked lane")
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "actual={actual}, expected={expected}");
}

#[test]
fn completion_does_not_reorder_shared_target_but_new_step_and_replay_do() {
    // Flat-list order is deliberately A0, B0, A1. B's completed exit must win
    // over A0 even when the later click completes the held A0 interval.
    let pres = original(vec![
        Animation { class: AnimClass::Entrance, effect: "fade".into(), repeat: u32::MAX, ..effect(Some(A), TARGET) },
        Animation { class: AnimClass::Exit, effect: "disappear".into(), duration_ms: 0, ..effect(Some(B), TARGET) },
        Animation { class: AnimClass::Entrance, effect: "appear".into(), duration_ms: 0, ..effect(Some(A), TARGET) },
    ]);
    let untouched = pres.clone();
    let slide = Arc::clone(&pres.slides[0]);
    let tl = Timeline::new(&slide, 640.0, 360.0);
    let mut show = ShowState::new(&pres);
    assert!(!show.anim_state(&tl, TARGET, 0.0).visible);
    click(&mut show, &pres, &tl, A, 0.0);
    show.tick(&tl, 3.25);
    assert!(show.anim_state(&tl, TARGET, 0.0).visible);
    close(show.anim_state(&tl, TARGET, 0.0).opacity, 0.25);
    click(&mut show, &pres, &tl, B, 3.25);
    assert!(!show.anim_state(&tl, TARGET, 0.0).visible);
    let before_completion = show.trigger_stamps();
    click(&mut show, &pres, &tl, A, 3.25);
    assert_eq!(show.trigger_stamps(), before_completion);
    assert_eq!(play(&show, A).step, 1);
    assert!(!show.anim_state(&tl, TARGET, 0.0).visible);
    click(&mut show, &pres, &tl, A, 3.25);
    assert_eq!(play(&show, A).step, 2);
    assert!(show.anim_state(&tl, TARGET, 0.0).visible);
    click(&mut show, &pres, &tl, A, 3.25);
    show.tick(&tl, 5.75);
    assert_eq!(play(&show, A).step, 0);
    assert_eq!(play(&show, B).step, 1);
    assert!(show.anim_state(&tl, TARGET, 0.0).visible);
    close(show.anim_state(&tl, TARGET, 0.0).opacity, 0.5);
    let stamps = show.trigger_stamps();
    assert_eq!(stamps.iter().filter(|s| s.trigger == A).count(), 1);
    assert!(stamps.iter().find(|s| s.trigger == A).unwrap().at > stamps.iter().find(|s| s.trigger == B).unwrap().at);
    assert_eq!(pres, untouched);
    assert!(Arc::ptr_eq(&pres.slides[0], &slide), "playback must not detach the shared slide");
    assert!(Arc::ptr_eq(&pres.slides[0], &untouched.slides[0]));
}

#[test]
fn paragraph_click_builds_hold_individually_and_never_skip_the_next_paragraph() {
    let pres = original(vec![Animation { repeat: u32::MAX, text_build: TextBuild::ByParagraph, delay_ms: 125, ..effect(Some(A), TARGET) }]);
    let tl = Timeline::new(&pres.slides[0], 640.0, 360.0);
    assert_eq!(tl.trigger_steps(A), 3);
    assert_eq!(
        tl.effects().iter().map(|e| (e.anim, e.paragraph, e.step)).collect::<Vec<_>>(),
        vec![(0, Some(0), 0), (0, Some(1), 1), (0, Some(2), 2)]
    );
    let mut show = ShowState::new(&pres);
    for (step, at) in [(0, 0.0), (1, 8.0), (2, 16.0)] {
        assert!(tl.trigger_step_waits_for_click(A, step));
        close(tl.trigger_step_duration(A, step), 1.125);
        click(&mut show, &pres, &tl, A, at);
        show.tick(&tl, at + 6.375);
        assert_eq!(play(&show, A).step, step);
        for paragraph in 0..3 {
            let expected = if paragraph < step {
                360.0
            } else if paragraph == step {
                90.0
            } else {
                0.0
            };
            close(tl.state_ordered(TARGET, Some(paragraph), 0, 0.0, &show.trigger_plays(), &show.trigger_stamps()).rotate, expected);
        }
        click(&mut show, &pres, &tl, A, at + 6.375);
        assert_eq!(play(&show, A).step, step + 1);
        assert!(!show.trigger_playing());
        assert_eq!(show.trigger_stamps().len(), step + 1);
    }
}

#[test]
fn hold_classification_and_preview_arithmetic_are_bounded_at_integer_extremes() {
    for class in [AnimClass::Entrance, AnimClass::Emphasis, AnimClass::Exit, AnimClass::Path, AnimClass::Media] {
        for duration_ms in [0, 1, u32::MAX] {
            for (repeat, iterations) in [(0, 1.0), (1, 1.0), (7, 7.0), (u32::MAX - 1, 1_000_000.0), (u32::MAX, 1.0)] {
                let pres = original(vec![Animation {
                    class,
                    effect: match class {
                        AnimClass::Entrance => "fade",
                        AnimClass::Emphasis => "spin",
                        AnimClass::Exit => "fadeOut",
                        AnimClass::Path => "lines",
                        AnimClass::Media => "play",
                    }
                    .into(),
                    duration_ms,
                    delay_ms: u32::MAX,
                    repeat,
                    auto_reverse: true,
                    ..effect(Some(A), TARGET)
                }]);
                let tl = Timeline::new(&pres.slides[0], 640.0, 360.0);
                assert_eq!(tl.trigger_step_waits_for_click(A, 0), class != AnimClass::Media && duration_ms != 0 && repeat == u32::MAX);
                let expected = 4_294_967.295 + if class == AnimClass::Media { 0.0 } else { duration_ms as f64 / 1000.0 * 2.0 * iterations };
                close(tl.trigger_step_duration(A, 0), expected);
                assert!(!tl.trigger_step_waits_for_click(B, 0));
                assert!(!tl.trigger_step_waits_for_click(A, usize::MAX));
                let mut show = ShowState::new(&pres);
                click(&mut show, &pres, &tl, A, 0.0);
                show.tick(&tl, f64::MAX);
                close(play(&show, A).t, 3600.0);
                assert_eq!(play(&show, A).step, 0);
                click(&mut show, &pres, &tl, A, f64::MAX);
                assert_eq!(play(&show, A).step, 1, "click during the very long delay completes only this step");
                assert!(!show.trigger_playing());
            }
        }
    }
}

#[test]
fn pending_groups_keep_original_timings_and_main_lane_remains_independent() {
    let mut animations = vec![
        Animation { repeat: u32::MAX, delay_ms: 250, ..effect(Some(A), TARGET) },
        effect(Some(B), ShapeId(33)),
        Animation { start: AnimStart::WithPrevious, duration_ms: 2000, ..effect(Some(A), ShapeId(33)) },
        Animation { start: AnimStart::AfterPrevious, delay_ms: 125, duration_ms: 500, ..effect(Some(A), ShapeId(34)) },
        Animation { start: AnimStart::WithPrevious, delay_ms: 25, ..effect(Some(A), ShapeId(35)) },
    ];
    // A main indefinite predecessor retains its existing finite preview dependency.
    animations
        .extend([Animation { repeat: u32::MAX, ..effect(None, TARGET) }, Animation { start: AnimStart::AfterPrevious, ..effect(None, ShapeId(34)) }]);
    let pres = original(animations);
    let tl = Timeline::new(&pres.slides[0], 640.0, 360.0);
    let timings = tl.effects();
    let trigger = timings.iter().filter(|e| e.sequence == Sequence::Trigger(A)).collect::<Vec<_>>();
    assert_eq!(trigger.iter().map(|e| e.anim).collect::<Vec<_>>(), vec![0, 2, 3, 4]);
    for (actual, expected) in trigger.iter().map(|e| e.start).zip([0.25, 0.0, 2.125, 2.025]) {
        close(actual, expected);
    }
    close(tl.trigger_step_duration(A, 0), 3.025);
    close(tl.step_duration(0), 2.0);
    close(tl.state(ShapeId(34), None, 0, 2.5).rotate, 360.0);
    let mut show = ShowState::new(&pres);
    click(&mut show, &pres, &tl, A, 0.0);
    for at in [0.125, 2.25, 100.25] {
        show.tick(&tl, at);
        assert_eq!(play(&show, A).step, 0);
        close(show.anim_state(&tl, ShapeId(34), 0.0).rotate, 0.0);
        close(show.anim_state(&tl, ShapeId(35), 0.0).rotate, 0.0);
    }
    click(&mut show, &pres, &tl, A, 100.25);
    close(show.anim_state(&tl, ShapeId(34), 0.0).rotate, 360.0);
    close(show.anim_state(&tl, ShapeId(35), 0.0).rotate, 360.0);
    assert_eq!(tl.effects(), timings, "playback cannot rewrite public timing identity");
}
