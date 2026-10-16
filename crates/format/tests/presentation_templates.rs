//! Independent native template persistence and corruption-boundary fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_format::{MIMETYPE, load, load_checked, save};
use deckcraft_model::*;
use serde_json::{Value, json};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

fn package(presentation: &str, positional_root: bool) -> Vec<u8> {
    let manifest = if positional_root {
        format!(r#"["deckcraft",1,"independent",{presentation}]"#)
    } else {
        format!(r#"{{"format":"deckcraft","version":1,"generator":"independent","presentation":{presentation}}}"#)
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in [("mimetype", MIMETYPE), ("presentation.json", manifest.as_str())] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn resource(id: u32, name: &str, label: Option<&str>) -> Arc<PresentationTemplate> {
    Arc::new(PresentationTemplate {
        id: PresentationTemplateId(id),
        name: name.into(),
        display_name: label.map(str::to_owned),
        slots: vec![
            PresentationTemplateSlot {
                id: PresentationTemplateSlotId(901),
                role: PresentationRole::Outline,
                bounds: PresentationTemplateBox::new(-5.25, 8., 80., 35.).unwrap(),
            },
            PresentationTemplateSlot {
                id: PresentationTemplateSlotId(902),
                role: PresentationRole::Text,
                bounds: PresentationTemplateBox::new(9., 10., 20., 30.).unwrap(),
            },
        ],
    })
}

fn fixture() -> Presentation {
    let mut p = Presentation {
        presentation_templates: vec![
            resource(700, "é", Some("")),
            resource(701, "e\u{301}", Some("漢 & < > \" \t\r\nب")),
            resource(702, "unused", None),
        ],
        ..Default::default()
    };
    let page = p.slides[0].as_ref().clone();
    p.slides = [700, 700, 701]
        .into_iter()
        .enumerate()
        .map(|(i, template)| {
            Arc::new(Slide { id: SlideId(100 + i as u32), presentation_template: Some(PresentationTemplateId(template)), ..page.clone() })
        })
        .collect();
    p
}

fn sequence(value: &Value, fields: &[&str]) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|field| {
                value.get(*field).cloned().unwrap_or_else(|| {
                    if matches!(*field, "notesMasters" | "tableStyles" | "tags" | "keptParts" | "guides" | "presentationTemplates") {
                        json!([])
                    } else {
                        Value::Null
                    }
                })
            })
            .collect(),
    )
}

const PRESENTATION_FIELDS: &[&str] = &[
    "slideSize",
    "notesSize",
    "slides",
    "masters",
    "notesMaster",
    "notesMasters",
    "handoutMaster",
    "sections",
    "customShows",
    "show",
    "headerFooter",
    "props",
    "defaultTextStyle",
    "media",
    "firstSlideNumber",
    "nextId",
    "embeddedFonts",
    "tableStyles",
    "tags",
    "keptParts",
    "guides",
    "rawExt",
    "presentationTemplates",
];
const SLIDE_FIELDS: &[&str] = &[
    "id",
    "layout",
    "name",
    "shapes",
    "background",
    "hidden",
    "showMasterShapes",
    "transition",
    "animations",
    "notes",
    "notesPage",
    "comments",
    "colorMap",
    "tags",
    "rawExt",
    "presentationTemplate",
];

#[test]
fn native_roundtrip_retains_exact_unused_owners_labels_order_and_selections() {
    let mut p = fixture();
    let tokens = [
        "title",
        "outline",
        "subtitle",
        "text",
        "graphic",
        "object",
        "chart",
        "table",
        "orgchart",
        "page",
        "notes",
        "handout",
        "header",
        "footer",
        "date-time",
        "page-number",
    ];
    Arc::make_mut(&mut p.presentation_templates[2]).slots = tokens
        .into_iter()
        .enumerate()
        .map(|(i, role)| PresentationTemplateSlot {
            id: PresentationTemplateSlotId(1000 + i as u32),
            role: PresentationRole::from_odf(role).unwrap(),
            bounds: PresentationTemplateBox::new(i as f64, -1., 10., 20.).unwrap(),
        })
        .collect();
    let before = p.clone();
    let (back, diagnostics) = load_checked(&save(&p).unwrap()).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(back.presentation_templates, p.presentation_templates);
    assert_eq!(back.slides, p.slides);
    assert_eq!(back.masters, p.masters);
    assert_eq!(p, before);
    assert!(Arc::ptr_eq(&p.presentation_templates[0], &before.presentation_templates[0]));
    assert_eq!(back.next_id, 1015, "unused slot identity must participate in load repair");
    assert_eq!(back.presentation_templates[0].display_name.as_deref(), Some(""));
    assert_eq!(back.presentation_templates[2].display_name, None);
}

#[test]
fn old_maps_and_positional_roots_slides_and_presentations_default_only_new_fields() {
    let mut old = serde_json::to_value(Presentation::default()).unwrap();
    old.as_object_mut().unwrap().remove("presentationTemplates");
    old["slides"][0].as_object_mut().unwrap().remove("presentationTemplate");
    old["slides"][0]["name"] = json!("historical slide");
    old["slides"][0]["hidden"] = json!(true);
    old["slides"][0]["tags"] = json!([["sentinel", "kept"]]);
    old["props"]["title"] = json!("historical title");
    old["rawExt"] = json!("historical raw extension");
    for positional_slide in [false, true] {
        let mut shape = old.clone();
        if positional_slide {
            shape["slides"][0] = sequence(&shape["slides"][0], &SLIDE_FIELDS[..SLIDE_FIELDS.len() - 1]);
        }
        for positional_presentation in [false, true] {
            let wire = if positional_presentation { sequence(&shape, &PRESENTATION_FIELDS[..PRESENTATION_FIELDS.len() - 1]) } else { shape.clone() };
            for positional_root in [false, true] {
                let back = load(&package(&wire.to_string(), positional_root)).unwrap();
                assert!(back.presentation_templates.is_empty());
                assert!(back.slides[0].presentation_template.is_none());
                assert_eq!(back.slides[0].name, "historical slide");
                assert!(back.slides[0].hidden);
                assert_eq!(back.slides[0].tags, [("sentinel".into(), "kept".into())]);
                assert_eq!(back.props.title, "historical title");
                assert_eq!(back.raw_ext.as_deref(), Some("historical raw extension"));
            }
        }
    }
}

#[test]
fn new_positional_owners_and_selection_preserve_exact_native_identity() {
    let p = fixture();
    let mut wire = serde_json::to_value(&p).unwrap();
    for owner in wire["presentationTemplates"].as_array_mut().unwrap() {
        for slot in owner["slots"].as_array_mut().unwrap() {
            slot["bounds"] = sequence(&slot["bounds"], &["x", "y", "w", "h"]);
            *slot = sequence(slot, &["id", "role", "bounds"]);
        }
        *owner = sequence(owner, &["id", "name", "displayName", "slots"]);
    }
    for slide in wire["slides"].as_array_mut().unwrap() {
        *slide = sequence(slide, SLIDE_FIELDS);
    }
    let wire = sequence(&wire, PRESENTATION_FIELDS);
    let back = load(&package(&wire.to_string(), true)).unwrap();
    assert_eq!(back.presentation_templates, p.presentation_templates);
    assert_eq!(back.slides, p.slides);
}

#[test]
fn template_null_coordinates_cannot_be_repaired_in_either_order_or_representation() {
    let legacy = r#""slides":[{"id":7,"shapes":[{"id":9,"xfrm":{"x":null,"y":0,"w":10,"h":10}}]}]"#;
    // This proves the legacy field really is repairable, rather than every case failing for an unrelated reason.
    let back = load(&package(&format!("{{{legacy}}}"), false)).unwrap();
    assert_eq!(back.slides[0].shapes[0].xfrm.as_ref().unwrap().x, 0.0);
    for positional_box in [false, true] {
        for index in 0..4 {
            let mut bounds = json!({"x":0,"y":0,"w":10,"h":10});
            if positional_box {
                bounds = json!([0, 0, 10, 10]);
                bounds[index] = Value::Null;
            } else {
                bounds[["x", "y", "w", "h"][index]] = Value::Null;
            }
            let owners = format!(r#""presentationTemplates":[{{"id":40,"name":"strict","slots":[{{"id":41,"role":"outline","bounds":{bounds}}}]}}]"#);
            for reversed in [false, true] {
                let wire = if reversed { format!("{{{owners},{legacy}}}") } else { format!("{{{legacy},{owners}}}") };
                for positional_root in [false, true] {
                    let error = load(&package(&wire, positional_root)).unwrap_err().to_string();
                    assert!(error.contains("bounded presentation-template coordinate"), "{error}");
                }
            }
            // A fully positional presentation must use the same strict box decoder.
            let mut wire = serde_json::to_value(fixture()).unwrap();
            wire["presentationTemplates"][0]["slots"][0]["bounds"] = bounds;
            assert!(load(&package(&sequence(&wire, PRESENTATION_FIELDS).to_string(), true)).is_err());
        }
    }
    // Valid template metadata does not accidentally disable legitimate historical repair.
    let owners = serde_json::to_string(&fixture().presentation_templates).unwrap();
    for wire in [format!(r#"{{{legacy},"presentationTemplates":{owners}}}"#), format!(r#"{{"presentationTemplates":{owners},{legacy}}}"#)] {
        let back = load(&package(&wire, false)).unwrap();
        assert_eq!(back.slides[0].shapes[0].xfrm.as_ref().unwrap().x, 0.0);
        assert_eq!(back.presentation_templates.len(), 3);
    }
}

#[test]
fn invalid_native_resources_and_selections_refuse_before_repair_or_save_sanitization() {
    for case in 0..7 {
        let mut p = fixture();
        match case {
            0 => Arc::make_mut(&mut p.presentation_templates[1]).id = PresentationTemplateId(700),
            1 => Arc::make_mut(&mut p.presentation_templates[1]).name = "é".into(),
            2 => Arc::make_mut(&mut p.presentation_templates[0]).slots[1].id = PresentationTemplateSlotId(901),
            3 => Arc::make_mut(&mut p.slides[0]).presentation_template = Some(PresentationTemplateId(999)),
            4 => Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds.w = 0.,
            5 => Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds.x = f64::NAN,
            _ => Arc::make_mut(&mut p.presentation_templates[0]).display_name = Some("bad\0label".into()),
        }
        let before = format!("{p:?}");
        let error = save(&p).unwrap_err().to_string();
        assert!(error.contains("presentation template"), "{error}");
        assert_eq!(format!("{p:?}"), before);
        let wire = serde_json::to_string(&p).unwrap();
        assert!(load(&package(&wire, false)).is_err(), "case {case}: raw invalid owners must not disappear during repair");
    }
    let mut wire = serde_json::to_value(fixture()).unwrap();
    wire["presentationTemplates"][0]["unexpected"] = json!(true);
    assert!(load(&package(&wire.to_string(), false)).is_err());
    wire["presentationTemplates"][0].as_object_mut().unwrap().remove("unexpected");
    wire["unrelatedFutureField"] = json!({"ordinary":"allowed"});
    assert!(load(&package(&wire.to_string(), false)).is_ok());
}

#[test]
fn native_repair_respects_exhausted_ids_from_unused_slots_and_templates() {
    for template_max in [false, true] {
        let mut p = fixture();
        p.next_id = 1;
        let unused = Arc::make_mut(&mut p.presentation_templates[2]);
        if template_max {
            unused.id = PresentationTemplateId(u32::MAX);
        } else {
            unused.slots[1].id = PresentationTemplateSlotId(u32::MAX);
        }
        let mut back = load(&save(&p).unwrap()).unwrap();
        assert_eq!(back.next_id, u32::MAX);
        let before = back.clone();
        assert_eq!(back.alloc_id(), None);
        assert_eq!(back, before);
        assert_eq!(back.presentation_templates, p.presentation_templates);
    }
}

#[test]
fn buffered_tagged_legacy_numeric_null_is_repairable_but_template_null_is_not() {
    let mut p = Presentation::default();
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape {
        id: ShapeId(90),
        kind: ShapeKind::Group { children: vec![], child: Xfrm { x: f64::NAN, w: 1., h: 1., ..Default::default() } },
        ..Default::default()
    }];
    let slides = serde_json::to_string(&p.slides).unwrap();
    let legacy = format!(r#""slides":{slides}"#);
    let control = load(&package(&format!("{{{legacy}}}"), false)).unwrap();
    let ShapeKind::Group { child, .. } = &control.slides[0].shapes[0].kind else { panic!("group") };
    assert_eq!(child.x, 0.0);
    let valid = r#""presentationTemplates":[{"id":40,"name":"strict","slots":[{"id":41,"role":"outline","bounds":{"x":0,"y":0,"w":10,"h":10}}]}]"#;
    let corrupt = valid.replace(r#""x":0"#, r#""x":null"#);
    for owners in [valid, corrupt.as_str()] {
        for reverse in [false, true] {
            let wire = if reverse { format!("{{{owners},{legacy}}}") } else { format!("{{{legacy},{owners}}}") };
            let result = load(&package(&wire, false));
            assert_eq!(result.is_ok(), owners == valid, "{result:?}");
            if let Err(error) = result {
                assert!(error.to_string().contains("bounded presentation-template coordinate"), "{error}");
            }
        }
    }
}
