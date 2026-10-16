#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use deckcraft_model::comments::CommentStatus;

fn add(s: &mut Session, text: &str) -> u64 {
    s.execute("comment.add", &json!({"text":text})).unwrap()["commentId"].as_u64().unwrap()
}
fn value(s: &mut Session, id: u64) -> Value {
    s.execute("comment.list", &json!({"commentId":id})).unwrap()
}
#[test]
fn edit_and_delete_stable_reply_after_removing_an_earlier_root() {
    let mut s = Session::with_new();
    let first = add(&mut s, "first");
    let root = add(&mut s, "second");
    let reply = s.execute("comment.reply", &json!({"commentId":root,"text":"reply"})).unwrap()["commentId"].as_u64().unwrap();
    s.execute("comment.delete", &json!({"commentId":first})).unwrap();
    s.execute("comment.edit", &json!({"commentId":reply,"text":"edited reply"})).unwrap();
    assert_eq!(value(&mut s, reply)["text"], "edited reply");
    s.execute("comment.delete", &json!({"commentId":reply})).unwrap();
    assert!(value(&mut s, root)["replies"].as_array().unwrap().is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(value(&mut s, reply)["text"], "edited reply");
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.execute("comment.list", &json!({"commentId":reply})).is_err());
}
#[test]
fn closed_resolved_reopen_and_reply_are_real_undoable_commands() {
    let mut s = Session::with_new();
    let root = add(&mut s, "thread");
    s.execute("comment.close", &json!({"commentId":root})).unwrap();
    assert_eq!(value(&mut s, root)["status"], "closed");
    assert!(s.execute("comment.reply", &json!({"commentId":root,"text":"blocked"})).is_err());
    let reply = s.execute("comment.reply", &json!({"commentId":root,"text":"posted","reopen":true})).unwrap()["commentId"].as_u64().unwrap();
    assert_eq!(value(&mut s, root)["status"], "active");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(value(&mut s, root)["status"], "closed");
    assert!(s.execute("comment.list", &json!({"commentId":reply})).is_err());
    s.execute("comment.reopen", &json!({"commentId":root})).unwrap();
    s.execute("comment.resolve", &json!({"commentId":root})).unwrap();
    assert_eq!(value(&mut s, root)["status"], "resolved");
    s.execute("comment.resolve", &json!({"commentId":root,"resolved":false})).unwrap();
    assert_eq!(value(&mut s, root)["status"], "active");
}
#[test]
fn nested_reply_targets_are_kept_and_other_threads_cannot_be_claimed() {
    let mut s = Session::with_new();
    let root = add(&mut s, "thread");
    let other = add(&mut s, "other");
    let reply = s.execute("comment.reply", &json!({"commentId":root,"text":"reply"})).unwrap()["commentId"].as_u64().unwrap();
    let deep = s.execute("comment.reply", &json!({"commentId":root,"replyTo":reply,"text":"deep"})).unwrap()["commentId"].as_u64().unwrap();
    assert_eq!(value(&mut s, reply)["replies"][0]["commentId"], deep);
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("comment.reply", &json!({"commentId":other,"replyTo":reply,"text":"wrong owner"})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
}
#[test]
fn file_attachment_uses_actual_bytes_and_undo_removes_only_the_reference() {
    let mut s = Session::with_new();
    let root = add(&mut s, "thread");
    let media = s.edit_resources(|doc, _| Ok(doc.add_media("original.txt", "text/plain", b"contributor original".to_vec()).unwrap())).unwrap();
    let attachment = s.execute("comment.attachment.add", &json!({"commentId":root,"mediaId":media.0})).unwrap()["attachmentId"].as_u64().unwrap();
    assert_eq!(value(&mut s, root)["attachments"][0]["media"], media.0);
    s.execute("comment.attachment.remove", &json!({"commentId":root,"attachmentId":attachment})).unwrap();
    assert!(value(&mut s, root).get("attachments").is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(value(&mut s, root)["attachments"][0]["id"], attachment);
    assert_eq!(s.doc().unwrap().doc.media(media).unwrap().data.as_slice(), b"contributor original");
}
#[test]
fn supplied_author_relationship_reuses_identity_without_name_matching() {
    let mut s = Session::with_new();
    let root = add(&mut s, "first");
    let identity = value(&mut s, root)["authorIdentity"]["id"].as_u64().unwrap();
    let separate = add(&mut s, "second");
    assert_ne!(value(&mut s, separate)["authorIdentity"]["id"], identity);
    let reply = s.execute("comment.reply", &json!({"commentId":root,"text":"same explicit identity","authorId":identity})).unwrap()["commentId"]
        .as_u64()
        .unwrap();
    assert_eq!(value(&mut s, reply)["authorIdentity"]["id"], identity);
}
#[test]
fn failed_hostile_command_leaves_doc_and_history_unchanged() {
    let mut s = Session::with_new();
    let root = add(&mut s, "thread");
    let before = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    for params in [json!({"commentId":root,"shape":u64::MAX}), json!({"commentId":root,"shape":999999})] {
        assert!(s.execute("comment.attach", &params).is_err());
    }
    assert!(s.execute("comment.edit", &json!({"commentId":root,"text":"x".repeat(comments::MAX_TEXT+1)})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo);
}
#[test]
fn old_resolved_bool_remains_synchronized_with_closed_after_native_json_reopen() {
    let mut s = Session::with_new();
    let root = add(&mut s, "thread");
    s.execute("comment.close", &json!({"commentId":root})).unwrap();
    let p: Presentation = serde_json::from_slice(&serde_json::to_vec(s.doc().unwrap().doc.as_ref()).unwrap()).unwrap();
    let c = &p.slides[0].comments[0];
    assert_eq!(c.comment_status(), CommentStatus::Closed);
    assert!(c.resolved);
    assert_eq!(c.comment_id, Some(root as u32));
}

#[test]
fn body_edit_keeps_unchanged_rich_spans_and_undo_restores_the_exact_body() {
    let mut s = Session::with_new();
    let mut body = deckcraft_model::text::TextBody::from_text("bold text");
    body.paragraphs[0].runs[0].props.bold = Some(true);
    let root = s.execute("comment.add", &json!({"body":body})).unwrap()["commentId"].as_u64().unwrap();
    assert_eq!(value(&mut s, root)["text"], "bold text");
    s.execute("comment.edit", &json!({"commentId":root,"text":"bold text!"})).unwrap();
    let changed = value(&mut s, root);
    assert_eq!(changed["body"]["paragraphs"][0]["runs"][0]["props"]["bold"], true);
    s.execute("edit.undo", &json!({})).unwrap();
    let restored: Comment = serde_json::from_value(value(&mut s, root)).unwrap();
    assert_eq!(restored.body, Some(body));
}
#[test]
fn typed_scalar_anchor_attach_detach_and_undo_are_actual_commands() {
    let mut s = Session::with_new();
    let id = s
        .edit_resources(|doc, sel| {
            let id = ShapeId(doc.alloc_id().unwrap());
            Arc::make_mut(&mut doc.slides[sel.slide]).shapes.push(deckcraft_model::Shape {
                id,
                text: Some(deckcraft_model::text::TextBody::from_text("Aé🦀Z")),
                ..Default::default()
            });
            Ok(id)
        })
        .unwrap();
    let root = add(&mut s, "range");
    s.execute("comment.attach", &json!({"commentId":root,"anchor":{"kind":"textRange","shape":id.0,"start":1,"end":3}})).unwrap();
    assert_eq!(value(&mut s, root)["anchor"]["kind"], "textRange");
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("comment.attach", &json!({"commentId":root,"anchor":{"kind":"textRange","shape":id.0,"start":1,"end":5}})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
    s.execute("comment.detach", &json!({"commentId":root})).unwrap();
    assert_eq!(value(&mut s, root)["anchor"]["kind"], "slide");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(value(&mut s, root)["anchor"]["kind"], "textRange");
}

#[test]
fn actual_text_edits_transform_scalar_ranges_and_shape_deletion_detaches_with_undo() {
    let mut s = Session::with_new();
    let result = s.execute("insert.textBox", &json!({"text":"a😀bcdef","rect":[20,20,200,60]})).unwrap();
    let shape = result["id"].clone();
    s.execute("comment.add", &json!({"text":"range","anchor":{"kind":"textRange","shape":shape,"start":2,"end":5}})).unwrap();
    let id = s.doc().unwrap().doc.slides[0].comments[0].comment_id.unwrap();
    s.execute("text.edit", &json!({"id":shape,"at":[0,0]})).unwrap();
    s.execute("text.insert", &json!({"text":"++"})).unwrap();
    let c = &s.doc().unwrap().doc.slides[0].comments[0];
    assert!(matches!(c.comment_anchor(), CommentAnchor::TextRange { start: 4, end: 7, .. }));
    s.execute("edit.delete", &json!({"ids":[shape]})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides[0].comments[0].comment_anchor(), CommentAnchor::Slide);
    s.execute("edit.undo", &json!({})).unwrap();
    let c = &s.doc().unwrap().doc.slides[0].comments[0];
    assert_eq!(c.comment_id, Some(id));
    assert!(matches!(c.comment_anchor(), CommentAnchor::TextRange { start: 4, end: 7, .. }));
}

#[test]
fn attachment_bytes_task_edits_and_wire_identity_are_real_undoable_commands() {
    let mut s = Session::with_new();
    let id = add(&mut s, "thread");
    let guid = value(&mut s, id)["wireGuid"].clone();
    assert!(guid.as_str().is_some());
    let result = s
        .execute(
            "comment.attachment.import",
            &json!({"commentId":id,"name":"original.txt","contentType":"text/plain","data":crate::cmd::base64_encode(b"owned bytes")}),
        )
        .unwrap();
    let attachment = result["attachmentId"].clone();
    let fetched = s.execute("comment.attachment.get", &json!({"commentId":id,"attachmentId":attachment})).unwrap();
    assert_eq!(fetched["name"], "original.txt");
    assert_eq!(crate::cmd::base64_decode(fetched["data"].as_str().unwrap()).unwrap(), b"owned bytes");
    s.execute("comment.task.set", &json!({"commentId":id,"task":{"title":"Check layout","dueDate":"2026-11-03T12:00:00Z"}})).unwrap();
    assert_eq!(value(&mut s, id)["task"]["title"], "Check layout");
    assert_eq!(value(&mut s, id)["wireGuid"], guid);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(value(&mut s, id).get("task").is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.execute("comment.attachment.get", &json!({"commentId":id,"attachmentId":attachment})).is_err());
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.execute("comment.attachment.get", &json!({"commentId":id,"attachmentId":attachment})).is_ok());
}
