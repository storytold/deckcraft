//! The speech-to-text protocol between the slide show and a native engine.
//!
//! A [`Transcriber`] owns its worker (thread, microphone, model). The UI never blocks on it:
//! [`Transcriber::poll`] takes whatever lines arrived since the last frame, and [`Event`] tells
//! a partial line (the text still changing) from a final one (replace the partial).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::accelerator::{Accelerator, Capabilities};

/// Why a provider could not be started or stopped working.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsrError {
    message: String,
}

impl AsrError {
    pub fn new(message: impl Into<String>) -> Self {
        let mut message = message.into();
        if message.chars().count() > 400 {
            message = message.chars().take(400).collect();
        }
        AsrError { message }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for AsrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AsrError {}

/// The provider's lifecycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderState {
    #[default]
    Idle,
    Starting,
    Running,
    Failed,
    Stopped,
}

impl ProviderState {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderState::Idle => "idle",
            ProviderState::Starting => "starting",
            ProviderState::Running => "running",
            ProviderState::Failed => "failed",
            ProviderState::Stopped => "stopped",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, ProviderState::Starting | ProviderState::Running)
    }
}

/// What the host reports to the engine (`Session::subtitle_status`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub state: ProviderState,
    /// Why the provider is not running: a missing model, no microphone, a load error.
    pub detail: Option<String>,
    /// The model id in use (`whisper-tiny`), when one loaded.
    pub model: Option<String>,
    /// The language hint handed to the engine.
    pub language: Option<String>,
    /// The capture device in use, when the host can name it.
    pub microphone: Option<String>,
    /// What the backend is running on (`cpu` today; an optional accelerator reports itself here).
    pub accelerator: Option<Accelerator>,
    /// Every accelerator this build can use here, and why a request fell back.
    pub capabilities: Option<Capabilities>,
}

impl ProviderStatus {
    pub fn idle() -> Self {
        ProviderStatus::default()
    }

    pub fn running(model: impl Into<String>, language: Option<String>) -> Self {
        ProviderStatus {
            state: ProviderState::Running,
            detail: None,
            model: Some(model.into()),
            language,
            microphone: None,
            accelerator: Some(Accelerator::Cpu),
            capabilities: None,
        }
    }

    pub fn starting(model: impl Into<String>, language: Option<String>) -> Self {
        ProviderStatus { state: ProviderState::Starting, ..ProviderStatus::running(model, language) }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        ProviderStatus { state: ProviderState::Failed, detail: Some(AsrError::new(message).message), ..ProviderStatus::default() }
    }

    pub fn stopped() -> Self {
        ProviderStatus { state: ProviderState::Stopped, ..ProviderStatus::default() }
    }

    /// A one-line description for a status message or `show.subtitles.state`.
    pub fn summary(&self) -> String {
        match (&self.state, &self.detail) {
            (ProviderState::Failed, Some(detail)) => format!("Subtitles unavailable: {detail}"),
            (ProviderState::Failed, None) => "Subtitles unavailable".to_string(),
            (state, _) => match state {
                ProviderState::Idle => "Subtitles idle".to_string(),
                ProviderState::Starting => "Starting subtitles…".to_string(),
                ProviderState::Running => "Subtitles running".to_string(),
                ProviderState::Stopped => "Subtitles stopped".to_string(),
                ProviderState::Failed => "Subtitles unavailable".to_string(),
            },
        }
    }
}

/// What a transcript reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A line still being recognised: show it in place of the current partial line.
    Partial { text: String },
    /// A recognised line: it stays until the next final line replaces it.
    Final { text: String },
    /// The provider failed after starting. The host reports this once and stops the stream.
    Error { message: String },
    /// The stream ended (stop requested, or the capture device went away).
    Stopped,
}

impl Event {
    /// The longest line a transcript may carry: a runaway decode cannot grow the caption
    /// indefinitely.
    pub const MAX_LINE_CHARS: usize = 500;

    pub fn partial(text: impl Into<String>) -> Event {
        Event::Partial { text: clip(text) }
    }

    pub fn final_line(text: impl Into<String>) -> Event {
        Event::Final { text: clip(text) }
    }

    pub fn error(message: impl Into<String>) -> Event {
        Event::Error { message: AsrError::new(message).message }
    }

    /// The line's text, or empty for a status event.
    pub fn text(&self) -> &str {
        match self {
            Event::Partial { text } | Event::Final { text } => text,
            _ => "",
        }
    }

    pub fn is_final(&self) -> bool {
        matches!(self, Event::Final { .. })
    }
}

fn clip(text: impl Into<String>) -> String {
    let text = clean(text.into());
    let count = text.chars().count();
    // Captions are read while they are still being spoken: an overlong transcript keeps its end,
    // the newest words, as the layout does with `wrap_tail`.
    if count > Event::MAX_LINE_CHARS { text.chars().skip(count - Event::MAX_LINE_CHARS).collect() } else { text }
}

/// Make a transcript line safe to draw: a model can echo control characters, a bidirectional
/// override or a zero-width joiner from the audio, and drawing them reorders the caption or hides
/// text. Line breaks and tabs become an ordinary space; everything else that is not printable
/// text is dropped.
fn clean(text: String) -> String {
    text.chars().map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c }).filter(|c| !c.is_control() && !is_invisible_format(*c)).collect()
}

/// Bidi controls and zero-width characters that a caption must never contain.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD | 0x061C | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xFEFF
    )
}

/// A speech engine the slide show can start, stop and poll.
pub trait Transcriber: Send {
    /// Begin listening. Return `Err` when the provider can't start at all (no model, no
    /// microphone); failures after this arrive as [`Event::Error`].
    fn start(&mut self) -> Result<(), AsrError>;
    /// Stop listening and release the microphone and model. Idempotent, and never blocks longer
    /// than the current window's inference.
    fn stop(&mut self);
    fn status(&self) -> ProviderStatus;
    /// Take the events that arrived since the last call. This never blocks.
    fn poll(&mut self) -> Vec<Event>;
}

/// A deterministic provider for tests and for hosts without audio capture: it plays a script of
/// events, one poll at a time.
#[derive(Debug)]
pub struct FakeTranscriber {
    status: ProviderStatus,
    script: VecDeque<Event>,
    start_error: Option<String>,
    started: bool,
    polls: usize,
}

impl FakeTranscriber {
    pub fn new(script: Vec<Event>) -> Self {
        FakeTranscriber { status: ProviderStatus::idle(), script: script.into(), start_error: None, started: false, polls: 0 }
    }

    /// One partial line per final line: `line` then `line` final, in order.
    pub fn scripted(lines: &[&str]) -> Self {
        let mut script = Vec::new();
        for line in lines {
            script.push(Event::partial(*line));
            script.push(Event::final_line(*line));
        }
        FakeTranscriber::new(script)
    }

    /// A provider that refuses to start with `message`.
    pub fn refusing(message: &str) -> Self {
        FakeTranscriber { start_error: Some(message.to_string()), ..FakeTranscriber::new(vec![]) }
    }

    /// A provider that starts, then fails with `message`.
    pub fn failing(message: &str) -> Self {
        FakeTranscriber::new(vec![Event::error(message)])
    }

    pub fn polls(&self) -> usize {
        self.polls
    }
}

impl Transcriber for FakeTranscriber {
    fn start(&mut self) -> Result<(), AsrError> {
        if let Some(message) = self.start_error.clone() {
            self.status = ProviderStatus::failed(message.clone());
            return Err(AsrError::new(message));
        }
        self.started = true;
        self.status = ProviderStatus::running("fake", Some("en".into()));
        Ok(())
    }

    fn stop(&mut self) {
        self.started = false;
        self.script.clear();
        // Production keeps a failure visible after a stop (the host reports it, the user sees
        // why); the fake must not pretend the stream ended cleanly.
        if self.status.state != ProviderState::Failed {
            self.status = ProviderStatus::stopped();
        }
    }

    fn status(&self) -> ProviderStatus {
        self.status.clone()
    }

    fn poll(&mut self) -> Vec<Event> {
        self.polls += 1;
        if !self.started {
            return vec![];
        }
        match self.script.pop_front() {
            Some(Event::Error { message }) => {
                self.status = ProviderStatus::failed(message.clone());
                vec![Event::Error { message }]
            }
            Some(event) => vec![event],
            None => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scripted_provider_reports_partial_then_final_lines() {
        let mut t = FakeTranscriber::scripted(&["hello", "world"]);
        assert_eq!(t.status().state, ProviderState::Idle);
        assert!(t.start().is_ok());
        assert_eq!(t.status().state, ProviderState::Running);
        assert_eq!(t.poll(), vec![Event::Partial { text: "hello".into() }]);
        assert_eq!(t.poll(), vec![Event::Final { text: "hello".into() }]);
        assert_eq!(t.poll(), vec![Event::Partial { text: "world".into() }]);
        assert_eq!(t.poll(), vec![Event::Final { text: "world".into() }]);
        assert!(t.poll().is_empty());
        assert_eq!(t.polls(), 5);
        t.stop();
        assert_eq!(t.status().state, ProviderState::Stopped);
        assert!(t.poll().is_empty(), "a stopped provider reports nothing");
    }

    #[test]
    fn refusing_to_start_is_an_error_and_a_failed_status() {
        let mut t = FakeTranscriber::refusing("no microphone");
        assert_eq!(t.start(), Err(AsrError::new("no microphone")));
        assert_eq!(t.status().state, ProviderState::Failed);
        assert!(t.status().summary().contains("no microphone"));
        t.stop();
        assert_eq!(t.status().state, ProviderState::Failed, "a failed stream stays failed after a stop");
    }

    #[test]
    fn a_failed_stream_stops_as_failed_and_a_clean_one_as_stopped() {
        let mut failed = FakeTranscriber::failing("model hash mismatch");
        assert!(failed.start().is_ok());
        assert!(matches!(failed.poll().first(), Some(Event::Error { .. })), "the stream failed before the stop");
        failed.stop();
        assert_eq!(failed.status().state, ProviderState::Failed, "the fake matches the production stop");
        let mut clean = FakeTranscriber::scripted(&["x"]);
        clean.start().unwrap_or(());
        clean.stop();
        assert_eq!(clean.status().state, ProviderState::Stopped);
    }

    #[test]
    fn transcript_text_is_stripped_of_controls_and_bidi_overrides() {
        let hostile = "he\u{202E}llo\u{0} wor\u{202B}ld\u{200B}!";
        let e = Event::final_line(hostile);
        assert_eq!(e.text(), "hello world!");
        assert_eq!(Event::partial("two\nlines\there").text(), "two lines here");
        assert_eq!(Event::partial("a\u{7F}b").text(), "ab");
        // Ordinary text, including non-Latin scripts, is untouched.
        let text = Event::final_line("¿Qué tal? Привет 日本語").text().to_string();
        assert_eq!(text, "¿Qué tal? Привет 日本語");
    }

    #[test]
    fn a_failure_after_start_arrives_once_as_an_error_event() {
        let mut t = FakeTranscriber::failing("model hash mismatch");
        t.start().unwrap_or(());
        assert_eq!(t.poll(), vec![Event::Error { message: "model hash mismatch".into() }]);
        assert_eq!(t.status().state, ProviderState::Failed);
        assert!(t.poll().is_empty(), "the error is reported once");
    }

    #[test]
    fn lines_are_bounded() {
        let long = "x".repeat(Event::MAX_LINE_CHARS * 3);
        let e = Event::final_line(long);
        assert_eq!(e.text().chars().count(), Event::MAX_LINE_CHARS);
        let overlong = "old ".repeat(200) + "newest";
        assert!(Event::partial(&overlong).text().ends_with("newest"), "the newest words are kept");
        assert!(e.is_final());
        assert!(!Event::partial("a").is_final());
        assert_eq!(Event::Stopped.text(), "");
        let message = AsrError::new("y".repeat(1000));
        assert_eq!(message.message().chars().count(), 400);
    }

    #[test]
    fn status_states_have_names_and_activity() {
        assert_eq!(ProviderState::Running.as_str(), "running");
        assert!(ProviderState::Starting.is_active() && !ProviderState::Idle.is_active());
        assert_eq!(ProviderStatus::failed("x").summary(), "Subtitles unavailable: x");
        let json = serde_json::to_string(&ProviderStatus::running("whisper-tiny", Some("en".into()))).unwrap_or_default();
        assert!(json.contains("\"state\":\"running\""));
        assert!(json.contains("\"model\":\"whisper-tiny\""));
    }
}
