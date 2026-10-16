//! Native SMIL timing, authored from OASIS OpenDocument 1.3 part 3 §15.
use super::*;
use deckcraft_model::anim::TextBuild;
const MAX_CLOCK: u32 = 86_400_000;

pub(super) fn page_id(slide: SlideId) -> String {
    format!("slide{}", slide.0)
}
pub(super) fn shape_id(slide: SlideId, shape: ShapeId) -> String {
    format!("slide{}-shape{}", slide.0, shape.0)
}
fn seconds(ms: u32) -> String {
    format!("{}s", f64::from(ms) / 1000.0)
}
fn direction(option: &str) -> Result<(&'static str, &'static str)> {
    Ok(match option {
        "" | "r" => ("leftToRight", "forward"),
        "l" => ("leftToRight", "reverse"),
        "d" => ("topToBottom", "forward"),
        "u" => ("topToBottom", "reverse"),
        _ => return Err(OdpError(format!("unsupported animation direction '{option}'"))),
    })
}
pub(super) fn transition_type(t: &Transition) -> Result<(&'static str, &'static str, &'static str)> {
    if t.duration_ms > MAX_CLOCK || t.advance_after_ms.is_some_and(|ms| ms > MAX_CLOCK) {
        return Err(OdpError("transition clock exceeds the 24-hour interchange limit".into()));
    }
    Ok(match t.kind.as_str() {
        "none" | "cut" if t.option.is_empty() => ("none", "", "forward"),
        "fade" if t.option.is_empty() || t.option == "smoothly" => ("fade", "crossfade", "forward"),
        "fade" | "cut" if t.option == "throughBlack" => ("fade", "fadeOverColor", "forward"),
        "push" => (
            "pushWipe",
            match t.option.as_str() {
                "l" => "fromRight",
                "r" => "fromLeft",
                "u" => "fromBottom",
                "d" => "fromTop",
                _ => return Err(OdpError("unsupported push direction".into())),
            },
            "forward",
        ),
        "wipe" => {
            let (sub, dir) = direction(&t.option)?;
            ("barWipe", sub, dir)
        }
        "dissolve" => ("dissolve", "", "forward"),
        "checker" => ("checkerBoardWipe", if t.option == "vert" { "down" } else { "across" }, "forward"),
        "blinds" => ("blindsWipe", if t.option == "vert" { "vertical" } else { "horizontal" }, "forward"),
        "randomBar" => ("randomBarWipe", if t.option == "vert" { "vertical" } else { "horizontal" }, "forward"),
        "clock" => (
            "clockWipe",
            "clockwiseTwelve",
            if t.option == "counterClockwise" {
                "reverse"
            } else if t.option == "clockwise" || t.option.is_empty() {
                "forward"
            } else {
                return Err(OdpError("unsupported clock variant".into()));
            },
        ),
        _ => return Err(OdpError(format!("transition '{}' has no faithful native SMIL mapping yet", t.kind))),
    })
}
fn animation_filter(a: &Animation) -> Result<(&'static str, &'static str, &'static str)> {
    let effect = a.effect.strip_suffix("Out").unwrap_or(&a.effect);
    Ok(match effect {
        "fade" => ("fade", "crossfade", "forward"),
        "wipe" => {
            let o = match a.option.as_str() {
                "b" => "u",
                "t" => "d",
                v => v,
            };
            let (sub, dir) = direction(o)?;
            ("barWipe", sub, dir)
        }
        "dissolve" => ("dissolve", "", "forward"),
        "randomBars" => ("randomBarWipe", if a.option == "vert" { "vertical" } else { "horizontal" }, "forward"),
        "blinds" => ("blindsWipe", if a.option == "vert" { "vertical" } else { "horizontal" }, "forward"),
        _ => return Err(OdpError(format!("animation '{}' has no faithful native SMIL mapping yet", a.effect))),
    })
}
impl Writer<'_> {
    pub(super) fn timing(&mut self, ctx: &Ctx<'_>, slide: &Slide, out: &mut String) -> Result<()> {
        if slide.animations.len() > 10_000 {
            return Err(OdpError("too many animations".into()));
        }
        let mut triggers = std::collections::BTreeSet::new();
        for a in &slide.animations {
            if let Some(trigger) = a.trigger
                && !triggers.insert(trigger)
            {
                return Err(OdpError("multiple click steps on one trigger need additional interactive sequence mapping".into()));
            }
        }
        if slide.transition.is_none() && slide.animations.is_empty() {
            return Ok(());
        }
        let page = page_id(slide.id);
        out.push_str(&format!("<anim:par xml:id=\"timing{}\" presentation:node-type=\"timing-root\">", slide.id.0));
        if let Some(t) = &slide.transition {
            let (kind, sub, dir) = transition_type(t)?;
            // Unlike the implementation-defined page-style speed, this filter has an exact clock.
            out.push_str(&format!("<anim:par presentation:preset-id=\"deckcraft-slide-transition\" smil:begin=\"{page}.begin\"><anim:transitionFilter smil:targetElement=\"{page}\" smil:type=\"{kind}\" smil:subtype=\"{sub}\" smil:direction=\"{dir}\" smil:dur=\"{}\" smil:mode=\"in\"/></anim:par>",seconds(t.duration_ms)));
            if t.stop_sound {
                out.push_str(&format!("<anim:command anim:command=\"stop-audio\" smil:begin=\"{page}.begin\"/>"));
            }
            if let Some(media) = t.sound {
                let path = self.image_path(media)?;
                let end = self
                    .pres
                    .slides
                    .iter()
                    .skip_while(|s| s.id != slide.id)
                    .skip(1)
                    .find(|s| s.transition.as_ref().is_some_and(|t| t.stop_sound || t.sound.is_some()))
                    .map(|s| format!(" smil:end=\"{}.begin\"", page_id(s.id)))
                    .unwrap_or_default();
                out.push_str(&format!("<anim:audio presentation:preset-id=\"deckcraft-transition-sound\" xlink:href=\"{path}\" smil:begin=\"{page}.begin\" smil:repeatCount=\"{}\"{end}/>",if t.sound_loop{"indefinite"}else{"1"}));
            }
        }
        // Explicit initial states make entrance effects meaningful to standard SMIL consumers.
        let mut hidden = std::collections::BTreeSet::new();
        for a in &slide.animations {
            if a.class == AnimClass::Entrance && hidden.insert((a.shape, a.paragraph)) {
                let target = self.animation_target(slide, a)?;
                out.push_str(&format!("<anim:par presentation:preset-id=\"deckcraft-initial-state\"><anim:set smil:targetElement=\"{target}\" smil:attributeName=\"visibility\" smil:to=\"hidden\" smil:begin=\"0s\" smil:dur=\"0s\" smil:fill=\"freeze\"/></anim:par>"));
            }
        }
        out.push_str("<anim:seq presentation:node-type=\"main-sequence\">");
        let mut group = false;
        let mut group_begin = 0u32;
        let mut max_end = 0u32;
        for (i, a) in slide.animations.iter().enumerate().filter(|(_, a)| a.trigger.is_none()) {
            if !group || a.start == AnimStart::OnClick {
                if group {
                    out.push_str("</anim:par>");
                }
                out.push_str(&format!("<anim:par smil:begin=\"{}\">", if a.start == AnimStart::OnClick { "indefinite" } else { "0s" }));
                group = true;
                group_begin = 0;
                max_end = 0;
            }
            if a.start == AnimStart::AfterPrevious {
                group_begin = max_end;
            }
            let start = group_begin.checked_add(a.delay_ms).ok_or_else(|| OdpError("animation start exceeds timing limit".into()))?;
            if start > MAX_CLOCK {
                return Err(OdpError("animation group start exceeds the 24-hour interchange limit".into()));
            }
            let count = if a.repeat == u32::MAX { 1 } else { a.repeat.clamp(1, 1_000_000) };
            let span = a
                .duration_ms
                .checked_mul(count)
                .and_then(|v| v.checked_mul(if a.auto_reverse { 2 } else { 1 }))
                .ok_or_else(|| OdpError("animation repeat duration exceeds timing limit".into()))?;
            max_end = max_end.max(start.checked_add(span).ok_or_else(|| OdpError("animation end exceeds timing limit".into()))?);
            let begin = seconds(start);
            self.animation(ctx, slide, a, i, &begin, out)?;
        }
        if group {
            out.push_str("</anim:par>");
        }
        out.push_str("</anim:seq>");
        for (i, a) in slide.animations.iter().enumerate().filter(|(_, a)| a.trigger.is_some()) {
            let trigger = a.trigger.ok_or_else(|| OdpError("missing animation trigger".into()))?;
            if !has_shape(&slide.shapes, trigger, 0) {
                return Err(OdpError("animation trigger shape is missing".into()));
            }
            if a.start != AnimStart::OnClick {
                return Err(OdpError("triggered with/after-previous animation sequences are not supported yet".into()));
            }
            let trigger = shape_id(slide.id, trigger);
            out.push_str(&format!("<anim:seq presentation:node-type=\"interactive-sequence\" smil:begin=\"{trigger}.click\">"));
            self.animation(ctx, slide, a, i, &seconds(a.delay_ms), out)?;
            out.push_str("</anim:seq>");
        }
        out.push_str("</anim:par>");
        Ok(())
    }
    fn animation_target(&self, slide: &Slide, a: &Animation) -> Result<String> {
        if !has_shape(&slide.shapes, a.shape, 0) {
            return Err(OdpError("animation target shape is missing".into()));
        }
        let id = shape_id(slide.id, a.shape);
        if let Some(paragraph) = a.paragraph {
            let shape = find_shape(&slide.shapes, a.shape, 0).ok_or_else(|| OdpError("missing text target".into()))?;
            if shape.text.as_ref().is_none_or(|b| paragraph as usize >= b.paragraphs.len()) {
                return Err(OdpError("animation paragraph target is missing".into()));
            }
            Ok(format!("{id}-p{paragraph}"))
        } else {
            Ok(id)
        }
    }
    fn animation(&mut self, ctx: &Ctx<'_>, slide: &Slide, a: &Animation, index: usize, begin: &str, out: &mut String) -> Result<()> {
        if a.duration_ms > MAX_CLOCK || a.delay_ms > MAX_CLOCK {
            return Err(OdpError("animation clock exceeds the 24-hour interchange limit".into()));
        }
        if a.repeat != u32::MAX && a.repeat > 1_000_000 {
            return Err(OdpError("animation repeat count exceeds the model playback limit".into()));
        }
        if a.text_build != TextBuild::AsOne || a.bounce_end != 0.0 || a.after.is_some() {
            return Err(OdpError("text-build, bounce or after-animation behavior needs additional native SMIL mapping".into()));
        }
        for v in [a.smooth_start, a.smooth_end, a.amount.unwrap_or(0.0)] {
            if !v.is_finite() {
                return Err(OdpError("non-finite animation value".into()));
            }
        }
        if !(0.0..=1.0).contains(&a.smooth_start) || !(0.0..=1.0).contains(&a.smooth_end) || a.smooth_start + a.smooth_end > 1.0 {
            return Err(OdpError("invalid animation acceleration".into()));
        }
        let target = self.animation_target(slide, a)?;
        let node = match a.start {
            AnimStart::OnClick => "on-click",
            AnimStart::WithPrevious => "with-previous",
            AnimStart::AfterPrevious => "after-previous",
        };
        let class = match a.class {
            AnimClass::Entrance => "entrance",
            AnimClass::Exit => "exit",
            AnimClass::Emphasis => "emphasis",
            AnimClass::Path => "motion-path",
            AnimClass::Media => "media-call",
        };
        let repeat = if a.repeat == u32::MAX { "indefinite".into() } else { a.repeat.max(1).to_string() };
        let end = if a.repeat == u32::MAX { format!(" smil:end=\"{}.click\"", page_id(slide.id)) } else { String::new() };
        out.push_str(&format!("<anim:par xml:id=\"effect{}-{index}\" presentation:group-id=\"deckcraft-order:{index}\" presentation:node-type=\"{node}\" presentation:preset-class=\"{class}\" presentation:preset-id=\"{}\" presentation:preset-sub-type=\"{}\" smil:begin=\"{}\" smil:dur=\"{}\" smil:repeatCount=\"{repeat}\" smil:fill=\"{}\" smil:accelerate=\"{}\" smil:decelerate=\"{}\" smil:autoReverse=\"{}\"{end}>",slide.id.0,esc(&a.effect),esc(&a.option),esc(begin),seconds(a.duration_ms),if a.rewind{"remove"}else{"freeze"},a.smooth_start,a.smooth_end,a.auto_reverse));
        let timing = format!(
            "smil:targetElement=\"{target}\" smil:dur=\"{}\" smil:fill=\"{}\"",
            seconds(a.duration_ms),
            if a.rewind { "remove" } else { "freeze" }
        );
        if a.class == AnimClass::Entrance {
            out.push_str(&format!("<anim:set smil:targetElement=\"{target}\" smil:attributeName=\"visibility\" smil:to=\"visible\" smil:dur=\"0s\" smil:fill=\"freeze\"/>"));
        }
        match (a.class, a.effect.as_str()) {
            (AnimClass::Entrance, "appear") | (AnimClass::Exit, "disappear") => {
                out.push_str(&format!(
                    "<anim:set {timing} smil:attributeName=\"visibility\" smil:to=\"{}\"/>",
                    if a.class == AnimClass::Exit { "hidden" } else { "visible" }
                ));
            }
            (AnimClass::Entrance | AnimClass::Exit, _) => {
                let (kind, sub, dir) = animation_filter(a)?;
                out.push_str(&format!(
                    "<anim:transitionFilter {timing} smil:type=\"{kind}\" smil:subtype=\"{sub}\" smil:direction=\"{dir}\" smil:mode=\"{}\"/>",
                    if a.class == AnimClass::Exit { "out" } else { "in" }
                ));
            }
            (AnimClass::Emphasis, "spin") => {
                let (cx, cy) = animation_center(ctx, &slide.shapes, a)?;
                let angle = a.amount.unwrap_or(360.0);
                if angle.abs() > 36_000.0 {
                    return Err(OdpError("spin amount exceeds the model playback limit".into()));
                }
                let angle = if a.option == "counterClockwise" { -angle } else { angle };
                out.push_str(&format!("<anim:animateTransform {timing} smil:attributeName=\"transform\" svg:type=\"rotate\" smil:from=\"0 {cx} {cy}\" smil:to=\"{angle} {cx} {cy}\" smil:additive=\"sum\"/>"));
            }
            (AnimClass::Emphasis, "growShrink") => {
                let (cx, cy) = animation_center(ctx, &slide.shapes, a)?;
                let scale = a.amount.unwrap_or(1.5);
                if scale <= 0.0 || scale > 100.0 {
                    return Err(OdpError("invalid animation scale".into()));
                }
                let (x, y) = match a.option.as_str() {
                    "horz" => (scale, 1.0),
                    "vert" => (1.0, scale),
                    "" | "both" => (scale, scale),
                    _ => return Err(OdpError("unknown grow/shrink axis".into())),
                };
                out.push_str(&format!("<anim:animateTransform {timing} smil:attributeName=\"transform\" svg:type=\"translate\" smil:from=\"0 0\" smil:to=\"{} {}\" smil:additive=\"sum\"/><anim:animateTransform {timing} smil:attributeName=\"transform\" svg:type=\"scale\" smil:from=\"1 1\" smil:to=\"{x} {y}\" smil:additive=\"sum\"/>",(1.0-x)*cx,(1.0-y)*cy));
            }
            (AnimClass::Emphasis, "transparency") => {
                let alpha = a.amount.unwrap_or(0.5);
                if !(0.0..=1.0).contains(&alpha) {
                    return Err(OdpError("invalid animation transparency".into()));
                }
                out.push_str(&format!(
                    "<anim:animate {timing} smil:attributeName=\"opacity\" smil:from=\"1\" smil:to=\"{}\" smil:calcMode=\"linear\"/>",
                    1.0 - alpha
                ));
            }
            (AnimClass::Emphasis, "fillColor" | "lineColor" | "fontColor") => {
                let color = a.color.as_ref().ok_or_else(|| OdpError("animation color is missing".into()))?;
                let color = ctx.color(color, None);
                if color.a != 255 {
                    return Err(OdpError("translucent animation color is unsupported".into()));
                }
                let attr = match a.effect.as_str() {
                    "lineColor" => "stroke-color",
                    "fontColor" => "color",
                    _ => "fill-color",
                };
                out.push_str(&format!(
                    "<anim:animateColor {timing} smil:attributeName=\"{attr}\" smil:to=\"#{}\" smil:calcMode=\"linear\"/>",
                    color.hex()
                ));
            }
            (AnimClass::Path, "customPath") => {
                let shape = slide
                    .shapes
                    .iter()
                    .find(|s| s.id == a.shape)
                    .ok_or_else(|| OdpError("grouped motion paths require additional coordinate mapping".into()))?;
                let x = resolve::xfrm(ctx, shape);
                if x.rot.abs() > 1e-8 || x.flip_h || x.flip_v || matches!(shape.kind, ShapeKind::Group { .. }) {
                    return Err(OdpError("transformed motion paths require additional coordinate mapping".into()));
                }
                let path = a.path.as_ref().ok_or_else(|| OdpError("motion path is missing".into()))?;
                if path.len() > 1_000_000 {
                    return Err(OdpError("motion path exceeds limit".into()));
                }
                let path = path.trim().strip_suffix('E').unwrap_or(path).trim();
                let path = deckcraft_geom::BezPath::from_svg(path).map_err(|e| OdpError(e.to_string()))?;
                let path = deckcraft_geom::Affine::scale_non_uniform(self.pres.slide_size.width, self.pres.slide_size.height) * path;
                if path.to_svg().len() > 8192 {
                    return Err(OdpError("serialized motion path exceeds XML attribute limit".into()));
                }
                out.push_str(&format!(
                    "<anim:animateMotion {timing} smil:attributeName=\"transform\" svg:path=\"{}\" svg:origin=\"0 0\" smil:calcMode=\"linear\" smil:additive=\"sum\"/>",
                    esc(&path.to_svg())
                ));
            }
            (AnimClass::Media, "play" | "pause" | "stop") => {
                let command = match a.effect.as_str() {
                    "play" => "play",
                    "pause" => "toggle-pause",
                    _ => "stop",
                };
                out.push_str(&format!("<anim:command smil:targetElement=\"{target}\" anim:command=\"{command}\" smil:begin=\"0s\">"));
                if command == "play" {
                    out.push_str("<anim:param anim:name=\"play\" anim:value=\"0\"/>");
                }
                out.push_str("</anim:command>");
            }
            _ => return Err(OdpError(format!("animation '{}' has no faithful native SMIL mapping yet", a.effect))),
        }
        if let Some(sound) = a.sound {
            let path = self.image_path(sound)?;
            out.push_str(&format!("<anim:audio xlink:href=\"{path}\" smil:begin=\"0s\"/>"));
        }
        if a.class == AnimClass::Exit && a.effect != "disappear" {
            out.push_str(&format!("<anim:set smil:targetElement=\"{target}\" smil:attributeName=\"visibility\" smil:to=\"hidden\" smil:begin=\"{}\" smil:dur=\"0s\" smil:fill=\"freeze\"/>",seconds(a.duration_ms)));
        }
        out.push_str("</anim:par>");
        Ok(())
    }
}
fn has_shape(shapes: &[Shape], id: ShapeId, depth: usize) -> bool {
    find_shape(shapes, id, depth).is_some()
}
fn animation_center(ctx: &Ctx<'_>, shapes: &[Shape], a: &Animation) -> Result<(f64, f64)> {
    fn locate(shapes: &[Shape], id: ShapeId, nested: bool, depth: usize) -> Option<(&Shape, bool)> {
        if depth >= 64 {
            return None;
        }
        for shape in shapes {
            if shape.id == id {
                return Some((shape, nested));
            }
            if let ShapeKind::Group { children, .. } = &shape.kind
                && let Some(found) = locate(children, id, true, depth + 1)
            {
                return Some(found);
            }
        }
        None
    }
    let (shape, nested) = locate(shapes, a.shape, false, 0).ok_or_else(|| OdpError("missing animation center target".into()))?;
    if a.paragraph.is_some() || matches!(shape.kind, ShapeKind::Group { .. }) {
        return Err(OdpError("group and paragraph transform animation centers require additional geometry mapping".into()));
    }
    let x = resolve::xfrm(ctx, shape);
    // Shapes with a static draw:transform use local coordinates; otherwise svg:x/y are in page space.
    if nested || x.rot.abs() > 1e-8 || x.flip_h || x.flip_v { Ok((x.w / 2.0, x.h / 2.0)) } else { Ok((x.x + x.w / 2.0, x.y + x.h / 2.0)) }
}
fn find_shape(shapes: &[Shape], id: ShapeId, depth: usize) -> Option<&Shape> {
    if depth >= 64 {
        return None;
    }
    for s in shapes {
        if s.id == id {
            return Some(s);
        }
        if let ShapeKind::Group { children, .. } = &s.kind
            && let Some(shape) = find_shape(children, id, depth + 1)
        {
            return Some(shape);
        }
    }
    None
}
