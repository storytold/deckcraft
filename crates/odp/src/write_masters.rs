//! Validated regular-master owners; notes coupling must not split live shared identities.
use super::*;
use std::collections::BTreeSet;

fn invalid(reason: &str) -> OdpError {
    OdpError(format!("regular master export refused: {reason}"))
}
fn shape_ids(shapes: &[Shape]) -> Result<()> {
    fn add(shapes: &[Shape], seen: &mut BTreeSet<ShapeId>, count: &mut usize, depth: usize) -> Result<()> {
        if depth >= 64 {
            return Err(invalid("excessive shape nesting"));
        }
        for shape in shapes {
            *count = count.saturating_add(1);
            if *count > 100_000 || !seen.insert(shape.id) {
                return Err(invalid("duplicate or excessive owner shape identities"));
            }
            if let ShapeKind::Group { children, .. } = &shape.kind {
                add(children, seen, count, depth + 1)?;
            }
        }
        Ok(())
    }
    add(shapes, &mut BTreeSet::new(), &mut 0, 0)
}
fn has_art(shapes: &[Shape]) -> bool {
    shapes.iter().any(|shape| shape.ph.is_none())
}

fn comparison_charge(budget: &mut usize, bytes: usize) -> Result<()> {
    *budget = budget.saturating_add(bytes);
    if *budget as u64 > crate::MAX_PART {
        return Err(invalid("shared background color comparison exceeds export budget"));
    }
    Ok(())
}
fn color_charge(color: &ColorRef, budget: &mut usize) -> Result<()> {
    let name = match &color.base {
        ColorBase::Preset { name } | ColorBase::System { name, .. } => name.len(),
        _ => 0,
    };
    comparison_charge(
        budget,
        2usize.saturating_mul(
            std::mem::size_of::<ColorRef>()
                .saturating_add(name)
                .saturating_add(color.mods.len().saturating_mul(std::mem::size_of::<ColorTransform>())),
        ),
    )
}
fn context_charge(master: &Ctx<'_>, slide: &Ctx<'_>, budget: &mut usize) -> Result<()> {
    // Color resolution searches each context's mapping. Charge that repeated work,
    // not only the temporary ColorRef clones, before visiting gradient stops.
    comparison_charge(budget, master.color_map.len().saturating_add(slide.color_map.len()).saturating_mul(64))
}
fn same_color(
    color: &ColorRef,
    master: &Ctx<'_>,
    slide: &Ctx<'_>,
    master_ph: Option<Rgba>,
    slide_ph: Option<Rgba>,
    budget: &mut usize,
) -> Result<bool> {
    color_charge(color, budget)?;
    context_charge(master, slide, budget)?;
    Ok(master.color(color, master_ph) == slide.color(color, slide_ph))
}
fn same_fill(fill: &Fill, master: &Ctx<'_>, slide: &Ctx<'_>, master_ph: Option<Rgba>, slide_ph: Option<Rgba>, budget: &mut usize) -> Result<bool> {
    match fill {
        Fill::Solid { color } => same_color(color, master, slide, master_ph, slide_ph, budget),
        Fill::Gradient(gradient) => {
            if gradient.stops.len() > 4096 {
                return Err(invalid("gradient has invalid stop count"));
            }
            for stop in &gradient.stops {
                if !same_color(&stop.color, master, slide, master_ph, slide_ph, budget)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Fill::Pattern(pattern) => Ok(same_color(&pattern.fg, master, slide, master_ph, slide_ph, budget)?
            && same_color(&pattern.bg, master, slide, master_ph, slide_ph, budget)?),
        Fill::Picture(picture) => match &picture.adjust.duotone {
            Some((dark, light)) => {
                Ok(same_color(dark, master, slide, master_ph, slide_ph, budget)? && same_color(light, master, slide, master_ph, slide_ph, budget)?)
            }
            None => Ok(true),
        },
        // These have no color-map-dependent paint; unresolved fills retain the writer's
        // ordinary refusal rather than manufacturing a color-map ownership difference.
        Fill::None | Fill::Group | Fill::Background => Ok(true),
    }
}
fn unchanged_inherited_paint(pres: &Presentation, master: &Master, layout: &Layout, slide: &Slide, budget: &mut usize) -> Result<bool> {
    if slide.background.is_some() || layout.background.is_some() {
        return Ok(true);
    }
    if slide.color_map.is_none() && layout.color_map.is_none() {
        return Ok(true);
    }
    let selected = slide.color_map.as_ref().or(layout.color_map.as_ref()).unwrap_or(&master.color_map);
    let bytes = selected.iter().chain(&master.color_map).fold(master.theme.colors.name.len().saturating_mul(4), |size, (key, value)| {
        size.saturating_add(64).saturating_add(key.len()).saturating_add(value.len())
    });
    comparison_charge(budget, bytes)?;
    if selected == &master.color_map {
        return Ok(true);
    }
    let master_ctx = Ctx::for_master(pres, master);
    let slide_ctx = Ctx::for_slide(pres, slide).ok_or_else(|| invalid("missing shared paint context"))?;
    match &master.background {
        Some(Background::Fill { fill }) => same_fill(fill, &master_ctx, &slide_ctx, None, None, budget),
        Some(Background::Ref { idx, color }) => {
            let fill = if *idx >= 1001 {
                master.theme.format.bg_fills.get((*idx - 1001) as usize)
            } else {
                master.theme.format.fills.get(idx.saturating_sub(1) as usize)
            };
            match fill {
                Some(fill) => {
                    color_charge(color, budget)?;
                    context_charge(&master_ctx, &slide_ctx, budget)?;
                    let master_ph = master_ctx.color(color, None);
                    let slide_ph = slide_ctx.color(color, None);
                    same_fill(fill, &master_ctx, &slide_ctx, Some(master_ph), Some(slide_ph), budget)
                }
                None => same_color(color, &master_ctx, &slide_ctx, None, None, budget),
            }
        }
        None => same_color(&ColorRef::scheme(SchemeSlot::Bg1), &master_ctx, &slide_ctx, None, None, budget),
    }
}

pub(super) struct MasterExportPlan<'a> {
    pres: &'a Presentation,
    names: BTreeMap<SlideId, String>,
    shared: BTreeSet<MasterId>,
    associations: BTreeMap<MasterId, Option<MasterId>>,
}
impl<'a> MasterExportPlan<'a> {
    pub(super) fn build(pres: &'a Presentation) -> Result<Self> {
        if pres.masters.len() > 10_000 || pres.notes_masters.len() > 10_000 {
            return Err(invalid("master inventory exceeds limit"));
        }
        let mut master_ids = BTreeSet::new();
        let mut layouts = BTreeMap::new();
        for master in &pres.masters {
            if !master_ids.insert(master.id) {
                return Err(invalid("duplicate regular master identity"));
            }
            shape_ids(&master.shapes)?;
            for layout in &master.layouts {
                if layouts.len() >= 10_000 || layouts.insert(layout.id, master.as_ref()).is_some() {
                    return Err(invalid("duplicate or excessive layout identities"));
                }
                shape_ids(&layout.shapes)?;
            }
        }
        let mut notes_ids = BTreeSet::new();
        for notes in &pres.notes_masters {
            if !notes_ids.insert(notes.master.id) {
                return Err(invalid("duplicate notes master identity"));
            }
        }
        if let Some(master) = &pres.notes_master
            && !notes_ids.insert(master.id)
        {
            return Err(invalid("duplicate notes master identity"));
        }
        let mut names = BTreeMap::new();
        let mut shared = BTreeSet::new();
        let mut associations = BTreeMap::new();
        let mut notes_owners = BTreeMap::new();
        let mut legacy_notes = BTreeSet::new();
        let mut paint_comparisons = 0usize;
        for slide in &pres.slides {
            shape_ids(&slide.shapes)?;
            let master = layouts.get(&slide.layout).copied().ok_or_else(|| invalid("slide has a dangling layout relationship"))?;
            let note = slide.notes_page.as_ref().and_then(|page| page.master).or_else(|| pres.notes_master.as_ref().map(|master| master.id));
            if note.is_some_and(|id| !notes_ids.contains(&id)) {
                return Err(invalid("missing linked notes master"));
            }
            // Native factory masters have theme-reference paint and empty prompt shapes;
            // preserve that existing notes-only compatibility lane. Concrete paint is owned.
            let share = master.preserve || has_art(&master.shapes) || matches!(master.background, Some(Background::Fill { .. }));
            let name = if share {
                let layout = master.layout(slide.layout).ok_or_else(|| invalid("missing exact shared layout"))?;
                if !unchanged_inherited_paint(pres, master, layout, slide, &mut paint_comparisons)? {
                    return Err(invalid(
                        "page color-map changes inherited shared master paint; editable theme remaps require additional representation",
                    ));
                }
                let mut placeholder = false;
                deckcraft_model::walk(&master.shapes, &mut |shape, _| placeholder |= shape.ph.is_some());
                if placeholder {
                    return Err(invalid("shared regular master placeholder ownership requires R05.2"));
                }
                shared.insert(master.id);
                if let Some(previous) = associations.insert(master.id, note)
                    && previous != note
                {
                    return Err(invalid("one shared regular master is paired with multiple notes masters"));
                }
                if let Some(note) = note
                    && notes_owners.insert(note, master.id).is_some_and(|owner| owner != master.id)
                {
                    return Err(invalid("one shared notes master is paired with multiple regular masters"));
                }
                format!("RegularMaster{}", master.id.0)
            } else {
                if let Some(note) = note {
                    legacy_notes.insert(note);
                }
                match slide.notes_page.as_ref().and_then(|page| page.master) {
                    Some(id) => format!("NotesMaster{}", id.0),
                    None => "Default".into(),
                }
            };
            if names.insert(slide.id, name).is_some() {
                return Err(invalid("duplicate slide identity"));
            }
        }
        if legacy_notes.iter().any(|id| notes_owners.contains_key(id)) {
            return Err(invalid("shared notes identity also has an incompatible legacy master owner"));
        }
        Ok(Self { pres, names, shared, associations })
    }
    pub(super) fn master_name(&self, slide: &Slide) -> Result<&str> {
        self.names.get(&slide.id).map(String::as_str).ok_or_else(|| invalid("missing validated page owner"))
    }
    pub(super) fn shares_master(&self, slide: &Slide) -> bool {
        self.pres.layout(slide.layout).is_some_and(|(master, _)| self.shared.contains(&master.id))
    }
    pub(super) fn emits_master(&self, id: MasterId) -> bool {
        self.shared.contains(&id)
    }
    pub(super) fn emit(&self, writer: &mut Writer<'a>, out: &mut String) -> Result<()> {
        let mut emitted_notes = BTreeSet::new();
        for master in self.pres.masters.iter().filter(|master| self.shared.contains(&master.id)) {
            let ctx = Ctx::for_master(self.pres, master);
            let (fill, ph) = resolve::background(&ctx, None);
            let paint = writer.fill_attrs(&ctx, Some(&fill), ph, (self.pres.slide_size.width, self.pres.slide_size.height))?;
            let style = writer.style("drawing-page", &format!("<style:drawing-page-properties {paint}/>"));
            out.push_str(&format!("<style:master-page style:name=\"RegularMaster{}\" style:display-name=\"{}\" style:page-layout-name=\"Page\" draw:style-name=\"{style}\">", master.id.0, esc(&master.name)));
            out.push_str("<draw:layer-set><draw:layer draw:name=\"Hidden\" draw:display=\"none\"/></draw:layer-set>");
            writer.with_owner(format!("master{}", master.id.0), |writer| {
                for shape in master.shapes.iter().filter(|shape| shape.ph.is_none()) {
                    writer.shape(&ctx, shape, out)?;
                }
                Ok(())
            })?;
            if let Some(note) = self.associations.get(&master.id).copied().flatten() {
                self.emit_notes(note, writer, out)?;
                emitted_notes.insert(note);
            }
            out.push_str("</style:master-page>");
            if out.len() as u64 > crate::MAX_PART {
                return Err(invalid("master XML exceeds part budget"));
            }
        }
        out.push_str("<style:master-page style:name=\"Default\" style:page-layout-name=\"Page\">");
        if let Some(master) = &self.pres.notes_master
            && !emitted_notes.contains(&master.id)
        {
            writer.notes_master(
                &deckcraft_model::notes::NotesMaster { master: master.clone(), size: self.pres.notes_size, fields: Default::default() },
                out,
            )?;
            emitted_notes.insert(master.id);
        }
        out.push_str("</style:master-page>");
        for notes in &self.pres.notes_masters {
            if emitted_notes.contains(&notes.master.id) {
                continue;
            }
            out.push_str(&format!("<style:master-page style:name=\"NotesMaster{}\" style:page-layout-name=\"Page\">", notes.master.id.0));
            writer.notes_master(notes, out)?;
            out.push_str("</style:master-page>");
            if out.len() as u64 > crate::MAX_PART {
                return Err(invalid("master XML exceeds part budget"));
            }
        }
        Ok(())
    }
    fn emit_notes(&self, id: MasterId, writer: &mut Writer<'a>, out: &mut String) -> Result<()> {
        if let Some(notes) = self.pres.notes_masters.iter().find(|notes| notes.master.id == id) {
            return writer.notes_master(notes, out);
        }
        let master = self.pres.notes_master.as_ref().filter(|master| master.id == id).ok_or_else(|| invalid("missing notes definition"))?;
        writer.notes_master(
            &deckcraft_model::notes::NotesMaster { master: master.clone(), size: self.pres.notes_size, fields: Default::default() },
            out,
        )
    }
}
