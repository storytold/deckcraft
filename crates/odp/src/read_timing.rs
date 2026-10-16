//! Bounded model-backed SMIL interpreter. Unrepresentable timing retains source and warns.
use super::*;
use deckcraft_model::anim::TextBuild;

const MAX_NODES: usize = 100_000;
const MAX_EFFECTS: usize = 10_000;
/// SMIL clock values (§19.446), bounded to 24 hours and integral milliseconds.
fn clock(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.starts_with("PT") {
        return super::milliseconds(value);
    }
    let seconds = if value.contains(':') {
        let parts = value.split(':').collect::<Vec<_>>();
        if !(2..=3).contains(&parts.len()) {
            return None;
        }
        let mut seconds = 0.0;
        for part in parts {
            seconds = seconds * 60.0 + crate::geometry::number(part)?;
        }
        seconds
    } else if let Some(v) = value.strip_suffix("ms") {
        crate::geometry::number(v)? / 1000.0
    } else if let Some(v) = value.strip_suffix("min") {
        crate::geometry::number(v)? * 60.0
    } else if let Some(v) = value.strip_suffix('h') {
        crate::geometry::number(v)? * 3600.0
    } else {
        crate::geometry::number(value.strip_suffix('s').unwrap_or(value))?
    };
    if !(0.0..=86400.0).contains(&seconds) {
        return None;
    }
    Some((seconds * 1000.0).round() as u32)
}
fn numeric(el: &El, key: &str, default: f64) -> Option<f64> {
    el.attr(key).map(crate::geometry::number).unwrap_or(Some(default))
}
// A finite container constrains its descendants; it is not a default duration inherited by them.
fn clips_children(el: &El, limit: u64, origin: u64, depth: usize) -> bool {
    if depth >= 64 {
        return true;
    }
    let mut cursor = origin;
    for child in el.elements().filter(|e| e.name != "anim:audio") {
        let Some(begin) = child.attr("smil:begin").map(clock).unwrap_or(Some(0)) else { return true };
        let start = if el.name == "anim:seq" { cursor } else { origin }.saturating_add(u64::from(begin));
        let duration = child.attr("smil:dur").and_then(clock).map(u64::from);
        if is_primitive(child) && child.name != "anim:command" && duration.is_none() {
            return true;
        }
        let repeats = child.attr("smil:repeatCount").and_then(|v| v.parse::<u64>().ok()).unwrap_or(1);
        let span = duration.unwrap_or(0).saturating_mul(repeats).saturating_mul(if child.attr("smil:autoReverse") == Some("true") { 2 } else { 1 });
        if start.saturating_add(span) > limit || clips_children(child, limit, start, depth + 1) {
            return true;
        }
        if el.name == "anim:seq" {
            if duration.is_none() && child.elements().next().is_some() {
                return true;
            }
            cursor = start.saturating_add(span);
        }
    }
    false
}
fn source_ids(shapes: &[Shape], map: &mut BTreeMap<String, (ShapeId, Option<u32>)>, depth: usize) -> Result<()> {
    if depth >= 64 {
        return Err(OdpError("animation target nesting exceeds limit".into()));
    }
    for s in shapes {
        for (_, id) in s.tags.iter().filter(|(k, _)| k == "odp:id") {
            if map.insert(id.clone(), (s.id, None)).is_some() {
                return Err(OdpError("duplicate timing target ID".into()));
            }
        }
        if let ShapeKind::Group { children, .. } = &s.kind {
            source_ids(children, map, depth + 1)?;
        }
    }
    Ok(())
}
fn paragraph_ids(
    el: &El,
    targets: &mut BTreeMap<String, (ShapeId, Option<u32>)>,
    current: Option<ShapeId>,
    counter: &mut u32,
    depth: usize,
) -> Result<()> {
    if depth >= 64 {
        return Err(OdpError("paragraph target nesting exceeds limit".into()));
    }
    let own = el.attr("xml:id").or_else(|| el.attr("draw:id")).and_then(|id| targets.get(id).copied()).filter(|(_, p)| p.is_none()).map(|(s, _)| s);
    let current = own.or(current);
    let mut local = 0;
    let counter = if own.is_some() { &mut local } else { counter };
    if matches!(el.name.as_str(), "text:p" | "text:h") {
        if let (Some(shape), Some(id)) = (current, el.attr("xml:id").or_else(|| el.attr("text:id"))) {
            targets.insert(id.into(), (shape, Some(*counter)));
        }
        *counter = counter.saturating_add(1);
    }
    for child in el.elements() {
        paragraph_ids(child, targets, current, counter, depth + 1)?;
    }
    Ok(())
}
#[derive(Clone)]
struct Frame {
    start: AnimStart,
    delay: u32,
    trigger: Option<ShapeId>,
    duration: Option<u32>,
    repeat: u32,
    rewind: bool,
    smooth_start: f64,
    smooth_end: f64,
    auto_reverse: bool,
    class: Option<AnimClass>,
    effect: Option<String>,
    option: Option<String>,
    centered_scale: bool,
    canonical: bool,
    main_sequence: bool,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            start: AnimStart::WithPrevious,
            delay: 0,
            trigger: None,
            duration: None,
            repeat: 1,
            rewind: false,
            smooth_start: 0.0,
            smooth_end: 0.0,
            auto_reverse: false,
            class: None,
            effect: None,
            option: None,
            centered_scale: false,
            canonical: false,
            main_sequence: false,
        }
    }
}
struct Timing<'a, 'b> {
    slide: &'a mut Slide,
    p: &'a mut Presentation,
    zip: &'a mut zip::ZipArchive<Cursor<&'b [u8]>>,
    total: &'a mut u64,
    cache: &'a mut BTreeMap<String, MediaId>,
    targets: BTreeMap<String, (ShapeId, Option<u32>)>,
    page_id: Option<String>,
    nodes: usize,
    last_effect_id: Option<String>,
    order: Vec<Option<usize>>,
    centers: BTreeMap<String, (f64, f64)>,
    group_begin: u64,
    max_end: u64,
}
impl Timing<'_, '_> {
    fn warn(&mut self, message: &str) {
        super::warn(self.p, message)
    }
    fn media(&mut self, el: &El) -> Result<Option<MediaId>> {
        let Some(href) = el.attr("xlink:href") else {
            self.warn("ODF timing audio lacks a resource; source retained");
            return Ok(None);
        };
        match super::super::timing_media(href, None, self.p, self.zip, self.total, self.cache) {
            Ok(id) => Ok(Some(id)),
            Err(_) => {
                self.warn("ODF timing audio could not be loaded safely; source retained");
                Ok(None)
            }
        }
    }
    fn frame(&mut self, el: &El, parent: &Frame) -> std::result::Result<Frame, ()> {
        let mut f = parent.clone();
        if el.attr("presentation:node-type") == Some("main-sequence") {
            f.main_sequence = true;
        }
        if el.attr("presentation:group-id").is_some_and(|id| id.strip_prefix("deckcraft-order:").is_some_and(|n| n.parse::<usize>().is_ok())) {
            f.canonical = true;
        }
        if let Some(kind) = el.attr("presentation:node-type") {
            f.start = match kind {
                "on-click" => AnimStart::OnClick,
                "with-previous" => AnimStart::WithPrevious,
                "after-previous" => AnimStart::AfterPrevious,
                _ => f.start,
            };
        }
        if let Some(class) = el.attr("presentation:preset-class") {
            f.class = Some(match class {
                "entrance" => AnimClass::Entrance,
                "exit" => AnimClass::Exit,
                "emphasis" => AnimClass::Emphasis,
                "motion-path" => AnimClass::Path,
                "media-call" => AnimClass::Media,
                _ => return Err(()),
            });
        }
        if let Some(effect) = el.attr("presentation:preset-id") {
            f.effect = Some(effect.into());
        }
        if let Some(option) = el.attr("presentation:preset-sub-type") {
            f.option = Some(option.into());
        }
        if let Some(begin) = el.attr("smil:begin") {
            if matches!(begin, "next" | "click") || (begin == "indefinite" && f.main_sequence && el.name == "anim:par") {
                f.start = AnimStart::OnClick;
                f.delay = 0;
            } else if let Some(ms) = clock(begin) {
                f.delay = f.delay.checked_add(ms).ok_or(())?;
            } else {
                let (base, offset) = begin.split_once('+').map(|(a, b)| (a, clock(b))).unwrap_or((begin, Some(0)));
                let offset = offset.ok_or(())?;
                if let Some(id) = base.strip_suffix(".click") {
                    let (shape, _) = self.targets.get(id).copied().ok_or(())?;
                    f.trigger = Some(shape);
                    f.start = AnimStart::OnClick;
                    f.delay = offset;
                } else if let Some(id) = base.strip_suffix(".begin") {
                    if self.page_id.as_deref() == Some(id) {
                        f.delay = offset;
                    } else if self.last_effect_id.as_deref() == Some(id) {
                        if !f.canonical && self.slide.animations.last().is_some_and(|a| a.delay_ms != 0) {
                            return Err(());
                        }
                        f.start = AnimStart::WithPrevious;
                        f.delay = offset;
                    } else {
                        return Err(());
                    }
                } else if let Some(id) = base.strip_suffix(".end") {
                    if self.last_effect_id.as_deref() != Some(id) {
                        return Err(());
                    }
                    if !f.canonical && self.slide.animations.len() > 1 {
                        return Err(());
                    }
                    f.start = AnimStart::AfterPrevious;
                    f.delay = offset;
                } else {
                    return Err(());
                }
            }
        }
        if let Some(duration) = el.attr("smil:dur") {
            f.duration =
                if duration == "indefinite" && matches!(el.name.as_str(), "anim:par" | "anim:seq") { None } else { Some(clock(duration).ok_or(())?) };
        }
        if let Some(repeat) = el.attr("smil:repeatCount") {
            if parent.repeat != 1 {
                return Err(());
            }
            f.repeat = if repeat == "indefinite" {
                u32::MAX
            } else {
                let n = crate::geometry::number(repeat).ok_or(())?;
                if n < 1.0 || n.fract() != 0.0 || n > 1_000_000.0 {
                    return Err(());
                }
                n as u32
            };
        }
        if let Some(fill) = el.attr("smil:fill") {
            f.rewind = match fill {
                "remove" => true,
                "freeze" | "hold" => false,
                _ => return Err(()),
            };
        }
        if el.attr("smil:end").is_some_and(|end| f.repeat != u32::MAX || self.page_id.as_ref().is_none_or(|id| end != format!("{id}.click"))) {
            return Err(());
        }
        if el.attr("smil:values").is_some()
            || el.attr("smil:by").is_some()
            || el.attr("smil:restart").is_some()
            || el.attr("smil:accumulate").is_some_and(|v| v != "none")
        {
            return Err(());
        }
        if el.attr("smil:repeatDur").is_some()
            || el.attr("anim:formula").is_some()
            || el.attr("smil:keyTimes").is_some()
            || el.attr("smil:keySplines").is_some()
        {
            return Err(());
        }
        f.smooth_start = numeric(el, "smil:accelerate", f.smooth_start).ok_or(())?;
        f.smooth_end = numeric(el, "smil:decelerate", f.smooth_end).ok_or(())?;
        f.auto_reverse = el.attr("smil:autoReverse").map(|s| s == "true").unwrap_or(f.auto_reverse);
        if !(0.0..=1.0).contains(&f.smooth_start) || !(0.0..=1.0).contains(&f.smooth_end) || f.smooth_start + f.smooth_end > 1.0 {
            return Err(());
        }
        Ok(f)
    }
    fn walk(&mut self, el: &El, parent: &Frame, depth: usize) -> Result<()> {
        self.nodes = self.nodes.saturating_add(1);
        if depth >= 64 || self.nodes > MAX_NODES {
            return Err(OdpError("animation timing exceeds structural limits".into()));
        }
        if matches!(el.attr("smil:begin"), Some("indefinite" | "next" | "click")) {
            self.group_begin = 0;
            self.max_end = 0;
            let (count, canonical) = group_effects(el, false, 0);
            if count > 1 && !canonical {
                self.warn("A compound external ODF click group requires richer scheduling than the editable model; source retained");
                return Ok(());
            }
        }
        if matches!(el.name.as_str(), "anim:par" | "anim:seq")
            && let Some(limit) = el.attr("smil:dur").and_then(clock)
            && clips_children(el, u64::from(limit), 0, 0)
        {
            self.warn("An ODF timing container clips or constrains its child effects; source retained");
            return Ok(());
        }
        if el.attr("presentation:preset-id") == Some("deckcraft-initial-state") {
            return Ok(());
        }
        if el.attr("presentation:preset-id") == Some("deckcraft-slide-transition") {
            if let Some(filter) = el.child("anim:transitionFilter") {
                self.transition_filter(filter);
            }
            return Ok(());
        }
        if el.name == "anim:audio" && el.attr("presentation:preset-id") == Some("deckcraft-transition-sound") {
            let page_begin = self.page_id.as_ref().map(|id| format!("{id}.begin"));
            if el.attr("smil:begin").is_some_and(|v| clock(v) != Some(0) && page_begin.as_deref() != Some(v))
                || el.attr("smil:repeatCount").is_some_and(|v| !matches!(v, "1" | "indefinite"))
                || el.attr("smil:dur").is_some()
                || el.attr("smil:repeatDur").is_some()
                || el.attr("anim:audio-level").is_some_and(|v| crate::geometry::number(v) != Some(1.0))
                || el.attr("smil:end").is_some_and(|v| v.strip_suffix(".begin").is_none_or(|id| id.is_empty() || id.contains(['+', ';', ' '])))
            {
                self.warn("ODF transition audio has unsupported offset, repetition, end or volume semantics; source retained");
                return Ok(());
            }
            let media = self.media(el)?;
            self.slide.tags.push(("odp:transition-sound-end".into(), el.attr("smil:end").unwrap_or("").into()));
            let t = self.slide.transition.get_or_insert_with(Transition::default);
            t.sound = media;
            t.sound_loop = el.attr("smil:repeatCount") == Some("indefinite");
            return Ok(());
        }
        if el.name == "anim:command" && el.attr("anim:command") == Some("stop-audio") {
            let page_begin = self.page_id.as_ref().map(|id| format!("{id}.begin"));
            if parent.delay != 0
                || parent.trigger.is_some()
                || parent.class.is_some()
                || el.attr("smil:begin").is_some_and(|v| clock(v) != Some(0) && page_begin.as_deref() != Some(v))
                || el.attr("smil:end").is_some()
            {
                self.warn("A delayed or triggered ODF stop-audio command has no slide transition equivalent; source retained");
                return Ok(());
            }
            self.slide.transition.get_or_insert_with(Transition::default).stop_sound = true;
            return Ok(());
        }
        if el.name == "anim:transitionFilter" && el.attr("smil:targetElement").is_some_and(|id| self.page_id.as_deref() == Some(id)) {
            self.transition_filter(el);
            return Ok(());
        }
        let mut frame = match self.frame(el, parent) {
            Ok(f) => f,
            Err(()) => {
                self.warn("ODF timing has unsupported clocks, dependencies or interpolation; source retained");
                return Ok(());
            }
        };
        let mut primitives = el.elements().filter(|c| is_primitive(c) && !visibility_helper(c)).collect::<Vec<_>>();
        if primitives.len() == 2 && el.elements().all(|child| is_primitive(child) || child.name == "anim:audio") {
            let scale = primitives.iter().copied().find(|p| p.name == "anim:animateTransform" && p.attr("svg:type") == Some("scale"));
            let translation = primitives.iter().copied().find(|p| p.name == "anim:animateTransform" && p.attr("svg:type") == Some("translate"));
            if let (Some(scale), Some(translation)) = (scale, translation)
                && self.centered_pair(scale, translation)
            {
                frame.centered_scale = true;
                primitives = vec![scale];
            }
        }
        if primitives.is_empty() && el.attr("presentation:preset-class").is_some() {
            let visible = el.elements().filter(|c| visibility_helper(c)).collect::<Vec<_>>();
            if visible.iter().all(|c| c.attr("smil:to") == visible.first().and_then(|v| v.attr("smil:to")))
                && let Some(last) = visible.last()
            {
                primitives.push(*last);
            }
        }
        let leaf = is_primitive(el);
        let effect_wrapper = el.attr("presentation:preset-class").is_some()
            || matches!(el.attr("presentation:node-type"), Some("on-click" | "with-previous" | "after-previous"));
        let compact = el.elements().all(|child| is_primitive(child) || child.name == "anim:audio");
        if leaf || (!primitives.is_empty() && (effect_wrapper || compact) && matches!(el.name.as_str(), "anim:par" | "anim:seq")) {
            let primitive = if leaf {
                Some(el)
            } else if primitives.len() == 1 {
                primitives.first().copied()
            } else {
                None
            };
            if let Some(primitive) = primitive {
                match self.effect(primitive, if leaf { parent } else { &frame }) {
                    Some(mut a) => {
                        if self.slide.animations.len() >= MAX_EFFECTS {
                            return Err(OdpError("too many animation effects".into()));
                        }
                        if a.trigger.is_some() && self.slide.animations.iter().any(|old| old.trigger == a.trigger) {
                            self.warn("Repeated external ODF trigger steps require additional interactive sequence mapping; source retained");
                            return Ok(());
                        }
                        if frame.canonical {
                            let actual = u64::from(a.delay_ms);
                            let base = match a.start {
                                AnimStart::OnClick => {
                                    self.max_end = 0;
                                    0
                                }
                                AnimStart::WithPrevious => self.group_begin,
                                AnimStart::AfterPrevious => self.max_end,
                            };
                            let Some(delay) = actual.checked_sub(base).and_then(|v| u32::try_from(v).ok()) else {
                                self.warn("An ODF effect start cannot be represented by the model's concurrent group timing; source retained");
                                return Ok(());
                            };
                            a.delay_ms = delay;
                            self.group_begin = base;
                            let count = if a.repeat == u32::MAX { 1 } else { a.repeat.clamp(1, 1_000_000) };
                            let span = u64::from(a.duration_ms) * u64::from(count) * if a.auto_reverse { 2 } else { 1 };
                            self.max_end = self.max_end.max(actual.saturating_add(span));
                        }
                        if !leaf {
                            for audio in el.elements().filter(|c| c.name == "anim:audio") {
                                if audio.attr("smil:begin").is_some_and(|v| clock(v) != Some(0))
                                    || audio.attr("smil:dur").is_some()
                                    || audio.attr("smil:end").is_some()
                                    || audio.attr("smil:repeatDur").is_some()
                                    || audio.attr("smil:repeatCount").is_some_and(|v| v != "1")
                                    || audio.attr("anim:audio-level").is_some_and(|v| crate::geometry::number(v) != Some(1.0))
                                {
                                    self.warn("ODF effect audio has unsupported offsets, duration, repetition or volume; source retained");
                                    continue;
                                }
                                if a.sound.is_some() {
                                    self.warn("Multiple simultaneous animation sounds are not modeled; source retained");
                                }
                                a.sound = self.media(audio)?;
                            }
                        }
                        self.slide.animations.push(a);
                        self.order.push(
                            el.attr("presentation:group-id").and_then(|id| id.strip_prefix("deckcraft-order:")).and_then(|index| index.parse().ok()),
                        );
                        self.last_effect_id = el.attr("xml:id").or_else(|| el.attr("anim:id")).map(String::from);
                    }
                    None => self.warn(&format!(
                        "An ODF animation effect ({}/{}) is retained in source but has no faithful editable mapping yet",
                        el.attr("presentation:preset-id").unwrap_or(&primitive.name),
                        primitive.attr("smil:targetElement").unwrap_or("missing target")
                    )),
                }
            } else {
                self.warn("A compound ODF animation effect is retained in source but cannot be represented by one model effect");
            }
            return Ok(());
        }
        if el.name == "anim:iterate" {
            self.warn("ODF text iteration timing is retained in source; the model lacks explicit per-item interval semantics");
            return Ok(());
        }
        if el.name == "anim:audio" {
            self.warn("An independent ODF audio timeline is retained in source; it is not a slide or effect sound");
            return Ok(());
        }
        if !matches!(el.name.as_str(), "anim:par" | "anim:seq") {
            self.warn("An unsupported ODF timing node is retained in source");
            return Ok(());
        }
        let children = el.elements().collect::<Vec<_>>();
        if frame.repeat != 1 && children.len() > 1 {
            self.warn("Repeated compound ODF timing is retained in source; grouped repeat has no model equivalent");
            return Ok(());
        }
        for (i, child) in children.into_iter().enumerate() {
            let mut f = frame.clone();
            if el.name == "anim:seq" && i > 0 {
                f.start = AnimStart::AfterPrevious;
                f.delay = 0;
            }
            self.walk(child, &f, depth + 1)?;
        }
        Ok(())
    }
    fn transition_filter(&mut self, el: &El) {
        if el.attr("smil:fadeColor").is_some_and(|color| color != "#000000") {
            self.warn("A colored ODF page fade cannot be represented as the model's through-black transition; source retained");
            return;
        }
        let Some((kind, option)) = transition_kind(
            el.attr("smil:type").unwrap_or("none"),
            el.attr("smil:subtype").unwrap_or(""),
            el.attr("smil:direction") == Some("reverse"),
        ) else {
            self.warn("An unsupported ODF page transition is retained in source");
            return;
        };
        let Some(duration) = el.attr("smil:dur").and_then(clock) else {
            self.warn("An ODF page transition lacks an exact supported duration; source retained");
            return;
        };
        let t = self.slide.transition.get_or_insert_with(Transition::default);
        t.kind = kind.into();
        t.option = option.into();
        t.duration_ms = duration;
    }
    fn effect(&mut self, el: &El, parent: &Frame) -> Option<Animation> {
        let f = self.frame(el, parent).ok()?;
        let target = el.attr("smil:targetElement")?;
        let (shape, paragraph) = self.targets.get(target).copied()?;
        let mut a = Animation {
            shape,
            paragraph,
            start: f.start,
            delay_ms: f.delay,
            duration_ms: f.duration.unwrap_or(0),
            repeat: f.repeat,
            rewind: f.rewind,
            smooth_start: f.smooth_start,
            smooth_end: f.smooth_end,
            auto_reverse: f.auto_reverse,
            trigger: f.trigger,
            text_build: TextBuild::AsOne,
            ..Default::default()
        };
        match el.name.as_str() {
            "anim:transitionFilter" => {
                let (kind, option) =
                    transition_kind(el.attr("smil:type")?, el.attr("smil:subtype").unwrap_or(""), el.attr("smil:direction") == Some("reverse"))?;
                if !matches!(kind, "fade" | "wipe" | "dissolve" | "randomBar" | "blinds")
                    || numeric(el, "smil:from", 0.0)? != 0.0
                    || numeric(el, "smil:to", 1.0)? != 1.0
                {
                    return None;
                }
                a.class = if el.attr("smil:mode") == Some("out") { AnimClass::Exit } else { AnimClass::Entrance };
                a.effect = match (kind, a.class) {
                    ("fade", AnimClass::Exit) => "fadeOut",
                    ("wipe", AnimClass::Exit) => "wipeOut",
                    ("dissolve", AnimClass::Exit) => "dissolveOut",
                    ("randomBar", AnimClass::Exit) => "randomBarsOut",
                    ("blinds", AnimClass::Exit) => "blindsOut",
                    ("randomBar", _) => "randomBars",
                    (kind, _) => kind,
                }
                .into();
                a.option = match option {
                    "u" => "b",
                    "d" => "t",
                    v => v,
                }
                .into();
            }
            "anim:set" if el.attr("smil:attributeName") == Some("visibility") => {
                a.class = if el.attr("smil:to") == Some("hidden") {
                    AnimClass::Exit
                } else if el.attr("smil:to") == Some("visible") {
                    AnimClass::Entrance
                } else {
                    return None;
                };
                a.effect = if a.class == AnimClass::Exit { "disappear" } else { "appear" }.into();
            }
            "anim:animate" if el.attr("smil:attributeName") == Some("opacity") => {
                if el.attr("smil:calcMode") != Some("linear") {
                    return None;
                }
                if numeric(el, "smil:from", 1.0)? != 1.0 {
                    return None;
                }
                let opacity = numeric(el, "smil:to", 1.0)?;
                if !(0.0..=1.0).contains(&opacity) {
                    return None;
                }
                a.class = AnimClass::Emphasis;
                a.effect = "transparency".into();
                a.amount = Some(1.0 - opacity);
            }
            "anim:animateTransform" => {
                if el.attr("smil:additive") != Some("sum") {
                    return None;
                }
                a.class = AnimClass::Emphasis;
                match el.attr("svg:type")? {
                    "rotate" => {
                        let values = crate::geometry::numbers(el.attr("smil:to")?).ok()?;
                        let from = crate::geometry::numbers(el.attr("smil:from")?).ok()?;
                        let center = self.centers.get(target)?;
                        if values.len() != 3
                            || from.len() != 3
                            || from.first().copied() != Some(0.0)
                            || (values.get(1)? - center.0).abs() > 1e-7
                            || (values.get(2)? - center.1).abs() > 1e-7
                            || from.get(1) != values.get(1)
                            || from.get(2) != values.get(2)
                        {
                            return None;
                        }
                        let angle = *values.first()?;
                        if angle.abs() > 36_000.0 {
                            return None;
                        }
                        a.effect = "spin".into();
                        a.option = if angle < 0.0 { "counterClockwise" } else { "clockwise" }.into();
                        a.amount = Some(angle.abs());
                    }
                    "scale" => {
                        if !f.centered_scale {
                            return None;
                        }
                        if el.attr("smil:from").is_some_and(|v| v != "1 1" && v != "1") {
                            return None;
                        }
                        let values = crate::geometry::numbers(el.attr("smil:to")?).ok()?;
                        let x = *values.first()?;
                        let y = values.get(1).copied().unwrap_or(x);
                        if x <= 0.0 || y <= 0.0 || x > 100.0 || y > 100.0 {
                            return None;
                        }
                        a.effect = "growShrink".into();
                        if x == y {
                            a.option = "both".into();
                            a.amount = Some(x);
                        } else if x == 1.0 {
                            a.option = "vert".into();
                            a.amount = Some(y);
                        } else if y == 1.0 {
                            a.option = "horz".into();
                            a.amount = Some(x);
                        } else {
                            return None;
                        }
                    }
                    _ => return None,
                }
            }
            "anim:animateColor" => {
                if el.attr("smil:from").is_some() || el.attr("smil:calcMode") != Some("linear") {
                    return None;
                }
                a.class = AnimClass::Emphasis;
                a.effect = match el.attr("smil:attributeName")? {
                    "fill-color" => "fillColor",
                    "stroke-color" => "lineColor",
                    "color" => "fontColor",
                    _ => return None,
                }
                .into();
                let rgb = super::super::color(el.attr("smil:to")?)?;
                a.color = Some(rgb);
            }
            "anim:animateMotion" => {
                let shape = self.slide.shapes.iter().find(|s| s.id == shape)?;
                if shape.xfrm.as_ref().is_some_and(|x| x.rot.abs() > 1e-8 || x.flip_h || x.flip_v) || matches!(shape.kind, ShapeKind::Group { .. }) {
                    return None;
                }
                if el.attr("smil:additive") != Some("sum") || el.attr("svg:origin") != Some("0 0") || el.attr("smil:calcMode") != Some("linear") {
                    return None;
                }
                let path = deckcraft_geom::BezPath::from_svg(el.attr("svg:path")?).ok()?;
                let path = deckcraft_geom::Affine::scale_non_uniform(1.0 / self.p.slide_size.width, 1.0 / self.p.slide_size.height) * path;
                a.class = AnimClass::Path;
                a.effect = "customPath".into();
                a.path = Some(format!("{} E", path.to_svg()));
            }
            "anim:command" => {
                a.class = AnimClass::Media;
                a.effect = match el.attr("anim:command")? {
                    "play" => "play",
                    "toggle-pause" => "pause",
                    "stop" => "stop",
                    _ => return None,
                }
                .into();
                if el.elements().any(|p| p.attr("anim:value").is_some_and(|v| v != "0")) {
                    return None;
                }
            }
            _ => return None,
        }
        // Preset labels are advisory: native primitive semantics above remain authoritative.
        if let Some(effect) = f.effect.as_deref()
            && effect == a.effect
            && let Some(option) = f.option
        {
            // Labels may choose an equivalent parameterization, never override native direction.
            if a.effect == "spin" && matches!(option.as_str(), "" | "clockwise" | "counterClockwise") {
                let angle = if a.option == "counterClockwise" { -a.amount? } else { a.amount? };
                a.amount = Some(if option == "counterClockwise" { -angle } else { angle });
                a.option = option;
            } else if option == a.option
                || (a.effect == "fade" && option.is_empty())
                || (a.effect == "growShrink" && a.option == "both" && option.is_empty())
            {
                a.option = option;
            }
        }
        Some(a)
    }
    fn centered_pair(&self, scale: &El, translation: &El) -> bool {
        let check = || -> Option<bool> {
            let target = scale.attr("smil:targetElement")?;
            if translation.attr("smil:targetElement") != Some(target)
                || translation.attr("smil:additive") != Some("sum")
                || translation.attr("smil:from") != Some("0 0")
            {
                return Some(false);
            }
            for name in
                ["smil:begin", "smil:dur", "smil:fill", "smil:accelerate", "smil:decelerate", "smil:autoReverse", "smil:repeatCount", "smil:end"]
            {
                if scale.attr(name) != translation.attr(name) {
                    return Some(false);
                }
            }
            let center = self.centers.get(target)?;
            let size = crate::geometry::numbers(scale.attr("smil:to")?).ok()?;
            let offset = crate::geometry::numbers(translation.attr("smil:to")?).ok()?;
            let x = *size.first()?;
            let y = size.get(1).copied().unwrap_or(x);
            Some(offset.len() == 2 && (offset.first()? - (1.0 - x) * center.0).abs() < 1e-7 && (offset.get(1)? - (1.0 - y) * center.1).abs() < 1e-7)
        };
        check() == Some(true)
    }
}
fn is_primitive(el: &El) -> bool {
    matches!(
        el.name.as_str(),
        "anim:transitionFilter" | "anim:set" | "anim:animate" | "anim:animateTransform" | "anim:animateColor" | "anim:animateMotion" | "anim:command"
    )
}
fn group_effects(el: &El, canonical: bool, depth: usize) -> (usize, bool) {
    if depth >= 64 {
        return (usize::MAX, false);
    }
    let canonical = canonical || el.attr("presentation:group-id").is_some_and(|v| v.starts_with("deckcraft-order:"));
    if is_primitive(el) {
        return (1, canonical);
    }
    el.elements().fold((0, true), |(n, all), child| {
        let (count, canonical) = group_effects(child, canonical, depth + 1);
        (n.saturating_add(count), all && canonical)
    })
}
fn visibility_helper(el: &El) -> bool {
    el.name == "anim:set" && el.attr("smil:attributeName") == Some("visibility") && el.attr("smil:dur").and_then(clock) == Some(0)
}
fn transition_kind<'a>(kind: &str, sub: &'a str, reverse: bool) -> Option<(&'static str, &'a str)> {
    Some(match kind {
        "none" => ("none", ""),
        "fade" => (
            "fade",
            if sub == "fadeOverColor" {
                "throughBlack"
            } else if sub == "crossfade" || sub.is_empty() {
                "smoothly"
            } else {
                return None;
            },
        ),
        "barWipe" => (
            "wipe",
            match (sub, reverse) {
                ("leftToRight", false) => "r",
                ("leftToRight", true) => "l",
                ("topToBottom", false) => "d",
                ("topToBottom", true) => "u",
                _ => return None,
            },
        ),
        "pushWipe" => (
            "push",
            match sub {
                "fromLeft" => "r",
                "fromRight" => "l",
                "fromTop" => "d",
                "fromBottom" => "u",
                _ => return None,
            },
        ),
        "dissolve" => ("dissolve", ""),
        "checkerBoardWipe" => (
            "checker",
            if sub == "down" {
                "vert"
            } else if sub == "across" {
                "horz"
            } else {
                return None;
            },
        ),
        "blindsWipe" => (
            "blinds",
            if sub == "vertical" {
                "vert"
            } else if sub == "horizontal" {
                "horz"
            } else {
                return None;
            },
        ),
        "randomBarWipe" => (
            "randomBar",
            if sub == "vertical" {
                "vert"
            } else if sub == "horizontal" {
                "horz"
            } else {
                return None;
            },
        ),
        "clockWipe" if sub == "clockwiseTwelve" => ("clock", if reverse { "counterClockwise" } else { "clockwise" }),
        _ => return None,
    })
}

pub(crate) fn timing(
    page: &El,
    styles: &Styles<'_>,
    props: &Props,
    slide: &mut Slide,
    p: &mut Presentation,
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    total: &mut u64,
    cache: &mut BTreeMap<String, MediaId>,
) -> Result<()> {
    let mut targets = BTreeMap::new();
    source_ids(&slide.shapes, &mut targets, 0)?;
    paragraph_ids(page, &mut targets, None, &mut 0, 0)?;
    let page_id = page.attr("xml:id").or_else(|| page.attr("draw:id")).map(String::from);
    fn collect_centers(el: &El, out: &mut BTreeMap<String, (f64, f64)>, depth: usize) -> Result<()> {
        if depth >= 64 {
            return Err(OdpError("animation source geometry exceeds nesting limit".into()));
        }
        if let Some(id) = el.attr("xml:id").or_else(|| el.attr("draw:id"))
            && let (Some(w), Some(h)) = (el.attr("svg:width").and_then(length), el.attr("svg:height").and_then(length))
        {
            let x = el.attr("svg:x").and_then(length).unwrap_or(0.0);
            let y = el.attr("svg:y").and_then(length).unwrap_or(0.0);
            out.insert(id.into(), (x + w / 2.0, y + h / 2.0));
        }
        for child in el.elements() {
            collect_centers(child, out, depth + 1)?;
        }
        Ok(())
    }
    let mut centers = BTreeMap::new();
    collect_centers(page, &mut centers, 0)?;
    let mut timing = Timing {
        slide,
        p,
        zip,
        total,
        cache,
        targets,
        page_id,
        nodes: 0,
        last_effect_id: None,
        order: Vec::new(),
        centers,
        group_begin: 0,
        max_end: 0,
    };
    let advance = props.get("presentation:duration").and_then(|v| clock(v));
    if let Some(kind) = props.get("smil:type").or_else(|| props.get("presentation:transition-style")) {
        let subtype = props.get("smil:subtype").map(String::as_str).unwrap_or("");
        if let Some((kind, option)) = transition_kind(kind, subtype, props.get("smil:direction").is_some_and(|s| s == "reverse")) {
            timing.slide.transition = Some(Transition {
                kind: kind.into(),
                option: option.into(),
                duration_ms: match props.get("presentation:transition-speed").map(String::as_str) {
                    Some("fast") => 500,
                    Some("slow") => 2000,
                    _ => 1000,
                },
                ..Default::default()
            });
        } else {
            timing.warn("An unsupported ODF page transition is retained in source");
        }
    }
    if advance.is_some() || props.contains_key("presentation:transition-type") {
        let t = timing.slide.transition.get_or_insert_with(Transition::default);
        t.advance_after_ms = advance;
        t.advance_on_click = props.get("presentation:transition-type").is_none_or(|s| s != "automatic");
    }
    // Legacy presentation:sound lives in drawing-page styles, not in scalar properties.
    if let Some(style) = page.attr("draw:style-name").and_then(|name| styles.get(&("drawing-page".into(), name.into())))
        && let Some(sound) = style.child("style:drawing-page-properties").and_then(|e| e.child("presentation:sound"))
    {
        let media = timing.media(sound)?;
        timing.slide.transition.get_or_insert_with(Transition::default).sound = media;
    }
    for child in page.elements().filter(|e| e.name.starts_with("anim:")) {
        timing.walk(child, &Frame::default(), 0)?;
    }
    if timing.order.iter().all(Option::is_some) && timing.order.len() == timing.slide.animations.len() {
        let mut ordered = std::mem::take(&mut timing.slide.animations).into_iter().zip(timing.order).collect::<Vec<_>>();
        ordered.sort_by_key(|(_, order)| *order);
        timing.slide.animations = ordered.into_iter().map(|(effect, _)| effect).collect();
    }
    Ok(())
}

/// Resolve audio stop targets only after all source pages and their transitions exist.
pub(crate) fn finish_timing(p: &mut Presentation) {
    let mut next_stop: Option<(String, bool)> = None;
    let mut unsupported = false;
    for slide in p.slides.iter_mut().rev() {
        let slide = Arc::make_mut(slide);
        for (_, end) in slide.tags.iter().filter(|(key, _)| key == "odp:transition-sound-end") {
            if end.is_empty() {
                // An actual stop-audio command is sufficient even without a syncbase end.
                if next_stop.as_ref().is_some_and(|(_, explicit)| !*explicit) {
                    unsupported = true;
                }
            } else if next_stop.as_ref().is_none_or(|(id, _)| end != &format!("{id}.begin")) {
                unsupported = true;
            }
        }
        slide.tags.retain(|(key, _)| key != "odp:transition-sound-end");
        if let Some(t) = &slide.transition
            && (t.stop_sound || t.sound.is_some())
        {
            if let Some((_, id)) = slide.tags.iter().find(|(key, _)| key == "odp:id") {
                next_stop = Some((id.clone(), t.stop_sound));
            } else {
                next_stop = Some((String::new(), t.stop_sound));
            }
        }
    }
    if unsupported {
        super::warn(p, "ODF audio ends at a missing, unrelated or unmodeled page event; source retained");
    }
}
