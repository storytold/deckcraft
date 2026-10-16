//! Independent OASIS 1.3 Appendix F regressions; contributor-original synthetic XML.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::*;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:anim='urn:oasis:names:tc:opendocument:xmlns:animation:1.0' xmlns:smil='urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";
fn package(timing: &str) -> Vec<u8> {
    let xml = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page xml:id='page'><draw:rect xml:id='target' svg:x='0pt' svg:y='0pt' svg:width='72pt' svg:height='72pt'/>{timing}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", &xml),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(value.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn opacity(begin: &str, duration: &str) -> String {
    format!(
        "<anim:animate smil:targetElement='target' smil:attributeName='opacity' smil:from='1' smil:to='0.5' smil:begin='{begin}' smil:dur='{duration}' smil:calcMode='linear'/>"
    )
}
fn warnings(p: &Presentation) -> bool {
    p.tags.iter().any(|(k, _)| k == "odp:warning")
}

#[test]
fn appendix_f_indefinite_group_imports_as_user_advance() {
    let timing = format!(
        "<anim:par presentation:node-type='timing-root'><anim:seq presentation:node-type='main-sequence'><anim:par smil:begin='indefinite'>{}</anim:par></anim:seq></anim:par>",
        opacity("0s", "1s")
    );
    let p = import(&package(&timing)).unwrap();
    assert!(!warnings(&p), "Appendix F.2 user advance is supported: {:?}", p.tags);
    assert_eq!(p.slides[0].animations.len(), 1);
    assert_eq!(p.slides[0].animations[0].start, AnimStart::OnClick);
}

#[test]
fn colored_page_fade_is_mapped_faithfully_or_reported() {
    let p = import(&package("<anim:par presentation:preset-id='deckcraft-slide-transition'><anim:transitionFilter smil:targetElement='page' smil:type='fade' smil:subtype='fadeOverColor' smil:fadeColor='#ff0000' smil:dur='1s'/></anim:par>")).unwrap();
    assert!(warnings(&p), "a red fade cannot silently become the model's through-black transition");
}

#[test]
fn parent_clipped_effect_is_mapped_faithfully_or_reported() {
    let p = import(&package(&format!("<anim:par smil:dur='1s'>{}</anim:par>", opacity("0s", "5s")))).unwrap();
    assert!(
        warnings(&p) || p.slides[0].animations.first().is_some_and(|a| a.duration_ms <= 1000),
        "SMIL container duration clips its children; silently importing a five-second effect changes playback"
    );
}

#[test]
fn external_xml_id_spelling_does_not_reorder_a_sequence() {
    let first = opacity("0s", "1s");
    let second = opacity("0s", "1s").replace("smil:to='0.5'", "smil:to='0.3'");
    let timing =
        format!("<anim:seq><anim:par xml:id='effectExternal-9'>{first}</anim:par><anim:par xml:id='effectExternal-0'>{second}</anim:par></anim:seq>");
    let p = import(&package(&timing)).unwrap();
    assert!(!warnings(&p), "{:?}", p.tags);
    assert_eq!(
        p.slides[0].animations.iter().map(|a| a.amount.unwrap()).collect::<Vec<_>>(),
        [0.5, 0.7],
        "SMIL sequence order is semantic; XML ID numeric suffixes are not"
    );
}

#[test]
fn disabled_slide_advance_is_preserved_or_export_refuses() {
    let mut p = Presentation::default();
    Arc::make_mut(&mut p.slides[0]).transition =
        Some(Transition { kind: "none".into(), duration_ms: 0, advance_on_click: false, advance_after_ms: None, ..Default::default() });
    if let Ok(bytes) = export(&p) {
        let q = import(&bytes).expect("exported package must be readable");
        assert!(!q.slides[0].transition.as_ref().unwrap().advance_on_click, "disabled click advance must not silently become manual advance");
    }
}

#[test]
fn exported_long_motion_path_respects_the_xml_attribute_import_budget() {
    let mut p = Presentation::default();
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes.push(Shape { id: ShapeId(100), xfrm: Some(Xfrm::new(0.0, 0.0, 72.0, 72.0)), ..Default::default() });
    slide.animations.push(Animation {
        shape: ShapeId(100),
        class: AnimClass::Path,
        effect: "customPath".into(),
        path: Some(format!("M 0 0 {} E", "L 0.123 0.456 ".repeat(1000))),
        ..Default::default()
    });
    if let Ok(bytes) = export(&p) {
        assert!(import(&bytes).is_ok(), "the writer must refuse paths exceeding its own reader's attribute budget");
    }
}

#[derive(Default)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    kids: Vec<Node>,
}
fn tree(xml: &str) -> Node {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack = vec![Node::default()];
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                let empty = e.as_ref().last() == Some(&b'/');
                // Event::Empty omits the slash from BytesStart; inspect source immediately before '>'.
                let empty = empty || xml.as_bytes().get(reader.buffer_position() as usize - 2) == Some(&b'/');
                let n = Node {
                    name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                    attrs: e
                        .attributes()
                        .map(|a| {
                            let a = a.unwrap();
                            (
                                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                                a.normalized_value(quick_xml::XmlVersion::Implicit1_0).unwrap().into_owned(),
                            )
                        })
                        .collect(),
                    kids: Vec::new(),
                };
                if empty {
                    stack.last_mut().unwrap().kids.push(n);
                } else {
                    stack.push(n);
                }
            }
            quick_xml::events::Event::End(_) => {
                let n = stack.pop().unwrap();
                stack.last_mut().unwrap().kids.push(n);
            }
            quick_xml::events::Event::Eof => return stack.pop().unwrap(),
            _ => {}
        }
    }
}
fn ms(value: &str) -> f64 {
    value.strip_suffix('s').unwrap().parse::<f64>().unwrap() * 1000.0
}
// Small independent SMIL scheduler: numeric offsets use their container's clock;
// syncbases use the referenced element's absolute start/end; seq children follow predecessors.
fn schedule(n: &Node, parent: f64, refs: &mut BTreeMap<String, (f64, f64)>, effects: &mut Vec<(String, f64)>) -> f64 {
    let begin = n.attrs.get("smil:begin").map(String::as_str).unwrap_or("0s");
    let start = if matches!(begin, "indefinite" | "next" | "page.begin") || begin.ends_with(".begin") && !begin.starts_with("effect") {
        parent
    } else if let Some((reference, offset)) = begin.split_once('+') {
        let (id, event) = reference.rsplit_once('.').unwrap();
        let &(start, end) = refs.get(id).expect("syncbase must resolve");
        (if event == "begin" { start } else { end }) + ms(offset)
    } else {
        parent + ms(begin)
    };
    if let Some(effect) = n.attrs.get("presentation:preset-id") {
        effects.push((effect.clone(), start));
    }
    let mut end = start;
    for child in &n.kids {
        let child_end = schedule(child, if n.name == "anim:seq" { end } else { start }, refs, effects);
        end = end.max(child_end);
    }
    if let Some(duration) = n.attrs.get("smil:dur") {
        let repeat = n.attrs.get("smil:repeatCount").map_or(1.0, |r| r.parse::<f64>().unwrap());
        end = start + ms(duration) * repeat * if n.attrs.get("smil:autoReverse").is_some_and(|v| v == "true") { 2.0 } else { 1.0 };
    }
    if let Some(id) = n.attrs.get("xml:id") {
        refs.insert(id.clone(), (start, end));
    }
    end
}

#[test]
fn exported_smil_matches_actual_model_concurrent_group_timing() {
    let mut p = defaults::blank_presentation(Size::new(720.0, 405.0), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let mut slide = Slide { id: SlideId(99), layout, show_master_shapes: false, ..Default::default() };
    slide.shapes.push(Shape { id: ShapeId(100), xfrm: Some(Xfrm::new(0.0, 0.0, 72.0, 72.0)), ..Default::default() });
    for (effect, start, duration_ms, delay_ms) in
        [("fade", AnimStart::OnClick, 1000, 200), ("transparency", AnimStart::WithPrevious, 100, 50), ("spin", AnimStart::AfterPrevious, 100, 25)]
    {
        slide.animations.push(Animation {
            shape: ShapeId(100),
            class: if effect == "fade" { AnimClass::Entrance } else { AnimClass::Emphasis },
            effect: effect.into(),
            start,
            duration_ms,
            delay_ms,
            amount: Some(if effect == "spin" { 90.0 } else { 0.5 }),
            ..Default::default()
        });
    }
    p.slides.push(Arc::new(slide));
    let bytes = export(&p).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    z.by_name("content.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut effects = Vec::new();
    schedule(&tree(&xml), 0.0, &mut BTreeMap::new(), &mut effects);
    let starts: Vec<_> =
        effects.into_iter().filter(|(name, _)| ["fade", "transparency", "spin"].contains(&name.as_str())).map(|(_, time)| time).collect();
    // Independently derived from anim::Timeline: the first concurrent group starts at zero;
    // after-previous follows the longest concurrent effect, ending at 1200ms.
    assert_eq!(starts, [200.0, 50.0, 1225.0]);
}
