//! Real dialog actions keep format drafts isolated and commit through commands.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_model::{
    ShapeId, ShapeKind,
    table::{
        Cell, DateFormatPart, NegativeDisplay, NumberComparison, NumberFormat, NumberFormatSection, NumberLocale, NumberStyle, Table, TablePath,
    },
};
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

use crate::{Services, SlideApp, dialogs::Dialog};

fn complex_format() -> NumberFormat {
    NumberFormat {
        name: "Quarterly custom".into(),
        locale: NumberLocale::FrBe,
        style: NumberStyle::Number { decimal_places: 3, min_decimal_places: 3, min_integer_digits: 1, grouping: true },
        prefix: "base ".into(),
        suffix: " units".into(),
        negative: NegativeDisplay::Parentheses,
        zero_display: Some("no activity".into()),
        color: Some([12, 34, 56, 255]),
        sections: vec![NumberFormatSection {
            comparison: NumberComparison::Greater,
            threshold: "1000".into(),
            style: NumberStyle::DateTime {
                parts: vec![
                    DateFormatPart::Year { long: true },
                    DateFormatPart::Literal { text: " custom ".into() },
                    DateFormatPart::Month { long: false, textual: true },
                    DateFormatPart::Zone,
                ],
            },
            prefix: "section ".into(),
            suffix: " section end".into(),
            negative: NegativeDisplay::Minus,
            color: Some([100, 20, 30, 255]),
        }],
    }
}

fn harness(format: Option<NumberFormat>, nested: bool) -> (Harness<'static, SlideApp>, ShapeId, TablePath) {
    let mut session = Session::with_new();
    let result = session.execute("insert.table", &json!({"rows":2,"cols":2,"rect":[80,80,420,140]})).unwrap();
    let id: ShapeId = serde_json::from_value(result["id"].clone()).unwrap();
    let path = if nested {
        session.execute("table.insertNested", &json!({"id":id,"row":1,"col":1,"rows":1,"cols":1})).unwrap();
        session.active().unwrap().selection.table_path.clone()
    } else {
        TablePath::default()
    };
    session.execute("table.setCellValue", &json!({"id":id,"path":path,"row":0,"col":0,"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"12.345"},"display":"12.345"})).unwrap();
    if let Some(format) = format {
        session.execute("table.setCellNumberFormat", &json!({"id":id,"path":path,"row":0,"col":0,"format":format})).unwrap();
    }
    let mut app = SlideApp::new(session, Services::default());
    app.ui.tab = "Layout".into();
    let mut h = Harness::builder().with_size(egui::vec2(1500., 1000.)).with_step_dt(1. / 60.).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, id, path)
}

fn root<'a>(h: &'a Harness<'_, SlideApp>, id: ShapeId) -> &'a Table {
    let ShapeKind::Table(table) = &h.state().session.active().unwrap().shape(id).unwrap().kind else { panic!("table") };
    table
}
fn cell<'a>(h: &'a Harness<'_, SlideApp>, id: ShapeId, path: &TablePath) -> &'a Cell {
    root(h, id).at_path(path).unwrap().cell(0, 0).unwrap()
}
fn open(h: &mut Harness<'_, SlideApp>) {
    h.state_mut().dialog = Some(Dialog::new("cellNumberFormat"));
    h.run_steps(3);
    assert!(h.state().dialog.as_ref().unwrap().params.get("target").is_some());
}
fn draft(h: &mut Harness<'_, SlideApp>, fields: &[(&str, &str)]) {
    let d = h.state_mut().dialog.as_mut().unwrap();
    for (key, value) in fields {
        d.fields.insert((*key).into(), (*value).into());
    }
}
fn apply(h: &mut Harness<'_, SlideApp>) {
    h.get_by_label("Apply").click_accesskit();
    h.run_steps(2);
}

#[test]
fn ribbon_opens_current_summary_and_keeps_complex_format_exactly() {
    let format = complex_format();
    let (mut h, id, path) = harness(Some(format.clone()), false);
    h.get_by_label("Number Format…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().id, "cellNumberFormat");
    assert!(h.query_by_label_contains("Current format: Quarterly custom").is_some());
    assert!(h.query_by_label("Keep current format").is_some());
    assert!(h.query_by_label("Decimal places").is_none());
    assert!(h.query_by_label("Prefix").is_none());
    if let Ok(path) = std::env::var("DECKCRAFT_TABLE_FORMAT_QA_PNG") {
        h.render().unwrap().save(path).unwrap();
    }
    apply(&mut h);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().status);
    assert_eq!(cell(&h, id, &path).number_format.as_deref(), Some(&format));
}

#[test]
fn explicit_replacement_applies_visible_controls_and_undo_restores_every_custom_field() {
    let format = complex_format();
    let (mut h, id, path) = harness(Some(format.clone()), false);
    open(&mut h);
    let before = h.state().session.active().unwrap().doc.clone();
    let undo = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("Replace current format").click();
    h.run_steps(2);
    assert!(h.query_by_label("Decimal places").is_some());
    draft(&mut h, &[("format", "Number"), ("decimals", "1"), ("locale", "English (US)"), ("prefix", "sold "), ("suffix", "")]);
    h.run_steps(1);
    if let Ok(path) = std::env::var("DECKCRAFT_TABLE_FORMAT_REPLACEMENT_QA_PNG") {
        h.render().unwrap().save(path).unwrap();
    }
    apply(&mut h);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().status);
    let current = cell(&h, id, &path);
    assert_eq!(current.text.text(), "sold 12.3");
    assert!(current.number_format.as_ref().unwrap().sections.is_empty());
    assert!(current.number_format.as_ref().unwrap().name.is_empty());
    assert_eq!(
        current.data.as_ref().unwrap().value(),
        &deckcraft_model::table::CellValue::Number { kind: deckcraft_model::table::CellNumberKind::Float, lexical: "12.345".into() }
    );
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo + 1);
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(h.state().session.active().unwrap().doc, before);
    assert_eq!(cell(&h, id, &path).number_format.as_deref(), Some(&format));
}

#[test]
fn affix_edit_preserves_conditional_sections_custom_date_parts_and_locale() {
    let format = complex_format();
    let (mut h, id, path) = harness(Some(format.clone()), false);
    open(&mut h);
    h.get_by_label("Edit prefix and suffix").click();
    h.run_steps(2);
    assert!(h.query_by_label("Prefix").is_some());
    assert!(h.query_by_label("Decimal places").is_none());
    assert!(h.query_by_label_contains("Conditional sections keep their own").is_some());
    draft(&mut h, &[("prefix", "revenue "), ("suffix", " EUR")]);
    apply(&mut h);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().status);
    let mut expected = format;
    expected.prefix = "revenue ".into();
    expected.suffix = " EUR".into();
    assert_eq!(cell(&h, id, &path).number_format.as_deref(), Some(&expected));
    assert_eq!(cell(&h, id, &path).text.text(), "revenue 12,345 EUR");
}

#[test]
fn cancel_discards_replacement_draft_without_history_or_document_changes() {
    let (mut h, _, _) = harness(Some(complex_format()), false);
    open(&mut h);
    let before = h.state().session.active().unwrap().doc.clone();
    let revision = h.state().session.active().unwrap().revision;
    let undo = h.state().session.active().unwrap().history.undo.len();
    h.get_by_label("Replace current format").click();
    h.run_steps(2);
    draft(&mut h, &[("format", "Currency"), ("currency", "USD"), ("symbol", "$"), ("prefix", "pending ")]);
    h.run_steps(1);
    h.get_by_label("Cancel").click_accesskit();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let st = h.state().session.active().unwrap();
    assert!(Arc::ptr_eq(&st.doc, &before));
    assert_eq!(st.revision, revision);
    assert_eq!(st.history.undo.len(), undo);
}

#[test]
fn invalid_visible_replacement_stays_open_and_can_be_corrected() {
    let (mut h, id, path) = harness(None, false);
    open(&mut h);
    let before = h.state().session.active().unwrap().doc.clone();
    let undo = h.state().session.active().unwrap().history.undo.len();
    draft(&mut h, &[("decimals", "29")]);
    apply(&mut h);
    assert!(h.state().dialog.is_some());
    assert!(h.query_by_label("Decimal places must be between 0 and 28.").is_some());
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before));
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo);
    draft(&mut h, &[("decimals", "2")]);
    apply(&mut h);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().status);
    assert_eq!(cell(&h, id, &path).text.text(), "12.35");
}

#[test]
fn existing_affix_draft_is_validated_without_replacing_the_program() {
    let (mut h, _, _) = harness(Some(complex_format()), false);
    open(&mut h);
    let before = h.state().session.active().unwrap().doc.clone();
    h.get_by_label("Edit prefix and suffix").click();
    h.run_steps(2);
    let too_long = "x".repeat(1025);
    draft(&mut h, &[("prefix", &too_long)]);
    apply(&mut h);
    assert!(h.state().dialog.as_ref().unwrap().fields.contains_key("error"));
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before));
}

#[test]
fn changed_revision_refuses_apply_and_retains_the_unapplied_draft() {
    let (mut h, id, _) = harness(None, false);
    open(&mut h);
    draft(&mut h, &[("decimals", "4")]);
    h.state_mut()
        .session
        .execute("table.setCellValue", &json!({"id":id,"row":0,"col":1,"value":{"kind":"boolean","value":true},"display":"TRUE"}))
        .unwrap();
    let before = h.state().session.active().unwrap().doc.clone();
    let undo = h.state().session.active().unwrap().history.undo.len();
    apply(&mut h);
    assert!(h.state().dialog.is_some());
    assert!(h.query_by_label_contains("The presentation changed while this dialog was open").is_some());
    assert_eq!(h.state().dialog.as_ref().unwrap().fields["decimals"], "4");
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before));
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo);
}

#[test]
fn different_active_document_refuses_apply_to_both_documents() {
    let (mut h, _, _) = harness(None, false);
    open(&mut h);
    let original = h.state().session.active().unwrap().doc.clone();
    h.state_mut().session.execute("file.new", &json!({})).unwrap();
    let current = h.state().session.active().unwrap().doc.clone();
    apply(&mut h);
    assert!(h.state().dialog.is_some());
    assert!(h.query_by_label("The original presentation is no longer active.").is_some());
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &current));
    assert!(Arc::ptr_eq(&h.state().session.documents()[0].doc, &original));
}

#[test]
fn nested_selected_cell_uses_the_captured_path_and_undo_restores_it() {
    let (mut h, id, path) = harness(None, true);
    open(&mut h);
    assert_eq!(h.state().dialog.as_ref().unwrap().params["target"]["path"], json!(path));
    let parent = root(&h, id).cell(0, 0).unwrap().clone();
    draft(&mut h, &[("decimals", "1")]);
    apply(&mut h);
    assert!(h.state().dialog.is_none(), "status {:?}; dialog {:?}", h.state().status, h.state().dialog);
    assert_eq!(cell(&h, id, &path).text.text(), "12.3");
    assert!(cell(&h, id, &path).number_format.is_some());
    assert_eq!(root(&h, id).cell(0, 0).unwrap(), &parent);
    h.state_mut().session.execute("edit.undo", &json!({})).unwrap();
    assert!(cell(&h, id, &path).number_format.is_none());
    assert_eq!(cell(&h, id, &path).text.text(), "12.345");
}

#[test]
fn leaving_nested_selection_bumps_revision_and_cannot_retarget_an_open_draft() {
    let (mut h, id, path) = harness(None, true);
    open(&mut h);
    let target = h.state().dialog.as_ref().unwrap().params["target"].clone();
    let revision = h.state().session.active().unwrap().revision;
    h.state_mut().session.execute("table.leaveNested", &json!({"id":id})).unwrap();
    assert!(h.state().session.active().unwrap().revision > revision);
    let before = h.state().session.active().unwrap().doc.clone();
    let undo = h.state().session.active().unwrap().history.undo.len();
    draft(&mut h, &[("decimals", "1")]);
    apply(&mut h);
    let dialog = h.state().dialog.as_ref().unwrap();
    assert_eq!(dialog.params["target"], target);
    assert!(dialog.fields.get("error").unwrap().contains("The presentation changed while this dialog was open"));
    assert_eq!(dialog.fields.get("decimals").map(String::as_str), Some("1"));
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &before));
    assert_eq!(h.state().session.active().unwrap().history.undo.len(), undo);
    assert!(cell(&h, id, &path).number_format.is_none());
    assert!(root(&h, id).cell(0, 0).unwrap().number_format.is_none());
}
