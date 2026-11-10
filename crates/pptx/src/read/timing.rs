//! Slide transitions (`p:transition`) and animations (`p:timing`).

use deckcraft_model::anim::{AnimClass, AnimStart, Animation, BuildRole, TextBuild, Transition, build_filled, paragraph_build_roles};
use deckcraft_model::{Shape, ShapeId, ShapeKind, TextBody, find_shape, find_shape_mut};

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
fn place_by_parent_delays(out: &mut [(Animation, u32, String, bool)], at: &[(Option<u32>, usize, u32)]) {
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
        let mut sorted: Vec<(Animation, u32, String, bool)> = order.iter().map(|&k| out[idx[k]].clone()).collect();
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

/// Does this time node wait for a click? Every condition in its start list must: a list that also
/// names `onBegin` (or a delay) starts on its own, as automatic groups do.
fn starts_on_click(e: &El) -> bool {
    let conds: Vec<&El> = e.child("stCondLst").map(|s| s.elements().filter(|c| c.is("cond")).collect()).unwrap_or_default();
    !conds.is_empty() && conds.iter().all(|c| c.attr("delay").is_some_and(|d| d.trim() == "indefinite") || c.attr("evt") == Some("onClick"))
}

/// A time node that waits a measured delay (`p:cond delay="500"`), so what runs inside it is not
/// part of the slide's entry.
fn starts_later(e: &El) -> bool {
    // Conditions are alternatives: a node that also starts `onBegin` on slide entry is not late
    // (a click condition is `starts_on_click`'s).
    e.child("stCondLst").is_some_and(|s| {
        let conds: Vec<&El> = s.elements().filter(|c| c.is("cond")).collect();
        !conds.is_empty() && conds.iter().all(|c| c.attr("delay").is_some_and(|d| d.trim().parse::<f64>().is_ok_and(|ms| ms > 0.0)))
    })
}

/// Shapes a slide-entry media call starts (`p:cmd type="call" cmd="playFrom(0.0)"`), which is how
/// PowerPoint writes a video set to start automatically: its `p:video` node says
/// `delay="indefinite"` and the call is what plays it. A call inside a click step, or inside a
/// time node that waits a measured delay, is not the slide's entry.
fn entry_media_calls(e: &El, ctx: &IdCtx, out: &mut std::collections::HashSet<ShapeId>, click: bool, depth: usize) {
    if depth > crate::xml::MAX_DEPTH || out.len() >= 10_000 {
        return;
    }
    let click = click || starts_on_click(e) || starts_later(e);
    if e.is("cmd")
        && !click
        && e.attr("cmd").is_some_and(|c| c.trim().starts_with("playFrom"))
        && let Some(id) = e.path(&["cBhvr", "tgtEl", "spTgt"]).and_then(|t| t.u32("spid")).and_then(|f| ctx.lookup(f))
    {
        out.insert(id);
    }
    for c in e.elements() {
        entry_media_calls(c, ctx, out, click, depth + 1);
    }
}

/// The `p:video`/`p:audio` nodes, each with whether an enclosing time node waits for a click.
fn media_nodes<'a>(e: &'a El, click: bool, out: &mut Vec<(&'a El, bool)>, depth: usize) {
    if depth > crate::xml::MAX_DEPTH || out.len() >= 10_000 {
        return;
    }
    let click = click || starts_on_click(e);
    if e.is("video") || e.is("audio") {
        out.push((e, click));
    }
    for c in e.elements() {
        media_nodes(c, click, out, depth + 1);
    }
}

/// Volume, mute, Loop until Stopped, Hide While Not Playing and Play Full Screen live in the timing tree
/// (`p:video`/`p:audio` > `p:cMediaNode`), not on the picture: copy them onto the target's `MediaClip`.
pub fn media_settings(timing: &El, ctx: &IdCtx, shapes: &mut [Shape]) {
    let mut found = vec![];
    media_nodes(timing, false, &mut found, 0);
    let mut started = std::collections::HashSet::new();
    entry_media_calls(timing, ctx, &mut started, false, 0);
    for (node, click) in found {
        let Some(cm) = node.child("cMediaNode") else { continue };
        let Some(id) = cm.path(&["tgtEl", "spTgt"]).and_then(|t| t.u32("spid")).and_then(|f| ctx.lookup(f)) else { continue };
        let Some(shape) = find_shape_mut(shapes, id) else { continue };
        let ShapeKind::Media(clip) = &mut shape.kind else {
            // A recorded Cameo's video is in the picture's timing node, not a media shape. It
            // autoplays only when its own condition starts at slide entry and no enclosing step
            // waits for a click (a click-step keeps `delay="0"` on the inner node).
            if let Some(cameo) = shape.cameo.as_mut().filter(|c| c.recorded.is_some()) {
                let own = cm.path(&["cTn", "stCondLst", "cond"]).is_some_and(|c| c.attr("delay") == Some("0") && c.attr("evt") != Some("onClick"));
                // A slide-entry media call starts it even when its own node says indefinite.
                cameo.autoplay = (own && !click) || started.contains(&id);
                // How much of the clip plays (`p:cTn/@dur`); absent: the whole recording.
                cameo.duration_ms = cm.path(&["cTn"]).and_then(|t| t.u32("dur")).filter(|ms| *ms > 0);
                // An imported recording can carry sound: keep its level (mute is a zero volume).
                cameo.volume = if cm.bool("mute") == Some(true) {
                    Some(0.0)
                } else {
                    // `vol` is in 1/100_000 and its schema default is 50%: a file that omits it
                    // plays at half volume, not silently.
                    let vol = match cm.attr("vol") {
                        Some(v) => v.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map_or(0.5, |v| (v / 100_000.0).clamp(0.0, 1.0)),
                        None => 0.5,
                    };
                    Some(vol)
                };
            }
            continue;
        };
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
    // Paragraph builds (`bldP build="p"`) by (spid, grpId).
    let by_para: Vec<(u32, String)> = timing
        .child("bldLst")
        .map(|b| {
            b.children_named("bldP")
                .filter(|p| p.attr("build") == Some("p"))
                .filter_map(|p| Some((p.u32("spid")?, p.attr("grpId").unwrap_or("0").to_string())))
                .collect()
        })
        .unwrap_or_default();
    let mut out: Vec<(Animation, u32, String, bool)> = vec![];
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
        let build = paragraph.is_some() && by_para.iter().any(|(s, g)| *s == file_id && *g == grp);
        out.push((a, file_id, grp, build));
        at.push((f.trigger, f.group, f.offset));
    }
    place_by_parent_delays(&mut out, &at);
    // A paragraph build's effects, one per paragraph, become one by-paragraph animation only when
    // the timeline plays that back exactly: PowerPoint lets each paragraph keep its own start and
    // effect, and other shapes' effects can sit between them (#844).
    let same_build =
        |x: &(Animation, u32, String, bool), y: &(Animation, u32, String, bool)| x.3 && y.3 && x.1 == y.1 && x.2 == y.2 && x.0.trigger == y.0.trigger;
    let mut anims = Vec::with_capacity(out.len());
    for run in out.chunk_by(same_build) {
        let effects: Vec<&Animation> = run.iter().map(|(a, ..)| a).collect();
        let build = run.first().is_some_and(|r| r.3);
        let text = effects.first().and_then(|a| find_shape(shapes, a.shape)).and_then(|s| s.text.as_ref());
        match (effects.first(), text) {
            (Some(first), Some(text)) if build && plays_as_build(&effects, text) => {
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
fn plays_as_build(effects: &[&Animation], text: &TextBody) -> bool {
    let roles = paragraph_build_roles(text);
    let expected = roles.iter().enumerate().filter(|(i, r)| match r {
        BuildRole::Top => true,
        BuildRole::Member(_) => text.paragraphs.get(*i).is_some_and(build_filled),
        BuildRole::Unowned => false,
    });
    let Some(first) = effects.first() else { return false };
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

#[cfg(test)]
mod tests {
    use deckcraft_model::{Cameo, MediaId, ShapeId};

    use super::*;

    fn ctx() -> IdCtx {
        let mut map = std::collections::HashMap::new();
        map.insert(2, ShapeId(2));
        IdCtx { used: Default::default(), map, fresh: false, count: 0 }
    }

    fn autoplay_of(xml: &str) -> bool {
        let doc = crate::xml::parse(xml.as_bytes()).expect("xml");
        let mut shapes =
            vec![Shape { id: ShapeId(2), cameo: Some(Cameo { recorded: Some(MediaId(1)), ..Default::default() }), ..Default::default() }];
        media_settings(&doc.root, &ctx(), &mut shapes);
        shapes[0].cameo.as_ref().expect("cameo").autoplay
    }

    const ENTRY: &str = r#"<p:timing xmlns:p="urn:x"><p:tnLst><p:par><p:cTn id="1" dur="indefinite"><p:childTnLst><p:video><p:cMediaNode vol="100000"><p:cTn id="2"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn><p:tgtEl><p:spTgt spid="2"/></p:tgtEl></p:cMediaNode></p:video></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"#;

    #[test]
    fn a_cameo_autoplays_only_from_a_slide_entry_condition() {
        assert!(autoplay_of(ENTRY), "delay 0 with no click above it is slide entry");
        let click_event = ENTRY.replace(r#"<p:cond delay="0"/>"#, r#"<p:cond evt="onClick" delay="0"/>"#);
        assert!(!autoplay_of(&click_event), "an on-click condition is not slide entry");
        let click_step = ENTRY
            .replace("<p:video>", r#"<p:par><p:cTn id="9"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst><p:video>"#)
            .replace("</p:video>", "</p:video></p:childTnLst></p:cTn></p:par>");
        assert!(!autoplay_of(&click_step), "an enclosing click step keeps delay 0 on the inner node");
        let waiting = ENTRY.replace(r#"<p:cond delay="0"/>"#, r#"<p:cond delay="indefinite"/>"#);
        assert!(!autoplay_of(&waiting), "an indefinite delay waits for a click");
        // PowerPoint's own automatic video: the node says indefinite and a slide-entry media call
        // plays it (`p:cmd type="call" cmd="playFrom(0.0)"`), so it starts by itself.
        const POWERPOINT_ENTRY: &str = r#"<p:timing xmlns:p="urn:x"><p:tnLst><p:par><p:cTn id="1" dur="indefinite"><p:childTnLst><p:video><p:cMediaNode vol="100000"><p:cTn id="2"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst></p:cTn><p:tgtEl><p:spTgt spid="2"/></p:tgtEl></p:cMediaNode></p:video><p:cmd type="call" cmd="playFrom(0.0)"><p:cBhvr><p:cTn id="3" dur="1" fill="hold"/><p:tgtEl><p:spTgt spid="2"/></p:tgtEl></p:cBhvr></p:cmd></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"#;
        assert!(autoplay_of(POWERPOINT_ENTRY), "a slide-entry media call starts the video");
        // A delayed condition beside an immediate one (onBegin) is an alternative, not a wait.
        let mixed_call = POWERPOINT_ENTRY
            .replace("<p:cmd", r#"<p:par><p:cTn id="9"><p:stCondLst><p:cond delay="500"/><p:cond evt="onBegin" delay="0"><p:tn val="2"/></p:cond></p:stCondLst><p:childTnLst><p:cmd"#)
            .replace("</p:cmd>", "</p:cmd></p:childTnLst></p:cTn></p:par>");
        assert!(autoplay_of(&mixed_call), "an immediate alternative keeps the call at slide entry");
        // A call that waits a measured delay is not the slide entry either.
        let delayed_call = POWERPOINT_ENTRY
            .replace("<p:cmd", r#"<p:par><p:cTn id="9"><p:stCondLst><p:cond delay="500"/></p:stCondLst><p:childTnLst><p:cmd"#)
            .replace("</p:cmd>", "</p:cmd></p:childTnLst></p:cTn></p:par>");
        assert!(!autoplay_of(&delayed_call), "a playFrom call under a 500 ms delay waits its turn");
        // The same call inside a click step does not.
        let click_call = POWERPOINT_ENTRY
            .replace("<p:cmd", r#"<p:par><p:cTn id="9"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst><p:cmd"#)
            .replace("</p:cmd>", "</p:cmd></p:childTnLst></p:cTn></p:par>");
        assert!(!autoplay_of(&click_call), "a media call in a click step waits for the click");
        // A group that also starts `onBegin` runs by itself, so it is not a click step.
        let automatic = ENTRY
            .replace("<p:video>", r#"<p:par><p:cTn id="9"><p:stCondLst><p:cond delay="indefinite"/><p:cond evt="onBegin" delay="0"><p:tn val="2"/></p:cond></p:stCondLst><p:childTnLst><p:video>"#)
            .replace("</p:video>", "</p:video></p:childTnLst></p:cTn></p:par>");
        assert!(autoplay_of(&automatic), "an automatic group is not a click wait");
    }
}
