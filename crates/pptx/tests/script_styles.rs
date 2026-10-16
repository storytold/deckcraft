//! Contributor-original script overrides must not be silently removed by another format.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    Presentation, Shape, ShapeKind, Table, TextBody,
    text::{LevelStyle, ListStyle, RunProps},
};
use std::sync::Arc;

fn reject(deck: &Presentation) {
    assert!(deckcraft_pptx::export(deck).unwrap_err().to_string().contains("independent East Asian or complex-script"));
}
#[test]
fn every_independent_script_override_is_guarded_by_core_export() {
    for props in [
        RunProps { size_ea: Some(24.), ..Default::default() },
        RunProps { size_cs: Some(30.), ..Default::default() },
        RunProps { bold_ea: Some(false), ..Default::default() },
        RunProps { bold_cs: Some(true), ..Default::default() },
        RunProps { italic_ea: Some(true), ..Default::default() },
        RunProps { italic_cs: Some(false), ..Default::default() },
        RunProps { lang_ea: Some("ja".into()), ..Default::default() },
        RunProps { lang_cs: Some(String::new()), ..Default::default() },
    ] {
        let mut deck = Presentation::default();
        let mut text = TextBody::from_text("Independent styles");
        text.paragraphs[0].runs[0].props = props;
        Arc::make_mut(&mut deck.slides[0]).shapes.push(Shape { text: Some(text), ..Default::default() });
        reject(&deck);
    }
}
#[test]
fn script_preflight_covers_empty_paragraph_defaults_end_marks_tables_and_master_levels() {
    let props = RunProps { size_cs: Some(30.), ..Default::default() };
    for owner in 0..6 {
        let mut deck = Presentation::default();
        let mut text = TextBody::from_text("");
        match owner {
            0 => text.paragraphs[0].def_props = props.clone(),
            1 => text.paragraphs[0].end_props = props.clone(),
            _ => text.list_style.set(8, LevelStyle { run: props.clone(), ..Default::default() }),
        }
        match owner {
            3 => Arc::make_mut(&mut deck.slides[0]).notes = text,
            4 => {
                let mut table = Table::new(1, 1, 200., 40.);
                table.rows[0].cells[0].text = text;
                Arc::make_mut(&mut deck.slides[0]).shapes.push(Shape { kind: ShapeKind::Table(table), ..Default::default() });
            }
            5 => {
                Arc::make_mut(&mut deck.masters[0]).body_style =
                    ListStyle { levels: vec![Some(LevelStyle { run: props.clone(), ..Default::default() })] }
            }
            _ => Arc::make_mut(&mut deck.slides[0]).shapes.push(Shape { text: Some(text), ..Default::default() }),
        }
        reject(&deck);
    }
    assert!(deckcraft_pptx::export(&Presentation::default()).is_ok());
}

#[test]
fn explicit_ordinary_style_can_represent_equal_explicit_script_values() {
    let mut deck = Presentation::default();
    let mut text = TextBody::from_text("One explicit style for every script");
    text.paragraphs[0].runs[0].props = RunProps {
        size: Some(24.),
        size_ea: Some(24.),
        size_cs: Some(24.),
        bold: Some(true),
        bold_ea: Some(true),
        bold_cs: Some(true),
        italic: Some(false),
        italic_ea: Some(false),
        italic_cs: Some(false),
        lang: Some("en".into()),
        lang_ea: Some("en".into()),
        lang_cs: Some("en".into()),
        ..Default::default()
    };
    Arc::make_mut(&mut deck.slides[0]).shapes.push(Shape { text: Some(text), ..Default::default() });
    let encoded = deckcraft_pptx::export(&deck).unwrap();
    let reopened = deckcraft_pptx::import(&encoded).unwrap();
    let text = reopened.slides[0]
        .shapes
        .iter()
        .filter_map(|shape| shape.text.as_ref())
        .find(|text| text.text() == "One explicit style for every script")
        .unwrap();
    let props = &text.paragraphs[0].runs[0].props;
    assert_eq!(props.size, Some(24.));
    assert_eq!(props.bold, Some(true));
    assert_eq!(props.italic, Some(false));
    assert_eq!(props.lang.as_deref(), Some("en"));
    assert!(!props.has_script_overrides());
}
