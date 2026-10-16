//! Contributor-original notes style ownership and sparse serialization fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    notes::{NotesMaster, NotesPage},
    style::{Effects, Shadow},
    text::{BodyProps, LevelStyle},
    *,
};
use std::{
    io::{Cursor, Read},
    sync::Arc,
};

fn fixture() -> Presentation {
    let mut p = Presentation { slide_size: Size::new(100., 50.), ..Default::default() };
    let mut text = TextBody::from_text("");
    text.body = BodyProps { inset_l: Some(10.), inset_t: Some(0.), inset_r: Some(0.), inset_b: Some(0.), ..Default::default() };
    text.paragraphs[0].def_props = RunProps { font: Some("Ubuntu".into()), size: Some(18.), bold: Some(true), ..Default::default() };
    let master = Master {
        id: MasterId(801),
        shapes: vec![Shape {
            id: ShapeId(802),
            xfrm: Some(Xfrm::new(10., 100., 180., 140.)),
            ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
            fill: Some(Fill::None),
            line: Some(Line::none()),
            text: Some(text),
            ..Default::default()
        }],
        ..Default::default()
    };
    p.notes_masters.push(NotesMaster { master: Arc::new(master), size: Size::new(200., 300.), ..Default::default() });
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.notes = TextBody::from_text("One  two\tthree\n\nLast");
    slide.notes_page = Some(NotesPage {
        master: Some(MasterId(801)),
        body_shape: Some(ShapeId(803)),
        shapes: vec![Shape { id: ShapeId(803), ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }), ..Default::default() }],
        ..Default::default()
    });
    p.fix_next_id();
    p
}
fn reopen(p: &Presentation) -> Presentation {
    let encoded = deckcraft_odp::export(p).unwrap();
    if let Ok(dir) = std::env::var("DECKCRAFT_NOTES_STYLE_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(&encoded)).unwrap();
        for name in ["styles.xml", "content.xml"] {
            let mut xml = String::new();
            zip.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(name), xml).unwrap();
        }
    }
    let out = deckcraft_odp::import(&encoded).unwrap();
    assert!(!out.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", out.tags);
    out
}

#[test]
fn canonical_paragraph_defaults_and_run_overrides_keep_distinct_owners() {
    let mut source = fixture();
    let notes = &mut Arc::make_mut(&mut source.slides[0]).notes;
    notes.paragraphs[0].def_props.size = Some(18.);
    notes.paragraphs[2].runs[0].props.bold = Some(true);
    let original = source.clone();
    let mut reopened = reopen(&source);
    assert_eq!(source, original, "serialization is observational");
    assert_eq!(reopened.slides[0].notes.paragraphs.len(), 3);
    assert_eq!(reopened.slides[0].notes.text(), source.slides[0].notes.text());
    let notes = &reopened.slides[0].notes;
    assert_eq!(notes.paragraphs[0].def_props.size, Some(18.));
    assert!(notes.paragraphs[0].runs.iter().all(|run| run.props.size.is_none()));
    assert_eq!(notes.paragraphs[2].runs[0].props.bold, Some(true));
    assert!(notes.paragraphs[1].runs.is_empty(), "empty probes must not survive");
    let parent = Arc::make_mut(&mut reopened.notes_masters[0].master).shapes[0].text.as_mut().unwrap();
    parent.paragraphs[0].def_props.size = Some(30.);
    parent.paragraphs[0].def_props.bold = Some(false);
    let doc = deckcraft_render::notes_page_document(&reopened, 0, &Default::default()).unwrap();
    let body = &doc.slides[0].shapes[0].text.as_ref().unwrap();
    assert_eq!(body.paragraphs[0].def_props.size, Some(18.), "explicit equal local default stays local");
    assert_eq!(body.paragraphs[2].def_props.size, Some(30.), "absent local default stays inherited");
    assert_eq!(body.paragraphs[2].runs[0].props.bold, Some(true));
}

#[test]
fn empty_master_text_has_a_real_shared_default_style_without_fake_paragraphs() {
    for text in [None, Some(TextBody::default())] {
        let mut source = fixture();
        Arc::make_mut(&mut source.notes_masters[0].master).shapes[0].text = text;
        let mut reopened = reopen(&source);
        assert!(reopened.notes_masters[0].master.shapes[0].text.is_none());
        let mut text = TextBody::from_text("");
        text.paragraphs[0].def_props.size = Some(31.);
        Arc::make_mut(&mut reopened.notes_masters[0].master).shapes[0].text = Some(text);
        assert!(reopened.slides[0].notes.paragraphs[0].def_props.size.is_none());
        let doc = deckcraft_render::notes_page_document(&reopened, 0, &Default::default()).unwrap();
        assert_eq!(doc.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].def_props.size, Some(31.));
    }
}

#[test]
fn explicit_empty_effects_clear_inherited_shadow_after_master_edits() {
    for explicit in [false, true] {
        let mut source = fixture();
        Arc::make_mut(&mut source.notes_masters[0].master).shapes[0].effects = Some(Effects {
            outer_shadow: Some(Shadow {
                color: ColorRef::rgb(Rgba::BLACK),
                dist: 4.,
                dir: 0.,
                blur: 0.,
                inner: false,
                sx: 1.,
                sy: 1.,
                kx: 0.,
                ky: 0.,
                align: "b".into(),
                rotate_with_shape: true,
            }),
            ..Default::default()
        });
        if explicit {
            Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[0].effects = Some(Effects::default());
        }
        let reopened = reopen(&source);
        let shape = &reopened.slides[0].notes_page.as_ref().unwrap().shapes[0];
        let ctx = deckcraft_model::resolve::Ctx::for_notes(&reopened, &reopened.slides[0]).unwrap();
        assert_eq!(deckcraft_model::resolve::effects(&ctx, shape).0.and_then(|effect| effect.outer_shadow).is_none(), explicit);
    }
}

#[test]
fn multilevel_notes_defaults_refuse_instead_of_flattening_live_ownership() {
    let mut source = fixture();
    Arc::make_mut(&mut source.notes_masters[0].master).shapes[0]
        .text
        .as_mut()
        .unwrap()
        .list_style
        .set(1, LevelStyle { run: RunProps { size: Some(25.), ..Default::default() }, ..Default::default() });
    assert!(deckcraft_odp::export(&source).unwrap_err().0.contains("multiple paragraph-level defaults"));
}
