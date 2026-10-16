//! Primary notes frames have one canonical owner; clipboard/duplicate cannot create another.
use std::sync::Arc;

use deckcraft_geom::{Size, Xfrm};
use deckcraft_model::{
    Master, MasterId, PhType, Placeholder, Shape, ShapeId, ShapeKind, TextBody,
    notes::{NotesFields, NotesMaster, NotesPage},
};
use serde_json::json;

use crate::{Session, Target};

fn frame(id: u32, kind: PhType) -> Shape {
    Shape { id: ShapeId(id), ph: Some(Placeholder { kind, ..Default::default() }), xfrm: Some(Xfrm::new(30., 40., 200., 100.)), ..Default::default() }
}
fn session(imported: bool) -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.edit(|doc, sel| {
        let master = Arc::new(Master {
            id: MasterId(900),
            name: "Shared notes".into(),
            shapes: vec![frame(901, PhType::Body), frame(902, PhType::SlideImage)],
            ..Default::default()
        });
        doc.notes_size = Size::new(400., 700.);
        if imported {
            doc.notes_master = None;
            doc.notes_masters = vec![NotesMaster {
                master,
                size: Size::new(500., 800.),
                fields: NotesFields { header: Some("Shared header".into()), display_header: Some(true), ..Default::default() },
            }];
        } else {
            doc.notes_master = Some(master);
            doc.notes_masters.clear();
        }
        for (i, slide) in doc.slides.iter_mut().enumerate() {
            let slide = Arc::make_mut(slide);
            slide.notes = TextBody::from_text(&format!("Canonical notes {i} 漢"));
            let body = ShapeId(910 + i as u32);
            slide.notes_page = Some(NotesPage {
                master: Some(MasterId(900)),
                size: Some(Size::new(420., 740.)),
                shapes: vec![frame(body.0, PhType::Body)],
                body_shape: Some(body),
                fields: NotesFields { footer: Some(format!("Local footer {i}")), display_header: Some(false), ..Default::default() },
                ..Default::default()
            });
        }
        doc.next_id = 1000;
        sel.slide = 0;
        sel.shapes.clear();
        sel.text = None;
        sel.slides.clear();
        sel.target = Target::Slides;
        Ok(())
    })
    .unwrap();
    assert!(s.doc().unwrap().doc.validate().is_empty());
    s
}
fn view(s: &mut Session, imported: bool) {
    s.execute("view.notesMaster", &if imported { json!({"master":0}) } else { json!({}) }).unwrap();
}
fn assert_primary_refusal(s: &mut Session, cmd: &str, params: serde_json::Value) {
    let before = s.doc().unwrap().doc.clone();
    let history = s.doc().unwrap().history.undo.len();
    let error = s.execute(cmd, &params).unwrap_err().to_string();
    assert!(error.contains("copying primary notes body frames is not supported"), "{error}");
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(history, s.doc().unwrap().history.undo.len());
    assert!(s.doc().unwrap().doc.validate().is_empty());
}

#[test]
fn duplicating_shared_legacy_or_imported_primary_body_is_an_atomic_refusal() {
    for imported in [false, true] {
        let mut s = session(imported);
        view(&mut s, imported);
        assert_primary_refusal(&mut s, "edit.duplicate", json!({"ids":[901]}));
        let doc = &s.doc().unwrap().doc;
        assert_eq!(doc.slides[0].notes.text(), "Canonical notes 0 漢");
        assert_eq!(doc.slides[1].notes.text(), "Canonical notes 1 漢");
        for slide in &doc.slides {
            let page = slide.notes_page.as_ref().unwrap();
            assert_eq!(page.master, Some(MasterId(900)));
            assert_eq!(page.size, Some(Size::new(420., 740.)));
            assert!(deckcraft_model::find_shape(&page.shapes, page.body_shape.unwrap()).unwrap().text.is_none());
        }
    }
}

#[test]
fn nested_primary_frame_cannot_be_duplicated_or_copied_from_a_notes_master() {
    let mut s = session(true);
    s.edit(|doc, _| {
        let master = Arc::make_mut(&mut doc.notes_masters[0].master);
        let body = master.shapes.remove(0);
        master.shapes.push(Shape {
            id: ShapeId(950),
            kind: ShapeKind::Group { children: vec![body], child: Xfrm::new(0., 0., 300., 200.) },
            ..Default::default()
        });
        Ok(())
    })
    .unwrap();
    view(&mut s, true);
    assert_primary_refusal(&mut s, "edit.duplicate", json!({"ids":[950]}));
    s.execute("edit.select", &json!({"ids":[950]})).unwrap();
    assert_primary_refusal(&mut s, "edit.copy", json!({}));
}

#[test]
fn pasting_a_body_role_into_notes_master_refuses_without_changing_source_or_clipboard_count() {
    let mut s = session(true);
    s.edit(|doc, _| {
        Arc::make_mut(&mut doc.slides[0]).shapes.push(Shape {
            id: ShapeId(960),
            text: Some(TextBody::from_text("Independent slide body")),
            ..frame(960, PhType::Body)
        });
        Ok(())
    })
    .unwrap();
    s.execute("edit.select", &json!({"ids":[960]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    let copied = s.clipboard.shapes.clone();
    let pastes = s.clipboard.pastes;
    view(&mut s, true);
    assert_primary_refusal(&mut s, "edit.paste", json!({}));
    assert_primary_refusal(&mut s, "edit.pasteSpecial", json!({"as":"keepSource"}));
    assert_eq!(s.clipboard.shapes, copied);
    assert_eq!(s.clipboard.pastes, pastes);
    s.execute("edit.select", &json!({"ids":[901]})).unwrap();
    assert_primary_refusal(&mut s, "edit.copy", json!({}));
    assert_eq!(s.clipboard.shapes, copied, "refused copy must not discard the preceding usable clipboard");
}

#[test]
fn slide_image_duplicates_remain_editable_with_undo_and_live_roles() {
    let mut s = session(true);
    view(&mut s, true);
    let pages: Vec<_> = s.doc().unwrap().doc.slides.iter().map(|slide| (slide.notes.clone(), slide.notes_page.clone())).collect();
    let master = s.doc().unwrap().doc.notes_masters[0].clone();
    let result = s.execute("edit.duplicate", &json!({"ids":[902]})).unwrap();
    let id = ShapeId(result["ids"][0].as_u64().unwrap() as u32);
    assert_eq!(s.doc().unwrap().shape(id).unwrap().ph_type(), Some(PhType::SlideImage));
    assert!(s.doc().unwrap().shape(id).unwrap().text.is_none());
    s.execute("edit.copy", &json!({})).unwrap();
    let pasted = s.execute("edit.paste", &json!({})).unwrap();
    let pasted = ShapeId(pasted["ids"][0].as_u64().unwrap() as u32);
    assert_eq!(s.doc().unwrap().shape(pasted).unwrap().ph_type(), Some(PhType::SlideImage));
    assert!(s.doc().unwrap().doc.validate().is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.notes_masters[0], master);
    for (slide, expected) in s.doc().unwrap().doc.slides.iter().zip(pages) {
        assert_eq!((slide.notes.clone(), slide.notes_page.clone()), expected);
    }
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().shape(id).unwrap().ph_type(), Some(PhType::SlideImage));
}

#[test]
fn raw_local_canonical_frame_duplication_refuses_instead_of_losing_out_of_line_text() {
    let mut s = session(true);
    for role in [Some(Placeholder { kind: PhType::Body, ..Default::default() }), None] {
        s.edit(|doc, _| {
            Arc::make_mut(&mut doc.slides[0]).notes_page.as_mut().unwrap().shapes[0].ph = role;
            Ok(())
        })
        .unwrap();
        assert_primary_refusal(&mut s, "edit.duplicate", json!({"ids":[910]}));
        assert_eq!(s.doc().unwrap().doc.slides[0].notes.text(), "Canonical notes 0 漢");
    }
}

#[test]
fn whole_slide_clipboard_preserves_the_single_notes_owner_and_undo() {
    let mut s = session(true);
    s.execute("slide.go", &json!({"index":0})).unwrap();
    let original = s.doc().unwrap().current_slide().unwrap().clone();
    let master = s.doc().unwrap().doc.notes_masters[0].clone();
    s.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("edit.paste", &json!({"scope":"slides"})).unwrap();
    let copied = s.doc().unwrap().current_slide().unwrap();
    assert_ne!(copied.id, original.id);
    assert_eq!(copied.notes, original.notes);
    assert_eq!(copied.notes_page, original.notes_page);
    assert_eq!(s.doc().unwrap().doc.notes_masters[0], master);
    assert!(s.doc().unwrap().doc.validate().is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.as_ref(), before.as_ref());
}

#[test]
fn slide_owned_art_with_same_numeric_id_as_notes_body_duplicates_and_undo_keeps_notes() {
    let mut s = session(true);
    let art = Shape {
        id: ShapeId(910),
        name: "Slide-owned art".into(),
        xfrm: Some(Xfrm::new(10., 20., 80., 40.)),
        text: Some(TextBody::from_text("Ordinary slide art")),
        ..Default::default()
    };
    s.edit(|doc, _| {
        Arc::make_mut(&mut doc.slides[0]).shapes.push(art.clone());
        Ok(())
    })
    .unwrap();
    assert!(s.doc().unwrap().doc.validate().is_empty(), "cross-owner numeric shape ids may coincide");
    let before = s.doc().unwrap().doc.clone();
    let notes = s.doc().unwrap().doc.slides[0].notes.clone();
    let page = s.doc().unwrap().doc.slides[0].notes_page.clone();
    let result = s.execute("edit.duplicate", &json!({"ids":[910]})).unwrap();
    let id = ShapeId(result["ids"][0].as_u64().unwrap() as u32);
    assert_ne!(id, art.id);
    let copied = s.doc().unwrap().shape(id).unwrap();
    assert_eq!(copied.text, art.text);
    let copied_box = copied.xfrm.unwrap();
    assert_eq!((copied_box.w, copied_box.h), (80., 40.));
    assert_eq!((copied_box.x, copied_box.y), (10. + s.prefs.duplicate_offset, 20. + s.prefs.duplicate_offset));
    assert_eq!(s.doc().unwrap().doc.slides[0].notes, notes);
    assert_eq!(s.doc().unwrap().doc.slides[0].notes_page, page);
    assert!(s.doc().unwrap().doc.validate().is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.as_ref(), before.as_ref());
}
