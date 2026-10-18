//! Tests for the comment commands that had no test: the query the pane is filled from, removing a
//! comment (one, a slide's or the deck's), replying in a thread and resolving one.
//! `comment.add`, `comment.next` and `comment.prev` are covered in `review.rs`'s own tests.

use deckcraft_model::{Comment, ShapeId};
use serde_json::json;

use crate::Session;

fn comments(s: &Session) -> Vec<Comment> {
    let st = s.doc().expect("a document");
    st.doc.slides.get(st.selection.slide).expect("the current slide").comments.clone()
}

fn comments_on(s: &Session, slide: usize) -> Vec<Comment> {
    s.doc().expect("a document").doc.slides.get(slide).expect("the slide").comments.clone()
}

fn texts(s: &Session) -> Vec<String> {
    comments(s).iter().map(|c| c.text.clone()).collect()
}

fn deck(n: usize) -> Session {
    let mut s = Session::with_new();
    while s.doc().expect("a document").doc.slides.len() < n {
        s.execute("slide.new", &json!({})).expect("a slide");
    }
    // A new slide takes the selection with it, so a deck a test can rely on starts on slide 0.
    s.execute("slide.go", &json!({"index": 0})).expect("the first slide");
    s
}

#[test]
fn a_comment_records_who_wrote_it_and_where_it_sits() {
    let mut s = Session::with_new();
    let err = s.execute("comment.add", &json!({})).expect_err("no text");
    assert!(err.to_string().contains("missing `text`"), "{err}");

    let r = s.execute("comment.add", &json!({"text": "Tighten this", "x": 120.0, "y": 80.0})).expect("a comment");
    assert_eq!(r["index"], 0, "the index is where the pane finds it again");
    let c = &comments(&s)[0];
    assert_eq!(c.text, "Tighten this");
    assert_eq!((c.x, c.y), (120.0, 80.0));
    assert_eq!(c.author, s.prefs.author, "the author comes from the settings, not the caller");
    let want: String = s.prefs.author.split_whitespace().filter_map(|w| w.chars().next()).collect();
    assert_eq!(c.initials, want, "the avatar is the author's initials");
    assert!(!c.date.is_empty(), "and the thread is stamped");
    assert!(!c.resolved && c.replies.is_empty() && c.shape.is_none(), "a new thread is open and on its own");

    // The pin's place has a default, so a caller that has nothing but text can still comment.
    let r = s.execute("comment.add", &json!({"text": "and this"})).expect("a comment with no place");
    assert_eq!(r["index"], 1);
    assert_eq!((comments(&s)[1].x, comments(&s)[1].y), (10.0, 10.0));

    // `id` attaches a thread to a shape, and so does having one selected.
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 120.0, 80.0]})).expect("a shape")["id"]
        .as_u64()
        .expect("a shape id") as u32;
    s.execute("comment.add", &json!({"text": "on the shape", "id": id})).expect("a comment on a shape");
    assert_eq!(comments(&s)[2].shape.map(|x| x.0), Some(id));
    s.select(|_, sel| sel.shapes = vec![ShapeId(id)]).expect("select the shape");
    s.execute("comment.add", &json!({"text": "on the selection"})).expect("a comment on the selection");
    assert_eq!(comments(&s)[3].shape.map(|x| x.0), Some(id), "a selected shape is the one the pin lands on");
}

#[test]
fn a_reply_joins_the_thread_it_names() {
    let mut s = Session::with_new();
    s.execute("comment.add", &json!({"text": "first"})).expect("a thread");

    let err = s.execute("comment.reply", &json!({"text": "to which thread?"})).expect_err("no index");
    assert!(err.to_string().contains("missing `index`"), "{err}");
    let err = s.execute("comment.reply", &json!({"index": 3, "text": "nowhere"})).expect_err("no such thread");
    assert!(err.to_string().contains("no such comment"), "{err}");

    s.execute("comment.reply", &json!({"index": 0, "text": "Looking at it now"})).expect("a reply");
    let c = &comments(&s)[0];
    assert_eq!(c.text, "first", "the thread keeps its own text");
    assert_eq!(c.replies.len(), 1);
    let r = &c.replies[0];
    assert_eq!(r.text, "Looking at it now");
    assert_eq!(r.author, s.prefs.author, "a reply is written by whoever replies");
    assert!(!r.date.is_empty());
    assert_eq!((r.x, r.y), (0.0, 0.0), "a reply has no pin of its own");
    assert!(r.replies.is_empty(), "and nothing is nested under it");

    s.execute("comment.reply", &json!({"index": 0, "text": "and one more"})).expect("another reply");
    let said: Vec<String> = comments(&s)[0].replies.iter().map(|x| x.text.clone()).collect();
    assert_eq!(said, ["Looking at it now", "and one more"], "in the order they were said");
}

#[test]
fn deleting_comments_can_take_one_comment_the_slide_or_the_whole_deck() {
    let mut s = deck(2);
    for t in ["first", "second", "third"] {
        s.execute("comment.add", &json!({"text": t})).expect("a comment");
    }
    s.execute("slide.go", &json!({"index": 1})).expect("the second slide");
    s.execute("comment.add", &json!({"text": "on the other slide"})).expect("a comment there");
    s.execute("slide.go", &json!({"index": 0})).expect("back to the first slide");

    s.execute("comment.delete", &json!({"index": 1})).expect("take out the second");
    assert_eq!(texts(&s), ["first", "third"], "the thread after it moves up");
    let err = s.execute("comment.delete", &json!({"index": 9})).expect_err("no such comment");
    assert!(err.to_string().contains("no such comment"), "{err}");
    assert_eq!(texts(&s), ["first", "third"], "and an error deletes nothing");

    // No index is the slide's own comments; `all` is the deck.
    s.execute("comment.delete", &json!({})).expect("clear the slide");
    assert!(comments(&s).is_empty());
    assert_eq!(comments_on(&s, 1).len(), 1, "the other slide keeps its comment");
    s.execute("comment.delete", &json!({"all": true})).expect("clear the deck");
    assert!(comments_on(&s, 1).is_empty(), "every slide, not just this one");
    assert!(comments_on(&s, 0).is_empty());

    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(comments_on(&s, 1).len(), 1, "one step puts the deck's comments back");
}

#[test]
fn resolving_a_thread_is_a_flag_the_author_can_take_back() {
    let mut s = Session::with_new();
    s.execute("comment.add", &json!({"text": "needs a look"})).expect("a thread");

    let err = s.execute("comment.resolve", &json!({})).expect_err("which thread?");
    assert!(err.to_string().contains("missing `index`"), "{err}");
    let err = s.execute("comment.resolve", &json!({"index": 2})).expect_err("no such thread");
    assert!(err.to_string().contains("no such comment"), "{err}");

    s.execute("comment.resolve", &json!({"index": 0})).expect("resolve it");
    assert!(comments(&s)[0].resolved, "resolving is what a caller gets without saying otherwise");
    s.execute("comment.resolve", &json!({"index": 0, "resolved": false})).expect("open it again");
    assert!(!comments(&s)[0].resolved, "a resolved thread can be reopened");
    assert_eq!(comments(&s)[0].text, "needs a look", "and the thread itself is untouched");

    s.execute("edit.undo", &json!({})).expect("undo");
    assert!(comments(&s)[0].resolved, "each change is one step back");
}

#[test]
fn the_list_is_a_query_for_one_slide() {
    let mut s = deck(2);
    s.execute("comment.add", &json!({"text": "first"})).expect("a comment");
    s.execute("comment.resolve", &json!({"index": 0})).expect("resolved");
    s.execute("comment.reply", &json!({"index": 0, "text": "answered"})).expect("a reply");
    s.execute("slide.go", &json!({"index": 1})).expect("the second slide");
    s.execute("comment.add", &json!({"text": "elsewhere"})).expect("a comment there");

    let before = s.doc().expect("a document").history.undo.len();
    let v = s.execute("comment.list", &json!({"slide": 0})).expect("the first slide's comments");
    let all = v.as_array().expect("an array");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["text"], "first");
    assert_eq!(all[0]["author"], s.prefs.author);
    assert_eq!(all[0]["resolved"], true);
    assert_eq!(all[0]["replies"][0]["text"], "answered", "the replies travel with their thread");

    let v = s.execute("comment.list", &json!({"slide": 99})).expect("a slide that is not there");
    assert_eq!(v.as_array().expect("an array").len(), 0, "no slide, no comments");
    assert_eq!(s.doc().expect("a document").history.undo.len(), before, "asking is not an edit");

    // Without `slide` it is the current one, which is what the pane shows.
    let v = s.execute("comment.list", &json!({})).expect("this slide's comments");
    assert_eq!(v.as_array().expect("an array")[0]["text"], "elsewhere");
}
