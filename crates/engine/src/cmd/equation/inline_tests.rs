use super::*;
use crate::{Mods, PointerEvent, PointerKind};
use equashion_math::Math;

fn fresh() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout":"blank"})).unwrap();
    s
}
fn begin(s: &mut Session) -> ShapeId {
    s.execute("insert.equation", &json!({})).unwrap();
    s.doc().unwrap().selection.text.as_ref().unwrap().shape
}
fn math(s: &Session) -> Math {
    editor(s.doc().unwrap()).unwrap().math().clone()
}
fn body(s: &Session, id: ShapeId) -> deckcraft_model::text::TextBody {
    s.doc().unwrap().shape(id).unwrap().text.clone().unwrap()
}
fn key(s: &mut Session, key: &str) {
    s.key(key, Mods::default()).unwrap();
}

#[test]
fn empty_field_is_omml_and_commands_convert_with_typing_and_history() {
    let mut s = fresh();
    let id = begin(&mut s);
    assert!(math(&s).is_empty());
    assert!(matches!(body(&s, id).paragraphs[0].runs[0].kind, RunKind::Math { .. }));
    for c in "\\alpha".chars() {
        s.type_text(&c.to_string()).unwrap();
    }
    assert_eq!(equashion_math::to_linear(&math(&s)), "α");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(math(&s).body, vec![equashion_math::Node::Text("\\alph".into())]);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(equashion_math::to_linear(&math(&s)), "α");
    s.type_text("+\\unknown ").unwrap();
    assert!(equashion_math::to_linear(&math(&s)).contains("\\unknown"));
}

#[test]
fn command_prefixes_allow_longer_names_and_delimiters() {
    for (suffix, expected) in [(" ", "∈"), ("+", "∈+"), ("fty", "∞")] {
        let mut s = fresh();
        begin(&mut s);
        for c in "\\in".chars() {
            s.type_text(&c.to_string()).unwrap();
        }
        assert_eq!(math(&s).body, vec![equashion_math::Node::Text("\\in".into())]);
        for c in suffix.chars() {
            s.type_text(&c.to_string()).unwrap();
        }
        assert_eq!(equashion_math::to_linear(&math(&s)), expected);
    }
    let mut s = fresh();
    begin(&mut s);
    for c in "\\int".chars() {
        s.type_text(&c.to_string()).unwrap();
    }
    assert!(matches!(math(&s).body.first(), Some(equashion_math::Node::Nary { op: '∫', .. })));
}

#[test]
fn insert_in_prose_exit_reentry_and_formatting() {
    for italic in [false, true] {
        let mut s = fresh();
        let id = s.execute("insert.textBox", &json!({"text":"Before after", "rect":[80,100,600,80]})).unwrap()["id"].clone();
        if italic {
            s.execute("format.italic", &json!({})).unwrap();
        }
        s.execute("text.edit", &json!({"id":id,"at":[0,7]})).unwrap();
        let count = s.doc().unwrap().shapes().len();
        let id = begin(&mut s);
        assert_eq!(s.doc().unwrap().shapes().len(), count);
        s.type_text("x+y").unwrap();
        key(&mut s, "ArrowRight");
        assert!(active(s.doc().unwrap()).is_none());
        assert_eq!(s.doc().unwrap().selection.text.as_ref().unwrap().caret, (0, 10));
        key(&mut s, "ArrowLeft");
        assert!(active(s.doc().unwrap()).is_some());
        key(&mut s, "ArrowRight");
        s.type_text(" normal ").unwrap();
        let b = body(&s, id);
        assert_eq!(b.text(), "Before x+y normal after");
        assert!(b.paragraphs[0].runs.iter().filter(|r| matches!(r.kind, RunKind::Text)).all(|r| r.props.italic.unwrap_or(false) == italic));
    }
}

#[test]
fn structured_fraction_typing_navigation_selection_and_undo() {
    let mut s = fresh();
    begin(&mut s);
    for c in "a/b".chars() {
        s.type_text(&c.to_string()).unwrap();
    }
    assert!(matches!(math(&s).body.as_slice(), [equashion_math::Node::Frac { .. }]));
    // In denominator, Right first climbs out; only a second Right leaves the equation.
    key(&mut s, "ArrowRight");
    assert!(active(s.doc().unwrap()).unwrap().caret.path.is_empty());
    key(&mut s, "ArrowLeft");
    s.type_text("c").unwrap();
    assert!(equashion_math::to_linear(&math(&s)).contains("bc"));
    s.execute("edit.selectAll", &json!({})).unwrap();
    let copied = s.execute("edit.copy", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(copied.contains("bc"));
    s.type_text("z").unwrap();
    assert_eq!(equashion_math::to_linear(&math(&s)), "z");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(editor(s.doc().unwrap()).unwrap().selection().is_some());
    assert!(equashion_math::to_linear(&math(&s)).contains("bc"));
    s.execute("edit.cut", &json!({})).unwrap();
    assert!(math(&s).is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.paste", &json!({"text":"\\beta "})).unwrap();
    assert_eq!(equashion_math::to_linear(&math(&s)), "β");
}

#[test]
fn commands_build_structures_and_empty_field_survives_history() {
    for input in ["\\sqrt x", "\\frac a", "\\sum i", "x^2", "\\sin x"] {
        let mut s = fresh();
        begin(&mut s);
        for c in input.chars() {
            s.type_text(&c.to_string()).unwrap();
        }
        assert!(math(&s).body.iter().any(|n| !matches!(n, equashion_math::Node::Text(_))), "{input}: {:?}", math(&s));
    }
    let mut s = fresh();
    begin(&mut s);
    s.type_text("x").unwrap();
    key(&mut s, "Backspace");
    assert!(math(&s).is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(equashion_math::to_linear(&math(&s)), "x");
}

#[test]
fn pointer_uses_fraction_geometry_in_rotated_and_rtl_text() {
    for rotation in [0.0, 35.0] {
        let mut s = fresh();
        let id = begin(&mut s);
        s.type_text("a/b").unwrap();
        s.edit(|doc, sel| {
            let sh = deckcraft_model::find_shape_mut(crate::shapes_mut(doc, sel).unwrap(), id).unwrap();
            sh.xfrm.as_mut().unwrap().rot = rotation;
            sh.text.as_mut().unwrap().paragraphs[0].props.rtl = Some(true);
            Ok(())
        })
        .unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.as_ref().unwrap();
        let layout = super::super::text::layout_for(st, t).unwrap();
        let eq = &layout.equations[0];
        let r = eq.geometry.caret_rect(&equashion_math::Caret { path: vec![(0, 0)], pos: 1 }).unwrap();
        let sh = st.shape(id).unwrap();
        let x = xfrm_of(&st.doc, &st.selection, sh);
        let tr = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
        let m = deckcraft_render::text_transform(x.affine(), tr, layout.rotation);
        let p = m * deckcraft_geom::Point::new(r.x0, (r.y0 + r.y1) / 2.0);
        s.execute("text.exit", &json!({})).unwrap();
        for kind in [PointerKind::Down, PointerKind::Up] {
            s.pointer(PointerEvent { kind, x: p.x, y: p.y, ..Default::default() }).unwrap();
        }
        let e = active(s.doc().unwrap()).unwrap();
        assert_eq!(e.caret.path, vec![(0, 0)]);
        s.type_text("c").unwrap();
        assert!(equashion_math::to_linear(&math(&s)).contains("ac"));
    }
}

#[test]
fn font_changes_target_the_active_equation_and_preserve_caret_and_history() {
    for input in ["", "a/b"] {
        let mut s = fresh();
        let id = s.execute("insert.textBox", &json!({"text":"Before after", "rect":[80,100,600,80]})).unwrap()["id"].clone();
        s.execute("format.font", &json!({"family":"Inter"})).unwrap();
        s.execute("text.edit", &json!({"id":id,"at":[0,7]})).unwrap();
        let id = begin(&mut s);
        s.type_text(input).unwrap();
        let before = body(&s, id);
        let tree = math(&s);
        let selection = s.doc().unwrap().selection.text.clone();
        s.execute("format.font", &json!({"family":"Latin Modern Math"})).unwrap();
        let after = body(&s, id);
        for r in &after.paragraphs[0].runs {
            let expected = if matches!(r.kind, RunKind::Math { .. }) { "Latin Modern Math" } else { "Inter" };
            assert_eq!(r.props.font.as_deref(), Some(expected));
        }
        assert_eq!(math(&s), tree);
        assert_eq!(s.doc().unwrap().selection.text, selection);
        assert_eq!(s.execute("format.state", &json!({})).unwrap()["font"], "Latin Modern Math");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(body(&s, id), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(body(&s, id), after);
        s.type_text("x").unwrap();
        assert!(equashion_math::to_linear(&math(&s)).contains('x'));
    }
}

#[test]
fn native_and_pptx_save_keep_structured_math_and_empty_fields() {
    for input in ["", "a/b", "x^2"] {
        let mut s = fresh();
        let id = begin(&mut s);
        s.type_text(input).unwrap();
        s.execute("format.font", &json!({"family":"Latin Modern Math"})).unwrap();
        let doc = &s.doc().unwrap().doc;
        let native = deckcraft_format::save(doc).unwrap();
        let restored = deckcraft_format::load(&native).unwrap();
        let pptx = deckcraft_pptx::export(doc).unwrap();
        let pptx_restored = deckcraft_pptx::import(&pptx).unwrap();
        for d in [restored, pptx_restored] {
            let run = &d
                .slides
                .last()
                .unwrap()
                .shapes
                .iter()
                .find(|sh| {
                    sh.id == id
                        || sh
                            .text
                            .as_ref()
                            .is_some_and(|t| t.paragraphs.iter().any(|p| p.runs.iter().any(|r| matches!(r.kind, RunKind::Math { .. }))))
                })
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .paragraphs[0]
                .runs[0];
            let RunKind::Math { omml } = &run.kind else { panic!("lost native math") };
            assert_eq!(run.props.font.as_deref(), Some("Latin Modern Math"), "{input:?}: {omml}");
            let m = equashion_math::from_omml(omml);
            assert_eq!(m.is_empty(), input.is_empty());
            assert!(!omml.contains("Type equation here"));
        }
    }
}

#[test]
fn table_cell_equation_uses_cell_layout_and_pointer_geometry() {
    let mut s = fresh();
    let id = s.execute("insert.table", &json!({"rows":2,"cols":2,"rect":[80,80,600,180]})).unwrap()["id"].clone();
    s.execute("text.edit", &json!({"id":id,"cell":[1,1]})).unwrap();
    begin(&mut s);
    s.type_text("a/b").unwrap();
    let st = s.doc().unwrap();
    let t = st.selection.text.as_ref().unwrap();
    assert_eq!(t.cell, Some((1, 1)));
    let layout = super::super::text::layout_for(st, t).unwrap();
    let rect = layout.equations[0].geometry.caret_rect(&equashion_math::Caret { path: vec![(0, 0)], pos: 1 }).unwrap();
    let m = super::super::text::text_frame(st, t).unwrap().1;
    let p = m * deckcraft_geom::Point::new(rect.x0, (rect.y0 + rect.y1) / 2.0);
    s.execute("text.exit", &json!({})).unwrap();
    for kind in [PointerKind::Down, PointerKind::Up] {
        s.pointer(PointerEvent { kind, x: p.x, y: p.y, ..Default::default() }).unwrap();
    }
    s.pointer(PointerEvent { kind: PointerKind::DoubleClick, x: p.x, y: p.y, ..Default::default() }).unwrap();
    assert_eq!(active(s.doc().unwrap()).unwrap().caret.path, vec![(0, 0)]);
    s.type_text("c").unwrap();
    assert!(equashion_math::to_linear(&math(&s)).contains("ac"));
}

#[test]
fn pasted_native_math_joins_the_active_equation() {
    let mut s = fresh();
    let id = begin(&mut s);
    s.type_text("x+").unwrap();
    let xml = equashion_math::to_omml(&equashion_math::from_linear("a/b"));
    s.execute("edit.paste", &json!({"text":xml})).unwrap();
    assert!(math(&s).body.iter().any(|n| matches!(n, equashion_math::Node::Frac { .. })));
    assert_eq!(body(&s, id).paragraphs[0].runs.len(), 1);
    assert!(!equashion_math::to_linear(&math(&s)).contains("xmlns"));
}

#[test]
fn adjacent_equations_target_the_new_run_and_selected_prose_is_replaced() {
    let mut s = fresh();
    let id = s.execute("insert.textBox", &json!({"text":"before replace after"})).unwrap()["id"].clone();
    s.execute("text.edit", &json!({"id":id,"at":[0,7]})).unwrap();
    for _ in 0..7 {
        s.execute("text.move", &json!({"to":"right","extend":true})).unwrap();
    }
    s.execute("insert.equation", &json!({"linear":"x"})).unwrap();
    s.execute("equation.leave", &json!({})).unwrap();
    s.execute("insert.equation", &json!({"linear":"y"})).unwrap();
    s.type_text("+z").unwrap();
    assert_eq!(equashion_math::to_linear(&math(&s)), "y+z");
    assert_eq!(s.execute("text.get", &json!({})).unwrap()["text"], "before xy+z after");
}

#[test]
fn typed_latex_arguments_survive_individual_keyboard_events() {
    let mut s = fresh();
    begin(&mut s);
    for c in "\\frac{a}{b}".chars() {
        s.type_text(&c.to_string()).unwrap();
    }
    assert_eq!(equashion_math::to_linear(&math(&s)), "a/b");
    let omml = s.execute("equation.get", &json!({})).unwrap()["omml"].as_str().unwrap().to_string();
    assert!(omml.contains("<m:f>"));
}

#[test]
fn template_commands_use_the_caret_and_share_document_history() {
    let mut s = fresh();
    begin(&mut s);
    s.type_text("x+").unwrap();
    let tpl = equashion_math::galleries().templates().find(|t| t.math.body.iter().any(|n| matches!(n, equashion_math::Node::Frac { .. }))).unwrap();
    s.execute("equation.template", &json!({"template":tpl.id})).unwrap();
    assert!(equashion_math::to_omml(&math(&s)).contains("<m:f>"));
    assert!(!active(s.doc().unwrap()).unwrap().caret.path.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(equashion_math::to_linear(&math(&s)), "x+");
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(equashion_math::to_omml(&math(&s)).contains("<m:f>"));
}
