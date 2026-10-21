//! Tables (graphic frames with `a:tbl`).

use serde::{Deserialize, Serialize};

use crate::style::{ColorRef, Fill, Line};
use crate::text::{Anchor, TextBody};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Cell {
    pub text: TextBody,
    pub fill: Option<Fill>,
    /// Left, right, top, bottom borders; `None` = from the table style.
    pub borders: [Option<Line>; 4],
    pub diag_down: Option<Line>,
    pub diag_up: Option<Line>,
    /// Margins (l, r, t, b) in points; `None` = defaults (7.2, 7.2, 3.6, 3.6).
    pub margins: Option<[f64; 4]>,
    pub anchor: Option<Anchor>,
    pub grid_span: u32,
    pub row_span: u32,
    /// Covered by a merge to the left / above.
    pub h_merge: bool,
    pub v_merge: bool,
    pub vertical: Option<crate::text::TextDir>,
}

impl Cell {
    pub fn new(text: &str) -> Self {
        Cell { text: TextBody::from_text(text), grid_span: 1, row_span: 1, ..Default::default() }
    }
    pub fn is_covered(&self) -> bool {
        self.h_merge || self.v_merge
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Row {
    pub height: f64,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Table {
    /// Column widths in points.
    pub cols: Vec<f64>,
    pub rows: Vec<Row>,
    /// Table style id (our built-in style names or a PPTX style GUID).
    pub style: String,
    pub first_row: bool,
    pub first_col: bool,
    pub last_row: bool,
    pub last_col: bool,
    pub band_row: bool,
    pub band_col: bool,
    /// `a:tblPr` fill: a fill behind the whole table (over the style's own background).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Fill>,
}

impl Default for Table {
    fn default() -> Self {
        Table {
            cols: vec![],
            rows: vec![],
            style: "medium2-accent1".into(),
            first_row: true,
            first_col: false,
            last_row: false,
            last_col: false,
            band_row: true,
            band_col: false,
            background: None,
        }
    }
}

impl Table {
    pub fn new(rows: usize, cols: usize, width: f64, row_height: f64) -> Self {
        let rows = rows.clamp(1, 1000);
        let cols = cols.clamp(1, 1000);
        let cw = if cols > 0 { width / cols as f64 } else { width };
        Table {
            cols: vec![cw; cols],
            rows: (0..rows).map(|_| Row { height: row_height, cells: (0..cols).map(|_| Cell::new("")).collect() }).collect(),
            ..Default::default()
        }
    }
    pub fn n_rows(&self) -> usize {
        self.rows.len()
    }
    pub fn n_cols(&self) -> usize {
        self.cols.len()
    }
    pub fn cell(&self, r: usize, c: usize) -> Option<&Cell> {
        self.rows.get(r).and_then(|row| row.cells.get(c))
    }
    pub fn cell_mut(&mut self, r: usize, c: usize) -> Option<&mut Cell> {
        self.rows.get_mut(r).and_then(|row| row.cells.get_mut(c))
    }
    pub fn width(&self) -> f64 {
        self.cols.iter().sum()
    }
    pub fn height(&self) -> f64 {
        self.rows.iter().map(|r| r.height).sum()
    }
    /// Insert an empty row before `at`. A merge the new row falls inside grows to cover it.
    pub fn insert_row(&mut self, at: usize) {
        let at = at.min(self.rows.len());
        let merges = self.merges();
        let h = self.rows.get(at.saturating_sub(1)).map(|r| r.height).unwrap_or(30.0);
        self.rows.insert(at, Row { height: h, cells: (0..self.cols.len()).map(|_| Cell::new("")).collect() });
        self.set_merges(merges.into_iter().map(|m| m.insert_row(at)));
    }
    /// Insert an empty column before `at`. A merge the new column falls inside grows to cover it.
    pub fn insert_col(&mut self, at: usize) {
        let at = at.min(self.cols.len());
        let merges = self.merges();
        let w = self.cols.get(at.saturating_sub(1)).copied().unwrap_or(80.0);
        self.cols.insert(at, w);
        for r in &mut self.rows {
            let i = at.min(r.cells.len());
            r.cells.insert(i, Cell::new(""));
        }
        self.set_merges(merges.into_iter().map(|m| m.insert_col(at)));
    }
    /// Delete row `at`. Merges through it shrink; a merge anchored on it moves its anchor (and
    /// text) to the next row it covers.
    pub fn delete_row(&mut self, at: usize) -> bool {
        if self.rows.len() <= 1 || at >= self.rows.len() {
            return false;
        }
        let merges = self.merges();
        for m in &merges {
            if m.r == at && m.rs > 1 {
                self.move_anchor((m.r, m.c), (m.r + 1, m.c));
            }
        }
        self.rows.remove(at);
        self.set_merges(merges.into_iter().filter_map(|m| m.delete_row(at)));
        true
    }
    /// Delete column `at`. Merges through it shrink; a merge anchored on it moves its anchor (and
    /// text) to the next column it covers.
    pub fn delete_col(&mut self, at: usize) -> bool {
        if self.cols.len() <= 1 || at >= self.cols.len() {
            return false;
        }
        let merges = self.merges();
        for m in &merges {
            if m.c == at && m.gs > 1 {
                self.move_anchor((m.r, m.c), (m.r, m.c + 1));
            }
        }
        self.cols.remove(at);
        for r in &mut self.rows {
            if at < r.cells.len() {
                r.cells.remove(at);
            }
        }
        self.set_merges(merges.into_iter().filter_map(|m| m.delete_col(at)));
        true
    }
    /// The merged areas: every uncovered cell spanning more than one cell, clipped to the grid.
    fn merges(&self) -> Vec<Merge> {
        let (nr, nc) = (self.rows.len(), self.cols.len());
        let mut out = vec![];
        for (r, row) in self.rows.iter().enumerate() {
            for (c, cell) in row.cells.iter().enumerate().take(nc) {
                if cell.is_covered() {
                    continue;
                }
                let rs = (cell.row_span.max(1) as usize).min(nr - r);
                let gs = (cell.grid_span.max(1) as usize).min(nc - c);
                if rs > 1 || gs > 1 {
                    out.push(Merge { r, c, rs, gs });
                }
            }
        }
        out
    }
    /// Rebuild every cell's spans and merge flags from `merges`.
    fn set_merges(&mut self, merges: impl IntoIterator<Item = Merge>) {
        for row in &mut self.rows {
            for cell in &mut row.cells {
                cell.grid_span = 1;
                cell.row_span = 1;
                cell.h_merge = false;
                cell.v_merge = false;
            }
        }
        for m in merges {
            if m.rs <= 1 && m.gs <= 1 {
                continue;
            }
            for r in m.r..m.r.saturating_add(m.rs) {
                for c in m.c..m.c.saturating_add(m.gs) {
                    if let Some(cell) = self.cell_mut(r, c) {
                        cell.h_merge = c > m.c;
                        cell.v_merge = r > m.r;
                    }
                }
            }
            if let Some(cell) = self.cell_mut(m.r, m.c) {
                cell.grid_span = u32::try_from(m.gs).unwrap_or(u32::MAX);
                cell.row_span = u32::try_from(m.rs).unwrap_or(u32::MAX);
            }
        }
    }
    /// Swap the anchor cell's contents into the cell that takes over as anchor.
    fn move_anchor(&mut self, from: (usize, usize), to: (usize, usize)) {
        let Some(a) = self.cell(from.0, from.1).cloned() else { return };
        let Some(b) = self.cell(to.0, to.1).cloned() else { return };
        if let Some(x) = self.cell_mut(to.0, to.1) {
            *x = a;
        }
        if let Some(x) = self.cell_mut(from.0, from.1) {
            *x = b;
        }
    }
    /// Merge the rectangle (r0,c0)–(r1,c1) inclusive into its top-left cell.
    pub fn merge(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) -> bool {
        let (r0, r1) = (r0.min(r1), r0.max(r1));
        let (c0, c1) = (c0.min(c1), c0.max(c1));
        if r1 >= self.rows.len() || c1 >= self.cols.len() || (r0 == r1 && c0 == c1) {
            return false;
        }
        // Grow the rectangle until no existing merge sticks out of it.
        let (mut r0, mut c0, mut r1, mut c1) = (r0, c0, r1, c1);
        loop {
            let before = (r0, c0, r1, c1);
            for (r, row) in self.rows.iter().enumerate() {
                for (c, cell) in row.cells.iter().enumerate().take(self.cols.len()) {
                    if cell.is_covered() {
                        continue;
                    }
                    let ar1 = r.saturating_add((cell.row_span.max(1) as usize).saturating_sub(1)).min(self.rows.len() - 1);
                    let ac1 = c.saturating_add((cell.grid_span.max(1) as usize).saturating_sub(1)).min(self.cols.len() - 1);
                    if r <= r1 && ar1 >= r0 && c <= c1 && ac1 >= c0 {
                        r0 = r0.min(r);
                        c0 = c0.min(c);
                        r1 = r1.max(ar1);
                        c1 = c1.max(ac1);
                    }
                }
            }
            if (r0, c0, r1, c1) == before {
                break;
            }
        }
        let mut text = vec![];
        for r in r0..=r1 {
            for c in c0..=c1 {
                if let Some(cell) = self.cell_mut(r, c) {
                    if !cell.text.is_empty() {
                        text.append(&mut cell.text.paragraphs);
                    }
                    cell.h_merge = c > c0;
                    cell.v_merge = r > r0;
                    cell.grid_span = 1;
                    cell.row_span = 1;
                }
            }
        }
        if let Some(cell) = self.cell_mut(r0, c0) {
            cell.grid_span = (c1 - c0 + 1) as u32;
            cell.row_span = (r1 - r0 + 1) as u32;
            cell.h_merge = false;
            cell.v_merge = false;
            if !text.is_empty() {
                cell.text.paragraphs = text;
            }
        }
        true
    }
    /// Undo a merge at (r, c).
    pub fn split(&mut self, r: usize, c: usize) -> bool {
        let Some(cell) = self.cell(r, c) else { return false };
        let (gs, rs) = (cell.grid_span.max(1) as usize, cell.row_span.max(1) as usize);
        if gs == 1 && rs == 1 {
            return false;
        }
        // Spans come from the file and may reach past the table; only walk the cells that exist.
        for rr in r..r.saturating_add(rs).min(self.rows.len()) {
            for cc in c..c.saturating_add(gs).min(self.cols.len()) {
                if let Some(x) = self.cell_mut(rr, cc) {
                    x.h_merge = false;
                    x.v_merge = false;
                    x.grid_span = 1;
                    x.row_span = 1;
                }
            }
        }
        true
    }
    /// Make every row have exactly `cols.len()` cells (repair after loading).
    pub fn normalize(&mut self) {
        let n = self.cols.len();
        for r in &mut self.rows {
            r.cells.resize_with(n, || Cell::new(""));
            for c in &mut r.cells {
                c.grid_span = c.grid_span.max(1);
                c.row_span = c.row_span.max(1);
            }
        }
    }
}

/// A merged area: anchor (r, c) spanning `rs` rows and `gs` columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Merge {
    r: usize,
    c: usize,
    rs: usize,
    gs: usize,
}

impl Merge {
    fn insert_row(self, at: usize) -> Self {
        if at <= self.r {
            Merge { r: self.r + 1, ..self }
        } else if at < self.r + self.rs {
            Merge { rs: self.rs + 1, ..self }
        } else {
            self
        }
    }
    fn insert_col(self, at: usize) -> Self {
        if at <= self.c {
            Merge { c: self.c + 1, ..self }
        } else if at < self.c + self.gs {
            Merge { gs: self.gs + 1, ..self }
        } else {
            self
        }
    }
    fn delete_row(self, at: usize) -> Option<Self> {
        let m = if at < self.r {
            Merge { r: self.r - 1, ..self }
        } else if at < self.r + self.rs {
            Merge { rs: self.rs - 1, ..self }
        } else {
            self
        };
        // A merge that lost its only row or column is gone, not a 0-span merge over its neighbours.
        (m.rs >= 1 && m.gs >= 1 && (m.rs > 1 || m.gs > 1)).then_some(m)
    }
    fn delete_col(self, at: usize) -> Option<Self> {
        let m = if at < self.c {
            Merge { c: self.c - 1, ..self }
        } else if at < self.c + self.gs {
            Merge { gs: self.gs - 1, ..self }
        } else {
            self
        };
        // A merge that lost its only row or column is gone, not a 0-span merge over its neighbours.
        (m.rs >= 1 && m.gs >= 1 && (m.rs > 1 || m.gs > 1)).then_some(m)
    }
}

/// A table style defined in the file (`ppt/tableStyles.xml`), as PowerPoint writes the built-in
/// styles a deck uses: how each part of a table (whole table, banded rows and columns, first and
/// last rows and columns, corner cells) is filled, bordered and lettered.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TableStyleDef {
    /// The style's GUID (`a:tableStyleId` on tables).
    pub id: String,
    pub name: String,
    pub parts: Vec<(TablePart, TablePartStyle)>,
    /// `a:tblBg`: a fill behind the whole table (only `fill` and `fill_ref` are used).
    pub background: Option<TablePartStyle>,
    /// The definition as read, written back unchanged.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub raw: String,
}

impl TableStyleDef {
    pub fn part(&self, p: TablePart) -> Option<&TablePartStyle> {
        self.parts.iter().find(|(k, _)| *k == p).map(|(_, s)| s)
    }
}

/// The parts of a table style, in the order they apply (later ones win).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TablePart {
    WholeTbl,
    Band1H,
    Band2H,
    Band1V,
    Band2V,
    LastCol,
    FirstCol,
    LastRow,
    FirstRow,
    SeCell,
    SwCell,
    NeCell,
    NwCell,
}

impl TablePart {
    pub const ALL: [TablePart; 13] = [
        TablePart::WholeTbl,
        TablePart::Band1H,
        TablePart::Band2H,
        TablePart::Band1V,
        TablePart::Band2V,
        TablePart::LastCol,
        TablePart::FirstCol,
        TablePart::LastRow,
        TablePart::FirstRow,
        TablePart::SeCell,
        TablePart::SwCell,
        TablePart::NeCell,
        TablePart::NwCell,
    ];

    pub fn xml(self) -> &'static str {
        match self {
            TablePart::WholeTbl => "wholeTbl",
            TablePart::Band1H => "band1H",
            TablePart::Band2H => "band2H",
            TablePart::Band1V => "band1V",
            TablePart::Band2V => "band2V",
            TablePart::LastCol => "lastCol",
            TablePart::FirstCol => "firstCol",
            TablePart::LastRow => "lastRow",
            TablePart::FirstRow => "firstRow",
            TablePart::SeCell => "seCell",
            TablePart::SwCell => "swCell",
            TablePart::NeCell => "neCell",
            TablePart::NwCell => "nwCell",
        }
    }

    pub fn from_xml(s: &str) -> Option<TablePart> {
        TablePart::ALL.into_iter().find(|p| p.xml() == s)
    }
}

/// One part of a table style. A theme reference (`a:fillRef`, `a:lnRef`) is kept as the theme's
/// style index with its colour.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TablePartStyle {
    pub fill: Option<Fill>,
    pub fill_ref: Option<(u32, ColorRef)>,
    /// Left, right, top, bottom, inside horizontal, inside vertical.
    pub borders: [Option<TableBorder>; 6],
    pub text_color: Option<ColorRef>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
}

/// A border of a table style part: a line, or a theme line style reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TableBorder {
    Line { line: Line },
    Ref { idx: u32, color: ColorRef },
}

/// Built-in table styles: (id, label). Styles are generated from the theme accents.
pub fn table_styles() -> Vec<(String, String)> {
    let mut v = vec![("none".to_string(), "No Style, No Grid".to_string()), ("grid".to_string(), "No Style, Table Grid".to_string())];
    for (kind, label) in [
        ("light1", "Light Style 1"),
        ("light2", "Light Style 2"),
        ("medium2", "Medium Style 2"),
        ("medium4", "Medium Style 4"),
        ("dark1", "Dark Style 1"),
    ] {
        v.push((format!("{kind}-tx1"), label.to_string()));
        for a in 1..=6 {
            v.push((format!("{kind}-accent{a}"), format!("{label} - Accent {a}")));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cell is owned by exactly one anchor (itself or a span over it), and no span leaves the grid.
    fn assert_topology(t: &Table) {
        let (nr, nc) = (t.n_rows(), t.n_cols());
        let mut owner = vec![vec![0u32; nc]; nr];
        for (r, row) in t.rows.iter().enumerate() {
            assert_eq!(row.cells.len(), nc);
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.is_covered() {
                    assert_eq!((cell.grid_span, cell.row_span), (1, 1), "covered ({r},{c}) has spans");
                    continue;
                }
                let (rs, gs) = (cell.row_span as usize, cell.grid_span as usize);
                assert!(r + rs <= nr && c + gs <= nc, "span at ({r},{c}) leaves the grid");
                for o in owner.iter_mut().skip(r).take(rs) {
                    for x in o.iter_mut().skip(c).take(gs) {
                        *x += 1;
                    }
                }
            }
        }
        for (r, o) in owner.iter().enumerate() {
            for (c, n) in o.iter().enumerate() {
                assert_eq!(*n, 1, "cell ({r},{c}) owned {n} times");
            }
        }
    }

    fn merged() -> Table {
        // 4x4 with a 2x2 merge at (1,1) holding "M".
        let mut t = Table::new(4, 4, 400.0, 30.0);
        if let Some(c) = t.cell_mut(1, 1) {
            c.text = TextBody::from_text("M");
        }
        assert!(t.merge(1, 1, 2, 2));
        assert_topology(&t);
        t
    }

    fn anchor_text(t: &Table, r: usize, c: usize) -> String {
        t.cell(r, c).unwrap().text.text()
    }

    #[test]
    fn merging_over_an_existing_merge_takes_it_in() {
        let mut t = Table::new(3, 3, 300.0, 30.0);
        if let Some(c) = t.cell_mut(1, 2) {
            c.text = TextBody::from_text("X");
        }
        assert!(t.merge(1, 1, 1, 2));
        assert!(t.merge(0, 0, 1, 1));
        assert_topology(&t);
        let a = t.cell(0, 0).unwrap();
        assert_eq!((a.row_span, a.grid_span), (2, 3));
        assert_eq!(anchor_text(&t, 0, 0), "X");
    }

    #[test]
    fn row_and_column_edits_keep_merges_consistent() {
        for at in 0..=4 {
            let mut t = merged();
            t.insert_row(at);
            assert_topology(&t);
            let mut t = merged();
            t.insert_col(at);
            assert_topology(&t);
        }
        for at in 0..4 {
            let mut t = merged();
            assert!(t.delete_row(at));
            assert_topology(&t);
            let mut t = merged();
            assert!(t.delete_col(at));
            assert_topology(&t);
        }
    }

    #[test]
    fn inserting_inside_a_merge_grows_it() {
        let mut t = merged();
        t.insert_row(2);
        let a = t.cell(1, 1).unwrap();
        assert_eq!((a.row_span, a.grid_span), (3, 2));
        assert!(t.cell(2, 2).unwrap().h_merge && t.cell(2, 2).unwrap().v_merge);
        t.insert_col(0);
        assert_eq!(t.cell(1, 2).unwrap().grid_span, 2);
        assert_eq!(anchor_text(&t, 1, 2), "M");
        assert_topology(&t);
    }

    #[test]
    fn deleting_the_anchor_row_or_column_keeps_the_text() {
        let mut t = merged();
        assert!(t.delete_row(1));
        let a = t.cell(1, 1).unwrap();
        assert!(!a.is_covered());
        assert_eq!((a.row_span, a.grid_span), (1, 2));
        assert_eq!(anchor_text(&t, 1, 1), "M");
        assert!(t.delete_col(1));
        let a = t.cell(1, 1).unwrap();
        assert!(!a.is_covered());
        assert_eq!((a.row_span, a.grid_span), (1, 1));
        assert_eq!(anchor_text(&t, 1, 1), "M");
        assert_topology(&t);
    }

    #[test]
    fn deleting_the_only_row_or_column_of_a_merge_removes_it() {
        // A 1x3 merge across a row: deleting that row leaves no merge behind.
        let mut t = Table::new(3, 3, 300.0, 30.0);
        assert!(t.merge(1, 0, 1, 2));
        assert!(t.delete_row(1));
        assert!((0..t.n_rows()).all(|r| (0..t.n_cols()).all(|c| t.cell(r, c).is_some_and(|x| !x.is_covered() && x.grid_span <= 1))));
        assert_topology(&t);
        // A 3x1 merge down a column: deleting that column leaves no merge behind.
        let mut t = Table::new(3, 3, 300.0, 30.0);
        assert!(t.merge(0, 1, 2, 1));
        assert!(t.delete_col(1));
        assert!((0..t.n_rows()).all(|r| (0..t.n_cols()).all(|c| t.cell(r, c).is_some_and(|x| !x.is_covered() && x.row_span <= 1))));
        assert_topology(&t);
    }

    #[test]
    fn insert_delete_merge_split() {
        let mut t = Table::new(3, 3, 300.0, 30.0);
        assert_eq!((t.n_rows(), t.n_cols()), (3, 3));
        t.insert_row(1);
        t.insert_col(3);
        assert_eq!((t.n_rows(), t.n_cols()), (4, 4));
        assert!(t.delete_row(0) && t.delete_col(0));
        assert_eq!((t.n_rows(), t.n_cols()), (3, 3));
        assert!(t.merge(0, 0, 1, 1));
        assert_eq!(t.cell(0, 0).map(|c| (c.grid_span, c.row_span)), Some((2, 2)));
        assert!(t.cell(1, 1).is_some_and(|c| c.is_covered()));
        assert!(t.split(0, 0));
        assert!(!t.cell(1, 1).is_some_and(|c| c.is_covered()));
        assert!(!t.merge(5, 5, 6, 6));
        assert!(!t.delete_row(99));
        let mut one = Table::new(1, 1, 10.0, 10.0);
        assert!(!one.delete_row(0) && !one.delete_col(0));
        assert!(table_styles().len() > 30);
    }

    #[test]
    fn split_ignores_spans_past_the_table() {
        let mut t = Table::new(2, 2, 100.0, 10.0);
        if let Some(c) = t.cell_mut(0, 0) {
            c.grid_span = u32::MAX;
            c.row_span = u32::MAX;
        }
        assert!(t.split(0, 0));
        assert_eq!(t.cell(0, 0).map(|c| (c.grid_span, c.row_span)), Some((1, 1)));
    }
}
