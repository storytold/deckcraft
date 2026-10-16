//! Structured notes projected into one standard notes master and editable local scenes.
//! Native sharing/override presence is retained only by a validated notes extension.
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use deckcraft_geom::{Size, Xfrm};
use deckcraft_model::{
    Background, Master, PhType, Placeholder, Presentation, Shape, ShapeId, ShapeKind, Slide,
    notes::{NotesDateTime, NotesFields, NotesPage},
    resolve::{self, Ctx},
    style::{ColorRef, Effects, Fill, Line},
    text::{ListStyle, ParaProps, RunKind, RunProps, TextBody},
};
use serde::{Deserialize, Serialize};

use crate::{PptxError, notes_codec::invalid};

pub(crate) const MAX_PAGES: usize = 10_000;
pub(crate) const MAX_SHAPES: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BodyStyle {
    /// Text-free original canonical formatting and field kinds, never a second body value.
    pub skeleton: TextBody,
    pub lengths: Vec<Vec<usize>>,
    pub vertical_tabs: Vec<Vec<Vec<usize>>>,
}

impl BodyStyle {
    pub(crate) fn new(original: &TextBody, projected: &TextBody) -> Result<Self, PptxError> {
        if original.paragraphs.len() != projected.paragraphs.len() {
            return Err(invalid("body paragraph projection changed topology"));
        }
        let mut skeleton = original.clone();
        let mut lengths = Vec::new();
        let mut vertical_tabs = Vec::new();
        for (source, projected) in skeleton.paragraphs.iter_mut().zip(&projected.paragraphs) {
            if source.runs.len() != projected.runs.len() {
                return Err(invalid("body run projection changed topology"));
            }
            lengths.push(projected.runs.iter().map(|run| run.text.len()).collect());
            vertical_tabs.push(projected.runs.iter().map(|run| run.text.match_indices('\u{b}').map(|(offset, _)| offset).collect()).collect());
            for run in &mut source.runs {
                run.text.clear();
            }
        }
        Ok(Self { skeleton, lengths, vertical_tabs })
    }

    pub(crate) fn restore(&self, actual: &TextBody) -> Result<TextBody, PptxError> {
        if self.skeleton.paragraphs.is_empty() && self.lengths.is_empty() && self.vertical_tabs.is_empty() && actual.text().is_empty() {
            return Ok(self.skeleton.clone());
        }
        if self.skeleton.paragraphs.len() != self.lengths.len()
            || actual.paragraphs.len() != self.lengths.len()
            || self.vertical_tabs.len() != self.lengths.len()
        {
            return Err(invalid("body projection paragraph mismatch"));
        }
        let mut restored = self.skeleton.clone();
        for (((paragraph, lengths), tabs), actual) in
            restored.paragraphs.iter_mut().zip(&self.lengths).zip(&self.vertical_tabs).zip(&actual.paragraphs)
        {
            if paragraph.runs.len() != lengths.len() || tabs.len() != lengths.len() || paragraph.runs.iter().any(|run| !run.text.is_empty()) {
                return Err(invalid("body skeleton contains text or mismatched run lengths"));
            }
            let text: String = actual.runs.iter().map(|run| if run.kind == RunKind::Break { "\n" } else { run.text.as_str() }).collect();
            let mut offset = 0usize;
            for ((run, length), tabs) in paragraph.runs.iter_mut().zip(lengths).zip(tabs) {
                if run.kind == RunKind::Break {
                    let end = offset.checked_add(1).ok_or_else(|| invalid("body break offset overflow"))?;
                    if text.get(offset..end) != Some("\n") || *length != 0 {
                        return Err(invalid("body break projection mismatch"));
                    }
                    offset = end;
                    continue;
                }
                let end = offset.checked_add(*length).ok_or_else(|| invalid("body text offset overflow"))?;
                run.text = text.get(offset..end).ok_or_else(|| invalid("body projection byte range mismatch"))?.to_string();
                for tab in tabs {
                    let end = tab.checked_add(1).ok_or_else(|| invalid("body tab offset overflow"))?;
                    if run.text.get(*tab..end) != Some("\n") && run.text.get(*tab..end) != Some("\u{b}") {
                        return Err(invalid("body tab projection mismatch"));
                    }
                    run.text.replace_range(*tab..end, "\u{b}");
                }
                offset = end;
            }
            if offset != text.len() {
                return Err(invalid("body projection has unbound text"));
            }
        }
        Ok(restored)
    }
}

pub(crate) struct PageProjection {
    pub shapes: Vec<Shape>,
    pub background: Option<Background>,
    pub body: ShapeId,
    pub native: Option<NotesPage>,
    pub body_style: BodyStyle,
    pub tables: Vec<crate::write::table_projection::notes::Binding>,
}

/// This plan always materializes notes scenes; external editors get one shared neutral
/// master, ordinary shapes, standard slide-image roles and editable canonical text.
pub(crate) struct Plan {
    pub pages: Vec<Option<PageProjection>>,
    pub size: Size,
}

impl Plan {
    pub(crate) fn new(p: &Presentation) -> Result<Option<Self>, PptxError> {
        let structured = p.notes_master.is_some() || !p.notes_masters.is_empty() || p.slides.iter().any(|slide| slide.notes_page.is_some());
        if !structured {
            return Ok(None);
        }
        if p.slides.len() > MAX_PAGES || p.notes_masters.len() > MAX_PAGES || !deckcraft_model::notes::valid_size(p.notes_size) {
            return Err(invalid("notes owner or size limit"));
        }
        // Bound all source data before cloning pages/body skeletons or allocating XML values.
        let mut budget = crate::notes_budget::Counter::new(crate::notes_codec::MAX_BYTES);
        budget.value(&p.notes_master)?;
        budget.value(&p.notes_masters)?;
        for slide in &p.slides {
            budget.value(&slide.notes)?;
            budget.value(&slide.notes_page)?;
        }
        let slide_ids: HashMap<_, _> = p.slides.iter().map(|slide| (slide.id.0, slide.id.0)).collect();
        let mut owners = HashSet::new();
        for master in p.notes_master.iter().map(Arc::as_ref).chain(p.notes_masters.iter().map(|notes| notes.master.as_ref())) {
            if !owners.insert(master.id) {
                return Err(invalid("duplicate notes master identity"));
            }
            validate_master(master)?;
            validate_slide_links(master, &slide_ids)?;
        }
        for notes in &p.notes_masters {
            if !deckcraft_model::notes::valid_size(notes.size) || !deckcraft_model::notes::valid_fields(&notes.fields) {
                return Err(invalid("invalid notes master fields or size"));
            }
            validate_fields(&notes.fields)?;
        }
        let mut pages = Vec::new();
        for (index, slide) in p.slides.iter().enumerate() {
            if let Some(error) = p.notes_page_error(slide) {
                return Err(invalid(error));
            }
            if slide.notes_page.is_none() && slide.notes.is_empty() {
                pages.push(None);
                continue;
            }
            pages.push(Some(project_page(p, slide, index)?));
        }
        Ok(Some(Self { pages, size: p.notes_size }))
    }
}

pub(crate) fn validate_master(master: &Master) -> Result<(), PptxError> {
    if master.raw_ext.is_some() {
        return Err(invalid("unclassified raw notes master resource extension"));
    }
    if master.layouts.len() > MAX_PAGES {
        return Err(invalid("notes master layout resource count exceeds limit"));
    }
    let mut count = 0;
    validate_shapes_counted(&master.shapes, true, &mut count)?;
    validate_background(master, master.background.as_ref())?;
    let mut layouts = HashSet::new();
    for layout in &master.layouts {
        if layout.raw_ext.is_some() {
            return Err(invalid("unclassified raw notes layout resource extension"));
        }
        if layout.id.0 == 0 || !layouts.insert(layout.id) {
            return Err(invalid("invalid or duplicate notes layout resource identity"));
        }
        // Layouts are deliberately unused native resources. They have no core
        // notes-layout peer, and therefore may retain ordinary layout roles.
        validate_shapes_counted(&layout.shapes, false, &mut count)?;
        validate_background(master, layout.background.as_ref())?;
    }
    // Validate unused canonical table programs too. They cannot borrow a page's
    // current cache witness, nor pass through normalization before validation.
    let mut budget = deckcraft_model::table::FormulaBudget::default();
    let mut error = None;
    for shapes in std::iter::once(master.shapes.as_slice()).chain(master.layouts.iter().map(|layout| layout.shapes.as_slice())) {
        deckcraft_model::walk(shapes, &mut |shape, _| {
            if error.is_none()
                && let ShapeKind::Table(table) = &shape.kind
                && let Err(failure) = validate_table_resource(table, &mut budget, 0)
            {
                error = Some(failure);
            }
        });
    }
    error.map_or(Ok(()), Err)
}

fn validate_background(master: &Master, background: Option<&Background>) -> Result<(), PptxError> {
    if let Some(Background::Ref { idx, .. }) = background {
        let found = if *idx >= 1001 {
            master.theme.format.bg_fills.get((*idx - 1001) as usize)
        } else {
            idx.checked_sub(1).and_then(|idx| master.theme.format.fills.get(idx as usize))
        };
        if found.is_none() {
            return Err(invalid("notes resource background references a missing theme fill"));
        }
    }
    Ok(())
}

fn validate_table_resource(
    table: &deckcraft_model::Table,
    budget: &mut deckcraft_model::table::FormulaBudget,
    depth: usize,
) -> Result<(), PptxError> {
    use deckcraft_model::table::{CellBlock, CellConstantState, CellFormulaState, MAX_NESTED_TABLE_DEPTH};
    if depth > MAX_NESTED_TABLE_DEPTH {
        return Err(invalid("notes resource table nesting exceeds limit"));
    }
    table.validate_content().map_err(invalid)?;
    deckcraft_model::table::validate_merged_grid(table).map_err(invalid)?;
    budget.next_pass();
    table.validate_formulas_with_budget(budget).map_err(invalid)?;
    for cell in table.rows.iter().flat_map(|row| &row.cells) {
        if matches!(cell.constant_state().map_err(invalid)?, CellConstantState::Stale)
            || matches!(cell.formula_state().map_err(invalid)?, CellFormulaState::Stale)
        {
            return Err(invalid("stale table display in notes resource"));
        }
        for block in &cell.blocks {
            if let CellBlock::Table(child) = block {
                validate_table_resource(&child.table, budget, depth + 1)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_shapes(shapes: &[Shape]) -> Result<(), PptxError> {
    validate_shapes_counted(shapes, true, &mut 0)
}

fn validate_shapes_counted(shapes: &[Shape], projected: bool, count: &mut usize) -> Result<(), PptxError> {
    let mut ids = HashSet::new();
    let mut error = None;
    deckcraft_model::walk(shapes, &mut |shape, depth| {
        *count = count.saturating_add(1);
        if *count > MAX_SHAPES || depth >= 32 {
            error = Some("shape count or depth limit");
        }
        if !ids.insert(shape.id) {
            error = Some("duplicate notes shape identity");
        }
        if !projected && shape.id.0 == 0 {
            error = Some("invalid notes layout shape identity");
        }
        if let Some(x) = shape.xfrm
            && ([x.x, x.y, x.w, x.h, x.rot].iter().any(|v| !v.is_finite()) || x.w < 0.0 || x.h < 0.0)
        {
            error = Some("invalid notes shape geometry");
        }
        if let ShapeKind::Group { child, .. } = &shape.kind
            && ([child.x, child.y, child.w, child.h, child.rot].iter().any(|v| !v.is_finite()) || child.w < 0.0 || child.h < 0.0)
        {
            error = Some("invalid notes group child geometry");
        }
        if matches!(shape.kind, ShapeKind::Opaque { .. }) || shape.raw_ext.is_some() || shape.diagram.is_some() {
            error = Some("unclassified notes object or raw extension");
        }
        if projected
            && shape.ph_type().is_some_and(|role| {
                !matches!(role, PhType::Body | PhType::SlideImage | PhType::Header | PhType::Footer | PhType::Date | PhType::SlideNum)
            })
        {
            error = Some("placeholder role is not permitted on a standard notes page");
        }
    });
    error.map_or(Ok(()), |error| Err(invalid(error)))
}

pub(crate) fn project_one(p: &Presentation, index: usize) -> Result<PageProjection, PptxError> {
    let slide = p.slides.get(index).ok_or_else(|| invalid("missing notes projection source slide"))?;
    if let Some(error) = p.notes_page_error(slide) {
        return Err(invalid(error));
    }
    project_page(p, slide, index)
}

fn project_page(p: &Presentation, slide: &Slide, index: usize) -> Result<PageProjection, PptxError> {
    let page = slide.notes_page.as_ref();
    if let Some(page) = page {
        validate_fields(&page.fields)?;
        validate_shapes(&page.shapes)?;
    }
    let size = p.notes_page_size(slide);
    let master = p.notes_master_for(slide);
    let context = Ctx::for_notes(p, slide).ok_or_else(|| invalid("notes page has no style context"))?;
    let fields = p.notes_fields(slide);
    let mut fields = fields;
    if fields.footer.is_none() && p.header_footer.footer {
        // Presentation footer state is inferred from ordinary slide placeholders on
        // PPTX import. It has no independent standard notes declaration. Keep this
        // precise dependency guarded until that global context has its own binding.
        let mut uses_global_footer = text_uses_footer(&slide.notes);
        if let Some(page) = page {
            uses_global_footer |= shapes_use_footer(&page.shapes)?;
        }
        if page.is_none_or(|page| page.show_master_shapes)
            && let Some(master) = master
        {
            uses_global_footer |= shapes_use_footer(&master.shapes)?;
        }
        if uses_global_footer {
            return Err(invalid("notes footer depends on an unbound presentation-wide footer; set a notes-page or notes-master footer declaration"));
        }
        fields.footer = Some(p.header_footer.footer_text.clone());
    }
    let mut originals = Vec::new();
    let mut body = page.and_then(|page| page.body_shape);
    let local = page.map(|page| page.shapes.as_slice()).unwrap_or_default();
    if page.is_some() && body.is_none() {
        let mut unbound = false;
        deckcraft_model::walk(local, &mut |shape, _| unbound |= shape.ph_type() == Some(PhType::Body));
        if unbound {
            return Err(invalid("notes body placeholder requires its canonical frame designation"));
        }
    }
    if page.is_none_or(|page| page.show_master_shapes)
        && let Some(master) = master
    {
        if body.is_none() {
            deckcraft_model::walk(&master.shapes, &mut |shape, _| {
                if shape.ph_type() == Some(PhType::Body) {
                    body = Some(shape.id);
                }
            });
        }
        for shape in &master.shapes {
            let overridden = shape.ph_type().is_some_and(|kind| {
                let mut found = false;
                deckcraft_model::walk(local, &mut |shape, _| found |= shape.ph_type() == Some(kind));
                found
            });
            if !overridden {
                originals.push((shape.clone(), page.and_then(|page| page.body_shape).is_none().then_some(body).flatten(), false));
            }
        }
    }
    originals.extend(local.iter().cloned().map(|shape| (shape, page.and_then(|page| page.body_shape), true)));
    // Page/master local IDs may overlap; give every projected member fresh part-local ids
    // without changing original records. Canonical identity is tracked while walking.
    let mut has_body = false;
    let mut next = 1u32;
    let mut projected_body = None;
    let mut shapes = Vec::new();
    for (shape, canonical, _) in &originals {
        shapes.push(materialize(&context, shape, &slide.notes, *canonical, &fields, p.slide_number(index), &mut next, &mut projected_body, 0)?);
        has_body |= projected_body.is_some();
    }
    let mut master_ids = HashMap::new();
    let mut local_ids = HashMap::new();
    for ((original, _, local), projected) in originals.iter().zip(&shapes) {
        bind_projection_ids(original, projected, if *local { &mut local_ids } else { &mut master_ids })?;
    }
    for ((_, _, local), projected) in originals.iter().zip(&mut shapes) {
        projection_connectors(std::slice::from_mut(projected), if *local { &local_ids } else { &master_ids })?;
    }
    if !has_body {
        let shape = Shape {
            id: ShapeId(0),
            ph: Some(Placeholder { kind: PhType::Body, idx: 1, ..Default::default() }),
            xfrm: Some(Xfrm::new(size.width * 0.1, size.height * 0.53, size.width * 0.8, size.height * 0.36)),
            ..Default::default()
        };
        shapes.push(materialize(&context, &shape, &slide.notes, Some(shape.id), &fields, p.slide_number(index), &mut next, &mut projected_body, 0)?);
    }
    let mut has_preview = false;
    for (shape, _, _) in &originals {
        deckcraft_model::walk(std::slice::from_ref(shape), &mut |shape, _| has_preview |= shape.ph_type() == Some(PhType::SlideImage));
    }
    if page.is_none() && !has_preview {
        next = next.checked_add(1).ok_or_else(|| invalid("notes projection id overflow"))?;
        shapes.insert(
            0,
            Shape {
                id: ShapeId(next),
                ph: Some(Placeholder { kind: PhType::SlideImage, ..Default::default() }),
                xfrm: Some(Xfrm::new(size.width * 0.1, size.height * 0.1, size.width * 0.8, size.height * 0.4)),
                ..Default::default()
            },
        );
    }
    let mut tables = Vec::new();
    for (original, _, local_owner) in &originals {
        let mapping = if *local_owner { &local_ids } else { &master_ids };
        let mut failure = None;
        deckcraft_model::walk(std::slice::from_ref(original), &mut |shape, _| {
            if let ShapeKind::Table(source) = &shape.kind {
                let result = (|| {
                    let projected_id = mapping.get(&shape.id).ok_or_else(|| invalid("notes table projection identity is missing"))?;
                    let projected =
                        deckcraft_model::find_shape(&shapes, *projected_id).ok_or_else(|| invalid("projected notes table disappeared"))?;
                    let ShapeKind::Table(current) = &projected.kind else {
                        return Err(invalid("projected notes table changed kind"));
                    };
                    tables.push(crate::write::table_projection::notes::Binding {
                        master: if *local_owner { None } else { master.map(|master| master.id.0) },
                        original_shape: shape.id.0,
                        projected_shape: projected_id.0,
                        bodies: crate::write::table_projection::notes::describe(source, current)?,
                    });
                    Ok(())
                })();
                if let Err(error) = result {
                    failure = Some(error);
                }
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
    }
    let body = projected_body.ok_or_else(|| invalid("missing projected canonical body"))?;
    let projected_text =
        deckcraft_model::find_shape(&shapes, body).and_then(|shape| shape.text.as_ref()).ok_or_else(|| invalid("missing projected notes text"))?;
    let body_style = BodyStyle::new(&slide.notes, projected_text)?;
    let background = page.and_then(|page| page.background.clone()).or_else(|| master.and_then(|master| master.background.clone()));
    let background = background.map(|background| match background {
        Background::Fill { mut fill } => {
            absolute_fill(&context, &mut fill, None);
            Background::Fill { fill }
        }
        Background::Ref { idx, color } => {
            let source = if idx >= 1001 {
                context.master.theme.format.bg_fills.get((idx - 1001) as usize)
            } else {
                context.master.theme.format.fills.get(idx.saturating_sub(1) as usize)
            };
            let mut fill = source.cloned().unwrap_or(Fill::None);
            absolute_fill(&context, &mut fill, Some(context.color(&color, None)));
            Background::Fill { fill }
        }
    });
    // Keep all content visible on the package's single global physical notes canvas.
    let scale = (p.notes_size.width / size.width).min(p.notes_size.height / size.height);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(invalid("invalid notes page projection scale"));
    }
    if size != p.notes_size {
        let x = (p.notes_size.width - size.width * scale) * 0.5;
        let y = (p.notes_size.height - size.height * scale) * 0.5;
        next = next.checked_add(1).ok_or_else(|| invalid("notes projection id overflow"))?;
        shapes = vec![Shape {
            id: ShapeId(next),
            xfrm: Some(Xfrm::new(x, y, size.width * scale, size.height * scale)),
            kind: ShapeKind::Group { children: shapes, child: Xfrm::new(0.0, 0.0, size.width, size.height) },
            ..Default::default()
        }];
    }
    validate_shapes(&shapes)?;
    Ok(PageProjection { shapes, background, body, native: page.cloned(), body_style, tables })
}

fn text_uses_footer(text: &TextBody) -> bool {
    text.paragraphs.iter().flat_map(|paragraph| &paragraph.runs).any(|run| matches!(&run.kind, RunKind::Field { field } if field == "footer"))
}

fn shapes_use_footer(shapes: &[Shape]) -> Result<bool, PptxError> {
    let mut found = false;
    let mut failure = None;
    deckcraft_model::walk(shapes, &mut |shape, _| {
        found |= shape.text.as_ref().is_some_and(text_uses_footer);
        if let ShapeKind::Table(table) = &shape.kind
            && let Err(error) = table.visit_texts(&mut |text| {
                found |= text_uses_footer(text);
                Ok(())
            })
        {
            failure = Some(invalid(error));
        }
    });
    failure.map_or(Ok(found), Err)
}

pub(crate) fn validate_fields(fields: &NotesFields) -> Result<(), PptxError> {
    if let Some(NotesDateTime::Current { format, lang }) = &fields.date_time {
        let known = format == "datetime"
            || format
                .strip_prefix("datetime")
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=13).contains(&n) && format == &format!("datetime{n}"));
        if !known {
            return Err(invalid("automatic notes date format is not a reserved DrawingML date field"));
        }
        if let Some(lang) = lang {
            let mut pieces = lang.split('-');
            let first = pieces.next().unwrap_or("");
            if first.is_empty()
                || first.len() > 8
                || !first.chars().all(|c| c.is_ascii_alphabetic())
                || pieces.any(|piece| piece.is_empty() || piece.len() > 8 || !piece.chars().all(|c| c.is_ascii_alphanumeric()))
            {
                return Err(invalid("automatic notes date language is not an XML language tag"));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn materialize(
    context: &Ctx<'_>,
    source: &Shape,
    canonical: &TextBody,
    body: Option<ShapeId>,
    fields: &NotesFields,
    number: u32,
    next: &mut u32,
    body_out: &mut Option<ShapeId>,
    depth: usize,
) -> Result<Shape, PptxError> {
    if depth >= 32 {
        return Err(invalid("notes group nesting limit"));
    }
    *next = next.checked_add(1).ok_or_else(|| invalid("notes projection id overflow"))?;
    let mut shape = source.clone();
    shape.id = ShapeId(*next);
    shape.xfrm = Some(resolve::xfrm(context, source));
    let (mut fill, ph) = resolve::fill(context, source);
    if let Some(fill) = &mut fill {
        absolute_fill(context, fill, ph);
    }
    shape.fill = fill;
    let (mut line, ph) = resolve::line(context, source);
    absolute_line(context, &mut line, ph);
    shape.line = Some(line);
    let (mut effects, ph) = resolve::effects(context, source);
    if let Some(effects) = &mut effects {
        absolute_effects(context, effects, ph);
    }
    shape.effects = effects;
    let kind = source.ph_type();
    shape.hidden |= match kind {
        Some(PhType::Header) => !fields.display_header.unwrap_or(fields.header.is_some()),
        Some(PhType::Footer) => !fields.display_footer.unwrap_or(fields.footer.is_some()),
        Some(PhType::Date) => !fields.display_date_time.unwrap_or(fields.date_time.is_some()),
        Some(PhType::SlideNum) => !fields.display_page_number.unwrap_or(false),
        _ => false,
    };
    if body == Some(source.id) {
        if body_out.is_some() {
            return Err(invalid("ambiguous projected body identity"));
        }
        *body_out = Some(shape.id);
        shape.ph = Some(Placeholder { kind: PhType::Body, idx: 1, ..Default::default() });
        shape.text = Some(canonical.clone());
        if let Some(parent) = context.master.shapes.iter().find(|shape| shape.ph_type() == Some(PhType::Body)).and_then(|shape| shape.text.as_ref())
            && let Some(text) = &mut shape.text
        {
            text.body.inherit(&parent.body);
            for paragraph in &mut text.paragraphs {
                if let Some(default) = parent.paragraphs.iter().find(|default| default.level == paragraph.level).or_else(|| parent.paragraphs.first())
                {
                    paragraph.props.inherit(&default.props);
                    paragraph.def_props.inherit(&default.def_props);
                }
            }
            for level in 0..9 {
                if let Some(inherited) = parent.list_style.level(level) {
                    let mut value = text.list_style.level(level).cloned().unwrap_or_default();
                    value.para.inherit(&inherited.para);
                    value.run.inherit(&inherited.run);
                    text.list_style.set(level, value);
                }
            }
        }
    }
    if shape.text.is_none() && kind.is_some_and(|role| matches!(role, PhType::Header | PhType::Footer | PhType::Date | PhType::SlideNum)) {
        shape.text = context.master.shapes.iter().find(|parent| parent.ph_type() == kind).and_then(|parent| parent.text.clone());
    }
    if let Some(text) = &shape.text {
        shape.text = Some(materialize_text(context, source, text, fields, number));
    }
    if kind == Some(PhType::SlideImage) {
        shape.kind = ShapeKind::Shape;
        shape.text = None;
        shape.fill = Some(Fill::None);
    }
    if let ShapeKind::Group { children, .. } = &mut shape.kind {
        for child in children {
            *child = materialize(context, child, canonical, body, fields, number, next, body_out, depth + 1)?;
        }
    }
    if let ShapeKind::Table(table) = &mut shape.kind {
        materialize_table(context, source, table, fields, number, 0)?;
    }
    // All inherited properties are explicit. Keep semantic roles for live image, body,
    // date and numbering but avoid a theme-font/style dependency on the neutral master.
    shape.style = None;
    Ok(shape)
}

fn materialize_text(context: &Ctx<'_>, source: &Shape, text: &TextBody, fields: &NotesFields, number: u32) -> TextBody {
    let mut out = text.clone();
    let mut temp = source.clone();
    temp.text = Some(text.clone());
    out.body = resolve::body(context, &temp);
    out.list_style = ListStyle::default();
    for paragraph in &mut out.paragraphs {
        let source_para = paragraph.clone();
        paragraph.props = resolve::para(context, &temp, &source_para);
        absolute_para(context, &mut paragraph.props);
        paragraph.def_props = resolve::run(context, &temp, &source_para, &source_para.def_props);
        absolute_run(context, &mut paragraph.def_props);
        paragraph.end_props = resolve::run(context, &temp, &source_para, &source_para.end_props);
        absolute_run(context, &mut paragraph.end_props);
        for run in &mut paragraph.runs {
            run.props = resolve::run(context, &temp, &source_para, &run.props);
            absolute_run(context, &mut run.props);
            if let RunKind::Field { field } = &mut run.kind {
                match field.as_str() {
                    "header" => {
                        if let Some(text) = &fields.header {
                            run.text = text.clone();
                        }
                        run.kind = RunKind::Text;
                    }
                    "footer" => {
                        if let Some(text) = &fields.footer {
                            run.text = text.clone();
                        }
                        run.kind = RunKind::Text;
                    }
                    "slidenum" => run.text = number.to_string(),
                    "notesdatetime" => match &fields.date_time {
                        Some(NotesDateTime::Fixed { text }) => {
                            run.text = text.clone();
                            run.kind = RunKind::Text;
                        }
                        Some(NotesDateTime::Current { format, lang }) => {
                            *field = format.clone();
                            run.props.lang.clone_from(lang);
                        }
                        None => *field = "datetime1".into(),
                    },
                    _ => {}
                }
            }
        }
    }
    out
}

fn absolute_color(context: &Ctx<'_>, color: &mut ColorRef, ph: Option<deckcraft_color::Rgba>) {
    *color = ColorRef::rgb(context.color(color, ph));
}
fn absolute_fill(context: &Ctx<'_>, fill: &mut Fill, ph: Option<deckcraft_color::Rgba>) {
    match fill {
        Fill::Solid { color } => absolute_color(context, color, ph),
        Fill::Gradient(gradient) => {
            for stop in &mut gradient.stops {
                absolute_color(context, &mut stop.color, ph);
            }
        }
        Fill::Pattern(pattern) => {
            absolute_color(context, &mut pattern.fg, ph);
            absolute_color(context, &mut pattern.bg, ph);
        }
        _ => {}
    }
}
fn absolute_line(context: &Ctx<'_>, line: &mut Line, ph: Option<deckcraft_color::Rgba>) {
    if let Some(fill) = &mut line.fill {
        absolute_fill(context, fill, ph);
    }
}
fn absolute_effects(context: &Ctx<'_>, effects: &mut Effects, ph: Option<deckcraft_color::Rgba>) {
    for shadow in [&mut effects.outer_shadow, &mut effects.inner_shadow].into_iter().flatten() {
        absolute_color(context, &mut shadow.color, ph);
    }
    if let Some(glow) = &mut effects.glow {
        absolute_color(context, &mut glow.color, ph);
    }
    if let Some(overlay) = &mut effects.fill_overlay {
        absolute_fill(context, &mut overlay.fill, ph);
    }
}
fn absolute_para(context: &Ctx<'_>, paragraph: &mut ParaProps) {
    if let Some(fill) = &mut paragraph.background {
        absolute_fill(context, fill, None);
    }
    if let Some(color) = &mut paragraph.bullet_color {
        absolute_color(context, color, None);
    }
    if let Some(font) = &mut paragraph.bullet_font {
        absolute_font(context, font);
    }
}
fn absolute_font(context: &Ctx<'_>, font: &mut String) {
    let set = if font.starts_with("+mj-") {
        Some(&context.master.theme.fonts.major)
    } else if font.starts_with("+mn-") {
        Some(&context.master.theme.fonts.minor)
    } else {
        None
    };
    if let Some(set) = set {
        *font = if font.ends_with("-ea") {
            set.ea.clone()
        } else if font.ends_with("-cs") {
            set.cs.clone()
        } else {
            set.latin.clone()
        };
    }
}
fn absolute_run(context: &Ctx<'_>, props: &mut RunProps) {
    if let Some(fill) = &mut props.fill {
        absolute_fill(context, fill, None);
    }
    if let Some(line) = &mut props.outline {
        absolute_line(context, line, None);
    }
    for font in [&mut props.font, &mut props.font_ea, &mut props.font_cs, &mut props.font_sym].into_iter().flatten() {
        absolute_font(context, font);
    }
    for color in [&mut props.underline_color, &mut props.highlight].into_iter().flatten() {
        absolute_color(context, color, None);
    }
    for shadow in [&mut props.shadow, &mut props.inner_shadow].into_iter().flatten() {
        absolute_color(context, &mut shadow.color, None);
    }
    if let Some(glow) = &mut props.glow {
        absolute_color(context, &mut glow.color, None);
    }
}

pub(crate) fn remap_shape_ids(
    shapes: &mut [Shape],
    allocate: &mut impl FnMut() -> Result<u32, PptxError>,
) -> Result<HashMap<ShapeId, ShapeId>, PptxError> {
    fn assign(
        shapes: &mut [Shape],
        map: &mut HashMap<ShapeId, ShapeId>,
        allocate: &mut impl FnMut() -> Result<u32, PptxError>,
        depth: usize,
    ) -> Result<(), PptxError> {
        if depth >= 32 {
            return Err(invalid("notes identity nesting limit"));
        }
        for shape in shapes {
            let id = ShapeId(allocate()?);
            if map.insert(shape.id, id).is_some() {
                return Err(invalid("duplicate original notes shape identity"));
            }
            shape.id = id;
            if let ShapeKind::Group { children, .. } = &mut shape.kind {
                assign(children, map, allocate, depth + 1)?;
            }
        }
        Ok(())
    }
    fn connectors(shapes: &mut [Shape], map: &HashMap<ShapeId, ShapeId>) -> Result<(), PptxError> {
        for shape in shapes {
            if let ShapeKind::Connector { start, end } = &mut shape.kind {
                for (id, _) in [start, end].into_iter().flatten() {
                    *id = *map.get(id).ok_or_else(|| invalid("notes connector target is missing"))?;
                }
            }
            if let ShapeKind::Group { children, .. } = &mut shape.kind {
                connectors(children, map)?;
            }
        }
        Ok(())
    }
    let mut map = HashMap::new();
    assign(shapes, &mut map, allocate, 0)?;
    connectors(shapes, &map)?;
    Ok(map)
}

fn remap_slide_value(value: &mut serde_json::Value, slides: &HashMap<u32, u32>, depth: usize, hyperlink: bool) -> Result<(), PptxError> {
    if depth > 512 {
        return Err(invalid("notes link nesting limit"));
    }
    match value {
        serde_json::Value::Object(object) => {
            if hyperlink {
                // Context is an admitted model hyperlink field and the value
                // decodes as Hyperlink. Do not rewrite similarly named raw keys.
                let mut link: deckcraft_model::text::Hyperlink =
                    serde_json::from_value(serde_json::Value::Object(object.clone())).map_err(invalid)?;
                if let deckcraft_model::text::Action::Slide { slide } = &mut link.action {
                    slide.0 = *slides.get(&slide.0).ok_or_else(|| invalid("missing notes slide-link target"))?;
                }
                if let Some(slide) = &mut link.zoom_return {
                    slide.0 = *slides.get(&slide.0).ok_or_else(|| invalid("missing notes zoom-return target"))?;
                }
                *value = serde_json::to_value(link).map_err(invalid)?;
                return Ok(());
            }
            if object.get("kind").and_then(serde_json::Value::as_str) == Some("slide") && object.contains_key("slide") {
                let old = object
                    .get("slide")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|id| u32::try_from(id).ok())
                    .ok_or_else(|| invalid("invalid notes slide-link identity"))?;
                let new = slides.get(&old).copied().ok_or_else(|| invalid("missing notes slide-link target"))?;
                object.insert("slide".into(), serde_json::Value::from(new));
            }
            for (key, value) in object.iter_mut() {
                let hyperlink =
                    matches!(key.as_str(), "click" | "hover" | "link") && value.as_object().is_some_and(|object| object.contains_key("action"));
                remap_slide_value(value, slides, depth + 1, hyperlink)?;
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                remap_slide_value(value, slides, depth + 1, false)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn validate_slide_links(record: &impl Serialize, slides: &HashMap<u32, u32>) -> Result<(), PptxError> {
    let mut value = serde_json::to_value(record).map_err(invalid)?;
    remap_slide_value(&mut value, slides, 0, false)
}

pub(crate) fn remap_slide_links<T: Serialize + for<'de> Deserialize<'de>>(record: &mut T, slide_ids: &HashMap<u32, u32>) -> Result<(), PptxError> {
    let mut value = serde_json::to_value(&*record).map_err(invalid)?;
    remap_slide_value(&mut value, slide_ids, 0, false)?;
    *record = serde_json::from_value(value).map_err(invalid)?;
    Ok(())
}

fn materialize_table(
    context: &Ctx<'_>,
    source: &Shape,
    table: &mut deckcraft_model::Table,
    fields: &NotesFields,
    number: u32,
    depth: usize,
) -> Result<(), PptxError> {
    use deckcraft_model::table::CellBlock;
    if depth > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
        return Err(invalid("nested notes table depth limit"));
    }
    table.validate_content().map_err(invalid)?;
    if let Some(fill) = &mut table.background {
        absolute_fill(context, fill, None);
    }
    for cell in table.rows.iter_mut().flat_map(|row| &mut row.cells) {
        cell.text = materialize_text(context, source, &cell.text, fields, number);
        for block in &mut cell.blocks {
            match block {
                CellBlock::Text(block) => block.body = materialize_text(context, source, &block.body, fields, number),
                CellBlock::Table(block) => materialize_table(context, source, &mut block.table, fields, number, depth + 1)?,
            }
        }
        if let Some(fill) = &mut cell.fill {
            absolute_fill(context, fill, None);
        }
        for line in cell.borders.iter_mut().chain([&mut cell.diag_down, &mut cell.diag_up]).flatten() {
            absolute_line(context, line, None);
        }
    }
    Ok(())
}

fn bind_projection_ids(source: &Shape, projected: &Shape, map: &mut HashMap<ShapeId, ShapeId>) -> Result<(), PptxError> {
    if map.insert(source.id, projected.id).is_some() {
        return Err(invalid("ambiguous notes projection identity"));
    }
    if source.children().len() != projected.children().len() {
        return Err(invalid("notes projection topology changed"));
    }
    for (source, projected) in source.children().iter().zip(projected.children()) {
        bind_projection_ids(source, projected, map)?;
    }
    Ok(())
}

fn projection_connectors(shapes: &mut [Shape], map: &HashMap<ShapeId, ShapeId>) -> Result<(), PptxError> {
    for shape in shapes {
        if let ShapeKind::Connector { start, end } = &mut shape.kind {
            for (id, _) in [start, end].into_iter().flatten() {
                *id = *map.get(id).ok_or_else(|| invalid("notes connector crosses an unprojected owner"))?;
            }
        }
        if let ShapeKind::Group { children, .. } = &mut shape.kind {
            projection_connectors(children, map)?;
        }
    }
    Ok(())
}
