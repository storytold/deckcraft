#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Contributor-original timing corpus; no application-produced fixture files.
use deckcraft_model::{anim::TextBuild, *};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
fn deck() -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(720.0, 405.0), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let shapes = (1..=3)
        .map(|i| Shape {
            id: ShapeId(i),
            name: format!("Object {i}"),
            xfrm: Some(Xfrm::new(20.0 + f64::from(i) * 50.0, 30.0, 100.0, 80.0)),
            fill: Some(Fill::None),
            line: Some(Line::none()),
            text: Some(TextBody::from_text("Original paragraph\nTarget paragraph")),
            ..Default::default()
        })
        .collect();
    p.slides.push(Arc::new(Slide { id: SlideId(20), layout, shapes, show_master_shapes: false, ..Default::default() }));
    p.slides.push(Arc::new(Slide { id: SlideId(21), layout, ..Default::default() }));
    p
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}
fn audio() -> Vec<u8> {
    let mut b = b"RIFF".to_vec();
    b.extend(36u32.to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(8000u32.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(0u32.to_le_bytes());
    b
}
fn semantic_fixture() -> Presentation {
    let mut p = deck();
    p.media.push(MediaItem {
        id: MediaId(777),
        name: "original-silence.wav".into(),
        content_type: "audio/wav".into(),
        data: Arc::new(audio()),
        ..Default::default()
    });
    let s = Arc::make_mut(&mut p.slides[0]);
    s.transition = Some(Transition {
        kind: "fade".into(),
        option: "smoothly".into(),
        duration_ms: 1325,
        advance_on_click: false,
        advance_after_ms: Some(3400),
        sound: Some(MediaId(777)),
        sound_loop: true,
        stop_sound: true,
        ..Default::default()
    });
    s.animations = vec![
        Animation {
            shape: ShapeId(1),
            effect: "fade".into(),
            duration_ms: 730,
            delay_ms: 125,
            repeat: 3,
            rewind: true,
            smooth_start: 0.15,
            smooth_end: 0.25,
            auto_reverse: true,
            ..Default::default()
        },
        Animation {
            shape: ShapeId(2),
            class: AnimClass::Emphasis,
            effect: "spin".into(),
            option: "counterClockwise".into(),
            duration_ms: 910,
            delay_ms: 50,
            start: AnimStart::WithPrevious,
            amount: Some(135.0),
            ..Default::default()
        },
        Animation {
            shape: ShapeId(1),
            class: AnimClass::Emphasis,
            effect: "transparency".into(),
            duration_ms: 515,
            delay_ms: 20,
            start: AnimStart::AfterPrevious,
            amount: Some(0.4),
            ..Default::default()
        },
        Animation {
            shape: ShapeId(3),
            class: AnimClass::Exit,
            effect: "fadeOut".into(),
            duration_ms: 402,
            trigger: Some(ShapeId(2)),
            ..Default::default()
        },
        Animation {
            shape: ShapeId(1),
            class: AnimClass::Path,
            effect: "customPath".into(),
            duration_ms: 603,
            path: Some("M 0 0 L 0.2 -0.1 E".into()),
            ..Default::default()
        },
        Animation { shape: ShapeId(3), effect: "appear".into(), duration_ms: 0, paragraph: Some(1), ..Default::default() },
        Animation {
            shape: ShapeId(3),
            class: AnimClass::Exit,
            effect: "disappear".into(),
            duration_ms: 0,
            start: AnimStart::AfterPrevious,
            ..Default::default()
        },
        Animation {
            shape: ShapeId(1),
            class: AnimClass::Emphasis,
            effect: "fontColor".into(),
            duration_ms: 611,
            color: Some(ColorRef::rgb(Rgba::rgb(30, 60, 90))),
            sound: Some(MediaId(777)),
            ..Default::default()
        },
        Animation {
            shape: ShapeId(2),
            class: AnimClass::Emphasis,
            effect: "growShrink".into(),
            option: "horz".into(),
            duration_ms: 1111,
            amount: Some(1.8),
            repeat: u32::MAX,
            ..Default::default()
        },
    ];
    Arc::make_mut(&mut p.slides[1]).transition = Some(Transition { kind: "none".into(), duration_ms: 0, stop_sound: true, ..Default::default() });
    p
}
fn shape_name(p: &Presentation, id: ShapeId) -> &str {
    p.slides[0].shapes.iter().find(|s| s.id == id).unwrap().name.as_str()
}
#[test]
fn native_timing_roundtrip_keeps_exact_clocks_targets_order_and_audio() {
    let p = semantic_fixture();
    let bytes = export(&p).unwrap();
    let q = import(&bytes).unwrap();
    assert!(!q.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", q.tags);
    let a = &p.slides[0].animations;
    let b = &q.slides[0].animations;
    assert_eq!(a.len(), b.len(), "{:?}", b);
    for (expected, actual) in a.iter().zip(b) {
        assert_eq!(expected.effect, actual.effect);
        assert_eq!(shape_name(&p, expected.shape), shape_name(&q, actual.shape));
        assert_eq!(expected.start, actual.start);
        assert_eq!(expected.duration_ms, actual.duration_ms);
        assert_eq!(expected.delay_ms, actual.delay_ms);
        assert_eq!(expected.repeat, actual.repeat);
        assert_eq!(expected.rewind, actual.rewind);
        assert_eq!(expected.auto_reverse, actual.auto_reverse);
        assert_eq!(expected.smooth_start, actual.smooth_start);
        assert_eq!(expected.smooth_end, actual.smooth_end);
        assert_eq!(expected.paragraph, actual.paragraph);
        assert_eq!(expected.trigger.map(|id| shape_name(&p, id)), actual.trigger.map(|id| shape_name(&q, id)));
        if let (Some(a), Some(b)) = (expected.amount, actual.amount) {
            assert!((a - b).abs() < 1e-10);
        }
    }
    let t = q.slides[0].transition.as_ref().unwrap();
    assert_eq!(t.duration_ms, 1325);
    assert_eq!(t.advance_after_ms, Some(3400));
    assert!(!t.advance_on_click);
    assert!(t.sound_loop);
    assert!(t.stop_sound);
    let media = q.media.iter().find(|m| Some(m.id) == t.sound).unwrap();
    assert_eq!(media.data.as_slice(), audio());
    assert!(q.slides[1].transition.as_ref().unwrap().stop_sound);
    let xml = part(&bytes, "content.xml");
    assert!(xml.contains("smil:dur=\"1.325s\""));
    assert!(xml.contains("slide20-shape3-p1"));
    assert!(xml.contains("smil:begin=\"4.525s\""));
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_TIMING_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml", "meta.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(name), part(&bytes, name)).unwrap();
        }
        std::fs::write(std::path::Path::new(&dir).join("native-timing.odp"), bytes).unwrap();
    }
}
fn package(shapes: &str, timing: &str) -> Vec<u8> {
    let ns = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:anim='urn:oasis:names:tc:opendocument:xmlns:animation:1.0' xmlns:smil='urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";
    let xml = format!(
        "<office:document-content {ns} office:version='1.3'><office:body><office:presentation><draw:page xml:id='pageA'>{shapes}{timing}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", &xml),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'/>"),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
const TARGET: &str = "<draw:rect xml:id='targetA' draw:name='Independent object' svg:x='1in' svg:y='1in' svg:width='1in' svg:height='1in'/>";
#[test]
fn independently_authored_smil_uses_declared_targets_and_real_clock_units() {
    let timing = "<anim:seq presentation:node-type='main-sequence'><anim:par xml:id='first' presentation:node-type='on-click' smil:begin='next' smil:dur='750ms'><anim:transitionFilter smil:targetElement='targetA' smil:type='fade' smil:subtype='crossfade' smil:mode='in' smil:dur='750ms'/></anim:par><anim:par presentation:node-type='after-previous' smil:begin='first.end+0.125s'><anim:animate smil:targetElement='targetA' smil:attributeName='opacity' smil:from='1' smil:to='0.3' smil:dur='00:01.250' smil:calcMode='linear'/></anim:par></anim:seq>";
    let p = import(&package(TARGET, timing)).unwrap();
    assert!(!p.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", p.tags);
    assert_eq!(p.slides[0].animations.len(), 2);
    let a = &p.slides[0].animations[0];
    assert_eq!(a.duration_ms, 750);
    assert_eq!(a.shape, p.slides[0].shapes[0].id);
    let a = &p.slides[0].animations[1];
    assert_eq!(a.start, AnimStart::AfterPrevious);
    assert_eq!(a.delay_ms, 125);
    assert_eq!(a.duration_ms, 1250);
    assert_eq!(a.amount, Some(0.7));
}
#[test]
fn unsupported_and_missing_target_timing_warns_and_keeps_original_archive() {
    for timing in [
        "<anim:animate smil:targetElement='missing' smil:attributeName='opacity' smil:to='0.3' smil:dur='1s'/>",
        "<anim:iterate anim:iterate-type='by-word' anim:iterate-interval='PT0.1S'><anim:animate smil:targetElement='targetA' smil:attributeName='opacity' smil:to='0.3' smil:dur='1s'/></anim:iterate>",
        "<anim:animate smil:targetElement='targetA' smil:attributeName='opacity' smil:to='0.3' smil:dur='NaNs'/>",
    ] {
        let bytes = package(TARGET, timing);
        let p = import(&bytes).unwrap();
        assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
        assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
        assert!(export(&p).is_err());
    }
}
#[test]
fn malformed_model_timing_refuses_without_dropping_animations() {
    let mut p = deck();
    for a in [
        Animation { shape: ShapeId(9999), ..Default::default() },
        Animation { shape: ShapeId(1), smooth_start: f64::NAN, ..Default::default() },
        Animation { shape: ShapeId(1), text_build: TextBuild::ByLetter, ..Default::default() },
        Animation { shape: ShapeId(1), duration_ms: 86_400_001, ..Default::default() },
        Animation { shape: ShapeId(1), delay_ms: 86_400_001, ..Default::default() },
    ] {
        Arc::make_mut(&mut p.slides[0]).animations = vec![a];
        assert!(export(&p).is_err());
    }
}

#[test]
fn exported_transform_samples_keep_shape_center_fixed() {
    use std::collections::BTreeMap;
    let bytes = export(&semantic_fixture()).unwrap();
    let xml = part(&bytes, "content.xml");
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut transforms = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) if e.name().as_ref() == b"anim:animateTransform" => {
                let attrs = e
                    .attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (
                            String::from_utf8(a.key.as_ref().to_vec()).unwrap(),
                            a.normalized_value(quick_xml::XmlVersion::Explicit1_0).unwrap().into_owned(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                transforms.push(attrs);
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    fn nums(s: &str) -> Vec<f64> {
        s.split_whitespace().map(|v| v.parse().unwrap()).collect()
    }
    let rotate = transforms.iter().find(|p| p.get("svg:type").map(String::as_str) == Some("rotate")).unwrap();
    let values = nums(&rotate["smil:to"]);
    assert_eq!(values, [-135.0, 170.0, 70.0]);
    let scale = transforms.iter().find(|p| p.get("svg:type").map(String::as_str) == Some("scale")).unwrap();
    let translate = transforms.iter().find(|p| p.get("svg:type").map(String::as_str) == Some("translate")).unwrap();
    let scale = nums(&scale["smil:to"]);
    let translate = nums(&translate["smil:to"]);
    for progress in [0.0, 0.25, 0.5, 1.0] {
        let x = translate[0] * progress + (1.0 + (scale[0] - 1.0) * progress) * 170.0;
        let y = translate[1] * progress + (1.0 + (scale[1] - 1.0) * progress) * 70.0;
        assert!((x - 170.0).abs() < 1e-8);
        assert!((y - 70.0).abs() < 1e-8);
    }
    assert!(xml.contains("smil:end=\"slide21.begin\""));
}

fn mutate_content(bytes: &[u8], change: impl FnOnce(String) -> String) -> Vec<u8> {
    let mut input = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = Vec::new();
    for i in 0..input.len() {
        let mut part = input.by_index(i).unwrap();
        let name = part.name().to_string();
        let mut data = Vec::new();
        part.read_to_end(&mut data).unwrap();
        entries.push((name, data));
    }
    let content = entries.iter_mut().find(|(name, _)| name == "content.xml").unwrap();
    content.1 = change(String::from_utf8(std::mem::take(&mut content.1)).unwrap()).into_bytes();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        let method = if name == "mimetype" { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
        output.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method)).unwrap();
        output.write_all(&data).unwrap();
    }
    output.finish().unwrap().into_inner()
}
#[test]
fn transition_audio_ends_are_validated_against_actual_future_stop_pages() {
    let mut p = semantic_fixture();
    p.slides.push(Arc::new(Slide { id: SlideId(22), layout: p.slides[0].layout, ..Default::default() }));
    let original = export(&p).unwrap();
    let q = import(&original).unwrap();
    assert!(!q.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", q.tags);
    for end in ["missing.begin", "slide22.begin"] {
        let bytes = mutate_content(&original, |xml| xml.replace("smil:end=\"slide21.begin\"", &format!("smil:end=\"{end}\"")));
        let q = import(&bytes).unwrap();
        assert!(q.tags.iter().any(|(k, v)| k == "odp:warning" && v.contains("audio ends")), "{:?}", q.tags);
        assert!(export(&q).is_err());
    }
}
#[test]
fn delayed_stop_and_transition_audio_are_reported_without_immediate_approximation() {
    let original = export(&semantic_fixture()).unwrap();
    let stopped = mutate_content(&original, |xml| {
        xml.replace("anim:command=\"stop-audio\" smil:begin=\"slide21.begin\"", "anim:command=\"stop-audio\" smil:begin=\"2s\"")
    });
    let q = import(&stopped).unwrap();
    assert!(q.tags.iter().any(|(k, v)| k == "odp:warning" && v.contains("stop-audio")));
    assert!(!q.slides[1].transition.as_ref().unwrap().stop_sound);
    let delayed = mutate_content(&original, |xml| {
        xml.replace("xlink:href=\"Pictures/media777.wav\" smil:begin=\"slide20.begin\"", "xlink:href=\"Pictures/media777.wav\" smil:begin=\"2s\"")
    });
    let q = import(&delayed).unwrap();
    assert!(q.tags.iter().any(|(k, v)| k == "odp:warning" && v.contains("transition audio")), "{:?}", q.tags);
    assert!(q.slides[0].transition.as_ref().unwrap().sound.is_none());
}
