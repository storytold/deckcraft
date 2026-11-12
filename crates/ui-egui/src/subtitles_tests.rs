//! Live captions: the host state machine, exercised with the deterministic fake provider.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_asr::{Event, FakeTranscriber, ModelState, Position, ProviderState, SubtitleSettings, Transcriber};

use crate::subtitles::Subtitles;

/// A unique, empty temp path per call: the tests run in parallel, and two of them sharing a
/// path would race on `remove_dir_all`.
fn temp_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("deckcraft-subtitles-{}-{name}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

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
    let dir = temp_dir("missing");
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
fn a_check_that_cannot_start_reports_instead_of_hashing_on_the_ui_thread() {
    let dir = temp_dir("present");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    for name in ["model.safetensors", "config.json", "tokenizer.json"] {
        std::fs::write(dir.join(name), b"not the model").expect("write");
    }
    let mut subs = Subtitles::default();
    subs.adopt_dir(dir.clone());
    assert_eq!(subs.presence_state(), ModelState::Ready, "presence is reported at once");
    // The worker thread could not start: keep the presence state and say so instead of
    // hashing 151 MB on the UI thread.
    subs.verify_unavailable();
    assert!(!subs.is_verifying(), "no worker means no repaint wait");
    assert_eq!(subs.presence_state(), ModelState::Ready, "the presence-only state is kept, not hashed here");
    assert!(subs.verify_error.as_deref().is_some_and(|e| e.contains("background check")), "the dialog names the failure: {:?}", subs.verify_error);
    let _ = std::fs::remove_dir_all(&dir);
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
fn adopting_a_folder_verifies_it_off_the_ui_thread() {
    let dir = temp_dir("adopt");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    for name in ["model.safetensors", "config.json", "tokenizer.json"] {
        std::fs::write(dir.join(name), b"not the model").expect("write");
    }
    let settings = SubtitleSettings::default();
    let mut subs = Subtitles::default();
    subs.adopt_dir(dir.clone());
    // Read without polling: the worker may finish at any moment, and this asserts what `adopt_dir`
    // reports before it does.
    assert_eq!(subs.presence_state(), ModelState::Ready, "the presence check reports without waiting for the hash");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        subs.sync(&settings, false);
        if subs.model().state != ModelState::Ready {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(subs.model().state, ModelState::Corrupt, "the full check still runs, and reports the wrong contents");
    let _ = std::fs::remove_dir_all(&dir);
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
    temp_dir("restart")
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
    assert!(subs.caption.is_none(), "the old stream's line does not outlive the restart");
}

#[test]
fn stopping_reports_stopped_and_a_failure_survives() {
    let settings = on(SubtitleSettings::default());
    let mut subs = Subtitles::default();
    subs.install(Box::new(FakeTranscriber::scripted(&["a"])), &settings);
    subs.sync(&settings, true);
    assert_eq!(subs.status().state, ProviderState::Running);
    subs.stop();
    assert_eq!(subs.status().state, ProviderState::Stopped, "the engine is not told captions are still live");

    let mut failed = Subtitles::default();
    failed.install(Box::new(FakeTranscriber::failing("no microphone")), &settings);
    failed.sync(&settings, true);
    assert_eq!(failed.status().state, ProviderState::Failed);
    failed.stop();
    assert_eq!(failed.status().state, ProviderState::Failed, "the failure stays visible after a stop");
}

#[test]
fn a_caption_is_drawn_only_when_the_show_can_show_it() {
    use crate::subtitles::caption_line;
    assert_eq!(caption_line(true, false, false, false, Some("hello")), Some("hello"));
    assert_eq!(caption_line(false, false, false, false, Some("hello")), None, "captions are off");
    assert_eq!(caption_line(true, true, false, false, Some("hello")), None, "the show ended");
    assert_eq!(caption_line(true, false, true, false, Some("hello")), None, "the grid is up");
    assert_eq!(caption_line(true, false, false, true, Some("hello")), None, "the screen is blank");
    assert_eq!(caption_line(true, false, false, false, None), None);
    assert_eq!(caption_line(true, false, false, false, Some("   ")), None, "whitespace is not a caption");
    assert_eq!(caption_line(true, false, false, false, Some("  padded  ")), Some("padded"));
}

#[test]
fn the_caption_layout_is_cached_until_its_key_changes() {
    use crate::subtitles::CaptionCache;
    use deckcraft_asr::caption::{self, LayoutInput};
    let screen = caption::Rect::new(0.0, 0.0, 1280.0, 720.0);
    let slide = caption::Rect::new(80.0, 45.0, 1120.0, 630.0);
    let input = LayoutInput::new(screen, slide, Position::BottomOverlay, "one two three four five six seven eight nine ten");
    let mut cache = CaptionCache::default();
    let first = cache.layout(&input).clone();
    let second = cache.layout(&input).clone();
    assert_eq!(first, second);
    let mut changed = input.clone();
    changed.text = "other words that wrap differently and then some more words";
    let third = cache.layout(&changed).clone();
    assert_ne!(first.lines, third.lines, "a changed transcript is laid out again");
    let mut moved = input.clone();
    moved.position = Position::TopOverlay;
    let fourth = cache.layout(&moved).clone();
    assert_eq!(fourth.position, Position::TopOverlay);
}

#[test]
fn a_short_screen_shows_only_the_lines_that_fit() {
    use deckcraft_asr::caption::{self, LayoutInput};
    let screen = caption::Rect::new(0.0, 0.0, 320.0, 120.0);
    let slide = caption::Rect::new(10.0, 10.0, 300.0, 100.0);
    let mut input = LayoutInput::new(screen, slide, Position::BottomOverlay, "a b c d e f g h i j k l m n o p q r s t");
    input.font_pt = deckcraft_asr::caption::MAX_FONT_PT;
    input.max_lines = deckcraft_asr::caption::MAX_LINES_CAP;
    let laid = caption::layout(&input);
    let fit = crate::subtitles::visible_lines(&laid);
    assert!(fit <= laid.lines.len());
    assert!(fit as f64 * laid.line_height <= laid.rect.h + 0.001, "the drawn lines fit the capped box");
}

#[test]
fn the_microphone_list_is_enumerated_once() {
    let mut subs = Subtitles::default();
    let first: *const String = subs.microphones().as_ptr();
    let second: *const String = subs.microphones().as_ptr();
    assert_eq!(first, second, "the second frame reuses the cached list");
}
