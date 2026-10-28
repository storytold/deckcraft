//! The playback mixer and clock.
//!
//! The UI host owns a [`Player`] and an optional [`AudioOut`] (cpal on desktop). Each playing
//! media shape is a *voice* with its own clock (`position`, in media seconds), advanced by the
//! audio callback as it pulls samples — so video frames synced to it stay in step with what is
//! heard. Without an output device (web, CI, no sound card) [`Player::tick`] advances the clocks on
//! wall time instead, so playback, looping and video still work silently.
//!
//! Audio is decoded once per media item (on a worker thread where there are threads) and cached.
//! The cache keeps those source bytes: its key is the allocation's address, which would otherwise
//! be free to reuse for different media. A voice whose audio is still decoding holds at its start
//! (`Loading`); one whose audio can't be decoded runs silently. Playing a voice again with a
//! different source replaces its audio instead of keeping the previous clip.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::{Bytes, ClipParams, Pcm};

/// An audio output device. `fill(buffer, channels, sample_rate)` is called from the device's
/// thread to fill an interleaved f32 buffer.
pub trait AudioOut {
    fn start(&mut self, fill: Box<dyn FnMut(&mut [f32], usize, u32) + Send>) -> Result<(), String>;
    fn stop(&mut self);
    /// Why the started stream has died, if it has (the device went away), reported once. Polled
    /// by [`Player::tick`] on the host thread; must not block. Outputs that cannot tell return
    /// `None`: the player also notices a stream whose callbacks stopped.
    fn failure(&mut self) -> Option<String> {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    /// Waiting for the audio to decode.
    Loading,
    Playing,
    Paused,
    /// Reached the end (not looping).
    Ended,
}

impl PlayState {
    pub fn name(self) -> &'static str {
        match self {
            PlayState::Loading => "loading",
            PlayState::Playing => "playing",
            PlayState::Paused => "paused",
            PlayState::Ended => "ended",
        }
    }
}

/// What to play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceSpec {
    pub clip: ClipParams,
    pub looping: bool,
    /// Media duration in seconds from the probe (0 = unknown: taken from the decoded audio).
    pub duration: f64,
    /// Decode and mix the audio (false for videos without sound, or muted).
    pub audio: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VoiceStatus {
    pub state: PlayState,
    /// Media time in seconds.
    pub position: f64,
    pub duration: f64,
    /// Why the audio is silent, if it couldn't be decoded.
    pub error: Option<String>,
}

type Slot = Arc<OnceLock<Result<Arc<Pcm>, String>>>;

/// One decoded item. `source` keeps the allocation `media_key` names alive for as long as the
/// entry does, so a later buffer cannot be given that address and read this PCM.
struct CachedAudio {
    slot: Slot,
    source: Bytes,
}

struct Voice {
    key: u64,
    /// Identity of the bytes this voice is playing. A different key replaces `audio`.
    media_key: u64,
    audio: Option<Slot>,
    spec: VoiceSpec,
    t: f64,
    state: PlayState,
}

impl Voice {
    fn duration(&self) -> f64 {
        if self.spec.duration > 0.0 {
            return self.spec.duration;
        }
        match self.audio.as_ref().and_then(|s| s.get()) {
            Some(Ok(p)) => p.duration(),
            _ => 0.0,
        }
    }
    fn pcm(&self) -> Option<&Arc<Pcm>> {
        self.audio.as_ref().and_then(|s| s.get()).and_then(|r| r.as_ref().ok())
    }
    fn loading(&self) -> bool {
        self.audio.as_ref().is_some_and(|s| s.get().is_none())
    }
}

/// The voices, shared with the audio callback.
#[derive(Default)]
pub struct Mixer {
    voices: Vec<Voice>,
}

impl Mixer {
    /// Mix `frames` frames at `rate` into `out` (interleaved, `channels` wide; `None`: advance the
    /// clocks only).
    pub fn mix(&mut self, mut out: Option<&mut [f32]>, frames: usize, channels: usize, rate: u32) {
        if let Some(o) = out.as_deref_mut() {
            o.fill(0.0);
        }
        let step = 1.0 / rate.max(1) as f64;
        let channels = channels.max(1);
        for v in &mut self.voices {
            if v.state != PlayState::Playing || v.loading() {
                continue;
            }
            let dur = v.duration();
            let (start, end) = v.spec.clip.range(dur);
            if v.t < start {
                v.t = start;
            }
            let pcm = v.pcm().cloned();
            match (&mut out, pcm) {
                (Some(o), Some(pcm)) => {
                    let src_ch = pcm.channels.max(1) as usize;
                    let n_src = pcm.frames();
                    for f in 0..frames {
                        if v.t >= end {
                            if v.spec.looping {
                                v.t = start;
                            } else {
                                v.state = PlayState::Ended;
                                v.t = end;
                                break;
                            }
                        }
                        let g = v.spec.clip.gain(v.t, dur);
                        let x = v.t * pcm.rate as f64;
                        let k = x as usize;
                        if g > 0.0 && k < n_src {
                            let fr = (x - k as f64) as f32;
                            for c in 0..channels {
                                let sc = if src_ch == 1 {
                                    0
                                } else if c < src_ch {
                                    c
                                } else {
                                    continue;
                                };
                                let a = pcm.at(k, sc);
                                let b = if k + 1 < n_src { pcm.at(k + 1, sc) } else { a };
                                if let Some(s) = o.get_mut(f * channels + c) {
                                    *s += (a + (b - a) * fr) * g;
                                }
                            }
                        }
                        v.t += step;
                    }
                }
                _ => {
                    // Silent: just run the clock.
                    let mut left = frames as f64 * step;
                    while left > 0.0 {
                        let room = end - v.t;
                        if left < room {
                            v.t += left;
                            break;
                        }
                        left -= room.max(0.0);
                        if v.spec.looping && end.is_finite() && end - start > 1e-3 {
                            v.t = start;
                        } else {
                            v.t = end;
                            v.state = PlayState::Ended;
                            break;
                        }
                    }
                }
            }
        }
        if let Some(o) = out {
            for s in o.iter_mut() {
                *s = s.clamp(-1.0, 1.0);
            }
        }
    }
}

/// Default mix rate when no device says otherwise.
const DEFAULT_RATE: u32 = 48_000;

/// Seconds without a single device callback, while a voice plays, after which the stream counts
/// as stalled and the wall clock takes over.
const STALL_SECS: f64 = 1.0;

/// Seconds a just-started stream may take to ask for its first samples (Bluetooth and other slow
/// devices) before it counts as stalled.
const FIRST_CALLBACK_SECS: f64 = 5.0;

/// Output failures after which the player stops retrying the device and plays silently.
const MAX_OUTPUT_FAILURES: u32 = 3;

pub struct Player {
    out: Option<Box<dyn AudioOut>>,
    streaming: bool,
    mixer: Arc<Mutex<Mixer>>,
    last_tick: Option<f64>,
    idle_since: Option<f64>,
    cache: HashMap<u64, CachedAudio>,
    /// Device callbacks so far (the audio thread counts, [`Player::tick`] watches).
    pulls: Arc<AtomicU64>,
    /// The pull count last seen by `tick` and when it last changed.
    pulls_seen: Option<(u64, f64)>,
    /// Times the output failed or stalled after starting.
    failures: u32,
    /// `pulls` when the current stream started: callbacks beyond it mean the device is running.
    pulls_at_start: u64,
}

impl Player {
    /// A player over `out` (`None`: silent, clocked by [`Player::tick`]).
    pub fn new(out: Option<Box<dyn AudioOut>>) -> Player {
        Player {
            out,
            streaming: false,
            mixer: Arc::new(Mutex::new(Mixer::default())),
            last_tick: None,
            idle_since: None,
            cache: HashMap::new(),
            pulls: Arc::new(AtomicU64::new(0)),
            pulls_seen: None,
            failures: 0,
            pulls_at_start: 0,
        }
    }

    pub fn has_output(&self) -> bool {
        self.out.is_some()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Mixer) -> R) -> R {
        let mut g = self.mixer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut g)
    }

    /// The decoded audio of `bytes`, cached by `media_key` (decoding starts now).
    fn slot(&mut self, media_key: u64, bytes: &Bytes) -> Slot {
        if self.cache.len() > 16 {
            // Forget audio no voice uses. Dropping the entry drops its source bytes too.
            let used: Vec<u64> = self.cache.iter().filter(|(_, c)| Arc::strong_count(&c.slot) > 1).map(|(k, _)| *k).collect();
            self.cache.retain(|k, _| used.contains(k));
        }
        if let Some(hit) = self.cache.get(&media_key)
            && Arc::as_ptr(&hit.source) as u64 == media_key
        {
            return hit.slot.clone();
        }
        // A key whose bytes are gone is not this clip. Decode again.
        self.cache.remove(&media_key);
        let slot: Slot = Arc::new(OnceLock::new());
        let (s, b) = (slot.clone(), bytes.clone());
        let job = move || {
            let _ = s.set(crate::audio::decode(&b).map(Arc::new).map_err(|e| e.to_string()));
        };
        if crate::THREADS {
            if std::thread::Builder::new().name("deckcraft-audio-decode".into()).spawn(job).is_err() {
                let _ = slot.set(Err("couldn't start the decoder".into()));
            }
        } else {
            job();
        }
        let cached = CachedAudio { slot: slot.clone(), source: bytes.clone() };
        self.cache.insert(media_key, cached);
        slot
    }

    /// Play voice `key` (a shape) from `from` seconds (default: the trimmed start). Resumes a
    /// paused voice when `from` is `None` and `media_key` is unchanged. A different `media_key`
    /// replaces the decoded audio and starts the new clip (at `from`, or at its start).
    pub fn play(&mut self, key: u64, media_key: u64, bytes: &Bytes, spec: VoiceSpec, from: Option<f64>) {
        let audio = spec.audio.then(|| self.slot(media_key, bytes));
        self.with(|m| {
            if let Some(v) = m.voices.iter_mut().find(|v| v.key == key) {
                let same = v.media_key == media_key;
                v.media_key = media_key;
                v.spec = spec;
                if same {
                    if v.audio.is_none() {
                        v.audio = audio;
                    }
                    if let Some(t) = from {
                        v.t = t;
                    } else if v.state == PlayState::Ended {
                        v.t = spec.clip.range(v.duration()).0;
                    }
                } else {
                    v.audio = audio;
                    v.t = from.unwrap_or_else(|| spec.clip.range(spec.duration).0);
                }
                v.state = PlayState::Playing;
            } else {
                let t = from.unwrap_or_else(|| spec.clip.range(spec.duration).0);
                m.voices.push(Voice { key, media_key, audio, spec, t, state: PlayState::Playing });
            }
        });
        self.ensure_stream();
    }

    fn ensure_stream(&mut self) {
        if self.streaming {
            return;
        }
        let Some(out) = self.out.as_mut() else { return };
        let m = self.mixer.clone();
        let pulls = self.pulls.clone();
        let fill = Box::new(move |buf: &mut [f32], channels: usize, rate: u32| match m.lock() {
            Ok(mut g) => {
                pulls.fetch_add(1, Ordering::Relaxed);
                let frames = buf.len() / channels.max(1);
                g.mix(Some(buf), frames, channels, rate)
            }
            Err(_) => buf.fill(0.0),
        });
        match out.start(fill) {
            Ok(()) => {
                self.streaming = true;
                self.pulls_seen = None;
                self.pulls_at_start = self.pulls.load(Ordering::Relaxed);
            }
            Err(e) => {
                log::warn!("audio output unavailable ({e}); media plays silently");
                self.out = None;
            }
        }
    }

    pub fn pause(&mut self, key: u64) {
        self.with(|m| {
            if let Some(v) = m.voices.iter_mut().find(|v| v.key == key && v.state == PlayState::Playing) {
                v.state = PlayState::Paused;
            }
        });
    }

    /// Remove voice `key`.
    pub fn stop(&mut self, key: u64) {
        self.with(|m| m.voices.retain(|v| v.key != key));
    }

    pub fn stop_all(&mut self) {
        self.with(|m| m.voices.clear());
    }

    /// Move voice `key` to `t` seconds (clamped to its played range).
    pub fn seek(&mut self, key: u64, t: f64) {
        self.with(|m| {
            if let Some(v) = m.voices.iter_mut().find(|v| v.key == key) {
                let (s, e) = v.spec.clip.range(v.duration());
                v.t = t.clamp(s, if e.is_finite() { e } else { f64::MAX });
                if v.state == PlayState::Ended {
                    v.state = PlayState::Paused;
                }
            }
        });
    }

    pub fn status(&self, key: u64) -> Option<VoiceStatus> {
        self.with(|m| {
            m.voices.iter().find(|v| v.key == key).map(|v| VoiceStatus {
                state: if v.state == PlayState::Playing && v.loading() { PlayState::Loading } else { v.state },
                position: v.t,
                duration: v.duration(),
                error: v.audio.as_ref().and_then(|s| s.get()).and_then(|r| r.as_ref().err().cloned()),
            })
        })
    }

    /// Keys of all voices (playing, paused or ended).
    pub fn keys(&self) -> Vec<u64> {
        self.with(|m| m.voices.iter().map(|v| v.key).collect())
    }

    pub fn any_playing(&self) -> bool {
        self.with(|m| m.voices.iter().any(|v| v.state == PlayState::Playing))
    }

    /// Call every frame with the host's clock (seconds): runs the voices' clocks when there is no
    /// audio device, and releases the device after a few idle seconds.
    pub fn tick(&mut self, now: f64) {
        let dt = self.last_tick.map(|l| (now - l).clamp(0.0, 0.25)).unwrap_or(0.0);
        self.last_tick = Some(now);
        self.check_output(now);
        if !self.streaming && dt > 0.0 {
            let frames = (dt * DEFAULT_RATE as f64).round() as usize;
            self.with(|m| m.mix(None, frames, 2, DEFAULT_RATE));
        }
        if self.streaming {
            if self.any_playing() {
                self.idle_since = None;
            } else if now - *self.idle_since.get_or_insert(now) > 3.0 {
                if let Some(o) = self.out.as_mut() {
                    o.stop();
                }
                self.streaming = false;
                self.idle_since = None;
            }
        }
    }

    /// Drop a stream that died or stalled, so the voices fall back to the wall clock instead of
    /// freezing. The next `play` tries the device again, up to [`MAX_OUTPUT_FAILURES`] times.
    fn check_output(&mut self, now: f64) {
        if !self.streaming {
            return;
        }
        // Only a playing voice needs callbacks; an idle stream may legitimately sleep.
        let playing = self.any_playing();
        let Some(out) = self.out.as_mut() else { return };
        let pulls = self.pulls.load(Ordering::Relaxed);
        let running = pulls > self.pulls_at_start;
        // A stream that delivers again has recovered: later failures start counting afresh.
        if running {
            self.failures = 0;
        }
        let limit = if running { STALL_SECS } else { FIRST_CALLBACK_SECS };
        let reason = if let Some(e) = out.failure() {
            Some(e)
        } else {
            match self.pulls_seen {
                Some((seen, since)) if seen == pulls => {
                    if !playing {
                        self.pulls_seen = Some((pulls, now));
                        None
                    } else if now - since > limit {
                        Some("the audio device stopped asking for samples".to_string())
                    } else {
                        None
                    }
                }
                _ => {
                    self.pulls_seen = Some((pulls, now));
                    None
                }
            }
        };
        let Some(reason) = reason else { return };
        out.stop();
        self.streaming = false;
        self.pulls_seen = None;
        self.idle_since = None;
        self.failures = self.failures.saturating_add(1);
        if self.failures >= MAX_OUTPUT_FAILURES {
            log::warn!("audio output failed ({reason}); giving up on the device, media plays silently");
            self.out = None;
        } else {
            log::warn!("audio output failed ({reason}); media plays silently until it restarts");
        }
    }

    /// Block until voice `key`'s audio has decoded (tests, offline rendering).
    pub fn wait_loaded(&self, key: u64, timeout: std::time::Duration) -> bool {
        let slot = self.with(|m| m.voices.iter().find(|v| v.key == key).and_then(|v| v.audio.clone()));
        let Some(slot) = slot else { return true };
        let t0 = web_now();
        while slot.get().is_none() {
            if web_now() - t0 > timeout.as_secs_f64() || !crate::THREADS {
                return slot.get().is_some();
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        true
    }

    /// Mix into `buf` directly (offline rendering and tests; the device callback does the same).
    pub fn render(&self, buf: &mut [f32], channels: usize, rate: u32) {
        let frames = buf.len() / channels.max(1);
        self.with(|m| m.mix(Some(buf), frames, channels, rate));
    }
}

/// Seconds on a monotonic clock where there is one (std's Instant panics on wasm32).
fn web_now() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static T0: OnceLock<std::time::Instant> = OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(o) = self.out.as_mut() {
            o.stop();
        }
    }
}

#[cfg(test)]
mod tests;
