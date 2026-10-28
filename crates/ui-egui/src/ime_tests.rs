//! #101: Korean, Japanese and Chinese in the interface: input method composition on the slide and
//! in the notes pane (egui `Ime` events, as egui-winit sends them), and interface text drawn with
//! real glyphs rather than boxes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui::{Event, FontId, ImeEvent};
use serde_json::json;

use crate::{Services, SlideApp};
use deckcraft_engine::Session;

/// Build the standard 1280×800 UI harness around a supplied session and run three frames.
fn harness(session: Session) -> egui_kittest::Harness<'static, SlideApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, Services::default()),
    );
    crate::install_ime_pass_hooks(&h.ctx);
    // The app keeps requesting repaints, so step frames instead of run().
    h.run_steps(3);
    h
}

fn preedit(text: &str) -> Event {
    Event::Ime(ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

fn title_text(app: &SlideApp) -> String {
    let sh = &app.session.active().unwrap().doc.slides[0].shapes[0];
    sh.text.as_ref().map(|t| t.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

fn undo_len(app: &SlideApp) -> usize {
    app.session.active().unwrap().history.undo.len()
}

/// Typing 한글 with the Korean input method in the title: each preedit replaces the last in the
/// text box (not ㅎ ㅏ ㄴ …), is underlined, and the commit is one undo step. The input method
/// is on while the slide's text is edited, with its window at the insertion point.
#[test]
fn korean_composes_in_a_slide_text_box() {
    let mut h = harness(Session::with_new());
    assert!(h.output().platform_output.ime.is_none(), "nothing to type into: the input method stays off");
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(2);
    let ime = h.output().platform_output.ime.as_ref().expect("the input method is on while editing");
    let slide = h.state().slide_rect.unwrap();
    assert!(slide.contains(ime.rect.center()), "candidate window at the insertion point {:?}, on the slide {slide:?}", ime.rect);
    assert!(ime.rect.width() < 8.0 && ime.rect.height() > 4.0, "a caret, not the box: {:?}", ime.rect);
    let caret0 = ime.rect;

    let undo0 = undo_len(h.state());
    for pre in ["ㅎ", "하", "한", "한ㄱ", "한그", "한글"] {
        h.event(preedit(pre));
        h.run_steps(1);
        assert_eq!(title_text(h.state()), pre, "the preedit is shown in place, replacing the last");
        assert_eq!(undo_len(h.state()), undo0, "no undo step while composing");
        let st = h.state().session.active().unwrap();
        assert!(!deckcraft_engine::cmd::text::composition_underlines(st).is_empty(), "underlined while composing");
    }
    h.run_steps(1);
    let ime = h.output().platform_output.ime.as_ref().unwrap();
    assert!(ime.rect.center().x > caret0.center().x, "the candidate window follows the insertion point");
    h.event(Event::Ime(ImeEvent::Commit("한글".into())));
    h.run_steps(2);
    assert_eq!(title_text(h.state()), "한글");
    assert!(h.state().session.active().unwrap().composition.is_none());
    assert!(deckcraft_engine::cmd::text::composition_underlines(h.state().session.active().unwrap()).is_empty());
    assert_eq!(undo_len(h.state()), undo0 + 1, "the composition is one undo step");

    // Escape cancels a composition: an empty preedit, and the text is as it was.
    h.event(preedit("ㅁ"));
    h.event(preedit(""));
    h.run_steps(2);
    assert_eq!(title_text(h.state()), "한글");
    assert_eq!(undo_len(h.state()), undo0 + 1);

    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "", "one undo removes the whole word");
}

/// Japanese conversion: the part being converted is drawn with a thicker line; a key settles a
/// composition the input method left open.
#[test]
fn japanese_conversion_shows_the_active_part() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.event(Event::Ime(ImeEvent::Preedit { text: "日本ご".into(), active_range_chars: Some(2..3) }));
    h.run_steps(2);
    let u = deckcraft_engine::cmd::text::composition_underlines(h.state().session.active().unwrap());
    assert_eq!(u.iter().filter(|x| x.2).count(), 1, "{u:?}");
    let undo0 = undo_len(h.state());
    // kittest runs one frame per queued event. Assert the key-down frame itself: the platform
    // composition must be cancelled there, not one frame later after the key-up releases it.
    h.key_down(egui::Key::End);
    h.run_steps(1);
    let interrupted = h.output().platform_output.ime.clone().expect("still editing");
    assert!(interrupted.should_interrupt_composition, "the platform composition is interrupted");
    assert_eq!(title_text(h.state()), "日本ご");
    assert!(h.state().session.active().unwrap().composition.is_none());
    assert_eq!(undo_len(h.state()), undo0 + 1);
    h.key_up(egui::Key::End);
    h.run_steps(1);
    // The published interrupt is the platform reset boundary; the tombstone for the settled
    // generation ends there, so a later commit-only event is treated as a new platform
    // generation rather than being dropped for the rest of the session.
    assert!(h.state().ime_cancelled.is_none(), "the published interrupt retired the settled tombstone");
}

/// A composition the application settled (here an End key) drops the platform pre-edit and commit
/// already queued in the same input batch, so the conversion is typed once. The published
/// interrupt is the explicit boundary: after it the next pre-edit is a new generation. A commit
/// the platform queues after that reset is indistinguishable from a legitimate direct commit
/// (egui's events carry no generation), so the bounded tombstone does not pretend otherwise.
#[test]
fn settled_composition_drops_queued_preedit_and_same_batch_commit() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.event(preedit("日本"));
    h.run_steps(1);
    let undo0 = undo_len(h.state());

    // One RawInput: the key settles the composition and the platform's queued pre-edit/commit
    // arrive after it in the same pass. None of them may be applied. Force an extra discarded
    // pass too: the interrupt may only be consumed by the settled pass the platform sees.
    h.ctx.request_discard("ime interrupt through a discarded pass");
    h.input_mut().events.push(Event::Key { key: egui::Key::End, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
    h.input_mut().events.push(preedit("日本"));
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("日本".into())));
    h.input_mut().events.push(Event::Key { key: egui::Key::End, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() });
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日本", "the stale pre-edit and commit do not restart the cancelled composition");
    assert!(h.state().session.active().unwrap().composition.is_none());
    assert_eq!(undo_len(h.state()), undo0 + 1, "the settled composition is one undo step");
    let interrupted = h.output().platform_output.ime.clone().expect("still editing");
    assert!(interrupted.should_interrupt_composition, "the settled platform composition is interrupted");
    assert!(h.state().ime_cancelled.is_none(), "the settled pass published the interrupt and retired the tombstone");
    let reset = interrupted.reset_request.expect("the settled pass carries the opaque reset request");

    // A real new composition is admitted only by the exact ordered completion.
    h.input_mut().events.extend([reset_completed(reset), preedit("新")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日本新", "a legitimate new pre-edit starts a new generation");
    assert!(h.state().session.active().unwrap().composition.is_some());
    h.event(Event::Ime(ImeEvent::Commit("新".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日本新");
}

/// A commit-only event in the settling batch is stale (it belongs to the composition the key
/// ended). After the published interrupt, a later commit-only event is a legitimate direct input
/// method commit and must not be swallowed by a permanent tombstone.
#[test]
fn stale_commit_in_the_settling_batch_is_rejected_and_a_later_direct_commit_is_accepted() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.event(preedit("日本"));
    h.run_steps(1);
    let undo0 = undo_len(h.state());

    // One RawInput: the key settles the composition and the platform's queued commit follows.
    h.input_mut().events.push(Event::Key { key: egui::Key::End, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("日本".into())));
    h.input_mut().events.push(Event::Key { key: egui::Key::End, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() });
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日本", "the same-batch commit does not type the conversion twice");
    assert_eq!(undo_len(h.state()), undo0 + 1);
    assert!(h.state().ime_cancelled.is_none(), "the published interrupt retired the tombstone");
    let reset = h.output().platform_output.ime.as_ref().and_then(|ime| ime.reset_request).expect("the settled pass carries the opaque reset request");

    // A direct commit with no pre-edit (a real IME's punctuation/full-width character path)
    // arrives after the exact ordered completion and is accepted.
    h.input_mut().events.extend([reset_completed(reset), Event::Ime(ImeEvent::Commit("。".into()))]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日本。", "a later commit-only is not lost");
}

/// The interrupt is only a boundary when it reaches the platform. A discarded layout/font pass
/// keeps both the pending interrupt and the tombstone; the next settled pass retires the tombstone
/// and publishes the reset. Assert the per-pass state, not just the final output carrier.
#[test]
fn a_discarded_pass_keeps_the_tombstone_until_a_settled_pass_publishes_the_interrupt() {
    use std::sync::{Arc, Mutex};
    let traces = Arc::new(Mutex::new(Vec::new()));
    let seen = traces.clone();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(
        move |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
            seen.lock().unwrap().push((
                app.ime_cancelled.is_some(),
                app.ime_interrupt_pending,
                app.ime_composition.is_none(),
                ui.ctx().will_discard(),
            ));
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    h.run_steps(3);
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(2);
    h.event(preedit("日本"));
    h.run_steps(1);
    traces.lock().unwrap().clear();

    h.ctx.request_discard("discarded IME interrupt boundary");
    h.input_mut().events.push(Event::Key { key: egui::Key::End, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
    h.input_mut().events.push(preedit("日本"));
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("日本".into())));
    h.run_steps(1);

    let t = traces.lock().unwrap().clone();
    assert_eq!(t.len(), 2, "one discarded pass followed by the settled pass: {t:?}");
    assert!(t[0].0, "the discarded pass keeps the settled tombstone");
    assert!(t[0].1, "the interrupt stays pending through the discarded pass");
    assert!(t[0].2, "the stale pre-edit/commit cleared the tracked composition");
    assert!(t[0].3, "the first pass is discarded");
    assert!(!t[1].0, "the settled pass publishes the interrupt and retires the tombstone");
    assert!(!t[1].1);
    assert!(!t[1].3);
    assert_eq!(title_text(h.state()), "日本");
    assert!(h.output().platform_output.ime.as_ref().is_some_and(|ime| ime.should_interrupt_composition));
}

/// A settled composition whose descriptor was not painted leaves a tombstone. A *discarded*
/// pass keeps it (the platform has not been told to reset), but an accepted no-descriptor pass is
/// the backend's reset boundary: the tombstone must be retired there, or the first legitimate
/// commit into the next text target is lost.
#[test]
fn accepted_no_descriptor_pass_retires_the_settled_tombstone() {
    use std::sync::{Arc, Mutex};

    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let ctx = egui::Context::default();
    let title = app.session.active().unwrap().doc.slides[0].shapes[0].id.0;
    app.run("text.edit", json!({"id": title})).unwrap();
    let owner = app.ime_target().expect("the edited title is an IME target").owner;
    let generation = 7;
    app.ime_generation = generation;
    app.ime_composition = Some(crate::ImeComposition { owner, generation, settled: true });
    app.ime_cancelled = Some(crate::ImeCancelled { owner, generation });
    app.ime_interrupt_pending = true;
    app.ime_pending = None;

    let traces = Arc::new(Mutex::new(Vec::new()));
    let seen = traces.clone();
    ctx.request_discard("no-descriptor IME boundary");
    ctx.run_ui(egui::RawInput::default(), |ui| {
        let will_discard = ui.ctx().will_discard();
        app.ime_pending = None;
        let _ = app.observe_ime(ui.ctx());
        seen.lock().unwrap().push((will_discard, app.ime_cancelled.is_some(), app.ime_interrupt_pending));
    })
    .drop_without_applying_deltas();

    let t = traces.lock().unwrap().clone();
    assert_eq!(t.len(), 2, "one discarded pass then the accepted pass: {t:?}");
    assert!(t[0].0, "the first pass is discarded");
    assert!(t[0].1, "a discarded pass keeps the tombstone");
    assert!(t[0].2, "a discarded pass keeps the interrupt pending");
    assert!(!t[1].0, "the second pass is accepted");
    assert!(!t[1].1, "the accepted no-descriptor pass retires the tombstone");
    assert!(!t[1].2, "there is no descriptor to publish the interrupt in");
    assert!(app.ime_composition.is_none(), "the settled composition is retired with its tombstone");

    // The next text target's first event is a legitimate direct commit, not the old generation.
    app.ime_event(&ctx, &egui::ImeEvent::Commit("。".into()));
    assert_eq!(title_text(&app), "。", "a first commit in the new target is not swallowed");
}

/// A focused stock `TextEdit` can publish its own `o.ime` while the canvas has no pending rect.
/// That descriptor is not a backend reset: the canvas tombstone and settled composition must
/// survive until an accepted pass really publishes no descriptor at all.
#[test]
fn focused_text_field_descriptor_keeps_the_settled_tombstone() {
    let mut h = harness(Session::with_new());
    let notes = egui::Id::unique("notes_edit");
    h.ctx.memory_mut(|m| m.request_focus(notes));
    h.run_steps(2);
    assert!(h.ctx.memory(|m| m.has_focus(notes)), "the notes TextEdit has focus");
    let owner = egui::Id::unique("old-canvas-ime");
    h.state_mut().ime_composition = Some(crate::ImeComposition { owner, generation: 7, settled: true });
    h.state_mut().ime_cancelled = Some(crate::ImeCancelled { owner, generation: 7 });
    h.state_mut().ime_interrupt_pending = true;

    h.run_steps(1);
    assert!(h.output().platform_output.ime.is_some(), "the focused stock TextEdit publishes its own descriptor");
    assert!(h.state().ime_cancelled.is_some(), "a descriptor-publishing pass is not a reset");
    assert!(h.state().ime_interrupt_pending, "the canvas interrupt is still waiting for a reset pass");
    assert!(h.state().ime_composition.is_some_and(|c| c.settled), "the settled composition is not retired");

    // With focus gone there is no canvas target and no TextEdit descriptor: this accepted pass
    // is the backend reset boundary, so the tombstone and settled composition retire.
    h.ctx.memory_mut(|m| m.surrender_focus(notes));
    h.run_steps(1);
    assert!(h.output().platform_output.ime.is_none(), "nothing publishes an IME descriptor");
    assert!(h.state().ime_cancelled.is_none(), "the accepted no-descriptor pass retires the tombstone");
    assert!(!h.state().ime_interrupt_pending);
    assert!(h.state().ime_composition.is_none(), "the settled composition retires with its tombstone");
}

/// Typing on a selected (not edited) shape with an input method replaces its text, as typing does;
/// the input method is on for it.
#[test]
fn composing_on_a_selected_shape_starts_editing_it() {
    let mut h = harness(Session::with_new());
    let r = h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "Old"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    h.state_mut().run("text.exit", json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().selection.text.is_none());
    assert!(h.output().platform_output.ime.is_some(), "a selected shape takes typing");
    h.event(preedit("新"));
    h.event(Event::Ime(ImeEvent::Commit("新しい".into())));
    h.run_steps(2);
    let v = h.state_mut().session.execute("text.get", &json!({"id": id})).unwrap();
    assert_eq!(v["text"], "新しい");
}

/// The notes pane is an egui text field: composition shows in it and replaces each preedit, the
/// candidate window is at its caret, and the notes are one undo step when it loses focus.
#[test]
fn korean_composes_in_the_notes_pane() {
    let mut h = harness(Session::with_new());
    let id = egui::Id::unique("notes_edit");
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run_steps(2);
    assert!(h.ctx.memory(|m| m.has_focus(id)));
    let undo0 = undo_len(h.state());
    for pre in ["ㄴ", "노", "놑", "노트"] {
        h.event(preedit(pre));
        h.run_steps(1);
        assert_eq!(h.state().notes_buf.2.text, pre, "the preedit replaces the last one");
    }
    let ime = h.output().platform_output.ime.as_ref().expect("the notes field turns the input method on");
    let slide = h.state().slide_rect.unwrap();
    assert!(ime.cursor_rect.min.y > slide.max.y, "candidate window at the notes caret, under the slide");
    h.event(Event::Ime(ImeEvent::Commit("노트".into())));
    h.run_steps(1);
    assert_eq!(h.state().notes_buf.2.text, "노트");
    // Focus leaves the field (Escape): the notes are committed as one step.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    let notes = h.state().session.active().unwrap().doc.slides[0].notes_text();
    assert_eq!(notes, "노트");
    assert_eq!(undo_len(h.state()), undo0 + 1);
    assert!(h.ctx.fonts_mut(|f| f.has_glyphs(&FontId::proportional(13.0), "노트")) || !system_has("노트"));

    // Composing, then going to another slide: the notes stay with the slide they were typed for.
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.state_mut().run("slide.go", json!({"index": 0})).unwrap();
    h.run_steps(2);
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run_steps(2);
    h.event(Event::Key { key: egui::Key::End, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
    h.event(preedit("ㅇ"));
    h.event(Event::Ime(ImeEvent::Commit("요".into())));
    h.run_steps(1);
    h.state_mut().run("slide.go", json!({"index": 1})).unwrap();
    h.run_steps(2);
    let d = h.state().session.active().unwrap();
    assert_eq!(d.doc.slides[0].notes_text(), "노트요");
    assert_eq!(d.doc.slides[1].notes_text(), "");
}

/// Does some font on this machine (embedded or installed) draw every letter of `s`? Machines
/// without a Korean or Chinese font (a bare Linux runner) can't draw it in the interface either.
fn system_has(s: &str) -> bool {
    s.chars().filter(|c| !c.is_whitespace()).all(|c| deckcraft_fonts::FontDb::global().fallback_for(c, u32::MAX).is_some_and(|f| !f.is_color()))
}

/// Assert that both interface faces draw every non-space character of `text`; `painted` describes
/// the sample in a failure message.
fn assert_faces_draw(h: &egui_kittest::Harness<'_, SlideApp>, text: &str, painted: &str) {
    for family in [FontId::proportional(13.0), crate::theme::bold(13.0)] {
        let missing: String = text.chars().filter(|c| !c.is_whitespace() && !h.ctx.fonts_mut(|f| f.has_glyph(&family, *c))).collect();
        assert!(missing.is_empty(), "{family:?} draws {missing:?} of {painted} as boxes");
    }
}

/// #101: a Korean file name, title, notes and comments, a Japanese slide and a Chinese section are
/// drawn with glyphs in the interface (title bar, outline, notes pane, comments), not boxes: faces
/// that draw them are borrowed from the font database. A Latin presentation borrows nothing.
#[test]
fn cjk_text_in_the_interface_is_drawn_with_real_glyphs() {
    let h = harness(Session::with_new());
    assert!(h.state().ui_fonts.faces.is_empty(), "nothing to borrow for {:?}", h.state().session.active().unwrap().title());

    let mut s = Session::with_new();
    let korean = "한국어 발표 자료";
    let japanese = "日本語のスライド";
    let chinese = "简体中文演示文稿";
    let notes = "발표자 노트입니다";
    let require_all = option_env!("CRAFT_FONTS_REQUIRED").is_some() || deckcraft_fonts::CRAFT_FONTS.iter().any(|f| f.family == "LXGW WenKai Lite");
    // The title slide's title and subtitle are painted by the initial Normal view, under
    // kittest's strict missing-glyph policy. Pre-paint CJK only when this build is required to
    // carry it or this machine can draw every initial sample; an optional build without those
    // fonts must skip before that first paint instead of panicking and only then checking.
    let initial_cjk = require_all || [chinese, japanese, korean].iter().all(|t| system_has(t));
    let slide = s.doc().unwrap().current_slide().unwrap();
    let title = slide.shapes[0].id.0;
    let body = slide.shapes[1].id.0;
    if initial_cjk {
        s.execute("text.set", &json!({"id": title, "text": chinese})).unwrap();
        s.execute("text.set", &json!({"id": body, "text": japanese})).unwrap();
        s.execute("slide.notes", &json!({"text": notes})).unwrap();
        s.execute("section.add", &json!({"name": chinese})).unwrap();
        s.doc_mut().unwrap().path = Some(format!("{korean}.pptx"));
    }
    let mut h = harness(s);
    if initial_cjk {
        // `harness` already completed its first passes. Both initially painted samples must have
        // real glyph coverage before any command (and the strict policy would have panicked
        // otherwise).
        let initial: Vec<String> = h.state().session.active().unwrap().doc.slides[0]
            .shapes
            .iter()
            .take(2)
            .map(|sh| sh.text.as_ref().map(|t| t.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n")).unwrap_or_default())
            .collect();
        assert_eq!(initial, [chinese.to_owned(), japanese.to_owned()]);
        for text in [chinese, japanese] {
            assert_faces_draw(&h, text, &format!("initially painted {text:?}"));
        }
    } else {
        eprintln!("optional font input: initial CJK paint skipped before layout (no complete local coverage)");
        // Each sample this machine can draw is still painted for real after the check; the
        // strict policy then checks it. Unsupported samples stay out of the document entirely.
        if system_has(chinese) {
            h.state_mut().run("text.set", json!({"id": title, "text": chinese})).unwrap();
            h.state_mut().run("section.add", json!({"name": chinese})).unwrap();
        }
        if system_has(japanese) {
            h.state_mut().run("text.set", json!({"id": body, "text": japanese})).unwrap();
        }
        if system_has(korean) {
            h.state_mut().run("slide.notes", json!({"text": notes})).unwrap();
            h.state_mut().session.doc_mut().unwrap().path = Some(format!("{korean}.pptx"));
        }
        if system_has(chinese) || system_has(japanese) || system_has(korean) {
            h.run_steps(3);
        }
    }
    // Outline and file-name text is painted only after this command; the Chinese title above has
    // already covered the initial-paint case.
    h.state_mut().run("view.outline", json!({})).unwrap();
    h.run_steps(3);
    assert!(!initial_cjk || h.state().session.active().unwrap().title().contains(korean));
    let mut checked = 0;
    for text in [korean, japanese, chinese, notes] {
        if !system_has(text) {
            assert!(!require_all, "required font input does not draw {text:?}");
            eprintln!("skipped {text:?}: no font on this machine draws it");
            continue;
        }
        assert_faces_draw(&h, text, &format!("{text:?}"));
        checked += 1;
    }
    if require_all || cfg!(windows) || cfg!(target_os = "macos") {
        assert_eq!(checked, 4, "all CJK samples must be exercised with required/bundled fonts");
    }
    // Text typed later (a dialog field, a new comment) is looked at too. Never type text this
    // machine cannot draw: the strict missing-glyph policy would panic before the optional check.
    let typed = "ไทย";
    if system_has(typed) {
        h.event(Event::Text(typed.into()));
        h.run_steps(2);
        assert!(h.ctx.fonts_mut(|f| f.has_glyphs(&FontId::proportional(13.0), typed)));
    } else if require_all {
        panic!("required font input does not draw {typed:?}");
    } else {
        eprintln!("skipped {typed:?}: no font on this machine draws it");
    }
}

/// An input method can ask for characters (stock `DeleteSurrounding`) or UTF-8 bytes (the fork's
/// `DeleteSurroundingBytes`) around the caret to be removed. The byte range never splits a
/// character, and a deletion made while composing stays in the composition's one undo step.
#[test]
fn ime_delete_surrounding_handles_chars_bytes_and_composition_undo() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.insert", &json!({"text": "abc"})).unwrap();
    h.run_steps(2);
    h.event(Event::Ime(ImeEvent::DeleteSurrounding { before_chars: 2, after_chars: 0 }));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "a");
    let deleted = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(deleted.change_cause, egui::output::ImeChangeCause::InputMethod, "an accepted deletion is an IME mutation");
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abc", "a non-composing deletion is one undo step");

    // UTF-8 bytes: the four bytes of an emoji are one character and must delete whole.
    h.state_mut().session.execute("text.set", &json!({"id": title, "text": "aé😀"})).unwrap();
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.move", &json!({"to": "end"})).unwrap();
    h.run_steps(2);
    h.event(Event::Ime(ImeEvent::DeleteSurroundingBytes { before_bytes: 4, after_bytes: 0 }));
    h.run_steps(2);
    assert_eq!(title_text(h.state()), "aé", "the emoji's bytes delete exactly one scalar");

    // Deleting before a composition, then committing, is a single undo step.
    h.state_mut().session.execute("text.set", &json!({"id": title, "text": "abc"})).unwrap();
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(2);
    let undo0 = undo_len(h.state());
    h.event(preedit("日本"));
    h.run_steps(1);
    h.event(Event::Ime(ImeEvent::DeleteSurroundingBytes { before_bytes: 1, after_bytes: 0 }));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "ab日本");
    h.event(Event::Ime(ImeEvent::Commit("日本".into())));
    h.run_steps(2);
    assert_eq!(title_text(h.state()), "ab日本");
    assert_eq!(undo_len(h.state()), undo0 + 1, "delete + commit is one composition step");
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abc");
}

/// The canvas producer publishes bounded logical committed text with UTF-8 byte offsets and a
/// stable owner/generation across ordinary typing, caret movement and reversed selection.
#[test]
fn ime_output_is_logical_bounded_and_owner_stable() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.insert", &json!({"text": "aé😀b"})).unwrap();
    h.run_steps(2);
    let first = h.output().platform_output.ime.clone().expect("editing text owns the input method");
    assert_eq!(first.purpose, egui::IMEPurpose::Normal);
    assert_eq!(first.change_cause, egui::output::ImeChangeCause::Other);
    let payload = first.surrounding_text.available().expect("model text is available");
    assert_eq!((payload.text.as_str(), payload.cursor, payload.anchor), ("aé😀b", 8, 8));
    let (owner, generation) = (first.owner, first.context_generation);

    // A caret move is not a new context and still reports logical UTF-8 offsets.
    h.state_mut().session.execute("text.select", &json!({"anchor": [0, 2], "caret": [0, 2]})).unwrap();
    h.run_steps(1);
    let moved = h.output().platform_output.ime.clone().unwrap();
    assert_eq!((moved.owner, moved.context_generation), (owner, generation));
    let payload = moved.surrounding_text.available().unwrap();
    assert_eq!((payload.cursor, payload.anchor), (3, 3));

    // A reversed selection keeps caret < anchor in the outgoing bytes.
    h.state_mut().session.execute("text.select", &json!({"anchor": [0, 3], "caret": [0, 1]})).unwrap();
    h.run_steps(1);
    let reversed = h.output().platform_output.ime.clone().unwrap();
    assert_eq!((reversed.owner, reversed.context_generation), (owner, generation));
    let payload = reversed.surrounding_text.available().unwrap();
    assert_eq!((payload.cursor, payload.anchor), (1, 7), "caret and anchor keep their direction");

    // A forced extra pass in the same frame keeps the same context.
    h.ctx.request_discard("ime multipass regression");
    h.run_steps(1);
    let multipass = h.output().platform_output.ime.clone().unwrap();
    assert_eq!((multipass.owner, multipass.context_generation), (owner, generation));

    // The extra window renders the slide and owns no text: whether the backend draws it as its
    // own native viewport or (as in kittest, which installs no immediate-viewport renderer)
    // embedded in this pass, it must neither own nor disturb the editor's context.
    h.state_mut().run("view.newWindow", json!({"on": true})).unwrap();
    h.run_steps(2);
    let second_window = h.output().platform_output.ime.clone().unwrap();
    assert_eq!((second_window.owner, second_window.context_generation), (owner, generation));
    h.state_mut().run("view.newWindow", json!({"on": false})).unwrap();
}

/// A model-stored composition is pre-edit, not committed text. An accepted commit changes the
/// committed snapshot and is the only path that earns `InputMethod`.
#[test]
fn preedit_is_excluded_and_a_commit_is_input_method() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.insert", &json!({"text": "ab"})).unwrap();
    h.run_steps(2);
    h.event(preedit("日本"));
    h.run_steps(1);
    let composing = h.output().platform_output.ime.clone().unwrap();
    let payload = composing.surrounding_text.available().unwrap();
    assert_eq!((payload.text.as_str(), payload.cursor, payload.anchor), ("ab", 2, 2), "pre-edit is not committed text");
    assert_eq!(composing.change_cause, egui::output::ImeChangeCause::Other);

    h.event(Event::Ime(ImeEvent::Commit("日本".into())));
    h.run_steps(1);
    let committed = h.output().platform_output.ime.clone().unwrap();
    let payload = committed.surrounding_text.available().unwrap();
    assert_eq!(payload.text, "ab日本");
    assert_eq!((payload.cursor, payload.anchor), (8, 8));
    assert_eq!(committed.change_cause, egui::output::ImeChangeCause::InputMethod);

    // A pre-edit and its commit in one frame is still one accepted IME mutation. `Harness::event`
    // queues one frame per event, so put both in a single RawInput.
    h.state_mut().session.execute("text.insert", &json!({"text": "X"})).unwrap();
    h.run_steps(1);
    h.input_mut().events.push(preedit("Y"));
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);
    let same_frame = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(title_text(h.state()), "ab日本XY");
    assert_eq!(same_frame.change_cause, egui::output::ImeChangeCause::InputMethod);
}

/// An application edit anywhere in the frame dominates an IME commit in that same frame.
#[test]
fn mixed_application_and_ime_edits_report_other() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.insert", &json!({"text": "a"})).unwrap();
    h.run_steps(2);
    // Both events belong to one pass. `Harness::event` queues one frame per event, so put them
    // in a single RawInput instead.
    h.input_mut().events.push(Event::Text("x".into()));
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("y".into())));
    h.run_steps(1);
    let ime = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(title_text(h.state()), "axy");
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::Other);
    let payload = ime.surrounding_text.available().unwrap();
    assert_eq!(payload.text, "axy");
}

/// An empty commit is a no-op: it must not claim an IME mutation. An empty body is still an
/// available surrounding context, not `Unavailable`.
#[test]
fn no_op_commit_stays_other_and_empty_text_is_available() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(1);
    let empty = h.output().platform_output.ime.clone().unwrap();
    let payload = empty.surrounding_text.available().expect("an empty editable body is available");
    assert_eq!((payload.text.as_str(), payload.cursor, payload.anchor), ("", 0, 0));

    h.event(Event::Ime(ImeEvent::Commit(String::new())));
    h.run_steps(1);
    let no_op = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(no_op.change_cause, egui::output::ImeChangeCause::Other);
    assert!(no_op.surrounding_text.available().is_some_and(|p| p.text.is_empty()));
}

/// A no-op delete is not a mutation: a real commit in the same `RawInput` is still an accepted
/// IME edit, not `Other` caused by a poisoned frame.
#[test]
fn no_op_delete_does_not_poison_a_commit_in_the_same_frame() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.insert", &json!({"text": "abc"})).unwrap();
    h.run_steps(2);

    h.input_mut().events.push(Event::Ime(ImeEvent::DeleteSurrounding { before_chars: 0, after_chars: 0 }));
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("X".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abcX");
    let ime = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod, "the no-op did not poison the real commit");
}

/// An empty commit is likewise a no-op; a real commit later in the same frame remains the
/// accepted input-method mutation.
#[test]
fn empty_commit_does_not_poison_a_commit_in_the_same_frame() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.insert", &json!({"text": "ab"})).unwrap();
    h.run_steps(2);

    h.input_mut().events.push(Event::Ime(ImeEvent::Commit(String::new())));
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abY");
    let ime = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod, "the empty commit did not poison the real commit");
}

/// A selection wider than the surrounding-text budget is published as `Unavailable`. A real
/// deletion from that state must still be certified through the unavailable revision, including
/// when the selection stays unavailable afterwards.
#[test]
fn wide_selection_delete_is_certified_with_the_unavailable_revision() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.set", &json!({"id": title, "text": "a".repeat(4500)})).unwrap();
    h.state_mut().session.execute("text.select", &json!({"anchor": [0, 1], "caret": [0, 4500]})).unwrap();
    h.run_steps(2);
    let before = h.output().platform_output.ime.clone().unwrap();
    assert!(before.surrounding_text.available().is_none(), "the wide selection is unavailable");

    h.input_mut().events.push(Event::Ime(ImeEvent::DeleteSurrounding { before_chars: 1, after_chars: 0 }));
    h.run_steps(1);
    assert_eq!(title_text(h.state()).len(), 4499, "one character before the selection was deleted");
    let after = h.output().platform_output.ime.clone().unwrap();
    assert!(after.surrounding_text.available().is_none(), "the selection is still wider than the budget");
    assert_eq!(after.change_cause, egui::output::ImeChangeCause::InputMethod, "the unavailable revision certifies the deletion");
}

/// A commit that replaces a wide (unavailable) selection produces available text; the mixed
/// unavailable -> available transition needs the before revision to be certified.
#[test]
fn wide_selection_commit_is_certified_with_the_unavailable_revision() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.state_mut().session.execute("text.set", &json!({"id": title, "text": "a".repeat(4500)})).unwrap();
    h.state_mut().session.execute("text.select", &json!({"anchor": [0, 1], "caret": [0, 4500]})).unwrap();
    h.run_steps(2);
    assert!(h.output().platform_output.ime.as_ref().unwrap().surrounding_text.available().is_none());

    h.event(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "aY", "the selected tail is replaced");
    let ime = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(ime.surrounding_text.available().unwrap().text, "aY");
    assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod, "the unavailable revision certifies the commit");
}

/// The surrounding window is bounded even in a large body, and switching the edited target
/// starts a new owner/generation once.
#[test]
fn surrounding_text_is_bounded_and_target_replacement_resets_generation() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    let long = "a".repeat(4500);
    h.state_mut().session.execute("text.set", &json!({"id": title, "text": long})).unwrap();
    h.state_mut().session.execute("text.move", &json!({"to": "end"})).unwrap();
    h.run_steps(2);
    let ime = h.output().platform_output.ime.clone().unwrap();
    let payload = ime.surrounding_text.available().unwrap();
    assert!(payload.text.len() <= 4000, "{}", payload.text.len());
    assert_eq!(payload.cursor, payload.text.len());
    let first = (ime.owner, ime.context_generation);

    // A selection wider than the protocol bound is explicitly unavailable, not truncated.
    h.state_mut().session.execute("text.selectAll", &json!({})).unwrap();
    h.run_steps(1);
    let wide = h.output().platform_output.ime.clone().unwrap();
    assert!(wide.surrounding_text.available().is_none(), "oversized selection is Unavailable");

    h.state_mut().run("text.exit", json!({})).unwrap();
    let r = h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [20, 20, 200, 80], "text": "other"})).unwrap();
    let other = r["id"].as_u64().unwrap();
    h.state_mut().run("text.edit", json!({"id": other})).unwrap();
    h.run_steps(2);
    let ime = h.output().platform_output.ime.clone().unwrap();
    assert_ne!((ime.owner, ime.context_generation), first, "a new target is a new context");
    let payload = ime.surrounding_text.available().unwrap();
    assert_eq!((payload.text.as_str(), payload.cursor, payload.anchor), ("other", 5, 5));
}

/// With no target, the custom owner is hidden: it observes `None` and publishes no descriptor.
#[test]
fn hidden_canvas_owner_observes_none() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(1);
    let first = h.output().platform_output.ime.clone().unwrap();
    h.state_mut().run("text.exit", json!({})).unwrap();
    h.state_mut().run("edit.select", json!({"ids": []})).unwrap();
    h.run_steps(1);
    assert!(h.output().platform_output.ime.is_none(), "nothing to type into: no custom owner");

    // Re-focusing the same shape starts one new generation after the hidden pass.
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(1);
    let again = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(again.owner, first.owner);
    assert_ne!(again.context_generation, first.context_generation);
}

/// A platform pre-edit or commit queued in the same pass as the application settling the
/// composition (a plain click on another shape) must not be typed into the new target: the
/// settled text stays in the body it was composed in, and the platform's copy is interrupted.
/// Once that interrupt is published, an event the OS still queues is indistinguishable from a
/// legitimate new composition (egui's event payload has no generation), so only the settling
/// batch can be rejected deterministically.
///
/// The pre-edit is plain ASCII on purpose: borrowing a CJK face installs fonts and discards the
/// pass, which would hide the one pass the interruption is published in.
#[test]
fn a_settling_batch_cannot_land_an_ime_event_in_the_next_target() {
    let mut h = harness(Session::with_new());
    let a = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    let r = h.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [400, 400, 240, 120], "text": "Confidential draft"})).unwrap();
    let b = r["id"].as_u64().unwrap() as u32;
    h.state_mut().run("text.exit", json!({})).unwrap();
    h.state_mut().session.execute("text.set", &json!({"id": a, "text": "Quarterly results"})).unwrap();
    h.state_mut().run("text.edit", json!({"id": a})).unwrap();
    h.run_steps(2);

    // An input-method pre-edit in A.
    h.event(preedit("nihon"));
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().composition.is_some());

    // A plain press on B settles the composition (PowerPoint does the same) and selects B. Its
    // queued pre-edit and commit travel in the same input batch as the settling pass.
    let press = deckcraft_engine::tools::PointerEvent { kind: deckcraft_engine::tools::PointerKind::Down, x: 500.0, y: 450.0, ..Default::default() };
    h.state_mut().session.pointer(press).unwrap();
    h.input_mut().events.push(preedit("nihon"));
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("nihon".into())));
    h.run_steps(1);
    let st = h.state().session.active().unwrap();
    assert!(st.composition.is_none(), "the click settles the composition");
    assert_eq!(st.selection.shapes, vec![deckcraft_model::ShapeId(b)], "B is the new target");
    assert!(st.selection.text.is_none(), "the queued IME events did not start editing B");
    assert_eq!(text_of(&h, a), "Quarterly resultsnihon", "the settled pre-edit stays in A");
    assert_eq!(text_of(&h, b), "Confidential draft", "B's whole text is not replaced");
    let ime = h.output().platform_output.ime.clone().expect("B is a typing target");
    assert!(ime.should_interrupt_composition, "the platform's copy is cancelled, not committed back");
    assert!(h.state().ime_cancelled.is_none(), "the settled pass published the interrupt boundary");
}

/// The extra document window is a rendered image, not a second text target: the canvas owner
/// stays in the editing viewport, and closing the last presentation really releases it.
#[test]
fn a_second_window_keeps_the_canvas_owner_and_closing_the_last_document_tears_it_down() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(1);
    let first = h.output().platform_output.ime.clone().unwrap();
    assert_eq!(h.state().ime_viewport, Some(egui::ViewportId::ROOT), "the canvas owner lives in the editing viewport");

    h.state_mut().run("view.newWindow", json!({"on": true})).unwrap();
    h.run_steps(2);
    let second = h.output().platform_output.ime.clone().expect("the canvas still owns the input method");
    assert_eq!(
        (second.owner, second.context_generation),
        (first.owner, first.context_generation),
        "the render-only window neither owns nor disturbs the input method"
    );
    assert_eq!(h.state().ime_viewport, Some(egui::ViewportId::ROOT));
    h.state_mut().run("view.newWindow", json!({"on": false})).unwrap();
    h.run_steps(1);

    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(1);
    assert!(h.state().session.active().is_none(), "the last presentation closed");
    assert_eq!(h.state().ime_viewport, None, "its viewport's IME state is released");
    assert!(h.output().platform_output.ime.is_none());
}

/// The text of the shape `id` in the active presentation.
fn text_of(h: &egui_kittest::Harness<'static, SlideApp>, id: u32) -> String {
    h.state()
        .session
        .active()
        .and_then(|d| d.shape(deckcraft_model::ShapeId(id)))
        .and_then(|sh| sh.text.as_ref())
        .map(deckcraft_model::text::TextBody::text)
        .unwrap_or_default()
}

/// Open the title editor on `abcd`, select the text, and return the caret point used for
/// pointer interruption.
fn pending_terminal_editor() -> (egui_kittest::Harness<'static, SlideApp>, egui::Pos2) {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.event(Event::Text("abcd".into()));
    h.run_steps(1);
    h.state_mut().run("text.select", json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();
    h.run_steps(1);
    let point = h.output().platform_output.ime.as_ref().expect("the actual canvas caret1").cursor_rect.center();
    assert!(h.state().canvas_rect.unwrap().contains(point));
    h.state_mut().run("text.select", json!({"anchor": [0, 4], "caret": [0, 4]})).unwrap();
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abcd");
    (h, point)
}

/// Clear a pending pre-edit and assert that the visible text and undo length are unchanged and
/// no platform interrupt was requested.
fn clear_pending_preedit(h: &mut egui_kittest::Harness<'_, SlideApp>) {
    let undo = undo_len(h.state());
    h.event(preedit("Q"));
    h.run_steps(1);
    assert!(h.state().session.active().unwrap().composition.is_some());
    h.event(preedit(""));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abcd", "the visible preedit was restored, not committed");
    assert!(h.state().session.active().unwrap().composition.is_none());
    assert_eq!(undo_len(h.state()), undo);
    assert!(!h.output().platform_output.ime.as_ref().unwrap().should_interrupt_composition, "empty preedit is not a platform reset");
}

/// Build the terminal `DeleteSurrounding` event in character or byte units.
fn pending_terminal_delete(bytes: bool) -> Event {
    Event::Ime(if bytes {
        ImeEvent::DeleteSurroundingBytes { before_bytes: 1, after_bytes: 0 }
    } else {
        ImeEvent::DeleteSurrounding { before_chars: 1, after_chars: 0 }
    })
}

/// A pointer interruption after clearing pre-edit rejects both later `DeleteSurrounding` and
/// commit forms.
#[test]
fn cleared_preedit_pointer_interruption_rejects_later_delete_and_terminal_commit() {
    for bytes in [false, true] {
        for split_terminal in [false, true] {
            let (mut h, point) = pending_terminal_editor();
            clear_pending_preedit(&mut h);
            let undo = undo_len(h.state());
            let doc = h.state().session.active().unwrap().doc.clone();
            h.input_mut().events.extend([
                Event::PointerMoved(point),
                Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            ]);
            h.run_steps(1);
            let reset = published_reset(&h);
            let t = h.state().session.active().unwrap().selection.text.as_ref().expect("the real pointer kept text editing");
            assert_eq!((t.anchor, t.caret), ((0, 1), (0, 1)), "the actual pointer moved the caret");
            h.input_mut().events.push(Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            });
            h.run_steps(3); // Publishing an interrupt and idle passes do not deliver the terminal.
            h.input_mut().events.push(pending_terminal_delete(bytes));
            if split_terminal {
                h.run_steps(1);
                assert_eq!(title_text(h.state()), "abcd", "the separately delivered old deletion is refused");
            }
            h.input_mut().events.extend([Event::Ime(ImeEvent::Commit("X".into())), reset_completed(reset), preedit("Y")]);
            h.run_steps(1);
            // Old terminal is refused, the exact completion admits the suffix in that same batch.
            assert_eq!(title_text(h.state()), "aYbcd");
            assert!(
                !h.output().platform_output.ime.as_ref().unwrap().should_interrupt_composition,
                "the observed old terminal must not reset the new composition"
            );
            assert_eq!(undo_len(h.state()), undo, "neither stale input nor new preedit creates undo");
            h.event(Event::Ime(ImeEvent::Commit("Y".into())));
            h.run_steps(1);
            assert_eq!(title_text(h.state()), "aYbcd");
            h.state_mut().run("edit.undo", json!({})).unwrap();
            assert_eq!(title_text(h.state()), "abcd", "only the fresh commit needs undo");
            // Before the new preedit the stale terminal must not have changed the presentation;
            // the fresh composition's own undo snapshot is exactly the original document.
            assert!(std::sync::Arc::ptr_eq(&doc, &h.state().session.active().unwrap().doc));
        }
    }
}

/// A key interruption after clearing pre-edit rejects a terminal event delivered in a different
/// batch.
#[test]
fn cleared_preedit_key_interruption_rejects_a_terminal_in_a_later_batch() {
    for bytes in [false, true] {
        let (mut h, _) = pending_terminal_editor();
        clear_pending_preedit(&mut h);
        let undo = undo_len(h.state());
        h.key_down(egui::Key::ArrowLeft);
        h.run_steps(1);
        let reset = published_reset(&h);
        h.key_up(egui::Key::ArrowLeft);
        h.run_steps(2);
        h.input_mut().events.extend([pending_terminal_delete(bytes), Event::Ime(ImeEvent::Commit("X".into()))]);
        h.run_steps(1);
        let t = h.state().session.active().unwrap().selection.text.as_ref().unwrap();
        assert_eq!((t.anchor, t.caret), ((0, 3), (0, 3)));
        assert_eq!(title_text(h.state()), "abcd");
        assert_eq!(undo_len(h.state()), undo);
        h.input_mut().events.extend([reset_completed(reset), Event::Ime(ImeEvent::Commit("Y".into()))]);
        h.run_steps(1);
        assert_eq!(title_text(h.state()), "abcYd", "the exact completion admits the later direct commit");
    }
}

/// Replacing the editing target rejects stale terminal mutations from the old composition.
#[test]
fn cleared_preedit_target_replacement_rejects_later_terminal_mutations() {
    for bytes in [false, true] {
        let (mut h, _) = pending_terminal_editor();
        let first = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
        let second = h.state().session.active().unwrap().doc.slides[0].shapes[1].id.0;
        h.state_mut().run("text.edit", json!({"id": second})).unwrap();
        h.event(Event::Text("WXYZ".into()));
        h.run_steps(1);
        h.state_mut().run("text.edit", json!({"id": first, "at": [0, 4]})).unwrap();
        h.run_steps(1);
        clear_pending_preedit(&mut h);
        let undo = undo_len(h.state());
        let doc = h.state().session.active().unwrap().doc.clone();
        // Actual production target-selection command, in a separate pass before the terminal.
        h.state_mut().run("text.edit", json!({"id": second, "at": [0, 1]})).unwrap();
        h.run_steps(1);
        let reset = published_reset(&h);
        h.run_steps(2);
        h.event(pending_terminal_delete(bytes));
        h.run_steps(1);
        h.event(Event::Ime(ImeEvent::Commit("X".into())));
        h.run_steps(1);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.state().session.active().unwrap().doc), "neither old mutation can edit the replacement target");
        assert_eq!(title_text(h.state()), "abcd");
        assert_eq!(undo_len(h.state()), undo);
        let t = h.state().session.active().unwrap().selection.text.as_ref().unwrap();
        assert_eq!((t.shape.0, t.anchor, t.caret), (second, (0, 1), (0, 1)));
        h.input_mut().events.extend([reset_completed(reset), preedit("Y")]);
        h.run_steps(1);
        h.event(Event::Ime(ImeEvent::Commit("Y".into())));
        h.run_steps(1);
        let body = h.state().session.active().unwrap().doc.slides[0].shapes[1].text.as_ref().unwrap();
        assert_eq!(body.paragraphs[0].text(), "WYXYZ", "a fresh composition uses only the new target");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().doc.slides[0].shapes[1].text.as_ref().unwrap().paragraphs[0].text(), "WXYZ");
    }
}

/// Closing the last document clears pending terminal state, and a new document needs no
/// stale-composition fence.
#[test]
fn closing_the_last_document_clears_a_pending_preedit_terminal() {
    let (mut h, _) = pending_terminal_editor();
    clear_pending_preedit(&mut h);
    assert!(h.state().ime_terminal_pending.is_some());

    h.state_mut().run("file.close", json!({})).unwrap();
    assert!(h.state().session.active().is_none(), "the last presentation closed");
    // An interruption observed after the document disappeared has no destination at all. It must
    // stay distinct from "not interrupted yet", which would compare against the retired origin
    // forever and request avoidable resets after a new document returns.
    let ctx = h.ctx.clone();
    h.state_mut().note_settled_composition(&ctx);
    let pending = h.state().ime_terminal_pending.as_ref().expect("the cleared-preedit terminal is still guarded");
    assert!(pending.cancelled && pending.interrupted);
    assert!(pending.interrupted_at.is_none(), "the interruption has no active document destination");

    h.run_steps(1);
    assert!(h.state().ime_terminal_pending.is_none(), "document teardown releases the terminal guard");
    assert!(h.state().ime_composition.is_none());
    assert!(h.state().ime_cancelled.is_none());
    assert!(!h.state().ime_interrupt_pending);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear));

    // Its dead-document terminal must not force a reset in a new presentation's editor.
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(1);
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(1);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear), "the new document's first target needs no stale-composition fence");
    assert!(h.output().platform_output.ime.as_ref().is_some_and(|ime| ime.reset_request.is_none()));
}

/// After a plain clear, both character- and byte-count terminal deletes are accepted.
#[test]
fn cleared_preedit_without_interruption_accepts_both_terminal_delete_forms() {
    for bytes in [false, true] {
        let (mut h, _) = pending_terminal_editor();
        clear_pending_preedit(&mut h);
        h.event(pending_terminal_delete(bytes));
        h.run_steps(1);
        assert_eq!(title_text(h.state()), "abc");
        h.event(Event::Ime(ImeEvent::Commit("X".into())));
        h.run_steps(1);
        assert_eq!(title_text(h.state()), "abcX", "the accepted deletion's caret shift does not stale its commit");
        assert!(h.state().ime_terminal_pending.is_none());
    }
}

/// A clear accepts its own commit, then permits a direct commit and a fresh composition without
/// stale state.
#[test]
fn cleared_preedit_accepts_its_commit_then_direct_and_fresh_compositions() {
    let (mut h, _) = pending_terminal_editor();
    clear_pending_preedit(&mut h);
    h.event(Event::Ime(ImeEvent::Commit("X".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abcdX");
    assert!(h.state().ime_terminal_pending.is_none());
    h.event(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abcdXY", "a direct commit has no old owner");
    h.event(preedit("Z"));
    h.run_steps(1);
    h.event(Event::Ime(ImeEvent::Commit("Z".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abcdXYZ");
}

// ---------------------------------------------------------------------------
// Token-bound IME reset adoption (revision02): opaque id + ordered completion.
// ---------------------------------------------------------------------------

/// Build the ordered reset-completion event for a request id.
fn reset_completed(id: egui::ImeResetId) -> Event {
    Event::ImeResetCompleted { request_id: id }
}

/// Return the custom descriptor's currently published reset id, asserting that a token-bound
/// request exists.
fn published_reset(h: &egui_kittest::Harness<'static, SlideApp>) -> egui::ImeResetId {
    h.output().platform_output.ime.as_ref().and_then(|ime| ime.reset_request).expect("the custom descriptor carries a token-bound reset request")
}

/// Read the reset phase recorded for the root viewport.
fn root_phase(h: &egui_kittest::Harness<'static, SlideApp>) -> crate::ImeResetPhase {
    h.state().ime_reset_for(egui::ViewportId::ROOT)
}

/// Match the backend contract: the active viewport must have an entry in RawInput.
fn viewport_input(viewport: egui::ViewportId) -> egui::RawInput {
    let mut input = egui::RawInput { viewport_id: viewport, ..Default::default() };
    input.viewports.insert(viewport, egui::ViewportInfo { parent: Some(egui::ViewportId::ROOT), ..Default::default() });
    input
}

/// Read the reset phase for an arbitrary viewport, defaulting to `Clear` when none is stored.
fn phase_at(h: &egui_kittest::Harness<'static, SlideApp>, viewport: egui::ViewportId) -> crate::ImeResetPhase {
    h.state().ime_reset_for(viewport)
}

#[derive(Debug, PartialEq, Eq)]
struct ImeStateSnapshot {
    composition: Option<(egui::Id, u64, bool)>,
    terminal: Option<(egui::Id, u64, (usize, usize), (usize, usize), bool, bool, Option<crate::ImeTerminalStamp>)>,
    cancelled: Option<(egui::Id, u64)>,
    generation: u64,
    settles_seen: u64,
    interrupt_pending: bool,
    viewport: Option<egui::ViewportId>,
    frame_viewport: Option<egui::ViewportId>,
    reset_entries: usize,
}

/// Every piece of the root composition lifetime that the per-viewport reset map does not own.
fn ime_state(h: &egui_kittest::Harness<'static, SlideApp>) -> ImeStateSnapshot {
    let app = h.state();
    ImeStateSnapshot {
        composition: app.ime_composition.map(|c| (c.owner, c.generation, c.settled)),
        terminal: app.ime_terminal_pending.as_ref().map(|p| (p.owner, p.revision, p.anchor, p.caret, p.cancelled, p.interrupted, p.interrupted_at)),
        cancelled: app.ime_cancelled.map(|c| (c.owner, c.generation)),
        generation: app.ime_generation,
        settles_seen: app.ime_settles_seen,
        interrupt_pending: app.ime_interrupt_pending,
        viewport: app.ime_viewport,
        frame_viewport: app.ime_frame_viewport,
        reset_entries: app.ime_reset.len(),
    }
}

/// Put the title editor into a settled composition and return the actual opaque request id.
fn settled_title_reset(h: &mut egui_kittest::Harness<'static, SlideApp>) -> egui::ImeResetId {
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    if h.state().session.active().unwrap().selection.text.is_none() {
        h.state_mut().run("text.edit", json!({"id": title})).unwrap();
        h.run_steps(2);
    }
    h.event(preedit("日"));
    h.run_steps(1);
    h.key_down(egui::Key::End);
    h.run_steps(1);
    let id = published_reset(h);
    h.key_up(egui::Key::End);
    h.run_steps(1);
    id
}

/// Only the exact ordered completion releases the fence; a fresh suffix survives while a stale
/// prefix is discarded.
#[test]
fn ordered_reset_completion_is_exact_and_fresh_suffix_survives_stale_prefix() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1));

    // Foreign completion and a fresh preedit in the same raw batch: inert, blocked.
    let foreign = egui::Context::default().request_ime_reset(egui::ViewportId::ROOT).unwrap();
    h.input_mut().events.extend([reset_completed(foreign), preedit("X")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日", "foreign completion cannot release the guard");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1));

    // A stale old commit is rejected while the fence is still Awaiting.
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("old".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日", "the stale prefix is rejected");

    // The exact completion in one batch, then a fresh suffix in the NEXT batch.
    h.input_mut().events.push(reset_completed(r1));
    h.run_steps(1);
    h.input_mut().events.push(preedit("Y"));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y", "the exact completion admits the next-batch suffix");
    h.event(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y");

    // A second interruption gets a newer id; completion plus the suffix may share one batch.
    h.event(preedit("Z"));
    h.run_steps(1);
    h.key_down(egui::Key::End);
    h.run_steps(1);
    let r2 = published_reset(&h);
    h.key_up(egui::Key::End);
    h.run_steps(1);
    assert_ne!(r1, r2, "another interruption supersedes the previous fence");
    h.input_mut().events.extend([reset_completed(r2), preedit("W")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日YZW", "the exact new completion admits the same-batch suffix");
}

/// Waiting frames without the exact completion emit no replacement token and keep input blocked.
#[test]
fn no_completion_fails_closed_and_unchanged_waiting_frames_do_not_emit_new_requests() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);
    for _ in 0..3 {
        h.input_mut().events.extend([Event::Ime(ImeEvent::Commit("X".into())), preedit("Y")]);
        h.run_steps(1);
        assert_eq!(title_text(h.state()), "日", "without a completion no IME mutation is admitted");
        assert!(h.output().platform_output.ime.as_ref().is_some_and(|ime| ime.reset_request.is_none()));
    }
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1));

    h.input_mut().events.extend([reset_completed(r1), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y", "the exact completion is the only release");
}

/// A discarded final `None` suspends the guard; returning to the same editor publishes a new id.
#[test]
fn discarded_final_none_suspends_and_the_same_editor_rearms_with_a_new_id() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);
    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("about"));
    h.ctx.request_discard("final IME descriptor is absent on this discarded pass");
    h.run_steps(1);
    assert!(
        matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }),
        "the discarded final-none pass retires R1 locally: {:?}",
        root_phase(&h)
    );
    assert!(h.output().platform_output.ime.is_none());

    h.state_mut().dialog = None;
    h.run_steps(1);
    let r2 = published_reset(&h);
    assert_ne!(r1, r2, "returning to the same caret publishes one replacement");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r2));

    // A late completion for the retired request is inert.
    h.input_mut().events.extend([reset_completed(r1), preedit("X")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日", "the retired R1 cannot release R2");
    h.input_mut().events.extend([reset_completed(r2), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y");
}

/// A real modal without a discarded pass suspends the guard and publishes no speculative token.
#[test]
fn actual_modal_without_discard_suspends_and_queues_no_token() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);

    // A normal About pass, no request_discard: the modal is an explicitly ineligible route even
    // though the canvas is painted behind it.
    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("about"));
    h.run_steps(2);
    assert!(
        matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }),
        "the modal retires R1 without allocating: {:?}",
        root_phase(&h)
    );
    assert!(h.output().platform_output.ime.is_none(), "the modal publishes no custom descriptor");

    // Force a stock descriptor while the modal stays open. A token queued on the modal pass would
    // be attached to this descriptor by egui; there must be none.
    let notes = egui::Id::unique("notes_edit");
    h.ctx.memory_mut(|m| m.request_focus(notes));
    h.run_steps(1);
    let stock = h.output().platform_output.ime.as_ref().expect("the notes field publishes its own descriptor");
    assert!(stock.reset_request.is_none(), "the modal pass did not queue a custom token for stock output");
    h.ctx.memory_mut(|m| m.surrender_focus(notes));

    // Returning to the custom editor publishes exactly one replacement, and only its exact
    // completion admits the suffix.
    h.state_mut().dialog = None;
    h.run_steps(1);
    let r2 = published_reset(&h);
    assert_ne!(r1, r2);
    h.input_mut().events.extend([reset_completed(r1), preedit("X")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日", "the retired modal-pass R1 stays inert");
    h.input_mut().events.extend([reset_completed(r2), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y");
}

/// A modal-owned observe/finalizer pass rejects a matching custom descriptor before allocating a
/// token.
#[test]
fn modal_observe_and_finalizer_reject_a_matching_custom_descriptor_before_allocation() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(1);
    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("about"));
    h.state_mut().set_ime_reset(
        egui::ViewportId::ROOT,
        crate::ImeResetPhase::Suspended { viewport: egui::ViewportId::ROOT, needs_rearm: true, request: None },
    );

    let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(1.0, 18.0));
    h.state_mut().ime_pending = Some(rect);
    let ctx = h.ctx.clone();
    ctx.output_mut(|o| o.ime = None);
    assert!(h.state_mut().observe_ime(&ctx).is_none(), "a pending canvas rect is hidden by the modal");
    assert!(ctx.output(|o| o.ime.is_none()), "the hidden custom owner is not republished");

    let owner = h.state().ime_target().expect("the title is still a custom target").owner;
    let key = crate::ImeDescriptorKey::new(owner, 1, egui::IMEPurpose::Normal);
    ctx.output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput {
            owner,
            context_generation: 1,
            purpose: egui::IMEPurpose::Normal,
            rect,
            cursor_rect: rect,
            should_interrupt_composition: false,
            reset_request: None,
            change_cause: egui::output::ImeChangeCause::Other,
            surrounding_text: egui::output::IMESurroundingText::Unavailable,
        });
    });
    h.state_mut().finalize_ime_reset(&ctx, Some(key));
    assert!(
        matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }),
        "the finalizer rejects a matching custom descriptor while the modal owns the route: {:?}",
        root_phase(&h)
    );
    assert!(ctx.output(|o| o.ime.as_ref().is_some_and(|ime| ime.reset_request.is_none())), "no token was added to the modal output");
}

/// Opening, Backstage, palette, and modal routes all report that the custom canvas cannot own
/// input.
#[test]
fn opening_backstage_palette_and_modal_routes_are_explicitly_ineligible() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(1);
    assert!(h.state().ime_custom_canvas_route(&h.ctx), "the plain canvas editor is eligible");

    h.state_mut().opening = Some(crate::opening::Opening::for_test("deck.pptx"));
    assert!(!h.state().ime_custom_canvas_route(&h.ctx), "the opening overlay hides the canvas");
    h.state_mut().opening = None;

    h.state_mut().open_backstage(crate::backstage::Page::Home);
    assert!(!h.state().ime_custom_canvas_route(&h.ctx), "Backstage hides the canvas");
    h.state_mut().close_backstage();

    h.state_mut().palette = Some((String::new(), 0));
    assert!(!h.state().ime_custom_canvas_route(&h.ctx), "the palette owns input");
    h.state_mut().palette = None;

    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("about"));
    assert!(!h.state().ime_custom_canvas_route(&h.ctx), "a modal owns input");
}

/// Stock focused controls keep their own descriptor; Backstage/no-editor routes suspend and then
/// re-arm once on return.
#[test]
fn stock_focus_backstage_and_no_editor_routes_preserve_stock_output_and_rearm() {
    // A focused stock TextEdit keeps its own descriptor; the custom fence is suspended.
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);
    let custom_owner = h.state().ime_target().unwrap().owner;
    let notes = egui::Id::unique("notes_edit");
    h.ctx.memory_mut(|m| m.request_focus(notes));
    h.run_steps(2);
    let stock = h.output().platform_output.ime.as_ref().expect("the notes field publishes its own IME descriptor");
    assert_ne!(stock.owner, custom_owner, "the stock field's descriptor was not overwritten");
    assert!(stock.reset_request.is_none(), "the finalizer did not attach a custom token to stock output");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { .. }));
    h.input_mut().events.extend([reset_completed(r1), preedit("X")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日", "late R1 is inert after stock focus");

    h.ctx.memory_mut(|m| m.surrender_focus(notes));
    h.run_steps(1);
    let r2 = published_reset(&h);
    assert_ne!(r1, r2);
    h.input_mut().events.extend([reset_completed(r2), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y", "returning to the custom editor re-arms once");
    h.event(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);

    // Backstage hides the canvas and must not edit the hidden slide.
    let r3 = settled_title_reset(&mut h);
    h.state_mut().open_backstage(crate::backstage::Page::Home);
    h.run_steps(1);
    assert!(h.output().platform_output.ime.is_none(), "Backstage publishes no canvas descriptor");
    assert_eq!(title_text(h.state()), "日Y日", "hidden slide is untouched");
    h.input_mut().events.extend([reset_completed(r3), preedit("Z")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y日", "Backstage leaves the old fence suspended");
    h.state_mut().close_backstage();
    h.run_steps(1);
    let r4 = published_reset(&h);
    assert_ne!(r3, r4);
    h.input_mut().events.extend([reset_completed(r4), preedit("Z")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y日Z", "returning from Backstage re-arms the same editor");
    h.event(Event::Ime(ImeEvent::Commit("Z".into())));
    h.run_steps(1);

    // A no-editor/multi-selection route also suspends and returns with one fresh request.
    let r5 = settled_title_reset(&mut h);
    h.state_mut().run("text.exit", json!({})).unwrap();
    h.state_mut().session.active_mut().unwrap().selection.shapes.clear();
    h.run_steps(1);
    assert!(h.output().platform_output.ime.is_none(), "no editor means no custom descriptor");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { .. }));
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title, "at": [0, 5]})).unwrap();
    h.run_steps(1);
    let r6 = published_reset(&h);
    assert_ne!(r5, r6);
    h.input_mut().events.extend([reset_completed(r6), preedit("K")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y日Z日K");
}

/// Full logic+UI passes do not re-arm, while standalone logic suspends exactly once without
/// replaying input.
#[test]
fn full_logic_ui_passes_do_not_rearm_while_standalone_logic_suspends_once() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);

    // The full pass runs App::logic before the UI. The mode hook must prevent the logic half
    // from treating the not-yet-painted UI as absence.
    h.run_steps(2);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1));
    assert!(h.output().platform_output.ime.as_ref().is_some_and(|ime| ime.reset_request.is_none()));

    let ctx = h.ctx.clone();
    // A child viewport frame supplies the actual viewport stack for its logic callback.
    let other = egui::ViewportId::from_hash_of("deckcraft test other viewport");
    ctx.run_ui(viewport_input(other), |ui| h.state_mut().logic(ui.ctx())).drop_without_applying_deltas();
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1));

    // Standalone root logic paints no geometry: suspend exactly once, without replaying input.
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| h.state_mut().logic(ctx));
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| h.state_mut().logic(ctx));
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));

    // The next full UI pass reacts to the actual custom descriptor and publishes one replacement.
    h.run_steps(1);
    let r2 = published_reset(&h);
    assert_ne!(r1, r2);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r2));
    h.run_steps(1);
    assert!(h.output().platform_output.ime.as_ref().is_some_and(|ime| ime.reset_request.is_none()));
}

/// A terminal change observed only in standalone logic never queues a token for a later stock
/// route.
#[test]
fn standalone_logic_changed_terminal_never_queues_a_token_for_the_next_stock_route() {
    let (mut h, _) = pending_terminal_editor();
    clear_pending_preedit(&mut h);
    let before = title_text(h.state());
    h.state_mut().run("text.select", json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();

    // A changed pending terminal reaches the interruption branch in a standalone run_logic tick.
    // Logic paints no geometry, so it must suspend locally instead of putting a request into
    // egui Memory; otherwise the next stock descriptor consumes the abandoned token.
    let ctx = h.ctx.clone();
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| h.state_mut().logic(ctx));
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }), "standalone logic allocates no token");
    assert!(h.state().ime_terminal_pending.as_ref().is_some_and(|p| p.cancelled), "the interruption is still recorded locally");
    assert!(h.state().ime_interrupt_pending, "the settled composition still needs its platform interrupt");

    h.state_mut().palette = Some((String::new(), 0));
    h.run_steps(1);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));
    assert!(h.output().platform_output.ime.as_ref().is_none_or(|ime| ime.reset_request.is_none()));
    // TextEdit requests focus after painting; its stock descriptor is on the next frame.
    h.run_steps(1);
    let stock = h.output().platform_output.ime.as_ref().expect("the palette field publishes its own descriptor");
    assert!(stock.reset_request.is_none(), "the abandoned logic tick did not attach a token to stock output");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));
    h.input_mut().events.push(preedit("X"));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), before, "stock input cannot edit the hidden slide");

    // Returning to the custom editor publishes exactly one replacement for the real new caret.
    h.state_mut().palette = None;
    if let Some(id) = h.ctx.memory(|m| m.focused()) {
        h.ctx.memory_mut(|m| m.surrender_focus(id));
    }
    h.run_steps(1);
    let r = published_reset(&h);
    h.input_mut().events.extend([reset_completed(r), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "aYbcd", "the exact replacement admits the fresh suffix at the real caret");
}

/// The exact completion admits a fresh suffix without admitting the old terminal commit.
#[test]
fn cleared_preedit_exact_completion_admits_fresh_suffix_without_the_old_terminal_commit() {
    // Same batch: the exact completion is the platform-reset boundary; no old Commit,
    // Enabled/Disabled or idle pass is needed before a new composition may start. The caret is
    // the real one the fixture selected, so the oracle is `aYbcd` (not the review's illustrative
    // `abcdY`, which assumed a different caret).
    let (mut h, _) = pending_terminal_editor();
    clear_pending_preedit(&mut h);
    let undo = undo_len(h.state());
    h.state_mut().run("text.select", json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();
    h.run_steps(1);
    let r1 = published_reset(&h);
    let pending = h.state().ime_terminal_pending.as_ref().expect("the retired cleared-preedit terminal is guarded");
    assert!(pending.cancelled);
    assert!(pending.interrupted_at.is_some(), "the interruption destination is stored separately from the origin");

    h.input_mut().events.extend([reset_completed(r1), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "aYbcd", "the exact completion admits the same-batch suffix");
    let t = h.state().session.active().unwrap().selection.text.as_ref().unwrap();
    assert_eq!((t.anchor, t.caret), ((0, 2), (0, 2)), "the fresh preedit advances the real caret");
    assert_eq!(undo_len(h.state()), undo, "a preedit creates no undo step");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear));
    assert!(h.state().ime_terminal_pending.is_none(), "the exact fence completion released the old terminal guard");
    for _ in 0..3 {
        h.run_steps(1);
        assert!(h.output().platform_output.ime.as_ref().is_some_and(|ime| ime.reset_request.is_none()));
        assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear), "unchanged idle passes do not request R churn");
    }
    h.event(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "aYbcd", "the committed suffix replaces its preedit");
    assert_eq!(undo_len(h.state()), undo + 1, "the fresh composition is exactly one undo step");

    // Next batch: completion alone releases, then the fresh suffix arrives in a later raw batch.
    let (mut h, _) = pending_terminal_editor();
    clear_pending_preedit(&mut h);
    let undo = undo_len(h.state());
    h.state_mut().run("text.select", json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();
    h.run_steps(1);
    let r1 = published_reset(&h);
    h.input_mut().events.push(reset_completed(r1));
    h.run_steps(1);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear));
    assert!(h.state().ime_terminal_pending.is_none());
    for _ in 0..3 {
        h.run_steps(1);
        assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear), "an idle pass after the exact completion keeps it clear");
        assert!(h.output().platform_output.ime.as_ref().is_some_and(|ime| ime.reset_request.is_none()));
    }
    h.input_mut().events.push(preedit("Y"));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "aYbcd", "the next-batch suffix is admitted");
    h.event(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "aYbcd");
    assert_eq!(undo_len(h.state()), undo + 1);
}

/// Repeating the exact completion after `Clear` changes no composition or undo state.
#[test]
fn duplicate_exact_completion_after_clear_is_inert() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);
    let undo = undo_len(h.state());

    h.input_mut().events.push(reset_completed(r1));
    h.run_steps(1);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear));

    // A duplicate after Clear does nothing, and the same batch may carry a fresh preedit.
    h.input_mut().events.extend([reset_completed(r1), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y");
    assert_eq!(undo_len(h.state()), undo);
    h.event(Event::Ime(ImeEvent::Commit("Y".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y");

    // A duplicate old completion never clears the latest replacement either.
    h.event(preedit("Z"));
    h.run_steps(1);
    h.key_down(egui::Key::End);
    h.run_steps(1);
    let r2 = published_reset(&h);
    h.key_up(egui::Key::End);
    h.run_steps(1);
    assert_ne!(r1, r2);
    h.input_mut().events.extend([reset_completed(r1), preedit("X")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日YZ", "the duplicate old completion cannot release R2");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r2));
    h.input_mut().events.extend([reset_completed(r2), preedit("W")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日YZW");
}

/// The font-install early return suspends without a token, and returning to the editor re-arms
/// once.
#[test]
fn font_install_early_return_suspends_without_a_speculative_token_and_rearms_once() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);
    let before = title_text(h.state());

    // Force the real ui_setup font-install early return. The outer all-exit wrapper must still
    // observe the absent custom descriptor and retire R1.
    h.state_mut().styled = false;
    // A discarded pass is invisible to `run_ui`: it retries immediately, up to `max_passes`.
    // Drive exactly one root UI pass here so the discarded pass itself can be observed, while
    // keeping the normal two-pass budget so `Context::will_discard` is genuinely true
    // (`num_completed_passes + 1 < max_passes`; with `max_passes == 1` it would be false and the
    // finalizer would take the accepted-pass branch). This calls only `SlideApp::ui`, not
    // `run_ui` or its plugin pass hooks, and not `SlideApp::logic`; the font-install early return
    // under test does not depend on them.
    let input = h.input_mut().take();
    let ctx = h.ctx.clone();
    ctx.begin_pass(input);
    ctx.root_ui(|ui| h.state_mut().ui(ui));
    assert!(ctx.will_discard(), "the font-install pass is a discarded pass under the normal budget");
    let pass = ctx.end_pass();
    let published = pass.platform_output.ime.clone();
    pass.drop_without_applying_deltas();
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));
    assert!(published.is_none(), "the discarded font pass publishes no custom descriptor");

    h.input_mut().events.extend([reset_completed(r1), preedit("X")]);
    h.run_steps(1);
    let r2 = published_reset(&h);
    assert_ne!(r1, r2, "the late R1 is inert; returning custom paints one replacement");
    assert_eq!(title_text(h.state()), before, "the late preedit stays blocked");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r2));

    h.input_mut().events.extend([reset_completed(r2), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), format!("{before}Y"));
}

/// The palette route retires the guard without a token; returning to the canvas publishes one
/// replacement.
#[test]
fn palette_route_retires_the_guard_without_a_speculative_token_and_rearms_once() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);
    let before = title_text(h.state());

    h.state_mut().palette = Some((String::new(), 0));
    h.run_steps(2);
    let output = h.output().platform_output.ime.as_ref();
    assert!(output.is_none_or(|output| output.reset_request.is_none()), "the finalizer never attaches a custom token to stock output");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));

    h.input_mut().events.extend([reset_completed(r1), preedit("X")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), before, "the palette leaves the hidden slide untouched");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { .. }));

    h.state_mut().palette = None;
    if let Some(id) = h.ctx.memory(|m| m.focused()) {
        h.ctx.memory_mut(|m| m.surrender_focus(id));
    }
    h.run_steps(1);
    let r2 = published_reset(&h);
    assert_ne!(r1, r2);
    h.input_mut().events.extend([reset_completed(r2), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), format!("{before}Y"));
}

/// A render-only viewport cannot mutate the root composition or bind the root reset guard.
#[test]
fn render_only_viewports_cannot_mutate_root_composition_or_bind_the_root_guard() {
    let (mut h, _) = pending_terminal_editor();
    clear_pending_preedit(&mut h);
    h.state_mut().run("text.select", json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();
    h.run_steps(1);
    let r1 = published_reset(&h);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1));

    // Move the live selection without a root pass. A second window/presenter viewport renders
    // through its own immediate viewport and never calls `SlideApp::logic`/`ui`; admitting such a
    // callback would mutate root composition state while only the reset map is per-viewport.
    h.state_mut().run("text.select", json!({"anchor": [0, 2], "caret": [0, 2]})).unwrap();
    let before = ime_state(&h);
    // `run_ui` for a child viewport runs that viewport's whole pass and returns its output; the
    // harness keeps the last ROOT output. Publish nothing for the render-only viewport and leave
    // the root's already-published token untouched.
    let root_ime_before = h.output().platform_output.ime.clone();
    assert_eq!(
        root_ime_before.as_ref().and_then(|ime| ime.reset_request),
        Some(r1),
        "the root descriptor legitimately still carries the published R1"
    );
    let child = egui::ViewportId::from_hash_of("deckcraft test child viewport");
    let ctx = h.ctx.clone();
    let child_logic = ctx.run_ui(viewport_input(child), |ui| h.state_mut().logic(ui.ctx()));
    assert!(child_logic.platform_output.ime.is_none(), "rejected child logic publishes no IME descriptor");
    child_logic.drop_without_applying_deltas();
    assert_eq!(ime_state(&h), before, "rejected child logic left every root composition field unchanged");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1), "root R1 survives the child tick");
    assert!(matches!(phase_at(&h, child), crate::ImeResetPhase::Clear), "no child guard was admitted");
    assert_eq!(h.output().platform_output.ime, root_ime_before, "the child tick does not clear or rebind the root's published token");

    let child_ui = ctx.run_ui(viewport_input(child), |ui| h.state_mut().ui(ui));
    assert!(child_ui.platform_output.ime.is_none(), "rejected child UI publishes no IME descriptor");
    child_ui.drop_without_applying_deltas();
    assert_eq!(ime_state(&h), before, "rejected child UI left every root composition field unchanged");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1));
    assert_eq!(h.output().platform_output.ime, root_ime_before, "the child UI does not clear or rebind the root's published token");

    // The root guard, not a child request, is what admits the real next-batch input.
    h.input_mut().events.extend([reset_completed(r1), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "abYcd", "the moved real caret receives the fresh suffix");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear));
}

/// A suspended guard or request stamped for another viewport is never rebound to root.
#[test]
fn a_suspended_guard_or_request_stamped_for_another_viewport_is_never_rebound() {
    let mut h = harness(Session::with_new());
    let _ = settled_title_reset(&mut h);
    let child = egui::ViewportId::from_hash_of("deckcraft foreign suspended viewport");
    let child_request = h.ctx.request_ime_reset(child).expect("a real child request token");
    h.state_mut()
        .ime_reset
        .insert(egui::ViewportId::ROOT, crate::ImeResetPhase::Suspended { viewport: child, needs_rearm: true, request: Some(child_request) });
    h.run_steps(1);
    let root_request = published_reset(&h);
    assert_ne!(root_request, child_request, "a request issued for the child viewport is not rebound to root");
    assert_eq!(root_request.viewport_id(), egui::ViewportId::ROOT);
    assert!(
        matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, viewport, .. } if id == root_request && viewport == egui::ViewportId::ROOT)
    );

    // A child completion delivered on the root stream is foreign and inert.
    h.input_mut().events.extend([reset_completed(child_request), preedit("X")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日", "the foreign request completion cannot clear the root fence");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == root_request));
    h.input_mut().events.extend([reset_completed(root_request), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y");
}

/// Sorter and Notes Page stock routes allocate no custom reset token when a composition settles.
#[test]
fn sorter_and_notes_page_settles_never_allocate_for_a_stock_route() {
    for (view, command) in [(crate::ViewMode::Sorter, "view.sorter"), (crate::ViewMode::NotesPage, "view.notesPage")] {
        let mut h = harness(Session::with_new());
        let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
        h.state_mut().run("text.edit", json!({"id": title})).unwrap();
        h.run_steps(2);
        assert!(h.state().ime_custom_canvas_route(&h.ctx), "the plain canvas editor is eligible");
        h.event(preedit("日"));
        h.run_steps(1);
        let before = title_text(h.state());

        // Switch to a production view that paints no canvas producer. The retained text
        // selection still yields an IME owner, so a negative-only admission check would let the
        // settle below allocate a token for a route that never publishes a custom descriptor.
        h.state_mut().run(command, json!({})).unwrap();
        assert_eq!(h.state().ui.view, view);
        assert!(h.state().ime_target().is_some(), "the retained text selection still yields an owner");
        assert!(!h.state().ime_custom_canvas_route(&h.ctx), "{command} does not call canvas::show");

        h.key_down(egui::Key::End);
        h.run_steps(1);
        assert!(
            matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }),
            "the settle retires the old guard before the finalizer; no request is allocated in input: {:?}",
            root_phase(&h)
        );
        h.key_up(egui::Key::End);
        h.run_steps(1);
        assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));

        // The palette's focused TextEdit is a stock descriptor. A token allocated on the sorter
        // settle would be attached here by egui.
        h.state_mut().palette = Some((String::new(), 0));
        h.run_steps(1);
        assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));
        assert!(h.output().platform_output.ime.as_ref().is_none_or(|ime| ime.reset_request.is_none()));
        // TextEdit requests focus after painting; its stock descriptor is on the next frame.
        h.run_steps(1);
        let stock = h.output().platform_output.ime.as_ref().expect("the palette TextEdit publishes its own descriptor");
        assert!(stock.reset_request.is_none(), "{command} did not attach a speculative token to stock output");
        h.input_mut().events.push(preedit("X"));
        h.run_steps(1);
        assert_eq!(title_text(h.state()), before, "stock input cannot edit the hidden canvas");

        // Returning to the custom canvas publishes exactly one replacement, and only its exact
        // completion admits the fresh suffix.
        h.state_mut().palette = None;
        if let Some(id) = h.ctx.memory(|m| m.focused()) {
            h.ctx.memory_mut(|m| m.surrender_focus(id));
        }
        h.state_mut().run("view.normal", json!({})).unwrap();
        h.run_steps(1);
        let reset = published_reset(&h);
        h.input_mut().events.extend([reset_completed(reset), preedit("Y")]);
        h.run_steps(1);
        assert_eq!(title_text(h.state()), format!("{before}Y"), "returning to the canvas re-arms once");
        h.event(Event::Ime(ImeEvent::Commit("Y".into())));
        h.run_steps(1);
        assert_eq!(title_text(h.state()), format!("{before}Y"));
    }
}

/// A same-pass switch from custom canvas to stock palette retires the guard without publishing a
/// token.
#[test]
fn same_pass_custom_to_stock_palette_retires_the_guard_without_a_new_token() {
    let mut h = harness(Session::with_new());
    let r1 = settled_title_reset(&mut h);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == r1));

    let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(1.0, 18.0));
    let ctx = h.ctx.clone();
    let mut custom = None;
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let ctx = ui.ctx();
        h.state_mut().ime_frame_viewport = Some(egui::ViewportId::ROOT);
        // A pre-UI interruption in a full pass must retire the guard locally, not allocate.
        // The old implementation allocated here because the canvas route looked available.
        h.state_mut().note_ime_interruption(ctx, true);
        assert!(matches!(h.state().ime_reset_for(egui::ViewportId::ROOT), crate::ImeResetPhase::Suspended { request: None, .. }));

        h.state_mut().ime_pending = Some(rect);
        // The canvas producer ran first and published a matching custom descriptor this pass.
        custom = h.state_mut().observe_ime(ctx);
        assert!(custom.is_some(), "the canvas's descriptor is eligible");

        // Later in the same pass the command palette opens and its TextEdit owns IME output.
        // Replace the pass output with that stock descriptor before the finalizer runs.
        h.state_mut().palette = Some((String::new(), 0));
        let stock_owner = egui::Id::unique("same-pass-stock-palette");
        ctx.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                owner: stock_owner,
                context_generation: 1,
                purpose: egui::IMEPurpose::Normal,
                rect,
                cursor_rect: rect,
                should_interrupt_composition: false,
                reset_request: None,
                change_cause: egui::output::ImeChangeCause::Other,
                surrounding_text: egui::output::IMESurroundingText::Unavailable,
            });
        });
        h.state_mut().finalize_ime_reset(ctx, custom);
    });
    // Read after `end_pass`: this is where egui would attach a request left in Memory to the
    // pass's stock descriptor. It must be absent.
    let stock_reset = output.platform_output.ime.as_ref().and_then(|ime| ime.reset_request);
    output.drop_without_applying_deltas();
    assert!(stock_reset.is_none(), "the pre-UI interruption did not leave a token for the stock descriptor");
    assert!(
        matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }),
        "the custom->stock pass retires R1 without allocating: {:?}",
        root_phase(&h)
    );

    // A full palette pass must not publish R1 or any replacement into the stock field.
    h.run_steps(1);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Suspended { request: None, .. }));
    assert!(h.output().platform_output.ime.as_ref().is_none_or(|ime| ime.reset_request.is_none()));
    // TextEdit requests focus after painting; its stock descriptor is on the next frame.
    h.run_steps(1);
    let stock = h.output().platform_output.ime.as_ref().expect("the palette TextEdit publishes its own descriptor");
    assert!(stock.reset_request.is_none());
    assert!(!h.state().ime_custom_canvas_route(&h.ctx));

    h.state_mut().palette = None;
    if let Some(id) = h.ctx.memory(|m| m.focused()) {
        h.ctx.memory_mut(|m| m.surrender_focus(id));
    }
    h.run_steps(1);
    let r2 = published_reset(&h);
    assert_ne!(r1, r2, "returning to the canvas requests one replacement");
    h.input_mut().events.extend([reset_completed(r1), preedit("X")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日", "the retired custom R1 stays inert");
    h.input_mut().events.extend([reset_completed(r2), preedit("Y")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日Y");
}

/// Closing and creating a deck before a pass keeps the old terminal fence until its exact reset
/// completion.
#[test]
fn close_new_then_edit_before_a_pass_keeps_the_old_terminal_fence_until_exact_reset() {
    let (mut h, _) = pending_terminal_editor();
    clear_pending_preedit(&mut h);
    let old_owner = h.state().ime_target().unwrap().owner;
    let old_uid = h.state().session.active().unwrap().uid;
    let pending = h.state().ime_terminal_pending.as_ref().expect("the cleared-preedit terminal is guarded");
    assert_eq!(pending.owner, old_owner);
    assert!(!pending.cancelled);

    // Replace the active document and enter its first editor without an intervening pass
    // observing `None`. Blindly clearing on the uid change would let the old platform suffix
    // land in the new document, so the old terminal must stay bound to its old owner.
    h.state_mut().run("file.close", json!({})).unwrap();
    h.state_mut().run("file.new", json!({})).unwrap();
    let new_uid = h.state().session.active().unwrap().uid;
    assert_ne!(old_uid, new_uid);
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.run_steps(1);

    let pending = h.state().ime_terminal_pending.as_ref().expect("the old terminal survives the document replacement");
    assert_eq!(pending.owner, old_owner, "the terminal is still bound to the retired composition's owner");
    assert!(pending.cancelled && pending.interrupted);
    let reset = published_reset(&h);
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == reset));
    let before = title_text(h.state());
    let undo = undo_len(h.state());

    // The old composition's suffix arrives before its terminal commit: no preedit may enter the
    // new document's editor, and the old terminal guard must still be live.
    h.input_mut().events.push(preedit("F"));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), before, "a stale old preedit is rejected");
    assert_eq!(undo_len(h.state()), undo);
    assert!(h.state().ime_terminal_pending.as_ref().is_some_and(|p| p.owner == old_owner && p.cancelled));
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == reset));

    // The old terminal commit is rejected and consumed as a terminal event. The new document's
    // reset fence remains Awaiting: terminal delivery is not the platform-reset boundary.
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("F".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), before, "the old terminal commit is rejected");
    assert_eq!(undo_len(h.state()), undo);
    assert!(h.state().ime_terminal_pending.is_none(), "the delivered old terminal no longer needs a guard");
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Awaiting { id, .. } if id == reset));

    // Another stale suffix still cannot cross the new document's reset fence.
    h.input_mut().events.push(preedit("F"));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), before, "the reset fence, not just the terminal, blocks the old suffix");

    // Only the exact completion of the new document's fence releases custom input. The latest
    // new composition then gets its own preedit and commit.
    h.input_mut().events.extend([reset_completed(reset), preedit("F")]);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), format!("{before}F"), "the fresh new-document composition is admitted");
    assert!(h.state().ime_terminal_pending.is_none());
    assert!(matches!(root_phase(&h), crate::ImeResetPhase::Clear));
    h.event(Event::Ime(ImeEvent::Commit("F".into())));
    h.run_steps(1);
    assert_eq!(title_text(h.state()), format!("{before}F"));
    assert_eq!(undo_len(h.state()), undo + 1, "only the latest new composition is one undo step");
}
