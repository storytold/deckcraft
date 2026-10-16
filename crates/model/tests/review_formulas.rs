//! Independent exact arithmetic, cache authority and cumulative owner-budget review.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::sync::Arc;

use deckcraft_model::{
    table::{
        CellConstant, CellFormula, CellNumberKind, CellValue, FormulaBudget, FormulaError, FormulaOutcome, reconcile_constants, validate_constants,
        validate_constants_with_budget,
    },
    text::{Run, RunProps},
    *,
};

fn number(table: &mut Table, col: usize, lexical: &str, shown: &str) {
    let cell = table.cell_mut(0, col).unwrap();
    cell.text = TextBody::from_text(shown);
    cell.data =
        Some(Arc::new(CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Float, lexical: lexical.into() }, &cell.text).unwrap()));
    cell.formula = None;
}

fn outcome(table: &Table, col: usize) -> &FormulaOutcome {
    table.cell(0, col).unwrap().formula.as_ref().unwrap().outcome()
}

fn owner(table: Table) -> Arc<Master> {
    Arc::new(Master { shapes: vec![Shape { kind: ShapeKind::Table(table), ..Default::default() }], ..Default::default() })
}

fn document(table: Table) -> Presentation {
    let mut p = Presentation::default();
    p.slides.clear();
    p.masters = vec![owner(table)];
    p.notes_master = None;
    p.notes_masters.clear();
    p.handout_master = None;
    p
}

#[test]
fn exact_precision_vectors_cover_cancellation_fractional_borrow_and_signed_exponents() {
    let mut t = Table::new(1, 1, 40., 20.);
    for (expression, expected) in [
        ("=9007199254740993-9007199254740992", "1"),
        ("=9999999999999999999999999999-9999999999999999999999999998", "1"),
        ("=1.000000000000000000000000001-1", "0.000000000000000000000000001"),
        ("=1e-28+9e-28", "0.000000000000000000000000001"),
        ("=1e28-1e28", "0"),
        ("=-.0000000000000000000000000001*10", "-0.000000000000000000000000001"),
        ("=1.25e-2*8e+1", "1"),
        ("=-(.125-.375)*-4", "-1"),
    ] {
        t.set_cell_formula(0, 0, expression).unwrap();
        assert_eq!(outcome(&t, 0), &FormulaOutcome::Number { lexical: expected.into() }, "{expression}");
        t.validate_formulas().unwrap();
    }
}

#[test]
fn normalized_long_tokens_are_admitted_without_binary_rounding_or_raw_coefficient_overflow() {
    let source = format!("=1{}e-90", "0".repeat(90));
    let mut t = Table::new(1, 1, 40., 20.);
    t.set_cell_formula(0, 0, &source).unwrap();
    assert_eq!(outcome(&t, 0), &FormulaOutcome::Number { lexical: "1".into() });
    for rejected in ["=1e29", "=1e-29", "=10000000000000000000000000001", "=1e2147483648", "=1e-2147483649"] {
        let before = t.clone();
        assert!(t.set_cell_formula(0, 0, rejected).is_err(), "{rejected}");
        assert_eq!(t, before);
    }
}

#[test]
fn checked_alignment_and_multiplication_limits_remain_explicit_native_outcomes() {
    let mut t = Table::new(1, 3, 120., 20.);
    number(&mut t, 0, "1", "authority differs from display");
    t.set_cell_formula(0, 1, "=[.A1]+1e-27").unwrap();
    t.set_cell_formula(0, 2, "=[.B1]*2").unwrap();
    number(&mut t, 0, "1e28", "still not an operand");
    t.recalculate().unwrap();
    assert_eq!(outcome(&t, 1), &FormulaOutcome::Error { code: FormulaError::NumericLimit });
    assert_eq!(outcome(&t, 2), &FormulaOutcome::Error { code: FormulaError::DependencyError });
    assert!(t.cell(0, 1).unwrap().data.is_none());
    t.validate_formulas().unwrap();
}

#[test]
fn unchanged_numeric_result_keeps_lexical_rich_display_and_arc_after_new_operand_spelling() {
    let mut t = Table::new(1, 2, 80., 20.);
    number(&mut t, 0, "3", "three");
    let mut cell = Cell::new("+003.000e0");
    cell.text.paragraphs[0].runs = vec![Run::new("+003."), Run::with("000e0", RunProps { bold: Some(true), ..Default::default() })];
    let original = Arc::new(CellFormula::number("=[.A1]".into(), "3e0".into(), &cell.text).unwrap());
    cell.formula = Some(original.clone());
    *t.cell_mut(0, 1).unwrap() = cell;
    number(&mut t, 0, "03.000", "unrelated");
    assert!(!t.recalculate().unwrap());
    assert!(Arc::ptr_eq(t.cell(0, 1).unwrap().formula.as_ref().unwrap(), &original));
    assert_eq!(t.cell(0, 1).unwrap().text.text(), "+003.000e0");
}

#[test]
fn stale_neighbor_cannot_launder_forged_current_cache_during_authority_validation() {
    let mut t = Table::new(1, 3, 120., 20.);
    number(&mut t, 0, "2", "two");
    t.set_cell_formula(0, 1, "=[.A1]*3").unwrap();
    let target = t.cell_mut(0, 2).unwrap();
    target.text = TextBody::from_text("999");
    target.formula = Some(Arc::new(CellFormula::number("=[.B1]+1".into(), "999".into(), &target.text).unwrap()));
    t.cell_mut(0, 1).unwrap().text = TextBody::from_text("literal");
    let p = document(t);
    assert!(validate_constants(&p).unwrap_err().to_string().contains("cached outcome"));
}

#[test]
fn old_authority_validates_before_stale_formula_lowering_and_dependency_recalculation() {
    let mut t = Table::new(1, 3, 120., 20.);
    number(&mut t, 0, "2", "two");
    t.set_cell_formula(0, 1, "=[.A1]*3").unwrap();
    t.set_cell_formula(0, 2, "=[.B1]+1").unwrap();
    t.cell_mut(0, 1).unwrap().text = TextBody::from_text("literal");
    let mut p = document(t);
    validate_constants(&p).unwrap();
    assert!(reconcile_constants(&mut p).unwrap());
    let ShapeKind::Table(t) = &p.masters[0].shapes[0].kind else { panic!() };
    assert!(t.cell(0, 1).unwrap().formula.is_none());
    assert_eq!(outcome(t, 2), &FormulaOutcome::Error { code: FormulaError::InputNotNumeric });
    validate_constants(&p).unwrap();
}

#[test]
fn cumulative_grid_work_counts_repeated_unused_owner_attachments() {
    let mut t = Table::new(9, 1000, 1000., 20.);
    t.set_cell_formula(0, 0, "=1").unwrap();
    let mut p = document(t);
    let shared = p.masters[0].clone();
    // Every attachment must count even when all owners share one Arc. Discover
    // the bounded frontier without freezing the cost of added ownership proofs.
    let mut accepted = 0;
    for copies in 1..100 {
        p.masters = vec![shared.clone(); copies];
        if validate_constants(&p).is_err() {
            break;
        }
        accepted = copies;
    }
    assert!(accepted > 1 && accepted < 99);
    let before = p.masters.clone();
    assert!(validate_constants(&p).unwrap_err().to_string().contains("resource limit"));
    assert!(reconcile_constants(&mut p).unwrap_err().to_string().contains("resource limit"));
    assert!(p.masters.iter().zip(before).all(|(a, b)| Arc::ptr_eq(a, &b)));
}

#[test]
fn next_pass_preserves_operation_fuel_while_resetting_graph_counters() {
    let mut t = Table::new(9, 1000, 1000., 20.);
    t.set_cell_formula(0, 0, "=1").unwrap();
    let mut p = document(t);
    p.masters = vec![p.masters[0].clone(); 6];
    let mut budget = FormulaBudget::default();
    let mut accepted = 0;
    let error = loop {
        budget.next_pass();
        match validate_constants_with_budget(&p, &mut budget) {
            Ok(_) => accepted += 1,
            Err(error) => break error,
        }
        assert!(accepted < 100, "next_pass must preserve cumulative fuel");
    };
    assert!(accepted > 1);
    assert!(error.to_string().contains("operation work limit"));
}

#[test]
fn maximum_formula_attachments_remain_valid_across_three_bounded_passes() {
    let mut t = Table::new(1, 128, 128., 20.);
    let body = TextBody::from_text("1");
    let shared = Arc::new(CellFormula::number("=1".into(), "1".into(), &body).unwrap());
    for cell in &mut t.rows[0].cells {
        cell.text = body.clone();
        cell.formula = Some(shared.clone());
    }
    let mut p = document(t);
    p.masters = vec![p.masters[0].clone(); 8];
    let mut budget = FormulaBudget::default();
    for _ in 0..3 {
        budget.next_pass();
        validate_constants_with_budget(&p, &mut budget).unwrap();
    }
}

#[test]
fn native_formula_keys_reject_duplicates_and_unknown_values_before_reading_payloads() {
    for source in [
        r#"{"unknown":{"unterminated": "#,
        r#"{"profile":"exact-decimal-v1","profile":{"unterminated": "#,
        r#"{"outcome":{"kind":"number","lexical":"1","lexical":{"unterminated": "#,
    ] {
        let error = serde_json::from_str::<CellFormula>(source).unwrap_err().to_string();
        assert!(error.contains("unknown") || error.contains("duplicate"), "{error}");
        assert!(!error.contains("EOF"), "{error}");
    }
}

#[test]
fn formula_error_cache_must_match_recomputed_closed_error_code() {
    let mut t = Table::new(1, 2, 80., 20.);
    let body = TextBody::from_text("#DEPEND!");
    t.cell_mut(0, 1).unwrap().text = body.clone();
    t.cell_mut(0, 1).unwrap().formula =
        Some(Arc::new(CellFormula::from_display("=[.A1]+1".into(), FormulaOutcome::Error { code: FormulaError::DependencyError }, &body).unwrap()));
    assert!(t.validate_formulas().is_err(), "literal A1 yields InputNotNumeric, not a dependency error");
}
