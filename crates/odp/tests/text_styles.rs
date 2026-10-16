#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Contributor-original text fixtures exercising editable data and actual text layout.
use deckcraft_model::{
    text::{Action, Anchor, AutoFit, Bullet, Caps, RunKind, Strike},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
fn document(styles: &str, shapes: &str) -> String {
    format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'><office:font-face-decls><style:font-face style:name='OpenFace' svg:font-family='Liberation Sans'/></office:font-face-decls><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation>{shapes}</office:presentation></office:body></office:document-content>"
    )
}
fn package(doc: &str) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", doc),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        z.start_file(name, SimpleFileOptions::default()).unwrap();
        z.write_all(data.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn frame(text: &str) -> String {
    format!("<draw:page><draw:frame svg:width='400pt' svg:height='100pt'><draw:text-box>{text}</draw:text-box></draw:frame></draw:page>")
}
#[test]
fn nested_character_styles_inherit_paragraph_and_immediate_parent() {
    let styles = "<style:default-style style:family='text'><style:text-properties fo:font-size='12pt' fo:font-weight='normal' fo:color='#000000'/></style:default-style><style:style style:name='p' style:family='paragraph'><style:text-properties style:font-name='OpenFace' fo:font-size='30pt' fo:color='#ffffff'/></style:style><style:style style:name='bold' style:family='text'><style:text-properties fo:font-weight='bold'/></style:style><style:style style:name='small' style:family='text' style:parent-style-name='bold'><style:text-properties fo:font-size='80%'/></style:style>";
    let d = document(
        styles,
        &frame(
            "<text:p text:style-name='p'><text:span text:style-name='bold'>A<text:span>B</text:span><text:span text:style-name='small'>C</text:span></text:span></text:p>",
        ),
    );
    let p = import(&package(&d)).unwrap();
    let body = p.slides[0].shapes[0].text.as_ref().unwrap();
    let runs = &body.paragraphs[0].runs;
    assert_eq!(runs.iter().map(|r| r.props.size).collect::<Vec<_>>(), [Some(30.0), Some(30.0), Some(24.0)]);
    assert!(runs.iter().all(|r| r.props.bold == Some(true) && r.props.font.as_deref() == Some("Liberation Sans")));
    assert!(runs.iter().all(|r| r.props.fill == Some(Fill::solid(ColorRef::rgb(Rgba::WHITE)))));
    let placed = deckcraft_render::place_slide(&p, 0);
    assert!(!placed.texts[0].layout.runs.is_empty());
    assert!(placed.texts[0].layout.runs.iter().all(|r| r.size >= 24.0));
    let q = import(&export(&p).unwrap()).unwrap();
    assert_eq!(
        q.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].runs.iter().map(|r| r.props.size).collect::<Vec<_>>(),
        [Some(30.0), Some(30.0), Some(24.0)]
    );
}
#[test]
fn text_decoration_locale_and_script_fonts_are_native() {
    let styles = "<style:style style:name='rich' style:family='text'><style:text-properties style:font-name='OpenFace' style:font-name-asian='OpenFace' style:font-name-complex='OpenFace' fo:language='nl' fo:country='BE' fo:font-variant='small-caps' style:text-line-through-style='solid' style:text-line-through-type='double' style:text-underline-style='wave' style:text-underline-type='double' style:text-underline-color='#28649a' fo:background-color='#f0e0c0' fo:letter-spacing='normal'/></style:style>";
    let p = import(&package(&document(styles, &frame("<text:p><text:span text:style-name='rich'>Original</text:span></text:p>")))).unwrap();
    let bytes = export(&p).unwrap();
    dump_export("rich", &bytes);
    let q = import(&bytes).unwrap();
    for deck in [&p, &q] {
        let r = &deck.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].runs[0].props;
        assert_eq!(r.lang.as_deref(), Some("nl-BE"));
        assert_eq!(r.font_ea.as_deref(), Some("Liberation Sans"));
        assert_eq!(r.font_cs.as_deref(), Some("Liberation Sans"));
        assert_eq!(r.caps, Some(Caps::Small));
        assert_eq!(r.strike, Some(Strike::Double));
        assert_eq!(r.underline.as_deref(), Some("wavyDbl"));
        assert!(r.highlight.is_some() && r.underline_color.is_some());
        assert_eq!(r.spacing, Some(0.0));
    }
}
#[test]
fn tabs_and_body_wrap_control_actual_line_layout() {
    let styles = "<style:style style:name='nowrap' style:family='graphic'><style:graphic-properties fo:wrap-option='no-wrap' fo:padding='3pt' draw:textarea-vertical-align='middle'/></style:style><style:style style:name='wrap' style:family='graphic'><style:graphic-properties fo:wrap-option='wrap'/></style:style><style:style style:name='p' style:family='paragraph'><style:paragraph-properties style:tab-stop-distance='24pt' style:writing-mode='rl-tb' fo:text-align='start'><style:tab-stops><style:tab-stop style:position='72pt' style:type='right'/><style:tab-stop style:position='36pt' style:type='center'/></style:tab-stops></style:paragraph-properties><style:text-properties fo:font-size='20pt' style:font-name='OpenFace'/></style:style>";
    let paragraph = "<text:p text:style-name='p'>A wide original heading with several words</text:p>";
    let d = document(
        styles,
        &format!(
            "<draw:page><draw:frame draw:style-name='nowrap' svg:width='150pt' svg:height='90pt'><draw:text-box>{paragraph}</draw:text-box></draw:frame><draw:frame draw:style-name='wrap' svg:width='150pt' svg:height='90pt'><draw:text-box>{paragraph}</draw:text-box></draw:frame></draw:page>"
        ),
    );
    let p = import(&package(&d)).unwrap();
    let a = p.slides[0].shapes[0].text.as_ref().unwrap();
    assert_eq!(a.body.wrap, Some(false));
    assert_eq!(a.body.inset_l, Some(3.0));
    assert_eq!(a.body.anchor, Some(Anchor::Middle));
    assert_eq!(a.paragraphs[0].props.align, Some(deckcraft_model::text::Align::Right));
    let tabs = a.paragraphs[0].props.tabs.as_ref().unwrap();
    assert_eq!(tabs.iter().map(|t| t.pos).collect::<Vec<_>>(), [36.0, 72.0]);
    assert_eq!(a.paragraphs[0].props.default_tab, Some(24.0));
    let placed = deckcraft_render::place_slide(&p, 0);
    assert_eq!(placed.texts[0].layout.lines.len(), 1);
    assert!(placed.texts[1].layout.lines.len() > 1);
    let q = import(&export(&p).unwrap()).unwrap();
    assert_eq!(q.slides[0].shapes[0].text.as_ref().unwrap().body.wrap, Some(false));
    let para = &q.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0];
    assert_eq!(para.props.tabs.as_ref().unwrap().iter().map(|t| t.pos).collect::<Vec<_>>(), [36.0, 72.0]);
    assert_eq!(para.props.default_tab, Some(24.0));
    dump_export("tabs", &export(&p).unwrap());
}
#[test]
fn list_schemes_continuation_paragraphs_and_label_geometry() {
    let styles = "<text:list-style style:name='numbers'><text:list-level-style-number text:level='1' style:num-format='a' style:num-prefix='(' style:num-suffix=')' text:start-value='3'><style:list-level-properties text:space-before='12pt' text:min-label-width='8pt'/></text:list-level-style-number></text:list-style>";
    let p=import(&package(&document(styles,&frame("<text:list text:style-name='numbers'><text:list-item><text:p>One</text:p><text:p>Continuation</text:p></text:list-item><text:list-item><text:p>Two</text:p></text:list-item></text:list>")))).unwrap();
    let paras = &p.slides[0].shapes[0].text.as_ref().unwrap().paragraphs;
    assert_eq!(paras[0].props.bullet, Some(Bullet::AutoNum { scheme: "alphaLcParenBoth".into(), start_at: 1 }));
    assert_eq!(paras[0].props.number_restart, Some(3));
    assert_eq!(paras[2].props.number_restart, Some(4));
    assert_eq!(paras[1].props.bullet, Some(Bullet::None));
    assert_eq!(paras[0].props.margin_left, Some(20.0));
    assert_eq!(paras[0].props.indent, Some(-8.0));
}
#[test]
fn shape_and_text_links_resolve_named_internal_slides_and_fields_keep_kinds() {
    let d = document(
        "",
        "<draw:page draw:name='First' xml:id='page-a'><draw:a xlink:href='#Next%20Page'><draw:rect xml:id='box-a' svg:width='20pt' svg:height='20pt'/></draw:a><draw:frame draw:id='text-a' svg:width='300pt' svg:height='100pt'><draw:text-box><text:p><text:a xlink:href='#Next%20Page' office:title='Jump'>Next</text:a><text:page-number>1</text:page-number><text:date text:fixed='true'>2026-10-15</text:date></text:p></draw:text-box></draw:frame></draw:page><draw:page draw:name='Next Page'/>",
    );
    let p = import(&package(&d)).unwrap();
    assert!(matches!(p.slides[0].shapes[0].click.as_ref().unwrap().action,Action::Slide{slide} if slide==p.slides[1].id));
    assert!(p.slides[0].shapes[0].tags.iter().any(|(k, v)| k == "odp:id" && v == "box-a"));
    let runs = &p.slides[0].shapes[1].text.as_ref().unwrap().paragraphs[0].runs;
    assert!(matches!(runs[0].props.link.as_ref().unwrap().action,Action::Slide{slide} if slide==p.slides[1].id));
    assert_eq!(runs[0].props.link.as_ref().unwrap().tooltip, "Jump");
    assert!(matches!(&runs[1].kind,RunKind::Field{field} if field=="slidenum"));
    assert!(matches!(runs[2].kind, RunKind::Text));
}
#[test]
#[ignore = "Private local diagnostic via DECKCRAFT_ODP_LOCAL_FIXTURE; no source content is printed"]
fn private_header_wrap_and_font_diagnostic() {
    let bytes = std::fs::read(std::env::var("DECKCRAFT_ODP_LOCAL_FIXTURE").unwrap()).unwrap();
    let p = import(&bytes).unwrap();
    let header = p.slides[0].shapes.iter().find(|s| s.fill == Some(Fill::solid(ColorRef::rgb(Rgba::rgb(125, 147, 138))))).unwrap();
    let body = header.text.as_ref().unwrap();
    assert_eq!(body.body.wrap, Some(true));
    assert_eq!(body.body.autofit, Some(AutoFit::Shape));
    assert!(body.paragraphs.iter().flat_map(|p| &p.runs).all(|r| r.props.size == Some(32.0)));
    let mut isolated = p.clone();
    let slide = std::sync::Arc::make_mut(&mut isolated.slides[0]);
    slide.shapes = vec![header.clone()];
    let placed = deckcraft_render::place_slide(&isolated, 0);
    let layout = &placed.texts[0].layout;
    eprintln!(
        "private header: chars={}, sourcefont={:?}, resolvedfonts={:?}, lines={}, wrap={:?}",
        body.paragraphs.iter().map(|p| p.char_len()).sum::<usize>(),
        body.paragraphs[0].runs[0].props.font,
        layout.runs.iter().map(|r| r.face.family.clone()).collect::<Vec<_>>(),
        layout.lines.len(),
        body.body.wrap
    );
}

#[test]
fn odf_whitespace_collapses_across_spans_but_explicit_spaces_and_breaks_remain() {
    let d = document("", &frame("<text:p> \n A  <text:span> \t B </text:span>  C <text:s text:c='2'/> D<text:tab/>E<text:line-break/>F  </text:p>"));
    let p = import(&package(&d)).unwrap();
    let para = &p.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0];
    assert_eq!(para.runs.iter().map(|r| r.text.as_str()).collect::<String>(), "A B C    D\tEF");
    assert_eq!(para.runs.iter().filter(|r| matches!(r.kind, RunKind::Break)).count(), 1);
    let q = import(&export(&p).unwrap()).unwrap();
    assert_eq!(q.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].text(), para.text());
}
#[test]
fn adjacent_lists_explicit_restarts_and_continuation_render_the_intended_labels() {
    let styles =
        "<text:list-style style:name='n'><text:list-level-style-number text:level='1' style:num-format='1' style:num-suffix='.'/></text:list-style>";
    let lists = "<text:list text:style-name='n' xml:id='first'><text:list-item><text:p>One</text:p></text:list-item><text:list-item><text:p>Two</text:p></text:list-item></text:list><text:list text:style-name='n'><text:list-item><text:p>Restart</text:p></text:list-item><text:list-item text:start-value='7'><text:p>Seven</text:p></text:list-item><text:list-item><text:p>Eight</text:p></text:list-item></text:list><text:list text:style-name='n' text:continue-list='first'><text:list-item><text:p>Continue three</text:p></text:list-item></text:list>";
    let p = import(&package(&document(styles, &frame(lists)))).unwrap();
    let expected = ["1.", "2.", "1.", "7.", "8.", "3."];
    for deck in [&p, &import(&export(&p).unwrap()).unwrap()] {
        let placed = deckcraft_render::place_slide(deck, 0);
        let layout = &placed.texts[0].layout;
        for (para, label) in expected.iter().enumerate() {
            let run = layout.runs.iter().find(|r| r.para == para && r.cells.is_empty()).unwrap();
            assert_eq!(
                run.glyphs.iter().map(|g| g.0).collect::<Vec<_>>(),
                label.chars().map(|c| run.face.glyph_for(c)).collect::<Vec<_>>(),
                "paragraph {para}"
            );
        }
    }
}
#[test]
fn excessive_tab_allocations_and_text_style_cycles_are_bounded() {
    let stops = "<style:tab-stop style:position='1pt' style:type='left'/>".repeat(129);
    let style = format!(
        "<style:style style:name='p' style:family='paragraph'><style:paragraph-properties><style:tab-stops>{stops}</style:tab-stops></style:paragraph-properties></style:style>"
    );
    assert!(import(&package(&document(&style, &frame("<text:p text:style-name='p'>Own text</text:p>")))).unwrap_err().0.contains("tab stops"));
    let styles = "<style:style style:name='cycle' style:family='text' style:parent-style-name='cycle'><style:text-properties fo:font-size='110%'/></style:style>";
    assert!(
        import(&package(&document(styles, &frame("<text:p><text:span text:style-name='cycle'>Own text</text:span></text:p>"))))
            .unwrap_err()
            .0
            .contains("style inheritance")
    );
}

#[test]
fn typography_outside_the_model_is_reported_with_recovery_source() {
    let styles = "<style:style style:name='p' style:family='paragraph'><style:paragraph-properties style:line-spacing='2pt'><style:tab-stops><style:tab-stop style:position='40pt' style:type='char' style:char=',' style:leader-style='dotted'/></style:tab-stops></style:paragraph-properties></style:style>";
    let bytes = package(&document(styles, &frame("<text:p text:style-name='p'>Own source</text:p>")));
    let p = import(&bytes).unwrap();
    let warnings = p.tags.iter().filter(|(key, _)| key == "odp:warning").map(|(_, value)| value.as_str()).collect::<Vec<_>>();
    assert!(warnings.iter().any(|m| m.contains("line spacing")));
    assert!(warnings.iter().any(|m| m.contains("tab leaders")));
    assert!(p.media.iter().any(|m| m.content_type == "application/vnd.deckcraft.odp-source" && m.data.as_slice() == bytes));
}

fn dump_export(label: &str, bytes: &[u8]) {
    use std::io::Read;
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_TEXT_OUTPUT") {
        std::fs::create_dir_all(&dir).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        for name in ["content.xml", "styles.xml"] {
            let mut xml = String::new();
            zip.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
            std::fs::write(format!("{dir}/{label}-{name}"), xml).unwrap();
        }
    }
}
#[test]
fn body_columns_and_autofit_roundtrip_and_unrepresentable_scaling_refuses() {
    let styles = "<style:style style:name='g' style:family='graphic'><style:graphic-properties draw:auto-grow-height='true' draw:textarea-horizontal-align='center'><style:columns fo:column-count='2' fo:column-gap='12pt'/></style:graphic-properties></style:style>";
    let d = document(
        styles,
        "<draw:page><draw:frame draw:style-name='g' svg:width='300pt' svg:height='100pt'><draw:text-box><text:p>Contributor original columns</text:p></draw:text-box></draw:frame></draw:page>",
    );
    let mut p = import(&package(&d)).unwrap();
    let bytes = export(&p).unwrap();
    dump_export("columns", &bytes);
    let q = import(&bytes).unwrap();
    for deck in [&p, &q] {
        let body = &deck.slides[0].shapes[0].text.as_ref().unwrap().body;
        assert_eq!(body.columns, Some(2));
        assert_eq!(body.col_spacing, Some(12.0));
        assert_eq!(body.anchor_ctr, Some(true));
        assert_eq!(body.autofit, Some(AutoFit::Shape));
    }
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().body.autofit =
        Some(AutoFit::Shrink { font_scale: Some(0.8), line_reduction: 0.0 });
    assert!(export(&p).unwrap_err().0.contains("pre-scaled text autofit"));
}

#[test]
fn font_size_dependent_kerning_refuses_instead_of_turning_it_off() {
    let mut p = import(&package(&document("", &frame("<text:p>Original kerning</text:p>")))).unwrap();
    std::sync::Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].props.kern = Some(12.0);
    assert!(export(&p).unwrap_err().0.contains("kerning thresholds"));
}
