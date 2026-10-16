//! Contributor-original notes masters exercise identity, page geometry and shared editing.
use std::sync::Arc;

use deckcraft_geom::{Size, Xfrm};
use deckcraft_model::{
    Master, MasterId, PhType, ShapeId,
    notes::{NotesMaster, NotesPage},
};
use serde_json::json;

use crate::{Session, Target};

fn session() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    s.edit(|doc, sel| {
        doc.notes_masters = vec![
            NotesMaster {
                master: Arc::new(Master { id: MasterId(900), name: "Shared portrait".into(), ..Default::default() }),
                size: Size::new(400.0, 700.0),
                ..Default::default()
            },
            NotesMaster {
                master: Arc::new(Master { id: MasterId(901), name: "Wide".into(), ..Default::default() }),
                size: Size::new(800.0, 500.0),
                ..Default::default()
            },
        ];
        for (i, slide) in doc.slides.iter_mut().enumerate() {
            Arc::make_mut(slide).notes_page = Some(NotesPage { master: Some(MasterId(if i == 2 { 901 } else { 900 })), ..Default::default() });
        }
        sel.slide = 0;
        Ok(())
    })
    .unwrap();
    s
}

#[test]
fn notes_view_uses_associated_master_and_native_dimensions_without_creating_legacy() {
    let mut s = session();
    let before = s.doc().unwrap().doc.clone();
    s.execute("view.notesMaster", &json!({})).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.selection.target, Target::ImportedNotesMaster { master: 0 });
    assert_eq!(crate::page_size(st), Size::new(400.0, 700.0));
    assert_eq!(crate::master_page(&st.doc, st.selection.target).unwrap().slide_size, Size::new(400.0, 700.0));
    assert!(st.doc.notes_master.is_none());
    assert!(Arc::ptr_eq(&before, &st.doc));
    s.select(|_, sel| sel.slide = 2).unwrap();
    s.execute("view.notesMaster", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().selection.target, Target::ImportedNotesMaster { master: 1 });
    assert_eq!(crate::page_size(s.doc().unwrap()), Size::new(800.0, 500.0));
}

#[test]
fn imported_master_shape_edit_and_undo_keep_shared_associations() {
    let mut s = session();
    s.execute("view.notesMaster", &json!({"master": 0})).unwrap();
    let other = s.doc().unwrap().doc.notes_masters[1].clone();
    let result = s.execute("shape.insert", &json!({"preset":"rect", "rect":[40, 60, 90, 30], "text":"Shared notes artwork"})).unwrap();
    let id = ShapeId(result["id"].as_u64().unwrap() as u32);
    let st = s.doc().unwrap();
    assert!(st.shape(id).is_some());
    assert_eq!(st.doc.notes_masters[1], other);
    for i in 0..2 {
        assert!(st.doc.notes_master_for(&st.doc.slides[i]).unwrap().shapes.iter().any(|s| s.id == id));
    }
    assert!(st.doc.notes_master_for(&st.doc.slides[2]).unwrap().shapes.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.notes_masters[0].master.shapes.is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.doc().unwrap().shape(id).is_some());
}

#[test]
fn notes_placeholders_edit_selected_imported_master_at_its_size() {
    let mut s = session();
    s.execute("view.notesMaster", &json!({"master":1})).unwrap();
    s.execute("notes.placeholders", &json!({"slideImage":true})).unwrap();
    let doc = &s.doc().unwrap().doc;
    assert!(doc.notes_masters[0].master.shapes.is_empty());
    let frame = doc.notes_masters[1].master.shapes.iter().find(|s| s.ph_type() == Some(PhType::SlideImage)).unwrap().xfrm.unwrap();
    assert_eq!(frame, Xfrm::new(100.0, 62.5, 600.0, 187.5));
    assert!(doc.notes_master.is_none());
}

#[test]
fn invalid_master_index_is_atomic_and_removed_selection_follows_identity() {
    let mut s = session();
    let before = s.doc().unwrap().doc.clone();
    for value in [json!({"master":2}), json!({"master":-1}), json!({"master":1.5}), json!({"master":"0"})] {
        assert!(s.execute("view.notesMaster", &value).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
    s.execute("view.notesMaster", &json!({"master":1})).unwrap();
    s.edit(|doc, _| {
        doc.notes_masters.remove(0);
        Ok(())
    })
    .unwrap();
    assert_eq!(s.doc().unwrap().selection.target, Target::ImportedNotesMaster { master: 0 });
    assert_eq!(crate::page_master(&s.doc().unwrap().doc, s.doc().unwrap().selection.target).unwrap().id, MasterId(901));
    s.edit(|doc, sel| {
        sel.shapes.push(ShapeId(99));
        doc.notes_masters.clear();
        Ok(())
    })
    .unwrap();
    assert_eq!(s.doc().unwrap().selection.target, Target::Slides);
    assert!(s.doc().unwrap().selection.shapes.is_empty());
}

#[test]
fn missing_association_is_rejected_without_creating_fallback() {
    let mut s = session();
    s.edit(|doc, _| {
        Arc::make_mut(&mut doc.slides[0]).notes_page.as_mut().unwrap().master = Some(MasterId(999));
        Ok(())
    })
    .unwrap();
    assert!(s.execute("view.notesMaster", &json!({})).is_err());
    assert!(s.doc().unwrap().doc.notes_master.is_none());
}

#[test]
fn rendering_shared_master_edits_changes_only_associated_notes_pages() {
    let mut s = session();
    s.execute("view.notesMaster", &json!({"master":0})).unwrap();
    s.execute("shape.insert", &json!({"preset":"rect", "rect":[40,60,90,30]})).unwrap();
    s.execute("shape.fill", &json!({"color":"#1264b4"})).unwrap();
    s.execute("shape.line", &json!({"none":true})).unwrap();
    let doc = &s.doc().unwrap().doc;
    for i in 0..2 {
        let image = deckcraft_render::render_notes_page(doc, i, &Default::default());
        assert_eq!(image.pixel(70, 75), [18, 100, 180, 255]);
        assert_eq!((image.width, image.height), (400, 700));
    }
    let image = deckcraft_render::render_notes_page(doc, 2, &Default::default());
    assert_eq!((image.width, image.height), (800, 500));
    assert_eq!(image.pixel(70, 75), [255, 255, 255, 255]);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_ne!(deckcraft_render::render_notes_page(&s.doc().unwrap().doc, 0, &Default::default()).pixel(70, 75), [18, 100, 180, 255]);
}

#[test]
fn theme_fonts_edit_only_the_selected_imported_master() {
    let mut s = session();
    s.execute("view.notesMaster", &json!({"master":1})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("design.fonts", &json!({"major":"Liberation Serif", "minor":"Liberation Sans"})).unwrap();
    let doc = &s.doc().unwrap().doc;
    assert_eq!(doc.notes_masters[1].master.theme.fonts.major.latin, "Liberation Serif");
    assert_eq!(doc.notes_masters[1].master.theme.fonts.minor.latin, "Liberation Sans");
    assert_eq!(doc.masters, before.masters);
    assert_eq!(doc.notes_masters[0], before.notes_masters[0]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.notes_masters, before.notes_masters);
}

#[test]
fn undo_budget_counts_owned_notes_text_artwork_master_and_wrapper_fields() {
    use deckcraft_model::{
        Shape,
        text::{Paragraph, TextBody},
    };
    let mut s = session();
    s.edit(|doc, _| {
        let text = TextBody { paragraphs: vec![Paragraph::new(&"notes ".repeat(40_000))], ..Default::default() };
        let slide = Arc::make_mut(&mut doc.slides[0]);
        slide.notes = text.clone();
        slide.notes_page.as_mut().unwrap().shapes.push(Shape { id: ShapeId(950), text: Some(text.clone()), ..Default::default() });
        Arc::make_mut(&mut doc.notes_masters[0].master).shapes.push(Shape { id: ShapeId(951), text: Some(text), ..Default::default() });
        doc.notes_masters[0].fields.footer = Some("footer ".repeat(8_000));
        doc.handout_master = Some(Arc::new(Master {
            id: MasterId(952),
            shapes: vec![Shape {
                text: Some(TextBody { paragraphs: vec![Paragraph::new(&"handout ".repeat(20_000))], ..Default::default() }),
                ..Default::default()
            }],
            ..Default::default()
        }));
        doc.notes_master = Some(Arc::new(Master {
            id: MasterId(953),
            shapes: vec![Shape {
                text: Some(TextBody { paragraphs: vec![Paragraph::new(&"legacy ".repeat(20_000))], ..Default::default() }),
                ..Default::default()
            }],
            ..Default::default()
        }));
        Ok(())
    })
    .unwrap();
    let original = s.doc().unwrap().doc.clone();
    let shared = crate::undo_cost(&original, &original);
    // An unchanged inner master Arc still leaves the wrapper's owned field strings in history.
    assert!(shared >= 56_000);
    let mut newer = (*original).clone();
    Arc::make_mut(&mut newer.slides[0]).notes = Default::default();
    assert!(crate::undo_cost(&original, &newer) >= shared + 480_000, "canonical notes and page artwork are separate owned values");
    newer = (*original).clone();
    Arc::make_mut(&mut newer.notes_masters[0].master).shapes.clear();
    assert!(crate::undo_cost(&original, &newer) >= shared + 240_000);
    newer = (*original).clone();
    newer.handout_master = None;
    newer.notes_master = None;
    assert!(crate::undo_cost(&original, &newer) >= shared + 300_000);
}
