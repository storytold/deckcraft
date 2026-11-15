//! The Library (#958): slides and elements kept in the user's own folders of presentations,
//! browsed from a side pane and brought into the deck being edited (`library.*` commands in
//! [`crate::cmd::library`]).
//!
//! The folders are read through [`crate::io`], so the automation read roots
//! ([`crate::file_access`]) confine every folder listing and every file read. Everything read is
//! hostile and bounded:
//!
//! | What | Limit |
//! |---|---|
//! | Folders | [`MAX_FOLDERS`] |
//! | Entries looked at per folder (files and folders of every kind) | [`MAX_ENTRIES`] |
//! | Folder depth below the library folder | [`MAX_DEPTH`] |
//! | File size (larger files are listed with an error, not opened) | [`MAX_FILE_BYTES`] |
//! | Slides listed per file | [`MAX_SLIDES`] |
//! | Links | never followed, so nothing outside the folder is reached through one |
//!
//! A file that can't be read is listed with its error rather than dropped. What has been read is
//! cached in [`Cache`]: an index of each file (slide titles and text, for search), a few parsed
//! presentations and the thumbnails drawn from them, each within a fixed budget.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deckcraft_model::{Presentation, ShapeKind, Slide};

use crate::io::{self, EntryKind, Stamp};

/// The most folders the Library browses.
pub const MAX_FOLDERS: usize = 32;
/// The most entries (files and folders of any kind) looked at under one library folder.
pub const MAX_ENTRIES: usize = 5_000;
/// The deepest folder entered below a library folder (its own files are at depth 0).
pub const MAX_DEPTH: usize = 8;
/// Presentations larger than this are listed with an error and never opened.
pub const MAX_FILE_BYTES: u64 = 256 << 20;
/// The most slides listed (and searched, and offered) per file.
pub const MAX_SLIDES: usize = 2_000;
/// Thumbnail widths, in pixels, and the default.
pub const THUMB_WIDTHS: RangeInclusive<u32> = 32..=512;
pub const THUMB_WIDTH: u32 = 160;
/// Search answer sizes, and the default.
pub const SEARCH_LIMITS: RangeInclusive<usize> = 1..=500;
pub const SEARCH_LIMIT: usize = 100;
/// The longest search query, in characters.
pub const MAX_QUERY: usize = 256;
/// The longest folder path kept, in bytes.
pub const MAX_PATH: usize = 4096;
/// The file kinds the Library lists.
pub const EXTENSIONS: [&str; 3] = ["pptx", "potx", "deckcraft"];
/// What every `library.*` command says in the browser build, which has no folders to read.
pub const NOT_IN_BROWSER: &str =
    "the Library is not available in the browser build: it reads folders of presentations on this computer, which a web page cannot";

/// Text kept per slide for search, in bytes (titles and text past it are not searched).
const MAX_SLIDE_TEXT: usize = 4_096;
/// Longest slide or file title kept, in characters.
const MAX_TITLE: usize = 256;
/// Index text kept across all files; the least recently used files are dropped past it.
const INDEX_BUDGET: usize = 64 << 20;
/// Parsed presentations kept for thumbnails and inserts, and the file bytes they may add up to
/// (the most recently used one is always kept).
const DECKS: usize = 4;
const DECK_BUDGET: u64 = 384 << 20;
/// Thumbnails kept, and the PNG bytes they may add up to.
const THUMBS: usize = 2_048;
const THUMB_BUDGET: usize = 32 << 20;
/// The tallest thumbnail drawn, in pixels (a very tall slide is drawn narrower).
const MAX_THUMB_HEIGHT: f64 = 2_048.0;

/// `folders` as a preferences file may hold them: no empty or over-long paths, no repeats, at most
/// [`MAX_FOLDERS`].
pub fn sanitized_folders(folders: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in folders {
        if out.len() >= MAX_FOLDERS {
            break;
        }
        if !f.trim().is_empty() && f.len() <= MAX_PATH && !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// Is `name` a file the Library lists? Hidden files and Office lock files (`~$…`) are not.
pub fn is_presentation(name: &str) -> bool {
    if name.starts_with('.') || name.starts_with("~$") {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    EXTENSIONS.iter().any(|e| lower.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && ext == *e))
}

/// A presentation found under a library folder.
#[derive(Clone, Debug)]
pub struct Found {
    pub path: PathBuf,
    pub stamp: Stamp,
}

/// What walking one library folder found.
#[derive(Clone, Debug, Default)]
pub struct Walk {
    /// The presentations, folder by folder in name order.
    pub files: Vec<Found>,
    /// Entries looked at, at most [`MAX_ENTRIES`].
    pub entries: usize,
    /// The folder holds more entries than [`MAX_ENTRIES`]; the rest were not looked at.
    pub truncated: bool,
    /// Folders deeper than [`MAX_DEPTH`], not entered.
    pub too_deep: usize,
    /// Links (to files or folders), not followed.
    pub links: usize,
    /// Subfolders that could not be read, with why.
    pub unreadable: Vec<String>,
}

/// Walk the library folder at `root`: an error when the folder itself can't be read (it is
/// missing, or outside the automation read roots); otherwise what was found within the bounds.
pub fn walk(root: &Path) -> Result<Walk, String> {
    let mut w = Walk::default();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut at_root = true;
    while let Some((dir, depth)) = stack.pop() {
        let budget = MAX_ENTRIES.saturating_sub(w.entries);
        if budget == 0 {
            w.truncated = true;
            break;
        }
        let (mut entries, more) = match io::read_dir(&dir, budget) {
            Ok(x) => x,
            Err(e) if at_root => return Err(e),
            Err(e) => {
                w.unreadable.push(e);
                continue;
            }
        };
        at_root = false;
        w.entries += entries.len();
        w.truncated |= more;
        entries.sort_by(|a, b| {
            let key = |e: &io::DirEntry| e.name.as_deref().map(str::to_lowercase);
            key(a).cmp(&key(b)).then_with(|| a.path.cmp(&b.path))
        });
        let mut folders = vec![];
        for e in entries {
            let Some(name) = e.name.as_deref() else { continue };
            if name.starts_with('.') {
                continue;
            }
            match e.kind {
                EntryKind::Link => w.links += 1,
                EntryKind::Folder if depth >= MAX_DEPTH => w.too_deep += 1,
                EntryKind::Folder => folders.push((e.path, depth + 1)),
                EntryKind::File if is_presentation(name) => w.files.push(Found { path: e.path, stamp: e.stamp }),
                EntryKind::File | EntryKind::Other => {}
            }
        }
        // Depth first, in name order: the first folder is walked next.
        stack.extend(folders.into_iter().rev());
    }
    Ok(w)
}

/// One slide as the index holds it.
#[derive(Clone, Debug)]
pub struct SlideEntry {
    pub title: String,
    pub hidden: bool,
    /// Title and text, lower case, for search (at most [`MAX_SLIDE_TEXT`] bytes).
    search: String,
}

/// What the Library knows of one file without opening it again.
#[derive(Clone, Debug)]
pub struct FileIndex {
    /// The presentation's title (File ▸ Properties), else the file name without its extension.
    pub title: String,
    /// The first [`MAX_SLIDES`] slides.
    pub slides: Vec<SlideEntry>,
    /// How many slides the file has (more than `slides` holds past [`MAX_SLIDES`]).
    pub slide_count: usize,
    /// Why the file couldn't be read; it has no slides then.
    pub error: Option<String>,
    stamp: Stamp,
    used: u64,
    bytes: usize,
}

/// How a search matched a slide, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    Title,
    Text,
    File,
}

impl Match {
    pub fn name(self) -> &'static str {
        match self {
            Match::Title => "title",
            Match::Text => "text",
            Match::File => "file",
        }
    }
}

/// `s` cut to at most `max` characters.
fn cap_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => s.get(..i).unwrap_or(s).to_string(),
        None => s.to_string(),
    }
}

/// `s` cut to at most `max` bytes, on a character boundary.
fn cap_bytes(s: &mut String, max: usize) {
    if s.len() > max {
        let mut i = max;
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        s.truncate(i);
    }
}

/// The text on a slide (shapes, grouped shapes and table cells), for search: up to
/// [`MAX_SLIDE_TEXT`] bytes.
pub fn slide_text(slide: &Slide) -> String {
    let mut out = String::new();
    let mut add = |t: &str| {
        if out.len() < MAX_SLIDE_TEXT && !t.trim().is_empty() {
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
    cap_bytes(&mut out, MAX_SLIDE_TEXT);
    out
}

/// The file name without its extension.
pub fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn index_of(path: &Path, doc: &Presentation, stamp: Stamp) -> FileIndex {
    let title = if doc.props.title.trim().is_empty() { stem(path) } else { doc.props.title.trim().to_string() };
    let slides: Vec<SlideEntry> = doc
        .slides
        .iter()
        .take(MAX_SLIDES)
        .map(|sl| {
            let title = cap_chars(sl.title().trim(), MAX_TITLE);
            let mut search = format!("{title}\n{}", slide_text(sl)).to_lowercase();
            cap_bytes(&mut search, MAX_SLIDE_TEXT);
            SlideEntry { title, hidden: sl.hidden, search }
        })
        .collect();
    let bytes = slides.iter().map(|s| s.title.len() + s.search.len() + 64).sum::<usize>() + 256;
    FileIndex { title: cap_chars(&title, MAX_TITLE), slides, slide_count: doc.slides.len(), error: None, stamp, used: 0, bytes }
}

fn failed(path: &Path, stamp: Stamp, error: String) -> FileIndex {
    FileIndex { title: stem(path), slides: vec![], slide_count: 0, error: Some(error), stamp, used: 0, bytes: 256 }
}

/// Open the presentation at `path`, which is no larger than [`MAX_FILE_BYTES`]. A reader that
/// panics on a hostile file gives an error for that file alone.
fn open(path: &Path, stamp: Stamp) -> Result<Presentation, String> {
    if stamp.len > MAX_FILE_BYTES {
        return Err(format!("larger than {} MB; not opened", MAX_FILE_BYTES >> 20));
    }
    let bytes = io::read_path(path, MAX_FILE_BYTES)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::cmd::file::open_presentation(&name, &bytes))) {
        Ok(Ok(doc)) => Ok(doc),
        Ok(Err(e)) => Err(e.to_string()),
        Err(payload) => Err(format!("{name} could not be read ({})", crate::guard::panic_message(payload.as_ref()))),
    }
}

struct Deck {
    path: PathBuf,
    stamp: Stamp,
    doc: Arc<Presentation>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ThumbKey {
    path: PathBuf,
    slide: usize,
    width: u32,
}

struct Thumb {
    stamp: Stamp,
    png: Arc<Vec<u8>>,
    size: (u32, u32),
    used: u64,
}

/// A thumbnail: PNG bytes and their size in pixels.
#[derive(Clone, Debug)]
pub struct Thumbnail {
    pub png: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// What the Library has read, each kind within its budget. A file whose size or modification time
/// changed is read again.
#[derive(Default)]
pub struct Cache {
    index: HashMap<PathBuf, FileIndex>,
    index_bytes: usize,
    /// Least recently used first.
    decks: Vec<Deck>,
    thumbs: HashMap<ThumbKey, Thumb>,
    thumb_bytes: usize,
    tick: u64,
}

impl Cache {
    fn tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    /// The index of the file at `path` (found with `stamp`), reading it when it isn't indexed or
    /// changed since.
    pub fn index(&mut self, path: &Path, stamp: Stamp) -> &FileIndex {
        let now = self.tick();
        let current = self.index.get(path).is_some_and(|f| f.stamp == stamp);
        if !current {
            let fresh = match self.deck(path, stamp) {
                Ok(doc) => index_of(path, &doc, stamp),
                Err(e) => failed(path, stamp, e),
            };
            if let Some(old) = self.index.insert(path.to_path_buf(), fresh) {
                self.index_bytes = self.index_bytes.saturating_sub(old.bytes);
            }
            self.index_bytes = self.index_bytes.saturating_add(self.index.get(path).map_or(0, |f| f.bytes));
            self.evict_index(path);
        }
        // Inserted above when it wasn't there; the fallback is never used.
        let entry = self.index.entry(path.to_path_buf()).or_insert_with(|| failed(path, stamp, "not indexed".into()));
        entry.used = now;
        entry
    }

    /// Drop the least recently used indexes until they fit the budget, never `keep`.
    fn evict_index(&mut self, keep: &Path) {
        while self.index_bytes > INDEX_BUDGET {
            let Some(oldest) = self.index.iter().filter(|(p, _)| p.as_path() != keep).min_by_key(|(_, f)| f.used).map(|(p, _)| p.clone()) else {
                break;
            };
            if let Some(f) = self.index.remove(&oldest) {
                self.index_bytes = self.index_bytes.saturating_sub(f.bytes);
            }
        }
    }

    /// The presentation at `path`, parsed (or from the cache when unchanged).
    pub fn deck(&mut self, path: &Path, stamp: Stamp) -> Result<Arc<Presentation>, String> {
        if let Some(i) = self.decks.iter().position(|d| d.path == path && d.stamp == stamp) {
            let d = self.decks.remove(i);
            let doc = d.doc.clone();
            self.decks.push(d);
            return Ok(doc);
        }
        self.decks.retain(|d| d.path != path);
        let doc = Arc::new(open(path, stamp)?);
        self.decks.push(Deck { path: path.to_path_buf(), stamp, doc: doc.clone() });
        while self.decks.len() > 1 && (self.decks.len() > DECKS || self.decks.iter().map(|d| d.stamp.len).sum::<u64>() > DECK_BUDGET) {
            self.decks.remove(0);
        }
        Ok(doc)
    }

    /// Slide `slide` of the presentation at `path` drawn `width` pixels wide (within
    /// [`THUMB_WIDTHS`]; no taller than 2,048 pixels).
    pub fn thumbnail(&mut self, path: &Path, stamp: Stamp, slide: usize, width: u32) -> Result<Thumbnail, String> {
        let width = width.clamp(*THUMB_WIDTHS.start(), *THUMB_WIDTHS.end());
        let key = ThumbKey { path: path.to_path_buf(), slide, width };
        let now = self.tick();
        if let Some(t) = self.thumbs.get_mut(&key).filter(|t| t.stamp == stamp) {
            t.used = now;
            return Ok(Thumbnail { png: t.png.clone(), width: t.size.0, height: t.size.1 });
        }
        let doc = self.deck(path, stamp)?;
        let n = doc.slides.len().min(MAX_SLIDES);
        if slide >= n {
            return Err(format!("{} has {n} slides; {slide} isn't one of them (indices count from 0)", stem(path)));
        }
        let (png, w, h) = draw_thumbnail(&doc, slide, width);
        let png = Arc::new(png);
        if let Some(old) = self.thumbs.insert(key.clone(), Thumb { stamp, png: png.clone(), size: (w, h), used: now }) {
            self.thumb_bytes = self.thumb_bytes.saturating_sub(old.png.len());
        }
        self.thumb_bytes = self.thumb_bytes.saturating_add(png.len());
        while self.thumbs.len() > 1 && (self.thumbs.len() > THUMBS || self.thumb_bytes > THUMB_BUDGET) {
            let Some(oldest) = self.thumbs.iter().filter(|(k, _)| **k != key).min_by_key(|(_, t)| t.used).map(|(k, _)| k.clone()) else { break };
            if let Some(t) = self.thumbs.remove(&oldest) {
                self.thumb_bytes = self.thumb_bytes.saturating_sub(t.png.len());
            }
        }
        Ok(Thumbnail { png, width: w, height: h })
    }

    /// How many thumbnails, indexes and decks are kept, and their bytes (for tests).
    pub fn sizes(&self) -> (usize, usize, usize, usize) {
        (self.thumbs.len(), self.thumb_bytes, self.index.len(), self.decks.len())
    }
}

/// Draw slide `index` of `doc` `width` pixels wide as PNG; returns the bytes and the size.
fn draw_thumbnail(doc: &Presentation, index: usize, width: u32) -> (Vec<u8>, u32, u32) {
    let (sw, sh) = (doc.slide_size.width, doc.slide_size.height);
    let (sw, sh) = if sw.is_finite() && sh.is_finite() && sw > 0.0 && sh > 0.0 { (sw, sh) } else { (960.0, 540.0) };
    let mut scale = f64::from(width) / sw;
    if sh * scale > MAX_THUMB_HEIGHT {
        scale = MAX_THUMB_HEIGHT / sh;
    }
    // A hostile slide size (a denormal width) must not make an infinite scale.
    if !scale.is_finite() || scale <= 0.0 {
        scale = 1.0;
    }
    let w = ((sw * scale).round() as u32).clamp(1, *THUMB_WIDTHS.end());
    let h = ((sh * scale).round() as u32).clamp(1, MAX_THUMB_HEIGHT as u32);
    let opts = deckcraft_render::RenderOpts { scale, size: Some((w, h)), ..Default::default() };
    let img = deckcraft_render::render_slide(doc, index, &opts);
    (img.to_png(), img.width, img.height)
}

/// A slide a search found.
#[derive(Clone, Debug)]
pub struct Hit {
    pub file: PathBuf,
    pub slide: usize,
    pub title: String,
    pub how: Match,
}

/// The slides of the indexed file `path` whose title, text or file name hold `query` (lower case).
pub fn matches(index: &FileIndex, path: &Path, query: &str) -> Vec<Hit> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let by_name = name.contains(query);
    index
        .slides
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let how = if s.title.to_lowercase().contains(query) {
                Match::Title
            } else if s.search.contains(query) {
                Match::Text
            } else if by_name {
                Match::File
            } else {
                return None;
            };
            Some(Hit { file: path.to_path_buf(), slide: i, title: s.title.clone(), how })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentations_are_named_by_extension_and_hidden_ones_are_not() {
        for yes in ["a.pptx", "A.PPTX", "b.potx", "c.deckcraft", "d.e.pptx"] {
            assert!(is_presentation(yes), "{yes}");
        }
        for no in [".pptx", "~$a.pptx", ".hidden.pptx", "a.ppt", "a.key", "a.pptx.txt", "pptx", "a.ppsx"] {
            assert!(!is_presentation(no), "{no}");
        }
    }

    #[test]
    fn folders_from_a_preferences_file_are_bounded() {
        let mut v: Vec<String> = (0..100).map(|i| format!("/f{i}")).collect();
        v.insert(0, String::new());
        v.insert(1, "/f0".into());
        v.insert(2, "x".repeat(MAX_PATH + 1));
        let s = sanitized_folders(v);
        assert_eq!(s.len(), MAX_FOLDERS);
        assert_eq!(s.first().map(String::as_str), Some("/f0"));
        assert!(s.iter().all(|f| !f.is_empty() && f.len() <= MAX_PATH));
    }

    #[test]
    fn caps_keep_whole_characters() {
        assert_eq!(cap_chars("héllo", 2), "hé");
        let mut s = "ééé".to_string();
        cap_bytes(&mut s, 3);
        assert_eq!(s, "é");
    }
}
