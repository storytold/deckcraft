//! Preserve the distinction between a creation timestamp and an authored display label.
use deckcraft_model::{Comment, Presentation, datetime::valid_date_time};

use crate::PptxError;

pub(super) fn check(presentation: &Presentation) -> Result<(), PptxError> {
    #[derive(Default)]
    struct Facts {
        missing_date: bool,
        modern: bool,
    }
    fn comments(items: &[Comment], depth: usize, count: &mut usize) -> Result<Facts, PptxError> {
        if depth > 64 || count.saturating_add(items.len()) > 100_000 {
            return Err(PptxError::Unsupported("comment metadata exceeds the export validation limit".into()));
        }
        *count += items.len();
        let mut facts = Facts::default();
        for comment in items {
            if comment.display_date.is_some() {
                return Err(PptxError::Unsupported("PPTX export cannot preserve independent comment date labels; save as ODP or DeckCraft".into()));
            }
            if !comment.date.is_empty() && !valid_date_time(&comment.date) {
                return Err(PptxError::Unsupported("comment creation timestamp is outside the supported XML Schema dateTime profile".into()));
            }
            facts.missing_date |= comment.date.is_empty();
            facts.modern |= !comment.replies.is_empty() || comment.resolved || comment.shape.is_some();
            if !comment.replies.is_empty() {
                let replies = comments(&comment.replies, depth + 1, count)?;
                facts.missing_date |= replies.missing_date;
                facts.modern |= replies.modern;
            }
        }
        Ok(facts)
    }
    let mut count = 0;
    for slide in &presentation.slides {
        let facts = comments(&slide.comments, 0, &mut count)?;
        if facts.missing_date {
            if facts.modern {
                return Err(PptxError::Unsupported("missing comment creation timestamps cannot preserve modern comments on the same slide".into()));
            }
            if slide.comments.len() > 10_000 {
                return Err(PptxError::Unsupported("legacy-only comments exceed the supported slide count".into()));
            }
            for comment in &slide.comments {
                if [comment.x, comment.y].into_iter().any(|position| {
                    let scaled = position * 8.0;
                    !scaled.is_finite() || !(0.0..=1.0e9).contains(&scaled) || scaled.fract() != 0.0
                }) {
                    return Err(PptxError::Unsupported("legacy-only comment positions must be exactly representable in eighths of a point".into()));
                }
            }
        }
    }
    Ok(())
}

/// Preflight has proved any undated slide is flat and faithfully legacy-only.
pub(super) fn uses_modern(items: &[Comment]) -> bool {
    items.iter().all(|comment| !comment.date.is_empty())
}
