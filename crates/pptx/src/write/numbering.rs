//! Preflight for paragraph settings without a faithful DrawingML mapping.
use deckcraft_model::{
    Presentation, Shape, ShapeKind, TextBody,
    text::{Bullet, ListStyle, ParaProps},
};

use crate::PptxError;

fn properties(properties: &ParaProps) -> Result<(), PptxError> {
    if properties.line_height.is_some() {
        return Err(PptxError::Unsupported(
            "PPTX export cannot yet preserve explicit normal/minimum line-height policies; save as ODP or DeckCraft".into(),
        ));
    }
    if properties.number_restart.is_some() {
        return Err(PptxError::Unsupported("PPTX export cannot yet preserve explicit numbered-list restarts; save as ODP or DeckCraft".into()));
    }
    if let Some(Bullet::AutoNum { start_at, .. }) = properties.bullet
        && !(1..=32767).contains(&start_at)
    {
        return Err(PptxError::Unsupported("PPTX numbered-list sequence starts must be in 1..32767; the value cannot be clamped faithfully".into()));
    }
    Ok(())
}
fn list(style: &ListStyle) -> Result<(), PptxError> {
    for level in style.levels.iter().flatten() {
        properties(&level.para)?;
    }
    Ok(())
}
fn text(body: &TextBody) -> Result<(), PptxError> {
    list(&body.list_style)?;
    for paragraph in &body.paragraphs {
        properties(&paragraph.props)?;
    }
    Ok(())
}
fn shapes(items: &[Shape], depth: usize) -> Result<(), PptxError> {
    if depth > 64 {
        return Err(PptxError::Unsupported("shape nesting exceeds numbered-list validation limit".into()));
    }
    for shape in items {
        if let Some(body) = &shape.text {
            text(body)?;
        }
        if let ShapeKind::Table(table) = &shape.kind {
            let mut failure = None;
            table
                .visit_texts(&mut |body| {
                    if failure.is_none() {
                        failure = text(body).err();
                    }
                    Ok(())
                })
                .map_err(|error| PptxError::Unsupported(error.to_string()))?;
            if let Some(error) = failure {
                return Err(error);
            }
        }
        if !shape.children().is_empty() {
            shapes(shape.children(), depth + 1)?;
        }
    }
    Ok(())
}

pub(super) fn check(presentation: &Presentation) -> Result<(), PptxError> {
    deckcraft_model::line_height::validate(presentation).map_err(|error| PptxError::Unsupported(error.into()))?;
    list(&presentation.default_text_style)?;
    for slide in &presentation.slides {
        text(&slide.notes)?;
        shapes(&slide.shapes, 0)?;
        if let Some(page) = &slide.notes_page {
            shapes(&page.shapes, 0)?;
        }
    }
    for master in presentation
        .masters
        .iter()
        .chain(presentation.notes_master.iter())
        .chain(presentation.handout_master.iter())
        .chain(presentation.notes_masters.iter().map(|notes| &notes.master))
    {
        for style in [&master.title_style, &master.body_style, &master.other_style] {
            list(style)?;
        }
        shapes(&master.shapes, 0)?;
        for layout in &master.layouts {
            shapes(&layout.shapes, 0)?;
        }
    }
    Ok(())
}
