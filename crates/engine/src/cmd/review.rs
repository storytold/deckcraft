//! Review tab: comments, spelling, accessibility; Slide Show requests.

use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::{Comment, InkStroke, LaserSample, LaserTrace, MediaClip, Rgba, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::*;
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
        cmd!(noundo "review.compare", "Compare", ["Review", "Compare"], None, "{path} | {name, data: base64} — compare with a revised copy → {name, changes}", has_doc, compare),
        cmd!(query "review.revisions", "Revisions", [], None, "{} → {name, changes: [{kind: inserted|deleted|changed, slide, revised, details}]}", has_doc, revisions),
        cmd!("review.accept", "Accept Revision", ["Review", "Compare"], None, "{index} | {all: true}", comparing, accept),
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
            "{type?: speaker|browsed|kiosk, loop?: bool, noNarration?, noAnimation?, useTimings?, showMediaControls?, from?, to?, custom?: name, laserColor?: red|green|blue|[r,g,b]}",
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
        cmd!(
            noundo "show.record",
            "Record Slide Show",
            ["Slide Show", "Record"],
            None,
            "{from?: index} → starts the show and keeps timings, narration, pen ink and laser",
            has_slide,
            record_show
        ),
        cmd!(
            "ink.stroke",
            "Record Ink",
            ["Slide Show", "Record"],
            None,
            "{points: [[x, y, pressure?]], index?: slide, width?, color?: [r, g, b], highlighter?}",
            has_slide,
            ink_stroke
        ),
        cmd!(
            "show.narration",
            "Record Narration",
            ["Slide Show", "Record"],
            None,
            "{data: base64 wav | path, name?, index?: slide} → replaces the slide's Narration audio",
            has_slide,
            narration
        ),
        cmd!(
            "show.laser",
            "Record Laser",
            ["Slide Show", "Record"],
            None,
            "{points: [[x, y, tMs]], index?: slide, color?: [r, g, b]} → replaces the slide's laser trace",
            has_slide,
            laser
        ),
        cmd!(
            "show.clearTiming",
            "Clear Timing on Current Slide",
            ["Slide Show", "Clear"],
            None,
            "{index?: slide} → clears that slide's advance-after time",
            has_slide,
            clear_timing
        ),
        cmd!(
            "show.clearTimingAll",
            "Clear Timing on All Slides",
            ["Slide Show", "Clear"],
            None,
            "{} → clears every slide's advance-after time",
            has_slide,
            clear_timing_all
        ),
        cmd!(
            "show.clearNarration",
            "Clear Narration on Current Slide",
            ["Slide Show", "Clear"],
            None,
            "{index?: slide} → removes that slide's narration audio",
            has_slide,
            clear_narration
        ),
        cmd!(
            "show.clearNarrationAll",
            "Clear Narration on All Slides",
            ["Slide Show", "Clear"],
            None,
            "{} → removes narration audio from every slide",
            has_slide,
            clear_narration_all
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
    let i = usize_param(p, "index");
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
                for sh in &sl.shapes {
                    let Some(t) = &sh.text else { continue };
                    let Some(para) = t.paragraphs.iter().find(|p| !p.is_empty()) else { continue };
                    let rp = deckcraft_model::resolve::run(&ctx, sh, para, &para.runs.first().map(|r| r.props.clone()).unwrap_or_default());
                    let (fill, _) = deckcraft_model::resolve::fill(&ctx, sh);
                    if fill.as_ref().is_some_and(|f| !f.is_none()) {
                        continue;
                    }
                    let fg = deckcraft_model::resolve::text_color(&ctx, &rp);
                    if fg.contrast(bg) < 3.0 {
                        out.push(json!({"slide": i, "shape": sh.id, "name": sh.name, "issue": format!("Hard-to-read text contrast ({:.1}:1)", fg.contrast(bg))}));
                    }
                }
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
        let end = start + word.chars().count();
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
    s.ui_requests.push(UiRequest::StartShow { from, record: false });
    Ok(json!({"from": from}))
}

fn record_show(s: &mut Session, p: &Value) -> Result<Value> {
    let n = s.doc()?.doc.slides.len();
    if n == 0 {
        return Err(bad("show.record", "no slides"));
    }
    let from = usize_param(p, "from").unwrap_or(0).min(n - 1);
    s.ui_requests.push(UiRequest::StartShow { from, record: true });
    Ok(json!({"from": from, "record": true}))
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
        apply_laser_color(sh, p)?;
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
    Ok(json!({"name": name, "changes": crate::compare::diff(&st.doc, &c.revised, c.by_position), "byPosition": c.by_position}))
}

fn compare(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = revised_bytes(p)?;
    let revised = super::file::open_presentation(&name, &bytes)?;
    let st = s.doc_mut()?;
    let by_position = crate::compare::unrelated(&st.doc, &revised);
    st.compare = Some(crate::compare::Compare { name, revised: Arc::new(revised), by_position });
    s.ui_requests.push(UiRequest::Pane { id: "revisions".into() });
    revisions_json(s.doc()?)
}

fn revisions(s: &mut Session, _: &Value) -> Result<Value> {
    revisions_json(s.doc()?)
}

fn accept(s: &mut Session, p: &Value) -> Result<Value> {
    let (rev, by_pos) = s.doc()?.compare.as_ref().map(|c| (c.revised.clone(), c.by_position)).ok_or_else(|| bad("review.accept", "not comparing"))?;
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
            let changes = crate::compare::diff(doc, &rev, by_pos);
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

fn end_compare(s: &mut Session, _: &Value) -> Result<Value> {
    s.doc_mut()?.compare = None;
    ok()
}

fn ink_stroke(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let index = usize_param(p, "index").unwrap_or(st.selection.slide);
    if index >= st.doc.slides.len() {
        return Err(bad("ink.stroke", format!("no slide {index}")));
    }
    let points = ink_points(p)?;
    if points.len() < 2 {
        return Err(bad("ink.stroke", "need at least two points"));
    }
    let width = p.get("width").and_then(Value::as_f64).filter(|w| w.is_finite()).unwrap_or(2.25).clamp(0.25, 40.0);
    let color = ink_color(p).unwrap_or(st.doc.show.pen_color);
    let highlighter = bool_or(p, "highlighter", false);
    let n = points.len();
    let stroke = InkStroke { points, color, width, highlighter };
    s.edit(|doc, _| {
        let Some(sl) = doc.slides.get_mut(index) else {
            return Err(bad("ink.stroke", format!("no slide {index}")));
        };
        let existing = Arc::make_mut(sl).shapes.iter().rev().find(|sh| matches!(sh.kind, ShapeKind::Ink { .. })).map(|sh| sh.id);
        let id = if let Some(id) = existing {
            id
        } else {
            let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("ink.stroke", NO_ID_SPACE))?);
            let size = doc.slide_size;
            let Some(sl) = doc.slides.get_mut(index) else {
                return Err(bad("ink.stroke", format!("no slide {index}")));
            };
            Arc::make_mut(sl).shapes.push(Shape {
                id,
                name: "Ink".into(),
                xfrm: Some(Xfrm::new(0.0, 0.0, size.width, size.height)),
                kind: ShapeKind::Ink { strokes: vec![] },
                ..Default::default()
            });
            id
        };
        let Some(sl) = doc.slides.get_mut(index) else {
            return Err(bad("ink.stroke", format!("no slide {index}")));
        };
        if let Some(sh) = deckcraft_model::find_shape_mut(&mut Arc::make_mut(sl).shapes, id)
            && let ShapeKind::Ink { strokes } = &mut sh.kind
        {
            strokes.push(stroke);
        }
        Ok(())
    })?;
    Ok(json!({"index": index, "points": n}))
}

/// Up to 4096 finite points. A missing pressure is 0.5.
fn ink_points(p: &Value) -> Result<Vec<(f64, f64, f32)>> {
    let raw = p.get("points").and_then(Value::as_array).ok_or_else(|| bad("ink.stroke", "missing `points`"))?;
    let mut out = Vec::new();
    for pt in raw.iter().take(4096) {
        let Some(a) = pt.as_array() else { continue };
        let (Some(x), Some(y)) = (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64)) else { continue };
        if !x.is_finite() || !y.is_finite() || x.abs() > 1.0e7 || y.abs() > 1.0e7 {
            continue;
        }
        let pressure = a.get(2).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.5).clamp(0.0, 1.0);
        out.push((x, y, pressure as f32));
    }
    Ok(out)
}

fn ink_color(p: &Value) -> Option<Rgba> {
    p.get("color").and_then(rgba_value)
}

fn rgba_value(v: &Value) -> Option<Rgba> {
    let a = v.as_array()?;
    let byte = |i: usize| -> Option<u8> {
        let v = a.get(i)?;
        let n = v.as_u64().or_else(|| v.as_f64().filter(|x| x.is_finite()).map(|x| x.round() as u64))?;
        Some(n.min(255) as u8)
    };
    Some(Rgba::rgb(byte(0)?, byte(1)?, byte(2)?))
}

/// PowerPoint's three laser-pointer colours, or an explicit `[r, g, b]`.
fn apply_laser_color(sh: &mut deckcraft_model::ShowSettings, p: &Value) -> Result<()> {
    let Some(v) = p.get("laserColor") else { return Ok(()) };
    let c = match v {
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "red" => Rgba::rgb(255, 0, 0),
            "green" => Rgba::rgb(0, 255, 0),
            "blue" => Rgba::rgb(0, 0, 255),
            _ => return Err(bad("show.setup", "laserColor is red, green, blue, or [r, g, b]")),
        },
        Value::Array(_) => rgba_value(v).ok_or_else(|| bad("show.setup", "laserColor needs three channels"))?,
        _ => return Err(bad("show.setup", "laserColor is red, green, blue, or [r, g, b]")),
    };
    sh.laser_color = c;
    Ok(())
}

/// Embed a WAV as the slide's narration. A later recording replaces the shape named Narration.
fn narration(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let index = usize_param(p, "index").unwrap_or(st.selection.slide);
    if index >= st.doc.slides.len() {
        return Err(bad("show.narration", format!("no slide {index}")));
    }
    let (name, bytes) = super::insert::media_bytes(p, "show.narration")?;
    if bytes.len() > 48_000_000 {
        return Err(bad("show.narration", "audio is too long"));
    }
    if bytes.len() < 44 {
        return Err(bad("show.narration", "audio is too short"));
    }
    let name = if name.is_empty() || name == "media" { "narration.wav".to_string() } else { name };
    let ct = super::insert::content_type(&name, &bytes);
    if !ct.starts_with("audio/") {
        return Err(bad("show.narration", format!("{name} is not an audio file")));
    }
    let owned = Arc::new(bytes);
    let info = deckcraft_media::probe(&owned).map_err(|e| bad("show.narration", e.to_string()))?;
    if info.audio.is_none() {
        return Err(bad("show.narration", format!("{name} has no audio")));
    }
    let duration_ms = info.duration_ms.min(u32::MAX as u64) as u32;
    if duration_ms < 200 {
        return Err(bad("show.narration", "shorter than 0.2 s"));
    }
    let data = Arc::try_unwrap(owned).unwrap_or_else(|a| (*a).clone());
    let bytes_len = data.len();
    let id = s.edit(|doc, _| {
        let media = doc.add_media(&name, ct, data).ok_or_else(|| bad("show.narration", NO_ID_SPACE))?;
        let size = doc.slide_size;
        let rect = Xfrm::new((size.width - 36.0).max(0.0), (size.height - 36.0).max(0.0), 32.0, 32.0);
        let clip = MediaClip {
            media,
            video: false,
            volume: 1.0,
            autoplay: true,
            narration: true,
            hide_while_not_playing: true,
            duration_ms,
            ..Default::default()
        };
        let existing = doc.slides.get(index).and_then(|sl| sl.shapes.iter().find(|sh| sh.name == "Narration").map(|sh| sh.id));
        let id = if let Some(id) = existing {
            id
        } else {
            let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("show.narration", NO_ID_SPACE))?);
            let Some(sl) = doc.slides.get_mut(index) else {
                return Err(bad("show.narration", format!("no slide {index}")));
            };
            Arc::make_mut(sl).shapes.push(Shape {
                id,
                name: "Narration".into(),
                xfrm: Some(rect),
                kind: ShapeKind::Media(MediaClip::default()),
                hidden: true,
                ..Default::default()
            });
            id
        };
        let Some(sl) = doc.slides.get_mut(index) else {
            return Err(bad("show.narration", format!("no slide {index}")));
        };
        if let Some(sh) = deckcraft_model::find_shape_mut(&mut Arc::make_mut(sl).shapes, id) {
            sh.name = "Narration".into();
            sh.hidden = true;
            sh.xfrm = Some(rect);
            sh.kind = ShapeKind::Media(clip);
        }
        Ok(id.0)
    })?;
    Ok(json!({"index": index, "id": id, "bytes": bytes_len, "durationMs": duration_ms}))
}

/// Replace the slide's recorded laser trace.
fn laser(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let index = usize_param(p, "index").unwrap_or(st.selection.slide);
    if index >= st.doc.slides.len() {
        return Err(bad("show.laser", format!("no slide {index}")));
    }
    let points = laser_points(p)?;
    if points.is_empty() {
        return Err(bad("show.laser", "need at least one point"));
    }
    let color = ink_color(p).unwrap_or(st.doc.show.laser_color);
    let n = points.len();
    let trace = LaserTrace { color, points };
    s.edit(|doc, _| {
        let Some(sl) = doc.slides.get_mut(index) else {
            return Err(bad("show.laser", format!("no slide {index}")));
        };
        Arc::make_mut(sl).laser = Some(trace);
        Ok(())
    })?;
    Ok(json!({"index": index, "points": n}))
}

fn laser_points(p: &Value) -> Result<Vec<LaserSample>> {
    let raw = p.get("points").and_then(Value::as_array).ok_or_else(|| bad("show.laser", "missing `points`"))?;
    let mut out = Vec::new();
    for pt in raw.iter().take(4096) {
        let Some(a) = pt.as_array() else { continue };
        let (Some(x), Some(y)) = (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64)) else { continue };
        if !x.is_finite() || !y.is_finite() || x.abs() > 1.0e7 || y.abs() > 1.0e7 {
            continue;
        }
        let t = a
            .get(2)
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).map(|x| x.round().clamp(0.0, 3_600_000.0) as u64)))
            .unwrap_or(0);
        out.push(LaserSample { x, y, t_ms: t.min(3_600_000) as u32 });
    }
    out.sort_by_key(|p| p.t_ms);
    Ok(out)
}

fn with_all(p: &Value) -> Value {
    let mut q = p.clone();
    if let Some(obj) = q.as_object_mut() {
        obj.insert("all".into(), Value::Bool(true));
        q
    } else {
        json!({"all": true})
    }
}

fn slide_scope(s: &Session, p: &Value, id: &str) -> Result<Vec<usize>> {
    let st = s.doc()?;
    if bool_or(p, "all", false) {
        return Ok((0..st.doc.slides.len()).collect());
    }
    let index = usize_param(p, "index").unwrap_or(st.selection.slide);
    if index >= st.doc.slides.len() {
        return Err(bad(id, format!("no slide {index}")));
    }
    Ok(vec![index])
}

fn clear_timing_all(s: &mut Session, p: &Value) -> Result<Value> {
    let q = with_all(p);
    clear_timing(s, &q)
}

fn clear_narration_all(s: &mut Session, p: &Value) -> Result<Value> {
    let q = with_all(p);
    clear_narration(s, &q)
}

/// Drop advance-after on the current slide, or on every slide. A transition effect stays.
fn clear_timing(s: &mut Session, p: &Value) -> Result<Value> {
    let list = slide_scope(s, p, "show.clearTiming")?;
    let any = {
        let st = s.doc()?;
        list.iter().any(|i| st.doc.slides.get(*i).and_then(|sl| sl.transition.as_ref()).is_some_and(|t| t.advance_after_ms.is_some()))
    };
    if !any {
        return Ok(json!({"cleared": 0}));
    }
    let n = s.edit(|doc, _| {
        let mut n = 0u32;
        for i in &list {
            let Some(sl) = doc.slides.get_mut(*i) else { continue };
            let sl = Arc::make_mut(sl);
            let had = sl.transition.as_ref().is_some_and(|t| t.advance_after_ms.is_some());
            if let Some(t) = sl.transition.as_mut() {
                t.advance_after_ms = None;
            }
            if had {
                n += 1;
            }
            let bare = sl
                .transition
                .as_ref()
                .is_some_and(|t| t.kind == "none" && t.option.is_empty() && t.advance_on_click && t.sound.is_none() && t.raw.is_none());
            if bare {
                sl.transition = None;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"cleared": n}))
}

/// Remove narration clips (the flag Record Slide Show sets). Other audio stays.
fn clear_narration(s: &mut Session, p: &Value) -> Result<Value> {
    let list = slide_scope(s, p, "show.clearNarration")?;
    let any = {
        let st = s.doc()?;
        list.iter().any(|i| st.doc.slides.get(*i).is_some_and(|sl| !narration_ids(&sl.shapes).is_empty()))
    };
    if !any {
        return Ok(json!({"cleared": 0}));
    }
    let n = s.edit(|doc, sel| {
        let mut n = 0u32;
        let mut media = Vec::new();
        for i in &list {
            let Some(sl) = doc.slides.get(*i) else { continue };
            let found = narration_ids(&sl.shapes);
            if found.is_empty() {
                continue;
            }
            let ids: Vec<ShapeId> = found.iter().map(|(id, _, _)| *id).collect();
            for (_, m, poster) in &found {
                media.push(*m);
                if let Some(p) = poster {
                    media.push(*p);
                }
            }
            n += ids.len() as u32;
            let Some(sl) = doc.slides.get_mut(*i) else { continue };
            let sl = Arc::make_mut(sl);
            super::edit::remove_shapes(&mut sl.shapes, &ids);
            sl.animations.retain(|a| !ids.contains(&a.shape));
            if sel.slide == *i {
                sel.shapes.retain(|id| !ids.contains(id));
            }
        }
        media.sort_by_key(|m| m.0);
        media.dedup();
        let drop: Vec<_> = media.into_iter().filter(|id| !media_used(doc, *id)).collect();
        doc.media.retain(|m| !drop.contains(&m.id));
        Ok(n)
    })?;
    Ok(json!({"cleared": n}))
}

fn narration_ids(shapes: &[Shape]) -> Vec<(ShapeId, deckcraft_model::MediaId, Option<deckcraft_model::MediaId>)> {
    let mut out = Vec::new();
    deckcraft_model::walk(shapes, &mut |sh, _| {
        if let ShapeKind::Media(c) = &sh.kind
            && c.narration
        {
            out.push((sh.id, c.media, c.poster));
        }
    });
    out
}

fn shapes_use_media(shapes: &[Shape], id: deckcraft_model::MediaId) -> bool {
    let mut used = false;
    deckcraft_model::walk(shapes, &mut |sh, _| {
        if shape_uses_media(sh, id) {
            used = true;
        }
    });
    used
}

fn media_used(doc: &deckcraft_model::Presentation, id: deckcraft_model::MediaId) -> bool {
    if doc.embedded_fonts.iter().any(|f| f.faces().contains(&Some(id))) {
        return true;
    }
    for sl in &doc.slides {
        if sl.transition.as_ref().is_some_and(|t| t.sound == Some(id))
            || sl.animations.iter().any(|a| a.sound == Some(id))
            || bg_uses(sl.background.as_ref(), id)
            || shapes_use_media(&sl.shapes, id)
        {
            return true;
        }
    }
    for m in &doc.masters {
        if bg_uses(m.background.as_ref(), id) || shapes_use_media(&m.shapes, id) {
            return true;
        }
        for l in &m.layouts {
            if bg_uses(l.background.as_ref(), id) || shapes_use_media(&l.shapes, id) {
                return true;
            }
        }
    }
    for m in [doc.notes_master.as_ref(), doc.handout_master.as_ref()].into_iter().flatten() {
        if bg_uses(m.background.as_ref(), id) || shapes_use_media(&m.shapes, id) {
            return true;
        }
    }
    false
}

fn shape_uses_media(sh: &Shape, id: deckcraft_model::MediaId) -> bool {
    if matches!(&sh.fill, Some(deckcraft_model::style::Fill::Picture(p)) if p.media == id) {
        return true;
    }
    if let Some(t) = &sh.text {
        for p in &t.paragraphs {
            if matches!(p.props.bullet, Some(deckcraft_model::text::Bullet::Picture { media }) if media == id) {
                return true;
            }
        }
    }
    match &sh.kind {
        ShapeKind::Media(c) => c.media == id || c.poster == Some(id),
        ShapeKind::Picture { fill } => fill.media == id,
        ShapeKind::Opaque { preview, .. } => *preview == Some(id),
        ShapeKind::Model3d { media, poster } => *media == id || *poster == Some(id),
        _ => false,
    }
}

fn bg_uses(bg: Option<&deckcraft_model::Background>, id: deckcraft_model::MediaId) -> bool {
    matches!(bg, Some(deckcraft_model::Background::Fill { fill: deckcraft_model::style::Fill::Picture(p) }) if p.media == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::Session;
    use crate::sample::open_sample;

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
    fn record_requests_a_recording_show() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        let v = s.execute("show.record", &json!({"from": 2})).unwrap();
        assert_eq!(v["from"], 2);
        assert_eq!(v["record"], true);
        assert!(matches!(s.ui_requests.last(), Some(UiRequest::StartShow { from: 2, record: true })));
    }

    #[test]
    fn recorded_ink_joins_one_layer_and_undoes() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        s.execute(
            "ink.stroke",
            &json!({"index": 0, "points": [[10.0, 20.0, 0.8], [30.0, 40.0, 0.2], [null, 1.0], [1.0e20, 0.0]], "width": 2.0, "color": [200, 30, 30]}),
        )
        .unwrap();
        s.execute("ink.stroke", &json!({"index": 0, "points": [[50.0, 60.0], [70.0, 80.0]]})).unwrap();
        let strokes = ink_on(&s, 0);
        assert_eq!(strokes.len(), 2);
        assert_eq!(strokes[0].points, vec![(10.0, 20.0, 0.8), (30.0, 40.0, 0.2)]);
        assert_eq!(strokes[0].color, Rgba::rgb(200, 30, 30));
        assert!((strokes[0].width - 2.0).abs() < 1e-9);
        assert_eq!(strokes[1].points[0].2, 0.5);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(ink_on(&s, 0).len(), 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(ink_on(&s, 0).is_empty());
    }

    #[test]
    fn recorded_ink_rejects_a_short_or_missing_stroke() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        assert!(s.execute("ink.stroke", &json!({"points": [[1.0, 2.0]]})).is_err());
        assert!(s.execute("ink.stroke", &json!({"points": [[1.0e20, 0.0], ["no", 2.0]]})).is_err());
        assert!(s.execute("ink.stroke", &json!({"index": 99, "points": [[0.0, 0.0], [1.0, 1.0]]})).is_err());
    }

    fn ink_on(s: &Session, slide: usize) -> Vec<InkStroke> {
        let st = s.doc().unwrap();
        st.doc.slides[slide]
            .shapes
            .iter()
            .find_map(|sh| match &sh.kind {
                ShapeKind::Ink { strokes } => Some(strokes.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn tone(n: usize) -> Vec<u8> {
        let samples: Vec<f32> = (0..n).map(|i| (i as f32 * 0.2).sin() * 0.3).collect();
        deckcraft_media::wav_mono(8000, &samples).unwrap()
    }

    fn narrations(s: &Session, slide: usize) -> Vec<MediaClip> {
        s.doc().unwrap().doc.slides[slide]
            .shapes
            .iter()
            .filter(|sh| sh.name == "Narration" && sh.hidden)
            .filter_map(|sh| match &sh.kind {
                ShapeKind::Media(m) => Some(m.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn narration_embeds_replaces_and_undoes() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        let wav = tone(4000);
        let v = s.execute("show.narration", &json!({"index": 0, "name": "narration.wav", "data": base64_encode(&wav)})).unwrap();
        assert_eq!(v["durationMs"], 500);
        let clips = narrations(&s, 0);
        assert_eq!(clips.len(), 1);
        assert!(clips[0].autoplay && clips[0].narration && !clips[0].video);
        assert!((clips[0].volume - 1.0).abs() < 1e-9);
        let longer = tone(8000);
        s.execute("show.narration", &json!({"index": 0, "name": "narration.wav", "data": base64_encode(&longer)})).unwrap();
        let clips = narrations(&s, 0);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].duration_ms, 1000);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(narrations(&s, 0)[0].duration_ms, 500);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(narrations(&s, 0).is_empty());
    }

    #[test]
    fn narration_rejects_short_junk_and_a_missing_slide() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        assert!(s.execute("show.narration", &json!({})).is_err());
        assert!(s.execute("show.narration", &json!({"data": base64_encode(b"hello"), "name": "x.wav"})).is_err());
        let short = tone(100);
        assert!(s.execute("show.narration", &json!({"index": 0, "name": "narration.wav", "data": base64_encode(&short)})).is_err());
        assert!(s.execute("show.narration", &json!({"index": 99, "name": "narration.wav", "data": base64_encode(&tone(4000))})).is_err());
        assert!(narrations(&s, 0).is_empty());
    }

    #[test]
    fn narration_and_laser_round_trip_through_pptx() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        let before = s.doc().unwrap().doc.slides[0].animations.len();
        let wav = tone(4000);
        s.execute("show.narration", &json!({"index": 0, "name": "narration.wav", "data": base64_encode(&wav)})).unwrap();
        s.execute("show.laser", &json!({"index": 0, "points": [[30.0, 40.0, 500], [10.0, 20.0, 0], [1.0e20, 0.0, 1]], "color": [0, 180, 40]}))
            .unwrap();
        let inserted = s.execute("insert.audio", &json!({"name": "tone.wav", "data": base64_encode(&wav)})).unwrap();
        let id = inserted["id"].as_u64().unwrap();
        s.execute("media.options", &json!({"autoplay": true, "ids": [id]})).unwrap();
        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let q = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(q.slides[0].animations.len(), before);
        let narr = q.slides[0].shapes.iter().find(|sh| sh.name == "Narration").unwrap();
        let m = match &narr.kind {
            ShapeKind::Media(m) => m,
            _ => {
                assert!(narr.name.is_empty(), "narration shape is media");
                return;
            }
        };
        assert!(m.autoplay && m.narration);
        let item = q.media(m.media).unwrap();
        assert_eq!(&item.data[..], &wav[..]);
        let laser = q.slides[0].laser.as_ref().unwrap();
        assert_eq!(laser.color, Rgba::rgb(0, 180, 40));
        assert_eq!(laser.points.len(), 2);
        assert_eq!(laser.points[0].t_ms, 0);
        assert!((laser.points[0].x - 10.0).abs() < 0.01);
        assert_eq!(laser.points[1].t_ms, 500);
        let auto = q.slides[0]
            .shapes
            .iter()
            .find_map(|sh| match &sh.kind {
                ShapeKind::Media(c) if !c.narration && c.autoplay => Some(c),
                _ => None,
            })
            .unwrap();
        assert!(!auto.narration && auto.autoplay);
    }

    #[test]
    fn laser_replaces_and_undoes() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        s.execute("show.laser", &json!({"index": 0, "points": [[1.0, 2.0, 10], [3.0, 4.0, 20]]})).unwrap();
        s.execute("show.laser", &json!({"index": 0, "points": [[8.0, 9.0, 30]]})).unwrap();
        let trace = s.doc().unwrap().doc.slides[0].laser.clone().unwrap();
        assert_eq!(trace.points.len(), 1);
        assert!((trace.points[0].x - 8.0).abs() < 1e-9);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.slides[0].laser.as_ref().unwrap().points.len(), 2);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.slides[0].laser.is_none());
        assert!(s.execute("show.laser", &json!({"points": []})).is_err());
        assert!(s.execute("show.laser", &json!({"index": 99, "points": [[0.0, 0.0, 0]]})).is_err());
    }

    #[test]
    fn laser_color_choices_round_trip_and_reject_unknown_names() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        assert_eq!(s.doc().unwrap().doc.show.laser_color, Rgba::rgb(255, 0, 0));
        s.execute("show.setup", &json!({"laserColor": "green"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.show.laser_color, Rgba::rgb(0, 255, 0));
        s.execute("show.setup", &json!({"laserColor": [0, 0, 255]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.show.laser_color, Rgba::rgb(0, 0, 255));
        assert!(s.execute("show.setup", &json!({"laserColor": "purple"})).is_err());
        assert_eq!(s.doc().unwrap().doc.show.laser_color, Rgba::rgb(0, 0, 255));
        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let q = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(q.show.laser_color, Rgba::rgb(0, 0, 255));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.show.laser_color, Rgba::rgb(0, 255, 0));
    }

    #[test]
    fn clear_timing_keeps_the_effect_and_can_cover_every_slide() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).unwrap();
        s.execute("transition.timing", &json!({"index": 0, "after": 800})).unwrap();
        s.execute("transition.set", &json!({"kind": "fade", "index": 1})).unwrap();
        s.execute("transition.timing", &json!({"index": 1, "after": 2500})).unwrap();
        let use_timings = s.doc().unwrap().doc.show.use_timings;
        let v = s.execute("show.clearTiming", &json!({"index": 0})).unwrap();
        assert_eq!(v["cleared"], 1);
        assert!(s.doc().unwrap().doc.slides[0].transition.is_none());
        assert_eq!(s.doc().unwrap().doc.slides[1].transition.as_ref().unwrap().advance_after_ms, Some(2500));
        assert_eq!(s.doc().unwrap().doc.show.use_timings, use_timings);
        s.execute("show.clearTiming", &json!({"index": 1})).unwrap();
        let t = s.doc().unwrap().doc.slides[1].transition.clone().unwrap();
        assert_eq!(t.kind, "fade");
        assert!(t.advance_after_ms.is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("transition.timing", &json!({"index": 0, "after": 400})).unwrap();
        s.execute("show.clearTimingAll", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.slides[0].transition.is_none());
        assert!(s.doc().unwrap().doc.slides[1].transition.as_ref().unwrap().advance_after_ms.is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.slides[1].transition.as_ref().unwrap().advance_after_ms, Some(2500));
        assert!(s.execute("show.clearTiming", &json!({"index": 99})).is_err());
        assert_eq!(s.execute("show.clearTiming", &json!({"index": 1})).unwrap()["cleared"], 1);
        assert_eq!(s.execute("show.clearTiming", &json!({"index": 1})).unwrap()["cleared"], 0);
    }

    #[test]
    fn clear_narration_leaves_other_audio_and_undoes() {
        let mut s = Session::new();
        open_sample(&mut s).unwrap();
        let wav = tone(4000);
        let other = tone(3200);
        s.execute("show.narration", &json!({"index": 0, "name": "narration.wav", "data": base64_encode(&wav)})).unwrap();
        s.execute("show.narration", &json!({"index": 1, "name": "narration.wav", "data": base64_encode(&wav)})).unwrap();
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        s.execute("insert.audio", &json!({"name": "tone.wav", "data": base64_encode(&other)})).unwrap();
        let shared = narrations(&s, 0)[0].media;
        assert_eq!(narrations(&s, 1)[0].media, shared);
        let v = s.execute("show.clearNarration", &json!({"index": 0})).unwrap();
        assert_eq!(v["cleared"], 1);
        assert!(narrations(&s, 0).is_empty());
        assert_eq!(narrations(&s, 1).len(), 1);
        assert!(s.doc().unwrap().doc.media(shared).is_some());
        assert!(s.doc().unwrap().doc.slides[0].shapes.iter().any(|sh| matches!(&sh.kind, ShapeKind::Media(m) if !m.narration)));
        s.execute("show.clearNarrationAll", &json!({})).unwrap();
        assert!(narrations(&s, 1).is_empty());
        assert!(s.doc().unwrap().doc.media(shared).is_none());
        assert!(s.doc().unwrap().doc.slides[0].shapes.iter().any(|sh| matches!(&sh.kind, ShapeKind::Media(_))));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(narrations(&s, 0).len(), 0);
        assert_eq!(narrations(&s, 1).len(), 1);
        assert!(s.doc().unwrap().doc.media(shared).is_some());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(narrations(&s, 0).len(), 1);
        assert!(s.execute("show.clearNarration", &json!({"index": 99})).is_err());
    }
}
