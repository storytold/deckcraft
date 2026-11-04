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

/// A media node that starts on `delay` (`0` or `indefinite`), as PowerPoint writes it.
fn delayed_node(tag: &str, spid: u32, delay: &str) -> String {
    format!(
        r#"<p:par><p:cTn id="2" fill="hold"><p:childTnLst><p:{tag}><p:cMediaNode><p:cTn id="3" fill="hold"><p:stCondLst><p:cond delay="{delay}"/></p:stCondLst></p:cTn><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cMediaNode></p:{tag}></p:childTnLst></p:cTn></p:par>"#
    )
}

/// A `playFrom(0.0)` start behaviour, in the main sequence or inside an interactive (click) one.
fn play_from(spid: u32, interactive: bool) -> String {
    let par = format!(
        r#"<p:par><p:cTn id="9" presetID="1" presetClass="mediacall" presetSubtype="0" fill="hold" nodeType="afterEffect"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:cmd type="call" cmd="playFrom(0.0)"><p:cBhvr><p:cTn id="10" dur="1" fill="hold"/><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cBhvr></p:cmd></p:childTnLst></p:cTn></p:par>"#
    );
    if interactive {
        format!(
            r#"<p:seq concurrent="1" nextAc="seek"><p:cTn id="11" dur="indefinite" nodeType="interactiveSeq"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst>{par}</p:childTnLst></p:cTn></p:seq>"#
        )
    } else {
        par
    }
}

#[test]
fn autoplay_comes_from_the_main_sequence_not_the_media_nodes_start_delay() {
    // PowerPoint writes an automatic start as a `playFrom` in the main sequence, and leaves the
    // media node's own start delay `indefinite` for it too: the delay alone must not decide.
    let c = clips(|v, a| {
        format!("{}{}{}{}", delayed_node("video", v, "indefinite"), delayed_node("audio", a, "indefinite"), play_from(v, false), play_from(a, true))
    });
    assert!(c[0].autoplay, "a main-sequence playFrom starts the video automatically");
    assert!(!c[1].autoplay, "a playFrom in an interactive sequence waits for a click");
}

#[test]
fn a_numeric_start_delay_alone_is_still_an_automatic_start() {
    // DeckCraft's own writer marks autoplay with the media node's delay and no behaviour node.
    let c = clips(|v, _| delayed_node("video", v, "0"));
    assert!(c[0].autoplay);
    assert!(!c[1].autoplay, "a clip with no timing node keeps the default");
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
