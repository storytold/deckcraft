//! Mappings between our transition / animation ids and PresentationML elements and preset
//! numbers, shared by the reader and the writer.

use deckcraft_model::anim::{ANIMATIONS, AnimClass};

use crate::xml::{A, El, W};

/// (our kind, namespace prefix, element) for transitions that map one-to-one.
const SIMPLE: &[(&str, &str, &str)] =
    &[("dissolve", "p", "dissolve"), ("random", "p", "random"), ("honeycomb", "p14", "honeycomb"), ("flash", "p14", "flash")];

/// Read a transition effect element → (kind, option). `None` for effects we don't model.
pub fn transition_from_xml(e: &El) -> Option<(String, String)> {
    let (k, o) = transition_from_xml_raw(e)?;
    // Kinds without options keep none.
    let has_opts = deckcraft_model::anim::TRANSITIONS.iter().find(|t| t.0 == k).is_none_or(|t| !t.4.is_empty());
    Some((k, if has_opts { o } else { String::new() }))
}

/// Preset transitions (`p15:prstTrans`) whose "Left" option PowerPoint writes with `invX="1"`
/// (the others write it for "Right"), as observed on PowerPoint's own files.
const INVX_LEFT: &[&str] = &["wind", "airplane", "origami"];

fn transition_from_xml_raw(e: &El) -> Option<(String, String)> {
    // Defaults are what PowerPoint leaves out: e.g. it writes Push Left as `<p:push/>`.
    let dir = |d: &str| e.attr("dir").unwrap_or(d).to_string();
    let k = |s: &str| s.to_string();
    Some(match e.local() {
        "fade" => (k("fade"), if e.bool("thruBlk") == Some(true) { k("throughBlack") } else { k("smoothly") }),
        "cut" => (k("cut"), if e.bool("thruBlk") == Some(true) { k("throughBlack") } else { String::new() }),
        "push" => (k("push"), dir("l")),
        "wipe" => (k("wipe"), dir("l")),
        // Diagonal wipes.
        "strips" => (k("wipe"), dir("lu")),
        "split" => {
            let o = e.attr("orient").unwrap_or("horz");
            let d = e.attr("dir").unwrap_or("out");
            (k("split"), format!("{o}{}", if d == "in" { "In" } else { "Out" }))
        }
        "randomBar" => (k("randomBar"), dir("horz")),
        "circle" => (k("shape"), k("circle")),
        "diamond" => (k("shape"), k("diamond")),
        "plus" => (k("shape"), k("plus")),
        // Shape In / Out (the slide zooming in a box).
        "zoom" => (k("shape"), if e.attr("dir") == Some("in") { k("in") } else { k("out") }),
        // The Zoom transition, and its Zoom and Rotate option.
        "warp" => (k("zoom"), if e.attr("dir") == Some("in") { k("in") } else { k("out") }),
        "newsflash" => (k("zoom"), k("rotate")),
        "pull" => (k("uncover"), dir("l")),
        "cover" => (k("cover"), dir("l")),
        "checker" => (k("checker"), dir("horz")),
        "blinds" => (k("blinds"), dir("horz")),
        "comb" => (k("comb"), dir("horz")),
        "wheel" => (k("clock"), k("clockwise")),
        "wheelReverse" => (k("clock"), k("counterClockwise")),
        "wedge" => (k("clock"), k("wedge")),
        "vortex" => (k("vortex"), dir("l")),
        "switch" => (k("switch"), dir("r")),
        "flip" => (k("flip"), dir("r")),
        "ripple" => (k("ripple"), dir("center")),
        "doors" => (k("doors"), dir("horz")),
        "window" => (k("window"), dir("horz")),
        "ferris" => (k("ferris"), dir("l")),
        "gallery" => (k("gallery"), dir("l")),
        "conveyor" => (k("conveyor"), dir("l")),
        "pan" => (k("pan"), dir("l")),
        "glitter" => {
            let d = dir("l");
            if e.attr("pattern") == Some("hexagon") { (k("glitter"), format!("hex{}", d.to_ascii_uppercase())) } else { (k("glitter"), d) }
        }
        "shred" => {
            // `crush` is what DeckCraft wrote before it knew PowerPoint's `rectangle`.
            let particles = matches!(e.attr("pattern"), Some("rectangle" | "crush"));
            let out = e.attr("dir") == Some("out");
            (k("shred"), format!("{}{}", if particles { "particles" } else { "strips" }, if out { "Out" } else { "In" }))
        }
        "reveal" => {
            let black = e.bool("thruBlk") == Some(true);
            let right = e.attr("dir") == Some("r");
            (k("reveal"), format!("{}{}", if black { "black" } else { "smoothly" }, if right { "Right" } else { "Left" }))
        }
        "flythrough" => {
            let out = e.attr("dir") == Some("out");
            let b = e.bool("hasBounce") == Some(true);
            (k("flythrough"), format!("{}{}", if out { "out" } else { "in" }, if b { "Bounce" } else { "" }))
        }
        "prism" => {
            let content = e.bool("isContent") == Some(true);
            let inv = e.bool("isInverted") == Some(true);
            let kind = match (content, inv) {
                (false, false) => "cube",
                (false, true) => "box",
                (true, false) => "rotate",
                (true, true) => "orbit",
            };
            (k(kind), dir("l"))
        }
        "prstTrans" => {
            let prst = e.attr("prst").unwrap_or("fallOver");
            let inv = e.bool("invX") == Some(true);
            if prst == "pageCurlSingle" {
                return Some((k("pageCurlDouble"), if inv { k("singleR") } else { k("singleL") }));
            }
            let known = deckcraft_model::anim::TRANSITIONS.iter().any(|t| t.0 == prst);
            if !known {
                return None;
            }
            let left = if INVX_LEFT.contains(&prst) { inv } else { !inv };
            (k(prst), if left { k("l") } else { k("r") })
        }
        "morph" => (
            k("morph"),
            match e.attr("option") {
                Some("byWord") => k("words"),
                Some("byChar") => k("characters"),
                _ => k("objects"),
            },
        ),
        other => (k(SIMPLE.iter().find(|s| s.2 == other)?.0), String::new()),
    })
}

/// Write the effect element for a transition. Returns the namespace prefix it needs
/// (`p`, `p14`, `p15`, `p159`), or `None` when the kind is unknown.
pub fn transition_to_xml(w: &mut W, kind: &str, option: &str) -> Option<&'static str> {
    let lr = |d: &'static str| match option {
        "l" | "r" => option.to_string(),
        _ => d.to_string(),
    };
    let lrud = |d: &'static str| match option {
        "l" | "r" | "u" | "d" => option.to_string(),
        _ => d.to_string(),
    };
    let hv = |d: &'static str| match option {
        "horz" | "vert" => option.to_string(),
        _ => d.to_string(),
    };
    let eight = |d: &'static str| match option {
        "l" | "r" | "u" | "d" | "lu" | "ru" | "ld" | "rd" => option.to_string(),
        _ => d.to_string(),
    };
    match kind {
        "fade" => w.empty("p:fade", A::new().t("thruBlk", option == "throughBlack")),
        "cut" => w.empty("p:cut", A::new().t("thruBlk", option == "throughBlack")),
        "push" => w.empty("p:push", A::new().a("dir", lrud("u"))),
        "wipe" => match option {
            "lu" | "ru" | "ld" | "rd" => w.empty("p:strips", A::new().a("dir", option)),
            _ => w.empty("p:wipe", A::new().a("dir", lrud("r"))),
        },
        "split" => {
            let (o, d) = match option {
                "vertIn" => ("vert", "in"),
                "vertOut" => ("vert", "out"),
                "horzIn" => ("horz", "in"),
                _ => ("horz", "out"),
            };
            w.empty("p:split", A::new().a("orient", o).a("dir", d));
        }
        "randomBar" => w.empty("p:randomBar", A::new().a("dir", hv("vert"))),
        "shape" => match option {
            "diamond" => w.empty0("p:diamond"),
            "plus" => w.empty0("p:plus"),
            "in" | "out" => w.empty("p:zoom", A::new().a("dir", option)),
            _ => w.empty0("p:circle"),
        },
        "zoom" => match option {
            "rotate" => w.empty0("p:newsflash"),
            _ => {
                w.empty("p14:warp", A::new().a("dir", if option == "out" { "out" } else { "in" }));
                return Some("p14");
            }
        },
        "uncover" => w.empty("p:pull", A::new().a("dir", eight("l"))),
        "cover" => w.empty("p:cover", A::new().a("dir", eight("l"))),
        "checker" => w.empty("p:checker", A::new().a("dir", hv("horz"))),
        "blinds" => w.empty("p:blinds", A::new().a("dir", hv("horz"))),
        "comb" => w.empty("p:comb", A::new().a("dir", hv("horz"))),
        "clock" => match option {
            "counterClockwise" => {
                w.empty("p14:wheelReverse", A::new().a("spokes", 1));
                return Some("p14");
            }
            "wedge" => w.empty0("p:wedge"),
            _ => w.empty("p:wheel", A::new().a("spokes", 1)),
        },
        "dissolve" => w.empty0("p:dissolve"),
        "random" => w.empty0("p:random"),
        "honeycomb" => {
            w.empty0("p14:honeycomb");
            return Some("p14");
        }
        "flash" => {
            w.empty0("p14:flash");
            return Some("p14");
        }
        "vortex" => {
            w.empty("p14:vortex", A::new().a("dir", lrud("l")));
            return Some("p14");
        }
        "glitter" => {
            let hex = option.starts_with("hex");
            let d = option.trim_start_matches("hex").to_ascii_lowercase();
            let d = if matches!(d.as_str(), "l" | "r" | "u" | "d") { d } else { "l".to_string() };
            w.empty("p14:glitter", A::new().a("dir", d).o("pattern", hex.then_some("hexagon")));
            return Some("p14");
        }
        "switch" | "flip" | "gallery" | "conveyor" | "ferris" => {
            let name = match kind {
                "switch" => "p14:switch",
                "flip" => "p14:flip",
                "gallery" => "p14:gallery",
                "conveyor" => "p14:conveyor",
                _ => "p14:ferris",
            };
            w.empty(name, A::new().a("dir", lr("l")));
            return Some("p14");
        }
        "ripple" => {
            let d = match option {
                "lu" | "ru" | "ld" | "rd" => option,
                _ => "center",
            };
            w.empty("p14:ripple", A::new().a("dir", d));
            return Some("p14");
        }
        "doors" | "window" => {
            let name = if kind == "doors" { "p14:doors" } else { "p14:window" };
            w.empty(name, A::new().a("dir", hv("vert")));
            return Some("p14");
        }
        "pan" => {
            w.empty("p14:pan", A::new().a("dir", lrud("d")));
            return Some("p14");
        }
        "shred" => {
            let particles = option.starts_with("particles");
            w.empty(
                "p14:shred",
                A::new().a("pattern", if particles { "rectangle" } else { "strip" }).a("dir", if option.ends_with("Out") { "out" } else { "in" }),
            );
            return Some("p14");
        }
        "reveal" => {
            w.empty("p14:reveal", A::new().t("thruBlk", option.starts_with("black")).a("dir", if option.ends_with("Right") { "r" } else { "l" }));
            return Some("p14");
        }
        "flythrough" => {
            w.empty(
                "p14:flythrough",
                A::new().a("dir", if option.starts_with("out") { "out" } else { "in" }).t("hasBounce", option.ends_with("Bounce")),
            );
            return Some("p14");
        }
        "cube" | "box" | "rotate" | "orbit" => {
            w.empty(
                "p14:prism",
                A::new().a("dir", lrud("l")).t("isContent", matches!(kind, "rotate" | "orbit")).t("isInverted", matches!(kind, "box" | "orbit")),
            );
            return Some("p14");
        }
        "pageCurlDouble" if option.starts_with("single") => {
            w.empty("p15:prstTrans", A::new().a("prst", "pageCurlSingle").t("invX", option == "singleR"));
            return Some("p15");
        }
        "fallOver" | "drape" | "curtains" | "wind" | "prestige" | "fracture" | "crush" | "peelOff" | "pageCurlDouble" | "airplane" | "origami" => {
            let inv = if INVX_LEFT.contains(&kind) { option == "l" } else { option == "r" };
            w.empty("p15:prstTrans", A::new().a("prst", kind).t("invX", inv));
            return Some("p15");
        }
        "morph" => {
            let o = match option {
                "words" => "byWord",
                "characters" => "byChar",
                _ => "byObject",
            };
            w.empty("p159:morph", A::new().a("option", o));
            return Some("p159");
        }
        _ => return None,
    }
    Some("p")
}

// ---------------------------------------------------------------------------------------------
// Animations

/// Effect options ↔ preset subtypes.
const SUBTYPES: &[(&str, &[(&str, u32)])] = &[
    ("fly", DIR8),
    ("flyOut", DIR8),
    ("crawl", DIR4),
    ("crawlOut", DIR4),
    ("peek", DIR4),
    ("wipe", DIR4),
    ("wipeOut", DIR4),
    ("strips", &[("lu", 9), ("ru", 3), ("ld", 12), ("rd", 6)]),
    ("split", SPLIT),
    ("splitOut", SPLIT),
    ("zoom", INOUT),
    ("zoomOut", INOUT),
    ("basicZoom", INOUT),
    ("basicZoomOut", INOUT),
    ("plus", INOUT),
    ("plusOut", INOUT),
    ("box", INOUT),
    ("circle", INOUT),
    ("diamond", INOUT),
    ("randomBars", HV),
    ("randomBarsOut", HV),
    ("blinds", HV),
    ("swivel", HV),
    ("swivelOut", HV),
    ("swivelBasic", HV),
    ("swivelBasicOut", HV),
    ("stretch", HV),
    ("stretchOut", HV),
    ("changeFontSize", &[("increase", 2), ("decrease", 1)]),
    ("blindsOut", HV),
    ("boxOut", INOUT),
    ("checkerboardOut", &[("across", 10), ("down", 5)]),
    ("diamondOut", INOUT),
    ("peekOut", DIR4),
    ("stripsOut", &[("lu", 9), ("ru", 3), ("ld", 12), ("rd", 6)]),
    ("checkerboard", &[("across", 10), ("down", 5)]),
    ("wheel", WHEEL),
    ("wheelOut", WHEEL),
    ("float", &[("u", 0), ("d", 4)]),
    ("floatOut", &[("u", 0), ("d", 4)]),
    (
        "shape",
        &[("circleIn", 16), ("circleOut", 32), ("boxIn", 16), ("boxOut", 32), ("diamondIn", 16), ("diamondOut", 32), ("plusIn", 16), ("plusOut", 32)],
    ),
    ("shapeOut", INOUT_SHAPE),
    ("lines", &[("down", 4), ("up", 1), ("right", 2), ("left", 8)]),
    ("arcs", &[("down", 4), ("up", 1), ("right", 2), ("left", 8)]),
    ("turns", &[("down", 4), ("up", 1), ("right", 2), ("left", 8)]),
];
const DIR8: &[(&str, u32)] = &[("b", 4), ("lb", 12), ("l", 8), ("lt", 9), ("t", 1), ("rt", 3), ("r", 2), ("rb", 6)];
const DIR4: &[(&str, u32)] = &[("b", 4), ("l", 8), ("t", 1), ("r", 2)];
const SPLIT: &[(&str, u32)] = &[("horzIn", 21), ("horzOut", 42), ("vertIn", 26), ("vertOut", 37)];
const INOUT: &[(&str, u32)] = &[("in", 16), ("out", 32)];
const INOUT_SHAPE: &[(&str, u32)] = &[("circleIn", 16), ("circleOut", 32)];
const HV: &[(&str, u32)] = &[("horz", 10), ("vert", 5)];
const WHEEL: &[(&str, u32)] = &[("1", 1), ("2", 2), ("3", 3), ("4", 4), ("8", 8)];

pub fn option_from_subtype(effect: &str, sub: u32) -> String {
    SUBTYPES.iter().find(|(e, _)| *e == effect).and_then(|(_, m)| m.iter().find(|(_, s)| *s == sub)).map(|(o, _)| o.to_string()).unwrap_or_default()
}

pub fn subtype_from_option(effect: &str, option: &str) -> Option<u32> {
    SUBTYPES.iter().find(|(e, _)| *e == effect).and_then(|(_, m)| m.iter().find(|(o, _)| *o == option)).map(|(_, s)| *s)
}

/// Our effect id for a preset (first match in [`ANIMATIONS`]).
pub fn effect_for_preset(class: AnimClass, preset: u32) -> Option<&'static str> {
    ANIMATIONS.iter().find(|a| a.2 == class && a.3 == preset && a.0 != "customPath").map(|a| a.0)
}

/// Effect used for presets we don't know.
pub fn fallback_effect(class: AnimClass) -> &'static str {
    match class {
        AnimClass::Entrance => "fade",
        AnimClass::Exit => "fadeOut",
        AnimClass::Emphasis => "pulse",
        AnimClass::Path => "customPath",
        AnimClass::Media => "play",
    }
}

/// The preset id to write for an animation.
pub fn preset_for(a: &deckcraft_model::Animation) -> u32 {
    // Before preset 7 was identified as Crawl, older files could store Fade with that ID.
    if a.class == AnimClass::Entrance && a.effect == "fade" && a.preset_id == Some(7) {
        return 7;
    }
    // Float Down is PowerPoint's preset 47 (Float Up is 42).
    if matches!(a.effect.as_str(), "float" | "floatOut") && a.option == "d" {
        return 47;
    }
    if let Some(p) = a.preset_id {
        let consistent = match effect_for_preset(a.class, p) {
            Some(e) => e == a.effect,
            None => a.effect == fallback_effect(a.class),
        };
        if consistent {
            return p;
        }
    }
    match deckcraft_model::anim::animation_info(&a.effect, a.class) {
        Some(info) if info.2 == a.class => info.3,
        _ => match a.class {
            AnimClass::Entrance | AnimClass::Exit => 10,
            AnimClass::Emphasis => 26,
            AnimClass::Path => 0,
            AnimClass::Media => 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    /// #147: these exits, with their directions, fell back to a plain fade (PowerPoint's ids).
    #[test]
    fn exit_presets_import_as_their_own_effects() {
        for (preset, sub, effect, option) in [
            (3, 10, "blindsOut", "horz"),
            (4, 32, "boxOut", "out"),
            (5, 5, "checkerboardOut", "down"),
            (8, 16, "diamondOut", "in"),
            (12, 4, "peekOut", "b"),
            (18, 6, "stripsOut", "rd"),
            (37, 0, "sinkDown", ""),
            (49, 0, "spinnerOut", ""),
            (55, 0, "contract", ""),
        ] {
            let e = effect_for_preset(AnimClass::Exit, preset).expect("known exit");
            assert_eq!((e, option_from_subtype(e, sub).as_str()), (effect, option), "preset {preset}");
        }
    }

    #[test]
    fn captured_animation_preset_ids_and_subtypes_map_to_their_effects() {
        for (class, preset, effect) in [
            (AnimClass::Entrance, 7, "crawl"),
            (AnimClass::Entrance, 23, "basicZoom"),
            (AnimClass::Entrance, 36, "shimmer"),
            (AnimClass::Emphasis, 4, "changeFontSize"),
            (AnimClass::Emphasis, 24, "darken"),
            (AnimClass::Emphasis, 25, "desaturate"),
            (AnimClass::Emphasis, 23, "contrastingColor"),
            (AnimClass::Exit, 19, "swivelBasicOut"),
            (AnimClass::Exit, 58, "foldOut"),
        ] {
            assert_eq!(effect_for_preset(class, preset), Some(effect), "preset {class:?}/{preset}");
        }
        assert_eq!(subtype_from_option("crawl", "b"), Some(4));
        assert_eq!(subtype_from_option("plus", "in"), Some(16));
        assert_eq!(subtype_from_option("basicZoomOut", "out"), Some(32));
        assert_eq!(subtype_from_option("changeFontSize", "increase"), Some(2));
    }

    #[test]
    fn every_transition_kind_round_trips() {
        for (kind, _, _, _, opts) in deckcraft_model::anim::TRANSITIONS {
            if *kind == "none" {
                continue;
            }
            let opts: Vec<&str> = if opts.is_empty() { vec![""] } else { opts.to_vec() };
            for o in opts {
                let mut w = W::frag();
                let ns = transition_to_xml(&mut w, kind, o).unwrap_or_else(|| panic!("no xml for {kind}"));
                assert!(!ns.is_empty());
                let d = parse(w.s.as_bytes()).unwrap();
                let (k, opt) = transition_from_xml(&d.root).unwrap_or_else(|| panic!("no read for {kind}"));
                assert_eq!(k, *kind, "kind for {kind}/{o}");
                if !o.is_empty() {
                    assert_eq!(opt, o, "option for {kind}");
                }
            }
        }
    }

    #[test]
    fn subtypes() {
        assert_eq!(subtype_from_option("fly", "b"), Some(4));
        assert_eq!(option_from_subtype("fly", 8), "l");
        assert_eq!(option_from_subtype("fade", 0), "");
        assert_eq!(effect_for_preset(AnimClass::Entrance, 2), Some("fly"));
        assert_eq!(effect_for_preset(AnimClass::Emphasis, 8), Some("spin"));
    }

    #[test]
    fn legacy_fade_keeps_its_preset_id() {
        let legacy = deckcraft_model::Animation { class: AnimClass::Entrance, effect: "fade".into(), preset_id: Some(7), ..Default::default() };
        assert_eq!(preset_for(&legacy), 7);

        let current = deckcraft_model::Animation { class: AnimClass::Entrance, effect: "fade".into(), ..Default::default() };
        assert_eq!(preset_for(&current), 10);

        let other = deckcraft_model::Animation { class: AnimClass::Entrance, effect: "fly".into(), preset_id: Some(7), ..Default::default() };
        assert_eq!(preset_for(&other), 2);
    }

    /// PowerPoint's own ids: Darken is 24 and Desaturate 25 (read back from its saved file).
    #[test]
    fn darken_and_desaturate_use_powerpoints_ids() {
        assert_eq!(effect_for_preset(AnimClass::Emphasis, 24), Some("darken"));
        assert_eq!(effect_for_preset(AnimClass::Emphasis, 25), Some("desaturate"));
        for (effect, id) in [("darken", 24), ("desaturate", 25)] {
            let a = deckcraft_model::Animation { class: AnimClass::Emphasis, effect: effect.into(), ..Default::default() };
            assert_eq!(preset_for(&a), id, "{effect}");
        }
    }
}
