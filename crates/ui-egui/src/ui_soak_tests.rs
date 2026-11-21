//! UI soak: the whole window driven by seeded random input (clicks, drags, keys with and without
//! Ctrl/Shift, typing, scrolling) over the sample deck, with invariants every few actions: no
//! panic, every document valid, undo/journal/texture caches bounded, and frame time bounded.
//!
//! `DECKCRAFT_UI_SOAK_STEPS=5000 cargo test -p deckcraft-ui-egui ui_soak_long -- --ignored`
//! runs a long one; `DECKCRAFT_UI_SOAK_SEED` picks the seed.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use deckcraft_engine::Session;
use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, Pos2, vec2};
use egui_kittest::Harness;

use crate::{Services, SlideApp};

const W: f32 = 1400.0;
const H: f32 = 900.0;
/// Bounds that a long session must stay under.
const MAX_TEXTURES: usize = 1_500;
const MAX_THUMBS: usize = 600;
const MAX_JOURNAL: usize = 10_000;
/// Mean wall time per frame, generous for slow CI machines (the test profile is optimized).
const MAX_MEAN_FRAME: Duration = Duration::from_millis(250);

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn pos(&mut self) -> Pos2 {
        Pos2::new(self.below(W as usize) as f32, self.below(H as usize) as f32)
    }
}

const KEYS: &[Key] = &[
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::Enter,
    Key::Tab,
    Key::Backspace,
    Key::Delete,
    Key::Escape,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::A,
    Key::C,
    Key::V,
    Key::X,
    Key::D,
    Key::G,
    Key::B,
    Key::I,
    Key::Z,
    Key::Y,
    Key::M,
    Key::F5,
];

fn harness() -> Harness<'static, SlideApp> {
    let mut s = Session::new();
    deckcraft_engine::sample::open_sample(&mut s).unwrap();
    let app = SlideApp::new(s, Services::default());
    let mut h = Harness::builder().with_size(vec2(W, H)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// Apply one deterministic random UI/document action and return a label describing it for the
/// soak log, which is used as context when a later assertion fails.
fn act(h: &mut Harness<'_, SlideApp>, r: &mut Rng) -> String {
    match r.below(9) {
        0..=2 => {
            let p = r.pos();
            h.hover_at(p);
            h.run_steps(1);
            h.drag_at(p);
            h.run_steps(1);
            h.drop_at(p);
            format!("click {p:?}")
        }
        3 => {
            let (a, b) = (r.pos(), r.pos());
            h.hover_at(a);
            h.run_steps(1);
            h.drag_at(a);
            for i in 1..=4 {
                h.hover_at(a + (b - a) * (i as f32 / 4.0));
                h.run_steps(1);
            }
            h.drop_at(b);
            format!("drag {a:?} -> {b:?}")
        }
        4 | 5 => {
            let k = KEYS[r.below(KEYS.len())];
            let m = match r.below(4) {
                0 => Modifiers::COMMAND,
                1 => Modifiers::SHIFT,
                _ => Modifiers::NONE,
            };
            h.key_press_modifiers(m, k);
            format!("key {m:?} {k:?}")
        }
        6 => {
            h.event(Event::Text("Soak ✓ שלום 漢字 ".into()));
            "type".into()
        }
        7 => {
            let p = r.pos();
            h.hover_at(p);
            h.event(Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: vec2(0.0, if r.below(2) == 0 { 3.0 } else { -3.0 }),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
                source: MouseWheelSource::Wheel,
            });
            format!("scroll {p:?}")
        }
        _ => {
            // Undo or redo through the keyboard, the way people hammer it.
            let k = if r.below(2) == 0 { Key::Z } else { Key::Y };
            h.key_press_modifiers(Modifiers::COMMAND, k);
            format!("ctrl {k:?}")
        }
    }
}

fn check(h: &Harness<'_, SlideApp>, seed: u64, step: usize, log: &VecDeque<String>) {
    let ctx = || format!("seed {seed} step {step}; last actions:\n{}", log.iter().cloned().collect::<Vec<_>>().join("\n"));
    let app = h.state();
    for d in app.session.documents() {
        let v = d.doc.validate();
        assert!(v.is_empty(), "invalid document {v:?}; {}", ctx());
        assert!(d.history.undo.len() <= d.history.limit.max(1), "undo over limit; {}", ctx());
        assert!(d.selection.slide < d.doc.slides.len().max(1), "selection slide out of range; {}", ctx());
    }
    assert!(app.session.journal.len() <= MAX_JOURNAL, "journal {}; {}", app.session.journal.len(), ctx());
    assert!(app.textures.thumb_count() <= MAX_THUMBS, "thumbnails {}; {}", app.textures.thumb_count(), ctx());
    assert_eq!(app.session.panics_caught, 0, "a command panicked (rolled back by the guard); {}", ctx());
    assert_eq!(deckcraft_render::panics_caught(), 0, "a render panic was caught and drawn blank; {}", ctx());
    let tex = h.ctx.tex_manager().read().num_allocated();
    assert!(tex <= MAX_TEXTURES, "{tex} textures allocated; {}", ctx());
}

fn soak(seed: u64, steps: usize) {
    let mut h = harness();
    let mut r = Rng(seed | 1);
    let mut log = VecDeque::new();
    let t0 = Instant::now();
    let mut frames = 0usize;
    for step in 0..steps {
        log.push_back(act(&mut h, &mut r));
        if log.len() > 20 {
            log.pop_front();
        }
        h.run_steps(2);
        frames += 8;
        if step % 10 == 9 {
            check(&h, seed, step, &log);
        }
        // A modal or the slide show can swallow input forever; leave it now and then.
        if step % 50 == 49 {
            h.key_press(Key::Escape);
            h.run_steps(2);
        }
    }
    check(&h, seed, steps, &log);
    let edits = h.state().session.journal.len();
    // Random input must reach the editor, not bounce off one modal.
    assert!(edits * 20 >= steps, "only {edits} commands ran in {steps} actions; seed {seed}");
    let mean = t0.elapsed() / frames.max(1) as u32;
    eprintln!("ui soak seed {seed}: {steps} actions, {edits} commands, ~{frames} frames, mean frame {mean:?}");
    assert!(mean <= MAX_MEAN_FRAME, "mean frame {mean:?} over {MAX_MEAN_FRAME:?}; seed {seed}");
}

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[test]
fn ui_soak_random_input_keeps_the_app_sound() {
    for seed in [3, 0xC0FFEE] {
        soak(seed, env("DECKCRAFT_UI_SOAK_STEPS", 150) as usize);
    }
}

#[test]
#[ignore = "long UI soak; set DECKCRAFT_UI_SOAK_STEPS"]
fn ui_soak_long() {
    soak(env("DECKCRAFT_UI_SOAK_SEED", 0x5EED), env("DECKCRAFT_UI_SOAK_STEPS", 3000) as usize);
}
