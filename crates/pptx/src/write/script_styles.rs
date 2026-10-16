//! DrawingML run properties do not carry separate size/weight/slant for each script slot.
//! Preserve the source semantics by refusing until an editable lowering is verified.
use crate::PptxError;
use deckcraft_model::{Presentation, Shape, ShapeKind, TextBody, text::ListStyle};

fn unrepresented(run: &deckcraft_model::text::RunProps) -> bool {
    // Equal explicit ordinary properties already fix the style for every script in
    // DrawingML. Equality with an inherited/default value cannot prove that ownership.
    fn different<T: PartialEq>(script: &Option<T>, ordinary: &Option<T>) -> bool {
        script.is_some() && script != ordinary
    }
    different(&run.size_ea, &run.size)
        || different(&run.size_cs, &run.size)
        || different(&run.bold_ea, &run.bold)
        || different(&run.bold_cs, &run.bold)
        || different(&run.italic_ea, &run.italic)
        || different(&run.italic_cs, &run.italic)
        || different(&run.lang_ea, &run.lang)
        || different(&run.lang_cs, &run.lang)
        || run.lang.as_ref().is_some_and(String::is_empty)
}

fn list(style: &ListStyle) -> bool {
    style.levels.iter().flatten().any(|level| unrepresented(&level.run))
}
fn text(body: &TextBody) -> bool {
    list(&body.list_style)
        || body.paragraphs.iter().any(|paragraph| {
            unrepresented(&paragraph.def_props) || unrepresented(&paragraph.end_props) || paragraph.runs.iter().any(|run| unrepresented(&run.props))
        })
}
fn shapes(items: &[Shape], depth: usize) -> Result<bool, PptxError> {
    if depth > 64 {
        return Err(PptxError::Unsupported("shape nesting exceeds script-style validation limit".into()));
    }
    for shape in items {
        if shape.text.as_ref().is_some_and(text) {
            return Ok(true);
        }
        if let ShapeKind::Table(table) = &shape.kind {
            let mut unsupported = false;
            table
                .visit_texts(&mut |body| {
                    unsupported |= text(body);
                    Ok(())
                })
                .map_err(|error| PptxError::Unsupported(error.to_string()))?;
            if unsupported {
                return Ok(true);
            }
        }
        if !shape.children().is_empty() && shapes(shape.children(), depth + 1)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn effective_text(ctx: &deckcraft_model::resolve::Ctx<'_>, shape: &Shape, body: &TextBody, possible_table_bold: bool) -> bool {
    body.paragraphs.iter().any(|paragraph| {
        std::iter::once(&paragraph.def_props).chain(std::iter::once(&paragraph.end_props)).chain(paragraph.runs.iter().map(|run| &run.props)).any(
            |props| {
                let resolved = deckcraft_model::resolve::run(ctx, shape, paragraph, props);
                if unrepresented(&resolved) {
                    return true;
                }
                // The table renderer applies style bold before paragraph/default inheritance.
                // A conservative extra case covers a styled cell without coupling exporters
                // to the drawing layer. Explicit ordinary run bold is unaffected.
                if possible_table_bold && props.bold.is_none() {
                    let mut styled = props.clone();
                    styled.bold = Some(true);
                    return unrepresented(&deckcraft_model::resolve::run(ctx, shape, paragraph, &styled));
                }
                false
            },
        )
    })
}

fn effective_shapes(ctx: &deckcraft_model::resolve::Ctx<'_>, items: &[Shape], depth: usize) -> Result<bool, PptxError> {
    if depth > 64 {
        return Err(PptxError::Unsupported("shape nesting exceeds script-style validation limit".into()));
    }
    for shape in items {
        if shape.text.as_ref().is_some_and(|body| effective_text(ctx, shape, body, false)) {
            return Ok(true);
        }
        if let ShapeKind::Table(table) = &shape.kind {
            let mut unsupported = false;
            table
                .visit_texts(&mut |body| {
                    let frame = Shape { text: Some(TextBody { list_style: body.list_style.clone(), ..Default::default() }), ..Default::default() };
                    unsupported |= effective_text(ctx, &frame, body, true);
                    Ok(())
                })
                .map_err(|error| PptxError::Unsupported(error.to_string()))?;
            if unsupported {
                return Ok(true);
            }
        }
        if !shape.children().is_empty() && effective_shapes(ctx, shape.children(), depth + 1)? {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn check(p: &Presentation) -> Result<(), PptxError> {
    let error = || {
        PptxError::Unsupported("PPTX export cannot yet preserve independent East Asian or complex-script run styles or explicit inherited-language clears; save as ODP or DeckCraft".into())
    };
    if list(&p.default_text_style) {
        return Err(error());
    }
    for slide in &p.slides {
        if text(&slide.notes)
            || shapes(&slide.shapes, 0)?
            || slide.notes_page.as_ref().map(|page| shapes(&page.shapes, 0)).transpose()?.unwrap_or(false)
        {
            return Err(error());
        }
        if let Some(ctx) = deckcraft_model::resolve::Ctx::for_slide(p, slide)
            && effective_shapes(&ctx, &slide.shapes, 0)?
        {
            return Err(error());
        }
        if let Some(ctx) = deckcraft_model::resolve::Ctx::for_notes(p, slide) {
            let frame = Shape {
                ph: Some(deckcraft_model::Placeholder { kind: deckcraft_model::PhType::Body, ..Default::default() }),
                text: Some(TextBody { list_style: slide.notes.list_style.clone(), ..Default::default() }),
                ..Default::default()
            };
            if effective_text(&ctx, &frame, &slide.notes, false)
                || slide.notes_page.as_ref().map(|page| effective_shapes(&ctx, &page.shapes, 0)).transpose()?.unwrap_or(false)
            {
                return Err(error());
            }
        }
    }
    for master in
        p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|notes| &notes.master))
    {
        if [&master.title_style, &master.body_style, &master.other_style].into_iter().any(list) || shapes(&master.shapes, 0)? {
            return Err(error());
        }
        for layout in &master.layouts {
            if shapes(&layout.shapes, 0)? {
                return Err(error());
            }
            if effective_shapes(&deckcraft_model::resolve::Ctx::for_layout(p, master, layout), &layout.shapes, 0)? {
                return Err(error());
            }
        }
        if effective_shapes(&deckcraft_model::resolve::Ctx::for_master(p, master), &master.shapes, 0)? {
            return Err(error());
        }
    }
    Ok(())
}
