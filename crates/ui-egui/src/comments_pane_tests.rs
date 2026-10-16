//! Actual rich-comment controls and captured asynchronous attachment targets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::sync::{Arc, Mutex};

use crate::{Inbox, InboxEvent, InboxFile, Services, SlideApp};
use deckcraft_engine::Session;
use deckcraft_model::{
    Comment, ShapeId, TextBody,
    comments::{self, AuthorOrigin, CommentAnchor, CommentAuthorIdentity, CommentStatus, CommentTask, CommentTaskAuthor},
    text::{Action, Hyperlink, Run, RunProps},
};
use egui::{
    accesskit::Role,
    text::{CCursor, CCursorRange},
};
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

fn add(session: &mut Session, text: &str) -> u32 {
    session.execute("comment.add", &json!({"text":text,"anchor":{"kind":"slide"}})).unwrap()["commentId"].as_u64().unwrap() as u32
}
fn harness(session: Session, services: Services) -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(860., 1500.)).with_step_dt(1. / 60.).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            egui::ScrollArea::vertical().show(ui, |ui| crate::comments_pane::show(app, ui));
            if let Some((message, _)) = &app.status {
                ui.label(message);
            }
        },
        SlideApp::new(session, services),
    );
    h.run_steps(3);
    h
}
fn comment(h: &Harness<'_, SlideApp>, uid: u64, id: u32) -> Comment {
    let doc = &h.state().session.documents().iter().find(|doc| doc.uid == uid).unwrap().doc;
    doc.slides.iter().find_map(|slide| comments::path(&slide.comments, id).and_then(|path| comments::at(&slide.comments, &path))).unwrap().clone()
}
fn click(h: &mut Harness<'_, SlideApp>, label: &str, index: usize) {
    h.query_all_by_label(label).nth(index).unwrap_or_else(|| panic!("missing {label} #{index}")).scroll_to_me();
    h.run_steps(2);
    h.query_all_by_label(label).nth(index).unwrap().click_accesskit();
    h.run_steps(3);
}
fn input(h: &mut Harness<'_, SlideApp>, role: Role, label: &str, index: usize, text: &str) {
    h.query_all_by_role_and_label(role, label).nth(index).unwrap().focus();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.query_all_by_role_and_label(role, label).nth(index).unwrap().type_text(text);
    h.run_steps(2);
}
fn rich_body() -> TextBody {
    let mut body = TextBody::from_text("Aé🦀Z\nsecond paragraph");
    body.body.inset_l = Some(9.25);
    body.paragraphs[0].props.margin_left = Some(17.5);
    body.paragraphs[0].runs = vec![Run::with(
        "Aé🦀Z",
        RunProps {
            bold: Some(false),
            italic: Some(false),
            underline: Some("none".into()),
            size: Some(14.5),
            link: Some(Hyperlink {
                action: Action::Url { url: "https://example.invalid/original".into() },
                tooltip: "retained link".into(),
                highlight_click: false,
                sound: None,
                stop_sound: false,
                zoom_return: None,
                sound_name: None,
            }),
            ..Default::default()
        },
    )];
    body.paragraphs[1].props.margin_left = Some(24.75);
    body.paragraphs[1].runs[0].props.italic = Some(true);
    body
}
fn rich_harness() -> (Harness<'static, SlideApp>, u64, u32, TextBody) {
    let mut session = Session::with_new();
    let body = rich_body();
    let id = session.execute("comment.add", &json!({"body":body,"anchor":{"kind":"slide"}})).unwrap()["commentId"].as_u64().unwrap() as u32;
    let uid = session.active().unwrap().uid;
    (harness(session, Services::default()), uid, id, body)
}

#[test]
fn rich_text_typing_preserves_untouched_link_paragraph_and_run_metadata_then_undoes() {
    let (mut h, uid, id, body) = rich_harness();
    click(&mut h, "Edit message", 0);
    let before = h.state().session.active().unwrap().doc.clone();
    let undo = h.state().session.active().unwrap().history.undo.len();
    input(&mut h, Role::MultilineTextInput, "Message text", 0, "Aé🦀Z\nsecond paragraph!");
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before), "typing remains a draft");
    click(&mut h, "Save changes", 0);
    let changed = comment(&h, uid, id);
    let edited = changed.body.unwrap();
    assert_eq!(changed.text, "Aé🦀Z\nsecond paragraph!");
    assert_eq!(edited.body, body.body);
    assert_eq!(edited.paragraphs[0], body.paragraphs[0]);
    assert_eq!(edited.paragraphs[1].props, body.paragraphs[1].props);
    assert_eq!(edited.paragraphs[1].runs[0].props, body.paragraphs[1].runs[0].props);
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo + 1);
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(comment(&h, uid, id).body, Some(body));
}

#[test]
fn rich_selected_scalar_range_bold_italic_underline_changes_only_that_range() {
    let (mut h, uid, id, body) = rich_harness();
    click(&mut h, "Edit message", 0);
    h.get_by_role_and_label(Role::MultilineTextInput, "Message text").focus();
    h.run_steps(1);
    let editor_id = egui::Id::new(("comment-rich-editor", uid, id));
    let mut state = egui::TextEdit::load_state(&h.ctx, editor_id).unwrap();
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(1), CCursor::new(3))));
    state.store(&h.ctx, editor_id);
    h.run_steps(2);
    for label in ["Bold", "Italic", "Underline"] {
        click(&mut h, label, 0);
    }
    if let Ok(path) = std::env::var("DECKCRAFT_RICH_COMMENT_QA_PNG") {
        h.render().unwrap().save(path).unwrap();
    }
    click(&mut h, "Save changes", 0);
    let result = comment(&h, uid, id).body.unwrap();
    assert_eq!(result.text(), body.text());
    assert_eq!(result.paragraphs[1], body.paragraphs[1]);
    assert_eq!(result.paragraphs[0].props, body.paragraphs[0].props);
    let runs = &result.paragraphs[0].runs;
    assert_eq!(runs.iter().map(|run| run.text.as_str()).collect::<Vec<_>>(), vec!["A", "é🦀", "Z"]);
    assert_eq!(runs[0].props, body.paragraphs[0].runs[0].props);
    assert_eq!(runs[2].props, body.paragraphs[0].runs[0].props);
    assert_eq!(runs[1].props.bold, Some(true));
    assert_eq!(runs[1].props.italic, Some(true));
    assert_eq!(runs[1].props.underline.as_deref(), Some("sng"));
    assert_eq!(runs[1].props.link, body.paragraphs[0].runs[0].props.link);
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(comment(&h, uid, id).body, Some(body));
}

#[test]
fn rich_cancel_and_external_edit_guard_do_not_publish_or_overwrite_a_draft() {
    let (mut h, uid, id, body) = rich_harness();
    click(&mut h, "Edit message", 0);
    let undo = h.state().session.active().unwrap().history.undo.len();
    input(&mut h, Role::MultilineTextInput, "Message text", 0, "unapplied draft");
    click(&mut h, "Cancel edit", 0);
    assert_eq!(comment(&h, uid, id).body, Some(body.clone()));
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo);
    input(&mut h, Role::MultilineTextInput, "Message text", 0, "another draft");
    h.state_mut().run("comment.edit", json!({"commentId":id,"text":"external replacement"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label_contains("This message changed while you were editing").is_some());
    assert_eq!(comment(&h, uid, id).text, "external replacement");
    click(&mut h, "Cancel edit", 0);
    assert_eq!(h.get_by_role_and_label(Role::MultilineTextInput, "Message text").value().as_deref(), Some("external replacement"));
}

#[test]
fn nested_reply_post_reopens_thread_and_undo_restores_closed_state_atomically() {
    let mut session = Session::with_new();
    let root = add(&mut session, "root thread");
    let reply = session.execute("comment.reply", &json!({"commentId":root,"text":"first reply"})).unwrap()["commentId"].as_u64().unwrap() as u32;
    let uid = session.active().unwrap().uid;
    let mut h = harness(session, Services::default());
    click(&mut h, "Close", 0);
    assert_eq!(comment(&h, uid, root).comment_status(), CommentStatus::Closed);
    input(&mut h, Role::MultilineTextInput, "Reply text", 1, "nested actual reply");
    click(&mut h, "Reopen and reply", 1);
    assert_eq!(comment(&h, uid, root).comment_status(), CommentStatus::Active);
    assert_eq!(comment(&h, uid, reply).replies.len(), 1);
    assert_eq!(comment(&h, uid, reply).replies[0].text, "nested actual reply");
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(comment(&h, uid, root).comment_status(), CommentStatus::Closed);
    assert!(comment(&h, uid, reply).replies.is_empty());
    h.run_steps(2);
    click(&mut h, "Reopen", 0);
    click(&mut h, "Resolve", 0);
    assert_eq!(comment(&h, uid, root).comment_status(), CommentStatus::Resolved);
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(comment(&h, uid, root).comment_status(), CommentStatus::Active);
}

#[test]
fn task_assignment_uses_known_identities_and_copies_exact_task_only_metadata() {
    let mut session = Session::with_new();
    session.prefs.author = "Alice".into();
    let root = add(&mut session, "assignment thread");
    session.prefs.author = "Bob".into();
    let other = add(&mut session, "other thread");
    let bob = session.active().unwrap().doc.slides[0].comments[1].author_identity.clone().unwrap();
    let retained = CommentTaskAuthor {
        identity: CommentAuthorIdentity {
            id: 700000,
            origin: AuthorOrigin::Imported,
            external_id: Some("exact-original".into()),
            user_id: Some("retained-user".into()),
            provider_id: Some("provider".into()),
            source_initials: Some("RP".into()),
            source_scope: Some("source owner".into()),
            wire_guid: None,
        },
        name: "Retained participant".into(),
        initials: "RP".into(),
    };
    session
        .execute("comment.task.set", &json!({"commentId":other,"task":CommentTask {authors:vec![retained.clone()],..Default::default()}}))
        .unwrap();
    let original = CommentTask { title: Some(String::new()), complete: Some("0.00%".into()), ..Default::default() };
    session.execute("comment.task.set", &json!({"commentId":root,"task":original})).unwrap();
    let uid = session.active().unwrap().uid;
    let mut h = harness(session, Services::default());
    click(&mut h, "Task", 0);
    h.get_by_role_and_label(Role::CheckBox, &format!("Bob · #{}", bob.id)).click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Retained participant · #700000").click();
    h.run_steps(2);
    assert!(comment(&h, uid, root).task.as_ref().unwrap().assigned_to.is_empty(), "assignment remains a draft");
    click(&mut h, "Save task", 0);
    let task = comment(&h, uid, root).task.unwrap();
    assert_eq!(task.assigned_to, vec![bob.id, retained.identity.id]);
    assert_eq!(task.authors, vec![retained]);
    assert_eq!(task.title, Some(String::new()));
    assert_eq!(task.complete.as_deref(), Some("0.00%"));
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(comment(&h, uid, root).task, Some(original));
}

#[test]
fn assignee_search_and_pagination_bound_visible_identity_controls() {
    let mut session = Session::with_new();
    for n in 0..25 {
        session.prefs.author = format!("Participant {n:02}");
        add(&mut session, &format!("message {n}"));
    }
    let uid = session.active().unwrap().uid;
    let first = session.active().unwrap().doc.slides[0].comments[0].comment_id.unwrap();
    let selected = session.active().unwrap().doc.slides[0].comments[24].author_identity.as_ref().unwrap().id;
    let mut h = harness(session, Services::default());
    click(&mut h, "Task", 0);
    assert_eq!(h.query_all_by_role(Role::CheckBox).count(), 20);
    assert!(h.query_by_label("Assignees 1 / 2").is_some());
    click(&mut h, "Next assignees", 0);
    assert_eq!(h.query_all_by_role(Role::CheckBox).count(), 5);
    input(&mut h, Role::TextInput, "Find assignee", 0, "Participant 24");
    assert_eq!(h.query_all_by_role(Role::CheckBox).count(), 1);
    assert!(h.query_by_label("Assignees 1 / 1").is_some());
    h.get_by_role_and_label(Role::CheckBox, &format!("Participant 24 · #{selected}")).click();
    h.run_steps(2);
    click(&mut h, "Save task", 0);
    assert_eq!(comment(&h, uid, first).task.unwrap().assigned_to, vec![selected]);
}

#[test]
fn selected_slide_text_range_uses_unicode_scalars_and_rejects_cell_or_notes_ranges() {
    let mut session = Session::with_new();
    let shape = session.execute("insert.textBox", &json!({"rect":[10,10,200,70],"text":"Aé🦀Z\nnext"})).unwrap()["id"].as_u64().unwrap();
    let root = add(&mut session, "range thread");
    session.execute("text.edit", &json!({"id":shape})).unwrap();
    session.execute("text.select", &json!({"anchor":[0,1],"caret":[1,2]})).unwrap();
    let uid = session.active().unwrap().uid;
    let mut h = harness(session, Services::default());
    click(&mut h, "Anchor and attachments", 0);
    click(&mut h, "Attach to selected text range", 0);
    assert_eq!(comment(&h, uid, root).comment_anchor(), CommentAnchor::TextRange { shape: ShapeId(shape as u32), start: 1, end: 7 });
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    let before = h.state().session.active().unwrap().doc.clone();
    for notes in [false, true] {
        h.state_mut()
            .session
            .select(|_, selection| {
                let text = selection.text.as_mut().unwrap();
                text.notes = notes;
                text.cell = (!notes).then_some((0, 0));
            })
            .unwrap();
        click(&mut h, "Attach to selected text range", 0);
        assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before));
        assert!(h.state().status.as_ref().unwrap().0.contains("Table cells, notes and masters"));
    }
}

#[test]
fn stable_next_previous_focus_crosses_comment_pages_and_manual_paging_remains_usable() {
    let mut session = Session::with_new();
    let ids: Vec<_> = (0..101).map(|n| add(&mut session, &format!("Thread {n:03}"))).collect();
    let mut h = harness(session, Services::default());
    h.state_mut().run("comment.next", json!({"commentId":ids[99]})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Comments 2 / 2").is_some());
    assert!(h.query_by_label("Thread 100").is_some());
    assert!(h.query_by_label("Thread 000").is_none());
    click(&mut h, "Previous comments", 0);
    assert!(h.query_by_label("Comments 1 / 2").is_some());
    h.run_steps(2);
    assert!(h.query_by_label("Comments 1 / 2").is_some());
    h.state_mut().run("comment.prev", json!({"commentId":ids[100]})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Thread 099").is_some());
}

#[test]
fn rich_draft_is_keyed_to_document_uid_when_message_ids_repeat() {
    let (mut h, uid, id, _) = rich_harness();
    click(&mut h, "Edit message", 0);
    input(&mut h, Role::MultilineTextInput, "Message text", 0, "original pending draft");
    h.state_mut().session.execute("file.new", &json!({})).unwrap();
    let other = add(&mut h.state_mut().session, "other presentation message");
    let other_uid = h.state().session.active().unwrap().uid;
    assert_eq!(other, id, "fixture repeats document-local message identities");
    h.run_steps(2);
    click(&mut h, "Edit message", 0);
    assert_eq!(h.get_by_role_and_label(Role::MultilineTextInput, "Message text").value().as_deref(), Some("other presentation message"));
    assert_eq!(comment(&h, other_uid, other).text, "other presentation message");
    h.state_mut().session.set_active(0);
    h.run_steps(2);
    assert_eq!(h.get_by_role_and_label(Role::MultilineTextInput, "Message text").value().as_deref(), Some("original pending draft"));
    assert_ne!(comment(&h, uid, id).text, "original pending draft");
}

type Picks = Arc<Mutex<Vec<u64>>>;
type Downloads = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
fn attachment_harness() -> (Harness<'static, SlideApp>, Inbox, Picks, Downloads, u64, u32) {
    let mut session = Session::with_new();
    let id = add(&mut session, "attachment thread");
    let uid = session.active().unwrap().uid;
    let inbox = Inbox::default();
    let picks: Picks = Default::default();
    let request = picks.clone();
    let downloads: Downloads = Default::default();
    let downloaded = downloads.clone();
    let services = Services {
        inbox: Some(inbox.clone()),
        open_async: Some(Box::new(move |_, id| request.lock().unwrap().push(id))),
        download: Some(Box::new(move |name, bytes| downloaded.lock().unwrap().push((name.into(), bytes.into())))),
        ..Default::default()
    };
    (harness(session, services), inbox, picks, downloads, uid, id)
}
fn deliver(inbox: &Inbox, id: u64) {
    inbox.lock().unwrap().push(InboxEvent::File(InboxFile { id, name: "original.txt".into(), bytes: b"contributor original bytes".to_vec() }));
}

#[test]
fn late_attachment_file_keeps_its_original_document_then_download_remove_and_undo_work() {
    let (mut h, inbox, picks, downloads, uid, id) = attachment_harness();
    click(&mut h, "Anchor and attachments", 0);
    click(&mut h, "Attach file…", 0);
    let request = *picks.lock().unwrap().last().unwrap();
    h.state_mut().session.execute("file.new", &json!({})).unwrap();
    let other_uid = h.state().session.active().unwrap().uid;
    let other = add(&mut h.state_mut().session, "same local id in other document");
    assert_eq!(other, id);
    deliver(&inbox, request);
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().uid, other_uid);
    assert!(comment(&h, other_uid, other).attachments.is_empty());
    assert_eq!(comment(&h, uid, id).attachments.len(), 1);
    h.state_mut().session.set_active(0);
    h.run_steps(3);
    click(&mut h, "Save file…", 0);
    assert_eq!(downloads.lock().unwrap().as_slice(), &[("original.txt".into(), b"contributor original bytes".to_vec())]);
    click(&mut h, "Remove", 0);
    assert!(comment(&h, uid, id).attachments.is_empty());
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(comment(&h, uid, id).attachments.len(), 1);
}

#[test]
fn file_callback_for_deleted_comment_does_not_allocate_media_or_history() {
    let (mut h, inbox, picks, _, _, id) = attachment_harness();
    click(&mut h, "Anchor and attachments", 0);
    click(&mut h, "Attach file…", 0);
    let request = *picks.lock().unwrap().last().unwrap();
    h.state_mut().run("comment.delete", json!({"commentId":id})).unwrap();
    let before = h.state().session.active().unwrap().doc.clone();
    let undo = h.state().session.active().unwrap().history.undo.len();
    deliver(&inbox, request);
    h.run_steps(3);
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before));
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo);
    assert!(h.state().status.as_ref().unwrap().0.contains("no such comment ID"));
}
