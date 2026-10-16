//! Script typography edits use ordinary commands and one undo step. Synthetic contributor content.
use crate::*;
use deckcraft_model::text::{RunProps, TextBody};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::with_new();
    s.execute("insert.textBox", &json!({"rect":[40,40,600,150],"text":"A漢 ب12 Z"})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    s.edit(|doc, sel| {
        let shape = shapes_mut(doc, sel).unwrap().iter_mut().find(|shape| sel.shapes.contains(&shape.id)).unwrap();
        shape.text.as_mut().unwrap().paragraphs[0].runs[0].props = RunProps {
            size: Some(18.0),
            size_ea: Some(30.0),
            size_cs: Some(24.0),
            bold: Some(false),
            bold_ea: Some(false),
            bold_cs: Some(true),
            italic: Some(false),
            italic_ea: Some(true),
            italic_cs: Some(false),
            lang: Some("en-US".into()),
            lang_ea: Some("ja-JP".into()),
            lang_cs: Some("ar-SA".into()),
            ..Default::default()
        };
        Ok(())
    })
    .unwrap();
    s
}
fn props(s: &Session) -> &RunProps {
    &s.doc().unwrap().selected_shapes()[0].text.as_ref().unwrap().paragraphs[0].runs[0].props
}
#[test]
fn global_bold_italic_size_language_set_all_scripts_and_undo_exactly() {
    let mut s = session();
    let before = props(&s).clone();
    for (cmd, value) in [
        ("format.bold", json!({"on":true})),
        ("format.italic", json!({"on":true})),
        ("format.size", json!({"size":26.0})),
        ("format.language", json!({"lang":"fr-FR"})),
    ] {
        s.execute(cmd, &value).unwrap();
    }
    let r = props(&s);
    assert_eq!((r.bold, r.bold_ea, r.bold_cs), (Some(true), Some(true), Some(true)));
    assert_eq!((r.italic, r.italic_ea, r.italic_cs), (Some(true), Some(true), Some(true)));
    assert_eq!((r.size, r.size_ea, r.size_cs), (Some(26.0), Some(26.0), Some(26.0)));
    assert_eq!((&r.lang, &r.lang_ea, &r.lang_cs), (&Some("fr-FR".into()), &Some("fr-FR".into()), &Some("fr-FR".into())));
    let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap();
    assert!(!bytes.is_empty(), "redundant global overrides have ordinary DrawingML representation");
    for _ in 0..4 {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert_eq!(props(&s), &before);
}
#[test]
fn script_only_patches_leave_ordinary_and_other_script_untouched() {
    let mut s = session();
    let before = props(&s).clone();
    s.execute("format.bold", &json!({"onEa":true})).unwrap();
    s.execute("format.italic", &json!({"onCs":true})).unwrap();
    s.execute("format.size", &json!({"sizeCs":32.0})).unwrap();
    s.execute("format.language", &json!({"langEa":""})).unwrap();
    let r = props(&s);
    assert_eq!((r.size, r.bold, r.italic, &r.lang), (before.size, before.bold, before.italic, &before.lang));
    assert_eq!((r.size_ea, r.bold_cs, r.italic_ea, &r.lang_cs), (before.size_ea, before.bold_cs, before.italic_ea, &before.lang_cs));
    assert_eq!((r.size_cs, r.bold_ea, r.italic_cs, r.lang_ea.as_deref()), (Some(32.0), Some(true), Some(true), Some("")));
}
#[test]
fn explicit_script_patches_override_global_values_in_one_font_dialog_edit() {
    let mut s = session();
    let before = props(&s).clone();
    s.execute("format.fontDialog", &json!({"size":20.0,"sizeEa":36.0,"bold":true,"boldCs":false,"italic":true,"italicEa":false})).unwrap();
    let r = props(&s);
    assert_eq!((r.size, r.size_ea, r.size_cs), (Some(20.0), Some(36.0), Some(20.0)));
    assert_eq!((r.bold, r.bold_ea, r.bold_cs), (Some(true), Some(true), Some(false)));
    assert_eq!((r.italic, r.italic_ea, r.italic_cs), (Some(true), Some(false), Some(true)));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(props(&s), &before);
}
#[test]
fn script_only_font_dialog_patch_does_not_change_ordinary_text() {
    let mut s = session();
    let before = props(&s).clone();
    s.execute("format.fontDialog", &json!({"sizeEa":36.0,"boldEa":true,"italicCs":true})).unwrap();
    let r = props(&s);
    assert_eq!(
        (r.size, r.bold, r.italic, r.size_cs, r.bold_cs, r.italic_ea),
        (before.size, before.bold, before.italic, before.size_cs, before.bold_cs, before.italic_ea)
    );
}
#[test]
fn malformed_script_sizes_booleans_and_languages_are_atomic_errors() {
    let mut s = session();
    let before = s.doc().unwrap().doc.clone();
    for (cmd, value) in [
        ("format.size", json!({"sizeEa":0})),
        ("format.size", json!({"sizeCs":-1})),
        ("format.size", json!({"sizeEa":4001})),
        ("format.size", json!({"sizeEa":"30"})),
        ("format.bold", json!({"onEa":"false"})),
        ("format.italic", json!({"onCs":null})),
        ("format.language", json!({"langCs":false})),
        ("format.language", json!({"langEa":"en US"})),
        ("format.fontDialog", json!({"sizeEa":0,"bold":true})),
    ] {
        assert!(s.execute(cmd, &value).is_err(), "{cmd} {value}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
}
#[test]
fn native_save_reload_preserves_all_script_options_and_explicit_clear() {
    let mut s = session();
    s.execute("format.language", &json!({"langEa":""})).unwrap();
    let before = props(&s).clone();
    let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, "deckcraft").unwrap();
    let back = deckcraft_format::load(&bytes).unwrap();
    let run = back.slides[0].shapes.iter().filter_map(|shape| shape.text.as_ref()).find(|body| body.text() == "A漢 ب12 Z").unwrap();
    assert_eq!(run.paragraphs[0].runs[0].props, before);
}
#[test]
fn global_format_overrides_inherited_script_normals_on_text_selection() {
    let mut s = session();
    s.edit(|doc, sel| {
        let shape = shapes_mut(doc, sel).unwrap().iter_mut().find(|shape| sel.shapes.contains(&shape.id)).unwrap();
        let body = shape.text.as_mut().unwrap();
        body.paragraphs[0].def_props = RunProps { bold_ea: Some(false), bold_cs: Some(false), ..Default::default() };
        body.paragraphs[0].runs[0].props = RunProps::default();
        Ok(())
    })
    .unwrap();
    s.execute("text.edit", &json!({})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
    s.execute("format.bold", &json!({"on":true})).unwrap();
    let st = s.doc().unwrap();
    let shape = st.selected_shapes()[0];
    let body: &TextBody = shape.text.as_ref().unwrap();
    let ctx = deckcraft_model::resolve::Ctx::for_slide(&st.doc, st.current_slide().unwrap()).unwrap();
    let r = deckcraft_model::resolve::run(&ctx, shape, &body.paragraphs[0], &body.paragraphs[0].runs[0].props);
    assert_eq!((r.bold, r.bold_ea, r.bold_cs), (Some(true), Some(true), Some(true)));
}

#[test]
fn grow_steps_each_script_from_its_own_effective_size() {
    let mut s = session();
    s.execute("format.grow", &json!({})).unwrap();
    let r = props(&s);
    assert_eq!((r.size, r.size_ea, r.size_cs), (Some(20.0), Some(32.0), Some(28.0)));
}
