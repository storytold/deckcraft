//! Independent contributor-original template-foundation tests; no external assets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{presentation_template::*, *};
use serde_json::{Value, json};
use std::sync::Arc;

fn slot(id: u32) -> PresentationTemplateSlot {
    PresentationTemplateSlot {
        id: PresentationTemplateSlotId(id),
        role: PresentationRole::Outline,
        bounds: PresentationTemplateBox::new(-4.5, 2.0, 80.0, 30.0).unwrap(),
    }
}

fn owner(id: u32, name: &str) -> Arc<PresentationTemplate> {
    Arc::new(PresentationTemplate {
        id: PresentationTemplateId(id),
        name: name.into(),
        display_name: Some(String::new()),
        slots: vec![slot(91), slot(92)],
    })
}

fn fixture() -> Presentation {
    let mut p = Presentation { presentation_templates: vec![owner(50, "é"), owner(51, "e\u{301}"), owner(52, "unused")], ..Default::default() };
    let mut page = p.slides[0].as_ref().clone();
    page.presentation_template = Some(PresentationTemplateId(50));
    p.slides = [50, 50, 51]
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let mut page = page.clone();
            page.id = SlideId(200 + i as u32);
            page.presentation_template = Some(PresentationTemplateId(id));
            Arc::new(page)
        })
        .collect();
    p
}

#[test]
fn all_roles_have_exact_distinct_odf_and_native_tokens() {
    let pairs = [
        (PresentationRole::Title, "title"),
        (PresentationRole::Outline, "outline"),
        (PresentationRole::Subtitle, "subtitle"),
        (PresentationRole::Text, "text"),
        (PresentationRole::Graphic, "graphic"),
        (PresentationRole::Object, "object"),
        (PresentationRole::Chart, "chart"),
        (PresentationRole::Table, "table"),
        (PresentationRole::OrgChart, "orgchart"),
        (PresentationRole::Page, "page"),
        (PresentationRole::Notes, "notes"),
        (PresentationRole::Handout, "handout"),
        (PresentationRole::Header, "header"),
        (PresentationRole::Footer, "footer"),
        (PresentationRole::DateTime, "date-time"),
        (PresentationRole::PageNumber, "page-number"),
    ];
    for (role, token) in pairs {
        assert_eq!(role.odf_token(), token);
        assert_eq!(PresentationRole::from_odf(token), Some(role));
        assert_eq!(serde_json::to_value(role).unwrap(), json!(token));
        assert_eq!(serde_json::from_value::<PresentationRole>(json!(token)).unwrap(), role);
    }
    for token in ["Body", "body", "subTitle", "orgChart", "dateTime", "pageNumber", "Title", " title", ""] {
        assert_eq!(PresentationRole::from_odf(token), None);
        assert!(serde_json::from_value::<PresentationRole>(json!(token)).is_err());
    }
}

#[test]
fn ncname_uses_unicode_xml_grammar_and_utf8_bytes() {
    for name in ["_", "A-0.x", "é", "漢", "ب", "e\u{301}", "a\u{b7}", "\u{10000}", "\u{effff}", "\u{200c}"] {
        assert!(valid_ncname(name), "{name:?}");
    }
    for name in [
        "",
        "a:b",
        "0a",
        ".a",
        "-a",
        " a",
        "a b",
        "\u{301}a",
        "\u{b7}a",
        "\u{d7}",
        "\u{f7}",
        "\u{37e}",
        "\u{2000}",
        "\u{3000}",
        "\u{fdd0}",
        "\u{fffe}",
        "\u{f0000}",
        "a\0",
    ] {
        assert!(!valid_ncname(name), "{name:?}");
    }
    assert!(valid_ncname(&"é".repeat(MAX_TEMPLATE_NAME_BYTES / 2)));
    assert!(!valid_ncname(&("é".repeat(MAX_TEMPLATE_NAME_BYTES / 2) + "x")));
}

#[test]
fn box_coordinates_are_required_strict_and_bounded_for_map_and_sequence() {
    assert_eq!(serde_json::from_value::<PresentationTemplateBox>(json!([-4.5, 2, 80, 30])).unwrap(), slot(1).bounds);
    let keys = ["x", "y", "w", "h"];
    for (index, key) in keys.into_iter().enumerate() {
        let mut missing = json!({"x":0,"y":0,"w":1,"h":1});
        missing.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<PresentationTemplateBox>(missing).is_err(), "missing {key}");
        for bad in [Value::Null, json!(true), json!("0"), json!([]), json!({}), json!(10_000_001), json!(-10_000_001)] {
            let mut map = json!({"x":0,"y":0,"w":1,"h":1});
            map[key] = bad.clone();
            let mut positional = json!([0, 0, 1, 1]);
            positional[index] = bad;
            for wire in [map, positional] {
                let error = serde_json::from_value::<PresentationTemplateBox>(wire).unwrap_err().to_string();
                assert!(!error.starts_with("invalid type: null, expected f"));
                assert!(!error.starts_with("invalid type: null, expected a number"));
            }
        }
    }
    for wire in ["[]", "[0,0,1]", "[0,0,1,1,2]", r#"{"x":0,"y":0,"w":1,"h":1,"rotation":0}"#, "[0,0,1e999,1]"] {
        assert!(serde_json::from_str::<PresentationTemplateBox>(wire).is_err());
    }
    for values in [
        [0., 0., 0., 1.],
        [0., 0., 1., -1.],
        [10_000_000., 0., 1., 1.],
        [0., 10_000_000., 1., 1.],
        [f64::NAN, 0., 1., 1.],
        [0., 0., f64::INFINITY, 1.],
    ] {
        assert!(PresentationTemplateBox::new(values[0], values[1], values[2], values[3]).is_err());
    }
    assert!(PresentationTemplateBox::new(-10_000_000., -10_000_000., 10_000_000., 10_000_000.).is_ok());
}

#[test]
fn new_owners_require_fields_and_refuse_unknown_fields() {
    let valid = serde_json::to_value(owner(50, "valid")).unwrap();
    for field in ["id", "name", "slots"] {
        let mut bad = valid.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<PresentationTemplate>(bad).is_err());
    }
    for where_ in [0, 1, 2] {
        let mut bad = valid.clone();
        let object = match where_ {
            0 => &mut bad,
            1 => &mut bad["slots"][0],
            _ => &mut bad["slots"][0]["bounds"],
        };
        object["unrepresentedOwner"] = json!(true);
        assert!(serde_json::from_value::<PresentationTemplate>(bad).is_err());
    }
    for field in ["id", "role", "bounds"] {
        let mut bad = valid["slots"][0].clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<PresentationTemplateSlot>(bad).is_err());
    }
}

#[test]
fn owners_are_exact_and_unused_resources_contribute_ids_without_shape_changes() {
    let p = fixture();
    let before = p.clone();
    let stats = validate(&p).unwrap();
    assert_eq!((stats.template_count, stats.slot_count, stats.max_id), (3, 6, 92));
    assert!(p.validate().is_empty(), "{:?}", p.validate());
    assert_eq!(p, before);
    assert_eq!(p.presentation_templates[0].slots, p.presentation_templates[1].slots);
    assert_ne!(p.presentation_templates[0].id, p.presentation_templates[1].id);
    let mut reordered = p.clone();
    reordered.presentation_templates.reverse();
    assert!(validate(&reordered).is_ok());
    assert_eq!(p.slides, reordered.slides);
    assert_eq!(p.masters, reordered.masters);
}

#[test]
fn invalid_identity_labels_and_geometry_fail_without_mutation() {
    for case in 0..9 {
        let mut p = fixture();
        match case {
            0 => Arc::make_mut(&mut p.presentation_templates[1]).id = PresentationTemplateId(50),
            1 => Arc::make_mut(&mut p.presentation_templates[1]).name = "é".into(),
            2 => Arc::make_mut(&mut p.presentation_templates[0]).slots[1].id = PresentationTemplateSlotId(91),
            3 => Arc::make_mut(&mut p.slides[0]).presentation_template = Some(PresentationTemplateId(999)),
            4 => Arc::make_mut(&mut p.presentation_templates[0]).name = "bad:name".into(),
            5 => Arc::make_mut(&mut p.presentation_templates[0]).display_name = Some("bad\0label".into()),
            6 => Arc::make_mut(&mut p.presentation_templates[0]).display_name = Some("x".repeat(MAX_TEMPLATE_NAME_BYTES + 1)),
            7 => Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds.w = 0.0,
            _ => Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds.x = MAX_TEMPLATE_COORDINATE,
        }
        let before = p.clone();
        assert!(validate(&p).is_err(), "case {case}");
        assert!(p.validate().iter().any(|error| error.contains("presentation template")));
        assert_eq!(p, before);
        assert!(Arc::ptr_eq(&p.presentation_templates[0], &before.presentation_templates[0]));
    }
}

#[test]
fn arc_snapshots_detach_only_the_edited_template_and_keep_slot_identity() {
    let original = fixture();
    let mut edited = original.clone();
    Arc::make_mut(&mut edited.presentation_templates[0]).slots.swap(0, 1);
    assert!(!Arc::ptr_eq(&original.presentation_templates[0], &edited.presentation_templates[0]));
    assert!(Arc::ptr_eq(&original.presentation_templates[1], &edited.presentation_templates[1]));
    assert_eq!(edited.presentation_templates[0].slots.iter().map(|slot| slot.id.0).collect::<Vec<_>>(), [92, 91]);
    assert_eq!(original.presentation_templates[0].slots.iter().map(|slot| slot.id.0).collect::<Vec<_>>(), [91, 92]);
    assert!(Arc::ptr_eq(&original.slides[0], &edited.slides[0]));
    assert_eq!(original.masters, edited.masters);
    assert!(validate(&edited).is_ok());
}

#[test]
fn unused_template_and_slot_ids_raise_high_water_and_exhaust_checked_allocator() {
    let mut p = fixture();
    p.next_id = 1;
    Arc::make_mut(&mut p.presentation_templates[2]).slots[1].id = PresentationTemplateSlotId(u32::MAX - 1);
    p.fix_next_id();
    assert_eq!(p.next_id, u32::MAX - 1);
    assert_eq!(p.alloc_id(), Some(u32::MAX));
    let before = p.clone();
    assert_eq!(p.alloc_id(), None);
    assert_eq!(p, before);
    Arc::make_mut(&mut p.presentation_templates[2]).id = PresentationTemplateId(u32::MAX);
    assert_eq!(validate(&p).unwrap().max_id, u32::MAX);
    assert!(validate(&p).is_ok(), "exhausted identity is valid existing state");
}

#[test]
fn structural_and_logical_byte_caps_apply_to_shared_and_unused_owners() {
    let mut p = Presentation {
        presentation_templates: (0..MAX_TEMPLATES)
            .map(|i| {
                Arc::new(PresentationTemplate { id: PresentationTemplateId(i as u32), name: format!("t{i}"), display_name: None, slots: vec![] })
            })
            .collect(),
        ..Default::default()
    };
    assert!(validate(&p).is_ok());
    p.presentation_templates.push(p.presentation_templates[0].clone());
    assert!(validate(&p).unwrap_err().to_string().contains("too many resources"));
    let mut resource = owner(50, "slots").as_ref().clone();
    resource.slots = (0..MAX_SLOTS_PER_TEMPLATE).map(|i| slot(i as u32)).collect();
    p.presentation_templates = vec![Arc::new(resource)];
    assert!(validate(&p).is_ok());
    Arc::make_mut(&mut p.presentation_templates[0]).slots.push(slot(999));
    assert!(validate(&p).unwrap_err().to_string().contains("too many slots in one"));
    let shared = Arc::new(PresentationTemplate {
        id: PresentationTemplateId(80),
        name: "x".repeat(MAX_TEMPLATE_NAME_BYTES),
        display_name: Some("y".repeat(MAX_TEMPLATE_NAME_BYTES)),
        slots: vec![],
    });
    p.presentation_templates = vec![shared.clone(); MAX_TEMPLATES];
    assert!(validate(&p).unwrap_err().to_string().contains("byte budget"));
    assert!(Arc::ptr_eq(&shared, &p.presentation_templates[0]));
}

#[test]
fn aggregate_slot_cap_is_independent_of_per_owner_cap() {
    let mut p = Presentation::default();
    let mut left = MAX_TOTAL_SLOTS;
    let mut id = 0u32;
    while left > 0 {
        let count = left.min(MAX_SLOTS_PER_TEMPLATE);
        p.presentation_templates.push(Arc::new(PresentationTemplate {
            id: PresentationTemplateId(id),
            name: format!("t{id}"),
            display_name: None,
            slots: (0..count).map(|i| slot(i as u32)).collect(),
        }));
        left -= count;
        id += 1;
    }
    assert_eq!(validate(&p).unwrap().slot_count, MAX_TOTAL_SLOTS);
    p.presentation_templates.push(Arc::new(PresentationTemplate {
        id: PresentationTemplateId(id),
        name: "last".into(),
        display_name: None,
        slots: vec![slot(1)],
    }));
    assert!(validate(&p).unwrap_err().to_string().contains("too many template slots"));
}

#[test]
fn scan_work_is_accounted_for_each_logical_owner_and_selected_reference() {
    let mut p = fixture();
    let selected = validate(&p).unwrap();
    for slide in &mut p.slides {
        Arc::make_mut(slide).presentation_template = None;
    }
    let unselected = validate(&p).unwrap();
    assert_eq!(selected.scan_work - unselected.scan_work, 3 * 16);
    assert!(selected.scan_work <= MAX_TEMPLATE_SCAN_WORK);
    assert!(selected.logical_bytes <= MAX_TEMPLATE_BYTES);
    assert_eq!(MAX_TEMPLATE_SLIDE_SCAN, 2_000_000);
}

#[test]
fn slide_and_work_caps_fail_at_their_own_boundaries_with_shared_arc_payloads() {
    let mut p = Presentation::default();
    let page = Arc::new(Slide::default());
    // One underlying Slide allocation, including the one-over count control.
    p.slides = vec![page.clone(); MAX_TEMPLATE_SLIDE_SCAN + 1];
    assert!(validate(&p).unwrap_err().to_string().contains("too many resources or page references"));
    p.slides.pop();
    p.presentation_templates = (1..=2050)
        .map(|id| {
            Arc::new(PresentationTemplate {
                id: PresentationTemplateId(id),
                name: format!("{}{:04}", "a".repeat(MAX_TEMPLATE_NAME_BYTES - 4), id),
                display_name: None,
                slots: vec![],
            })
        })
        .collect();
    // The full names/index/selection-byte profile passes; only selected lookup work differs below.
    let control = validate(&p).unwrap();
    assert!(control.logical_bytes < MAX_TEMPLATE_BYTES);
    assert!(control.scan_work < MAX_TEMPLATE_SCAN_WORK);
    let selected = Arc::new(Slide { presentation_template: Some(PresentationTemplateId(1)), ..Default::default() });
    p.slides.fill(selected.clone());
    let error = validate(&p).unwrap_err().to_string();
    assert!(error.contains("resource scan budget exceeded"), "earlier structural/byte failure cannot stand in for work coverage: {error}");
    assert!(Arc::ptr_eq(&p.slides[0], &selected));
    assert!(Arc::ptr_eq(p.slides.last().unwrap(), &selected));
    assert_eq!(page.presentation_template, None);
}

#[test]
fn direct_nonfinite_serde_numbers_cannot_enter_a_template_box() {
    use serde::Deserialize;
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let decoder =
            serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new([("x", bad), ("y", 0.0), ("w", 1.0), ("h", 1.0)].into_iter());
        assert!(PresentationTemplateBox::deserialize(decoder).is_err());
    }
}
