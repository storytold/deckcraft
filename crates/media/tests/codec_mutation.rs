//! Mutation fuzzing of the hand-written video decoders (AV1, VP9, HEVC) at the bitstream level.
//!
//! `damaged_media_neither_panics_nor_hangs` (decode.rs) damages whole files, so most mutations land
//! in the container and never reach a decoder. Here the clips are demuxed the way `video.rs` does,
//! and the compressed samples themselves are damaged (truncated, bit-flipped, overwritten, dropped,
//! repeated, reordered) before they reach the decoder. Every mutation must give an error or
//! pictures: never a panic, never a hang (AGENTS.md: never crash).
//!
//! The AV1 film grain clips (`clip-av1-grain-*.mkv`) are there so the mutations reach the grain
//! synthesis too; `av1_grain_fixtures_carry_film_grain` checks they really carry grain.
//!
//! Seeded and deterministic. `DECKCRAFT_MUTATION_ITERS` (default 200 per codec) runs it longer;
//! `DECKCRAFT_MUTATION_SEED` replays from another seed. A failure prints codec, seed, iteration and
//! mutation so it can be replayed and turned into a fixture.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deckcraft_isobmff::{CodecConfig, TrackKind as Mp4Kind};
use deckcraft_matroska::{Codec as MkvCodec, TrackKind as MkvKind};

/// How long one decode of a ~5-frame 64×48 clip may take before it counts as a hang.
const HANG: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
enum Kind {
    Av1,
    Vp9,
    Hevc,
}

/// A demuxed clip: codec, its configuration record (av1C config OBUs or hvcC) and the samples.
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
    let config = match &t.entries.first().expect("sample entry").codec {
        CodecConfig::Hevc(c) => c.to_bytes(),
        _ => panic!("{name}: expected HEVC"),
    };
    let samples = (0..t.samples.len()).map(|i| f.read_sample(&bytes, ti, i).expect("sample")).collect();
    Clip { name: name.to_string(), kind: Kind::Hevc, config, samples }
}

/// Decodes a clip the way `video.rs` drives each decoder; returns the number of pictures.
/// Decoder errors are fine (damaged input); only panics and hangs are failures.
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

/// The panic hook is process-wide: the two tests below take this lock so they never share it.
static HOOK: Mutex<()> = Mutex::new(());

/// Installs a hook that records where the last panic happened (the payload alone lacks it).
fn record_panics() -> Arc<Mutex<String>> {
    let last_panic = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&last_panic);
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
    }));
    last_panic
}

/// The grain fixtures must really exercise film grain synthesis, or fuzzing them adds nothing:
/// the sequence header says so, and decoding with grain changes the pictures.
#[test]
fn av1_grain_fixtures_carry_film_grain() {
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

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[test]
fn damaged_video_bitstreams_neither_panic_nor_hang() {
    let _serial = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    let last_panic = record_panics();
    let iters = env_u64("DECKCRAFT_MUTATION_ITERS", 200);
    let seed0 = env_u64("DECKCRAFT_MUTATION_SEED", 0x00de_c0de);
    let clips = [
        demux_mkv("clip-av1.mkv"),
        // Film grain synthesis (crates/av1/src/grain.rs) only runs on streams that carry grain.
        demux_mkv("clip-av1-grain-4.mkv"),
        demux_mkv("clip-av1-grain-6.mkv"),
        demux_mkv("clip-av1-grain-15.mkv"),
        demux_mkv("clip-vp9.webm"),
        demux_mp4("clip-hevc.mp4"),
    ];
    let mut failures = Vec::new();

    for (ci, clip) in clips.into_iter().enumerate() {
        // Control: the clean clip must decode to pictures, or the harness tests nothing.
        let c = clip.clone();
        let clean = run_guarded(move || decode(&c), HANG, &last_panic).unwrap_or_else(|e| panic!("{} clean clip: {e}", clip.name));
        assert!(clean > 0, "{}: the clean clip decoded to no pictures, the harness is broken", clip.name);

        for i in 0..iters {
            let seed = seed0 ^ (i.wrapping_mul(0x9e37_79b9_7f4a_7c15)) ^ (ci as u64) << 56;
            let mut rng = Rng(seed | 1);
            let mut damaged = clip.clone();
            let steps = 1 + rng.below(3);
            let mutations: Vec<String> = (0..steps).map(|_| mutate(&mut damaged, &mut rng)).collect();
            if let Err(e) = run_guarded(move || decode(&damaged), HANG, &last_panic) {
                failures.push(format!("{} seed={seed:#x} iter={i}: {} -> {e}", clip.name, mutations.join("; ")));
            }
        }
        eprintln!("{}: {} clean pictures, {iters} mutations done", clip.name, clean);
    }

    let _ = panic::take_hook();
    assert!(failures.is_empty(), "{} damaged bitstreams panicked or hung:\n{}", failures.len(), failures.join("\n"));
}
