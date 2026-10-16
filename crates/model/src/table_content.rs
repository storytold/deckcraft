//! Ordered table-cell content and explicit bounded paths to nested owners.
//!
//! `Cell::text` is the first text block. `Cell::blocks` continues after it in
//! document order, so text never has two authoritative stored copies.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{Cell, Table};
use crate::{ModelError, Result, TextBody};

pub const MAX_NESTED_TABLE_DEPTH: usize = 16;
pub const MAX_CELL_BLOCKS: usize = 1024;
pub const MAX_TABLE_CONTENT_BLOCKS: usize = 1_000_000;
pub const MAX_TABLE_CONTENT_CELLS: usize = 1_000_000;

fn invalid(message: &'static str) -> ModelError {
    ModelError::Invalid(message.into())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellTextBlock {
    pub id: u32,
    pub body: TextBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NestedTable {
    pub id: u32,
    pub table: Box<Table>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase", deny_unknown_fields)]
pub enum CellBlock {
    Text(CellTextBlock),
    Table(NestedTable),
}

impl CellBlock {
    pub fn id(&self) -> u32 {
        match self {
            Self::Text(block) => block.id,
            Self::Table(block) => block.id,
        }
    }
}

/// A step resolves one child by its stable cell-local identity, never a stale
/// vector offset. Row/column relocation is applied by structural edit plans.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TablePathStep {
    pub row: usize,
    pub col: usize,
    pub child: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TablePath(pub Vec<TablePathStep>);

impl TablePath {
    pub fn validate(&self) -> Result<()> {
        if self.0.len() > MAX_NESTED_TABLE_DEPTH || self.0.iter().any(|s| s.child == 0) {
            return Err(invalid("invalid or over-depth nested-table path"));
        }
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Cell {
    pub fn text_block(&self, id: Option<u32>) -> Option<&TextBody> {
        match id {
            None | Some(0) => Some(&self.text),
            Some(id) => self.blocks.iter().find_map(|block| match block {
                CellBlock::Text(block) if block.id == id => Some(&block.body),
                _ => None,
            }),
        }
    }
    pub fn text_block_mut(&mut self, id: Option<u32>) -> Option<&mut TextBody> {
        match id {
            None | Some(0) => Some(&mut self.text),
            Some(id) => self.blocks.iter_mut().find_map(|block| match block {
                CellBlock::Text(block) if block.id == id => Some(&mut block.body),
                _ => None,
            }),
        }
    }
    pub fn next_block_id(&self) -> Result<u32> {
        if self.blocks.len() >= MAX_CELL_BLOCKS {
            return Err(invalid("table cell block count exceeds limit"));
        }
        self.blocks.iter().map(CellBlock::id).max().unwrap_or(0).checked_add(1).ok_or_else(|| invalid("cell block identity space exhausted"))
    }
    pub fn append_nested_table(&mut self, table: Table) -> Result<u32> {
        table.validate_content()?;
        let id = self.next_block_id()?;
        self.blocks.try_reserve(1).map_err(|_| invalid("cannot allocate nested-table block"))?;
        self.blocks.push(CellBlock::Table(NestedTable { id, table: Box::new(table) }));
        Ok(id)
    }
    pub fn append_text_block(&mut self, body: TextBody) -> Result<u32> {
        let id = self.next_block_id()?;
        self.blocks.try_reserve(1).map_err(|_| invalid("cannot allocate table-cell text block"))?;
        self.blocks.push(CellBlock::Text(CellTextBlock { id, body }));
        Ok(id)
    }
    pub fn text_blocks(&self) -> impl Iterator<Item = &TextBody> {
        std::iter::once(&self.text).chain(self.blocks.iter().filter_map(|b| match b {
            CellBlock::Text(b) => Some(&b.body),
            _ => None,
        }))
    }
}

impl Table {
    pub fn visit_texts(&self, visit: &mut impl FnMut(&TextBody) -> Result<()>) -> Result<()> {
        if self.rows.iter().flat_map(|r| &r.cells).any(|c| !c.blocks.is_empty()) {
            self.validate_content()?;
        }
        fn walk(table: &Table, depth: usize, visit: &mut impl FnMut(&TextBody) -> Result<()>) -> Result<()> {
            if depth > MAX_NESTED_TABLE_DEPTH {
                return Err(invalid("nested-table depth exceeds limit"));
            }
            for cell in table.rows.iter().flat_map(|r| &r.cells) {
                visit(&cell.text)?;
                for block in &cell.blocks {
                    match block {
                        CellBlock::Text(block) => visit(&block.body)?,
                        CellBlock::Table(child) => walk(&child.table, depth + 1, visit)?,
                    }
                }
            }
            Ok(())
        }
        walk(self, 0, visit)
    }

    pub fn visit_texts_mut(&mut self, visit: &mut impl FnMut(&mut TextBody) -> Result<()>) -> Result<()> {
        if self.rows.iter().flat_map(|r| &r.cells).any(|c| !c.blocks.is_empty()) {
            self.validate_content()?;
        }
        fn walk(table: &mut Table, depth: usize, visit: &mut impl FnMut(&mut TextBody) -> Result<()>) -> Result<()> {
            if depth > MAX_NESTED_TABLE_DEPTH {
                return Err(invalid("nested-table depth exceeds limit"));
            }
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    visit(&mut cell.text)?;
                    for block in &mut cell.blocks {
                        match block {
                            CellBlock::Text(block) => visit(&mut block.body)?,
                            CellBlock::Table(child) => walk(&mut child.table, depth + 1, visit)?,
                        }
                    }
                }
            }
            Ok(())
        }
        walk(self, 0, visit)
    }

    pub fn at_path(&self, path: &TablePath) -> Result<&Self> {
        path.validate()?;
        let mut current = self;
        for step in &path.0 {
            let cell = current.cell(step.row, step.col).ok_or_else(|| invalid("nested-table path cell is missing"))?;
            let mut found = cell.blocks.iter().filter_map(|b| match b {
                CellBlock::Table(child) if child.id == step.child => Some(child.table.as_ref()),
                _ => None,
            });
            current = found.next().ok_or_else(|| invalid("nested-table path child is missing"))?;
            if found.next().is_some() {
                return Err(invalid("nested-table path child identity is ambiguous"));
            }
        }
        Ok(current)
    }
    pub fn at_path_mut(&mut self, path: &TablePath) -> Result<&mut Self> {
        self.at_path(path)?;
        fn descend<'a>(table: &'a mut Table, steps: &[TablePathStep]) -> Result<&'a mut Table> {
            let Some((step, rest)) = steps.split_first() else {
                return Ok(table);
            };
            let cell = table.cell_mut(step.row, step.col).ok_or_else(|| invalid("nested-table path cell is missing"))?;
            let child = cell
                .blocks
                .iter_mut()
                .find_map(|b| match b {
                    CellBlock::Table(child) if child.id == step.child => Some(child.table.as_mut()),
                    _ => None,
                })
                .ok_or_else(|| invalid("nested-table path child is missing"))?;
            descend(child, rest)
        }
        descend(self, &path.0)
    }

    /// Validate depth/count/identity/geometry before recursive clone, layout or
    /// serialization. Every attachment is counted even if values happen to match.
    pub fn validate_content(&self) -> Result<()> {
        let mut stack = Vec::new();
        stack.try_reserve(MAX_NESTED_TABLE_DEPTH + 1).map_err(|_| invalid("cannot allocate table validation stack"))?;
        stack.push((self, 0usize));
        let mut cells = 0usize;
        let mut blocks = 0usize;
        let mut tables = 0usize;
        while let Some((table, depth)) = stack.pop() {
            tables =
                tables.checked_add(1).filter(|v| *v <= MAX_TABLE_CONTENT_CELLS).ok_or_else(|| invalid("nested-table owner count exceeds limit"))?;
            if depth > MAX_NESTED_TABLE_DEPTH {
                return Err(invalid("nested-table depth exceeds limit"));
            }
            if table.cols.is_empty()
                || table.rows.is_empty()
                || table.cols.len() > MAX_TABLE_CONTENT_CELLS
                || table.rows.len() > MAX_TABLE_CONTENT_CELLS
                || table.cols.iter().any(|v| !v.is_finite() || *v <= 0.0 || *v > 20_000.0)
            {
                return Err(invalid("invalid nested-table grid dimensions"));
            }
            for row in &table.rows {
                if row.cells.len() != table.cols.len() || !row.height.is_finite() || row.height < 0.0 || row.height > 20_000.0 {
                    return Err(invalid("invalid nested-table row dimensions"));
                }
                cells = cells
                    .checked_add(row.cells.len())
                    .filter(|v| *v <= MAX_TABLE_CONTENT_CELLS)
                    .ok_or_else(|| invalid("nested-table cell count exceeds limit"))?;
                for cell in &row.cells {
                    if cell.blocks.len() > MAX_CELL_BLOCKS {
                        return Err(invalid("table cell block count exceeds limit"));
                    }
                    blocks = blocks
                        .checked_add(cell.blocks.len())
                        .filter(|v| *v <= MAX_TABLE_CONTENT_BLOCKS)
                        .ok_or_else(|| invalid("table content block count exceeds limit"))?;
                    let mut identities = HashSet::new();
                    identities.try_reserve(cell.blocks.len()).map_err(|_| invalid("cannot allocate cell block identity index"))?;
                    for block in &cell.blocks {
                        if block.id() == 0 || !identities.insert(block.id()) {
                            return Err(invalid("duplicate or zero cell block identity"));
                        }
                        if let CellBlock::Table(child) = block {
                            if cell.is_covered() {
                                return Err(invalid("nested content has a covered cell owner"));
                            }
                            if depth >= MAX_NESTED_TABLE_DEPTH {
                                return Err(invalid("nested-table depth exceeds limit"));
                            }
                            stack.try_reserve(1).map_err(|_| invalid("cannot allocate nested-table traversal"))?;
                            stack.push((&child.table, depth + 1));
                        }
                    }
                    if let Some(format) = &cell.number_format {
                        format.validate()?;
                    }
                }
            }
        }
        Ok(())
    }
}
