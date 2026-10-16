//! Contributor-original script typography. Semantics: ODF 1.3 Part 3 §§20.284–285,
//! 20.289–290, 20.294–295, 20.302–303, and 20.256–257; no vendor documents/assets.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    text::{Paragraph, Run, RunProps, TextBody},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";
fn package(styles: &str, text: &str) -> Vec<u8> {
    package_frame(styles, text, "")
}
fn package_frame(styles: &str, text: &str, frame_attrs: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page><draw:frame svg:width='600pt' svg:height='150pt' {frame_attrs}><draw:text-box><text:p text:style-name='P'>{text}</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!("<office:document-styles {NS} office:version='1.3'><office:styles>{styles}</office:styles></office:document-styles>");
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("styles.xml", styles.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(value.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn paragraph_style(attrs: &str) -> String {
    format!("<style:style style:name='P' style:family='paragraph'><style:text-properties {attrs}/></style:style>")
}
fn props(p: &Presentation) -> &RunProps {
    &p.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].runs[0].props
}
fn native(props: RunProps) -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(720.0, 405.0), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let body = TextBody { paragraphs: vec![Paragraph { runs: vec![Run::with("A漢 ب12 Z", props)], ..Default::default() }], ..Default::default() };
    p.slides.push(Arc::new(Slide {
        id: SlideId(100),
        layout,
        show_master_shapes: false,
        shapes: vec![Shape {
            id: ShapeId(200),
            text: Some(body),
            text_box: true,
            xfrm: Some(Xfrm::new(20.0, 20.0, 600.0, 150.0)),
            ..Default::default()
        }],
        ..Default::default()
    }));
    p
}
fn part(bytes: &[u8], path: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    z.by_name(path).unwrap().read_to_string(&mut out).unwrap();
    out
}
fn script_values(r: &RunProps) -> (Option<f64>, Option<f64>, Option<bool>, Option<bool>, Option<bool>, Option<bool>, Option<&str>, Option<&str>) {
    (r.size_ea, r.size_cs, r.bold_ea, r.bold_cs, r.italic_ea, r.italic_cs, r.lang_ea.as_deref(), r.lang_cs.as_deref())
}
#[test]
fn script_attributes_are_native_and_reopen_with_independent_explicit_normal_and_clear() {
    let r = RunProps {
        size: Some(18.0),
        bold: Some(true),
        italic: Some(true),
        lang: Some("en-US".into()),
        size_ea: Some(30.0),
        size_cs: Some(24.0),
        bold_ea: Some(false),
        bold_cs: Some(true),
        italic_ea: Some(true),
        italic_cs: Some(false),
        lang_ea: Some(String::new()),
        lang_cs: Some("ar-SA".into()),
        ..Default::default()
    };
    let p = native(r.clone());
    let bytes = export(&p).unwrap();
    let xml = part(&bytes, "content.xml");
    for expected in [
        "style:font-size-asian=\"30pt\"",
        "style:font-size-complex=\"24pt\"",
        "style:font-weight-asian=\"normal\"",
        "style:font-weight-complex=\"bold\"",
        "style:font-style-asian=\"italic\"",
        "style:font-style-complex=\"normal\"",
        "style:language-asian=\"none\"",
        "style:country-asian=\"none\"",
        "style:language-complex=\"ar\"",
        "style:country-complex=\"SA\"",
    ] {
        assert!(xml.contains(expected), "missing {expected}: {xml}");
    }
    let back = import(&bytes).unwrap();
    assert_eq!(script_values(props(&back)), script_values(&r));
    assert!(!back.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", back.tags);
    export(&back).unwrap();
    let placed = deckcraft_render::place_slide(&back, 0);
    let l = &placed.texts[0].layout;
    for (ch, size) in [(0, 18.0), (1, 30.0), (3, 24.0), (4, 24.0), (7, 18.0)] {
        assert_eq!(l.runs.iter().find(|r| r.chars.0 <= ch && ch < r.chars.1).unwrap().size, size);
    }
    if let Ok(dir) = std::env::var("DECKCRAFT_SCRIPT_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/content.xml"), xml).unwrap();
        std::fs::write(format!("{dir}/styles.xml"), part(&bytes, "styles.xml")).unwrap();
        std::fs::write(format!("{dir}/script-styles.odp"), bytes).unwrap();
    }
}
#[test]
fn common_style_percentages_use_script_parent_not_sibling_latin_size() {
    let styles = format!(
        "{}<style:style style:name='Child' style:family='text'><style:text-properties fo:font-size='99pt' style:font-size-asian='150%' style:font-size-complex='50%' style:font-weight-asian='normal' style:font-style-complex='normal' style:country-asian='CN' style:language-complex='none' style:country-complex='none'/></style:style>",
        paragraph_style(
            "fo:font-size='20pt' style:font-size-asian='24pt' style:font-size-complex='30pt' style:font-weight-asian='bold' style:font-style-complex='italic' style:language-asian='ja' style:country-asian='JP' style:language-complex='ar' style:country-complex='SA'"
        )
    );
    let p = import(&package(&styles, "<text:span text:style-name='Child'>A漢ب</text:span>")).unwrap();
    let r = props(&p);
    assert_eq!(r.size, Some(99.0));
    assert_eq!(script_values(r), (Some(36.0), Some(15.0), Some(false), None, None, Some(false), Some("ja-CN"), Some("")));
    let back = import(&export(&p).unwrap()).unwrap();
    assert_eq!(script_values(props(&back)), script_values(r));
}
#[test]
fn script_percentage_falls_back_to_parent_ordinary_size_and_country_only_uses_parent_language() {
    let styles = format!(
        "{}<style:style style:name='Child' style:family='text'><style:text-properties style:font-size-asian='150%' style:font-size-complex='50%' style:country-complex='EG'/></style:style>",
        paragraph_style("fo:font-size='20pt' fo:language='ar' fo:country='SA'")
    );
    let p = import(&package(&styles, "<text:span text:style-name='Child'>漢ب</text:span>")).unwrap();
    let r = props(&p);
    assert_eq!((r.size_ea, r.size_cs), (Some(30.0), Some(10.0)));
    assert_eq!(r.lang_cs.as_deref(), Some("ar-EG"));
}
#[test]
fn script_paragraph_marker_properties_survive_an_empty_paragraph() {
    let mut p = native(RunProps::default());
    let sh = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.runs.clear();
    para.end_props = RunProps { size_ea: Some(28.0), bold_ea: Some(false), lang_cs: Some(String::new()), ..Default::default() };
    let back = import(&export(&p).unwrap()).unwrap();
    let para = &back.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0];
    let c = resolve::Ctx::for_slide(&back, &back.slides[0]).unwrap();
    let effective = resolve::run(&c, &back.slides[0].shapes[0], para, &para.end_props);
    assert_eq!(effective.size_ea, Some(28.0));
    assert_eq!(effective.bold_ea, Some(false));
    assert_eq!(effective.lang_cs.as_deref(), Some(""));
}
#[test]
fn invalid_script_sizes_and_unresolved_percentages_are_refused_without_clamping() {
    for value in [0.0, 0.5, -2.0, f64::NAN, f64::INFINITY, 4000.5, 10001.0] {
        let p = native(RunProps { size_ea: Some(value), ..Default::default() });
        assert!(export(&p).is_err(), "{value}");
    }
    for value in ["0pt", "0.5pt", "-2pt", "NaNpt", "infpt", "4000.5pt", "10001pt", "0%", "-20%", "99999999999999999999999%"] {
        assert!(import(&package(&paragraph_style(&format!("fo:font-size='20pt' style:font-size-asian='{value}'")), "漢")).is_err(), "{value}");
    }
    assert!(import(&package(&paragraph_style("style:font-size-asian='150%'"), "漢")).is_err());
}
#[test]
fn richer_unrepresented_script_weights_and_styles_stay_guarded() {
    for attr in
        ["style:font-weight-asian='300'", "style:font-weight-complex='600'", "style:font-style-asian='oblique'", "style:font-size-rel-complex='2pt'"]
    {
        let p = import(&package(&paragraph_style(attr), "漢ب")).unwrap();
        assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "{attr}");
        assert!(export(&p).is_err(), "{attr}");
    }
    assert!(export(&native(RunProps { lang_ea: Some("zh-Hant-TW".into()), ..Default::default() })).is_err());
}

#[test]
fn graphic_script_percentages_resolve_style_owners_before_paragraph_fallback() {
    let styles = "<style:style style:name='Base' style:family='graphic'><style:text-properties fo:font-size='20pt' style:font-size-asian='24pt' style:font-size-complex='30pt' style:language-asian='ja' style:country-asian='JP'/></style:style><style:style style:name='Child' style:family='graphic' style:parent-style-name='Base'><style:text-properties fo:font-size='99pt' style:font-size-asian='150%' style:font-size-complex='50%' style:country-asian='CN'/></style:style>";
    let p = import(&package_frame(styles, "漢ب", "draw:style-name='Child'")).unwrap();
    assert_eq!((props(&p).size_ea, props(&p).size_cs, props(&p).lang_ea.as_deref()), (Some(36.0), Some(15.0), Some("ja-CN")));
    let back = import(&export(&p).unwrap()).unwrap();
    assert_eq!((props(&back).size_ea, props(&back).size_cs, props(&back).lang_ea.as_deref()), (Some(36.0), Some(15.0), Some("ja-CN")));
}
