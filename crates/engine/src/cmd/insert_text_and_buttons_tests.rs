//! `insert.wordArt`, `insert.slideNumber`, `insert.symbol` and `insert.actionButton` — the Insert
//! tab's text and button inserts — had no test. Each is a small handler, but each decides something
//! visible: the WordArt styling, whether a slide number becomes a footer field on the slide or a
//! field run at the caret, and which action a button carries.

use deckcraft_color::SchemeSlot;
use deckcraft_model::text::{Action, Align, RunKind};
use deckcraft_model::{ColorRef, Fill, Line, PhType, Shape, ShapeId};
use serde_json::{Value, json};

use crate::Session;

fn shape_of(s: &Session, id: u32) -> Shape {
    s.doc().expect("a document").shape(ShapeId(id)).cloned().expect("the shape")
}

/// Every run's text of a shape, joined: what a user reads on the slide.
fn plain(sh: &Shape) -> String {
    sh.text.as_ref().map(|t| t.paragraphs.iter().flat_map(|p| p.runs.iter()).map(|r| r.text.as_str()).collect()).unwrap_or_default()
}

/// The slide-number placeholders the current slide shows.
fn number_placeholders(s: &Session) -> Vec<Shape> {
    s.doc()
        .expect("a document")
        .current_slide()
        .map(|sl| sl.shapes.iter().filter(|x| x.ph_type() == Some(PhType::SlideNum)).cloned().collect())
        .unwrap_or_default()
}

/// A text box on the current slide, and the id of the shape it made.
fn text_box(s: &mut Session, text: &str) -> u32 {
    s.execute("insert.textBox", &json!({"rect": [40.0, 40.0, 300.0, 60.0], "text": text})).expect("a text box")["id"].as_u64().expect("an id") as u32
}

/// Put the insertion point in a shape's text, `at` characters in.
fn caret_at(s: &mut Session, id: u32, at: usize) {
    s.select(|_, sel| sel.text = Some(crate::TextSel { shape: ShapeId(id), anchor: (0, at), caret: (0, at), ..Default::default() }))
        .expect("a caret");
}

#[test]
fn word_art_creates_a_centred_text_box_in_the_style_asked_for() {
    let mut s = Session::with_new();
    let v = s.execute("insert.wordArt", &json!({})).expect("word art");
    let id = v["id"].as_u64().expect("an id") as u32;
    let sh = shape_of(&s, id);
    assert!(sh.text_box, "WordArt is a text box, so the text tools apply to it");
    let para = &sh.text.as_ref().expect("text").paragraphs[0];
    assert_eq!(para.props.align, Some(Align::Center), "WordArt is centred");
    let run = &para.runs[0];
    assert_eq!(run.text, "Your text here", "with prompt text when none is given");
    assert_eq!(run.props.size, Some(54.0));
    assert_eq!(run.props.bold, Some(false), "style 0 is not bold");
    assert_eq!(run.props.fill, Some(Fill::solid(ColorRef::scheme(SchemeSlot::Tx1))));
    assert_eq!(run.props.outline, None, "style 0 has no outline");
    assert_eq!(s.doc().expect("a document").selection.shapes, vec![ShapeId(id)], "the new WordArt is selected");

    // The style walks the four colour slots, bolds every other one and outlines every third.
    for (style, slot, bold, outline) in [
        (1, SchemeSlot::Accent1, true, false),
        (2, SchemeSlot::Accent2, false, true),
        (3, SchemeSlot::Accent4, true, false),
        (4, SchemeSlot::Tx1, false, false),
        (5, SchemeSlot::Accent1, true, true),
    ] {
        let v = s.execute("insert.wordArt", &json!({"text": "Hi", "style": style})).expect("word art");
        let id = v["id"].as_u64().expect("an id") as u32;
        let sh = shape_of(&s, id);
        assert_eq!(plain(&sh), "Hi", "the text asked for");
        let run = &sh.text.as_ref().expect("text").paragraphs[0].runs[0];
        assert_eq!(run.props.bold, Some(bold), "style {style}");
        assert_eq!(run.props.fill, Some(Fill::solid(ColorRef::scheme(slot))), "style {style}");
        assert_eq!(run.props.outline.is_some(), outline, "style {style}");
    }
    let v = s.execute("insert.wordArt", &json!({"text": "Hi", "style": 2})).expect("word art");
    let id = v["id"].as_u64().expect("an id") as u32;
    let sh = shape_of(&s, id);
    let run = &sh.text.as_ref().expect("text").paragraphs[0].runs[0];
    assert_eq!(run.props.outline, Some(Line::solid(ColorRef::scheme(SchemeSlot::Bg1), 1.0)), "the outline is the theme background");
}

#[test]
fn slide_number_turns_on_the_footer_field_for_the_slide() {
    let mut s = Session::with_new();
    assert!(!s.doc().expect("a document").doc.header_footer.slide_number);
    assert_eq!(s.execute("insert.slideNumber", &json!({})).expect("slide number"), Value::Null);
    assert!(s.doc().expect("a document").doc.header_footer.slide_number, "without a caret it is the Header & Footer switch");
    let num = number_placeholders(&s);
    assert_eq!(num.len(), 1, "the slide gains the number placeholder");
    let run = &num[0].text.as_ref().expect("text").paragraphs[0].runs[0];
    assert_eq!(run.text, "‹#›");
    assert!(matches!(&run.kind, RunKind::Field { field } if field == "slidenum"));
    // Asking twice leaves one placeholder, not two.
    s.execute("insert.slideNumber", &json!({})).expect("again");
    assert_eq!(number_placeholders(&s).len(), 1);
}

#[test]
fn slide_number_with_a_caret_writes_the_field_where_it_points() {
    let mut s = Session::with_new();
    let id = text_box(&mut s, "Page ");
    caret_at(&mut s, id, 5);
    assert_eq!(s.execute("insert.slideNumber", &json!({})).expect("a field"), Value::Null);
    let sh = shape_of(&s, id);
    assert_eq!(plain(&sh), "Page ‹#›", "the field lands at the insertion point");
    let field = sh.text.as_ref().expect("text").paragraphs[0].runs.iter().find(|r| matches!(&r.kind, RunKind::Field { .. })).expect("a field run");
    assert!(matches!(&field.kind, RunKind::Field { field } if field == "slidenum"));
    assert!(!s.doc().expect("a document").doc.header_footer.slide_number, "a caret field is not the slide's footer switch");
    // One undo takes the typed field away.
    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(plain(&shape_of(&s, id)), "Page ");
}

#[test]
fn symbol_needs_a_point_in_text_and_types_at_it() {
    let mut s = Session::with_new();
    let err = s.execute("insert.symbol", &json!({})).expect_err("missing text");
    assert!(err.to_string().contains("missing `text`"), "{err}");
    let err = s.execute("insert.symbol", &json!({"text": "©"})).expect_err("no insertion point");
    assert!(err.to_string().contains("place the insertion point in text first"), "{err}");

    let id = text_box(&mut s, "A B");
    assert_eq!(plain(&shape_of(&s, id)), "A B", "the text box holds what was typed");
    caret_at(&mut s, id, 1);
    let v = s.execute("insert.symbol", &json!({"text": "©"})).expect("a sign");
    assert_eq!(v["caret"], json!([0, 2]), "the caret ends up after the sign");
    assert_eq!(plain(&shape_of(&s, id)), "A© B", "the sign lands where the caret was");
    caret_at(&mut s, id, 4);
    s.execute("insert.symbol", &json!({"text": "!"})).expect("another one");
    assert_eq!(plain(&shape_of(&s, id)), "A© B!");
    // One undo per typed sign.
    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(plain(&shape_of(&s, id)), "A© B");
}

#[test]
fn an_action_button_carries_the_action_its_kind_means() {
    let mut s = Session::with_new();
    for (kind, preset, action) in [
        ("back", "actionButtonBackPrevious", Some(Action::PreviousSlide)),
        ("previous", "actionButtonBackPrevious", Some(Action::PreviousSlide)),
        ("forward", "actionButtonForwardNext", Some(Action::NextSlide)),
        ("next", "actionButtonForwardNext", Some(Action::NextSlide)),
        ("beginning", "actionButtonBeginning", Some(Action::FirstSlide)),
        ("end", "actionButtonEnd", Some(Action::LastSlide)),
        ("home", "actionButtonHome", Some(Action::FirstSlide)),
        ("information", "actionButtonInformation", None),
        ("return", "actionButtonReturn", Some(Action::LastViewed)),
        ("movie", "actionButtonMovie", None),
        ("document", "actionButtonDocument", None),
        ("sound", "actionButtonSound", None),
        ("help", "actionButtonHelp", None),
        ("custom", "actionButtonBlank", None),
        // A kind nobody knows is the blank button, not an error.
        ("banana", "actionButtonBlank", None),
    ] {
        let v = s.execute("insert.actionButton", &json!({"kind": kind, "rect": [0.0, 0.0, 48.0, 48.0]})).expect("a button");
        let id = v["id"].as_u64().expect("an id") as u32;
        let sh = shape_of(&s, id);
        assert_eq!(sh.geom.preset_name(), Some(preset), "{kind}");
        match action {
            Some(action) => {
                let click = sh.click.as_ref().unwrap_or_else(|| panic!("{kind} runs an action"));
                assert_eq!(click.action, action, "{kind}");
                assert!(click.highlight_click, "{kind} lights up when clicked");
            }
            None => assert!(sh.click.is_none(), "{kind} has nothing to run"),
        }
    }
}

#[test]
fn action_buttons_land_centred_or_in_the_rect_they_are_given() {
    let mut s = Session::with_new();
    let v = s.execute("insert.actionButton", &json!({"kind": "back"})).expect("a button");
    let id = v["id"].as_u64().expect("an id") as u32;
    let x = shape_of(&s, id).xfrm.expect("a frame");
    assert_eq!((x.x, x.y, x.w, x.h), (456.0, 246.0, 48.0, 48.0), "a click-inserted button lands in the middle");
    assert_eq!(s.doc().expect("a document").selection.shapes, vec![ShapeId(id)], "the new button is selected");

    let v = s.execute("insert.actionButton", &json!({"kind": "back", "rect": [10.0, 20.0, 30.0, 40.0]})).expect("a placed button");
    let id = v["id"].as_u64().expect("an id") as u32;
    let x = shape_of(&s, id).xfrm.expect("a frame");
    assert_eq!((x.x, x.y, x.w, x.h), (10.0, 20.0, 30.0, 40.0));
}
