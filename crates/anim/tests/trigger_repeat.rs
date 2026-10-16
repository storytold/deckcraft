//! Contributor-original public playback regressions for indefinite trigger steps.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_anim::{AnimState, Click, HitShape, LinkJump, ShowAction, ShowState, Timeline, TriggerPlay};
use deckcraft_model::{AnimClass, AnimStart, Animation, Presentation, Shape, ShapeId, Slide, SlideId, Transition, Xfrm, text::Action};
use std::sync::Arc;

fn spin(trigger: Option<u32>, target: u32) -> Animation {
    Animation {
        shape: ShapeId(target),
        trigger: trigger.map(ShapeId),
        class: AnimClass::Emphasis,
        effect: "spin".into(),
        amount: Some(360.0),
        duration_ms: 1000,
        ..Default::default()
    }
}
fn looping(trigger: u32, target: u32) -> Animation {
    Animation { repeat: u32::MAX, ..spin(Some(trigger), target) }
}
fn fixture(animations: Vec<Animation>) -> Presentation {
    let slide = Slide {
        id: SlideId(100),
        shapes: (1..=12)
            .map(|id| Shape {
                id: ShapeId(id),
                name: format!("Original shape {id}"),
                xfrm: Some(Xfrm::new(0.0, 0.0, 40.0, 40.0)),
                ..Default::default()
            })
            .collect(),
        transition: Some(Transition { advance_on_click: false, ..Default::default() }),
        animations,
        ..Default::default()
    };
    Presentation { slides: vec![Arc::new(slide)], ..Default::default() }
}
struct Run {
    presentation: Presentation,
    timeline: Timeline,
    show: ShowState,
    now: f64,
}
impl Run {
    fn new(animations: Vec<Animation>) -> Self {
        let presentation = fixture(animations);
        let timeline = Timeline::new(&presentation.slides[0], 720.0, 405.0);
        let show = ShowState::new(&presentation);
        Self { presentation, timeline, show, now: 0.0 }
    }
    fn click(&mut self, trigger: Option<u32>) -> Click {
        let hits =
            trigger.map(|id| vec![HitShape { id: ShapeId(id), parent: None, is_group: false, link: None, under_pointer: true }]).unwrap_or_default();
        self.show.click(&self.presentation, &self.timeline, &hits, &|_| true, self.now, None)
    }
    fn tick(&mut self, now: f64) {
        self.now = now;
        self.show.tick(&self.timeline, now);
    }
    fn play(&self, trigger: u32) -> TriggerPlay {
        self.show.trigger_plays().into_iter().find(|play| play.trigger == ShapeId(trigger)).expect("trigger was clicked")
    }
    fn state(&self, target: u32) -> AnimState {
        self.show.anim_state(&self.timeline, ShapeId(target), 0.0)
    }
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} differs from {expected}");
}

#[test]
fn nonzero_indefinite_trigger_keeps_its_playhead_and_fractional_state_past_preview_duration() {
    let mut run = Run::new(vec![looping(1, 2), spin(Some(1), 3)]);
    let original = run.presentation.clone();
    assert_eq!(run.timeline.trigger_step_duration(ShapeId(1), 0), 1.0);
    assert!(run.timeline.trigger_step_waits_for_click(ShapeId(1), 0));
    assert!(!run.timeline.trigger_step_waits_for_click(ShapeId(1), 1));
    assert_eq!(run.click(Some(1)), Click::Animated);
    for (now, rotation) in [(0.25, 90.0), (1.25, 90.0), (12.75, 270.0), (30.25, 90.0)] {
        run.tick(now);
        assert!(run.show.trigger_playing());
        assert_eq!(run.play(1).step, 0);
        close(run.play(1).t, now);
        close(run.state(2).rotate, rotation);
        close(run.state(3).rotate, 0.0);
    }
    assert_eq!(run.presentation, original, "playback must not modify the model");
}

#[test]
fn own_click_completes_once_before_or_after_preview_and_next_click_starts_the_next_step() {
    for click_time in [0.25, 1.25, 12.25] {
        let mut run = Run::new(vec![looping(1, 2), Animation { duration_ms: 500, ..spin(Some(1), 3) }]);
        run.click(Some(1));
        run.tick(click_time);
        assert_eq!(run.click(Some(1)), Click::Animated);
        assert_eq!(run.play(1).step, 1);
        close(run.play(1).t, 0.0);
        assert!(!run.show.trigger_playing());
        close(run.state(2).rotate, 360.0);
        close(run.state(3).rotate, 0.0);
        assert_eq!(run.show.trigger_stamps().len(), 1, "completion is not a new start");
        run.click(Some(1));
        assert_eq!(run.play(1).step, 1);
        assert!(run.show.trigger_playing());
        assert_eq!(run.show.trigger_stamps().len(), 2);
        run.tick(click_time + 0.6);
        assert_eq!(run.play(1).step, 2);
        assert!(!run.show.trigger_playing());
        close(run.state(3).rotate, 360.0);
        run.click(Some(1));
        assert_eq!(run.play(1).step, 0, "next click after final completion replays step zero");
        assert!(run.show.trigger_playing());
        assert_eq!(run.show.trigger_stamps().len(), 1, "replay discards the previous lap's stamps");
        close(run.state(3).rotate, 0.0);
    }
}

#[test]
fn delayed_and_auto_reversed_loops_wait_and_rewind_at_explicit_completion() {
    for rewind in [false, true] {
        for auto_reverse in [false, true] {
            let effect = Animation { delay_ms: 1500, rewind, auto_reverse, ..looping(1, 2) };
            let mut run = Run::new(vec![effect, spin(Some(1), 3)]);
            run.click(Some(1));
            run.tick(0.25);
            assert_eq!(run.play(1).step, 0);
            close(run.state(2).rotate, 0.0);
            run.tick(12.75);
            assert_eq!(run.play(1).step, 0);
            close(run.state(2).rotate, if auto_reverse { 270.0 } else { 90.0 });
            run.click(Some(1));
            assert_eq!(run.play(1).step, 1);
            close(run.state(2).rotate, if rewind || auto_reverse { 0.0 } else { 360.0 });
        }
    }
    let mut during_delay = Run::new(vec![Animation { delay_ms: 3000, ..looping(1, 2) }, spin(Some(1), 3)]);
    during_delay.click(Some(1));
    during_delay.tick(0.25);
    during_delay.click(Some(1));
    assert_eq!(during_delay.play(1).step, 1, "active click completes the entire delayed step");
    close(during_delay.state(2).rotate, 360.0);
}

#[test]
fn finite_repeats_autoreverse_zero_duration_and_media_do_not_create_holds() {
    let mut finite = Run::new(vec![Animation { repeat: 3, auto_reverse: true, ..spin(Some(1), 2) }]);
    assert!(!finite.timeline.trigger_step_waits_for_click(ShapeId(1), 0));
    close(finite.timeline.trigger_step_duration(ShapeId(1), 0), 6.0);
    finite.click(Some(1));
    finite.tick(5.75);
    assert_eq!(finite.play(1).step, 0);
    finite.tick(6.25);
    assert_eq!(finite.play(1).step, 1);
    assert!(!finite.show.trigger_playing());
    close(finite.state(2).rotate, 0.0);

    let mut zero = Run::new(vec![Animation { duration_ms: 0, ..looping(1, 2) }, spin(Some(1), 3)]);
    assert!(!zero.timeline.trigger_step_waits_for_click(ShapeId(1), 0));
    zero.click(Some(1));
    assert_eq!(zero.play(1).step, 1);
    assert!(!zero.show.trigger_playing());
    zero.click(Some(1));
    assert_eq!(zero.play(1).step, 1);
    zero.tick(1.25);
    assert_eq!(zero.play(1).step, 2);

    let mut mixed = Run::new(vec![Animation { duration_ms: 0, ..looping(1, 2) }, Animation { start: AnimStart::WithPrevious, ..spin(Some(1), 3) }]);
    assert!(!mixed.timeline.trigger_step_waits_for_click(ShapeId(1), 0));
    mixed.click(Some(1));
    mixed.tick(1.25);
    assert_eq!(mixed.play(1).step, 1, "a zero-length repeat cannot hold its finite concurrent group");

    // This is scheduling policy only; no triggered media playback feature is claimed.
    let mut media = Run::new(vec![Animation { class: AnimClass::Media, effect: "play".into(), delay_ms: 200, ..looping(1, 2) }]);
    assert!(!media.timeline.trigger_step_waits_for_click(ShapeId(1), 0));
    close(media.timeline.trigger_step_duration(ShapeId(1), 0), 0.2);
    media.click(Some(1));
    media.tick(0.25);
    assert_eq!(media.play(1).step, 1);
    assert!(!media.show.trigger_playing());
}

#[test]
fn first_trigger_effect_still_waits_for_its_trigger_for_each_stored_start_mode() {
    for start in [AnimStart::OnClick, AnimStart::WithPrevious, AnimStart::AfterPrevious] {
        let mut run = Run::new(vec![Animation { start, ..looping(1, 2) }]);
        let original = run.presentation.clone();
        assert!(!run.show.trigger_playing());
        assert_eq!(run.click(None), Click::None);
        assert_eq!(run.click(Some(1)), Click::Animated);
        run.tick(2.25);
        assert_eq!(run.play(1).step, 0);
        assert!(run.show.trigger_playing());
        assert_eq!(run.presentation, original);
    }
}

#[test]
fn held_trigger_other_trigger_and_main_next_prev_counters_remain_independent() {
    let mut run = Run::new(vec![
        looping(1, 2),
        Animation { duration_ms: 300, ..spin(Some(4), 5) },
        spin(Some(1), 3),
        Animation { duration_ms: 400, ..spin(None, 6) },
    ]);
    run.click(Some(1));
    run.tick(0.2);
    run.click(Some(4));
    run.tick(0.8);
    assert_eq!(run.play(1).step, 0);
    assert_eq!(run.play(4).step, 1, "finite lane naturally completes beside the held lane");
    let a = run.play(1);
    let stamps = run.show.trigger_stamps();
    assert_eq!(run.click(None), Click::Show(ShowAction::PlayStep(0)));
    assert_eq!(run.show.next(&run.presentation), ShowAction::FinishStep(0));
    assert_eq!(run.show.prev(&run.presentation), ShowAction::StepBack(0));
    assert_eq!(run.play(1), a);
    assert_eq!(run.show.trigger_stamps(), stamps);
    run.click(Some(4));
    assert_eq!(run.play(4).step, 0, "other trigger can replay independently");
    assert_eq!(run.play(1), a);
    run.tick(2.25);
    assert_eq!(run.play(1).step, 0);
    run.click(Some(1));
    assert_eq!(run.play(1).step, 1);
    assert_eq!(run.play(4).step, 1);
    run.click(Some(1));
    run.tick(3.5);
    assert_eq!(run.play(1).step, 2, "next finite step has no inherited hold");
}

#[test]
fn after_previous_group_stays_pending_behind_an_indefinite_loop_until_step_completion() {
    let mut run = Run::new(vec![
        looping(1, 2),
        Animation { start: AnimStart::WithPrevious, amount: Some(90.0), ..spin(Some(1), 3) },
        Animation { start: AnimStart::AfterPrevious, effect: "transparency".into(), amount: Some(0.75), duration_ms: 200, ..spin(Some(1), 4) },
        Animation { start: AnimStart::WithPrevious, amount: Some(180.0), ..spin(Some(1), 5) },
        spin(Some(1), 6),
    ]);
    run.click(Some(1));
    run.tick(20.25);
    assert_eq!(run.play(1).step, 0);
    close(run.state(2).rotate, 90.0);
    close(run.state(3).rotate, 90.0);
    close(run.state(4).opacity, 1.0);
    close(run.state(5).rotate, 0.0);
    run.click(Some(1));
    assert_eq!(run.play(1).step, 1);
    close(run.state(4).opacity, 0.25);
    close(run.state(5).rotate, 180.0);
    close(run.state(6).rotate, 0.0);
    run.click(Some(1));
    run.tick(20.5);
    close(run.state(6).rotate, 90.0);
    run.tick(21.5);
    assert_eq!(run.play(1).step, 2);
}

#[test]
fn loop_after_a_finite_group_starts_on_time_but_blocks_the_following_group() {
    let mut run = Run::new(vec![
        Animation { duration_ms: 500, delay_ms: 250, ..spin(Some(1), 2) },
        Animation { start: AnimStart::AfterPrevious, delay_ms: 250, ..looping(1, 3) },
        Animation { start: AnimStart::AfterPrevious, rewind: true, ..spin(Some(1), 4) },
    ]);
    run.click(Some(1));
    run.tick(0.5);
    close(run.state(3).rotate, 0.0);
    run.tick(1.25);
    close(run.state(3).rotate, 90.0);
    run.tick(11.25);
    assert_eq!(run.play(1).step, 0);
    close(run.state(3).rotate, 90.0);
    close(run.state(4).rotate, 0.0);
    run.click(Some(1));
    assert_eq!(run.play(1).step, 1);
    close(run.state(4).rotate, 0.0); // Completing a pending rewound effect preserves restoration.
}

#[test]
fn hold_lookup_and_hostile_clock_inputs_are_bounded_and_finite() {
    let mut run = Run::new(vec![looping(1, 2)]);
    assert!(!run.timeline.trigger_step_waits_for_click(ShapeId(99), 0));
    assert!(!run.timeline.trigger_step_waits_for_click(ShapeId(1), usize::MAX));
    assert!(!Timeline::default().trigger_step_waits_for_click(ShapeId(1), 0));
    run.click(Some(1));
    run.tick(2.25);
    let play = run.play(1);
    for now in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        run.tick(now);
        assert_eq!(run.play(1), play, "nonfinite/backward input cannot finish or corrupt a held step");
        assert!(run.state(2).rotate.is_finite());
    }
    run.tick(1.0e100);
    assert_eq!(run.play(1).step, 0);
    assert!(run.play(1).t.is_finite());
    assert!(run.state(2).rotate.is_finite());
}

#[test]
fn held_loops_pause_and_resume_without_counting_time_on_another_slide() {
    let mut run = Run::new(vec![looping(1, 2)]);
    run.presentation.slides.push(Arc::new(Slide { id: SlideId(101), ..Default::default() }));
    run.click(Some(1));
    run.tick(2.25);
    let before = run.play(1);
    assert_eq!(run.show.follow(&run.presentation, &Action::Slide { slide: SlideId(101) }, None), LinkJump::Show(ShowAction::Slide(1)));
    run.show.tick(&Timeline::default(), 50.0);
    assert_eq!(run.show.follow(&run.presentation, &Action::Slide { slide: SlideId(100) }, None), LinkJump::Show(ShowAction::Slide(0)));
    run.tick(100.0);
    assert_eq!(run.play(1), before, "off-slide clocks do not contribute to the held animation");
    run.tick(100.5);
    close(run.play(1).t, 2.75);
    close(run.state(2).rotate, 270.0);
    assert_eq!(run.play(1).step, 0);
}
