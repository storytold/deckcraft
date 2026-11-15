//! Easing (`accel`/`decel`) written on a behaviour's own `p:cTn` is read; the effect node wins.

use std::io::{Cursor, Read, Write};

use deckcraft_geom::Xfrm;
use deckcraft_model::anim::{AnimClass, Animation};
use deckcraft_model::{Shape, ShapeId, defaults};

fn roundtrip(anim: Animation, patch: impl Fn(String) -> String) -> Animation {
    let mut p = defaults::new_presentation(None);
    let id = ShapeId(p.alloc_id().expect("id"));
    let shape = Shape { id, name: "Box".into(), xfrm: Some(Xfrm::new(10.0, 10.0, 200.0, 100.0)), ..Default::default() };
    let sl = std::sync::Arc::make_mut(&mut p.slides[0]);
    sl.shapes = vec![shape];
    let mut anim = anim;
    anim.shape = id;
    sl.animations = vec![anim];
    let mut z = zip::ZipArchive::new(Cursor::new(deckcraft_pptx::export(&p).expect("export"))).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        if name == "ppt/slides/slide1.xml" {
            d = patch(String::from_utf8(d).expect("utf8")).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).expect("start");
        out.write_all(&d).expect("write");
    }
    let q = deckcraft_pptx::import(&out.finish().expect("finish").into_inner()).expect("import");
    assert_eq!(q.slides[0].animations.len(), 1);
    q.slides[0].animations[0].clone()
}

/// Insert `attrs` into the first `<p:cTn ...>` after `marker` (the behaviour's own time node).
fn patch_behaviour_ctn(xml: String, marker: &str, attrs: &str) -> String {
    let at = xml.find(marker).expect("behaviour written");
    let cb = at + xml[at..].find("<p:cTn ").expect("behaviour node");
    format!("{}<p:cTn {}{}", &xml[..cb], attrs, &xml[cb + "<p:cTn ".len()..])
}

fn grow_shrink() -> Animation {
    Animation { class: AnimClass::Emphasis, effect: "growShrink".into(), duration_ms: 1000, ..Default::default() }
}

#[test]
fn behaviour_easing_is_read() {
    let a = roundtrip(grow_shrink(), |xml| patch_behaviour_ctn(xml, "<p:animScale>", "accel=\"50000\" decel=\"100000\" "));
    assert_eq!(a.smooth_start, 0.5);
    assert_eq!(a.smooth_end, 1.0);
}

#[test]
fn no_easing_anywhere_stays_zero() {
    let a = roundtrip(grow_shrink(), |xml| xml);
    assert_eq!(a.smooth_start, 0.0);
    assert_eq!(a.smooth_end, 0.0);
}

#[test]
fn effect_node_wins_over_behaviour() {
    // DeckCraft writes easing on the effect node, so when both carry a value the effect node's wins.
    let anim = Animation { smooth_end: 0.2, ..grow_shrink() };
    let a = roundtrip(anim, |xml| {
        assert!(xml.contains("decel=\"20000\""), "effect node easing written");
        patch_behaviour_ctn(xml, "<p:animScale>", "decel=\"100000\" ")
    });
    assert_eq!(a.smooth_end, 0.2);
}
