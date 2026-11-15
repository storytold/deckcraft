//! The Library (#958) the Craft apps share: the user's own folders of documents, browsed from a
//! side pane, searched, drawn as thumbnails and brought into the document being edited.
//!
//! This crate is app-neutral, so DeckCraft, WordCraft and GridCraft give the same `library.*`
//! answers from the same code. An app supplies two things:
//!
//! - [`Fs`]: its file access, so its own confinement (DeckCraft's automation read roots) applies
//!   to every folder listing and file read;
//! - [`Kind`]: what its Library holds — the file extensions, how to open one, its items (slides,
//!   pages, sheets) with their titles and text, and how to draw an item's thumbnail.
//!
//! [`Library`] then answers `library.folders`, `addFolder`, `removeFolder`, `list`, `search` and
//! `thumbnail`, and finds the file and opened document an app's own `library.insert` works on.
//! The answers name items with the app's [`Nouns`] (`slides`, `slide`, `slideCount`).
//!
//! Everything read is hostile and bounded:
//!
//! | What | Limit |
//! |---|---|
//! | Folders | [`MAX_FOLDERS`] |
//! | Entries looked at per folder (files and folders of every kind) | [`MAX_ENTRIES`] |
//! | Folder depth below the library folder | [`MAX_DEPTH`] |
//! | File size (larger files are listed with an error, not opened) | [`MAX_FILE_BYTES`] |
//! | Items listed per file | [`MAX_ITEMS`] |
//! | Links | never followed, so nothing outside the folder is reached through one |
//!
//! A file that can't be read is listed with its error rather than dropped, and a reader that
//! panics on a hostile file gives an error for that file alone. What has been read is cached: an
//! index of each file (titles and text, for search), a few opened documents and the thumbnails
//! drawn from them, each within a fixed budget; a file whose size or time changed is read again.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

/// The most folders the Library browses.
pub const MAX_FOLDERS: usize = 32;
/// The most entries (files and folders of any kind) looked at under one library folder.
pub const MAX_ENTRIES: usize = 5_000;
/// The deepest folder entered below a library folder (its own files are at depth 0).
pub const MAX_DEPTH: usize = 8;
/// Files larger than this are listed with an error and never opened.
pub const MAX_FILE_BYTES: u64 = 256 << 20;
/// The most items (slides, pages, sheets) listed, searched and offered per file.
pub const MAX_ITEMS: usize = 2_000;
/// Thumbnail widths, in pixels, and the default.
pub const THUMB_WIDTHS: RangeInclusive<u32> = 32..=512;
pub const THUMB_WIDTH: u32 = 160;
/// The tallest thumbnail an app should draw, in pixels (a very tall item is drawn narrower).
pub const MAX_THUMB_HEIGHT: u32 = 2_048;
/// Search answer sizes, and the default.
pub const SEARCH_LIMITS: RangeInclusive<usize> = 1..=500;
pub const SEARCH_LIMIT: usize = 100;
/// The longest search query, in characters.
pub const MAX_QUERY: usize = 256;
/// The longest folder or file path taken, in bytes.
pub const MAX_PATH: usize = 4096;

/// Text kept per item for search, in bytes (titles and text past it are not searched).
pub const MAX_ITEM_TEXT: usize = 4_096;
/// Longest item or file title kept, in characters.
const MAX_TITLE: usize = 256;
/// Index text kept across all files; the least recently used files are dropped past it.
const INDEX_BUDGET: usize = 64 << 20;
/// Opened documents kept for thumbnails and inserts, and the file bytes they may add up to (the
/// most recently used one is always kept).
const DOCS: usize = 4;
const DOC_BUDGET: u64 = 384 << 20;
/// Thumbnails kept, and the PNG bytes they may add up to.
const THUMBS: usize = 2_048;
const THUMB_BUDGET: usize = 32 << 20;

// ---------- the app's file access ----------

/// What a folder entry is. A link is reported as a link and never followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Folder,
    Link,
    Other,
}

/// A file's size and when it last changed: a cached reading of the file is current while these
/// are the same.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
}

/// One entry of a folder ([`Fs::read_dir`]).
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub path: PathBuf,
    /// The file name; `None` when it isn't UTF-8 (a path a command could not name back).
    pub name: Option<String>,
    pub kind: EntryKind,
    /// For a file; the default for anything else.
    pub stamp: Stamp,
}

/// The app's file access: every folder listing and file read the Library makes goes through it,
/// so the app's confinement applies. Errors are messages for people.
pub trait Fs {
    /// At most `max` entries of the folder at `path`, and whether it holds more. Links must be
    /// reported as [`EntryKind::Link`], not followed.
    fn read_dir(&self, path: &Path, max: usize) -> Result<(Vec<DirEntry>, bool), String>;
    /// What is at `path` (a link reported as a link) and, for a file, its [`Stamp`].
    fn stat(&self, path: &Path) -> Result<(EntryKind, Stamp), String>;
    /// `path` made absolute with its links followed.
    fn canonical(&self, path: &Path) -> Result<PathBuf, String>;
    /// The bytes of the regular file at `path`; more than `max` of them is an error.
    fn read(&self, path: &Path, max: u64) -> Result<Vec<u8>, String>;
}

// ---------- the app's documents ----------

/// What an app calls its items, singular and plural (`slide`, `slides`): the keys of the
/// answers (`slide`, `slides`, `slideCount`) and the words of their errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nouns {
    pub one: &'static str,
    pub many: &'static str,
}

/// One item of a document as the index keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub hidden: bool,
    /// The item's text for search (cut to [`MAX_ITEM_TEXT`] bytes).
    pub text: String,
}

/// A thumbnail: PNG bytes and their size in pixels.
#[derive(Clone, Debug)]
pub struct Thumbnail {
    pub png: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// What one app's Library holds.
pub trait Kind {
    /// An opened document.
    type Doc;
    /// The file extensions listed, lower case, without the dot.
    fn extensions(&self) -> &[&str];
    fn nouns(&self) -> Nouns;
    /// Open the file `name` from its bytes.
    fn open(&self, name: &str, bytes: &[u8]) -> Result<Self::Doc, String>;
    /// The document's own title, if it has one (the file name stands in otherwise).
    fn title(&self, doc: &Self::Doc) -> Option<String>;
    /// How many items the document has.
    fn count(&self, doc: &Self::Doc) -> usize;
    /// Item `index` (below [`Kind::count`]).
    fn item(&self, doc: &Self::Doc, index: usize) -> Item;
    /// Item `index` drawn `width` pixels wide (within [`THUMB_WIDTHS`]) and no taller than
    /// [`MAX_THUMB_HEIGHT`], as PNG.
    fn thumbnail(&self, doc: &Self::Doc, index: usize, width: u32) -> Result<Thumbnail, String>;
}

// ---------- folders ----------

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

/// Is `name` a file with one of `extensions`? Hidden files and Office lock files (`~$…`) are not.
pub fn is_listed(name: &str, extensions: &[&str]) -> bool {
    if name.starts_with('.') || name.starts_with("~$") {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    lower.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && extensions.contains(&ext))
}

/// A document found under a library folder.
#[derive(Clone, Debug)]
pub struct Found {
    pub path: PathBuf,
    pub stamp: Stamp,
}

/// What walking one library folder found.
#[derive(Clone, Debug, Default)]
pub struct Walk {
    /// The documents, folder by folder in name order.
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

/// Walk the library folder at `root` for files with `extensions`: an error when the folder itself
/// can't be read; otherwise what was found within the bounds. Hidden entries (`.…`) are skipped.
pub fn walk(fs: &dyn Fs, root: &Path, extensions: &[&str]) -> Result<Walk, String> {
    let mut w = Walk::default();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut at_root = true;
    while let Some((dir, depth)) = stack.pop() {
        let budget = MAX_ENTRIES.saturating_sub(w.entries);
        if budget == 0 {
            w.truncated = true;
            break;
        }
        let (mut entries, more) = match fs.read_dir(&dir, budget) {
            Ok(x) => x,
            Err(e) if at_root => return Err(e),
            Err(e) => {
                w.unreadable.push(e);
                continue;
            }
        };
        at_root = false;
        entries.truncate(budget);
        w.entries += entries.len();
        w.truncated |= more;
        entries.sort_by(|a, b| {
            let key = |e: &DirEntry| e.name.as_deref().map(str::to_lowercase);
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
                EntryKind::File if is_listed(name, extensions) => w.files.push(Found { path: e.path, stamp: e.stamp }),
                EntryKind::File | EntryKind::Other => {}
            }
        }
        // Depth first, in name order: the first folder is walked next.
        stack.extend(folders.into_iter().rev());
    }
    Ok(w)
}

// ---------- the index ----------

/// One item as the index holds it.
#[derive(Clone, Debug)]
pub struct IndexItem {
    pub title: String,
    pub hidden: bool,
    /// Title and text, lower case, for search (at most [`MAX_ITEM_TEXT`] bytes).
    search: String,
}

/// What the Library knows of one file without opening it again.
#[derive(Clone, Debug)]
pub struct FileIndex {
    /// The document's title, else the file name without its extension.
    pub title: String,
    /// The first [`MAX_ITEMS`] items.
    pub items: Vec<IndexItem>,
    /// How many items the file has (more than `items` holds past [`MAX_ITEMS`]).
    pub count: usize,
    /// Why the file couldn't be read; it has no items then.
    pub error: Option<String>,
    stamp: Stamp,
    used: u64,
    bytes: usize,
}

/// How a search matched an item, best first.
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
pub fn cap_bytes(s: &mut String, max: usize) {
    if s.len() > max {
        let mut i = max;
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        s.truncate(i);
    }
}

/// The file name without its extension.
pub fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn index_of<K: Kind>(kind: &K, path: &Path, doc: &K::Doc, stamp: Stamp) -> FileIndex {
    let title = kind.title(doc).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| stem(path));
    let count = kind.count(doc);
    let items: Vec<IndexItem> = (0..count.min(MAX_ITEMS))
        .map(|i| {
            let it = kind.item(doc, i);
            let title = cap_chars(it.title.trim(), MAX_TITLE);
            let mut text = it.text;
            cap_bytes(&mut text, MAX_ITEM_TEXT);
            let mut search = format!("{title}\n{text}").to_lowercase();
            cap_bytes(&mut search, MAX_ITEM_TEXT);
            IndexItem { title, hidden: it.hidden, search }
        })
        .collect();
    let bytes = items.iter().map(|s| s.title.len() + s.search.len() + 64).sum::<usize>() + 256;
    FileIndex { title: cap_chars(&title, MAX_TITLE), items, count, error: None, stamp, used: 0, bytes }
}

fn failed(path: &Path, stamp: Stamp, error: String) -> FileIndex {
    FileIndex { title: stem(path), items: vec![], count: 0, error: Some(error), stamp, used: 0, bytes: 256 }
}

/// Text of a panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// Open the document at `path`, no larger than [`MAX_FILE_BYTES`]. A reader that panics on a
/// hostile file gives an error for that file alone.
fn open<K: Kind>(kind: &K, fs: &dyn Fs, path: &Path, stamp: Stamp) -> Result<K::Doc, String> {
    if stamp.len > MAX_FILE_BYTES {
        return Err(too_large());
    }
    let bytes = fs.read(path, MAX_FILE_BYTES)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| kind.open(&name, &bytes))) {
        Ok(r) => r,
        Err(payload) => Err(format!("{name} could not be read ({})", panic_message(payload.as_ref()))),
    }
}

fn too_large() -> String {
    format!("larger than {} MB; not opened", MAX_FILE_BYTES >> 20)
}

/// An item a search found.
#[derive(Clone, Debug)]
pub struct Hit {
    pub file: PathBuf,
    pub item: usize,
    pub title: String,
    pub how: Match,
}

/// The items of the indexed file `path` whose title, text or file name hold `query` (lower case).
pub fn matches(index: &FileIndex, path: &Path, query: &str) -> Vec<Hit> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let by_name = name.contains(query);
    index
        .items
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
            Some(Hit { file: path.to_path_buf(), item: i, title: s.title.clone(), how })
        })
        .collect()
}

// ---------- parameters ----------

fn str_param<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}

/// A non-empty `key` no longer than a path may be.
fn path_param<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    let v = str_param(p, key).map(str::trim).filter(|v| !v.is_empty()).ok_or_else(|| format!("missing `{key}`"))?;
    if v.len() > MAX_PATH {
        return Err(format!("`{key}` is longer than {MAX_PATH} bytes"));
    }
    Ok(v)
}

/// A count or index in `key`, which must be in `range` when given.
fn ranged(p: &Value, key: &str, range: RangeInclusive<usize>, what: &str) -> Result<Option<usize>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| range.contains(n))
            .map(Some)
            .ok_or_else(|| format!("`{key}` is {what}, not {v}")),
    }
}

/// The base64 of `data` (standard alphabet, padded).
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (k, shift) in [18u32, 12, 6, 0].iter().enumerate() {
            if k <= chunk.len() {
                out.push(char::from(T.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---------- the Library ----------

struct Opened<D> {
    path: PathBuf,
    stamp: Stamp,
    doc: Arc<D>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ThumbKey {
    path: PathBuf,
    item: usize,
    width: u32,
}

struct Thumb {
    stamp: Stamp,
    thumb: Thumbnail,
    used: u64,
}

/// One app's Library: its [`Kind`] and what it has read, each kind of thing within its budget.
pub struct Library<K: Kind> {
    kind: K,
    index: HashMap<PathBuf, FileIndex>,
    index_bytes: usize,
    /// Least recently used first.
    docs: Vec<Opened<K::Doc>>,
    thumbs: HashMap<ThumbKey, Thumb>,
    thumb_bytes: usize,
    tick: u64,
}

impl<K: Kind + Default> Default for Library<K> {
    fn default() -> Self {
        Self::new(K::default())
    }
}

impl<K: Kind> Library<K> {
    pub fn new(kind: K) -> Self {
        Library { kind, index: HashMap::new(), index_bytes: 0, docs: vec![], thumbs: HashMap::new(), thumb_bytes: 0, tick: 0 }
    }

    pub fn kind(&self) -> &K {
        &self.kind
    }

    fn tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    /// The index of the file at `path` (found with `stamp`), reading it when it isn't indexed or
    /// changed since.
    pub fn index(&mut self, fs: &dyn Fs, path: &Path, stamp: Stamp) -> &FileIndex {
        let now = self.tick();
        let current = self.index.get(path).is_some_and(|f| f.stamp == stamp);
        if !current {
            let fresh = match self.doc(fs, path, stamp) {
                Ok(doc) => index_of(&self.kind, path, &doc, stamp),
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

    /// The document at `path`, opened (or from the cache when unchanged).
    pub fn doc(&mut self, fs: &dyn Fs, path: &Path, stamp: Stamp) -> Result<Arc<K::Doc>, String> {
        if let Some(i) = self.docs.iter().position(|d| d.path == path && d.stamp == stamp) {
            let d = self.docs.remove(i);
            let doc = d.doc.clone();
            self.docs.push(d);
            return Ok(doc);
        }
        self.docs.retain(|d| d.path != path);
        let doc = Arc::new(open(&self.kind, fs, path, stamp)?);
        self.docs.push(Opened { path: path.to_path_buf(), stamp, doc: doc.clone() });
        while self.docs.len() > 1 && (self.docs.len() > DOCS || self.docs.iter().map(|d| d.stamp.len).sum::<u64>() > DOC_BUDGET) {
            self.docs.remove(0);
        }
        Ok(doc)
    }

    /// Item `item` of the document at `path` drawn `width` pixels wide.
    pub fn thumbnail_of(&mut self, fs: &dyn Fs, path: &Path, stamp: Stamp, item: usize, width: u32) -> Result<Thumbnail, String> {
        let width = width.clamp(*THUMB_WIDTHS.start(), *THUMB_WIDTHS.end());
        let key = ThumbKey { path: path.to_path_buf(), item, width };
        let now = self.tick();
        if let Some(t) = self.thumbs.get_mut(&key).filter(|t| t.stamp == stamp) {
            t.used = now;
            return Ok(t.thumb.clone());
        }
        let doc = self.doc(fs, path, stamp)?;
        let n = self.kind.count(&doc).min(MAX_ITEMS);
        if item >= n {
            let many = self.kind.nouns().many;
            return Err(format!("{} has {n} {many}; {item} isn't one of them (indices count from 0)", stem(path)));
        }
        let mut thumb = self.kind.thumbnail(&doc, item, width)?;
        thumb.width = thumb.width.min(*THUMB_WIDTHS.end());
        thumb.height = thumb.height.min(MAX_THUMB_HEIGHT);
        if let Some(old) = self.thumbs.insert(key.clone(), Thumb { stamp, thumb: thumb.clone(), used: now }) {
            self.thumb_bytes = self.thumb_bytes.saturating_sub(old.thumb.png.len());
        }
        self.thumb_bytes = self.thumb_bytes.saturating_add(thumb.png.len());
        while self.thumbs.len() > 1 && (self.thumbs.len() > THUMBS || self.thumb_bytes > THUMB_BUDGET) {
            let Some(oldest) = self.thumbs.iter().filter(|(k, _)| **k != key).min_by_key(|(_, t)| t.used).map(|(k, _)| k.clone()) else { break };
            if let Some(t) = self.thumbs.remove(&oldest) {
                self.thumb_bytes = self.thumb_bytes.saturating_sub(t.thumb.png.len());
            }
        }
        Ok(thumb)
    }

    /// How many thumbnails, indexes and opened documents are kept, and the thumbnails' bytes.
    pub fn sizes(&self) -> (usize, usize, usize, usize) {
        (self.thumbs.len(), self.thumb_bytes, self.index.len(), self.docs.len())
    }

    // ---------- the library.* contract ----------

    /// The Library folder `given` names: as kept, or the same folder written another way.
    pub fn find_folder(fs: &dyn Fs, folders: &[String], given: &str) -> Option<String> {
        if let Some(f) = folders.iter().find(|f| f.as_str() == given) {
            return Some(f.clone());
        }
        let real = fs.canonical(Path::new(given)).ok()?;
        folders.iter().find(|f| Path::new(f.as_str()) == real || fs.canonical(Path::new(f.as_str())).is_ok_and(|r| r == real)).cloned()
    }

    /// The folders a command covers: `folder` when given (it must be a Library folder), else all;
    /// and whether one was named.
    fn chosen(fs: &dyn Fs, folders: &[String], p: &Value) -> Result<(Vec<String>, bool), String> {
        match str_param(p, "folder").filter(|f| !f.trim().is_empty()) {
            Some(_) => {
                let f = path_param(p, "folder")?;
                let found = Self::find_folder(fs, folders, f).ok_or_else(|| format!("{f} is not a Library folder (library.folders lists them)"))?;
                Ok((vec![found], true))
            }
            None => Ok((folders.to_vec(), false)),
        }
    }

    /// The documents under `folders`, each once, with the folder it was found in. A folder that
    /// can't be read is skipped when listing all of them (library.folders says why) and an error
    /// when it was asked for by name.
    fn files_in(&self, fs: &dyn Fs, folders: &[String], named: bool) -> Result<Vec<(PathBuf, Stamp, String)>, String> {
        let mut seen = HashSet::new();
        let mut out = vec![];
        for f in folders {
            let w = match walk(fs, Path::new(f), self.kind.extensions()) {
                Ok(w) => w,
                Err(e) if named => return Err(e),
                Err(_) => continue,
            };
            for found in w.files {
                if seen.insert(found.path.clone()) {
                    out.push((found.path, found.stamp, f.clone()));
                }
            }
        }
        Ok(out)
    }

    /// The document `file` names: one of the app's file kinds inside a Library folder (following
    /// its links), readable through `fs`. Its real path and stamp, and the folder it is in.
    pub fn file(&self, fs: &dyn Fs, folders: &[String], p: &Value) -> Result<(PathBuf, Stamp, String), String> {
        let file = path_param(p, "file")?;
        let path = Path::new(file);
        let exts = self.kind.extensions();
        if !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| is_listed(n, exts)) {
            let kinds: Vec<String> = exts.iter().map(|e| format!(".{e}")).collect();
            return Err(format!("{file} is not a {} file", kinds.join(", ")));
        }
        let real = fs.canonical(path)?;
        let folder = folders
            .iter()
            .find(|f| fs.canonical(Path::new(f.as_str())).is_ok_and(|root| real.starts_with(root)))
            .cloned()
            .ok_or_else(|| format!("{file} is not in a Library folder; library.addFolder adds its folder"))?;
        let (kind, stamp) = fs.stat(&real)?;
        if kind != EntryKind::File {
            return Err(format!("{file} is not a file"));
        }
        Ok((real, stamp, folder))
    }

    /// The required item index (`slide`, `page`…).
    pub fn item_param(&self, p: &Value) -> Result<usize, String> {
        let one = self.kind.nouns().one;
        match p.get(one) {
            None | Some(Value::Null) => Err(format!("missing `{one}` (an index from library.list, counting from 0)")),
            Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| format!("`{one}` is not a valid index: {v}")),
        }
    }

    /// `library.folders`: `[{path, files, entries, truncated?, skippedDeep?, skippedLinks?,
    /// unreadable?, error?}]`.
    pub fn folders(&self, fs: &dyn Fs, folders: &[String]) -> Value {
        let out: Vec<Value> = folders
            .iter()
            .map(|f| match walk(fs, Path::new(f), self.kind.extensions()) {
                Ok(w) => {
                    let mut v = json!({"path": f, "files": w.files.len(), "entries": w.entries});
                    if w.truncated {
                        v["truncated"] = json!(true);
                    }
                    if w.too_deep > 0 {
                        v["skippedDeep"] = json!(w.too_deep);
                    }
                    if w.links > 0 {
                        v["skippedLinks"] = json!(w.links);
                    }
                    if !w.unreadable.is_empty() {
                        v["unreadable"] = json!(w.unreadable.iter().take(16).collect::<Vec<_>>());
                    }
                    v
                }
                Err(e) => json!({"path": f, "files": 0, "entries": 0, "error": e}),
            })
            .collect();
        Value::Array(out)
    }

    /// `library.addFolder {path}` → `{path, added, folders}`: the folder, made absolute with its
    /// links followed, joins `folders` (at most [`MAX_FOLDERS`]).
    pub fn add_folder(fs: &dyn Fs, folders: &mut Vec<String>, p: &Value) -> Result<Value, String> {
        let given = path_param(p, "path")?;
        let real = fs.canonical(Path::new(given))?;
        let (kind, _) = fs.stat(&real)?;
        if kind != EntryKind::Folder {
            return Err(format!("{given} is not a folder"));
        }
        let real = real.to_str().ok_or_else(|| format!("{given}: the folder's path is not valid Unicode"))?.to_string();
        if real.len() > MAX_PATH {
            return Err(format!("{given}: the folder's path is longer than {MAX_PATH} bytes"));
        }
        let added = !folders.contains(&real);
        if added {
            if folders.len() >= MAX_FOLDERS {
                return Err(format!("the Library holds at most {MAX_FOLDERS} folders; remove one first"));
            }
            folders.push(real.clone());
        }
        Ok(json!({"path": real, "added": added, "folders": folders}))
    }

    /// `library.removeFolder {path}` → `{path, folders}`.
    pub fn remove_folder(fs: &dyn Fs, folders: &mut Vec<String>, p: &Value) -> Result<Value, String> {
        let given = path_param(p, "path")?;
        let found = Self::find_folder(fs, folders, given).ok_or_else(|| format!("{given} is not a Library folder (library.folders lists them)"))?;
        folders.retain(|f| *f != found);
        Ok(json!({"path": found, "folders": folders}))
    }

    /// One file as `library.list` answers it.
    fn file_json(&mut self, fs: &dyn Fs, path: &Path, stamp: Stamp, folder: &str, open: bool) -> Value {
        let mut v = json!({"file": path.to_string_lossy(), "folder": folder});
        if !open {
            v["title"] = json!(stem(path));
            if stamp.len > MAX_FILE_BYTES {
                v["error"] = json!(too_large());
            }
            return v;
        }
        let Nouns { one, many } = self.kind.nouns();
        let idx = self.index(fs, path, stamp);
        v["title"] = json!(idx.title);
        v[many] = idx.items.iter().enumerate().map(|(i, it)| json!({"index": i, "title": it.title, "hidden": it.hidden})).collect();
        v[format!("{one}Count")] = json!(idx.count);
        if idx.count > idx.items.len() {
            v["truncated"] = json!(true);
        }
        if let Some(e) = &idx.error {
            v["error"] = json!(e);
        }
        v
    }

    /// `library.list {folder?, file?, <many>?: bool}` → `[{file, folder, title, <many>: [{index,
    /// title, hidden}], <one>Count, truncated?, error?}]`; with `<many>: false` the files are
    /// listed without being opened.
    pub fn list(&mut self, fs: &dyn Fs, folders: &[String], p: &Value) -> Result<Value, String> {
        let open = p.get(self.kind.nouns().many).and_then(Value::as_bool).unwrap_or(true);
        let files = if str_param(p, "file").is_some() {
            vec![self.file(fs, folders, p)?]
        } else {
            let (chosen, named) = Self::chosen(fs, folders, p)?;
            self.files_in(fs, &chosen, named)?
        };
        Ok(Value::Array(files.iter().map(|(path, stamp, folder)| self.file_json(fs, path, *stamp, folder, open)).collect()))
    }

    /// `library.search {query, limit?, folder?}` → `[{file, <one>, title, match}]`, title matches
    /// first, then text, then file names.
    pub fn search(&mut self, fs: &dyn Fs, folders: &[String], p: &Value) -> Result<Value, String> {
        let query = str_param(p, "query").map(str::trim).filter(|q| !q.is_empty()).ok_or("missing `query`")?;
        if query.chars().count() > MAX_QUERY {
            return Err(format!("`query` is longer than {MAX_QUERY} characters"));
        }
        let limit = ranged(p, "limit", SEARCH_LIMITS, "a count from 1 to 500")?.unwrap_or(SEARCH_LIMIT);
        let query = query.to_lowercase();
        let (chosen, named) = Self::chosen(fs, folders, p)?;
        let files = self.files_in(fs, &chosen, named)?;
        // The first `limit` of each kind of match is enough to rank.
        let mut kinds: [Vec<Hit>; 3] = Default::default();
        for (path, stamp, _) in &files {
            let idx = self.index(fs, path, *stamp);
            for hit in matches(idx, path, &query) {
                let k = match hit.how {
                    Match::Title => 0,
                    Match::Text => 1,
                    Match::File => 2,
                };
                if let Some(v) = kinds.get_mut(k).filter(|v| v.len() < limit) {
                    v.push(hit);
                }
            }
        }
        let one = self.kind.nouns().one;
        let hits: Vec<Value> = kinds
            .iter()
            .flatten()
            .take(limit)
            .map(|h| {
                let mut v = json!({"file": h.file.to_string_lossy(), "title": h.title, "match": h.how.name()});
                v[one] = json!(h.item);
                v
            })
            .collect();
        Ok(Value::Array(hits))
    }

    /// `library.thumbnail {file, <one>, width?}` → `{png: base64, width, height}`.
    pub fn thumbnail(&mut self, fs: &dyn Fs, folders: &[String], p: &Value) -> Result<Value, String> {
        let (path, stamp, _) = self.file(fs, folders, p)?;
        let item = self.item_param(p)?;
        let width = match p.get("width") {
            None | Some(Value::Null) => THUMB_WIDTH,
            Some(v) => v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| THUMB_WIDTHS.contains(n))
                .ok_or_else(|| format!("`width` is 32 to 512 pixels, not {v}"))?,
        };
        let t = self.thumbnail_of(fs, &path, stamp, item, width)?;
        Ok(json!({"png": base64(&t.png), "width": t.width, "height": t.height}))
    }
}

#[cfg(test)]
mod tests;
