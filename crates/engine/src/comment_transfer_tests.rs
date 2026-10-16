#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::Session;
use deckcraft_model::{
    Comment, Presentation,
    comments::{self, CommentAnchor, CommentStatus},
    text::{Action, Hyperlink, TextBody},
};
use serde_json::json;
use std::sync::Arc;
fn fixture() -> Session {
    let mut s = Session::with_new();
    let shape = s.execute("insert.textBox", &json!({"text":"a😀bcde","rect":[20,20,200,60]})).unwrap()["id"].clone();
    let mut body = TextBody::from_text("original rich comment");
    body.paragraphs[0].runs[0].props.bold = Some(true);
    let root =
        s.execute("comment.add", &json!({"body":body,"anchor":{"kind":"textRange","shape":shape,"start":1,"end":4}})).unwrap()["commentId"].clone();
    let identity = s.execute("comment.list", &json!({"commentId":root})).unwrap()["authorIdentity"]["id"].clone();
    let reply = s.execute("comment.reply", &json!({"commentId":root,"text":"reply","authorId":identity})).unwrap()["commentId"].clone();
    s.execute("comment.reply", &json!({"commentId":root,"replyTo":reply,"text":"deep","authorId":identity})).unwrap();
    s.execute("comment.attachment.import", &json!({"commentId":root,"name":"own.bin","contentType":"application/octet-stream","data":"AQMDBwAq"}))
        .unwrap();
    s.execute("comment.task.set", &json!({"commentId":root,"task":{"assignedTo":[identity],"title":"original task","complete":"37.5%"}})).unwrap();
    s.execute("comment.close", &json!({"commentId":root})).unwrap();
    s
}
fn root(p: &Presentation, index: usize) -> &Comment {
    &p.slides[index].comments[0]
}
fn assert_copy(source: &Presentation, copied: &Presentation, index: usize) {
    let a = root(source, 0);
    let b = root(copied, index);
    assert_ne!(a.comment_id, b.comment_id);
    assert_ne!(a.wire_guid, b.wire_guid);
    assert_ne!(a.author_identity.as_ref().unwrap().id, b.author_identity.as_ref().unwrap().id);
    assert_ne!(a.author_identity.as_ref().unwrap().wire_guid, b.author_identity.as_ref().unwrap().wire_guid);
    assert_eq!(b.comment_status(), CommentStatus::Closed);
    assert_eq!(a.body, b.body);
    assert_eq!(a.date, b.date);
    assert_eq!(a.display_date, b.display_date);
    assert_eq!(b.replies[0].replies[0].text, "deep");
    assert_eq!(b.author_identity, b.replies[0].author_identity);
    assert_eq!(b.author_identity, b.replies[0].replies[0].author_identity);
    assert_eq!(b.task.as_ref().unwrap().assigned_to, [b.author_identity.as_ref().unwrap().id]);
    assert_eq!(b.task.as_ref().unwrap().complete, Some("37.5%".into()));
    let owner = match b.comment_anchor() {
        CommentAnchor::TextRange { shape, start: 1, end: 4 } => shape,
        other => panic!("{other:?}"),
    };
    assert!(copied.slides[index].shapes.iter().any(|s| s.id == owner && s.text.as_ref().is_some_and(|t| t.text() == "a😀bcde")));
    assert_eq!(copied.media(b.attachments[0].media).unwrap().data.as_slice(), [1, 3, 3, 7, 0, 42]);
    comments::validate(copied, true).unwrap();
}
#[test]
fn duplicate_preserves_full_thread_with_fresh_ids_and_exact_undo_redo() {
    let mut s = fixture();
    let before = s.doc().unwrap().doc.clone();
    s.execute("slide.duplicate", &json!({"index":0})).unwrap();
    assert_copy(&before, &s.doc().unwrap().doc, 1);
    let copied = s.doc().unwrap().doc.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, copied);
}
#[test]
fn clipboard_cross_document_preserves_full_comment_resources_and_undo() {
    let mut s = fixture();
    let source = s.doc().unwrap().doc.clone();
    s.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("edit.paste", &json!({})).unwrap();
    assert_copy(&source, &s.doc().unwrap().doc, 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
}
#[test]
fn reuse_preserves_full_comment_resources_and_undo() {
    let source = fixture().doc().unwrap().doc.clone();
    let bytes = deckcraft_format::save(&source).unwrap();
    let mut s = Session::with_new();
    let before = s.doc().unwrap().doc.clone();
    s.execute("slide.reuse", &json!({"name":"own.deckcraft","data":crate::cmd::base64_encode(&bytes),"slides":[0]})).unwrap();
    assert_copy(&source, &s.doc().unwrap().doc, 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
}
#[test]
fn compare_detects_only_comment_changes_and_accepts_actual_full_thread_with_undo() {
    let mut s = fixture();
    let before = s.doc().unwrap().doc.clone();
    let mut revision = before.as_ref().clone();
    Arc::make_mut(&mut revision.slides[0]).comments[0].replies[0].text = "revised reply".into();
    let bytes = deckcraft_format::save(&revision).unwrap();
    let changes = s.execute("review.compare", &json!({"name":"own.deckcraft","data":crate::cmd::base64_encode(&bytes)})).unwrap();
    assert!(changes["changes"][0]["details"].as_array().unwrap().iter().any(|v| v == "comments"));
    s.execute("review.accept", &json!({"all":true})).unwrap();
    assert_copy(&revision, &s.doc().unwrap().doc, 0);
    assert_eq!(root(&s.doc().unwrap().doc, 0).replies[0].text, "revised reply");
    assert!(s.execute("review.revisions", &json!({})).unwrap()["changes"].as_array().unwrap().is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(root(&s.doc().unwrap().doc, 0).replies[0].text, "revised reply");
}
#[test]
fn uncopied_slide_link_refuses_cross_document_paste_atomically() {
    let mut s = fixture();
    s.execute("slide.new", &json!({})).unwrap();
    let other = s.doc().unwrap().doc.slides[1].id;
    s.edit_resources(|doc, _| {
        let c = &mut Arc::make_mut(&mut doc.slides[0]).comments[0];
        c.body.as_mut().unwrap().paragraphs[0].runs[0].props.link = Some(Hyperlink {
            action: Action::Slide { slide: other },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
            sound_name: None,
        });
        Ok(())
    })
    .unwrap();
    s.execute("slide.selectSlides", &json!({"indices":[0]})).unwrap();
    s.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    assert!(s.execute("edit.paste", &json!({})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo);
}
#[test]
fn duplicate_maps_internal_comment_links_to_copied_slide() {
    let mut s = fixture();
    let source = s.doc().unwrap().doc.slides[0].id;
    s.edit_resources(|doc, _| {
        Arc::make_mut(&mut doc.slides[0]).comments[0].body.as_mut().unwrap().paragraphs[0].runs[0].props.link = Some(Hyperlink {
            action: Action::Slide { slide: source },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: Some(source),
            sound_name: None,
        });
        Ok(())
    })
    .unwrap();
    s.execute("slide.duplicate", &json!({"index":0})).unwrap();
    let p = &s.doc().unwrap().doc;
    let link = root(p, 1).body.as_ref().unwrap().paragraphs[0].runs[0].props.link.as_ref().unwrap();
    assert_eq!(link.action, Action::Slide { slide: p.slides[1].id });
    assert_eq!(link.zoom_return, Some(p.slides[1].id));
}

fn two_slides_sharing_author() -> Presentation {
    let mut p = fixture().doc().unwrap().doc.as_ref().clone();
    let c = &mut Arc::make_mut(&mut p.slides[0]).comments[0];
    c.set_comment_anchor(CommentAnchor::Slide);
    c.replies.clear();
    c.task = None;
    let mut second = p.slides[0].as_ref().clone();
    second.id = deckcraft_model::SlideId(p.alloc_id().unwrap());
    second.shapes.clear();
    second.comments[0].comment_id = Some(p.alloc_id().unwrap());
    second.comments[0].wire_guid = Some(crate::comment_identity::fresh_guid().unwrap());
    second.comments[0].attachments.clear();
    p.slides.push(Arc::new(second));
    p
}
#[test]
fn compare_preserves_shared_author_equivalence_after_all_local_ids_change() {
    let current = two_slides_sharing_author();
    let mut revised = current.clone();
    let id = revised.alloc_id().unwrap();
    let wire = crate::comment_identity::fresh_guid().unwrap();
    for slide in &mut revised.slides {
        let author = Arc::make_mut(slide).comments[0].author_identity.as_mut().unwrap();
        author.id = id;
        author.wire_guid = Some(wire.clone());
    }
    comments::validate(&current, true).unwrap();
    comments::validate(&revised, true).unwrap();
    assert!(crate::compare::diff(&current, &revised, false).is_empty());
}
#[test]
fn compare_detects_two_distinct_authors_merged_into_one_relationship() {
    let mut current = two_slides_sharing_author();
    let id = current.alloc_id().unwrap();
    let author = Arc::make_mut(&mut current.slides[1]).comments[0].author_identity.as_mut().unwrap();
    author.id = id;
    author.wire_guid = Some(crate::comment_identity::fresh_guid().unwrap());
    let mut revised = current.clone();
    let shared = revised.slides[0].comments[0].author_identity.clone();
    Arc::make_mut(&mut revised.slides[1]).comments[0].author_identity = shared;
    comments::validate(&current, true).unwrap();
    comments::validate(&revised, true).unwrap();
    let changes = crate::compare::diff(&current, &revised, false);
    assert!(changes.iter().any(|c| c.details.iter().any(|d| d == "comments")), "{changes:?}");
}
#[test]
fn compare_does_not_canonicalize_unmatched_slide_targets_as_equal() {
    let mut current = two_slides_sharing_author();
    let old = current.slides[1].id;
    Arc::make_mut(&mut current.slides[0]).comments[0].body.as_mut().unwrap().paragraphs[0].runs[0].props.link = Some(Hyperlink {
        action: Action::Slide { slide: old },
        tooltip: String::new(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: Some(old),
        sound_name: None,
    });
    let mut revised = current.clone();
    let new = deckcraft_model::SlideId(revised.alloc_id().unwrap());
    Arc::make_mut(&mut revised.slides[1]).id = new;
    let map = std::collections::BTreeMap::from([(current.slides[0].id, current.slides[0].id), (old, new)]);
    for slide in &mut revised.slides {
        comments::resources::map_slides(&mut Arc::make_mut(slide).comments, &map).unwrap();
    }
    let changes = crate::compare::diff(&current, &revised, false);
    assert!(changes.iter().any(|c| c.slide == 0 && c.details.iter().any(|d| d == "comments")), "{changes:?}");
}

// Proposed additions to crates/engine/src/comment_transfer_tests.rs. UNRUN.
#[test]
fn by_position_compare_treats_matched_comment_slide_targets_as_equivalent() {
    let mut s = fixture();
    s.execute("slide.new", &json!({})).unwrap();
    let mut current = s.doc().unwrap().doc.as_ref().clone();
    let target = current.slides[1].id;
    Arc::make_mut(&mut current.slides[0]).comments[0].body.as_mut().unwrap().paragraphs[0].runs[0].props.link = Some(Hyperlink {
        action: Action::Slide { slide: target },
        tooltip: String::new(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: Some(target),
        sound_name: None,
    });
    let mut revised = current.clone();
    let mut slides = std::collections::BTreeMap::new();
    for slide in &mut revised.slides {
        let old = slide.id;
        let new = deckcraft_model::SlideId(old.0 + 10_000);
        Arc::make_mut(slide).id = new;
        slides.insert(old, new);
    }
    for slide in &mut revised.slides {
        comments::resources::map_slides(&mut Arc::make_mut(slide).comments, &slides).unwrap();
    }
    comments::validate(&current, true).unwrap();
    comments::validate(&revised, true).unwrap();
    let changes = crate::compare::diff(&current, &revised, true);
    assert!(changes.is_empty(), "matched slide links must compare by actual owner: {changes:?}");
}

#[test]
fn compare_detects_author_relationship_split_across_slides() {
    let mut current = fixture().doc().unwrap().doc.as_ref().clone();
    let root = &mut Arc::make_mut(&mut current.slides[0]).comments[0];
    root.replies.clear();
    root.task = None;
    root.set_comment_anchor(CommentAnchor::Slide);
    let mut second = current.slides[0].as_ref().clone();
    second.id = deckcraft_model::SlideId(current.alloc_id().unwrap());
    second.shapes.clear();
    second.comments[0].comment_id = Some(current.alloc_id().unwrap());
    second.comments[0].wire_guid = Some(crate::comment_identity::fresh_guid().unwrap());
    second.comments[0].attachments.clear();
    current.slides.push(Arc::new(second));
    let mut revised = current.clone();
    let new_author = revised.alloc_id().unwrap();
    let author = Arc::make_mut(&mut revised.slides[1]).comments[0].author_identity.as_mut().unwrap();
    author.id = new_author;
    author.wire_guid = Some(crate::comment_identity::fresh_guid().unwrap());
    comments::validate(&current, true).unwrap();
    comments::validate(&revised, true).unwrap();
    let changes = crate::compare::diff(&current, &revised, false);
    assert!(
        changes.iter().any(|change| change.details.iter().any(|detail| detail == "comments")),
        "shared author changed into two separate relationships but compare missed it: {changes:?}"
    );
}
