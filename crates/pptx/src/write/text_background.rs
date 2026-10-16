//! DrawingML has no interoperable paragraph-area fill. Refuse before writing a package rather
//! than substituting character highlighting or silently dropping the editable paragraph fill.
use deckcraft_model::{Fill, Presentation, Shape, ShapeKind, TextBody, text::ListStyle};

use crate::PptxError;

fn painted(fill: &Option<Fill>) -> bool {
    fill.as_ref().is_some_and(|fill| !matches!(fill, Fill::None))
}

fn list(style: &ListStyle) -> bool {
    style.levels.iter().flatten().any(|level| painted(&level.para.background))
}

fn text(body: &TextBody) -> bool {
    list(&body.list_style) || body.paragraphs.iter().any(|paragraph| painted(&paragraph.props.background))
}

fn shapes(items: &[Shape], depth: usize) -> Result<bool, PptxError> {
    if depth > 64 {
        return Err(PptxError::Unsupported("shape nesting exceeds paragraph-background validation limit".into()));
    }
    for shape in items {
        if shape.text.as_ref().is_some_and(text) {
            return Ok(true);
        }
        if let ShapeKind::Table(table) = &shape.kind {
            let mut has_background = false;
            table
                .visit_texts(&mut |body| {
                    has_background |= text(body);
                    Ok(())
                })
                .map_err(|error| PptxError::Unsupported(error.to_string()))?;
            if has_background {
                return Ok(true);
            }
        }
        if !shape.children().is_empty() && shapes(shape.children(), depth + 1)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn check(presentation: &Presentation) -> Result<(), PptxError> {
    let unsupported =
        || PptxError::Unsupported("PPTX export cannot preserve paragraph backgrounds. Save as ODP or DeckCraft to keep the paragraph fill.".into());
    if list(&presentation.default_text_style) {
        return Err(unsupported());
    }
    for slide in &presentation.slides {
        if text(&slide.notes)
            || shapes(&slide.shapes, 0)?
            || slide.notes_page.as_ref().map(|page| shapes(&page.shapes, 0)).transpose()?.unwrap_or(false)
        {
            return Err(unsupported());
        }
    }
    for master in presentation
        .masters
        .iter()
        .chain(presentation.notes_master.iter())
        .chain(presentation.handout_master.iter())
        .map(|master| master.as_ref())
        .chain(presentation.notes_masters.iter().map(|notes| notes.master.as_ref()))
    {
        if [&master.title_style, &master.body_style, &master.other_style].into_iter().any(list) || shapes(&master.shapes, 0)? {
            return Err(unsupported());
        }
        for layout in &master.layouts {
            if shapes(&layout.shapes, 0)? {
                return Err(unsupported());
            }
        }
    }
    Ok(())
}
