//! Slide transitions (`p:transition`) and animations (`p:timing`).

use deckcraft_model::anim::{AnimClass, AnimStart, Animation, TextBuild, Transition};
use deckcraft_model::{Shape, ShapeId, ShapeKind};

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
}

fn collect<'a>(e: &'a El, trigger: Option<u32>, out: &mut Vec<Found<'a>>, depth: usize) {
    if depth > 64 || out.len() > 10_000 {
        return;
    }
    for c in e.elements() {
        if c.is("cTn") && c.has_attr("presetClass") {
            out.push(Found { ctn: c, trigger });
            continue;
        }
        let mut trig = trigger;
        if c.is("seq")
            && let Some(ctn) = c.child("cTn")
            && ctn.attr("nodeType") == Some("interactiveSeq")
        {
            trig = ctn.path(&["stCondLst", "cond", "tgtEl", "spTgt"]).and_then(|s| s.u32("spid"));
        }
        collect(c, trig, out, depth + 1);
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

/// Flatten the timing tree into the Animation Pane list.
pub fn animations(imp: &mut Imp, part: &Part, ctx: &IdCtx, timing: &El, _shapes: &[Shape]) -> Vec<Animation> {
    let _ = (imp, part);
    let Some(tn) = timing.child("tnLst") else { return vec![] };
    let mut found = vec![];
    collect(tn, None, &mut found, 0);
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
    let mut out: Vec<(Animation, u32, String)> = vec![];
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
            auto_reverse: c.bool("autoRev").unwrap_or(false),
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
        // Collapse a paragraph-by-paragraph build into one animation.
        if paragraph.is_some() && by_para.iter().any(|(s, g)| *s == file_id && *g == grp) {
            if out.last().is_some_and(|(l, lf, lg)| *lf == file_id && *lg == grp && l.text_build == TextBuild::ByParagraph) {
                continue;
            }
            a.text_build = TextBuild::ByParagraph;
            a.paragraph = None;
        }
        out.push((a, file_id, grp));
    }
    out.into_iter().map(|(a, _, _)| a).collect()
}

/// Media playback options from the timing tree's `p:video`/`p:audio` nodes (#74): volume,
/// play-across-slides, hide-while-not-playing, loop, rewind and the automatic start. The nodes
/// sit beside the main sequence and carry them for clips whose `p14:media` element only holds
/// trim, fades and bookmarks.
pub fn media_options(timing: &El, ctx: &IdCtx, shapes: &mut [Shape]) {
    let mut nodes: Vec<&El> = vec![];
    timing.find_all("video", &mut nodes);
    timing.find_all("audio", &mut nodes);
    for n in nodes {
        let Some(mn) = n.child("cMediaNode") else { continue };
        let Some(id) = mn.path(&["tgtEl", "spTgt"]).and_then(|t| t.u32("spid")).and_then(|s| ctx.lookup(s)) else { continue };
        let Some(clip) = clip_mut(shapes, id) else { continue };
        if let Some(v) = mn.attr("vol").and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()) {
            clip.volume = (v / 100_000.0).clamp(0.0, 1.0);
        }
        if let Some(n) = mn.attr("numSld").and_then(|n| n.parse::<u32>().ok()) {
            clip.play_across_slides = n > 1;
        }
        // `showWhenStopped` says the clip stays on screen when it is not playing; both XML
        // spellings of false mean the same.
        if let Some(b) = mn.attr("showWhenStopped").and_then(xml_bool) {
            clip.hide_while_not_playing = !b;
        }
        if let Some(tn) = mn.child("cTn") {
            clip.loop_play = tn.attr("repeatCount") == Some("indefinite");
            clip.rewind = tn.attr("fill") == Some("remove");
            // The clip starts by itself when its start condition asks for it without a click:
            // a missing condition list delays indefinitely, like a missing delay attribute, and
            // a click condition waits for the click.
            clip.autoplay = matches!(tn.path(&["stCondLst", "cond"]), Some(c) if c.attr("evt") != Some("onClick") && c.attr("delay").is_some_and(|d| d != "indefinite"));
        }
    }
}

/// An XML boolean written as an attribute: "1"/"true"/"on" and "0"/"false"/"off".
fn xml_bool(v: &str) -> Option<bool> {
    match v.trim() {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

/// The media clip of shape `id`, in groups too.
fn clip_mut(shapes: &mut [Shape], id: ShapeId) -> Option<&mut deckcraft_model::MediaClip> {
    for s in shapes {
        if s.id == id {
            return match &mut s.kind {
                ShapeKind::Media(m) => Some(m),
                _ => None,
            };
        }
        if let Some(m) = s.children_mut().and_then(|c| clip_mut(c, id)) {
            return Some(m);
        }
    }
    None
}
