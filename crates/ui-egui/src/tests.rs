#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use egui_kittest::kittest::Queryable;

fn shape_rot(app: &SlideApp) -> f64 {
    let d = app.session.active().unwrap();
    let id = d.selection.shapes[0];
    d.shape(id).unwrap().xfrm.unwrap().rot
}

/// A two-finger twist is egui's Rotate event. Over the canvas it turns the selection;
/// letting go is one undo step. Off the canvas it does nothing.
#[test]
fn a_trackpad_twist_rotates_the_shape_under_the_canvas() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let id = harness.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 100.0, 60.0]})).unwrap();
    let id = id.get("id").and_then(|v| v.as_u64()).unwrap();
    harness.state_mut().session.execute("edit.select", &json!({"ids": [id]})).unwrap();
    harness.run_steps(1);
    let canvas = harness.state().canvas_rect.expect("canvas");
    let outside = egui::pos2(1.0, 1.0);
    assert!(!canvas.contains(outside), "the ribbon corner is not the canvas: {canvas:?}");

    harness.input_mut().events.push(egui::Event::PointerMoved(outside));
    harness.input_mut().events.push(egui::Event::Rotate(20.0_f32.to_radians()));
    harness.run_steps(1);
    assert!((shape_rot(harness.state()) - 0.0).abs() < 0.01, "off the canvas, still {}", shape_rot(harness.state()));

    harness.input_mut().events.push(egui::Event::PointerMoved(canvas.center()));
    harness.input_mut().events.push(egui::Event::Rotate(20.0_f32.to_radians()));
    harness.run_steps(1);
    harness.input_mut().events.push(egui::Event::PointerMoved(canvas.center()));
    harness.input_mut().events.push(egui::Event::Rotate(10.0_f32.to_radians()));
    harness.run_steps(1);
    assert!((shape_rot(harness.state()) - 30.0).abs() < 0.05, "twist landed on {}", shape_rot(harness.state()));
    harness.run_steps(1);
    assert!(harness.state().session.active().unwrap().interaction.is_none());
    harness.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert!((shape_rot(harness.state()) - 0.0).abs() < 0.01, "one undo, got {}", shape_rot(harness.state()));
}

/// The browser "PNG images (all slides)" export yields one image per slide, not just the selected one (#64).
#[test]
fn web_export_all_slides_gives_one_file_per_slide() {
    let mut doc = deckcraft_model::Presentation::default();
    if let Some(first) = doc.slides.first().cloned() {
        doc.slides.push(first.clone());
        doc.slides.push(first);
    }
    assert_eq!(doc.slides.len(), 3);
    let all = web_image_files(&doc, 1, true, "Deck", "png", false);
    let names: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Deck1.png", "Deck2.png", "Deck3.png"]);
    let one = web_image_files(&doc, 1, false, "Deck", "png", false);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].0, "Deck.png");
}

/// The web PNG download honours "Transparent background" (#784): the slide background is
/// left out, so the corner is transparent; a JPEG download stays opaque.
#[test]
fn web_png_export_can_leave_the_background_transparent() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let got: Rc<RefCell<Vec<(String, Vec<u8>)>>> = Rc::default();
    let log = got.clone();
    let services = Services {
        download: Some(Box::new(move |name: &str, b: &[u8]| log.borrow_mut().push((name.to_string(), b.to_vec())))),
        ..Default::default()
    };
    let mut app = SlideApp::new(Session::with_new(), services);
    let corner_alpha = |bytes: &[u8]| deckcraft_render::decode_image(bytes).unwrap().get_pixel(2, 2).0[3];
    assert!(app.export_as("png", json!({})));
    assert!(app.export_as("png", json!({"transparentBackground": true})));
    assert!(app.export_as("jpeg", json!({"transparentBackground": true})));
    let got = got.borrow();
    assert_eq!(got.len(), 3);
    assert_eq!(corner_alpha(&got[0].1), 255, "opaque by default");
    assert_eq!(corner_alpha(&got[1].1), 0, "transparent background");
    assert_eq!(corner_alpha(&got[2].1), 255, "JPEG has no transparency");
}

fn title_text(app: &SlideApp) -> String {
    let d = app.session.active().unwrap();
    let sh = &d.doc.slides[0].shapes[0];
    sh.text.as_ref().map(|t| t.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

#[test]
fn quick_access_toolbar_adds_removes_resets_and_persists() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let ids = |app: &SlideApp| app.ui.qat.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(harness.state()), ["app.home", "file.save", "app.exportDialog", "edit.undo", "edit.redo", "show.fromStart"]);
    let app = harness.state_mut();
    app.run("app.qat", json!({"add": "insert.table", "label": "Table", "icon": "table"})).unwrap();
    app.run("app.qat", json!({"add": "insert.table"})).unwrap();
    app.run("app.qat", json!({"add": "view.pane", "params": {"pane": "selection"}, "label": "Selection Pane"})).unwrap();
    assert!(app.run("app.qat", json!({"add": "no.such"})).is_err());
    app.run("app.qat", json!({"remove": "edit.redo"})).unwrap();
    assert_eq!(ids(app), ["app.home", "file.save", "app.exportDialog", "edit.undo", "show.fromStart", "insert.table", "view.pane"]);
    assert_eq!(app.ui.qat[5].label, "Table");
    // Saved with the rest of the UI state and read back.
    let back: UiState = serde_json::from_value(serde_json::to_value(&app.ui).unwrap()).unwrap();
    assert_eq!(back.qat, app.ui.qat);
    harness.run_steps(2);
    let app = harness.state_mut();
    app.run("app.qat", json!({"remove": "view.pane", "params": {"pane": "selection"}})).unwrap();
    assert_eq!(app.ui.qat.len(), 6);
    app.run("app.qat", json!({"reset": true})).unwrap();
    assert_eq!(ids(app), ["app.home", "file.save", "app.exportDialog", "edit.undo", "edit.redo", "show.fromStart"]);
    // Alt+1 runs position 0 (app.home) when on Windows/Linux, opening Backstage.
    app.session.mac_keys = false;
    assert!(!app.backstage_open());
    assert!(crate::menus::ui_shortcut(app, egui::Key::Num1, deckcraft_engine::Mods { shift: false, alt: true, cmd: false }));
    assert!(app.backstage_open());
    // Disabled while Backstage is open.
    assert!(!crate::menus::ui_shortcut(app, egui::Key::Num1, deckcraft_engine::Mods { shift: false, alt: true, cmd: false }));
    app.close_backstage();
    // Alt+9 is unassigned when there are only 6 items.
    assert!(!crate::menus::ui_shortcut(app, egui::Key::Num9, deckcraft_engine::Mods { shift: false, alt: true, cmd: false }));
    // Disabled on macOS so Option+digit types characters.
    app.session.mac_keys = true;
    assert!(!crate::menus::ui_shortcut(app, egui::Key::Num1, deckcraft_engine::Mods { shift: false, alt: true, cmd: false }));
    app.session.mac_keys = false;
    // Disabled when text is being edited.
    let shape_id = deckcraft_model::ShapeId(app.session.active().unwrap().selection.shapes.first().map(|s| s.0).unwrap_or(0));
    app.session.active_mut().unwrap().selection.text = Some(deckcraft_engine::TextSel { shape: shape_id, ..Default::default() });
    assert!(!crate::menus::ui_shortcut(app, egui::Key::Num1, deckcraft_engine::Mods { shift: false, alt: true, cmd: false }));
    app.session.active_mut().unwrap().selection.text = None;
    // Disabled QAT items (e.g. edit.undo when stack is empty) cannot run.
    assert!(!crate::qat::run_index(app, 3));
    // An older settings file without the toolbar gets the defaults.
    let old: UiState = serde_json::from_value(json!({"tab": "Home"})).unwrap();
    assert_eq!(old.qat.len(), 6);
}

#[test]
fn opening_a_file_with_missing_fonts_shows_a_toast_once() {
    use egui_kittest::kittest::Queryable;
    let mut p = deckcraft_model::Presentation::default();
    let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
    let mut body = deckcraft_model::text::TextBody::from_text("Hello");
    body.paragraphs[0].runs[0].props.font = Some("Qqq Nonexistent Sans".into());
    slide.shapes[0].text = Some(body);
    let data = deckcraft_engine::cmd::base64_encode(&deckcraft_engine::cmd::file::save_bytes(&p, "pptx").unwrap());
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    assert!(harness.state().toast.is_none(), "a new presentation has nothing to say");
    harness.state_mut().session.execute("file.openBytes", &json!({"name": "a.pptx", "data": data})).unwrap();
    harness.run_steps(2);
    let text = harness.state().toast.as_ref().map(|t| t.text.clone()).unwrap_or_default();
    assert!(text.starts_with("Qqq Nonexistent Sans isn't installed"), "{text}");
    assert!(harness.query_by_label_contains("Qqq Nonexistent Sans isn't installed").is_some());
    // Closed, it stays closed for that document.
    harness.get_by_label("Close").click();
    harness.run_steps(2);
    assert!(harness.state().toast.is_none());
    harness.run_steps(2);
    assert!(harness.state().toast.is_none());
}

/// #115: Change Picture replaced nothing — it inserted a second picture.
#[test]
fn change_picture_replaces_the_selected_picture() {
    let svg =
        |c: &str| format!("<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'><rect width='4' height='4' fill='{c}'/></svg>").into_bytes();
    let media = |app: &SlideApp| {
        let d = app.session.active().unwrap();
        let pics: Vec<_> = d.doc.slides[0]
            .shapes
            .iter()
            .filter_map(|s| if let deckcraft_model::ShapeKind::Picture { fill } = &s.kind { Some(fill.media) } else { None })
            .collect();
        assert_eq!(pics.len(), 1, "one picture, replaced in place");
        pics[0]
    };
    let insert = |app: &mut SlideApp| {
        let id = app.session.execute("insert.picture", &json!({"name": "a.svg", "data": deckcraft_engine::cmd::base64_encode(&svg("red"))})).unwrap()
            ["id"]
            .clone();
        app.session.execute("edit.select", &json!({"ids": [id]})).unwrap();
        media(app)
    };

    // Desktop: the file dialog returns a path.
    let blue = svg("blue");
    let services = Services {
        pick_open: Some(Box::new(|_: &str| Some("b.svg".into()))),
        read: Some(Box::new(move |_: &str| Ok(blue.clone()))),
        ..Default::default()
    };
    let mut app = SlideApp::new(Session::with_new(), services);
    let before = insert(&mut app);
    app.run("app.changePictureDialog", json!({})).unwrap();
    assert_ne!(media(&app), before);

    // Web: the browser picker delivers the file through the inbox later.
    let inbox = Inbox::default();
    let started = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = started.clone();
    let services =
        Services { open_async: Some(Box::new(move |_: &str, id| record.lock().unwrap().push(id))), inbox: Some(inbox.clone()), ..Default::default() };
    let mut app = SlideApp::new(Session::with_new(), services);
    let before = insert(&mut app);
    app.run("app.changePictureDialog", json!({})).unwrap();
    let id = started.lock().unwrap()[0];
    inbox.lock().unwrap().push(InboxEvent::File(InboxFile { id, name: "b.svg".into(), bytes: svg("blue") }));
    app.drain_inbox();
    assert_ne!(media(&app), before);
}

/// #17: Home created a new blank presentation instead of going to the start screen.
#[test]
fn home_shows_the_start_screen_over_the_open_presentation() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    // The app keeps requesting repaints, so step frames instead of run().
    harness.run_steps(2);
    let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();

    harness.state_mut().run("app.home", json!({})).unwrap();
    harness.run_steps(2);
    assert!(harness.state().backstage_open());
    assert_eq!(harness.state().session.documents().len(), 1, "Home doesn't create a presentation");

    // Typing on the start screen doesn't reach the presentation behind it; Escape goes back.
    harness.event(egui::Event::Text("a".into()));
    harness.key_press(egui::Key::Backspace);
    harness.run_steps(2);
    assert_eq!(title_text(harness.state()), "");
    assert!(!harness.state().session.active().unwrap().is_dirty());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().backstage_open());

    // Home again toggles; picking a theme on the start screen (a new presentation) leaves it.
    harness.state_mut().run("app.home", json!({})).unwrap();
    harness.run_steps(2);
    assert!(harness.state().backstage_open());
    harness.state_mut().run("app.home", json!({})).unwrap();
    assert!(!harness.state().backstage_open());
    harness.state_mut().run("app.home", json!({"on": true})).unwrap();
    harness.state_mut().run("file.new", json!({})).unwrap();
    harness.run_steps(2);
    assert!(!harness.state().backstage_open(), "a new presentation leaves Home");
    assert_eq!(harness.state().session.documents().len(), 2);
}

/// #268: Ctrl+Backspace, Ctrl+Delete, Ctrl+arrows and Ctrl+Shift+arrows in text, through real
/// egui key events (Windows keys).
#[test]
fn ctrl_word_keys_edit_text() {
    use egui::{Key, Modifiers};
    let mut session = Session::with_new();
    session.mac_keys = false;
    session.prefs.autocorrect = false;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, Services::default()),
    );
    harness.run_steps(2);
    let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
    harness.event(egui::Event::Text("Hello, world.  Foo bar".into()));
    harness.run_steps(1);
    assert_eq!(title_text(harness.state()), "Hello, world.  Foo bar");
    let caret = |h: &egui_kittest::Harness<'_, SlideApp>| h.state().session.active().unwrap().selection.text.clone().unwrap();
    let ctrl = Modifiers::COMMAND;
    let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;

    // Ctrl+Left stops at every word and punctuation run, as in PowerPoint.
    let mut stops = vec![];
    for _ in 0..6 {
        harness.key_press_modifiers(ctrl, Key::ArrowLeft);
        harness.run_steps(1);
        stops.push(caret(&harness).caret.1);
    }
    assert_eq!(stops, vec![19, 15, 12, 7, 5, 0]);
    harness.key_press_modifiers(ctrl, Key::ArrowRight);
    harness.run_steps(1);
    assert_eq!(caret(&harness).caret, (0, 5));

    // Ctrl+Shift+Right selects to the next stops.
    harness.key_press_modifiers(ctrl_shift, Key::ArrowRight);
    harness.key_press_modifiers(ctrl_shift, Key::ArrowRight);
    harness.run_steps(1);
    let t = caret(&harness);
    assert_eq!((t.anchor, t.caret), ((0, 5), (0, 12)));

    // Ctrl+Delete deletes the next word from the selection's start; Ctrl+Backspace the one
    // before the caret.
    harness.key_press_modifiers(ctrl, Key::Delete);
    harness.run_steps(1);
    assert_eq!(title_text(harness.state()), "Helloworld.  Foo bar");
    harness.key_press(Key::End);
    harness.key_press_modifiers(ctrl, Key::Backspace);
    harness.run_steps(1);
    assert_eq!(title_text(harness.state()), "Helloworld.  Foo ");

    // Ctrl+Up/Down go by paragraph, Ctrl+Home/End to the text's ends.
    harness.key_press(Key::Enter);
    harness.event(egui::Event::Text("Next".into()));
    harness.key_press_modifiers(ctrl, Key::ArrowUp);
    harness.run_steps(1);
    assert_eq!(caret(&harness).caret, (1, 0));
    harness.key_press_modifiers(ctrl, Key::ArrowUp);
    harness.run_steps(1);
    assert_eq!(caret(&harness).caret, (0, 0));
    harness.key_press_modifiers(ctrl, Key::ArrowDown);
    harness.run_steps(1);
    assert_eq!(caret(&harness).caret, (1, 0));
    harness.key_press_modifiers(ctrl_shift, Key::Home);
    harness.run_steps(1);
    let t = caret(&harness);
    assert_eq!((t.anchor, t.caret), ((1, 0), (0, 0)));

    // Ctrl+Shift+> (egui reports the Period key) grows the selected text's font.
    let size = |h: &mut egui_kittest::Harness<'_, SlideApp>| h.state_mut().session.execute("format.state", &json!({})).unwrap()["size"].as_f64();
    let before = size(&mut harness);
    harness.key_press_modifiers(ctrl_shift, Key::Period);
    harness.run_steps(1);
    let after = size(&mut harness).unwrap();
    assert!(after > before.unwrap_or(0.0), "{before:?} → {after}");
    harness.key_press_modifiers(ctrl, Key::OpenBracket);
    harness.run_steps(1);
    assert!(size(&mut harness).unwrap_or(f64::MAX) < after, "Ctrl+[ shrinks");
}

/// Export… had no visible control: only the command palette reached it.
#[test]
fn title_bar_export_opens_the_export_dialog() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none());
    // Quick access: Home, Save, Export, each 24 points wide with 2 between, from x = 10.
    let pos = egui::pos2(10.0 + 2.0 * 26.0 + 12.0, ribbon::TITLE_H / 2.0);
    harness.hover_at(pos);
    harness.run_steps(1);
    harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    harness.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    harness.run_steps(2);
    assert_eq!(harness.state().dialog.as_ref().map(|d| d.id.as_str()), Some("export"));
}

/// #53: Ctrl+V did nothing for an image-only clipboard: egui-winit sends no Paste event for it and
/// swallows the key press, so only the V release reaches the app.
#[test]
fn ctrl_v_pastes_an_image_when_no_paste_event_arrives() {
    let session = Session::with_new();
    let png = {
        let d = session.active().unwrap();
        deckcraft_engine::cmd::file::render_png(&d.doc, 0, 0.1, false).0
    };
    assert!(!png.is_empty());
    let services = Services { clipboard_image: Some(Box::new(move || Some(png.clone()))), ..Default::default() };
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, services),
    );
    harness.run_steps(2);
    let pictures = |app: &SlideApp| {
        let d = app.session.active().unwrap();
        d.doc.slides[0].shapes.iter().filter(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })).count()
    };
    let v = |pressed| egui::Event::Key { key: egui::Key::V, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::COMMAND };
    // A text paste (Paste event on the press) does not also paste the image on release.
    harness.event(egui::Event::Paste("hello".into()));
    harness.run_steps(2);
    harness.event(v(false));
    harness.run_steps(2);
    assert_eq!(pictures(harness.state()), 0);
    // A V release without Command/Ctrl is not a paste.
    harness.event(egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE });
    harness.run_steps(2);
    assert_eq!(pictures(harness.state()), 0);
    // What egui-winit sends for Ctrl+V with only an image on the clipboard: the release alone.
    harness.event(v(false));
    harness.run_steps(2);
    assert_eq!(pictures(harness.state()), 1);
}

/// #78: after Tab in a text box, typing and deleting no longer reached the text.
#[test]
fn typing_goes_on_after_tab_in_a_text_box() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        {
            let mut session = Session::with_new();
            session.prefs.autocorrect = false;
            SlideApp::new(session, Services::default())
        },
    );
    harness.run_steps(2);
    let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
    harness.event(egui::Event::Text("ab".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    harness.event(egui::Event::Text("c".into()));
    harness.run_steps(2);
    assert_eq!(title_text(harness.state()), "ab\tc");
    harness.key_press(egui::Key::Backspace);
    harness.run_steps(2);
    assert_eq!(title_text(harness.state()), "ab\t");
    assert!(harness.state().session.active().unwrap().selection.text.is_some(), "still editing");
    // Tab at the start of a paragraph indents it, and typing goes on there too (#93).
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    harness.event(egui::Event::Text("d".into()));
    harness.run_steps(2);
    assert_eq!(title_text(harness.state()), "ab\t\nd");
    let level =
        |h: &egui_kittest::Harness<SlideApp>| h.state().session.active().unwrap().doc.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[1].level;
    assert_eq!(level(&harness), 1);
}

/// #93: Tab in an edited list indented the item but handed the keyboard to egui's focus
/// cycle: the stray focus (e.g. on the window's drag handle) swallowed every later key, so the
/// list could neither be typed into nor continued with Enter.
#[test]
fn tab_in_an_edited_list_keeps_the_keyboard() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
    harness.event(egui::Event::Text("one".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    harness.key_press(egui::Key::Tab); // Increase List Level
    harness.run_steps(2);
    harness.event(egui::Event::Text("sub".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter); // continues the list
    harness.run_steps(2);
    harness.event(egui::Event::Text("two".into()));
    harness.run_steps(2);
    assert_eq!(title_text(harness.state()), "one\nsub\ntwo", "typing after Tab must reach the text");
    let body = harness.state().session.active().unwrap().doc.slides[0].shapes[0].text.clone().unwrap();
    let levels: Vec<u8> = body.paragraphs.iter().map(|p| p.level).collect();
    assert_eq!(levels, vec![0, 1, 1], "Tab indents one level; Enter keeps the level");
}

#[test]
fn home_needs_an_open_presentation() {
    let mut app = SlideApp::new(Session::new(), Services::default());
    app.run("app.home", json!({})).unwrap();
    assert!(!app.backstage_open(), "with nothing open the start screen shows anyway");
}

/// View › New Window is a second window on the open presentation. Its slide is its own;
/// the editor's selection stays where it was. Closing the window leaves the document open.
#[test]
fn new_window_shows_the_same_document_on_its_own_slide() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.session.execute("slide.new", &json!({})).unwrap();
    app.session.execute("slide.go", &json!({"index": 1})).unwrap();
    let err = {
        let mut empty = SlideApp::new(Session::new(), Services::default());
        empty.run("view.newWindow", json!({})).unwrap_err()
    };
    assert!(err.contains("presentation"), "{err}");

    let name = app.session.active().unwrap().title();
    let opened = app.run("view.newWindow", json!({})).unwrap();
    assert_eq!(opened["open"], true);
    assert_eq!(opened["window"], 2);
    assert_eq!(opened["slide"], 1);
    assert_eq!(opened["title"], format!("{name}:2"));
    assert_eq!(app.session.active().unwrap().selection.slide, 1);

    let moved = app.run("view.newWindow", json!({"delta": -1})).unwrap();
    assert_eq!(moved["slide"], 0);
    assert_eq!(app.doc_window.as_ref().unwrap().slide, 0);
    assert_eq!(app.session.active().unwrap().selection.slide, 1, "the editor stays on its slide");

    let again = app.run("view.newWindow", json!({})).unwrap();
    assert_eq!(again["window"], 2);
    assert_eq!(again["slide"], 0, "opening again keeps the window's slide");

    app.session.execute("file.close", &json!({})).unwrap();
    doc_window::show(&mut app, &egui::Context::default());
    assert!(app.doc_window.is_none(), "closing the presentation closes the extra window");
    assert!(app.session.active().is_none());
}

#[test]
fn grid_options_sets_spacing_and_display_or_opens_the_dialog() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    assert!((app.session.prefs.grid_spacing - 6.0).abs() < 1e-9);
    let v = app.run("view.gridOptions", json!({"spacing": 72.0, "snap": false, "smartGuides": false, "gridlines": true, "guides": true})).unwrap();
    assert_eq!(v["spacing"], 72.0);
    assert_eq!(v["snap"], false);
    assert_eq!(v["smartGuides"], false);
    assert_eq!(v["gridlines"], true);
    assert_eq!(v["guides"], true);
    assert!(app.dialog.is_none());
    assert!(app.ui.gridlines && app.ui.guides);
    assert!(!app.session.prefs.snap_to_grid && !app.ui.smart_guides);

    app.run("view.gridOptions", json!({"snap": true})).unwrap();
    assert!(app.session.prefs.snap_to_grid);
    assert!((app.session.prefs.grid_spacing - 72.0).abs() < 1e-9);
    assert!(app.dialog.is_none());

    for bad in [json!({"spacing": 0}), json!({"spacing": 289}), json!({"spacing": "72"}), json!({"spacing": null}), json!({"snap": 1})] {
        assert!(app.run("view.gridOptions", bad).is_err(), "rejected a bad grid option");
    }
    assert!((app.session.prefs.grid_spacing - 72.0).abs() < 1e-9);
    assert!(app.session.prefs.snap_to_grid);

    app.run("view.gridOptions", json!({})).unwrap();
    assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some("gridOptions"));
    assert!((app.session.prefs.grid_spacing - 72.0).abs() < 1e-9);
}

#[test]
fn grid_options_dialog_lists_spacing_and_display() {
    use egui_kittest::kittest::Queryable;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    harness.state_mut().run("view.gridOptions", json!({})).unwrap();
    harness.run_steps(2);
    assert!(harness.query_by_label_contains("Display grid on screen").is_some());
    assert!(harness.query_by_label_contains("Snap objects to grid").is_some());
    assert!(harness.query_by_label_contains("Spacing (pt)").is_some());
    let show_grid = harness
        .query_all_by_role_and_label(egui::accesskit::Role::CheckBox, "Display grid on screen")
        .next()
        .unwrap_or_else(|| panic!("no grid checkbox"));
    show_grid.click_accesskit();
    harness.run_steps(2);
    harness.query_all_by_role_and_label(egui::accesskit::Role::Button, "OK").next().unwrap_or_else(|| panic!("no OK button")).click_accesskit();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "OK closes Grid Options");
    assert!(harness.state().ui.gridlines, "OK applies the grid checkbox");
    assert!((harness.state().session.prefs.grid_spacing - 6.0).abs() < 1e-9);

    harness.state_mut().run("view.gridOptions", json!({})).unwrap();
    harness.run_steps(2);
    harness
        .query_all_by_role_and_label(egui::accesskit::Role::Button, "Cancel")
        .next()
        .unwrap_or_else(|| panic!("no Cancel button"))
        .click_accesskit();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "Cancel closes Grid Options");
    assert!(harness.state().ui.gridlines, "Cancel leaves the grid on");
    assert!((harness.state().session.prefs.grid_spacing - 6.0).abs() < 1e-9);
}

/// Preferences (#752): Cancel, Esc and the window's close button put back what the dialog
/// changed while it was open; Done keeps it.
#[test]
fn preferences_cancel_restores_the_settings_done_keeps_them() {
    use egui_kittest::kittest::Queryable;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let before = harness.state().session.prefs.clone();
    let dark_before = harness.state().ui.brightness;
    let change = |harness: &mut egui_kittest::Harness<'static, SlideApp>| {
        harness.state_mut().run("app.preferences", json!({})).unwrap();
        harness.run_steps(2);
        for label in ["Dark appearance", "Show smart guides", "Snap objects to grid"] {
            harness
                .query_all_by_role_and_label(egui::accesskit::Role::CheckBox, label)
                .next()
                .unwrap_or_else(|| panic!("no {label} checkbox"))
                .click_accesskit();
            harness.run_steps(2);
        }
        assert_ne!(harness.state().ui.brightness, dark_before, "the dialog previews the appearance");
        assert_ne!(harness.state().session.prefs.smart_guides, before.smart_guides);
    };
    let unchanged = |harness: &egui_kittest::Harness<'static, SlideApp>, how: &str| {
        let s = harness.state();
        assert!(s.dialog.is_none(), "{how} closes Preferences");
        assert_eq!(s.ui.brightness, dark_before, "{how} restores the appearance");
        assert_eq!(s.session.prefs.smart_guides, before.smart_guides, "{how} restores smart guides");
        assert_eq!(s.session.prefs.snap_to_grid, before.snap_to_grid, "{how} restores snap to grid");
    };

    change(&mut harness);
    harness
        .query_all_by_role_and_label(egui::accesskit::Role::Button, "Cancel")
        .next()
        .unwrap_or_else(|| panic!("no Cancel button"))
        .click_accesskit();
    harness.run_steps(2);
    unchanged(&harness, "Cancel");

    change(&mut harness);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    unchanged(&harness, "Esc");

    change(&mut harness);
    harness.query_all_by_role_and_label(egui::accesskit::Role::Button, "Done").next().unwrap_or_else(|| panic!("no Done button")).click_accesskit();
    harness.run_steps(2);
    let s = harness.state();
    assert!(s.dialog.is_none(), "Done closes Preferences");
    assert_ne!(s.ui.brightness, dark_before, "Done keeps the appearance");
    assert_ne!(s.session.prefs.smart_guides, before.smart_guides, "Done keeps smart guides");
    assert_ne!(s.session.prefs.snap_to_grid, before.snap_to_grid, "Done keeps snap to grid");
}

/// About dialog: Close and Cancel each close the dialog.
#[test]
fn about_dialog_close_and_cancel_close_it() {
    use egui_kittest::kittest::Queryable;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    harness.state_mut().run("app.about", json!({})).unwrap();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_some());
    harness
        .query_all_by_role_and_label(egui::accesskit::Role::Button, "Cancel")
        .next()
        .unwrap_or_else(|| panic!("no Cancel button in About dialog"))
        .click_accesskit();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "Cancel closes About dialog");

    harness.state_mut().run("app.about", json!({})).unwrap();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_some());
    harness
        .query_all_by_role_and_label(egui::accesskit::Role::Button, "Close")
        .next()
        .unwrap_or_else(|| panic!("no Close button in About dialog"))
        .click_accesskit();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "Close closes About dialog");
}

fn canvas_harness() -> egui_kittest::Harness<'static, SlideApp> {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let app = harness.state_mut();
    // Zoomed out, the canvas shows plenty of pasteboard around the slide.
    app.ui.zoom = Some(0.4);
    app.session.prefs.snap_to_grid = false;
    app.session.prefs.smart_guides = false;
    harness
}

fn selected(app: &SlideApp) -> Vec<u64> {
    app.session.active().unwrap().selection.shapes.iter().map(|i| i.0 as u64).collect()
}

fn shape_x(app: &SlideApp, id: u64) -> f64 {
    let st = app.session.active().unwrap();
    st.shape(deckcraft_model::ShapeId(id as u32)).and_then(|s| s.xfrm).unwrap().x
}

/// #270: PowerPoint's Normal view draws objects off the slide on the area around it, where
/// they can be selected and moved.
#[test]
fn off_slide_objects_show_on_the_pasteboard_and_can_be_selected_and_moved() {
    let _gpu = crate::gpu_lock::gpu_lock();
    let mut harness = canvas_harness();
    let off = harness.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [1000.0, 100.0, 120.0, 80.0]})).unwrap()["id"]
        .as_u64()
        .unwrap();
    harness.state_mut().session.execute("edit.select", &json!({"ids": []})).unwrap();
    harness.run_steps(3);

    let app = harness.state();
    let (slide, region, k) = (app.slide_rect.unwrap(), app.canvas_region.unwrap(), app.canvas_scale);
    // The texture reaches past the slide's right edge to the shape's, and no further left/up.
    assert!(region.max.x >= slide.min.x + 1120.0 * k - 1.0, "{region:?} {slide:?}");
    assert!((region.min.x - slide.min.x).abs() < 1.0 && (region.min.y - slide.min.y).abs() < 1.0, "{region:?} {slide:?}");
    let centre = slide.min + egui::vec2(1060.0 * k, 140.0 * k);
    assert!(app.canvas_rect.unwrap().contains(centre));

    // Drawn at full strength on the pasteboard (when a GPU renderer is available here).
    if let Ok(img) = harness.render() {
        let px = |p: egui::Pos2| img.get_pixel(p.x as u32, p.y as u32).0;
        let shape = px(centre);
        let board = px(slide.min + egui::vec2(1060.0 * k, 400.0 * k));
        let doc = &harness.state().session.active().unwrap().doc;
        let accent = doc.masters[0].theme.colors.get(deckcraft_color::SchemeSlot::Accent1);
        let close = |a: u8, b: u8| (a as i32 - b as i32).abs() <= 4;
        assert!(close(shape[0], accent.r) && close(shape[1], accent.g) && close(shape[2], accent.b), "{shape:?} vs {accent:?}");
        assert_ne!(shape, board);
    }

    // Click selects it.
    harness.hover_at(centre);
    harness.run_steps(1);
    harness.drag_at(centre);
    harness.run_steps(1);
    harness.drop_at(centre);
    harness.run_steps(2);
    assert_eq!(selected(harness.state()), vec![off]);

    // Drag moves it, further off the slide.
    let to = centre + egui::vec2(40.0, 0.0);
    harness.hover_at(centre);
    harness.run_steps(1);
    harness.drag_at(centre);
    harness.run_steps(1);
    for i in 1..=4 {
        harness.hover_at(centre + egui::vec2(10.0 * i as f32, 0.0));
        harness.run_steps(1);
    }
    harness.drop_at(to);
    harness.run_steps(2);
    let x = shape_x(harness.state(), off);
    assert!((x - (1000.0 + 40.0 / k as f64)).abs() < 1.5, "{x}");
    // The pasteboard followed it.
    let (slide, region) = (harness.state().slide_rect.unwrap(), harness.state().canvas_region.unwrap());
    assert!(region.max.x >= slide.min.x + (x as f32 + 120.0) * k - 1.0, "{region:?}");

    // Idle frames don't render the canvas again.
    let n = harness.state().textures.canvas_renders;
    harness.run_steps(3);
    assert_eq!(harness.state().textures.canvas_renders, n);
}

/// A slide with nothing off it renders just the slide, as before.
#[test]
fn the_canvas_renders_only_the_slide_when_nothing_is_off_it() {
    let mut harness = canvas_harness();
    harness.run_steps(3);
    let app = harness.state();
    let (slide, region) = (app.slide_rect.unwrap(), app.canvas_region.unwrap());
    assert!((region.min - slide.min).length() < 0.5 && (region.max - slide.max).length() < 1.0, "{region:?} {slide:?}");
    let n = app.textures.canvas_renders;
    harness.run_steps(3);
    assert_eq!(harness.state().textures.canvas_renders, n, "an unchanged slide isn't rendered again");
}

/// #431: the ribbon's Copy and Cut, the slide's context menu, the menus and the control
/// channel all run the commands; they put the text on the system clipboard, as Ctrl+C and
/// Ctrl+X do.
#[test]
fn copy_and_cut_commands_reach_the_system_clipboard() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let copied = |h: &egui_kittest::Harness<'_, SlideApp>| {
        h.output().platform_output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
    };
    let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
    harness.state_mut().session.type_text("Aurora").unwrap();
    harness.state_mut().session.key("Escape", Mods::default()).unwrap();
    harness.run_steps(1);
    assert_eq!(copied(&harness), None);

    harness.state_mut().run("edit.copy", json!({})).unwrap();
    harness.run_steps(1);
    assert_eq!(copied(&harness).as_deref(), Some("Aurora"), "Copy puts the text on the clipboard");
    harness.run_steps(1);
    assert_eq!(copied(&harness), None, "only once");

    harness.state_mut().run("edit.cut", json!({})).unwrap();
    harness.run_steps(1);
    assert_eq!(copied(&harness).as_deref(), Some("Aurora"), "Cut does too");
    assert!(harness.state().session.active().unwrap().doc.slides[0].shapes.iter().all(|s| s.id.0 != title), "and removes the shape");
}

/// #182: Ctrl+C then Ctrl+V did nothing for objects without text (a rectangle), because
/// egui-winit drops Ctrl+V when the system clipboard is empty; the context menu worked.
#[test]
fn copy_then_paste_shortcut_duplicates_an_object_without_text() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    harness.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap();
    let count = |h: &egui_kittest::Harness<'_, SlideApp>| h.state().session.doc().unwrap().current_slide().unwrap().shapes.len();
    let before = count(&harness);

    // What the copy leaves on the system clipboard is what egui-winit hands back on Ctrl+V.
    assert_eq!(system_clipboard_text(""), OBJECTS_MARKER);
    assert_eq!(system_clipboard_text("hello"), "hello");
    harness.event(egui::Event::Copy);
    harness.run_steps(2);
    harness.event(egui::Event::Paste(OBJECTS_MARKER.to_string()));
    harness.run_steps(2);
    assert_eq!(count(&harness), before + 1, "the shortcut pastes a copy of the rectangle");
    let d = harness.state().session.active().unwrap();
    let pasted = d.doc.slides[0].shapes.last().unwrap();
    assert!(pasted.text.as_ref().is_none_or(|t| t.text().is_empty()), "a copy of the rectangle, not a text box holding the marker");
}

/// #188: Ctrl/Cmd+A with the slide thumbnail pane focused selects every slide; with the
/// canvas focused it still selects shapes.
#[test]
fn select_all_follows_the_focused_pane() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    for _ in 0..2 {
        harness.state_mut().run("slide.new", json!({})).unwrap();
    }
    harness.state_mut().run("slide.go", json!({"index": 0})).unwrap();
    harness.run_steps(3);
    let click = |h: &mut egui_kittest::Harness<'_, SlideApp>, pos: egui::Pos2| {
        h.event(egui::Event::PointerMoved(pos));
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.run_steps(2);
    };
    // Press in the thumbnail pane, then Cmd+A.
    let w = harness.state().ui.thumbs_width;
    click(&mut harness, egui::pos2(w / 2.0, 300.0));
    assert!(harness.state().slides_focused);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(2);
    let st = harness.state().session.active().unwrap();
    assert_eq!(st.selection.slides.len(), 3, "every slide is selected");

    // A press on the canvas moves focus away: Cmd+A selects shapes again.
    click(&mut harness, egui::pos2(700.0, 400.0));
    assert!(!harness.state().slides_focused);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(2);
    let st = harness.state().session.active().unwrap();
    assert!(!st.selection.shapes.is_empty(), "canvas select all picks the shapes");
}

fn harness() -> egui_kittest::Harness<'static, SlideApp> {
    egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::new(), Services::default()),
    )
}

/// #172: a recent file that no longer exists is marked on File › Home, and clicking it draws
/// the open error right there (the status bar is hidden while the backstage is open, so the
/// message used to vanish).
#[test]
fn missing_recent_is_marked_and_its_error_shows() {
    let mut harness = harness();
    let missing = "/deckcraft-tests/172/missing.deckcraft";
    harness.state_mut().ui.recent = vec![missing.to_string()];
    harness.run_steps(2);
    // The row is labelled with the file's name. Clicking it raises the open error, and the
    // drawn message is a second node that mentions the name (the OS text after it varies).
    harness.get_by_label("missing.deckcraft").click();
    harness.run_steps(2);
    // The file is read in the background (#350): wait for that read to finish.
    for _ in 0..500 {
        if harness.state().opening.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.run_steps(1);
    }
    harness.run_steps(2);
    assert!(harness.state().toast.is_some(), "the open error was raised");
    assert!(harness.get_all_by_label_contains("missing.deckcraft").count() >= 2, "the open error is drawn on the page, next to the entry");
}

/// #172: a Recent entry can be removed from the list.
#[test]
fn recent_entry_can_be_removed() {
    let mut harness = harness();
    harness.state_mut().ui.recent = vec!["/deckcraft-tests/172/a.deckcraft".into(), "/deckcraft-tests/172/b.deckcraft".into()];
    harness.run_steps(2);
    let removes = harness.get_all_by_label("Remove from Recent").collect::<Vec<_>>();
    assert_eq!(removes.len(), 2);
    // The remove button must sit inside the visible part of the row even when the page's
    // content is wider than the window.
    assert!(removes[1].rect().right() < 1200.0, "reachable in the 1200-wide window: {:?}", removes[1].rect());
    removes[1].click();
    harness.run_steps(1);
    assert_eq!(harness.state().ui.recent, vec!["/deckcraft-tests/172/a.deckcraft"]);
}

/// The existence probe tells present files from missing ones and never touches remote paths.
#[test]
fn recent_exists_tells_missing_from_present() {
    let mut app = SlideApp::new(Session::new(), Services::default());
    let mut path = std::env::temp_dir();
    path.push(format!("deckcraft-test-172-{}.deckcraft", std::process::id()));
    std::fs::write(&path, b"").unwrap();
    assert!(app.recent_exists(&path.to_string_lossy()), "the file exists");
    assert!(!app.recent_exists("/deckcraft-tests/172/missing.deckcraft"));
    assert!(app.recent_exists("\\\\server/share/deck.deckcraft"), "UNC paths are never probed");
    std::fs::remove_file(&path).ok();
}

/// #339: clicking a language in Review > Language did nothing, and OK didn't mark the text.
#[test]
fn the_language_dialog_marks_the_selected_text() {
    use egui_kittest::kittest::Queryable;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let title = harness.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    harness.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
    harness.state_mut().session.execute("text.insert", &json!({"text": "Bonjour"})).unwrap();
    harness.state_mut().session.execute("text.selectAll", &json!({})).unwrap();
    harness.state_mut().run("app.dialog", json!({"id": "language"})).unwrap();
    // The anchored window settles over its first frames; click once it stays put.
    harness.run_steps(4);
    harness.get_by_label("Français (France)").click();
    harness.run_steps(2);
    harness.get_by_label("OK").click();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none());
    let d = harness.state().session.active().unwrap();
    let body = d.doc.slides[0].shapes[0].text.as_ref().unwrap();
    assert_eq!(body.paragraphs[0].runs[0].props.lang.as_deref(), Some("fr-FR"));
}

/// Review > Language > Whole presentation marks every text box, not only the selected one.
#[test]
fn the_language_dialog_marks_the_whole_presentation() {
    use egui_kittest::kittest::Queryable;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let s = &mut harness.state_mut().session;
    let title = s.active().unwrap().doc.slides[0].shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Hola"})).unwrap();
    s.execute("slide.new", &json!({"title": "Adiós"})).unwrap();
    s.execute("text.edit", &json!({"id": s.active().unwrap().doc.slides[1].shapes[0].id.0})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
    harness.state_mut().run("app.dialog", json!({"id": "language"})).unwrap();
    harness.run_steps(4);
    harness.get_by_label("Whole presentation").click();
    harness.run_steps(2);
    harness.get_by_label("Español (España)").click();
    harness.run_steps(2);
    harness.get_by_label("OK").click();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none());
    let d = harness.state().session.active().unwrap();
    for sl in &d.doc.slides {
        let body = sl.shapes[0].text.as_ref().unwrap();
        assert_eq!(body.paragraphs[0].runs[0].props.lang.as_deref(), Some("es-ES"));
    }
}

/// With nothing selected, Review > Language applies to the whole presentation without
/// choosing it, and Selected text can't be picked.
#[test]
fn the_language_dialog_marks_the_whole_presentation_with_nothing_selected() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let s = &mut harness.state_mut().session;
    let title = s.active().unwrap().doc.slides[0].shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Hola"})).unwrap();
    s.execute("slide.new", &json!({"title": "Adiós"})).unwrap();
    s.execute("edit.select", &json!({"ids": []})).unwrap();
    assert!(s.active().unwrap().selection.text.is_none() && s.active().unwrap().selection.shapes.is_empty());
    harness.state_mut().run("app.dialog", json!({"id": "language"})).unwrap();
    harness.run_steps(4);
    assert!(harness.get_by_label("Selected text").accesskit_node().is_disabled());
    harness.get_by_label("Español (España)").click();
    harness.run_steps(2);
    harness.get_by_label("OK").click();
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none());
    let d = harness.state().session.active().unwrap();
    for sl in &d.doc.slides {
        let body = sl.shapes[0].text.as_ref().unwrap();
        assert_eq!(body.paragraphs[0].runs[0].props.lang.as_deref(), Some("es-ES"));
    }
}

/// Speaker notes keep their formatting when edited in the notes pane: typing into a bold
/// heading, Enter in a bulleted point, then saving as .pptx and reopening.
#[test]
fn editing_notes_in_the_pane_keeps_their_formatting() {
    use deckcraft_model::text::{Bullet, ParaProps, Paragraph, Run, RunProps, TextBody};
    let point = |runs: Vec<Run>| Paragraph {
        level: 1,
        props: ParaProps {
            margin_left: Some(22.5),
            indent: Some(-22.5),
            bullet: Some(Bullet::Char { char: "§".into() }),
            bullet_font: Some("Wingdings".into()),
            ..Default::default()
        },
        runs,
        ..Default::default()
    };
    let bold = RunProps { bold: Some(true), ..Default::default() };
    let notes = TextBody {
        paragraphs: vec![
            Paragraph { runs: vec![Run::with("Metodický prehľad", bold.clone())], ..Default::default() },
            Paragraph::new("Žiaci pracujú vo dvojiciach."),
            point(vec![Run::with("Cieľ: ", bold.clone()), Run::new("vysvetliť cyklus")]),
            point(vec![Run::new("Reflexia")]),
            Paragraph::new("Pomôcky: počítač"),
        ],
        ..Default::default()
    };
    let mut session = Session::with_new();
    session.execute("slide.notes", &json!({"index": 0, "body": notes})).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, Services::default()),
    );
    harness.run_steps(2);
    let id = egui::Id::unique("notes_edit");
    let caret = |h: &mut egui_kittest::Harness<'_, SlideApp>, at: usize| {
        let mut st = egui::TextEdit::load_state(&h.ctx, id).unwrap_or_default();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
        egui::TextEdit::store_state(&h.ctx, id, st);
    };
    let chars = |s: &str, of: &str| s.find(of).map(|b| s[..b].chars().count() + of.chars().count()).unwrap();
    let text = notes.text();
    harness.ctx.memory_mut(|m| m.request_focus(id));
    harness.run_steps(1);
    // Type into the bold heading.
    caret(&mut harness, chars(&text, "prehľad"));
    harness.run_steps(1);
    harness.event(egui::Event::Text(" hodiny".into()));
    harness.run_steps(1);
    // Enter after a bulleted point, and type the next one.
    let text = text.replacen("prehľad", "prehľad hodiny", 1);
    caret(&mut harness, chars(&text, "cyklus"));
    harness.run_steps(1);
    harness.key_press(egui::Key::Enter);
    harness.event(egui::Event::Text("Nový bod".into()));
    harness.run_steps(1);
    // Leave the field: the notes are saved.
    harness.ctx.memory_mut(|m| m.surrender_focus(id));
    harness.run_steps(2);
    let check = |n: &TextBody| {
        assert_eq!(n.text(), "Metodický prehľad hodiny\nŽiaci pracujú vo dvojiciach.\nCieľ: vysvetliť cyklus\nNový bod\nReflexia\nPomôcky: počítač");
        assert_eq!(n.paragraphs[0].runs.len(), 1, "{n:?}");
        assert_eq!(n.paragraphs[0].runs[0].props.bold, Some(true));
        assert_eq!(n.paragraphs[1].props, ParaProps::default());
        assert_eq!(n.paragraphs[2].runs[0].props.bold, Some(true), "the bold label");
        assert_eq!(n.paragraphs[2].runs[1].props.bold, None);
        for i in [2, 3, 4] {
            assert_eq!((n.paragraphs[i].level, &n.paragraphs[i].props), (1, &notes.paragraphs[2].props), "point {i}");
        }
        assert_eq!((n.paragraphs[5].level, &n.paragraphs[5].props), (0, &ParaProps::default()));
    };
    check(&harness.state().session.active().unwrap().doc.slides[0].notes);
    // Saved as .pptx and reopened.
    let s = &mut harness.state_mut().session;
    let data = s.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].clone();
    s.execute("file.openBytes", &json!({"name": "notes.pptx", "data": data})).unwrap();
    check(&s.active().unwrap().doc.slides[0].notes);
}

fn step_app(services: Services) -> egui_kittest::Harness<'static, SlideApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), services),
    );
    h.run_steps(2);
    h
}

/// Copy of a bold selection offers HTML; paste of Pages-like HTML keeps the bold.
#[test]
fn copy_offers_html_and_paste_keeps_bold() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let seen = Rc::new(RefCell::new(None));
    let record = seen.clone();
    let services = Services {
        clipboard_set_html: Some(Box::new(move |html, plain| {
            *record.borrow_mut() = Some((html.to_string(), plain.to_string()));
            true
        })),
        ..Default::default()
    };
    let mut h = step_app(services);
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
    h.event(egui::Event::Text("Hi".into()));
    h.run_steps(1);
    h.state_mut().session.execute("text.selectAll", &json!({})).unwrap();
    h.state_mut().session.execute("format.bold", &json!({"on": true})).unwrap();
    h.event(egui::Event::Copy);
    h.run_steps(1);
    let (html, plain) = seen.borrow().clone().expect("html offered");
    assert!(html.contains("<b>"), "{html}");
    assert_eq!(plain, "Hi");
    assert!(!h.output().platform_output.commands.iter().any(|c| matches!(c, egui::output::OutputCommand::CopyText(_))));

    let services = Services { clipboard_html: Some(Box::new(|| Some("<p><b>Hi</b></p>".into()))), ..Default::default() };
    let mut h = step_app(services);
    h.event(egui::Event::Paste("Hi".into()));
    h.run_steps(1);
    let st = h.state().session.active().unwrap();
    let body = st
        .current_slide()
        .unwrap()
        .shapes
        .iter()
        .find(|sh| sh.text.as_ref().is_some_and(|t| t.text() == "Hi"))
        .and_then(|sh| sh.text.clone())
        .expect("pasted text");
    assert_eq!(body.paragraphs[0].runs[0].props.bold, Some(true), "{body:?}");
}

/// A shape with no text copies out as the objects marker (not a picture), so Ctrl+V can
/// still paste it here (#182).
#[test]
fn a_textless_shape_copies_as_the_objects_marker() {
    let mut h = step_app(Services::default());
    h.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 120.0, 80.0]})).unwrap();
    h.event(egui::Event::Copy);
    h.run_steps(1);
    let cmds = &h.output().platform_output.commands;
    assert!(cmds.iter().any(|c| matches!(c, egui::output::OutputCommand::CopyText(t) if t == OBJECTS_MARKER)), "{cmds:?}");
}

fn redder(px: [u8; 4]) -> i32 {
    px[0] as i32 - px[1].max(px[2]) as i32
}

/// A fade entrance is hidden at the start of the preview clock and opaque once it has played.
#[test]
fn preview_hides_an_entrance_until_it_plays() {
    let mut session = Session::with_new();
    let text = session.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 220.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
    session.execute("text.set", &json!({"id": text, "text": "Alpha\nBeta"})).unwrap();
    session.execute("animation.set", &json!({"effect": "fade", "duration": 1000})).unwrap();
    session.execute("animation.options", &json!({"textBuild": "byParagraph"})).unwrap();
    let doc = session.active().unwrap().doc.clone();
    let tl = deckcraft_anim::ShowState::timeline(&doc, 0);
    let shape = deckcraft_model::ShapeId(text as u32);
    let para = |time: f64, i: usize| preview::shape_state_at(&tl, shape, time).paras.into_iter().find(|(p, _)| *p == i).map(|(_, s)| s).unwrap();
    assert!(!para(0.0, 0).visible && !para(0.0, 1).visible, "entrances have not started");
    let mid = para(0.25, 0);
    assert!(mid.visible && mid.opacity > 0.05 && mid.opacity < 0.95, "{mid:?}");
    assert!(!para(0.25, 1).visible, "the next paragraph waits for its step");

    session.execute("animation.clear", &json!({})).unwrap();
    let id = session.execute("shape.insert", &json!({"preset": "rect", "rect": [400.0, 300.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
    session.execute("shape.fill", &json!({"id": id, "color": "#CC2200"})).unwrap();
    session.execute("animation.set", &json!({"effect": "fade", "duration": 1000})).unwrap();
    let doc = session.active().unwrap().doc.clone();
    let tl = deckcraft_anim::ShowState::timeline(&doc, 0);
    let (w, h) = (doc.slide_size.width.round().max(1.0) as u32, doc.slide_size.height.round().max(1.0) as u32);
    let paint = |time: Option<f64>| {
        let f = |sid: deckcraft_model::ShapeId| preview::shape_state_at(&tl, sid, time.unwrap_or(0.0));
        let state: Option<&dyn Fn(deckcraft_model::ShapeId) -> deckcraft_render::ShapeState> = if time.is_some() { Some(&f) } else { None };
        let opts = deckcraft_render::RenderOpts { scale: 1.0, size: Some((w, h)), state, ..Default::default() };
        deckcraft_render::render_slide(&doc, 0, &opts).pixel(460, 340)
    };
    let rest = paint(None);
    let start = paint(Some(0.0));
    let end = paint(Some(tl.total_duration() + 0.05));
    assert!(redder(rest) > 40, "the shape is filled at rest {rest:?}");
    assert!(redder(start) < redder(rest) / 2, "hidden at the first preview frame {start:?} vs {rest:?}");
    assert!(redder(end) > 40, "visible again once the sequence has played {end:?}");
}

#[test]
fn animation_preview_plays_on_the_canvas_then_stops() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(0.05).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let none = harness.state_mut().run("animation.preview", json!({})).unwrap();
    assert_eq!(none["playing"], false);
    assert!(harness.state().status.as_ref().is_some_and(|s| s.0.contains("no animations")));
    assert!(harness.state().show.is_none());

    let id = harness.state_mut().session.execute("shape.insert", &json!({"preset": "rect", "rect": [200.0, 280.0, 180.0, 80.0]})).unwrap()["id"]
        .as_u64()
        .unwrap();
    harness.state_mut().session.execute("animation.set", &json!({"effect": "fade", "duration": 400})).unwrap();
    let started = harness.state_mut().run("animation.preview", json!({})).unwrap();
    assert_eq!(started["playing"], true);
    assert!(harness.state().show.is_none(), "preview stays on the editor canvas");
    harness.run_steps(1);
    let elapsed = harness.state().anim_preview.as_ref().map(|p| p.elapsed).unwrap();
    assert!(elapsed < 0.02, "the clock latches on the first frame, got {elapsed}");
    let renders = harness.state().textures.canvas_renders;
    harness.run_steps(6);
    let elapsed = harness.state().anim_preview.as_ref().map(|p| p.elapsed).unwrap();
    assert!((0.2..0.5).contains(&elapsed), "{elapsed}");
    assert!(harness.state().textures.canvas_renders > renders, "each preview frame redraws the slide");
    assert!(harness.state().show.is_none());

    harness.key_press(egui::Key::Escape);
    harness.run_steps(1);
    assert!(harness.state().anim_preview.is_none(), "Escape stops the preview");

    harness.state_mut().run("animation.preview", json!({})).unwrap();
    harness.run_steps(1);
    assert!(harness.state().anim_preview.is_some());
    harness.state_mut().run("animation.preview", json!({"on": false})).unwrap();
    assert!(harness.state().anim_preview.is_none());

    harness.state_mut().run("animation.preview", json!({})).unwrap();
    harness.run_steps(1);
    let centre = harness.state().slide_rect.unwrap().center();
    harness.hover_at(centre);
    harness.run_steps(1);
    harness.drag_at(centre);
    harness.run_steps(2);
    assert!(harness.state().anim_preview.is_none(), "a click on the slide stops the preview");

    harness.state_mut().run("animation.preview", json!({})).unwrap();
    harness.run_steps(1);
    harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
    harness.run_steps(1);
    assert!(harness.state().anim_preview.is_none(), "leaving the slide stops the preview");

    harness.state_mut().session.execute("slide.previous", &json!({})).unwrap();
    harness.state_mut().session.execute("edit.select", &json!({"ids": [id]})).unwrap();
    harness.state_mut().run("animation.preview", json!({})).unwrap();
    harness.run_steps(1);
    assert!(harness.state().anim_preview.is_some());
    // 400 ms effect plus the hold.
    harness.run_steps(30);
    assert!(harness.state().anim_preview.is_none(), "the preview ends after the sequence");
    assert!(harness.state().show.is_none());
}

fn flick_harness() -> egui_kittest::Harness<'static, SlideApp> {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    // Fitted: the slide is smaller than the window, so a flick turns the page instead of panning.
    harness.state_mut().ui.zoom = Some(0.4);
    harness
}

fn slide_at(app: &SlideApp) -> usize {
    app.session.active().unwrap().selection.slide
}

/// One frame of a trackpad flick: four short samples, each under egui's cutoff for smoothing a
/// mouse notch. The harness runs one frame per queued event, so they have to share a frame or
/// none of them reaches the page threshold.
fn flick(harness: &mut egui_kittest::Harness<'static, SlideApp>, delta: egui::Vec2) {
    let centre = harness.state().canvas_rect.unwrap_or(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))).center();
    let input = harness.input_mut();
    input.events.push(egui::Event::PointerMoved(centre));
    let step = delta / 4.0;
    for _ in 0..4 {
        input.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: step,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Trackpad,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run_steps(1);
}

#[test]
fn page_flick_uses_the_stronger_axis() {
    assert_eq!(crate::canvas::page_flick(egui::vec2(-24.0, -4.0)), Some(true));
    assert_eq!(crate::canvas::page_flick(egui::vec2(24.0, 4.0)), Some(false));
    assert_eq!(crate::canvas::page_flick(egui::vec2(-4.0, -24.0)), Some(true));
    assert_eq!(crate::canvas::page_flick(egui::vec2(4.0, 10.0)), None);
}

/// Test helper: place the pointer over the canvas centre, inject wheel notches, and step once.
fn mouse_wheel(harness: &mut egui_kittest::Harness<'static, SlideApp>, notches: &[f32]) {
    let centre = harness.state().canvas_rect.unwrap().center();
    let input = harness.input_mut();
    input.events.push(egui::Event::PointerMoved(centre));
    for &y in notches {
        input.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, y),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run_steps(1);
}

#[test]
fn mouse_wheel_keeps_fast_notches_in_both_directions_without_a_smoothed_tail() {
    let mut h = flick_harness();
    for _ in 0..8 {
        h.state_mut().run("slide.new", json!({})).unwrap();
    }
    h.state_mut().run("slide.go", json!({"index": 0})).unwrap();
    h.run_steps(1);

    mouse_wheel(&mut h, &[-1.0]);
    assert_eq!(slide_at(h.state()), 1, "one notch responds immediately");
    mouse_wheel(&mut h, &[-1.0]);
    assert_eq!(slide_at(h.state()), 2, "the next frame does not lose a notch to the cooldown");
    mouse_wheel(&mut h, &[-1.0, -1.0]);
    assert_eq!(slide_at(h.state()), 4, "notches batched into a frame are retained");
    mouse_wheel(&mut h, &[-3.0]);
    assert_eq!(slide_at(h.state()), 7, "an accelerated wheel event keeps its steps");
    h.run_steps(30);
    assert_eq!(slide_at(h.state()), 7, "smoothing must not turn more pages");
    mouse_wheel(&mut h, &[2.0]);
    assert_eq!(slide_at(h.state()), 5, "reversing direction responds immediately");
    mouse_wheel(&mut h, &[100.0]);
    assert_eq!(slide_at(h.state()), 0, "stop at the first slide");
    mouse_wheel(&mut h, &[-100.0]);
    assert_eq!(slide_at(h.state()), 8, "stop at the last slide");
}

/// A high-resolution wheel on Windows sends one notch as eight 0.125-line events.
#[test]
fn mouse_wheel_fractional_lines_accumulate_into_one_slide_per_line() {
    let mut h = flick_harness();
    for _ in 0..4 {
        h.state_mut().run("slide.new", json!({})).unwrap();
    }
    h.state_mut().run("slide.go", json!({"index": 0})).unwrap();
    h.run_steps(1);

    for _ in 0..7 {
        mouse_wheel(&mut h, &[-0.125]);
        assert_eq!(slide_at(h.state()), 0, "a fraction of a line does not turn the page");
    }
    mouse_wheel(&mut h, &[-0.125]);
    assert_eq!(slide_at(h.state()), 1, "eight eighths make exactly one slide");
    h.run_steps(30);
    assert_eq!(slide_at(h.state()), 1, "nothing is left over to turn another page");

    mouse_wheel(&mut h, &[-0.5]);
    mouse_wheel(&mut h, &[0.5, 0.25]);
    assert_eq!(slide_at(h.state()), 1, "a change of direction drops the leftover fraction");
    mouse_wheel(&mut h, &[0.25]);
    assert_eq!(slide_at(h.state()), 0, "a whole line back is one slide back");
}

/// A fraction of a line left over in one presentation does not carry into another.
#[test]
fn mouse_wheel_fraction_does_not_carry_into_another_presentation() {
    let mut h = flick_harness();
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.state_mut().run("slide.go", json!({"index": 0})).unwrap();
    h.run_steps(1);
    mouse_wheel(&mut h, &[-0.75]);
    assert_eq!(slide_at(h.state()), 0);

    let a = h.state().session.active_index().unwrap();
    h.state_mut().run("file.new", json!({})).unwrap();
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.state_mut().run("slide.go", json!({"index": 0})).unwrap();
    h.state_mut().ui.zoom = Some(0.4);
    h.run_steps(2);
    mouse_wheel(&mut h, &[-0.25]);
    assert_eq!(slide_at(h.state()), 0, "a quarter line in the new presentation does not turn the page");
    mouse_wheel(&mut h, &[-0.75]);
    assert_eq!(slide_at(h.state()), 1, "a whole line in the new presentation does");

    // Back in the first one, leave 0.75 of a line, then switch away and back with the pointer
    // off the canvas (on the tabs): the canvas never sees a hover in the other presentation.
    let b = h.state().session.active_index().unwrap();
    h.state_mut().session.set_active(a);
    h.state_mut().ui.zoom = Some(0.4);
    h.run_steps(2);
    mouse_wheel(&mut h, &[-0.75]);
    assert_eq!(slide_at(h.state()), 0);
    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(-10.0, -10.0)));
    h.run_steps(1);
    h.state_mut().session.set_active(b);
    h.run_steps(2);
    h.state_mut().session.set_active(a);
    h.run_steps(2);
    mouse_wheel(&mut h, &[-0.25]);
    assert_eq!(slide_at(h.state()), 0, "switching away without a hover still drops the fraction");
}

/// Wheel input over a zoomed canvas pans it, while the same input outside the canvas must not
/// navigate slides.
#[test]
fn mouse_wheel_over_a_zoomed_canvas_pans_and_outside_the_canvas_does_not_navigate() {
    let mut h = flick_harness();
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.state_mut().run("slide.go", json!({"index": 0})).unwrap();
    h.state_mut().ui.zoom = Some(3.0);
    h.run_steps(2);
    let before = h.state().slide_rect.unwrap();
    mouse_wheel(&mut h, &[-3.0]);
    h.run_steps(30);
    assert_eq!(slide_at(h.state()), 0);
    assert!(h.state().slide_rect.unwrap().min.y < before.min.y, "the enlarged canvas scrolls");

    h.state_mut().ui.zoom = Some(0.4);
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(-10.0, -10.0)));
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -3.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    assert_eq!(slide_at(h.state()), 0, "wheel input outside the canvas is ignored");
}

/// A sideways swipe on a fitted slide turns the page, like PowerPoint on a Mac trackpad.
/// The frames of one gesture do not skip ahead.
#[test]
fn a_sideways_trackpad_swipe_turns_the_page_and_a_second_flick_waits() {
    let mut harness = flick_harness();
    harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
    harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
    harness.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
    harness.run_steps(1);
    assert_eq!(slide_at(harness.state()), 0);

    flick(&mut harness, egui::vec2(-24.0, 0.0));
    assert_eq!(slide_at(harness.state()), 1, "swipe left goes to the next slide");

    flick(&mut harness, egui::vec2(-24.0, 0.0));
    assert_eq!(slide_at(harness.state()), 1, "the next frame of the same gesture does not skip a slide");

    harness.run_steps(30);
    flick(&mut harness, egui::vec2(24.0, 0.0));
    assert_eq!(slide_at(harness.state()), 0, "swipe right goes back");
}

#[test]
fn a_vertical_wheel_still_turns_the_page_and_a_zoomed_slide_pans_instead() {
    let mut harness = flick_harness();
    harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
    harness.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
    harness.run_steps(1);
    flick(&mut harness, egui::vec2(-2.0, -24.0));
    assert_eq!(slide_at(harness.state()), 1, "a vertical wheel still goes to the next slide");

    harness.state_mut().ui.zoom = Some(3.0);
    harness.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
    harness.run_steps(2);
    flick(&mut harness, egui::vec2(-24.0, 0.0));
    assert_eq!(slide_at(harness.state()), 0, "zoomed in, a sideways swipe pans instead of turning the page");
}

/// `show.start {from}` that names no slide of the deck is refused, over `run` and over the
/// control channel, instead of starting at slide 0 or on the end screen. A hidden slide is a
/// slide: From Current Slide plays it.
#[test]
fn show_start_refuses_a_from_that_is_not_a_slide() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.hide", &json!({"index": 1, "hidden": true})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    let ctx = egui::Context::default();
    for from in [json!(-1), json!(1.5), json!("1"), json!(3), json!(u64::MAX)] {
        assert!(app.run("show.start", json!({"from": from})).is_err(), "{from}");
        assert!(app.show.is_none(), "{from}");
        let (req, _rx) = control::ControlRequest::new("show.start", json!({"from": from}));
        let control::Outcome::Done(answer) = control::handle(&mut app, &ctx, &req) else { panic!("{from}") };
        assert_eq!(answer["ok"], false, "{from}: {answer}");
        assert!(app.show.is_none(), "{from}");
    }
    app.run("show.start", json!({"from": 1})).unwrap();
    assert_eq!(app.show.as_ref().map(|s| (s.state.slide, s.state.ended)), Some((1, false)));
    app.show = None;
    let (req, _rx) = control::ControlRequest::new("show.start", json!({"from": 2, "reading": true}));
    let control::Outcome::Done(answer) = control::handle(&mut app, &ctx, &req) else { panic!() };
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(app.show.as_ref().map(|s| (s.state.slide, s.reading)), Some((2, true)));
}

#[test]
fn a_swipe_during_the_show_turns_the_page() {
    let mut harness = flick_harness();
    harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
    harness.state_mut().session.execute("slide.go", &json!({"index": 0})).unwrap();
    harness.state_mut().start_show(0, true);
    harness.run_steps(2);
    flick(&mut harness, egui::vec2(-24.0, 0.0));
    assert_eq!(harness.state().show.as_ref().map(|s| s.state.slide), Some(1));
    harness.run_steps(30);
    flick(&mut harness, egui::vec2(24.0, 0.0));
    assert_eq!(harness.state().show.as_ref().map(|s| s.state.slide), Some(0));
}

/// The pen is drawing. A flick must not turn the page out from under the stroke.
#[test]
fn the_pen_ignores_a_swipe_during_the_show() {
    let mut harness = flick_harness();
    harness.state_mut().session.execute("slide.new", &json!({})).unwrap();
    harness.state_mut().start_show(0, true);
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::P);
    harness.run_steps(1);
    flick(&mut harness, egui::vec2(-24.0, 0.0));
    assert_eq!(harness.state().show.as_ref().map(|s| s.state.slide), Some(0));
}

/// Preferences › Interface size (#767): dragging the slider leaves the interface size alone
/// until the pointer lets go, so the slider doesn't grow under the pointer mid-drag.
#[test]
fn interface_size_applies_when_the_slider_is_let_go() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    harness.state_mut().run("app.preferences", json!({})).unwrap();
    harness.run_steps(4);
    let before = harness.state().ui.ui_scale;
    let rect = harness.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Interface size").next().expect("the Interface size slider").rect();
    let (from, to) = (rect.left_center() + egui::vec2(4.0, 0.0), rect.right_center() - egui::vec2(4.0, 0.0));
    harness.hover_at(from);
    harness.run_steps(1);
    harness.drag_at(from);
    harness.run_steps(1);
    for k in 1..=8 {
        harness.hover_at(from.lerp(to, k as f32 / 8.0));
        harness.run_steps(1);
    }
    assert_eq!(harness.state().ui.ui_scale, before, "nothing rescales while the slider is held");
    harness.drop_at(to);
    harness.run_steps(2);
    let after = harness.state().ui.ui_scale;
    assert!(after > before + 0.3, "letting go applies the size dragged to: {before} -> {after}");
    assert!(harness.state().dialog.is_some(), "Preferences stays open");
}
/// The status bar's zoom slider has no room for a visible label; it still needs an accessible
/// name, and adding that name must not change the slider's shared control geometry at either
/// size.
#[test]
fn status_zoom_slider_is_named_without_growing_the_status_bar() {
    for size in [egui::vec2(900.0, 560.0), egui::vec2(1440.0, 900.0)] {
        let mut harness = egui_kittest::Harness::builder().with_size(size).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        harness.run_steps(3);
        let mut zoom = harness.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Zoom");
        let rect = zoom.next().unwrap_or_else(|| panic!("a named Zoom slider at {size:?}")).rect();
        assert!(zoom.next().is_none(), "exactly one Zoom slider");
        // egui 0.37's slider uses its own `slider_width` and the theme's control height
        // (22 px), not the old hard-coded 110x18. Assert that real, shared geometry so the
        // status row stays consistent with every other control instead of loosening a bound.
        let style = harness.ctx.global_style();
        let (expected_w, expected_h) = (style.spacing.slider_width, style.spacing.interact_size.y);
        assert!(
            (rect.width() - expected_w).abs() <= 0.5 && (rect.height() - expected_h).abs() <= 0.5,
            "the status-bar slider uses the shared {expected_w}x{expected_h} control size at {size:?}: {rect:?}"
        );
    }
}

/// Accessibility names survive the port: the Preferences dialog and the Format Shape fill page
/// expose no unnamed input widgets, and the fill Transparency row has one named slider rather
/// than a visible label duplicated by `Slider::text`.
#[test]
fn preferences_and_format_pane_name_every_input() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(3);
    harness.state_mut().run("app.preferences", json!({})).unwrap();
    harness.run_steps(3);
    harness.check_accessibility();
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "Preferences closed before the pane check");

    harness.state_mut().run("shape.insert", json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    harness.state_mut().ui.pane = Some("format".into());
    harness.state_mut().ui.format_tab = "fill".into();
    harness.run_steps(3);
    harness.check_accessibility();
    let sliders: Vec<_> = harness.query_all_by_role_and_label(egui::accesskit::Role::Slider, "Transparency").collect();
    assert_eq!(sliders.len(), 1, "one fill Transparency slider");
    assert!(sliders[0].rect().width() > 40.0, "the slider keeps room next to its one visible label: {:?}", sliders[0].rect());
}

/// A control `ui.screenshot` is admitted once, issued from a settled real UI pass, completed by
/// kittest's screenshot callback, and replied to exactly once after a logic drain.
#[test]
fn ui_screenshot_round_trips_through_the_callable_slot() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    let (tx, rx) = std::sync::mpsc::channel();
    harness.state_mut().control_rx = Some(rx);
    let (req, reply) = ControlRequest::new("ui.screenshot", json!({}));
    tx.send(req).unwrap();
    harness.run_steps(2);
    assert_eq!(harness.state().capture.pending(), 1, "the request is admitted");
    harness.state_mut().capture.force_settle();
    harness.run_steps(4);
    let reply = reply.recv_timeout(std::time::Duration::from_secs(5)).expect("screenshot reply");
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(reply["result"]["width"].as_u64().is_some_and(|w| w > 0));
    assert_eq!(harness.state().capture.pending(), 0);
    assert_eq!(harness.state().capture.reserved_bytes(), 0, "the reservation is released after the reply");
}

/// A hidden/logic-only iteration drains control requests and autosave/screenshot queues; it does
/// not need a painted frame for those side effects.
#[test]
fn logic_drains_control_without_a_ui_pass() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let ctx = egui::Context::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.control_rx = Some(rx);
    let (req, reply) = ControlRequest::new("ui.inspect", json!({}));
    tx.send(req).unwrap();
    app.logic(&ctx);
    let v = reply.recv_timeout(std::time::Duration::from_secs(1)).expect("reply from a logic-only call");
    assert_eq!(v["ok"], true, "{v}");
}

/// One synthetic pointer batch is injected at most once while it waits for a real UI pass.
/// eframe carries the whole `RawInput` from a logic-only frame into the next full pass, so a
/// document or layout change between them must retract the already-appended events too.
#[test]
fn synthetic_input_is_one_pending_batch_and_never_replays() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let ctx = egui::Context::default();
    app.queue_input(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    let mut raw = egui::RawInput::default();
    app.raw_input_hook(&ctx, &mut raw);
    assert_eq!(raw.events.len(), 1, "the one pending event is injected once");
    assert!(app.synthetic.is_empty());
    // eframe's next logic-only frame hands the same RawInput back to the hook. The batch is
    // still unacknowledged, so it must not be appended again.
    let mut carried = raw.clone();
    app.raw_input_hook(&ctx, &mut carried);
    assert_eq!(carried.events.len(), 1, "the unacknowledged batch is not injected twice");

    // A settled UI pass acknowledges it; the next batch may then be injected. This test has
    // no renderer, so clear the frame's texture uploads explicitly: egui 0.37 panics when a
    // `TexturesDelta` with pending uploads is dropped.
    app.fonts_ready = true;
    app.styled = true;
    ctx.run_ui(Default::default(), |ui| app.ui(ui)).drop_without_applying_deltas();

    // Changing the document before the next pass drops a batch queued for the old one.
    app.queue_input(egui::Event::PointerMoved(egui::pos2(20.0, 20.0)));
    app.session.execute("file.new", &json!({})).unwrap();
    let mut raw2 = egui::RawInput::default();
    app.raw_input_hook(&ctx, &mut raw2);
    assert!(raw2.events.is_empty(), "a batch queued for the old document is dropped");
    assert!(app.synthetic.is_empty());

    // A pending batch for the current document is injected, then a document change before
    // the next pass must retract it from the carried RawInput while keeping the newer click
    // queued for the new document.
    // The host supplies an identical pointer event before the injected batch: retraction must
    // remove the batch at its recorded index, not the first identical window in the batch.
    let moved = egui::Event::PointerMoved(egui::pos2(30.0, 30.0));
    app.queue_input(moved.clone());
    let mut raw3 = egui::RawInput::default();
    raw3.events.push(moved.clone());
    app.raw_input_hook(&ctx, &mut raw3);
    assert_eq!(raw3.events, vec![moved.clone(), moved.clone()], "a settled pass permits the next batch");
    app.session.execute("file.new", &json!({})).unwrap();
    let newer = egui::Event::PointerMoved(egui::pos2(40.0, 40.0));
    app.queue_input(newer.clone());
    let host = egui::Event::Text("host".into());
    let mut carried3 = raw3.clone();
    carried3.events.push(host.clone());
    app.raw_input_hook(&ctx, &mut carried3);
    assert_eq!(
        carried3.events,
        vec![moved, host, newer],
        "the stale batch is retracted, host input and the newer click for the changed document are kept"
    );
    assert!(app.synthetic.is_empty(), "the newer batch was injected, not cleared with the stale one");
    assert!(app.synthetic_pending, "the newer batch is now the one in flight");

    let mut raw4 = egui::RawInput::default();
    app.raw_input_hook(&ctx, &mut raw4);
    assert!(raw4.events.is_empty(), "the in-flight batch is not injected twice");
}

/// The first real pass installs the interface fonts and asks egui to discard it before any
/// layout. A control click that arrived in that same pass has already been consumed by egui,
/// so the app must put it back in the queue for the next frame instead of acknowledging and
/// losing it.
#[test]
fn a_control_click_on_the_first_font_discard_is_requeued_and_lands() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.fonts_ready = true;
    app.styled = true;
    let ctx = egui::Context::default();
    let pass = |app: &mut SlideApp, time: f64| {
        let mut raw = egui::RawInput { time: Some(time), ..Default::default() };
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo { minimized: Some(false), occluded: Some(false), native_pixels_per_point: Some(1.0), ..Default::default() },
        );
        app.raw_input_hook(&ctx, &mut raw);
        ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
        .drop_without_applying_deltas();
    };
    pass(&mut app, 1.0);
    let size = app.session.active().unwrap().doc.slide_size;
    app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [0.0, 0.0, size.width, size.height]})).unwrap();
    app.session.execute("edit.deselect", &json!({})).unwrap();
    pass(&mut app, 2.0);
    let point = app.slide_rect.expect("the canvas has been laid out").center();
    assert!(app.session.active().unwrap().selection.shapes.is_empty());

    // Back to the first-frame state: the next pass installs fonts and is discarded.
    app.styled = false;
    app.fonts_ready = false;
    app.queue_input(egui::Event::PointerMoved(point));
    app.queue_input(egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    app.queue_input(egui::Event::PointerButton {
        pos: point,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    pass(&mut app, 3.0);
    assert!(app.session.active().unwrap().selection.shapes.is_empty(), "the discarded pass did not run the canvas");
    assert!(!app.synthetic.is_empty(), "the click was requeued for the next frame");

    // One pointer event is injected per pass: move, press, release.
    for time in [4.0, 5.0, 6.0] {
        pass(&mut app, time);
    }
    assert!(!app.session.active().unwrap().selection.shapes.is_empty(), "the requeued click selected the slide's shape");
    assert!(!app.synthetic_pending && app.synthetic.is_empty(), "the click was consumed exactly once");
}

/// A modal opening changes what a queued window-point event means even when the document and
/// canvas are unchanged; the injected batch is retracted before it reaches the modal.
#[test]
fn synthetic_batch_is_retracted_when_a_modal_opens_first() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let ctx = egui::Context::default();
    app.queue_input(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    let mut raw = egui::RawInput::default();
    app.raw_input_hook(&ctx, &mut raw);
    assert_eq!(raw.events.len(), 1);
    app.dialog = Some(crate::dialogs::Dialog::new("replaceFonts"));
    app.raw_input_hook(&ctx, &mut raw);
    assert!(raw.events.is_empty(), "the click queued for the canvas does not reach the modal");
    assert!(!app.synthetic_pending);
}

/// A control click is admitted for the full pass it was queued for. In that pass egui has
/// already derived `PointerState` from the injected press/release, so retracting only the raw
/// events would still let the click reach a document opened by the same pass's logic. The
/// control request therefore waits: the old target's real `logic` + `ui` pass consumes the
/// whole pointer gesture, and the next pass (no synthetic input) applies the document change.
#[test]
fn synthetic_click_is_consumed_on_its_old_target_before_logic_changes_the_document() {
    // The destination has one shape over the whole slide, so a click that leaked into it
    // would leave the shape selected. Opening it creates a fresh document state with an
    // empty selection.
    let mut destination = Session::with_new();
    let size = destination.active().unwrap().doc.slide_size;
    destination.execute("shape.insert", &json!({"preset": "rect", "rect": [0.0, 0.0, size.width, size.height]})).unwrap();
    let data = destination.execute("file.saveBytes", &json!({})).unwrap()["data"].as_str().unwrap().to_string();

    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.fonts_ready = true;
    app.styled = true;
    let ctx = egui::Context::default();
    let pass = |app: &mut SlideApp, time: f64| {
        let mut raw = egui::RawInput { time: Some(time), ..Default::default() };
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo { minimized: Some(false), occluded: Some(false), native_pixels_per_point: Some(1.0), ..Default::default() },
        );
        app.raw_input_hook(&ctx, &mut raw);
        ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
        .drop_without_applying_deltas();
    };

    pass(&mut app, 1.0);
    app.run("shape.insert", json!({"preset": "rect", "rect": [100.0, 100.0, 200.0, 100.0]})).unwrap();
    assert!(!app.session.active().unwrap().selection.shapes.is_empty(), "the old document has a selection to clear");
    pass(&mut app, 2.0);
    let old = app.session.active().unwrap().uid;
    let slide = app.slide_rect.expect("the canvas has been laid out");
    // Well away from the inserted rectangle, so a press on the bare canvas clears the
    // selection in the old document.
    let point = egui::pos2(slide.min.x + slide.width() * 0.8, slide.min.y + slide.height() * 0.8);
    app.queue_input(egui::Event::PointerMoved(point));
    app.queue_input(egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    app.queue_input(egui::Event::PointerButton {
        pos: point,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });

    let (tx, rx) = std::sync::mpsc::channel();
    app.control_rx = Some(rx);
    let (req, reply) =
        ControlRequest::new("engine.execute", json!({"command": "file.openBytes", "params": {"name": "destination.deckcraft", "data": data}}));
    tx.send(req).unwrap();

    // `raw_input_hook` admits one pointer event per pass. Every one of those passes must stay
    // on the old target; the control request may not create the destination before ui sees
    // the press/release.
    for time in [3.0, 4.0, 5.0] {
        pass(&mut app, time);
        assert_eq!(app.session.active().unwrap().uid, old, "the click's pass stays on its captured target");
        assert_eq!(app.session.documents().len(), 1, "logic must not open the destination under the click");
    }
    assert!(!app.synthetic_pending, "the full passes consumed the gesture");
    assert!(app.session.active().unwrap().selection.shapes.is_empty(), "the press selected the bare canvas on the old document");

    // With no synthetic input pending, the next logic applies the deferred request. The
    // destination's full-slide shape is unselected because no pointer state from the old
    // pass was replayed into it.
    pass(&mut app, 6.0);
    let reply = reply.recv_timeout(std::time::Duration::from_secs(1)).expect("control reply after the click");
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(app.session.documents().len(), 2);
    let active = app.session.active().unwrap();
    assert_ne!(active.uid, old, "the destination opened only after the click was consumed");
    assert!(!active.doc.slides[0].shapes.is_empty(), "the destination really has a clickable shape");
    assert!(active.selection.shapes.is_empty(), "the old pass's click did not select the destination shape");
}

/// A queued control click belongs to the full-screen show, opening window and palette
/// selection that were on screen when it was queued; each transition is its own target.
#[test]
fn input_target_tracks_slideshow_opening_and_palette_selection() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let base = app.input_target();
    app.start_show(0, true);
    assert_ne!(app.input_target(), base, "a slideshow is a different input target");
    let started = app.input_target();
    app.show.as_mut().unwrap().state.step += 1;
    let stepped = app.input_target();
    assert_ne!(stepped, started, "the show step changes which click areas are live");
    let reading_now = app.show.as_ref().unwrap().reading;
    app.show.as_mut().unwrap().reading = !reading_now;
    let reading = app.input_target();
    assert_ne!(reading, stepped, "the reading layout moves the slide");
    app.show.as_mut().unwrap().grid = true;
    let grid = app.input_target();
    assert_ne!(grid, reading, "See All Slides turns a click into a thumbnail choice");
    app.show.as_mut().unwrap().ended = true;
    let ended = app.input_target();
    assert_ne!(ended, grid, "the end screen has different click targets");
    app.show = None;
    assert_eq!(app.input_target(), base, "leaving the show returns to the editor target");

    // Starting the show again is a new instance even at the same slide, so an old batch cannot
    // match by value after an ABA transition.
    app.start_show(0, true);
    let first_instance = app.input_target();
    app.start_show(0, true);
    assert_ne!(app.input_target(), first_instance, "a new show instance is a new target");
    app.show = None;

    app.opening = Some(crate::opening::Opening::for_test("/tmp/quarterly.pptx"));
    assert_ne!(app.input_target(), base, "the background-open window is a different target");
    let opening = app.input_target();
    app.opening.as_mut().unwrap().generation = 2;
    assert_ne!(app.input_target(), opening, "reopening the same path is a new attempt");
    app.opening = None;

    app.palette = Some(("open".into(), 0));
    let first = app.input_target();
    app.palette = Some(("open".into(), 1));
    assert_ne!(app.input_target(), first, "the palette's highlighted entry is part of the target");
    app.palette = None;

    app.ui.tab = "Insert".into();
    assert_ne!(app.input_target(), base, "another ribbon tab is another click target");
    let insert = app.input_target();
    app.ui.ribbon_collapsed = true;
    assert_ne!(app.input_target(), insert, "collapsing the ribbon changes the layout under a click");
    app.ui.ribbon_collapsed = false;
    app.ribbon_scroll.insert("Insert".into(), 64.0_f32.to_bits());
    assert_ne!(app.input_target(), insert, "scrolling the ribbon moves the buttons under a click");
    app.ui.tab = "Home".into();
    app.ribbon_scroll.remove("Insert");
    assert_eq!(app.input_target(), base);
}

/// A click queued for the editor is retracted when a slideshow, background-open window or
/// different palette entry takes over before the input pass.
#[test]
fn input_target_changes_retract_a_queued_click() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let ctx = egui::Context::default();
    let pointer = || egui::Event::PointerMoved(egui::pos2(10.0, 10.0));

    app.queue_input(pointer());
    app.start_show(0, true);
    let mut raw = egui::RawInput::default();
    app.raw_input_hook(&ctx, &mut raw);
    assert!(raw.events.is_empty(), "a click queued for the editor does not enter the slideshow");
    assert!(app.synthetic.is_empty());
    app.show = None;

    app.queue_input(pointer());
    app.opening = Some(crate::opening::Opening::for_test("/tmp/quarterly.pptx"));
    let mut raw = egui::RawInput::default();
    app.raw_input_hook(&ctx, &mut raw);
    assert!(raw.events.is_empty(), "a click queued for the editor does not enter the opening window");
    app.opening = None;

    app.palette = Some(("open".into(), 0));
    app.queue_input(pointer());
    app.palette = Some(("open".into(), 1));
    let mut raw = egui::RawInput::default();
    app.raw_input_hook(&ctx, &mut raw);
    assert!(raw.events.is_empty(), "a click queued for one palette selection does not activate another");
    assert!(app.synthetic.is_empty());
}

/// A logic-only callback does not consume events, but eframe carries the whole RawInput into
/// the next full pass. The hook injects without guessing the phase; when a control request
/// drained by that logic-only callback changes the document, the carried batch is retracted at
/// the next hook instead of being delivered into the new document.
#[test]
fn hidden_logic_pass_carries_a_synthetic_batch_and_drops_it_when_the_document_changes() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let ctx = egui::Context::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.control_rx = Some(rx);
    app.queue_input(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    let mut hidden = egui::RawInput::default();
    hidden.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo { minimized: Some(true), occluded: Some(true), native_pixels_per_point: Some(1.0), ..Default::default() },
    );
    app.raw_input_hook(&ctx, &mut hidden);
    assert_eq!(hidden.events.len(), 1, "the batch is injected into the RawInput eframe will carry");
    assert!(app.synthetic.is_empty());
    assert!(app.synthetic_pending, "run_logic leaves the batch unacknowledged");

    let (req, reply) = ControlRequest::new("engine.execute", json!({"command": "file.new", "params": {}}));
    tx.send(req).unwrap();
    let _ = ctx.run_logic(&hidden, |ctx| app.logic(ctx));
    let v = reply.recv_timeout(std::time::Duration::from_secs(1)).expect("control reply from the hidden pass");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(app.session.documents().len(), 2, "the hidden pass opened another document");
    assert!(app.synthetic_pending, "a logic-only callback cannot retract the carried batch");

    // eframe hands the hidden RawInput to the next visible pass, appending new host input.
    let mut visible = hidden.clone();
    visible.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo { minimized: Some(false), occluded: Some(false), native_pixels_per_point: Some(1.0), ..Default::default() },
    );
    let host = egui::Event::Text("host".into());
    visible.events.push(host.clone());
    app.raw_input_hook(&ctx, &mut visible);
    assert_eq!(visible.events, vec![host.clone()], "the click queued for the old document is dropped");
    assert!(app.synthetic.is_empty());
    ctx.run_ui(visible, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
    .drop_without_applying_deltas();
    assert!(app.synthetic.is_empty(), "nothing replays after the visible pass");
}

/// eframe may run logic-only for a *visible* root when a redraw deadline is overdue. The
/// previous pass's InputState is still live, so retraction must not drain an identical event
/// from it; the batch belongs to the carried RawInput and is removed at the next hook.
#[test]
fn visible_logic_pass_does_not_retract_from_the_previous_input_state() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    let ctx = egui::Context::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.control_rx = Some(rx);
    let pointer = egui::Event::PointerMoved(egui::pos2(10.0, 10.0));

    // Live InputState from the previous full pass, with an identical event at the same index.
    let mut previous = egui::RawInput { time: Some(1.0), ..Default::default() };
    previous.events.push(pointer.clone());
    ctx.run_ui(previous, |_ui| {}).drop_without_applying_deltas();

    app.queue_input(pointer.clone());
    let mut raw = egui::RawInput { time: Some(2.0), ..Default::default() };
    raw.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo { minimized: Some(false), occluded: Some(false), native_pixels_per_point: Some(1.0), ..Default::default() },
    );
    app.raw_input_hook(&ctx, &mut raw);
    assert_eq!(raw.events, vec![pointer.clone()]);

    let (req, reply) = ControlRequest::new("engine.execute", json!({"command": "file.new", "params": {}}));
    tx.send(req).unwrap();
    let _ = ctx.run_logic(&raw, |ctx| app.logic(ctx));
    let v = reply.recv_timeout(std::time::Duration::from_secs(1)).expect("control reply from the visible logic pass");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(app.session.documents().len(), 2);
    assert!(app.synthetic_pending, "the carried batch is still pending");
    assert_eq!(ctx.input(|i| i.events.clone()), vec![pointer.clone()], "the previous pass's events are untouched");

    let mut next = raw.clone();
    next.time = Some(3.0);
    let host = egui::Event::Text("host".into());
    next.events.push(host.clone());
    app.raw_input_hook(&ctx, &mut next);
    assert_eq!(next.events, vec![host.clone()], "the stale batch is removed from the carried RawInput");
    assert!(!app.synthetic_pending);
    assert!(app.synthetic.is_empty());
}

/// A batch injected in a logic-only pass is carried into the full pass with a refreshed
/// `RawInput::time`. That full pass must consume it on the old target: a control request
/// drained by the same logic waits until the next pass instead of changing the document
/// after egui has already derived the carried pointer state.
#[test]
fn carried_batch_is_consumed_before_the_full_pass_applies_a_control_change() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.fonts_ready = true;
    app.styled = true;
    let ctx = egui::Context::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.control_rx = Some(rx);
    app.queue_input(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    let mut hidden = egui::RawInput { time: Some(1.0), ..Default::default() };
    hidden.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo { minimized: Some(true), occluded: Some(true), native_pixels_per_point: Some(1.0), ..Default::default() },
    );
    app.raw_input_hook(&ctx, &mut hidden);
    let _ = ctx.run_logic(&hidden, |ctx| app.logic(ctx));
    assert!(app.synthetic_pending, "the logic-only pass keeps the carried batch");

    // eframe refreshes the carried input's time before calling the hook for the full pass.
    let mut full = hidden.clone();
    full.time = Some(2.0);
    app.raw_input_hook(&ctx, &mut full);
    assert!(app.synthetic_pending, "the carried batch is not injected twice");

    let (req, reply) = ControlRequest::new("engine.execute", json!({"command": "file.new", "params": {}}));
    tx.send(req).unwrap();
    ctx.run_ui(full, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
    .drop_without_applying_deltas();
    assert_eq!(app.session.documents().len(), 1, "the full pass's UI sees its captured document");
    assert!(!app.synthetic_pending, "the full pass consumed the carried batch");

    // The next pass has no synthetic input, so logic applies the deferred request.
    let mut next = egui::RawInput { time: Some(3.0), ..Default::default() };
    next.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo { minimized: Some(false), occluded: Some(false), native_pixels_per_point: Some(1.0), ..Default::default() },
    );
    app.raw_input_hook(&ctx, &mut next);
    ctx.run_ui(next, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
    .drop_without_applying_deltas();
    let v = reply.recv_timeout(std::time::Duration::from_secs(1)).expect("control reply after the full pass");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(app.session.documents().len(), 2, "the deferred control request ran on the next pass");
}

/// `visible() == false` does not prove a logic-only pass: a screenshot or
/// `RequestPaintWhileHidden` makes eframe run the full UI. The synthetic batch must be
/// delivered there, not postponed until the viewport is visible.
#[test]
fn hidden_full_pass_receives_synthetic_ui_input() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.fonts_ready = true;
    app.styled = true;
    app.dialog = Some(crate::dialogs::Dialog::new("replaceFonts"));
    app.queue_input(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
    let ctx = egui::Context::default();
    let mut hidden = egui::RawInput { time: Some(4.0), ..Default::default() };
    hidden.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo { minimized: Some(true), occluded: Some(true), native_pixels_per_point: Some(1.0), ..Default::default() },
    );
    app.raw_input_hook(&ctx, &mut hidden);
    assert_eq!(hidden.events.len(), 1, "a forced hidden full pass still gets the event");
    assert!(app.synthetic_pending);

    ctx.run_ui(hidden, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
    .drop_without_applying_deltas();
    assert!(app.dialog.is_none(), "the synthetic Escape reached the UI in the hidden full pass");
    assert!(!app.synthetic_pending, "the full pass acknowledged the batch");
    assert!(app.synthetic.is_empty());
}

/// egui 0.37 carries a clipboard image as `Event::PasteImage`; it inserts once, without
/// asking the host clipboard service for the same image again.
#[test]
fn paste_image_event_inserts_one_picture() {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    let before = harness.state().session.active().unwrap().doc.slides[0].shapes.len();
    let image = egui::ColorImage::filled([2, 2], egui::Color32::from_rgb(1, 2, 3));
    harness.event(egui::Event::PasteImage(std::sync::Arc::new(image)));
    harness.run_steps(2);
    let after = harness.state().session.active().unwrap().doc.slides[0].shapes.len();
    assert_eq!(after, before + 1, "one image is inserted");
}

/// A minimized/occluded show keeps playing from logic alone: the clock advances on wall
/// time, an After transition advances the slide without a draw, and a visible pass resumes
/// from the show state instead of inventing a paused-show policy.
#[test]
fn hidden_viewport_advances_show_and_presenter_clocks_from_logic() {
    /// Nested helper in the hidden-show test: build root `RawInput` with the given time and
    /// visibility flags.
    fn raw_at(time: f64, minimized: bool, occluded: bool) -> egui::RawInput {
        let mut raw = egui::RawInput { time: Some(time), ..Default::default() };
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo { minimized: Some(minimized), occluded: Some(occluded), native_pixels_per_point: Some(1.0), ..Default::default() },
        );
        raw
    }

    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.session.execute("slide.new", &json!({})).unwrap();
    app.session.execute("slide.new", &json!({})).unwrap();
    app.session.execute("transition.timing", &json!({"index": 0, "after": 1000})).unwrap();
    app.session.execute("transition.timing", &json!({"index": 1, "after": null})).unwrap();
    app.start_show(0, false);
    let ctx = egui::Context::default();

    // A real visible UI pass starts the show and its clocks. There is no renderer here, so
    // each frame's texture uploads are cleared explicitly (egui 0.37 panics on drop).
    ctx.run_ui(raw_at(10.0, false, false), |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
    .drop_without_applying_deltas();
    let start = app.show.as_ref().expect("show started").clock_snapshot();
    assert_eq!(start.step_start, 10.0);
    assert_eq!(start.show_start, 10.0);

    // Hidden logic starts at the last visible egui time, then advances on the monotonic
    // instant. Two seconds later the slide's After: 1 s transition has advanced the show.
    let t0 = web_time::Instant::now();
    app.show.as_mut().expect("show started").observe_visible_frame(10.0, t0);
    let _ = ctx.run_logic(&raw_at(12.0, true, false), |ctx| app.tick_hidden_show(ctx, t0));
    let _ = ctx.run_logic(&raw_at(12.0, true, false), |ctx| app.tick_hidden_show(ctx, t0 + std::time::Duration::from_secs(2)));
    let hidden = app.show.as_ref().expect("hidden show");
    assert_eq!(hidden.state.slide, 1, "the After transition advances without a draw");
    assert!((hidden.clock_snapshot().slide_start - 12.0).abs() < 1e-9);
    assert!((hidden.presenter_elapsed(12.0) - 2.0).abs() < 1e-9, "presenter time keeps running");

    // A later hidden tick does not replay the transition or stack time.
    let _ = ctx.run_logic(&raw_at(12.0, true, false), |ctx| app.tick_hidden_show(ctx, t0 + std::time::Duration::from_secs(3)));
    assert_eq!(app.show.as_ref().unwrap().state.slide, 1);
    assert!((app.show.as_ref().unwrap().clock_snapshot().slide_start - 12.0).abs() < 1e-9);

    // A visible pass re-anchors to egui's real time and keeps the state it reached hidden.
    ctx.run_ui(raw_at(20.0, false, false), |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
    .drop_without_applying_deltas();
    let resumed = app.show.as_ref().expect("show resumed");
    assert_eq!(resumed.state.slide, 1);
    assert!((resumed.presenter_elapsed(20.0) - 10.0).abs() < 1e-9);
    assert!((resumed.clock_snapshot().slide_start - 12.0).abs() < 1e-9);
}

/// A visible window can receive an overdue logic-only pass before the next paint. That pass
/// still carries the previous full frame's `InputState::time`; treating it as a visible frame
/// re-anchors the hidden clock to the later monotonic instant and loses elapsed show time.
/// Re-anchoring belongs to `show::ui`, which only runs on a real visible pass.
#[test]
fn overdue_visible_logic_does_not_reanchor_a_later_hidden_show() {
    /// Nested helper in the overdue-logic test: build root `RawInput` with the given time and
    /// visibility flags.
    fn raw_at(time: f64, minimized: bool, occluded: bool) -> egui::RawInput {
        let mut raw = egui::RawInput { time: Some(time), ..Default::default() };
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo { minimized: Some(minimized), occluded: Some(occluded), native_pixels_per_point: Some(1.0), ..Default::default() },
        );
        raw
    }

    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.session.execute("slide.new", &json!({})).unwrap();
    app.session.execute("slide.new", &json!({})).unwrap();
    app.session.execute("transition.timing", &json!({"index": 0, "after": 2500})).unwrap();
    app.session.execute("transition.timing", &json!({"index": 1, "after": null})).unwrap();
    app.start_show(0, false);
    let ctx = egui::Context::default();

    // The full visible pass is the only pass that anchors the show: egui time 10.
    ctx.run_ui(raw_at(10.0, false, false), |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    })
    .drop_without_applying_deltas();
    let visible_anchor = web_time::Instant::now();

    // An overdue visible logic-only pass does not refresh InputState::time. It must not move
    // the hidden anchor from the full visible frame to this later instant.
    let _ = ctx.run_logic(&raw_at(12.0, false, false), |ctx| app.tick_hidden_show(ctx, visible_anchor + std::time::Duration::from_secs(2)));
    assert_eq!(ctx.input(|i| i.time), 10.0, "run_logic retains the last full pass's egui time");
    assert_eq!(app.show.as_ref().unwrap().state.slide, 0);

    // Three seconds after the full visible frame is past the slide's 2.5 s After delay. If the
    // logic-only pass had re-anchored at +2 s, this hidden tick would still be only +1 s and
    // the slide would not advance.
    let _ = ctx.run_logic(&raw_at(12.0, true, false), |ctx| app.tick_hidden_show(ctx, visible_anchor + std::time::Duration::from_secs(3)));
    assert_eq!(app.show.as_ref().unwrap().state.slide, 1, "time from the full visible frame is preserved through the logic-only pass");
}

/// The `ViewportCommand::Title`s one logic tick sends to the window.
fn sent_titles(app: &mut SlideApp, ctx: &egui::Context) -> Vec<String> {
    let out = ctx.run_ui(egui::RawInput::default(), |ui| app.logic(ui.ctx()));
    let titles = out
        .viewport_output
        .values()
        .flat_map(|v| &v.commands)
        .filter_map(|c| match c {
            egui::ViewportCommand::Title(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    out.drop_without_applying_deltas();
    titles
}

/// #1058: the window title names the open presentation, with • while it has unsaved changes, so a
/// host embedding the web build can tell which file a drop opened. It goes out once per change.
#[test]
fn the_window_title_follows_the_dropped_presentation_and_its_changes() {
    let inbox = Inbox::default();
    let mut app = SlideApp::new(Session::new(), Services { inbox: Some(inbox.clone()), ..Default::default() });
    let ctx = egui::Context::default();
    assert_eq!(sent_titles(&mut app, &ctx), ["DeckCraft"]);
    assert!(sent_titles(&mut app, &ctx).is_empty(), "an unchanged title is not sent again");

    // The web build's drop: the file arrives through the inbox and opens on the next tick.
    let mut deck = Session::with_new();
    let data = deck.execute("file.saveBytes", &json!({})).unwrap()["data"].as_str().unwrap().to_string();
    let id = app.begin_external_file("drop");
    let bytes = deckcraft_engine::cmd::base64_decode(&data).unwrap();
    inbox.lock().unwrap().push(InboxEvent::File(InboxFile { id, name: "Quarterly review.deckcraft".into(), bytes }));
    assert_eq!(sent_titles(&mut app, &ctx), ["Quarterly review — DeckCraft"]);

    app.session.execute("slide.new", &json!({})).unwrap();
    assert_eq!(sent_titles(&mut app, &ctx), ["Quarterly review • — DeckCraft"]);
    assert!(sent_titles(&mut app, &ctx).is_empty());

    // Save As under another name renames it and clears the mark.
    let dir = std::env::temp_dir().join(format!("deckcraft-title-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Board update.deckcraft").to_string_lossy().to_string();
    app.session.execute("file.saveAs", &json!({"path": path})).unwrap();
    assert_eq!(sent_titles(&mut app, &ctx), ["Board update — DeckCraft"]);
    let _ = std::fs::remove_dir_all(&dir);

    app.session.execute("file.close", &json!({})).unwrap();
    assert_eq!(sent_titles(&mut app, &ctx), ["DeckCraft"]);
}

/// X11 window titles are C strings (a NUL panicked there in a sibling app): a file name sent with a
/// NUL over the control channel must not carry it into the window title.
#[test]
fn the_window_title_drops_nul_characters() {
    let mut app = SlideApp::new(Session::new(), Services::default());
    let data = Session::with_new().execute("file.saveBytes", &json!({})).unwrap()["data"].as_str().unwrap().to_string();
    app.session.execute("file.openBytes", &json!({"name": "Q3\u{0}deck.deckcraft", "data": data})).unwrap();
    assert_eq!(app.window_title(), "Q3deck — DeckCraft");
}
