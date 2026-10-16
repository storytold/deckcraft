//! The `.deck` folder: the same document as a `.deckcraft` file, split into indented JSON files
//! that diff and merge well in Git (`docs/deck-folder-format.md`).
//!
//! ```text
//! talk.deck/
//! ├── deck.json          format, version and the presentation fields not stored elsewhere
//! ├── slides.order       slide order and sections, one slide key per line
//! ├── slides/<KEY>.json  one slide per file, named by a stable ULID key
//! ├── masters/<id>.json  one master with its layouts and theme (+ notes.json, handout.json)
//! ├── media-meta/<id>.json  {"id", "name", "contentType", "link"?, "sha256"}
//! └── media/<sha256>.<ext>  media bytes, named by content
//! ```
//!
//! Everything here works on an in-memory [`FileMap`] (pure, wasm-safe); the app and the CLI do
//! the disk I/O. [`from_folder`] ends in the same media and [`repair`](crate::repair) path as
//! [`load`](crate::load).

mod json;
mod key;
mod order;
mod sha256;

use std::collections::{BTreeMap, HashMap, HashSet};

use deckcraft_model::{Master, MediaItem, Presentation, Section, Slide};
use serde_json::{Map, Value, json};

pub use key::{KeyGen, is_key};

use crate::{FormatError, Result};
use json::J;
use order::{Order, Sections};

/// Files of a folder, by relative path with `/` separators.
pub type FileMap = BTreeMap<String, Vec<u8>>;

/// Folder name extension (`talk.deck`).
pub const EXTENSION: &str = "deck";
/// `format` written in a new folder's `deck.json`.
pub const FORMAT: &str = "deckcraft-folder";
/// `format` values read: ours, and `deckgit`, the external tool this format comes from. An
/// existing folder keeps its value when saved again.
pub const FORMATS: [&str; 2] = [FORMAT, "deckgit"];
/// Folder layout version.
pub const VERSION: u64 = 1;
pub const DECK_JSON: &str = "deck.json";
pub const ORDER: &str = "slides.order";
pub const SECTIONS: &str = "sections.json";

/// Presentation fields stored in their own files, or not at all (`nextId`, R9).
const SPLIT_KEYS: [&str; 7] = ["slides", "masters", "notesMaster", "handoutMaster", "sections", "media", "nextId"];
/// `deck.json` keys this version writes.
const DECK_KEYS: [&str; 6] = ["format", "version", "deckcraftFormatVersion", "presentation", "mastersOrder", "mediaOrder"];
/// Largest folder file read into memory.
const MAX_FILE: usize = 2 << 30;

/// A folder made by [`to_folder_with`], and what it couldn't write in the usual layout.
#[derive(Debug, Default)]
pub struct Folder {
    pub files: FileMap,
    pub warnings: Vec<String>,
}

/// Is `rel` a file this format owns (written, replaced or removed on save)? Anything else in the
/// folder (a README, `.gitattributes`, notes…) is left alone.
pub fn is_owned(rel: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit());
    match rel.split_once('/') {
        None => matches!(rel, DECK_JSON | ORDER | SECTIONS),
        Some(("slides", f)) => f.strip_suffix(".json").is_some_and(is_key),
        Some(("masters", f)) => matches!(f, "notes.json" | "handout.json") || f.strip_suffix(".json").is_some_and(digits),
        Some(("media-meta", f)) => f.strip_suffix(".json").is_some_and(digits),
        Some(("media", f)) => {
            let (hash, ext) = f.split_once('.').unwrap_or((f, ""));
            hash.len() == 64
                && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && ext.len() <= 16
                && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        }
        _ => false,
    }
}

/// Does this file map look like a folder deck (has a `deck.json` of a known format)?
pub fn sniff(files: &FileMap) -> bool {
    files
        .get(DECK_JSON)
        .and_then(|b| serde_json::from_slice::<Value>(b).ok())
        .and_then(|v| v.get("format").and_then(Value::as_str).map(|f| FORMATS.contains(&f)))
        .unwrap_or(false)
}

/// Extension of a media file, from its content type, else from its original name.
pub fn media_ext(content_type: &str, name: &str) -> String {
    let ct = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let e = match ct.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/x-emf" | "image/emf" => "emf",
        "image/x-wmf" | "image/wmf" => "wmf",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/wav" | "audio/x-wav" => "wav",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        _ => "",
    };
    if !e.is_empty() {
        return e.into();
    }
    match name.rsplit_once('.') {
        Some((_, x)) if !x.is_empty() && x.len() <= 6 && x.bytes().all(|b| b.is_ascii_alphanumeric()) => x.to_ascii_lowercase(),
        _ => "bin".into(),
    }
}

fn write_err(e: String) -> FormatError {
    FormatError::Write(e)
}

/// Slide id → key, from the slide files of an existing folder (R4). Unreadable files are skipped.
fn slide_keys(files: &FileMap) -> HashMap<u64, String> {
    let mut out = HashMap::new();
    for (path, bytes) in files {
        let Some(key) = path.strip_prefix("slides/").and_then(|p| p.strip_suffix(".json")).filter(|k| is_key(k)) else { continue };
        if let Some(id) = serde_json::from_slice::<Value>(bytes).ok().and_then(|v| v.get("id").and_then(Value::as_u64)) {
            out.entry(id).or_insert_with(|| key.to_string());
        }
    }
    out
}

fn prev_json(prev: &FileMap, path: &str) -> Option<J> {
    prev.get(path).and_then(|b| json::parse(b).ok())
}

fn ascending(ids: &[u64]) -> bool {
    ids.windows(2).all(|w| matches!(w, [a, b] if a < b))
}

fn ids_json(ids: &[u64]) -> J {
    J::Arr(ids.iter().map(|i| J::Num((*i).into())).collect())
}

/// The folder for `p` with fresh keys from the system clock and randomness. `previous` is the
/// folder as it is on disk: its slide keys are kept (matched by slide id), and its files come out
/// byte-identical when nothing changed.
pub fn to_folder(p: &Presentation, previous: Option<&FileMap>) -> Result<FileMap> {
    to_folder_with(p, previous, &mut KeyGen::system()).map(|f| f.files)
}

/// [`to_folder`] with a given key generator, also returning warnings.
pub fn to_folder_with(p: &Presentation, previous: Option<&FileMap>, keys: &mut KeyGen) -> Result<Folder> {
    let empty = FileMap::new();
    let prev = previous.unwrap_or(&empty);
    let mut out = Folder::default();
    let files = &mut out.files;

    // Slides: one file each, key found again by the numeric id (R3, R4).
    let prev_keys = slide_keys(prev);
    let mut used: HashSet<String> = HashSet::new();
    let mut order_keys = vec![];
    let mut key_of_id: HashMap<u64, String> = HashMap::new();
    for s in &p.slides {
        let id = u64::from(s.id.0);
        let key = match prev_keys.get(&id) {
            Some(k) if !used.contains(k) => k.clone(),
            _ => {
                let mut k = keys.key();
                for _ in 0..16 {
                    if !used.contains(&k) && !prev.contains_key(&format!("slides/{k}.json")) {
                        break;
                    }
                    k = keys.key();
                }
                k
            }
        };
        used.insert(key.clone());
        let path = format!("slides/{key}.json");
        let v = json::canonical::<Slide>(s, prev_json(prev, &path).as_ref()).map_err(write_err)?;
        files.insert(path, json::pretty(&v));
        key_of_id.entry(id).or_insert_with(|| key.clone());
        order_keys.push(key);
    }

    // Order and sections (R8).
    let header = prev
        .get(ORDER)
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(|t| t.lines().next())
        .filter(|l| l.starts_with('#') && l.contains("slides.order"))
        .unwrap_or(order::HEADER)
        .trim_end_matches('\r')
        .to_string();
    let sections: Option<Sections> = p
        .sections
        .iter()
        .map(|sec| {
            let ks: Option<Vec<String>> = sec.slides.iter().map(|id| key_of_id.get(&u64::from(id.0)).cloned()).collect();
            ks.map(|ks| (sec.name.clone(), ks))
        })
        .collect();
    match sections.and_then(|s| Order::build(&order_keys, &s)) {
        Some(o) => {
            files.insert(ORDER.into(), o.render(&header).into_bytes());
        }
        None => {
            let o = Order::build(&order_keys, &vec![]).ok_or_else(|| FormatError::Write("slide order can't be written".into()))?;
            files.insert(ORDER.into(), o.render(&header).into_bytes());
            let v = json::canonical::<Vec<Section>>(&p.sections, prev_json(prev, SECTIONS).as_ref()).map_err(write_err)?;
            files.insert(SECTIONS.into(), json::pretty(&v));
            out.warnings.push("sections aren't contiguous runs of slides in slide order: written as they are in sections.json".into());
        }
    }

    // Masters, notes master, handout master.
    let mut extra: Vec<(String, J)> = vec![];
    let master_ids: Vec<u64> = p.masters.iter().map(|m| u64::from(m.id.0)).collect();
    for (m, id) in p.masters.iter().zip(&master_ids) {
        let path = format!("masters/{id}.json");
        if files.contains_key(&path) {
            return Err(FormatError::Write(format!("two masters share id {id}")));
        }
        let v = json::canonical::<Master>(m, prev_json(prev, &path).as_ref()).map_err(write_err)?;
        files.insert(path, json::pretty(&v));
    }
    if !ascending(&master_ids) {
        extra.push(("mastersOrder".into(), ids_json(&master_ids)));
        out.warnings.push("masters aren't in id order: their order is written in deck.json (mastersOrder)".into());
    }
    for (m, file) in [(&p.notes_master, "masters/notes.json"), (&p.handout_master, "masters/handout.json")] {
        if let Some(m) = m {
            let v = json::canonical::<Master>(m, prev_json(prev, file).as_ref()).map_err(write_err)?;
            files.insert(file.into(), json::pretty(&v));
        }
    }

    // Media: a record per id, bytes per content hash (R6).
    let media_ids: Vec<u64> = p.media.iter().map(|m| u64::from(m.id.0)).collect();
    for (item, id) in p.media.iter().zip(&media_ids) {
        let path = format!("media-meta/{id}.json");
        if files.contains_key(&path) {
            return Err(FormatError::Write(format!("two media items share id {id}")));
        }
        let mut prevj = prev_json(prev, &path);
        if let Some(pj) = prevj.as_mut() {
            pj.remove("sha256");
        }
        let mut v = json::canonical::<MediaItem>(item, prevj.as_ref()).map_err(write_err)?;
        // Like the zip: linked media without bytes has no bytes file.
        if !(item.link.is_some() && item.data.is_empty()) {
            let sha = sha256::hex(&item.data);
            let bytes_path = format!("media/{sha}.{}", media_ext(&item.content_type, &item.name));
            files.entry(bytes_path).or_insert_with(|| item.data.to_vec());
            v.set("sha256", J::Str(sha));
        }
        files.insert(path, json::pretty(&v));
    }
    if !ascending(&media_ids) {
        extra.push(("mediaOrder".into(), ids_json(&media_ids)));
        out.warnings.push("media aren't in id order: their order is written in deck.json (mediaOrder)".into());
    }

    // deck.json: the remaining presentation fields, checked as a presentation whose split parts
    // are empty.
    let rest = Presentation {
        slides: vec![],
        masters: vec![],
        notes_master: None,
        handout_master: None,
        sections: vec![],
        media: vec![],
        next_id: 0,
        ..p.clone()
    };
    let prev_deck = prev_json(prev, DECK_JSON);
    let mut restj = json::canonical::<Presentation>(&rest, prev_deck.as_ref().and_then(|d| d.get("presentation"))).map_err(write_err)?;
    for k in SPLIT_KEYS {
        restj.remove(k);
    }
    let format = prev_deck.as_ref().and_then(|d| d.get("format")).and_then(J::as_str).filter(|f| FORMATS.contains(f)).unwrap_or(FORMAT).to_string();
    let mut deck = vec![
        ("format".to_string(), J::Str(format)),
        ("version".to_string(), J::Num(VERSION.into())),
        ("deckcraftFormatVersion".to_string(), J::Num(u64::from(crate::VERSION).into())),
        ("presentation".to_string(), restj),
    ];
    deck.extend(extra);
    // Keys a newer version (or another tool) put in deck.json stay.
    if let Some(J::Obj(m)) = &prev_deck {
        deck.extend(m.iter().filter(|(k, _)| !DECK_KEYS.contains(&k.as_str())).cloned());
    }
    files.insert(DECK_JSON.into(), json::pretty(&J::Obj(deck)));
    Ok(out)
}

fn damaged(msg: impl Into<String>) -> FormatError {
    FormatError::Damaged(msg.into())
}

fn parse_file(files: &FileMap, path: &str) -> Result<Value> {
    let b = files.get(path).ok_or_else(|| damaged(format!("{path} is missing")))?;
    if b.len() > MAX_FILE {
        return Err(damaged(format!("{path} is too large")));
    }
    serde_json::from_slice(b).map_err(|e| damaged(format!("{path}: {e}")))
}

fn id_of(v: &Value) -> Option<u64> {
    v.get("id").and_then(Value::as_u64)
}

/// `v` sorted by id, or in the explicit order of `deck.json` (`mastersOrder`, `mediaOrder`).
fn in_order(mut v: Vec<Value>, explicit: Option<&Value>, what: &str) -> Result<Vec<Value>> {
    match explicit.and_then(Value::as_array) {
        Some(order) => {
            let mut by_id: BTreeMap<u64, Value> = v.into_iter().filter_map(|x| id_of(&x).map(|i| (i, x))).collect();
            let mut out = vec![];
            for id in order.iter().filter_map(Value::as_u64) {
                out.push(by_id.remove(&id).ok_or_else(|| damaged(format!("deck.json: {what} lists {id}, which has no file")))?);
            }
            out.extend(by_id.into_values());
            Ok(out)
        }
        None => {
            v.sort_by_key(id_of);
            Ok(v)
        }
    }
}

/// Read a folder: the same model a `.deckcraft` with this content gives, after the same repair.
pub fn from_folder(files: &FileMap) -> Result<Presentation> {
    let deck = parse_file(files, DECK_JSON).map_err(|_| FormatError::NotOurs("no readable deck.json".into()))?;
    let format = deck.get("format").and_then(Value::as_str).unwrap_or("");
    if !FORMATS.contains(&format) {
        return Err(FormatError::NotOurs(format!("deck.json format `{format}`")));
    }
    match deck.get("version").and_then(Value::as_u64) {
        Some(VERSION) => {}
        v => return Err(damaged(format!("deck.json: folder version {v:?} isn't supported (this build reads {VERSION})"))),
    }
    let rest = deck.get("presentation").and_then(Value::as_object).cloned().unwrap_or_default();

    // Slides in slides.order order; slide files it doesn't list come last rather than being lost
    // (an interrupted save, a hand edit).
    let order_text = files.get(ORDER).ok_or_else(|| damaged("slides.order is missing"))?;
    let order = Order::parse(&String::from_utf8_lossy(order_text)).map_err(damaged)?;
    let mut keys: Vec<&str> = order.keys();
    let mut seen = HashSet::new();
    for k in &keys {
        if !seen.insert(*k) {
            return Err(damaged(format!("slides.order lists {k} twice (an unresolved merge?)")));
        }
    }
    let unlisted: Vec<&str> = files
        .keys()
        .filter_map(|p| p.strip_prefix("slides/").and_then(|p| p.strip_suffix(".json")))
        .filter(|k| is_key(k) && !seen.contains(k))
        .collect();
    keys.extend(unlisted);
    let mut slides = vec![];
    let mut id_of_key = HashMap::new();
    for k in &keys {
        let v = parse_file(files, &format!("slides/{k}.json"))?;
        if let Some(id) = id_of(&v) {
            id_of_key.insert(k.to_string(), id);
        }
        slides.push(v);
    }
    let sections = if files.contains_key(SECTIONS) {
        parse_file(files, SECTIONS)?
    } else {
        Value::Array(
            order
                .sections()
                .into_iter()
                .map(|(name, ks)| json!({"name": name, "slides": ks.iter().filter_map(|k| id_of_key.get(k)).collect::<Vec<_>>()}))
                .collect(),
        )
    };

    // Masters.
    let mut masters = vec![];
    for p in files.keys().filter(|p| p.starts_with("masters/") && is_owned(p)) {
        if p != "masters/notes.json" && p != "masters/handout.json" {
            masters.push(parse_file(files, p)?);
        }
    }
    let masters = in_order(masters, deck.get("mastersOrder"), "mastersOrder")?;
    let optional = |path: &str| -> Result<Value> { if files.contains_key(path) { parse_file(files, path) } else { Ok(Value::Null) } };
    let notes_master = optional("masters/notes.json")?;
    let handout_master = optional("masters/handout.json")?;

    // Media records, and their bytes found by hash.
    let mut metas = vec![];
    for p in files.keys().filter(|p| p.starts_with("media-meta/") && is_owned(p)) {
        metas.push(parse_file(files, p)?);
    }
    let metas = in_order(metas, deck.get("mediaOrder"), "mediaOrder")?;
    let mut items = vec![];
    let mut data: HashMap<u64, Vec<u8>> = HashMap::new();
    for mut m in metas {
        let sha = m.as_object_mut().and_then(|o| o.remove("sha256"));
        if let (Some(sha), Some(id)) = (sha.as_ref().and_then(Value::as_str), id_of(&m)) {
            let prefix = format!("media/{sha}");
            let (path, bytes) = files
                .range(prefix.clone()..)
                .take_while(|(p, _)| p.starts_with(&prefix))
                .find(|(p, _)| p.strip_prefix(&prefix).is_some_and(|r| r.is_empty() || r.starts_with('.')))
                .ok_or_else(|| damaged(format!("media {id}: media/{sha}.* is missing")))?;
            if sha256::hex(bytes) != sha {
                let hint = if bytes.starts_with(b"version https://git-lfs") { " (a Git LFS pointer: run `git lfs pull`)" } else { "" };
                return Err(damaged(format!("{path}: content doesn't match its SHA-256{hint}")));
            }
            data.insert(id, bytes.clone());
        }
        items.push(m);
    }

    // The presentation object, as in a `.deckcraft` manifest; `nextId` is recomputed by repair.
    let mut pres = Map::new();
    pres.insert("slides".into(), Value::Array(slides));
    pres.insert("masters".into(), Value::Array(masters));
    pres.insert("notesMaster".into(), notes_master);
    pres.insert("handoutMaster".into(), handout_master);
    pres.insert("sections".into(), sections);
    pres.insert("media".into(), Value::Array(items));
    pres.insert("nextId".into(), json!(0));
    for (k, v) in rest {
        if !SPLIT_KEYS.contains(&k.as_str()) {
            pres.insert(k, v);
        }
    }
    let p: Presentation = serde_json::from_value(Value::Object(pres)).map_err(|e| damaged(format!("{e}")))?;
    Ok(crate::finish(p, |id| data.remove(&u64::from(id))))
}

#[cfg(test)]
mod tests;
