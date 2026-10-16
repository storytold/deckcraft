//! Formula settlement at the unpublished document transaction boundary.

use deckcraft_model::{Master, Presentation, Shape, ShapeKind, Table};

use crate::{Result, cmd::bad};

#[derive(Default)]
struct Scan {
    owners: usize,
    shapes: usize,
    rows: usize,
    cells: usize,
}

fn charge(total: &mut usize, amount: usize, limit: usize) -> Result<()> {
    *total = total.checked_add(amount).filter(|sum| *sum <= limit).ok_or_else(|| bad("table.recalculate", "formula owner scan exceeds limit"))?;
    Ok(())
}

impl Scan {
    fn table(&mut self, table: &Table, depth: usize) -> Result<bool> {
        if depth > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
            return Err(bad("table.recalculate", "nested-table depth exceeds limit"));
        }
        charge(&mut self.rows, table.rows.len(), 1_000_000)?;
        for row in &table.rows {
            charge(&mut self.cells, row.cells.len(), 1_000_000)?;
            for cell in &row.cells {
                if cell.formula.is_some() || cell.number_format.is_some() || !cell.blocks.is_empty() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn shapes(&mut self, shapes: &[Shape], depth: usize) -> Result<bool> {
        if depth > 64 {
            return Err(bad("table.recalculate", "formula owner nesting exceeds limit"));
        }
        charge(&mut self.shapes, shapes.len(), 100_000)?;
        for shape in shapes {
            match &shape.kind {
                ShapeKind::Table(table) if self.table(table, 0)? => {
                    return Ok(true);
                }
                ShapeKind::Group { children, .. } if !children.is_empty() && self.shapes(children, depth + 1)? => {
                    return Ok(true);
                }
                _ => {}
            }
        }
        Ok(false)
    }

    fn master(&mut self, master: &Master) -> Result<bool> {
        charge(&mut self.owners, master.layouts.len(), 100_000)?;
        if self.shapes(&master.shapes, 0)? {
            return Ok(true);
        }
        for layout in &master.layouts {
            if self.shapes(&layout.shapes, 0)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Settle all current formula tables before publishing a committed document edit.
///
/// Formula-free edits keep the established constant-edit behavior. Once a formula is
/// present, the model's all-owner preflight charges the complete document budget and
/// recalculates from authoritative cell values, never from the rendered display.
/// The caller owns the unpublished transaction copy and rolls it back on any error.
pub(crate) fn reconcile_document_with_budget(doc: &mut Presentation, budget: &mut deckcraft_model::table::FormulaBudget) -> Result<bool> {
    let mut scan = Scan::default();
    charge(&mut scan.owners, doc.slides.len(), 100_000)?;
    charge(&mut scan.owners, doc.masters.len(), 100_000)?;
    charge(&mut scan.owners, doc.notes_masters.len(), 100_000)?;
    charge(&mut scan.owners, usize::from(doc.notes_master.is_some()) + usize::from(doc.handout_master.is_some()), 100_000)?;
    for slide in &doc.slides {
        let has_formula = scan.shapes(&slide.shapes, 0)?;
        if has_formula {
            budget.next_pass();
            return Ok(deckcraft_model::table::reconcile_constants_with_budget(doc, budget)?);
        }
        if let Some(page) = &slide.notes_page {
            charge(&mut scan.owners, 1, 100_000)?;
            if scan.shapes(&page.shapes, 0)? {
                budget.next_pass();
                return Ok(deckcraft_model::table::reconcile_constants_with_budget(doc, budget)?);
            }
        }
    }
    for master in
        doc.masters.iter().chain(doc.notes_master.iter()).chain(doc.handout_master.iter()).chain(doc.notes_masters.iter().map(|notes| &notes.master))
    {
        if scan.master(master)? {
            budget.next_pass();
            return Ok(deckcraft_model::table::reconcile_constants_with_budget(doc, budget)?);
        }
    }
    Ok(false)
}
