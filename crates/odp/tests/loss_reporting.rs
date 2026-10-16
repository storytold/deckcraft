//! Contributor-original semantic-loss fixtures. A recognized ODF namespace is not
//! evidence that a property, its value, or its children have editable representation.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_odp::{MIME, export, import};
use std::io::{Cursor, Read, Write};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";
fn package(local_styles: &str, external_styles: Option<&str>, text: &str) -> Vec<u8> {
    package_with_metadata(local_styles, external_styles, text, None)
}
fn package_with_metadata(local_styles: &str, external_styles: Option<&str>, text: &str, metadata: Option<&str>) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles>{local_styles}</office:automatic-styles><office:body><office:presentation><draw:page><draw:frame svg:width='144pt' svg:height='72pt'><draw:text-box><text:p text:style-name='P'>{text}</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let external = external_styles
        .map(|s| format!("<office:document-styles {NS} office:version='1.3'><office:styles>{s}</office:styles></office:document-styles>"));
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, value) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ]
    .into_iter()
    .chain(external.as_deref().map(|s| ("styles.xml", s)))
    .chain(metadata.map(|s| ("meta.xml", s)))
    {
        zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(value.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn style(bag: &str) -> String {
    format!("<style:style style:name='P' style:family='paragraph'>{bag}</style:style>")
}
fn assert_guarded(bytes: &[u8]) {
    let presentation = import(bytes).unwrap();
    assert!(presentation.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("semantic audit v1")), "{:?}", presentation.tags);
    assert!(export(&presentation).is_err());
    assert_eq!(presentation.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes);
}

#[test]
fn standard_paragraph_omissions_guard_used_defaults_and_external_parents() {
    for field in [
        "fo:text-align-last='center'",
        "fo:keep-together='always'",
        "fo:keep-with-next='always'",
        "style:line-spacing='3pt'",
        "style:font-independent-line-spacing='true'",
    ] {
        let bag = format!("<style:paragraph-properties {field}/>");
        assert_guarded(&package(&style(&bag), None, "Editable text"));
        let parent = format!("<style:style style:name='Parent' style:family='paragraph'>{bag}</style:style>");
        let local = "<style:style style:name='P' style:family='paragraph' style:parent-style-name='Parent'/>";
        assert_guarded(&package(local, Some(&parent), "Editable text"));
        let default = format!("<style:default-style style:family='paragraph'>{bag}</style:default-style>");
        assert_guarded(&package("", Some(&default), "Editable text"));
    }
}
#[test]
fn malformed_or_unsupported_values_and_partial_compounds_guard() {
    for bag in [
        "<style:paragraph-properties style:writing-mode='tb-rl'/>",
        "<style:paragraph-properties fo:text-align='made-up'/>",
        "<style:paragraph-properties fo:line-height='NaN%'/>",
        "<style:paragraph-properties fo:margin-left='ten pt'/>",
        "<style:text-properties fo:text-transform='lowercase'/>",
        "<style:text-properties style:text-underline-style='dotted' style:text-underline-type='double'/>",
        "<style:text-properties style:text-underline-style='solid' style:text-underline-width='bold' style:text-underline-mode='skip-white-space'/>",
        "<style:paragraph-properties><style:tab-stops><style:tab-stop style:position='12pt' style:type='char' style:char=','/></style:tab-stops></style:paragraph-properties>",
    ] {
        assert_guarded(&package(&style(bag), None, "Editable text"));
    }
}
#[test]
fn unrepresented_children_and_unknown_attributes_on_supported_children_guard() {
    for bag in [
        "<style:paragraph-properties><style:drop-cap style:lines='3'/></style:paragraph-properties>",
        "<style:text-properties><style:background-image/></style:text-properties>",
        "<style:paragraph-properties><style:tab-stops fo:keep-together='always'/></style:paragraph-properties>",
        "<style:paragraph-properties><style:tab-stops><style:tab-stop style:position='12pt' fo:keep-with-next='always'/></style:tab-stops></style:paragraph-properties>",
    ] {
        assert_guarded(&package(&style(bag), None, "Editable text"));
    }
    assert_guarded(&package("", None, "<text:ruby><text:ruby-base>base</text:ruby-base><text:ruby-text>annotation</text:ruby-text></text:ruby>"));
}
#[test]
fn same_local_foreign_attributes_remain_guarded_and_exact_source_is_recoverable() {
    let bag = "<style:paragraph-properties xmlns:a='urn:original:a' xmlns:b='urn:original:b' a:tracking='one' b:tracking='two'/>";
    assert_guarded(&package(&style(bag), None, "Editable text"));
}
#[test]
fn audit_saturation_never_allows_fresh_export_or_leaks_private_values() {
    let properties = (0..400).map(|i| format!("<style:style style:family='paragraph' style:name='Unused{i}'><style:paragraph-properties fo:text-align-last='PRIVATE Ω {i}'/></style:style>")).collect::<String>();
    let bytes = package(&properties, None, "Editable text");
    assert_guarded(&bytes);
    let presentation = import(&bytes).unwrap();
    assert!(!presentation.tags.iter().any(|(_, value)| value.contains("PRIVATE") || value.contains('Ω')));
}
#[test]
fn represented_properties_and_fresh_writer_output_remain_exportable() {
    let properties = style(
        "<style:paragraph-properties fo:text-align='center' fo:margin-left='6pt' fo:line-height='120%' style:writing-mode='rl-tb' fo:background-color='#abcdef'/><style:text-properties fo:font-family='Liberation Sans' fo:font-size='18pt' fo:font-weight='bold' fo:font-style='italic' fo:color='#123456'/>",
    );
    let presentation = import(&package(&properties, None, "Editable text")).unwrap();
    assert!(!presentation.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", presentation.tags);
    let encoded = export(&presentation).unwrap();
    let reopened = import(&encoded).unwrap();
    assert!(!reopened.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", reopened.tags);
    assert!(export(&reopened).is_ok());
}

#[test]
fn explicit_script_size_matches_native_layout_and_writer() {
    for position in ["30% 100%", "sub 58%", "0% 66%"] {
        assert_guarded(&package(&style(&format!("<style:text-properties style:text-position='{position}'/>")), None, "Script"));
    }
    for position in ["30% 66%", "sub 66.0%", "super", "0% 100%"] {
        let presentation = import(&package(&style(&format!("<style:text-properties style:text-position='{position}'/>")), None, "Script")).unwrap();
        assert!(!presentation.tags.iter().any(|(key, _)| key == "odp:warning"), "{position}: {:?}", presentation.tags);
        let encoded = export(&presentation).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(&encoded)).unwrap();
        let mut content = String::new();
        zip.by_name("content.xml").unwrap().read_to_string(&mut content).unwrap();
        assert!(
            content.contains(if position.starts_with("0%") {
                "0% 100%"
            } else if position.starts_with("sub") {
                "-25% 66%"
            } else {
                "30% 66%"
            }),
            "{position}: {content}"
        );
        assert!(export(&import(&encoded).unwrap()).is_ok());
    }
}

#[test]
fn metadata_parts_receive_the_same_foreign_semantic_guard() {
    let meta = format!(
        "<office:document-meta {NS} xmlns:q='urn:original:metadata-extension'><office:meta><q:tracking>private payload</q:tracking></office:meta></office:document-meta>"
    );
    assert_guarded(&package_with_metadata("", None, "Editable text", Some(&meta)));
}

#[test]
fn explicit_inherited_character_clears_are_not_mistaken_for_consumption() {
    let defaults = "<style:default-style style:family='paragraph'><style:text-properties fo:font-variant='small-caps' fo:language='en' style:text-underline-style='solid' style:text-underline-color='#123456'/></style:default-style>";
    for clears in ["fo:font-variant='normal'", "style:text-underline-color='font-color'"] {
        let named = style(&format!("<style:text-properties {clears}/>"));
        assert_guarded(&package(&named, Some(defaults), "Editable text"));
    }
}

#[test]
fn explicit_character_highlight_and_strike_clears_override_and_roundtrip() {
    use deckcraft_model::{Rgba, text::Strike};
    let defaults = "<style:default-style style:family='paragraph'><style:text-properties fo:background-color='#c91f2b' style:text-line-through-style='solid' style:text-line-through-type='double'/></style:default-style>";
    let inherited = import(&package(&style(""), Some(defaults), "Highlight control")).unwrap();
    let clear =
        style("<style:text-properties fo:background-color='transparent' style:text-line-through-style='none' style:text-line-through-type='none'/>");
    let deck = import(&package(&clear, Some(defaults), "Highlight control")).unwrap();
    assert!(!deck.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", deck.tags);
    let encoded = export(&deck).unwrap();
    let reopened = import(&encoded).unwrap();
    for presentation in [&deck, &reopened] {
        let run = &presentation.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].runs[0];
        assert_eq!(run.props.highlight.as_ref().unwrap().resolve(&presentation.masters[0].theme.colors, None), Rgba::TRANSPARENT);
        assert_eq!(run.props.strike, Some(Strike::None));
        let image = deckcraft_render::render_slide(presentation, 0, &Default::default());
        assert_eq!(image.pixels.as_chunks::<4>().0.iter().filter(|pixel| **pixel == [201, 31, 43, 255]).count(), 0);
    }
    let control = deckcraft_render::render_slide(&inherited, 0, &Default::default());
    assert!(control.pixels.as_chunks::<4>().0.iter().filter(|pixel| **pixel == [201, 31, 43, 255]).count() > 10);
}

#[test]
fn explicit_language_none_clears_parent_language_without_becoming_absent() {
    let defaults = "<style:default-style style:family='paragraph'><style:text-properties fo:language='en' fo:country='US'/></style:default-style>";
    let local = style("<style:text-properties fo:language='none'/>");
    let deck = import(&package(&local, Some(defaults), "Language clear")).unwrap();
    assert!(!deck.tags.iter().any(|(key, _)| key == "odp:warning"));
    let reopened = import(&export(&deck).unwrap()).unwrap();
    for p in [&deck, &reopened] {
        let props = &p.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].runs[0].props;
        assert_eq!(props.lang.as_deref(), Some(""));
        let mut changed_parent = props.clone();
        changed_parent.inherit(&deckcraft_model::text::RunProps { lang: Some("fr-BE".into()), ..Default::default() });
        assert_eq!(changed_parent.lang.as_deref(), Some(""));
    }
}

#[test]
fn partial_character_highlight_alpha_still_refuses_lossy_export() {
    use deckcraft_model::{ColorRef, Presentation, Rgba, Shape, TextBody};
    use std::sync::Arc;
    let mut deck = Presentation::default();
    let mut body = TextBody::from_text("Partial alpha remains guarded");
    body.paragraphs[0].runs[0].props.highlight = Some(ColorRef::rgb(Rgba::rgba(201, 31, 43, 127)));
    Arc::make_mut(&mut deck.slides[0]).shapes.push(Shape { text: Some(body), ..Default::default() });
    assert!(export(&deck).is_err());
}

#[test]
fn script_size_percentages_require_named_common_styles() {
    let parent = "<style:style style:name='Parent' style:family='paragraph'><style:text-properties fo:font-size='20pt'/></style:style>";
    for field in ["style:font-size-asian", "style:font-size-complex"] {
        let named = format!(
            "<style:style style:name='P' style:family='paragraph' style:parent-style-name='Parent'><style:text-properties {field}='125%'/></style:style>"
        );
        assert_guarded(&package(&named, Some(parent), "Script size"));
        let default = format!("<style:default-style style:family='paragraph'><style:text-properties {field}='125%'/></style:default-style>");
        // An unresolved default percentage may refuse import altogether. If a
        // future resolver accepts it, the invalid context must still block export.
        if let Ok(deck) = import(&package("", Some(&default), "Script size")) {
            assert!(deck.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("semantic audit v1")));
            assert!(export(&deck).is_err());
        }
        let common = import(&package("", Some(&format!("{parent}{named}")), "Script size")).unwrap();
        assert!(!common.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", common.tags);
        let reopened = import(&export(&common).unwrap()).unwrap();
        assert!(!reopened.tags.iter().any(|(key, _)| key == "odp:warning"));
    }
}
