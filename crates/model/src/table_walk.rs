//! Bounded validation and copy-on-write reconciliation of every table owner.

use std::sync::Arc;

use crate::{Master, ModelError, Presentation, Result, Shape, ShapeKind, Slide};

use super::{
    CellConstantState, CellFormulaState, FormulaBudget, Table, TableCalculationPlan,
    data::{MAX_DISPLAY_BYTES, MAX_DISPLAY_PARAGRAPHS, MAX_DISPLAY_RUNS},
};

const MAX_OWNERS: usize = 100_000;
const MAX_SHAPES: usize = 100_000;
const MAX_ROWS: usize = 1_000_000;
const MAX_CELLS: usize = 1_000_000;
const MAX_DEPTH: usize = 64;
const MAX_METADATA: usize = 32 * 1024 * 1024;
const MAX_DISPLAY_WORK: usize = 1_000_000;
const MAX_CURRENT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy)]
struct Limits {
    owners: usize,
    shapes: usize,
    rows: usize,
    cells: usize,
    depth: usize,
    metadata: usize,
    display_work: usize,
    current_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            owners: MAX_OWNERS,
            shapes: MAX_SHAPES,
            rows: MAX_ROWS,
            cells: MAX_CELLS,
            depth: MAX_DEPTH,
            metadata: MAX_METADATA,
            display_work: MAX_DISPLAY_WORK,
            current_bytes: MAX_CURRENT_BYTES,
        }
    }
}

#[derive(Default)]
struct Counts {
    owners: usize,
    shapes: usize,
    rows: usize,
    cells: usize,
    metadata: usize,
    display_work: usize,
    current_bytes: usize,
    blocks: usize,
    formulas: FormulaBudget,
    recalculate: bool,
    plans: Vec<TableCalculationPlan>,
}

fn charge_display_work(body: &crate::TextBody, counts: &mut Counts, limits: Limits) -> Result<()> {
    // Follow the same bounded early exits as CellConstant::matches_display. A collection
    // already known to be unsupported from its length needs no traversal or byte charge.
    if body.paragraphs.len() > MAX_DISPLAY_PARAGRAPHS {
        return Ok(());
    }
    charge(&mut counts.display_work, body.paragraphs.len(), limits.display_work, "table constant display comparison work exceeds limit")?;
    let mut runs = 0usize;
    let mut bytes = 0usize;
    for paragraph in &body.paragraphs {
        runs = runs.saturating_add(paragraph.runs.len());
        if runs > MAX_DISPLAY_RUNS {
            return Ok(());
        }
        charge(&mut counts.display_work, paragraph.runs.len(), limits.display_work, "table constant display comparison work exceeds limit")?;
        for run in &paragraph.runs {
            if !matches!(run.kind, crate::text::RunKind::Text | crate::text::RunKind::Break) {
                return Ok(());
            }
            let len = run.content().len();
            if len > MAX_DISPLAY_BYTES.saturating_sub(bytes) {
                return Ok(());
            }
            charge(&mut counts.current_bytes, len, limits.current_bytes, "table constant current display bytes exceed limit")?;
            bytes += len;
        }
    }
    Ok(())
}

fn invalid(message: &'static str) -> ModelError {
    ModelError::Invalid(message.into())
}

fn charge(total: &mut usize, add: usize, limit: usize, message: &'static str) -> Result<()> {
    let next = total.checked_add(add).filter(|next| *next <= limit).ok_or_else(|| invalid(message))?;
    *total = next;
    Ok(())
}

impl Counts {
    fn roots(&mut self, p: &Presentation, limits: Limits) -> Result<()> {
        // Charge attachments rather than deduplicating Arc identity: each owner is visited.
        for count in
            [p.slides.len(), p.masters.len(), p.notes_masters.len(), usize::from(p.notes_master.is_some()), usize::from(p.handout_master.is_some())]
        {
            charge(&mut self.owners, count, limits.owners, "table constant owner count exceeds limit")?;
        }
        Ok(())
    }
}

fn scan_table(table: &Table, counts: &mut Counts, limits: Limits) -> Result<bool> {
    if table.rows.iter().flat_map(|r| &r.cells).any(|c| !c.blocks.is_empty()) {
        table.validate_content()?;
    }
    scan_table_depth(table, counts, limits, 0)
}

fn scan_table_depth(table: &Table, counts: &mut Counts, limits: Limits, depth: usize) -> Result<bool> {
    if depth > super::MAX_NESTED_TABLE_DEPTH {
        return Err(invalid("nested-table depth exceeds limit"));
    }
    charge(&mut counts.rows, table.rows.len(), limits.rows, "table constant row count exceeds limit")?;
    let mut stale = false;
    let mut has_data = false;
    let mut has_formulas = false;
    let mut ragged = table.cols.is_empty();
    for row in &table.rows {
        ragged |= row.cells.len() != table.cols.len();
        charge(&mut counts.cells, row.cells.len(), limits.cells, "table constant cell count exceeds limit")?;
        for cell in &row.cells {
            if let Some(format) = &cell.number_format {
                charge(&mut counts.metadata, format.metadata_bytes()?, limits.metadata, "attached number-format metadata exceeds size limit")?;
                if let Some(data) = &cell.data {
                    let expected = format.format_value(data.value())?.text;
                    if !data.display_baseline().iter().map(String::as_str).eq(expected.split('\n')) {
                        return Err(invalid("formatted constant baseline differs from its stored value and format"));
                    }
                }
                if cell.formula.as_ref().is_some_and(|formula| !formula.uses_formatted_display()) {
                    return Err(invalid("number formatting requires explicit migration of a version-one formula"));
                }
            }
            charge(&mut counts.blocks, cell.blocks.len(), super::MAX_TABLE_CONTENT_BLOCKS, "table content block count exceeds limit")?;
            charge(
                &mut counts.metadata,
                cell.blocks.len().saturating_mul(std::mem::size_of::<super::CellBlock>()),
                limits.metadata,
                "table content metadata exceeds size limit",
            )?;
            for block in &cell.blocks {
                if let super::CellBlock::Text(block) = block {
                    charge(
                        &mut counts.metadata,
                        block.body.paragraphs.len().saturating_mul(std::mem::size_of::<crate::Paragraph>()),
                        limits.metadata,
                        "table content metadata exceeds size limit",
                    )?;
                    for paragraph in &block.body.paragraphs {
                        charge(
                            &mut counts.metadata,
                            paragraph.runs.len().saturating_mul(std::mem::size_of::<crate::Run>()),
                            limits.metadata,
                            "table content metadata exceeds size limit",
                        )?;
                        for run in &paragraph.runs {
                            charge(
                                &mut counts.metadata,
                                run.text.len().saturating_mul(2),
                                limits.metadata,
                                "table content metadata exceeds size limit",
                            )?;
                        }
                    }
                }
            }
            if cell.data.is_some() && cell.formula.is_some() {
                return Err(invalid("cell cannot contain both constant data and formula"));
            }
            if let Some(data) = &cell.data {
                has_data = true;
                if cell.is_covered() {
                    return Err(invalid("typed constants in covered table cells are not supported yet"));
                }
                charge(&mut counts.metadata, data.metadata_bytes(), limits.metadata, "attached table constant metadata exceeds size limit")?;
                charge_display_work(&cell.text, counts, limits)?;
                stale |= matches!(cell.constant_state()?, CellConstantState::Stale);
            }
            if let Some(formula) = &cell.formula {
                has_data = true;
                has_formulas = true;
                if cell.is_covered() {
                    return Err(invalid("formulas in covered table cells are not supported"));
                }
                charge(&mut counts.metadata, formula.metadata_bytes(), limits.metadata, "attached table formula metadata exceeds size limit")?;
                charge_display_work(&cell.text, counts, limits)?;
                stale |= matches!(cell.formula_state()?, CellFormulaState::Stale);
            }
        }
    }
    if has_data && ragged {
        return Err(invalid("typed constants require a nonempty rectangular table grid"));
    }
    if has_data {
        super::validate_constant_grid(table)?;
    }
    if counts.recalculate {
        let plan = if has_formulas { table.plan_recalculation_with_budget(&mut counts.formulas)? } else { TableCalculationPlan::default() };
        stale |= !plan.is_empty();
        charge(&mut counts.metadata, plan.additional_metadata(), limits.metadata, "recalculated table formula metadata exceeds size limit")?;
        counts.plans.try_reserve(1).map_err(|_| invalid("cannot allocate all-owner formula plans"))?;
        counts.plans.push(plan);
    } else if has_formulas {
        // Retained authoritative scalars/formula baselines validate the old graph even
        // when content is stale. A stale neighbour cannot hide a forged current cache.
        table.validate_formulas_with_budget(&mut counts.formulas)?;
    }
    for row in &table.rows {
        for cell in &row.cells {
            for block in &cell.blocks {
                if let super::CellBlock::Table(child) = block {
                    stale |= scan_table_depth(&child.table, counts, limits, depth + 1)?;
                }
            }
        }
    }
    Ok(stale)
}

fn scan_shapes(shapes: &[Shape], depth: usize, counts: &mut Counts, limits: Limits) -> Result<bool> {
    if depth > limits.depth {
        return Err(invalid("shape nesting exceeds table constant validation limit"));
    }
    charge(&mut counts.shapes, shapes.len(), limits.shapes, "table constant shape count exceeds limit")?;
    let mut stale = false;
    for shape in shapes {
        match &shape.kind {
            ShapeKind::Table(table) => stale |= scan_table(table, counts, limits)?,
            ShapeKind::Group { children, .. } if !children.is_empty() => stale |= scan_shapes(children, depth + 1, counts, limits)?,
            _ => {}
        }
    }
    Ok(stale)
}

fn scan_slide(slide: &Slide, counts: &mut Counts, limits: Limits) -> Result<bool> {
    if slide.notes_page.is_some() {
        charge(&mut counts.owners, 1, limits.owners, "table constant owner count exceeds limit")?;
    }
    let mut stale = scan_shapes(&slide.shapes, 0, counts, limits)?;
    if let Some(page) = &slide.notes_page {
        stale |= scan_shapes(&page.shapes, 0, counts, limits)?;
    }
    Ok(stale)
}

fn scan_master(master: &Master, counts: &mut Counts, limits: Limits) -> Result<bool> {
    charge(&mut counts.owners, master.layouts.len(), limits.owners, "table constant owner count exceeds limit")?;
    let mut stale = scan_shapes(&master.shapes, 0, counts, limits)?;
    for layout in &master.layouts {
        stale |= scan_shapes(&layout.shapes, 0, counts, limits)?;
    }
    Ok(stale)
}

fn validate_with_limits(p: &Presentation, limits: Limits) -> Result<usize> {
    validate_with_limits_and_budget(p, limits, &mut FormulaBudget::default())
}
fn validate_with_limits_and_budget(p: &Presentation, limits: Limits, budget: &mut FormulaBudget) -> Result<usize> {
    let mut counts = Counts { formulas: budget.clone(), ..Default::default() };
    let result: Result<usize> = (|| {
        counts.roots(p, limits)?;
        for slide in &p.slides {
            scan_slide(slide, &mut counts, limits)?;
        }
        for master in
            p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|notes| &notes.master))
        {
            scan_master(master, &mut counts, limits)?;
        }
        Ok(counts.metadata)
    })();
    *budget = counts.formulas;
    result
}
/// Validate every attached table record. Shared records and hidden owners are
/// counted for each attachment; all retained authority is checked before lowering.
pub fn validate_constants(p: &Presentation) -> Result<usize> {
    validate_with_limits(p, Limits::default())
}
pub fn validate_constants_with_budget(p: &Presentation, budget: &mut FormulaBudget) -> Result<usize> {
    validate_with_limits_and_budget(p, Limits::default(), budget)
}

// Only called after the complete immutable preflight. Plans are consumed in
// precisely the same owner/table order, including unchanged owners.
fn reconcile_shapes(shapes: &mut [Shape], plans: &mut std::vec::IntoIter<TableCalculationPlan>) -> Result<()> {
    for shape in shapes {
        match &mut shape.kind {
            ShapeKind::Table(table) => {
                reconcile_table(table, plans)?;
            }
            ShapeKind::Group { children, .. } => reconcile_shapes(children, plans)?,
            _ => {}
        }
    }
    Ok(())
}
fn reconcile_table(table: &mut Table, plans: &mut std::vec::IntoIter<TableCalculationPlan>) -> Result<()> {
    plans.next().ok_or_else(|| invalid("missing table recalculation plan"))?.apply(table)?;
    for row in &mut table.rows {
        for cell in &mut row.cells {
            cell.reconcile_constant()?;
            for block in &mut cell.blocks {
                if let super::CellBlock::Table(child) = block {
                    reconcile_table(&mut child.table, plans)?;
                }
            }
        }
    }
    Ok(())
}
fn skip_table_plans(table: &Table, plans: &mut std::vec::IntoIter<TableCalculationPlan>) {
    plans.next();
    for block in table.rows.iter().flat_map(|r| &r.cells).flat_map(|c| &c.blocks) {
        if let super::CellBlock::Table(child) = block {
            skip_table_plans(&child.table, plans);
        }
    }
}
fn skip_plans(shapes: &[Shape], plans: &mut std::vec::IntoIter<TableCalculationPlan>) {
    for shape in shapes {
        match &shape.kind {
            ShapeKind::Table(table) => skip_table_plans(table, plans),
            ShapeKind::Group { children, .. } => skip_plans(children, plans),
            _ => {}
        }
    }
}
fn reconcile_with_limits(p: &mut Presentation, limits: Limits) -> Result<bool> {
    reconcile_with_limits_and_budget(p, limits, &mut FormulaBudget::default())
}
fn reconcile_with_limits_and_budget(p: &mut Presentation, limits: Limits, budget: &mut FormulaBudget) -> Result<bool> {
    let mut counts = Counts { formulas: budget.clone(), recalculate: true, ..Default::default() };
    let preflight: Result<Vec<bool>> = (|| {
        counts.roots(p, limits)?;
        let mut changed_owners = Vec::new();
        changed_owners.try_reserve_exact(counts.owners).map_err(|_| invalid("cannot allocate table owner plans"))?;
        for slide in &p.slides {
            changed_owners.push(scan_slide(slide, &mut counts, limits)?);
        }
        for master in
            p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|notes| &notes.master))
        {
            changed_owners.push(scan_master(master, &mut counts, limits)?);
        }
        Ok(changed_owners)
    })();
    *budget = counts.formulas;
    let changed_owners: Vec<bool> = preflight?;
    let changed = changed_owners.iter().any(|changed| *changed);
    let mut flags = changed_owners.into_iter();
    let mut plans = counts.plans.into_iter();
    for slide in &mut p.slides {
        if flags.next() == Some(true) {
            let slide = Arc::make_mut(slide);
            reconcile_shapes(&mut slide.shapes, &mut plans)?;
            if let Some(page) = &mut slide.notes_page {
                reconcile_shapes(&mut page.shapes, &mut plans)?;
            }
        } else {
            skip_plans(&slide.shapes, &mut plans);
            if let Some(page) = &slide.notes_page {
                skip_plans(&page.shapes, &mut plans);
            }
        }
    }
    for master in p
        .masters
        .iter_mut()
        .chain(p.notes_master.iter_mut())
        .chain(p.handout_master.iter_mut())
        .chain(p.notes_masters.iter_mut().map(|notes| &mut notes.master))
    {
        if flags.next() == Some(true) {
            let master = Arc::make_mut(master);
            reconcile_shapes(&mut master.shapes, &mut plans)?;
            for layout in &mut master.layouts {
                reconcile_shapes(&mut layout.shapes, &mut plans)?;
            }
        } else {
            skip_plans(&master.shapes, &mut plans);
            for layout in &master.layouts {
                skip_plans(&layout.shapes, &mut plans);
            }
        }
    }
    Ok(changed)
}

/// Preflight the whole document, clear stale records and recalculate current formulas.
/// Native readers/writers must call `validate_constants` first to reject forged caches.
/// Unchanged owners and numerically equal formula outcomes preserve Arc identity.
pub fn reconcile_constants(p: &mut Presentation) -> Result<bool> {
    reconcile_with_limits(p, Limits::default())
}
pub fn reconcile_constants_with_budget(p: &mut Presentation, budget: &mut FormulaBudget) -> Result<bool> {
    reconcile_with_limits_and_budget(p, Limits::default(), budget)
}

/// Interim core export guard until editable typed-constant mappings are implemented.
/// This deliberately refuses stale attachments too; callers must not silently drop data.
pub fn reject_constants(p: &Presentation) -> Result<()> {
    if validate_constants(p)? > 0 {
        return Err(invalid("export cannot yet preserve typed table constants or formulas"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::{
        Cell, Layout, TextBody, Xfrm,
        notes::{NotesMaster, NotesPage},
        table::{CellConstant, CellValue, Row},
    };

    fn empty() -> Presentation {
        let mut p = Presentation::default();
        p.slides.clear();
        p.masters.clear();
        p.notes_master = None;
        p.handout_master = None;
        p.notes_masters.clear();
        p
    }
    fn data() -> Arc<CellConstant> {
        Arc::new(CellConstant::from_display(CellValue::Boolean { value: false }, &TextBody::from_text("before")).unwrap())
    }
    fn cell(data: &Arc<CellConstant>, stale: bool) -> Cell {
        Cell { data: Some(data.clone()), ..Cell::new(if stale { "after" } else { "before" }) }
    }
    fn table_shape(cells: Vec<Cell>) -> Shape {
        Shape {
            kind: ShapeKind::Table(Table { cols: vec![10.; cells.len()], rows: vec![Row { height: 10., cells }], ..Default::default() }),
            ..Default::default()
        }
    }
    fn group(shape: Shape) -> Shape {
        Shape { kind: ShapeKind::Group { children: vec![shape], child: Xfrm::new(0., 0., 10., 10.) }, ..Default::default() }
    }
    fn slide(shapes: Vec<Shape>) -> Arc<Slide> {
        Arc::new(Slide { shapes, hidden: true, show_master_shapes: false, ..Default::default() })
    }
    fn master(shapes: Vec<Shape>, layouts: Vec<Layout>) -> Arc<Master> {
        Arc::new(Master { shapes, layouts, ..Default::default() })
    }
    fn layout(shapes: Vec<Shape>) -> Layout {
        Layout { shapes, show_master_shapes: false, ..Default::default() }
    }
    fn at_owner(p: &mut Presentation, owner: usize, shape: Shape) {
        match owner {
            0 => p.slides.push(slide(vec![shape])),
            1 => p.slides.push(slide(vec![group(shape)])),
            2 => p.masters.push(master(vec![shape], vec![])),
            3 => p.masters.push(master(vec![], vec![layout(vec![shape])])),
            4 => p.notes_master = Some(master(vec![shape], vec![])),
            5 => p.notes_master = Some(master(vec![], vec![layout(vec![shape])])),
            6 => p.notes_masters.push(NotesMaster { master: master(vec![shape], vec![]), ..Default::default() }),
            7 => p.notes_masters.push(NotesMaster { master: master(vec![], vec![layout(vec![shape])]), ..Default::default() }),
            8 => p.handout_master = Some(master(vec![shape], vec![])),
            9 => p.handout_master = Some(master(vec![], vec![layout(vec![shape])])),
            10 | 11 => p.slides.push(Arc::new(Slide {
                notes_page: Some(NotesPage {
                    shapes: vec![if owner == 11 { group(shape) } else { shape }],
                    show_master_shapes: false,
                    ..Default::default()
                }),
                ..Default::default()
            })),
            _ => {
                p.masters.push(master(vec![], vec![]));
                p.masters.push(master(vec![], vec![layout(vec![]), layout(vec![group(shape)])]));
            }
        }
    }
    fn first_table(shapes: &[Shape]) -> &Table {
        let shape = &shapes[0];
        match &shape.kind {
            ShapeKind::Table(t) => t,
            ShapeKind::Group { children, .. } => first_table(children),
            _ => panic!("table missing"),
        }
    }

    #[test]
    fn empty_and_ordinary_documents_need_no_metadata_or_owner_detachment() {
        let mut p = empty();
        assert_eq!(validate_constants(&p).unwrap(), 0);
        assert!(!reconcile_constants(&mut p).unwrap());
        reject_constants(&p).unwrap();
        p.slides.push(slide(vec![table_shape(vec![Cell::new("ordinary"); 3])]));
        let before = p.slides[0].clone();
        assert_eq!(validate_constants(&p).unwrap(), 0);
        assert!(!reconcile_constants(&mut p).unwrap());
        assert!(Arc::ptr_eq(&before, &p.slides[0]));
    }

    #[test]
    fn every_hidden_and_unreferenced_owner_is_validated_reconciled_and_export_guarded() {
        for owner in 0..13 {
            let data = data();
            let mut p = empty();
            at_owner(&mut p, owner, table_shape(vec![cell(&data, true)]));
            assert_eq!(validate_constants(&p).unwrap(), data.metadata_bytes(), "owner {owner}");
            assert!(reject_constants(&p).unwrap_err().to_string().contains("export cannot yet preserve typed table constants or formulas"));
            assert!(reconcile_constants(&mut p).unwrap(), "owner {owner}");
            assert_eq!(validate_constants(&p).unwrap(), 0, "owner {owner}");
            assert!(!reconcile_constants(&mut p).unwrap());
            reject_constants(&p).unwrap();
        }
    }

    #[test]
    fn current_constants_keep_record_and_owner_arc_identity() {
        let data = data();
        let mut p = empty();
        p.slides.push(slide(vec![table_shape(vec![cell(&data, false)])]));
        let original = p.slides[0].clone();
        assert_eq!(validate_constants(&p).unwrap(), data.metadata_bytes());
        assert!(!reconcile_constants(&mut p).unwrap());
        assert!(Arc::ptr_eq(&p.slides[0], &original));
        assert!(Arc::ptr_eq(first_table(&p.slides[0].shapes).rows[0].cells[0].data.as_ref().unwrap(), &data));
        assert!(reject_constants(&p).is_err());
    }

    #[test]
    fn only_changed_slide_or_master_owners_detach_and_current_records_survive() {
        let data = data();
        let current = table_shape(vec![cell(&data, false)]);
        let stale = table_shape(vec![cell(&data, true)]);
        let mut p = empty();
        p.slides = vec![slide(vec![current.clone()]), slide(vec![current.clone(), group(stale.clone())])];
        p.masters = vec![master(vec![current.clone()], vec![])];
        p.notes_master = Some(master(vec![current.clone()], vec![]));
        p.handout_master = Some(master(vec![current], vec![]));
        p.notes_masters.push(NotesMaster { master: master(vec![], vec![layout(vec![stale])]), ..Default::default() });
        let old_slides = p.slides.clone();
        let old_master = p.masters[0].clone();
        let old_notes = p.notes_master.as_ref().unwrap().clone();
        let old_handout = p.handout_master.as_ref().unwrap().clone();
        let old_imported = p.notes_masters[0].master.clone();
        assert!(reconcile_constants(&mut p).unwrap());
        assert!(Arc::ptr_eq(&p.slides[0], &old_slides[0]));
        assert!(!Arc::ptr_eq(&p.slides[1], &old_slides[1]));
        assert!(Arc::ptr_eq(&p.masters[0], &old_master));
        assert!(Arc::ptr_eq(p.notes_master.as_ref().unwrap(), &old_notes));
        assert!(Arc::ptr_eq(p.handout_master.as_ref().unwrap(), &old_handout));
        assert!(!Arc::ptr_eq(&p.notes_masters[0].master, &old_imported));
        assert!(Arc::ptr_eq(first_table(&p.slides[1].shapes).rows[0].cells[0].data.as_ref().unwrap(), &data));
        assert!(first_table(&old_imported.layouts[0].shapes).rows[0].cells[0].data.is_some());
        assert!(first_table(&p.notes_masters[0].master.layouts[0].shapes).rows[0].cells[0].data.is_none());
    }

    #[test]
    fn local_notes_page_only_change_detaches_its_slide_but_not_other_owners() {
        let data = data();
        let mut p = empty();
        p.slides.push(Arc::new(Slide {
            notes_page: Some(NotesPage { shapes: vec![group(table_shape(vec![cell(&data, true)]))], ..Default::default() }),
            ..Default::default()
        }));
        p.slides.push(slide(vec![]));
        let before = p.slides.clone();
        assert!(reconcile_constants(&mut p).unwrap());
        assert!(!Arc::ptr_eq(&p.slides[0], &before[0]));
        assert!(Arc::ptr_eq(&p.slides[1], &before[1]));
    }

    #[test]
    fn repeated_record_and_master_attachments_are_charged_individually() {
        let data = data();
        let mut p = empty();
        let owner = master(vec![table_shape(vec![cell(&data, true); 3])], vec![]);
        p.masters.push(owner.clone());
        p.notes_master = Some(owner.clone());
        p.handout_master = Some(owner.clone());
        assert_eq!(validate_constants(&p).unwrap(), 9 * data.metadata_bytes());
        assert!(reconcile_constants(&mut p).unwrap());
        assert_eq!(validate_constants(&p).unwrap(), 0);
        assert!(first_table(&owner.shapes).rows[0].cells[0].data.is_some());
    }

    #[test]
    fn covered_constant_refusal_preflights_all_owners_before_any_detachment() {
        let data = data();
        let mut p = empty();
        p.slides.push(slide(vec![table_shape(vec![cell(&data, true)])]));
        let mut covered = cell(&data, false);
        covered.h_merge = true;
        p.handout_master = Some(master(vec![], vec![layout(vec![table_shape(vec![covered])])]));
        let before_slide = p.slides[0].clone();
        let before_master = p.handout_master.as_ref().unwrap().clone();
        let error = reconcile_constants(&mut p).unwrap_err().to_string();
        assert!(error.contains("covered table cells"));
        assert!(Arc::ptr_eq(&p.slides[0], &before_slide));
        assert!(Arc::ptr_eq(p.handout_master.as_ref().unwrap(), &before_master));
        assert!(first_table(&p.slides[0].shapes).rows[0].cells[0].data.is_some());
    }

    #[test]
    fn both_covered_flags_refuse_even_when_the_attached_display_is_stale() {
        for horizontal in [false, true] {
            let data = data();
            let mut item = cell(&data, true);
            item.h_merge = horizontal;
            item.v_merge = !horizontal;
            let mut p = empty();
            p.slides.push(slide(vec![table_shape(vec![item])]));
            assert!(validate_constants(&p).unwrap_err().to_string().contains("covered table cells"));
        }
    }

    #[test]
    fn group_depth_boundary_is_checked_even_without_constants() {
        for depth in [0, MAX_DEPTH, MAX_DEPTH + 1] {
            let mut item = table_shape(vec![Cell::new("")]);
            for _ in 0..depth {
                item = group(item);
            }
            let mut p = empty();
            p.slides.push(slide(vec![item]));
            if depth > MAX_DEPTH {
                assert!(validate_constants(&p).unwrap_err().to_string().contains("nesting"));
            } else {
                assert_eq!(validate_constants(&p).unwrap(), 0);
            }
        }
    }

    #[test]
    fn exact_shape_cell_row_and_owner_limits_use_prechecked_collection_counts() {
        let mut p = empty();
        p.slides.push(slide(vec![Shape::default(); 3]));
        assert!(validate_with_limits(&p, Limits { shapes: 3, ..Default::default() }).is_ok());
        assert!(validate_with_limits(&p, Limits { shapes: 2, ..Default::default() }).unwrap_err().to_string().contains("shape count"));
        let mut p = empty();
        p.slides.push(slide(vec![table_shape(vec![Cell::new(""); 3])]));
        assert!(validate_with_limits(&p, Limits { cells: 3, ..Default::default() }).is_ok());
        assert!(validate_with_limits(&p, Limits { cells: 2, ..Default::default() }).unwrap_err().to_string().contains("cell count"));
        let mut p = empty();
        p.slides
            .push(slide(vec![Shape { kind: ShapeKind::Table(Table { rows: vec![Row::default(); 3], ..Default::default() }), ..Default::default() }]));
        assert!(validate_with_limits(&p, Limits { rows: 3, ..Default::default() }).is_ok());
        assert!(validate_with_limits(&p, Limits { rows: 2, ..Default::default() }).unwrap_err().to_string().contains("row count"));
        let mut p = empty();
        p.masters.push(master(vec![], vec![layout(vec![]); 2]));
        assert!(validate_with_limits(&p, Limits { owners: 3, ..Default::default() }).is_ok());
        assert!(validate_with_limits(&p, Limits { owners: 2, ..Default::default() }).unwrap_err().to_string().contains("owner count"));
        assert_eq!((MAX_SHAPES, MAX_CELLS, MAX_ROWS, MAX_OWNERS), (100_000, 1_000_000, 1_000_000, 100_000));
    }

    #[test]
    fn cumulative_cell_row_and_shape_counts_do_not_reset_between_owners() {
        let mut p = empty();
        p.slides.push(slide(vec![table_shape(vec![Cell::new(""); 2])]));
        p.notes_master = Some(master(vec![], vec![layout(vec![table_shape(vec![Cell::new(""); 2])])]));
        assert!(validate_with_limits(&p, Limits { cells: 3, ..Default::default() }).unwrap_err().to_string().contains("cell count"));
        assert!(validate_with_limits(&p, Limits { rows: 1, ..Default::default() }).unwrap_err().to_string().contains("row count"));
        assert!(validate_with_limits(&p, Limits { shapes: 1, ..Default::default() }).unwrap_err().to_string().contains("shape count"));
    }

    #[test]
    fn metadata_limit_and_atomicity_count_shared_records_per_attachment() {
        let data = data();
        let mut p = empty();
        p.slides.push(slide(vec![table_shape(vec![cell(&data, true); 2])]));
        let exact = 2 * data.metadata_bytes();
        assert_eq!(validate_with_limits(&p, Limits { metadata: exact, ..Default::default() }).unwrap(), exact);
        let before = p.slides[0].clone();
        assert!(reconcile_with_limits(&mut p, Limits { metadata: exact - 1, ..Default::default() }).unwrap_err().to_string().contains("metadata"));
        assert!(Arc::ptr_eq(&p.slides[0], &before));
        assert!(first_table(&p.slides[0].shapes).rows[0].cells.iter().all(|cell| cell.data.is_some()));
    }

    #[test]
    fn production_metadata_and_empty_owner_limits_are_exercised_without_giant_payloads() {
        let body = TextBody::from_text("");
        let data = Arc::new(CellConstant::from_display(CellValue::String { value: "x".repeat(64 * 1024) }, &body).unwrap());
        let count = MAX_METADATA / data.metadata_bytes() + 1;
        let item = Cell { text: body, data: Some(data), ..Cell::new("") };
        let mut p = empty();
        p.slides.push(slide(vec![table_shape(vec![item; count])]));
        assert!(validate_constants(&p).unwrap_err().to_string().contains("metadata"));
        let mut p = empty();
        p.slides = vec![slide(vec![]); MAX_OWNERS];
        assert_eq!(validate_constants(&p).unwrap(), 0);
        p.slides.push(slide(vec![]));
        assert!(validate_constants(&p).unwrap_err().to_string().contains("owner count"));
    }

    #[test]
    fn typed_constants_refuse_grids_that_native_repair_could_truncate_or_reassign() {
        let data = data();
        for (cols, rows) in [
            (vec![], vec![Row { height: 10., cells: vec![cell(&data, true)] }]),
            (vec![10.], vec![Row { height: 10., cells: vec![Cell::new("plain"), cell(&data, true)] }]),
            (vec![10., 10.], vec![Row { height: 10., cells: vec![cell(&data, false)] }]),
            (vec![10.], vec![Row::default(), Row { height: 10., cells: vec![cell(&data, false)] }]),
        ] {
            let mut p = empty();
            p.slides.push(slide(vec![Shape { kind: ShapeKind::Table(Table { cols, rows, ..Default::default() }), ..Default::default() }]));
            let before = p.slides[0].clone();
            assert!(reconcile_constants(&mut p).unwrap_err().to_string().contains("nonempty rectangular table grid"));
            assert!(Arc::ptr_eq(&p.slides[0], &before));
        }
    }

    #[test]
    fn ordinary_ragged_and_empty_column_tables_remain_legacy_repairable() {
        let mut p = empty();
        for cols in [vec![], vec![10.], vec![10., 10.]] {
            p.slides.push(slide(vec![Shape {
                kind: ShapeKind::Table(Table {
                    cols,
                    rows: vec![Row::default(), Row { height: 10., cells: vec![Cell::new("one"), Cell::new("two"), Cell::new("three")] }],
                    ..Default::default()
                }),
                ..Default::default()
            }]));
        }
        assert_eq!(validate_constants(&p).unwrap(), 0);
        assert!(!reconcile_constants(&mut p).unwrap());
        reject_constants(&p).unwrap();
    }

    #[test]
    fn unsupported_native_metadata_is_rejected_before_owner_attachment() {
        let invalid = r#"{"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"NaN"},"display":["shown"]}"#;
        assert!(serde_json::from_str::<CellConstant>(invalid).is_err());
        let invalid = r#"{"value":{"kind":"boolean","value":false},"display":["shown"],"trusted":true}"#;
        assert!(serde_json::from_str::<CellConstant>(invalid).is_err());
    }

    #[test]
    fn checked_budget_overflow_does_not_wrap_or_modify_the_counter() {
        let mut total = usize::MAX;
        assert!(charge(&mut total, 1, usize::MAX, "test budget overflow").is_err());
        assert_eq!(total, usize::MAX);
    }

    #[test]
    fn aggregate_comparison_caps_count_repeated_owner_attachments_before_scanning() {
        use crate::text::{Paragraph, Run};
        let text =
            TextBody { paragraphs: vec![Paragraph { runs: vec![Run::new(""); MAX_DISPLAY_RUNS], ..Default::default() }], ..Default::default() };
        let data = Arc::new(CellConstant::from_display(CellValue::Boolean { value: false }, &TextBody::from_text("")).unwrap());
        let item = Cell { text, data: Some(data), ..Cell::new("") };
        let owner = master(vec![table_shape(vec![item])], vec![]);
        let mut p = empty();
        p.masters = vec![owner; 12];
        assert!(validate_constants(&p).unwrap_err().to_string().contains("comparison work"));
    }

    #[test]
    fn current_display_byte_work_limit_is_separate_from_metadata_and_preflight_atomic() {
        let data = data();
        let mut p = empty();
        p.slides.push(slide(vec![table_shape(vec![cell(&data, true); 2])]));
        let before = p.slides[0].clone();
        assert!(
            reconcile_with_limits(&mut p, Limits { current_bytes: 9, ..Default::default() })
                .unwrap_err()
                .to_string()
                .contains("current display bytes")
        );
        assert!(Arc::ptr_eq(&p.slides[0], &before));
        assert_eq!(validate_with_limits(&p, Limits { current_bytes: 10, ..Default::default() }).unwrap(), 2 * data.metadata_bytes());
    }

    #[test]
    fn oversized_current_collections_are_stale_without_walking_their_contents() {
        let data = data();
        let text = TextBody { paragraphs: vec![crate::Paragraph::default(); MAX_DISPLAY_PARAGRAPHS + 1], ..Default::default() };
        let mut p = empty();
        p.slides.push(slide(vec![table_shape(vec![Cell { text, data: Some(data), ..Cell::new("") }])]));
        assert!(reconcile_with_limits(&mut p, Limits { display_work: 0, current_bytes: 0, ..Default::default() }).unwrap());
        assert_eq!(validate_constants(&p).unwrap(), 0);
    }
}
