//! Contributor-original native notes fixtures exercise actual commands and resource history.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::{Session, notes_transfer};
use deckcraft_geom::{Size, Xfrm};
use deckcraft_model::{
    notes::{NotesDateTime, NotesFields, NotesMaster, NotesPage},
    style::PictureFill,
    text::{Action, Bullet, Hyperlink, Run, RunKind},
    *,
};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};

pub(crate) fn frame(id: u32, kind: PhType) -> Shape {
    Shape { id: ShapeId(id), ph: Some(Placeholder { kind, ..Default::default() }), xfrm: Some(Xfrm::new(20., 30., 160., 90.)), ..Default::default() }
}
pub(crate) fn fixture() -> Presentation {
    let mut source = Presentation {
        notes_master: None,
        notes_masters: (0..3)
            .map(|at| NotesMaster {
                master: Arc::new(Master {
                    id: MasterId(900 + at),
                    name: format!("Notes owner {at}"),
                    shapes: vec![frame(910, PhType::Body), frame(911, PhType::SlideImage)],
                    ..Default::default()
                }),
                size: Size::new(400. + f64::from(at), 600.),
                fields: NotesFields {
                    header: Some(format!("Owner {at}")),
                    footer: Some("Inherited footer".into()),
                    date_time: Some(NotesDateTime::Current { format: "datetime1".into(), lang: Some("en-US".into()) }),
                    display_header: Some(true),
                    display_page_number: Some(true),
                    ..Default::default()
                },
            })
            .collect(),
        ..Default::default()
    };
    let seed = source.slides[0].as_ref().clone();
    source.slides = (0..3)
        .map(|at| {
            let mut slide = seed.clone();
            slide.id = SlideId(100 + at);
            slide.name = format!("Selected {at}");
            slide.shapes = vec![Shape { id: ShapeId(910), xfrm: Some(Xfrm::new(5., 8., 90., 50.)), ..Default::default() }];
            slide.notes = TextBody::from_text(&format!("Canonical café 漢字 {at}"));
            slide.notes.paragraphs[0].runs[0].props.bold = Some(true);
            slide.notes_page = Some(NotesPage {
                master: Some(MasterId(if at < 2 { 900 } else { 901 })),
                shapes: vec![
                    frame(910, PhType::Body),
                    frame(911, PhType::SlideImage),
                    Shape {
                        id: ShapeId(912),
                        kind: ShapeKind::Connector { start: Some((ShapeId(910), 2)), end: Some((ShapeId(911), 1)) },
                        xfrm: Some(Xfrm::new(17., 18., 111., 44.)),
                        ..Default::default()
                    },
                ],
                body_shape: Some(ShapeId(910)),
                fields: NotesFields { header: Some(String::new()), display_header: Some(false), ..Default::default() },
                ..Default::default()
            });
            Arc::new(slide)
        })
        .collect();
    source.next_id = 20; // deliberately stale; all owner-local 910 IDs overlap.
    source
}
pub(crate) fn install(session: &mut Session, source: Presentation) {
    session
        .edit_resources(|doc, sel| {
            *doc = source;
            sel.slide = 0;
            sel.slides.clear();
            sel.shapes.clear();
            sel.text = None;
            Ok(())
        })
        .unwrap();
}
fn copy(session: &mut Session) {
    session.execute("edit.selectAll", &json!({"scope":"slides"})).unwrap();
    session.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
}
fn copied_source() -> Session {
    let mut session = Session::with_new();
    install(&mut session, fixture());
    copy(&mut session);
    session
}
pub(crate) fn refused(session: &mut Session, command: &str, params: serde_json::Value, phrase: &str) {
    let state = session.doc().unwrap();
    let before = state.doc.clone();
    let selection = state.selection.clone();
    let undo = state.history.undo.len();
    let redo = state.history.redo.len();
    let revision = state.revision;
    let clipboard = format!("{:?}", session.clipboard);
    let journal = session.journal.len();
    let requests = format!("{:?}", session.ui_requests);
    let error = session.execute(command, &params).unwrap_err().to_string();
    assert!(error.contains(phrase), "{command}: expected {phrase:?}, got {error}");
    let state = session.doc().unwrap();
    assert!(Arc::ptr_eq(&state.doc, &before));
    assert_eq!(state.selection, selection);
    assert_eq!((state.history.undo.len(), state.history.redo.len(), state.revision), (undo, redo, revision));
    assert_eq!(format!("{:?}", session.clipboard), clipboard);
    assert_eq!(session.journal.len(), journal);
    assert_eq!(format!("{:?}", session.ui_requests), requests);
}
fn assert_transferred(doc: &Presentation, first: usize) {
    let pages: Vec<_> = doc.slides.iter().skip(first).map(|slide| slide.notes_page.as_ref().unwrap()).collect();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0].master, pages[1].master);
    assert_ne!(pages[0].master, pages[2].master);
    for (at, slide) in doc.slides.iter().skip(first).enumerate() {
        assert!(doc.notes_page_error(slide).is_none());
        assert_eq!(slide.notes, fixture().slides[at].notes);
        let page = slide.notes_page.as_ref().unwrap();
        assert!(find_shape(&page.shapes, page.body_shape.unwrap()).unwrap().text.is_none());
        assert_eq!(page.shapes[1].ph_type(), Some(PhType::SlideImage));
        let ShapeKind::Connector { start, end } = page.shapes[2].kind else { panic!("connector") };
        assert_eq!(start, Some((page.shapes[0].id, 2)));
        assert_eq!(end, Some((page.shapes[1].id, 1)));
        assert_eq!(page.shapes[2].xfrm, Some(Xfrm::new(17., 18., 111., 44.)));
        assert_ne!(slide.shapes[0].id, page.shapes[0].id);
        assert_eq!(doc.notes_fields(slide).header.as_deref(), Some(""));
        assert_eq!(doc.notes_fields(slide).display_header, Some(false));
    }
}
#[test]
fn commands_capture_shared_owners_after_source_close_with_all_paste_aliases_and_history() {
    for alias in [None, Some("destinationTheme"), Some("useDestinationTheme"), Some("keepSource"), Some("keepSourceFormatting")] {
        let mut session = copied_source();
        let source = session.doc().unwrap().doc.clone();
        let source_bytes = deckcraft_format::save(&source).unwrap();
        session
            .edit_resources(|doc, _| {
                doc.notes_masters.clear();
                doc.media.clear();
                doc.tags.push(("odp:warning".into(), "Changed after capture".into()));
                Ok(())
            })
            .unwrap();
        session.execute("file.close", &json!({})).unwrap();
        session.execute("file.new", &json!({})).unwrap();
        let before = session.doc().unwrap().doc.clone();
        let undo = session.doc().unwrap().history.undo.len();
        match alias {
            None => session.execute("edit.paste", &json!({})),
            Some(alias) => session.execute("edit.pasteSpecial", &json!({"as":alias})),
        }
        .unwrap();
        let transferred = session.doc().unwrap().doc.clone();
        assert_eq!(transferred.notes_masters.len(), 2);
        assert_transferred(&transferred, 1);
        assert_eq!(session.doc().unwrap().history.undo.len(), undo + 1);
        assert_eq!(deckcraft_format::save(&source).unwrap(), source_bytes);
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(*session.doc().unwrap().doc, *before);
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(*session.doc().unwrap().doc, *transferred);
        for (left, right) in session.doc().unwrap().doc.notes_masters.iter().zip(&transferred.notes_masters) {
            assert!(Arc::ptr_eq(&left.master, &right.master));
        }
        let bytes = deckcraft_format::save(&transferred).unwrap();
        let (back, repairs) = deckcraft_format::load_checked(&bytes).unwrap();
        assert!(repairs.is_empty(), "{repairs:?}");
        assert_transferred(&back, 1);
        assert_eq!(deckcraft_format::save(&back).unwrap(), bytes);
    }
}
#[test]
fn reuse_both_modes_copies_real_notes_owners_and_undo_restores_resources() {
    let source = fixture();
    let bytes = deckcraft_format::save(&source).unwrap();
    for keep in [false, true] {
        let mut session = Session::with_new();
        let before = session.doc().unwrap().doc.clone();
        session.execute("slide.reuse", &json!({"name":"native.deckcraft", "data":crate::cmd::base64_encode(&bytes), "keepFormatting":keep})).unwrap();
        let doc = session.doc().unwrap().doc.clone();
        assert_eq!(doc.notes_masters.len(), 2);
        assert_transferred(&doc, 1);
        assert_eq!(doc.masters.len(), before.masters.len() + usize::from(keep));
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(*session.doc().unwrap().doc, *before);
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(*session.doc().unwrap().doc, *doc);
    }
}
#[test]
fn legacy_fallback_becomes_one_explicit_owner_and_preserves_destination_fallback() {
    let mut source = fixture();
    let owner = source.notes_masters.remove(0).master;
    source.notes_masters.clear();
    source.notes_master = Some(owner);
    source.notes_size = Size::new(450., 700.);
    source.slides.truncate(2);
    Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().master = None;
    Arc::make_mut(&mut source.slides[1]).notes_page.as_mut().unwrap().size = Some(Size::new(400., 500.));
    let mut session = Session::with_new();
    install(&mut session, source);
    copy(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    let fallback = Arc::new(Master { id: MasterId(800), name: "Destination fallback".into(), ..Default::default() });
    session
        .edit_resources(|doc, _| {
            doc.notes_master = Some(fallback.clone());
            Ok(())
        })
        .unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert!(Arc::ptr_eq(doc.notes_master.as_ref().unwrap(), &fallback));
    assert_eq!(doc.notes_masters.len(), 1);
    assert_eq!(doc.notes_masters[0].size, Size::new(450., 700.));
    assert_eq!(doc.notes_masters[0].fields, NotesFields::default());
    assert_eq!(doc.slides[1].notes_page.as_ref().unwrap().master, doc.slides[2].notes_page.as_ref().unwrap().master);
    assert_eq!(doc.notes_page_size(&doc.slides[1]), Size::new(450., 700.));
    assert_eq!(doc.notes_page_size(&doc.slides[2]), Size::new(400., 500.));
}
#[test]
fn owner_media_closure_includes_canonical_bullets_url_sound_backgrounds_theme_and_is_always_fresh() {
    let mut source = fixture();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([70, 120, 180, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    source.media = vec![MediaItem {
        id: MediaId(950),
        name: "original.png".into(),
        content_type: "image/png".into(),
        data: Arc::new(png.into_inner()),
        link: None,
    }];
    let pic = || Fill::Picture(PictureFill { media: MediaId(950), ..Default::default() });
    for slide in &mut source.slides {
        let slide = Arc::make_mut(slide);
        slide.notes.paragraphs[0].props.bullet = Some(Bullet::Picture { media: MediaId(950) });
        slide.notes.paragraphs[0].runs[0].props.link = Some(Hyperlink {
            action: Action::Url { url: "https://example.test/notes".into() },
            sound: Some(MediaId(950)),
            tooltip: "URL survives".into(),
            highlight_click: false,
            stop_sound: false,
            zoom_return: None,
            sound_name: None,
        });
        slide.notes_page.as_mut().unwrap().background = Some(Background::Fill { fill: pic() });
    }
    for owner in &mut source.notes_masters {
        Arc::make_mut(&mut owner.master).theme.format.bg_fills.push(pic());
    }
    let expected = source.media[0].clone();
    let mut session = Session::with_new();
    install(&mut session, source);
    copy(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    session
        .edit_resources(|doc, _| {
            doc.media.push(expected.clone());
            doc.next_id = 1;
            Ok(())
        })
        .unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.media.len(), 2);
    let copied = &doc.media[1];
    assert_ne!(copied.id, expected.id);
    assert!(Arc::ptr_eq(&copied.data, &expected.data));
    assert_eq!(copied.name, expected.name);
    assert_eq!(copied.content_type, expected.content_type);
    let mut refs = HashSet::new();
    for slide in &doc.slides[1..] {
        crate::media_refs::visit::slide(slide, &mut |id| {
            refs.insert(*id);
        });
    }
    for owner in &doc.notes_masters {
        crate::media_refs::visit::master(&owner.master, &mut |id| {
            refs.insert(*id);
        });
    }
    assert_eq!(refs, HashSet::from([copied.id]));
    assert!(!doc.media.iter().any(|item| item.name == "live-notes-thumbnail.png"));
}
#[test]
fn clipboard_subset_uses_only_selected_owner_and_rejects_snapshot_mutation() {
    let mut session = copied_source();
    session.clipboard.slides = vec![session.clipboard.slides[2].clone()];
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    assert_eq!(session.doc().unwrap().doc.notes_masters.len(), 1);
    assert_eq!(session.doc().unwrap().doc.notes_masters[0].master.name, "Notes owner 1");
    let mut session = copied_source();
    session.clipboard.slides[0].notes_page.as_mut().unwrap().master = Some(MasterId(902));
    session.execute("file.new", &json!({})).unwrap();
    refused(&mut session, "edit.paste", json!({}), "changed after notes closure");
}
#[test]
fn stale_allocator_scans_extra_master_layouts_and_identity_exhaustion_is_atomic() {
    let mut session = copied_source();
    session.execute("file.new", &json!({})).unwrap();
    session
        .edit_resources(|doc, _| {
            doc.handout_master = Some(Arc::new(Master {
                layouts: vec![Layout {
                    id: LayoutId(40_000),
                    shapes: vec![Shape { id: ShapeId(50_000), ..Default::default() }],
                    ..Default::default()
                }],
                ..Default::default()
            }));
            doc.next_id = 1;
            Ok(())
        })
        .unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert!(doc.slides[1].id.0 > 50_000);
    assert!(doc.notes_masters[0].master.id.0 > 50_000);
    let mut session = copied_source();
    session.execute("file.new", &json!({})).unwrap();
    session
        .edit_resources(|doc, _| {
            doc.next_id = u32::MAX - 2;
            Ok(())
        })
        .unwrap();
    refused(&mut session, "edit.paste", json!({}), "identity space exhausted");
}
#[test]
fn defaults_and_native_provenance_are_explicit_atomic_boundaries() {
    for mode in 0..5 {
        let mut session = copied_source();
        session.execute("file.new", &json!({})).unwrap();
        session
            .edit_resources(|doc, _| {
                match mode {
                    0 => doc.header_footer.footer_text = "Different global fallback".into(),
                    1 => doc.default_text_style.levels.push(Some(Default::default())),
                    2 => doc.tags.push(("odp:source-range".into(), "foreign".into())),
                    3 => doc.media.push(MediaItem {
                        id: MediaId(999),
                        content_type: "application/vnd.deckcraft.odp-source".into(),
                        data: Arc::new(vec![1]),
                        ..Default::default()
                    }),
                    _ => doc.embedded_fonts.push(Default::default()),
                };
                Ok(())
            })
            .unwrap();
        refused(
            &mut session,
            "edit.paste",
            json!({}),
            if mode < 2 {
                "must match exactly"
            } else if mode < 4 {
                "ODP provenance"
            } else {
                "embedded fonts"
            },
        );
    }
}
#[test]
fn invalid_source_graphs_refuse_copy_without_replacing_previous_clipboard() {
    for mode in 0..12 {
        let mut source = fixture();
        match mode {
            0 => source.notes_masters.clear(),
            1 => source.notes_masters.push(source.notes_masters[0].clone()),
            2 => Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().body_shape = Some(ShapeId(999)),
            3 => Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[0].text = Some(TextBody::from_text("duplicated")),
            4 => Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[1].id = ShapeId(910),
            5 => {
                Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[2].kind =
                    ShapeKind::Connector { start: Some((ShapeId(999), 0)), end: None }
            }
            6 => Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[1].raw_ext = Some("<unknown/>".into()),
            7 => Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().shapes[1].tags.push(("odp:part".into(), "foreign".into())),
            8 => Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap().size = Some(Size::new(f64::NAN, 500.)),
            9 => Arc::make_mut(&mut source.notes_masters[0].master).layouts.push(Layout::default()),
            10 => {
                Arc::make_mut(&mut source.slides[0]).shapes[0].click = Some(Hyperlink {
                    action: Action::Slide { slide: SlideId(100) },
                    tooltip: String::new(),
                    highlight_click: false,
                    sound: None,
                    stop_sound: false,
                    zoom_return: None,
                    sound_name: None,
                })
            }
            _ => Arc::make_mut(&mut source.slides[0]).shapes[0].name = "x".repeat(33 * 1024 * 1024),
        }
        let mut session = Session::with_new();
        session.clipboard.plain = "Previous clipboard".into();
        install(&mut session, source);
        // Directly select without running an editor validator on the deliberately hostile graph.
        session.doc_mut().unwrap().selection.slides = vec![SlideId(100)];
        if [6, 7, 9, 10].contains(&mode) {
            session.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
            assert!(session.clipboard.notes.is_none());
            session.execute("file.new", &json!({})).unwrap();
            refused(&mut session, "edit.paste", json!({}), "transfer refused");
        } else {
            refused(&mut session, "edit.copy", json!({"scope":"slides"}), "transfer refused");
        }
    }
}
#[test]
fn media_missing_empty_linked_and_duplicate_identity_refuse_before_copy() {
    for mode in 0..4 {
        let mut source = fixture();
        Arc::make_mut(&mut source.slides[0]).notes.paragraphs[0].props.bullet = Some(Bullet::Picture { media: MediaId(950) });
        if mode > 0 {
            source.media.push(MediaItem {
                id: MediaId(950),
                name: "media".into(),
                content_type: "image/png".into(),
                data: Arc::new(if mode == 1 { vec![] } else { vec![1, 2, 3] }),
                link: if mode == 2 { Some("https://example.test/image".into()) } else { None },
            });
        }
        if mode == 3 {
            source.media.push(source.media[0].clone());
        }
        let mut session = Session::with_new();
        install(&mut session, source);
        session.doc_mut().unwrap().selection.slides = vec![SlideId(100)];
        if mode == 2 {
            session.execute("edit.copy", &json!({"scope":"slides"})).unwrap();
            session.execute("file.new", &json!({})).unwrap();
            refused(&mut session, "edit.paste", json!({}), "linked media");
        } else {
            refused(
                &mut session,
                "edit.copy",
                json!({"scope":"slides"}),
                if mode == 0 {
                    "reference is missing"
                } else if mode == 3 {
                    "duplicate source media"
                } else {
                    "nonempty embedded"
                },
            );
        }
    }
}
#[test]
fn inherited_body_and_live_fields_stay_semantic_after_native_reopen() {
    let mut source = fixture();
    source.slides.truncate(1);
    let page = Arc::make_mut(&mut source.slides[0]).notes_page.as_mut().unwrap();
    page.shapes.clear();
    page.body_shape = None;
    let master = Arc::make_mut(&mut source.notes_masters[0].master);
    let mut field = frame(913, PhType::SlideNum);
    field.text = Some(TextBody::from_text("cached"));
    field.text.as_mut().unwrap().paragraphs[0].runs[0] = Run { kind: RunKind::Field { field: "slidenum".into() }, ..Run::new("cached") };
    master.shapes.push(field);
    let mut session = Session::with_new();
    install(&mut session, source);
    copy(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let bytes = deckcraft_format::save(&session.doc().unwrap().doc).unwrap();
    let (back, warnings) = deckcraft_format::load_checked(&bytes).unwrap();
    assert!(warnings.is_empty());
    let page = back.slides[1].notes_page.as_ref().unwrap();
    assert!(page.shapes.is_empty());
    assert!(page.body_shape.is_none());
    let drawing = deckcraft_render::notes_page_document(&back, 1, &Default::default()).unwrap();
    assert!(drawing.slides[0].shapes.iter().any(|shape| shape.ph_type() == Some(PhType::Body) && shape.text.as_ref() == Some(&back.slides[1].notes)));
    assert!(
        drawing.slides[0]
            .shapes
            .iter()
            .any(|shape| shape.ph_type() == Some(PhType::SlideNum) && shape.text.as_ref().is_some_and(|text| text.text() == "2"))
    );
    assert!(back.media.is_empty());
}
#[test]
fn closure_shape_depth_64_accepts_and_65_refuses_without_recursive_clone() {
    fn nested(depth: usize) -> Shape {
        let mut shape = Shape { id: ShapeId(999), ..Default::default() };
        for at in 0..depth {
            shape = Shape {
                id: ShapeId(2000 + at as u32),
                kind: ShapeKind::Group { children: vec![shape], child: Default::default() },
                ..Default::default()
            };
        }
        shape
    }
    for depth in [64, 65] {
        let mut source = fixture();
        source.slides.truncate(1);
        Arc::make_mut(&mut source.slides[0]).shapes = vec![nested(depth)];
        let selected = vec![source.slides[0].as_ref()];
        let got = notes_transfer::capture(&source, &selected);
        assert_eq!(got.is_ok(), depth == 64, "depth {depth}: {got:?}");
    }
}

#[test]
fn deterministic_notes_pixels_keep_each_live_preview_bound_to_its_new_owning_slide() {
    let mut source = fixture();
    source.slides.truncate(2);
    for (slide, color) in source.slides.iter_mut().zip([Rgba::rgb(210, 40, 70), Rgba::rgb(30, 90, 210)]) {
        Arc::make_mut(slide).background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(color)) });
    }
    let source_bytes = deckcraft_format::save(&source).unwrap();
    let opts = deckcraft_render::RenderOpts { scale: 0.2, date_text: Some("2026-10-16".into()), ..Default::default() };
    let source_pixels: Vec<_> = (0..2).map(|index| deckcraft_render::render_notes_page(&source, index, &opts).pixels).collect();
    assert_ne!(source_pixels[0], source_pixels[1]);
    let mut session = Session::with_new();
    install(&mut session, source.clone());
    copy(&mut session);
    session.execute("file.new", &json!({})).unwrap();
    session.execute("edit.paste", &json!({})).unwrap();
    let destination = session.doc().unwrap().doc.clone();
    for (index, expected) in source_pixels.iter().enumerate() {
        assert_eq!(&deckcraft_render::render_notes_page(&destination, index + 1, &opts).pixels, expected);
    }
    assert!(destination.media.is_empty());
    assert_eq!(deckcraft_format::save(&source).unwrap(), source_bytes);
    session
        .edit_resources(|doc, _| {
            doc.slides.swap(1, 2);
            Ok(())
        })
        .unwrap();
    let bytes = deckcraft_format::save(&session.doc().unwrap().doc).unwrap();
    let (mut back, warnings) = deckcraft_format::load_checked(&bytes).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(deckcraft_render::render_notes_page(&back, 1, &opts).pixels, source_pixels[1]);
    assert_eq!(deckcraft_render::render_notes_page(&back, 2, &opts).pixels, source_pixels[0]);
    Arc::make_mut(&mut back.slides[1]).background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(20, 200, 80))) });
    assert_ne!(deckcraft_render::render_notes_page(&back, 1, &opts).pixels, source_pixels[1]);
    assert_eq!(deckcraft_render::render_notes_page(&back, 2, &opts).pixels, source_pixels[0]);
    assert!(back.media.is_empty());
}

#[test]
fn cut_captures_notes_before_deletion_and_cross_document_paste_has_one_history_step() {
    let mut session = Session::with_new();
    install(&mut session, fixture());
    session.doc_mut().unwrap().selection.slides = vec![SlideId(100)];
    let before = session.doc().unwrap().doc.clone();
    let undo = session.doc().unwrap().history.undo.len();
    session.execute("edit.cut", &json!({"scope":"slides"})).unwrap();
    assert_eq!(session.doc().unwrap().doc.slides.len(), 2);
    assert_eq!(session.doc().unwrap().history.undo.len(), undo + 1);
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*session.doc().unwrap().doc, *before);
    session.execute("file.new", &json!({})).unwrap();
    let undo = session.doc().unwrap().history.undo.len();
    session.execute("edit.paste", &json!({})).unwrap();
    assert_eq!(session.doc().unwrap().history.undo.len(), undo + 1);
    let doc = &session.doc().unwrap().doc;
    assert_eq!(doc.notes_masters.len(), 1);
    assert_eq!(doc.slides[1].notes, before.slides[0].notes);
}

#[test]
fn warned_native_notes_copy_remains_same_document_only_and_keeps_immutable_loss_guard() {
    let mut source = fixture();
    source.tags.push(("odp:warning".into(), "Original source loss".into()));
    source.tags.push(("odp:imported".into(), "1".into()));
    source.fix_next_id();
    let mut session = Session::with_new();
    install(&mut session, source);
    copy(&mut session);
    assert!(session.clipboard.notes.is_none());
    session.execute("edit.paste", &json!({})).unwrap();
    session
        .edit_resources(|doc, _| {
            doc.tags.clear();
            Ok(())
        })
        .unwrap();
    refused(&mut session, "edit.paste", json!({}), "guard was cleared");
    session.execute("file.new", &json!({})).unwrap();
    refused(&mut session, "edit.paste", json!({}), "unattributed ODP loss");
}
