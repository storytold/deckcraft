//! Mutation fuzzing of the hand-written decoders (AV1, VP9, HEVC, H.264 video; Opus audio) at the
//! bitstream level.
//!
//! `damaged_media_neither_panics_nor_hangs` (decode.rs) damages whole files, so most mutations land
//! in the container and never reach a decoder. Here the clips are demuxed the way `video.rs` and
//! `audio.rs` do, and the compressed samples themselves are damaged (truncated, bit-flipped,
//! overwritten, dropped, repeated, reordered) before they reach the decoder. Every mutation must
//! give an error or output: never a panic, never a hang (AGENTS.md: never crash). An Opus packet
//! truncated to nothing is fed as a lost packet (`decode(None)`), so concealment is fuzzed too.
//!
//! The AV1 film grain clips (`clip-av1-grain-*.mkv`) are there so the mutations reach the grain
//! synthesis too; `av1_grain_fixtures_carry_film_grain` checks they really carry grain.
//!
//! Seeded and deterministic. `DECKCRAFT_MUTATION_ITERS` (default 200 per clip) runs it longer;
//! `DECKCRAFT_MUTATION_SEED` (decimal or `0x` hex) starts from another seed. A failure prints the
//! clip, iteration and mutation, and a command that replays it on its own
//! (`DECKCRAFT_MUTATION_CLIP` and `DECKCRAFT_MUTATION_FROM` select the clip and iteration), so it
//! can be turned into a fixture.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deckcraft_isobmff::{CodecConfig, TrackKind as Mp4Kind};
use deckcraft_matroska::{Codec as MkvCodec, TrackKind as MkvKind};

/// How long one decode of a fixture (a ~6-frame 64×48 clip, half a second of audio) may take
/// before it counts as a hang.
const HANG: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
enum Kind {
    Av1,
    Vp9,
    Hevc,
    H264,
    Opus,
}

/// A demuxed clip: codec, its configuration record (av1C config OBUs, hvcC, avcC or OpusHead)
/// and the samples.
#[derive(Clone)]
struct Clip {
    name: String,
    kind: Kind,
    config: Vec<u8>,
    samples: Vec<Vec<u8>>,
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn demux_mkv(name: &str) -> Clip {
    let bytes = fixture(name);
    let f = deckcraft_matroska::open(&bytes).expect("matroska");
    let ti = f.tracks.iter().position(|t| t.kind == MkvKind::Video && !t.samples.is_empty()).expect("video track");
    let t = &f.tracks[ti];
    let (kind, config) = match &t.codec {
        MkvCodec::Av1 { av1c } => (Kind::Av1, deckcraft_isobmff::Av1Config::parse(av1c).map(|c| c.config_obus).unwrap_or_default()),
        MkvCodec::Vp9 { .. } => (Kind::Vp9, Vec::new()),
        other => panic!("{name}: unexpected codec {}", other.name()),
    };
    let samples = (0..t.samples.len()).map(|i| f.read_sample(&bytes, ti, i).expect("sample")).collect();
    Clip { name: name.to_string(), kind, config, samples }
}

fn demux_mp4(name: &str) -> Clip {
    let bytes = fixture(name);
    let f = deckcraft_isobmff::open(&bytes).expect("mp4");
    let ti = f.track_of_kind(Mp4Kind::Video).expect("video track");
    let t = &f.tracks[ti];
    let (kind, config) = match &t.entries.first().expect("sample entry").codec {
        CodecConfig::Hevc(c) => (Kind::Hevc, c.to_bytes()),
        CodecConfig::Avc(c) => (Kind::H264, c.to_bytes()),
        _ => panic!("{name}: expected HEVC or H.264"),
    };
    let samples = (0..t.samples.len()).map(|i| f.read_sample(&bytes, ti, i).expect("sample")).collect();
    Clip { name: name.to_string(), kind, config, samples }
}

/// The Opus stream of an Ogg file, read the way `audio.rs` reads it.
fn demux_ogg_opus(name: &str) -> Clip {
    let bytes = fixture(name);
    let src: &[u8] = &bytes;
    let f = deckcraft_ogg::open(src).expect("ogg");
    let s = f.stream_of(deckcraft_ogg::Codec::Opus).expect("opus stream");
    let config = f.streams[s].headers.first().expect("OpusHead").clone();
    let samples = (0..f.streams[s].packets.len()).map(|i| f.read_packet(src, s, i).expect("packet")).collect();
    Clip { name: name.to_string(), kind: Kind::Opus, config, samples }
}

/// The Opus track of a WebM file.
fn demux_mkv_opus(name: &str) -> Clip {
    let bytes = fixture(name);
    let f = deckcraft_matroska::open(&bytes).expect("matroska");
    let (ti, head) = f
        .tracks
        .iter()
        .enumerate()
        .find_map(|(i, t)| match &t.codec {
            MkvCodec::Opus { head } => Some((i, head.clone())),
            _ => None,
        })
        .expect("opus track");
    let samples = (0..f.tracks[ti].samples.len()).map(|i| f.read_sample(&bytes, ti, i).expect("sample")).collect();
    Clip { name: format!("{name} (Opus)"), kind: Kind::Opus, config: head, samples }
}

/// Decodes a clip the way `video.rs` and `audio.rs` drive each decoder; returns the number of
/// pictures (or, for Opus, of packets that gave audio). Decoder errors are fine (damaged input);
/// only panics and hangs are failures.
fn decode(clip: &Clip) -> usize {
    let mut pictures = 0;
    match clip.kind {
        Kind::Av1 => {
            let mut d = deckcraft_av1::Decoder::new();
            if !clip.config.is_empty() {
                let _ = d.decode(&clip.config);
            }
            for (i, s) in clip.samples.iter().enumerate() {
                pictures += d.decode_pts(s, i as i64).map(|p| p.len()).unwrap_or(0);
            }
            pictures += d.flush().len();
        }
        Kind::Vp9 => {
            let mut d = deckcraft_vp9::Decoder::new();
            for (i, s) in clip.samples.iter().enumerate() {
                pictures += d.decode(s, i as i64).map(|p| p.len()).unwrap_or(0);
            }
            pictures += d.flush().len();
        }
        Kind::Hevc => {
            let Ok(mut d) = deckcraft_hevc::Decoder::from_hvcc(&clip.config) else { return 0 };
            for (i, s) in clip.samples.iter().enumerate() {
                pictures += d.decode(s, i as i64).map(|p| p.len()).unwrap_or(0);
            }
            pictures += d.flush().len();
        }
        Kind::H264 => {
            let Ok(mut d) = deckcraft_h264::Decoder::from_avcc(&clip.config) else { return 0 };
            for (i, s) in clip.samples.iter().enumerate() {
                pictures += d.decode(s, i as i64).map(|p| p.len()).unwrap_or(0);
            }
            pictures += d.flush().len();
        }
        Kind::Opus => {
            let Ok(mut d) = deckcraft_opus::Decoder::new(&clip.config) else { return 0 };
            d.set_trim_pre_skip(true);
            for s in &clip.samples {
                // An empty packet stands for a lost one: concealment runs instead of decoding.
                let packet = (!s.is_empty()).then_some(s.as_slice());
                if d.decode(packet).is_ok_and(|planes| planes.iter().any(|p| !p.is_empty())) {
                    pictures += 1;
                }
            }
        }
    }
    pictures
}

/// xorshift64*: small, seeded, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

/// Applies one random mutation; returns a short description for the failure report.
fn mutate(clip: &mut Clip, rng: &mut Rng) -> String {
    let n = clip.samples.len();
    // Mostly damage the samples; sometimes the configuration record.
    let target_config = !clip.config.is_empty() && rng.below(8) == 0;
    let k = rng.below(n);
    let buf = if target_config { &mut clip.config } else { &mut clip.samples[k] };
    let what = if target_config { "config".to_string() } else { format!("sample {k}") };
    match rng.below(7) {
        0 if !buf.is_empty() => {
            let len = rng.below(buf.len());
            buf.truncate(len);
            format!("truncate {what} to {len} bytes")
        }
        1 if !buf.is_empty() => {
            let flips = 1 + rng.below(8);
            for _ in 0..flips {
                let at = rng.below(buf.len());
                buf[at] ^= 1 << rng.below(8);
            }
            format!("flip {flips} bits in {what}")
        }
        2 if !buf.is_empty() => {
            let at = rng.below(buf.len());
            let len = 1 + rng.below((buf.len() - at).min(32));
            let fill = if rng.below(2) == 0 { 0x00 } else { 0xff };
            buf[at..at + len].fill(fill);
            format!("fill {len} bytes of {what} at {at} with {fill:#04x}")
        }
        3 if !buf.is_empty() => {
            let at = rng.below(buf.len());
            let b = (rng.next() & 0xff) as u8;
            buf[at] = b;
            format!("set byte {at} of {what} to {b:#04x}")
        }
        4 if n > 1 => {
            clip.samples.remove(k);
            format!("drop sample {k}")
        }
        5 => {
            let copy = clip.samples[k].clone();
            clip.samples.insert(k, copy);
            format!("repeat sample {k}")
        }
        _ if n > 1 => {
            let j = rng.below(n);
            clip.samples.swap(k, j);
            format!("swap samples {k} and {j}")
        }
        _ => "no-op".to_string(),
    }
}

/// Runs `work` on its own thread; Err(message) on panic or when it outlives `limit`.
fn run_guarded(work: impl FnOnce() -> usize + Send + 'static, limit: Duration, last_panic: &Arc<Mutex<String>>) -> Result<usize, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let r = panic::catch_unwind(AssertUnwindSafe(work));
        let _ = tx.send(r);
    });
    match rx.recv_timeout(limit) {
        Ok(Ok(pictures)) => Ok(pictures),
        Ok(Err(_)) => Err(format!("panic: {}", last_panic.lock().map(|s| s.clone()).unwrap_or_default())),
        Err(_) => Err(format!("hang: still decoding after {} s", limit.as_secs_f32())),
    }
}

/// The panic hook is process-wide: every test in this file takes this lock, so none of them runs
/// while another one's hook is installed.
static HOOK: Mutex<()> = Mutex::new(());

/// Installs a hook that records where the last panic happened (the payload alone lacks it), then
/// hands the panic to the hook that was there before, so a failing assertion still prints its
/// message.
fn record_panics() -> Arc<Mutex<String>> {
    let last_panic = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&last_panic);
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        if let Ok(mut s) = sink.lock() {
            *s = format!("{msg} at {at}");
        }
        previous(info);
    }));
    last_panic
}

/// The grain fixtures must really exercise film grain synthesis, or fuzzing them adds nothing:
/// the sequence header says so, and decoding with grain changes the pictures.
#[test]
fn av1_grain_fixtures_carry_film_grain() {
    let _serial = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    for name in ["clip-av1-grain-4.mkv", "clip-av1-grain-6.mkv", "clip-av1-grain-15.mkv"] {
        let clip = demux_mkv(name);
        let pictures = |grain: bool| {
            let mut d = deckcraft_av1::Decoder::new();
            d.apply_film_grain = grain;
            d.decode(&clip.config).unwrap_or_else(|e| panic!("{name}: config: {e}"));
            let mut out: Vec<Vec<u16>> = Vec::new();
            for (i, s) in clip.samples.iter().enumerate() {
                let p = d.decode_pts(s, i as i64).unwrap_or_else(|e| panic!("{name}: sample {i}: {e}"));
                out.extend(p.into_iter().map(|p| p.planes.concat()));
            }
            out.extend(d.flush().into_iter().map(|p| p.planes.concat()));
            let present = d.sequence_header().is_some_and(|s| s.film_grain_params_present);
            (present, out)
        };
        let (present, with) = pictures(true);
        let (_, without) = pictures(false);
        assert!(present, "{name}: sequence header has no film grain");
        assert_eq!((with.len(), without.len()), (6, 6), "{name}");
        let changed = with.iter().zip(&without).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 6, "{name}: grain left {} of 6 pictures unchanged", 6 - changed);
    }
}

/// Control for the harness itself: a fuzzer that finds nothing proves nothing until it is seen
/// to catch a panic and a hang.
#[test]
fn harness_reports_panics_and_hangs() {
    let _serial = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let last_panic = record_panics();
    let ok = run_guarded(|| 3, Duration::from_secs(5), &last_panic);
    assert_eq!(ok, Ok(3));
    let crashed = run_guarded(|| panic!("control panic"), Duration::from_secs(5), &last_panic);
    assert!(crashed.as_ref().is_err_and(|e| e.starts_with("panic: control panic at ") && e.contains("codec_mutation.rs")), "{crashed:?}");
    let hung = run_guarded(
        || loop {
            std::thread::sleep(Duration::from_millis(50))
        },
        Duration::from_millis(300),
        &last_panic,
    );
    assert!(hung.as_ref().is_err_and(|e| e.starts_with("hang:")), "{hung:?}");
    let _ = panic::take_hook();
}

/// A number in decimal or, with a `0x` prefix, in hexadecimal.
fn parse_u64(v: &str) -> Option<u64> {
    let v = v.trim();
    match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(&hex.replace('_', ""), 16).ok(),
        None => v.replace('_', "").parse().ok(),
    }
}

/// Reads a number from the environment. A value that does not parse is an error, not the
/// default: a mistyped replay seed must not silently run the default mutations.
fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(v) => parse_u64(&v).unwrap_or_else(|| panic!("{name}={v:?} is not a number (decimal or 0x hex)")),
        Err(_) => default,
    }
}

/// The damaged copy of clip number `ci` for iteration `i` of a run started from `seed0`, and the
/// mutations that made it. Deterministic: the same three numbers always give the same copy.
fn damaged_copy(clip: &Clip, seed0: u64, ci: usize, i: u64) -> (Clip, Vec<String>) {
    let seed = seed0 ^ (i.wrapping_mul(0x9e37_79b9_7f4a_7c15)) ^ (ci as u64) << 56;
    let mut rng = Rng(seed | 1);
    let mut damaged = clip.clone();
    let steps = 1 + rng.below(3);
    let mutations = (0..steps).map(|_| mutate(&mut damaged, &mut rng)).collect();
    (damaged, mutations)
}

/// The command that replays one failing mutation on its own.
fn replay_command(seed0: u64, clip: &str, i: u64) -> String {
    format!(
        "DECKCRAFT_MUTATION_SEED={seed0:#x} DECKCRAFT_MUTATION_CLIP='{clip}' DECKCRAFT_MUTATION_FROM={i} DECKCRAFT_MUTATION_ITERS=1 \
         cargo test -p deckcraft-media --test codec_mutation damaged_bitstreams"
    )
}

/// The replay command really replays: its numbers parse back (hex included), and the same seed,
/// clip and iteration give the same damaged copy.
#[test]
fn reported_failures_replay_exactly() {
    let _serial = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(parse_u64("0x00de_c0de"), Some(0xdec0de));
    assert_eq!(parse_u64("14598366"), Some(14_598_366));
    assert_eq!(parse_u64("0xZZ"), None);
    let cmd = replay_command(0x00de_c0de, "clip-hevc.mp4", 42);
    let seed = cmd.split_whitespace().find_map(|w| w.strip_prefix("DECKCRAFT_MUTATION_SEED=")).and_then(parse_u64);
    assert_eq!(seed, Some(0xdec0de), "{cmd}");
    let clip = demux_mp4("clip-hevc.mp4");
    let (a, why_a) = damaged_copy(&clip, 0xdec0de, 5, 42);
    let (b, why_b) = damaged_copy(&clip, 0xdec0de, 5, 42);
    assert_eq!((a.config, a.samples, why_a), (b.config, b.samples, why_b));
}

/// The recording hook must hand every panic on to the hook it replaced: otherwise a failing
/// assert in a test prints no message while it is installed.
#[test]
fn panic_hook_forwards_to_the_previous_hook() {
    let _serial = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let forwarded = Arc::new(Mutex::new(0usize));
    let seen = Arc::clone(&forwarded);
    panic::set_hook(Box::new(move |_| *seen.lock().unwrap_or_else(|e| e.into_inner()) += 1));
    let last_panic = record_panics();
    let crashed = run_guarded(|| panic!("forwarded control panic"), Duration::from_secs(5), &last_panic);
    let _ = panic::take_hook();
    assert!(crashed.as_ref().is_err_and(|e| e.contains("forwarded control panic")), "{crashed:?}");
    assert_eq!(*forwarded.lock().unwrap_or_else(|e| e.into_inner()), 1, "the previous hook did not see the panic");
}

#[test]
fn damaged_bitstreams_neither_panic_nor_hang() {
    let _serial = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let last_panic = record_panics();
    let only = std::env::var("DECKCRAFT_MUTATION_CLIP").ok();
    let run = Run {
        seed0: env_u64("DECKCRAFT_MUTATION_SEED", 0x00de_c0de),
        // Replaying one failure: start at its iteration, on its clip only (see `replay_command`).
        from: env_u64("DECKCRAFT_MUTATION_FROM", 0),
        iters: env_u64("DECKCRAFT_MUTATION_ITERS", 200),
        only: only.as_deref(),
        limit: HANG,
    };
    let clips = vec![
        demux_mkv("clip-av1.mkv"),
        // Film grain synthesis (crates/av1/src/grain.rs) only runs on streams that carry grain.
        demux_mkv("clip-av1-grain-4.mkv"),
        demux_mkv("clip-av1-grain-6.mkv"),
        demux_mkv("clip-av1-grain-15.mkv"),
        demux_mkv("clip-vp9.webm"),
        demux_mp4("clip-hevc.mp4"),
        demux_mp4("clip-h264.mp4"),
        demux_mp4("clip-h264.mov"),
        demux_ogg_opus("tone.opus"),
        demux_mkv_opus("clip-vp9.webm"),
    ];
    let failures = fuzz(clips, &run, decode, &last_panic);
    let _ = panic::take_hook();
    assert!(failures.is_empty(), "{} damaged bitstreams panicked or hung:\n{}", failures.len(), failures.join("\n"));
}

/// What one fuzzing run is asked to do.
struct Run<'a> {
    seed0: u64,
    from: u64,
    iters: u64,
    /// Only the clip with this name (`DECKCRAFT_MUTATION_CLIP`).
    only: Option<&'a str>,
    /// How long one decode may take before it counts as a hang.
    limit: Duration,
}

/// Fuzzes `clips` through `work` (the decoder driver) and returns the failures.
///
/// A clip filter that matches no clip is an error: a mistyped replay must not pass while
/// decoding nothing. The run stops at the first hang: the hung thread keeps running, so going on
/// would slow every later decode and could pile up more hangs before the report.
fn fuzz(clips: Vec<Clip>, run: &Run, work: fn(&Clip) -> usize, last_panic: &Arc<Mutex<String>>) -> Vec<String> {
    if let Some(name) = run.only {
        let names: Vec<&str> = clips.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&name), "DECKCRAFT_MUTATION_CLIP={name:?} matches no clip; the clips are: {}", names.join(", "));
    }
    let mut failures = Vec::new();
    // The clip's index is part of every seed, so it is taken before any filtering.
    'clips: for (ci, clip) in clips.into_iter().enumerate() {
        if run.only.is_some_and(|name| name != clip.name) {
            continue;
        }
        // Control: the clean clip must decode to pictures, or the harness tests nothing.
        let c = clip.clone();
        let clean = run_guarded(move || work(&c), run.limit, last_panic).unwrap_or_else(|e| panic!("{} clean clip: {e}", clip.name));
        assert!(clean > 0, "{}: the clean clip decoded to no pictures, the harness is broken", clip.name);

        for i in run.from..run.from.saturating_add(run.iters) {
            let (damaged, mutations) = damaged_copy(&clip, run.seed0, ci, i);
            if let Err(e) = run_guarded(move || work(&damaged), run.limit, last_panic) {
                let hung = e.starts_with("hang:");
                failures.push(format!(
                    "{} iter={i}: {} -> {e}\n  replay: {}",
                    clip.name,
                    mutations.join("; "),
                    replay_command(run.seed0, &clip.name, i)
                ));
                if hung {
                    failures.push("stopped at the first hang: its thread is still running".to_string());
                    break 'clips;
                }
            }
        }
        eprintln!("{}: {} clean pictures, {} mutations done", clip.name, clean, run.iters);
    }
    failures
}

/// A clip filter with a typo is an error, not a run that decodes nothing and passes.
#[test]
fn clip_filter_that_matches_no_clip_is_an_error() {
    let _serial = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let last_panic = Arc::new(Mutex::new(String::new()));
    let run = Run { seed0: 1, from: 0, iters: 1, only: Some("clip-hevc.mp44"), limit: HANG };
    let result = panic::catch_unwind(AssertUnwindSafe(|| fuzz(vec![demux_mp4("clip-hevc.mp4")], &run, decode, &last_panic)));
    assert!(result.is_err(), "a filter that matches no clip passed: {result:?}");
}

static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Decodes once (the clean-clip control), then hangs on every later call.
fn hang_after_the_first_call(_: &Clip) -> usize {
    if CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
        return 1;
    }
    loop {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The first hang ends the whole run: no more mutations of that clip, no later clip.
#[test]
fn fuzzing_stops_at_the_first_hang() {
    let _serial = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let last_panic = Arc::new(Mutex::new(String::new()));
    let mut second = demux_mp4("clip-hevc.mp4");
    second.name = "second clip".to_string();
    let run = Run { seed0: 1, from: 0, iters: 5, only: None, limit: Duration::from_millis(200) };
    let failures = fuzz(vec![demux_mp4("clip-hevc.mp4"), second], &run, hang_after_the_first_call, &last_panic);
    assert_eq!(failures.len(), 2, "{failures:#?}");
    assert!(failures[0].contains("hang:") && failures[1].starts_with("stopped at the first hang"), "{failures:#?}");
    assert_eq!(CALLS.load(std::sync::atomic::Ordering::SeqCst), 2, "the run went on after the first hang");
}
