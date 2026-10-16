//! Original native line policies must never silently become exact PPTX spacing.
use deckcraft_model::{
    text::{LevelStyle, LineHeight, ParaProps},
    *,
};
use std::sync::Arc;
fn policy_body(policy: LineHeight) -> TextBody {
    let mut body = TextBody::from_text("Original line settings");
    body.paragraphs[0].props.line_height = Some(policy);
    body
}
fn shape(body: TextBody) -> Shape {
    Shape { text: Some(body), ..Default::default() }
}
fn refused(presentation: &Presentation) {
    let original = presentation.clone();
    for ext in ["pptx", "potx", "ppsx"] {
        assert!(deckcraft_pptx::export_as(presentation, ext).unwrap_err().to_string().contains("line-height"));
        assert_eq!(presentation, &original);
    }
}
#[test]
fn core_pptx_refuses_new_line_policies_in_every_text_owner() {
    for policy in [LineHeight::Normal, LineHeight::Minimum { points: 28.0 }] {
        for owner in 0..14 {
            let mut presentation = Presentation::default();
            let body = policy_body(policy);
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
                        vec![text::Paragraph { props: ParaProps { line_height: Some(policy), ..Default::default() }, ..Default::default() }];
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
}
