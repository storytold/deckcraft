//! Transitions tab.

use std::sync::Arc;

use deckcraft_model::Transition;
use deckcraft_model::anim::TRANSITIONS;
use serde_json::{Value, json};

use super::{CommandSpec, NO_ID_SPACE, always, bad, bool_param, cmd, has_doc, has_slide, ok, opt_usize_param, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "transition.set",
            "Transition",
            ["Transitions", "Transition to This Slide"],
            None,
            "{kind: none|morph|fade|push|wipe|split|reveal|cut|randomBar|shape|uncover|cover|flash|… (transition.list), option?, duration?: ms, index?}",
            has_slide,
            set
        ),
        cmd!(
            "slide.transition.set",
            "Set Slide Transition",
            [],
            None,
            "{index, transition: the whole transition as JSON or null} (exact final value for collaboration)",
            has_doc,
            set_exact
        ),
        cmd!("transition.options", "Effect Options", ["Transitions", "Transition to This Slide"], None, "{option, index?}", has_slide, options),
        cmd!(
            "transition.timing",
            "Timing",
            ["Transitions", "Timing"],
            None,
            "{duration?: ms, onClick?: bool, after?: ms | null, index?}",
            has_slide,
            timing
        ),
        cmd!(
            "transition.sound",
            "Sound",
            ["Transitions", "Timing"],
            None,
            "{builtin?: chime|click|camera|whoosh|drumRoll|laser | path? | data: base64, name?, none?: true (No Sound), stopPrevious?: true, loop?: bool (Loop Until Next Sound), index?} — the sound plays as the slide comes in",
            has_slide,
            sound
        ),
        cmd!("transition.applyAll", "Apply To All", ["Transitions", "Timing"], None, "{}", has_slide, apply_all),
        cmd!(query "transition.list", "Transition Gallery", [], None, "{} → [{id, label, category, duration, options}]", always, list),
    ]
}

fn slide_indices(s: &Session, p: &Value, cmd: &str) -> Result<Vec<usize>> {
    let st = s.doc()?;
    if let Some(i) = opt_usize_param(p, "index", cmd)? {
        return Ok(vec![i]);
    }
    let v: Vec<usize> = st.selection.slides.iter().filter_map(|id| st.doc.slide_index(*id)).collect();
    Ok(if v.is_empty() { vec![st.selection.slide] } else { v })
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let list = slide_indices(s, p, "transition.set")?;
    s.edit(|doc, _| {
        for i in list {
            if let Some(sl) = doc.slides.get_mut(i) {
                let sl = Arc::make_mut(sl);
                sl.transition = set_plan(sl.transition.as_ref(), p)?;
            }
        }
        Ok(())
    })?;
    let kind = set_plan(None, p)?.map(|t| t.kind).unwrap_or_else(|| "none".into());
    Ok(json!({"kind": kind}))
}

/// The transition `transition.set` leaves on one slide, from the slide's current value and the
/// command's parameters.
///
/// The single-user command and the collaboration normaliser both run this, so a shared session
/// reaches exactly the value the command would set (RFC §3.2.1): the effect and its option, the
/// duration, and the advance timings, embedded sound and kept XML of the value it replaced.
pub fn set_plan(current: Option<&Transition>, p: &Value) -> Result<Option<Transition>> {
    let kind = str_param(p, "kind").ok_or_else(|| bad("transition.set", "missing `kind`"))?;
    let info =
        TRANSITIONS.iter().find(|t| t.0.eq_ignore_ascii_case(kind)).ok_or_else(|| bad("transition.set", format!("unknown transition `{kind}`")))?;
    let option = str_param(p, "option").map(String::from).or_else(|| info.4.first().map(|o| o.to_string())).unwrap_or_default();
    let duration = p.get("duration").and_then(Value::as_u64).map(|v| v.min(59_000) as u32);
    let mut t = current.cloned().unwrap_or_default();
    t.kind = info.0.to_string();
    t.option = option;
    t.duration_ms = duration.unwrap_or(info.3);
    t.raw = None;
    Ok(if info.0 == "none" && t.advance_after_ms.is_none() && t.advance_on_click && t.sound.is_none() && !t.stop_sound { None } else { Some(t) })
}

/// Longest transition image `slide.transition.set` may carry, as JSON bytes. A transition is a few
/// hundred bytes; the bound leaves room for kept XML and the rest of the 256 KiB wire message.
const MAX_TRANSITION_JSON: usize = 8 * 1024;

/// Set one slide's transition to an exact final value (collaboration).
///
/// This is the confirmed command of `Payload::SlideTransition`: the value itself, not the
/// arguments of `transition.set`, because that command keeps the advance timings and the embedded
/// sound of the value it replaces. It is also the exact-image restore of a transition step
/// (ADR 0010), so an undo puts back the kept XML of an effect the model does not know. The value
/// is hostile input even though the server builds it: it is size-bounded, deserialised into the
/// model type, and an effect the model cannot name is accepted only when its XML came along.
fn set_exact(s: &mut Session, p: &Value) -> Result<Value> {
    let index = opt_usize_param(p, "index", "slide.transition.set")?.ok_or_else(|| bad("slide.transition.set", "missing `index`"))?;
    let value = p.get("transition").cloned().ok_or_else(|| bad("slide.transition.set", "missing `transition` (use null to clear)"))?;
    let bytes = serde_json::to_vec(&value).map_err(|e| bad("slide.transition.set", format!("invalid transition: {e}")))?;
    if bytes.len() > MAX_TRANSITION_JSON {
        return Err(bad("slide.transition.set", "the transition image is too large"));
    }
    let transition: Option<Transition> = if value.is_null() {
        None
    } else {
        Some(serde_json::from_value(value).map_err(|e| bad("slide.transition.set", format!("invalid transition: {e}")))?)
    };
    if let Some(t) = &transition {
        let known = TRANSITIONS.iter().any(|x| x.0 == t.kind);
        if !known && t.raw.is_none() {
            return Err(bad("slide.transition.set", format!("unknown transition `{}`", t.kind)));
        }
        if t.option.len() > 64 {
            return Err(bad("slide.transition.set", "the transition option is too long"));
        }
    }
    s.edit(|doc, _| {
        if doc.slides.get(index).is_none() {
            return Err(bad("slide.transition.set", "no such slide"));
        }
        if let Some(sound) = transition.as_ref().and_then(|t| t.sound)
            && !doc.media.iter().any(|m| m.id == sound)
        {
            return Err(bad("slide.transition.set", "the transition sound is not in the document"));
        }
        if let Some(sl) = doc.slides.get_mut(index) {
            Arc::make_mut(sl).transition = transition.clone();
        }
        Ok(())
    })?;
    Ok(json!({"index": index}))
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let option = str_param(p, "option").ok_or_else(|| bad("transition.options", "missing `option`"))?.to_string();
    let list = slide_indices(s, p, "transition.options")?;
    s.edit(|doc, _| {
        for i in list {
            if let Some(sl) = doc.slides.get_mut(i)
                && let Some(t) = Arc::make_mut(sl).transition.as_mut()
            {
                t.option = option.clone();
            }
        }
        Ok(())
    })?;
    ok()
}

fn timing(s: &mut Session, p: &Value) -> Result<Value> {
    let list = slide_indices(s, p, "transition.timing")?;
    s.edit(|doc, _| {
        for i in list {
            if let Some(sl) = doc.slides.get_mut(i) {
                let t = Arc::make_mut(sl).transition.get_or_insert_with(Transition::default);
                if let Some(d) = p.get("duration").and_then(Value::as_u64) {
                    t.duration_ms = d.min(59_000) as u32;
                }
                if let Some(c) = bool_param(p, "onClick") {
                    t.advance_on_click = c;
                }
                match p.get("after") {
                    Some(Value::Null) => t.advance_after_ms = None,
                    Some(v) => {
                        if let Some(x) = v.as_u64() {
                            t.advance_after_ms = Some(x.min(86_400_000) as u32);
                        }
                    }
                    None => {}
                }
            }
        }
        Ok(())
    })?;
    ok()
}

/// Transitions › Sound: embed an audio file as the slides' transition sound, or `none` to remove it.
fn sound(s: &mut Session, p: &Value) -> Result<Value> {
    let list = slide_indices(s, p, "transition.sound")?;
    let looping = bool_param(p, "loop");
    let edit_all = |s: &mut Session, f: &dyn Fn(&mut Transition)| {
        s.edit(|doc, _| {
            for &i in &list {
                if let Some(sl) = doc.slides.get_mut(i) {
                    f(Arc::make_mut(sl).transition.get_or_insert_with(Transition::default));
                }
            }
            Ok(())
        })
    };
    if bool_param(p, "none") == Some(true) {
        edit_all(s, &|t| {
            t.sound = None;
            t.stop_sound = false;
            t.sound_loop = false;
        })?;
        return Ok(json!({"sound": null}));
    }
    if bool_param(p, "stopPrevious") == Some(true) {
        edit_all(s, &|t| {
            t.sound = None;
            t.sound_loop = false;
            t.stop_sound = true;
        })?;
        return Ok(json!({"sound": null, "stopPrevious": true}));
    }
    let wants_file = p.get("path").is_some() || p.get("data").is_some() || p.get("builtin").is_some();
    if !wants_file {
        // Only Loop Until Next Sound changes.
        let Some(l) = looping else { return Err(bad("transition.sound", "give `path`, `data`, `builtin`, `none`, `stopPrevious` or `loop`")) };
        edit_all(s, &|t| t.sound_loop = l && t.sound.is_some())?;
        return Ok(json!({"loop": l}));
    }
    let (name, bytes) = match str_param(p, "builtin") {
        Some(id) => {
            let (id, label) = crate::sounds::BUILTIN.iter().find(|b| b.0.eq_ignore_ascii_case(id)).ok_or_else(|| {
                let ids: Vec<&str> = crate::sounds::BUILTIN.iter().map(|b| b.0).collect();
                bad("transition.sound", format!("unknown built-in sound `{id}` (one of {})", ids.join(", ")))
            })?;
            let bytes = crate::sounds::builtin(id).ok_or_else(|| bad("transition.sound", "built-in sound failed"))?;
            (format!("{label}.wav"), bytes)
        }
        None => super::insert::media_bytes(p, "transition.sound")?,
    };
    let ct = super::insert::content_type(&name, &bytes);
    let bytes = Arc::new(bytes);
    let audible = deckcraft_media::probe(&bytes).is_ok_and(|i| i.audio.is_some());
    let bytes = Arc::try_unwrap(bytes).unwrap_or_else(|a| (*a).clone());
    if !audible && !ct.starts_with("audio/") {
        return Err(bad("transition.sound", format!("{name} is not a sound file ({ct})")));
    }
    let lp = looping.unwrap_or(false);
    let media = s.edit(|doc, _| {
        // The same built-in sound is embedded once.
        let existing = doc.media.iter().find(|m| m.name == name && m.data.as_slice() == bytes.as_slice()).map(|m| m.id);
        let media = match existing {
            Some(m) => m,
            None => doc.add_media(&name, ct, bytes).ok_or_else(|| bad("transition.sound", NO_ID_SPACE))?,
        };
        for &i in &list {
            if let Some(sl) = doc.slides.get_mut(i) {
                let t = Arc::make_mut(sl).transition.get_or_insert_with(Transition::default);
                t.sound = Some(media);
                t.stop_sound = false;
                t.sound_loop = lp;
            }
        }
        Ok(media)
    })?;
    Ok(json!({"sound": name, "media": media.0, "playable": audible, "loop": lp}))
}

fn apply_all(s: &mut Session, _p: &Value) -> Result<Value> {
    s.edit(|doc, sel| {
        let t = doc.slides.get(sel.slide).and_then(|x| x.transition.clone());
        for sl in &mut doc.slides {
            Arc::make_mut(sl).transition.clone_from(&t);
        }
        Ok(())
    })?;
    ok()
}

fn list(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(TRANSITIONS.iter().map(|t| json!({"id": t.0, "label": t.1, "category": t.2, "duration": t.3, "options": t.4})).collect()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    #[test]
    fn the_exact_setter_restores_the_whole_transition_including_kept_xml() {
        let mut s = Session::with_new();
        s.execute("transition.timing", &json!({"duration": 900, "after": 5000})).expect("timing");
        s.execute("transition.set", &json!({"kind": "fade"})).expect("fade");
        let fade = s.doc().expect("doc").doc.slides[0].transition.clone().expect("a transition");
        assert_eq!((fade.kind.as_str(), fade.duration_ms, fade.advance_after_ms), ("fade", 700, Some(5000)));

        // An effect the model does not name is accepted only with the XML a file kept.
        let kept =
            json!({"kind": "mystery", "option": "l", "durationMs": 1200, "advanceOnClick": false, "advanceAfterMs": 3000, "raw": "<p14:mystery/>"});
        s.execute("slide.transition.set", &json!({"index": 0, "transition": kept})).expect("an effect the model kept");
        let t = s.doc().expect("doc").doc.slides[0].transition.clone().expect("the kept effect");
        assert_eq!((t.kind.as_str(), t.option.as_str(), t.duration_ms), ("mystery", "l", 1200));
        assert_eq!(t.raw.as_deref(), Some("<p14:mystery/>"));
        assert!(!t.advance_on_click, "the whole value, not just the effect id");

        let mut bare = kept.clone();
        bare["raw"] = Value::Null;
        assert!(s.execute("slide.transition.set", &json!({"index": 0, "transition": bare})).is_err(), "unknown effect without XML");
        assert_eq!(
            s.doc().expect("doc").doc.slides[0].transition.as_ref().map(|t| t.kind.as_str()),
            Some("mystery"),
            "a refused value writes nothing"
        );

        // `null` is the model's "no transition"; the exact value of the step comes back as it was.
        s.execute("slide.transition.set", &json!({"index": 0, "transition": null})).expect("clear");
        assert!(s.doc().expect("doc").doc.slides[0].transition.is_none());
        s.execute("slide.transition.set", &json!({"index": 0, "transition": serde_json::to_value(&fade).expect("json")})).expect("restore");
        assert_eq!(s.doc().expect("doc").doc.slides[0].transition, Some(fade));

        for p in [json!({"index": 99, "transition": null}), json!({"index": 0, "transition": {"kind": "fade", "sound": 42}})] {
            assert!(s.execute("slide.transition.set", &p).is_err(), "{p}");
        }
        assert!(s.execute("slide.transition.set", &json!({"index": 0})).is_err(), "a missing value is not a clear");
    }

    #[test]
    fn the_user_command_and_the_plan_leave_the_same_value() {
        let mut s = Session::with_new();
        s.execute("transition.timing", &json!({"after": 4000})).expect("timing");
        let plan = super::set_plan(s.doc().expect("doc").doc.slides[0].transition.as_ref(), &json!({"kind": "push", "option": "l"})).expect("plan");
        s.execute("transition.set", &json!({"kind": "push", "option": "l"})).expect("set");
        assert_eq!(s.doc().expect("doc").doc.slides[0].transition, plan);
        let plan = super::set_plan(s.doc().expect("doc").doc.slides[0].transition.as_ref(), &json!({"kind": "none"})).expect("plan none");
        s.execute("transition.set", &json!({"kind": "none"})).expect("none");
        assert_eq!(s.doc().expect("doc").doc.slides[0].transition, plan, "the advance timing keeps the value alive");
    }

    #[test]
    fn invalid_index_params_are_rejected_not_defaulted_to_selection() {
        let mut s = Session::with_new();
        while s.doc().expect("doc").doc.slides.len() < 3 {
            s.execute("slide.new", &json!({})).expect("new slide");
        }
        for bad in [json!(-1), json!(1.5)] {
            for (cmd, p) in [
                ("transition.set", json!({"kind": "fade", "index": bad})),
                ("transition.options", json!({"option": "x", "index": bad})),
                ("transition.timing", json!({"duration": 500, "index": bad})),
                ("transition.sound", json!({"none": true, "index": bad})),
            ] {
                assert!(s.execute(cmd, &p).is_err(), "{cmd} must reject {p}");
                assert!(s.doc().expect("doc").doc.slides.iter().all(|sl| sl.transition.is_none()), "{cmd} {p}");
            }
        }
    }
}
