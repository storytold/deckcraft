//! Contributor-original numbering fixtures. Explicit sequence boundaries must not
//! silently degrade to inherited bullet defaults during PPTX conversion.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    text::{Bullet, LevelStyle, ParaProps},
    *,
};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

fn numbered(restart: Option<u32>, start_at: u32) -> TextBody {
    let mut body = TextBody::from_text("First item\nSecond item");
    for paragraph in &mut body.paragraphs {
        paragraph.props.bullet = Some(Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at });
    }
    body.paragraphs[0].props.number_restart = restart;
    body
}
fn shape(body: TextBody) -> Shape {
    Shape { xfrm: Some(Xfrm::new(50., 50., 500., 300.)), text: Some(body), ..Default::default() }
}
fn local_deck(body: TextBody) -> Presentation {
    let mut presentation = Presentation::default();
    Arc::make_mut(&mut presentation.slides[0]).shapes = vec![shape(body)];
    presentation
}
fn refused(presentation: &Presentation) {
    for format in ["pptx", "potx", "ppsx"] {
        let error = deckcraft_pptx::export_as(presentation, format).unwrap_err().to_string();
        assert!(error.contains("number"), "numbering refusal should explain the problem: {error}");
    }
    assert!(deckcraft_pptx::export(presentation).is_err());
}
fn replace_numbering(bytes: &[u8], value: &str) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut dst = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut entry = src.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if name == "ppt/slides/slide1.xml" {
            let text = String::from_utf8(data)
                .unwrap()
                .replace("<a:buAutoNum type=\"arabicPeriod\"/>", &format!("<a:buAutoNum type=\"arabicPeriod\" startAt=\"{value}\"/>"));
            assert!(text.contains(&format!("startAt=\"{value}\"")));
            data = text.into_bytes();
        }
        dst.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        dst.write_all(&data).unwrap();
    }
    dst.finish().unwrap().into_inner()
}

#[test]
fn core_pptx_refuses_unverified_explicit_restarts_in_every_text_owner() {
    for owner in 0..14 {
        let mut presentation = Presentation::default();
        let body = numbered(Some(1), 1);
        let item = shape(body.clone());
        match owner {
            0 => Arc::make_mut(&mut presentation.slides[0]).shapes = vec![item],
            1 => Arc::make_mut(&mut presentation.slides[0]).notes = body,
            2 => Arc::make_mut(&mut presentation.masters[0]).shapes = vec![item],
            3 => Arc::make_mut(&mut presentation.masters[0]).layouts[0].shapes = vec![item],
            4 => presentation.default_text_style.set(0, LevelStyle { para: body.paragraphs[0].props.clone(), ..Default::default() }),
            5 => Arc::make_mut(&mut presentation.masters[0])
                .body_style
                .set(0, LevelStyle { para: body.paragraphs[0].props.clone(), ..Default::default() }),
            6 => {
                let mut inherited = TextBody::from_text("Inherited numbering");
                inherited.list_style.set(0, LevelStyle { para: body.paragraphs[0].props.clone(), ..Default::default() });
                Arc::make_mut(&mut presentation.slides[0]).shapes = vec![shape(inherited)];
            }
            7 => {
                let mut table = Table::new(1, 1, 100., 40.);
                table.rows[0].cells[0].text = body;
                Arc::make_mut(&mut presentation.slides[0]).shapes = vec![Shape { kind: ShapeKind::Table(table), ..Default::default() }];
            }
            8 => {
                presentation.notes_master = Some(presentation.masters[0].clone());
                Arc::make_mut(presentation.notes_master.as_mut().unwrap()).shapes = vec![item];
            }
            9 => {
                presentation.handout_master = Some(presentation.masters[0].clone());
                Arc::make_mut(presentation.handout_master.as_mut().unwrap()).shapes = vec![item];
            }
            10 => {
                Arc::make_mut(&mut presentation.slides[0]).shapes =
                    vec![Shape { kind: ShapeKind::Group { children: vec![item], child: Xfrm::new(0., 0., 500., 300.) }, ..Default::default() }];
            }
            11 => {
                let paragraph = &mut Arc::make_mut(&mut presentation.slides[0]).notes;
                paragraph.paragraphs =
                    vec![text::Paragraph { props: ParaProps { number_restart: Some(1), ..Default::default() }, ..Default::default() }];
            }
            12 => {
                Arc::make_mut(&mut presentation.slides[0]).notes_page = Some(notes::NotesPage { shapes: vec![item], ..Default::default() });
            }
            _ => {
                let mut master = notes::NotesMaster::default();
                Arc::make_mut(&mut master.master).shapes = vec![item];
                presentation.notes_masters.push(master);
            }
        }
        refused(&presentation);
    }
}

#[test]
fn plain_empty_and_nested_paragraph_boundaries_remain_guarded() {
    for level in [0, 4, 8] {
        for empty in [false, true] {
            for restart in [0, 1, 7, 32767, 32768, u32::MAX] {
                let mut body = numbered(None, 1);
                body.paragraphs[1].level = level;
                body.paragraphs[1].props.number_restart = Some(restart);
                if empty {
                    body.paragraphs[1].runs.clear();
                }
                body.paragraphs.insert(1, text::Paragraph::new("Intervening plain paragraph"));
                refused(&local_deck(body));
            }
        }
    }
}

#[test]
fn out_of_range_bullet_starts_are_rejected_without_clamping() {
    for value in [0, 32768, u32::MAX] {
        refused(&local_deck(numbered(None, value)));
        let mut presentation = Presentation::default();
        presentation.default_text_style.set(8, LevelStyle { para: numbered(None, value).paragraphs[0].props.clone(), ..Default::default() });
        refused(&presentation);
    }
}

#[test]
fn import_retains_out_of_range_numeric_start_values_for_export_preflight() {
    let source = deckcraft_pptx::export(&local_deck(numbered(None, 1))).unwrap();
    for value in [0, 32768, u32::MAX] {
        let imported = deckcraft_pptx::import(&replace_numbering(&source, &value.to_string())).unwrap();
        let props = &imported.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].props;
        assert!(matches!(props.bullet, Some(Bullet::AutoNum { start_at, .. }) if start_at == value));
        refused(&imported);
    }
}

#[test]
fn malformed_start_attributes_cannot_turn_into_exportable_defaults() {
    let source = deckcraft_pptx::export(&local_deck(numbered(None, 1))).unwrap();
    for value in ["-1", "4294967296", "1.5", "NaN", ""] {
        let imported = deckcraft_pptx::import(&replace_numbering(&source, value)).unwrap();
        refused(&imported);
    }
}

#[test]
fn supported_sequence_starts_and_continuation_do_not_become_paragraph_restarts() {
    for value in [1, 7, 32767] {
        let source = deckcraft_pptx::export(&local_deck(numbered(None, value))).unwrap();
        let imported = deckcraft_pptx::import(&source).unwrap();
        for paragraph in &imported.slides[0].shapes[0].text.as_ref().unwrap().paragraphs {
            assert_eq!(paragraph.props.number_restart, None);
            assert!(matches!(paragraph.props.bullet, Some(Bullet::AutoNum { start_at, .. }) if start_at == value));
        }
        assert!(deckcraft_pptx::export(&imported).is_ok());
    }
}
