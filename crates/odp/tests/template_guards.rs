//! Independent core template mapping and guarded-preservation boundaries.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::*;
use std::sync::Arc;

fn resource() -> Arc<PresentationTemplate> {
    Arc::new(PresentationTemplate {
        id: PresentationTemplateId(700),
        name: "unused_template".into(),
        display_name: None,
        slots: vec![PresentationTemplateSlot {
            id: PresentationTemplateSlotId(701),
            role: PresentationRole::Outline,
            bounds: PresentationTemplateBox::new(-1., 2., 30., 40.).unwrap(),
        }],
    })
}

#[test]
fn core_odp_maps_unused_resources_but_never_bypasses_guarded_preservation() {
    let source = deckcraft_odp::export(&Presentation::default()).unwrap();
    let mut p = deckcraft_odp::import(&source).unwrap();
    p.presentation_templates = vec![resource()];
    let before = p.clone();
    let bytes = deckcraft_odp::export(&p).unwrap();
    let back = deckcraft_odp::import(&bytes).unwrap();
    assert_eq!(back.presentation_templates.len(), 1);
    assert_eq!(back.presentation_templates[0].name, "unused_template");
    assert!(back.slides.iter().all(|slide| slide.presentation_template.is_none()));
    assert!(!back.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", back.tags);
    assert_eq!(p, before);
    assert!(Arc::ptr_eq(&p.presentation_templates[0], &before.presentation_templates[0]));
    p.tags.push(("odp:warning".into(), "independent source-loss warning".into()));
    let before = p.clone();
    let error = deckcraft_odp::export(&p).unwrap_err().to_string();
    assert!(error.contains("import retained unsupported source content"), "{error}");
    assert_eq!(p, before);
}

#[test]
fn core_odp_validates_refs_and_numeric_metadata_before_mapping_and_source_preservation() {
    for warning in [false, true] {
        for case in 0..5 {
            let mut p = Presentation { presentation_templates: vec![resource()], ..Default::default() };
            if warning {
                p.tags.push(("odp:warning".into(), "independent source-loss warning".into()));
            }
            match case {
                0 => Arc::make_mut(&mut p.slides[0]).presentation_template = Some(PresentationTemplateId(999)),
                1 => Arc::make_mut(&mut p.presentation_templates[0]).name = "bad:name".into(),
                2 => Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds.w = 0.,
                3 => Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds.x = f64::NAN,
                _ => {
                    p.presentation_templates.clear();
                    Arc::make_mut(&mut p.slides[0]).presentation_template = Some(PresentationTemplateId(700));
                }
            }
            let before = format!("{p:?}");
            let error = deckcraft_odp::export(&p).unwrap_err().to_string();
            assert!(error.contains("presentation template:"), "{error}");
            assert!(!error.contains("additional ODP mapping"));
            assert!(!error.contains("import retained unsupported source content"));
            assert_eq!(format!("{p:?}"), before);
        }
    }
}

#[test]
fn historical_resource_empty_documents_keep_the_plain_odp_export_lane() {
    let p = Presentation::default();
    let bytes = deckcraft_odp::export(&p).unwrap();
    let back = deckcraft_odp::import(&bytes).unwrap();
    assert!(back.presentation_templates.is_empty());
    assert!(back.slides.iter().all(|slide| slide.presentation_template.is_none()));
    assert_eq!(back.slides.len(), p.slides.len());
    assert!(!back.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", back.tags);
    assert!(deckcraft_odp::export(&back).is_ok());
}
