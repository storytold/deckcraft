//! Independent direct-library guards for independent template resources.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::*;
use std::sync::Arc;

fn fixture() -> Presentation {
    Presentation {
        presentation_templates: vec![Arc::new(PresentationTemplate {
            id: PresentationTemplateId(700),
            name: "unused_template".into(),
            display_name: Some("independent".into()),
            slots: vec![PresentationTemplateSlot {
                id: PresentationTemplateSlotId(701),
                role: PresentationRole::Text,
                bounds: PresentationTemplateBox::new(-1., 2., 30., 40.).unwrap(),
            }],
        })],
        ..Default::default()
    }
}

#[test]
fn every_core_pptx_entry_refuses_valid_unused_template_resources_without_mutation() {
    let p = fixture();
    let before = p.clone();
    assert!(p.slides.iter().all(|slide| slide.presentation_template.is_none()));
    for result in [
        deckcraft_pptx::export(&p),
        deckcraft_pptx::export_as(&p, "pptx"),
        deckcraft_pptx::export_as(&p, "potx"),
        deckcraft_pptx::export_as(&p, "ppsx"),
    ] {
        let error = result.unwrap_err().to_string();
        assert!(error.contains("independent presentation template resources have no PPTX mapping"), "{error}");
    }
    assert_eq!(p, before);
    assert!(Arc::ptr_eq(&p.presentation_templates[0], &before.presentation_templates[0]));
}

#[test]
fn invalid_template_metadata_is_rejected_before_pptx_mapping_or_numeric_sanitization() {
    for case in 0..4 {
        let mut p = fixture();
        match case {
            0 => Arc::make_mut(&mut p.slides[0]).presentation_template = Some(PresentationTemplateId(999)),
            1 => Arc::make_mut(&mut p.presentation_templates[0]).name = "bad:name".into(),
            2 => Arc::make_mut(&mut p.presentation_templates[0]).slots[0].bounds.x = f64::NAN,
            _ => {
                p.presentation_templates.clear();
                Arc::make_mut(&mut p.slides[0]).presentation_template = Some(PresentationTemplateId(700));
            }
        }
        let before = format!("{p:?}");
        for result in [
            deckcraft_pptx::export(&p),
            deckcraft_pptx::export_as(&p, "pptx"),
            deckcraft_pptx::export_as(&p, "potx"),
            deckcraft_pptx::export_as(&p, "ppsx"),
        ] {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("presentation template:"), "{error}");
            assert!(!error.contains("no PPTX mapping"), "structural validation must happen before capability refusal");
        }
        assert_eq!(format!("{p:?}"), before);
    }
}

#[test]
fn historical_resource_empty_documents_still_export_all_pptx_package_kinds() {
    let p = Presentation::default();
    assert!(p.presentation_templates.is_empty());
    assert!(!deckcraft_pptx::export(&p).unwrap().is_empty());
    for ext in ["pptx", "potx", "ppsx"] {
        let back = deckcraft_pptx::import(&deckcraft_pptx::export_as(&p, ext).unwrap()).unwrap();
        assert!(back.presentation_templates.is_empty());
        assert!(back.slides.iter().all(|slide| slide.presentation_template.is_none()));
        assert_eq!(back.slides.len(), p.slides.len());
    }
}
