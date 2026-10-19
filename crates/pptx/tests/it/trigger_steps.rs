//! Synthetic interactive sequences must keep one step per click across a save.

use std::io::{Cursor, Read};
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::anim::{AnimStart, Animation, TextBuild};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Presentation, Shape, ShapeId, defaults};

fn deck() -> Presentation {
    let mut p = defaults::new_presentation(None);
    let shapes: Vec<_> = ["Button", "Other button", "Content"]
        .into_iter()
        .map(|name| Shape {
            id: ShapeId(p.alloc_id().expect("id")),
            name: name.into(),
            xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
            text: Some(TextBody::from_text("First\nSecond\nThird")),
            ..Default::default()
        })
        .collect();
    Arc::make_mut(&mut p.slides[0]).shapes = shapes;
    p
}

fn round(p: &Presentation) -> Presentation {
    deckcraft_pptx::import(&deckcraft_pptx::export(p).expect("export")).expect("import")
}

fn slide_xml(p: &Presentation) -> String {
    let bytes = deckcraft_pptx::export(p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut xml = String::new();
    z.by_name("ppt/slides/slide1.xml").expect("slide").read_to_string(&mut xml).expect("xml");
    xml
}

#[test]
fn triggered_click_steps_survive_import_edit_save_reopen() {
    use AnimStart::{AfterPrevious, OnClick, WithPrevious};
    let mut p = deck();
    let ids: Vec<_> = p.slides[0].shapes.iter().map(|s| s.id).collect();
    let starts = [OnClick, WithPrevious, AfterPrevious, OnClick, WithPrevious, OnClick];
    for (i, start) in starts.into_iter().enumerate() {
        Arc::make_mut(&mut p.slides[0]).animations.push(Animation {
            shape: ids[2],
            trigger: Some(ids[0]),
            start,
            effect: "appear".into(),
            delay_ms: i as u32 * 100,
            ..Default::default()
        });
    }
    // An independent button and the main sequence must not borrow this button's steps.
    for trigger in [Some(ids[1]), None] {
        for start in [OnClick, AfterPrevious, OnClick] {
            Arc::make_mut(&mut p.slides[0]).animations.push(Animation { shape: ids[2], trigger, start, ..Default::default() });
        }
    }
    let mut q = round(&p);
    for pass in 0..2 {
        for trigger in [Some(ids[0]), Some(ids[1]), None] {
            let state = |deck: &Presentation| {
                deck.slides[0].animations.iter().filter(|a| a.trigger == trigger).map(|a| (a.start, a.delay_ms)).collect::<Vec<_>>()
            };
            assert_eq!(state(&q), state(&p), "trigger {trigger:?}, pass {pass}");
        }
        assert_eq!(q.slides[0].animations.len(), p.slides[0].animations.len());
        Arc::make_mut(&mut q.slides[0]).shapes[2].name = "Edited content".into();
        q = round(&q);
    }
    let xml = slide_xml(&p);
    assert_eq!(xml.matches("nodeType=\"interactiveSeq\"").count(), 2);
    assert_eq!(xml.matches("nodeType=\"clickEffect\"").count(), 7);
    // The first step in each interactive sequence begins at zero; subsequent steps wait.
    assert_eq!(xml.matches("delay=\"indefinite\"").count(), 5);
}

#[test]
fn triggered_paragraph_build_keeps_a_click_for_each_paragraph() {
    let mut p = deck();
    let ids: Vec<_> = p.slides[0].shapes.iter().map(|s| s.id).collect();
    Arc::make_mut(&mut p.slides[0]).animations =
        vec![Animation { shape: ids[2], trigger: Some(ids[0]), text_build: TextBuild::ByParagraph, start: AnimStart::OnClick, ..Default::default() }];
    for q in [p.clone(), round(&p)] {
        let xml = slide_xml(&q);
        assert_eq!(xml.matches("nodeType=\"clickEffect\"").count(), 3);
        assert_eq!(xml.matches("delay=\"indefinite\"").count(), 2);
        let back = round(&q);
        assert_eq!(back.slides[0].animations.len(), 1);
        assert_eq!(back.slides[0].animations[0].text_build, TextBuild::ByParagraph);
        assert_eq!(back.slides[0].animations[0].trigger, Some(ids[0]));
    }
}

/// A sequence that starts at a media bookmark (`onMediaBookmark`, #362) is not a click on the
/// media shape: it is kept verbatim, with its build entry, and a save writes it back with the
/// slide's shape ids and time-node ids of its own; once its shape is gone it is left out.
#[test]
fn a_media_bookmark_sequence_is_kept_and_written_back() {
    use std::io::Write;
    let mut p = deck();
    let ids: Vec<_> = p.slides[0].shapes.iter().map(|s| s.id).collect();
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.animations.push(Animation {
        shape: ids[2],
        trigger: Some(ids[0]),
        effect: "appear".into(),
        text_build: TextBuild::AsOne,
        ..Default::default()
    });
    slide.animations.push(Animation { shape: ids[1], effect: "fade".into(), ..Default::default() });
    assert_eq!(round(&p).slides[0].animations.len(), 2, "a click trigger still round-trips");
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut data = Vec::new();
        f.read_to_end(&mut data).expect("read");
        if f.name() == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(data).expect("utf8").replace(r#"evt="onClick""#, r#"evt="onMediaBookmark""#);
            data = xml.into_bytes();
        }
        out.start_file(f.name(), zip::write::SimpleFileOptions::default()).expect("start");
        out.write_all(&data).expect("write");
    }
    let q = deckcraft_pptx::import(&out.finish().expect("finish").into_inner()).expect("import");
    let s = &q.slides[0];
    assert_eq!(s.animations.len(), 1, "only the main sequence is modelled: {:?}", s.animations);
    assert_eq!(s.kept_sequences.len(), 1);
    assert_eq!(s.kept_sequences[0].builds.len(), 1, "its build entry is kept with it");
    let xml = slide_xml(&q);
    assert_eq!(xml.matches(r#"evt="onMediaBookmark""#).count(), 2, "start and next conditions: {xml}");
    let button = q.slides[0].shapes.iter().find(|sh| sh.name == "Button").expect("button");
    let file_id = |name: &str| {
        let at = xml.find(&format!(r#"name="{name}""#)).expect("shape");
        let head = &xml[..at];
        let id = &head[head.rfind(r#"id=""#).expect("id") + 4..];
        id[..id.find('"').expect("end")].to_string()
    };
    assert!(xml.contains(&format!(r#"<p:spTgt spid="{}"/></p:tgtEl></p:cond>"#, file_id("Button"))), "{xml}");
    assert_eq!(xml.matches(&format!(r#"<p:bldP spid="{}""#, file_id("Content"))).count(), 1, "{xml}");
    let mut ctn_ids: Vec<&str> = xml.split("<p:cTn id=\"").skip(1).map(|t| &t[..t.find('"').expect("end")]).collect();
    let n = ctn_ids.len();
    ctn_ids.sort_unstable();
    ctn_ids.dedup();
    assert_eq!(ctn_ids.len(), n, "time-node ids are unique: {xml}");
    let again = round(&q);
    assert_eq!(again.slides[0].kept_sequences.len(), 1, "a second save keeps it too");
    assert_eq!(again.slides[0].animations.len(), 1);
    // One that plays a sound names a relationship of the old file: it is left out, not written
    // with an id the saved slide does not have.
    let bytes = deckcraft_pptx::export(&q).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut data = Vec::new();
        f.read_to_end(&mut data).expect("read");
        if f.name() == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(data).expect("utf8").replacen(
                r#"evt="onMediaBookmark" delay="0">"#,
                r#"evt="onMediaBookmark" delay="0"><p:tgtEl><p:sndTgt r:embed="rId99" name="ding"/></p:tgtEl>"#,
                1,
            );
            data = xml.into_bytes();
        }
        out.start_file(f.name(), zip::write::SimpleFileOptions::default()).expect("start");
        out.write_all(&data).expect("write");
    }
    let snd = deckcraft_pptx::import(&out.finish().expect("finish").into_inner()).expect("import");
    assert!(snd.slides[0].kept_sequences.is_empty(), "{:?}", snd.slides[0].kept_sequences);
    // Without its trigger shape the sequence is left out rather than written dangling.
    let mut r = q.clone();
    Arc::make_mut(&mut r.slides[0]).shapes.retain(|sh| sh.id != button.id);
    assert!(!slide_xml(&r).contains("onMediaBookmark"));
}

/// A kept sequence on shapes whose model ids are not file ids (id 1 is reserved in the file), with a
/// `p:tn` reference and a build group on a shape that also has a modelled animation: the writer
/// maps the ids, renumbers the time nodes with the reference following, and gives the modelled
/// animation a group of its own.
#[test]
fn a_kept_sequence_is_written_with_file_ids_its_own_time_nodes_and_groups() {
    let mut p = deck();
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes[0].id = ShapeId(1);
    let (button, content) = (slide.shapes[0].id, slide.shapes[2].id);
    slide.animations.push(Animation { shape: content, effect: "fade".into(), text_build: TextBuild::AsOne, ..Default::default() });
    let ns = r#"xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
    slide.kept_sequences.push(deckcraft_model::KeptSequence {
        xml: format!(
            r#"<p:seq {ns} concurrent="1" nextAc="seek"><p:cTn id="7" restart="whenNotActive" fill="hold" evtFilter="cancelBubble" nodeType="interactiveSeq"><p:stCondLst><p:cond evt="onMediaBookmark" delay="0"><p:tgtEl><p:spTgt spid="{b}"/></p:tgtEl></p:cond></p:stCondLst><p:childTnLst><p:par><p:cTn id="8" fill="hold"><p:stCondLst><p:cond delay="0"><p:tn val="7"/></p:cond></p:stCondLst><p:childTnLst><p:par><p:cTn id="9" presetID="1" presetClass="entr" presetSubtype="0" fill="hold" grpId="0" nodeType="clickEffect"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst><p:set><p:cBhvr><p:cTn id="10" dur="1" fill="hold"/><p:tgtEl><p:spTgt spid="{c}"/></p:tgtEl><p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible"/></p:to></p:set></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn><p:nextCondLst><p:cond evt="onMediaBookmark" delay="0"><p:tgtEl><p:spTgt spid="{b}"/></p:tgtEl></p:cond></p:nextCondLst></p:seq>"#,
            b = button.0,
            c = content.0
        ),
        builds: vec![format!(r#"<p:bldP spid="{}" grpId="0" animBg="1" {ns}/>"#, content.0)],
    });
    let xml = slide_xml(&p);
    let file_id = |name: &str| {
        let at = xml.find(&format!(r#"name="{name}""#)).expect("shape");
        let head = &xml[..at];
        let id = &head[head.rfind(r#"id=""#).expect("id") + 4..];
        id[..id.find('"').expect("end")].parse::<u32>().expect("number")
    };
    let b = file_id("Button");
    assert_ne!(b, 1, "model id 1 is not a file id");
    assert_eq!(xml.matches(&format!(r#"<p:spTgt spid="{b}"/></p:tgtEl></p:cond>"#)).count(), 2, "{xml}");
    // The kept sequence's root node and the `p:tn` naming it carry the same new id.
    let seq_at = xml.find(r#"nodeType="interactiveSeq""#).expect("kept seq");
    let head = &xml[..seq_at];
    let root = &head[head.rfind(r#"<p:cTn id=""#).expect("cTn") + 11..];
    let root = &root[..root.find('"').expect("end")];
    assert_ne!(root, "7", "renumbered after the main sequence");
    assert!(xml.contains(&format!(r#"<p:tn val="{root}"/>"#)), "{xml}");
    // Content's kept build keeps group 0; its modelled fade takes another group.
    let c = file_id("Content");
    assert_eq!(xml.matches(&format!(r#"<p:bldP spid="{c}" grpId="0""#)).count(), 1, "{xml}");
    assert_eq!(xml.matches(&format!(r#"<p:bldP spid="{c}" grpId="1""#)).count(), 1, "{xml}");
    let back = round(&p);
    assert_eq!(back.slides[0].kept_sequences.len(), 1);
    assert_eq!(back.slides[0].animations.len(), 1);
    // A second kept sequence carrying the same build entry: the entry is written once, and stays
    // one through another save.
    let slide = Arc::make_mut(&mut p.slides[0]);
    let twin = slide.kept_sequences[0].clone();
    slide.kept_sequences.push(twin);
    let xml = slide_xml(&p);
    assert_eq!(xml.matches(&format!(r#"<p:bldP spid="{c}" grpId="0""#)).count(), 1, "{xml}");
    assert_eq!(slide_xml(&round(&p)).matches(r#"grpId="0" animBg="1""#).count(), 1);
}
