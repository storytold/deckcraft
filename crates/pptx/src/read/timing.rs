//! Slide transitions (`p:transition`) and animations (`p:timing`).

use deckcraft_model::anim::{AnimClass, AnimStart, Animation, BuildRole, TextBuild, Transition, build_filled, paragraph_build_roles};
use deckcraft_model::{Shape, ShapeKind, TextBody, find_shape, find_shape_mut};

use super::shapes::IdCtx;
use super::{Imp, Part, dml};
use crate::maps;
use crate::xml::El;

fn ms_attr(e: &El, name: &str) -> Option<u32> {
    let v = e.attr(name)?.trim();
    if v == "indefinite" {
        return None;
    }
    v.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 86_400_000.0) as u32)
}

pub fn transition(imp: &mut Imp, part: &Part, t: &El) -> Option<Transition> {
    let effect = t.elements().find(|e| !matches!(e.local(), "sndAc" | "extLst"));
    let mut tr = Transition { advance_on_click: t.bool("advClick").unwrap_or(true), advance_after_ms: ms_attr(t, "advTm"), ..Default::default() };
    match effect.and_then(maps::transition_from_xml) {
        Some((k, o)) => {
            tr.kind = k;
            tr.option = o;
        }
        None => match effect {
            Some(e) => {
                tr.kind = e.local().to_string();
                tr.raw = Some(part.keep(t));
            }
            None => tr.kind = "none".into(),
        },
    }
    let default_dur = deckcraft_model::anim::TRANSITIONS.iter().find(|x| x.0 == tr.kind).map(|x| x.3).unwrap_or(1000);
    // `spd` defaults to "fast" in the schema; PowerPoint omits both attributes for 0.5 s.
    tr.duration_ms = ms_attr(t, "dur").unwrap_or(match t.attr("spd") {
        Some("med") => 750,
        Some("slow") => default_dur.max(1000),
        _ => 500,
    });
    if let Some(snd) = t.path(&["sndAc", "stSnd", "snd"]) {
        tr.sound = dml::media_ref(imp, part, snd);
        tr.sound_loop = t.path(&["sndAc", "stSnd"]).and_then(|s| s.attr("loop")).is_some_and(|v| v == "1" || v == "true");
    }
    tr.stop_sound = t.path(&["sndAc", "endSnd"]).is_some();
    if tr.kind == "none" && tr.advance_on_click && tr.advance_after_ms.is_none() && tr.sound.is_none() && !tr.stop_sound {
        return None;
    }
    Some(tr)
}

struct Found<'a> {
    ctn: &'a El,
    trigger: Option<u32>,
    /// Click group the effect sits in: a new one starts at every time node whose start is `indefinite`.
    group: usize,
    /// Sum of the start delays of the time nodes between the click group and the effect, in ms.
    offset: u32,
}

/// Where `collect` is in the time tree: the click group and the delay accumulated inside it.
#[derive(Clone, Copy)]
struct At {
    trigger: Option<u32>,
    group: usize,
    offset: u32,
}

fn collect<'a>(e: &'a El, at: At, groups: &mut usize, out: &mut Vec<Found<'a>>, depth: usize) {
    if depth > 64 || out.len() > 10_000 {
        return;
    }
    for c in e.elements() {
        if c.is("cTn") && c.has_attr("presetClass") {
            out.push(Found { ctn: c, trigger: at.trigger, group: at.group, offset: at.offset });
            continue;
        }
        let mut at = at;
        if c.is("seq")
            && let Some(ctn) = c.child("cTn")
            && ctn.attr("nodeType") == Some("interactiveSeq")
        {
            at.trigger = ctn.path(&["stCondLst", "cond", "tgtEl", "spTgt"]).and_then(|s| s.u32("spid"));
        }
        if c.is("cTn")
            && let Some(cond) = c.path(&["stCondLst", "cond"])
        {
            if cond.attr("delay").map(str::trim) == Some("indefinite") {
                *groups += 1;
                at.group = *groups;
                at.offset = 0;
            } else {
                at.offset = at.offset.saturating_add(ms_attr(cond, "delay").unwrap_or(0));
            }
        }
        collect(c, at, groups, out, depth + 1);
    }
}

/// How long an effect holds back an After Previous one, as the show timeline counts it.
fn span_ms(a: &Animation) -> u32 {
    let iterations = if a.repeat == u32::MAX || a.repeat <= 1 { 1 } else { a.repeat.min(1_000) };
    a.duration_ms.saturating_mul(if a.auto_reverse { 2 } else { 1 }).saturating_mul(iterations)
}

/// Start times (ms after the click) that the flat effect list plays at, effect after effect.
fn flat_starts(anims: &[&Animation]) -> Vec<u32> {
    let (mut begin, mut end) = (0u32, 0u32);
    anims
        .iter()
        .map(|a| {
            if a.start == AnimStart::AfterPrevious {
                begin = end;
            }
            let start = begin.saturating_add(a.delay_ms);
            end = end.max(start.saturating_add(span_ms(a)));
            start
        })
        .collect()
}

/// PowerPoint places each effect at the delays of all its enclosing time nodes, so a file may list
/// effects out of time order (a countdown written first, then effects at 5 s placed only by their
/// parent's delay). The flat list only knows "after/with previous" plus the effect's own delay, so
/// a click group listed out of time order, whose flat reading disagrees with the file, is sorted by
/// start time and its starts and delays rewritten to land every effect where PowerPoint plays it.
/// Every other group is left untouched.
fn place_by_parent_delays(out: &mut [(Animation, u32, String, bool, usize, u32)], at: &[(Option<u32>, usize, u32)]) {
    let mut groups: Vec<Vec<usize>> = vec![];
    let mut slot: std::collections::HashMap<(Option<u32>, usize), usize> = std::collections::HashMap::new();
    for (i, &(t, g, _)) in at.iter().enumerate() {
        let n = *slot.entry((t, g)).or_insert_with(|| {
            groups.push(vec![]);
            groups.len() - 1
        });
        groups[n].push(i);
    }
    for idx in groups {
        // More than one click in a group means the tree isn't one click per group: leave it.
        if idx.iter().skip(1).any(|&i| out[i].0.start == AnimStart::OnClick) {
            continue;
        }
        let abs: Vec<u32> = idx.iter().map(|&i| at[i].2.saturating_add(out[i].0.delay_ms)).collect();
        // Only a file that lists effects out of time order needs this: files PowerPoint or DeckCraft
        // save are in order, and their reading stays exactly as before.
        if abs.is_sorted() {
            continue;
        }
        let flat = flat_starts(&idx.iter().map(|&i| &out[i].0).collect::<Vec<_>>());
        if flat == abs {
            continue;
        }
        // The group's click stays on whichever effect now plays first.
        let had_click = out[idx[0]].0.start == AnimStart::OnClick;
        let mut order: Vec<usize> = (0..idx.len()).collect();
        order.sort_by_key(|&k| abs[k]);
        let mut sorted: Vec<(Animation, u32, String, bool, usize, u32)> = order.iter().map(|&k| out[idx[k]].clone()).collect();
        let (mut begin, mut end) = (0u32, 0u32);
        for (n, (&k, item)) in order.iter().zip(sorted.iter_mut()).enumerate() {
            let a = &mut item.0;
            let t = abs[k];
            if n == 0 && had_click {
                a.start = AnimStart::OnClick;
                a.delay_ms = t;
            } else if a.start == AnimStart::AfterPrevious && t >= end {
                begin = end;
                a.delay_ms = t - end;
            } else {
                a.start = AnimStart::WithPrevious;
                a.delay_ms = t.saturating_sub(begin);
            }
            item.5 = 0;
            end = end.max(t.saturating_add(span_ms(a)));
        }
        for (&i, item) in idx.iter().zip(sorted) {
            out[i] = item;
        }
    }
}

fn max_dur(e: &El, depth: usize) -> Option<u32> {
    if depth > 64 {
        return None;
    }
    let mut best: Option<u32> = None;
    for c in e.elements() {
        if c.is("cTn")
            && let Some(d) = ms_attr(c, "dur")
            && d > 1
        {
            best = Some(best.map_or(d, |b: u32| b.max(d)));
        }
        if let Some(d) = max_dur(c, depth + 1) {
            best = Some(best.map_or(d, |b| b.max(d)));
        }
    }
    best
}

/// Does every behaviour time node (`p:cBhvr/p:cTn`) under the effect node `c` carry `autoRev`?
fn behaviour_auto_reverses(c: &El) -> bool {
    let mut behaviours = vec![];
    c.find_all("cBhvr", &mut behaviours);
    !behaviours.is_empty() && behaviours.iter().all(|b| b.child("cTn").and_then(|t| t.bool("autoRev")).unwrap_or(false))
}

/// Volume, mute, Loop until Stopped, Hide While Not Playing and Play Full Screen live in the timing tree
/// (`p:video`/`p:audio` > `p:cMediaNode`), not on the picture: copy them onto the target's `MediaClip`.
/// The first node of a clip decides all four options, so an option another application turned off stays off
/// even if DeckCraft's private copy on the picture says on; a further node for the same clip can only switch options on.
pub fn media_settings(timing: &El, ctx: &IdCtx, shapes: &mut [Shape]) {
    let mut nodes = vec![];
    for tag in ["video", "audio"] {
        timing.find_all(tag, &mut nodes);
    }
    let mut seen = std::collections::HashSet::new();
    for node in nodes.into_iter().take(10_000) {
        let Some(cm) = node.child("cMediaNode") else { continue };
        let Some(id) = cm.path(&["tgtEl", "spTgt"]).and_then(|t| t.u32("spid")).and_then(|f| ctx.lookup(f)) else { continue };
        let Some(ShapeKind::Media(clip)) = find_shape_mut(shapes, id).map(|s| &mut s.kind) else { continue };
        let first = seen.insert(id);
        if first {
            clip.volume = 1.0;
            clip.hide_while_not_playing = false;
            clip.loop_play = false;
            clip.full_screen = false;
        }
        if let Some(v) = cm.attr("vol").and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()) {
            clip.volume = (v / 100_000.0).clamp(0.0, 1.0);
        }
        if cm.bool("mute") == Some(true) {
            clip.volume = 0.0;
        }
        if cm.bool("showWhenStopped") == Some(false) {
            clip.hide_while_not_playing = true;
        }
        if cm.child("cTn").and_then(|c| c.attr("repeatCount")) == Some("indefinite") {
            clip.loop_play = true;
        }
        if node.bool("fullScrn") == Some(true) {
            clip.full_screen = true;
        }
    }
}

/// Flatten the timing tree into the Animation Pane list.
pub fn animations(imp: &mut Imp, part: &Part, ctx: &IdCtx, timing: &El, shapes: &[Shape]) -> Vec<Animation> {
    let _ = (imp, part);
    let Some(tn) = timing.child("tnLst") else { return vec![] };
    let mut found = vec![];
    collect(tn, At { trigger: None, group: 0, offset: 0 }, &mut 0, &mut found, 0);
    // Paragraph builds (`bldP`) by (spid, grpId) -> whether `build="p"`.
    let bld_p: std::collections::HashMap<(u32, String), bool> = timing
        .child("bldLst")
        .map(|b| {
            b.children_named("bldP")
                .filter_map(|p| Some(((p.u32("spid")?, p.attr("grpId").unwrap_or("0").to_string()), p.attr("build") == Some("p"))))
                .fold(std::collections::HashMap::new(), |mut m, (k, v)| {
                    m.entry(k).and_modify(|e| *e |= v).or_insert(v);
                    m
                })
        })
        .unwrap_or_default();
    let mut out: Vec<(Animation, u32, String, bool, usize, u32)> = vec![];
    let mut at: Vec<(Option<u32>, usize, u32)> = vec![];
    for f in found {
        let c = f.ctn;
        let class = AnimClass::from_xml(c.attr("presetClass").unwrap_or("entr"));
        let preset = c.u32("presetID").unwrap_or(0);
        let sub = c.u32("presetSubtype").unwrap_or(0);
        let Some(tgt) = c.find("spTgt") else { continue };
        let Some(file_id) = tgt.u32("spid") else { continue };
        let Some(shape) = ctx.lookup(file_id) else { continue };
        let paragraph = tgt.path(&["txEl", "pRg"]).and_then(|p| p.u32("st"));
        let effect = maps::effect_for_preset(class, preset).unwrap_or_else(|| maps::fallback_effect(class));
        // PowerPoint writes Float Down as its own preset (47), not as Float with a direction.
        let (effect, option) = match (class, preset) {
            (AnimClass::Entrance, 47) => ("float", "d".to_string()),
            (AnimClass::Exit, 47) => ("floatOut", "d".to_string()),
            _ => (effect, maps::option_from_subtype(effect, sub)),
        };
        let start = match c.attr("nodeType") {
            Some("clickEffect") => AnimStart::OnClick,
            Some("afterEffect") => AnimStart::AfterPrevious,
            _ => AnimStart::WithPrevious,
        };
        let mut a = Animation {
            shape,
            class,
            effect: effect.to_string(),
            option,
            start,
            duration_ms: max_dur(c, 0).unwrap_or(0),
            delay_ms: c.path(&["stCondLst", "cond"]).and_then(|d| ms_attr(d, "delay")).unwrap_or(0),
            paragraph,
            trigger: f.trigger.and_then(|t| ctx.lookup(t)),
            preset_id: Some(preset),
            preset_subtype: Some(sub),
            // PowerPoint's "Auto-reverse" is `autoRev` on the behaviour nodes (`p:cBhvr/p:cTn`); Color Pulse
            // always writes it there and plays back and forth on its own, so it isn't read as the option.
            auto_reverse: c.bool("autoRev").unwrap_or(false) || (effect != "colorPulse" && behaviour_auto_reverses(c)),
            // PowerPoint's "Rewind when done playing" is `fill="remove"` on the effect node.
            rewind: c.attr("fill") == Some("remove"),
            smooth_start: c.f64("accel").map(|v| v / 100_000.0).unwrap_or(0.0),
            smooth_end: c.f64("decel").map(|v| v / 100_000.0).unwrap_or(0.0),
            bounce_end: c.f64("bounceEnd").map(|v| (v / 100_000.0).clamp(0.0, 1.0)).unwrap_or(0.0),
            ..Default::default()
        };
        a.repeat = match c.attr("repeatCount") {
            Some("indefinite") => u32::MAX,
            Some(v) => v.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| (v / 1000.0).round().clamp(1.0, 10_000.0) as u32).unwrap_or(1),
            None => 1,
        };
        if let Some(m) = c.find("animMotion") {
            a.path = m.attr("path").map(String::from);
        }
        if let Some(r) = c.find("animRot") {
            a.amount = r.f64("by").map(|v| v / 60_000.0);
            if a.effect == "spin" && a.amount.is_some_and(|v| v < 0.0) {
                a.option = "counterClockwise".into();
            }
        } else if let Some(s) = c.find("animScale").and_then(|s| s.child("by")) {
            a.amount = s.f64("x").map(|v| v / 100_000.0);
        }
        if let Some(clr) = c.find("animClr").and_then(|x| x.child("to")) {
            a.color = dml::color(clr);
        }
        let grp = c.attr("grpId").unwrap_or("0").to_string();
        let build = paragraph.is_some() && bld_p.get(&(file_id, grp.clone())) != Some(&false);
        out.push((a, file_id, grp, build, f.group, f.offset));
        at.push((f.trigger, f.group, f.offset));
    }
    place_by_parent_delays(&mut out, &at);
    // A paragraph build's effects, one per paragraph, become one by-paragraph animation only when
    // the timeline plays that back exactly: PowerPoint lets each paragraph keep its own start and
    // effect, and other shapes' effects can sit between them (#844).
    let same_build = |x: &(Animation, u32, String, bool, usize, u32), y: &(Animation, u32, String, bool, usize, u32)| {
        x.3 && y.3 && x.1 == y.1 && x.2 == y.2 && x.0.trigger == y.0.trigger
    };
    let mut anims = Vec::with_capacity(out.len());
    for run in out.chunk_by(same_build) {
        let effects: Vec<&Animation> = run.iter().map(|(a, ..)| a).collect();
        let offsets: Vec<u32> = run.iter().map(|r| r.5).collect();
        let build = run.first().is_some_and(|r| r.3);
        let explicit_bld = run.first().is_some_and(|r| bld_p.get(&(r.1, r.2.clone())) == Some(&true));
        let same_click_group = run.first().is_some_and(|first| first.0.start == AnimStart::OnClick || run.iter().all(|r| r.4 == first.4));
        let text = effects.first().and_then(|a| find_shape(shapes, a.shape)).and_then(|s| s.text.as_ref());
        match (effects.first(), text) {
            (Some(first), Some(text)) if build && (explicit_bld || same_click_group) && plays_as_build(&effects, &offsets, text, explicit_bld) => {
                anims.push(Animation { text_build: TextBuild::ByParagraph, paragraph: None, ..(*first).clone() });
            }
            _ => anims.extend(effects.into_iter().cloned()),
        }
    }
    anims
}

/// Are `effects` (paragraph effects of one shape, in file order) what a by-paragraph build of the
/// first one plays: one effect per paragraph the build animates, in paragraph order, later top
/// paragraphs on click (after previous when the build doesn't start on click), deeper ones with
/// their parent, all with the same settings?
fn plays_as_build(effects: &[&Animation], offsets: &[u32], text: &TextBody, explicit_bld: bool) -> bool {
    let roles = paragraph_build_roles(text);
    let expected = roles.iter().enumerate().filter(|(i, r)| match r {
        BuildRole::Top => true,
        BuildRole::Member(_) => text.paragraphs.get(*i).is_some_and(build_filled),
        BuildRole::Unowned => false,
    });
    let Some(first) = effects.first() else { return false };
    let off0 = offsets.first().copied().unwrap_or(0);
    let flat = flat_starts(effects);
    let parent_starts_match = offsets
        .iter()
        .zip(effects.iter())
        .zip(flat.iter())
        .all(|((&off, a), &ft)| off == off0 || off.saturating_add(a.delay_ms) == off0.saturating_add(ft));
    if !explicit_bld && (!parent_starts_match || (first.start != AnimStart::OnClick && (span_ms(first) != 500 || first.delay_ms != 0 || off0 != 0))) {
        return false;
    }
    let settings = |a: &Animation| Animation { start: AnimStart::OnClick, paragraph: None, ..a.clone() };
    let base = settings(first);
    let mut matched = 0;
    for (k, (i, role)) in expected.enumerate() {
        let Some(a) = effects.get(k) else { return false };
        let start_ok = match role {
            BuildRole::Top if k == 0 => true,
            BuildRole::Top if first.start == AnimStart::OnClick => a.start == AnimStart::OnClick,
            BuildRole::Top => a.start == AnimStart::AfterPrevious,
            _ => a.start == AnimStart::WithPrevious,
        };
        if a.paragraph != u32::try_from(i).ok() || !start_ok || settings(a) != base {
            return false;
        }
        matched = k + 1;
    }
    matched == effects.len()
}
