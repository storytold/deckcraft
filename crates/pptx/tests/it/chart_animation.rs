//! Original synthetic chart-animation preservation probes (#2061).
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::anim::{Animation, TextBuild};
use deckcraft_model::chart::{Chart, ChartType, Series};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, TextBody, defaults};
use deckcraft_pptx::XmlEl;

fn chart_deck(count: usize) -> Presentation {
    let mut p = defaults::new_presentation(None);
    let id = ShapeId(p.alloc_id().expect("id"));
    let chart = Chart {
        kind: ChartType::Column,
        categories: vec!["Cycle A".into(), "Cycle B".into()],
        series: vec![
            Series { name: "North".into(), values: vec![Some(2.0), Some(5.0)], ..Default::default() },
            Series { name: "South".into(), values: vec![Some(4.0), Some(3.0)], ..Default::default() },
        ],
        ..Default::default()
    };
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes = vec![Shape {
        id,
        name: "Original chart".into(),
        xfrm: Some(Xfrm::new(100.0, 70.0, 600.0, 360.0)),
        kind: ShapeKind::Chart(Box::new(chart)),
        ..Default::default()
    }];
    slide.animations = (0..count).map(|_| Animation { shape: id, effect: "appear".into(), duration_ms: 1, ..Default::default() }).collect();
    p
}

fn effects<'a>(el: &'a XmlEl, out: &mut Vec<&'a XmlEl>) {
    if el.is("cTn") && el.has_attr("presetClass") {
        out.push(el);
    }
    for child in el.elements() {
        effects(child, out);
    }
}

fn slide_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    std::io::read_to_string(z.by_name("ppt/slides/slide1.xml").expect("slide")).expect("xml")
}

fn rewrite_xml(bytes: &[u8], change: impl Fn(&str, &str) -> String) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut data = vec![];
        f.read_to_end(&mut data).expect("read");
        if name.ends_with(".xml") {
            data = change(&name, std::str::from_utf8(&data).expect("utf8")).into_bytes();
        }
        w.start_file(name, zip::write::SimpleFileOptions::default()).expect("entry");
        w.write_all(&data).expect("write");
    }
    w.finish().expect("zip").into_inner()
}

/// Give the chart a single native series build, leaving an optional whole-chart effect outside it.
fn fixture(background: bool, generic_tail: bool) -> Vec<u8> {
    let mut selectors = vec![r#"bldStep="series" seriesIdx="0""#, r#"bldStep="series" seriesIdx="1""#];
    if background {
        selectors.insert(0, r#"bldStep="gridLegend""#);
    }
    let bytes = deckcraft_pptx::export(&chart_deck(selectors.len() + usize::from(generic_tail))).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut data = vec![];
        f.read_to_end(&mut data).expect("read");
        if name == "ppt/slides/slide1.xml" {
            let mut xml = String::from_utf8(data).expect("utf8");
            let doc = deckcraft_pptx::parse_xml(xml.as_bytes()).expect("xml");
            let mut found = vec![];
            effects(doc.root.child("timing").expect("timing"), &mut found);
            let mut spid = String::new();
            for (effect, selector) in found.iter().zip(&selectors) {
                let original = effect.to_xml();
                assert!(xml.contains(&original));
                let target = effect.find("spTgt").expect("target");
                spid = target.attr("spid").expect("spid").to_string();
                let mut grouped = (*effect).clone();
                grouped.set_attr("grpId", "0");
                let changed = grouped
                    .to_xml()
                    .replace(&target.to_xml(), &format!(r#"<p:spTgt spid="{spid}"><p:graphicEl><a:chart {selector}/></p:graphicEl></p:spTgt>"#));
                xml = xml.replace(&original, &changed);
            }
            xml = xml.replace("</p:timing>", &format!(r#"<p:bldLst><p:bldGraphic spid="{spid}" grpId="0"><p:bldSub><a:bldChart bld="series" animBg="{}"/></p:bldSub></p:bldGraphic></p:bldLst></p:timing>"#, u8::from(background)));
            data = xml.into_bytes();
        }
        w.start_file(name, zip::write::SimpleFileOptions::default()).expect("entry");
        w.write_all(&data).expect("write");
    }
    w.finish().expect("zip").into_inner()
}

#[derive(Debug, PartialEq)]
struct Witness {
    targets: Vec<Option<(String, Option<i32>, Option<i32>)>>,
    groups: Vec<u32>,
    builds: Vec<(u32, String, bool)>,
}

fn witness(bytes: &[u8]) -> Witness {
    let xml = slide_xml(bytes);
    let doc = deckcraft_pptx::parse_xml(xml.as_bytes()).expect("xml");
    let timing = doc.root.child("timing").expect("timing");
    let mut found = vec![];
    effects(timing, &mut found);
    Witness {
        targets: found
            .iter()
            .map(|e| {
                e.find("spTgt")
                    .and_then(|t| t.path(&["graphicEl", "chart"]))
                    .map(|t| (t.attr("bldStep").expect("step").into(), t.i32("seriesIdx"), t.i32("categoryIdx")))
            })
            .collect(),
        groups: found.iter().map(|e| e.u32("grpId").expect("group")).collect(),
        builds: timing
            .child("bldLst")
            .into_iter()
            .flat_map(|l| l.children_named("bldGraphic"))
            .map(|g| {
                let b = g.path(&["bldSub", "bldChart"]).expect("chart build");
                (g.u32("grpId").expect("group"), b.attr("bld").expect("build").into(), b.bool("animBg").unwrap_or(false))
            })
            .collect(),
    }
}

#[test]
fn chart_series_targets_and_builds_survive_two_saves() {
    for background in [true, false] {
        let mut bytes = fixture(background, false);
        let expected = witness(&bytes);
        assert_eq!(expected.targets.len(), if background { 3 } else { 2 });
        for _ in 0..2 {
            let (p, lost) = deckcraft_pptx::import_checked(&bytes).expect("import");
            assert!(lost.is_empty());
            let native: Presentation = serde_json::from_value(serde_json::to_value(&p).expect("serialize")).expect("deserialize");
            assert_eq!(native.slides[0].animations, p.slides[0].animations);
            let native_bytes = deckcraft_engine::cmd::file::save_bytes(&native, "deckcraft").expect("native save");
            let native = deckcraft_engine::cmd::file::open_presentation("chart.deckcraft", &native_bytes).expect("native reopen");
            assert_eq!(native.slides[0].animations, p.slides[0].animations);
            bytes = deckcraft_pptx::export(&native).expect("export");
            assert_eq!(witness(&bytes), expected, "chart targets/build grouping must survive");
        }
    }
}

#[test]
fn generic_effect_does_not_join_an_imported_chart_build() {
    let mut bytes = fixture(true, true);
    let expected = witness(&bytes);
    for _ in 0..2 {
        let p = deckcraft_pptx::import(&bytes).expect("import");
        bytes = deckcraft_pptx::export(&p).expect("export");
        let actual = witness(&bytes);
        assert_eq!(actual.targets, expected.targets);
        assert_eq!(actual.builds, expected.builds);
        assert_eq!(&actual.groups[..3], &[0, 0, 0]);
        assert_ne!(actual.groups[3], 0, "whole-chart effect must not join the series group");
    }
}

#[test]
fn sparse_chart_indices_and_maximal_group_ids_are_retained() {
    for group in [7, u32::MAX] {
        let mut bytes = rewrite_xml(&fixture(true, true), |name, xml| {
            if name == "ppt/slides/slide1.xml" {
                xml.replace(r#"grpId="0""#, &format!(r#"grpId="{group}""#))
                    .replace(r#"bldStep="gridLegend""#, r#"bldStep="gridLegend" seriesIdx="-1" categoryIdx="-1""#)
                    .replace(r#"seriesIdx="0""#, r#"seriesIdx="7" categoryIdx="0""#)
                    .replace(r#"seriesIdx="1""#, r#"seriesIdx="19" categoryIdx="1""#)
            } else if name.starts_with("ppt/charts/chart") {
                xml.replace(r#"<c:idx val="0"/>"#, r#"<c:idx val="7"/>"#).replace(r#"<c:idx val="1"/>"#, r#"<c:idx val="19"/>"#)
            } else {
                xml.into()
            }
        });
        let expected = witness(&bytes);
        assert_eq!(expected.targets[1].as_ref().expect("series").1, Some(7));
        assert_eq!(expected.targets[2].as_ref().expect("series").1, Some(19));
        for _ in 0..2 {
            let p = deckcraft_pptx::import(&bytes).expect("import");
            bytes = deckcraft_pptx::export(&p).expect("export");
            let actual = witness(&bytes);
            assert_eq!(actual.targets, expected.targets);
            assert_eq!(actual.builds, expected.builds);
            assert_eq!(&actual.groups[..3], &[group, group, group]);
            assert_ne!(actual.groups[3], group);
            let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
            let chart = std::io::read_to_string(z.by_name("ppt/charts/chart1.xml").expect("chart")).expect("xml");
            assert!(
                chart.contains(r#"<c:idx val="7"/>"#) && chart.contains(r#"<c:idx val="19"/>"#),
                "series identities must still match their targets"
            );
        }
    }
}

#[test]
fn generic_chart_and_paragraph_animations_remain_unchanged() {
    let mut p = chart_deck(1);
    let text_id = ShapeId(p.alloc_id().expect("id"));
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes.push(Shape { id: text_id, text: Some(TextBody::from_text("First\nSecond")), ..Default::default() });
    slide.animations.push(Animation { shape: text_id, effect: "appear".into(), text_build: TextBuild::ByParagraph, ..Default::default() });
    let mut bytes = deckcraft_pptx::export(&p).expect("export");
    for _ in 0..2 {
        let p = deckcraft_pptx::import(&bytes).expect("import");
        assert_eq!(p.slides[0].animations.len(), 2);
        assert_eq!(p.slides[0].animations[1].text_build, TextBuild::ByParagraph);
        assert!(!serde_json::to_string(&p.slides[0].animations).expect("json").contains("chartTarget"));
        bytes = deckcraft_pptx::export(&p).expect("export");
        let w = witness(&bytes);
        assert_eq!(w.targets, vec![None, None, None]);
        assert!(w.builds.is_empty());
    }
}

#[test]
fn painter_keeps_chart_selectors_only_on_the_original_shape() {
    for target_kind in ["plain", "chart", "self"] {
        let mut p = deckcraft_pptx::import(&fixture(true, false)).expect("import");
        let original = p.slides[0].shapes[0].id;
        let target = if target_kind == "self" {
            original
        } else {
            let id = ShapeId(p.alloc_id().expect("id"));
            let mut shape = p.slides[0].shapes[0].clone();
            shape.id = id;
            if target_kind == "plain" {
                shape.kind = ShapeKind::Shape;
            }
            Arc::make_mut(&mut p.slides[0]).shapes.push(shape);
            id
        };
        let expected = p.slides[0].animations.clone();
        let mut session = deckcraft_engine::Session::new();
        session.add_document(deckcraft_engine::DocState::new(p, None, "Painter probe".into()));
        session.execute("animation.painter", &serde_json::json!({"id": original.0})).expect("pick");
        session.execute("animation.painterApply", &serde_json::json!({"ids": [target.0]})).expect("paint");
        let doc = &session.doc().expect("doc").doc;
        let painted: Vec<_> = doc.slides[0].animations.iter().filter(|a| a.shape == target).collect();
        assert_eq!(painted.len(), expected.len());
        for (actual, original_animation) in painted.iter().zip(&expected) {
            if target == original {
                assert_eq!(*actual, original_animation, "painting onto itself retains native selectors");
            } else {
                assert_eq!(actual.chart_target, None, "{target_kind}: selectors belong to the original chart");
                assert_eq!(actual.chart_build, None, "{target_kind}: build groups belong to the original chart");
            }
        }
        let saved = deckcraft_pptx::export(doc).expect("export");
        let reopened = deckcraft_pptx::import(&saved).expect("reopen");
        assert_eq!(reopened.slides[0].animations.len(), doc.slides[0].animations.len());
    }
}

#[test]
fn chart_build_expansion_state_survives_two_saves() {
    for expanded in [false, true] {
        let mut bytes = rewrite_xml(&fixture(true, false), |name, xml| {
            if name == "ppt/slides/slide1.xml" {
                xml.replace("<p:bldGraphic ", &format!("<p:bldGraphic uiExpand=\"{}\" ", u8::from(expanded)))
            } else {
                xml.into()
            }
        });
        for _ in 0..2 {
            let p = deckcraft_pptx::import(&bytes).expect("import");
            assert!(p.slides[0].animations.iter().all(|a| a.chart_build.expect("build").ui_expand == Some(expanded)));
            bytes = deckcraft_pptx::export(&p).expect("export");
            let xml = slide_xml(&bytes);
            let doc = deckcraft_pptx::parse_xml(xml.as_bytes()).expect("xml");
            let build = doc.root.path(&["timing", "bldLst", "bldGraphic"]).expect("build");
            assert_eq!(build.bool("uiExpand"), Some(expanded));
        }
    }
}

#[test]
fn painter_does_not_reuse_chart_selectors_for_another_documents_matching_shape_id() {
    let original = deckcraft_pptx::import(&fixture(true, false)).expect("import");
    let source = original.slides[0].shapes[0].id;
    let expected = original.slides[0].animations.clone();
    let mut other = original.clone();
    let slide = Arc::make_mut(&mut other.slides[0]);
    slide.animations.clear();
    if let ShapeKind::Chart(chart) = &mut slide.shapes[0].kind {
        chart.series.truncate(1);
    }
    assert_eq!(slide.shapes[0].id, source, "different documents can allocate the same numeric shape id");
    let mut session = deckcraft_engine::Session::new();
    let original_index = session.add_document(deckcraft_engine::DocState::new(original, None, "Source".into()));
    let source_uid = session.doc().expect("doc").uid;
    let other_index = session.add_document(deckcraft_engine::DocState::new(other, None, "Other".into()));
    assert_ne!(session.doc().expect("doc").uid, source_uid);
    session.execute("window.next", &serde_json::json!({"index": original_index})).expect("activate source");
    session.execute("animation.painter", &serde_json::json!({"id": source.0, "sticky": true})).expect("pick");
    session.execute("window.next", &serde_json::json!({"index": other_index})).expect("activate other");
    session.execute("animation.painterApply", &serde_json::json!({"ids": [source.0]})).expect("paint other document");
    let other_animations = &session.doc().expect("doc").doc.slides[0].animations;
    assert_eq!(other_animations.len(), expected.len());
    for animation in other_animations {
        assert_eq!(animation.chart_target, None, "matching numeric IDs in different documents are not the same chart");
        assert_eq!(animation.chart_build, None, "build groups belong to the source document");
    }
    session.execute("window.next", &serde_json::json!({"index": original_index})).expect("return to source");
    session.execute("animation.painterApply", &serde_json::json!({"ids": [source.0]})).expect("paint original chart");
    assert_eq!(session.doc().expect("doc").doc.slides[0].animations, expected, "returning to the source chart retains its selectors");
}
