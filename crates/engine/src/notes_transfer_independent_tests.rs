//! Independent native notes transfer adversaries. All fixtures are contributor-original.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{Session, notes_transfer};
use deckcraft_geom::{Size, Xfrm};
use deckcraft_model::{
    notes::{NotesFields, NotesMaster, NotesPage},
    style::{Artistic, PictureFill},
    text::Bullet,
    *,
};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};

fn artwork(id: u32) -> Shape {
    Shape { id: ShapeId(id), xfrm: Some(Xfrm::new(11., 23., 72., 31.)), ..Default::default() }
}

fn original() -> Presentation {
    let mut doc = Presentation {
        notes_master: None,
        notes_masters: vec![NotesMaster {
            master: Arc::new(Master { id: MasterId(700), name: "Original notes owner".into(), shapes: vec![artwork(10)], ..Default::default() }),
            size: Size::new(480., 680.),
            fields: NotesFields { footer: Some("Source footer".into()), display_footer: Some(true), ..Default::default() },
        }],
        ..Default::default()
    };
    let slide = Arc::make_mut(&mut doc.slides[0]);
    slide.id = SlideId(600);
    slide.notes = TextBody::from_text("Original Unicode notes — café 漢字");
    slide.shapes = vec![artwork(10)];
    slide.notes_page = Some(NotesPage { master: Some(MasterId(700)), shapes: vec![artwork(10)], ..Default::default() });
    doc
}

fn install(session: &mut Session, doc: Presentation) {
    session
        .edit_resources(|live, sel| {
            *live = doc;
            sel.slide = 0;
            sel.slides.clear();
            sel.shapes.clear();
            sel.text = None;
            Ok(())
        })
        .unwrap();
}

fn copy_all(session: &mut Session) {
    session.execute("edit.selectAll", &json!({"scope":"slides"})).unwrap();
    session.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
}

fn copied() -> Session {
    let mut session = Session::with_new();
    install(&mut session, original());
    copy_all(&mut session);
    session
}

fn refuses_atomically(session: &mut Session, command: &str, params: serde_json::Value) {
    let before = session.doc().unwrap();
    let doc = before.doc.clone();
    let selection = before.selection.clone();
    let revision = before.revision;
    let history = (before.history.undo.len(), before.history.redo.len());
    let clipboard = format!("{:?}", session.clipboard);
    let journal = session.journal.len();
    let requests = format!("{:?}", session.ui_requests);
    let error = session.execute(command, &params).expect_err("unsupported notes transfer must refuse");
    assert!(error.to_string().contains("transfer"), "{error}");
    let after = session.doc().unwrap();
    assert!(Arc::ptr_eq(&doc, &after.doc));
    assert_eq!(after.selection, selection);
    assert_eq!(after.revision, revision);
    assert_eq!((after.history.undo.len(), after.history.redo.len()), history);
    assert_eq!(format!("{:?}", session.clipboard), clipboard);
    assert_eq!(session.journal.len(), journal);
    assert_eq!(format!("{:?}", session.ui_requests), requests);
}

fn excludes_external_transfer(source: Presentation) {
    let capture = notes_transfer::capture(&source, &[source.slides[0].as_ref()]).unwrap();
    assert!(capture.closure.is_none(), "unsupported source must never receive an external dependency receipt");
    assert!(capture.same_document_only.is_some(), "validated profile exclusion must retain its immutable reason");
    let mut session = Session::with_new();
    install(&mut session, source);
    copy_all(&mut session);
    assert!(session.clipboard.notes.is_none());
    session.execute("file.new", &json!({})).unwrap();
    refuses_atomically(&mut session, "edit.paste", json!({}));
}

#[test]
fn eligible_original_source_gets_a_real_shared_owner_closure() {
    let source = original();
    let capture = notes_transfer::capture(&source, &[source.slides[0].as_ref()]).unwrap();
    assert!(capture.same_document_only.is_none());
    assert!(capture.closure.is_some());
}

#[test]
fn same_document_table_notes_copy_keeps_live_owner_but_cannot_authorize_external_paste() {
    let mut source = original();
    Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[0].kind = ShapeKind::Table(Default::default());
    assert!(source.notes_page_error(&source.slides[0]).is_none());
    let mut session = Session::with_new();
    install(&mut session, source);
    copy_all(&mut session);
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.slides.len(), 2);
    assert_eq!(doc.notes_masters.len(), 1);
    assert_eq!(doc.slides[0].notes_page.as_ref().unwrap().master, doc.slides[1].notes_page.as_ref().unwrap().master);
    assert!(matches!(doc.slides[1].notes_page.as_ref().unwrap().shapes[0].kind, ShapeKind::Table(_)));
    session.execute("file.new", &json!({})).unwrap();
    refuses_atomically(&mut session, "edit.paste", json!({}));
}

#[test]
fn same_document_warning_origin_remains_copyable_and_never_becomes_external_proof() {
    let mut source = original();
    source.tags.push(("odp:warning".into(), "Original retained unsupported semantics".into()));
    let mut session = Session::with_new();
    install(&mut session, source);
    copy_all(&mut session);
    session.execute("edit.paste", &json!({})).unwrap();
    assert_eq!(session.doc().unwrap().doc.notes_masters.len(), 1);
    session.execute("file.new", &json!({})).unwrap();
    session
        .edit_resources(|doc, _| {
            doc.tags.push(("odp:warning".into(), "Unrelated destination warning".into()));
            Ok(())
        })
        .unwrap();
    refuses_atomically(&mut session, "edit.paste", json!({}));
}

#[test]
fn compact_recursive_destination_comments_are_bounded_before_counting_serializer() {
    let mut session = copied();
    session.execute("file.new", &json!({})).unwrap();
    let mut reply = Comment::default();
    for _ in 0..65 {
        reply = Comment { replies: vec![reply], ..Default::default() };
    }
    // Direct model input intentionally bypasses unrelated editor settlement.
    let state = session.doc_mut().unwrap();
    Arc::make_mut(&mut Arc::make_mut(&mut state.doc).slides[0]).comments = vec![reply];
    refuses_atomically(&mut session, "edit.paste", json!({}));
}

#[test]
fn copied_theme_opaque_xml_refuses_for_notes_and_regular_selected_owners() {
    for notes in [false, true] {
        let mut source = original();
        let master = if notes { &mut source.notes_masters[0].master } else { &mut source.masters[0] };
        Arc::make_mut(master).theme.raw_extra = Some("<originalUnmappedDependency r:id='missing'/>".into());
        excludes_external_transfer(source);
    }
}

#[test]
fn raw_transition_xml_cannot_escape_the_selected_dependency_closure() {
    let mut source = original();
    Arc::make_mut(&mut source.slides[0]).transition =
        Some(anim::Transition { raw: Some("<originalUnknownTransition r:embed='unclosed'/>".into()), ..Default::default() });
    excludes_external_transfer(source);
}

#[test]
fn opaque_effects_and_unwalked_picture_overlays_cannot_hide_dependencies() {
    for mode in 0..4 {
        let mut source = original();
        let effect = if mode % 2 == 0 {
            style::Effects { raw3d: Some("<originalUnknown3d/>".into()), ..Default::default() }
        } else {
            style::Effects {
                fill_overlay: Some(style::FillOverlay {
                    blend: "over".into(),
                    fill: Fill::Picture(PictureFill { media: MediaId(987_654), ..Default::default() }),
                }),
                ..Default::default()
            }
        };
        if mode < 2 {
            Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[0].effects = Some(effect);
        } else {
            Arc::make_mut(&mut source.notes_masters[0].master).theme.format.effects.push(effect);
        }
        excludes_external_transfer(source);
    }
}

fn connected_owner() -> Vec<Shape> {
    vec![
        Shape {
            id: ShapeId(11),
            kind: ShapeKind::Connector { start: Some((ShapeId(12), 7)), end: Some((ShapeId(13), 9)) },
            xfrm: Some(Xfrm::new(127., 39., 17., 63.)),
            ..Default::default()
        },
        Shape {
            id: ShapeId(14),
            kind: ShapeKind::Group { children: vec![artwork(12), artwork(13)], child: Xfrm::new(0., 0., 100., 100.) },
            ..Default::default()
        },
    ]
}

fn assert_connected(shapes: &[Shape]) -> HashSet<ShapeId> {
    let children = shapes[1].children();
    let ShapeKind::Connector { start, end } = shapes[0].kind else { panic!("expected connector") };
    assert_eq!(start, Some((children[0].id, 7)));
    assert_eq!(end, Some((children[1].id, 9)));
    assert_eq!(shapes[0].xfrm, Some(Xfrm::new(127., 39., 17., 63.)));
    let mut ids = HashSet::new();
    walk(shapes, &mut |shape, _| {
        assert!(ids.insert(shape.id));
    });
    ids
}

#[test]
fn forward_nested_connectors_rebind_per_owner_and_existing_destination_geometry_is_untouched() {
    let mut source = original();
    Arc::make_mut(&mut source.notes_masters[0].master).shapes = connected_owner();
    let slide = Arc::make_mut(&mut source.slides[0]);
    slide.shapes = connected_owner();
    slide.notes_page.as_mut().unwrap().shapes = connected_owner();
    let mut session = Session::with_new();
    install(&mut session, source);
    copy_all(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    let existing = {
        let state = session.doc_mut().unwrap();
        let doc = Arc::make_mut(&mut state.doc);
        Arc::make_mut(&mut doc.slides[0]).shapes = connected_owner();
        doc.next_id = 1;
        doc.slides[0].clone()
    };
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert!(Arc::ptr_eq(&existing, &doc.slides[0]));
    let slide = &doc.slides[1];
    let a = assert_connected(&slide.shapes);
    let b = assert_connected(&slide.notes_page.as_ref().unwrap().shapes);
    let c = assert_connected(&doc.notes_master_for(slide).unwrap().shapes);
    assert!(a.is_disjoint(&b) && a.is_disjoint(&c) && b.is_disjoint(&c));
}

#[test]
fn mixed_structured_and_ordinary_batch_closes_canonical_notes_and_artistic_original_media() {
    let mut source = original();
    source.media = [801, 802, 803]
        .into_iter()
        .map(|id| MediaItem {
            id: MediaId(id),
            name: format!("original-{id}"),
            content_type: "application/octet-stream".into(),
            data: Arc::new(vec![1, 2, 3, (id % 256) as u8]),
            link: None,
        })
        .collect();
    let mut ordinary = source.slides[0].as_ref().clone();
    ordinary.id = SlideId(601);
    ordinary.notes_page = None;
    ordinary.notes.paragraphs[0].props.bullet = Some(Bullet::Picture { media: MediaId(803) });
    ordinary.shapes[0].kind = ShapeKind::Picture {
        fill: PictureFill {
            media: MediaId(801),
            artistic: Some(Artistic { effect: "original-contributor-effect".into(), original: MediaId(802) }),
            ..Default::default()
        },
    };
    source.slides.push(Arc::new(ordinary));
    let mut session = Session::with_new();
    install(&mut session, source);
    copy_all(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.notes_masters.len(), 1);
    assert!(doc.slides[1].notes_page.is_some());
    assert!(doc.slides[2].notes_page.is_none());
    let mut refs = HashSet::new();
    crate::media_refs::visit::slide(&doc.slides[2], &mut |id| {
        refs.insert(*id);
    });
    assert_eq!(refs.len(), 3);
    assert_eq!(doc.media.len(), 3);
    for media in &doc.media {
        assert!(refs.contains(&media.id));
    }
    let bytes = deckcraft_format::save(doc).unwrap();
    let (restored, repairs) = deckcraft_format::load_checked(&bytes).unwrap();
    assert!(repairs.is_empty(), "{repairs:?}");
    assert_eq!(restored, **doc);
}

#[test]
fn source_wrapper_and_payload_edits_after_copy_cannot_change_captured_transfer() {
    let mut session = copied();
    session
        .edit_resources(|doc, _| {
            doc.notes_masters[0].size = Size::new(333., 555.);
            doc.notes_masters[0].fields.footer = Some("Later wrapper edit".into());
            Arc::make_mut(&mut doc.notes_masters[0].master).name = "Later master edit".into();
            Arc::make_mut(&mut doc.slides[0]).notes = TextBody::from_text("Later canonical edit");
            doc.notes_masters.clear();
            Ok(())
        })
        .unwrap();
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.notes_masters[0].size, Size::new(480., 680.));
    assert_eq!(doc.notes_masters[0].fields.footer.as_deref(), Some("Source footer"));
    assert_eq!(doc.notes_masters[0].master.name, "Original notes owner");
    assert_eq!(doc.slides[1].notes.text(), "Original Unicode notes — café 漢字");
}

#[test]
fn original_pixels_preserve_live_preview_ownership_without_persistent_thumbnail_media() {
    let mut source = original();
    source.notes_masters[0].fields = NotesFields::default();
    Arc::make_mut(&mut source.notes_masters[0].master).shapes = vec![Shape {
        id: ShapeId(15),
        xfrm: Some(Xfrm::new(60., 80., 320., 200.)),
        ph: Some(Placeholder { kind: PhType::SlideImage, ..Default::default() }),
        ..Default::default()
    }];
    let mut second = source.slides[0].as_ref().clone();
    second.id = SlideId(601);
    source.slides.push(Arc::new(second));
    for (slide, color) in source.slides.iter_mut().zip([Rgba::rgb(210, 30, 70), Rgba::rgb(20, 80, 210)]) {
        let slide = Arc::make_mut(slide);
        slide.notes = TextBody::default();
        slide.shapes.clear();
        slide.notes_page.as_mut().unwrap().shapes.clear();
        slide.background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(color)) });
    }
    let opts = deckcraft_render::RenderOpts { scale: 0.15, date_text: Some("2026-10-16".into()), ..Default::default() };
    let expected: Vec<_> = (0..2).map(|at| deckcraft_render::render_notes_page(&source, at, &opts).pixels).collect();
    assert_ne!(expected[0], expected[1]);
    let original_bytes = deckcraft_format::save(&source).unwrap();
    let mut session = Session::with_new();
    install(&mut session, source.clone());
    copy_all(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert!(doc.media.is_empty());
    assert_eq!(doc.notes_masters.len(), 1);
    assert_eq!(deckcraft_render::render_notes_page(doc, 1, &opts).pixels, expected[0]);
    assert_eq!(deckcraft_render::render_notes_page(doc, 2, &opts).pixels, expected[1]);
    let bytes = deckcraft_format::save(doc).unwrap();
    let (mut reopened, repairs) = deckcraft_format::load_checked(&bytes).unwrap();
    assert!(repairs.is_empty());
    reopened.slides.swap(1, 2);
    assert_eq!(deckcraft_render::render_notes_page(&reopened, 1, &opts).pixels, expected[1]);
    assert_eq!(deckcraft_render::render_notes_page(&reopened, 2, &opts).pixels, expected[0]);
    assert!(reopened.media.is_empty());
    assert_eq!(deckcraft_format::save(&source).unwrap(), original_bytes);
}

#[test]
fn equal_labels_and_payloads_from_distinct_notes_owners_do_not_merge() {
    let mut source = original();
    let mut other = source.notes_masters[0].clone();
    Arc::make_mut(&mut other.master).id = MasterId(701);
    source.notes_masters.push(other);
    let mut slide = source.slides[0].as_ref().clone();
    slide.id = SlideId(601);
    slide.notes_page.as_mut().unwrap().master = Some(MasterId(701));
    source.slides.push(Arc::new(slide));
    let mut session = Session::with_new();
    install(&mut session, source);
    copy_all(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.notes_masters.len(), 2);
    assert_eq!(doc.notes_masters[0].master.name, doc.notes_masters[1].master.name);
    assert_ne!(doc.slides[1].notes_page.as_ref().unwrap().master, doc.slides[2].notes_page.as_ref().unwrap().master);
    assert!(!Arc::ptr_eq(&doc.notes_masters[0].master, &doc.notes_masters[1].master));
}

#[test]
fn uncaptured_clipboard_origin_cannot_borrow_a_real_notes_receipt() {
    let mut session = copied();
    session.clipboard.origin = Default::default();
    session.execute("file.new", &json!({})).unwrap();
    refuses_atomically(&mut session, "edit.paste", json!({}));
}

#[test]
fn rejected_cut_preserves_previous_clipboard_and_source_document() {
    let mut session = copied();
    session
        .edit_resources(|doc, _| {
            Arc::make_mut(&mut doc.slides[0]).notes_page.as_mut().unwrap().body_shape = Some(ShapeId(999));
            Ok(())
        })
        .unwrap();
    refuses_atomically(&mut session, "edit.cut", json!({"scope":"slides"}));
}
