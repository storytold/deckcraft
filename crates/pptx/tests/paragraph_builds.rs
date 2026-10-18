//! Paragraph builds (`bldP build="p"`) as other files write them: each must play the clicks the
//! file asks for, and keep them across a save. Only the default pattern (the same effect on every
//! paragraph, one click each) becomes a single by-paragraph animation.

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_anim::ShowState;
use deckcraft_geom::Xfrm;
use deckcraft_model::anim::{AnimStart, TextBuild};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Presentation, Shape, ShapeId, defaults};

/// One entrance (fade) on paragraph `para` of the body, in PowerPoint's form.
struct Eff {
    node: &'static str,
    para: u32,
    grp: u32,
}

const fn eff(node: &'static str, para: u32, grp: u32) -> Eff {
    Eff { node, para, grp }
}

fn effect_xml(id: &mut u32, spid: u32, e: &Eff) -> String {
    let tgt = format!(r#"<p:tgtEl><p:spTgt spid="{spid}"><p:txEl><p:pRg st="{0}" end="{0}"/></p:txEl></p:spTgt></p:tgtEl>"#, e.para);
    *id += 3;
    format!(
        r#"<p:par><p:cTn id="{}" presetID="10" presetClass="entr" presetSubtype="0" fill="hold" grpId="{}" nodeType="{}"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:set><p:cBhvr><p:cTn id="{}" dur="1" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn>{tgt}<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible"/></p:to></p:set><p:animEffect transition="in" filter="fade"><p:cBhvr><p:cTn id="{}" dur="500"/>{tgt}</p:cBhvr></p:animEffect></p:childTnLst></p:cTn></p:par>"#,
        *id,
        e.grp,
        e.node,
        *id + 1,
        *id + 2
    )
}

/// `p:timing` for a main sequence: one outer `p:par` per click group (a group's first effect
/// decides whether it waits for a click), and a `bldP build="p"` per group id.
fn timing_xml(spid: u32, groups: &[&[Eff]]) -> String {
    let mut id = 2;
    let mut pars = String::new();
    for g in groups {
        let auto = g.first().is_some_and(|e| e.node != "clickEffect");
        let delay = if auto { "0" } else { "indefinite" };
        let begin = if auto { r#"<p:cond evt="onBegin" delay="0"><p:tn val="2"/></p:cond>"# } else { "" };
        let inner: String = g.iter().map(|e| effect_xml(&mut id, spid, e)).collect();
        id += 2;
        pars.push_str(&format!(
            r#"<p:par><p:cTn id="{}" fill="hold"><p:stCondLst><p:cond delay="{delay}"/>{begin}</p:stCondLst><p:childTnLst><p:par><p:cTn id="{}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>{inner}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>"#,
            id - 1,
            id
        ));
    }
    let mut grps: Vec<u32> = groups.iter().flat_map(|g| g.iter().map(|e| e.grp)).collect();
    grps.sort_unstable();
    grps.dedup();
    let bld: String = grps.iter().map(|g| format!(r#"<p:bldP spid="{spid}" grpId="{g}" build="p"/>"#)).collect();
    format!(
        r#"<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst><p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst>{pars}</p:childTnLst></p:cTn><p:prevCondLst><p:cond evt="onPrev" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst><p:nextCondLst><p:cond evt="onNext" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst><p:bldLst>{bld}</p:bldLst></p:timing>"#
    )
}

/// A one-slide deck whose only shape, "Body", holds `text` (one paragraph per line, a leading tab
/// for a sub-bullet), saved, given the timing `groups` and opened again.
fn import_with(text: &str, groups: &[&[Eff]]) -> Presentation {
    let mut p = defaults::new_presentation(None);
    let mut body = TextBody::from_text(&text.replace('\t', ""));
    for (para, line) in body.paragraphs.iter_mut().zip(text.split('\n')) {
        para.level = u8::from(line.starts_with('\t'));
    }
    let body = Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: "Body".into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 400.0, 300.0)),
        text: Some(body),
        ..Default::default()
    };
    Arc::make_mut(&mut p.slides[0]).shapes = vec![body];
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        if f.name() == "ppt/slides/slide1.xml" {
            let s = String::from_utf8(d).expect("utf8");
            let n = s.find(r#"name="Body""#).expect("body");
            let id_at = s[..n].rfind(r#"id=""#).expect("cNvPr id") + 4;
            let spid: u32 = s[id_at..].split('"').next().and_then(|v| v.parse().ok()).expect("spid");
            assert!(!s.contains("<p:timing"), "the base slide has no animations");
            let at = s.rfind("</p:sld>").expect("sld end");
            d = format!("{}{}{}", &s[..at], timing_xml(spid, groups), &s[at..]).into_bytes();
        }
        out.start_file(f.name(), o).expect("start");
        out.write_all(&d).expect("write");
    }
    deckcraft_pptx::import(&out.finish().expect("finish").into_inner()).expect("import")
}

/// (steps that wait for a click, steps that start by themselves) on slide 1.
fn clicks(p: &Presentation) -> (usize, usize) {
    let tl = ShowState::timeline(p, 0);
    let auto = (0..tl.steps()).filter(|&i| tl.step_is_auto(i)).count();
    (tl.steps() - auto, auto)
}

fn paragraphs(p: &Presentation) -> Vec<Option<u32>> {
    p.slides[0].animations.iter().map(|a| a.paragraph).collect()
}

/// Imports, checks the clicks, then checks a save and reopen keeps the animations and clicks.
fn check(p: &Presentation, want: (usize, usize)) {
    assert_eq!(clicks(p), want, "clicks as imported");
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(p).expect("export")).expect("reimport");
    let key = |p: &Presentation| p.slides[0].animations.iter().map(|a| (a.paragraph, a.text_build, a.start)).collect::<Vec<_>>();
    assert_eq!(key(&back), key(p), "animations after a save");
    assert_eq!(clicks(&back), want, "clicks after a save");
}

#[test]
fn one_click_per_paragraph_becomes_one_by_paragraph_animation() {
    let p = import_with("One\nTwo\nThree", &[&[eff("clickEffect", 0, 0)], &[eff("clickEffect", 1, 0)], &[eff("clickEffect", 2, 0)]]);
    let a = &p.slides[0].animations;
    assert_eq!(a.len(), 1);
    assert_eq!((a[0].text_build, a[0].paragraph, a[0].start), (TextBuild::ByParagraph, None, AnimStart::OnClick));
    check(&p, (3, 0));
}

/// PowerPoint's by-first-level build: a sub-bullet comes in with its parent.
#[test]
fn sub_bullets_with_their_parent_become_one_by_paragraph_animation() {
    let p = import_with(
        "One\n\tOne a\n\tOne b\nTwo",
        &[&[eff("clickEffect", 0, 0), eff("withEffect", 1, 0), eff("withEffect", 2, 0)], &[eff("clickEffect", 3, 0)]],
    );
    let a = &p.slides[0].animations;
    assert_eq!(a.len(), 1);
    assert_eq!((a[0].text_build, a[0].paragraph), (TextBuild::ByParagraph, None));
    check(&p, (2, 0));
}

/// Sub-bullets on clicks of their own are not what a by-paragraph build plays.
#[test]
fn sub_bullets_on_their_own_clicks_stay_per_paragraph() {
    let p = import_with("One\n\tOne a\nTwo", &[&[eff("clickEffect", 0, 0)], &[eff("clickEffect", 1, 0)], &[eff("clickEffect", 2, 0)]]);
    assert_eq!(paragraphs(&p), vec![Some(0), Some(1), Some(2)]);
    check(&p, (3, 0));
}

/// Each paragraph after the one before, as a by-paragraph After Previous build saves.
#[test]
fn after_previous_on_every_paragraph_becomes_one_by_paragraph_animation() {
    let p = import_with("One\nTwo\nThree", &[&[eff("afterEffect", 0, 0), eff("afterEffect", 1, 0), eff("afterEffect", 2, 0)]]);
    let a = &p.slides[0].animations;
    assert_eq!(a.len(), 1);
    assert_eq!((a[0].text_build, a[0].paragraph, a[0].start), (TextBuild::ByParagraph, None, AnimStart::AfterPrevious));
    check(&p, (0, 1));
}

/// The first paragraph comes in with the slide, each later one on a click.
#[test]
fn first_paragraph_after_previous_then_clicks() {
    let p = import_with(
        "One\nTwo\nThree\nFour",
        &[&[eff("afterEffect", 0, 0)], &[eff("clickEffect", 1, 0)], &[eff("clickEffect", 2, 0)], &[eff("clickEffect", 3, 0)]],
    );
    assert_eq!(paragraphs(&p), vec![Some(0), Some(1), Some(2), Some(3)]);
    assert_eq!(p.slides[0].animations[0].start, AnimStart::AfterPrevious);
    check(&p, (3, 1));
}

/// Several paragraphs share one click.
#[test]
fn paragraphs_with_previous_share_a_click() {
    let p = import_with("One\nTwo\nThree", &[&[eff("clickEffect", 0, 0), eff("withEffect", 1, 0), eff("withEffect", 2, 0)]]);
    assert_eq!(paragraphs(&p), vec![Some(0), Some(1), Some(2)]);
    check(&p, (1, 0));
}

/// Only the last two paragraphs are built; the first two stay on the slide.
#[test]
fn a_subset_of_the_paragraphs_is_built() {
    let p = import_with("One\nTwo\nThree\nFour", &[&[eff("clickEffect", 2, 0)], &[eff("clickEffect", 3, 0)]]);
    assert_eq!(paragraphs(&p), vec![Some(2), Some(3)]);
    check(&p, (2, 0));
}

/// One shape, two paragraph builds whose paragraphs interleave.
#[test]
fn interleaved_build_groups_keep_their_order() {
    let p = import_with("One\nTwo\nThree", &[&[eff("clickEffect", 0, 0)], &[eff("clickEffect", 1, 1)], &[eff("clickEffect", 2, 0)]]);
    assert_eq!(paragraphs(&p), vec![Some(0), Some(1), Some(2)]);
    check(&p, (3, 0));
}
