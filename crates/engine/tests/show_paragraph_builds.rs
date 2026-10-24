//! #844: a paragraph-by-paragraph build read from a PPTX file takes the clicks PowerPoint gives
//! it. The file lists one effect per paragraph, each with its own start (On Click or With
//! Previous); the show must not ask for more clicks, or fewer, than those effects.

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_anim::{ShowAction, ShowState};
use deckcraft_geom::Xfrm;
use deckcraft_model::anim::{AnimStart, Animation, TextBuild};
use deckcraft_model::text::TextBody;
use deckcraft_model::{LayoutType, Presentation, Shape, ShapeId, defaults};
use zip::write::SimpleFileOptions;

fn shape(p: &mut Presentation, name: &str, text: &str, y: f64) -> Shape {
    Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: name.into(),
        xfrm: Some(Xfrm::new(100.0, y, 600.0, 150.0)),
        text: Some(TextBody::from_text(text)),
        ..Default::default()
    }
}

/// Two slides: "Body" (paragraphs `body`, at `levels`) and "Picture" on the first, nothing on
/// the second, saved as PPTX.
fn saved(body: &str, levels: &[u8]) -> Vec<u8> {
    let mut p = defaults::new_presentation(None);
    p.slides.clear();
    let mut b = shape(&mut p, "Body", body, 50.0);
    if let Some(t) = b.text.as_mut() {
        for (para, lvl) in t.paragraphs.iter_mut().zip(levels) {
            para.level = *lvl;
        }
    }
    let pic = shape(&mut p, "Picture", "Picture", 300.0);
    let anims = vec![
        Animation { shape: b.id, text_build: TextBuild::ByParagraph, start: AnimStart::OnClick, duration_ms: 500, ..Default::default() },
        Animation { shape: pic.id, start: AnimStart::WithPrevious, duration_ms: 500, ..Default::default() },
    ];
    for (shapes, animations) in [(vec![b, pic], anims), (vec![], vec![])] {
        let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap_or_default();
        let mut s = defaults::new_slide(&mut p, layout).expect("slide");
        s.shapes = shapes;
        s.animations = animations;
        p.slides.push(Arc::new(s));
    }
    deckcraft_pptx::export(&p).expect("export")
}

fn spid(xml: &str, name: &str) -> String {
    let at = xml.find(&format!(r#"name="{name}""#)).expect("shape");
    let head = &xml[..at];
    let id = head.rfind(r#"id=""#).expect("id") + 4;
    head[id..].split('"').next().expect("spid").to_string()
}

/// One effect node as PowerPoint writes it: Fade in on `spid` (paragraph `para`).
fn effect(ids: &mut u32, spid: &str, para: Option<u32>, node: &str) -> String {
    let mut id = || {
        *ids += 1;
        *ids
    };
    let tgt = match para {
        Some(p) => format!(r#"<p:spTgt spid="{spid}"><p:txEl><p:pRg st="{p}" end="{p}"/></p:txEl></p:spTgt>"#),
        None => format!(r#"<p:spTgt spid="{spid}"/>"#),
    };
    format!(
        r#"<p:par><p:cTn id="{}" presetID="10" presetClass="entr" presetSubtype="0" fill="hold" grpId="0" nodeType="{node}"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:set><p:cBhvr><p:cTn id="{}" dur="1" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn><p:tgtEl>{tgt}</p:tgtEl><p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible"/></p:to></p:set><p:animEffect transition="in" filter="fade"><p:cBhvr><p:cTn id="{}" dur="500"/><p:tgtEl>{tgt}</p:tgtEl></p:cBhvr></p:animEffect></p:childTnLst></p:cTn></p:par>"#,
        id(),
        id(),
        id()
    )
}

/// A click group holding `effects`.
fn click(ids: &mut u32, effects: &[String]) -> String {
    *ids += 2;
    format!(
        r#"<p:par><p:cTn id="{}" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst><p:par><p:cTn id="{}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>{}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>"#,
        *ids - 1,
        *ids,
        effects.concat()
    )
}

/// `bytes` with the first slide's timing replaced by `clicks` (built with `effect` and `click`
/// from the spids of Body and Picture), Body's build "By paragraph".
fn with_timing(bytes: &[u8], clicks: impl Fn(&mut u32, &str, &str) -> Vec<String>) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut f = src.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).expect("read");
        if name == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(data).expect("utf8");
            let (body, pic) = (spid(&xml, "Body"), spid(&xml, "Picture"));
            let mut ids = 2;
            let main = clicks(&mut ids, &body, &pic).concat();
            let timing = format!(
                r#"<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst><p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst>{main}</p:childTnLst></p:cTn><p:prevCondLst><p:cond evt="onPrev" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst><p:nextCondLst><p:cond evt="onNext" delay="0"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst><p:bldLst><p:bldP spid="{body}" grpId="0" build="p"/><p:bldP spid="{pic}" grpId="0" animBg="1"/></p:bldLst></p:timing>"#
            );
            let a = xml.find("<p:timing>").expect("timing");
            let b = xml.find("</p:timing>").expect("timing end") + "</p:timing>".len();
            data = format!("{}{timing}{}", &xml[..a], &xml[b..]).into_bytes();
        }
        out.start_file(name, SimpleFileOptions::default()).expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// Clicks (Next with nothing playing) from the start of the first slide to the second one.
fn clicks_to_second_slide(pres: &Presentation) -> usize {
    let mut s = ShowState::new(pres);
    for n in 1..=50 {
        match s.next(pres) {
            ShowAction::PlayStep(_) => {
                // The step clock runs out before the next click.
                s.step_done(pres);
            }
            ShowAction::Slide(1) => return n,
            a => panic!("unexpected {a:?}"),
        }
    }
    usize::MAX
}

#[test]
fn a_paragraph_build_split_by_another_shapes_effect_takes_powerpoints_clicks() {
    // As PowerPoint writes "Body: By paragraph, Picture: With Previous after the first
    // paragraph": click 1 = Body ¶0 + Picture, click 2 = Body ¶1, click 3 = Body ¶2.
    let bytes = with_timing(&saved("One\nTwo\nThree", &[0, 0, 0]), |ids, body, pic| {
        let e0 = effect(ids, body, Some(0), "clickEffect");
        let ep = effect(ids, pic, None, "withEffect");
        let e1 = effect(ids, body, Some(1), "clickEffect");
        let e2 = effect(ids, body, Some(2), "clickEffect");
        vec![click(ids, &[e0, ep]), click(ids, &[e1]), click(ids, &[e2])]
    });
    let pres = deckcraft_pptx::import(&bytes).expect("import");
    assert_eq!(ShowState::steps(&pres, 0), 3, "three click steps, as in the file");
    assert_eq!(clicks_to_second_slide(&pres), 4, "three builds and one click to move on");
}

#[test]
fn a_build_by_second_level_paragraphs_takes_a_click_per_paragraph() {
    // "By 2nd level paragraphs": the heading and each sub-point have their own click.
    // Picture comes in with the last one, so nothing splits Body's build.
    let bytes = with_timing(&saved("Question\nFirst\nSecond", &[0, 1, 1]), |ids, body, pic| {
        let e0 = effect(ids, body, Some(0), "clickEffect");
        let e1 = effect(ids, body, Some(1), "clickEffect");
        let e2 = effect(ids, body, Some(2), "clickEffect");
        let ep = effect(ids, pic, None, "withEffect");
        vec![click(ids, &[e0]), click(ids, &[e1]), click(ids, &[e2, ep])]
    });
    let pres = deckcraft_pptx::import(&bytes).expect("import");
    assert_eq!(ShowState::steps(&pres, 0), 3, "three click steps, as in the file");
}

/// The first slide's XML in a saved PPTX.
fn slide_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut xml = String::new();
    zip.by_name("ppt/slides/slide1.xml").expect("slide").read_to_string(&mut xml).expect("read");
    xml
}

/// Paragraph and start of each animation on the first slide.
fn effects(pres: &Presentation) -> Vec<(Option<u32>, AnimStart, TextBuild)> {
    pres.slides[0].animations.iter().map(|a| (a.paragraph, a.start, a.text_build)).collect()
}

/// "By 1st level paragraphs" as PowerPoint writes it: a sub-point comes in with its parent.
fn first_level_build() -> Vec<u8> {
    with_timing(&saved("One\nSub\nTwo", &[0, 1, 0]), |ids, body, pic| {
        let e0 = effect(ids, body, Some(0), "clickEffect");
        let e1 = effect(ids, body, Some(1), "withEffect");
        let e2 = effect(ids, body, Some(2), "clickEffect");
        let ep = effect(ids, pic, None, "withEffect");
        vec![click(ids, &[e0, e1]), click(ids, &[e2, ep])]
    })
}

#[test]
fn a_whole_first_level_build_still_reads_as_one_by_paragraph_animation() {
    let pres = deckcraft_pptx::import(&first_level_build()).expect("import");
    assert_eq!(effects(&pres)[0], (None, AnimStart::OnClick, TextBuild::ByParagraph));
    assert_eq!(pres.slides[0].animations.len(), 2, "Body's build and Picture");
    assert_eq!(ShowState::steps(&pres, 0), 2);
}

#[test]
fn a_by_paragraph_build_with_sub_points_survives_saving() {
    let pres = deckcraft_pptx::import(&first_level_build()).expect("import");
    let again = deckcraft_pptx::import(&deckcraft_pptx::export(&pres).expect("export")).expect("reimport");
    assert_eq!(effects(&again), effects(&pres));
    assert_eq!(ShowState::steps(&again, 0), 2, "the sub-point still comes in with its parent");
}

#[test]
fn paragraph_effects_save_as_powerpoint_writes_them_and_read_back_unchanged() {
    let bytes = with_timing(&saved("One\nTwo\nThree", &[0, 0, 0]), |ids, body, pic| {
        let e0 = effect(ids, body, Some(0), "clickEffect");
        let ep = effect(ids, pic, None, "withEffect");
        let e1 = effect(ids, body, Some(1), "clickEffect");
        let e2 = effect(ids, body, Some(2), "clickEffect");
        vec![click(ids, &[e0, ep]), click(ids, &[e1]), click(ids, &[e2])]
    });
    let pres = deckcraft_pptx::import(&bytes).expect("import");
    let saved = deckcraft_pptx::export(&pres).expect("export");
    let xml = slide_xml(&saved);
    let body = spid(&xml, "Body");
    let blds: Vec<&str> = xml.split("<p:bldP ").skip(1).filter(|b| b.starts_with(&format!(r#"spid="{body}""#))).collect();
    // ¶0 is its own build (Picture sits between), ¶1 and ¶2 share one.
    assert_eq!(blds.len(), 2, "{blds:?}");
    assert!(blds.iter().all(|b| b.contains(r#"build="p""#) && !b.contains("animBg")), "{blds:?}");
    let again = deckcraft_pptx::import(&saved).expect("reimport");
    assert_eq!(effects(&again), effects(&pres));
    assert_eq!(ShowState::steps(&again, 0), 3);
}

#[test]
fn a_by_paragraph_build_on_blank_text_saves_as_a_whole_shape_effect() {
    let xml = slide_xml(&saved(" ", &[0]));
    assert!(!xml.contains(r#"build="p""#), "no paragraph build without a paragraph to animate");
    assert!(!xml.contains("<p:txEl>"), "the effect targets the whole shape");
}

/// Body's three paragraphs under `build="p"`, effects in the order `effects` gives them, then
/// Picture with the last one.
fn three_paragraphs(effects: impl Fn(&mut u32, &str) -> Vec<String>) -> Presentation {
    let bytes = with_timing(&saved("One\nTwo\nThree", &[0, 0, 0]), |ids, body, pic| {
        let mut clicks: Vec<String> = effects(ids, body).iter().map(|e| click(ids, std::slice::from_ref(e))).collect();
        let ep = effect(ids, pic, None, "clickEffect");
        clicks.push(click(ids, &[ep]));
        clicks
    });
    deckcraft_pptx::import(&bytes).expect("import")
}

#[test]
fn a_reversed_build_keeps_its_order() {
    let pres = three_paragraphs(|ids, body| (0..3).rev().map(|p| effect(ids, body, Some(p), "clickEffect")).collect());
    let paras: Vec<Option<u32>> = pres.slides[0].animations.iter().map(|a| a.paragraph).collect();
    assert_eq!(paras, vec![Some(2), Some(1), Some(0), None], "last paragraph first, as in the file");
}

#[test]
fn a_paragraph_with_its_own_duration_keeps_its_own_effect() {
    let pres = three_paragraphs(|ids, body| {
        (0..3)
            .map(|p| {
                let e = effect(ids, body, Some(p), "clickEffect");
                if p == 1 { e.replace(r#"dur="500""#, r#"dur="900""#) } else { e }
            })
            .collect()
    });
    let durs: Vec<(Option<u32>, u32)> = pres.slides[0].animations.iter().map(|a| (a.paragraph, a.duration_ms)).collect();
    assert_eq!(durs, vec![(Some(0), 500), (Some(1), 900), (Some(2), 500), (None, 500)]);
}
