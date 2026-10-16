//! Commands for value-preserving cell formats and ordered nested table owners.
use super::*;
use deckcraft_model::table::{CellBlock, CellTextBlock, NestedTable};
use deckcraft_model::table::{CellConstant, CellConstantState, CellFormula, CellFormulaState, CellValue, NumberFormat, TablePathStep};
use deckcraft_model::{ShapeId, TextBody, edit};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "table.setCellNumberFormat",
            "Number Format",
            ["Table Layout", "Data"],
            None,
            "{format: object|null, row?, col?, id?, path?} — format an authoritative cell value without changing it",
            has_selection,
            set_format
        ),
        cmd!(
            "table.convertToLiteral",
            "Convert to Text",
            ["Table Layout", "Data"],
            None,
            "{row?, col?, id?, path?} — retain the displayed result as ordinary text",
            has_selection,
            literal
        ),
        cmd!(
            "table.insertNested",
            "Insert Nested Table",
            ["Table Layout", "Data"],
            None,
            "{rows?: 2, cols?: 2, row?, col?, after?: blockId, id?, path?} — insert at the cell text selection, or after a block",
            has_selection,
            insert_nested
        ),
        cmd!(
            "table.removeNested",
            "Delete Nested Table",
            ["Table Layout", "Data"],
            None,
            "{child, row?, col?, id?, path?}",
            has_selection,
            remove_nested
        ),
        cmd!(noundo "table.enterNested", "Edit Nested Table", ["Table Layout", "Data"], None,
            "{child, row?, col?, id?, path?}", has_selection, enter_nested),
        cmd!(noundo "table.leaveNested", "Parent Table", ["Table Layout", "Data"], None,
            "{id?} — return to the text following the current nested table", has_selection, leave_nested),
    ]
}

fn target(s: &Session, p: &Value, command: &str) -> Result<(ShapeId, deckcraft_model::table::TablePath, usize, usize)> {
    let id = formula_target(s, p, command)?;
    let path = path_for(s, p, id, command)?;
    let (row, col) = at(s, p);
    let shape = s.doc()?.shape(id).ok_or_else(|| bad(command, "no such table"))?;
    let ShapeKind::Table(table) = &shape.kind else { return Err(bad(command, "not a table")) };
    let table = table.at_path(&path)?;
    let cell = table.cell(row, col).ok_or_else(|| bad(command, "no such cell"))?;
    if cell.is_covered() {
        return Err(bad(command, "select the visible owner of this merged cell"));
    }
    Ok((id, path, row, col))
}

fn table_mut<'a>(doc: &'a mut deckcraft_model::Presentation, selection: &crate::Selection, id: ShapeId) -> Result<&'a mut Table> {
    let shapes = crate::shapes_mut(doc, selection).ok_or_else(|| bad("table", "no shape owner"))?;
    let shape = deckcraft_model::find_shape_mut(shapes, id).ok_or_else(|| bad("table", "no such table"))?;
    let ShapeKind::Table(table) = &mut shape.kind else { return Err(bad("table", "not a table")) };
    Ok(table)
}

pub(super) fn body_with_display(before: &TextBody, text: &str) -> TextBody {
    if before.text() == text {
        return before.clone();
    }
    let mut body = TextBody::from_text(text);
    body.body.clone_from(&before.body);
    body.list_style.clone_from(&before.list_style);
    for (index, paragraph) in body.paragraphs.iter_mut().enumerate() {
        if let Some(source) = before.paragraphs.get(index).or_else(|| before.paragraphs.first()) {
            paragraph.level = source.level;
            paragraph.props.clone_from(&source.props);
            paragraph.def_props.clone_from(&source.def_props);
            paragraph.end_props.clone_from(&source.end_props);
            if let Some(props) = source.runs.first().map(|run| &run.props) {
                for run in &mut paragraph.runs {
                    run.props.clone_from(props);
                }
            }
        }
    }
    body
}

fn formula_value(formula: &CellFormula) -> Option<CellValue> {
    match formula.outcome() {
        deckcraft_model::table::FormulaOutcome::Number { lexical } => {
            Some(CellValue::Number { kind: deckcraft_model::table::CellNumberKind::Float, lexical: lexical.clone() })
        }
        deckcraft_model::table::FormulaOutcome::Boolean { value } => Some(CellValue::Boolean { value: *value }),
        deckcraft_model::table::FormulaOutcome::String { value } => Some(CellValue::String { value: value.clone() }),
        deckcraft_model::table::FormulaOutcome::Error { .. } => None,
    }
}

fn set_format(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.setCellNumberFormat";
    strict_params(p, CMD, &["format", "row", "col", "id", "path"])?;
    let format: Option<Arc<NumberFormat>> = match p.get("format") {
        Some(Value::Null) => None,
        Some(value) => {
            let format: NumberFormat = serde_json::from_value(value.clone()).map_err(|e| bad(CMD, format!("invalid number format: {e}")))?;
            format.validate()?;
            Some(Arc::new(format))
        }
        None => return Err(bad(CMD, "missing `format`")),
    };
    let (id, path, row, col) = target(s, p, CMD)?;
    s.edit_calculated(|doc, selection, budget| {
        let table = table_mut(doc, selection, id)?.at_path_mut(&path)?;
        let cell = table.cell_mut(row, col).ok_or_else(|| bad(CMD, "no such cell"))?;
        let formula = match cell.formula_state()? {
            CellFormulaState::Current(formula) => Some(formula.clone()),
            CellFormulaState::Stale => return Err(bad(CMD, "the formula display was edited; commit the text edit first")),
            CellFormulaState::Absent => None,
        };
        let value = if let Some(formula) = &formula {
            formula_value(formula)
        } else {
            match cell.constant_state()? {
                CellConstantState::Current(data) => Some(data.value().clone()),
                CellConstantState::Stale => return Err(bad(CMD, "the stored-value display was edited; commit the text edit first")),
                CellConstantState::Absent => Some(CellValue::String { value: cell.text.text() }),
            }
        };
        let display = match (&format, &value) {
            (Some(format), Some(value)) => format.format_value(value)?.text,
            (None, Some(value)) => match value {
                CellValue::String { value } => value.clone(),
                CellValue::Boolean { value } => {
                    if *value {
                        "TRUE".into()
                    } else {
                        "FALSE".into()
                    }
                }
                CellValue::Number { lexical, .. } | CellValue::Date { lexical } | CellValue::Duration { lexical } => lexical.clone(),
            },
            (_, None) => formula.as_ref().ok_or_else(|| bad(CMD, "missing formula outcome"))?.outcome().plain_display_text()?,
        };
        let body = body_with_display(&cell.text, &display);
        if let Some(formula) = formula {
            let profile = if formula.is_v2() { formula.profile() } else { "exact-decimal-v1-formatted" };
            cell.formula = Some(Arc::new(CellFormula::from_profile_display(profile, formula.expression().into(), formula.outcome().clone(), &body)?));
        } else if let Some(value) = value {
            cell.data = Some(Arc::new(CellConstant::from_display(value, &body)?));
        }
        cell.text = body;
        cell.number_format = format;
        budget.next_pass();
        deckcraft_model::table::validate_constants_with_budget(doc, budget)?;
        Ok(())
    })?;
    ok()
}

fn literal(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.convertToLiteral";
    strict_params(p, CMD, &["row", "col", "id", "path"])?;
    let (id, path, row, col) = target(s, p, CMD)?;
    s.edit(|doc, selection| {
        let cell = table_mut(doc, selection, id)?.at_path_mut(&path)?.cell_mut(row, col).ok_or_else(|| bad(CMD, "no such cell"))?;
        let value = cell.formula.as_deref().and_then(formula_value).or_else(|| cell.data.as_ref().map(|data| data.value().clone()));
        if let (Some(format), Some(value)) = (&cell.number_format, value)
            && let Some([r, g, b, a]) = format.format_value(&value)?.color
        {
            for run in cell.text.paragraphs.iter_mut().flat_map(|p| &mut p.runs) {
                run.props.fill = Some(deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::rgba(r, g, b, a))));
            }
        }
        cell.data = None;
        cell.formula = None;
        cell.number_format = None;
        Ok(())
    })?;
    ok()
}

fn insert_nested(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.insertNested";
    strict_params(p, CMD, &["rows", "cols", "row", "col", "after", "id", "path"])?;
    let dimension = |key: &str| -> Result<usize> {
        let n = if p.get(key).is_none() { 2 } else { usize_param(p, key).ok_or_else(|| bad(CMD, "invalid table dimensions"))? };
        if !(1..=100).contains(&n) {
            return Err(bad(CMD, "nested-table dimensions must be between 1 and 100"));
        }
        Ok(n)
    };
    let rows = dimension("rows")?;
    let cols = dimension("cols")?;
    let (id, path, row, col) = target(s, p, CMD)?;
    if path.0.len() >= deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
        return Err(bad(CMD, "nested-table depth exceeds limit"));
    }
    let after = p.get("after").map(|_| u32_param(p, "after").ok_or_else(|| bad(CMD, "invalid `after` block identity"))).transpose()?;
    let child = s.edit_calculated(|doc, selection, budget| {
        let editing = selection
            .text
            .as_ref()
            .filter(|t| selection.table_owner == Some(id) && selection.table_path == path && t.shape == id && t.cell == Some((row, col)) && !t.notes)
            .cloned();
        let block_id = selection.cell_block;
        let table = table_mut(doc, selection, id)?.at_path_mut(&path)?;
        let width = table.cols.get(col).copied().unwrap_or(100.0).max(12.0);
        let cell = table.cell_mut(row, col).ok_or_else(|| bad(CMD, "no such cell"))?;
        if cell.blocks.len().saturating_add(2) > deckcraft_model::table::MAX_CELL_BLOCKS {
            return Err(bad(CMD, "cell block count exceeds limit"));
        }
        let first = cell.next_block_id()?;
        let trailing = first.checked_add(1).ok_or_else(|| bad(CMD, "cell block identity space exhausted"))?;
        let (position, suffix) = if let Some(after) = after {
            let position = if after == 0 {
                0
            } else {
                cell.blocks.iter().position(|b| b.id() == after).ok_or_else(|| bad(CMD, "no such `after` block"))?.saturating_add(1)
            };
            (position, TextBody::from_text(""))
        } else if let Some(text) = editing {
            if matches!(block_id, None | Some(0)) && cell.formula.is_some() {
                return Err(bad(CMD, "convert the formula to text before splitting its display"));
            }
            let position = match block_id {
                None | Some(0) => 0,
                Some(id) => cell.blocks.iter().position(|b| b.id() == id).ok_or_else(|| bad(CMD, "edited cell block is missing"))?.saturating_add(1),
            };
            let body = cell.text_block(block_id).ok_or_else(|| bad(CMD, "edited cell block is missing"))?;
            let (a, b) = text.ordered();
            let prefix = edit::slice(body, (0, 0), a);
            let suffix = edit::slice(body, b, edit::end(body));
            *cell.text_block_mut(block_id).ok_or_else(|| bad(CMD, "edited cell block is missing"))? = prefix;
            (position, suffix)
        } else {
            (cell.blocks.len(), TextBody::from_text(""))
        };
        cell.blocks.try_reserve(2).map_err(|_| bad(CMD, "cannot allocate nested table"))?;
        cell.blocks.insert(position, CellBlock::Table(NestedTable { id: first, table: Box::new(Table::new(rows, cols, width, 24.0)) }));
        cell.blocks.insert(position.saturating_add(1), CellBlock::Text(CellTextBlock { id: trailing, body: suffix }));
        budget.next_pass();
        deckcraft_model::table::reconcile_constants_with_budget(doc, budget)?;
        let root = table_mut(doc, selection, id)?;
        root.validate_content()?;
        selection.shapes = vec![id];
        selection.table_owner = Some(id);
        selection.table_path = path.clone();
        selection.table_path.0.push(TablePathStep { row, col, child: first });
        selection.cell_block = None;
        selection.cells = None;
        selection.text = Some(crate::TextSel { shape: id, cell: Some((0, 0)), ..Default::default() });
        Ok(first)
    })?;
    Ok(json!({"child": child, "path": s.doc()?.selection.table_path}))
}

fn child_parameter(p: &Value, command: &str) -> Result<u32> {
    let child = u32_param(p, "child").ok_or_else(|| bad(command, "missing or invalid `child`"))?;
    if child == 0 {
        return Err(bad(command, "child identity must be nonzero"));
    }
    Ok(child)
}

fn remove_nested(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.removeNested";
    strict_params(p, CMD, &["child", "row", "col", "id", "path"])?;
    let child = child_parameter(p, CMD)?;
    let (id, path, row, col) = target(s, p, CMD)?;
    s.edit(|doc, selection| {
        let cell = table_mut(doc, selection, id)?.at_path_mut(&path)?.cell_mut(row, col).ok_or_else(|| bad(CMD, "no such cell"))?;
        let index =
            cell.blocks.iter().position(|b| matches!(b, CellBlock::Table(t) if t.id == child)).ok_or_else(|| bad(CMD, "no such nested table"))?;
        cell.blocks.remove(index);
        Ok(())
    })?;
    ok()
}

fn enter_nested(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.enterNested";
    strict_params(p, CMD, &["child", "row", "col", "id", "path"])?;
    let child = child_parameter(p, CMD)?;
    let (id, mut path, row, col) = target(s, p, CMD)?;
    path.0.push(TablePathStep { row, col, child });
    path.validate()?;
    let shape = s.doc()?.shape(id).ok_or_else(|| bad(CMD, "no such table"))?;
    let ShapeKind::Table(table) = &shape.kind else { return Err(bad(CMD, "not a table")) };
    table.at_path(&path)?.cell(0, 0).ok_or_else(|| bad(CMD, "nested table is empty"))?;
    s.select(|_, selection| {
        selection.shapes = vec![id];
        selection.table_owner = Some(id);
        selection.table_path = path;
        selection.cell_block = None;
        selection.cells = None;
        selection.text = Some(crate::TextSel { shape: id, cell: Some((0, 0)), ..Default::default() });
    })?;
    ok()
}

fn leave_nested(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "table.leaveNested";
    strict_params(p, CMD, &["id"])?;
    let id = formula_target(s, p, CMD)?;
    let mut path = path_for(s, &json!({}), id, CMD)?;
    let step = path.0.pop().ok_or_else(|| bad(CMD, "already in the top-level table"))?;
    let shape = s.doc()?.shape(id).ok_or_else(|| bad(CMD, "no such table"))?;
    let ShapeKind::Table(table) = &shape.kind else { return Err(bad(CMD, "not a table")) };
    let cell = table.at_path(&path)?.cell(step.row, step.col).ok_or_else(|| bad(CMD, "parent cell is missing"))?;
    let block = cell.blocks.iter().position(|b| b.id() == step.child).and_then(|i| cell.blocks.get(i.saturating_add(1))).and_then(|b| match b {
        CellBlock::Text(t) => Some(t.id),
        _ => None,
    });
    s.select(|_, selection| {
        selection.shapes = vec![id];
        selection.table_owner = Some(id);
        selection.table_path = path;
        selection.cell_block = block;
        selection.cells = None;
        selection.text = Some(crate::TextSel { shape: id, cell: Some((step.row, step.col)), ..Default::default() });
    })?;
    ok()
}

fn u32_param(p: &Value, key: &str) -> Option<u32> {
    u32::try_from(p.get(key)?.as_u64()?).ok()
}
