//! Narrow transfer admission safeguards until source loss and notes dependencies are owned.
//! This policy is used for copied objects/whole slides, never explicit text or raster extraction.
use deckcraft_model::{Presentation, Shape, Slide};

use crate::{EngineError, Result};

pub(crate) const MAX_POLICY_PAGES: usize = 10_000;
const MAX_POLICY_TAGS: usize = 100_000;
const MAX_POLICY_NOTES_OWNERS: usize = 100_000;
const MAX_POLICY_WORK: usize = 64 * 1024 * 1024;
const MAX_NOTES_SHAPES: usize = 100_000;
const MAX_DEPTH: usize = 64;

/// Immutable clipboard provenance, independent of the subsequently active document.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClipboardOrigin {
    // Option distinguishes an uncaptured default from a real UID whose numeric value is zero.
    source_uid: Option<u64>,
    source_has_odp_loss: bool,
    has_structured_notes: bool,
    same_document_only: Option<&'static str>,
}

pub(crate) fn same_document(origin: &ClipboardOrigin, uid: u64) -> bool {
    origin.source_uid == Some(uid)
}

fn invalid(reason: &str) -> EngineError {
    EngineError::Other(format!("slide transfer refused: {reason}"))
}

fn source_loss() -> EngineError {
    invalid("source has unattributed ODP loss; selected dependencies cannot yet be transferred safely")
}

fn notes_dependency() -> EngineError {
    invalid("structured notes need an explicit notes-master and media dependency transfer")
}

#[derive(Default)]
struct Budget {
    work: usize,
    pages: usize,
}

impl Budget {
    fn work(&mut self, amount: usize) -> Result<()> {
        self.work = self.work.checked_add(amount).ok_or_else(|| invalid("policy work overflow"))?;
        if self.work > MAX_POLICY_WORK {
            return Err(invalid("policy work limit exceeded"));
        }
        Ok(())
    }
    fn page(&mut self) -> Result<()> {
        self.pages = self.pages.checked_add(1).ok_or_else(|| invalid("policy page count overflow"))?;
        if self.pages > MAX_POLICY_PAGES {
            return Err(invalid("too many selected pages for transfer policy"));
        }
        self.work(32)
    }
}

fn has_loss(source: &Presentation, budget: &mut Budget) -> Result<bool> {
    if source.tags.len() > MAX_POLICY_TAGS {
        return Err(invalid("too many source tags for transfer policy"));
    }
    budget.work(source.tags.len())?;
    // Values are deliberately neither scanned nor copied: the fixed key is the blocking signal.
    Ok(source.tags.iter().any(|(key, _)| matches!(key.as_str(), "odp:warning" | "pptx:comment-warning")))
}

fn tree_work(shapes: &[Shape], depth: usize, count: &mut usize, budget: &mut Budget) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(invalid("notes dependency nesting exceeds limit"));
    }
    for shape in shapes {
        *count = count.checked_add(1).ok_or_else(|| invalid("notes shape count overflow"))?;
        if *count > MAX_NOTES_SHAPES {
            return Err(invalid("notes dependency shape count exceeds limit"));
        }
        budget.work(4)?;
        if !shape.children().is_empty() {
            tree_work(shape.children(), depth + 1, count, budget)?;
        }
    }
    Ok(())
}

fn current_notes(source: &Presentation, slide: &Slide, budget: &mut Budget) -> Result<()> {
    let Some(page) = &slide.notes_page else { return Ok(()) };
    if source.notes_masters.len() > MAX_POLICY_NOTES_OWNERS {
        return Err(invalid("too many notes owners for transfer policy"));
    }
    // Cover all repeated native owner/size/error lookups before any of them run.
    budget.work(source.notes_masters.len().checked_mul(16).ok_or_else(|| invalid("notes owner work overflow"))?)?;
    tree_work(&page.shapes, 0, &mut 0, budget)?;
    if let Some(master) = source.notes_master_for(slide) {
        tree_work(&master.shapes, 0, &mut 0, budget)?;
    }
    if let Some(reason) = source.notes_page_error(slide) {
        return Err(invalid(reason));
    }
    Ok(())
}

/// Empty `slides` captures origin for copied shapes/objects; explicit text extraction bypasses it.
pub(crate) fn capture<'a>(source: &Presentation, source_uid: u64, slides: impl IntoIterator<Item = &'a Slide>) -> Result<ClipboardOrigin> {
    let mut budget = Budget::default();
    let source_has_odp_loss = has_loss(source, &mut budget)?;
    let mut has_structured_notes = false;
    for slide in slides {
        budget.page()?;
        has_structured_notes |= slide.notes_page.is_some();
        current_notes(source, slide, &mut budget)?;
    }
    Ok(ClipboardOrigin { source_uid: Some(source_uid), source_has_odp_loss, has_structured_notes, same_document_only: None })
}

/// Prove clipboard origin before cloning/publishing copied objects or whole slides.
pub(crate) fn check_clipboard<'a>(
    origin: &ClipboardOrigin,
    destination: &Presentation,
    destination_uid: u64,
    slides: impl IntoIterator<Item = &'a Slide>,
) -> Result<()> {
    let same_document = origin.source_uid == Some(destination_uid);
    let mut budget = Budget::default();
    if origin.source_has_odp_loss {
        if !same_document {
            return Err(source_loss());
        }
        if !has_loss(destination, &mut budget)? {
            return Err(invalid("source loss guard was cleared since clipboard capture"));
        }
    }
    if origin.has_structured_notes && !same_document {
        if let Some(reason) = origin.same_document_only {
            return Err(invalid(reason));
        }
        return Err(notes_dependency());
    }
    for slide in slides {
        budget.page()?;
        if slide.notes_page.is_some() && !same_document {
            return Err(notes_dependency());
        }
        // Body/master validation is against the current destination, not an obsolete source snapshot.
        current_notes(destination, slide, &mut budget)?;
    }
    Ok(())
}

/// Reuse and compare load another presentation and therefore always cross an origin boundary.
pub(crate) fn check_external<'a>(source: &Presentation, slides: impl IntoIterator<Item = &'a Slide>) -> Result<()> {
    let mut budget = Budget::default();
    if has_loss(source, &mut budget)? {
        return Err(source_loss());
    }
    for slide in slides {
        budget.page()?;
        // Refuse before an expensive notes validation: no cross-document notes closure is claimed.
        if slide.notes_page.is_some() {
            return Err(notes_dependency());
        }
    }
    Ok(())
}

/// A closure-aware lane is separate from check_external: review still refuses notes.
pub(crate) fn notes_clipboard_crosses_document(origin: &ClipboardOrigin, destination_uid: u64) -> Result<bool> {
    if origin.source_uid == Some(destination_uid) {
        return Ok(false);
    }
    if origin.source_uid.is_none() {
        return Err(invalid("notes closure needs an immutable captured clipboard origin"));
    }
    if origin.source_has_odp_loss {
        return Err(source_loss());
    }
    Ok(true)
}

/// A successful notes closure has already validated selected pages and shared owner trees.
/// Capture only immutable source warning identity here, avoiding a second per-page owner scan.
pub(crate) fn capture_closed_notes(
    source: &Presentation,
    source_uid: u64,
    _closure: &crate::notes_transfer::NotesClosure,
) -> Result<ClipboardOrigin> {
    let source_has_odp_loss = has_loss(source, &mut Budget::default())?;
    Ok(ClipboardOrigin { source_uid: Some(source_uid), source_has_odp_loss, has_structured_notes: true, same_document_only: None })
}

/// Explicit validated profile exclusion. It never grants a cross-document exception.
pub(crate) fn capture_same_document_only<'a>(
    source: &Presentation,
    uid: u64,
    slides: impl IntoIterator<Item = &'a Slide>,
    reason: Option<&'static str>,
) -> Result<ClipboardOrigin> {
    let mut origin = capture(source, uid, slides)?;
    origin.same_document_only = reason;
    Ok(origin)
}
