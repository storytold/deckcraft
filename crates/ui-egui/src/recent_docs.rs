//! macOS Dock and Apple-menu recent documents.
//!
//! Opening or saving a presentation tells the system, which is what the Dock's recent-documents
//! list reads. Pictures, PDFs and outlines are copies, not documents, so they stay off that list.
//! Other platforms keep only DeckCraft's own recent list.

/// A path the Dock should remember: a presentation, not an export.
pub fn is_presentation_path(path: &str) -> bool {
    matches!(
        std::path::Path::new(path).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("deckcraft" | "slidecraft" | "pptx" | "potx" | "ppsx")
    )
}

/// Record `path` as a recent document when it is a presentation that exists.
pub fn note_system_recent(path: &str) {
    let Some(absolute) = absolute_presentation(path) else { return };
    #[cfg(target_os = "macos")]
    macos::note(&absolute);
    #[cfg(not(target_os = "macos"))]
    let _ = absolute;
}

/// Paths in this process's system recent-documents list.
///
/// Empty off the main thread, and on every platform other than macOS. The Dock menu of a packaged
/// app shows these only once the bundle declares its document types.
#[doc(hidden)]
pub fn system_recent_paths() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        match objc2::MainThreadMarker::new() {
            Some(mtm) => macos::listed(mtm),
            None => Vec::new(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

fn absolute_presentation(path: &str) -> Option<String> {
    if !is_presentation_path(path) {
        return None;
    }
    std::fs::canonicalize(path).ok().and_then(|p| p.to_str().map(str::to_string))
}

#[cfg(target_os = "macos")]
mod macos {
    pub fn note(path: &str) {
        let Some(mtm) = objc2::MainThreadMarker::new() else { return };
        note_on(path, mtm);
    }

    pub fn note_on(path: &str, mtm: objc2::MainThreadMarker) {
        let s = objc2_foundation::NSString::from_str(path);
        let url = objc2_foundation::NSURL::fileURLWithPath(&s);
        objc2_app_kit::NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url);
    }

    /// Paths currently in this process's system recent-documents list.
    pub fn listed(mtm: objc2::MainThreadMarker) -> Vec<String> {
        let urls = objc2_app_kit::NSDocumentController::sharedDocumentController(mtm).recentDocumentURLs();
        (0..urls.count()).map(|i| urls.objectAtIndex(i)).filter_map(|url| url.path().map(|p| p.to_string())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_presentations_are_recent_documents() {
        assert!(is_presentation_path("/Talks/Aurora.deckcraft"));
        assert!(is_presentation_path("/Talks/Aurora.PPTX"));
        assert!(is_presentation_path("template.potx"));
        assert!(is_presentation_path("show.ppsx"));
        assert!(is_presentation_path("old.slidecraft"));
        assert!(!is_presentation_path("/Talks/slide.png"));
        assert!(!is_presentation_path("/Talks/handout.pdf"));
        assert!(!is_presentation_path("/Talks/outline.txt"));
        assert!(!is_presentation_path("/Talks/notes.md"));
        assert!(!is_presentation_path("/Talks/deckcraft"));
    }
}
