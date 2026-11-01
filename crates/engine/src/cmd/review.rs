//! Review tab: comments, spelling, accessibility; Slide Show requests.

use std::sync::Arc;

use deckcraft_model::Comment;
use serde_json::{Value, json};

use super::{
    CommandSpec, always, bad, base64_decode, bool_or, bool_param, cmd, f64_or, has_doc, has_slide, has_text, id_param, ok, str_param, usize_param,
};
use crate::{Result, Session, Target, UiRequest};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("comment.add", "New Comment", ["Review", "Comments"], Some("Cmd+Alt+M"), "{text, x?, y?, id?: shape}", has_slide, add),
        cmd!("comment.reply", "Reply", ["Comments"], None, "{index, text}", has_slide, reply),
        cmd!("comment.delete", "Delete Comment", ["Review", "Comments"], None, "{index? | all?: bool}", has_slide, delete),
        cmd!("comment.resolve", "Resolve Thread", ["Comments"], None, "{index, resolved?: bool}", has_slide, resolve),
        cmd!(query "comment.list", "Comments", [], None, "{slide?} → comments", has_slide, list),
        cmd!(
            noundo "comment.next",
            "Next Comment",
            ["Review", "Comments"],
            None,
            "{slide?, index?} → {slide, index, text}",
            has_slide,
            |s, p| neighbour(s, p, true)
        ),
        cmd!(
            noundo "comment.prev",
            "Previous Comment",
            ["Review", "Comments"],
            None,
            "{slide?, index?} → {slide, index, text}",
            has_slide,
            |s, p| neighbour(s, p, false)
        ),
        cmd!(query "review.accessibility", "Check Accessibility", ["Review", "Accessibility"], None, "{} → [{slide, shape, issue}]", has_doc, accessibility),
        cmd!(query "review.spelling", "Spelling", ["Review", "Proofing"], Some("F7"), "{lang?, ignore?:[word]} → [{slide, shape?, row?, col?, notes?, para, start, end, word, lang, suggestions}] (shapes, table cells and notes; repeated words as {repeated, deleteFrom}; runs marked noProof skipped; each run in its own language, lang? for runs without one; Hunspell dictionaries installed on the system, else a built-in list of common misspellings)", has_doc, spelling),
        cmd!(
            "review.spellingChange",
            "Change Spelling",
            ["Review", "Proofing"],
            None,
            "{shape, word, to, all?, para?, start?, row?, col?} → {changed} (replace the whole word in that shape's text, the one at para/start, or in every shape with all)",
            has_doc,
            spelling_change
        ),
        cmd!(noundo "review.addToDictionary", "Add to Dictionary", ["Review", "Proofing"], None, "{word} → {word, file} (append to the user dictionary file; not an undo step)", always, add_to_dictionary),
        cmd!(query "review.dictionary", "Spelling Dictionary", ["Review", "Proofing"], None, "{lang?} → {lang, available, found, folders} (whether review.spelling has a dictionary, and which file)", always, dictionary_info),
        cmd!(query "review.thesaurus", "Thesaurus", ["Review", "Proofing"], None, "{word? (default: the selected word, or the word at the caret)} → {word, senses: [{word, synonyms}]}", has_doc, thesaurus),
        cmd!(
            "review.replaceWord",
            "Insert Synonym",
            [],
            None,
            "{with}: replace the selected word (or the word at the caret) while editing text",
            has_text,
            insert_synonym
        ),
        cmd!(noundo "review.compare", "Compare", ["Review", "Compare"], None, "{path} | {name, data: base64} — compare the open deck with another file → {name, changes: [{kind, slide, number, onlyIn?, revised, details}]}; each change is described from the open deck: `onlyIn: \"deck\"` is in the deck and not in the file, `onlyIn: \"file\"` is in the file and not in the deck, `changed` is in both and differs; `slide` is 0-based, `number` the editor's slide number; review.accept takes the FILE's side, so it deletes what is only in the deck (Accept All keeps the deck's last remaining slide rather than deleting it)", has_doc, compare),
        cmd!(query "review.revisions", "Revisions", [], None, "{} → {name, changes: [{kind: inserted|deleted|changed, slide (0-based), number (editor's), onlyIn?: deck|file, revised, details}]}; `onlyIn: deck` = only in the open deck, `file` = only in the compared file; accepting takes the file's side", has_doc, revisions),
        cmd!(
            "review.accept",
            "Accept Revision",
            ["Review", "Compare"],
            None,
            "{index} | {all: true} — accept the compared file's side of a change (or all): what is only in the file is added, what is only in the open deck is deleted (all: true leaves the deck's last remaining slide in place), a changed slide becomes the file's",
            comparing,
            accept
        ),
        cmd!(
            noundo "review.reject",
            "Reject Revision",
            ["Review", "Compare"],
            None,
            "{index} | {all: true} → {rejected} (drop revisions without applying them: the open deck stays as it is; not an undo step)",
            comparing,
            reject
        ),
        cmd!(noundo "review.endCompare", "End Review", ["Review", "Compare"], None, "{}", comparing, end_compare),
        cmd!(noundo "show.fromStart", "Play from Start", ["Slide Show", "Start Slide Show"], Some("Cmd+Shift+Enter"), "{}", has_slide, |s, _| start(s, 0)),
        cmd!(noundo "show.fromCurrent", "Play from Current Slide", ["Slide Show", "Start Slide Show"], Some("Cmd+Enter"), "{}", has_slide, |s, _| {
            let i = s.doc()?.selection.slide;
            start(s, i)
        }),
        cmd!(
            "show.setup",
            "Set Up Slide Show…",
            ["Slide Show", "Set Up"],
            None,
            "{type?: speaker|browsed|kiosk, loop?: bool, noNarration?, noAnimation?, useTimings?, showMediaControls?, from?, to?, custom?: name}",
            has_doc,
            setup
        ),
        cmd!(
            "show.customShow",
            "Custom Slide Show",
            ["Slide Show", "Start Slide Show"],
            None,
            "{name, slides: [index]} | {name, delete: true}",
            has_doc,
            custom_show
        ),
    ]
}

/// Seconds since the Unix epoch: the system clock, or the browser's on wasm (which has no `SystemTime`).
fn now_secs() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0).max(0.0) as u64
    }
}

/// A UTC timestamp as saved on comments: `2026-10-10T13:10:35Z`.
fn iso_utc(secs: u64) -> String {
    let (y, m, d) = super::design::civil((secs / 86_400) as i64);
    let t = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

fn now() -> String {
    iso_utc(now_secs())
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").ok_or_else(|| bad("comment.add", "missing `text`"))?.to_string();
    let author = s.prefs.author.clone();
    let shape = id_param(p, "id", "comment.add")?.or_else(|| s.active().and_then(|d| d.selection.shapes.first().copied()));
    let (x, y) = (f64_or(p, "x", 10.0), f64_or(p, "y", 10.0));
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("comment.add", "no slide"))?);
        let initials: String = author.split_whitespace().filter_map(|w| w.chars().next()).collect();
        sl.comments.push(Comment { author: author.clone(), initials, text, date: now(), x, y, resolved: false, replies: vec![], shape });
        Ok(json!({"index": sl.comments.len() - 1}))
    })
}

fn reply(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").ok_or_else(|| bad("comment.reply", "missing `index`"))?;
    let text = str_param(p, "text").unwrap_or("").to_string();
    let author = s.prefs.author.clone();
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("comment.reply", "no slide"))?);
        let c = sl.comments.get_mut(i).ok_or_else(|| bad("comment.reply", "no such comment"))?;
        c.replies.push(Comment { author, text, date: now(), ..Default::default() });
        Ok(())
    })?;
    ok()
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let all = bool_or(p, "all", false);
    let i = match p.get("index") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| bad("comment.delete", format!("`index` is not a valid comment index: {v}")))?,
        ),
    };
    s.edit(|doc, sel| {
        if all {
            for sl in &mut doc.slides {
                Arc::make_mut(sl).comments.clear();
            }
            return Ok(());
        }
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("comment.delete", "no slide"))?);
        match i {
            Some(i) if i < sl.comments.len() => {
                sl.comments.remove(i);
            }
            None => sl.comments.clear(),
            _ => return Err(bad("comment.delete", "no such comment")),
        }
        Ok(())
    })?;
    ok()
}

fn resolve(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").ok_or_else(|| bad("comment.resolve", "missing `index`"))?;
    let v = bool_or(p, "resolved", true);
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("comment.resolve", "no slide"))?);
        sl.comments.get_mut(i).ok_or_else(|| bad("comment.resolve", "no such comment"))?.resolved = v;
        Ok(())
    })?;
    ok()
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let i = usize_param(p, "slide").unwrap_or(st.selection.slide);
    Ok(serde_json::to_value(st.doc.slides.get(i).map(|x| x.comments.clone()).unwrap_or_default()).unwrap_or_default())
}

/// Flat `(slide, comment index)` list in deck order.
fn comment_threads(doc: &deckcraft_model::Presentation) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (si, sl) in doc.slides.iter().enumerate() {
        for ci in 0..sl.comments.len() {
            out.push((si, ci));
        }
    }
    out
}

/// Step to the next or previous comment. With `{slide, index}` matching a thread, move one step
/// and stop at the ends. Without `index`, next is the first thread on or after `slide` (the
/// current slide when omitted) and previous is the last thread strictly before it.
fn neighbour(s: &mut Session, p: &Value, forward: bool) -> Result<Value> {
    let cmd = if forward { "comment.next" } else { "comment.prev" };
    let (threads, slide, index) = {
        let st = s.doc()?;
        (comment_threads(&st.doc), usize_param(p, "slide").unwrap_or(st.selection.slide), usize_param(p, "index"))
    };
    if threads.is_empty() {
        return Err(bad(cmd, "no comments"));
    }
    let pos = index.and_then(|i| threads.iter().position(|&(sl, c)| sl == slide && c == i));
    let target = match (pos, forward) {
        (Some(i), true) => threads.get(i + 1).copied(),
        (Some(i), false) => i.checked_sub(1).and_then(|j| threads.get(j).copied()),
        (None, true) => threads.iter().copied().find(|(sl, _)| *sl >= slide),
        (None, false) => threads.iter().copied().rev().find(|(sl, _)| *sl < slide),
    };
    let Some((slide, index)) = target else {
        return Err(bad(cmd, "no more comments"));
    };
    s.select(|doc, sel| {
        sel.slide = slide;
        sel.slides = doc.slides.get(slide).map(|x| vec![x.id]).unwrap_or_default();
        sel.shapes.clear();
        sel.text = None;
        sel.cells = None;
        sel.target = Target::Slides;
    })?;
    let text = s.doc()?.doc.slides.get(slide).and_then(|sl| sl.comments.get(index)).map(|c| c.text.clone()).unwrap_or_default();
    Ok(json!({"slide": slide, "index": index, "text": text}))
}

fn accessibility(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let mut out = vec![];
    for (i, sl) in st.doc.slides.iter().enumerate() {
        if sl.title().trim().is_empty() {
            out.push(json!({"slide": i, "issue": "Missing slide title"}));
        }
        deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
            let needs_alt = matches!(
                sh.kind,
                deckcraft_model::ShapeKind::Picture { .. }
                    | deckcraft_model::ShapeKind::Chart(_)
                    | deckcraft_model::ShapeKind::Media(_)
                    | deckcraft_model::ShapeKind::Group { .. }
            );
            if needs_alt && sh.descr.trim().is_empty() && !sh.decorative {
                out.push(json!({"slide": i, "shape": sh.id, "name": sh.name, "issue": "Missing alternative text"}));
            }
        });
        // Low-contrast text against the slide background.
        if let Some(ctx) = deckcraft_model::resolve::Ctx::for_slide(&st.doc, sl) {
            let bg = match deckcraft_model::resolve::background(&ctx, Some(sl)) {
                (deckcraft_model::Fill::Solid { color }, ph) => Some(ctx.color(&color, ph)),
                _ => None,
            };
            if let Some(bg) = bg {
                deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
                    let Some(t) = &sh.text else { return };
                    let (fill, _) = deckcraft_model::resolve::fill(&ctx, sh);
                    if fill.as_ref().is_some_and(|f| !f.is_none()) {
                        return;
                    }
                    let low = t
                        .paragraphs
                        .iter()
                        .flat_map(|para| {
                            // A line break or blank run shows no glyphs, so its colour can't be hard to read.
                            para.runs.iter().filter(|run| run.kind != deckcraft_model::text::RunKind::Break && !run.text.trim().is_empty()).map(
                                |run| {
                                    let rp = deckcraft_model::resolve::run(&ctx, sh, para, &run.props);
                                    deckcraft_model::resolve::text_color(&ctx, &rp).contrast(bg)
                                },
                            )
                        })
                        .find(|contrast| *contrast < 3.0);
                    if let Some(contrast) = low {
                        out.push(
                            json!({"slide": i, "shape": sh.id, "name": sh.name, "issue": format!("Hard-to-read text contrast ({contrast:.1}:1)")}),
                        );
                    }
                });
            }
        }
    }
    Ok(Value::Array(out))
}

/// Frequent misspellings, used when no dictionary for the language is installed.
const COMMON: &[&str] = &[
    "teh",
    "recieve",
    "seperate",
    "occured",
    "untill",
    "wich",
    "becuase",
    "definately",
    "thier",
    "alot",
    "accomodate",
    "acheive",
    "adress",
    "begining",
    "beleive",
    "calender",
    "commited",
    "enviroment",
    "goverment",
    "independant",
    "neccessary",
    "occassion",
    "publically",
    "reccomend",
    "tommorow",
    "wierd",
];

/// The language a check uses: the `lang` parameter, else the first run that says, else US English.
fn check_lang(doc: &deckcraft_model::Presentation, p: &Value) -> String {
    if let Some(l) = str_param(p, "lang") {
        return l.to_string();
    }
    let mut found = None;
    for sl in &doc.slides {
        deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
            if found.is_none()
                && let Some(t) = &sh.text
            {
                found = t.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| r.props.lang.clone());
            }
        });
    }
    found.unwrap_or_else(|| "en-US".into())
}

fn spelling(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let default = check_lang(&st.doc, p);
    let ignore: Vec<String> =
        p.get("ignore").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_lowercase).collect()).unwrap_or_default();
    let mut dicts: std::collections::HashMap<String, Option<Arc<crate::spell::Dictionary>>> = Default::default();
    let mut out = vec![];
    // Each misspelt word of `para`, reported with `at` (where the paragraph is) merged in.
    let mut check = |para: &deckcraft_model::Paragraph, at: Value, out: &mut Vec<Value>| {
        for w in crate::spell::repeated(para, &default) {
            let mut v = json!({"start": w.start, "end": w.end, "word": w.text, "lang": w.lang, "repeated": true, "deleteFrom": w.repeated, "suggestions": []});
            if let (Some(o), Some(a)) = (v.as_object_mut(), at.as_object()) {
                o.extend(a.clone());
            }
            out.push(v);
        }
        let rep: Vec<usize> = crate::spell::repeated(para, &default).iter().map(|w| w.start).collect();
        for w in crate::spell::paragraph_words(para, &default) {
            if ignore.contains(&w.text.to_lowercase()) || rep.contains(&w.start) {
                continue;
            }
            let dict = dicts.entry(w.lang.clone()).or_insert_with(|| crate::spell::dictionary(&w.lang)).clone();
            let bad = match &dict {
                Some(d) => !crate::spell::is_correct(d, &w.text),
                None => COMMON.contains(&w.text.to_lowercase().as_str()),
            };
            if bad {
                let sug = dict.as_ref().map(|d| d.suggest(&w.text, 5)).unwrap_or_default();
                let mut v = json!({"start": w.start, "end": w.end, "word": w.text, "lang": w.lang, "suggestions": sug});
                if let (Some(o), Some(a)) = (v.as_object_mut(), at.as_object()) {
                    o.extend(a.clone());
                }
                out.push(v);
            }
        }
    };
    for (i, sl) in st.doc.slides.iter().enumerate() {
        deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
            if let Some(t) = &sh.text {
                for (pi, para) in t.paragraphs.iter().enumerate() {
                    check(para, json!({"slide": i, "shape": sh.id, "para": pi}), &mut out);
                }
            }
            if let deckcraft_model::ShapeKind::Table(tb) = &sh.kind {
                for (r, row) in tb.rows.iter().enumerate() {
                    for (c, cell) in row.cells.iter().enumerate().filter(|(_, c)| !c.is_covered()) {
                        for (pi, para) in cell.text.paragraphs.iter().enumerate() {
                            check(para, json!({"slide": i, "shape": sh.id, "row": r, "col": c, "para": pi}), &mut out);
                        }
                    }
                }
            }
        });
        // Speaker notes, a paragraph per line.
        for (pi, para) in sl.notes.paragraphs.iter().enumerate() {
            check(para, json!({"slide": i, "notes": true, "para": pi}), &mut out);
        }
    }
    Ok(Value::Array(out))
}

fn add_to_dictionary(_s: &mut Session, p: &Value) -> Result<Value> {
    let word = str_param(p, "word").filter(|w| !w.trim().is_empty()).ok_or_else(|| bad("review.addToDictionary", "missing `word`"))?;
    crate::spell::add_user_word(word).map_err(|e| bad("review.addToDictionary", e.to_string()))?;
    Ok(json!({"word": word.trim(), "file": crate::spell::user_dictionary_path().map(|p| p.display().to_string())}))
}

/// Replace whole-word `from` with `to` in `text`; how many were replaced.
fn replace_word(text: &mut String, from: &str, to: &str, all: bool) -> usize {
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’';
    let mut n = 0;
    let mut at = 0;
    while let Some(i) = text.get(at..).and_then(|t| t.find(from)).map(|i| i + at) {
        let end = i + from.len();
        let before = text.get(..i).and_then(|t| t.chars().next_back()).is_some_and(is_word);
        let after = text.get(end..).and_then(|t| t.chars().next()).is_some_and(is_word);
        if before || after {
            at = end;
            continue;
        }
        text.replace_range(i..end, to);
        n += 1;
        at = i + to.len();
        if !all {
            break;
        }
    }
    n
}

fn spelling_change(s: &mut Session, p: &Value) -> Result<Value> {
    let word = str_param(p, "word").filter(|w| !w.is_empty()).ok_or_else(|| bad("review.spellingChange", "missing `word`"))?.to_string();
    let to = str_param(p, "to").ok_or_else(|| bad("review.spellingChange", "missing `to`"))?.to_string();
    let all = bool_or(p, "all", false);
    let shape = p.get("shape").and_then(Value::as_u64);
    if shape.is_none() && !all {
        return Err(bad("review.spellingChange", "give `shape`, or `all: true`"));
    }
    // One exact occurrence (a squiggled word): paragraph `para`, from character `start`.
    if let (Some(id), Some(para), Some(start)) = (shape, usize_param(p, "para"), usize_param(p, "start")) {
        let id = deckcraft_model::ShapeId(u32::try_from(id).map_err(|_| bad("review.spellingChange", "bad `shape`"))?);
        let end = start.checked_add(word.chars().count()).ok_or_else(|| bad("review.spellingChange", "bad `start`"))?;
        let cell = usize_param(p, "row").zip(usize_param(p, "col"));
        let slide = {
            let st = s.doc()?;
            let found = st.doc.slides.iter().position(|sl| {
                deckcraft_model::find_shape(&sl.shapes, id)
                    .and_then(|sh| match (cell, &sh.kind) {
                        (Some((r, c)), deckcraft_model::ShapeKind::Table(tb)) => tb.cell(r, c)?.text.paragraphs.get(para),
                        (Some(_), _) => None,
                        (None, _) => sh.text.as_ref()?.paragraphs.get(para),
                    })
                    .is_some_and(|pp| pp.text().chars().skip(start).take(end - start).collect::<String>() == word)
            });
            found.ok_or_else(|| bad("review.spellingChange", "the word isn't there"))?
        };
        s.edit(|doc, _| {
            let sl = doc.slides.get_mut(slide).ok_or_else(|| bad("review.spellingChange", "no slide"))?;
            let sh = deckcraft_model::find_shape_mut(&mut Arc::make_mut(sl).shapes, id).ok_or_else(|| bad("review.spellingChange", "no shape"))?;
            let pp = match (cell, &mut sh.kind) {
                (Some((r, c)), deckcraft_model::ShapeKind::Table(tb)) => tb.cell_mut(r, c).and_then(|cl| cl.text.paragraphs.get_mut(para)),
                (Some(_), _) => None,
                (None, _) => sh.text.as_mut().and_then(|t| t.paragraphs.get_mut(para)),
            };
            if let Some(pp) = pp {
                replace_chars(pp, start, end, &to);
            }
            Ok(())
        })?;
        return Ok(json!({"changed": 1}));
    }
    // Nothing to change makes no undo step.
    let mut any = false;
    let st = s.doc()?;
    for sl in &st.doc.slides {
        deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
            if shape.is_none_or(|id| serde_json::to_value(sh.id).ok().and_then(|v| v.as_u64()) == Some(id))
                && let Some(t) = &sh.text
            {
                any |= t.paragraphs.iter().flat_map(|p| &p.runs).any(|r| replace_word(&mut r.text.clone(), &word, &to, false) > 0);
            }
        });
    }
    if !any {
        return Ok(json!({"changed": 0}));
    }
    let changed = s.edit(|doc, _| {
        let mut n = 0;
        let id_of = |sh: &deckcraft_model::Shape| serde_json::to_value(sh.id).ok().and_then(|v| v.as_u64());
        for sl in &mut doc.slides {
            let mut hit = false;
            deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
                hit |= shape.is_none_or(|id| id_of(sh) == Some(id)) && sh.text.as_ref().is_some_and(|t| t.text().contains(word.as_str()));
            });
            if !hit || (!all && n > 0) {
                continue;
            }
            walk_text_mut(&mut Arc::make_mut(sl).shapes, 0, &mut |sh| {
                if shape.is_some_and(|id| id_of(sh) != Some(id)) || (!all && n > 0) {
                    return;
                }
                let Some(t) = sh.text.as_mut() else { return };
                for r in t.paragraphs.iter_mut().flat_map(|p| p.runs.iter_mut()) {
                    if !all && n > 0 {
                        break;
                    }
                    n += replace_word(&mut r.text, &word, &to, all);
                }
            });
        }
        Ok(n)
    })?;
    Ok(json!({"changed": changed}))
}

/// Replace characters `start..end` of `p` with `to`, in the run where they start (later runs lose
/// the rest of the range).
fn replace_chars(p: &mut deckcraft_model::Paragraph, start: usize, end: usize, to: &str) {
    let mut at = 0;
    let mut placed = false;
    for r in &mut p.runs {
        let n = r.char_len();
        let (a, b) = (start.max(at), end.min(at + n));
        if a < b && r.kind == deckcraft_model::text::RunKind::Text {
            let byte = |c: usize| r.text.char_indices().nth(c).map_or(r.text.len(), |(i, _)| i);
            let (x, y) = (byte(a - at), byte(b - at));
            r.text.replace_range(x..y, if placed { "" } else { to });
            placed = true;
        }
        at += n;
    }
    p.runs.retain(|r| !(r.kind == deckcraft_model::text::RunKind::Text && r.text.is_empty()));
}

fn walk_text_mut(shapes: &mut [deckcraft_model::Shape], depth: usize, f: &mut dyn FnMut(&mut deckcraft_model::Shape)) {
    if depth > 64 {
        return;
    }
    for sh in shapes.iter_mut() {
        f(sh);
        if let deckcraft_model::ShapeKind::Group { children, .. } = &mut sh.kind {
            walk_text_mut(children, depth + 1, f);
        }
    }
}

fn dictionary_info(s: &mut Session, p: &Value) -> Result<Value> {
    let lang = match s.doc() {
        Ok(st) => check_lang(&st.doc, p),
        Err(_) => str_param(p, "lang").unwrap_or("en-US").to_string(),
    };
    let dirs = crate::spell::search_dirs();
    let found = crate::spell::find_in(&dirs, &lang).map(|(a, _)| a.with_extension("").display().to_string());
    let available = crate::spell::dictionary(&lang).is_some();
    Ok(json!({"lang": lang, "available": available, "found": found, "folders": dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>()}))
}

fn start(s: &mut Session, from: usize) -> Result<Value> {
    s.ui_requests.push(UiRequest::StartShow { from });
    Ok(json!({"from": from}))
}

fn setup(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, _| {
        let sh = &mut doc.show;
        if let Some(t) = str_param(p, "type") {
            sh.show_type = t.to_string();
            if t == "kiosk" {
                sh.loop_until_esc = true;
            }
        }
        if let Some(v) = bool_param(p, "loop") {
            sh.loop_until_esc = v;
        }
        if let Some(v) = bool_param(p, "noNarration") {
            sh.without_narration = v;
        }
        if let Some(v) = bool_param(p, "noAnimation") {
            sh.without_animation = v;
        }
        if let Some(v) = bool_param(p, "useTimings") {
            sh.use_timings = v;
        }
        if let Some(v) = bool_param(p, "showMediaControls") {
            sh.show_media_controls = v;
        }
        match (p.get("from").and_then(Value::as_u64), p.get("to").and_then(Value::as_u64)) {
            (Some(a), Some(b)) => sh.range = Some((a.min(u32::MAX as u64) as u32, b.min(u32::MAX as u64) as u32)),
            (None, None) if p.get("all").is_some() => sh.range = None,
            _ => {}
        }
        if p.get("all").and_then(Value::as_bool) == Some(true) {
            sh.range = None;
            sh.custom_show = None;
        }
        if p.get("custom").is_some() {
            let name = p.get("custom").and_then(Value::as_str).unwrap_or("");
            sh.custom_show = if name.is_empty() { None } else { Some(name.to_string()) };
            if sh.custom_show.is_some() {
                sh.range = None;
            }
        }
        Ok(serde_json::to_value(&*sh).unwrap_or_default())
    })
}

fn custom_show(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("show.customShow", "missing `name`"))?.to_string();
    let delete = bool_or(p, "delete", false);
    let idx: Vec<usize> =
        p.get("slides").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect()).unwrap_or_default();
    s.edit(|doc, _| {
        doc.custom_shows.retain(|c| c.name != name);
        if !delete {
            let slides = idx.iter().filter_map(|i| doc.slides.get(*i).map(|x| x.id)).collect();
            doc.custom_shows.push(deckcraft_model::CustomShow { name: name.clone(), slides });
        }
        Ok(())
    })?;
    ok()
}

/// The selected text when it lies in one paragraph, else the word at the caret.
fn word_under_caret(s: &Session) -> Option<String> {
    let st = s.doc().ok()?;
    let t = st.selection.text.clone()?;
    let body = super::text::body_of(st, &t)?;
    let (a, b) = if t.anchor != t.caret { t.ordered() } else { deckcraft_model::edit::word_at(body, t.caret) };
    if a.0 != b.0 {
        return None;
    }
    let text = body.paragraphs.get(a.0)?.text();
    let w: String = text.chars().skip(a.1).take(b.1.saturating_sub(a.1)).collect();
    let w = w.trim().to_string();
    (!w.is_empty()).then_some(w)
}

fn thesaurus(s: &mut Session, p: &Value) -> Result<Value> {
    let word = str_param(p, "word").map(str::to_string).or_else(|| word_under_caret(s)).unwrap_or_default();
    let senses = crate::thesaurus::lookup(&word);
    Ok(json!({"word": word, "senses": senses}))
}

fn insert_synonym(s: &mut Session, p: &Value) -> Result<Value> {
    let with = str_param(p, "with").filter(|w| !w.trim().is_empty()).ok_or_else(|| bad("review.replaceWord", "missing `with`"))?.to_string();
    if with.chars().count() > 200 {
        return Err(bad("review.replaceWord", "too long"));
    }
    let collapsed = s.doc()?.selection.text.as_ref().is_some_and(|t| t.anchor == t.caret);
    if collapsed {
        s.execute("text.selectWord", &json!({}))?;
    }
    s.execute("text.insert", &json!({"text": with}))
}

fn comparing(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|d| d.compare.is_some()) { Ok(()) } else { Err("not comparing".into()) }
}

fn revised_bytes(p: &Value) -> Result<(String, Vec<u8>)> {
    if let Some(data) = str_param(p, "data") {
        let bytes = base64_decode(data).ok_or_else(|| bad("review.compare", "invalid base64 `data`"))?;
        return Ok((str_param(p, "name").unwrap_or("Revised").to_string(), bytes));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = str_param(p, "path") {
        let bytes = crate::io::read_file(path).map_err(crate::EngineError::Other)?;
        return Ok((path.to_string(), bytes));
    }
    Err(bad("review.compare", "give `path` or `data`"))
}

fn revisions_json(st: &crate::DocState) -> Result<Value> {
    let c = st.compare.as_ref().ok_or_else(|| bad("review.revisions", "not comparing"))?;
    let name = std::path::Path::new(&c.name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| c.name.clone());
    Ok(json!({"name": name, "changes": crate::compare::pending(&st.doc, c), "byPosition": c.by_position}))
}

fn compare(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = revised_bytes(p)?;
    let revised = super::file::open_presentation(&name, &bytes)?;
    let st = s.doc_mut()?;
    let by_position = crate::compare::unrelated(&st.doc, &revised);
    st.compare = Some(crate::compare::Compare { name, revised: Arc::new(revised), by_position, rejected: Default::default() });
    s.ui_requests.push(UiRequest::Pane { id: "revisions".into() });
    revisions_json(s.doc()?)
}

fn revisions(s: &mut Session, _: &Value) -> Result<Value> {
    revisions_json(s.doc()?)
}

fn accept(s: &mut Session, p: &Value) -> Result<Value> {
    let cmp = s.doc()?.compare.clone().ok_or_else(|| bad("review.accept", "not comparing"))?;
    let rev = cmp.revised.clone();
    let all = bool_param(p, "all").unwrap_or(false);
    let index = usize_param(p, "index");
    if !all && index.is_none() {
        return Err(bad("review.accept", "give `index` or `all`"));
    }
    s.edit(|doc, sel| {
        let mut accepted = 0usize;
        // Each accept shifts indices, so the list is recomputed; bounded by the slide counts.
        let limit = doc.slides.len().saturating_add(rev.slides.len()).saturating_add(1);
        for _ in 0..limit {
            let changes = crate::compare::pending(doc, &cmp);
            let c = if all {
                changes.into_iter().find(|c| !(c.kind == crate::compare::ChangeKind::Deleted && doc.slides.len() <= 1))
            } else {
                changes.into_iter().nth(index.unwrap_or(0))
            };
            let Some(c) = c else { break };
            let Some(at) = crate::compare::accept(doc, &rev, &c)? else { break };
            accepted += 1;
            sel.slide = at;
            sel.shapes.clear();
            sel.text = None;
            if !all {
                break;
            }
        }
        if accepted == 0 {
            return Err(bad("review.accept", "no such revision"));
        }
        Ok(json!({"accepted": accepted}))
    })
}

fn reject(s: &mut Session, p: &Value) -> Result<Value> {
    let all = bool_param(p, "all").unwrap_or(false);
    let index = usize_param(p, "index");
    if !all && index.is_none() {
        return Err(bad("review.reject", "give `index` or `all`"));
    }
    let st = s.doc_mut()?;
    let doc = st.doc.clone();
    let c = st.compare.as_mut().ok_or_else(|| bad("review.reject", "not comparing"))?;
    let pending = crate::compare::pending(&doc, c);
    let picked: Vec<_> = if all { pending } else { pending.into_iter().nth(index.unwrap_or(0)).into_iter().collect() };
    if picked.is_empty() {
        return Err(bad("review.reject", "no such revision"));
    }
    c.rejected.extend(picked.iter().map(|ch| crate::compare::ChangeKey::of(&doc, ch)));
    Ok(json!({"rejected": picked.len()}))
}

fn end_compare(s: &mut Session, _: &Value) -> Result<Value> {
    s.doc_mut()?.compare = None;
    ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::Session;

    #[test]
    fn previous_and_next_comment_walk_the_deck() {
        let mut s = Session::with_new();
        let err = s.execute("comment.next", &json!({})).unwrap_err();
        assert!(err.to_string().contains("no comments"), "{err}");

        s.execute("comment.add", &json!({"text": "on first"})).unwrap();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [10.0, 10.0, 40.0, 40.0]})).unwrap();
        s.execute("slide.new", &json!({})).unwrap();
        s.execute("comment.add", &json!({"text": "on second a"})).unwrap();
        s.execute("comment.add", &json!({"text": "on second b"})).unwrap();
        s.execute("slide.new", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().selection.slide, 2);

        let err = s.execute("comment.next", &json!({})).unwrap_err();
        assert!(err.to_string().contains("no more comments"), "{err}");

        let undo_before = s.doc().unwrap().history.undo.len();
        let back = s.execute("comment.prev", &json!({})).unwrap();
        assert_eq!(back["slide"], 1);
        assert_eq!(back["index"], 1);
        assert_eq!(back["text"], "on second b");
        assert_eq!(s.doc().unwrap().selection.slide, 1);
        assert!(s.doc().unwrap().selection.shapes.is_empty());
        assert_eq!(s.doc().unwrap().history.undo.len(), undo_before);

        let prev = s.execute("comment.prev", &json!({"slide": 1, "index": 1})).unwrap();
        assert_eq!(prev["index"], 0);
        assert_eq!(prev["text"], "on second a");
        let first = s.execute("comment.prev", &json!({"slide": prev["slide"], "index": prev["index"]})).unwrap();
        assert_eq!(first["slide"], 0);
        assert_eq!(first["text"], "on first");
        let err = s.execute("comment.prev", &json!({"slide": 0, "index": 0})).unwrap_err();
        assert!(err.to_string().contains("no more comments"), "{err}");

        let next = s.execute("comment.next", &json!({"slide": 0, "index": 0})).unwrap();
        assert_eq!(next["slide"], 1);
        assert_eq!(next["text"], "on second a");
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        let here = s.execute("comment.next", &json!({})).unwrap();
        assert_eq!(here["text"], "on first");
    }

    #[test]
    fn timestamps_are_iso_utc() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_791_637_835), "2026-10-10T13:10:35Z");
    }

    #[test]
    fn the_clock_is_not_empty() {
        // On wasm this used to return an empty string, so browser comments were saved undated.
        assert!(now().starts_with("20"), "{}", now());
    }

    #[test]
    fn show_media_controls_follow_setup() {
        let mut s = Session::with_new();
        assert!(s.doc().expect("doc").doc.show.show_media_controls);
        let off = s.execute("show.setup", &json!({"showMediaControls": false})).expect("off");
        assert_eq!(off["showMediaControls"], false);
        assert_eq!(off["useTimings"], true, "other setup flags stay");
        let timings = s.execute("show.setup", &json!({"useTimings": false})).expect("timings");
        assert_eq!(timings["showMediaControls"], false);
        assert_eq!(timings["useTimings"], false);
        s.execute("show.setup", &json!({"showMediaControls": true})).expect("on");
        assert!(s.doc().expect("doc").doc.show.show_media_controls);
    }

    #[test]
    fn setup_remembers_a_custom_show() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        s.execute("slide.new", &json!({})).unwrap();
        s.execute("show.customShow", &json!({"name": "Short", "slides": [0, 1]})).unwrap();
        let v = s.execute("show.setup", &json!({"custom": "Short"})).unwrap();
        assert_eq!(v.get("customShow").and_then(Value::as_str), Some("Short"));
        assert!(s.active().unwrap().doc.show.range.is_none());
        s.execute("show.setup", &json!({"custom": ""})).unwrap();
        assert!(s.active().unwrap().doc.show.custom_show.is_none());
    }

    #[test]
    fn delete_rejects_an_invalid_index_and_keeps_the_comments() {
        let mut s = Session::with_new();
        s.execute("comment.add", &json!({"text": "a"})).unwrap();
        s.execute("comment.add", &json!({"text": "b"})).unwrap();
        for bad_index in [json!(-1), json!("2"), json!(1.5)] {
            let r = s.execute("comment.delete", &json!({"index": bad_index}));
            assert!(r.is_err(), "index {bad_index} gave {r:?}");
            assert_eq!(s.active().unwrap().doc.slides[0].comments.len(), 2, "index {bad_index}");
        }
        s.execute("comment.delete", &json!({})).unwrap();
        assert!(s.active().unwrap().doc.slides[0].comments.is_empty());
    }
}

#[cfg(test)]
mod spelling_change_range_tests {
    use crate::{EngineError, Session};
    use serde_json::json;

    #[test]
    fn spelling_change_huge_start_is_bad_params() {
        let mut s = Session::with_new();
        let r = s.execute("insert.textBox", &json!({"text": "Hello"})).unwrap();
        let id = r["id"].clone();
        let err = s.execute("review.spellingChange", &json!({"shape": id, "para": 0, "start": u64::MAX, "word": "Hello", "to": "x"})).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "{err:?}");
    }
}

#[cfg(test)]
mod accessibility_contrast_tests {
    use super::*;
    use deckcraft_model::text::{Paragraph, Run, RunKind, RunProps};
    use deckcraft_model::{ColorRef, Fill, Rgba, Shape, ShapeId, ShapeKind, TextBody};
    use serde_json::json;

    fn run(text: &str, color: Rgba) -> Run {
        Run::with(text, RunProps { fill: Some(Fill::solid(ColorRef::rgb(color))), ..Default::default() })
    }

    fn text_shape(id: u32, name: &str, paragraphs: Vec<Paragraph>) -> Shape {
        Shape { id: ShapeId(id), name: name.into(), text: Some(TextBody { paragraphs, ..Default::default() }), ..Default::default() }
    }

    #[test]
    fn contrast_checks_mixed_runs_later_paragraphs_and_group_descendants_once() {
        let mut s = Session::with_new();
        s.edit(|doc, _| {
            let black = Rgba::BLACK;
            let white = Rgba::WHITE;
            let mixed = Paragraph { runs: vec![run("visible", black), run("hidden", white)], ..Default::default() };
            let later = vec![
                Paragraph { runs: vec![run("visible", black)], ..Default::default() },
                Paragraph { runs: vec![run("hidden", white)], ..Default::default() },
            ];
            let child = text_shape(5, "nested text", vec![Paragraph { runs: vec![run("hidden", white)], ..Default::default() }]);
            let group = Shape {
                id: ShapeId(4),
                name: "group".into(),
                descr: "group of text".into(),
                kind: ShapeKind::Group {
                    children: vec![Shape {
                        id: ShapeId(7),
                        name: "nested group".into(),
                        descr: "nested group of text".into(),
                        kind: ShapeKind::Group { children: vec![child], child: Default::default() },
                        ..Default::default()
                    }],
                    child: Default::default(),
                },
                ..Default::default()
            };
            let sl = Arc::make_mut(doc.slides.get_mut(0).ok_or_else(|| bad("test", "no slide"))?);
            sl.shapes = vec![
                text_shape(2, "mixed", vec![mixed]),
                text_shape(3, "later paragraph", later),
                group,
                text_shape(6, "black control", vec![Paragraph { runs: vec![run("visible", black)], ..Default::default() }]),
                text_shape(
                    8,
                    "invisible white runs",
                    vec![Paragraph {
                        runs: vec![run("visible", black), Run { kind: RunKind::Break, ..run("\n", white) }, run("  ", white), run("visible", black)],
                        ..Default::default()
                    }],
                ),
            ];
            Ok(())
        })
        .unwrap();

        let issues = s.execute("review.accessibility", &json!({})).unwrap();
        let contrasts: Vec<_> = issues
            .as_array()
            .unwrap()
            .iter()
            .filter(|issue| issue["issue"].as_str().unwrap_or("").starts_with("Hard-to-read text contrast"))
            .collect();
        assert_eq!(contrasts.len(), 3, "{issues}");
        for id in [2, 3, 5] {
            assert!(contrasts.iter().any(|issue| issue["shape"] == id), "missing shape {id}: {issues}");
        }
        assert!(!contrasts.iter().any(|issue| issue["shape"] == 6), "black control warned: {issues}");
        assert!(!contrasts.iter().any(|issue| issue["shape"] == 8), "white break or spaces warned: {issues}");
    }
}
