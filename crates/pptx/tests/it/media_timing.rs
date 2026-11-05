//! Media settings PowerPoint keeps in the timing tree, spliced into an exported deck and read back.

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::{MediaClip, ShapeId, ShapeKind, defaults};

fn node(tag: &str, attrs: &str, spid: u32, media: &str, ctn: &str) -> String {
    format!(
        r#"<p:par><p:cTn id="2" fill="hold"><p:childTnLst><p:{tag}{attrs}><p:cMediaNode{media}><p:cTn id="3"{ctn}/><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cMediaNode></p:{tag}></p:childTnLst></p:cTn></p:par>"#
    )
}

fn clips(nodes: impl Fn(u32, u32) -> String) -> Vec<MediaClip> {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
    let audio = p.add_media("clip.m4a", "audio/mp4", b"audio".to_vec()).expect("media");
    let mut ids = vec![];
    for (media, is_video) in [(video, true), (audio, false)] {
        let id = ShapeId(p.alloc_id().expect("id"));
        ids.push(id.0);
        let s = deckcraft_model::Shape {
            id,
            name: "Clip".into(),
            xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
            kind: ShapeKind::Media(MediaClip { media, video: is_video, volume: 1.0, ..Default::default() }),
            ..Default::default()
        };
        Arc::make_mut(&mut p.slides[0]).shapes.push(s);
    }
    let timing = format!(
        r#"<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst>{}</p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"#,
        nodes(ids[0], ids[1])
    );
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        if f.name() == "ppt/slides/slide1.xml" {
            let s = String::from_utf8(d).expect("utf8");
            assert!(!s.contains("<p:timing"));
            let end = s.rfind("</p:sld>").expect("end");
            d = format!("{}{timing}{}", &s[..end], &s[end..]).into_bytes();
        }
        out.start_file(f.name(), zip::write::SimpleFileOptions::default()).expect("start");
        out.write_all(&d).expect("write");
    }
    let q = deckcraft_pptx::import(&out.finish().expect("finish").into_inner()).expect("import");
    q.slides[0]
        .shapes
        .iter()
        .filter_map(|s| match &s.kind {
            ShapeKind::Media(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn video_settings_are_read_from_the_timing_tree() {
    let c = clips(|v, _| node("video", r#" fullScrn="1""#, v, r#" vol="80000" showWhenStopped="0""#, r#" repeatCount="indefinite""#));
    assert_eq!((c[0].volume, c[0].hide_while_not_playing, c[0].loop_play, c[0].full_screen), (0.8, true, true, true));
    // The sound next to it has no node: defaults stay.
    assert_eq!((c[1].volume, c[1].hide_while_not_playing, c[1].loop_play, c[1].full_screen), (1.0, false, false, false));
}

#[test]
fn muted_audio_has_no_volume_and_other_clips_keep_theirs() {
    let c = clips(|_, a| node("audio", "", a, r#" vol="50000" mute="1""#, ""));
    assert_eq!(c[0].volume, 1.0);
    assert_eq!((c[1].volume, c[1].loop_play, c[1].full_screen), (0.0, false, false));
}

/// A time node that starts `indefinite` waits for a click, so the clip under it is not
/// slide-start media and must not come back as autoplay.
fn click_node(tag: &str, spid: u32) -> String {
    format!(
        r#"<p:par><p:cTn id="2" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst><p:{tag}><p:cMediaNode><p:cTn id="3"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cMediaNode></p:{tag}></p:childTnLst></p:cTn></p:par>"#
    )
}

#[test]
fn a_click_triggered_clip_is_not_autoplay() {
    let c = clips(|v, a| format!("{}{}", node("video", "", v, "", ""), click_node("audio", a)));
    assert!(!c[1].autoplay, "the clip waits for a click");
    assert!(c[0].autoplay, "a slide-start clip still autoplays");
}

/// PowerPoint (and our own writer) starts an automatic effect with `indefinite` *and*
/// `evt="onBegin"`: the event condition starts the node, so the clip is not click-triggered.
#[test]
fn an_on_begin_condition_is_a_slide_start() {
    let auto = |tag: &str, spid: u32| {
        format!(
            r#"<p:par><p:cTn id="2" fill="hold"><p:stCondLst><p:cond delay="indefinite"/><p:cond evt="onBegin" delay="0"><p:tn val="2"/></p:cond></p:stCondLst><p:childTnLst><p:{tag}><p:cMediaNode><p:cTn id="3"/><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cMediaNode></p:{tag}></p:childTnLst></p:cTn></p:par>"#
        )
    };
    let c = clips(|v, a| format!("{}{}", auto("video", v), auto("audio", a)));
    assert!(c[0].autoplay, "onBegin starts the video with the slide");
    assert!(c[1].autoplay, "onBegin starts the audio with the slide");
}

#[test]
fn a_slide_start_clip_is_autoplay() {
    let c = clips(|_, a| node("audio", "", a, "", ""));
    assert!(c[1].autoplay, "no click node: the slide starts it");
}

/// `isNarration` is an attribute of `p:audio` in the schema — not of `p:cMediaNode`, which a strict
/// reader would reject or ask to repair.
#[test]
fn narration_audio_carries_the_schema_flag_on_the_audio_node() {
    let mut p = defaults::new_presentation(None);
    let audio = p.add_media("tone.wav", "audio/wav", b"audio".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
        id,
        name: "Narration".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip { media: audio, narration: true, autoplay: true, ..Default::default() }),
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml").expect("slide").read_to_string(&mut xml).expect("read");
    assert!(xml.contains("<p:audio isNarration=\"1\""), "the flag rides on p:audio: {xml}");
    let media_node = xml.split("<p:cMediaNode").nth(1).and_then(|s| s.split('>').next()).unwrap_or_default();
    assert!(!media_node.contains("isNarration"), "p:cMediaNode does not declare it: {media_node}");
}

/// An imported video that plays on slide entry keeps that start: the writer emits a `p:video`
/// slide-start node for it, so a save and reopen does not turn it back into a click-to-play clip.
#[test]
fn an_autoplay_video_keeps_its_slide_start_node() {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
        id,
        name: "Clip".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip { media: video, video: true, autoplay: true, ..Default::default() }),
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml").expect("slide").read_to_string(&mut xml).expect("read");
    assert!(xml.contains("<p:video>"), "the video has its slide-start node: {xml}");
    assert!(!xml.contains("<p:audio"), "and no audio node is invented for it: {xml}");
    assert!(xml.contains(&format!("<p:spTgt spid=\"{}\"/>", id.0)), "the node targets the video: {xml}");
}

/// Loop, Hide While Not Playing and Play Full Screen live on the slide-start timing node: an
/// autoplay video keeps them through a save, and narration is never crowded out of the list.
#[test]
fn an_autoplay_video_keeps_its_playback_flags() {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
        id,
        name: "Clip".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip {
            media: video,
            video: true,
            autoplay: true,
            loop_play: true,
            hide_while_not_playing: true,
            full_screen: true,
            ..Default::default()
        }),
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml").expect("slide").read_to_string(&mut xml).expect("read");
    assert!(xml.contains(r#"repeatCount="indefinite""#), "Loop until Stopped: {xml}");
    assert!(xml.contains(r#"showWhenStopped="0""#), "Hide While Not Playing: {xml}");
    assert!(xml.contains(r#"<p:video fullScrn="1">"#), "Play Full Screen rides on p:video: {xml}");
    let media_node = xml.split("<p:cMediaNode").nth(1).and_then(|s| s.split('>').next()).unwrap_or_default();
    assert!(!media_node.contains("fullScrn"), "p:cMediaNode does not declare it: {media_node}");
}

/// The reader accepts `repeatCount="indefinite"` on an audio node too: looping autoplay audio
/// must keep it, or the clip reopens as one-shot.
#[test]
fn a_looping_autoplay_audio_keeps_its_repeat_count() {
    let mut p = defaults::new_presentation(None);
    let audio = p.add_media("tone.wav", "audio/wav", b"audio".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
        id,
        name: "Music".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip { media: audio, autoplay: true, loop_play: true, ..Default::default() }),
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml").expect("slide").read_to_string(&mut xml).expect("read");
    assert!(xml.contains("<p:audio"), "an audio node is written: {xml}");
    assert!(xml.contains(r#"repeatCount="indefinite""#), "looping audio keeps its repeat: {xml}");
}
