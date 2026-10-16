//! Contributor-original formula profile, dependency and owner regressions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    Layout, Master, Presentation, Run, Shape, ShapeKind, Slide, Table, TextBody, Xfrm,
    notes::{NotesMaster, NotesPage},
    table::{
        CellConstant, CellFormula, CellNumberKind, CellValue, FormulaError, FormulaOutcome, reconcile_constants, reject_constants,
        validate_constants, validate_formula_topology,
    },
};
use std::sync::Arc;

fn numeric(table: &mut Table, row: usize, col: usize, lexical: &str) {
    let cell = table.cell_mut(row, col).unwrap();
    cell.text = TextBody::from_text(lexical);
    cell.formula = None;
    cell.data =
        Some(Arc::new(CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Float, lexical: lexical.into() }, &cell.text).unwrap()));
}
fn attach(table: &mut Table, row: usize, col: usize, source: &str, lexical: &str, display: &str) {
    let cell = table.cell_mut(row, col).unwrap();
    cell.data = None;
    cell.text = TextBody::from_text(display);
    cell.formula = Some(Arc::new(CellFormula::number(source.into(), lexical.into(), &cell.text).unwrap()));
}
fn text(table: &Table, col: usize) -> String {
    CellFormula::display_text(&table.cell(0, col).unwrap().text).unwrap()
}
fn outcome(table: &Table, col: usize) -> &FormulaOutcome {
    table.cell(0, col).unwrap().formula.as_ref().unwrap().outcome()
}
fn shape(table: Table) -> Shape {
    Shape { kind: ShapeKind::Table(table), ..Default::default() }
}
fn document(table: Table) -> Presentation {
    Presentation {
        slides: vec![Arc::new(Slide { shapes: vec![shape(table)], ..Default::default() })],
        masters: Vec::new(),
        notes_master: None,
        notes_masters: Vec::new(),
        handout_master: None,
        ..Default::default()
    }
}
fn table(p: &Presentation) -> &Table {
    match &p.slides[0].shapes[0].kind {
        ShapeKind::Table(table) => table,
        _ => panic!("expected table"),
    }
}

#[test]
fn arithmetic_is_exact_and_obeys_precedence_unary_and_parentheses() {
    for (source, expected) in [
        ("=9007199254740993+1", "9007199254740994"),
        ("=0.1+0.2", "0.3"),
        ("=2+3*4", "14"),
        ("=(2+3)*4", "20"),
        ("=-(-(+2))+3*-4", "-10"),
        ("= 1\t+\r\n2 ", "3"),
        ("=.5+.05", "0.55"),
        ("=1-2-3", "-4"),
        ("=000.100000000000000000000000000000+0.2", "0.3"),
        ("=1e+2+2E-1", "100.2"),
        ("=-0e28", "0"),
    ] {
        let mut table = Table::new(1, 1, 100., 20.);
        table.set_cell_formula(0, 0, source).unwrap();
        assert_eq!(text(&table, 0), expected, "{source}");
        table.validate_formulas().unwrap();
        assert_eq!(table.cell(0, 0).unwrap().formula.as_ref().unwrap().expression(), source);
    }
}
#[test]
fn normalized_precision_and_scale_boundaries_are_enforced() {
    for (source, expected) in [
        ("=9999999999999999999999999999+1", "10000000000000000000000000000"),
        ("=1e-28", "0.0000000000000000000000000001"),
        ("=1e28", "10000000000000000000000000000"),
        ("=1.0000000000000000000000000000", "1"),
    ] {
        let mut t = Table::new(1, 1, 10., 10.);
        t.set_cell_formula(0, 0, source).unwrap();
        assert_eq!(text(&t, 0), expected);
    }
    for source in ["=1e29", "=1e-29", "=12345678901234567890123456789", "=1e28+1e-28", "=1e-28*.1", "=1e28*10", "=9999999999999999999999999999*9"] {
        let mut t = Table::new(1, 1, 10., 10.);
        let before = t.clone();
        assert!(t.set_cell_formula(0, 0, source).is_err(), "{source}");
        assert_eq!(t, before);
    }
}
#[test]
fn lexical_zeroes_do_not_require_an_unnormalized_i128_coefficient() {
    let mut t = Table::new(1, 1, 10., 10.);
    let source = format!("=1.{}", "0".repeat(100));
    t.set_cell_formula(0, 0, &source).unwrap();
    assert_eq!(text(&t, 0), "1");
}
#[test]
fn unsupported_grammar_is_refused_without_mutation() {
    for source in [
        "1+2",
        "=",
        "=1/3",
        "=SUM(1)",
        "=1.",
        "=1e",
        "=1e+",
        "=１+2",
        "=1\u{a0}+2",
        "=2(3)",
        "=1,2",
        "=[.a1]",
        "=[.A0]",
        "=[.$A1]",
        "=[A1]",
        "=[.A01]",
        "=[.A1:.B1]",
        "=[Sheet.A1]",
        "=[.A184467440737095516160]",
        "=[.ZZZZZZZZZZZZZZZZZZZZZZZ1]",
        "=--",
        "=(1",
        "=1)",
    ] {
        let mut t = Table::new(1, 1, 10., 10.);
        let before = t.clone();
        assert!(t.set_cell_formula(0, 0, source).is_err(), "{source}");
        assert_eq!(t, before);
    }
}
#[test]
fn forward_references_chains_diamonds_and_duplicate_edges_are_table_local() {
    let mut t = Table::new(1, 5, 100., 10.);
    numeric(&mut t, 0, 4, "2");
    attach(&mut t, 0, 0, "=[.E1]+1", "3", "3");
    attach(&mut t, 0, 1, "=[.A1]*2", "6", "6");
    attach(&mut t, 0, 2, "=[.A1]+[.A1]", "6", "6");
    attach(&mut t, 0, 3, "=[.B1]+[.C1]", "12", "12");
    t.validate_formulas().unwrap();
    let independent = t.clone();
    numeric(&mut t, 0, 4, "3");
    t.recalculate().unwrap();
    assert_eq!([text(&t, 0), text(&t, 1), text(&t, 2), text(&t, 3)], ["4", "8", "8", "16"]);
    assert_eq!(text(&independent, 3), "12");
    independent.validate_formulas().unwrap();
    t.validate_formulas().unwrap();
}
#[test]
fn cycles_and_out_of_grid_references_refuse_atomically() {
    let mut t = Table::new(1, 2, 10., 10.);
    numeric(&mut t, 0, 1, "1");
    t.set_cell_formula(0, 0, "=[.B1]").unwrap();
    let before = t.clone();
    assert!(t.set_cell_formula(0, 1, "=[.A1]").unwrap_err().to_string().contains("cycle"));
    assert_eq!(t, before);
    for source in ["=[.A1]", "=[.C1]", "=[.A2]"] {
        assert!(t.set_cell_formula(0, 0, source).is_err());
        assert_eq!(t, before);
    }
    attach(&mut t, 0, 1, "=[.A1]", "1", "1");
    assert!(t.validate_formulas().unwrap_err().to_string().contains("cycle"));
}
#[test]
fn later_literal_input_produces_closed_errors_and_restoring_scalar_resolves_them() {
    let mut t = Table::new(1, 3, 10., 10.);
    numeric(&mut t, 0, 0, "2");
    t.set_cell_formula(0, 1, "=[.A1]+1").unwrap();
    t.set_cell_formula(0, 2, "=[.B1]*2").unwrap();
    t.cell_mut(0, 0).unwrap().edit_display(|body| *body = TextBody::from_text("12")).unwrap();
    assert!(t.cell(0, 0).unwrap().data.is_none());
    t.recalculate().unwrap();
    assert_eq!(outcome(&t, 1), &FormulaOutcome::Error { code: FormulaError::InputNotNumeric });
    assert_eq!(outcome(&t, 2), &FormulaOutcome::Error { code: FormulaError::DependencyError });
    assert_eq!(text(&t, 1), "#VALUE!");
    assert_eq!(text(&t, 2), "#DEPEND!");
    t.validate_formulas().unwrap();
    numeric(&mut t, 0, 0, "4");
    t.recalculate().unwrap();
    assert_eq!(text(&t, 1), "5");
    assert_eq!(text(&t, 2), "10");
}
#[test]
fn only_float_constants_are_numeric_never_display_text_or_other_scalar_kinds() {
    let kinds = [
        CellValue::String { value: "12".into() },
        CellValue::Boolean { value: true },
        CellValue::Number { kind: CellNumberKind::Percentage, lexical: "12".into() },
        CellValue::Number { kind: CellNumberKind::Currency { currency: None }, lexical: "12".into() },
        CellValue::Date { lexical: "2026-10-16".into() },
        CellValue::Duration { lexical: "PT12S".into() },
    ];
    for value in kinds {
        let mut t = Table::new(1, 2, 10., 10.);
        let body = TextBody::from_text("12");
        t.cell_mut(0, 0).unwrap().data = Some(Arc::new(CellConstant::from_display(value, &body).unwrap()));
        t.cell_mut(0, 0).unwrap().text = body;
        assert!(t.set_cell_formula(0, 1, "=[.A1]+1").is_err());
    }
    for display in ["", "12"] {
        let mut t = Table::new(1, 2, 10., 10.);
        t.cell_mut(0, 0).unwrap().text = TextBody::from_text(display);
        assert!(t.set_cell_formula(0, 1, "=[.A1]").is_err());
    }
}
#[test]
fn out_of_profile_float_input_becomes_numeric_limit_and_does_not_change_constant() {
    let mut t = Table::new(1, 2, 10., 10.);
    numeric(&mut t, 0, 0, "1");
    t.set_cell_formula(0, 1, "=[.A1]+1").unwrap();
    numeric(&mut t, 0, 0, "1e100");
    let scalar = t.cell(0, 0).unwrap().data.clone().unwrap();
    t.recalculate().unwrap();
    assert_eq!(outcome(&t, 1), &FormulaOutcome::Error { code: FormulaError::NumericLimit });
    assert!(Arc::ptr_eq(t.cell(0, 0).unwrap().data.as_ref().unwrap(), &scalar));
}
#[test]
fn equal_numeric_results_preserve_cache_display_and_record_identity() {
    let mut t = Table::new(1, 2, 10., 10.);
    numeric(&mut t, 0, 0, "1");
    attach(&mut t, 0, 1, "=[.A1]+2", "3e0", "003.00");
    t.validate_formulas().unwrap();
    let record = t.cell(0, 1).unwrap().formula.clone().unwrap();
    numeric(&mut t, 0, 0, "1.000");
    assert!(!t.recalculate().unwrap());
    assert_eq!(text(&t, 1), "003.00");
    assert_eq!(outcome(&t, 1).numeric_lexical(), Some("3e0"));
    assert!(Arc::ptr_eq(t.cell(0, 1).unwrap().formula.as_ref().unwrap(), &record));
}
#[test]
fn segmented_formatting_keeps_formula_and_changed_values_use_first_run_style() {
    let mut t = Table::new(1, 2, 10., 10.);
    numeric(&mut t, 0, 0, "1");
    attach(&mut t, 0, 1, "=[.A1]+2", "3", "03.00");
    let record = t.cell(0, 1).unwrap().formula.clone().unwrap();
    let cell = t.cell_mut(0, 1).unwrap();
    cell.text.paragraphs[0].runs = vec![Run::new("03"), Run::new(".00")];
    cell.text.paragraphs[0].runs[0].props.bold = Some(true);
    cell.text.paragraphs[0].runs[1].props.italic = Some(true);
    assert!(!cell.reconcile_constant().unwrap());
    assert!(!t.recalculate().unwrap());
    assert!(Arc::ptr_eq(t.cell(0, 1).unwrap().formula.as_ref().unwrap(), &record));
    let native = serde_json::to_string(&t).unwrap();
    let mut reopened: Table = serde_json::from_str(&native).unwrap();
    reopened.validate_formulas().unwrap();
    numeric(&mut reopened, 0, 0, "2");
    reopened.recalculate().unwrap();
    let runs = &reopened.cell(0, 1).unwrap().text.paragraphs[0].runs;
    assert_eq!(runs[0].text, "4");
    assert_eq!(runs[0].props.bold, Some(true));
    assert!(runs[1].text.is_empty());
    assert_eq!(runs[1].props.italic, Some(true));
}
#[test]
fn changed_formula_display_detaches_source_and_recalculates_remaining_dependents() {
    let mut t = Table::new(1, 3, 10., 10.);
    numeric(&mut t, 0, 0, "1");
    t.set_cell_formula(0, 1, "=[.A1]+1").unwrap();
    t.set_cell_formula(0, 2, "=[.B1]+1").unwrap();
    t.cell_mut(0, 1).unwrap().text = TextBody::from_text("2.0");
    t.recalculate().unwrap();
    assert!(t.cell(0, 1).unwrap().formula.is_none());
    assert_eq!(text(&t, 1), "2.0");
    assert_eq!(outcome(&t, 2), &FormulaOutcome::Error { code: FormulaError::InputNotNumeric });
}
#[test]
fn malformed_or_forged_cache_records_are_rejected() {
    let valid = r#"{"profile":"exact-decimal-v1","expression":"=1+2","outcome":{"kind":"number","lexical":"3"},"display":"03.00"}"#;
    serde_json::from_str::<CellFormula>(valid).unwrap();
    for bad in [
        valid.replace("exact-decimal-v1", "binary64"),
        valid.replace("\"display\":\"03.00\"", "\"display\":\"4\""),
        valid.replace("\"expression\":", "\"expression\":\"=0\",\"expression\":"),
        valid.replace("\"profile\":", "\"unknown\":[],\"profile\":"),
        valid.replace("\"kind\":\"number\"", "\"kind\":\"number\",\"kind\":\"number\""),
        valid.replace("\"kind\":\"number\"", "\"kind\":\"number\",\"code\":\"numericLimit\""),
        valid.replace("\"lexical\":\"3\"", "\"lexical\":\"NaN\""),
        valid.replace("\"lexical\":\"3\"", "\"lexical\":\"1e100\""),
        valid.replace("=1+2", "=1/3"),
        valid.replace("\"outcome\":", "\"ast\":[],\"outcome\":"),
    ] {
        assert!(serde_json::from_str::<CellFormula>(&bad).is_err(), "{bad}");
    }
    let mut t = Table::new(1, 1, 10., 10.);
    attach(&mut t, 0, 0, "=1+2", "4", "4");
    assert!(t.validate_formulas().unwrap_err().to_string().contains("cached outcome"));
}
#[test]
fn forged_closed_errors_are_rejected_by_graph_validation_and_real_errors_roundtrip() {
    let mut t = Table::new(1, 1, 10., 10.);
    let cell = t.cell_mut(0, 0).unwrap();
    cell.text = TextBody::from_text("#NUM!");
    cell.formula =
        Some(Arc::new(CellFormula::from_display("=1+2".into(), FormulaOutcome::Error { code: FormulaError::NumericLimit }, &cell.text).unwrap()));
    assert!(t.validate_formulas().is_err());
    let mut t = Table::new(1, 2, 10., 10.);
    numeric(&mut t, 0, 0, "1");
    t.set_cell_formula(0, 1, "=[.A1]").unwrap();
    t.cell_mut(0, 0).unwrap().data = None;
    t.recalculate().unwrap();
    let native = serde_json::to_string(&t).unwrap();
    let restored: Table = serde_json::from_str(&native).unwrap();
    restored.validate_formulas().unwrap();
    assert_eq!(restored, t);
}
#[test]
fn stale_content_does_not_hide_forged_neighbour_cache() {
    let mut t = Table::new(1, 2, 10., 10.);
    numeric(&mut t, 0, 0, "2");
    attach(&mut t, 0, 1, "=[.A1]+1", "4", "4");
    t.cell_mut(0, 0).unwrap().text = TextBody::from_text("changed");
    let mut p = document(t);
    let old = p.slides[0].clone();
    assert!(validate_constants(&p).unwrap_err().to_string().contains("cached outcome"));
    assert!(Arc::ptr_eq(&p.slides[0], &old));
    // Edit reconciliation is deliberately separate from untrusted native validation.
    assert!(reconcile_constants(&mut p).unwrap());
    assert_eq!(outcome(table(&p), 1), &FormulaOutcome::Error { code: FormulaError::InputNotNumeric });
}
#[test]
fn valid_old_authority_is_checked_before_stale_content_is_lowered() {
    let mut t = Table::new(1, 2, 10., 10.);
    numeric(&mut t, 0, 0, "2");
    t.set_cell_formula(0, 1, "=[.A1]+1").unwrap();
    t.cell_mut(0, 0).unwrap().text = TextBody::from_text("changed");
    let mut p = document(t);
    validate_constants(&p).unwrap();
    reconcile_constants(&mut p).unwrap();
    validate_constants(&p).unwrap();
    assert_eq!(outcome(table(&p), 1), &FormulaOutcome::Error { code: FormulaError::InputNotNumeric });
}
#[test]
fn constants_and_formulas_are_mutually_exclusive_and_topology_is_guarded() {
    let mut t = Table::new(1, 2, 10., 10.);
    t.set_cell_formula(0, 0, "=1").unwrap();
    let before = t.clone();
    assert!(validate_formula_topology(&t).is_err());
    t.insert_row(1);
    t.insert_col(1);
    assert!(!t.delete_row(0));
    assert!(!t.delete_col(0));
    assert!(!t.merge(0, 0, 0, 1));
    assert!(!t.split(0, 0));
    t.normalize();
    assert_eq!(t, before);
    let cell = t.cell_mut(0, 0).unwrap();
    cell.data = Some(Arc::new(CellConstant::from_display(CellValue::Boolean { value: true }, &cell.text).unwrap()));
    assert!(t.recalculate().is_err());
    assert!(validate_constants(&document(t)).is_err());
}
#[test]
fn source_depth_nodes_size_and_table_formula_count_are_bounded() {
    let mut t = Table::new(1, 1, 10., 10.);
    for source in [
        format!("={}1", "-".repeat(32)),
        format!("={}1{}", "(".repeat(32), ")".repeat(32)),
        format!("=1{}", " ".repeat(4096)),
        format!("={}", vec!["1"; 129].join("+")),
    ] {
        assert!(t.set_cell_formula(0, 0, &source).is_err());
    }
    let formula = Arc::new(CellFormula::number("=1".into(), "1".into(), &TextBody::from_text("1")).unwrap());
    let mut t = Table::new(1, 129, 100., 10.);
    for cell in &mut t.rows[0].cells {
        cell.text = TextBody::from_text("1");
        cell.formula = Some(formula.clone());
    }
    assert!(t.validate_formulas().unwrap_err().to_string().contains("count"));
    t.rows[0].cells.pop();
    t.cols.pop();
    t.validate_formulas().unwrap();
}
#[test]
fn error_display_and_unknown_outcome_serde_are_closed() {
    for bad in [
        r#"{"kind":"error","code":"anything"}"#,
        r#"{"kind":"error","code":"numericLimit","lexical":"0"}"#,
        r#"{"kind":"pending"}"#,
        r#"{"kind":"number","lexical":"3","extra":true}"#,
    ] {
        assert!(serde_json::from_str::<FormulaOutcome>(bad).is_err());
    }
    assert!(
        CellFormula::from_display("=1".into(), FormulaOutcome::Error { code: FormulaError::NumericLimit }, &TextBody::from_text("#VALUE!")).is_err()
    );
}
#[test]
fn every_owner_is_validated_guarded_and_reconciled_with_unchanged_arcs_preserved() {
    for owner in 0..12 {
        let mut t = Table::new(1, 2, 10., 10.);
        numeric(&mut t, 0, 0, "1");
        t.set_cell_formula(0, 1, "=[.A1]+1").unwrap();
        t.cell_mut(0, 0).unwrap().text = TextBody::from_text("literal");
        let mut p = document(Table::new(1, 1, 10., 10.));
        let unchanged = p.slides[0].clone();
        let s = shape(t);
        let group = |s| Shape { kind: ShapeKind::Group { children: vec![s], child: Xfrm::new(0., 0., 10., 10.) }, ..Default::default() };
        let master = |s| Arc::new(Master { shapes: vec![s], ..Default::default() });
        match owner {
            0 => p.slides.push(Arc::new(Slide { shapes: vec![s], ..Default::default() })),
            1 => p.slides.push(Arc::new(Slide { shapes: vec![group(s)], ..Default::default() })),
            2 => p.masters.push(master(s)),
            3 => p.masters.push(Arc::new(Master { layouts: vec![Layout { shapes: vec![s], ..Default::default() }], ..Default::default() })),
            4 => p.notes_master = Some(master(s)),
            5 => p.handout_master = Some(master(s)),
            6 => p.notes_masters.push(NotesMaster { master: master(s), ..Default::default() }),
            7 => p.notes_masters.push(NotesMaster {
                master: Arc::new(Master { layouts: vec![Layout { shapes: vec![s], ..Default::default() }], ..Default::default() }),
                ..Default::default()
            }),
            8 | 9 => p.slides.push(Arc::new(Slide {
                notes_page: Some(NotesPage { shapes: vec![if owner == 9 { group(s) } else { s }], ..Default::default() }),
                ..Default::default()
            })),
            10 => p.notes_master = Some(Arc::new(Master { layouts: vec![Layout { shapes: vec![s], ..Default::default() }], ..Default::default() })),
            _ => {
                p.handout_master =
                    Some(Arc::new(Master { layouts: vec![Layout { shapes: vec![group(s)], ..Default::default() }], ..Default::default() }))
            }
        }
        validate_constants(&p).unwrap();
        assert!(reject_constants(&p).is_err());
        assert!(reconcile_constants(&mut p).unwrap());
        validate_constants(&p).unwrap();
        assert!(!reconcile_constants(&mut p).unwrap());
        assert!(Arc::ptr_eq(&p.slides[0], &unchanged));
    }
}
#[test]
fn document_formula_and_work_budgets_count_shared_attachments() {
    let mut t = Table::new(1, 128, 100., 10.);
    for cell in &mut t.rows[0].cells {
        cell.text = TextBody::from_text("1");
        cell.formula = Some(Arc::new(CellFormula::number("=1".into(), "1".into(), &cell.text).unwrap()));
    }
    let mut p = document(t);
    let owner = p.slides[0].clone();
    p.slides = vec![owner; 9];
    assert!(validate_constants(&p).unwrap_err().to_string().contains("resource limit"));
    let mut t = Table::new(40, 1000, 100., 10.);
    t.set_cell_formula(0, 0, "=1").unwrap();
    let mut p = document(t);
    let owner = p.slides[0].clone();
    p.slides = vec![owner; 3];
    assert!(validate_constants(&p).unwrap_err().to_string().contains("resource limit"));
}

#[test]
fn planned_updates_refuse_changed_predecessors_before_any_destination_mutation() {
    let mut t = Table::new(1, 3, 10., 10.);
    numeric(&mut t, 0, 0, "1");
    t.set_cell_formula(0, 1, "=[.A1]+1").unwrap();
    t.set_cell_formula(0, 2, "=[.B1]*2").unwrap();
    numeric(&mut t, 0, 0, "2");
    let plan = t.plan_recalculation().unwrap();
    numeric(&mut t, 0, 0, "3");
    let before = t.clone();
    assert!(plan.apply(&mut t).is_err());
    assert_eq!(t, before);
}
#[test]
fn formula_display_admission_rejects_fields_links_breaks_and_excessive_segmentation() {
    use deckcraft_model::text::RunKind;
    for kind in [RunKind::Break, RunKind::Math { omml: String::new() }] {
        let mut body = TextBody::from_text("3");
        body.paragraphs[0].runs[0].kind = kind;
        assert!(CellFormula::number("=1+2".into(), "3".into(), &body).is_err());
    }
    let mut body = TextBody::from_text("3");
    body.paragraphs.push(Default::default());
    assert!(CellFormula::number("=1+2".into(), "3".into(), &body).is_err());
    let mut body = TextBody::from_text("3");
    body.paragraphs[0].runs = vec![Run::new(""); 1025];
    assert!(CellFormula::number("=1+2".into(), "3".into(), &body).is_err());
}
#[test]
fn pass_reset_preserves_cumulative_operation_fuel_and_graph_attachment_limits() {
    use deckcraft_model::table::{FormulaBudget, validate_constants_with_budget};
    let mut t = Table::new(40, 1000, 100., 10.);
    t.set_cell_formula(0, 0, "=1").unwrap();
    let p = document(t);
    let mut budget = FormulaBudget::default();
    let mut accepted = 0;
    let error = loop {
        budget.next_pass();
        match validate_constants_with_budget(&p, &mut budget) {
            Ok(_) => accepted += 1,
            Err(error) => break error,
        }
        assert!(accepted < 100, "pass reset must not reset cumulative operation fuel");
    };
    assert!(accepted > 1, "each individual graph fits the per-pass budget");
    assert!(error.to_string().contains("operation work"));
}

#[test]
fn failed_budget_charge_remains_visible_after_pass_reset() {
    use deckcraft_model::table::{FormulaBudget, validate_constants_with_budget};
    let mut t = Table::new(40, 1000, 100., 10.);
    t.set_cell_formula(0, 0, "=1").unwrap();
    let p = document(t);
    let mut budget = FormulaBudget::default();
    validate_constants_with_budget(&p, &mut budget).unwrap();
    let mut accepted = 1;
    while validate_constants_with_budget(&p, &mut budget).is_ok() {
        accepted += 1;
        assert!(accepted < 100, "a shared graph pass must exhaust bounded work");
    }
    assert!(budget.check_limits().is_err());
    budget.next_pass();
    assert!(budget.check_limits().is_err());
    assert!(validate_constants_with_budget(&p, &mut budget).is_err());
}

#[test]
fn assignment_materializes_empty_target_runs_and_preserves_existing_styles() {
    for no_paragraph in [false, true] {
        let mut t = Table::new(1, 1, 10., 10.);
        let cell = t.cell_mut(0, 0).unwrap();
        if no_paragraph {
            cell.text.paragraphs.clear();
        } else {
            assert!(cell.text.paragraphs[0].runs.is_empty());
            cell.text.paragraphs[0].def_props.bold = Some(true);
        }
        t.set_cell_formula(0, 0, "=1+2").unwrap();
        assert_eq!(text(&t, 0), "3");
        if !no_paragraph {
            assert_eq!(t.cell(0, 0).unwrap().text.paragraphs[0].def_props.bold, Some(true));
        }
        t.validate_formulas().unwrap();
    }
    let body = TextBody::from_text("");
    assert!(CellFormula::number("=1".into(), "1".into(), &body).is_err());
}
