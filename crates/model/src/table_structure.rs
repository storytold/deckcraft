//! Atomic grid edits: rewrite references before publishing any changed owner.
use super::{CellFormula, FormulaBudget, Table};
use crate::{ModelError, Result};
use std::sync::Arc;
fn invalid(message: &'static str) -> ModelError {
    ModelError::Invalid(message.into())
}
impl Table {
    /// Insert/delete one physical row or column, retaining the formula profile.
    /// References to removed cells become typed #REF; ranges shrink in place.
    pub fn edit_axis_with_budget(&mut self, columns: bool, at: usize, delete: bool, budget: &mut FormulaBudget) -> Result<()> {
        self.validate_content()?;
        self.validate_formulas_with_budget(budget)?;
        super::validate_constant_grid(self)?;
        let length = if columns { self.n_cols() } else { self.n_rows() };
        if (delete && (at >= length || length <= 1)) || (!delete && at > length) {
            return Err(invalid("invalid table structural edit position"));
        }
        let mut candidate = self.clone();
        let mut formulas = Vec::new();
        for (row, values) in candidate.rows.iter_mut().enumerate() {
            for (col, cell) in values.cells.iter_mut().enumerate() {
                if let Some(formula) = cell.formula.take() {
                    let coordinate = if columns { col } else { row };
                    if delete && coordinate == at {
                        continue;
                    }
                    let shifted = if delete && coordinate > at {
                        coordinate - 1
                    } else if !delete && coordinate >= at {
                        coordinate.checked_add(1).ok_or_else(|| invalid("table coordinate overflow"))?
                    } else {
                        coordinate
                    };
                    let expression = formula.rewritten_axis(columns, at, 1, delete)?;
                    formulas.push((if columns { row } else { shifted }, if columns { shifted } else { col }, formula, expression));
                }
            }
        }
        if delete {
            let changed = if columns { candidate.delete_col(at) } else { candidate.delete_row(at) };
            if !changed {
                return Err(invalid("table deletion could not be applied"));
            }
        } else {
            super::validate_constant_growth(&candidate, usize::from(!columns), usize::from(columns))?;
            if columns {
                candidate.insert_col(at);
            } else {
                candidate.insert_row(at);
            }
        }
        for (row, col, formula, expression) in formulas {
            let cell = candidate.cell_mut(row, col).ok_or_else(|| invalid("rewritten formula owner is missing"))?;
            cell.formula = Some(Arc::new(CellFormula::from_profile_display(formula.profile(), expression, formula.outcome().clone(), &cell.text)?));
        }
        budget.next_pass();
        candidate.recalculate_with_budget(budget)?;
        candidate.validate_content()?;
        super::validate_constant_grid(&candidate)?;
        *self = candidate;
        Ok(())
    }
}

impl Table {
    pub fn merge_with_budget(&mut self, from: (usize, usize), to: (usize, usize), budget: &mut FormulaBudget) -> Result<()> {
        self.validate_content()?;
        self.validate_formulas_with_budget(budget)?;
        super::validate_constant_grid(self)?;
        let (mut r0, mut r1) = (from.0.min(to.0), from.0.max(to.0));
        let (mut c0, mut c1) = (from.1.min(to.1), from.1.max(to.1));
        if r1 >= self.n_rows() || c1 >= self.n_cols() || (r0 == r1 && c0 == c1) {
            return Err(invalid("invalid merge rectangle"));
        }
        let mut work = 0usize;
        loop {
            let before = (r0, c0, r1, c1);
            for (row, values) in self.rows.iter().enumerate() {
                for (col, cell) in values.cells.iter().enumerate() {
                    work = work.checked_add(1).filter(|n| *n <= 2_000_000).ok_or_else(|| invalid("merge planning exceeds work limit"))?;
                    if cell.is_covered() {
                        continue;
                    }
                    let end_row = row.saturating_add(cell.row_span.max(1) as usize).saturating_sub(1).min(self.n_rows() - 1);
                    let end_col = col.saturating_add(cell.grid_span.max(1) as usize).saturating_sub(1).min(self.n_cols() - 1);
                    if row <= r1 && end_row >= r0 && col <= c1 && end_col >= c0 {
                        r0 = r0.min(row);
                        c0 = c0.min(col);
                        r1 = r1.max(end_row);
                        c1 = c1.max(end_col);
                    }
                }
            }
            if before == (r0, c0, r1, c1) {
                break;
            }
        }
        let mut candidate = self.clone();
        let mut saved = Vec::new();
        for (row, values) in candidate.rows.iter_mut().enumerate() {
            for (col, cell) in values.cells.iter_mut().enumerate() {
                if let Some(formula) = cell.formula.take()
                    && !(row >= r0 && row <= r1 && col >= c0 && col <= c1)
                {
                    saved.push((row, col, formula));
                }
            }
        }
        let ordered = (r0..=r1).any(|r| (c0..=c1).any(|c| candidate.cell(r, c).is_some_and(|cell| !cell.blocks.is_empty())));
        let mut bodies = Vec::new();
        let mut blocks = Vec::new();
        let mut first = true;
        let mut next = 0u32;
        let mut push = |mut block: super::CellBlock| -> Result<super::CellBlock> {
            next = next.checked_add(1).ok_or_else(|| invalid("merged block identity overflow"))?;
            match &mut block {
                super::CellBlock::Text(block) => block.id = next,
                super::CellBlock::Table(child) => child.id = next,
            };
            Ok(block)
        };
        for row in r0..=r1 {
            for col in c0..=c1 {
                let cell = candidate.cell_mut(row, col).ok_or_else(|| invalid("missing merged owner"))?;
                if cell.is_covered() {
                    cell.data = None;
                    cell.number_format = None;
                    cell.blocks.clear();
                    continue;
                }
                // Formatting is baked into ordinary rich text as values become literal.
                if let Some(format) = &cell.number_format {
                    let value = cell.data.as_ref().map(|data| data.value().clone()).or_else(|| {
                        self.cell(row, col).and_then(|original| original.formula.as_ref()).and_then(|formula| match formula.outcome() {
                            super::FormulaOutcome::Number { lexical } => {
                                Some(super::CellValue::Number { lexical: lexical.clone(), kind: super::CellNumberKind::Float })
                            }
                            super::FormulaOutcome::String { value } => Some(super::CellValue::String { value: value.clone() }),
                            super::FormulaOutcome::Boolean { value } => Some(super::CellValue::Boolean { value: *value }),
                            _ => None,
                        })
                    });
                    if let Some(value) = value
                        && let Some(color) = format.format_value(&value)?.color
                    {
                        for paragraph in &mut cell.text.paragraphs {
                            for run in &mut paragraph.runs {
                                run.props.fill = Some(crate::Fill::solid(crate::ColorRef::rgb(deckcraft_color::Rgba::rgba(
                                    color[0], color[1], color[2], color[3],
                                ))));
                            }
                        }
                    }
                }
                cell.data = None;
                cell.number_format = None;
                let body = std::mem::take(&mut cell.text);
                if ordered {
                    if first {
                        bodies.push(body);
                        first = false;
                    } else if !body.paragraphs.is_empty() {
                        blocks.push(push(super::CellBlock::Text(super::CellTextBlock { id: 0, body }))?);
                    }
                    for block in std::mem::take(&mut cell.blocks) {
                        blocks.push(push(block)?);
                    }
                    if blocks.len() > super::MAX_CELL_BLOCKS {
                        return Err(invalid("merged content exceeds cell block limit"));
                    }
                } else if !body.is_empty() {
                    bodies.push(body);
                }
            }
        }
        let merges: Vec<_> = candidate
            .merges()
            .into_iter()
            .filter(|merge| merge.r < r0 || merge.r > r1 || merge.c < c0 || merge.c > c1)
            .chain(std::iter::once(super::Merge { r: r0, c: c0, rs: r1 - r0 + 1, gs: c1 - c0 + 1 }))
            .collect();
        candidate.set_merges(merges);
        let anchor = candidate.cell_mut(r0, c0).ok_or_else(|| invalid("missing merge anchor"))?;
        let mut bodies = bodies.into_iter();
        if ordered {
            anchor.text = bodies.next().unwrap_or_default();
        } else {
            let original = self.cell(r0, c0).ok_or_else(|| invalid("missing original anchor"))?;
            anchor.text = original.text.clone();
            let paragraphs: Vec<_> = bodies.flat_map(|body| body.paragraphs).collect();
            if !paragraphs.is_empty() {
                anchor.text.paragraphs = paragraphs;
            }
        }
        anchor.blocks = blocks;
        for (row, col, formula) in saved {
            candidate.cell_mut(row, col).ok_or_else(|| invalid("missing surviving formula"))?.formula = Some(formula);
        }
        budget.next_pass();
        candidate.recalculate_with_budget(budget)?;
        candidate.validate_content()?;
        super::validate_constant_grid(&candidate)?;
        *self = candidate;
        Ok(())
    }
    pub fn split_with_budget(&mut self, row: usize, col: usize, budget: &mut FormulaBudget) -> Result<()> {
        self.validate_content()?;
        self.validate_formulas_with_budget(budget)?;
        super::validate_constant_grid(self)?;
        let mut candidate = self.clone();
        let mut formulas = Vec::new();
        for (row, values) in candidate.rows.iter_mut().enumerate() {
            for (col, cell) in values.cells.iter_mut().enumerate() {
                if let Some(formula) = cell.formula.take() {
                    formulas.push((row, col, formula));
                }
            }
        }
        if !candidate.split(row, col) {
            return Err(invalid("selected cell is not a merged anchor"));
        }
        for (row, col, formula) in formulas {
            candidate.cell_mut(row, col).ok_or_else(|| invalid("missing surviving formula"))?.formula = Some(formula);
        }
        budget.next_pass();
        candidate.recalculate_with_budget(budget)?;
        candidate.validate_content()?;
        super::validate_constant_grid(&candidate)?;
        *self = candidate;
        Ok(())
    }
}
