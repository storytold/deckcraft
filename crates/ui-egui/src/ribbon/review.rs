use super::{Icon, SlideApp, Ui, Value, big, big_button, big_button_ex, enabled, group, json, rows, run, small, small_button, tip, vec2};

fn comment_nav(app: &mut SlideApp, ui: &mut Ui, forward: bool) {
    let (icon, label, id) = if forward { (Icon::NextComment, "Next", "comment.next") } else { (Icon::PrevComment, "Previous", "comment.prev") };
    if small_button(ui, icon, label, enabled(app, id), false).on_hover_text(tip(id, label)).clicked() {
        let params = match app.ui.comment_focus {
            Some((slide, index)) if app.session.active().is_some_and(|d| d.selection.slide == slide) => json!({"slide": slide, "index": index}),
            _ => json!({}),
        };
        if let Ok(v) = app.run(id, params)
            && let (Some(slide), Some(index)) = (v.get("slide").and_then(Value::as_u64), v.get("index").and_then(Value::as_u64))
            && let (Ok(slide), Ok(index)) = (usize::try_from(slide), usize::try_from(index))
        {
            app.ui.comment_focus = Some((slide, index));
            app.ui.pane = Some("comments".into());
        }
    }
}

pub(super) fn review(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, "Proofing", |ui| {
        if crate::ribbon_fit::group_level(ui.ctx()) == crate::ribbon_fit::STACKED {
            // Measured two rows between 819 and 880 points: one proofing command per row.
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
                small(app, ui, Icon::Spelling, "Spelling", "app.dialog", json!({"id": "spelling"}));
                small(app, ui, Icon::Thesaurus, "Thesaurus", "review.thesaurusPane", json!({}));
            });
        } else {
            big(app, ui, Icon::Spelling, "Spelling", "app.dialog", json!({"id": "spelling"}));
            big(app, ui, Icon::Thesaurus, "Thesaurus", "review.thesaurusPane", json!({}));
        }
    });
    group(ui, "Accessibility", |ui| {
        big(app, ui, Icon::Accessibility, "Check\nAccessibility", "app.dialog", json!({"id": "accessibility"}));
    });
    group(ui, "Language", |ui| {
        let r = big_button(ui, Icon::Translate, "Translate", false);
        let _ = r.on_hover_text("Translation needs a language model (not bundled)");
        big(app, ui, Icon::Language, "Language", "app.dialog", json!({"id": "language"}));
    });
    group(ui, "Compare", |ui| {
        // PowerPoint calls this measured section Activity; DeckCraft's activity tracking is
        // Compare/Accept/End Review. It uses the same two-row band as Proofing.
        if crate::ribbon_fit::group_level(ui.ctx()) == crate::ribbon_fit::STACKED {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
                ui.horizontal(|ui| {
                    small(app, ui, Icon::Compare, "Compare", "app.compareDialog", json!({}));
                    let comparing = enabled(app, "review.endCompare");
                    if small_button(ui, Icon::Check, "Accept All", comparing, false).clicked() {
                        run(app, "review.accept", json!({"all": true}));
                    }
                });
                ui.horizontal(|ui| {
                    let comparing = enabled(app, "review.endCompare");
                    if small_button(ui, Icon::Close, "End Review", comparing, false).clicked() {
                        run(app, "review.endCompare", json!({}));
                        if app.ui.pane.as_deref() == Some("revisions") {
                            app.ui.pane = None;
                        }
                    }
                });
            });
        } else {
            big(app, ui, Icon::Compare, "Compare", "app.compareDialog", json!({}));
            let comparing = enabled(app, "review.endCompare");
            if big_button(ui, Icon::Check, "Accept\nAll", comparing).clicked() {
                run(app, "review.accept", json!({"all": true}));
            }
            if big_button(ui, Icon::Close, "End\nReview", comparing).clicked() {
                run(app, "review.endCompare", json!({}));
                if app.ui.pane.as_deref() == Some("revisions") {
                    app.ui.pane = None;
                }
            }
        }
    });
    group(ui, "Comments", |ui| {
        big(app, ui, Icon::NewComment, "New\nComment", "app.dialog", json!({"id": "comment"}));
        rows(ui, |ui| {
            comment_nav(app, ui, false);
            comment_nav(app, ui, true);
        });
        big(app, ui, Icon::DeleteComment, "Delete", "comment.delete", json!({}));
        let show = app.ui.pane.as_deref() == Some("comments");
        if big_button_ex(ui, Icon::ShowComments, "Show\nComments", true, false, show).0.clicked() {
            let _ = app.run("view.pane", json!({"pane": "comments", "toggle": true}));
        }
    });
}
