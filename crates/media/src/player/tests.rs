use super::*;

fn spec(duration: f64, looping: bool) -> VoiceSpec {
    VoiceSpec { clip: ClipParams::default(), looping, duration, audio: false }
}

#[test]
fn wall_clock_advances_pauses_and_seeks() {
    let mut p = Player::new(None);
    let b: Bytes = Arc::new(vec![]);
    p.play(1, 1, &b, spec(2.0, false), None);
    p.tick(10.0);
    p.tick(10.2);
    p.tick(10.4);
    let s = p.status(1).map(|s| s.position).unwrap_or(-1.0);
    assert!((s - 0.4).abs() < 1e-3, "{s}");
    p.pause(1);
    p.tick(10.6);
    assert!((p.status(1).map(|s| s.position).unwrap_or(-1.0) - 0.4).abs() < 1e-3);
    assert_eq!(p.status(1).map(|s| s.state), Some(PlayState::Paused));
    p.seek(1, 1.5);
    p.play(1, 1, &b, spec(2.0, false), None);
    p.tick(10.8);
    p.tick(11.0);
    p.tick(11.2);
    p.tick(11.4);
    let st = p.status(1);
    assert_eq!(st.as_ref().map(|s| s.state), Some(PlayState::Ended));
    assert_eq!(st.map(|s| s.position), Some(2.0));
    // Playing an ended voice starts over.
    p.play(1, 1, &b, spec(2.0, false), None);
    assert_eq!(p.status(1).map(|s| s.position), Some(0.0));
}

#[test]
fn looping_wraps_within_the_trimmed_range() {
    let mut p = Player::new(None);
    let b: Bytes = Arc::new(vec![]);
    let mut s = spec(1.0, true);
    s.clip.trim_start = 0.5;
    p.play(7, 7, &b, s, None);
    p.tick(0.0);
    for i in 1..=4 {
        p.tick(i as f64 * 0.2);
    }
    // 0.8 s played in a 0.5 s loop starting at 0.5: 0.5 + (0.8 mod 0.5) = 0.8.
    let pos = p.status(7).map(|s| s.position).unwrap_or(-1.0);
    assert!((pos - 0.8).abs() < 1e-3, "{pos}");
    assert_eq!(p.status(7).map(|s| s.state), Some(PlayState::Playing));
}

#[test]
fn mixer_applies_volume_fade_and_channel_mapping() {
    let pcm = Arc::new(Pcm { rate: 10, channels: 1, samples: vec![1.0; 10] });
    let slot: Slot = Arc::new(OnceLock::new());
    let _ = slot.set(Ok(pcm));
    let clip = ClipParams { fade_in: 0.5, volume: 0.5, ..Default::default() };
    let mut m = Mixer {
        voices: vec![Voice {
            key: 1,
            media_key: 1,
            audio: Some(slot),
            spec: VoiceSpec { clip, looping: false, duration: 1.0, audio: true },
            t: 0.0,
            state: PlayState::Playing,
        }],
    };
    let mut buf = vec![0.0f32; 2 * 12];
    m.mix(Some(&mut buf), 12, 2, 10);
    // Frame 0: fade starts at 0; frame 3: 0.3/0.5 × 0.5 = 0.3; both channels carry the mono source.
    assert_eq!(buf[0], 0.0);
    assert!((buf[6] - 0.3).abs() < 1e-5 && (buf[7] - 0.3).abs() < 1e-5, "{buf:?}");
    assert!((buf[16] - 0.5).abs() < 1e-5);
    // Past the end: silence, and the voice has ended.
    assert_eq!(buf[22], 0.0);
    assert_eq!(m.voices[0].state, PlayState::Ended);
}

#[test]
fn device_clock_drives_position() {
    struct Fake(Arc<Mutex<Option<Box<dyn FnMut(&mut [f32], usize, u32) + Send>>>>);
    impl AudioOut for Fake {
        fn start(&mut self, fill: Box<dyn FnMut(&mut [f32], usize, u32) + Send>) -> Result<(), String> {
            *self.0.lock().map_err(|e| e.to_string())? = Some(fill);
            Ok(())
        }
        fn stop(&mut self) {}
    }
    let cb = Arc::new(Mutex::new(None));
    let mut p = Player::new(Some(Box::new(Fake(cb.clone()))));
    let b: Bytes = Arc::new(vec![]);
    p.play(3, 3, &b, spec(10.0, false), None);
    // Wall time alone doesn't move a device-clocked voice…
    p.tick(0.0);
    p.tick(0.2);
    assert_eq!(p.status(3).map(|s| s.position), Some(0.0));
    // …the device pulling 4800 frames at 48 kHz does.
    let mut buf = vec![0.0; 4800 * 2];
    if let Some(f) = cb.lock().ok().and_then(|mut g| g.take()).as_mut() {
        f(&mut buf, 2, 48_000);
    }
    let pos = p.status(3).map(|s| s.position).unwrap_or(-1.0);
    assert!((pos - 0.1).abs() < 1e-6, "{pos}");
}

/// 16-bit mono WAV of a constant level, `secs` long at 8 kHz.
fn wav_level(secs: f64, amp: f32) -> Bytes {
    let rate = 8_000u32;
    let n = (rate as f64 * secs).round() as usize;
    let data: Vec<u8> = (0..n).flat_map(|_| ((amp * 32767.0) as i16).to_le_bytes()).collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    Arc::new(b)
}

fn audible(duration: f64) -> VoiceSpec {
    VoiceSpec { clip: ClipParams::default(), looping: false, duration, audio: true }
}

/// First mixed sample of voice `key`, after its audio has decoded.
fn first_sample(p: &Player, key: u64) -> f32 {
    assert!(p.wait_loaded(key, std::time::Duration::from_secs(5)), "decode");
    let mut buf = vec![0.0f32; 4];
    p.render(&mut buf, 1, 8_000);
    buf[0]
}

#[test]
fn replacing_the_source_plays_the_new_audio_and_keeps_its_bytes() {
    let mut p = Player::new(None);
    let loud = wav_level(0.05, 0.5);
    let quiet = wav_level(0.10, 0.25);
    let loud_key = Arc::as_ptr(&loud) as u64;
    let quiet_key = Arc::as_ptr(&quiet) as u64;
    assert_ne!(loud_key, quiet_key);

    // Duration 0: the clock takes its length from the decoded audio, so a stale slot would
    // report the previous clip.
    p.play(1, loud_key, &loud, audible(0.0), None);
    let got = first_sample(&p, 1);
    assert!((got - 0.5).abs() < 0.02, "loud sample {got}");
    let dur = p.status(1).map(|s| s.duration).unwrap_or(-1.0);
    assert!((dur - 0.05).abs() < 0.01, "loud duration {dur}");
    assert!(p.cache.get(&loud_key).is_some_and(|c| Arc::ptr_eq(&c.source, &loud)));

    // The caller's buffer can go away; the cache still owns the allocation its key names.
    drop(loud);
    assert_eq!(Arc::as_ptr(&p.cache.get(&loud_key).expect("cached").source) as u64, loud_key);

    p.play(1, quiet_key, &quiet, audible(0.0), None);
    let got = first_sample(&p, 1);
    assert!((got - 0.25).abs() < 0.02, "quiet sample {got}");
    let dur = p.status(1).map(|s| s.duration).unwrap_or(-1.0);
    assert!((dur - 0.10).abs() < 0.01, "quiet duration {dur}");
    assert!(p.cache.get(&quiet_key).is_some_and(|c| Arc::ptr_eq(&c.source, &quiet)));
    // Resuming the same source does not jump back to the start.
    let pos = p.status(1).map(|s| s.position).unwrap_or(-1.0);
    p.play(1, quiet_key, &quiet, audible(0.0), None);
    let again = p.status(1).map(|s| s.position).unwrap_or(-1.0);
    assert!((again - pos).abs() < 1e-6, "resume moved {pos} -> {again}");
}

type Fill = Box<dyn FnMut(&mut [f32], usize, u32) + Send>;

/// An output that starts, then fails or goes quiet on command.
#[derive(Clone, Default)]
struct Flaky {
    fill: Arc<Mutex<Option<Fill>>>,
    fail: Arc<Mutex<Option<String>>>,
    starts: Arc<Mutex<u32>>,
}
impl AudioOut for Flaky {
    fn start(&mut self, fill: Fill) -> Result<(), String> {
        *self.fill.lock().map_err(|e| e.to_string())? = Some(fill);
        *self.starts.lock().map_err(|e| e.to_string())? += 1;
        Ok(())
    }
    fn stop(&mut self) {
        if let Ok(mut f) = self.fill.lock() {
            *f = None;
        }
    }
    fn failure(&mut self) -> Option<String> {
        self.fail.lock().ok().and_then(|mut f| f.take())
    }
}
impl Flaky {
    fn pull(&self, frames: usize) {
        let mut buf = vec![0.0; frames * 2];
        if let Some(f) = self.fill.lock().ok().as_mut().and_then(|g| g.as_mut()) {
            f(&mut buf, 2, 48_000);
        }
    }
    fn starts(&self) -> u32 {
        self.starts.lock().map(|s| *s).unwrap_or(0)
    }
}

fn pos(p: &Player, key: u64) -> f64 {
    p.status(key).map(|s| s.position).unwrap_or(-1.0)
}

#[test]
fn a_failed_output_falls_back_to_the_wall_clock() {
    let out = Flaky::default();
    let mut p = Player::new(Some(Box::new(out.clone())));
    let b: Bytes = Arc::new(vec![]);
    p.play(1, 1, &b, spec(10.0, false), None);
    p.tick(0.0);
    out.pull(4800);
    assert!((pos(&p, 1) - 0.1).abs() < 1e-6);
    if let Ok(mut f) = out.fail.lock() {
        *f = Some("unplugged".into());
    }
    p.tick(0.2);
    p.tick(0.4);
    // The voice keeps moving on wall time instead of freezing: 0.1 s from the device, then
    // the 0.4 s of wall time since the last tick before the failure.
    assert!((pos(&p, 1) - 0.5).abs() < 1e-3, "{}", pos(&p, 1));
    assert_eq!(p.status(1).map(|s| s.state), Some(PlayState::Playing));
    // Playing again restarts the device.
    p.play(2, 2, &b, spec(10.0, false), None);
    assert_eq!(out.starts(), 2);
    assert!(p.has_output());
}

#[test]
fn a_stalled_output_falls_back_to_the_wall_clock() {
    let out = Flaky::default();
    let mut p = Player::new(Some(Box::new(out.clone())));
    let b: Bytes = Arc::new(vec![]);
    p.play(1, 1, &b, spec(10.0, false), None);
    // No callback ever arrives: after the first-callback grace the wall clock takes over.
    for i in 0..=21 {
        p.tick(i as f64 * 0.25);
    }
    let before = pos(&p, 1);
    assert!(before > 0.0, "{before}");
    p.tick(5.5);
    assert!((pos(&p, 1) - before - 0.25).abs() < 1e-3);
}

#[test]
fn a_slow_device_that_answers_within_the_grace_is_not_a_stall() {
    let out = Flaky::default();
    let mut p = Player::new(Some(Box::new(out.clone())));
    let b: Bytes = Arc::new(vec![]);
    p.play(1, 1, &b, spec(10.0, false), None);
    for i in 0..=8 {
        p.tick(i as f64 * 0.25);
    }
    // Two seconds in, the device starts asking for samples: the same stream keeps playing.
    out.pull(256);
    p.tick(2.25);
    p.tick(2.5);
    assert_eq!(out.starts(), 1, "the stream was not restarted");
}

#[test]
fn an_idle_stream_without_callbacks_is_not_a_stall() {
    let out = Flaky::default();
    let mut p = Player::new(Some(Box::new(out.clone())));
    let b: Bytes = Arc::new(vec![]);
    p.play(1, 1, &b, spec(10.0, false), None);
    p.pause(1);
    for i in 0..=8 {
        p.tick(i as f64 * 0.25);
    }
    // Resuming keeps the running stream: no restart.
    p.play(1, 1, &b, spec(10.0, false), None);
    assert_eq!(out.starts(), 1);
}

#[test]
fn repeated_failures_give_up_on_the_device() {
    let out = Flaky::default();
    let mut p = Player::new(Some(Box::new(out.clone())));
    let b: Bytes = Arc::new(vec![]);
    let mut now = 0.0;
    for _ in 0..MAX_OUTPUT_FAILURES {
        p.play(1, 1, &b, spec(100.0, false), None);
        if let Ok(mut f) = out.fail.lock() {
            *f = Some("broken".into());
        }
        p.tick(now);
        now += 0.1;
    }
    assert!(!p.has_output());
    assert_eq!(out.starts(), MAX_OUTPUT_FAILURES);
    // Still plays, silently on wall time.
    let before = pos(&p, 1);
    p.tick(now + 0.2);
    assert!(pos(&p, 1) > before);
}
