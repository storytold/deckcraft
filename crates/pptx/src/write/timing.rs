//! Transitions and the animation timing tree.

use deckcraft_model::anim::{AnimClass, AnimStart, Animation, TextBuild, Transition};
use deckcraft_model::{Shape, ShapeKind, find_shape};

use super::shapes::IdMap;
use super::{Exp, Out};
use crate::opc::{NS_MC, NS_P14, NS_P15, NS_P159};
use crate::xml::{A, W};

pub fn transition(w: &mut W, x: &mut Exp, o: &mut Out, t: &Transition) {
    let adv_click = (!t.advance_on_click).then_some("0");
    let adv_tm = t.advance_after_ms.map(|v| v.min(86_400_000));
    let spd = match t.duration_ms {
        0..=600 => "fast",
        601..=900 => "med",
        _ => "slow",
    };
    let sound = t.sound.and_then(|m| x.media_rel(o, m, "audio"));
    let snd = |w: &mut W| {
        if let Some(r) = &sound {
            w.open0("p:sndAc");
            w.open0("p:stSnd");
            w.empty("p:snd", A::new().a("r:embed", r).a("name", "sound.wav"));
            w.close("p:stSnd");
            w.close("p:sndAc");
        }
    };
    let mut effect = W::frag();
    let ns = if t.kind == "none" { Some("p") } else { super::super::maps::transition_to_xml(&mut effect, &t.kind, &t.option) };
    let ns = match ns {
        Some(ns) => ns,
        None => {
            // Unknown kind: write the kept element if there is one, else a fade.
            if let Some(raw) = &t.raw
                && let Ok(d) = crate::xml::parse(raw.as_bytes())
                && let Some(e) = d.root.elements().find(|e| !matches!(e.local(), "sndAc" | "extLst"))
                && !crate::read::has_rel_refs(e)
            {
                let prefix = e.prefix().to_string();
                let mut ns_decl = d.ns_decls();
                ns_decl.retain(|(k, _)| k == &format!("xmlns:{prefix}"));
                let mut e2 = e.clone();
                for (k, v) in ns_decl {
                    e2.attrs.push((k, v));
                }
                effect.raw(&e2.to_xml());
                match prefix.as_str() {
                    "p14" => "p14",
                    "p15" => "p15",
                    "p159" => "p159",
                    "p" => "p",
                    _ => {
                        effect = W::frag();
                        effect.empty0("p:fade");
                        "p"
                    }
                }
            } else {
                effect.empty0("p:fade");
                "p"
            }
        }
    };
    let is_none = t.kind == "none";
    let dur = t.duration_ms.min(60_000);
    w.open("mc:AlternateContent", A::new().a("xmlns:mc", NS_MC));
    let mut ch = A::new().a("xmlns:p14", NS_P14);
    let req = match ns {
        "p15" => {
            ch = ch.a("xmlns:p15", NS_P15);
            "p15"
        }
        "p159" => {
            ch = ch.a("xmlns:p159", NS_P159);
            "p159"
        }
        _ => "p14",
    };
    w.open("mc:Choice", ch.a("Requires", req));
    w.open("p:transition", A::new().a("spd", spd).o("p14:dur", (!is_none).then_some(dur)).o("advClick", adv_click).o("advTm", adv_tm));
    if !is_none {
        w.raw(&effect.s);
    }
    snd(w);
    w.close("p:transition");
    w.close("mc:Choice");
    w.open0("mc:Fallback");
    w.open("p:transition", A::new().a("spd", spd).o("advClick", adv_click).o("advTm", adv_tm));
    if !is_none {
        if ns == "p" {
            w.raw(&effect.s);
        } else {
            w.empty0("p:fade");
        }
    }
    snd(w);
    w.close("p:transition");
    w.close("mc:Fallback");
    w.close("mc:AlternateContent");
}

/// One effect to write (an animation, or one paragraph of a by-paragraph build).
struct Eff<'a> {
    a: &'a Animation,
    spid: u32,
    start: AnimStart,
    paragraph: Option<u32>,
    grp: u32,
}

struct Ids(u32);

impl Ids {
    fn next(&mut self) -> u32 {
        self.0 = self.0.saturating_add(1);
        self.0
    }
}

fn tgt(w: &mut W, spid: u32, para: Option<u32>) {
    w.open0("p:tgtEl");
    match para {
        Some(p) => {
            w.open("p:spTgt", A::new().a("spid", spid));
            w.open0("p:txEl");
            w.empty("p:pRg", A::new().a("st", p).a("end", p));
            w.close("p:txEl");
            w.close("p:spTgt");
        }
        None => w.empty("p:spTgt", A::new().a("spid", spid)),
    }
    w.close("p:tgtEl");
}

fn cbhvr(w: &mut W, ids: &mut Ids, e: &Eff, dur: u32, delay: Option<u32>, attrs: &[&str], additive: Option<&str>) {
    w.open("p:cBhvr", A::new().o("additive", additive));
    let a = A::new().a("id", ids.next()).a("dur", dur.max(1)).a("fill", "hold");
    match delay {
        Some(d) => {
            w.open("p:cTn", a);
            w.open0("p:stCondLst");
            w.empty("p:cond", A::new().a("delay", d));
            w.close("p:stCondLst");
            w.close("p:cTn");
        }
        None => w.empty("p:cTn", a),
    }
    tgt(w, e.spid, e.paragraph);
    if !attrs.is_empty() {
        w.open0("p:attrNameLst");
        for n in attrs {
            w.elt("p:attrName", n);
        }
        w.close("p:attrNameLst");
    }
    w.close("p:cBhvr");
}

fn set_vis(w: &mut W, ids: &mut Ids, e: &Eff, delay: u32, visible: bool) {
    w.open0("p:set");
    cbhvr(w, ids, e, 1, Some(delay), &["style.visibility"], None);
    w.open0("p:to");
    w.val("p:strVal", if visible { "visible" } else { "hidden" });
    w.close("p:to");
    w.close("p:set");
}

fn anim_effect(w: &mut W, ids: &mut Ids, e: &Eff, dur: u32, filter: &str, out: bool) {
    w.open("p:animEffect", A::new().a("transition", if out { "out" } else { "in" }).a("filter", filter));
    cbhvr(w, ids, e, dur, None, &[], None);
    w.close("p:animEffect");
}

fn anim_prop(w: &mut W, ids: &mut Ids, e: &Eff, dur: u32, attr: &str, from: &str, to: &str) {
    w.open("p:anim", A::new().a("calcmode", "lin").a("valueType", "num"));
    cbhvr(w, ids, e, dur, None, &[attr], Some("base"));
    w.open0("p:tavLst");
    for (tm, v) in [(0, from), (100_000, to)] {
        w.open("p:tav", A::new().a("tm", tm));
        w.open0("p:val");
        w.val("p:strVal", v);
        w.close("p:val");
        w.close("p:tav");
    }
    w.close("p:tavLst");
    w.close("p:anim");
}

fn filter_for(effect: &str, option: &str) -> &'static str {
    match effect {
        "wipe" | "wipeOut" => match option {
            "t" => "wipe(down)",
            "l" => "wipe(right)",
            "r" => "wipe(left)",
            _ => "wipe(up)",
        },
        "split" | "splitOut" => match option {
            "horzOut" => "barn(outHorizontal)",
            "vertIn" => "barn(inVertical)",
            "vertOut" => "barn(outVertical)",
            _ => "barn(inHorizontal)",
        },
        "randomBars" | "randomBarsOut" => {
            if option == "vert" {
                "randombar(vertical)"
            } else {
                "randombar(horizontal)"
            }
        }
        "blinds" => {
            if option == "vert" {
                "blinds(vertical)"
            } else {
                "blinds(horizontal)"
            }
        }
        "box" => {
            if option == "out" {
                "box(out)"
            } else {
                "box(in)"
            }
        }
        "circle" => {
            if option == "out" {
                "circle(out)"
            } else {
                "circle(in)"
            }
        }
        "diamond" => {
            if option == "out" {
                "diamond(out)"
            } else {
                "diamond(in)"
            }
        }
        "shape" | "shapeOut" => match option {
            "circleOut" => "circle(out)",
            "boxIn" => "box(in)",
            "boxOut" => "box(out)",
            "diamondIn" => "diamond(in)",
            "diamondOut" => "diamond(out)",
            "plusIn" => "plus(in)",
            "plusOut" => "plus(out)",
            _ => "circle(in)",
        },
        "checkerboard" => {
            if option == "down" {
                "checkerboard(down)"
            } else {
                "checkerboard(across)"
            }
        }
        "dissolve" | "dissolveOut" => "dissolve",
        "wheel" | "wheelOut" => match option {
            "2" => "wheel(2)",
            "3" => "wheel(3)",
            "4" => "wheel(4)",
            "8" => "wheel(8)",
            _ => "wheel(1)",
        },
        "strips" => match option {
            "ru" => "strips(upRight)",
            "ld" => "strips(downLeft)",
            "rd" => "strips(downRight)",
            _ => "strips(upLeft)",
        },
        "peek" => match option {
            "t" => "slide(fromTop)",
            "l" => "slide(fromLeft)",
            "r" => "slide(fromRight)",
            _ => "slide(fromBottom)",
        },
        _ => "fade",
    }
}

fn fly_from(option: &str) -> (&'static str, &'static str) {
    let x = if option.contains('l') {
        "0-#ppt_w/2"
    } else if option.contains('r') {
        "1+#ppt_w/2"
    } else {
        "#ppt_x"
    };
    let y = if option.contains('t') {
        "0-#ppt_h/2"
    } else if option.contains('b') || option.is_empty() {
        "1+#ppt_h/2"
    } else {
        "#ppt_y"
    };
    (x, y)
}

fn behaviors(w: &mut W, x: &mut Exp, ids: &mut Ids, e: &Eff) {
    let a = e.a;
    let dur = a.duration_ms.clamp(1, 3_600_000);
    match a.class {
        AnimClass::Entrance => {
            set_vis(w, ids, e, 0, true);
            match a.effect.as_str() {
                "appear" => {}
                "fly" => {
                    let (fx, fy) = fly_from(&a.option);
                    anim_prop(w, ids, e, dur, "ppt_x", fx, "#ppt_x");
                    anim_prop(w, ids, e, dur, "ppt_y", fy, "#ppt_y");
                }
                "zoom" | "grow" | "expand" | "spinner" => {
                    anim_prop(w, ids, e, dur, "ppt_w", "0", "#ppt_w");
                    anim_prop(w, ids, e, dur, "ppt_h", "0", "#ppt_h");
                    anim_effect(w, ids, e, dur, "fade", false);
                }
                "float" | "rise" => {
                    anim_prop(w, ids, e, dur, "ppt_y", if a.option == "d" { "#ppt_y-.1" } else { "#ppt_y+.1" }, "#ppt_y");
                    anim_effect(w, ids, e, dur, "fade", false);
                }
                other => anim_effect(w, ids, e, dur, filter_for(other, &a.option), false),
            }
        }
        AnimClass::Exit => {
            match a.effect.as_str() {
                "disappear" => {}
                "flyOut" => {
                    let (fx, fy) = fly_from(&a.option);
                    anim_prop(w, ids, e, dur, "ppt_x", "#ppt_x", fx);
                    anim_prop(w, ids, e, dur, "ppt_y", "#ppt_y", fy);
                }
                "zoomOut" | "shrinkTurn" => {
                    anim_prop(w, ids, e, dur, "ppt_w", "#ppt_w", "0");
                    anim_prop(w, ids, e, dur, "ppt_h", "#ppt_h", "0");
                    anim_effect(w, ids, e, dur, "fade", true);
                }
                other => anim_effect(w, ids, e, dur, filter_for(other, &a.option), true),
            }
            set_vis(w, ids, e, if a.effect == "disappear" { 0 } else { dur.saturating_sub(1) }, false);
        }
        AnimClass::Emphasis => match a.effect.as_str() {
            "spin" | "teeter" => {
                let deg = a.amount.unwrap_or(if a.effect == "teeter" { 4.0 } else { 360.0 });
                let deg = if a.option == "counterClockwise" { -deg.abs() } else { deg };
                let by = (deg.clamp(-36_000.0, 36_000.0) * 60_000.0).round() as i64;
                w.open("p:animRot", A::new().a("by", by));
                cbhvr(w, ids, e, dur, None, &["r"], None);
                w.close("p:animRot");
            }
            "transparency" => {
                w.open0("p:set");
                cbhvr(w, ids, e, dur, None, &["style.opacity"], None);
                w.open0("p:to");
                w.val("p:strVal", format!("{:.2}", (1.0 - a.amount.unwrap_or(0.5)).clamp(0.0, 1.0)));
                w.close("p:to");
                w.close("p:set");
            }
            "fillColor" | "objectColor" | "fontColor" | "lineColor" | "complementaryColor" | "darken" | "lighten" => {
                let attr = match a.effect.as_str() {
                    "fontColor" => "style.color",
                    "lineColor" => "stroke.color",
                    _ => "fillcolor",
                };
                let c = a.color.clone().unwrap_or(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent2));
                w.open("p:animClr", A::new().a("clrSpc", "rgb").a("dir", "cw"));
                cbhvr(w, ids, e, dur, None, &[attr], None);
                w.open0("p:to");
                super::dml::color(w, &c);
                w.close("p:to");
                w.close("p:animClr");
            }
            _ => {
                let s = a.amount.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(if a.effect == "growShrink" { 1.5 } else { 1.1 });
                let (sx, sy) = match a.option.as_str() {
                    "horz" => (s, 1.0),
                    "vert" => (1.0, s),
                    _ => (s, s),
                };
                w.open0("p:animScale");
                cbhvr(w, ids, e, dur, None, &[], None);
                w.empty(
                    "p:by",
                    A::new().a("x", (sx.clamp(0.01, 100.0) * 100_000.0).round() as i64).a("y", (sy.clamp(0.01, 100.0) * 100_000.0).round() as i64),
                );
                w.close("p:animScale");
            }
        },
        AnimClass::Path => {
            let path = a
                .path
                .clone()
                .filter(|p| !p.trim().is_empty() && p.chars().all(|c| c.is_ascii() && c != '"' && c != '<' && c != '&'))
                .unwrap_or_else(|| {
                    match a.option.as_str() {
                        "up" => "M 0 0 L 0 -0.25 E",
                        "right" => "M 0 0 L 0.25 0 E",
                        "left" => "M 0 0 L -0.25 0 E",
                        _ => "M 0 0 L 0 0.25 E",
                    }
                    .to_string()
                });
            w.open("p:animMotion", A::new().a("origin", "layout").a("path", path).a("pathEditMode", "relative").a("ptsTypes", ""));
            cbhvr(w, ids, e, dur, None, &["ppt_x", "ppt_y"], None);
            w.close("p:animMotion");
        }
        AnimClass::Media => {
            let cmd = match a.effect.as_str() {
                "pause" => "togglePause",
                "stop" => "stop",
                _ => "playFrom(0.0)",
            };
            w.open("p:cmd", A::new().a("type", "call").a("cmd", cmd));
            cbhvr(w, ids, e, 1, None, &[], None);
            w.close("p:cmd");
        }
    }
    let _ = x;
}

fn effect_par(w: &mut W, x: &mut Exp, ids: &mut Ids, e: &Eff, first_in_click: bool) {
    let a = e.a;
    let node = match e.start {
        AnimStart::OnClick => "clickEffect",
        AnimStart::WithPrevious => "withEffect",
        AnimStart::AfterPrevious => "afterEffect",
    };
    let _ = first_in_click;
    let preset = crate::maps::preset_for(a);
    let sub = crate::maps::subtype_from_option(&a.effect, &a.option).or(a.preset_subtype.filter(|_| a.preset_id == Some(preset))).unwrap_or(0);
    let repeat = match a.repeat {
        u32::MAX => Some("indefinite".to_string()),
        0 | 1 => None,
        n => Some((n.min(10_000) * 1000).to_string()),
    };
    let accel = (a.smooth_start > 0.0).then(|| (a.smooth_start.clamp(0.0, 1.0) * 100_000.0).round() as i64);
    let decel = (a.smooth_end > 0.0).then(|| (a.smooth_end.clamp(0.0, 1.0) * 100_000.0).round() as i64);
    w.open0("p:par");
    w.open(
        "p:cTn",
        A::new()
            .a("id", ids.next())
            .a("presetID", preset)
            .a("presetClass", a.class.xml())
            .a("presetSubtype", sub)
            .o("repeatCount", repeat)
            .o("accel", accel)
            .o("decel", decel)
            .t("autoRev", a.auto_reverse)
            .a("fill", if a.rewind { "remove" } else { "hold" })
            .a("grpId", e.grp)
            .a("nodeType", node),
    );
    w.open0("p:stCondLst");
    w.empty("p:cond", A::new().a("delay", a.delay_ms.min(3_600_000)));
    w.close("p:stCondLst");
    w.open0("p:childTnLst");
    behaviors(w, x, ids, e);
    w.close("p:childTnLst");
    w.close("p:cTn");
    w.close("p:par");
}

fn end_time(e: &Eff) -> u32 {
    let rep = match e.a.repeat {
        0 | 1 => 1,
        u32::MAX => 1,
        n => n.min(100),
    };
    e.a.delay_ms.saturating_add(e.a.duration_ms.saturating_mul(rep)).min(3_600_000)
}

/// click groups → after-groups → effects
type Groups<'a> = Vec<(bool, Vec<Vec<Eff<'a>>>)>;

fn group<'a>(effects: Vec<Eff<'a>>) -> Groups<'a> {
    let mut clicks: Groups = vec![];
    for e in effects {
        match e.start {
            AnimStart::OnClick => clicks.push((false, vec![vec![e]])),
            AnimStart::WithPrevious => match clicks.last_mut().and_then(|c| c.1.last_mut()) {
                Some(g) => g.push(e),
                None => clicks.push((true, vec![vec![e]])),
            },
            AnimStart::AfterPrevious => match clicks.last_mut() {
                Some(c) => c.1.push(vec![e]),
                None => clicks.push((true, vec![vec![e]])),
            },
        }
    }
    clicks
}

fn click_pars(w: &mut W, x: &mut Exp, ids: &mut Ids, clicks: &Groups, interactive: bool) {
    for (ci, (auto, afters)) in clicks.iter().enumerate() {
        w.open0("p:par");
        w.open("p:cTn", A::new().a("id", ids.next()).a("fill", "hold"));
        w.open0("p:stCondLst");
        if interactive && ci == 0 {
            w.empty("p:cond", A::new().a("delay", 0));
        } else {
            w.empty("p:cond", A::new().a("delay", "indefinite"));
            if *auto {
                w.open("p:cond", A::new().a("evt", "onBegin").a("delay", 0));
                w.val("p:tn", 2);
                w.close("p:cond");
            }
        }
        w.close("p:stCondLst");
        w.open0("p:childTnLst");
        let mut t = 0u32;
        for g in afters {
            w.open0("p:par");
            w.open("p:cTn", A::new().a("id", ids.next()).a("fill", "hold"));
            w.open0("p:stCondLst");
            w.empty("p:cond", A::new().a("delay", t));
            w.close("p:stCondLst");
            w.open0("p:childTnLst");
            for (i, e) in g.iter().enumerate() {
                effect_par(w, x, ids, e, i == 0);
            }
            w.close("p:childTnLst");
            w.close("p:cTn");
            w.close("p:par");
            t = t.saturating_add(g.iter().map(end_time).max().unwrap_or(0));
        }
        w.close("p:childTnLst");
        w.close("p:cTn");
        w.close("p:par");
    }
}

fn para_count(s: &Shape) -> u32 {
    s.text.as_ref().map(|t| t.paragraphs.iter().filter(|p| !p.is_empty()).count()).unwrap_or(0).min(10_000) as u32
}

/// Write `p:timing` for a slide's animations.
pub fn timing(w: &mut W, x: &mut Exp, anims: &[Animation], shapes: &[Shape], idmap: &IdMap) {
    let mut grp_count: std::collections::HashMap<u32, u32> = Default::default();
    let mut main: Vec<Eff> = vec![];
    let mut triggered: Vec<(u32, Vec<Eff>)> = vec![];
    let mut bld: Vec<(u32, u32, bool)> = vec![];
    for a in anims.iter().take(10_000) {
        let Some(spid) = idmap.get(a.shape) else {
            log::warn!("pptx: animation on missing shape {} skipped", a.shape);
            continue;
        };
        let Some(shape) = find_shape(shapes, a.shape) else { continue };
        let grp = {
            let c = grp_count.entry(spid).or_insert(0);
            let g = *c;
            *c += 1;
            g
        };
        let is_text_sp = matches!(shape.kind, ShapeKind::Shape) && shape.text.as_ref().is_some_and(|t| !t.is_empty());
        let by_para = a.text_build == TextBuild::ByParagraph && is_text_sp && para_count(shape) > 0;
        let mut effs = vec![];
        if by_para {
            // Paragraph indices in the file count empty paragraphs too.
            let idxs: Vec<u32> = shape
                .text
                .as_ref()
                .map(|t| t.paragraphs.iter().enumerate().filter(|(_, p)| !p.is_empty()).map(|(i, _)| i as u32).collect())
                .unwrap_or_default();
            for (k, pi) in idxs.into_iter().enumerate() {
                let start = if k == 0 {
                    a.start
                } else if a.start == AnimStart::OnClick {
                    AnimStart::OnClick
                } else {
                    AnimStart::AfterPrevious
                };
                effs.push(Eff { a, spid, start, paragraph: Some(pi), grp });
            }
        } else {
            effs.push(Eff { a, spid, start: a.start, paragraph: a.paragraph, grp });
        }
        if matches!(shape.kind, ShapeKind::Shape) && shape.text.is_some() {
            bld.push((spid, grp, by_para));
        }
        match a.trigger.and_then(|t| idmap.get(t)) {
            Some(t) => match triggered.iter_mut().find(|(s, _)| *s == t) {
                Some((_, v)) => v.extend(effs),
                None => triggered.push((t, effs)),
            },
            None => main.extend(effs),
        }
    }
    if main.is_empty() && triggered.is_empty() {
        return;
    }
    let mut ids = Ids(0);
    w.open0("p:timing");
    w.open0("p:tnLst");
    w.open0("p:par");
    w.open("p:cTn", A::new().a("id", ids.next()).a("dur", "indefinite").a("restart", "never").a("nodeType", "tmRoot"));
    w.open0("p:childTnLst");
    if !main.is_empty() {
        let clicks = group(main);
        w.open("p:seq", A::new().a("concurrent", 1).a("nextAc", "seek"));
        w.open("p:cTn", A::new().a("id", ids.next()).a("dur", "indefinite").a("nodeType", "mainSeq"));
        w.open0("p:childTnLst");
        click_pars(w, x, &mut ids, &clicks, false);
        w.close("p:childTnLst");
        w.close("p:cTn");
        w.open0("p:prevCondLst");
        w.open("p:cond", A::new().a("evt", "onPrev").a("delay", 0));
        w.open0("p:tgtEl");
        w.empty0("p:sldTgt");
        w.close("p:tgtEl");
        w.close("p:cond");
        w.close("p:prevCondLst");
        w.open0("p:nextCondLst");
        w.open("p:cond", A::new().a("evt", "onNext").a("delay", 0));
        w.open0("p:tgtEl");
        w.empty0("p:sldTgt");
        w.close("p:tgtEl");
        w.close("p:cond");
        w.close("p:nextCondLst");
        w.close("p:seq");
    }
    for (trig, effs) in triggered {
        // Everything a trigger starts plays from its click.
        let effs: Vec<Eff> = effs
            .into_iter()
            .enumerate()
            .map(|(i, e)| Eff {
                start: if i == 0 {
                    AnimStart::OnClick
                } else if e.start == AnimStart::OnClick {
                    AnimStart::AfterPrevious
                } else {
                    e.start
                },
                ..e
            })
            .collect();
        let clicks = group(effs);
        let trig_cond = |w: &mut W| {
            w.open("p:cond", A::new().a("evt", "onClick").a("delay", 0));
            w.open0("p:tgtEl");
            w.empty("p:spTgt", A::new().a("spid", trig));
            w.close("p:tgtEl");
            w.close("p:cond");
        };
        w.open("p:seq", A::new().a("concurrent", 1).a("nextAc", "seek"));
        w.open(
            "p:cTn",
            A::new()
                .a("id", ids.next())
                .a("restart", "whenNotActive")
                .a("fill", "hold")
                .a("evtFilter", "cancelBubble")
                .a("nodeType", "interactiveSeq"),
        );
        w.open0("p:stCondLst");
        trig_cond(w);
        w.close("p:stCondLst");
        w.open("p:endSync", A::new().a("evt", "end").a("delay", 0));
        w.empty("p:rtn", A::new().a("val", "all"));
        w.close("p:endSync");
        w.open0("p:childTnLst");
        click_pars(w, x, &mut ids, &clicks, true);
        w.close("p:childTnLst");
        w.close("p:cTn");
        w.open0("p:nextCondLst");
        trig_cond(w);
        w.close("p:nextCondLst");
        w.close("p:seq");
    }
    w.close("p:childTnLst");
    w.close("p:cTn");
    w.close("p:par");
    w.close("p:tnLst");
    if !bld.is_empty() {
        w.open0("p:bldLst");
        for (spid, grp, by_para) in bld {
            w.empty("p:bldP", A::new().a("spid", spid).a("grpId", grp).o("build", by_para.then_some("p")).t("animBg", !by_para));
        }
        w.close("p:bldLst");
    }
    w.close("p:timing");
}
