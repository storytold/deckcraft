//! #101: Korean, Japanese and Chinese in the interface: input method composition on the slide and
//! in the notes pane (egui `Ime` events, as egui-winit sends them), and interface text drawn with
//! real glyphs rather than boxes.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use egui::{Event, FontId, ImeEvent};
use serde_json::json;

use crate::{Services, SlideApp};
use deckcraft_engine::Session;

fn harness(session: Session) -> egui_kittest::Harness<'static, SlideApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, Services::default()),
    );
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
    let ime = h.output().platform_output.ime.expect("the input method is on while editing");
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
    let ime = h.output().platform_output.ime.unwrap();
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
    h.key_press(egui::Key::End);
    h.run_steps(1);
    assert_eq!(title_text(h.state()), "日本ご");
    assert!(h.state().session.active().unwrap().composition.is_none());
    assert_eq!(undo_len(h.state()), undo0 + 1);
}

/// A click or a command settles a composition the input method is still composing (하). The app
/// tells the input method to stop, so the next key starts a new syllable instead of growing the
/// settled one (하 + ㄴ → 한), and the input method's late commit of 하 isn't typed a second time
/// (Windows' Korean IME sends it with the next key: "안녕" became "안녕녕나").
#[test]
fn a_settled_composition_interrupts_the_input_method_and_is_not_typed_twice() {
    let mut h = harness(Session::with_new());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().run("text.edit", json!({"id": title})).unwrap();
    h.event(preedit("ㅎ"));
    h.event(preedit("하"));
    h.run_steps(2);
    assert!(!h.output().platform_output.ime.unwrap().should_interrupt_composition);
    h.state_mut().run("format.bold", json!({})).unwrap();
    h.run_steps(1);
    assert!(h.output().platform_output.ime.unwrap().should_interrupt_composition, "the input method is told to stop composing");
    assert_eq!(title_text(h.state()), "하", "the syllable stays");
    h.run_steps(1);
    assert!(!h.output().platform_output.ime.unwrap().should_interrupt_composition, "once");
    // What the input method sends with the next key, ㄴ.
    for e in [preedit(""), Event::Ime(ImeEvent::Commit("하".into())), preedit("ㄴ"), Event::Ime(ImeEvent::Commit("ㄴ".into()))] {
        h.event(e);
        h.run_steps(1);
    }
    assert_eq!(title_text(h.state()), "하ㄴ");
    // A later word that happens to be the same syllable is typed as usual.
    for e in [preedit("ㅎ"), preedit("하"), Event::Ime(ImeEvent::Commit("하".into()))] {
        h.event(e);
        h.run_steps(1);
    }
    assert_eq!(title_text(h.state()), "하ㄴ하");
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
    let id = egui::Id::new("notes_edit");
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run_steps(2);
    assert!(h.ctx.memory(|m| m.has_focus(id)));
    let undo0 = undo_len(h.state());
    for pre in ["ㄴ", "노", "놑", "노트"] {
        h.event(preedit(pre));
        h.run_steps(1);
        assert_eq!(h.state().notes_buf.2.text, pre, "the preedit replaces the last one");
    }
    let ime = h.output().platform_output.ime.expect("the notes field turns the input method on");
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
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.set", &json!({"id": title, "text": japanese})).unwrap();
    s.execute("slide.notes", &json!({"text": "발표자 노트입니다"})).unwrap();
    s.execute("section.add", &json!({"name": chinese})).unwrap();
    s.doc_mut().unwrap().path = Some(format!("{korean}.pptx"));
    let mut h = harness(s);
    h.state_mut().run("view.outline", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().title().contains(korean));
    let mut checked = 0;
    for text in [korean, japanese, chinese, "발표자 노트입니다"] {
        if !system_has(text) {
            eprintln!("skipped {text:?}: no font on this machine draws it");
            continue;
        }
        for family in [FontId::proportional(13.0), crate::theme::bold(13.0)] {
            let missing: String = text.chars().filter(|c| !c.is_whitespace() && !h.ctx.fonts_mut(|f| f.has_glyph(&family, *c))).collect();
            assert!(missing.is_empty(), "{family:?} draws {missing:?} of {text:?} as boxes");
        }
        checked += 1;
    }
    if cfg!(windows) || cfg!(target_os = "macos") {
        assert_eq!(checked, 4, "Windows and macOS have Korean and Chinese fonts");
    }
    // Text typed later (a dialog field, a new comment) is looked at too.
    let typed = "ไทย";
    h.event(Event::Text(typed.into()));
    h.run_steps(2);
    if system_has(typed) {
        assert!(h.ctx.fonts_mut(|f| f.has_glyphs(&FontId::proportional(13.0), typed)));
    }
}
