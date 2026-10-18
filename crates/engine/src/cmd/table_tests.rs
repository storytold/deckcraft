//! Tests for the table commands that had no test: the four that add or remove a row or a column,
//! the two that distribute rows and columns, `split`, the style options, and the two border tools.
//! Fills, borders, cell margins, cell alignment, column width, row height, styles and cell
//! selection are covered in `tests.rs`.

use deckcraft_model::ShapeId;
use deckcraft_model::ShapeKind;
use deckcraft_model::table::Table;
use serde_json::{Value, json};

use crate::Session;

/// A table of `rows` × `cols` holding `data` row by row, its top-left at (100, 100).
fn table(s: &mut Session, rows: usize, cols: usize, w: f64, h: f64, data: Value) -> u32 {
    s.execute("insert.table", &json!({"rows": rows, "cols": cols, "rect": [100.0, 100.0, w, h], "data": data})).expect("insert a table")["id"]
        .as_u64()
        .expect("table id") as u32
}

/// The table as it stands now, whatever the command did to it.
fn grid(s: &Session, id: u32) -> Table {
    let st = s.doc().expect("a document");
    let sh = st.shape(ShapeId(id)).expect("the table");
    let ShapeKind::Table(t) = &sh.kind else { panic!("not a table") };
    t.clone()
}

fn cell(t: &Table, r: usize, c: usize) -> String {
    t.cell(r, c).map(|x| x.text.text()).unwrap_or_default()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn three_by_two(s: &mut Session) -> u32 {
    table(s, 3, 2, 400.0, 120.0, json!([["a", "b"], ["c", "d"], ["e", "f"]]))
}

// ---------------------------------------------------------------------------------------------
// Adding and removing rows and columns
// ---------------------------------------------------------------------------------------------

/// The row is inserted empty and the rows after it move down: the text of the table is what the
/// user sees, so it has to stay with its own row.
#[test]
fn a_row_inserted_above_is_empty_and_pushes_the_rest_down() {
    let mut s = Session::with_new();
    let id = three_by_two(&mut s);
    let before = s.doc().unwrap().history.undo.len();

    s.execute("table.insertRowAbove", &json!({"id": id, "row": 1})).expect("insert a row");
    assert_eq!(s.doc().unwrap().history.undo.len(), before + 1, "one undo step");
    let t = grid(&s, id);
    assert_eq!(t.n_rows(), 4);
    assert_eq!(cell(&t, 0, 0), "a");
    assert_eq!(cell(&t, 1, 0), "", "the new row is empty");
    assert_eq!(cell(&t, 2, 0), "c", "the row that was first is now second");
    assert_eq!(cell(&t, 3, 0), "e");
    assert!(near(t.rows[1].height, t.rows[0].height), "the new row takes the height of the one above it");

    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(grid(&s, id).n_rows(), 3);
}

/// A new column is empty too, and it widens the table rather than squeezing the others: the widths
/// the user set are theirs to keep.
#[test]
fn a_column_inserted_to_the_left_is_empty_and_widens_the_table() {
    let mut s = Session::with_new();
    let id = table(&mut s, 2, 2, 400.0, 100.0, json!([["a", "b"], ["c", "d"]]));
    assert!(near(grid(&s, id).width(), 400.0));

    s.execute("table.insertColumnLeft", &json!({"id": id, "col": 1})).expect("insert a column");
    let t = grid(&s, id);
    assert_eq!(t.n_cols(), 3);
    assert!(near(t.width(), 600.0), "the table is a column wider: {:?}", t.cols);
    assert_eq!(cell(&t, 0, 0), "a");
    assert_eq!(cell(&t, 0, 1), "", "the new column is empty");
    assert_eq!(cell(&t, 0, 2), "b", "the column that was first is now second");
    let sh = s.doc().unwrap().shape(ShapeId(id)).unwrap().clone();
    assert!(near(sh.xfrm.expect("a transform").w, 600.0), "the box follows the table");
}

/// Deleting a row takes its text with it and pulls everything below up, which is what the user
/// means by "delete this row" — not "empty it".
#[test]
fn deleting_a_row_takes_its_text_with_it() {
    let mut s = Session::with_new();
    let id = three_by_two(&mut s);

    s.execute("table.deleteRow", &json!({"id": id, "row": 1})).expect("delete the row");
    let t = grid(&s, id);
    assert_eq!(t.n_rows(), 2);
    assert_eq!(cell(&t, 0, 0), "a");
    assert_eq!(cell(&t, 1, 0), "e", "the last row moved up");
    assert!(near(t.height(), 80.0), "the table is a row shorter");
}

/// The same for a column, and the table gets narrower rather than leaving a gap.
#[test]
fn deleting_a_column_narrows_the_table() {
    let mut s = Session::with_new();
    let id = table(&mut s, 2, 3, 600.0, 100.0, json!([["a", "b", "c"], ["d", "e", "f"]]));

    s.execute("table.deleteColumn", &json!({"id": id, "col": 1})).expect("delete the column");
    let t = grid(&s, id);
    assert_eq!(t.n_cols(), 2);
    assert_eq!(cell(&t, 0, 0), "a");
    assert_eq!(cell(&t, 0, 1), "c", "the last column moved left");
    assert!(near(t.width(), 400.0), "the table is a column narrower");
}

/// A table is never left with no rows or no columns: the command refuses, says so, and changes
/// nothing at all — no half-applied edit and no undo step for the user to step back over.
#[test]
fn the_last_row_and_column_refuse_to_be_deleted() {
    let mut s = Session::with_new();
    let id = table(&mut s, 1, 1, 200.0, 60.0, json!([["only"]]));
    let before = s.doc().unwrap().history.undo.len();

    let err = s.execute("table.deleteRow", &json!({"id": id, "row": 0})).expect_err("the only row");
    assert!(err.to_string().contains("last row"), "{err}");
    let err = s.execute("table.deleteColumn", &json!({"id": id, "col": 0})).expect_err("the only column");
    assert!(err.to_string().contains("last column"), "{err}");

    let t = grid(&s, id);
    assert_eq!((t.n_rows(), t.n_cols()), (1, 1));
    assert_eq!(cell(&t, 0, 0), "only", "the cell it refused to delete is still there");
    assert_eq!(s.doc().unwrap().history.undo.len(), before, "a refused command is not an undo step");
}

// ---------------------------------------------------------------------------------------------
// Cell size
// ---------------------------------------------------------------------------------------------

/// Distribute gives every row or every column the same share of the table it already had: it is a
/// re-spacing, not a resize.
#[test]
fn distributing_rows_and_columns_evens_them_out_without_resizing_the_table() {
    let mut s = Session::with_new();
    let id = table(&mut s, 2, 2, 400.0, 100.0, json!([["a", "b"], ["c", "d"]]));
    s.execute("table.columnWidth", &json!({"id": id, "col": 0, "width": 300.0})).expect("one wide column");
    s.execute("table.rowHeight", &json!({"id": id, "row": 0, "height": 20.0})).expect("one short row");

    let (w, h) = (grid(&s, id).width(), grid(&s, id).height());
    assert!(near(w, 500.0) && near(h, 70.0), "{w} × {h}");

    s.execute("table.distributeColumns", &json!({"id": id})).expect("distribute the columns");
    let t = grid(&s, id);
    assert!(t.cols.iter().all(|c| near(*c, 250.0)), "{:?}", t.cols);
    assert!(near(t.width(), w), "the table is exactly as wide as it was");

    s.execute("table.distributeRows", &json!({"id": id})).expect("distribute the rows");
    let t = grid(&s, id);
    assert!(t.rows.iter().all(|r| near(r.height, 35.0)), "{:?}", t.rows.iter().map(|r| r.height).collect::<Vec<_>>());
    assert!(near(t.height(), h), "and exactly as tall");
}

// ---------------------------------------------------------------------------------------------
// Merging and splitting
// ---------------------------------------------------------------------------------------------

/// Split takes apart the merge at a cell and keeps the text where it is: a merge collects the text
/// of every cell it covers into its anchor, and splitting does not know how to give it back.
#[test]
fn splitting_a_merged_cell_undoes_the_merge_and_keeps_the_text_in_place() {
    let mut s = Session::with_new();
    let id = table(&mut s, 2, 2, 400.0, 100.0, json!([["a", "b"], ["c", "d"]]));

    let err = s.execute("table.split", &json!({"id": id, "cell": [0, 0]})).expect_err("nothing is merged");
    assert!(err.to_string().contains("isn't merged"), "{err}");

    s.execute("table.merge", &json!({"id": id, "from": [0, 0], "to": [1, 1]})).expect("merge the four cells");
    let t = grid(&s, id);
    assert_eq!((t.cell(0, 0).expect("a cell").grid_span, t.cell(0, 0).expect("a cell").row_span), (2, 2));
    assert!(t.cell(1, 1).expect("a cell").is_covered());
    let merged = cell(&t, 0, 0);
    assert!(merged.contains('a') && merged.contains('d'), "the merge collected every cell's text: {merged:?}");

    s.execute("table.split", &json!({"id": id, "cell": [0, 0]})).expect("split it again");
    let t = grid(&s, id);
    assert_eq!((t.cell(0, 0).expect("a cell").grid_span, t.cell(0, 0).expect("a cell").row_span), (1, 1));
    assert!(!t.cell(1, 1).expect("a cell").is_covered(), "the cells it covered are their own again");
    assert!(t.cell(1, 1).expect("a cell").text.is_empty(), "and they come back empty");
    assert_eq!(cell(&t, 0, 0), merged, "the text stays in the anchor");
}

// ---------------------------------------------------------------------------------------------
// Style options
// ---------------------------------------------------------------------------------------------

/// Each switch stands on its own, so setting one leaves the others as the style had them.
#[test]
fn table_options_set_only_the_switches_they_are_given() {
    let mut s = Session::with_new();
    let id = table(&mut s, 2, 2, 400.0, 100.0, json!([["a", "b"], ["c", "d"]]));
    let t = grid(&s, id);
    assert!(t.first_row && t.band_row, "a table starts with a header row and banded rows");
    assert!(!t.last_row && !t.last_col && !t.first_col && !t.band_col);

    s.execute("table.options", &json!({"id": id, "totalRow": true, "bandedRows": false, "firstColumn": true})).expect("three switches");
    let t = grid(&s, id);
    assert!(t.last_row, "the total row is on");
    assert!(!t.band_row, "the banding is off");
    assert!(t.first_col, "the first column is emphasised");
    assert!(t.first_row, "the header row was not mentioned, so it was left alone");
    assert!(!t.band_col && !t.last_col, "and neither was anything else");

    // Nothing to set still goes through one edit, so the call leaves an undo step of its own and
    // the switches are reached by the second undo, not the first. (An undo step for a no-op is a
    // wart, but it is the command's behaviour today; callers avoid it by only calling when the
    // dialog's switches changed.)
    s.execute("table.options", &json!({"id": id})).expect("no switches at all");
    let t2 = grid(&s, id);
    assert_eq!((t2.last_row, t2.band_row, t2.first_col), (true, false, true), "nothing to change, nothing changed");

    s.execute("edit.undo", &json!({})).expect("undo the call that set nothing");
    let t = grid(&s, id);
    assert_eq!((t.last_row, t.band_row, t.first_col), (true, false, true), "so the switches are still set");
    s.execute("edit.undo", &json!({})).expect("undo the three switches");
    let t = grid(&s, id);
    assert!(!t.last_row && t.band_row && !t.first_col, "and now the three switches go back together");
}

// ---------------------------------------------------------------------------------------------
// The border tools
// ---------------------------------------------------------------------------------------------

/// Draw Table splits the cell a line crosses for more than half of it, turning a one-cell table
/// into two columns; a stub of a line or a point off the table is refused with a reason.
#[test]
fn drawing_a_line_through_a_cell_splits_it() {
    let mut s = Session::with_new();
    let id = table(&mut s, 1, 1, 200.0, 100.0, json!([["a"]]));

    // Down the middle of the cell, from inside its top to inside its bottom.
    s.execute("table.drawLine", &json!({"from": [200.0, 110.0], "to": [200.0, 190.0]})).expect("split it");
    let t = grid(&s, id);
    assert_eq!((t.n_rows(), t.n_cols()), (1, 2), "the line split the cell in two");
    assert!(t.cols.iter().all(|c| near(*c, 100.0)), "{:?}", t.cols);
    assert_eq!(cell(&t, 0, 0), "a", "the text stays in the left half");
    assert_eq!(cell(&t, 0, 1), "");

    let err = s.execute("table.drawLine", &json!({"from": [200.0, 110.0], "to": [200.0, 112.0]})).expect_err("a stub");
    assert!(err.to_string().contains("doesn't divide"), "{err}");
    let err = s.execute("table.drawLine", &json!({"from": [900.0, 900.0], "to": [900.0, 980.0]})).expect_err("nothing there");
    assert!(err.to_string().contains("no table there"), "{err}");
    // A malformed id names no table at all — it never falls back to the one under the point.
    let err = s.execute("table.drawLine", &json!({"from": [200.0, 110.0], "to": [200.0, 190.0], "id": "nope"})).expect_err("a bad id");
    assert!(err.to_string().contains("no table there"), "{err}");
}

/// The eraser merges the two cells on either side of the border nearest the pointer, and says so
/// when there is no border within reach — including when the pointer is inside a cell.
#[test]
fn the_eraser_merges_the_cells_a_border_separates() {
    let mut s = Session::with_new();
    let id = table(&mut s, 2, 2, 200.0, 100.0, json!([["a", "b"], ["c", "d"]]));
    assert!(near(grid(&s, id).cols[0], 100.0), "two columns of 100 pt");

    // On the border between the two columns, inside the first row.
    s.execute("table.eraseLine", &json!({"at": [200.0, 125.0]})).expect("erase the border");
    let t = grid(&s, id);
    assert_eq!(t.cell(0, 0).expect("a cell").grid_span, 2, "the two cells are one now");
    assert_eq!(t.n_cols(), 2, "the grid keeps its columns; only the merge changed");
    let merged = cell(&t, 0, 0);
    assert!(merged.contains('a') && merged.contains('b'), "the text of both cells is kept: {merged:?}");
    assert_eq!(cell(&t, 1, 1), "d", "the row below is untouched");

    // In the middle of a cell, 50 pt from the nearest border.
    let err = s.execute("table.eraseLine", &json!({"at": [150.0, 125.0]})).expect_err("no border in reach");
    assert!(err.to_string().contains("no border to erase"), "{err}");
    let err = s.execute("table.eraseLine", &json!({"at": [200.0, 900.0]})).expect_err("off the table");
    assert!(err.to_string().contains("no table there"), "{err}");

    s.execute("edit.undo", &json!({})).expect("undo");
    assert_eq!(grid(&s, id).cell(0, 0).expect("a cell").grid_span, 1);
}
