//! Media settings PowerPoint keeps in the timing tree, spliced into an exported deck and read back.

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::{MediaClip, ShapeId, ShapeKind, defaults};

/// The slide XML without its own `p:timing` tree (spliced timing is the test's input).
fn without_timing(xml: &str) -> String {
    let Some(start) = xml.find("<p:timing") else { return xml.to_string() };
    let Some(close) = xml[start..].find("</p:timing>").map(|i| start + i + "</p:timing>".len()) else { return xml.to_string() };
    format!("{}{}", &xml[..start], &xml[close..])
}

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
            // The export writes media timing itself; replace it with the spliced tree so the test
            // controls exactly what is read.
            let s = without_timing(&s);
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

#[test]
fn autoplay_and_click_to_play_round_trip_through_the_media_timing() {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
    for (autoplay, name) in [(true, "Automatic"), (false, "On click")] {
        let id = ShapeId(p.alloc_id().expect("id"));
        Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
            id,
            name: name.into(),
            xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
            kind: ShapeKind::Media(MediaClip { media: video, video: true, autoplay, ..Default::default() }),
            ..Default::default()
        });
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = {
        use std::io::Read;
        let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
        let mut f = z.by_name("ppt/slides/slide1.xml").expect("slide");
        let mut s = String::new();
        f.read_to_string(&mut s).expect("read");
        s
    };
    assert!(xml.contains("<p:video"), "media timing is written even without animations");
    assert!(xml.contains(r#"<p:cond delay="0"/>"#), "autoplay starts at zero");
    assert!(xml.contains(r#"<p:cond delay="indefinite"/>"#), "click-to-play waits");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let autoplay: Vec<bool> = q.slides[0]
        .shapes
        .iter()
        .filter_map(|s| match &s.kind {
            ShapeKind::Media(m) if m.video => Some(m.autoplay),
            _ => None,
        })
        .collect();
    assert_eq!(autoplay, vec![true, false]);
}
