//! Independent, contributor-original script-style preservation regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    text::{LevelStyle, RunProps},
    *,
};
use std::sync::Arc;

fn deck(body: TextBody) -> Presentation {
    let mut p = Presentation::default();
    Arc::make_mut(&mut p.slides[0]).shapes = vec![shape(body)];
    p
}
fn shape(body: TextBody) -> Shape {
    Shape { id: ShapeId(987), xfrm: Some(Xfrm::new(10., 10., 500., 100.)), text: Some(body), ..Default::default() }
}
fn refusal(p: &Presentation) {
    let Err(e) = deckcraft_pptx::export(p) else { panic!("PPTX export accepted unrepresented script semantics") };
    let e = e.to_string();
    assert!(e.contains("script") || e.contains("language"), "unexpected refusal: {e}");
}

#[test]
fn redundant_parent_script_defaults_do_not_hide_independent_descendant_semantics() {
    for source in 0..3 {
        for property in 0..4 {
            let (parent, child) = match property {
                0 => {
                    (RunProps { bold: Some(false), bold_cs: Some(false), ..Default::default() }, RunProps { bold: Some(true), ..Default::default() })
                }
                1 => (
                    RunProps { italic: Some(false), italic_ea: Some(false), ..Default::default() },
                    RunProps { italic: Some(true), ..Default::default() },
                ),
                2 => (RunProps { size: Some(18.), size_cs: Some(18.), ..Default::default() }, RunProps { size: Some(36.), ..Default::default() }),
                _ => (
                    RunProps { lang: Some("en-US".into()), lang_ea: Some("en-US".into()), ..Default::default() },
                    RunProps { lang: Some("ja-JP".into()), ..Default::default() },
                ),
            };
            let mut body = TextBody::from_text("A漢ب");
            body.paragraphs[0].runs[0].props = child;
            if source == 0 {
                body.paragraphs[0].def_props = parent.clone();
            }
            if source == 1 {
                body.list_style.set(0, LevelStyle { run: parent.clone(), ..Default::default() });
            }
            let mut p = deck(body);
            if source == 2 {
                p.default_text_style.set(0, LevelStyle { run: parent, ..Default::default() });
            }
            let sh = &p.slides[0].shapes[0];
            let para = &sh.text.as_ref().unwrap().paragraphs[0];
            let ctx = resolve::Ctx::for_slide(&p, &p.slides[0]).unwrap();
            let r = resolve::run(&ctx, sh, para, &para.runs[0].props);
            assert!(match property {
                0 => r.bold_cs != r.bold,
                1 => r.italic_ea != r.italic,
                2 => r.size_cs != r.size,
                _ => r.lang_ea != r.lang,
            });
            refusal(&p);
        }
    }
}

#[test]
fn missing_master_repair_cannot_bypass_local_script_inheritance_guard() {
    let mut body = TextBody::from_text("漢ب");
    body.paragraphs[0].def_props = RunProps { bold: Some(false), bold_cs: Some(false), ..Default::default() };
    body.paragraphs[0].runs[0].props.bold = Some(true);
    let mut p = deck(body);
    p.masters.clear();
    refusal(&p);
}

#[test]
fn independent_properties_and_language_clears_are_guarded_in_every_text_owner() {
    for owner in 0..15 {
        let mut p = Presentation::default();
        let mut body = TextBody::from_text("漢ب");
        body.paragraphs[0].runs[0].props.bold_cs = Some(false);
        let item = shape(body.clone());
        let level = LevelStyle { run: body.paragraphs[0].runs[0].props.clone(), ..Default::default() };
        match owner {
            0 => Arc::make_mut(&mut p.slides[0]).shapes = vec![item],
            1 => Arc::make_mut(&mut p.slides[0]).notes = body,
            2 => Arc::make_mut(&mut p.masters[0]).shapes = vec![item],
            3 => Arc::make_mut(&mut p.masters[0]).layouts[0].shapes = vec![item],
            4 => p.default_text_style.set(0, level),
            5 => Arc::make_mut(&mut p.masters[0]).title_style.set(8, level),
            6 => Arc::make_mut(&mut p.masters[0]).body_style.set(8, level),
            7 => Arc::make_mut(&mut p.masters[0]).other_style.set(8, level),
            8 => {
                let mut table = Table::new(1, 1, 100., 50.);
                table.rows[0].cells[0].text = body;
                Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { kind: ShapeKind::Table(table), ..Default::default() }];
            }
            9 => {
                p.notes_master = Some(p.masters[0].clone());
                Arc::make_mut(p.notes_master.as_mut().unwrap()).shapes = vec![item];
            }
            10 => {
                p.handout_master = Some(p.masters[0].clone());
                Arc::make_mut(p.handout_master.as_mut().unwrap()).shapes = vec![item];
            }
            11 => Arc::make_mut(&mut p.slides[0]).notes_page = Some(notes::NotesPage { shapes: vec![item], ..Default::default() }),
            12 => {
                let mut m = notes::NotesMaster::default();
                Arc::make_mut(&mut m.master).shapes = vec![item];
                p.notes_masters.push(m);
            }
            13 => {
                body.paragraphs[0].runs.clear();
                body.paragraphs[0].end_props.lang = Some(String::new());
                Arc::make_mut(&mut p.slides[0]).shapes = vec![shape(body)];
            }
            _ => {
                Arc::make_mut(&mut p.slides[0]).shapes =
                    vec![Shape { kind: ShapeKind::Group { children: vec![item], child: Xfrm::new(0., 0., 500., 100.) }, ..Default::default() }]
            }
        }
        refusal(&p);
    }
}

#[test]
fn script_guard_bounds_nested_groups_without_recursing_forever() {
    let mut item = shape(TextBody::from_text("Ordinary text"));
    for _ in 0..66 {
        item = Shape { kind: ShapeKind::Group { children: vec![item], child: Xfrm::new(0., 0., 500., 100.) }, ..Default::default() };
    }
    let mut p = Presentation::default();
    Arc::make_mut(&mut p.slides[0]).shapes = vec![item];
    let error = deckcraft_pptx::export(&p).unwrap_err().to_string();
    assert!(error.contains("nesting"), "{error}");
}

#[test]
fn global_editor_formatting_survives_native_reopen_and_still_exports_to_pptx() {
    use deckcraft_engine::{
        Session,
        cmd::file::{open_presentation, save_bytes},
    };
    use serde_json::json;
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset":"rect","rect":[10,10,500,100],"text":"A漢ب"})).unwrap();
    s.execute("format.bold", &json!({"on":true})).unwrap();
    s.execute("format.italic", &json!({"on":false})).unwrap();
    s.execute("format.size", &json!({"size":28})).unwrap();
    let p = &s.doc().unwrap().doc;
    let sh = p.slides[0].shapes.iter().find(|s| s.text.as_ref().is_some_and(|b| b.text() == "A漢ب")).unwrap();
    let r = &sh.text.as_ref().unwrap().paragraphs[0].runs[0].props;
    assert_eq!((r.bold, r.bold_ea, r.bold_cs), (Some(true), Some(true), Some(true)));
    assert_eq!((r.size, r.size_ea, r.size_cs), (Some(28.), Some(28.), Some(28.)));
    let reopened = open_presentation("review.deckcraft", &save_bytes(p, "deckcraft").unwrap()).unwrap();
    let pptx = deckcraft_pptx::export(&reopened).unwrap();
    let back = deckcraft_pptx::import(&pptx).unwrap();
    let sh = back.slides[0].shapes.iter().find(|s| s.text.as_ref().is_some_and(|b| b.text() == "A漢ب")).unwrap();
    let para = &sh.text.as_ref().unwrap().paragraphs[0];
    let ctx = resolve::Ctx::for_slide(&back, &back.slides[0]).unwrap();
    let r = resolve::run(&ctx, sh, para, &para.runs[0].props);
    assert_eq!((r.bold, r.italic, r.size), (Some(true), Some(false), Some(28.)));
}
