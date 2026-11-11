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

fn imported(nodes: impl Fn(u32, u32) -> String) -> (Vec<MediaClip>, Vec<deckcraft_model::Animation>) {
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
    let clips = q.slides[0]
        .shapes
        .iter()
        .filter_map(|s| match &s.kind {
            ShapeKind::Media(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    (clips, q.slides[0].animations.clone())
}

fn clips(nodes: impl Fn(u32, u32) -> String) -> Vec<MediaClip> {
    imported(nodes).0
}

/// Narration is PresentationML's own flag on the media node, so a clip whose shape is not called
/// "Narration" still comes back as narration (and PowerPoint skips it with Play Narrations off).
#[test]
fn narration_is_marked_on_the_media_node_and_read_back() {
    let mut p = defaults::new_presentation(None);
    let audio = p.add_media("take.wav", "audio/wav", b"RIFF".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
        id,
        name: "Audio 1".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip { media: audio, video: false, autoplay: true, narration: true, ..Default::default() }),
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = {
        let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
        let mut f = z.by_name("ppt/slides/slide1.xml").expect("slide");
        let mut s = String::new();
        f.read_to_string(&mut s).expect("read");
        s
    };
    assert!(xml.contains(r#"isNarration="1""#), "the media node carries PowerPoint's narration flag");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let clip = q.slides[0]
        .shapes
        .iter()
        .find_map(|s| match &s.kind {
            ShapeKind::Media(m) if !m.video => Some(m.clone()),
            _ => None,
        })
        .expect("the audio");
    assert!(clip.narration, "the flag is read back, not only the shape name");
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
fn a_play_from_inside_a_click_effect_is_not_an_automatic_start() {
    // DeckCraft's own writer emits an On Click media "play" as a main-sequence `clickEffect` with
    // a `playFrom(0.0)`: it must not come back as autoplay.
    let click = |spid: u32| {
        format!(
            r#"<p:par><p:cTn id="12" presetID="1" presetClass="mediacall" presetSubtype="0" fill="hold" nodeType="clickEffect"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:cmd type="call" cmd="playFrom(0.0)"><p:cBhvr><p:cTn id="13" dur="1" fill="hold"/><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cBhvr></p:cmd></p:childTnLst></p:cTn></p:par>"#
        )
    };
    let c = clips(|v, a| format!("{}{}{}{}", delayed_node("video", v, "0"), delayed_node("audio", a, "indefinite"), click(v), click(a)));
    assert!(!c[0].autoplay, "a clickEffect playFrom waits for the click, even with a numeric delay");
    assert!(!c[1].autoplay);
}

#[test]
fn a_click_media_effect_without_a_grp_id_keeps_its_play() {
    // An authored click-triggered media effect need not carry `grpId`: it waits for the click, so it
    // is not the automatic-start behaviour and must survive import as a Play animation.
    let click = |spid: u32| {
        format!(
            r#"<p:par><p:cTn id="14" presetID="1" presetClass="mediacall" presetSubtype="0" fill="hold" nodeType="clickEffect"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:cmd type="call" cmd="playFrom(0.0)"><p:cBhvr><p:cTn id="15" dur="1" fill="hold"/><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cBhvr></p:cmd></p:childTnLst></p:cTn></p:par>"#
        )
    };
    let (c, anims) = imported(|v, _| format!("{}{}", delayed_node("video", v, "indefinite"), click(v)));
    assert!(!c[0].autoplay, "a clickEffect waits for the click even without a grpId");
    let plays: Vec<_> = anims.iter().filter(|a| a.effect == "play" && a.class == deckcraft_model::anim::AnimClass::Media).collect();
    assert_eq!(plays.len(), 1, "the authored click media effect keeps its Play: {anims:?}");
    assert_eq!(plays[0].start, deckcraft_model::anim::AnimStart::OnClick);
}

/// A `mediacall` `afterEffect` with no `grpId` inside a group the deck starts on its own (an
/// indefinite delay) is an authored effect, not the writer's automatic-start trigger.
fn play_after_indefinite(spid: u32) -> String {
    format!(
        r#"<p:par><p:cTn id="20" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst><p:par><p:cTn id="21" presetID="1" presetClass="mediacall" presetSubtype="0" fill="hold" nodeType="afterEffect"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:cmd type="call" cmd="playFrom(0.0)"><p:cBhvr><p:cTn id="22" dur="1" fill="hold"/><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cBhvr></p:cmd></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>"#
    )
}

#[test]
fn a_media_after_effect_inside_an_indefinite_group_keeps_its_play() {
    let click = |spid: u32| {
        format!(
            r#"<p:par><p:cTn id="14" presetID="1" presetClass="mediacall" presetSubtype="0" fill="hold" nodeType="clickEffect"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:cmd type="call" cmd="playFrom(0.0)"><p:cBhvr><p:cTn id="15" dur="1" fill="hold"/><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cBhvr></p:cmd></p:childTnLst></p:cTn></p:par>"#
        )
    };
    // Both effects wait for a click (the second group starts after the first, and the clip's own
    // `playFrom` in the first group is the click effect), so autoplay itself is not the point
    // here: the grouped effect must stay an animation the Animation Pane lists.
    let (_, anims) = imported(|v, _| format!("{}{}{}", delayed_node("video", v, "indefinite"), click(v), play_after_indefinite(v)));
    let plays: Vec<_> = anims.iter().filter(|a| a.effect == "play" && a.class == deckcraft_model::anim::AnimClass::Media).collect();
    assert_eq!(plays.len(), 2, "the click effect and the grouped afterEffect both stay: {anims:?}");
}

#[test]
fn an_on_click_media_play_animation_round_trips_without_autoplay() {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
    slide.shapes.push(deckcraft_model::Shape {
        id,
        name: "Clip".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip { media: video, video: true, autoplay: false, ..Default::default() }),
        ..Default::default()
    });
    slide.animations.push(deckcraft_model::Animation {
        shape: id,
        class: deckcraft_model::anim::AnimClass::Media,
        effect: "play".into(),
        start: deckcraft_model::anim::AnimStart::OnClick,
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let clip = q.slides[0]
        .shapes
        .iter()
        .find_map(|s| match &s.kind {
            ShapeKind::Media(m) if m.video => Some(m.clone()),
            _ => None,
        })
        .expect("the video");
    assert!(!clip.autoplay, "an On Click media play stays click-triggered after a round trip");
    assert!(!q.slides[0].animations.is_empty(), "the play animation round-trips");
}

#[test]
fn an_on_click_media_play_does_not_stop_an_autoplay_clip_from_starting() {
    // The clip plays by itself; a second, On Click play animation is an effect the click sets off.
    // It must not suppress the main-sequence automatic start, or the clip stops autoplaying.
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
    slide.shapes.push(deckcraft_model::Shape {
        id,
        name: "Clip".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip { media: video, video: true, autoplay: true, ..Default::default() }),
        ..Default::default()
    });
    slide.animations.push(deckcraft_model::Animation {
        shape: id,
        class: deckcraft_model::anim::AnimClass::Media,
        effect: "play".into(),
        start: deckcraft_model::anim::AnimStart::OnClick,
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let xml = {
        let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
        let mut f = z.by_name("ppt/slides/slide1.xml").expect("slide");
        let mut s = String::new();
        f.read_to_string(&mut s).expect("read");
        s
    };
    assert!(xml.contains(r#"cmd="playFrom(0.0)""#), "the autoplay clip gets its automatic start");
    assert!(xml.contains(r#"nodeType="afterEffect""#), "the automatic start is the main-sequence behaviour, not only the click effect");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let clip = q.slides[0]
        .shapes
        .iter()
        .find_map(|s| match &s.kind {
            ShapeKind::Media(m) if m.video => Some(m.clone()),
            _ => None,
        })
        .expect("the video");
    assert!(clip.autoplay, "the click effect does not take the automatic start away");
    let plays: Vec<_> = q.slides[0].animations.iter().filter(|a| a.effect == "play" && a.class == deckcraft_model::anim::AnimClass::Media).collect();
    assert_eq!(plays.len(), 1, "the On Click play animation round-trips: {:?}", q.slides[0].animations);
    assert_eq!(plays[0].start, deckcraft_model::anim::AnimStart::OnClick);
}

/// An autoplay clip on a slide whose first effect starts with the slide (With Previous or After
/// Previous): the automatic start must sit inside that first group, or PowerPoint's group begin
/// (the mainSeq's own begin) fires before it and the group waits for a click.
#[test]
fn an_automatic_start_of_the_first_group_goes_inside_that_group() {
    for start in [deckcraft_model::anim::AnimStart::WithPrevious, deckcraft_model::anim::AnimStart::AfterPrevious] {
        let mut p = defaults::new_presentation(None);
        let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
        let clip = ShapeId(p.alloc_id().expect("id"));
        let rect = ShapeId(p.alloc_id().expect("id"));
        {
            let slide = Arc::make_mut(&mut p.slides[0]);
            slide.shapes.push(deckcraft_model::Shape {
                id: clip,
                name: "Automatic".into(),
                xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
                kind: ShapeKind::Media(MediaClip { media: video, video: true, autoplay: true, ..Default::default() }),
                ..Default::default()
            });
            slide.shapes.push(deckcraft_model::Shape {
                id: rect,
                name: "Box".into(),
                xfrm: Some(Xfrm::new(300.0, 40.0, 200.0, 100.0)),
                ..Default::default()
            });
            // The slide's first effect starts with the slide, not on a click.
            slide.animations.push(deckcraft_model::Animation {
                shape: rect,
                class: deckcraft_model::anim::AnimClass::Entrance,
                effect: "fade".into(),
                start,
                ..Default::default()
            });
        }
        let bytes = deckcraft_pptx::export(&p).expect("export");
        let xml = {
            let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
            let mut f = z.by_name("ppt/slides/slide1.xml").expect("slide");
            let mut s = String::new();
            f.read_to_string(&mut s).expect("read");
            s
        };
        // The trigger is inside the first group: the `playFrom` comes after that group's own
        // `indefinite` start condition, before any effect paragraph of the group.
        let group = xml.find(r#"<p:cond delay="indefinite"/>"#).expect("the first group waits for the slide's begin");
        let trigger = xml.find(r#"cmd="playFrom(0.0)""#).expect("the automatic start is written");
        assert!(trigger > group, "{start:?}: the trigger sits inside the first group");
        let q = deckcraft_pptx::import(&bytes).expect("import");
        let clip = q.slides[0]
            .shapes
            .iter()
            .find_map(|s| match &s.kind {
                ShapeKind::Media(m) if m.video => Some(m.clone()),
                _ => None,
            })
            .expect("the video");
        assert!(clip.autoplay, "{start:?}: the automatic start survives the round trip");
        let plays: Vec<_> =
            q.slides[0].animations.iter().filter(|a| a.effect == "play" && a.class == deckcraft_model::anim::AnimClass::Media).collect();
        assert!(plays.is_empty(), "{start:?}: the trigger is not an Animation Pane effect: {:?}", q.slides[0].animations);
    }
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
    assert!(xml.contains(r#"cmd="playFrom(0.0)""#), "the autoplay clip gets a PowerPoint automatic-start behaviour");
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
    // The behaviour starts playback, it is not an animation of its own: importing the file does not
    // add a "play" effect to the Animation Pane.
    assert!(q.slides[0].animations.is_empty(), "{:?}", q.slides[0].animations);
}

#[test]
fn a_media_play_after_a_click_round_trips_as_click_triggered() {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    let text = ShapeId(p.alloc_id().expect("id"));
    let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
    slide.shapes.push(deckcraft_model::Shape {
        id,
        name: "Clip".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip { media: video, video: true, autoplay: false, ..Default::default() }),
        ..Default::default()
    });
    // Fade (On Click) on another shape, then Play media (After Previous): both land in the same
    // click group, in sibling paragraphs, so the play must wait for that click too.
    slide.shapes.push(deckcraft_model::Shape {
        id: text,
        name: "Text".into(),
        xfrm: Some(Xfrm::new(40.0, 200.0, 200.0, 60.0)),
        ..Default::default()
    });
    slide.animations.push(deckcraft_model::Animation {
        shape: text,
        class: deckcraft_model::anim::AnimClass::Entrance,
        effect: "fade".into(),
        start: deckcraft_model::anim::AnimStart::OnClick,
        ..Default::default()
    });
    slide.animations.push(deckcraft_model::Animation {
        shape: id,
        class: deckcraft_model::anim::AnimClass::Media,
        effect: "play".into(),
        start: deckcraft_model::anim::AnimStart::AfterPrevious,
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let clip = q.slides[0]
        .shapes
        .iter()
        .find_map(|s| match &s.kind {
            ShapeKind::Media(m) => Some(m.clone()),
            _ => None,
        })
        .expect("the clip");
    assert!(!clip.autoplay, "a media play after a click waits for that click");
    assert_eq!(q.slides[0].animations.len(), 2, "the two effects round-trip");
}

#[test]
fn an_automatic_start_behaviour_does_not_add_an_animation() {
    let mut p = defaults::new_presentation(None);
    let video = p.add_media("clip.mp4", "video/mp4", b"video".to_vec()).expect("media");
    let id = ShapeId(p.alloc_id().expect("id"));
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
        id,
        name: "Clip".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        kind: ShapeKind::Media(MediaClip { media: video, video: true, autoplay: true, ..Default::default() }),
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    assert!(q.slides[0].animations.is_empty(), "an automatic start is not an animation: {:?}", q.slides[0].animations);
    let clip = q.slides[0]
        .shapes
        .iter()
        .find_map(|s| match &s.kind {
            ShapeKind::Media(m) => Some(m.clone()),
            _ => None,
        })
        .expect("the clip");
    assert!(clip.autoplay, "the automatic start survives the round trip");
}

/// Characterization of the ordinary animation import: a non-media effect after a click effect is
/// still read (the media autoplay rule must not drop it) and keeps its authored start and delay.
#[test]
fn a_non_media_animation_after_a_click_effect_is_still_read() {
    use deckcraft_model::Animation;
    use deckcraft_model::anim::AnimStart;

    let mut p = defaults::new_presentation(None);
    let id = ShapeId(p.alloc_id().expect("id"));
    Arc::make_mut(&mut p.slides[0]).shapes.push(deckcraft_model::Shape {
        id,
        name: "Box".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 100.0, 60.0)),
        kind: ShapeKind::Shape,
        ..Default::default()
    });
    let effect = |ctn: &str, delay: u32| {
        format!(
            r#"<p:par><p:cTn id="9" presetID="1" presetClass="entr"{ctn} fill="hold"><p:stCondLst><p:cond delay="{delay}"/></p:stCondLst><p:childTnLst><p:set><p:cBhvr><p:cTn id="10" dur="500"/><p:tgtEl><p:spTgt spid="{}"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par>"#,
            id.0
        )
    };
    // A click effect first, then an effect that plays with/after it.
    let first = effect(r#" nodeType="clickEffect""#, 0);
    let second = effect("", 500);
    let timing = format!(
        r#"<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst>{first}{second}</p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"#
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
            let s = without_timing(&s);
            let end = s.rfind("</p:sld>").expect("end");
            d = format!("{}{timing}{}", &s[..end], &s[end..]).into_bytes();
        }
        out.start_file(f.name(), zip::write::SimpleFileOptions::default()).expect("start");
        out.write_all(&d).expect("write");
    }
    let q = deckcraft_pptx::import(&out.finish().expect("finish").into_inner()).expect("import");
    let anims: Vec<&Animation> = q.slides[0].animations.iter().collect();
    assert_eq!(anims.len(), 2, "the effect after the click is not dropped: {anims:?}");
    assert_eq!(anims[0].start, AnimStart::OnClick);
    assert_eq!(anims[1].start, AnimStart::WithPrevious);
    assert_eq!(anims[1].delay_ms, 500);
    assert!(anims.iter().all(|a| a.shape == id), "both effects target the box");
}
