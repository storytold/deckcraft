//! Decoding and probing every supported format.
//!
//! WAV and AIFF are written here; the compressed fixtures in `tests/fixtures` are a 0.5 s, 440 Hz
//! sine (amplitude 1/8) and a 6-frame 64×48 colour gradient we synthesised and encoded with
//! `fixtures/make.sh` (CC0, see ATTRIBUTION.md).

use std::sync::Arc;
use std::time::Duration;

use deckcraft_media::{Bytes, Container, MediaError, audio, probe, video};

fn fixture(name: &str) -> Bytes {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    Arc::new(std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
}

fn sine(rate: u32, secs: f64) -> Vec<f32> {
    (0..(rate as f64 * secs) as usize).map(|i| (i as f64 * 440.0 * std::f64::consts::TAU / rate as f64).sin() as f32 * 0.125).collect()
}

fn wav(rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| ((s * 32767.0) as i16).to_le_bytes()).collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    b
}

/// 80-bit IEEE extended sample rate for AIFF's COMM chunk.
fn ext80(v: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    let e = 31 - v.leading_zeros();
    let exp = (16383 + e) as u16;
    out[..2].copy_from_slice(&exp.to_be_bytes());
    let mant = (v as u64) << (63 - e);
    out[2..].copy_from_slice(&mant.to_be_bytes());
    out
}

fn aiff(rate: u32, samples: &[f32]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| ((s * 32767.0) as i16).to_be_bytes()).collect();
    let mut comm = Vec::new();
    comm.extend_from_slice(&1u16.to_be_bytes());
    comm.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    comm.extend_from_slice(&16u16.to_be_bytes());
    comm.extend_from_slice(&ext80(rate));
    let mut b = Vec::new();
    b.extend_from_slice(b"FORM");
    b.extend_from_slice(&((4 + 8 + comm.len() + 8 + 8 + data.len()) as u32).to_be_bytes());
    b.extend_from_slice(b"AIFF");
    b.extend_from_slice(b"COMM");
    b.extend_from_slice(&(comm.len() as u32).to_be_bytes());
    b.extend_from_slice(&comm);
    b.extend_from_slice(b"SSND");
    b.extend_from_slice(&((8 + data.len()) as u32).to_be_bytes());
    b.extend_from_slice(&[0; 8]);
    b.extend_from_slice(&data);
    b
}

/// Frequency of channel 0 from its zero crossings (with hysteresis, so codec noise around silence
/// doesn't count), over the span between the first and last crossing.
fn frequency(p: &audio::Pcm) -> f64 {
    let mut sign = 0i8;
    let mut crossings = vec![];
    for i in 0..p.frames() {
        let s = p.at(i, 0);
        let now = if s > 0.03 {
            1
        } else if s < -0.03 {
            -1
        } else {
            0
        };
        if now != 0 && now != sign {
            if sign != 0 {
                crossings.push(i);
            }
            sign = now;
        }
    }
    match (crossings.first(), crossings.last()) {
        (Some(a), Some(b)) if b > a => (crossings.len() - 1) as f64 / 2.0 / ((b - a) as f64 / p.rate as f64),
        _ => 0.0,
    }
}

/// The tone decodes to ~0.5 s of a 440 Hz sine at amplitude 1/8.
fn check_tone(name: &str, bytes: &Bytes, rate: u32, channels: u16) {
    let p = audio::decode(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!((p.rate, p.channels), (rate, channels), "{name}");
    assert!((p.duration() - 0.5).abs() < 0.06, "{name}: {} s", p.duration());
    let f = frequency(&p);
    assert!((f - 440.0).abs() < 15.0, "{name}: {f} Hz");
    let rms = p.rms();
    // 1/8 / √2 (ffmpeg's mono → stereo upmix for the Vorbis fixture lowers it by another √2).
    assert!((0.055..0.105).contains(&rms), "{name}: rms {rms}");
    let info = probe(bytes).unwrap_or_else(|e| panic!("{name}: probe {e}"));
    let a = info.audio.unwrap_or_else(|| panic!("{name}: no audio info"));
    assert!(a.decodable, "{name}");
    assert_eq!((a.sample_rate, a.channels), (rate, channels), "{name}");
    assert!((info.duration_ms as i64 - 500).abs() <= 60, "{name}: {} ms", info.duration_ms);
    assert!(info.video.is_none(), "{name}");
}

#[test]
fn wav_written_in_code() {
    let b: Bytes = Arc::new(wav(44_100, 1, &sine(44_100, 0.5)));
    assert_eq!(Container::sniff(&b), Container::Wav);
    check_tone("wav", &b, 44_100, 1);
    // Stereo.
    let st: Vec<f32> = sine(22_050, 0.5).iter().flat_map(|s| [*s, *s]).collect();
    check_tone("wav stereo", &Arc::new(wav(22_050, 2, &st)), 22_050, 2);
}

#[test]
fn aiff_written_in_code() {
    let b: Bytes = Arc::new(aiff(48_000, &sine(48_000, 0.5)));
    assert_eq!(Container::sniff(&b), Container::Aiff);
    check_tone("aiff", &b, 48_000, 1);
}

#[test]
fn mp3() {
    let b = fixture("tone.mp3");
    assert_eq!(Container::sniff(&b), Container::Mp3);
    check_tone("mp3", &b, 44_100, 1);
}

#[test]
fn aac_m4a_and_adts() {
    check_tone("m4a", &fixture("tone.m4a"), 44_100, 1);
    let adts = fixture("tone.aac");
    assert_eq!(Container::sniff(&adts), Container::Adts);
    check_tone("aac", &adts, 44_100, 1);
}

#[test]
fn alac() {
    check_tone("alac", &fixture("tone-alac.m4a"), 44_100, 1);
}

#[test]
fn flac() {
    check_tone("flac", &fixture("tone.flac"), 44_100, 1);
}

#[test]
fn ogg_vorbis() {
    check_tone("vorbis", &fixture("tone.ogg"), 44_100, 2);
}

#[test]
fn ogg_opus() {
    check_tone("opus", &fixture("tone.opus"), 48_000, 1);
}

#[test]
fn wma_is_probed_but_not_decoded() {
    let b = fixture("tone.wma");
    assert_eq!(Container::sniff(&b), Container::Asf);
    let info = probe(&b).expect("probe");
    let a = info.audio.clone().expect("audio");
    assert_eq!((a.codec.as_str(), a.sample_rate, a.channels, a.decodable), ("WMA 2", 44_100, 1, false));
    assert!((info.duration_ms as i64 - 500).abs() <= 120, "{}", info.duration_ms);
    assert!(!info.playable());
    assert!(matches!(audio::decode(&b), Err(MediaError::Unsupported(_))));
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let b: Bytes = Arc::new((0..4000u32).map(|i| (i * 7919 % 251) as u8).collect());
    assert!(probe(&b).is_err());
    assert!(audio::decode(&b).is_err());
    assert!(video::VideoDecoder::open(b.clone()).is_err());
    // Truncated real files too.
    for name in ["tone.mp3", "tone.m4a", "tone.opus", "clip-h264.mp4", "clip-vp9.webm"] {
        let full = fixture(name);
        let cut: Bytes = Arc::new(full[..full.len() / 3].to_vec());
        let _ = probe(&cut);
        let _ = audio::decode(&cut);
        if let Ok(mut d) = video::VideoDecoder::open(cut) {
            while let Ok(Some(_)) = d.next_frame() {}
        }
    }
}

/// Mean colour of a 16×16 block in the middle of the frame.
fn centre(f: &video::Frame) -> [f64; 3] {
    let (w, h) = (f.width as usize, f.height as usize);
    let mut s = [0.0; 3];
    let mut n = 0.0;
    for y in h / 2 - 8..h / 2 + 8 {
        for x in w / 2 - 8..w / 2 + 8 {
            for (c, v) in s.iter_mut().enumerate() {
                *v += f.rgba[(y * w + x) * 4 + c] as f64;
            }
            n += 1.0;
        }
    }
    s.map(|v| v / n)
}

/// Our gradient's mean over the same block in frame `k`: R = (4x + 20k) mod 256, G = 5y mod 256,
/// B = 40k mod 256.
fn expected_centre(w: usize, h: usize, k: usize) -> [f64; 3] {
    let mut s = [0.0; 3];
    let mut n = 0.0;
    for y in h / 2 - 8..h / 2 + 8 {
        for x in w / 2 - 8..w / 2 + 8 {
            s[0] += ((x * 4 + k * 20) % 256) as f64;
            s[1] += ((y * 5) % 256) as f64;
            s[2] += ((k * 40) % 256) as f64;
            n += 1.0;
        }
    }
    s.map(|v| v / n)
}

fn check_video(name: &str, w: u32, h: u32, frames: usize, codec: &str) {
    let b = fixture(name);
    let info = probe(&b).unwrap_or_else(|e| panic!("{name}: {e}"));
    let v = info.video.clone().unwrap_or_else(|| panic!("{name}: no video info"));
    assert_eq!((v.codec.as_str(), v.width, v.height, v.decodable), (codec, w, h, true), "{name}");
    assert!((v.fps - 10.0).abs() < 0.6, "{name}: {} fps", v.fps);
    assert!((info.duration_ms as i64 - frames as i64 * 100).abs() <= 120, "{name}: {} ms", info.duration_ms);
    let mut d = video::VideoDecoder::open(b.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut got = vec![];
    while let Some(f) = d.next_frame().unwrap_or_else(|e| panic!("{name}: {e}")) {
        assert_eq!((f.width, f.height, f.rgba.len()), (w, h, (w * h * 4) as usize), "{name}");
        got.push(f);
    }
    assert_eq!(got.len(), frames, "{name}");
    for (k, f) in got.iter().enumerate() {
        assert!((f.time - k as f64 * 0.1).abs() < 0.02, "{name}: frame {k} at {}", f.time);
        let (c, e) = (centre(f), expected_centre(w as usize, h as usize, k));
        for i in 0..3 {
            assert!((c[i] - e[i]).abs() < 24.0, "{name}: frame {k} channel {i}: {c:?} vs {e:?}");
        }
    }
    // Seeking lands on the frame shown at that time.
    d.seek(0.35);
    let f = d.next_frame().ok().flatten().unwrap_or_else(|| panic!("{name}: no frame after seek"));
    assert!((f.time - 0.3).abs() < 0.02, "{name}: seek gave {}", f.time);
    // Poster frames.
    let p = video::poster_frame(&b, 0.25).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!((p.time - 0.2).abs() < 0.02, "{name}: poster at {}", p.time);
    let png = video::poster_png(&b, 0.0).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
}

#[test]
fn h264_mp4_with_b_frames_and_aac_audio() {
    check_video("clip-h264.mp4", 64, 48, 5, "H.264");
    // Its audio track decodes too.
    let b = fixture("clip-h264.mp4");
    let info = probe(&b).expect("probe");
    assert_eq!(info.audio.as_ref().map(|a| (a.codec.as_str(), a.decodable)), Some(("AAC", true)));
    let p = audio::decode(&b).expect("audio of the video");
    assert!((frequency(&p) - 440.0).abs() < 15.0);
}

#[test]
fn h264_mov() {
    check_video("clip-h264.mov", 64, 48, 6, "H.264");
    assert_eq!(probe(&fixture("clip-h264.mov")).map(|i| i.container), Ok("mov"));
}

#[test]
fn hevc_mp4() {
    check_video("clip-hevc.mp4", 64, 48, 6, "HEVC");
}

#[test]
fn vp9_webm_with_opus_audio() {
    check_video("clip-vp9.webm", 64, 48, 5, "VP9");
    let b = fixture("clip-vp9.webm");
    assert_eq!(probe(&b).ok().and_then(|i| i.audio).map(|a| (a.codec, a.sample_rate)), Some(("Opus".to_string(), 48_000)));
    let p = audio::decode(&b).expect("opus in webm");
    assert_eq!(p.rate, 48_000);
    assert!((frequency(&p) - 440.0).abs() < 15.0);
}

#[test]
fn av1_mkv() {
    check_video("clip-av1.mkv", 64, 64, 6, "AV1");
}

#[test]
fn video_feed_follows_the_clock() {
    let mut feed = deckcraft_media::VideoFeed::new(fixture("clip-h264.mov")).expect("feed");
    assert_eq!(feed.size(), (64, 48));
    let t = Duration::from_secs(5);
    assert!(feed.frame_at_blocking(0.0, t).is_some_and(|f| f.time == 0.0));
    let f = feed.frame_at_blocking(0.25, t).map(|f| f.time);
    assert!(f.is_some_and(|v| (v - 0.2).abs() < 0.02), "{f:?}");
    // Jumping back (a loop) seeks.
    let f = feed.frame_at_blocking(0.05, t).map(|f| f.time);
    assert_eq!(f, Some(0.0));
    let f = feed.frame_at_blocking(0.55, t).map(|f| f.time);
    assert!(f.is_some_and(|v| (v - 0.5).abs() < 0.02), "{f:?}");
}

#[test]
fn video_feed_repeated_seeks_keep_the_requested_frame() {
    let timeout = Duration::from_secs(5);
    for name in ["clip-h264.mov", "clip-hevc.mp4", "clip-vp9.webm", "clip-av1.mkv"] {
        let mut feed = deckcraft_media::VideoFeed::new(fixture(name)).expect("feed");
        for _ in 0..20 {
            // A decoder can finish this small clip before the consumer next polls it.
            // Rewinding must keep the new generation's first frame, even at end of stream.
            feed.seek(0.05);
            let frame = feed.frame_at_blocking(0.05, timeout).expect("frame after rewind");
            assert_eq!(frame.time, 0.0, "{name}");
            let frame = feed.frame_at_blocking(0.45, timeout).expect("frame after advancing");
            assert!((frame.time - 0.4).abs() < 0.02, "{name}: {}", frame.time);
        }
    }
}

#[test]
fn player_plays_decoded_audio_through_the_mixer() {
    use deckcraft_media::{ClipParams, Player, VoiceSpec};
    let b: Bytes = Arc::new(wav(48_000, 1, &sine(48_000, 0.5)));
    let mut p = Player::new(None);
    p.play(1, 1, &b, VoiceSpec { clip: ClipParams { volume: 0.5, ..Default::default() }, looping: false, duration: 0.5, audio: true }, None);
    assert!(p.wait_loaded(1, Duration::from_secs(5)));
    let mut buf = vec![0.0f32; 4800 * 2];
    p.render(&mut buf, 2, 48_000);
    let rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
    assert!((rms - 0.0884 * 0.5).abs() < 0.005, "{rms}");
    assert!((p.status(1).map(|s| s.position).unwrap_or(0.0) - 0.1).abs() < 1e-6);
}

/// Damaged files found by mutation fuzzing the fixtures above (so CC0 like them): each must come
/// back as an error or as audio — not panic (symphonia's Matroska reader panics on the first two)
/// and not hang (the MP4 demuxer reported the same error forever on the last two).
#[test]
fn damaged_media_neither_panics_nor_hangs() {
    for name in ["damaged/ebml-header.mkv", "damaged/ebml-position.webm", "damaged/demux-loop.mp4", "damaged/demux-loop.m4a"] {
        let b = fixture(name);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ok = std::panic::catch_unwind(|| {
                let _ = probe::probe(&b);
                let _ = audio::decode(&b);
                if let Ok(mut d) = video::VideoDecoder::open(b.clone()) {
                    for _ in 0..12 {
                        if !matches!(d.next_frame(), Ok(Some(_))) {
                            break;
                        }
                    }
                }
            })
            .is_ok();
            let _ = tx.send(ok);
        });
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(ok) => assert!(ok, "{name} panicked"),
            Err(_) => panic!("{name} still decoding after 20 s"),
        }
    }
}

/// MP4s whose sample tables were damaged by a mutation fuzzer (from `tone.m4a`, so CC0 like it):
/// one claims billions of `stsc` entries, one a 4 GB sample. Symphonia reserved that much memory
/// up front and the process aborted; now they are corrupt files.
#[test]
fn mp4_sample_tables_that_overrun_the_file_are_corrupt() {
    for name in ["damaged/stsc-count.m4a", "damaged/stsz-size.m4a"] {
        let b = fixture(name);
        assert!(matches!(audio::decode(&b), Err(MediaError::Corrupt(_))), "{name}");
        let _ = probe::probe(&b);
    }
    for name in ["tone.m4a", "tone-alac.m4a", "clip-h264.mp4", "clip-h264.mov", "clip-hevc.mp4"] {
        assert!(audio::decode(&fixture(name)).is_ok() || name.starts_with("clip"), "{name} still decodes");
    }
}
