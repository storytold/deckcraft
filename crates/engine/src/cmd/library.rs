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

use super::{CommandSpec, always, bad, bool_or, cmd, has_doc, opt_usize_param, str_param};
use crate::library::{self, Confined, Stamp};
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

/// The Library file `p` names (`file`): its real path and stamp, and its folder.
fn library_file(s: &Session, p: &Value, cmd: &str) -> Result<(PathBuf, Stamp, String)> {
    s.library.file(&Confined, &s.prefs.library_folders, p).map_err(|e| bad(cmd, e))
}

/// The required `slide` index.
fn slide_param(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    s.library.item_param(p).map_err(|e| bad(cmd, e))
}

fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn folders(s: &mut Session, _p: &Value) -> Result<Value> {
    native("library.folders")?;
    Ok(s.library.folders(&Confined, &s.prefs.library_folders))
}

fn add_folder(s: &mut Session, p: &Value) -> Result<Value> {
    native("library.addFolder")?;
    library::Cache::add_folder(&Confined, &mut s.prefs.library_folders, p).map_err(|e| bad("library.addFolder", e))
}

fn remove_folder(s: &mut Session, p: &Value) -> Result<Value> {
    native("library.removeFolder")?;
    library::Cache::remove_folder(&Confined, &mut s.prefs.library_folders, p).map_err(|e| bad("library.removeFolder", e))
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    native("library.list")?;
    s.library.list(&Confined, &s.prefs.library_folders, p).map_err(|e| bad("library.list", e))
}

fn search(s: &mut Session, p: &Value) -> Result<Value> {
    native("library.search")?;
    s.library.search(&Confined, &s.prefs.library_folders, p).map_err(|e| bad("library.search", e))
}

fn thumbnail(s: &mut Session, p: &Value) -> Result<Value> {
    native("library.thumbnail")?;
    s.library.thumbnail(&Confined, &s.prefs.library_folders, p).map_err(|e| bad("library.thumbnail", e))
}

fn shapes_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "library.shapes";
    native(CMD)?;
    let (path, stamp, _) = library_file(s, p, CMD)?;
    let index = slide_param(s, p, CMD)?;
    let src = s.library.doc(&Confined, &path, stamp).map_err(|e| bad(CMD, e))?;
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
    let slide = slide_param(s, p, CMD)?;
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
    let src = s.library.doc(&Confined, &path, stamp).map_err(|e| bad(CMD, e))?;
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
