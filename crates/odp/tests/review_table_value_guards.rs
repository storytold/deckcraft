//! Independent core ODP tests permit verified typed mapping or a precise source-safe refusal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    table::{CellConstant, CellValue},
    *,
};
use std::{
    io::{Cursor, Read},
    sync::Arc,
};

fn table(stale: bool) -> Shape {
    let mut t = Table::new(1, 1, 80.0, 30.0);
    let c = &mut t.rows[0].cells[0];
    c.text = TextBody::from_text("shown");
    c.data = Some(Arc::new(CellConstant::from_display(CellValue::String { value: "OWNED-STORED-SCALAR".into() }, &c.text).unwrap()));
    if stale {
        c.text = TextBody::from_text("edited");
    }
    Shape { id: ShapeId(500), name: "independent typed table".into(), hidden: true, kind: ShapeKind::Table(t), ..Default::default() }
}
fn group(shape: Shape) -> Shape {
    Shape { id: ShapeId(501), kind: ShapeKind::Group { children: vec![shape], child: Xfrm::new(0.0, 0.0, 80.0, 30.0) }, ..Default::default() }
}
fn master(id: u32, shape: Shape, layout: bool) -> Arc<Master> {
    let mut m = Master { id: MasterId(id), layouts: vec![Layout { id: LayoutId(id + 1), ..Default::default() }], ..Default::default() };
    if layout {
        m.layouts[0].shapes.push(group(shape));
    } else {
        m.shapes.push(group(shape));
    }
    Arc::new(m)
}
fn fixture(owner: usize, stale: bool) -> Presentation {
    let mut p = Presentation {
        masters: vec![Arc::new(Master {
            id: MasterId(100),
            layouts: vec![Layout { id: LayoutId(101), ..Default::default() }],
            ..Default::default()
        })],
        slides: vec![Arc::new(Slide { id: SlideId(200), layout: LayoutId(101), hidden: true, show_master_shapes: false, ..Default::default() })],
        notes_master: None,
        ..Default::default()
    };
    p.notes_masters.clear();
    p.handout_master = None;
    let t = table(stale);
    match owner {
        0 => Arc::make_mut(&mut p.slides[0]).shapes.push(t),
        1 => Arc::make_mut(&mut p.slides[0]).shapes.push(group(t)),
        2 | 3 => p.masters.push(master(300, t, owner == 3)),
        4 | 5 => p.notes_master = Some(master(310, t, owner == 5)),
        6 | 7 => p.notes_masters.push(notes::NotesMaster { master: master(320, t, owner == 7), ..Default::default() }),
        8 | 9 => p.handout_master = Some(master(330, t, owner == 9)),
        _ => {
            Arc::make_mut(&mut p.slides[0]).notes_page =
                Some(notes::NotesPage { shapes: vec![group(t)], show_master_shapes: false, ..Default::default() })
        }
    }
    p.fix_next_id();
    p
}
fn cells(p: &Presentation) -> Vec<&Cell> {
    fn shapes<'a>(items: &'a [Shape], out: &mut Vec<&'a Cell>) {
        for s in items {
            match &s.kind {
                ShapeKind::Table(t) => out.extend(t.rows.iter().flat_map(|r| &r.cells)),
                ShapeKind::Group { children, .. } => shapes(children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for slide in &p.slides {
        shapes(&slide.shapes, &mut out);
        if let Some(notes) = &slide.notes_page {
            shapes(&notes.shapes, &mut out);
        }
    }
    for m in p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|n| &n.master)) {
        shapes(&m.shapes, &mut out);
        for l in &m.layouts {
            shapes(&l.shapes, &mut out);
        }
    }
    out
}

#[test]
fn direct_odp_export_must_preserve_or_precisely_refuse_every_typed_owner() {
    for owner in 0..11 {
        for stale in [false, true] {
            let p = fixture(owner, stale);
            let before = p.clone();
            match deckcraft_odp::export(&p) {
                Err(error) => {
                    let message = error.to_string();
                    assert!(
                        ["table", "constant", "typed", "owner", "master", "notes", "handout", "layout"].iter().any(|word| message.contains(word)),
                        "owner {owner}: {message}"
                    );
                }
                Ok(bytes) => {
                    let reopened = deckcraft_odp::import(&bytes).unwrap();
                    let back = cells(&reopened);
                    assert_eq!(back.len(), 1, "owner {owner}: success must not drop or duplicate a hidden/unreferenced table");
                    assert_eq!(back[0].text.text(), if stale { "edited" } else { "shown" });
                    if stale {
                        assert!(back[0].data.is_none());
                        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
                        for name in ["content.xml", "styles.xml"] {
                            let mut xml = String::new();
                            zip.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
                            assert!(!xml.contains("OWNED-STORED-SCALAR"), "stale scalar escaped into {name}");
                        }
                    } else {
                        assert_eq!(back[0].data.as_ref().unwrap().value(), cells(&p)[0].data.as_ref().unwrap().value());
                    }
                }
            }
            assert_eq!(p, before, "export modified original owner {owner}");
        }
    }
}

#[test]
fn unsupported_source_preservation_lane_cannot_publish_invalid_covered_constants() {
    let mut p = fixture(0, true);
    p.tags.push(("odp:warning".into(), "independent unmapped source".into()));
    let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
    t.rows[0].cells[0].h_merge = true;
    let error = deckcraft_odp::export(&p).unwrap_err().to_string();
    assert!(error.contains("covered table cells"), "raw validation must precede the preservation lane: {error}");
}
