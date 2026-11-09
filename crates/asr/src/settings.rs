//! The subtitle settings: per user, not per deck. They live in the app's settings folder
//! (`prefs.json`), never in the `.pptx`, so a deck shown on another machine keeps its own
//! captions. A hand-edited or damaged file is brought into range by [`SubtitleSettings::sanitized`].

use serde::{Deserialize, Serialize};

use crate::accelerator::Accelerator;
use crate::caption::{DEFAULT_FONT_PT, DEFAULT_MAX_LINES, MAX_FONT_PT, MAX_LINES_CAP, MIN_FONT_PT, Position};

/// The language token for "detect the spoken language".
pub const AUTO_LANGUAGE: &str = "auto";

/// Every option the Subtitle Settings dialog offers for the spoken language, as `(tag, label)`.
/// The engine takes any Whisper language token; this list is for the menu.
pub const SPOKEN_LANGUAGES: &[(&str, &str)] = &[
    (AUTO_LANGUAGE, "Detect language"),
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("ru", "Russian"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("pl", "Polish"),
    ("sv", "Swedish"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
];

/// The language tokens the speech model recognises (Whisper's 99 languages). A command that
/// names a language outside this list, or a regional tag such as `pt-br`, is refused: the
/// provider could not run it and would fail after the user asked for it.
pub const LANGUAGE_CODES: [&str; 99] = [
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it", "id", "hi", "fi", "vi", "he", "uk", "el", "ms",
    "cs", "ro", "da", "hu", "ta", "no", "th", "ur", "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn",
    "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si", "km", "sn", "yo", "so", "af", "oc", "ka", "be",
    "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo", "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln", "ha",
    "ba", "jw", "su",
];

/// Whether `tag` is one of [`LANGUAGE_CODES`] (case-insensitive).
pub fn is_known_language(tag: &str) -> bool {
    let tag = tag.trim();
    LANGUAGE_CODES.iter().any(|code| code.eq_ignore_ascii_case(tag))
}
/// The languages a subtitle line can be shown in. Translation is not part of this cut, so the
/// dialog disables the choice; the list is the languages the engine transcribes and the setting
/// records the choice.
pub const SUBTITLE_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("ru", "Russian"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("pl", "Polish"),
    ("sv", "Swedish"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
];

/// The per-user subtitle settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubtitleSettings {
    /// Subtitles are drawn in the slide show.
    pub on: bool,
    /// The language spoken (a Whisper token, or [`AUTO_LANGUAGE`]).
    pub spoken: String,
    /// The language the caption is shown in.
    pub subtitle: String,
    /// Capture device name as the host reports it; `None` is the system default.
    pub microphone: Option<String>,
    /// An unknown spelling (a hand-edited file) is the default position rather than a damaged
    /// settings file that loses every other value too.
    #[serde(default, deserialize_with = "lenient_position")]
    pub position: Position,
    /// Text size in points. The request field is `fontSizePt`, and it is the response field too;
    /// `fontPt` is accepted when reading an older `prefs.json`.
    #[serde(rename = "fontSizePt", alias = "fontPt")]
    pub font_pt: f64,
    pub max_lines: usize,
    /// The processor to run the model on; an unavailable request falls back to the CPU with a
    /// note in the provider status. An unknown spelling (a hand-edited file, or a variant from a
    /// newer build) is the default rather than a damaged settings file that loses every value.
    #[serde(default, deserialize_with = "lenient_accelerator")]
    pub accelerator: Accelerator,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        SubtitleSettings {
            on: false,
            spoken: AUTO_LANGUAGE.into(),
            subtitle: "en".into(),
            microphone: None,
            position: Position::BottomOverlay,
            font_pt: DEFAULT_FONT_PT,
            max_lines: DEFAULT_MAX_LINES,
            accelerator: Accelerator::Cpu,
        }
    }
}

impl SubtitleSettings {
    /// Whether two settings would run the same stream: the provider restarts for the on switch,
    /// the spoken language and the microphone only, so moving the caption or resizing its text
    /// does not drop the audio.
    pub fn provider_equivalent(&self, other: &SubtitleSettings) -> bool {
        self.on == other.on && self.spoken == other.spoken && self.microphone == other.microphone && self.accelerator == other.accelerator
    }

    /// The spoken language meant for the engine: `None` when it should detect.
    pub fn language_hint(&self) -> Option<&str> {
        if self.spoken.eq_ignore_ascii_case(AUTO_LANGUAGE) { None } else { Some(self.spoken.as_str()) }
    }

    /// The settings with every value usable: languages are short alphanumeric tags, and the
    /// size and line cap stay in the ranges the caption layout accepts.
    pub fn sanitized(mut self) -> Self {
        let d = SubtitleSettings::default();
        self.spoken = language(&self.spoken, AUTO_LANGUAGE, true);
        self.subtitle = language(&self.subtitle, "en", false);
        self.microphone = self.microphone.and_then(|m| {
            let m = m.trim();
            if m.is_empty() { None } else { Some(m.chars().take(200).collect()) }
        });
        let font = if self.font_pt.is_finite() { self.font_pt.clamp(MIN_FONT_PT, MAX_FONT_PT) } else { d.font_pt };
        self.font_pt = font;
        self.max_lines = self.max_lines.clamp(1, MAX_LINES_CAP);
        self
    }
}

/// Read a position leniently: anything but a known spelling is the default.
fn lenient_position<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Position, D::Error> {
    use serde::Deserialize;
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_str().and_then(Position::parse).unwrap_or_default())
}

/// Read an accelerator leniently: an unknown spelling is the default.
fn lenient_accelerator<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Accelerator, D::Error> {
    use serde::Deserialize;
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_str().and_then(Accelerator::parse).unwrap_or_default())
}

/// A short language tag: letters and `-`, at most 16 characters. `auto` is accepted only where
/// `allow_auto` is set.
fn language(value: &str, fallback: &str, allow_auto: bool) -> String {
    let v = value.trim().to_ascii_lowercase();
    if v.is_empty() || v.chars().count() > 16 {
        return fallback.to_string();
    }
    if v == AUTO_LANGUAGE {
        return if allow_auto { AUTO_LANGUAGE.to_string() } else { fallback.to_string() };
    }
    if !v.chars().all(|c| c.is_ascii_alphabetic() || c == '-') {
        return fallback.to_string();
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_json() {
        let s = SubtitleSettings {
            on: true,
            spoken: "es".into(),
            subtitle: "en".into(),
            microphone: Some("USB Microphone".into()),
            position: Position::BelowSlide,
            font_pt: 32.0,
            max_lines: 2,
            accelerator: Accelerator::Cpu,
        };
        let json = serde_json::to_string(&s).unwrap_or_default();
        let back: SubtitleSettings = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back, s);
        assert_eq!(back.language_hint(), Some("es"));
        let old: SubtitleSettings = serde_json::from_str("{\"on\":true}").unwrap_or_default();
        assert!(old.on && old.position == Position::BottomOverlay && old.spoken == AUTO_LANGUAGE);
    }

    #[test]
    fn a_damaged_file_gives_usable_settings() {
        let hostile: SubtitleSettings = serde_json::from_value(serde_json::json!({
            "on": true,
            "spoken": "",
            "subtitle": "!!",
            "microphone": "   ",
            "position": "sideways",
            "fontPt": 1.0e300,
            "maxLines": 0,
        }))
        .unwrap_or_default();
        assert_eq!(hostile.position, Position::BottomOverlay, "an unknown position is the default");
        let s = hostile.sanitized();
        assert_eq!(s.spoken, AUTO_LANGUAGE);
        assert_eq!(s.subtitle, "en");
        assert_eq!(s.microphone, None);
        assert_eq!(s.font_pt, MAX_FONT_PT);
        assert_eq!(s.max_lines, 1);

        let nan: SubtitleSettings = serde_json::from_str("{\"fontPt\": 1e400}").unwrap_or_default();
        assert_eq!(nan.sanitized().font_pt, DEFAULT_FONT_PT);

        let long = SubtitleSettings { spoken: "e".repeat(100), microphone: Some("m".repeat(1000)), ..Default::default() };
        let s = long.sanitized();
        assert_eq!(s.spoken, AUTO_LANGUAGE);
        assert_eq!(s.microphone.as_deref().map(str::len), Some(200));
    }

    #[test]
    fn language_tokens_are_kept_short_and_plain() {
        let s = SubtitleSettings { spoken: "PT-BR".into(), subtitle: " ZH ".into(), ..Default::default() };
        let s = s.sanitized();
        assert_eq!((s.spoken.as_str(), s.subtitle.as_str()), ("pt-br", "zh"));
        assert_eq!(s.language_hint(), Some("pt-br"));
        let auto = SubtitleSettings { spoken: AUTO_LANGUAGE.into(), ..Default::default() };
        assert_eq!(auto.language_hint(), None);
        let a = SubtitleSettings { on: true, spoken: "en".into(), microphone: Some("mic".into()), ..Default::default() };
        let b = SubtitleSettings { position: Position::TopOverlay, font_pt: 40.0, ..a.clone() };
        assert!(a.provider_equivalent(&b), "cosmetic changes keep the stream");
        assert!(!a.provider_equivalent(&SubtitleSettings { spoken: "fr".into(), ..b.clone() }));
        assert!(!a.provider_equivalent(&SubtitleSettings { microphone: None, ..b.clone() }));
        assert!(!a.provider_equivalent(&SubtitleSettings { on: false, ..b }));
        // A hand-edited or newer accelerator spelling must not cost the rest of the settings.
        let lenient: SubtitleSettings = serde_json::from_str(r#"{"spoken":"fr","accelerator":"npu-3000"}"#).expect("lenient accelerator");
        assert_eq!(lenient.spoken, "fr");
        assert_eq!(lenient.accelerator, Accelerator::Cpu, "an unknown accelerator is the default");
        let spelled: SubtitleSettings = serde_json::from_str(r#"{"accelerator":"OpenVINO"}"#).expect("a lenient spelling is read");
        assert_eq!(spelled.accelerator, Accelerator::OpenVino, "a known accelerator spelled differently is kept");
        assert!(SPOKEN_LANGUAGES.iter().any(|(t, _)| *t == AUTO_LANGUAGE));
        assert!(SUBTITLE_LANGUAGES.iter().all(|(t, _)| *t != AUTO_LANGUAGE));
        // `auto` is a spoken-language choice only: a subtitle language falls back, so a command or
        // a hand-edited prefs.json cannot store a value the dialog never offers.
        let auto_subtitle = SubtitleSettings { subtitle: AUTO_LANGUAGE.into(), ..Default::default() }.sanitized();
        assert_eq!(auto_subtitle.subtitle, "en");
    }
}
