//! Slide Show ▸ Captions & Subtitles: the per-user subtitle settings and the provider's
//! host-reported status.
//!
//! The settings are not deck state: they live in the app's `prefs.json`, so a deck copied to
//! another machine keeps that machine's own captions. The transcriber itself belongs to the UI
//! host, which watches these settings and starts or stops its provider; the engine only records
//! what the host reports in `Session::subtitle_status`.

use serde_json::{Value, json};

use super::{CommandSpec, always, bool_param, cmd, str_param, usize_param};
use crate::{Result, Session};
use deckcraft_asr::{Accelerator, Position, SubtitleSettings};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "show.subtitles",
            "Subtitle Settings",
            ["Slide Show", "Captions & Subtitles"],
            None,
            "{on?: bool, spoken?: lang, subtitle?: lang, position?: bottomOverlay|topOverlay|belowSlide|aboveSlide, microphone?: name, fontSizePt?: pt, maxLines?: n, accelerator?: cpu|openVino} → {on, settings, provider} (per-user; not saved in the presentation; an accelerator this build cannot use runs on the CPU and says so in provider.capabilities.note)",
            always,
            set
        ),
        cmd!(query "show.subtitles.state", "Subtitle Status", ["Slide Show", "Captions & Subtitles"], None, "{} → {on, settings, provider: {state, detail, model, language, microphone}}", always, state),
    ]
}

/// Update the settings the parameters name and leave the rest as they were.
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let mut next: SubtitleSettings = s.prefs.subtitles.clone();
    if let Some(on) = bool_param(p, "on") {
        next.on = on;
    }
    if let Some(spoken) = str_param(p, "spoken") {
        next.spoken = spoken.to_string();
    }
    if let Some(subtitle) = str_param(p, "subtitle") {
        next.subtitle = subtitle.to_string();
    }
    if let Some(position) = str_param(p, "position") {
        next.position = Position::parse(position).ok_or_else(|| {
            super::bad("show.subtitles", format!("`{position}` is not a subtitle position (bottomOverlay, topOverlay, belowSlide, aboveSlide)"))
        })?;
    }
    match p.get("microphone") {
        None => {}
        Some(Value::Null) => next.microphone = None,
        Some(Value::String(name)) => next.microphone = Some(name.clone()),
        Some(other) => return Err(super::bad("show.subtitles", format!("`microphone` must be a device name or null, not {other}"))),
    }
    if let Some(font) = p.get("fontSizePt") {
        next.font_pt = font.as_f64().unwrap_or(next.font_pt);
    }
    if let Some(lines) = usize_param(p, "maxLines") {
        next.max_lines = lines;
    }
    if let Some(accelerator) = str_param(p, "accelerator") {
        next.accelerator = Accelerator::parse(accelerator)
            .ok_or_else(|| super::bad("show.subtitles", format!("`{accelerator}` is not an accelerator (cpu, openVino)")))?;
    }
    s.prefs.subtitles = next.sanitized();
    Ok(report(s))
}

/// The current settings and the provider status the host last reported.
fn state(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(report(s))
}

fn report(s: &Session) -> Value {
    let settings = &s.prefs.subtitles;
    json!({
        "on": settings.on,
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
        assert_eq!(v["on"], false);
        assert_eq!(v["settings"]["position"], "bottomOverlay");
        assert_eq!(v["settings"]["spoken"], "auto");
        assert_eq!(v["provider"]["state"], "idle");
    }

    #[test]
    fn a_command_updates_only_the_settings_it_names_and_reports_them() {
        let mut s = session();
        let v = s.execute("show.subtitles", &json!({"on": true, "spoken": "es", "position": "belowSlide"})).unwrap();
        assert_eq!(v["on"], true);
        assert_eq!(v["settings"]["spoken"], "es");
        assert_eq!(v["settings"]["position"], "belowSlide");
        assert_eq!(v["settings"]["subtitle"], "en", "untouched");
        assert_eq!(s.prefs.subtitles.microphone, None);
        let v = s.execute("show.subtitles", &json!({"microphone": "USB Mic", "maxLines": 2, "fontSizePt": 30})).unwrap();
        assert_eq!(v["settings"]["microphone"], "USB Mic");
        assert_eq!(v["settings"]["maxLines"], 2);
        assert_eq!(v["settings"]["fontPt"], 30.0);
        let v = s.execute("show.subtitles", &json!({"microphone": null})).unwrap();
        assert!(v["settings"]["microphone"].is_null(), "null means the default device");
        let v = s.execute("show.subtitles", &json!({"on": false})).unwrap();
        assert_eq!(v["on"], false);
        assert_eq!(v["settings"]["spoken"], "es", "other settings survive");
    }

    #[test]
    fn hostile_values_are_clamped_or_rejected() {
        let mut s = session();
        s.execute("show.subtitles", &json!({"position": "sideways"})).unwrap_err();
        s.execute("show.subtitles", &json!({"microphone": 5})).unwrap_err();
        let v = s.execute("show.subtitles", &json!({"fontSizePt": 1.0e300, "maxLines": 99, "spoken": "!!"})).unwrap();
        assert_eq!(v["settings"]["fontPt"], deckcraft_asr::caption::MAX_FONT_PT);
        assert_eq!(v["settings"]["maxLines"], deckcraft_asr::caption::MAX_LINES_CAP);
        assert_eq!(v["settings"]["spoken"], "auto");
        let v = s.execute("show.subtitles", &json!({"fontSizePt": "big"})).unwrap();
        assert_eq!(v["settings"]["fontPt"], deckcraft_asr::caption::MAX_FONT_PT, "a non-number leaves the value");
    }

    #[test]
    fn a_failing_provider_is_reported_once_and_changes_nothing_else() {
        let mut s = session();
        s.execute("show.subtitles", &json!({"on": true})).unwrap();
        s.subtitle_status = deckcraft_asr::ProviderStatus::failed("no microphone");
        let v = s.execute("show.subtitles.state", &json!({})).unwrap();
        assert_eq!(v["provider"]["state"], "failed");
        assert_eq!(v["provider"]["detail"], "no microphone");
        assert_eq!(v["on"], true, "the setting stays as the user chose");
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
