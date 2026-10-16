//! Video export (File › Export › Create a Video): the deck as an MP4 or MOV, one frame stream
//! of each slide's transition then the slide held for its timing. Frames come from the slide
//! renderer and are encoded with DeckCraft's own H.264 encoder ([`deckcraft_h264::Encoder`]:
//! intra-coded changes, skipped unchanged macroblocks) into an MP4 or QuickTime file
//! ([`deckcraft_isobmff::Mp4Writer`]); no external tool is needed. Animations within a slide are
//! shown in their final state, and there is no audio track.

use deckcraft_model::Presentation;

/// The most frames one export writes (two hours at 30 fps), against runaway timings.
pub const MAX_FRAMES: u64 = 2 * 60 * 60 * 30;

/// Export options, as `file.export` takes them.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoOpts {
    /// Frame height in pixels (PowerPoint's 480p / 720p / 1080p / 4K); the width follows the
    /// slide's aspect ratio. Both are rounded to even numbers, as H.264 needs.
    pub height: u32,
    pub fps: u32,
    /// How long a slide stays up when it has no "advance after" timing of its own.
    pub seconds_per_slide: f64,
    pub include_hidden: bool,
}

impl Default for VideoOpts {
    fn default() -> Self {
        VideoOpts { height: 1080, fps: 30, seconds_per_slide: 5.0, include_hidden: false }
    }
}

/// One step of the timeline: a transition frame at progress `t`, or slide `index` held for
/// `frames` frames.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Transition { index: usize, t: f64 },
    Hold { index: usize, frames: u64 },
}

/// The frame size for `opts` on this deck's slide size: even, at least 2×2.
pub fn frame_size(pres: &Presentation, opts: &VideoOpts) -> (u32, u32) {
    let h = opts.height.clamp(2, 4320) & !1;
    let aspect = pres.slide_size.width / pres.slide_size.height.max(1.0);
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect.clamp(0.1, 10.0) } else { 16.0 / 9.0 };
    let w = ((h as f64 * aspect).round() as u32).clamp(2, 8192) & !1;
    (w, h)
}

/// Seconds as frames at `fps`, at least one.
fn frames_for(seconds: f64, fps: u32) -> u64 {
    let s = if seconds.is_finite() { seconds.clamp(0.0, 3600.0) } else { 0.0 };
    ((s * fps as f64).round() as u64).max(1)
}

/// The timeline: for each slide shown, its transition's frames (none for "none", a cut or a
/// zero duration) then the slide held for its "advance after" time, else `seconds_per_slide`.
/// The total is capped at [`MAX_FRAMES`].
pub fn timeline(pres: &Presentation, opts: &VideoOpts) -> Vec<Step> {
    let fps = opts.fps.clamp(1, 60);
    let mut out = vec![];
    let mut total: u64 = 0;
    for (index, slide) in pres.slides.iter().enumerate() {
        if slide.hidden && !opts.include_hidden {
            continue;
        }
        if let Some(tr) = slide.transition.as_ref().filter(|t| !matches!(t.kind.as_str(), "none" | "cut" | "") && t.duration_ms > 0) {
            let n = frames_for(tr.duration_ms as f64 / 1000.0, fps);
            for k in 0..n {
                if total >= MAX_FRAMES {
                    return out;
                }
                out.push(Step::Transition { index, t: k as f64 / n as f64 });
                total = total.saturating_add(1);
            }
        }
        let secs = slide.transition.as_ref().and_then(|t| t.advance_after_ms).map(|ms| ms as f64 / 1000.0).unwrap_or(opts.seconds_per_slide);
        let frames = frames_for(secs, fps).min(MAX_FRAMES.saturating_sub(total));
        if frames == 0 {
            return out;
        }
        out.push(Step::Hold { index, frames });
        total = total.saturating_add(frames);
    }
    out
}

/// An image fitted to exactly `w`×`h` RGBA (the renderer's size can differ by a pixel): copied
/// into a black frame, cropped where it overhangs.
fn fit(img: &deckcraft_render::Image, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w as usize).saturating_mul(h as usize).saturating_mul(4)];
    for px in out.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let cw = img.width.min(w) as usize;
    for y in 0..img.height.min(h) as usize {
        let src = y.saturating_mul(img.width as usize).saturating_mul(4);
        let dst = y.saturating_mul(w as usize).saturating_mul(4);
        if let (Some(s), Some(d)) = (img.pixels.get(src..src + cw * 4), out.get_mut(dst..dst + cw * 4)) {
            d.copy_from_slice(s);
        }
    }
    out
}

/// The renderer options for a frame `h` pixels high.
fn render_opts(pres: &Presentation, h: u32) -> deckcraft_render::RenderOpts<'static> {
    let scale = h as f64 / pres.slide_size.height.max(1.0);
    deckcraft_render::RenderOpts { scale: if scale.is_finite() { scale.clamp(0.01, 16.0) } else { 1.0 }, ..Default::default() }
}

/// Every frame of `steps`, in order, handed to `sink` (RGBA, `w`×`h`). A held slide is rendered
/// once. Stops at the first error `sink` returns.
pub fn frames(pres: &Presentation, steps: &[Step], (w, h): (u32, u32), sink: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<u64, String> {
    let opts = render_opts(pres, h);
    let mut n: u64 = 0;
    for step in steps {
        match step {
            Step::Transition { index, t } => {
                let img = deckcraft_render::render_transition(pres, *index, *t, &opts);
                sink(&fit(&img, w, h))?;
                n = n.saturating_add(1);
            }
            Step::Hold { index, frames } => {
                let img = fit(&deckcraft_render::render_slide(pres, *index, &opts), w, h);
                for _ in 0..*frames {
                    sink(&img)?;
                    n = n.saturating_add(1);
                }
            }
        }
    }
    Ok(n)
}

/// A key frame at least this often (seconds), so players can seek.
const KEY_EVERY_S: u32 = 2;

/// Encode the deck to `path` as `mp4` or `mov`, at quality `qp` (H.264 QP, lower is better).
/// The file is assembled in memory and written through a temporary sibling, so a failed export
/// leaves no half file. Returns the frame count and size.
pub fn encode(pres: &Presentation, format: &str, opts: &VideoOpts) -> Result<(Vec<u8>, u64, (u32, u32)), String> {
    use deckcraft_isobmff::{AvcConfig, Brand, CodecConfig, FourCc, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions};
    let steps = timeline(pres, opts);
    if steps.is_empty() {
        return Err("there are no slides to put in the video".into());
    }
    let (w, h) = frame_size(pres, opts);
    let fps = opts.fps.clamp(1, 60);
    let mut enc = deckcraft_h264::Encoder::new(deckcraft_h264::EncoderConfig { width: w, height: h, fps, qp: 24 }).map_err(|e| e.to_string())?;
    let avc = AvcConfig::new(vec![enc.sps()], vec![enc.pps()], 4);
    let entry = SampleEntry::video(FourCc::new(b"avc1"), CodecConfig::Avc(avc), w.min(u32::from(u16::MAX)) as u16, h.min(u32::from(u16::MAX)) as u16);
    let brand = if format == "mov" { Brand::Mov } else { Brand::Mp4 };
    let mut mux = Mp4Writer::new(std::io::Cursor::new(Vec::new()), WriterOptions::new(brand)).map_err(|e| e.to_string())?;
    let track = mux.add_track(TrackConfig::new(entry, fps)).map_err(|e| e.to_string())?;
    let key_every = u64::from(fps * KEY_EVERY_S);
    let mut since_key: u64 = 0;
    let mut total: u64 = 0;
    for (i, step) in steps.iter().enumerate() {
        // A key frame starts each slide and each transition, so the file seeks by slide.
        let continues = matches!((i.checked_sub(1).and_then(|p| steps.get(p)), step), (Some(Step::Transition { index: a, .. }), Step::Transition { index: b, .. }) if a == b);
        let mut start = !continues;
        let mut sink = |px: &[u8]| -> Result<(), String> {
            let pic = enc.encode_rgba(px, start || since_key >= key_every);
            start = false;
            since_key = if pic.key { 1 } else { since_key.saturating_add(1) };
            let data = pic.length_prefixed();
            mux.write_sample(track, WriteSample { data: &data, duration: 1, composition_offset: 0, is_sync: pic.key }).map_err(|e| e.to_string())
        };
        total = total.saturating_add(frames(pres, std::slice::from_ref(step), (w, h), &mut sink)?);
    }
    let n = total;
    let bytes = mux.finish_faststart().map_err(|e| e.to_string())?.into_inner();
    Ok((bytes, n, (w, h)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::anim::Transition;

    fn deck(n: usize) -> Presentation {
        let mut p = deckcraft_model::defaults::new_presentation(None);
        while p.slides.len() < n {
            let s = p.slides[0].clone();
            p.slides.push(s);
        }
        p.slides.truncate(n);
        p
    }

    #[test]
    fn timeline_holds_slides_and_plays_transitions() {
        let mut p = deck(3);
        std::sync::Arc::make_mut(&mut p.slides[1]).transition =
            Some(Transition { kind: "fade".into(), duration_ms: 500, advance_after_ms: Some(2000), ..Default::default() });
        std::sync::Arc::make_mut(&mut p.slides[2]).hidden = true;
        let o = VideoOpts { fps: 10, seconds_per_slide: 3.0, ..Default::default() };
        let t = timeline(&p, &o);
        assert_eq!(t.first(), Some(&Step::Hold { index: 0, frames: 30 }));
        assert_eq!(t.iter().filter(|s| matches!(s, Step::Transition { index: 1, .. })).count(), 5);
        assert_eq!(t.last(), Some(&Step::Hold { index: 1, frames: 20 }), "the hidden slide is left out");
        let with_hidden = timeline(&p, &VideoOpts { include_hidden: true, ..o });
        assert_eq!(with_hidden.last(), Some(&Step::Hold { index: 2, frames: 30 }));
    }

    #[test]
    fn timeline_is_capped_and_sizes_are_even() {
        let mut p = deck(2);
        for s in &mut p.slides {
            std::sync::Arc::make_mut(s).transition = Some(Transition { advance_after_ms: Some(u32::MAX), ..Default::default() });
        }
        let total: u64 = timeline(&p, &VideoOpts { fps: 60, ..Default::default() })
            .iter()
            .map(|s| match s {
                Step::Hold { frames, .. } => *frames,
                Step::Transition { .. } => 1,
            })
            .sum();
        assert!(total <= MAX_FRAMES);
        let (w, h) = frame_size(&p, &VideoOpts { height: 721, ..Default::default() });
        assert_eq!((w % 2, h), (0, 720));
    }

    #[test]
    fn frames_are_exactly_the_frame_size() {
        let p = deck(1);
        let size = frame_size(&p, &VideoOpts { height: 90, ..Default::default() });
        let mut seen = vec![];
        let n = frames(&p, &[Step::Hold { index: 0, frames: 2 }], size, &mut |px| {
            seen.push(px.len());
            Ok(())
        });
        assert_eq!(n, Ok(2));
        assert!(seen.iter().all(|l| *l == size.0 as usize * size.1 as usize * 4));
    }

    #[test]
    fn encodes_a_playable_mp4() {
        let mut p = deck(2);
        std::sync::Arc::make_mut(&mut p.slides[1]).transition = Some(Transition { kind: "fade".into(), duration_ms: 200, ..Default::default() });
        let o = VideoOpts { height: 72, fps: 10, seconds_per_slide: 0.5, include_hidden: false };
        let (bytes, n, (w, h)) = encode(&p, "mp4", &o).unwrap();
        assert_eq!(n, 5 + 2 + 5);
        let f = deckcraft_isobmff::open(bytes.as_slice()).unwrap();
        let t = &f.tracks[0];
        assert_eq!(t.samples.len(), n as usize);
        // Key frames start each slide and each transition.
        let keys: Vec<usize> = t.samples.iter().enumerate().filter(|(_, s)| s.is_sync).map(|(i, _)| i).collect();
        assert_eq!(keys, vec![0, 5, 7]);
        let mut dec = deckcraft_h264::Decoder::new();
        // Rebuild an Annex B stream from the length-prefixed samples and the avcC parameter sets.
        let Some(deckcraft_isobmff::CodecConfig::Avc(avc)) = t.entries.first().map(|e| &e.codec) else { panic!("not AVC") };
        let mut stream = vec![];
        for ps in avc.sps.iter().chain(&avc.pps) {
            stream.extend([0, 0, 0, 1]);
            stream.extend(ps);
        }
        for i in 0..t.samples.len() {
            let data = f.read_sample(bytes.as_slice(), 0, i).unwrap();
            let mut k = 0;
            while k + 4 <= data.len() {
                let len = u32::from_be_bytes([data[k], data[k + 1], data[k + 2], data[k + 3]]) as usize;
                stream.extend([0, 0, 0, 1]);
                stream.extend(&data[k + 4..k + 4 + len]);
                k += 4 + len;
            }
        }
        let mut pics = dec.decode(&stream, 0).unwrap();
        pics.extend(dec.flush());
        assert_eq!(pics.len(), n as usize);
        assert_eq!((pics[0].width, pics[0].height), (w, h));
    }
}
