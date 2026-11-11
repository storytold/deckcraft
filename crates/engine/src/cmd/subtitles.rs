//! Slide Show ▸ Captions & Subtitles: the per-user subtitle settings and the provider's
//! host-reported status.
//!
//! The settings are not deck state: they live in the app's `prefs.json`, so a deck copied to
//! another machine keeps that machine's own captions. The transcriber itself belongs to the UI
//! host, which watches these settings and starts or stops its provider; the engine only records
//! what the host reports in `Session::subtitle_status`.

use serde_json::{Value, json};

use super::{CommandSpec, always, cmd};
use crate::{Result, Session};
use deckcraft_asr::{Accelerator, Position, SubtitleSettings};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "show.subtitles",
            "Subtitle Settings",
            ["Slide Show", "Captions & Subtitles"],
            None,
            "{on?: bool, spoken?: auto|en|es|… (a Whisper language tag), subtitle?: en|es|… (a Whisper language tag), position?: bottomOverlay|topOverlay|belowSlide|aboveSlide, microphone?: name (not empty)|null (system default), fontSizePt?: number 8-96 (clamped), maxLines?: integer 1-8 (clamped), accelerator?: cpu|openVino} → {settings: {on, spoken, subtitle, microphone, position, fontSizePt, maxLines, accelerator}, provider: {state, detail?, model?, language?, microphone?, accelerator?, capabilities?}} (per-user; not saved in the presentation; every supplied field is type-checked and a wrong type is an error; an accelerator this build cannot use runs on the CPU and says so in provider.capabilities.note)",
            always,
            set
        ),
        cmd!(
            query "show.subtitles.state",
            "Subtitle Status",
            ["Slide Show", "Captions & Subtitles"],
            None,
            "{} → {settings: {on, spoken, subtitle, microphone, position, fontSizePt, maxLines, accelerator}, provider: {state, detail?, model?, language?, microphone?, accelerator?, capabilities?}} (takes no parameters; a non-empty parameter object is an error)",
            always,
            state
        ),
    ]
}

/// The command's error for a field of the wrong type.
fn wrong_type(cmd: &str, field: &str, want: &str, got: &Value) -> crate::Result<Value> {
    Err(super::bad(cmd, format!("`{field}` must be {want}, not {got}")))
}

/// Update the settings the parameters name and leave the rest as they were.
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let empty = Value::Object(serde_json::Map::new());
    let p = if p.is_null() { &empty } else { p };
    let Some(map) = p.as_object() else {
        return wrong_type("show.subtitles", "parameters", "an object", p);
    };
    let mut next: SubtitleSettings = s.prefs.subtitles.clone();
    if let Some(on) = map.get("on") {
        next.on = on.as_bool().ok_or_else(|| super::bad("show.subtitles", format!("`on` must be a bool, not {on}")))?;
    }
    if let Some(spoken) = map.get("spoken") {
        let tag = spoken.as_str().ok_or_else(|| super::bad("show.subtitles", format!("`spoken` must be a language tag, not {spoken}")))?;
        if !tag.trim().eq_ignore_ascii_case(deckcraft_asr::settings::AUTO_LANGUAGE) && !deckcraft_asr::is_known_language(tag) {
            return Err(super::bad(
                "show.subtitles",
                format!("`{tag}` is not a recognised spoken language; use `auto` or one of: {}", deckcraft_asr::LANGUAGE_CODES.join(", ")),
            ));
        }
        next.spoken = tag.to_string();
    }
    if let Some(subtitle) = map.get("subtitle") {
        let tag = subtitle.as_str().ok_or_else(|| super::bad("show.subtitles", format!("`subtitle` must be a language tag, not {subtitle}")))?;
        if !deckcraft_asr::is_known_language(tag) {
            return Err(super::bad(
                "show.subtitles",
                format!("`{tag}` is not a recognised subtitle language; use one of: {}", deckcraft_asr::LANGUAGE_CODES.join(", ")),
            ));
        }
        next.subtitle = tag.to_string();
    }
    if let Some(position) = map.get("position") {
        let position =
            position.as_str().ok_or_else(|| super::bad("show.subtitles", format!("`position` must be a position name, not {position}")))?;
        next.position = Position::parse(position).ok_or_else(|| {
            super::bad("show.subtitles", format!("`{position}` is not a subtitle position (bottomOverlay, topOverlay, belowSlide, aboveSlide)"))
        })?;
    }
    match map.get("microphone") {
        None => {}
        Some(Value::Null) => next.microphone = None,
        Some(Value::String(name)) if !name.trim().is_empty() => next.microphone = Some(name.clone()),
        // An empty string is a mistake, not the same request as `null`.
        Some(Value::String(_)) => {
            return Err(super::bad("show.subtitles", "`microphone` must be a device name or null (the system default), not an empty string"));
        }
        Some(other) => return Err(super::bad("show.subtitles", format!("`microphone` must be a device name or null, not {other}"))),
    }
    if let Some(font) = map.get("fontSizePt") {
        let value = font
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| super::bad("show.subtitles", format!("`fontSizePt` must be a finite number, not {font}")))?;
        next.font_pt = value;
    }
    if let Some(lines) = map.get("maxLines") {
        let value = lines
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| super::bad("show.subtitles", format!("`maxLines` must be a non-negative integer, not {lines}")))?;
        next.max_lines = value;
    }
    if let Some(accelerator) = map.get("accelerator") {
        let spelling =
            accelerator.as_str().ok_or_else(|| super::bad("show.subtitles", format!("`accelerator` must be a name, not {accelerator}")))?;
        next.accelerator = Accelerator::parse(spelling)
            .ok_or_else(|| super::bad("show.subtitles", format!("`{spelling}` is not an accelerator (cpu, openVino)")))?;
    }
    s.prefs.subtitles = next.sanitized();
    Ok(report(s))
}

/// The current settings and the provider status the host last reported.
fn state(s: &mut Session, p: &Value) -> Result<Value> {
    // The query takes no parameters: a misspelled one must not look like it worked.
    if !(p.is_null() || p.as_object().is_some_and(|o| o.is_empty())) {
        return Err(super::bad("show.subtitles.state", "this query takes no parameters"));
    }
    Ok(report(s))
}

fn report(s: &Session) -> Value {
    let settings = &s.prefs.subtitles;
    // One source for the switch (`settings.on`): a duplicated top-level `on` could disagree.
    json!({
        "settings": settings,
        "provider": &s.subtitle_status,
    })
}

/// Whether subtitles are on (the UI and the show's caption overlay read this).
pub fn enabled(s: &Session) -> bool {
    s.prefs.subtitles.on
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::cmd::find_command;

    fn session() -> Session {
        Session::new()
    }

    #[test]
    fn the_settings_start_off_and_defaulted() {
        let mut s = session();
        let v = s.execute("show.subtitles.state", &json!({})).unwrap();
        assert_eq!(v["settings"]["on"], false);
        assert_eq!(v["settings"]["position"], "bottomOverlay");
        assert_eq!(v["settings"]["spoken"], "auto");
        assert_eq!(v["settings"]["fontSizePt"], deckcraft_asr::caption::DEFAULT_FONT_PT);
        assert_eq!(v["provider"]["state"], "idle");
        assert!(v.get("on").is_none(), "the switch has one source: settings.on");
    }

    #[test]
    fn a_command_updates_only_the_settings_it_names_and_reports_them() {
        let mut s = session();
        let v = s.execute("show.subtitles", &json!({"on": true, "spoken": "es", "position": "belowSlide"})).unwrap();
        assert_eq!(v["settings"]["on"], true);
        assert_eq!(v["settings"]["spoken"], "es");
        assert_eq!(v["settings"]["position"], "belowSlide");
        assert_eq!(v["settings"]["subtitle"], "en", "untouched");
        assert_eq!(s.prefs.subtitles.microphone, None);
        let v = s.execute("show.subtitles", &json!({"microphone": "USB Mic", "maxLines": 2, "fontSizePt": 30})).unwrap();
        assert_eq!(v["settings"]["microphone"], "USB Mic");
        assert_eq!(v["settings"]["maxLines"], 2);
        assert_eq!(v["settings"]["fontSizePt"], 30.0);
        assert!(v["settings"].get("fontPt").is_none(), "one field name in request and response");
        let v = s.execute("show.subtitles", &json!({"microphone": null})).unwrap();
        assert!(v["settings"]["microphone"].is_null(), "null means the default device");
        let v = s.execute("show.subtitles", &json!({"on": false})).unwrap();
        assert_eq!(v["settings"]["on"], false);
        assert_eq!(v["settings"]["spoken"], "es", "other settings survive");
    }

    #[test]
    fn hostile_values_are_clamped_or_rejected() {
        let mut s = session();
        s.execute("show.subtitles", &json!({"position": "sideways"})).unwrap_err();
        s.execute("show.subtitles", &json!({"microphone": 5})).unwrap_err();
        // Out-of-range numbers are clamped, not silently dropped.
        let v = s.execute("show.subtitles", &json!({"fontSizePt": 1.0e300, "maxLines": 99})).unwrap();
        assert_eq!(v["settings"]["fontSizePt"], deckcraft_asr::caption::MAX_FONT_PT);
        assert_eq!(v["settings"]["maxLines"], deckcraft_asr::caption::MAX_LINES_CAP);
        // A wrong type is an error for every field, and changes nothing.
        s.execute("show.subtitles", &json!({"fontSizePt": 20.0})).unwrap();
        for bad in [
            json!({"fontSizePt": "big"}),
            json!({"on": "yes"}),
            json!({"maxLines": "three"}),
            json!({"spoken": 7}),
            json!({"subtitle": 7}),
            json!({"position": 7}),
            json!({"accelerator": 7}),
            json!({"microphone": ""}),
            json!({"microphone": "   "}),
            json!({"spoken": "!!"}),
            json!({"spoken": "pt-br"}),
            json!({"subtitle": "xx"}),
            json!([1, 2]),
        ] {
            assert!(s.execute("show.subtitles", &bad).is_err(), "{bad} was accepted");
        }
        let v = s.execute("show.subtitles.state", &json!({})).unwrap();
        assert_eq!(v["settings"]["fontSizePt"], 20.0, "a rejected value leaves the setting alone");
        assert_eq!(v["settings"]["spoken"], "auto");
        assert!(v["settings"]["microphone"].is_null());
    }

    #[test]
    fn every_whisper_language_is_accepted_and_auto_only_as_a_spoken_language() {
        let mut s = session();
        for tag in ["en", "es", "zh", "uk", "haw"] {
            assert!(s.execute("show.subtitles", &json!({"spoken": tag, "subtitle": tag})).is_ok(), "{tag}");
        }
        assert!(s.execute("show.subtitles", &json!({"spoken": "auto"})).is_ok());
        assert!(s.execute("show.subtitles", &json!({"subtitle": "auto"})).is_err(), "a caption cannot be `auto`");
    }

    #[test]
    fn the_state_query_rejects_parameters() {
        let mut s = session();
        assert!(s.execute("show.subtitles.state", &json!({})).is_ok());
        assert!(s.execute("show.subtitles.state", &json!(null)).is_ok());
        assert!(s.execute("show.subtitles.state", &json!({"ons": true})).is_err(), "a misspelled field is an error");
        assert!(s.execute("show.subtitles.state", &json!({"on": true})).is_err());
    }

    #[test]
    fn a_failing_provider_is_reported_once_and_changes_nothing_else() {
        let mut s = session();
        s.execute("show.subtitles", &json!({"on": true})).unwrap();
        s.subtitle_status = deckcraft_asr::ProviderStatus::failed("no microphone");
        let v = s.execute("show.subtitles.state", &json!({})).unwrap();
        assert_eq!(v["provider"]["state"], "failed");
        assert_eq!(v["provider"]["detail"], "no microphone");
        assert_eq!(v["settings"]["on"], true, "the setting stays as the user chose");
        // The status is host-reported: the engine never invents one.
        let mut fresh = session();
        assert_eq!(fresh.execute("show.subtitles.state", &json!({})).unwrap()["provider"]["state"], "idle");
    }

    #[test]
    fn the_commands_exist_and_take_empty_params() {
        assert!(find_command("show.subtitles").is_some());
        assert!(find_command("show.subtitles.state").is_some());
        let mut s = session();
        assert!(s.execute("show.subtitles", &json!(null)).is_ok());
        assert!(s.execute("show.subtitles.state", &json!({})).is_ok());
        // No presentation needed: the settings are the user's, not the deck's.
        assert_eq!(s.documents().len(), 0);
    }

    #[test]
    fn an_unavailable_accelerator_is_recorded_and_a_bad_one_rejected() {
        let mut s = session();
        s.execute("show.subtitles", &json!({"accelerator": "openVino"})).unwrap();
        assert_eq!(s.prefs.subtitles.accelerator, Accelerator::OpenVino);
        let v = s.execute("show.subtitles.state", &json!({})).unwrap();
        assert_eq!(v["settings"]["accelerator"], "openVino");
        s.execute("show.subtitles", &json!({"accelerator": "gpu"})).unwrap_err();
        let v = s.execute("show.subtitles", &json!({"accelerator": "cpu"})).unwrap();
        assert_eq!(v["settings"]["accelerator"], "cpu");
    }

    #[test]
    fn settings_survive_a_prefs_round_trip() {
        let mut s = session();
        s.execute("show.subtitles", &json!({"on": true, "spoken": "fr", "position": "topOverlay", "microphone": "Mic 2"})).unwrap();
        let json = serde_json::to_string(&s.prefs).unwrap();
        let back: crate::Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.subtitles, s.prefs.subtitles);
        assert!(json.contains("\"subtitles\""), "prefs.json holds the settings: {json}");
        let damaged: crate::Prefs = serde_json::from_str("{\"subtitles\": {\"on\": true, \"position\": \"sideways\"}}").unwrap();
        assert_eq!(damaged.subtitles.clone().sanitized().position, Position::BottomOverlay);
        assert!(damaged.subtitles.on);
    }
}
