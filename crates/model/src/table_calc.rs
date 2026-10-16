//! Deterministic bounded table-local DAG evaluation and preflighted updates.
use super::{
    CellConstant, CellConstantState, CellFormula, CellFormulaState, CellNumberKind, CellValue, FormulaError, FormulaOutcome, Table,
    decimal::Decimal,
    formula::{Node, invalid, ordinary_display},
};
use crate::{Paragraph, Result, Run, TextBody};
use std::sync::Arc;

#[path = "table_calc_v2.rs"]
mod v2;

type Number = std::result::Result<Decimal, FormulaError>;
const MAX_TABLE_FORMULAS: usize = 128;
const MAX_DOCUMENT_FORMULAS: usize = 1024;
const MAX_NODES: usize = 65_536;
const MAX_EDGES: usize = 65_536;
const MAX_WORK: usize = 262_144;
const MAX_OPERATION_WORK: usize = 1_048_576;

/// One instance must be shared across all owners during document operations.
#[derive(Default, Clone, Debug)]
pub struct FormulaBudget {
    formulas: usize,
    nodes: usize,
    edges: usize,
    work: usize,
    operation_work: usize,
    numeric_work: usize,
    exhausted: bool,
}
impl FormulaBudget {
    /// Begin another whole-graph pass in the SAME operation. Call once before a
    /// top-level document traversal, never once per table/owner. Graph limits reset;
    /// cumulative operation fuel is retained (at most 1,048,576 visits).
    pub fn next_pass(&mut self) {
        self.formulas = 0;
        self.nodes = 0;
        self.edges = 0;
        self.work = 0;
    }
    /// A failed cumulative charge permanently exhausts this operation budget.
    /// Call after declining an unsupported import candidate so resource failure
    /// cannot accidentally be converted into a recoverable formula warning.
    pub fn check_limits(&self) -> Result<()> {
        if self.exhausted { Err(invalid("table formula cumulative resource limit exceeded")) } else { Ok(()) }
    }
    fn charge(counter: &mut usize, amount: usize, limit: usize) -> Result<()> {
        *counter =
            counter.checked_add(amount).filter(|next| *next <= limit).ok_or_else(|| invalid("table formula cumulative resource limit exceeded"))?;
        Ok(())
    }
    fn work(&mut self, amount: usize) -> Result<()> {
        self.check_limits()?;
        let pass = self.work.checked_add(amount).filter(|value| *value <= MAX_WORK);
        let operation = self.operation_work.checked_add(amount).filter(|value| *value <= MAX_OPERATION_WORK);
        match (pass, operation) {
            (Some(pass), Some(operation)) => {
                self.work = pass;
                self.operation_work = operation;
                Ok(())
            }
            (_, None) => {
                self.exhausted = true;
                Err(invalid("table formula operation work limit exceeded"))
            }
            _ => {
                self.exhausted = true;
                Err(invalid("table formula cumulative resource limit exceeded"))
            }
        }
    }
    fn numeric(&mut self, units: usize) -> Result<()> {
        self.check_limits()?;
        if Self::charge(&mut self.numeric_work, units, 256 * 1024 * 1024).is_err() {
            self.exhausted = true;
            return Err(invalid("formula numeric work limit exceeded"));
        }
        Ok(())
    }
    fn formula(&mut self, formula: &CellFormula) -> Result<()> {
        self.check_limits()?;
        let result = (|| {
            Self::charge(&mut self.formulas, 1, MAX_DOCUMENT_FORMULAS)?;
            Self::charge(&mut self.nodes, formula.node_count(), MAX_NODES)?;
            Self::charge(&mut self.edges, formula.reference_count(), MAX_EDGES)?;
            if formula.is_v2() {
                self.work(formula.expression().len())?;
            }
            self.work(formula.reference_count())
        })();
        if result.is_err() {
            self.exhausted = true;
        }
        result
    }
}

struct Update {
    row: usize,
    col: usize,
    expected: Arc<CellFormula>,
    current: bool,
    formula: Option<Arc<CellFormula>>,
    replacement: Option<String>,
}
struct Observation {
    row: usize,
    col: usize,
    data: Option<Arc<CellConstant>>,
    formula: Option<Arc<CellFormula>>,
    current: bool,
    literal: Option<Option<String>>,
    format: Option<Option<Arc<super::NumberFormat>>>,
}
/// Merge ownership is input authority even when every scalar cache is unchanged.
/// Row lengths bind physical coordinates as well as the flattened cell sequence.
struct TableTopology {
    row_lengths: Vec<usize>,
    cells: Vec<(u32, u32, bool, bool)>,
}
impl TableTopology {
    fn capture(table: &Table, budget: &mut FormulaBudget) -> Result<Self> {
        let count = table.rows.iter().try_fold(0usize, |count, row| {
            count
                .checked_add(row.cells.len())
                .filter(|count| *count <= 100_000)
                .ok_or_else(|| invalid("formula topology snapshot exceeds cell limit"))
        })?;
        // Reserve the bounded comparison work now too: apply has no budget
        // parameter and must never perform an unaccounted second owner walk.
        let work = count
            .checked_add(table.rows.len())
            .and_then(|work| work.checked_mul(2))
            .ok_or_else(|| invalid("formula topology snapshot work overflow"))?;
        budget.work(work)?;
        let mut row_lengths = Vec::new();
        row_lengths.try_reserve_exact(table.rows.len()).map_err(|_| invalid("cannot allocate formula topology rows"))?;
        let mut cells = Vec::new();
        cells.try_reserve_exact(count).map_err(|_| invalid("cannot allocate formula topology cells"))?;
        for row in &table.rows {
            row_lengths.push(row.cells.len());
            cells.extend(row.cells.iter().map(|cell| (cell.grid_span, cell.row_span, cell.h_merge, cell.v_merge)));
        }
        Ok(Self { row_lengths, cells })
    }
    fn matches(&self, table: &Table) -> bool {
        self.row_lengths.iter().copied().eq(table.rows.iter().map(|row| row.cells.len()))
            && self.cells.iter().copied().eq(table
                .rows
                .iter()
                .flat_map(|row| &row.cells)
                .map(|cell| (cell.grid_span, cell.row_span, cell.h_merge, cell.v_merge)))
    }
}

/// Private updates cannot be forged. Apply only to the unchanged table used to plan.
#[derive(Default)]
pub struct TableCalculationPlan {
    updates: Vec<Update>,
    observations: Vec<Observation>,
    dimensions: Option<(usize, usize)>,
    topology: Option<TableTopology>,
}
impl TableCalculationPlan {
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }
    pub(crate) fn additional_metadata(&self) -> usize {
        self.updates.iter().fold(0usize, |bytes, update| {
            bytes.saturating_add(
                update.formula.as_ref().map_or(0, |formula| formula.metadata_bytes().saturating_sub(update.expected.metadata_bytes())),
            )
        })
    }
    pub fn apply(self, table: &mut Table) -> Result<bool> {
        // No edits until every destination and referenced authority has been checked.
        if self.dimensions.is_some_and(|shape| shape != (table.rows.len(), table.cols.len())) {
            return Err(invalid("formula plan table dimensions changed"));
        }
        if let Some(topology) = &self.topology {
            validate_formula_grid(table)?;
            if !topology.matches(table) {
                return Err(invalid("formula plan merged ownership changed"));
            }
        }
        for observed in &self.observations {
            let cell = table.cell(observed.row, observed.col).ok_or_else(|| invalid("formula plan predecessor changed"))?;
            let same_data = match (&cell.data, &observed.data) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            let same_formula = match (&cell.formula, &observed.formula) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            let current = if let Some(data) = &cell.data {
                data.matches_display(&cell.text)?
            } else if let Some(formula) = &cell.formula {
                formula.matches_display(&cell.text)?
            } else {
                false
            };
            let literal_changed = match &observed.literal {
                Some(expected) => &v2::literal_state(cell)? != expected,
                None => false,
            };
            if !same_data
                || !same_formula
                || current != observed.current
                || literal_changed
                || observed.format.as_ref().is_some_and(|expected| expected != &cell.number_format)
            {
                return Err(invalid("formula plan predecessor changed"));
            }
        }
        for update in &self.updates {
            let cell = table.cell(update.row, update.col).ok_or_else(|| invalid("formula plan table changed"))?;
            if cell.data.is_some()
                || !cell.formula.as_ref().is_some_and(|formula| Arc::ptr_eq(formula, &update.expected))
                || update.expected.matches_display(&cell.text)? != update.current
            {
                return Err(invalid("formula plan destination changed"));
            }
            if update.replacement.is_some() && !ordinary_display(&cell.text) {
                return Err(invalid("formula plan display changed"));
            }
        }
        let changed = !self.updates.is_empty();
        for update in self.updates {
            // Destination existence and display structure were proved above; no fallible
            // work or allocation remains after publication starts.
            if let Some(cell) = table.cell_mut(update.row, update.col) {
                cell.formula = update.formula;
                if let Some(text) = update.replacement {
                    replace_display(&mut cell.text, text);
                }
            }
        }
        Ok(changed)
    }
}

fn replace_display(body: &mut TextBody, text: String) {
    if let Some(paragraph) = body.paragraphs.first_mut() {
        let mut runs = paragraph.runs.iter_mut();
        if let Some(first) = runs.next() {
            first.text = text;
        }
        for run in runs {
            run.text.clear();
        }
    }
}

struct Entry<'a> {
    row: usize,
    col: usize,
    formula: &'a Arc<CellFormula>,
}

impl Table {
    pub fn has_formulas(&self) -> bool {
        self.rows.iter().any(|row| row.cells.iter().any(|cell| cell.formula.is_some()))
    }

    pub fn plan_recalculation(&self) -> Result<TableCalculationPlan> {
        self.plan_recalculation_with_budget(&mut FormulaBudget::default())
    }

    pub fn plan_recalculation_with_budget(&self, budget: &mut FormulaBudget) -> Result<TableCalculationPlan> {
        self.plan_recalculation_mode(budget, false)
    }
    fn plan_recalculation_mode(&self, budget: &mut FormulaBudget, authority: bool) -> Result<TableCalculationPlan> {
        if self.rows.iter().flat_map(|r| &r.cells).any(|c| c.formula.as_ref().is_some_and(|f| f.uses_formatted_display())) {
            return v2::plan(self, budget, authority);
        }
        let mut entries = Vec::new();
        let mut updates = Vec::new();
        entries.try_reserve_exact(MAX_TABLE_FORMULAS).map_err(|_| invalid("cannot allocate table formula graph"))?;
        updates.try_reserve_exact(MAX_TABLE_FORMULAS).map_err(|_| invalid("cannot allocate table formula updates"))?;
        let mut attached = 0usize;
        let mut physical_cells = 0usize;
        if self.rows.len() > 1_000_000 || self.cols.len() > 1_000_000 {
            return Err(invalid("table grid exceeds bounded cell or dimension limits"));
        }
        for (row, cells) in self.rows.iter().enumerate() {
            physical_cells = physical_cells
                .checked_add(cells.cells.len())
                .filter(|count| *count <= 1_000_000)
                .ok_or_else(|| invalid("table grid exceeds bounded cell or dimension limits"))?;
            for (col, cell) in cells.cells.iter().enumerate() {
                if let Some(formula) = &cell.formula {
                    attached += 1;
                    if attached > MAX_TABLE_FORMULAS {
                        return Err(invalid("table formula count exceeds limit"));
                    }
                    budget.formula(formula)?;
                    if cell.data.is_some() {
                        return Err(invalid("cell cannot contain both constant data and formula"));
                    }
                    entries.push(Entry { row, col, formula });
                    if !authority && !formula.matches_display(&cell.text)? {
                        updates.push(Update { row, col, expected: formula.clone(), current: false, formula: None, replacement: None });
                    }
                }
            }
        }
        if attached == 0 {
            return Ok(TableCalculationPlan::default());
        }
        validate_formula_grid(self)?;
        // Each source cell was visited once, including literal/nonformula predecessors.
        let cells = self.rows.len().checked_mul(self.cols.len()).ok_or_else(|| invalid("formula grid size overflow"))?;
        budget.work(cells)?;
        let mut indegree = Vec::new();
        let mut done = Vec::new();
        let mut results: Vec<Option<Number>> = Vec::new();
        indegree.try_reserve_exact(entries.len()).map_err(|_| invalid("cannot allocate formula indegrees"))?;
        done.try_reserve_exact(entries.len()).map_err(|_| invalid("cannot allocate formula traversal"))?;
        results.try_reserve_exact(entries.len()).map_err(|_| invalid("cannot allocate formula results"))?;
        done.resize(entries.len(), false);
        results.resize(entries.len(), None);
        for entry in &entries {
            let mut dependencies = 0usize;
            for &(row, col) in &entry.formula.parsed.references {
                if self.cell(row, col).is_none() {
                    return Err(invalid("formula reference lies outside table grid"));
                }
                if find_entry(&entries, row, col).is_some() {
                    dependencies += 1;
                }
            }
            indegree.push(dependencies);
        }
        for _ in 0..entries.len() {
            let mut ready = None;
            for (index, (&degree, &visited)) in indegree.iter().zip(&done).enumerate() {
                budget.work(1)?;
                if !visited && degree == 0 {
                    ready = Some(index);
                    break;
                }
            }
            let index = ready.ok_or_else(|| invalid("formula dependency cycle"))?;
            let entry = entries.get(index).ok_or_else(|| invalid("invalid formula graph index"))?;
            let current =
                authority || self.cell(entry.row, entry.col).is_some_and(|cell| entry.formula.matches_display(&cell.text).is_ok_and(|value| value));
            let result =
                if current { evaluate(self, entry.formula, &entries, &results, budget, authority)? } else { Err(FormulaError::InputNotNumeric) };
            if let Some(slot) = results.get_mut(index) {
                *slot = Some(result);
            }
            if let Some(slot) = done.get_mut(index) {
                *slot = true;
            }
            let coordinate = (entry.row, entry.col);
            for (dependent, candidate) in entries.iter().enumerate() {
                budget.work(1)?;
                if candidate.formula.parsed.references.binary_search(&coordinate).is_ok() {
                    let slot = indegree.get_mut(dependent).ok_or_else(|| invalid("invalid formula dependency index"))?;
                    *slot = slot.checked_sub(1).ok_or_else(|| invalid("invalid formula dependency count"))?;
                }
            }
            let outcome = match result {
                Ok(number) => FormulaOutcome::Number { lexical: number.canonical() },
                Err(code) => FormulaOutcome::Error { code },
            };
            if current && !entry.formula.outcome().equivalent(&outcome)? {
                if authority {
                    return Err(invalid("formula cached outcome differs from recomputation"));
                }
                let cell = self.cell(entry.row, entry.col).ok_or_else(|| invalid("formula cell missing"))?;
                let replacement = outcome.display_text()?;
                let formula =
                    Arc::new(CellFormula::from_display(entry.formula.expression().into(), outcome, &replacement_body(&cell.text, &replacement)?)?);
                updates.push(Update {
                    row: entry.row,
                    col: entry.col,
                    expected: entry.formula.clone(),
                    current: true,
                    formula: Some(formula),
                    replacement: Some(replacement),
                });
            }
        }
        let mut observations = Vec::new();
        if !authority {
            let count = entries
                .iter()
                .try_fold(entries.len(), |count, entry| count.checked_add(entry.formula.parsed.references.len()))
                .ok_or_else(|| invalid("formula observation size overflow"))?;
            budget.work(count)?;
            let mut coordinates = Vec::new();
            coordinates.try_reserve_exact(count).map_err(|_| invalid("cannot allocate formula observation coordinates"))?;
            for entry in &entries {
                coordinates.push((entry.row, entry.col));
                coordinates.extend_from_slice(&entry.formula.parsed.references);
            }
            coordinates.sort_unstable();
            coordinates.dedup();
            budget.work(coordinates.len().checked_mul(2).ok_or_else(|| invalid("formula observation work overflow"))?)?;
            observations.try_reserve_exact(coordinates.len()).map_err(|_| invalid("cannot allocate formula predecessor observations"))?;
            for (row, col) in coordinates {
                let cell = self.cell(row, col).ok_or_else(|| invalid("formula observation coordinate missing"))?;
                let current = if let Some(data) = &cell.data {
                    data.matches_display(&cell.text)?
                } else if let Some(formula) = &cell.formula {
                    formula.matches_display(&cell.text)?
                } else {
                    false
                };
                observations.push(Observation {
                    row,
                    col,
                    data: cell.data.clone(),
                    formula: cell.formula.clone(),
                    current,
                    literal: None,
                    format: None,
                });
            }
        }
        Ok(TableCalculationPlan {
            updates,
            observations,
            dimensions: Some((self.rows.len(), self.cols.len())),
            topology: Some(TableTopology::capture(self, budget)?),
        })
    }
    pub fn recalculate(&mut self) -> Result<bool> {
        self.recalculate_with_budget(&mut FormulaBudget::default())
    }
    pub fn recalculate_with_budget(&mut self, budget: &mut FormulaBudget) -> Result<bool> {
        self.plan_recalculation_with_budget(budget)?.apply(self)
    }

    /// Strict validation recomputes every current cache; changed display metadata is
    /// handled separately by reconciliation, never mistaken for an authenticated cache.
    pub fn validate_formulas(&self) -> Result<()> {
        self.validate_formulas_with_budget(&mut FormulaBudget::default())
    }
    pub fn validate_formulas_with_budget(&self, budget: &mut FormulaBudget) -> Result<()> {
        let plan = self.plan_recalculation_mode(budget, true)?;
        if plan.updates.iter().any(|update| update.current) {
            return Err(invalid("formula cached outcome differs from recomputation"));
        }
        Ok(())
    }
    /// Assignment is staged and requires a successful numeric result for the new source.
    pub fn set_cell_formula(&mut self, row: usize, col: usize, expression: &str) -> Result<bool> {
        self.set_cell_formula_with_budget(row, col, expression, &mut FormulaBudget::default())
    }
    pub fn set_cell_formula_with_budget(&mut self, row: usize, col: usize, expression: &str, budget: &mut FormulaBudget) -> Result<bool> {
        let cell = self.cell(row, col).ok_or_else(|| invalid("formula cell coordinate outside table"))?;
        // Grid geometry may be merged, but the existing assignment profile
        // continues to require one visible unmerged target cell.
        if cell.is_covered() || cell.grid_span != 1 || cell.row_span != 1 {
            return Err(invalid("formula assignment requires an unmerged visible cell"));
        }
        if cell.formula.as_ref().is_some_and(|formula| formula.expression() == expression)
            && matches!(cell.formula_state()?, CellFormulaState::Current(_))
        {
            self.validate_formulas_with_budget(budget)?;
            return Ok(false);
        }
        let empty_target = cell.text.paragraphs.is_empty()
            || (cell.text.paragraphs.len() == 1 && cell.text.paragraphs.first().is_some_and(|paragraph| paragraph.runs.is_empty()));
        if !ordinary_display(&cell.text) && !empty_target {
            return Err(invalid("formula assignment requires ordinary display text"));
        }
        // Parse/bound source before copying the table or allocating attachment metadata.
        let parsed = super::formula::Parsed::parse(expression)?;
        budget.work(parsed.nodes.len())?;
        validate_formula_grid(self)?;
        // Stage only the target, rather than cloning an entire table's unrelated
        // rich displays. Every dependent update remains private in the plan.
        let original = cell.clone();
        let result = (|| {
            let target = self.cell_mut(row, col).ok_or_else(|| invalid("formula cell coordinate outside table"))?;
            if target.text.paragraphs.is_empty() {
                target.text.paragraphs.try_reserve_exact(1).map_err(|_| invalid("cannot allocate formula target paragraph"))?;
                target.text.paragraphs.push(Paragraph::default());
            }
            if let Some(paragraph) = target.text.paragraphs.first_mut()
                && paragraph.runs.is_empty()
            {
                paragraph.runs.try_reserve_exact(1).map_err(|_| invalid("cannot allocate formula target run"))?;
                paragraph.runs.push(Run::new(""));
            }
            let original_display = CellFormula::display_text(&target.text)?;
            let lexical = match Decimal::parse(&original_display) {
                Ok(_) => original_display,
                Err(_) => {
                    replace_display(&mut target.text, "0".into());
                    "0".into()
                }
            };
            target.data = None;
            target.formula = Some(Arc::new(CellFormula::number(expression.into(), lexical, &target.text)?));
            let plan = self.plan_recalculation_with_budget(budget)?;
            let projected = plan
                .updates
                .iter()
                .find(|update| update.row == row && update.col == col)
                .map(|update| update.formula.as_ref())
                .unwrap_or_else(|| self.cell(row, col).and_then(|cell| cell.formula.as_ref()));
            if !matches!(projected.map(|formula| formula.outcome()), Some(FormulaOutcome::Number { .. })) {
                return Err(invalid("formula assignment requires supported numeric predecessors and result"));
            }
            let changed = !plan.is_empty() || self.cell(row, col).is_some_and(|cell| cell != &original);
            plan.apply(self)?;
            Ok(changed)
        })();
        if result.is_err()
            && let Some(target) = self.cell_mut(row, col)
        {
            *target = original;
        }
        result
    }
}

fn replacement_body(original: &TextBody, replacement: &str) -> Result<TextBody> {
    // Current formula displays have bounded text and run counts, so this copy has
    // a fixed profile bound. Styling is preserved; replacement takes first-run style.
    if !ordinary_display(original) {
        return Err(invalid("formula display is unsupported"));
    }
    let mut body = original.clone();
    replace_display(&mut body, replacement.into());
    Ok(body)
}
fn find_entry(entries: &[Entry<'_>], row: usize, col: usize) -> Option<usize> {
    entries.binary_search_by_key(&(row, col), |entry| (entry.row, entry.col)).ok()
}
fn operand(table: &Table, row: usize, col: usize, entries: &[Entry<'_>], results: &[Option<Number>], authority: bool) -> Result<Number> {
    let cell = table.cell(row, col).ok_or_else(|| invalid("formula reference lies outside table grid"))?;
    if let Some(formula) = &cell.formula
        && !authority
        && !formula.matches_display(&cell.text)?
    {
        return Ok(Err(FormulaError::InputNotNumeric));
    }
    if let Some(index) = find_entry(entries, row, col) {
        return match results.get(index).copied().flatten() {
            Some(Ok(number)) => Ok(Ok(number)),
            Some(Err(_)) => Ok(Err(FormulaError::DependencyError)),
            None => Err(invalid("formula dependency evaluated out of order")),
        };
    }
    let state = if authority { cell.data.as_deref().map_or(CellConstantState::Absent, CellConstantState::Current) } else { cell.constant_state()? };
    match state {
        CellConstantState::Current(data) => match data.value() {
            CellValue::Number { kind: CellNumberKind::Float, lexical } => Ok(Decimal::parse(lexical).map_err(|_| FormulaError::NumericLimit)),
            _ => Ok(Err(FormulaError::InputNotNumeric)),
        },
        _ => Ok(Err(FormulaError::InputNotNumeric)),
    }
}
fn evaluate(
    table: &Table,
    formula: &CellFormula,
    entries: &[Entry<'_>],
    results: &[Option<Number>],
    budget: &mut FormulaBudget,
    authority: bool,
) -> Result<Number> {
    let mut values: Vec<Number> = Vec::new();
    values.try_reserve_exact(formula.parsed.nodes.len()).map_err(|_| invalid("cannot allocate formula evaluation scratch"))?;
    for node in &formula.parsed.nodes {
        budget.work(1)?;
        let get = |index: usize| values.get(index).copied().ok_or_else(|| invalid("invalid formula AST child"));
        let value = match *node {
            Node::Number(number) => Ok(number),
            Node::BrokenReference => Err(FormulaError::InvalidReference),
            Node::Reference(row, col) => operand(table, row, col, entries, results, authority)?,
            Node::Positive(child) => get(child)?,
            Node::Negative(child) => get(child)?.and_then(|number| number.neg().map_err(|_| FormulaError::NumericLimit)),
            Node::Add(a, b) | Node::Subtract(a, b) | Node::Multiply(a, b) => match (get(a)?, get(b)?) {
                (Ok(left), Ok(right)) => match node {
                    Node::Add(..) => left.add(right),
                    Node::Subtract(..) => left.sub(right),
                    _ => left.mul(right),
                }
                .map_err(|_| FormulaError::NumericLimit),
                (Err(error), _) | (_, Err(error)) => Err(error),
            },
        };
        values.push(value);
    }
    values.last().copied().ok_or_else(|| invalid("empty formula AST"))
}
fn validate_formula_grid(table: &Table) -> Result<()> {
    if table.rows.is_empty()
        || table.cols.is_empty()
        || table.rows.len() > 1000
        || table.cols.len() > 1000
        || table.rows.len().checked_mul(table.cols.len()).is_none_or(|count| count > 100_000)
        || table.rows.iter().any(|row| row.cells.len() != table.cols.len())
    {
        return Err(invalid("formulas require a bounded nonempty rectangular table grid"));
    }
    super::topology::validate_formula_grid_topology(table)
}
/// Low-level topology edits still require the checked reference-rewriting APIs.
pub fn validate_formula_topology(table: &Table) -> Result<()> {
    if table.has_formulas() {
        return Err(invalid("table formula reference rewriting is not supported for topology changes"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "table_merged_formula_tests.rs"]
mod merged_formula_tests;
