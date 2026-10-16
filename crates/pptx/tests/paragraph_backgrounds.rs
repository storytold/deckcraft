//! Contributor-original model fixtures. Paragraph fill must never silently become run highlighting.
use std::sync::Arc;

use deckcraft_model::{
    Fill, Presentation, Shape, ShapeKind, Table, TextBody,
    text::{LevelStyle, ParaProps},
};

fn painted() -> ParaProps {
    ParaProps { background: Some(Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::rgb(90, 130, 170)))), ..Default::default() }
}

fn body() -> TextBody {
    let mut body = TextBody::from_text("Editable paragraph");
    body.paragraphs[0].props = painted();
    body
}

fn rejected(p: &Presentation) {
    for ext in ["pptx", "potx", "ppsx"] {
        assert!(deckcraft_pptx::export_as(p, ext).unwrap_err().to_string().contains("paragraph backgrounds"));
    }
    assert!(deckcraft_pptx::export(p).is_err());
}

#[test]
fn pptx_core_refuses_paragraph_fill_in_all_text_owners() {
    for owner in 0..11 {
        let mut p = Presentation::default();
        let shape = Shape { text: Some(body()), ..Default::default() };
        match owner {
            0 => Arc::make_mut(&mut p.slides[0]).shapes = vec![shape],
            1 => Arc::make_mut(&mut p.slides[0]).notes = body(),
            2 => Arc::make_mut(&mut p.masters[0]).shapes = vec![shape],
            3 => Arc::make_mut(&mut p.masters[0]).layouts[0].shapes = vec![shape],
            4 => p.default_text_style.set(0, LevelStyle { para: painted(), ..Default::default() }),
            5 => Arc::make_mut(&mut p.masters[0]).body_style.set(0, LevelStyle { para: painted(), ..Default::default() }),
            6 => {
                let mut shape = Shape { text: Some(TextBody::from_text("Inherited")), ..Default::default() };
                shape.text.as_mut().unwrap().list_style.set(0, LevelStyle { para: painted(), ..Default::default() });
                Arc::make_mut(&mut p.slides[0]).shapes = vec![shape];
            }
            7 => {
                let mut table = Table::new(1, 1, 100.0, 40.0);
                table.rows[0].cells[0].text = body();
                Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { kind: ShapeKind::Table(table), ..Default::default() }];
            }
            8 => {
                p.notes_master = Some(p.masters[0].clone());
                Arc::make_mut(p.notes_master.as_mut().unwrap()).shapes = vec![shape];
            }
            9 => {
                p.handout_master = Some(p.masters[0].clone());
                Arc::make_mut(p.handout_master.as_mut().unwrap()).shapes = vec![shape];
            }
            _ => {
                Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape {
                    kind: ShapeKind::Group { children: vec![shape], child: deckcraft_model::Xfrm::new(0.0, 0.0, 100.0, 100.0) },
                    ..Default::default()
                }];
            }
        }
        rejected(&p);
    }
}

#[test]
fn explicit_no_fill_and_unstyled_old_documents_still_export() {
    let mut p = Presentation::default();
    assert!(deckcraft_pptx::export(&p).is_ok());
    let mut body = body();
    body.paragraphs[0].props.background = Some(Fill::None);
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { text: Some(body), ..Default::default() }];
    assert!(deckcraft_pptx::export(&p).is_ok());
}
