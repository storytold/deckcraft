use crate::Mods;

use super::*;

fn body(s: &Session) -> TextBody {
    let st = s.doc().unwrap();
    super::super::text::body_of(st, st.selection.text.as_ref().unwrap()).unwrap().clone()
}

fn runs(s: &Session) -> Vec<Run> {
    body(s).paragraphs[0].runs.clone()
}

fn sel(s: &Session) -> TextSel {
    s.doc().unwrap().selection.text.clone().unwrap()
}

fn key(s: &mut Session, k: &str) {
    s.key(k, Mods::default()).unwrap();
}

fn type_keys(s: &mut Session, text: &str) {
    for c in text.chars() {
        s.type_text(&c.to_string()).unwrap();
    }
}

/// A text box reading "Area: " with an equation `linear` typed after it.
fn after_text(linear: &str) -> Session {
    let mut s = Session::with_new();
    s.execute("insert.textBox", &json!({"rect": [100, 100, 400, 80], "text": "Area: "})).unwrap();
    s.execute("text.edit", &json!({})).unwrap();
    s.execute("insert.equation", &json!({})).unwrap();
    type_keys(&mut s, linear);
    s
}

#[test]
fn an_equation_starts_as_a_selected_prompt_in_a_new_text_box() {
    let mut s = Session::with_new();
    let before = s.doc().unwrap().doc.clone();
    s.execute("insert.equation", &json!({})).unwrap();
    let r = &runs(&s)[0];
    assert_eq!((r.text.as_str(), &r.kind), (PROMPT, &RunKind::Math { omml: String::new() }));
    assert_eq!(body(&s).paragraphs[0].props.align, Some(deckcraft_model::text::Align::Center));
    assert_eq!(sel(&s).ordered(), ((0, 0), (0, PROMPT.chars().count())));
    // Typing replaces the prompt, also with the caret inside it.
    s.execute("text.select", &json!({"anchor": [0, 4]})).unwrap();
    type_keys(&mut s, "a/b");
    assert_eq!(runs(&s)[0].text, "a/b");
    assert_eq!(sel(&s).caret, (0, 3));
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, *before);
}

#[test]
fn text_can_go_before_an_equation_that_starts_a_paragraph() {
    let mut s = Session::with_new();
    s.execute("insert.equation", &json!({})).unwrap();
    type_keys(&mut s, "x^2");
    s.execute("text.move", &json!({"to": "start"})).unwrap();
    assert_eq!(at(&body(&s), &sel(&s)), None, "before the equation is outside it");
    type_keys(&mut s, "so ");
    let r = runs(&s);
    // Ordinary text: AutoCorrect capitalises it.
    assert_eq!((r[0].text.as_str(), &r[0].kind, r[1].text.as_str()), ("So ", &RunKind::Text, "x^2"));
    // Enter there pushes the equation down.
    s.execute("text.move", &json!({"to": "start"})).unwrap();
    s.type_text("\n").unwrap();
    assert_eq!(body(&s).text(), "\nSo x^2");
}

#[test]
fn the_box_fits_the_equation_as_it_is_typed() {
    let mut s = Session::with_new();
    s.execute("insert.equation", &json!({})).unwrap();
    type_keys(&mut s, "(a+b)/(c+d)+(e+f)/(g+h)");
    let st = s.doc().unwrap();
    let t = sel(&s);
    let (l, _) = super::super::text::text_frame(st, &t).unwrap();
    let w = st.shape(t.shape).and_then(|sh| sh.xfrm).unwrap().w;
    assert!(l.lines[0].extent.1 - l.lines[0].extent.0 <= w, "the linear text fits its box");
}

#[test]
fn selected_text_becomes_the_equation() {
    let mut s = Session::with_new();
    s.execute("insert.textBox", &json!({"rect": [100, 100, 400, 80], "text": "so x^2 grows"})).unwrap();
    s.execute("text.edit", &json!({})).unwrap();
    s.execute("text.select", &json!({"anchor": [0, 3], "caret": [0, 6]})).unwrap();
    s.execute("insert.equation", &json!({})).unwrap();
    let r = runs(&s);
    assert_eq!((r[1].text.as_str(), &r[1].kind), ("x^2", &RunKind::Math { omml: String::new() }));
    assert_eq!((r[0].text.as_str(), r[2].text.as_str()), ("so ", " grows"));
    // The caret is at its end, in it.
    assert_eq!(sel(&s).ordered(), ((0, 6), (0, 6)));
    assert!(at(&body(&s), &sel(&s)).is_some());
}

#[test]
fn typing_edits_the_linear_text_and_space_turns_names_into_symbols() {
    let mut s = after_text("\\alpha ");
    type_keys(&mut s, "+x^2 \\nosuch ");
    let r = &runs(&s)[1];
    assert_eq!(r.text, "α+x^2 \\nosuch ");
    assert!(matches!(r.kind, RunKind::Math { .. }));
    // No smart quotes or AutoCorrect in an equation.
    type_keys(&mut s, "f'");
    assert!(runs(&s)[1].text.ends_with("f'"));
    // Backspace and Delete edit it; emptied, it goes.
    key(&mut s, "Backspace");
    assert!(runs(&s)[1].text.ends_with('f'));
    s.execute("text.select", &json!({"anchor": [0, 6], "caret": [0, 6 + runs(&s)[1].text.chars().count()]})).unwrap();
    key(&mut s, "Backspace");
    assert_eq!(runs(&s).len(), 1);
    assert_eq!(body(&s).text(), "Area: ");
}

#[test]
fn deleting_text_beside_an_equation_leaves_the_equation() {
    let mut s = after_text("x^2");
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 5]})).unwrap();
    key(&mut s, "Backspace");
    let r = runs(&s);
    assert_eq!((r[0].text.as_str(), r[1].text.as_str()), (" ", "x^2"));
    // Delete just before it removes its first character.
    s.execute("text.select", &json!({"anchor": [0, 1]})).unwrap();
    key(&mut s, "Delete");
    assert_eq!(runs(&s)[1].text, "^2");
}

#[test]
fn right_arrow_leaves_the_equation_and_left_arrow_goes_back() {
    let mut s = after_text("x^2");
    // Home ▸ Font in an equation sets the whole equation's font.
    s.execute("format.font", &json!({"family": "Latin Modern Math"})).unwrap();
    assert_eq!(runs(&s)[1].props.font.as_deref(), Some("Latin Modern Math"));
    assert_eq!(runs(&s)[0].props.font, None);
    key(&mut s, "Right");
    assert_eq!(sel(&s).caret, (0, 9), "the caret stays where it is");
    type_keys(&mut s, " m");
    let r = runs(&s);
    assert_eq!((r[1].text.as_str(), r[2].text.as_str()), ("x^2", " m"));
    assert_eq!((&r[2].kind, r[2].props.font.as_deref()), (&RunKind::Text, None), "text after it, as the text before it");
    // Back at the equation's end, Left Arrow goes in again (without moving).
    key(&mut s, "Left");
    key(&mut s, "Left");
    assert_eq!(sel(&s).caret, (0, 9));
    s.execute("text.move", &json!({"to": "right"})).unwrap();
    key(&mut s, "Left");
    assert_eq!(sel(&s).caret, (0, 9));
    type_keys(&mut s, "+1");
    assert_eq!(runs(&s)[1].text, "x^2+1");
}

#[test]
fn the_equation_being_typed_shows_as_text_and_a_click_on_another_goes_into_it() {
    let mut s = after_text("a/b");
    let st = s.doc().unwrap();
    let t = sel(&s);
    assert_eq!(shown(st, &t), Some((0, 6)));
    assert_eq!(active(st), Some((None, 0, 6)));
    // Laid out as text while typed in: each character a caret stop.
    let (l, _) = super::super::text::text_frame(st, &t).unwrap();
    assert!(l.lines[0].boundary.iter().all(|b| *b));
    // Elsewhere it is one object; a click on it puts the caret at its end.
    s.execute("text.select", &json!({"anchor": [0, 2]})).unwrap();
    let st = s.doc().unwrap();
    let t = sel(&s);
    assert_eq!(shown(st, &t), None);
    let (l, m) = super::super::text::text_frame(st, &t).unwrap();
    let (x0, x1) = l.lines[0].span(6, 9).unwrap();
    let mid = m * deckcraft_geom::Point::new(x0 + (x1 - x0) * 0.2, l.lines[0].baseline);
    assert_eq!(super::super::text::text_pos_at(st, &t, mid), (0, 9));
}

#[test]
fn an_imported_equation_typed_in_is_rebuilt_from_its_text() {
    let mut s = Session::with_new();
    s.execute("insert.textBox", &json!({"rect": [100, 100, 400, 80], "text": ""})).unwrap();
    let omml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>"#;
    let src = TextBody {
        paragraphs: vec![Paragraph { runs: vec![Run::math(omml.into(), RunProps::default())], ..Default::default() }],
        ..Default::default()
    };
    super::super::text::paste_body(&mut s, &src).unwrap();
    assert_eq!(runs(&s)[0].text, "a/b");
    assert_eq!(sel(&s).caret, (0, 3), "the caret after the pasted equation is in it");
    type_keys(&mut s, "+c");
    assert_eq!((runs(&s)[0].text.as_str(), &runs(&s)[0].kind), ("a/b+c", &RunKind::Math { omml: String::new() }));
    // An input method composes in it too.
    s.execute("text.compose", &json!({"text": "d"})).unwrap();
    s.execute("text.compose", &json!({"text": "de"})).unwrap();
    s.execute("text.commit", &json!({"text": "de"})).unwrap();
    assert_eq!(runs(&s)[0].text, "a/b+cde");
}

#[test]
fn equations_survive_native_and_pptx_saves() {
    let mut s = after_text("sqrt(x)/2");
    s.execute("format.font", &json!({"family": "Latin Modern Math"})).unwrap();
    s.execute("format.size", &json!({"size": 32})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let run = |p: &deckcraft_model::Presentation| {
        p.slides[0]
            .shapes
            .iter()
            .filter_map(|sh| sh.text.as_ref())
            .flat_map(|t| t.paragraphs.iter().flat_map(|p| &p.runs))
            .find(|r| matches!(r.kind, RunKind::Math { .. }))
            .cloned()
    };
    let native = deckcraft_format::load(&deckcraft_format::save(&doc).unwrap()).unwrap();
    assert_eq!(run(&native), run(&doc));
    let pptx = run(&deckcraft_pptx::import(&deckcraft_pptx::export(&doc).unwrap()).unwrap()).unwrap();
    assert!(matches!(&pptx.kind, RunKind::Math { omml } if omml.contains("<m:rad>") && omml.contains("<m:f>")), "{pptx:?}");
    assert_eq!((pptx.props.font.as_deref(), pptx.props.size), (Some("Latin Modern Math"), Some(32.0)));
}
