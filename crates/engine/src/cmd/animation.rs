//! Animations tab and Animation Pane.

use std::sync::Arc;

use deckcraft_model::ShapeId;
use deckcraft_model::anim::{ANIMATIONS, AnimClass, AnimStart, Animation, TextBuild, animation_info};
use serde_json::{Value, json};

use super::{
    CommandSpec, always, bad, bool_or, bool_param, cmd, has_selection, has_slide, id_param, ok, opt_usize_param, str_param, targets, usize_param,
};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "animation.set",
            "Animation Styles",
            ["Animations", "Animation"],
            None,
            "{index? (replace that effect alone, keeping its target and start), effect: fade|fly|zoom|wipe|appear|spin|pulse|fadeOut|lines|… (animation.list), class?: entrance|emphasis|exit|path, option?, path?: [[x,y],…] or OOXML string, closed?: bool, ids?, paragraphs?: [n, …] or paragraph?: n (0-based; else the highlighted text's paragraphs)} — replaces the shapes' (or those paragraphs') animations. path (motion paths): points in slide points, as offsets from where the shape sits, or the OOXML path string (slide fractions) animation.get answers",
            has_selection,
            set
        ),
        cmd!(
            "animation.add",
            "Add Animation",
            ["Animations", "Advanced Animation"],
            None,
            "{effect, class?, option?, path?: [[x,y],…] or OOXML string, closed?: bool, start?: onClick|withPrevious|afterPrevious, duration?: ms, delay?: ms, ids?, paragraphs?: [n, …] or paragraph?: n (0-based; else the highlighted text's paragraphs)} → {count, path?}",
            has_selection,
            add
        ),
        cmd!("animation.remove", "Remove Animation", ["Animation Pane"], None, "{index} | {ids? (all of the shapes' animations)}", has_slide, remove),
        cmd!("animation.move", "Reorder Animation", ["Animations", "Timing"], None, "{index, to}", has_slide, move_anim),
        cmd!(
            "animation.timing",
            "Timing",
            ["Animations", "Timing"],
            None,
            "{index? (else highlighted paragraphs, else selected shapes), paragraphs?: [n, …] or paragraph?: n (0-based), start?: onClick|withPrevious|afterPrevious, duration?: ms, delay?: ms, repeat?: n, rewind?: bool, trigger?: shape id | null}",
            has_slide,
            timing
        ),
        cmd!(
            "animation.options",
            "Effect Options",
            ["Animations", "Animation"],
            None,
            "{option?, textBuild?: asOne|byParagraph|byWord|byLetter (always the whole box), path?: [[x,y],…] or OOXML string, closed?: bool, index? (else highlighted paragraphs, else selected shapes), paragraphs?: [n, …] or paragraph?: n (0-based)}",
            has_slide,
            options
        ),
        cmd!("animation.clear", "Remove All Animations", [], None, "{index?: slide}", has_slide, clear),
        cmd!(
            noundo "animation.painter",
            "Animation Painter",
            ["Animations", "Advanced Animation"],
            Some("Alt+Shift+C"),
            "{sticky?: bool, id?}",
            has_selection,
            painter_pick
        ),
        cmd!("animation.painterApply", "Apply Animation", [], Some("Alt+Shift+V"), "{ids?}", has_selection, painter_apply),
        cmd!(query "animation.list", "Animation Gallery", [], None, "{} → [{id, label, class, options}]", always, list),
        cmd!(query "animation.get", "Animations on Slide", [], None, "{slide?} → [animation]", has_slide, get),
    ]
}

fn class_of(s: Option<&str>) -> Option<AnimClass> {
    s.map(|c| match c {
        "emphasis" | "emph" => AnimClass::Emphasis,
        "exit" => AnimClass::Exit,
        "path" | "motion" | "motionPath" => AnimClass::Path,
        "media" => AnimClass::Media,
        _ => AnimClass::Entrance,
    })
}

fn start_of(s: &str) -> AnimStart {
    match s {
        "withPrevious" | "with" => AnimStart::WithPrevious,
        "afterPrevious" | "after" => AnimStart::AfterPrevious,
        _ => AnimStart::OnClick,
    }
}

/// The effect option `given` as one of `allowed`. Besides the stored codes ("b", "lt", "u", …)
/// it takes the direction words people type: "fromBottom", "bottom", "fromTopLeft",
/// "upRight". `None` when it matches none of them: the renderer reads direction codes letter
/// by letter, so an unchecked "fromBottom" would fly in from the right and the bottom.
fn resolve_option(allowed: &[&str], given: &str) -> Option<String> {
    if allowed.is_empty() || allowed.contains(&given) {
        return Some(given.to_string());
    }
    let low = given.to_ascii_lowercase();
    if let Some(o) = allowed.iter().find(|o| o.to_ascii_lowercase() == low) {
        return Some(o.to_string());
    }
    let mut rest = low.strip_prefix("from").unwrap_or(&low).trim_start_matches(['-', '_', ' ']);
    // Direction words in order, each as both its edge letter and its arrow letter.
    let mut dirs: Vec<(char, char)> = Vec::new();
    while !rest.is_empty() {
        let words: [(&str, (char, char)); 8] = [
            ("bottom", ('b', 'd')),
            ("down", ('b', 'd')),
            ("top", ('t', 'u')),
            ("up", ('t', 'u')),
            ("left", ('l', 'l')),
            ("right", ('r', 'r')),
            ("-", ('-', '-')),
            ("_", ('-', '-')),
        ];
        let (w, d) = words.iter().find(|(w, _)| rest.starts_with(w))?;
        if d.0 != '-' {
            dirs.push(*d);
        }
        rest = &rest[w.len()..];
    }
    if dirs.is_empty() || dirs.len() > 2 {
        return None;
    }
    let mut cands: Vec<String> = Vec::new();
    for pick in [|d: &(char, char)| d.0, |d: &(char, char)| d.1] {
        let code: String = dirs.iter().map(pick).collect();
        let rev: String = code.chars().rev().collect();
        cands.push(code);
        cands.push(rev);
    }
    cands.into_iter().find(|c| allowed.contains(&c.as_str()))
}

fn build(p: &Value, slide: (f64, f64)) -> Result<Animation> {
    let effect = str_param(p, "effect").ok_or_else(|| bad("animation", "missing `effect`"))?;
    let class = class_of(str_param(p, "class"));
    let info = match class {
        Some(c) => animation_info(effect, c),
        None => ANIMATIONS.iter().find(|a| a.0 == effect),
    }
    .ok_or_else(|| bad("animation", format!("unknown effect `{effect}`")))?;
    let mut a = Animation {
        class: info.2,
        effect: info.0.to_string(),
        option: match str_param(p, "option") {
            Some(o) => {
                resolve_option(info.5, o).ok_or_else(|| bad("animation", format!("option `{o}` is not one of {:?} for `{}`", info.5, info.0)))?
            }
            None => info.5.first().map(|o| o.to_string()).unwrap_or_default(),
        },
        duration_ms: p.get("duration").and_then(Value::as_u64).map(|v| v.min(600_000) as u32).unwrap_or(info.4),
        delay_ms: p.get("delay").and_then(Value::as_u64).map(|v| v.min(600_000) as u32).unwrap_or(0),
        preset_id: Some(info.3),
        ..Default::default()
    };
    if let Some(st) = str_param(p, "start") {
        a.start = start_of(st);
    }
    if a.class == AnimClass::Path {
        a.path = Some(path_param(p, slide)?.unwrap_or_else(|| default_path(&a.effect, &a.option)));
    } else if p.get("path").is_some() {
        return Err(bad("animation", "`path` only fits a motion path (class `path`)"));
    }
    if a.effect == "spin" {
        a.amount = Some(360.0);
    }
    if a.effect == "growShrink" {
        a.amount = Some(1.5);
    }
    Ok(a)
}

/// Most points of a motion path given as an array, and longest path string given as text.
const MAX_PATH_POINTS: usize = 1_000;
const MAX_PATH_TEXT: usize = 16 * 1024;
/// Largest offset of a point, in slide points; anything beyond is not a place on any slide.
const MAX_PATH_OFFSET: f64 = 1.0e6;

/// A number of a path string: slide fractions, rounded to 6 places.
fn path_num(v: f64) -> String {
    let t = format!("{:.6}", v);
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" || t.is_empty() { "0".into() } else { t.into() }
}

/// Does a raw motion path string draw: only commands and numbers, every number finite, and at
/// least two points? (A lone `e`, or `1e999`, would play as an empty or partial path.)
fn raw_path_draws(text: &str) -> bool {
    let cs: Vec<char> = text.chars().collect();
    let (mut i, mut nums) = (0, 0usize);
    while let Some(&c) = cs.get(i) {
        if c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit() || *d == '.')) {
            let start = i;
            i += 1;
            while cs.get(i).is_some_and(|d| d.is_ascii_digit() || *d == '.') {
                i += 1;
            }
            if cs.get(i).is_some_and(|d| *d == 'e' || *d == 'E') {
                let n1 = cs.get(i + 1).copied().unwrap_or(' ');
                let n2 = cs.get(i + 2).copied().unwrap_or(' ');
                if n1.is_ascii_digit() || ((n1 == '-' || n1 == '+') && n2.is_ascii_digit()) {
                    i += 2;
                    while cs.get(i).is_some_and(char::is_ascii_digit) {
                        i += 1;
                    }
                }
            }
            let tok: String = cs.get(start..i).map(|t| t.iter().collect()).unwrap_or_default();
            match tok.parse::<f64>() {
                Ok(v) if v.is_finite() => nums += 1,
                _ => return false,
            }
        } else if " ,MmLlCcQqHhVvZzE".contains(c) {
            i += 1;
        } else {
            return false;
        }
    }
    nums >= 4
}

/// The motion path `path` asks for, as the OOXML path string animations store: either points
/// (`[[x, y], …]`, slide points as offsets from where the shape sits, joined by lines, `closed?`
/// to return to the first) or that string itself.
fn path_param(p: &Value, slide: (f64, f64)) -> Result<Option<String>> {
    let Some(v) = p.get("path") else { return Ok(None) };
    let refuse = |why: &str| bad("animation", format!("`path` {why}"));
    match v {
        Value::String(text) => {
            let ok = text.len() <= MAX_PATH_TEXT
                && text.contains(|c: char| c.is_ascii_digit())
                && text.chars().all(|c| c.is_ascii_digit() || " .+-eE,MmLlCcQqHhVvZz".contains(c))
                && raw_path_draws(text);
            if !ok {
                return Err(refuse("string must be an OOXML motion path such as `M 0 0 L 0.25 0 E` (at most 16 KiB)"));
            }
            Ok(Some(text.clone()))
        }
        Value::Array(points) => {
            if !(2..=MAX_PATH_POINTS).contains(&points.len()) {
                return Err(refuse(&format!("needs 2 to {MAX_PATH_POINTS} points")));
            }
            if !(slide.0.is_finite() && slide.1.is_finite() && slide.0 > 0.0 && slide.1 > 0.0) {
                return Err(refuse("can't be placed: the slide has no size"));
            }
            let mut out = String::new();
            for (k, pt) in points.iter().enumerate() {
                let xy = pt.as_array().filter(|a| a.len() == 2).and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
                let Some((x, y)) = xy.filter(|(x, y)| x.abs() <= MAX_PATH_OFFSET && y.abs() <= MAX_PATH_OFFSET) else {
                    return Err(refuse(&format!("point {k} must be [x, y], two finite numbers within ±{MAX_PATH_OFFSET} points")));
                };
                out.push_str(&format!("{} {} {} ", if k == 0 { "M" } else { "L" }, path_num(x / slide.0), path_num(y / slide.1)));
            }
            if p.get("closed").and_then(Value::as_bool) == Some(true) {
                out.push_str("Z ");
            }
            out.push('E');
            Ok(Some(out))
        }
        _ => Err(refuse("must be [[x, y], …] or an OOXML path string")),
    }
}

/// Default motion paths, in slide-fraction units relative to the shape's position.
pub fn default_path(effect: &str, option: &str) -> String {
    let (dx, dy) = match option {
        "up" => (0.0, -0.25),
        "right" => (0.25, 0.0),
        "left" => (-0.25, 0.0),
        _ => (0.0, 0.25),
    };
    match effect {
        "arcs" => format!("M 0 0 C {} {} {} {} {dx} {dy} E", dx * 0.1 + dy * 0.5, dy * 0.1 - dx * 0.5, dx * 0.9 + dy * 0.5, dy * 0.9 - dx * 0.5),
        "turns" => format!("M 0 0 L {} {} C {} {} {dx} {} {dx} {dy} E", dx * 0.5, dy * 0.5, dx, dy * 0.5, dy * 0.75),
        "shapes" => "M 0 0 C 0.1 0 0.1 0.15 0 0.15 C -0.1 0.15 -0.1 0 0 0 Z E".into(),
        "loops" => "M 0 0 C 0.1 -0.1 0.2 0.1 0.1 0.1 C 0 0.1 0.1 -0.1 0.25 0 E".into(),
        other => deckcraft_model::anim::preset_path(other).unwrap_or_else(|| format!("M 0 0 L {dx} {dy} E")),
    }
}

/// `animation.set`: replaces the target shapes' effects with this one, each keeping its place in
/// the sequence, or only the chosen or highlighted paragraphs' effects (`paragraph_targets`).
/// `animation.set` with `index`: replaces that one effect, which keeps its target, its place and
/// when it starts (its delay too, unless `delay` is given).
fn set_at(s: &mut Session, p: &Value, index: usize) -> Result<Value> {
    let a = build(p, slide_extent(s)?)?;
    let keep_delay = p.get("delay").is_none();
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation.set", "no slide"))?);
        let x = sl.animations.get_mut(index).ok_or_else(|| bad("animation.set", "no such animation"))?;
        if a.effect == "none" {
            sl.animations.remove(index);
        } else {
            let delay_ms = if keep_delay { x.delay_ms } else { a.delay_ms };
            *x = Animation {
                shape: x.shape,
                paragraph: x.paragraph,
                start: x.start,
                trigger: x.trigger,
                text_build: x.text_build,
                delay_ms,
                ..a.clone()
            };
        }
        Ok(json!({"count": sl.animations.len()}))
    })
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(index) = opt_usize_param(p, "index", "animation.set")? {
        return set_at(s, p, index);
    }
    let ids = targets(s, p)?;
    let a = build(p, slide_extent(s)?)?;
    let paras = paragraph_targets(s, p, &ids)?;
    let built = splittable(s, &paras, &ids)?;
    // A by-word or by-letter effect can't be split per bullet without changing how it plays, and
    // a bullet's own effect beside it would animate that bullet twice.
    if let (false, [id]) = (built.is_empty(), ids.as_slice())
        && s.doc()?.current_slide().is_some_and(|sl| sl.animations.iter().any(|x| x.shape == *id && x.paragraph.is_none() && !can_split(x)))
    {
        return Err(bad("animation.set", "this box plays by word or letter: set Effect Options to By Paragraph"));
    }
    s.edit(|doc, sel| {
        let sl = doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation.set", "no slide"))?;
        let sl = Arc::make_mut(sl);
        if let (Some(paras), [id]) = (&paras, ids.as_slice()) {
            split_builds(&mut sl.animations, *id, &built);
            // Replace only these paragraphs' effects, in place, each keeping when it starts
            // (unless a delay is given); the shape's other effects stay.
            let keep_delay = p.get("delay").is_none();
            let fresh = per_paragraph(&a, *id, paras);
            let mut out = vec![];
            let mut placed = vec![];
            for x in sl.animations.drain(..) {
                match fresh.iter().find(|f| x.shape == *id && x.paragraph == f.paragraph) {
                    Some(f) if !placed.contains(&f.paragraph) => {
                        placed.push(f.paragraph);
                        out.push(Animation { start: x.start, delay_ms: if keep_delay { x.delay_ms } else { f.delay_ms }, ..f.clone() });
                    }
                    Some(_) => {}
                    None => out.push(x),
                }
            }
            out.extend(fresh.into_iter().filter(|f| !placed.contains(&f.paragraph)));
            if a.effect == "none" {
                out.retain(|x| !(x.shape == *id && paras.iter().any(|q| x.paragraph == Some(q.0))));
            }
            sl.animations = out;
            return Ok(json!({"count": sl.animations.len()}));
        }
        // Replace each shape's animations, keeping their place in the sequence.
        let mut out = vec![];
        let mut placed = vec![];
        for x in sl.animations.drain(..) {
            if ids.contains(&x.shape) {
                if !placed.contains(&x.shape) {
                    placed.push(x.shape);
                    out.push(Animation { shape: x.shape, start: x.start, ..a.clone() });
                }
            } else {
                out.push(x);
            }
        }
        for id in &ids {
            if !placed.contains(id) {
                out.push(Animation { shape: *id, ..a.clone() });
            }
        }
        if a.effect == "none" {
            out.retain(|x| !ids.contains(&x.shape));
        }
        sl.animations = out;
        Ok(json!({"count": sl.animations.len()}))
    })
}

/// The slide's width and height in points.
fn slide_extent(s: &mut Session) -> Result<(f64, f64)> {
    let size = &s.doc()?.doc.slide_size;
    Ok((size.width, size.height))
}

/// The paragraphs of one text box an effect should animate on their own, with the level of each,
/// or `None` to animate whole shapes. Explicit `paragraphs: [n, …]` (or `paragraph: n`) with one
/// target shape; otherwise `DocState::animated_paragraphs`, blank paragraphs skipped, an error
/// when all are blank.
fn paragraph_targets(s: &Session, p: &Value, ids: &[ShapeId]) -> Result<Option<Vec<(u32, u8)>>> {
    let st = s.doc()?;
    let text_of =
        |id: ShapeId| st.shape(id).and_then(|sh| sh.text.as_ref()).ok_or_else(|| bad("animation", "the shape has no text to animate by paragraph"));
    let explicit: Option<Vec<u64>> = match (p.get("paragraphs"), p.get("paragraph")) {
        (Some(Value::Array(a)), _) => Some(
            a.iter().map(|v| v.as_u64().ok_or_else(|| bad("animation", format!("`paragraphs` has an invalid index: {v}")))).collect::<Result<_>>()?,
        ),
        (Some(v), _) if !v.is_null() => return Err(bad("animation", format!("`paragraphs` must be an array of indices: {v}"))),
        (_, Some(v)) if !v.is_null() => Some(vec![v.as_u64().ok_or_else(|| bad("animation", format!("`paragraph` is not an index: {v}")))?]),
        _ => None,
    };
    let (id, wanted): (ShapeId, Vec<usize>) = if let Some(list) = explicit {
        let [id] = ids else { return Err(bad("animation", "`paragraphs` needs exactly one target shape")) };
        if list.is_empty() {
            return Err(bad("animation", "`paragraphs` is empty"));
        }
        let n = text_of(*id)?.paragraphs.len();
        let mut wanted: Vec<usize> = vec![];
        for v in list {
            let i = usize::try_from(v).unwrap_or(usize::MAX);
            if i >= n {
                return Err(bad("animation", format!("no paragraph {i} in the shape (0-based)")));
            }
            if !wanted.contains(&i) {
                wanted.push(i);
            }
        }
        (*id, wanted)
    } else {
        if p.get("ids").is_some() || p.get("id").is_some() {
            return Ok(None);
        }
        let (Some(t), Some((shape, chosen))) = (st.selection.text.as_ref(), st.animated_paragraphs()) else { return Ok(None) };
        if ids != [shape] {
            return Ok(None);
        }
        let ((p0, c0), (p1, c1)) = t.ordered();
        // A script can set any selection; check both ends against the text before listing its
        // paragraphs.
        let paragraphs = &text_of(t.shape)?.paragraphs;
        let n = paragraphs.len();
        for (p, c) in [(p0, c0), (p1, c1)] {
            let Some(para) = paragraphs.get(p) else {
                return Err(bad("animation", format!("the highlight reaches paragraph {p}, past the text ({n} paragraphs)")));
            };
            let len = para.char_len();
            if c > len {
                return Err(bad("animation", format!("the highlight reaches character {c} of paragraph {p}, past its text ({len} characters)")));
            }
        }
        (shape, chosen.collect())
    };
    let text = text_of(id)?;
    let mut out = vec![];
    for i in wanted {
        let para = text.paragraphs.get(i).ok_or_else(|| bad("animation", format!("no paragraph {i} in the shape (0-based)")))?;
        if deckcraft_model::anim::build_filled(para) {
            out.push((u32::try_from(i).map_err(|_| bad("animation", "paragraph index too large"))?, para.level));
        }
    }
    if out.is_empty() {
        return Err(bad("animation", "the chosen paragraphs are blank: nothing to animate"));
    }
    Ok(Some(out))
}

/// Delay between the paragraphs of a by-paragraph build that isn't On Click, as the show's
/// timeline plays it (`deckcraft_anim`'s `PARAGRAPH_STAGGER`; `splitting_a_build_keeps_its_timing`
/// checks they agree).
const PARAGRAPH_STAGGER_MS: u32 = 500;

/// How step `n` (0-based) of a by-paragraph sequence plays when its first step starts `first`
/// after `delay_ms`, as the show plays a by-paragraph build: On Click gives a click per step;
/// otherwise later steps start With Previous, `PARAGRAPH_STAGGER_MS` apart, rather than waiting
/// for the step before to finish.
fn step_timing(first: AnimStart, delay_ms: u32, n: u32) -> (AnimStart, u32) {
    match (n, first) {
        (0, f) => (f, delay_ms),
        (_, AnimStart::OnClick) => (AnimStart::OnClick, delay_ms),
        _ => (AnimStart::WithPrevious, delay_ms.saturating_add(n.saturating_mul(PARAGRAPH_STAGGER_MS))),
    }
}

/// One effect per paragraph: the first and each later shallowest paragraph take a step of
/// `step_timing`, deeper ones start With Previous at their bullet's time, so sub-bullets come in
/// with their bullet.
fn per_paragraph(a: &Animation, id: ShapeId, paras: &[(u32, u8)]) -> Vec<Animation> {
    let top = paras.iter().map(|x| x.1).min().unwrap_or(0);
    let mut step: u32 = 0;
    paras
        .iter()
        .enumerate()
        .map(|(k, &(i, lvl))| {
            let deeper = k > 0 && lvl > top;
            if k > 0 && !deeper {
                step += 1;
            }
            let (start, delay_ms) = step_timing(a.start, a.delay_ms, step);
            let start = if deeper { AnimStart::WithPrevious } else { start };
            Animation { shape: id, paragraph: Some(i), text_build: TextBuild::AsOne, start, delay_ms, ..a.clone() }
        })
        .collect()
}

/// The paragraphs a by-paragraph build of shape `id` animates, as PowerPoint writes them:
/// `(index, true)` for each paragraph with its own step, `(index, false)` for deeper ones that
/// come in with it.
fn build_paragraphs(s: &Session, id: ShapeId) -> Result<Vec<(u32, bool)>> {
    use deckcraft_model::anim::{BuildRole, build_filled, paragraph_build_roles};
    let Some(text) = s.doc()?.shape(id).and_then(|sh| sh.text.as_ref()) else { return Ok(vec![]) };
    Ok(paragraph_build_roles(text)
        .iter()
        .enumerate()
        .filter_map(|(i, role)| {
            let top = match role {
                BuildRole::Top => true,
                BuildRole::Member(_) if text.paragraphs.get(i).is_some_and(build_filled) => false,
                _ => return None,
            };
            u32::try_from(i).ok().map(|i| (i, top))
        })
        .collect())
}

/// Can this whole-shape effect become one effect per paragraph and still play the same?
fn can_split(a: &Animation) -> bool {
    matches!(a.text_build, TextBuild::AsOne | TextBuild::ByParagraph)
}

/// The paragraphs to split shape `ids[0]`'s whole-box effects into before acting on `paras`: none
/// when `paras` all have effects of their own, as those are the target and a whole-box effect
/// layered under them stays.
fn splittable(s: &Session, paras: &Option<Vec<(u32, u8)>>, ids: &[ShapeId]) -> Result<Vec<(u32, bool)>> {
    let (Some(paras), [id]) = (paras, ids) else { return Ok(vec![]) };
    let anims = s.doc()?.current_slide().map(|sl| sl.animations.as_slice()).unwrap_or(&[]);
    let own = |i: u32| anims.iter().any(|a| a.shape == *id && a.paragraph == Some(i));
    if paras.iter().all(|q| own(q.0)) { Ok(vec![]) } else { build_paragraphs(s, *id) }
}

/// Splits shape `id`'s whole-box As One and by-paragraph effects into one effect per paragraph, in
/// place and timed as they play, so one bullet's effect can change without moving the others: a
/// build by `step_timing`, deeper paragraphs with their bullet; As One all together.
fn split_builds(anims: &mut Vec<Animation>, id: ShapeId, paras: &[(u32, bool)]) {
    if paras.is_empty() {
        return;
    }
    let mut out = Vec::with_capacity(anims.len());
    for a in anims.drain(..) {
        if a.shape != id || a.paragraph.is_some() || !can_split(&a) {
            out.push(a);
            continue;
        }
        let as_one = a.text_build == TextBuild::AsOne;
        let mut tops: u32 = 0;
        for (n, &(i, top)) in paras.iter().enumerate() {
            if top {
                tops += 1;
            }
            let k = tops.saturating_sub(1);
            let (start, delay_ms) = if as_one { (a.start, a.delay_ms) } else { step_timing(a.start, a.delay_ms, k) };
            let start = if (top && !as_one) || n == 0 { start } else { AnimStart::WithPrevious };
            out.push(Animation { paragraph: Some(i), text_build: TextBuild::AsOne, start, delay_ms, ..a.clone() });
        }
    }
    *anims = out;
}

/// `animation.add`: appends this effect to the end of the sequence for each target shape, or one
/// effect per chosen or highlighted paragraph (`paragraph_targets`).
fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let a = build(p, slide_extent(s)?)?;
    let path = a.path.clone();
    let paras = paragraph_targets(s, p, &ids)?;
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation.add", "no slide"))?);
        if let (Some(paras), [id]) = (&paras, ids.as_slice()) {
            sl.animations.extend(per_paragraph(&a, *id, paras));
            return Ok(match &path {
                Some(path) => json!({"count": sl.animations.len(), "path": path}),
                None => json!({"count": sl.animations.len()}),
            });
        }
        for (k, id) in ids.iter().enumerate() {
            let mut x = Animation { shape: *id, ..a.clone() };
            if k > 0 {
                x.start = AnimStart::WithPrevious;
            }
            sl.animations.push(x);
        }
        Ok(match &path {
            Some(path) => json!({"count": sl.animations.len(), "path": path}),
            None => json!({"count": sl.animations.len()}),
        })
    })
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let index = opt_usize_param(p, "index", "animation.remove")?;
    let ids = if index.is_none() { targets(s, p)? } else { vec![] };
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation.remove", "no slide"))?);
        match index {
            Some(i) if i < sl.animations.len() => {
                sl.animations.remove(i);
            }
            Some(_) => return Err(bad("animation.remove", "no such animation")),
            None => sl.animations.retain(|a| !ids.contains(&a.shape)),
        }
        Ok(())
    })?;
    ok()
}

fn move_anim(s: &mut Session, p: &Value) -> Result<Value> {
    let i = opt_usize_param(p, "index", "animation.move")?.ok_or_else(|| bad("animation.move", "missing `index`"))?;
    let to = usize_param(p, "to").ok_or_else(|| bad("animation.move", "missing `to`"))?;
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation.move", "no slide"))?);
        if i >= sl.animations.len() {
            return Err(bad("animation.move", "no such animation"));
        }
        let a = sl.animations.remove(i);
        let to = to.min(sl.animations.len());
        sl.animations.insert(to, a);
        Ok(())
    })?;
    ok()
}

/// Applies `f` to the effect at `index`, else to the chosen or highlighted paragraphs' effects
/// (`paragraph_targets`), else to every effect of the target shapes. For paragraphs, a whole-box
/// effect is split first so only they change (`splittable`); one that can't be split changes
/// whole. `textBuild` says how a whole box plays, so paragraphs don't narrow it.
fn each_target(s: &mut Session, p: &Value, f: impl Fn(&mut Animation)) -> Result<Value> {
    let index = opt_usize_param(p, "index", "animation")?;
    let ids = if index.is_none() { targets(s, p)? } else { vec![] };
    let paras = if index.is_none() && p.get("textBuild").is_none() { paragraph_targets(s, p, &ids)? } else { None };
    let built = splittable(s, &paras, &ids)?;
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation", "no slide"))?);
        if let (Some(_), [id]) = (&paras, ids.as_slice()) {
            split_builds(&mut sl.animations, *id, &built);
        }
        let chosen = |a: &Animation| paras.iter().flatten().any(|q| a.paragraph == Some(q.0));
        let any_chosen = sl.animations.iter().any(|a| ids.contains(&a.shape) && chosen(a));
        let mut n = 0;
        for (k, a) in sl.animations.iter_mut().enumerate() {
            let hit = match (index, &paras) {
                (Some(i), _) => i == k,
                (None, None) => ids.contains(&a.shape),
                (None, Some(_)) => ids.contains(&a.shape) && if any_chosen { chosen(a) } else { a.paragraph.is_none() },
            };
            if hit {
                f(a);
                n += 1;
            }
        }
        if n == 0 {
            return Err(bad("animation", "no animation to change (select an animated object or give `index`)"));
        }
        Ok(json!({"changed": n}))
    })
}

fn timing(s: &mut Session, p: &Value) -> Result<Value> {
    let start = str_param(p, "start").map(start_of);
    let dur = p.get("duration").and_then(Value::as_u64).map(|v| v.min(600_000) as u32);
    let delay = p.get("delay").and_then(Value::as_u64).map(|v| v.min(600_000) as u32);
    let repeat = p.get("repeat").and_then(Value::as_u64).map(|v| v.min(u32::MAX as u64) as u32);
    let rewind = bool_param(p, "rewind");
    let trigger = p.get("trigger").map(|v| v.as_u64().and_then(|x| u32::try_from(x).ok()).map(deckcraft_model::ShapeId));
    each_target(s, p, |a| {
        if let Some(x) = start {
            a.start = x;
        }
        if let Some(x) = dur {
            a.duration_ms = x;
        }
        if let Some(x) = delay {
            a.delay_ms = x;
        }
        if let Some(x) = repeat {
            a.repeat = x;
        }
        if let Some(x) = rewind {
            a.rewind = x;
        }
        if let Some(t) = trigger {
            a.trigger = t;
        }
    })
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let option = str_param(p, "option").map(String::from);
    let build_mode = str_param(p, "textBuild").map(|b| match b {
        "byParagraph" => TextBuild::ByParagraph,
        "byWord" => TextBuild::ByWord,
        "byLetter" => TextBuild::ByLetter,
        _ => TextBuild::AsOne,
    });
    let path = path_param(p, slide_extent(s)?)?;
    each_target(s, p, |a| {
        if let (Some(path), AnimClass::Path) = (&path, a.class) {
            a.path = Some(path.clone());
        }
        if let Some(o) = &option {
            // Each target takes the option in its own effect's terms; one it has no such
            // option for keeps its own.
            let allowed = animation_info(&a.effect, a.class).map(|i| i.5).unwrap_or(&[]);
            if let Some(o) = resolve_option(allowed, o) {
                if a.class == AnimClass::Path && path.is_none() {
                    a.path = Some(default_path(&a.effect, &o));
                }
                a.option = o;
            }
        }
        if let Some(b) = build_mode {
            a.text_build = b;
        }
    })
}

fn clear(s: &mut Session, p: &Value) -> Result<Value> {
    let i = match opt_usize_param(p, "index", "animation.clear")? {
        Some(i) => i,
        None => s.doc()?.selection.slide,
    };
    s.edit(|doc, _| {
        if let Some(sl) = doc.slides.get_mut(i) {
            Arc::make_mut(sl).animations.clear();
        }
        Ok(())
    })?;
    ok()
}

fn list(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(ANIMATIONS.iter().map(|a| json!({"id": a.0, "label": a.1, "class": a.2.xml(), "options": a.5})).collect()))
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let i = usize_param(p, "slide").unwrap_or(st.selection.slide);
    let sl = st.doc.slides.get(i).ok_or_else(|| bad("animation.get", "no such slide"))?;
    Ok(serde_json::to_value(&sl.animations).unwrap_or_default())
}

fn painter_pick(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let id = id_param(p, "id", "animation.painter")?
        .or_else(|| st.selection.shapes.first().copied())
        .ok_or_else(|| bad("animation.painter", "select an animated shape first"))?;
    let sl = st.current_slide().ok_or_else(|| bad("animation.painter", "no slide"))?;
    let anims: Vec<Animation> = sl.animations.iter().filter(|a| a.shape == id).cloned().collect();
    if anims.is_empty() {
        return Err(bad("animation.painter", format!("shape {id} has no animations to copy")));
    }
    let sticky = bool_or(p, "sticky", false);
    let count = anims.len();
    s.anim_painter = Some((anims, sticky));
    Ok(json!({"armed": true, "count": count}))
}

fn painter_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let Some((painted, sticky)) = s.anim_painter.clone() else {
        return Err(bad("animation.painterApply", "pick animations with the Animation Painter first"));
    };
    if !sticky {
        s.anim_painter = None;
    }
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad("animation.painterApply", "no target shapes selected"));
    }
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation.painterApply", "no slide"))?);
        // As in PowerPoint, painting replaces the target's own animations (painting a shape onto
        // itself doesn't double them). A paragraph effect keeps its paragraph only if the target
        // has that paragraph; otherwise it animates the whole shape.
        sl.animations.retain(|a| !ids.contains(&a.shape));
        let mut added = 0;
        for id in &ids {
            let paras = deckcraft_model::find_shape(&sl.shapes, *id).and_then(|sh| sh.text.as_ref()).map_or(0, |t| t.paragraphs.len());
            for a in &painted {
                let mut cloned = a.clone();
                cloned.shape = *id;
                if cloned.paragraph.is_some_and(|i| i as usize >= paras) {
                    cloned.paragraph = None;
                }
                sl.animations.push(cloned);
                added += 1;
            }
        }
        Ok(json!({"applied": added}))
    })
}

#[cfg(test)]
mod tests {
    use super::resolve_option;

    #[test]
    fn direction_words_resolve_to_codes() {
        let fly = &["b", "lb", "l", "lt", "t", "rt", "r", "rb"][..];
        assert_eq!(resolve_option(fly, "fromBottom").as_deref(), Some("b"));
        assert_eq!(resolve_option(fly, "bottom").as_deref(), Some("b"));
        assert_eq!(resolve_option(fly, "fromTopLeft").as_deref(), Some("lt"));
        assert_eq!(resolve_option(fly, "fromBottomRight").as_deref(), Some("rb"));
        assert_eq!(resolve_option(fly, "lt").as_deref(), Some("lt"));
        assert_eq!(resolve_option(fly, "sideways"), None);
        assert_eq!(resolve_option(fly, "fromBottomTopLeft"), None);
        let wipe = &["r", "l", "u", "d", "ru", "lu", "rd", "ld"][..];
        assert_eq!(resolve_option(wipe, "fromBottom").as_deref(), Some("d"));
        assert_eq!(resolve_option(wipe, "upRight").as_deref(), Some("ru"));
        let split = &["horzIn", "horzOut", "vertIn", "vertOut"][..];
        assert_eq!(resolve_option(split, "HorzIn").as_deref(), Some("horzIn"));
        assert_eq!(resolve_option(&[], "anything").as_deref(), Some("anything"));
    }

    use super::*;

    #[test]
    fn clear_rejects_invalid_index() {
        let mut s = Session::with_new();
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10.0, 10.0, 100.0, 100.0]})).unwrap();
        let id = r.get("id").unwrap().as_u64().unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [id]})).unwrap();
        for bad_index in [json!(-1), json!(1.5)] {
            assert!(s.execute("animation.clear", &json!({"index": bad_index})).is_err());
            let left = s.execute("animation.get", &json!({})).unwrap();
            assert_eq!(left.as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn animation_painter_copies_effects_between_shapes() {
        let mut s = Session::with_new();
        // Insert two shapes
        let s1 = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10.0, 10.0, 100.0, 100.0]})).unwrap();
        let id1 = s1.get("id").unwrap().as_u64().unwrap();
        let s2 = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [150.0, 10.0, 100.0, 100.0]})).unwrap();
        let id2 = s2.get("id").unwrap().as_u64().unwrap();

        // Add 2 animations to shape 1: fade (entrance) and spin (emphasis)
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [id1]})).unwrap();
        s.execute("animation.add", &json!({"effect": "spin", "class": "emphasis", "ids": [id1]})).unwrap();

        // Shape 2 initially has no animations
        let anims_before = s.execute("animation.get", &json!({})).unwrap();
        let arr_before = anims_before.as_array().unwrap();
        assert_eq!(arr_before.len(), 2);
        assert!(arr_before.iter().all(|a| a.get("shape").unwrap().as_u64().unwrap() == id1));

        // Pick animations from shape 1
        s.select(|_, sel| {
            sel.shapes = vec![deckcraft_model::ShapeId(id1 as u32)];
        })
        .unwrap();
        let res = s.execute("animation.painter", &json!({})).unwrap();
        assert_eq!(res.get("armed").unwrap().as_bool(), Some(true));
        assert_eq!(res.get("count").unwrap().as_u64(), Some(2));
        assert!(s.anim_painter.is_some());

        // Apply animations to shape 2
        s.select(|_, sel| {
            sel.shapes = vec![deckcraft_model::ShapeId(id2 as u32)];
        })
        .unwrap();
        let applied = s.execute("animation.painterApply", &json!({})).unwrap();
        assert_eq!(applied.get("applied").unwrap().as_u64(), Some(2));

        // Single-click should disarm the painter
        assert!(s.anim_painter.is_none());

        // Painting again replaces shape 2's animations instead of adding to them; undo restores.
        s.execute("animation.painter", &json!({"id": id1})).unwrap();
        s.execute("animation.painterApply", &json!({"ids": [id2]})).unwrap();
        let count_on = |s: &mut Session, id: u64| {
            let all = s.execute("animation.get", &json!({})).unwrap();
            all.as_array().unwrap().iter().filter(|a| a.get("shape").unwrap().as_u64().unwrap() == id).count()
        };
        assert_eq!(count_on(&mut s, id2), 2);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(count_on(&mut s, id2), 2, "undo restores the first painting");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(count_on(&mut s, id2), 0, "undo removes the painted animations");
        s.execute("edit.redo", &json!({})).unwrap();

        // Verify shape 2 now has the 2 animations cloned
        let anims_after = s.execute("animation.get", &json!({})).unwrap();
        let arr_after = anims_after.as_array().unwrap();
        assert_eq!(arr_after.len(), 4);
        let s2_anims: Vec<_> = arr_after.iter().filter(|a| a.get("shape").unwrap().as_u64().unwrap() == id2).collect();
        assert_eq!(s2_anims.len(), 2);
        assert_eq!(s2_anims[0].get("effect").unwrap().as_str(), Some("fade"));
        assert_eq!(s2_anims[1].get("effect").unwrap().as_str(), Some("spin"));

        // Picking from shape with no animations returns an error
        s.execute("animation.clear", &json!({})).unwrap();
        let err = s.execute("animation.painter", &json!({"id": id1}));
        assert!(err.is_err());
    }

    #[test]
    fn animation_painter_sticky_mode() {
        let mut s = Session::with_new();
        let s1 = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10.0, 10.0, 100.0, 100.0]})).unwrap();
        let id1 = s1.get("id").unwrap().as_u64().unwrap();
        let s2 = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [150.0, 10.0, 100.0, 100.0]})).unwrap();
        let id2 = s2.get("id").unwrap().as_u64().unwrap();
        let s3 = s.execute("shape.insert", &json!({"preset": "roundRect", "rect": [300.0, 10.0, 100.0, 100.0]})).unwrap();
        let id3 = s3.get("id").unwrap().as_u64().unwrap();

        s.execute("animation.add", &json!({"effect": "zoom", "class": "entrance", "ids": [id1]})).unwrap();

        // Pick with sticky = true
        s.execute("animation.painter", &json!({"id": id1, "sticky": true})).unwrap();
        assert!(s.anim_painter.is_some());

        // Apply to shape 2 - painter remains armed
        s.execute("animation.painterApply", &json!({"ids": [id2]})).unwrap();
        assert!(s.anim_painter.is_some());

        // Apply to shape 3 - painter still armed
        s.execute("animation.painterApply", &json!({"ids": [id3]})).unwrap();
        assert!(s.anim_painter.is_some());
    }

    fn setup_triggers() -> (Session, [u64; 4]) {
        let mut s = Session::with_new();
        let mut ids = [0u64; 4];
        for (slot, x) in ids.iter_mut().zip([10.0, 150.0, 300.0, 450.0]) {
            let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 10.0, 100.0, 100.0]})).unwrap();
            *slot = r.get("id").unwrap().as_u64().unwrap();
        }
        (s, ids)
    }

    fn triggers(s: &Session) -> Vec<Option<deckcraft_model::ShapeId>> {
        s.doc().unwrap().current_slide().unwrap().animations.iter().map(|x| x.trigger).collect()
    }

    #[test]
    fn deleting_a_trigger_shape_clears_the_trigger() {
        let (mut s, [a, b, c, d]) = setup_triggers();
        let id = |v: u64| Some(deckcraft_model::ShapeId(v as u32));
        s.execute("animation.add", &json!({"effect": "fade", "ids": [a]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "ids": [d]})).unwrap();
        s.execute("animation.timing", &json!({"index": 0, "trigger": b})).unwrap();
        s.execute("animation.timing", &json!({"index": 1, "trigger": c})).unwrap();
        assert_eq!(triggers(&s), vec![id(b), id(c)]);

        s.execute("edit.delete", &json!({"ids": [b]})).unwrap();
        assert_eq!(triggers(&s), vec![None, id(c)], "only the deleted trigger is cleared");
        assert_eq!(s.doc().unwrap().current_slide().unwrap().animations.len(), 2, "the animated shapes keep their effects");
    }

    #[test]
    fn deleting_a_group_clears_triggers_of_its_children() {
        let (mut s, [a, b, c, _]) = setup_triggers();
        let g = s.execute("arrange.group", &json!({"ids": [b, c]})).unwrap().get("id").unwrap().as_u64().unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "ids": [a]})).unwrap();
        s.execute("animation.timing", &json!({"index": 0, "trigger": b})).unwrap();
        assert_eq!(triggers(&s), vec![Some(deckcraft_model::ShapeId(b as u32))]);

        s.execute("edit.delete", &json!({"ids": [g]})).unwrap();
        assert_eq!(triggers(&s), vec![None], "the trigger was inside the deleted group");
        assert_eq!(s.doc().unwrap().current_slide().unwrap().animations.len(), 1);
    }

    #[test]
    fn invalid_index_is_an_error_not_the_selection() {
        let mut s = Session::with_new();
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10.0, 10.0, 100.0, 100.0]})).unwrap();
        let id = r.get("id").unwrap().as_u64().unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [id]})).unwrap();
        s.select(|_, sel| {
            sel.shapes = vec![deckcraft_model::ShapeId(id as u32)];
        })
        .unwrap();
        for bad_index in [json!(-1), json!(1.5), json!("x")] {
            for cmd in ["animation.remove", "animation.timing", "animation.options"] {
                let r = s.execute(cmd, &json!({"index": bad_index, "duration": 100}));
                assert!(r.is_err(), "{cmd} accepted index {bad_index}: {r:?}");
            }
            let r = s.execute("animation.move", &json!({"index": bad_index, "to": 0}));
            assert!(r.is_err(), "animation.move accepted index {bad_index}: {r:?}");
            assert_eq!(s.execute("animation.get", &json!({})).unwrap().as_array().unwrap().len(), 1);
        }
    }

    fn path_of(s: &mut Session) -> Option<String> {
        let all = s.execute("animation.get", &json!({})).unwrap();
        all.as_array().unwrap().last().and_then(|a| a.get("path")).and_then(Value::as_str).map(String::from)
    }

    fn shape(s: &mut Session) -> u64 {
        s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [100.0, 400.0, 20.0, 20.0]})).unwrap().get("id").unwrap().as_u64().unwrap()
    }

    /// #1576: points in slide points, as offsets from the shape, become the slide-fraction path.
    #[test]
    fn a_custom_path_takes_its_points() {
        let mut s = Session::with_new();
        let id = shape(&mut s);
        let w = s.doc().unwrap().doc.slide_size.width;
        let h = s.doc().unwrap().doc.slide_size.height;
        let r = s
            .execute(
                "animation.add",
                &json!({"effect": "customPath", "class": "path", "path": [[0, 0], [w / 2.0, -h / 4.0], [w, -h / 2.0]], "ids": [id]}),
            )
            .unwrap();
        assert_eq!(r["path"], "M 0 0 L 0.5 -0.25 L 1 -0.5 E");
        assert_eq!(path_of(&mut s).as_deref(), Some("M 0 0 L 0.5 -0.25 L 1 -0.5 E"));
        // closed shapes end back at the start; set replaces; no path keeps the default
        s.execute("animation.set", &json!({"effect": "customPath", "path": [[0, 0], [w / 4.0, 0], [w / 4.0, h / 4.0]], "closed": true, "ids": [id]}))
            .unwrap();
        assert_eq!(path_of(&mut s).as_deref(), Some("M 0 0 L 0.25 0 L 0.25 0.25 Z E"));
        s.execute("animation.set", &json!({"effect": "customPath", "ids": [id]})).unwrap();
        assert_eq!(path_of(&mut s).as_deref(), Some("M 0 0 L 0 0.25 E"));
    }

    #[test]
    fn a_raw_path_string_is_kept_and_options_edits_it() {
        let mut s = Session::with_new();
        let id = shape(&mut s);
        s.execute("animation.add", &json!({"effect": "customPath", "path": "M 0 0 C 0.1 0 0.2 0.1 0.3 0.1 E", "ids": [id]})).unwrap();
        assert_eq!(path_of(&mut s).as_deref(), Some("M 0 0 C 0.1 0 0.2 0.1 0.3 0.1 E"));
        s.execute("animation.options", &json!({"index": 0, "path": [[0, 0], [96, 54]]})).unwrap();
        assert_eq!(path_of(&mut s).as_deref(), Some("M 0 0 L 0.1 0.1 E"));
    }

    #[test]
    fn a_bad_path_is_refused_and_changes_nothing() {
        let mut s = Session::with_new();
        let id = shape(&mut s);
        let many: Vec<Value> = (0..2_000).map(|i| json!([i, i])).collect();
        for bad_path in [
            json!([[0, 0]]),
            json!([]),
            json!([[0, 0], [1]]),
            json!([[0, 0], ["a", 1]]),
            json!([[0, 0], [1e300, 1]]),
            json!([[0, 0], [null, 1]]),
            json!("M 0 0 L <script> E"),
            json!("not a path"),
            json!("eeee1"),
            json!("M 0 0 L 1e999 0 E"),
            json!(5),
            Value::Array(many),
            Value::String("M 0 0 L 0.1 0.1 ".repeat(10_000)),
        ] {
            let r = s.execute("animation.add", &json!({"effect": "customPath", "path": bad_path, "ids": [id]}));
            assert!(r.is_err(), "{bad_path:.60}");
        }
        assert!(
            s.execute("animation.add", &json!({"effect": "fade", "path": [[0, 0], [1, 1]], "ids": [id]})).is_err(),
            "path only fits a motion path"
        );
        assert!(s.execute("animation.get", &json!({})).unwrap().as_array().unwrap().is_empty(), "a refused add left an animation behind");
    }

    /// A text box with bullets "One", "Two", "Three", being edited, plus a picture-like shape.
    fn bullets() -> (Session, u64, u64) {
        let mut s = Session::with_new();
        let r = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 200], "text": "One\nTwo\nThree"})).unwrap();
        let tb = r.get("id").unwrap().as_u64().unwrap();
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [500.0, 50.0, 100.0, 100.0]})).unwrap();
        let pic = r.get("id").unwrap().as_u64().unwrap();
        s.execute("text.edit", &json!({"id": tb})).unwrap();
        (s, tb, pic)
    }

    fn effects(s: &mut Session) -> Vec<(u64, Option<u64>, String, String)> {
        let v = s.execute("animation.get", &json!({})).unwrap();
        v.as_array()
            .unwrap()
            .iter()
            .map(|a| {
                (
                    a["shape"].as_u64().unwrap(),
                    a["paragraph"].as_u64(),
                    a["effect"].as_str().unwrap().to_string(),
                    a["start"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn highlighted_bullet_gets_its_own_effect() {
        let (mut s, tb, pic) = bullets();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, Some(1), "fade".into(), "onClick".into())]);
        // The picture joins bullet 2 With Previous.
        s.execute("animation.add", &json!({"effect": "zoom", "class": "entrance", "ids": [pic], "start": "withPrevious"})).unwrap();
        let e = effects(&mut s);
        assert_eq!(e[1], (pic, None, "zoom".into(), "withPrevious".into()));
    }

    #[test]
    fn caret_without_highlight_animates_whole_box() {
        let (mut s, tb, _) = bullets();
        s.execute("text.select", &json!({"anchor": [1, 1], "caret": [1, 1]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, None, "fade".into(), "onClick".into())]);
    }

    #[test]
    fn highlight_over_two_bullets_gives_one_click_each() {
        let (mut s, tb, _) = bullets();
        // From inside "One" to the very start of "Three": "Three" is not included.
        s.execute("text.select", &json!({"anchor": [0, 1], "caret": [2, 0]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, Some(0), "fade".into(), "onClick".into()), (tb, Some(1), "fade".into(), "onClick".into())]);
    }

    #[test]
    fn gallery_on_highlighted_bullet_replaces_only_that_bullet() {
        let (mut s, tb, _) = bullets();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 3]})).unwrap();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        s.execute("text.select", &json!({"anchor": [2, 0], "caret": [2, 5]})).unwrap();
        s.execute("animation.set", &json!({"effect": "fly", "class": "entrance"})).unwrap();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 3]})).unwrap();
        s.execute("animation.set", &json!({"effect": "zoom", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, Some(0), "zoom".into(), "onClick".into()), (tb, Some(2), "fly".into(), "onClick".into())]);
    }

    #[test]
    fn explicit_paragraphs_param() {
        let mut s = Session::with_new();
        let r = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 200], "text": "One\nTwo\nThree"})).unwrap();
        let tb = r.get("id").unwrap().as_u64().unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [tb], "paragraphs": [2]})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, Some(2), "fade".into(), "onClick".into())]);
        assert!(s.execute("animation.add", &json!({"effect": "fade", "ids": [tb], "paragraphs": [7]})).is_err());
        assert!(s.execute("animation.add", &json!({"effect": "fade", "ids": [tb], "paragraphs": []})).is_err());
        assert!(s.execute("animation.add", &json!({"effect": "fade", "ids": [tb], "paragraphs": 2})).is_err());
        assert!(s.execute("animation.add", &json!({"effect": "fade", "ids": [tb], "paragraphs": "1"})).is_err());
        s.execute("animation.add", &json!({"effect": "fly", "ids": [tb], "paragraph": 0})).unwrap();
        s.execute("animation.add", &json!({"effect": "zoom", "ids": [tb], "paragraphs": [1, 1]})).unwrap();
        let e = effects(&mut s);
        assert_eq!(e.len(), 3);
        assert_eq!(e[1], (tb, Some(0), "fly".into(), "onClick".into()));
        assert_eq!(e[2], (tb, Some(1), "zoom".into(), "onClick".into()));
    }

    #[test]
    fn gallery_on_bullet_of_by_paragraph_build_splits_it() {
        let (mut s, tb, _) = bullets();
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("edit.select", &json!({"ids": [tb]})).unwrap();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        s.execute("animation.options", &json!({"textBuild": "byParagraph"})).unwrap();
        s.execute("text.edit", &json!({"id": tb})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.set", &json!({"effect": "fly", "class": "entrance"})).unwrap();
        assert_eq!(
            effects(&mut s),
            vec![
                (tb, Some(0), "fade".into(), "onClick".into()),
                (tb, Some(1), "fly".into(), "onClick".into()),
                (tb, Some(2), "fade".into(), "onClick".into()),
            ]
        );
    }

    #[test]
    fn sub_bullets_come_in_with_their_bullet() {
        let mut s = Session::with_new();
        let r = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 200], "text": "One\nOne a\nTwo"})).unwrap();
        let tb = r.get("id").unwrap().as_u64().unwrap();
        s.execute("text.edit", &json!({"id": tb, "at": [1, 0]})).unwrap();
        s.execute("format.indent", &json!({})).unwrap();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [2, 3]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        assert_eq!(
            effects(&mut s),
            vec![
                (tb, Some(0), "fade".into(), "onClick".into()),
                (tb, Some(1), "fade".into(), "withPrevious".into()),
                (tb, Some(2), "fade".into(), "onClick".into()),
            ]
        );
    }

    #[test]
    fn highlighting_only_blank_lines_is_refused() {
        let mut s = Session::with_new();
        let r = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 200], "text": "One\n\nThree"})).unwrap();
        let tb = r.get("id").unwrap().as_u64().unwrap();
        s.execute("text.edit", &json!({"id": tb})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [2, 0]})).unwrap();
        assert!(s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).is_err());
        assert!(effects(&mut s).is_empty());
    }

    #[test]
    fn highlight_past_the_text_is_refused_before_listing_paragraphs() {
        // `text.select` stores any coordinates; a huge one must be refused, not listed paragraph by paragraph.
        let (mut s, _, _) = bullets();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [1_000_000_000_000_000_u64, 0]})).unwrap();
        assert!(s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).is_err());
        assert!(s.execute("animation.set", &json!({"effect": "fade", "class": "entrance"})).is_err());
        s.execute("text.select", &json!({"anchor": [3, 0], "caret": [3, 2]})).unwrap();
        assert!(s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).is_err());
        assert!(effects(&mut s).is_empty());
    }

    #[test]
    fn highlighting_bullets_two_and_three_of_five() {
        let mut s = Session::with_new();
        let r = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 300], "text": "One\nTwo\nThree\nFour\nFive"})).unwrap();
        let tb = r.get("id").unwrap().as_u64().unwrap();
        s.execute("text.edit", &json!({"id": tb})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 1], "caret": [2, 3]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, Some(1), "fade".into(), "onClick".into()), (tb, Some(2), "fade".into(), "onClick".into())]);
        // The same by index, from a script.
        s.execute("animation.set", &json!({"effect": "fly", "class": "entrance", "ids": [tb], "paragraphs": [1, 2]})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, Some(1), "fly".into(), "onClick".into()), (tb, Some(2), "fly".into(), "onClick".into())]);
    }

    #[test]
    fn highlight_inside_one_bullet_animates_the_whole_bullet() {
        let (mut s, tb, _) = bullets();
        // Only the "w" of "Two".
        s.execute("text.select", &json!({"anchor": [1, 1], "caret": [1, 2]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, Some(1), "fade".into(), "onClick".into())]);
        let st = s.doc().unwrap();
        let size = st.doc.slide_size;
        let tl = deckcraft_anim::Timeline::new(st.current_slide().unwrap(), size.width, size.height);
        assert_eq!(tl.paragraph_targets(deckcraft_model::ShapeId(tb as u32)), vec![1], "the paragraph plays as a whole, the others not at all");
    }

    #[test]
    fn undo_restores_paragraph_effects() {
        let (mut s, tb, _) = bullets();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 3]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        s.execute("text.select", &json!({"anchor": [2, 0], "caret": [2, 5]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fly", "class": "entrance"})).unwrap();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 3]})).unwrap();
        s.execute("animation.set", &json!({"effect": "zoom", "class": "entrance"})).unwrap();
        let fade0 = (tb, Some(0), "fade".to_string(), "onClick".to_string());
        let fly2 = (tb, Some(2), "fly".to_string(), "onClick".to_string());
        assert_eq!(effects(&mut s), vec![(tb, Some(0), "zoom".into(), "onClick".into()), fly2.clone()]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(effects(&mut s), vec![fade0.clone(), fly2], "undo brings back the replaced bullet's effect");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(effects(&mut s), vec![fade0.clone()]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(effects(&mut s).is_empty());
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(effects(&mut s), vec![fade0]);
    }

    #[test]
    fn undo_joins_a_split_by_paragraph_build_again() {
        let (mut s, tb, _) = bullets();
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("edit.select", &json!({"ids": [tb]})).unwrap();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        s.execute("animation.options", &json!({"textBuild": "byParagraph"})).unwrap();
        s.execute("text.edit", &json!({"id": tb})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.set", &json!({"effect": "fly", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s).len(), 3);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, None, "fade".into(), "onClick".into())]);
        let st = s.doc().unwrap();
        assert_eq!(st.current_slide().unwrap().animations[0].text_build, TextBuild::ByParagraph);
    }

    #[test]
    fn paragraph_effects_play_in_order() {
        let mut s = Session::with_new();
        let r = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 300], "text": "One\nOne a\nTwo\nThree"})).unwrap();
        let tb = r.get("id").unwrap().as_u64().unwrap();
        s.execute("text.edit", &json!({"id": tb, "at": [1, 0]})).unwrap();
        s.execute("format.indent", &json!({})).unwrap();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [3, 5]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        let timings = |s: &mut Session| {
            let st = s.doc().unwrap();
            let size = st.doc.slide_size;
            let tl = deckcraft_anim::Timeline::new(st.current_slide().unwrap(), size.width, size.height);
            tl.effects().into_iter().map(|e| (e.paragraph, e.step, e.start)).collect::<Vec<_>>()
        };
        // On Click: one click per bullet; the sub-bullet comes in with "One".
        let t = timings(&mut s);
        assert_eq!(t.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(Some(0), 0), (Some(1), 0), (Some(2), 1), (Some(3), 2)]);
        assert_eq!(t[0].2, t[1].2, "the sub-bullet starts with its bullet");
        // After Previous: one click for all, each bullet staggered after the one before.
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute(
            "animation.add",
            &json!({"effect": "fade", "class": "entrance", "ids": [tb], "paragraphs": [0, 1, 2, 3], "start": "afterPrevious"}),
        )
        .unwrap();
        let t = timings(&mut s);
        assert!(t.iter().all(|x| x.1 == t[0].1), "one step: {t:?}");
        assert_eq!(t[0].2, t[1].2, "the sub-bullet starts with its bullet");
        assert!(t[1].2 < t[2].2 && t[2].2 < t[3].2, "the bullets play one after another: {t:?}");
    }

    /// Each effect's paragraph, duration and delay, in order.
    fn times(s: &Session) -> Vec<(Option<u32>, u32, u32)> {
        s.doc().unwrap().current_slide().unwrap().animations.iter().map(|a| (a.paragraph, a.duration_ms, a.delay_ms)).collect()
    }

    /// `bullets()` with a Fade on each bullet, On Click, and "Two" highlighted.
    fn bullets_with_effects() -> (Session, u64) {
        let (mut s, tb, _) = bullets();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [tb], "paragraphs": [0, 1, 2], "duration": 500})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        (s, tb)
    }

    #[test]
    fn timing_on_a_highlighted_bullet_changes_only_that_bullet() {
        let (mut s, _) = bullets_with_effects();
        s.execute("animation.timing", &json!({"duration": 2000, "delay": 300, "start": "withPrevious"})).unwrap();
        assert_eq!(times(&s), vec![(Some(0), 500, 0), (Some(1), 2000, 300), (Some(2), 500, 0)]);
        assert_eq!(effects(&mut s).iter().map(|e| e.3.as_str()).collect::<Vec<_>>(), vec!["onClick", "withPrevious", "onClick"]);
        // `index` still means that one effect, and the shape on its own means all of them.
        s.execute("animation.timing", &json!({"index": 2, "duration": 700})).unwrap();
        assert_eq!(times(&s), vec![(Some(0), 500, 0), (Some(1), 2000, 300), (Some(2), 700, 0)]);
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("animation.timing", &json!({"duration": 900})).unwrap();
        assert_eq!(times(&s), vec![(Some(0), 900, 0), (Some(1), 900, 300), (Some(2), 900, 0)]);
    }

    #[test]
    fn timing_takes_explicit_paragraphs() {
        let (mut s, tb) = bullets_with_effects();
        s.execute("animation.timing", &json!({"ids": [tb], "paragraphs": [0, 2], "duration": 2000})).unwrap();
        assert_eq!(times(&s), vec![(Some(0), 2000, 0), (Some(1), 500, 0), (Some(2), 2000, 0)]);
        // With `ids` alone, the highlight doesn't narrow it.
        s.execute("animation.timing", &json!({"ids": [tb], "duration": 800})).unwrap();
        assert_eq!(times(&s), vec![(Some(0), 800, 0), (Some(1), 800, 0), (Some(2), 800, 0)]);
    }

    #[test]
    fn effect_options_on_a_highlighted_bullet_change_only_that_bullet() {
        let (mut s, tb, _) = bullets();
        s.execute("animation.add", &json!({"effect": "fly", "class": "entrance", "ids": [tb], "paragraphs": [0, 1, 2]})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        let options = |s: &Session| s.doc().unwrap().current_slide().unwrap().animations.iter().map(|a| a.option.clone()).collect::<Vec<_>>();
        let was = options(&s)[0].clone();
        let other = animation_info("fly", AnimClass::Entrance).unwrap().5.iter().find(|o| **o != was).unwrap().to_string();
        s.execute("animation.options", &json!({"option": other})).unwrap();
        assert_eq!(options(&s), vec![was.clone(), other, was]);
    }

    #[test]
    fn timing_on_a_bullet_of_a_build_splits_it() {
        let (mut s, tb, _) = bullets();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance", "ids": [tb], "duration": 500})).unwrap();
        s.execute("animation.options", &json!({"ids": [tb], "textBuild": "byParagraph"})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.timing", &json!({"duration": 2000})).unwrap();
        assert_eq!(times(&s), vec![(Some(0), 500, 0), (Some(1), 2000, 0), (Some(2), 500, 0)]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(times(&s), vec![(None, 500, 0)], "one undo joins the build again");
    }

    #[test]
    fn timing_on_a_bullet_of_an_as_one_box_splits_it() {
        let (mut s, tb, _) = bullets();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance", "ids": [tb], "duration": 500})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.timing", &json!({"duration": 2000})).unwrap();
        assert_eq!(times(&s), vec![(Some(0), 500, 0), (Some(1), 2000, 0), (Some(2), 500, 0)]);
        // As One plays everything together: one click, the rest With Previous.
        assert_eq!(effects(&mut s).iter().map(|e| e.3.as_str()).collect::<Vec<_>>(), vec!["onClick", "withPrevious", "withPrevious"]);
    }

    #[test]
    fn timing_on_a_by_word_box_changes_its_one_effect() {
        let (mut s, tb, _) = bullets();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance", "ids": [tb], "duration": 500})).unwrap();
        s.execute("animation.options", &json!({"ids": [tb], "textBuild": "byWord"})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.timing", &json!({"duration": 2000})).unwrap();
        assert_eq!(times(&s), vec![(None, 2000, 0)]);
    }

    #[test]
    fn a_bullets_own_effect_changes_without_splitting_a_layered_box() {
        // A whole-box entrance with an emphasis on one bullet: that bullet's effect is the target.
        let (mut s, tb, _) = bullets();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance", "ids": [tb], "duration": 500})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.add", &json!({"effect": "pulse", "class": "emphasis", "duration": 500})).unwrap();
        s.execute("animation.timing", &json!({"duration": 2000})).unwrap();
        assert_eq!(times(&s), vec![(None, 500, 0), (Some(1), 2000, 0)]);
        s.execute("animation.set", &json!({"effect": "spin", "class": "emphasis"})).unwrap();
        assert_eq!(effects(&mut s).iter().map(|e| (e.1, e.2.as_str())).collect::<Vec<_>>(), vec![(None, "fade"), (Some(1), "spin")]);
    }

    #[test]
    fn a_caret_in_a_box_of_bullet_effects_means_that_bullet() {
        let (mut s, _) = bullets_with_effects();
        s.execute("text.select", &json!({"anchor": [1, 1], "caret": [1, 1]})).unwrap();
        s.execute("animation.timing", &json!({"duration": 2000})).unwrap();
        assert_eq!(times(&s), vec![(Some(0), 500, 0), (Some(1), 2000, 0), (Some(2), 500, 0)]);
        s.execute("animation.set", &json!({"effect": "zoom", "class": "entrance"})).unwrap();
        assert_eq!(
            effects(&mut s).iter().map(|e| (e.1, e.2.as_str())).collect::<Vec<_>>(),
            vec![(Some(0), "fade"), (Some(1), "zoom"), (Some(2), "fade")]
        );
    }

    #[test]
    fn a_caret_in_a_bullet_without_an_effect_keeps_the_others() {
        let (mut s, tb, _) = bullets();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [tb], "paragraphs": [0]})).unwrap();
        s.execute("text.select", &json!({"anchor": [2, 1], "caret": [2, 1]})).unwrap();
        s.execute("animation.set", &json!({"effect": "zoom", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s).iter().map(|e| (e.1, e.2.as_str())).collect::<Vec<_>>(), vec![(Some(0), "fade"), (Some(2), "zoom")]);
    }

    #[test]
    fn set_with_an_index_replaces_that_effect_only() {
        let (mut s, _) = bullets_with_effects();
        s.execute("animation.timing", &json!({"start": "withPrevious", "delay": 300})).unwrap();
        s.execute("animation.set", &json!({"index": 1, "effect": "zoom", "class": "entrance"})).unwrap();
        let e = effects(&mut s);
        assert_eq!(
            e.iter().map(|e| (e.1, e.2.as_str(), e.3.as_str())).collect::<Vec<_>>(),
            vec![(Some(0), "fade", "onClick"), (Some(1), "zoom", "withPrevious"), (Some(2), "fade", "onClick")]
        );
        assert_eq!(times(&s)[1].2, 300, "the replaced effect keeps its delay");
        assert!(s.execute("animation.set", &json!({"index": 9, "effect": "zoom", "class": "entrance"})).is_err());
    }

    #[test]
    fn timing_on_a_bullet_without_an_effect_leaves_the_others() {
        let (mut s, tb, _) = bullets();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [tb], "paragraphs": [0], "duration": 500})).unwrap();
        s.execute("text.select", &json!({"anchor": [2, 0], "caret": [2, 5]})).unwrap();
        assert!(s.execute("animation.timing", &json!({"duration": 2000})).is_err());
        assert_eq!(times(&s), vec![(Some(0), 500, 0)]);
    }

    #[test]
    fn text_build_is_for_the_whole_box_even_with_a_highlight() {
        let (mut s, tb, _) = bullets();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance", "ids": [tb]})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.options", &json!({"textBuild": "byParagraph"})).unwrap();
        let sl = s.doc().unwrap().current_slide().unwrap();
        assert_eq!((sl.animations.len(), sl.animations[0].text_build), (1, TextBuild::ByParagraph));
        // Which is the way out of the gallery's refusal.
        s.execute("animation.set", &json!({"effect": "fly", "class": "entrance"})).unwrap();
        assert_eq!(
            effects(&mut s).iter().map(|e| (e.1, e.2.as_str())).collect::<Vec<_>>(),
            vec![(Some(0), "fade"), (Some(1), "fly"), (Some(2), "fade")]
        );
    }

    #[test]
    fn gallery_on_a_bullet_of_an_as_one_box_splits_it() {
        // Otherwise a box set As One could never get a per-bullet effect.
        let (mut s, tb, _) = bullets();
        s.execute("animation.set", &json!({"effect": "fade", "class": "entrance", "ids": [tb]})).unwrap();
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.set", &json!({"effect": "fly", "class": "entrance"})).unwrap();
        assert_eq!(
            effects(&mut s).iter().map(|e| (e.1, e.2.as_str(), e.3.as_str())).collect::<Vec<_>>(),
            vec![(Some(0), "fade", "onClick"), (Some(1), "fly", "withPrevious"), (Some(2), "fade", "withPrevious")]
        );
    }

    #[test]
    fn gallery_on_a_bullet_of_a_by_word_box_is_refused() {
        // Splitting these would change how they play; a bullet's effect beside them animates it twice.
        for build in ["byWord", "byLetter"] {
            let (mut s, tb, _) = bullets();
            s.execute("animation.set", &json!({"effect": "fade", "class": "entrance", "ids": [tb]})).unwrap();
            s.execute("animation.options", &json!({"ids": [tb], "textBuild": build})).unwrap();
            s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
            let before = effects(&mut s);
            assert!(s.execute("animation.set", &json!({"effect": "fly", "class": "entrance"})).is_err(), "{build}");
            assert_eq!(effects(&mut s), before, "{build}: a refused gallery choice changes nothing");
            s.execute("animation.add", &json!({"effect": "pulse", "class": "emphasis"})).unwrap();
            assert_eq!(effects(&mut s).len(), 2, "{build}: Add Animation still layers");
        }
    }

    #[test]
    fn highlighted_bullets_play_as_a_build_of_them_does() {
        // Choosing every bullet must play as the box's own By Paragraph build plays, whatever the
        // start: a build that isn't On Click staggers its bullets, it doesn't wait for each to end.
        for start in ["afterPrevious", "withPrevious", "onClick"] {
            let box_with_sub_bullet = || {
                let mut s = Session::with_new();
                let r = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 300], "text": "One\nOne a\nTwo\nThree"})).unwrap();
                let tb = r.get("id").unwrap().as_u64().unwrap();
                s.execute("text.edit", &json!({"id": tb, "at": [1, 0]})).unwrap();
                s.execute("format.indent", &json!({})).unwrap();
                s.execute("text.exit", &json!({})).unwrap();
                s.execute("edit.select", &json!({"ids": [tb]})).unwrap();
                (s, tb)
            };
            let timings = |s: &mut Session| {
                let st = s.doc().unwrap();
                let size = st.doc.slide_size;
                let tl = deckcraft_anim::Timeline::new(st.current_slide().unwrap(), size.width, size.height);
                let mut t = tl.effects().into_iter().map(|e| (e.paragraph, e.step, e.start)).collect::<Vec<_>>();
                t.sort_by_key(|x| x.0);
                t
            };
            let (mut build, _) = box_with_sub_bullet();
            build.execute("animation.set", &json!({"effect": "fade", "class": "entrance"})).unwrap();
            build.execute("animation.options", &json!({"textBuild": "byParagraph"})).unwrap();
            build.execute("animation.timing", &json!({"start": start, "duration": 2000, "delay": 300})).unwrap();
            let want = timings(&mut build);

            let (mut chosen, tb) = box_with_sub_bullet();
            let p = json!({"effect": "fade", "class": "entrance", "ids": [tb], "paragraphs": [0, 1, 2, 3], "start": start, "duration": 2000, "delay": 300});
            chosen.execute("animation.add", &p).unwrap();
            let got = timings(&mut chosen);
            assert_eq!(got.iter().filter(|x| x.0 != Some(1)).copied().collect::<Vec<_>>(), want, "{start}");
            assert_eq!((got[1].1, got[1].2), (got[0].1, got[0].2), "{start}: the sub-bullet starts with its bullet");
        }
    }

    #[test]
    fn splitting_a_build_keeps_its_timing() {
        // Replacing one bullet of a By Paragraph build must not move the others: a build that
        // isn't On Click starts its bullets PARAGRAPH_STAGGER apart, not each after the last ends.
        for start in ["afterPrevious", "withPrevious", "onClick"] {
            let mut s = Session::with_new();
            let r = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 300], "text": "One\nOne a\nTwo\nThree"})).unwrap();
            let tb = r.get("id").unwrap().as_u64().unwrap();
            s.execute("text.edit", &json!({"id": tb, "at": [1, 0]})).unwrap();
            s.execute("format.indent", &json!({})).unwrap();
            s.execute("text.exit", &json!({})).unwrap();
            s.execute("edit.select", &json!({"ids": [tb]})).unwrap();
            s.execute("animation.set", &json!({"effect": "fade", "class": "entrance"})).unwrap();
            s.execute("animation.options", &json!({"textBuild": "byParagraph"})).unwrap();
            s.execute("animation.timing", &json!({"start": start, "duration": 2000, "delay": 300})).unwrap();
            let timings = |s: &mut Session| {
                let st = s.doc().unwrap();
                let size = st.doc.slide_size;
                let tl = deckcraft_anim::Timeline::new(st.current_slide().unwrap(), size.width, size.height);
                let mut t = tl.effects().into_iter().map(|e| (e.paragraph, e.step, e.start)).collect::<Vec<_>>();
                t.sort_by_key(|x| x.0);
                t
            };
            // The sub-bullet plays as part of "One" here, not as its own effect.
            let before = timings(&mut s);
            assert_eq!(before.iter().map(|x| x.0).collect::<Vec<_>>(), vec![Some(0), Some(2), Some(3)], "{start}");
            s.execute("text.edit", &json!({"id": tb})).unwrap();
            s.execute("text.select", &json!({"anchor": [2, 0], "caret": [2, 3]})).unwrap();
            s.execute("animation.set", &json!({"effect": "fly", "class": "entrance"})).unwrap();
            assert_eq!(effects(&mut s).len(), 4, "{start}: split into one effect per paragraph");
            let after = timings(&mut s);
            let bullets_after = after.iter().filter(|x| x.0 != Some(1)).copied().collect::<Vec<_>>();
            assert_eq!(bullets_after, before, "{start}: every bullet, the replaced one too, starts when it did");
            assert_eq!(after[1].0, Some(1));
            assert_eq!((after[1].1, after[1].2), (after[0].1, after[0].2), "{start}: the sub-bullet starts with its bullet");
        }
    }

    #[test]
    fn highlight_past_the_end_of_a_bullet_is_refused() {
        let (mut s, tb, _) = bullets();
        // "Two" has three characters: [1, 4]..[1, 5] holds no text.
        s.execute("text.select", &json!({"anchor": [1, 4], "caret": [1, 5]})).unwrap();
        assert!(s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).is_err());
        s.execute("text.select", &json!({"anchor": [1, 5], "caret": [2, 1]})).unwrap();
        assert!(s.execute("animation.set", &json!({"effect": "fade", "class": "entrance"})).is_err());
        assert!(effects(&mut s).is_empty());
        // Both ends of the text are still fine.
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 3]})).unwrap();
        s.execute("animation.add", &json!({"effect": "fade", "class": "entrance"})).unwrap();
        assert_eq!(effects(&mut s), vec![(tb, Some(1), "fade".into(), "onClick".into())]);
    }
}
