//! Animations tab and Animation Pane.

use std::sync::Arc;

use deckcraft_model::anim::{ANIMATIONS, AnimClass, AnimStart, Animation, TextBuild, animation_info};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "animation.set",
            "Animation Styles",
            ["Animations", "Animation"],
            None,
            "{effect: fade|fly|zoom|wipe|appear|spin|pulse|fadeOut|lines|… (animation.list), class?: entrance|emphasis|exit|path, option?, ids?} — replaces the shapes' animations",
            has_selection,
            set
        ),
        cmd!(
            "animation.add",
            "Add Animation",
            ["Animations", "Advanced Animation"],
            None,
            "{effect, class?, option?, start?: onClick|withPrevious|afterPrevious, duration?: ms, delay?: ms, ids?}",
            has_selection,
            add
        ),
        cmd!("animation.remove", "Remove Animation", ["Animation Pane"], None, "{index} | {ids?} (all of the shapes' animations)", has_slide, remove),
        cmd!("animation.move", "Reorder Animation", ["Animations", "Timing"], None, "{index, to}", has_slide, move_anim),
        cmd!(
            "animation.timing",
            "Timing",
            ["Animations", "Timing"],
            None,
            "{index? (else selected shapes), start?: onClick|withPrevious|afterPrevious, duration?: ms, delay?: ms, repeat?: n, rewind?: bool, trigger?: shape id | null}",
            has_slide,
            timing
        ),
        cmd!(
            "animation.options",
            "Effect Options",
            ["Animations", "Animation"],
            None,
            "{option?, textBuild?: asOne|byParagraph|byWord|byLetter, index?}",
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

fn build(p: &Value) -> Result<Animation> {
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
        option: str_param(p, "option").map(String::from).or_else(|| info.5.first().map(|o| o.to_string())).unwrap_or_default(),
        duration_ms: p.get("duration").and_then(Value::as_u64).map(|v| v.min(600_000) as u32).unwrap_or(info.4),
        delay_ms: p.get("delay").and_then(Value::as_u64).map(|v| v.min(600_000) as u32).unwrap_or(0),
        preset_id: Some(info.3),
        ..Default::default()
    };
    if let Some(st) = str_param(p, "start") {
        a.start = start_of(st);
    }
    if a.class == AnimClass::Path {
        a.path = Some(default_path(&a.effect, &a.option));
    }
    if a.effect == "spin" {
        a.amount = Some(360.0);
    }
    if a.effect == "growShrink" {
        a.amount = Some(1.5);
    }
    Ok(a)
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
        _ => format!("M 0 0 L {dx} {dy} E"),
    }
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let a = build(p)?;
    s.edit(|doc, sel| {
        let sl = doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation.set", "no slide"))?;
        let sl = Arc::make_mut(sl);
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

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let a = build(p)?;
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation.add", "no slide"))?);
        for (k, id) in ids.iter().enumerate() {
            let mut x = Animation { shape: *id, ..a.clone() };
            if k > 0 {
                x.start = AnimStart::WithPrevious;
            }
            sl.animations.push(x);
        }
        Ok(json!({"count": sl.animations.len()}))
    })
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let index = usize_param(p, "index");
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
    let i = usize_param(p, "index").ok_or_else(|| bad("animation.move", "missing `index`"))?;
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

fn each_target(s: &mut Session, p: &Value, f: impl Fn(&mut Animation)) -> Result<Value> {
    let index = usize_param(p, "index");
    let ids = if index.is_none() { targets(s, p)? } else { vec![] };
    s.edit(|doc, sel| {
        let sl = Arc::make_mut(doc.slides.get_mut(sel.slide).ok_or_else(|| bad("animation", "no slide"))?);
        let mut n = 0;
        for (k, a) in sl.animations.iter_mut().enumerate() {
            if index == Some(k) || (index.is_none() && ids.contains(&a.shape)) {
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
    each_target(s, p, |a| {
        if let Some(o) = &option {
            a.option = o.clone();
            if a.class == AnimClass::Path {
                a.path = Some(default_path(&a.effect, o));
            }
        }
        if let Some(b) = build_mode {
            a.text_build = b;
        }
    })
}

fn clear(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").unwrap_or(s.doc()?.selection.slide);
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
    use super::*;

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
}
