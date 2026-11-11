//! A screen-recording take survives a PPTX round trip: the video shape, its media type and the
//! `deckcraft.recording=media` tag that Clear Recording uses to find takes without touching
//! ordinary videos.

use std::io::{Cursor, Read};
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::{MediaClip, Shape, ShapeId, ShapeKind, defaults};

/// One part of the package as text.
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("zip");
    let mut f = z.by_name(name).expect("part");
    let mut s = String::new();
    f.read_to_string(&mut s).expect("read");
    s
}

/// An MP4 head (`ftyp`), which is all the format needs to keep the part as video.
fn mp4() -> Vec<u8> {
    b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2\x00\x00\x00\x08free".to_vec()
}

#[test]
fn a_screen_recording_take_round_trips() {
    let mut p = defaults::new_presentation(None);
    let media = p.add_media("Screen Recording.mp4", "video/mp4", mp4()).expect("media");
    let take = Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: "Screen Recording".into(),
        xfrm: Some(Xfrm::new(80.0, 60.0, 320.0, 180.0)),
        kind: ShapeKind::Media(MediaClip { media, video: true, volume: 1.0, ..Default::default() }),
        tags: vec![("deckcraft.recording".into(), "media".into())],
        ..Default::default()
    };
    Arc::make_mut(&mut p.slides[0]).shapes.push(take);

    let bytes = deckcraft_pptx::export(&p).expect("export");
    // `p:nvPr` orders `p:custDataLst` before `p:extLst`; PowerPoint repairs a slide that swaps
    // them, so the take's tags must not sit after the media extension.
    let xml = part(&bytes, "ppt/slides/slide1.xml");
    let tags_at = xml.find("<p:custDataLst>").expect("the take carries its tag in p:nvPr");
    let ext_at = xml.find("<p:extLst>").expect("the video reference is an extension");
    assert!(tags_at < ext_at, "p:custDataLst comes before p:extLst: {xml}");

    let back = deckcraft_pptx::import(&bytes).expect("import");
    let found = back.slides[0].shapes.iter().find(|s| s.name == "Screen Recording").expect("the take survives");
    let clip = match &found.kind {
        ShapeKind::Media(clip) => clip.clone(),
        other => panic!("the take came back as {other:?}, not a video"),
    };
    assert!(clip.video, "it is a video, not audio");
    let part = back.media(clip.media).expect("the media part");
    assert_eq!(part.content_type, "video/mp4");
    assert_eq!(part.data.as_slice(), mp4().as_slice(), "the recording's bytes survive");
    assert_eq!(found.tags, vec![("deckcraft.recording".to_string(), "media".to_string())], "the tag Clear Recording looks for survives");
}
