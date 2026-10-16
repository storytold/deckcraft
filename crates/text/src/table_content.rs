//! One immutable geometry plan shared by paint, hit testing and text placement.
use crate::Fields;
use deckcraft_geom::{Affine, Rect};
use deckcraft_model::resolve::Ctx;
use deckcraft_model::table::{CellBlock, TablePath};
use deckcraft_model::text::Anchor;
use deckcraft_model::{Result, Shape, Table, TextBody};

pub struct TableLayout {
    pub heights: Vec<f64>,
    pub cells: Vec<CellLayout>,
}
pub struct CellLayout {
    pub row: usize,
    pub col: usize,
    pub rect: Rect,
    pub blocks: Vec<BlockLayout>,
}
pub enum BlockLayout {
    Text { id: Option<u32>, body: TextBody, rect: Rect },
    Table { id: u32, transform: Affine, layout: Box<TableLayout> },
}
impl TableLayout {
    pub fn height(&self) -> f64 {
        self.heights.iter().sum()
    }
    pub fn text_at(&self, path: &TablePath, row: usize, col: usize, id: Option<u32>) -> Option<(&TextBody, Rect, Affine)> {
        let mut current = self;
        let mut transform = Affine::IDENTITY;
        for step in &path.0 {
            let cell = current.cells.iter().find(|c| c.row == step.row && c.col == step.col)?;
            let (next, matrix) = cell.blocks.iter().find_map(|b| match b {
                BlockLayout::Table { id, transform, layout } if *id == step.child => Some((layout.as_ref(), *transform)),
                _ => None,
            })?;
            current = next;
            transform *= matrix;
        }
        let cell = current.cells.iter().find(|c| c.row == row && c.col == col)?;
        let (body, rect) = cell.blocks.iter().find_map(|b| match b {
            BlockLayout::Text { id: block, body, rect } if block.unwrap_or(0) == id.unwrap_or(0) => Some((body, *rect)),
            _ => None,
        })?;
        Some((body, rect, transform))
    }
}

pub fn layout(ctx: &Ctx, table: &Table, fields: &dyn Fields) -> Result<TableLayout> {
    if table.rows.iter().flat_map(|r| &r.cells).any(|c| !c.blocks.is_empty()) {
        table.validate_content()?;
    }
    build(ctx, table, fields, 0)
}
fn build(ctx: &Ctx, table: &Table, fields: &dyn Fields, depth: usize) -> Result<TableLayout> {
    if depth > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
        return Err(deckcraft_model::ModelError::Invalid("nested layout exceeds depth limit".into()));
    }
    let xs = super::table::edges(&table.cols);
    let mut heights = super::table::plain_row_heights(ctx, table, fields);
    let mut cells = Vec::new();
    for (row, values) in table.rows.iter().enumerate() {
        for (col, cell) in values.cells.iter().enumerate().take(table.cols.len()) {
            if cell.is_covered() {
                continue;
            }
            let end = col.saturating_add(cell.grid_span.max(1) as usize).min(table.cols.len());
            let (Some(x0), Some(x1)) = (xs.get(col), xs.get(end)) else { continue };
            if cell.blocks.is_empty() {
                cells.push(CellLayout {
                    row,
                    col,
                    rect: Rect::ZERO,
                    blocks: vec![BlockLayout::Text { id: None, body: super::table::cell_body(ctx, table, row, col, cell), rect: Rect::ZERO }],
                });
                continue;
            }
            let [left, right, top, bottom] = cell.margins.unwrap_or([7.2, 7.2, 3.6, 3.6]);
            let width = (*x1 - *x0 - left - right).max(1.0);
            let mut blocks = Vec::new();
            let mut y = top;
            for (id, source) in std::iter::once((None, Some(&cell.text))).chain(cell.blocks.iter().map(|b| match b {
                CellBlock::Text(b) => (Some(b.id), Some(&b.body)),
                CellBlock::Table(b) => (Some(b.id), None),
            })) {
                if let Some(source) = source {
                    if source.paragraphs.is_empty() {
                        continue;
                    }
                    let mut body = super::table::block_body(ctx, table, row, col, cell, source);
                    body.body.inset_l = Some(0.0);
                    body.body.inset_r = Some(0.0);
                    body.body.inset_t = Some(0.0);
                    body.body.inset_b = Some(0.0);
                    body.body.anchor = Some(Anchor::Top);
                    let shape = Shape { text: Some(body.clone()), ..Default::default() };
                    let height = crate::fit_height(ctx, &shape, &body, Rect::new(0.0, 0.0, width, 1.0), fields).clamp(0.0, 20_000.0);
                    blocks.push(BlockLayout::Text { id, body, rect: Rect::new(*x0 + left, y, *x0 + left + width, y + height) });
                    y += height;
                } else if let Some(child) = cell.blocks.iter().find_map(|b| match b {
                    CellBlock::Table(b) if Some(b.id) == id => Some(b),
                    _ => None,
                }) {
                    let layout = build(ctx, &child.table, fields, depth + 1)?;
                    let child_width: f64 = child.table.cols.iter().sum();
                    let scale = (width / child_width).min(1.0);
                    blocks.push(BlockLayout::Table {
                        id: child.id,
                        transform: Affine::translate((*x0 + left, y)) * Affine::scale(scale),
                        layout: Box::new(layout),
                    });
                    if let Some(BlockLayout::Table { layout, .. }) = blocks.last() {
                        y += layout.height() * scale;
                    }
                }
            }
            let last = row.saturating_add(cell.row_span.max(1) as usize).min(heights.len());
            let current: f64 = heights.get(row..last).unwrap_or(&[]).iter().sum();
            let needed = y + bottom;
            if needed > current
                && let Some(height) = heights.get_mut(last.saturating_sub(1))
            {
                *height += needed - current;
            }
            cells.push(CellLayout { row, col, rect: Rect::ZERO, blocks });
        }
    }
    let rectangles = super::table::cell_rects(table, &heights);
    for cell in &mut cells {
        let Some((_, _, rect)) = rectangles.iter().find(|(r, c, _)| *r == cell.row && *c == cell.col) else { continue };
        cell.rect = *rect;
        let Some(source) = table.cell(cell.row, cell.col) else { continue };
        if source.blocks.is_empty() {
            if let Some(BlockLayout::Text { rect: text, .. }) = cell.blocks.first_mut() {
                *text = *rect;
            }
            continue;
        }
        let bottom = source.margins.unwrap_or([7.2, 7.2, 3.6, 3.6])[3];
        let extent = cell
            .blocks
            .iter()
            .map(|block| match block {
                BlockLayout::Text { rect, .. } => rect.y1,
                BlockLayout::Table { transform, layout, .. } => (*transform * deckcraft_geom::Point::new(0.0, layout.height())).y,
            })
            .fold(0.0, f64::max)
            + bottom;
        let free = (rect.height() - extent).max(0.0);
        let shift = rect.y0
            + match source.anchor.unwrap_or(Anchor::Top) {
                Anchor::Middle => free / 2.0,
                Anchor::Bottom => free,
                _ => 0.0,
            };
        for block in &mut cell.blocks {
            match block {
                BlockLayout::Text { rect, .. } => *rect = *rect + deckcraft_geom::Vec2::new(0.0, shift),
                BlockLayout::Table { transform, .. } => *transform = Affine::translate((0.0, shift)) * *transform,
            }
        }
    }
    Ok(TableLayout { heights, cells })
}

pub struct TableTextHit {
    pub path: TablePath,
    pub row: usize,
    pub col: usize,
    pub block: Option<u32>,
    pub position: (usize, usize),
}
impl TableLayout {
    /// Prefer the deepest visible child and then the closest text block in its cell.
    pub fn hit_text(&self, ctx: &Ctx, fields: &dyn Fields, point: deckcraft_geom::Point) -> Option<TableTextHit> {
        fn hit(plan: &TableLayout, ctx: &Ctx, fields: &dyn Fields, point: deckcraft_geom::Point, path: &TablePath) -> Option<TableTextHit> {
            let cell = plan.cells.iter().find(|cell| cell.rect.contains(point))?;
            for block in &cell.blocks {
                if let BlockLayout::Table { id, transform, layout } = block {
                    let local = transform.inverse() * point;
                    let mut child_path = path.clone();
                    child_path.0.push(deckcraft_model::table::TablePathStep { row: cell.row, col: cell.col, child: *id });
                    if let Some(found) = hit(layout, ctx, fields, local, &child_path) {
                        return Some(found);
                    }
                }
            }
            let candidate = cell
                .blocks
                .iter()
                .filter_map(|block| match block {
                    BlockLayout::Text { id, body, rect } => Some((*id, body, *rect)),
                    _ => None,
                })
                .min_by(|a, b| {
                    let distance = |rect: Rect| if point.y < rect.y0 { rect.y0 - point.y } else { (point.y - rect.y1).max(0.0) };
                    distance(a.2).total_cmp(&distance(b.2))
                })?;
            let shape = Shape { text: Some(candidate.1.clone()), ..Default::default() };
            let layout = crate::layout(ctx, &shape, candidate.1, &crate::Opts { rect: candidate.2, fields, prompt_color: None, no_shrink: false });
            let transform = crate::text_transform(Affine::IDENTITY, candidate.2, layout.rotation);
            let position = layout.hit(transform.inverse() * point);
            Some(TableTextHit { path: path.clone(), row: cell.row, col: cell.col, block: candidate.0, position: (position.para, position.ch) })
        }
        hit(self, ctx, fields, point, &TablePath::default())
    }
}
