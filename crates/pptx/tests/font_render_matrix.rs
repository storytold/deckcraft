//! Tests use only fonts already licensed and bundled by craft-fonts; no font assets are stored.
use deckcraft_engine::render::{RenderOpts, render_slide};
use deckcraft_model::text::{Bullet, RunProps, TextBody};
use deckcraft_model::{EmbeddedFont, LayoutType, Presentation, Shape, ShapeId, Xfrm, defaults};
use std::sync::Arc;

fn probe() -> Presentation {
    let mut p = Presentation::default();
    p.slides.clear();
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let mut slide = defaults::new_slide(&mut p, layout).unwrap();
    for (i, (face, text, lang)) in [
        ("Inter", "OpenType: office ffi / 123 / italic", "en-US"),
        ("BIZ UDPGothic", "文化 日本 中文 / 123", "ja-JP"),
        ("Shippori Mincho", "文化 日本 中文 / 456", "zh-TW"),
        ("Noto Sans Arabic", "مرحبا بالعالم 123", "ar-SA"),
        ("Noto Sans Thai", "ภาษาไทย 123", "th-TH"),
        ("Noto Sans Devanagari", "नमस्ते दुनिया 123", "hi-IN"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut body = TextBody::from_text(text);
        body.body.inset_l = Some(0.0);
        body.body.inset_t = Some(0.0);
        let para = &mut body.paragraphs[0];
        para.props.bullet = Some(Bullet::None);
        para.props.rtl = Some(lang == "ar-SA");
        para.runs[0].props = RunProps {
            font: Some(face.into()),
            font_ea: Some(face.into()),
            font_cs: Some(face.into()),
            size: Some(30.0),
            lang: Some(lang.into()),
            italic: Some(i == 0),
            ..Default::default()
        };
        slide.shapes.push(Shape {
            id: ShapeId(8000 + i as u32),
            xfrm: Some(Xfrm::new(60.0, 30.0 + i as f64 * 80.0, 840.0, 70.0)),
            text: Some(body),
            ..Default::default()
        });
    }
    p.slides.push(Arc::new(slide));
    p
}

fn eot(raw: &[u8]) -> Vec<u8> {
    // Original uncompressed EOT wrapper, defined by the public W3C submission.
    let mut b = vec![0u8; 82];
    b[0..4].copy_from_slice(&((82 + raw.len()) as u32).to_le_bytes());
    b[4..8].copy_from_slice(&(raw.len() as u32).to_le_bytes());
    b[8..12].copy_from_slice(&0x0002_0001u32.to_le_bytes());
    b[34..36].copy_from_slice(&0x504cu16.to_le_bytes());
    b.extend_from_slice(raw);
    b
}

#[test]
fn multilingual_shaping_and_embedded_faces_survive_pptx_and_reach_renderer() {
    let mut p = probe();
    let regular = render_slide(&p, 0, &RenderOpts::default());
    for family in ["Inter", "BIZ UDPGothic", "Shippori Mincho", "Noto Sans Arabic", "Noto Sans Thai", "Noto Sans Devanagari"] {
        let Some(font) = deckcraft_fonts::CRAFT_FONTS.iter().find(|f| f.family == family && f.style == "Regular") else { continue };
        let data = if family == "BIZ UDPGothic" { eot(font.bytes) } else { font.bytes.to_vec() };
        let id = p.add_media("probe-font.fntdata", "application/x-fontdata", data).unwrap();
        let italic = deckcraft_fonts::CRAFT_FONTS
            .iter()
            .find(|f| f.family == family && f.style == "Italic")
            .map(|f| p.add_media("probe-italic.fntdata", "application/x-fontdata", f.bytes.to_vec()).unwrap());
        p.embedded_fonts.push(EmbeddedFont { typeface: family.into(), regular: Some(id), italic, ..Default::default() });
    }
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let img = render_slide(&back, 0, &RenderOpts::default());
    assert_eq!(regular.pixels, img.pixels, "embedding/round-trip changed shaping or metrics");
    let info = deckcraft_engine::fonts::inspect_fonts(&back);
    assert!(info.as_array().unwrap().iter().all(|f| f["missingGlyph"] == false));
    for family in &back.embedded_fonts {
        assert!(
            info.as_array()
                .unwrap()
                .iter()
                .any(|f| f["requested"].as_array().is_some_and(|names| names.contains(&serde_json::json!(family.typeface))) && f["embedded"] == true),
            "{}",
            family.typeface
        );
    }
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("multilingual-fonts.pptx"), bytes).unwrap();
        std::fs::write(dir.join("multilingual-fonts.png"), img.to_png()).unwrap();
        std::fs::write(dir.join("multilingual-font-report.json"), serde_json::to_vec_pretty(&info).unwrap()).unwrap();
    }
}

#[test]
fn unavailable_font_is_reported_instead_of_silently_claiming_fidelity() {
    let mut p = probe();
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].props.font = Some("Synthetic Missing Family 9817".into());
    assert!(deckcraft_engine::fonts::missing_fonts(&p).contains(&"Synthetic Missing Family 9817".into()));
    let report = deckcraft_engine::fonts::inspect_fonts(&p);
    assert!(
        report.as_array().unwrap().iter().any(|row| row["requested"][0] == "Synthetic Missing Family 9817" && row["actual"] != row["requested"][0])
    );
}

#[test]
#[ignore = "requires MTX_ORACLE_SOURCE and MTX_ORACLE_INPUT: local fonts, never repository assets"]
fn microtype_embedded_font_matches_uncompressed_render_and_roundtrip() {
    let source = std::fs::read(std::env::var("MTX_ORACLE_SOURCE").unwrap()).unwrap();
    let compressed = std::fs::read(std::env::var("MTX_ORACLE_INPUT").unwrap()).unwrap();
    let mut base = probe();
    let slide = Arc::make_mut(&mut base.slides[0]);
    slide.shapes.truncate(1);
    let run = &mut slide.shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0];
    run.text = "MTX: office ffi ABC xyz 0123456789 Áéö / مرحبا بالعالم / 日本語".into();
    run.props.font = Some("Local MTX Probe".into());
    run.props.font_ea = Some("Local MTX Probe".into());
    run.props.font_cs = Some("Local MTX Probe".into());
    run.props.italic = Some(false);
    let with_font = |font: Vec<u8>| {
        let mut p = base.clone();
        let id = p.add_media("original-font.fntdata", "application/x-fontdata", font).unwrap();
        p.embedded_fonts.push(EmbeddedFont { typeface: "Local MTX Probe".into(), regular: Some(id), ..Default::default() });
        p
    };
    let expected = render_slide(&with_font(source), 0, &RenderOpts::default());
    for bytes in [compressed.clone(), eot(&compressed)] {
        let p = with_font(bytes);
        let exported = deckcraft_pptx::export(&p).unwrap();
        let back = deckcraft_pptx::import(&exported).unwrap();
        let actual = render_slide(&back, 0, &RenderOpts::default());
        assert_eq!(expected.pixels, actual.pixels, "compressed embedding changed final slide pixels");
        let report = deckcraft_engine::fonts::inspect_fonts(&back);
        assert!(report.as_array().unwrap().iter().any(|row| row["embedded"] == true));
    }
}
