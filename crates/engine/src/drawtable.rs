//! Atomic table pencil/eraser operations in intrinsic table coordinates.
//! New physical boundaries use the model's checked reference rewrite; cell
//! ownership is then split without copying canonical text or nested authority.
use deckcraft_geom::{Affine, Point};
use deckcraft_model::table::{Cell, CellBlock, CellConstantState, CellFormulaState, FormulaBudget, Table};

const SNAP: f64 = 4.0;
const MAX_GRID: usize = 200;
const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
struct Anchor {
    r: usize,
    c: usize,
    rs: usize,
    cs: usize,
}
struct OwnedAnchor {
    anchor: Anchor,
    cell: Cell,
}

fn anchors(table: &Table) -> Option<Vec<Anchor>> {
    let mut out = Vec::new();
    out.try_reserve_exact(table.n_rows().checked_mul(table.n_cols())?).ok()?;
    for (r, row) in table.rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().enumerate() {
            if !cell.is_covered() {
                out.push(Anchor { r, c, rs: usize::try_from(cell.row_span).ok()?, cs: usize::try_from(cell.grid_span).ok()? });
            }
        }
    }
    Some(out)
}

/// Bound the complete recursive source before cloning it. Serialization counts
/// bytes without allocating another JSON value/string or copying display bodies.
fn source_within_budget(table: &Table) -> bool {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|n| *n <= MAX_BYTES)
                .ok_or_else(|| std::io::Error::other("table gesture metadata byte limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    table.validate_content().is_ok() && serde_json::to_writer(Counter(0), table).is_ok()
}

fn validate_closure(table: &Table, budget: &mut FormulaBudget, depth: usize) -> Option<()> {
    if depth > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
        return None;
    }
    deckcraft_model::table::validate_merged_grid(table).ok()?;
    for cell in table.rows.iter().flat_map(|row| &row.cells) {
        if matches!(cell.constant_state().ok()?, CellConstantState::Stale) || matches!(cell.formula_state().ok()?, CellFormulaState::Stale) {
            return None;
        }
    }
    table.validate_formulas_with_budget(budget).ok()?;
    for cell in table.rows.iter().flat_map(|row| &row.cells) {
        for block in &cell.blocks {
            if let CellBlock::Table(child) = block {
                validate_closure(&child.table, budget, depth + 1)?;
            }
        }
    }
    Some(())
}
fn preflight(table: &Table, budget: &mut FormulaBudget) -> Option<()> {
    if table.n_rows() > 1000 || table.n_cols() > 1000 || !source_within_budget(table) {
        return None;
    }
    budget.next_pass();
    validate_closure(table, budget, 0)
}
fn edges(sizes: impl Iterator<Item = f64>) -> Option<Vec<f64>> {
    let mut out = vec![0.0];
    let mut total = 0.0;
    for size in sizes {
        total += size;
        if !total.is_finite() {
            return None;
        }
        out.try_reserve(1).ok()?;
        out.push(total);
    }
    Some(out)
}

/// A newly visible half inherits cell paint/borders/alignment, never another
/// authoritative value, formula/format program, canonical body or ordered child.
fn empty_like(cell: &Cell) -> Cell {
    Cell {
        text: deckcraft_model::TextBody::default(),
        fill: cell.fill.clone(),
        borders: cell.borders.clone(),
        diag_down: cell.diag_down.clone(),
        diag_up: cell.diag_up.clone(),
        margins: cell.margins,
        anchor: cell.anchor,
        vertical: cell.vertical,
        grid_span: 1,
        row_span: 1,
        ..Default::default()
    }
}
fn rebuild(table: &mut Table, anchors: Vec<OwnedAnchor>) -> Option<()> {
    let (nr, nc) = (table.n_rows(), table.n_cols());
    for row in &mut table.rows {
        let mut cells = Vec::new();
        cells.try_reserve_exact(nc).ok()?;
        cells.resize_with(nc, || Cell::new(""));
        row.cells = cells;
    }
    for OwnedAnchor { anchor, mut cell } in anchors {
        let end_r = anchor.r.checked_add(anchor.rs).filter(|end| *end <= nr)?;
        let end_c = anchor.c.checked_add(anchor.cs).filter(|end| *end <= nc)?;
        for r in anchor.r..end_r {
            for c in anchor.c..end_c {
                let covered = table.cell_mut(r, c)?;
                covered.h_merge = c > anchor.c;
                covered.v_merge = r > anchor.r;
            }
        }
        cell.grid_span = u32::try_from(anchor.cs).ok()?;
        cell.row_span = u32::try_from(anchor.rs).ok()?;
        cell.h_merge = false;
        cell.v_merge = false;
        *table.cell_mut(anchor.r, anchor.c)? = cell;
    }
    deckcraft_model::table::validate_merged_grid(table).ok()
}

pub fn draw_line(table: &mut Table, a: (f64, f64), b: (f64, f64)) -> bool {
    draw_line_with_budget(table, a, b, &mut FormulaBudget::default())
}
/// Share the operation's budget with its command/gesture transaction. False
/// leaves the source unchanged, including no-op lines that miss every owner.
pub fn draw_line_with_budget(table: &mut Table, a: (f64, f64), b: (f64, f64), budget: &mut FormulaBudget) -> bool {
    let Some(candidate) = projected_split(table, a, b, budget) else { return false };
    *table = candidate;
    true
}
fn projected_split(table: &Table, a: (f64, f64), b: (f64, f64), budget: &mut FormulaBudget) -> Option<Table> {
    if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
        return None;
    }
    preflight(table, budget)?;
    // Work directly on the chosen axis: transposing a formula table would change
    // its reference semantics before the model's checked rewrite could run.
    let columns = (b.0 - a.0).abs() < (b.1 - a.1).abs();
    let (position, low, high) =
        if columns { (a.0 / 2.0 + b.0 / 2.0, a.1.min(b.1), a.1.max(b.1)) } else { (a.1 / 2.0 + b.1 / 2.0, a.0.min(b.0), a.0.max(b.0)) };
    let axis = if columns { edges(table.cols.iter().copied())? } else { edges(table.rows.iter().map(|row| row.height))? };
    let cross = if columns { edges(table.rows.iter().map(|row| row.height))? } else { edges(table.cols.iter().copied())? };
    let total = *axis.last()?;
    if position <= SNAP || position >= total - SNAP || high - low < SNAP {
        return None;
    }
    let (boundary, inserted) = if let Some(index) = axis.iter().position(|edge| (*edge - position).abs() <= SNAP) {
        (index, None)
    } else {
        if axis.len().checked_sub(1)? >= MAX_GRID {
            return None;
        }
        let index = axis.windows(2).position(|pair| pair.first().is_some_and(|a| *a < position) && pair.get(1).is_some_and(|b| position < *b))?;
        (index.checked_add(1)?, Some(index))
    };
    let mut owners = anchors(table)?;
    if let Some(index) = inserted {
        for owner in &mut owners {
            let (start, span) = if columns { (&mut owner.c, &mut owner.cs) } else { (&mut owner.r, &mut owner.rs) };
            if *start > index {
                *start = start.checked_add(1)?;
            } else if start.checked_add(*span)? > index {
                *span = span.checked_add(1)?;
            }
        }
    }
    let crosses = |owner: &Anchor| -> Option<bool> {
        let (start, span, offset, extent) = if columns { (owner.c, owner.cs, owner.r, owner.rs) } else { (owner.r, owner.rs, owner.c, owner.cs) };
        let left = *cross.get(offset)?;
        let right = *cross.get(offset.checked_add(extent)?)?;
        let overlap = (right.min(high) - left.max(low)).max(0.0);
        Some(start < boundary && boundary < start.checked_add(span)? && overlap * 2.0 > right - left)
    };
    let mut split = Vec::new();
    split.try_reserve_exact(owners.len()).ok()?;
    for owner in &owners {
        split.push(crosses(owner)?);
    }
    if !split.iter().any(|split| *split) {
        return None;
    }
    let mut candidate = table.clone();
    if let Some(index) = inserted {
        budget.next_pass();
        candidate.edit_axis_with_budget(columns, boundary, false, budget).ok()?;
        let first = position - *axis.get(index)?;
        let second = *axis.get(boundary)? - position;
        if columns {
            *candidate.cols.get_mut(index)? = first;
            *candidate.cols.get_mut(boundary)? = second;
        } else {
            candidate.rows.get_mut(index)?.height = first;
            candidate.rows.get_mut(boundary)?.height = second;
        }
    }
    let mut output = Vec::new();
    output.try_reserve_exact(owners.len().checked_mul(2)?).ok()?;
    for (mut owner, split) in owners.into_iter().zip(split) {
        let cell = std::mem::take(candidate.cell_mut(owner.r, owner.c)?);
        if split {
            let mut lower = owner;
            if columns {
                lower.c = boundary;
                lower.cs = owner.c.checked_add(owner.cs)?.checked_sub(boundary)?;
                owner.cs = boundary.checked_sub(owner.c)?;
            } else {
                lower.r = boundary;
                lower.rs = owner.r.checked_add(owner.rs)?.checked_sub(boundary)?;
                owner.rs = boundary.checked_sub(owner.r)?;
            }
            output.push(OwnedAnchor { anchor: lower, cell: empty_like(&cell) });
        }
        output.push(OwnedAnchor { anchor: owner, cell });
    }
    rebuild(&mut candidate, output)?;
    budget.next_pass();
    candidate.recalculate_with_budget(budget).ok()?;
    candidate.validate_content().ok()?;
    budget.next_pass();
    validate_closure(&candidate, budget, 0)?;
    budget.check_limits().ok()?;
    Some(candidate)
}

pub fn erase_at(table: &mut Table, point: (f64, f64), tolerance: f64) -> bool {
    erase_at_with_budget(table, point, tolerance, &mut FormulaBudget::default())
}
pub fn erase_at_with_budget(table: &mut Table, point: (f64, f64), tolerance: f64, budget: &mut FormulaBudget) -> bool {
    erase_at_with_affine_budget(table, point, tolerance, Affine::IDENTITY, budget)
}
/// `point` remains intrinsic; tolerance and nearest-border ranking are measured
/// after `affine`, in the caller's slide units. Identity retains the pure API.
pub fn erase_at_with_affine_budget(table: &mut Table, point: (f64, f64), tolerance: f64, affine: Affine, budget: &mut FormulaBudget) -> bool {
    let Some(candidate) = projected_erase(table, point, tolerance, affine, budget) else { return false };
    *table = candidate;
    true
}
fn border_distance(point: Point, from: Point, to: Point) -> Option<f64> {
    let edge = to - from;
    let length = edge.x.hypot(edge.y);
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let unit = edge / length;
    let offset = point - from;
    let projection = offset.dot(unit);
    if !projection.is_finite() {
        return None;
    }
    let distance = (offset - unit * projection.clamp(0.0, length)).hypot();
    distance.is_finite().then_some(distance)
}
fn projected_erase(table: &Table, point: (f64, f64), tolerance: f64, affine: Affine, budget: &mut FormulaBudget) -> Option<Table> {
    if ![point.0, point.1, tolerance].iter().all(|v| v.is_finite()) || tolerance < 0.0 {
        return None;
    }
    let determinant = affine.determinant();
    if !affine.as_coeffs().iter().all(|v| v.is_finite()) || !determinant.is_finite() || determinant == 0.0 {
        return None;
    }
    let projected_point = affine * Point::new(point.0, point.1);
    preflight(table, budget)?;
    let xs = edges(table.cols.iter().copied())?;
    let ys = edges(table.rows.iter().map(|row| row.height))?;
    let owners = anchors(table)?;
    let mut best = None;
    for (i, owner) in owners.iter().enumerate() {
        for neighbour in [(owner.r, owner.c.checked_add(owner.cs)?), (owner.r.checked_add(owner.rs)?, owner.c)] {
            let Ok(j) = owners.binary_search_by_key(&neighbour, |owner| (owner.r, owner.c)) else { continue };
            let next = owners.get(j)?;
            let (left, top, right, bottom) =
                (*xs.get(owner.c)?, *ys.get(owner.r)?, *xs.get(owner.c.checked_add(owner.cs)?)?, *ys.get(owner.r.checked_add(owner.rs)?)?);
            // Retain the literal eraser's strict intrinsic endpoint criterion;
            // measure the finite candidate segment in slide space, including
            // nonuniform scaling/shear, before applying tolerance or ranking.
            let (from, to) =
                if next.c == owner.c.checked_add(owner.cs)? && next.r == owner.r && next.rs == owner.rs && point.1 > top && point.1 < bottom {
                    (Point::new(right, top), Point::new(right, bottom))
                } else if next.r == owner.r.checked_add(owner.rs)? && next.c == owner.c && next.cs == owner.cs && point.0 > left && point.0 < right {
                    (Point::new(left, bottom), Point::new(right, bottom))
                } else {
                    continue;
                };
            let distance = border_distance(projected_point, affine * from, affine * to)?;
            if distance <= tolerance && best.is_none_or(|(old, _, _)| distance < old) {
                best = Some((distance, i, j));
            }
        }
    }
    let (_, i, j) = best?;
    let first = owners.get(i)?;
    let second = owners.get(j)?;
    let to = (second.r.checked_add(second.rs)?.checked_sub(1)?, second.c.checked_add(second.cs)?.checked_sub(1)?);
    let mut candidate = table.clone();
    budget.next_pass();
    candidate.merge_with_budget((first.r, first.c), to, budget).ok()?;
    candidate.validate_content().ok()?;
    budget.next_pass();
    validate_closure(&candidate, budget, 0)?;
    budget.check_limits().ok()?;
    Some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(t: &Table) -> String {
        t.rows
            .iter()
            .map(|r| {
                r.cells
                    .iter()
                    .map(|c| {
                        if c.is_covered() {
                            "."
                        } else if c.grid_span > 1 || c.row_span > 1 {
                            "M"
                        } else {
                            "o"
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    #[test]
    fn pencil_splits_only_the_cells_it_crosses() {
        let mut t = Table::new(1, 1, 300.0, 100.0);
        assert!(draw_line(&mut t, (0.0, 40.0), (300.0, 40.0)), "full-width line adds a row");
        assert_eq!((grid(&t), t.rows[0].height, t.rows[1].height), ("o/o".into(), 40.0, 60.0));
        assert!(draw_line(&mut t, (120.0, 40.0), (120.0, 100.0)), "a vertical line in the bottom row only");
        assert_eq!(grid(&t), "M./oo");
        assert_eq!(t.cols, vec![120.0, 180.0]);
        assert!(draw_line(&mut t, (120.0, 0.0), (120.0, 40.0)), "on the existing grid line: splits the merged top cell");
        assert_eq!(grid(&t), "oo/oo");
        assert!(!draw_line(&mut t, (0.0, 1.0), (300.0, 1.0)), "on the outer border");
        assert!(!draw_line(&mut t, (f64::NAN, 1.0), (300.0, 1.0)));
        assert_eq!(t.width(), 300.0);
        assert_eq!(t.height(), 100.0);
    }

    #[test]
    fn eraser_merges_neighbours_and_keeps_text() {
        let mut t = Table::new(2, 2, 200.0, 50.0);
        t.cell_mut(0, 0).unwrap().text = Cell::new("A").text;
        t.cell_mut(0, 1).unwrap().text = Cell::new("B").text;
        assert!(erase_at(&mut t, (100.0, 25.0), 5.0), "the vertical border in row 0");
        assert_eq!(grid(&t), "M./oo");
        assert_eq!(t.cell(0, 0).unwrap().text.paragraphs.len(), 2);
        assert!(!erase_at(&mut t, (50.0, 75.0), 5.0), "nowhere near a border");
        let mut t = Table::new(2, 2, 200.0, 50.0);
        t.merge(0, 0, 0, 1);
        assert!(!erase_at(&mut t, (50.0, 50.0), 3.0), "the merged top cell and one bottom cell aren't a rectangle");
        assert!(erase_at(&mut t, (100.0, 75.0), 3.0));
        assert!(erase_at(&mut t, (50.0, 50.0), 3.0));
        assert_eq!(grid(&t), "M./..");
    }

    #[test]
    fn grid_is_capped() {
        let mut t = Table::new(1, 1, 300.0, 4000.0);
        let mut n = 0;
        for i in 1..400 {
            if draw_line(&mut t, (0.0, i as f64 * 9.9), (300.0, i as f64 * 9.9)) {
                n += 1;
            }
        }
        assert!(t.rows.len() <= MAX_GRID && n > 100, "{} rows", t.rows.len());
    }
}
