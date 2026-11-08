//! Live captions: the host state machine, exercised with the deterministic fake provider.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_asr::{Event, FakeTranscriber, Position, ProviderState, SubtitleSettings, Transcriber};

use crate::subtitles::Subtitles;

fn on(settings: SubtitleSettings) -> SubtitleSettings {
    SubtitleSettings { on: true, ..settings }
}

#[test]
fn partial_then_final_lines_become_the_caption() {
    let settings = on(SubtitleSettings { position: Position::BottomOverlay, ..Default::default() });
    let mut subs = Subtitles::default();
    subs.install(Box::new(FakeTranscriber::scripted(&["hello"])), &settings);
    assert!(subs.caption.is_none(), "nothing is drawn before the first line");
    subs.sync(&settings, true);
    assert_eq!(subs.caption.as_deref(), Some("hello"));
    subs.sync(&settings, true);
    assert_eq!(subs.caption.as_deref(), Some("hello"), "a final line stays until the next one");
    assert!(subs.is_running());
}

#[test]
fn a_provider_failure_is_reported_once_and_leaves_the_show_alone() {
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.install(Box::new(FakeTranscriber::failing("model hash mismatch")), &settings);
    let message = subs.sync(&settings, true).expect("one report");
    assert!(message.contains("model hash mismatch"), "{message}");
    assert_eq!(subs.status().state, ProviderState::Failed);
    assert!(!subs.is_running(), "a failed provider is dropped");
    // The show keeps running: the same settings do not restart and do not report again.
    assert_eq!(subs.sync(&settings, true), None);
    assert!(subs.caption.is_none());
    // Changing the settings retries, as the user asked.
    let other = on(SubtitleSettings { spoken: "es".into(), ..Default::default() });
    subs.install(Box::new(FakeTranscriber::scripted(&["hola"])), &other);
    subs.sync(&other, true);
    assert_eq!(subs.caption.as_deref(), Some("hola"));
}

#[test]
fn ending_the_show_stops_the_stream_and_takes_the_caption_with_it() {
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.install(Box::new(FakeTranscriber::scripted(&["a line"])), &settings);
    subs.sync(&settings, true);
    assert!(subs.caption.is_some());
    subs.sync(&settings, false);
    assert!(subs.caption.is_none(), "the caption does not outlive the show");
    assert!(!subs.is_running(), "no thread and no microphone are left");
    assert_eq!(subs.status().state, ProviderState::Stopped);
    // Calling it again is harmless (the show was already closed).
    subs.sync(&settings, false);
    assert!(!subs.is_running());
}

#[test]
fn turning_captions_off_clears_the_caption_and_stops() {
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.install(Box::new(FakeTranscriber::scripted(&["a line"])), &settings);
    subs.sync(&settings, true);
    let off = SubtitleSettings { on: false, ..settings.clone() };
    subs.sync(&off, true);
    assert!(subs.caption.is_none());
    assert!(!subs.is_running());
    assert_eq!(subs.status().state, ProviderState::Idle);
    // Turning them back on starts a fresh stream (the same settings may run again).
    subs.install(Box::new(FakeTranscriber::scripted(&["again"])), &settings);
    subs.sync(&settings, true);
    assert_eq!(subs.caption.as_deref(), Some("again"));
}

#[test]
fn a_missing_model_is_reported_once_and_does_not_restart_each_frame() {
    let dir = std::env::temp_dir().join(format!("deckcraft-subtitles-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.set_model_dir(dir);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let first = subs.sync(&settings, true).expect("a missing model is reported");
        assert!(first.contains("not installed"), "{first}");
        assert_eq!(subs.sync(&settings, true), None, "the failure is not repeated every frame");
        assert!(!subs.is_running());
    }
    #[cfg(target_arch = "wasm32")]
    {
        assert!(subs.sync(&settings, true).is_some());
    }
}

#[test]
fn cosmetic_settings_do_not_restart_the_stream() {
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.install(Box::new(FakeTranscriber::scripted(&["steady"])), &settings);
    subs.sync(&settings, true);
    assert!(subs.is_running());
    let moved = on(SubtitleSettings { position: Position::TopOverlay, font_pt: 40.0, ..settings.clone() });
    subs.sync(&moved, true);
    assert!(subs.is_running(), "moving the caption keeps the audio");
    assert_eq!(subs.caption.as_deref(), Some("steady"));
}

#[test]
fn the_fake_provider_reports_lines_through_the_host() {
    let mut fake = FakeTranscriber::scripted(&["one", "two"]);
    assert!(fake.start().is_ok());
    assert_eq!(fake.poll(), vec![Event::Partial { text: "one".into() }]);
    fake.stop();
    assert!(fake.poll().is_empty());
}

#[test]
fn a_provider_that_stops_on_its_own_takes_the_caption_with_it() {
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.install(Box::new(FakeTranscriber::new(vec![Event::partial("half a line"), Event::Stopped])), &settings);
    subs.sync(&settings, true);
    assert_eq!(subs.caption.as_deref(), Some("half a line"));
    subs.sync(&settings, true);
    assert!(subs.caption.is_none(), "a stopped provider leaves no live-looking line on screen");
    assert_eq!(subs.status().state, ProviderState::Stopped);
    assert!(!subs.is_running());
}

#[cfg(not(target_arch = "wasm32"))]
fn missing_model_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-subtitles-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_second_show_starts_again_after_the_first_one_ended() {
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.set_model_dir(missing_model_dir());
    let first = subs.sync(&settings, true).expect("a missing model is reported");
    assert!(first.contains("not installed"), "{first}");
    subs.sync(&settings, false);
    // Ending a show must not remember that captions "already started": the next one tries again.
    let second = subs.sync(&settings, true).expect("the next show reports again");
    assert!(second.contains("not installed"), "{second}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_settings_change_restarts_a_running_stream() {
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.set_model_dir(missing_model_dir());
    subs.install(Box::new(FakeTranscriber::scripted(&["one"])), &settings);
    subs.sync(&settings, true);
    assert!(subs.is_running());
    // The model vanished after the provider started; a spoken-language change must still stop the
    // old stream and attempt the new settings instead of keeping the stale provider.
    let changed = on(SubtitleSettings { spoken: "es".into(), ..settings.clone() });
    let message = subs.sync(&changed, true).expect("the restart is reported");
    assert!(message.contains("not installed"), "{message}");
    assert!(!subs.is_running(), "the provider started with the old settings was stopped");
}
