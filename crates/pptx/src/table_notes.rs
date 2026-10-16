//! Text-free bridge from original notes table styles to verified current table content.
use super::invalid;
use crate::{PptxError, notes_projection::BodyStyle};
use deckcraft_model::{
    Table, TextBody,
    table::{CellBlock, CellConstant, CellFormula, TablePath, TablePathStep},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Body {
    pub path: TablePath,
    pub row: usize,
    pub col: usize,
    pub block: Option<u32>,
    pub style: BodyStyle,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    /// Some identifies an original shared master; None identifies this local page.
    pub master: Option<u32>,
    pub original_shape: u32,
    pub projected_shape: u32,
    pub bodies: Vec<Body>,
}
fn clear(body: &mut TextBody) {
    for paragraph in &mut body.paragraphs {
        for run in &mut paragraph.runs {
            run.text.clear();
        }
    }
}
pub(crate) fn strip(table: &mut Table, depth: usize) -> Result<(), PptxError> {
    if depth > 16 {
        return Err(invalid("notes table bridge depth limit"));
    }
    for cell in table.rows.iter_mut().flat_map(|r| &mut r.cells) {
        clear(&mut cell.text);
        cell.data = None;
        cell.formula = None;
        for block in &mut cell.blocks {
            match block {
                CellBlock::Text(block) => clear(&mut block.body),
                CellBlock::Table(child) => strip(&mut child.table, depth + 1)?,
            }
        }
    }
    Ok(())
}
pub(crate) fn describe(original: &Table, projected: &Table) -> Result<Vec<Body>, PptxError> {
    fn visit(original: &Table, projected: &Table, path: &TablePath, out: &mut Vec<Body>, depth: usize) -> Result<(), PptxError> {
        if depth > 16 || original.rows.len() != projected.rows.len() || original.cols.len() != projected.cols.len() {
            return Err(invalid("notes table bridge topology changed"));
        }
        for (row, values) in original.rows.iter().enumerate() {
            for (col, cell) in values.cells.iter().enumerate() {
                let current = projected.cell(row, col).ok_or_else(|| invalid("notes projected cell disappeared"))?;
                if cell.blocks.len() != current.blocks.len() {
                    return Err(invalid("notes projected cell blocks changed"));
                }
                out.push(Body { path: path.clone(), row, col, block: None, style: BodyStyle::new(&cell.text, &current.text)? });
                for (source, current) in cell.blocks.iter().zip(&current.blocks) {
                    match (source, current) {
                        (CellBlock::Text(source), CellBlock::Text(current)) if source.id == current.id => out.push(Body {
                            path: path.clone(),
                            row,
                            col,
                            block: Some(source.id),
                            style: BodyStyle::new(&source.body, &current.body)?,
                        }),
                        (CellBlock::Table(source), CellBlock::Table(current)) if source.id == current.id => {
                            let mut path = path.clone();
                            path.0.push(TablePathStep { row, col, child: source.id });
                            visit(&source.table, &current.table, &path, out, depth + 1)?;
                        }
                        _ => return Err(invalid("notes table bridge block identity changed")),
                    }
                }
            }
        }
        Ok(())
    }
    let mut bodies = Vec::new();
    visit(original, projected, &TablePath::default(), &mut bodies, 0)?;
    Ok(bodies)
}
pub(crate) fn restore(target: &mut Table, current: &Table, bodies: &[Body]) -> Result<(), PptxError> {
    super::validate_skeleton(target, 0)?;
    let mut seen = std::collections::HashSet::new();
    for binding in bodies {
        let key = serde_json::to_string(&(binding.path.clone(), binding.row, binding.col, binding.block)).map_err(invalid)?;
        if !seen.insert(key) {
            return Err(invalid("duplicate notes table text binding"));
        }
        let core = current
            .at_path(&binding.path)
            .map_err(invalid)?
            .cell(binding.row, binding.col)
            .and_then(|c| c.text_block(binding.block))
            .ok_or_else(|| invalid("current notes table text was deleted"))?;
        let source = target
            .at_path_mut(&binding.path)
            .map_err(invalid)?
            .cell_mut(binding.row, binding.col)
            .and_then(|c| c.text_block_mut(binding.block))
            .ok_or_else(|| invalid("original notes table text address is missing"))?;
        *source = binding.style.restore(core)?;
    }
    fn values(target: &mut Table, current: &Table, depth: usize) -> Result<(), PptxError> {
        if depth > 16 || target.rows.len() != current.rows.len() || target.cols.len() != current.cols.len() {
            return Err(invalid("notes table value topology changed"));
        }
        for (row, values_row) in target.rows.iter_mut().enumerate() {
            for (col, cell) in values_row.cells.iter_mut().enumerate() {
                let core = current.cell(row, col).ok_or_else(|| invalid("current notes typed cell was deleted"))?;
                if cell.number_format != core.number_format || cell.blocks.len() != core.blocks.len() {
                    return Err(invalid("notes typed table style or block ownership changed"));
                }
                if let Some(value) = &core.data {
                    cell.data = Some(Arc::new(CellConstant::from_display(value.value().clone(), &cell.text).map_err(invalid)?));
                }
                if let Some(formula) = &core.formula {
                    cell.formula = Some(Arc::new(
                        CellFormula::from_profile_display(formula.profile(), formula.expression().into(), formula.outcome().clone(), &cell.text)
                            .map_err(invalid)?,
                    ));
                }
                for (source, current) in cell.blocks.iter_mut().zip(&core.blocks) {
                    if let (CellBlock::Table(source), CellBlock::Table(current)) = (source, current) {
                        if source.id != current.id {
                            return Err(invalid("notes nested table identity changed"));
                        }
                        values(&mut source.table, &current.table, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }
    values(target, current, 0)
}
