use super::{Icon, SlideApp, Ui, Value, big, big_button, big_button_ex, enabled, group, json, rows, run, small_button, tip};

fn comment_nav(app: &mut SlideApp, ui: &mut Ui, forward: bool) {
    let (icon, label, id) = if forward { (Icon::NextComment, "다음", "comment.next") } else { (Icon::PrevComment, "이전", "comment.prev") };
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
    group(ui, |ui| {
        big(app, ui, Icon::Spelling, "맞춤법 검사", "app.dialog", json!({"id": "spelling"}));
        big(app, ui, Icon::Thesaurus, "동의어 사전", "review.thesaurusPane", json!({}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::Accessibility, "접근성\n검사", "app.dialog", json!({"id": "accessibility"}));
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Translate, "번역", false);
        let _ = r.on_hover_text("번역에는 언어 모델이 필요합니다 (미포함)");
        big(app, ui, Icon::Language, "언어", "app.dialog", json!({"id": "language"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::Compare, "비교", "app.compareDialog", json!({}));
        let comparing = enabled(app, "review.endCompare");
        if big_button(ui, Icon::Check, "모두 적용", comparing).clicked() {
            run(app, "review.accept", json!({"all": true}));
        }
        if big_button(ui, Icon::Close, "검토\n종료", comparing).clicked() {
            run(app, "review.endCompare", json!({}));
            if app.ui.pane.as_deref() == Some("revisions") {
                app.ui.pane = None;
            }
        }
    });
    group(ui, |ui| {
        big(app, ui, Icon::NewComment, "새 메모", "app.dialog", json!({"id": "comment"}));
        rows(ui, |ui| {
            comment_nav(app, ui, false);
            comment_nav(app, ui, true);
        });
        big(app, ui, Icon::DeleteComment, "삭제", "comment.delete", json!({}));
        let show = app.ui.pane.as_deref() == Some("comments");
        if big_button_ex(ui, Icon::ShowComments, "메모\n표시", true, false, show).0.clicked() {
            let _ = app.run("view.pane", json!({"pane": "comments", "toggle": true}));
        }
    });
}
