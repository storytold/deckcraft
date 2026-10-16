//! `.deck` folders on disk (`deckcraft_format::folder` holds the format; this is the I/O).
//!
//! Saving writes only what changed: every new or changed file first goes to a hidden temporary
//! sibling (`.<name>.saving`), then all of them are renamed into place, then the files the format
//! owns that no longer belong (a deleted slide, an unused picture) are removed. Files the format
//! doesn't own (a README, `.gitattributes`…) are never touched. A new folder is written in full
//! next to its final place and renamed in one step.
//!
//! Like [`crate::io`], this is a checked gateway: every path read or written here passes the
//! automation roots ([`crate::file_access`]) first, the folder itself and each file in it (a link
//! inside the folder can't reach outside the roots).

use std::path::{Path, PathBuf};

use deckcraft_format::folder::{self, FileMap, KeyGen};
use deckcraft_model::Presentation;

use crate::{EngineError, Result};

/// Subfolders the format owns files in.
const SUBDIRS: [&str; 4] = ["slides", "masters", "media-meta", "media"];
/// Largest file read from a folder.
const MAX_FILE: u64 = 2 << 30;

fn err(path: &Path, e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("{}: {e}", path.display()))
}

/// May `path` be read here (automation roots)?
fn may_read(path: &Path) -> Result<()> {
    crate::file_access::check_read(&path.to_string_lossy()).map_err(EngineError::Other)
}

/// May `path` be written here (automation roots)?
fn may_write(path: &Path) -> Result<()> {
    crate::file_access::check_write(&path.to_string_lossy()).map_err(EngineError::Other)
}

fn trimmed(path: &str) -> &str {
    let t = path.trim_end_matches(['/', '\\']);
    if t.is_empty() { path } else { t }
}

/// Does `path` name a folder deck to save to: a `.deck` path, or an existing folder?
pub fn is_folder_path(path: &str) -> bool {
    let t = trimmed(path);
    t.to_ascii_lowercase().ends_with(&format!(".{}", folder::EXTENSION)) || Path::new(t).is_dir()
}

/// The folder deck `path` opens: the folder itself, or the folder of a `deck.json`.
pub fn folder_to_open(path: &str) -> Option<PathBuf> {
    let p = Path::new(trimmed(path));
    if p.is_dir() {
        return Some(p.to_path_buf());
    }
    (p.file_name().is_some_and(|n| n == folder::DECK_JSON) && p.is_file()).then(|| p.parent().map(Path::to_path_buf)).flatten()
}

/// Owned files of `dir` (relative paths), without reading them.
fn owned_paths(dir: &Path) -> Result<Vec<String>> {
    may_read(dir)?;
    let mut out = vec![];
    for top in [folder::DECK_JSON, folder::ORDER, folder::SECTIONS] {
        if dir.join(top).is_file() {
            out.push(top.to_string());
        }
    }
    for sub in SUBDIRS {
        let d = dir.join(sub);
        if !d.is_dir() {
            continue;
        }
        for e in std::fs::read_dir(&d).map_err(|e| err(&d, e))? {
            let e = e.map_err(|e| err(&d, e))?;
            if !e.file_type().map_err(|e| err(&d, e))?.is_file() {
                continue;
            }
            let Some(name) = e.file_name().to_str().map(String::from) else { continue };
            let rel = format!("{sub}/{name}");
            if folder::is_owned(&rel) {
                out.push(rel);
            }
        }
    }
    Ok(out)
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    may_read(path)?;
    let len = std::fs::metadata(path).map_err(|e| err(path, e))?.len();
    if len > MAX_FILE {
        return Err(err(path, "file is too large"));
    }
    std::fs::read(path).map_err(|e| err(path, e))
}

/// Read the files the format owns in `dir`; media bytes only with `media`.
pub fn read(dir: &Path, media: bool) -> Result<FileMap> {
    let mut files = FileMap::new();
    for rel in owned_paths(dir)? {
        if !media && rel.starts_with("media/") {
            continue;
        }
        let bytes = read_file(&dir.join(&rel))?;
        files.insert(rel, bytes);
    }
    Ok(files)
}

/// Open the folder deck at `dir`.
pub fn open(dir: &Path) -> Result<Presentation> {
    let files = read(dir, true)?;
    folder::from_folder(&files).map_err(|e| err(dir, e))
}

/// What a save did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub written: usize,
    pub removed: usize,
    pub warnings: Vec<String>,
}

/// Save `doc` as a folder deck at `dir`, keeping the slide keys and unknown keys of what is there.
pub fn save(dir: &Path, doc: &Presentation) -> Result<Saved> {
    save_with(dir, doc, &mut KeyGen::system())
}

pub fn save_with(dir: &Path, doc: &Presentation, keys: &mut KeyGen) -> Result<Saved> {
    may_write(dir)?;
    let previous = if dir.exists() {
        if !dir.is_dir() {
            return Err(err(dir, "exists and isn't a folder"));
        }
        let prev = read(dir, false)?;
        let empty = std::fs::read_dir(dir).map_err(|e| err(dir, e))?.next().is_none();
        if !empty && !folder::sniff(&prev) {
            return Err(err(dir, "this folder isn't a DeckCraft folder deck (no deck.json); choose a new or empty folder"));
        }
        Some(prev)
    } else {
        None
    };
    let out = folder::to_folder_with(doc, previous.as_ref(), keys).map_err(|e| err(dir, e))?;
    let (written, removed) = write(dir, &out.files)?;
    Ok(Saved { written, removed, warnings: out.warnings })
}

/// Name of the hidden temporary sibling of `p`.
fn temp_of(p: &Path) -> PathBuf {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    p.with_file_name(format!(".{name}.saving"))
}

fn write_new(dir: &Path, files: &FileMap) -> Result<(usize, usize)> {
    let tmp = temp_of(dir);
    may_write(dir)?;
    may_write(&tmp)?;
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| err(&tmp, e))?;
    }
    let result = (|| {
        for (rel, bytes) in files {
            let p = tmp.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| err(parent, e))?;
            }
            std::fs::write(&p, bytes).map_err(|e| err(&p, e))?;
        }
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| err(parent, e))?;
        }
        std::fs::rename(&tmp, dir).map_err(|e| err(dir, e))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    result.map(|()| (files.len(), 0))
}

/// Size on disk of a saved presentation: its file, or every file of a `.deck` folder. `None` when
/// it can't be read, or is outside the automation read root.
pub fn size_on_disk(p: &Path) -> Option<u64> {
    fn walk(p: &Path, depth: usize) -> Option<u64> {
        let m = std::fs::metadata(p).ok()?;
        if !m.is_dir() {
            return Some(m.len());
        }
        if depth > 4 {
            return Some(0);
        }
        let mut total = 0u64;
        for e in std::fs::read_dir(p).ok()?.flatten() {
            total = total.saturating_add(walk(&e.path(), depth + 1).unwrap_or(0));
        }
        Some(total)
    }
    may_read(p).ok()?;
    walk(p, 0)
}

/// Order in which staged files are moved into place: content before what points at it.
fn rank(rel: &str) -> u8 {
    match rel.split_once('/').map(|(d, _)| d) {
        Some("media") => 0,
        Some("media-meta") => 1,
        Some("masters") => 2,
        Some("slides") => 3,
        _ if rel == folder::DECK_JSON => 6,
        _ if rel == folder::ORDER => 5,
        _ => 4,
    }
}

/// Write `files` into `dir` (see the module comment). Returns (files written, files removed).
pub fn write(dir: &Path, files: &FileMap) -> Result<(usize, usize)> {
    if !dir.exists() {
        return write_new(dir, files);
    }
    let existing = owned_paths(dir)?;
    // 1. Stage what changed. Media files are named by their hash: same name and size, same bytes.
    let mut staged: Vec<(PathBuf, PathBuf, u8)> = vec![];
    let stage = |staged: &mut Vec<(PathBuf, PathBuf, u8)>| -> Result<()> {
        for (rel, bytes) in files {
            let target = dir.join(rel);
            may_write(&target)?;
            let unchanged = if rel.starts_with("media/") {
                std::fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() == bytes.len() as u64)
            } else {
                std::fs::read(&target).is_ok_and(|b| b == *bytes)
            };
            if unchanged {
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| err(parent, e))?;
            }
            let tmp = temp_of(&target);
            std::fs::write(&tmp, bytes).map_err(|e| err(&tmp, e))?;
            staged.push((tmp, target, rank(rel)));
        }
        Ok(())
    };
    if let Err(e) = stage(&mut staged) {
        for (tmp, _, _) in &staged {
            let _ = std::fs::remove_file(tmp);
        }
        return Err(e);
    }
    // 2. Move them into place.
    staged.sort_by_key(|(_, _, r)| *r);
    let written = staged.len();
    for (i, (tmp, target, _)) in staged.iter().enumerate() {
        if let Err(e) = std::fs::rename(tmp, target) {
            for (t, _, _) in staged.iter().skip(i) {
                let _ = std::fs::remove_file(t);
            }
            return Err(err(target, e));
        }
    }
    // 3. Remove the owned files that no longer belong, then emptied subfolders.
    let mut removed = 0;
    for rel in existing.iter().filter(|r| !files.contains_key(*r)) {
        let p = dir.join(rel);
        may_write(&p)?;
        std::fs::remove_file(&p).map_err(|e| err(&p, e))?;
        removed += 1;
    }
    for sub in SUBDIRS {
        let d = dir.join(sub);
        if std::fs::read_dir(&d).is_ok_and(|mut r| r.next().is_none()) {
            let _ = std::fs::remove_dir(&d);
        }
    }
    Ok((written, removed))
}

#[cfg(test)]
mod tests;
