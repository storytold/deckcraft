//! Independent mixed-script inheritance and rendering checks; original synthetic content.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{
    text::{AutoFit, Paragraph, Run, RunProps, TextDir},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};

fn package(styles: &str, text: &str) -> Vec<u8> {
    const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";
    let content = format!(
        "<office:document-content {NS}><office:body><office:presentation><draw:page><draw:frame svg:width='600pt' svg:height='200pt'><draw:text-box><text:p text:style-name='P'>{text}</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!("<office:document-styles {NS}><office:styles>{styles}</office:styles></office:document-styles>");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("styles.xml", styles.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn paragraph(p: &Presentation) -> &Paragraph {
    &p.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0]
}
fn deck(mut runs: Vec<Run>, vertical: bool) -> Presentation {
    for run in &mut runs {
        run.props.font = Some("BIZ UDPGothic".into());
        run.props.font_ea = Some("BIZ UDPGothic".into());
        run.props.font_cs = Some("Noto Sans Arabic".into());
    }
    let mut p = defaults::blank_presentation(Size::new(720., 300.), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    let mut body = TextBody { paragraphs: vec![Paragraph { runs, ..Default::default() }], ..Default::default() };
    body.body.autofit = Some(AutoFit::None);
    body.body.vert = Some(if vertical { TextDir::EaVertical } else { TextDir::Horizontal });
    p.slides.push(Arc::new(Slide {
        id: SlideId(932),
        layout,
        show_master_shapes: false,
        shapes: vec![Shape { id: ShapeId(999), text: Some(body), xfrm: Some(Xfrm::new(10., 10., 600., 250.)), text_box: true, ..Default::default() }],
        ..Default::default()
    }));
    p
}

#[test]
fn default_parent_nested_inline_percentages_and_explicit_false_clear_remain_independent() {
    let styles = "<style:default-style style:family='paragraph'><style:text-properties fo:font-size='20pt' fo:font-weight='bold' fo:font-style='italic' fo:language='en' fo:country='US' style:font-size-asian='24pt' style:font-size-complex='30pt' style:language-asian='ja' style:country-asian='JP' style:language-complex='ar' style:country-complex='SA'/></style:default-style>
        <style:style style:name='Base' style:family='paragraph'><style:text-properties style:font-size-asian='150%' style:font-weight-asian='normal'/></style:style>
        <style:style style:name='P' style:family='paragraph' style:parent-style-name='Base'><style:text-properties style:font-size-complex='50%'/></style:style>
        <style:style style:name='Outer' style:family='text'><style:text-properties fo:font-size='80pt' style:font-size-asian='50%' style:font-style-complex='normal' style:country-asian='CN'/></style:style>
        <style:style style:name='Inner' style:family='text'><style:text-properties style:font-size-complex='200%' style:language-asian='none' style:country-asian='none'/></style:style>";
    let p = import(&package(styles, "<text:span text:style-name='Outer'>漢<text:span text:style-name='Inner'>ب</text:span>字</text:span>")).unwrap();
    let para = paragraph(&p);
    assert_eq!(para.runs.len(), 3);
    for r in &para.runs {
        assert_eq!(
            (r.props.size, r.props.size_ea, r.props.bold, r.props.bold_ea, r.props.italic_cs),
            (Some(80.), Some(18.), Some(true), Some(false), Some(false))
        );
    }
    assert_eq!((para.runs[0].props.size_cs, para.runs[1].props.size_cs, para.runs[2].props.size_cs), (Some(15.), Some(30.), Some(15.)));
    assert_eq!(para.runs[0].props.lang_ea.as_deref(), Some("ja-CN"));
    assert_eq!(para.runs[1].props.lang_ea.as_deref(), Some(""));
    assert_eq!(para.runs[2].props.lang_ea.as_deref(), Some("ja-CN"));
    let back = import(&export(&p).unwrap()).unwrap();
    for (a, b) in para.runs.iter().zip(&paragraph(&back).runs) {
        let typography = |p: &RunProps| {
            (
                p.size,
                p.size_ea,
                p.size_cs,
                p.bold,
                p.bold_ea,
                p.bold_cs,
                p.italic,
                p.italic_ea,
                p.italic_cs,
                p.lang.clone(),
                p.lang_ea.clone(),
                p.lang_cs.clone(),
            )
        };
        assert_eq!(typography(&a.props), typography(&b.props));
    }
}

#[test]
fn script_graphemes_use_base_style_across_run_boundaries_and_keep_vertical_and_break_ranges() {
    for vertical in [false, true] {
        let props =
            RunProps { size: Some(16.), size_ea: Some(32.), size_cs: Some(24.), bold: Some(true), bold_cs: Some(false), ..Default::default() };
        let runs = vec![
            Run::with("漢", props.clone()),
            Run::with("\u{fe00}", RunProps { size_ea: Some(80.), ..Default::default() }),
            Run::with("ب", props.clone()),
            Run::with("\u{064e}", RunProps { size_cs: Some(90.), ..Default::default() }),
            Run { kind: text::RunKind::Break, ..Run::with("", props.clone()) },
            Run::with("A", props),
        ];
        let p = deck(runs, vertical);
        for candidate in [&p, &import(&export(&p).unwrap()).unwrap()] {
            let placed = deckcraft_render::place_slide(candidate, 0);
            let l = &placed.texts[0].layout;
            let at = |ch| l.runs.iter().find(|r| r.chars.0 <= ch && ch < r.chars.1).unwrap();
            assert_eq!(at(0).size, 32.);
            assert_eq!(at(2).size, 24.);
            assert_eq!(at(5).size, 16.);
            assert_eq!(at(0).face.family, "BIZ UDPGothic");
            assert_eq!(at(2).face.family, "Noto Sans Arabic");
            assert_eq!(at(5).face.family, "BIZ UDPGothic");
            assert!(!at(2).face.italic && !at(2).fake_italic);
            assert_eq!(at(0).upright, vertical);
            assert_eq!(l.lines.len(), 2);
            assert!(
                !l.lines.iter().any(|line| line
                    .boundary
                    .iter()
                    .enumerate()
                    .any(|(i, boundary)| *boundary && (line.start + i == 1 || line.start + i == 3))),
                "variation selector and combining mark stay in their base grapheme"
            );
        }
    }
}

#[test]
fn actual_east_asian_ink_scales_independently_and_roundtrip_retains_pixels() {
    let ordinary = RunProps { size: Some(16.), ..Default::default() };
    let small = deck(vec![Run::with("漢", ordinary.clone())], false);
    let large = deck(vec![Run::with("漢", RunProps { size_ea: Some(48.), ..ordinary })], false);
    let small_image = deckcraft_render::render_slide(&small, 0, &Default::default());
    let large_image = deckcraft_render::render_slide(&large, 0, &Default::default());
    let ink = |pixels: &[u8]| pixels.as_chunks::<4>().0.iter().filter(|p| p[0] < 200 && p[1] < 200 && p[2] < 200 && p[3] > 200).count();
    assert!(ink(&small_image.pixels) > 5);
    assert!(ink(&large_image.pixels) > ink(&small_image.pixels) * 4, "tripling script height must substantially increase actual painted glyph area");
    let back = import(&export(&large).unwrap()).unwrap();
    let reopened = deckcraft_render::render_slide(&back, 0, &Default::default());
    assert_eq!(large_image.pixels, reopened.pixels);
}

#[test]
fn script_font_size_bounds_prevent_native_reopen_clamping_including_relative_sizes() {
    for slot in ["asian", "complex"] {
        for value in ["0.5pt", "0.999pt", "4000.01pt", "4.99%", "20000.1%"] {
            let styles = format!(
                "<style:style style:name='P' style:family='paragraph'><style:text-properties fo:font-size='20pt'/></style:style><style:style style:name='S' style:family='text'><style:text-properties style:font-size-{slot}='{value}'/></style:style>"
            );
            let Err(error) = import(&package(&styles, "<text:span text:style-name='S'>漢ب</text:span>")) else {
                panic!("accepted script {slot} size {value} outside native range")
            };
            assert!(error.to_string().contains("script font size"), "unexpected refusal for {value}: {error}");
        }
        for value in [1., 4000.] {
            let mut props = RunProps::default();
            if slot == "asian" {
                props.size_ea = Some(value);
            } else {
                props.size_cs = Some(value);
            }
            let p = deck(vec![Run::with("漢ب", props)], false);
            let back = import(&export(&p).unwrap()).unwrap();
            let props = &paragraph(&back).runs[0].props;
            assert_eq!(if slot == "asian" { props.size_ea } else { props.size_cs }, Some(value));
        }
    }
}
