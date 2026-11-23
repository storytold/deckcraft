//! DeckCraft's Library (#958): the slides of the presentations in the user's own folders, on the
//! Library core the Craft apps share ([`craft_library`]). This module is what is DeckCraft's own:
//! the files it lists ([`Slides`]: `.pptx`, `.potx`, `.deckcraft`), how it opens them and draws a
//! slide's thumbnail, and its file access ([`Confined`]), which goes through [`crate::io`] so the
//! automation read roots confine every folder listing and file read. The `library.*` commands are
//! in [`crate::cmd::library`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use craft_library::{
    EntryKind, Item, MAX_DEPTH, MAX_ENTRIES, MAX_FILE_BYTES, MAX_FOLDERS, MAX_ITEM_TEXT, MAX_ITEMS as MAX_SLIDES, MAX_PATH, MAX_THUMB_HEIGHT, Nouns,
    Stamp, THUMB_WIDTH, THUMB_WIDTHS, Thumbnail, sanitized_folders,
};
use deckcraft_model::{Presentation, ShapeKind, Slide};

use crate::io;

/// The files DeckCraft's Library lists.
pub const EXTENSIONS: [&str; 3] = ["pptx", "potx", "deckcraft"];
/// What every `library.*` command says in the browser build, which has no folders to read.
pub const NOT_IN_BROWSER: &str =
    "the Library is not available in the browser build: it reads folders of presentations on this computer, which a web page cannot";

/// DeckCraft's Library and what it has read (in [`crate::Session`]).
pub type Cache = craft_library::Library<Slides>;

/// Is `name` a file DeckCraft's Library lists? Hidden files and Office lock files are not.
pub fn is_presentation(name: &str) -> bool {
    craft_library::is_listed(name, &EXTENSIONS)
}

/// File access through [`crate::io`], inside the automation read roots when they are in force.
pub struct Confined;

impl craft_library::Fs for Confined {
    fn read_dir(&self, path: &Path, max: usize) -> Result<(Vec<craft_library::DirEntry>, bool), String> {
        io::read_dir(path, max)
    }
    fn stat(&self, path: &Path) -> Result<(EntryKind, Stamp), String> {
        io::stat(path)
    }
    fn canonical(&self, path: &Path) -> Result<PathBuf, String> {
        io::canonical(path)
    }
    fn read(&self, path: &Path, max: u64) -> Result<Vec<u8>, String> {
        io::read_path(path, max)
    }
}

/// Presentations, whose items are slides.
#[derive(Clone, Copy, Debug, Default)]
pub struct Slides;

impl craft_library::Kind for Slides {
    type Doc = Presentation;

    fn extensions(&self) -> &[&str] {
        &EXTENSIONS
    }

    fn nouns(&self) -> Nouns {
        Nouns { one: "slide", many: "slides" }
    }

    fn open(&self, name: &str, bytes: &[u8]) -> Result<Presentation, String> {
        crate::cmd::file::open_presentation(name, bytes).map_err(|e| e.to_string())
    }

    fn title(&self, doc: &Presentation) -> Option<String> {
        Some(doc.props.title.clone())
    }

    fn count(&self, doc: &Presentation) -> usize {
        doc.slides.len()
    }

    fn item(&self, doc: &Presentation, index: usize) -> Item {
        doc.slides.get(index).map(|sl| Item { title: sl.title(), hidden: sl.hidden, text: slide_text(sl) }).unwrap_or_default()
    }

    fn thumbnail(&self, doc: &Presentation, index: usize, width: u32) -> Result<Thumbnail, String> {
        let (png, width, height) = draw_thumbnail(doc, index, width);
        Ok(Thumbnail { png: Arc::new(png), width, height })
    }
}

/// The text on a slide (shapes, grouped shapes and table cells), for search: up to
/// [`MAX_ITEM_TEXT`] bytes.
pub fn slide_text(slide: &Slide) -> String {
    let mut out = String::new();
    let mut add = |t: &str| {
        if out.len() < MAX_ITEM_TEXT && !t.trim().is_empty() {
            out.push_str(t);
            out.push('\n');
        }
    };
    deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
        if let Some(t) = &sh.text {
            add(&t.text());
        }
        if let ShapeKind::Table(t) = &sh.kind {
            for c in t.rows.iter().flat_map(|r| r.cells.iter()) {
                add(&c.text.text());
            }
        }
    });
    craft_library::cap_bytes(&mut out, MAX_ITEM_TEXT);
    out
}

/// Draw slide `index` of `doc` `width` pixels wide (no taller than [`MAX_THUMB_HEIGHT`]) as PNG;
/// returns the bytes and the size.
fn draw_thumbnail(doc: &Presentation, index: usize, width: u32) -> (Vec<u8>, u32, u32) {
    let tallest = f64::from(MAX_THUMB_HEIGHT);
    let (sw, sh) = (doc.slide_size.width, doc.slide_size.height);
    let (sw, sh) = if sw.is_finite() && sh.is_finite() && sw > 0.0 && sh > 0.0 { (sw, sh) } else { (960.0, 540.0) };
    let mut scale = f64::from(width) / sw;
    if sh * scale > tallest {
        scale = tallest / sh;
    }
    // A hostile slide size (a denormal width) must not make an infinite scale.
    if !scale.is_finite() || scale <= 0.0 {
        scale = 1.0;
    }
    let w = ((sw * scale).round() as u32).clamp(1, *THUMB_WIDTHS.end());
    let h = ((sh * scale).round() as u32).clamp(1, MAX_THUMB_HEIGHT);
    let opts = deckcraft_render::RenderOpts { scale, size: Some((w, h)), ..Default::default() };
    let img = deckcraft_render::render_slide(doc, index, &opts);
    (img.to_png(), img.width, img.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentations_are_named_by_extension() {
        for yes in ["a.pptx", "A.PPTX", "b.potx", "c.deckcraft"] {
            assert!(is_presentation(yes), "{yes}");
        }
        for no in ["~$a.pptx", ".a.pptx", "a.ppt", "a.key", "a.ppsx"] {
            assert!(!is_presentation(no), "{no}");
        }
    }
}
