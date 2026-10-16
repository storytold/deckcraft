//! Bounded validation of line-height settings across editable text owners.
use crate::{Presentation, Shape, ShapeKind, TextBody, text::ListStyle};

pub fn validate(presentation: &Presentation) -> Result<(), &'static str> {
    fn list(style: &ListStyle) -> Result<(), &'static str> {
        for level in style.levels.iter().flatten() {
            properties(&level.para)?;
        }
        Ok(())
    }
    fn properties(props: &crate::text::ParaProps) -> Result<(), &'static str> {
        if props.line_height.is_some_and(|policy| !policy.valid()) || (props.line_height.is_some() && props.line_spacing.is_some()) {
            return Err("invalid or conflicting paragraph line-height policy");
        }
        Ok(())
    }
    fn body(body: &TextBody) -> Result<(), &'static str> {
        list(&body.list_style)?;
        for paragraph in &body.paragraphs {
            properties(&paragraph.props)?;
        }
        Ok(())
    }
    fn shapes(items: &[Shape], depth: usize) -> Result<(), &'static str> {
        if depth > 64 {
            return Err("shape nesting exceeds line-height validation limit");
        }
        for shape in items {
            if let Some(text) = &shape.text {
                body(text)?;
            }
            if let ShapeKind::Table(table) = &shape.kind {
                table
                    .visit_texts(&mut |text| body(text).map_err(|error| crate::ModelError::Invalid(error.into())))
                    .map_err(|_| "invalid table content or paragraph line-height policy")?;
            }
            if !shape.children().is_empty() {
                shapes(shape.children(), depth + 1)?;
            }
        }
        Ok(())
    }
    list(&presentation.default_text_style)?;
    for slide in &presentation.slides {
        body(&slide.notes)?;
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
