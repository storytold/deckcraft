//! Contributor-original merged-grid arithmetic and plan-authority regressions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::table::{CellNumberKind, FORMULA_PROFILE, FORMULA_PROFILE_V2};
use std::sync::Arc;

fn constant(table: &mut Table, row: usize, col: usize, lexical: &str) {
    let cell = table.cell_mut(row, col).unwrap();
    cell.text = TextBody::from_text(lexical);
    cell.data =
        Some(Arc::new(CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Float, lexical: lexical.into() }, &cell.text).unwrap()));
}
fn formula(table: &mut Table, row: usize, col: usize, source: &str, v2: bool) {
    if v2 {
        table.set_cell_formula_v2(row, col, source).unwrap();
    } else {
        table.set_cell_formula(row, col, source).unwrap();
    }
}
fn merge(table: &mut Table, from: (usize, usize), to: (usize, usize)) {
    table.merge_with_budget(from, to, &mut FormulaBudget::default()).unwrap();
}
fn current_text(table: &Table, row: usize, col: usize) -> String {
    table.cell(row, col).unwrap().text.text()
}

#[test]
fn surrounding_merged_geometry_keeps_both_arithmetic_profiles() {
    for v2 in [false, true] {
        let mut table = Table::new(3, 3, 300., 30.);
        formula(&mut table, 0, 0, "=2+3", v2);
        formula(&mut table, 2, 2, "=7*8", v2);
        merge(&mut table, (0, 0), (0, 1));
        assert_eq!(current_text(&table, 0, 0), "5");
        assert!(table.cell(0, 0).unwrap().formula.is_none());
        assert_eq!(current_text(&table, 2, 2), "56");
        assert_eq!(table.cell(2, 2).unwrap().formula.as_ref().unwrap().profile(), if v2 { FORMULA_PROFILE_V2 } else { FORMULA_PROFILE });
        table.validate_formulas().unwrap();
        table.split_with_budget(0, 0, &mut FormulaBudget::default()).unwrap();
        assert_eq!(current_text(&table, 2, 2), "56");
    }
}
#[test]
fn first_formula_assignment_checks_literal_only_merged_ownership_strictly() {
    for v2 in [false, true] {
        for invalid_kind in 0..5 {
            let mut table = Table::new(2, 3, 300., 30.);
            match invalid_kind {
                0 => table.cell_mut(0, 1).unwrap().h_merge = true,
                1 => table.cell_mut(0, 0).unwrap().grid_span = 2,
                2 => table.cell_mut(0, 0).unwrap().row_span = 3,
                3 => table.cell_mut(0, 0).unwrap().grid_span = 0,
                _ => {
                    table.cell_mut(0, 0).unwrap().grid_span = 2;
                    table.cell_mut(0, 1).unwrap().h_merge = true;
                    table.cell_mut(0, 1).unwrap().row_span = 2;
                }
            }
            let before = table.clone();
            let result = if v2 { table.set_cell_formula_v2(1, 2, "=1+2") } else { table.set_cell_formula(1, 2, "=1+2") };
            assert!(result.is_err(), "v2={v2}, topology={invalid_kind}");
            assert_eq!(table, before);
        }
    }
}
#[test]
fn first_formula_assignment_accepts_valid_literal_only_surrounding_merge() {
    for v2 in [false, true] {
        let mut table = Table::new(2, 3, 300., 30.);
        merge(&mut table, (0, 0), (0, 1));
        formula(&mut table, 1, 2, "=1+2", v2);
        assert_eq!(current_text(&table, 1, 2), "3");
        table.validate_formulas().unwrap();
    }
}
#[test]
fn assignment_targets_remain_visible_and_unmerged() {
    for v2 in [false, true] {
        let mut table = Table::new(2, 3, 300., 30.);
        merge(&mut table, (0, 0), (0, 1));
        for col in [0, 1] {
            let before = table.clone();
            let result = if v2 { table.set_cell_formula_v2(0, col, "=1+2") } else { table.set_cell_formula(0, col, "=1+2") };
            assert!(result.is_err());
            assert_eq!(table, before);
        }
    }
}
#[test]
fn covered_text_is_empty_for_v2_and_never_coerces_legacy_blanks_to_numbers() {
    let mut table = Table::new(2, 4, 400., 30.);
    constant(&mut table, 0, 0, "2");
    constant(&mut table, 0, 1, "3");
    formula(&mut table, 1, 0, "=LEN([.A1])", true);
    formula(&mut table, 1, 1, "=[.B1]+1", true);
    formula(&mut table, 1, 2, "=[.B1]+1", false);
    merge(&mut table, (0, 0), (0, 1));
    table.cell_mut(0, 1).unwrap().text = TextBody::from_text("HOSTILE HIDDEN VALUE");
    table.recalculate().unwrap();
    assert_eq!(current_text(&table, 1, 0), "#VALUE!");
    assert_eq!(current_text(&table, 1, 1), "1");
    assert_eq!(current_text(&table, 1, 2), "#VALUE!");
    assert_eq!(table.cell(1, 2).unwrap().formula.as_ref().unwrap().profile(), FORMULA_PROFILE);
    table.validate_formulas().unwrap();
}
#[test]
fn covered_numeric_or_formula_authority_is_rejected_without_mutation() {
    for attach_formula in [false, true] {
        let mut table = Table::new(2, 3, 300., 30.);
        formula(&mut table, 1, 2, "=1+2", true);
        merge(&mut table, (0, 0), (0, 1));
        if attach_formula {
            table.cell_mut(0, 1).unwrap().formula = table.cell(1, 2).unwrap().formula.clone();
        } else {
            constant(&mut table, 0, 1, "99");
        }
        let before = table.clone();
        assert!(table.recalculate().is_err());
        assert_eq!(table, before);
    }
}
#[test]
fn current_formula_cache_is_not_authenticated_by_the_geometry_broadening() {
    for v2 in [false, true] {
        let mut table = Table::new(2, 3, 300., 30.);
        merge(&mut table, (0, 0), (0, 1));
        formula(&mut table, 1, 2, "=1+2", v2);
        let body = table.cell(1, 2).unwrap().text.clone();
        table.cell_mut(1, 2).unwrap().formula = Some(Arc::new(
            CellFormula::from_profile_display(
                if v2 { FORMULA_PROFILE_V2 } else { FORMULA_PROFILE },
                "=1+3".into(),
                FormulaOutcome::Number { lexical: "3".into() },
                &body,
            )
            .unwrap(),
        ));
        let before = table.clone();
        assert!(table.validate_formulas().is_err());
        assert_eq!(table, before);
    }
}
#[test]
fn plans_bind_valid_merge_ownership_even_when_all_scalars_are_identical() {
    for v2 in [false, true] {
        let mut table = Table::new(2, 3, 300., 30.);
        formula(&mut table, 1, 2, "=1+2", v2);
        let plan = table.plan_recalculation().unwrap();
        table.cell_mut(0, 0).unwrap().grid_span = 2;
        table.cell_mut(0, 1).unwrap().h_merge = true;
        let before = table.clone();
        assert!(plan.apply(&mut table).is_err());
        assert_eq!(table, before);
    }
}
#[test]
fn plans_refuse_ragged_rows_with_the_same_flat_cell_count() {
    for v2 in [false, true] {
        let mut table = Table::new(3, 3, 300., 30.);
        formula(&mut table, 2, 2, "=1+2", v2);
        let plan = table.plan_recalculation().unwrap();
        let moved = table.rows[0].cells.pop().unwrap();
        table.rows[1].cells.push(moved);
        let before = table.clone();
        assert!(plan.apply(&mut table).is_err());
        assert_eq!(table, before);
    }
}
#[test]
fn checked_axis_rewrites_keep_merged_ownership_and_profile() {
    for v2 in [false, true] {
        let mut table = Table::new(3, 3, 300., 30.);
        constant(&mut table, 1, 0, "7");
        formula(&mut table, 2, 2, "=[.A2]*2", v2);
        merge(&mut table, (0, 0), (0, 1));
        table.edit_axis_with_budget(false, 0, false, &mut FormulaBudget::default()).unwrap();
        assert_eq!(table.cell(1, 0).unwrap().grid_span, 2);
        assert_eq!(table.cell(3, 2).unwrap().formula.as_ref().unwrap().expression(), "=[.A3]*2");
        assert_eq!(current_text(&table, 3, 2), "14");
        table.edit_axis_with_budget(true, 0, false, &mut FormulaBudget::default()).unwrap();
        assert_eq!(table.cell(3, 3).unwrap().formula.as_ref().unwrap().expression(), "=[.B3]*2");
        assert_eq!(current_text(&table, 3, 3), "14");
        table.validate_formulas().unwrap();
    }
}
#[test]
fn unchanged_plans_apply_and_changed_current_displays_still_detach() {
    for v2 in [false, true] {
        let mut table = Table::new(2, 3, 300., 30.);
        merge(&mut table, (0, 0), (0, 1));
        formula(&mut table, 1, 2, "=1+2", v2);
        let plan = table.plan_recalculation().unwrap();
        assert!(!plan.apply(&mut table).unwrap());
        table.cell_mut(1, 2).unwrap().text = TextBody::from_text("edited literal");
        table.recalculate().unwrap();
        assert!(table.cell(1, 2).unwrap().formula.is_none());
        assert_eq!(current_text(&table, 1, 2), "edited literal");
    }
}
