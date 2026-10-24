//! File ▸ Save Selected Slides As… and File ▸ Split Presentation…: write part of the open
//! presentation to new files, leaving the open one as it is.
//!
//! A part keeps what its slides need: the masters and layouts they use, the pictures and media
//! they point at, their sections and custom shows. Masters, media and embedded fonts nothing in
//! the part refers to are left out, so a part of a big deck is not as big as the deck.

use std::collections::HashSet;

use deckcraft_model::{Presentation, SlideId};
use serde_json::{Value, json};

use super::file::{parse_slide_range, save_bytes, writable_format_for_path};
use super::*;
use crate::{EngineError, Result, Session, media_refs};

/// Most files one split writes, so a typo (`every: 1` on a huge deck) can't fill a folder.
const MAX_PARTS: usize = 500;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "file.saveSlidesAs",
            "Save Selected Slides As…",
            ["File"],
            None,
            "{path, slides?: [index] (0-based) or a range text (1-based, such as 1-3;5) (default the selected slides), format?} → {path, slides, bytes}",
            has_doc,
            save_slides_as
        ),
        cmd!(
            noundo "file.split",
            "Split Presentation…",
            ["File"],
            None,
            "{path (base name: parts are written as `<name>-1.<ext>`, `<name>-2.<ext>` …), every?: slides per file | bySection?: bool (one file per section), format?, overwrite?: bool (default false)} → {files: [{path, slides, bytes}]}",
            has_doc,
            split
        ),
    ]
}

/// A copy of `doc` holding only the slides at `indices` (in that order), with the masters, media,
/// sections and custom shows they need.
pub fn slice(doc: &Presentation, indices: &[usize]) -> Presentation {
    let mut out = doc.clone();
    out.slides = indices.iter().filter_map(|i| doc.slides.get(*i).cloned()).collect();
    let kept: HashSet<SlideId> = out.slides.iter().map(|s| s.id).collect();
    for sec in &mut out.sections {
        sec.slides.retain(|id| kept.contains(id));
    }
    out.sections.retain(|sec| !sec.slides.is_empty());
    for show in &mut out.custom_shows {
        show.slides.retain(|id| kept.contains(id));
    }
    out.custom_shows.retain(|show| !show.slides.is_empty());
    // Masters no slide uses go, unless they are marked to be preserved; the first stays so the
    // part is never left without one.
    let used: HashSet<_> = out.slides.iter().filter_map(|s| doc.layout(s.layout).map(|(m, _)| m.id)).collect();
    let first = out.masters.first().map(|m| m.id);
    out.masters = std::mem::take(&mut out.masters)
        .into_iter()
        .filter(|m| m.preserve || used.contains(&m.id) || (used.is_empty() && Some(m.id) == first))
        .collect();
    if out.masters.is_empty() {
        out.masters = doc.masters.iter().take(1).cloned().collect();
    }
    // Embedded fonts keep their bytes in `media`, so they count as references.
    let refs: HashSet<_> = media_refs::presentation_media(&out).into_iter().collect();
    out.media.retain(|m| refs.contains(&m.id));
    out
}

/// The slides a command works on: `slides` (indices or a range text), else the selection, else the
/// current slide.
fn slides_param(s: &Session, p: &Value, cmd: &str) -> Result<Vec<usize>> {
    let st = s.doc()?;
    let count = st.doc.slides.len();
    let list = match p.get("slides") {
        Some(Value::String(text)) => parse_slide_range(text, count)?,
        Some(Value::Array(a)) => {
            let mut v: Vec<usize> = vec![];
            for x in a {
                let i = x.as_u64().and_then(|x| usize::try_from(x).ok()).ok_or_else(|| bad(cmd, "`slides` holds slide indices"))?;
                if i >= count {
                    return Err(bad(cmd, format!("no slide {i}")));
                }
                if !v.contains(&i) {
                    v.push(i);
                }
            }
            v
        }
        Some(Value::Null) | None => {
            let mut v: Vec<usize> = st.selection.slides.iter().filter_map(|id| st.doc.slide_index(*id)).collect();
            if v.is_empty() && st.selection.slide < count {
                v.push(st.selection.slide);
            }
            v.sort_unstable();
            v
        }
        Some(_) => return Err(bad(cmd, "`slides` is a list of indices or a range such as \"1-3, 5\"")),
    };
    if list.is_empty() {
        return Err(bad(cmd, "there are no slides to save"));
    }
    Ok(list)
}

/// The format to write `path` in: `format` when given, else by the extension. An extension no
/// writer handles (`.odp`, none …) is an error rather than native bytes under a foreign name.
fn target_format(p: &Value, path: &str, cmd: &str) -> Result<String> {
    if let Some(f) = str_param(p, "format") {
        if !matches!(f, "deckcraft" | "pptx" | "potx" | "ppsx") {
            return Err(bad(cmd, format!("a part is saved as deckcraft, pptx, potx or ppsx, not {f}")));
        }
        return Ok(f.to_string());
    }
    match writable_format_for_path(path) {
        Some(f @ ("deckcraft" | "pptx" | "potx" | "ppsx")) => Ok(f.to_string()),
        _ => Err(bad(cmd, format!("“{path}” doesn't end in .pptx, .deckcraft, .potx or .ppsx"))),
    }
}

fn save_slides_as(s: &mut Session, p: &Value) -> Result<Value> {
    let Some(path) = str_param(p, "path").map(String::from) else {
        s.ui_requests.push(crate::UiRequest::PickFile { purpose: "saveSlidesAs".into(), params: p.clone() });
        return Ok(json!({"needsPath": true}));
    };
    refuse_the_open_file(s, &path, "file.saveSlidesAs")?;
    let indices = slides_param(s, p, "file.saveSlidesAs")?;
    let format = target_format(p, &path, "file.saveSlidesAs")?;
    let bytes = save_bytes(&slice(&s.doc()?.doc, &indices), &format)?;
    crate::io::write_file(&path, &bytes).map_err(EngineError::Other)?;
    Ok(json!({"path": path, "slides": indices.len(), "bytes": bytes.len()}))
}

/// The open presentation is unchanged in memory, so writing part of it over its own file would
/// lose the rest of it on disk.
fn refuse_the_open_file(s: &Session, path: &str, cmd: &str) -> Result<()> {
    match s.active().and_then(|d| d.path.as_deref()) {
        Some(open) if crate::io::same_file(open, path) => Err(bad(cmd, format!("{path} is the open presentation; choose another file name"))),
        _ => Ok(()),
    }
}

/// `(file stem, extension)` of the base path, e.g. `("/d/talk", "pptx")`.
fn split_path(path: &str) -> Option<(String, String)> {
    let dot = path.rfind('.')?;
    let sep = path.rfind(['/', '\\']).map_or(0, |i| i + 1);
    (dot > sep && dot + 1 < path.len()).then(|| (path[..dot].to_string(), path[dot + 1..].to_string()))
}

/// A section name made fit for a file name.
fn file_safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' }).collect();
    s.trim().chars().take(40).collect::<String>().trim().to_string()
}

fn split(s: &mut Session, p: &Value) -> Result<Value> {
    let Some(path) = str_param(p, "path").map(String::from) else {
        s.ui_requests.push(crate::UiRequest::PickFile { purpose: "splitPresentation".into(), params: p.clone() });
        return Ok(json!({"needsPath": true}));
    };
    let format = target_format(p, &path, "file.split")?;
    let (stem, ext) = split_path(&path).ok_or_else(|| bad("file.split", "`path` needs a file name with an extension"))?;
    let by_section = bool_or(p, "bySection", false);
    let every = usize_param(p, "every");
    if by_section == every.is_some() {
        return Err(bad("file.split", "give either `every` (slides per file) or `bySection`"));
    }
    if every == Some(0) {
        return Err(bad("file.split", "`every` must be at least 1"));
    }
    let doc = s.doc()?.doc.clone();
    // Each part: its slide indices and, for sections, its name.
    let mut parts: Vec<(Vec<usize>, Option<String>)> = vec![];
    if let Some(n) = every {
        let all: Vec<usize> = (0..doc.slides.len()).collect();
        parts = all.chunks(n).map(|c| (c.to_vec(), None)).collect();
    } else {
        // Slides outside every section (all of them, in a deck without sections) form a part of their own.
        let lead: Vec<usize> = doc.slides.iter().enumerate().filter(|(_, sl)| doc.section_of(sl.id).is_none()).map(|(i, _)| i).collect();
        if !lead.is_empty() {
            parts.push((lead, None));
        }
        for sec in &doc.sections {
            let idx: Vec<usize> = doc.slides.iter().enumerate().filter(|(_, sl)| sec.slides.contains(&sl.id)).map(|(i, _)| i).collect();
            if !idx.is_empty() {
                parts.push((idx, Some(sec.name.clone())));
            }
        }
        if parts.is_empty() {
            return Err(bad("file.split", "the presentation has no sections to split at"));
        }
    }
    if parts.len() > MAX_PARTS {
        return Err(bad("file.split", format!("that makes {} files; the most one split writes is {MAX_PARTS}", parts.len())));
    }
    if parts.len() < 2 {
        return Err(bad("file.split", "the presentation would stay in one piece; there is nothing to split"));
    }
    // Everything is encoded and every name checked before the first file is written, so a bad
    // name or encoding leaves nothing behind. A write that fails later says which parts are written.
    let width = parts.len().to_string().len();
    let overwrite = bool_or(p, "overwrite", false);
    let mut out: Vec<(String, usize, Vec<u8>)> = vec![];
    for (k, (idx, name)) in parts.iter().enumerate() {
        let label = name.as_deref().map(file_safe).filter(|n| !n.is_empty());
        let file = match label {
            Some(l) => format!("{stem}-{:0width$}-{l}.{ext}", k + 1),
            None => format!("{stem}-{:0width$}.{ext}", k + 1),
        };
        refuse_the_open_file(s, &file, "file.split")?;
        if !overwrite && crate::io::file_len(&file).is_some() {
            return Err(bad("file.split", format!("{file} exists already; pass `overwrite: true` to replace it")));
        }
        out.push((file, idx.len(), save_bytes(&slice(&doc, idx), &format)?));
    }
    let mut files: Vec<Value> = vec![];
    for (file, slides, bytes) in out {
        if let Err(e) = crate::io::write_file(&file, &bytes) {
            let done: Vec<&str> = files.iter().filter_map(|f| f["path"].as_str()).collect();
            return Err(EngineError::Other(if done.is_empty() { e } else { format!("{e} (already written: {})", done.join(", ")) }));
        }
        files.push(json!({"path": file, "slides": slides, "bytes": bytes.len()}));
    }
    Ok(json!({"files": files}))
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use serde_json::{Value, json};

    use crate::Session;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deckcraft-split-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A deck of `n` slides.
    fn deck(n: usize) -> Session {
        let mut s = Session::with_new();
        while s.doc().unwrap().doc.slides.len() < n {
            s.execute("slide.new", &json!({})).unwrap();
        }
        s
    }

    fn open_count(path: &std::path::Path) -> usize {
        let mut s = Session::new();
        s.execute("file.open", &json!({"path": path.to_str().unwrap()})).unwrap();
        s.doc().unwrap().doc.slides.len()
    }

    #[test]
    fn save_selected_slides_writes_only_those_and_leaves_the_deck_alone() {
        let dir = scratch("save");
        let mut s = deck(5);
        s.execute("slide.selectSlides", &json!({"indices": [1, 3]})).unwrap();
        let out = dir.join("part.pptx");
        let r = s.execute("file.saveSlidesAs", &json!({"path": out.to_str().unwrap()})).unwrap();
        assert_eq!(r["slides"], 2);
        assert_eq!(open_count(&out), 2);
        assert_eq!(s.doc().unwrap().doc.slides.len(), 5, "the open deck keeps all its slides");
        assert!(s.doc().unwrap().path.is_none(), "saving a part doesn't rename the open deck");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_slides_accepts_a_range_and_rejects_bad_input() {
        let dir = scratch("range");
        let mut s = deck(5);
        let out = dir.join("r.deckcraft");
        s.execute("file.saveSlidesAs", &json!({"path": out.to_str().unwrap(), "slides": "2-4"})).unwrap();
        assert_eq!(open_count(&out), 3);
        let p = dir.join("x.pptx");
        let p = p.to_str().unwrap();
        assert!(s.execute("file.saveSlidesAs", &json!({"path": p, "slides": [9]})).is_err());
        assert!(s.execute("file.saveSlidesAs", &json!({"path": p, "slides": "0-2"})).is_err());
        assert!(s.execute("file.saveSlidesAs", &json!({"path": dir.join("x.odp").to_str().unwrap()})).is_err());
        assert!(!std::path::Path::new(p).exists(), "a refused save writes nothing");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn split_every_n_slides() {
        let dir = scratch("every");
        let mut s = deck(5);
        let base = dir.join("talk.pptx");
        let r = s.execute("file.split", &json!({"path": base.to_str().unwrap(), "every": 2})).unwrap();
        let files = r["files"].as_array().unwrap();
        assert_eq!(files.len(), 3);
        let counts: Vec<usize> = ["talk-1.pptx", "talk-2.pptx", "talk-3.pptx"].iter().map(|n| open_count(&dir.join(n))).collect();
        assert_eq!(counts, vec![2, 2, 1]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn split_by_section_names_the_files_after_the_sections() {
        let dir = scratch("sections");
        let mut s = deck(5);
        s.execute("section.add", &json!({"at": 0, "name": "Intro"})).unwrap();
        s.execute("section.add", &json!({"at": 2, "name": "Main/Part"})).unwrap();
        let base = dir.join("talk.deckcraft");
        s.execute("file.split", &json!({"path": base.to_str().unwrap(), "bySection": true})).unwrap();
        assert_eq!(open_count(&dir.join("talk-1-Intro.deckcraft")), 2);
        assert_eq!(open_count(&dir.join("talk-2-Main_Part.deckcraft")), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn split_refuses_before_writing_anything() {
        let dir = scratch("refuse");
        let mut s = deck(4);
        let base = dir.join("talk.pptx");
        let base = base.to_str().unwrap();
        std::fs::write(dir.join("talk-2.pptx"), "mine").unwrap();
        assert!(s.execute("file.split", &json!({"path": base, "every": 2})).is_err(), "an existing part is not overwritten");
        assert!(!dir.join("talk-1.pptx").exists(), "nothing was written before the refusal");
        assert_eq!(std::fs::read_to_string(dir.join("talk-2.pptx")).unwrap(), "mine");
        assert!(s.execute("file.split", &json!({"path": base})).is_err(), "needs every or bySection");
        assert!(s.execute("file.split", &json!({"path": base, "every": 0})).is_err());
        assert!(s.execute("file.split", &json!({"path": base, "every": 9})).is_err(), "one piece is not a split");
        assert!(s.execute("file.split", &json!({"path": base, "bySection": true})).is_err(), "no sections");
        s.execute("file.split", &json!({"path": base, "every": 2, "overwrite": true})).unwrap();
        assert_eq!(open_count(&dir.join("talk-2.pptx")), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_part_drops_sections_and_media_its_slides_do_not_use() {
        let mut s = deck(3);
        s.doc_mut().unwrap().doc = std::sync::Arc::new({
            let mut d = (*s.doc().unwrap().doc).clone();
            d.media.push(deckcraft_model::MediaItem {
                id: deckcraft_model::MediaId(900),
                name: "orphan.png".into(),
                content_type: "image/png".into(),
                data: std::sync::Arc::new(vec![1, 2, 3]),
                link: None,
            });
            d
        });
        s.execute("section.add", &json!({"at": 0, "name": "A"})).unwrap();
        s.execute("section.add", &json!({"at": 2, "name": "B"})).unwrap();
        let st = s.doc().unwrap();
        let part = super::slice(&st.doc, &[0, 1]);
        assert_eq!(part.slides.len(), 2);
        assert_eq!(part.sections.len(), 1);
        assert_eq!(part.sections[0].name, "A");
        assert!(part.media.iter().all(|m| m.name != "orphan.png"), "media nothing refers to is left out");
        assert!(!part.masters.is_empty());
        assert!(part.validate().is_empty(), "{:?}", part.validate());
    }

    #[test]
    fn saving_slides_over_the_open_presentation_is_refused() {
        let dir = scratch("self");
        let mut s = deck(3);
        let own = dir.join("talk.pptx");
        s.execute("file.saveAs", &json!({"path": own.to_str().unwrap()})).unwrap();
        let before = std::fs::read(&own).unwrap();
        s.execute("slide.selectSlides", &json!({"indices": [0]})).unwrap();
        // Also when the same file is spelled differently.
        let other_spelling = dir.join(".").join("talk.pptx");
        for path in [own.to_str().unwrap(), other_spelling.to_str().unwrap()] {
            assert!(s.execute("file.saveSlidesAs", &json!({"path": path})).is_err(), "{path}");
        }
        assert_eq!(std::fs::read(&own).unwrap(), before, "the presentation's file is untouched");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_request_for_a_file_name_keeps_the_other_parameters() {
        let mut s = deck(4);
        s.execute("file.saveSlidesAs", &json!({"slides": [1, 2]})).unwrap();
        s.execute("file.split", &json!({"every": 3})).unwrap();
        let params: Vec<(String, Value)> = s
            .ui_requests
            .iter()
            .filter_map(|r| match r {
                crate::UiRequest::PickFile { purpose, params } => Some((purpose.clone(), params.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(params, vec![("saveSlidesAs".into(), json!({"slides": [1, 2]})), ("splitPresentation".into(), json!({"every": 3}))]);
    }
}
