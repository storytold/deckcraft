//! Draw Table: the pencil splits cells along the line you draw, the table eraser merges the two
//! cells on either side of the border you click. Pure operations on a [`Table`] in its own
//! coordinates (points from its top-left corner), unit-tested without a session.
//!
//! Both work on the table's *anchors* (each visible cell with its row and column span), edit
//! that list, then lay the grid back out, so merges anywhere in the table survive.

use deckcraft_model::table::{Cell, Table};

/// A line closer than this to an existing grid line reuses it rather than adding a sliver.
const SNAP: f64 = 4.0;
/// Cap on the grid so a runaway pencil can't build an enormous table.
const MAX_GRID: usize = 200;

#[derive(Clone, Debug)]
struct Anchor {
    r: usize,
    c: usize,
    rs: usize,
    cs: usize,
    cell: Cell,
}

fn anchors(t: &Table) -> Vec<Anchor> {
    let mut v = vec![];
    for (r, row) in t.rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().enumerate() {
            if !cell.is_covered() {
                let rs = (cell.row_span.max(1) as usize).min(t.rows.len().saturating_sub(r)).max(1);
                let cs = (cell.grid_span.max(1) as usize).min(t.cols.len().saturating_sub(c)).max(1);
                v.push(Anchor { r, c, rs, cs, cell: cell.clone() });
            }
        }
    }
    v
}

/// Lay `anchors` out into `t`'s grid (its rows and columns already sized).
fn rebuild(t: &mut Table, anchors: Vec<Anchor>) {
    let (nr, nc) = (t.rows.len(), t.cols.len());
    for row in &mut t.rows {
        row.cells = (0..nc).map(|_| Cell::new("")).collect();
    }
    for a in anchors {
        for r in a.r..(a.r + a.rs).min(nr) {
            for c in a.c..(a.c + a.cs).min(nc) {
                if let Some(x) = t.cell_mut(r, c) {
                    x.h_merge = c > a.c;
                    x.v_merge = r > a.r;
                }
            }
        }
        if let Some(x) = t.cell_mut(a.r, a.c) {
            *x = Cell { grid_span: a.cs as u32, row_span: a.rs as u32, h_merge: false, v_merge: false, ..a.cell };
        }
    }
    t.normalize();
}

/// Swap rows and columns, so the vertical case reuses the horizontal one.
fn transpose(t: &mut Table) {
    let (nr, nc) = (t.rows.len(), t.cols.len());
    let heights: Vec<f64> = t.rows.iter().map(|r| r.height).collect();
    let mut a = anchors(t);
    for x in &mut a {
        std::mem::swap(&mut x.r, &mut x.c);
        std::mem::swap(&mut x.rs, &mut x.cs);
        let [l, r, tp, b] = std::mem::take(&mut x.cell.borders);
        x.cell.borders = [tp, b, l, r];
    }
    let widths = std::mem::take(&mut t.cols);
    t.cols = heights;
    t.rows = widths.into_iter().map(|h| deckcraft_model::table::Row { height: h, cells: vec![] }).collect();
    debug_assert_eq!((t.rows.len(), t.cols.len()), (nc, nr));
    rebuild(t, a);
}

fn edges(sizes: impl Iterator<Item = f64>) -> Vec<f64> {
    let mut e = vec![0.0];
    let mut acc = 0.0;
    for s in sizes {
        acc += s;
        e.push(acc);
    }
    e
}

/// Draw a line from `a` to `b` (table points). Mostly-horizontal lines split rows, mostly-vertical
/// ones split columns; only the cells the line crosses are divided. Returns whether anything
/// changed.
pub fn draw_line(t: &mut Table, a: (f64, f64), b: (f64, f64)) -> bool {
    if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) || t.rows.is_empty() || t.cols.is_empty() {
        return false;
    }
    if (b.0 - a.0).abs() >= (b.1 - a.1).abs() {
        split_rows(t, (a.1 + b.1) / 2.0, a.0.min(b.0), a.0.max(b.0))
    } else {
        transpose(t);
        let changed = split_rows(t, (a.0 + b.0) / 2.0, a.1.min(b.1), a.1.max(b.1));
        transpose(t);
        changed
    }
}

/// A horizontal line at `y` from `x0` to `x1`.
fn split_rows(t: &mut Table, y: f64, x0: f64, x1: f64) -> bool {
    let ys = edges(t.rows.iter().map(|r| r.height));
    let xs = edges(t.cols.iter().copied());
    let total = ys.last().copied().unwrap_or(0.0);
    if y <= SNAP || y >= total - SNAP || x1 - x0 < SNAP {
        return false;
    }
    let mut a = anchors(t);
    // Boundary index b: an existing grid line near y, or a new one splitting row k.
    let b = match ys.iter().position(|e| (e - y).abs() <= SNAP) {
        Some(b) => b,
        None => {
            if t.rows.len() >= MAX_GRID {
                return false;
            }
            let Some(k) = ys.windows(2).position(|w| w[0] < y && y < w[1]) else { return false };
            let top = ys[k];
            let (h0, h1) = (y - top, ys[k + 1] - y);
            if let Some(r) = t.rows.get_mut(k) {
                r.height = h0;
            }
            t.rows.insert(k + 1, deckcraft_model::table::Row { height: h1, cells: vec![] });
            for x in &mut a {
                if x.r > k {
                    x.r += 1;
                } else if x.r + x.rs > k {
                    x.rs += 1;
                }
            }
            k + 1
        }
    };
    let mut changed = false;
    let mut out = Vec::with_capacity(a.len() + 4);
    for x in a {
        let (l, r) = (xs.get(x.c).copied().unwrap_or(0.0), xs.get(x.c + x.cs).copied().unwrap_or(0.0));
        // The line divides a cell it crosses for more than half of the cell's width.
        let overlap = (r.min(x1) - l.max(x0)).max(0.0);
        if x.r < b && b < x.r + x.rs && overlap * 2.0 > r - l {
            let below =
                Anchor { r: b, c: x.c, rs: x.r + x.rs - b, cs: x.cs, cell: Cell { text: deckcraft_model::TextBody::default(), ..x.cell.clone() } };
            out.push(Anchor { rs: b - x.r, ..x });
            out.push(below);
            changed = true;
        } else {
            out.push(x);
        }
    }
    rebuild(t, out);
    changed
}

/// Erase the border nearest `p` (within `tol` points): the two cells it separates merge, when
/// together they make a rectangle. Returns whether a border was erased.
pub fn erase_at(t: &mut Table, p: (f64, f64), tol: f64) -> bool {
    if !(p.0.is_finite() && p.1.is_finite()) {
        return false;
    }
    let ys = edges(t.rows.iter().map(|r| r.height));
    let xs = edges(t.cols.iter().copied());
    let a = anchors(t);
    let rect = |x: &Anchor| (xs[x.c], ys[x.r], xs[x.c + x.cs], ys[x.r + x.rs]);
    let mut best: Option<(f64, usize, usize)> = None;
    for (i, u) in a.iter().enumerate() {
        for (j, v) in a.iter().enumerate() {
            let (ul, ut, ur, ub) = rect(u);
            // v directly right of u with the same rows, or directly below with the same columns.
            let d = if v.c == u.c + u.cs && v.r == u.r && v.rs == u.rs && p.1 > ut && p.1 < ub {
                (p.0 - ur).abs()
            } else if v.r == u.r + u.rs && v.c == u.c && v.cs == u.cs && p.0 > ul && p.0 < ur {
                (p.1 - ub).abs()
            } else {
                continue;
            };
            if d <= tol && best.is_none_or(|b| d < b.0) {
                best = Some((d, i, j));
            }
        }
    }
    let Some((_, i, j)) = best else { return false };
    let mut a = a;
    let v = a[j].clone();
    let u = &mut a[i];
    if v.r == u.r {
        u.cs += v.cs;
    } else {
        u.rs += v.rs;
    }
    let mut more = v.cell.text.paragraphs.clone();
    if !v.cell.text.is_empty() {
        if u.cell.text.is_empty() {
            u.cell.text.paragraphs = more;
        } else {
            u.cell.text.paragraphs.append(&mut more);
        }
    }
    a.remove(j);
    rebuild(t, a);
    true
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
