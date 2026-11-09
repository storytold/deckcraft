#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::*;
use deckcraft_engine::Session;
use serde_json::json;

/// Optional export of real test frames for reviewing translated glyphs and clipping.
pub(crate) fn preview(harness: &mut egui_kittest::Harness<'_, SlideApp>, name: &str) {
    if let Some(dir) = std::env::var_os("DECKCRAFT_I18N_PREVIEW_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        harness.render().unwrap().save(dir.join(format!("{name}.png"))).unwrap();
    }
}

/// A text glyph's visible paint rectangle, independent of AccessKit metadata.
fn painted_text_rect(output: &egui::FullOutput, expected: &str) -> Option<egui::Rect> {
    /// Find an unclipped painted text rectangle, including text inside nested shape groups.
    fn find(shape: &egui::Shape, clip: egui::Rect, expected: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == expected => {
                let rect = text.visual_bounding_rect();
                clip.contains_rect(rect).then_some(rect)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, clip, expected)),
            _ => None,
        }
    }
    output.shapes.iter().find_map(|paint| find(&paint.shape, paint.clip_rect, expected))
}

/// Comment actions must fit narrow panes in both interface languages.
#[test]
fn comment_actions_fit_the_pane_and_resolve_only_the_clicked_thread() {
    /// Count matching painted labels and require their glyph bounds to fit the clip and viewport.
    fn painted_count(shape: &egui::Shape, clip: egui::Rect, viewport: egui::Rect, label: &str) -> usize {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                let rect = text.visual_bounding_rect();
                assert!(clip.contains_rect(rect), "{label}: {rect:?} outside clip {clip:?}");
                assert!(viewport.contains_rect(rect), "{label}: {rect:?} outside viewport {viewport:?}");
                1
            }
            egui::Shape::Vec(shapes) => shapes.iter().map(|shape| painted_count(shape, clip, viewport, label)).sum(),
            _ => 0,
        }
    }
    for language in ["en", "ja"] {
        if language == "ja" && !i18n::japanese_font_input_available() {
            continue;
        }
        for width in [240.0, UiState::default().pane_width] {
            let mut app = SlideApp::new(Session::with_new(), Services::default());
            app.run("app.language", json!({"language": language})).unwrap();
            app.ui.pane = Some("comments".into());
            app.ui.pane_width = width;
            let comments = vec![
                deckcraft_model::Comment {
                    author: "Reviewer A".into(),
                    initials: "A".into(),
                    text: "First {thread}".into(),
                    date: "2026-10-14T12:00:00Z".into(),
                    ..Default::default()
                },
                deckcraft_model::Comment {
                    author: "Reviewer B".into(),
                    initials: "B".into(),
                    text: "Second {thread}".into(),
                    date: "2026-10-14T13:00:00Z".into(),
                    resolved: true,
                    ..Default::default()
                },
            ];
            let doc = std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc);
            std::sync::Arc::make_mut(&mut doc.slides[0]).comments = comments.clone();
            let resolve = i18n::text(app.lang(), "pane.comments.resolve");
            let reopen = i18n::text(app.lang(), "pane.comments.reopen");
            let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
                |ui, app: &mut SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            harness.run_steps(3);
            let viewport = harness.ctx.viewport_rect();
            for (label, count) in
                [("Reviewer A", 1), ("Reviewer B", 1), ("First {thread}", 1), ("Second {thread}", 1), (resolve, 1), (reopen, 1), ("🗑", 2)]
            {
                assert!(painted_text_rect(harness.output(), label).is_some(), "{language} at {width} pt: {label} is clipped or absent");
                let painted =
                    harness.output().shapes.iter().map(|paint| painted_count(&paint.shape, paint.clip_rect, viewport, label)).sum::<usize>();
                assert_eq!(painted, count, "{language} at {width} pt: every {label} must be painted");
            }
            preview(&mut harness, &format!("{language}-comments-{width:.0}"));
            let pos = painted_text_rect(harness.output(), resolve).unwrap().center();
            harness.hover_at(pos);
            harness.drag_at(pos);
            harness.run_steps(1);
            harness.drop_at(pos);
            harness.run_steps(3);
            let mut expected = comments;
            expected[0].resolved = true;
            assert_eq!(
                harness.state().session.active().unwrap().doc.slides[0].comments,
                expected,
                "{language} at {width} pt: clicking Resolve changes only that thread's resolved state"
            );
        }
    }
}

/// Japanese renames the shell and the translated Home controls.
#[test]
fn japanese_renames_the_file_tab_and_preferences() {
    if !i18n::japanese_font_input_available() {
        return;
    }
    use egui_kittest::kittest::Queryable;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.run_steps(2);
    assert!(harness.query_by_label("File").is_some());
    preview(&mut harness, "en-home");
    harness.state_mut().run("app.language", json!({"language": "ja"})).unwrap();
    harness.run_steps(2);
    assert_eq!(harness.state().ui.language, i18n::LanguageChoice::Ja);
    assert!(harness.query_by_label("ファイル").is_some(), "the File tab follows the language");
    assert!(harness.query_by_label("挿入").is_some());
    // Custom ribbon widgets paint their caption directly; the text is not an AccessKit label.
    assert!(painted_text_rect(harness.output(), "貼り付け").is_some(), "Home paints the translated Paste caption");
    preview(&mut harness, "ja-home");
    // Bold's language-independent B glyph locates the real icon, without fixed coordinates.
    let bold = painted_text_rect(harness.output(), "B").expect("Home paints the Bold icon");
    let tooltip_delay = harness.ctx.global_style().interaction.tooltip_delay;
    harness.ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
    harness.hover_at(bold.center());
    harness.run_steps(3);
    let shortcut = deckcraft_engine::find_command("format.bold").unwrap().shortcut.unwrap();
    let tooltip = format!("{} ({})", i18n::text(i18n::Resolved::Ja, "ribbon.font.bold"), crate::ribbon::pretty_shortcut(shortcut));
    assert!(painted_text_rect(harness.output(), &tooltip).is_some(), "the actual Bold hover tooltip follows the language");
    harness.hover_at(egui::pos2(0.0, 0.0));
    harness.run_steps(2);
    harness.ctx.global_style_mut(|style| style.interaction.tooltip_delay = tooltip_delay);
    assert!(harness.query_by_label("File").is_none());
    harness.state_mut().run("app.preferences", json!({})).unwrap();
    harness.run_steps(2);
    assert!(harness.query_by_label_contains("表示言語").is_some());
    assert!(harness.query_by_label_contains("環境設定").is_some());
    preview(&mut harness, "ja-preferences");
    let err = harness.state_mut().run("app.language", json!({"language": "zh-Hans"})).unwrap_err();
    assert!(err.contains("ja"), "{err}");
    assert_eq!(harness.state().ui.language, i18n::LanguageChoice::Ja, "a rejected code leaves the choice");
    harness.state_mut().run("app.language", json!({"language": "en"})).unwrap();
    harness.run_steps(2);
    assert!(harness.query_by_label("File").is_some());
}

#[test]
/// Default older saved interface state to System and resolve Japanese system locales.
fn an_older_ui_state_without_a_language_stays_on_system() {
    let ui: UiState = serde_json::from_str(r#"{"tab":"Home"}"#).unwrap();
    assert_eq!(ui.language, i18n::LanguageChoice::System);
    assert_eq!(ui.language.resolve(Some("ja-JP")), i18n::Resolved::Ja);
}

/// Preferences cancellation must also restore the newly added language choice.
#[test]
fn preferences_cancel_restores_the_interface_language() {
    if !i18n::japanese_font_input_available() {
        return;
    }
    use egui_kittest::kittest::Queryable;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    harness.state_mut().run("app.language", json!({"language": "en"})).unwrap();
    harness.state_mut().run("app.preferences", json!({})).unwrap();
    harness.run_steps(3);
    harness.state_mut().run("app.language", json!({"language": "ja"})).unwrap();
    harness.run_steps(3);
    assert!(harness.query_by_label_contains("表示言語").is_some());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().dialog.is_none());
    assert_eq!(harness.state().ui.language, i18n::LanguageChoice::En);
    assert!(harness.query_by_label("File").is_some());
}

/// Rendering another interface language must preserve user-authored document and toolbar data.
#[test]
fn interface_language_preserves_document_and_custom_toolbar_labels() {
    if !i18n::japanese_font_input_available() {
        return;
    }
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("app.qat", json!({"add": "slide.new", "label": "My {literal} label"})).unwrap();
    let document = serde_json::to_value(&app.session.active().unwrap().doc).unwrap();
    let toolbar = serde_json::to_value(&app.ui.qat).unwrap();
    app.run("app.language", json!({"language": "ja"})).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    assert_eq!(serde_json::to_value(&harness.state().session.active().unwrap().doc).unwrap(), document);
    assert_eq!(serde_json::to_value(&harness.state().ui.qat).unwrap(), toolbar);
}
