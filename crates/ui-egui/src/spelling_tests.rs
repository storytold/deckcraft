//! The Spelling dialog with a dictionary: suggestions change the word, Ignore All hides it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

#[test]
fn spelling_dialog_changes_and_ignores_words() {
    let aff = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwzxjq\nSFX S Y 1\nSFX S 0 s .\n";
    let dic = "6\nthe/S\nslide/S\nquarter/S\nplan/S\nour\nfor\n";
    deckcraft_engine::spell::install("en-US", deckcraft_engine::spell::Dictionary::parse(aff.as_bytes(), dic.as_bytes()));
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 500, 120], "text": "Our plna for teh quarter"})).unwrap();
    app.dialog = Some(crate::dialogs::Dialog::new("spelling"));
    let mut h = Harness::builder().with_size(egui::vec2(1100.0, 700.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    if let (Ok(path), Ok(img)) = (std::env::var("DECKCRAFT_PNG_OUT"), h.render()) {
        let _ = img.save(path);
    }
    h.get_by_label("plan").click();
    h.run_steps(3);
    let text = |h: &Harness<'_, SlideApp>| {
        let st = h.state().session.active().unwrap();
        let mut out = String::new();
        deckcraft_model::walk(&st.doc.slides[0].shapes, &mut |s, _| {
            if let Some(t) = &s.text {
                out += &t.text();
            }
        });
        out
    };
    assert!(text(&h).contains("Our plan for teh"), "{}", text(&h));
    h.get_by_label("Ignore All").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("No issues found").is_some());
}

/// Misspelt words on the slide get squiggles from the background check; not the word being
/// typed; the context menu's correction and Add to Dictionary clear them.
#[test]
fn squiggles_mark_misspelt_words_in_the_background() {
    let aff = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwzxjq\nSFX S Y 1\nSFX S 0 s .\n";
    let dic = "7\nthe/S\nslide/S\nquarter/S\nplan/S\nour\nfor\nreview\n";
    deckcraft_engine::spell::install("xx-SQ", deckcraft_engine::spell::Dictionary::parse(aff.as_bytes(), dic.as_bytes()));
    let dir = std::env::temp_dir().join(format!("deckcraft-ui-userdic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    deckcraft_engine::spell::set_user_dictionary_path(dir.join("user.dic"));
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.session.execute("slide.layout", &json!({"layout": "blank"})).ok();
    let id = app
        .session
        .execute("shape.insert", &json!({"preset": "rect", "rect": [80, 120, 700, 160], "text": "Our plna for teh quarter reveiw"}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    app.session.execute("format.size", &json!({"size": 40})).unwrap();
    // Mark the runs' language directly (what a PPTX run's `lang` gives).
    let sid = deckcraft_model::ShapeId(id as u32);
    app.session
        .edit(|doc, _| {
            let sl = std::sync::Arc::make_mut(&mut doc.slides[0]);
            let sh = deckcraft_model::find_shape_mut(&mut sl.shapes, sid).unwrap();
            for r in sh.text.as_mut().unwrap().paragraphs.iter_mut().flat_map(|p| p.runs.iter_mut()) {
                r.props.lang = Some("xx-SQ".into());
            }
            Ok(())
        })
        .unwrap();
    app.session.execute("edit.select", &json!({"ids": []})).unwrap();
    let st = app.session.active().unwrap().clone();
    let lines = app.squiggles.settle(&st);
    assert_eq!(lines.len(), 3, "{lines:?}");
    let mut h = Harness::builder().with_size(egui::vec2(1100.0, 700.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    if let (Ok(path), Ok(img)) = (std::env::var("DECKCRAFT_PNG_OUT_SQUIGGLES"), h.render()) {
        let _ = img.save(path);
    }
    // The context menu's word under the first squiggle, and its correction.
    let st = h.state().session.active().unwrap().clone();
    let (a, _) = lines[0];
    let w = h.state().squiggles.word_at(&st, deckcraft_geom::Point::new(a.x + 5.0, a.y - 5.0), 2.0).unwrap();
    assert_eq!(w.word, "plna");
    assert_eq!(w.suggestions.first().map(String::as_str), Some("plan"));
    h.state_mut().session.execute("review.spellingChange", &json!({"shape": id, "para": 0, "start": w.start, "word": w.word, "to": "plan"})).unwrap();
    h.state_mut().session.execute("review.addToDictionary", &json!({"word": "reveiw"})).unwrap();
    let st = h.state().session.active().unwrap().clone();
    assert_eq!(h.state_mut().squiggles.settle(&st).len(), 1);
    // Typing at the end of a misspelt word: no squiggle under it until the caret leaves.
    h.state_mut().session.execute("edit.select", &json!({"ids": [id]})).unwrap();
    h.state_mut().session.execute("text.edit", &json!({})).unwrap();
    h.state_mut().session.execute("text.select", &json!({"anchor": [0, 16], "caret": [0, 16]})).unwrap();
    let st = h.state().session.active().unwrap().clone();
    assert_eq!(st.selection.text.as_ref().map(|t| t.caret), Some((0, 16)));
    assert_eq!(h.state_mut().squiggles.settle(&st).len(), 0, "the word being typed isn't marked");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Table cells and speaker notes are squiggled too, and the UI-thread checker (the web build's)
/// gets there a few milliseconds per frame.
#[test]
fn squiggles_in_table_cells_and_notes_and_on_the_ui_thread() {
    let aff = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwzxjq\nSFX S Y 1\nSFX S 0 s .\n";
    let dic = "10\nthe/S\nplan/S\nsales\nregion/S\ntotal/S\nnorth\nsouth\nremember\nto\nshow\n";
    deckcraft_engine::spell::install("xx-TB", deckcraft_engine::spell::Dictionary::parse(aff.as_bytes(), dic.as_bytes()));
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.squiggles.force_inline = true;
    app.session.execute("slide.layout", &json!({"layout": "blank"})).ok();
    let id = app.session.execute("insert.table", &json!({"rows": 2, "cols": 2, "rect": [80, 80, 600, 160]})).unwrap()["id"].as_u64().unwrap();
    let sid = deckcraft_model::ShapeId(id as u32);
    let lang = deckcraft_model::RunProps { lang: Some("xx-TB".into()), size: Some(28.0), ..Default::default() };
    app.session
        .edit(|doc, _| {
            let sl = std::sync::Arc::make_mut(&mut doc.slides[0]);
            sl.notes = deckcraft_model::TextBody {
                paragraphs: vec![deckcraft_model::Paragraph {
                    runs: vec![deckcraft_model::Run::with("Remmeber to show the totl", lang.clone())],
                    ..Default::default()
                }],
                ..Default::default()
            };
            let sh = deckcraft_model::find_shape_mut(&mut sl.shapes, sid).unwrap();
            let deckcraft_model::ShapeKind::Table(tb) = &mut sh.kind else { panic!("not a table") };
            for (r, c, t) in [(0, 0, "Regoin"), (0, 1, "Sales"), (1, 0, "North"), (1, 1, "Soth totals")] {
                tb.cell_mut(r, c).unwrap().text.paragraphs =
                    vec![deckcraft_model::Paragraph { runs: vec![deckcraft_model::Run::with(t, lang.clone())], ..Default::default() }];
            }
            Ok(())
        })
        .unwrap();
    app.session.execute("edit.select", &json!({"ids": []})).unwrap();
    let st = app.session.active().unwrap().clone();
    let lines = app.squiggles.settle(&st);
    assert_eq!(lines.len(), 2, "Regoin and Soth: {lines:?}");
    // Each under its own cell: the first in the top-left one, the second in the bottom-right one.
    assert!(lines[0].0.x < 380.0 && lines[0].0.y < 160.0 && lines[1].0.x > 380.0 && lines[1].0.y > 160.0, "{lines:?}");
    assert_eq!(app.squiggles.settle_notes("Remmeber to show the totl", "xx-TB"), vec![(0, 8), (21, 25)]);
    let mut h = Harness::builder().with_size(egui::vec2(1100.0, 760.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(6);
    if let (Ok(path), Ok(img)) = (std::env::var("DECKCRAFT_PNG_OUT_TABLE"), h.render()) {
        let _ = img.save(path);
    }
    // Correcting a cell's word through the context menu's command (settled again: frames may
    // have moved the document on).
    let st = h.state().session.active().unwrap().clone();
    let lines = h.state_mut().squiggles.settle(&st);
    let (a, _) = lines[0];
    let w = h.state().squiggles.word_at(&st, deckcraft_geom::Point::new(a.x + 3.0, a.y - 5.0), 2.0).unwrap();
    assert_eq!((w.word.as_str(), w.suggestions.first().map(String::as_str)), ("Regoin", Some("Region")));
    h.state_mut()
        .session
        .execute("review.spellingChange", &json!({"shape": id, "row": 0, "col": 0, "para": 0, "start": w.start, "word": w.word, "to": "Region"}))
        .unwrap();
    let st = h.state().session.active().unwrap().clone();
    assert_eq!(h.state_mut().squiggles.settle(&st).len(), 1);
    // The Spelling dialog's list has the cell and the notes too.
    let v = h.state_mut().session.execute("review.spelling", &json!({})).unwrap();
    let words: Vec<&str> = v.as_array().unwrap().iter().filter_map(|w| w["word"].as_str()).collect();
    assert_eq!(words, vec!["Soth", "Remmeber", "totl"]);
}

fn right_click(h: &mut Harness<'_, SlideApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::default() });
    }
    h.run_steps(3);
}

/// The notes pane's right-click menu corrects a word and deletes a repeated one; the Language
/// dialog marks text and Do Not Check Spelling clears its squiggles.
#[test]
fn notes_menu_language_dialog_and_repeated_words() {
    use egui::accesskit::Role;
    let aff = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwzxjq\n";
    let dic = "6\nthe\nplan\nremember\nis\ngood\nwork\n";
    deckcraft_engine::spell::install("xx-NM", deckcraft_engine::spell::Dictionary::parse(aff.as_bytes(), dic.as_bytes()));
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.session.execute("slide.layout", &json!({"layout": "blank"})).ok();
    let id = app
        .session
        .execute("shape.insert", &json!({"preset": "rect", "rect": [80, 120, 700, 160], "text": "the plan is is good wrok"}))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    app.session.execute("format.size", &json!({"size": 40})).unwrap();
    app.session.execute("format.language", &json!({"lang": "xx-NM"})).unwrap();
    let lang = deckcraft_model::RunProps { lang: Some("xx-NM".into()), ..Default::default() };
    app.session
        .edit(|doc, _| {
            std::sync::Arc::make_mut(&mut doc.slides[0]).notes = deckcraft_model::TextBody {
                paragraphs: vec![deckcraft_model::Paragraph {
                    runs: vec![deckcraft_model::Run::with("Remmeber the the plan", lang.clone())],
                    ..Default::default()
                }],
                ..Default::default()
            };
            Ok(())
        })
        .unwrap();
    let st = app.session.active().unwrap().clone();
    // "is is" is flagged as repeated, "wrok" as misspelt.
    assert_eq!(app.squiggles.settle(&st).len(), 2);
    let mut h = Harness::builder().with_size(egui::vec2(1100.0, 760.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(8);
    if let (Ok(path), Ok(img)) = (std::env::var("DECKCRAFT_PNG_OUT_NOTES"), h.render()) {
        let _ = img.save(path);
    }
    // Right-click the notes' first word, once the (background) notes check is in.
    assert_eq!(h.state_mut().squiggles.settle_notes("Remmeber the the plan", "xx-NM"), vec![(0, 8), (13, 16)]);
    h.run_steps(2);
    let notes = h.query_all_by_role(Role::MultilineTextInput).next().map(|n| n.rect()).unwrap();
    right_click(&mut h, egui::pos2(notes.left() + 14.0, notes.top() + 8.0));
    if let (Ok(path), Ok(img)) = (std::env::var("DECKCRAFT_PNG_OUT_NOTES_MENU"), h.render()) {
        let _ = img.save(path);
    }
    assert_eq!(h.state().squiggles.notes_menu.as_ref().map(|w| w.word.as_str()), Some("Remmeber"));
    h.get_by_label("Remember").click();
    h.run_steps(3);
    let notes_text = |h: &Harness<'_, SlideApp>| h.state().session.active().unwrap().doc.slides[0].notes_text();
    assert_eq!(notes_text(&h), "Remember the the plan");
    // The repeated "the": its squiggle starts after "Remember the ".
    h.state_mut().squiggles.settle_notes("Remember the the plan", "xx-NM");
    h.run_steps(4);
    let x = h.query_all_by_role(Role::MultilineTextInput).next().map(|n| n.rect()).unwrap();
    let mut found = false;
    for dx in (60..160).step_by(6) {
        right_click(&mut h, egui::pos2(x.left() + dx as f32, x.top() + 8.0));
        if h.state().squiggles.notes_menu.as_ref().is_some_and(|w| w.delete_from.is_some()) {
            found = true;
            break;
        }
        h.event(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
        h.run_steps(2);
    }
    assert!(found, "no repeated word menu in the notes");
    h.get_by_label("Delete Repeated Word").click();
    h.run_steps(3);
    assert_eq!(notes_text(&h), "Remember the plan");
    // Language dialog: Do Not Check Spelling on the shape clears its squiggles.
    h.state_mut().session.execute("edit.select", &json!({"ids": [id]})).unwrap();
    h.state_mut().dialog = Some(crate::dialogs::Dialog::new("language"));
    h.run_steps(4);
    if let (Ok(path), Ok(img)) = (std::env::var("DECKCRAFT_PNG_OUT_LANG"), h.render()) {
        let _ = img.save(path);
    }
    assert_eq!(crate::dialogs::selected_language(h.state()).0, "xx-NM");
    h.get_by_label("Do not check spelling or grammar").click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert_eq!(crate::dialogs::selected_language(h.state()), ("xx-NM".to_string(), true));
    let st = h.state().session.active().unwrap().clone();
    assert!(h.state_mut().squiggles.settle(&st).is_empty(), "nothing in text marked Do Not Check is flagged, repeats included");
}
