//! Record ▸ Audio: synthetic PCM → WAV, take validation, the microphone error path and the
//! insert/undo round trip through `insert.audio`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::f64::consts::TAU;

use deckcraft_engine::Session;
use deckcraft_media::MicIn;
use deckcraft_model::ShapeKind;
use serde_json::json;

use crate::{Services, SlideApp};

use crate::record::{MAX_SECONDS, MAX_WAV_BYTES, MIN_SECONDS, Recorder, check, elapsed_text, encode, max_samples};

/// A microphone whose capture is supplied up front, so tests never touch a device.
struct FakeMic {
    rate: u32,
    samples: Vec<f32>,
    started: bool,
    fail: Option<String>,
}

impl FakeMic {
    fn ok(rate: u32, samples: Vec<f32>) -> Self {
        FakeMic { rate, samples, started: false, fail: None }
    }
    fn failing(message: &str) -> Self {
        FakeMic { rate: 0, samples: Vec::new(), started: false, fail: Some(message.into()) }
    }
}

impl MicIn for FakeMic {
    fn start(&mut self) -> Result<(), String> {
        if let Some(e) = &self.fail {
            return Err(e.clone());
        }
        self.started = true;
        Ok(())
    }
    fn take(&mut self) -> (u32, Vec<f32>) {
        if !self.started {
            return (0, Vec::new());
        }
        (self.rate, std::mem::take(&mut self.samples))
    }
    fn captured(&self) -> usize {
        self.samples.len()
    }
    fn stop(&mut self) {
        self.started = false;
    }
}

fn tone(rate: u32, secs: f64) -> Vec<f32> {
    let n = (rate as f64 * secs) as usize;
    (0..n).map(|i| (TAU * 440.0 * i as f64 / rate as f64).sin() as f32 * 0.5).collect()
}

#[test]
fn synthetic_pcm_becomes_a_mono_16_bit_wav() {
    let rate = 48_000;
    let samples = tone(rate, 0.5);
    let wav = encode(rate, &samples).expect("encode");
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1, "PCM");
    assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1, "mono");
    assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), rate);
    assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16, "16-bit");
    assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize, samples.len() * 2);
    let info = deckcraft_media::probe(&std::sync::Arc::new(wav)).expect("probe");
    assert_eq!(info.container, "wav");
    assert!((info.duration() - 0.5).abs() < 1e-3, "duration {}", info.duration());
    assert!(info.playable());
    let audio = info.audio.expect("audio");
    assert_eq!((audio.channels, audio.sample_rate), (1, rate));
}

#[test]
fn a_take_shorter_than_the_minimum_is_refused() {
    let too_short = vec![0.0; (8_000.0 * (MIN_SECONDS - 0.05)) as usize];
    assert!(encode(8_000, &too_short).is_err());
    // Exactly the minimum is accepted.
    let shortest = vec![0.0; (8_000.0 * MIN_SECONDS) as usize];
    let wav = encode(8_000, &shortest).expect("minimum take");
    assert_eq!(deckcraft_media::probe(&std::sync::Arc::new(wav)).expect("probe").audio.expect("audio").channels, 1);
    assert!(check(8_000, 0).is_err(), "an empty take is refused");
    assert!(check(0, 8_000).is_err(), "a take with no rate is refused");
}

#[test]
fn a_take_is_capped_by_duration_and_size() {
    // At 8 kHz the ten-minute limit is smaller than the byte budget.
    assert_eq!(max_samples(8_000), (8_000.0 * MAX_SECONDS) as usize);
    assert!(check(8_000, max_samples(8_000) + 1).is_err());
    // 16-bit mono at 192 kHz is 384 kB/s, so the 64 MiB byte budget bites before ten minutes.
    assert_eq!(max_samples(192_000), (MAX_WAV_BYTES - 44) / 2);
    assert!(max_samples(192_000) < 192_000 * MAX_SECONDS as usize, "the byte budget is smaller than ten minutes at this rate");
    assert!(check(192_000, (MAX_WAV_BYTES - 44) / 2 + 1).is_err());
    assert!(check(192_000, max_samples(192_000)).is_ok());
    assert_eq!(elapsed_text(0.0), "0:00.0");
    assert_eq!(elapsed_text(65.34), "1:05.3");
    assert_eq!(elapsed_text(f64::NAN), "0:00.0");
}

#[test]
fn a_device_or_permission_error_leaves_the_recorder_idle() {
    let mut rec = Recorder::default();
    let mut mic = FakeMic::failing("permission denied");
    let e = rec.start(Some(&mut mic), 0.0, crate::FileTarget::Absent).expect_err("start must fail");
    assert!(e.contains("permission denied"), "{e}");
    assert!(!rec.is_recording());
    assert!(rec.start(None, 0.0, crate::FileTarget::Absent).is_err(), "no microphone is reported, not a panic");
    let mut mic = FakeMic::ok(8_000, tone(8_000, 0.5));
    rec.start(Some(&mut mic), 0.0, crate::FileTarget::Absent).expect("start");
    assert!(rec.start(Some(&mut mic), 0.0, crate::FileTarget::Absent).is_err(), "a second take is refused");
    rec.cancel(Some(&mut mic));
    assert!(!rec.is_recording());
}

#[test]
fn start_needs_a_presentation_and_a_microphone() {
    let mut app = SlideApp::new(Session::new(), Services::default());
    assert!(app.start_audio_recording().is_err(), "no presentation");
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    let mut app = SlideApp::new(s, Services::default());
    let e = app.start_audio_recording().expect_err("no microphone");
    assert!(e.contains("isn't available"), "{e}");
    assert!(!app.audio_recording());
}

#[test]
fn recording_inserts_one_undoable_audio_shape() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    let before = s.doc().expect("doc").doc.slides.iter().map(|sl| sl.shapes.len()).sum::<usize>();
    // Silence: a valid take, and it must not be mistaken for "no device".
    let mic = FakeMic::ok(8_000, vec![0.0; (8_000.0 * 0.5) as usize]);
    let mut app = SlideApp::new(s, Services { mic: Some(Box::new(mic)), ..Default::default() });
    app.start_audio_recording().expect("start");
    app.tick_audio_recording();
    app.stop_audio_recording().expect("stop");

    let d = app.session.active().expect("doc");
    let media: Vec<_> = d.doc.slides.iter().flat_map(|sl| sl.shapes.iter()).filter(|sh| matches!(sh.kind, ShapeKind::Media(_))).collect();
    assert_eq!(media.len(), 1, "one audio shape");
    assert!(matches!(media[0].kind, ShapeKind::Media(_)), "not media");
    let ShapeKind::Media(clip) = &media[0].kind else { return };
    assert!(!clip.video);
    assert!(clip.duration_ms >= 400, "duration {}", clip.duration_ms);
    let item = d.doc.media(clip.media).expect("media item");
    assert_eq!(item.name, "Recording 1.wav");
    assert!(item.data.starts_with(b"RIFF"));
    let info = deckcraft_media::probe(&item.data).expect("probe");
    assert!(info.playable() && info.audio.is_some());
    assert_eq!(d.selection.shapes, vec![media[0].id], "the new clip is selected");

    app.session.execute("edit.undo", &json!({})).expect("undo");
    let after = app.session.active().expect("doc").doc.slides.iter().map(|sl| sl.shapes.len()).sum::<usize>();
    assert_eq!(after, before, "one undo removes the recording");
}

/// A microphone that opens and then loses its device, as an unplugged one does: `start` succeeds
/// and `failure()` reports why capture stopped.
struct LostMic;

impl MicIn for LostMic {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn take(&mut self) -> (u32, Vec<f32>) {
        (8_000, tone(8_000, 0.5))
    }
    fn failure(&self) -> Option<String> {
        Some("the device was unplugged".into())
    }
    fn stop(&mut self) {}
}

#[test]
fn a_microphone_lost_mid_take_cancels_the_recording_and_says_why() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    let mut app = SlideApp::new(s, Services { mic: Some(Box::new(LostMic)), ..Default::default() });
    app.start_audio_recording().expect("start");
    assert!(app.record.is_recording(), "the take runs");
    app.tick_audio_recording();
    assert!(!app.record.is_recording(), "the take is cancelled, not left counting");
    let status = app.status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(status.contains("The microphone stopped") && status.contains("unplugged"), "{status}");
    let toast = app.toast.as_ref().expect("a notice the user must see");
    assert!(toast.sticky, "the notice stays until it is closed");
    assert!(toast.text.contains("unplugged"), "{}", toast.text);
}

#[test]
fn a_sticky_microphone_notice_is_not_replaced_by_a_transient_one() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    let mut app = SlideApp::new(s, Services { mic: Some(Box::new(LostMic)), ..Default::default() });
    app.start_audio_recording().expect("start");
    app.tick_audio_recording();
    let before = app.toast.as_ref().map(|t| t.text.clone()).expect("the failure notice");
    app.notify("Saved");
    assert_eq!(app.toast.as_ref().map(|t| t.text.clone()), Some(before), "the failure stays until closed");
    assert_eq!(app.toast_queue, vec!["Saved".to_string()], "the later notice waits instead of being lost");
    // Closing the failure shows the notice that waited.
    app.toast = None;
    crate::toast::show_queued(&mut app);
    assert_eq!(app.toast.as_ref().map(|t| t.text.clone()), Some("Saved".to_string()));
}

/// The take outlives the slide it started on: closing that presentation while recording stops the
/// microphone and inserts the audio on the slide in view instead of losing it.
#[test]
fn a_take_survives_the_presentation_it_started_in_closing() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    s.execute("file.new", &json!({})).expect("a second presentation");
    let mic = FakeMic::ok(8_000, vec![0.0; 4_000]);
    let mut app = SlideApp::new(s, Services { mic: Some(Box::new(mic)), ..Default::default() });
    app.start_audio_recording().expect("start");
    app.tick_audio_recording();
    app.session.execute("file.close", &json!({})).expect("close the presentation the take was made for");
    app.stop_audio_recording().expect("the take is kept");
    let d = app.session.active().expect("doc");
    assert!(
        d.doc.slides.iter().flat_map(|sl| sl.shapes.iter()).any(|sh| matches!(sh.kind, ShapeKind::Media(_))),
        "the audio lands on the slide in view"
    );
}
