//! Constant metadata must belong to an owner that the native ODP writer emits.
use super::*;

fn has_data(shapes: &[Shape]) -> bool {
    let mut found = false;
    deckcraft_model::walk(shapes, &mut |shape, _| {
        if let ShapeKind::Table(table) = &shape.kind {
            found |= table.rows.iter().any(|row| {
                row.cells.iter().any(|cell| cell.data.is_some() || cell.formula.is_some() || cell.number_format.is_some() || !cell.blocks.is_empty())
            });
        }
    });
    found
}

fn refused(owner: &str) -> OdpError {
    OdpError(format!("stored table values on {owner} require additional ODP owner representation"))
}

/// Called after the all-owner model validation has bounded depth and total work.
pub(super) fn validate(p: &Presentation, plan: &masters_writer::MasterExportPlan<'_>) -> Result<()> {
    for master in &p.masters {
        if !plan.emits_master(master.id) && has_data(&master.shapes) {
            return Err(refused("unemitted regular masters"));
        }
        if master.layouts.iter().any(|layout| has_data(&layout.shapes)) {
            return Err(refused("regular layouts"));
        }
    }
    // Notes master/layout and handout value ownership is a separate mapping lane.
    // Refuse attached records even when stale rather than silently dropping owners.
    for master in p.notes_master.iter().chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|notes| &notes.master)) {
        if has_data(&master.shapes) || master.layouts.iter().any(|layout| has_data(&layout.shapes)) {
            return Err(refused("notes or handout masters/layouts"));
        }
    }
    for slide in &p.slides {
        let mut suppressed = false;
        deckcraft_model::walk(&slide.shapes, &mut |shape, _| {
            if shape.ph.is_some() && has_data(std::slice::from_ref(shape)) {
                suppressed = true;
            }
        });
        if suppressed {
            return Err(refused("slide placeholder bodies"));
        }
        if let Some(page) = &slide.notes_page {
            let mut suppressed = false;
            deckcraft_model::walk(&page.shapes, &mut |shape, _| {
                if (shape.ph.is_some() || page.body_shape == Some(shape.id)) && has_data(std::slice::from_ref(shape)) {
                    suppressed = true;
                }
            });
            if suppressed {
                return Err(refused("notes placeholder bodies"));
            }
        }
    }
    Ok(())
}
