//! Insert ▸ Library (#958): slides and elements from the user's own folders of presentations.
//!
//! The folders are a preference ([`crate::Prefs::library_folders`]); `library.addFolder` and
//! `library.removeFolder` change it, and the host saves it with the other preferences. Listing,
//! searching and thumbnails only read (no undo step, no journal entry). `library.insert` brings a
//! slide in through the Reuse Slides code (`slide.reuse`), or that slide's shapes onto the current
//! slide; either way it is one undo step. Every folder and file goes through [`crate::io`], so the
//! automation read roots confine them, and every bound is in [`crate::library`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use deckcraft_model::{Presentation, Shape, ShapeId};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, base64_encode, bool_or, cmd, has_doc, opt_usize_param, str_param};
use crate::io::{self, EntryKind, Stamp};
use crate::library::{self, Match};
use crate::{Result, Selection, Session, Target, media_refs};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "library.folders",
            "Library Folders",
            [],
            None,
            "{} → [{path, files, entries, truncated?, skippedDeep?, skippedLinks?, unreadable?, error?}]: the folders the Library browses and how many presentations each holds (an unreadable folder carries its `error`)",
            always,
            folders
        ),
        cmd!(
            noundo "library.addFolder",
            "Add Library Folder…",
            ["Insert", "Library"],
            None,
            "{path} → {path, added, folders}: browse a folder of .pptx, .potx and .deckcraft files in the Library (kept with the preferences; at most 32 folders)",
            always,
            add_folder
        ),
        cmd!(
            noundo "library.removeFolder",
            "Remove Library Folder",
            ["Insert", "Library"],
            None,
            "{path} → {path, folders}: stop browsing a Library folder (its files are not touched)",
            always,
            remove_folder
        ),
        cmd!(
            query "library.list",
            "List Library",
            [],
            None,
            "{folder?, file?, slides?: bool (default true; false lists the files without opening them)} → [{file, folder, title, slides: [{index, title, hidden}], slideCount, truncated?, error?}]: the presentations in the Library folders (or one folder, or one file); a file that can't be read is listed with its `error`",
            always,
            list
        ),
        cmd!(
            query "library.search",
            "Search Library",
            [],
            None,
            "{query, limit?: 1..=500 (default 100), folder?} → [{file, slide, title, match: title|text|file}]: slides whose title, text or file name hold `query` (any case), title matches first",
            always,
            search
        ),
        cmd!(
            query "library.thumbnail",
            "Library Thumbnail",
            [],
            None,
            "{file, slide: index (0-based), width?: px 32..=512 (default 160)} → {png: base64, width, height}: a picture of a Library slide",
            always,
            thumbnail
        ),
        cmd!(
            query "library.shapes",
            "Library Slide Shapes",
            [],
            None,
            "{file, slide: index (0-based)} → {file, slide, shapes: [{name, kind, box: [x,y,w,h] pt, parent?, empty?}], truncated?}: the shapes on a Library slide, back to front, grouped ones after their group (`parent`), each with where it is drawn on the slide; the names are what library.insert's `names` picks",
            always,
            shapes_cmd
        ),
        cmd!(
            "library.insert",
            "Insert from Library",
            ["Insert", "Library"],
            None,
            "{file, slide: index (0-based), as?: slide|shapes (default slide; shapes when `names` is given), names?: [shape name] (shapes only: just the shapes with these names, any case; a grouped one comes out of its group where it is drawn), keepFormatting?: bool (default false: use this presentation's theme), at?: index (slide: where it goes, default after the current slide)} → {as, ids}: bring a Library slide in as a new slide, or put its shapes (or the named ones) on the current slide where they were (one undo step)",
            has_doc,
            insert
        ),
    ]
}

/// Every `library.*` command reads folders on this computer, which the browser build has not.
fn native(cmd: &str) -> Result<()> {
    if cfg!(target_arch = "wasm32") { Err(bad(cmd, library::NOT_IN_BROWSER)) } else { Ok(()) }
}

/// A non-empty `key` no longer than a path may be.
fn path_param<'a>(p: &'a Value, key: &str, cmd: &str) -> Result<&'a str> {
    let v = str_param(p, key).map(str::trim).filter(|v| !v.is_empty()).ok_or_else(|| bad(cmd, format!("missing `{key}`")))?;
    if v.len() > library::MAX_PATH {
        return Err(bad(cmd, format!("`{key}` is longer than {} bytes", library::MAX_PATH)));
    }
    Ok(v)
}

/// The Library folder `given` names: as kept, or the same folder written another way.
fn find_folder(s: &Session, given: &str) -> Option<String> {
    let kept = &s.prefs.library_folders;
    if let Some(f) = kept.iter().find(|f| f.as_str() == given) {
        return Some(f.clone());
    }
    let real = io::canonical(Path::new(given)).ok()?;
    kept.iter().find(|f| Path::new(f.as_str()) == real || io::canonical(Path::new(f.as_str())).is_ok_and(|r| r == real)).cloned()
}

/// The folders a command covers: `folder` when given (it must be a Library folder), else all.
fn chosen_folders(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    match str_param(p, "folder").filter(|f| !f.trim().is_empty()) {
        Some(f) => {
            let f = path_param(p, "folder", cmd).map(|_| f.trim())?;
            Ok(vec![find_folder(s, f).ok_or_else(|| bad(cmd, format!("{f} is not a Library folder (library.folders lists them)")))?])
        }
        None => Ok(s.prefs.library_folders.clone()),
    }
}

/// The presentations under `folders`, each once, with the folder it was found in. A folder that
/// can't be read is skipped when listing all of them (library.folders says why) and an error when
/// it was asked for by name.
fn files_in(folders: &[String], named: bool, cmd: &str) -> Result<Vec<(PathBuf, Stamp, String)>> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    for f in folders {
        let w = match library::walk(Path::new(f)) {
            Ok(w) => w,
            Err(e) if named => return Err(bad(cmd, e)),
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

/// The presentation `file` names: it must be a .pptx, .potx or .deckcraft file inside a Library
/// folder (following its links), readable under the automation read roots. Returns its real path
/// and stamp, and the folder it is in.
fn library_file(s: &Session, p: &Value, cmd: &str) -> Result<(PathBuf, Stamp, String)> {
    let file = path_param(p, "file", cmd)?;
    let path = Path::new(file);
    if !path.file_name().and_then(|n| n.to_str()).is_some_and(library::is_presentation) {
        return Err(bad(cmd, format!("{file} is not a .pptx, .potx or .deckcraft file")));
    }
    let real = io::canonical(path).map_err(|e| bad(cmd, e))?;
    let folder = s
        .prefs
        .library_folders
        .iter()
        .find(|f| io::canonical(Path::new(f.as_str())).is_ok_and(|root| real.starts_with(root)))
        .cloned()
        .ok_or_else(|| bad(cmd, format!("{file} is not in a Library folder; library.addFolder adds its folder")))?;
    let (kind, stamp) = io::stat(&real).map_err(|e| bad(cmd, e))?;
    if kind != EntryKind::File {
        return Err(bad(cmd, format!("{file} is not a file")));
    }
    Ok((real, stamp, folder))
}

/// The required `slide` index.
fn slide_param(p: &Value, cmd: &str) -> Result<usize> {
    opt_usize_param(p, "slide", cmd)?.ok_or_else(|| bad(cmd, "missing `slide` (an index from library.list, counting from 0)"))
}

fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn folders(s: &mut Session, _p: &Value) -> Result<Value> {
    native("library.folders")?;
    let out: Vec<Value> = s
        .prefs
        .library_folders
        .iter()
        .map(|f| match library::walk(Path::new(f)) {
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
    Ok(Value::Array(out))
}

fn add_folder(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "library.addFolder";
    native(CMD)?;
    let given = path_param(p, "path", CMD)?;
    let real = io::canonical(Path::new(given)).map_err(|e| bad(CMD, e))?;
    let (kind, _) = io::stat(&real).map_err(|e| bad(CMD, e))?;
    if kind != EntryKind::Folder {
        return Err(bad(CMD, format!("{given} is not a folder")));
    }
    let real = real.to_str().ok_or_else(|| bad(CMD, format!("{given}: the folder's path is not valid Unicode")))?.to_string();
    if real.len() > library::MAX_PATH {
        return Err(bad(CMD, format!("{given}: the folder's path is longer than {} bytes", library::MAX_PATH)));
    }
    let added = !s.prefs.library_folders.contains(&real);
    if added {
        if s.prefs.library_folders.len() >= library::MAX_FOLDERS {
            return Err(bad(CMD, format!("the Library holds at most {} folders; remove one first", library::MAX_FOLDERS)));
        }
        s.prefs.library_folders.push(real.clone());
    }
    Ok(json!({"path": real, "added": added, "folders": s.prefs.library_folders}))
}

fn remove_folder(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "library.removeFolder";
    native(CMD)?;
    let given = path_param(p, "path", CMD)?;
    let found = find_folder(s, given).ok_or_else(|| bad(CMD, format!("{given} is not a Library folder (library.folders lists them)")))?;
    s.prefs.library_folders.retain(|f| *f != found);
    Ok(json!({"path": found, "folders": s.prefs.library_folders}))
}

/// One file as `library.list` answers it.
fn file_json(s: &mut Session, path: &Path, stamp: Stamp, folder: &str, open: bool) -> Value {
    let mut v = json!({"file": shown(path), "folder": folder});
    if !open {
        v["title"] = json!(library::stem(path));
        if stamp.len > library::MAX_FILE_BYTES {
            v["error"] = json!(format!("larger than {} MB; not opened", library::MAX_FILE_BYTES >> 20));
        }
        return v;
    }
    let idx = s.library.index(path, stamp);
    v["title"] = json!(idx.title);
    v["slides"] = idx.slides.iter().enumerate().map(|(i, sl)| json!({"index": i, "title": sl.title, "hidden": sl.hidden})).collect();
    v["slideCount"] = json!(idx.slide_count);
    if idx.slide_count > idx.slides.len() {
        v["truncated"] = json!(true);
    }
    if let Some(e) = &idx.error {
        v["error"] = json!(e);
    }
    v
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "library.list";
    native(CMD)?;
    let open = bool_or(p, "slides", true);
    let files = if str_param(p, "file").is_some() {
        vec![library_file(s, p, CMD)?]
    } else {
        let named = str_param(p, "folder").is_some_and(|f| !f.trim().is_empty());
        files_in(&chosen_folders(s, p, CMD)?, named, CMD)?
    };
    Ok(Value::Array(files.iter().map(|(path, stamp, folder)| file_json(s, path, *stamp, folder, open)).collect()))
}

fn search(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "library.search";
    native(CMD)?;
    let query = str_param(p, "query").map(str::trim).filter(|q| !q.is_empty()).ok_or_else(|| bad(CMD, "missing `query`"))?;
    if query.chars().count() > library::MAX_QUERY {
        return Err(bad(CMD, format!("`query` is longer than {} characters", library::MAX_QUERY)));
    }
    let limit = match p.get("limit") {
        None | Some(Value::Null) => library::SEARCH_LIMIT,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| library::SEARCH_LIMITS.contains(n))
            .ok_or_else(|| bad(CMD, format!("`limit` is a count from 1 to 500, not {v}")))?,
    };
    let query = query.to_lowercase();
    let named = str_param(p, "folder").is_some_and(|f| !f.trim().is_empty());
    let files = files_in(&chosen_folders(s, p, CMD)?, named, CMD)?;
    // The first `limit` of each kind of match is enough to rank: title, then text, then file name.
    let mut kinds: [Vec<library::Hit>; 3] = Default::default();
    for (path, stamp, _) in &files {
        let idx = s.library.index(path, *stamp);
        for hit in library::matches(idx, path, &query) {
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
    let hits: Vec<Value> = kinds
        .iter()
        .flatten()
        .take(limit)
        .map(|h| json!({"file": shown(&h.file), "slide": h.slide, "title": h.title, "match": h.how.name()}))
        .collect();
    Ok(Value::Array(hits))
}

fn thumbnail(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "library.thumbnail";
    native(CMD)?;
    let (path, stamp, _) = library_file(s, p, CMD)?;
    let slide = slide_param(p, CMD)?;
    let width = match p.get("width") {
        None | Some(Value::Null) => library::THUMB_WIDTH,
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| library::THUMB_WIDTHS.contains(n))
            .ok_or_else(|| bad(CMD, format!("`width` is 32 to 512 pixels, not {v}")))?,
    };
    let t = s.library.thumbnail(&path, stamp, slide, width).map_err(|e| bad(CMD, e))?;
    Ok(json!({"png": base64_encode(&t.png), "width": t.width, "height": t.height}))
}

fn shapes_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "library.shapes";
    native(CMD)?;
    let (path, stamp, _) = library_file(s, p, CMD)?;
    let index = slide_param(p, CMD)?;
    let src = s.library.deck(&path, stamp).map_err(|e| bad(CMD, e))?;
    let n = src.slides.len().min(library::MAX_SLIDES);
    let slide = src.slides.get(index).filter(|_| index < n).ok_or_else(|| bad(CMD, format!("the file has {n} slides; {index} isn't one of them")))?;
    let at = Selection { slide: index, target: Target::Slides, ..Default::default() };
    struct Walk<'a> {
        src: &'a Presentation,
        at: &'a Selection,
        slide: &'a deckcraft_model::Slide,
        out: Vec<Value>,
        truncated: bool,
    }
    fn rec(w: &mut Walk<'_>, shapes: &[Shape], parent: Option<&str>, depth: usize) {
        if depth > 64 {
            return;
        }
        for sh in shapes {
            if w.out.len() >= MAX_LISTED {
                w.truncated = true;
                return;
            }
            let b = on_slide(w.src, w.at, w.slide, sh, parent.is_some()).xfrm.unwrap_or_default();
            let mut v = json!({"name": sh.name, "kind": sh.kind_name(), "box": [b.x, b.y, b.w, b.h]});
            if let Some(parent) = parent {
                v["parent"] = json!(parent);
            }
            if empty_placeholder(sh) {
                v["empty"] = json!(true);
            }
            w.out.push(v);
            rec(w, sh.children(), Some(&sh.name), depth + 1);
        }
    }
    let mut w = Walk { src: &src, at: &at, slide, out: vec![], truncated: false };
    rec(&mut w, &slide.shapes, None, 0);
    let mut v = json!({"file": shown(&path), "slide": index, "shapes": w.out});
    if w.truncated {
        v["truncated"] = json!(true);
    }
    Ok(v)
}

/// The most shape names one insert picks, and the longest name.
const MAX_NAMES: usize = 64;
const MAX_NAME: usize = 256;
/// The most shapes `library.shapes` lists.
const MAX_LISTED: usize = 1_000;

/// `names`: one shape name or a list of 1 to [`MAX_NAMES`] of them.
fn names_param(p: &Value, cmd: &str) -> Result<Option<Vec<String>>> {
    let one = |v: &Value| {
        v.as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty() && n.chars().count() <= MAX_NAME)
            .map(String::from)
            .ok_or_else(|| bad(cmd, format!("`names` holds shape names (1 to {MAX_NAME} characters), not {v}")))
    };
    match p.get("names") {
        None | Some(Value::Null) => Ok(None),
        Some(v @ Value::String(_)) => Ok(Some(vec![one(v)?])),
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= MAX_NAMES => a.iter().map(one).collect::<Result<Vec<_>>>().map(Some),
        Some(v) => Err(bad(cmd, format!("`names` is a list of 1 to {MAX_NAMES} shape names, not {v}"))),
    }
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "library.insert";
    native(CMD)?;
    let (path, stamp, _) = library_file(s, p, CMD)?;
    let slide = slide_param(p, CMD)?;
    let keep = bool_or(p, "keepFormatting", false);
    let names = names_param(p, CMD)?;
    let shapes = match str_param(p, "as") {
        None => names.is_some(),
        Some("slide") if names.is_some() => return Err(bad(CMD, "`names` picks shapes, so it goes with as: shapes")),
        Some("slide") => false,
        Some("shapes") => true,
        Some(other) => return Err(bad(CMD, format!("`as` is slide or shapes, not {other}"))),
    };
    let at = opt_usize_param(p, "at", CMD)?;
    let src = s.library.deck(&path, stamp).map_err(|e| bad(CMD, e))?;
    let n = src.slides.len().min(library::MAX_SLIDES);
    let name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    if slide >= n {
        return Err(bad(CMD, format!("{name} has {n} slides; {slide} isn't one of them (indices count from 0)")));
    }
    if shapes {
        let ids = insert_shapes(s, &src, slide, keep, names.as_deref())?;
        return Ok(json!({"as": "shapes", "ids": ids, "file": shown(&path), "slide": slide}));
    }
    let r = super::reuse::reuse_slides(s, CMD, &name, &src, &[slide], keep, at)?;
    Ok(json!({"as": "slide", "ids": r["ids"], "file": shown(&path), "slide": slide}))
}

/// Is `sh` a placeholder showing nothing of its own (an empty title or body with no fill or
/// outline of its own, which only shows its prompt while editing)? Those are left out when shapes
/// come from the Library.
fn empty_placeholder(sh: &Shape) -> bool {
    sh.ph.is_some()
        && matches!(sh.kind, deckcraft_model::ShapeKind::Shape)
        && sh.text.as_ref().is_none_or(|t| t.text().trim().is_empty())
        && sh.fill.is_none()
        && sh.line.is_none()
        && sh.style.is_none()
}

/// `sh`, a shape of `slide`, as it is drawn on the slide: its own box made explicit, and one inside
/// a group (`grouped`) given the box it is drawn in, as Ungroup places it.
fn on_slide(src: &Presentation, at: &Selection, slide: &deckcraft_model::Slide, sh: &Shape, grouped: bool) -> Shape {
    let mut sh = sh.clone();
    let x = sh.xfrm.unwrap_or_else(|| super::xfrm_of(src, at, &sh));
    let x = if grouped { super::edit::box_on_slide(src, at, &slide.shapes, sh.id, &x).unwrap_or(x) } else { x };
    sh.xfrm = Some(x);
    sh
}

/// The shapes of `slide` named in `names` (any case), back to front. A group that matches comes
/// whole; a match inside a group comes out of it. Every name must match at least one shape.
fn named(src: &Presentation, at: &Selection, slide: &deckcraft_model::Slide, names: &[String]) -> Result<Vec<Shape>> {
    const CMD: &str = "library.insert";
    fn rec<'a>(shapes: &'a [Shape], grouped: bool, wanted: &[String], seen: &mut [bool], found: &mut Vec<(&'a Shape, bool)>, depth: usize) {
        if depth > 64 {
            return;
        }
        for sh in shapes {
            let name = sh.name.trim().to_lowercase();
            let mut hit = false;
            for (k, w) in wanted.iter().enumerate() {
                if *w == name {
                    hit = true;
                    if let Some(seen) = seen.get_mut(k) {
                        *seen = true;
                    }
                }
            }
            if hit {
                found.push((sh, grouped));
            } else {
                rec(sh.children(), true, wanted, seen, found, depth + 1);
            }
        }
    }
    let wanted: Vec<String> = names.iter().map(|n| n.trim().to_lowercase()).collect();
    let mut seen = vec![false; wanted.len()];
    let mut found = vec![];
    rec(&slide.shapes, false, &wanted, &mut seen, &mut found, 0);
    let missing: Vec<&str> = names.iter().zip(&seen).filter(|(_, s)| !**s).map(|(n, _)| n.as_str()).collect();
    if !missing.is_empty() {
        let mut there = vec![];
        deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
            if there.len() < 20 && !sh.name.trim().is_empty() {
                there.push(format!("\"{}\"", sh.name.trim()));
            }
        });
        return Err(bad(
            CMD,
            format!("the slide has no shape named {}; its shapes include {} (library.shapes lists them)", missing.join(", "), there.join(", ")),
        ));
    }
    Ok(found.into_iter().map(|(sh, grouped)| on_slide(src, at, slide, sh, grouped)).collect())
}

/// Unglue connector ends whose shape didn't come along: after renumbering, an end still holds the
/// source deck's id, which may name some other shape here.
fn unglue_missing(shapes: &mut [Shape]) {
    let mut kept = HashSet::new();
    deckcraft_model::walk(shapes, &mut |sh, _| {
        kept.insert(sh.id);
    });
    fn rec(shapes: &mut [Shape], kept: &HashSet<ShapeId>, depth: usize) {
        if depth > 64 {
            return;
        }
        for sh in shapes {
            if let deckcraft_model::ShapeKind::Connector { start, end } = &mut sh.kind {
                for c in [start, end] {
                    if c.as_ref().is_some_and(|(id, _)| !kept.contains(id)) {
                        *c = None;
                    }
                }
            }
            if let Some(ch) = sh.children_mut() {
                rec(ch, kept, depth + 1);
            }
        }
    }
    rec(shapes, &kept, 0);
}

/// Put the shapes of slide `index` of `src` (or those named in `names`) on the current slide,
/// each where it was on the Library slide: placeholders become regular shapes with the look they
/// had there, pictures and table styles come along, ids are fresh, and a connector whose shape
/// stays behind comes unglued. Returns the new top-level ids.
fn insert_shapes(s: &mut Session, src: &Presentation, index: usize, keep: bool, names: Option<&[String]>) -> Result<Vec<ShapeId>> {
    const CMD: &str = "library.insert";
    let slide = src.slides.get(index).ok_or_else(|| bad(CMD, format!("slide {index} isn't in the file")))?;
    let at = Selection { slide: index, target: Target::Slides, ..Default::default() };
    let mut shapes: Vec<Shape> = match names {
        Some(names) => named(src, &at, slide, names)?,
        None => slide.shapes.iter().filter(|sh| !empty_placeholder(sh)).map(|sh| on_slide(src, &at, slide, sh, false)).collect(),
    };
    if shapes.is_empty() {
        return Err(bad(CMD, format!("slide {index} has no shapes to insert")));
    }
    super::edit::detach_on_slide(src, &at, &mut shapes);
    for sh in &mut shapes {
        sh.ph = None;
    }
    if keep && let Some(ctx) = super::text::ctx_for(src, &at) {
        let theme =
            crate::SourceTheme { colors: ctx.master.theme.colors.clone(), color_map: ctx.color_map.clone(), fonts: ctx.master.theme.fonts.clone() };
        super::edit::bake_theme(&mut shapes, &theme);
    }
    let media = media_refs::shapes_media(&shapes);
    let mut table_styles = vec![];
    super::reuse::collect_table_styles(&shapes, &mut table_styles, &mut HashSet::new(), 0);
    s.edit(|doc, sel| {
        if sel.target == Target::Slides && doc.slides.get(sel.slide).is_none() {
            return Err(bad(CMD, "the presentation has no slide to put the shapes on; add a slide first"));
        }
        let mut media_map = std::collections::HashMap::new();
        super::reuse::copy_media(CMD, doc, src, &media, &mut media_map)?;
        media_refs::visit_mut::shapes(&mut shapes, &mut super::reuse::remap(&media_map));
        let styles = super::reuse::import_table_styles(CMD, doc, src, &table_styles)?;
        super::reuse::remap_table_styles(&mut shapes, &styles, 0);
        super::reuse::renumber(CMD, doc, &mut shapes)?;
        unglue_missing(&mut shapes);
        // A name already on the slide gets the next free number, as Paste does.
        let (mut taken, mut count) = super::current_names(doc, sel);
        for sh in &mut shapes {
            if taken.contains(&sh.name)
                && let Some((base, n)) = sh.name.rsplit_once(' ')
                && !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
            {
                sh.name = super::insert::first_free_name(base, &taken, count);
            }
            deckcraft_model::walk(std::slice::from_ref(sh), &mut |m, _| {
                taken.insert(m.name.clone());
                count += 1;
            });
        }
        let ids: Vec<ShapeId> = shapes.iter().map(|x| x.id).collect();
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide to put the shapes on"))?;
        list.extend(shapes);
        sel.shapes = ids.clone();
        sel.text = None;
        sel.cells = None;
        Ok(ids)
    })
}

#[cfg(test)]
mod tests;
