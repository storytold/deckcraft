//! Typed cell owners must not overlap or rely on structural normalization.
use super::Table;
use crate::{ModelError, Result};

fn invalid() -> ModelError {
    ModelError::Invalid("typed table constants require a valid nonoverlapping merged grid".into())
}

fn oversized() -> ModelError {
    ModelError::Invalid("table grid exceeds bounded cell or dimension limits".into())
}

/// Check prospective insertion before allocating an enlarged grid, including literals.
pub fn validate_constant_growth(table: &Table, extra_rows: usize, extra_cols: usize) -> Result<()> {
    super::validate_formula_topology(table)?;
    validate_constant_grid(table)?;
    let rows = table.rows.len().checked_add(extra_rows).ok_or_else(oversized)?;
    let cols = table.cols.len().checked_add(extra_cols).ok_or_else(oversized)?;
    if rows > 1_000_000 || cols > 1_000_000 || rows.checked_mul(cols).is_none_or(|cells| cells > 1_000_000) {
        return Err(oversized());
    }
    let existing = table.rows.iter().try_fold(0usize, |count, row| count.checked_add(row.cells.len())).ok_or_else(oversized)?;
    let new_rows = extra_rows.checked_mul(table.cols.len()).ok_or_else(oversized)?;
    let new_cols = extra_cols.checked_mul(rows).ok_or_else(oversized)?;
    if existing.checked_add(new_rows).and_then(|count| count.checked_add(new_cols)).is_none_or(|count| count > 1_000_000) {
        return Err(oversized());
    }
    Ok(())
}

/// Legacy literal-only tables retain their existing repair behavior. A typed grid
/// is checked in linear work with bounded column scratch before any mutation.
pub fn validate_constant_grid(table: &Table) -> Result<()> {
    validate_grid(table, false)
}

/// Formula assignment must prove ownership before the first typed attachment,
/// when the ordinary constant validator still permits legacy literal repairs.
pub(super) fn validate_formula_grid_topology(table: &Table) -> Result<()> {
    validate_merged_grid(table)
}

/// Prove physical cell ownership, including literal-only merged tables, without
/// repairing overlaps, orphaned covered cells or out-of-bounds spans.
pub fn validate_merged_grid(table: &Table) -> Result<()> {
    validate_grid(table, true)
}

fn validate_grid(table: &Table, strict_topology: bool) -> Result<()> {
    if table.rows.len() > 1_000_000
        || table.cols.len() > 1_000_000
        || table.rows.len().checked_mul(table.cols.len()).is_none_or(|cells| cells > 1_000_000)
    {
        return Err(oversized());
    }
    let mut count = 0usize;
    let mut has_data = false;
    for row in &table.rows {
        count = count.checked_add(row.cells.len()).filter(|count| *count <= 1_000_000).ok_or_else(invalid)?;
        has_data |= row.cells.iter().any(|cell| cell.data.is_some() || cell.formula.is_some());
    }
    if !has_data && !strict_topology {
        return Ok(());
    }
    let cols = table.cols.len();
    let rows = table.rows.len();
    if cols == 0 || rows == 0 || rows.checked_mul(cols).is_none_or(|cells| cells > 1_000_000) {
        return Err(invalid());
    }
    if table.rows.iter().any(|row| row.cells.len() != cols) {
        return Err(invalid());
    }
    // (anchor row, anchor column, exclusive last row). Covered columns inherit
    // one owner; expired entries can be reused without rescanning span areas.
    let mut active: Vec<Option<(usize, usize, usize)>> = Vec::new();
    active.try_reserve_exact(cols).map_err(|_| invalid())?;
    active.resize(cols, None);
    for (r, row) in table.rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().enumerate() {
            let owner = active.get(c).copied().flatten().filter(|(_, _, end)| *end > r);
            if let Some((anchor_r, anchor_c, _)) = owner {
                if cell.h_merge != (c > anchor_c)
                    || cell.v_merge != (r > anchor_r)
                    || (cell.data.is_some() || cell.formula.is_some())
                    || cell.grid_span != 1
                    || cell.row_span != 1
                {
                    return Err(invalid());
                }
                continue;
            }
            if cell.is_covered() || cell.grid_span == 0 || cell.row_span == 0 {
                return Err(invalid());
            }
            let end_c = c.checked_add(cell.grid_span as usize).filter(|end| *end <= cols).ok_or_else(invalid)?;
            let end_r = r.checked_add(cell.row_span as usize).filter(|end| *end <= rows).ok_or_else(invalid)?;
            for slot in active.get_mut(c..end_c).ok_or_else(invalid)? {
                if slot.is_some_and(|(_, _, end)| end > r) {
                    return Err(invalid());
                }
                *slot = Some((r, c, end_r));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::table::{Cell, Row};

    #[test]
    fn literal_growth_cannot_cross_the_same_limit_that_would_prevent_later_deletion() {
        let table = Table::new(2, 1, 100., 20.);
        let before = table.clone();
        for (rows, cols) in [(999_998, 0), (0, 499_999), (998, 999)] {
            validate_constant_growth(&table, rows, cols).unwrap();
        }
        for (rows, cols) in [(999_999, 0), (0, 500_000), (998, 1000), (usize::MAX, 0), (0, usize::MAX)] {
            assert!(validate_constant_growth(&table, rows, cols).is_err());
        }
        assert_eq!(table, before);
    }

    #[test]
    fn literal_ragged_normalization_refuses_rectangular_expansion_before_allocating_cells() {
        let mut table = Table { cols: vec![1.; 1000], rows: vec![Row::default(); 1001], ..Default::default() };
        let before = table.clone();
        assert!(validate_constant_grid(&table).is_err());
        table.normalize();
        assert_eq!(table, before);
    }

    #[test]
    fn ragged_literal_growth_counts_existing_physical_cells_in_addition_to_future_rectangle() {
        let table = Table { cols: vec![1.], rows: vec![Row { cells: vec![Cell::new(""); 2], ..Default::default() }], ..Default::default() };
        validate_constant_growth(&table, 999_998, 0).unwrap();
        assert!(validate_constant_growth(&table, 999_999, 0).is_err());
        assert!(validate_constant_growth(&Table::default(), 1_000_001, 0).is_err());
        assert!(validate_constant_growth(&Table::default(), 0, 1_000_001).is_err());
    }
}
