//! Independent contributor-original regressions for the narrowly passive scripts/font slice.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MANIFEST: &str = "META-INF/manifest.xml";
const THUMB: &str = "Thumbnails/thumbnail.png";
const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:xlink='http://www.w3.org/1999/xlink' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";
const FONT: &str =
    "<style:font-face style:name='ReviewFont' svg:font-family='BIZ UDPGothic' style:font-family-generic='swiss' style:font-pitch='variable'/>";

fn fixture(version: &str, statistics: bool, preview: bool) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='{version}'>\r\n<!--root gap--><office:scripts/>\t<!--scripts gap--><office:font-face-decls>{FONT}</office:font-face-decls><office:automatic-styles><style:style style:name='ReviewParagraph' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/><style:text-properties style:font-name='ReviewFont'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Independent' draw:master-page-name='ReviewMaster'><draw:frame svg:width='160pt' svg:height='70pt'><draw:text-box><text:p text:style-name='ReviewParagraph'>Before</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:font-face-decls>{FONT}</office:font-face-decls><office:styles/><office:automatic-styles><style:page-layout style:name='ReviewPaper'><style:page-layout-properties fo:page-width='400pt' fo:page-height='300pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='ReviewMaster' style:page-layout-name='ReviewPaper'/></office:master-styles></office:document-styles>"
    );
    let statistic = if statistics { "<meta:document-statistic meta:word-count='1' meta:page-count='1'/>" } else { "" };
    let metadata =
        format!("<office:document-meta {NS} office:version='{version}'><office:meta><!--meta gap-->{statistic}</office:meta></office:document-meta>");
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), metadata.into_bytes()),
        (
            "settings.xml".into(),
            format!("<office:document-settings {NS} office:version='{version}'><!--no configuration--></office:document-settings>").into_bytes(),
        ),
    ]);
    if preview {
        files.insert(THUMB.into(), deckcraft_render::Image { width: 1, height: 1, pixels: vec![61, 139, 83, 255] }.to_png());
    }
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'>\n<manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/><!--owner gap-->"
    );
    for name in files.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name == THUMB { "image/png" } else { "text/xml" };
        manifest.push_str(&format!("\n<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/><!--member gap-->\t"));
    }
    manifest.push_str("</manifest:manifest>");
    files.insert(MANIFEST.into(), manifest.into_bytes());
    files
}

fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, before: &str, after: &str) {
    let text = String::from_utf8(files[part].clone()).unwrap();
    assert!(text.contains(before), "mutation missing in {part}: {before}");
    files.insert(part.into(), text.replace(before, after).into_bytes());
}

fn package(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("mimetype", options).unwrap();
    writer.write_all(MIME.as_bytes()).unwrap();
    for (name, bytes) in files.iter().filter(|(name, _)| name.as_str() != "mimetype") {
        writer.start_file(name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn unpack(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut data = Vec::new();
        member.read_to_end(&mut data).unwrap();
        assert!(files.insert(member.name().into(), data).is_none());
    }
    files
}

fn edit(presentation: &mut Presentation) {
    // Equal UTF-8 length deliberately keeps byte provenance comparable without
    // deleting or normalizing any model tags during the full equality assertion.
    Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "After!".into();
}

fn assert_exact(files: &BTreeMap<String, Vec<u8>>) {
    let source = package(files);
    let mut desired = import(&source).expect("original independent fixture imports");
    edit(&mut desired);
    let unchanged = desired.clone();
    let output = export(&desired).expect("proved passive scripts/font declarations permit text edit");
    assert_eq!(desired, unchanged, "successful export must not mutate model/source");
    let mut expected = files.clone();
    replace(&mut expected, "content.xml", ">Before</text:p>", ">After!</text:p>");
    if String::from_utf8_lossy(&expected["meta.xml"]).contains(" meta:word-count='1'") {
        replace(&mut expected, "meta.xml", " meta:word-count='1'", "");
    }
    if expected.remove(THUMB).is_some() {
        replace(&mut expected, MANIFEST, &format!("<manifest:file-entry manifest:full-path='{THUMB}' manifest:media-type='image/png'/>"), "");
    }
    assert_eq!(unpack(&output), expected, "complete member inventory, payloads and surviving XML gaps are exact");
    let mut reopened = import(&output).expect("edited output reimports");
    let mut attachments = reopened.media.iter_mut().filter(|media| media.content_type == SOURCE);
    let attachment = attachments.next().unwrap();
    assert_eq!(attachment.data.as_slice(), output, "reimport attaches exact exported bytes");
    attachment.data = Arc::new(source.clone());
    assert!(attachments.next().is_none());
    assert_eq!(reopened, desired, "every requested model field survives; only new source bytes differ");
    assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
}

fn assert_refused(files: &BTreeMap<String, Vec<u8>>, expected_reason: Option<&str>) {
    let source = package(files);
    let mut current = import(&source).unwrap_or_else(|_| import(&package(&fixture("1.3", false, false))).unwrap());
    edit(&mut current);
    current.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(source.clone());
    let unchanged = current.clone();
    for _ in 0..2 {
        let error = export(&current).expect_err("unproved package must refuse a real text edit").to_string();
        if let Some(reason) = expected_reason {
            assert!(error.contains(reason), "wrong guard for {reason}: {error}");
        }
        assert_eq!(current, unchanged, "failure is atomic across every model field and original bytes");
        assert_eq!(current.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    }
}

#[test]
fn independent_exact_edit_reimport_both_versions_and_all_cache_combinations() {
    for version in ["1.2", "1.3"] {
        for statistics in [false, true] {
            for preview in [false, true] {
                assert_exact(&fixture(version, statistics, preview));
            }
        }
    }
}

#[test]
fn independent_scripts_aliases_default_namespace_comments_and_xml_whitespace() {
    for cache in [false, true] {
        for scripts in [
            format!("<o:scripts xmlns:o='{OFFICE}'> \t\r\n<!--passive--> </o:scripts>"),
            format!("<scripts xmlns='{OFFICE}'><![CDATA[ \t\r\n]]>&#32;&#x9;</scripts>"),
            "<office:scripts></office:scripts>".into(),
        ] {
            let mut files = fixture("1.3", cache, cache);
            replace(&mut files, "content.xml", "<office:scripts/>", &scripts);
            assert_exact(&files);
        }
    }
}

#[test]
fn independent_zero_scripts_or_optional_font_classification_attributes() {
    for cache in [false, true] {
        let mut files = fixture("1.2", cache, false);
        replace(&mut files, "content.xml", "<office:scripts/>", "");
        for part in ["content.xml", "styles.xml"] {
            replace(&mut files, part, " style:font-family-generic='swiss'", "");
            replace(&mut files, part, " style:font-pitch='variable'", "");
        }
        assert_exact(&files);
    }
}

#[test]
fn independent_no_cache_parent_guard_refuses_duplicates_and_misplacement() {
    const GUARD: &str = "unverified XML, active content, or content-derived statistics";
    for version in ["1.2", "1.3"] {
        let mut duplicate = fixture(version, false, false);
        replace(&mut duplicate, "content.xml", "<office:scripts/>", &format!("<office:scripts/><o:scripts xmlns:o='{OFFICE}'/>"));
        assert_refused(&duplicate, Some(GUARD));
        for (part, location) in [
            ("content.xml", "<office:body>"),
            ("content.xml", "<office:presentation>"),
            ("content.xml", "<draw:text-box>"),
            ("styles.xml", "<office:styles/>"),
            ("settings.xml", "<!--no configuration-->"),
        ] {
            let mut files = fixture(version, false, false);
            replace(&mut files, "content.xml", "<office:scripts/>", "");
            replace(&mut files, part, location, &format!("{location}<office:scripts/>"));
            assert_refused(&files, Some(GUARD));
        }
        let mut page = fixture(version, false, false);
        replace(&mut page, "content.xml", "<draw:frame", "<office:scripts/><draw:frame");
        assert_refused(&page, Some(GUARD));
    }
}

#[test]
fn independent_no_cache_parent_guard_refuses_foreign_rebound_and_unqualified_lookalikes() {
    for bad in [
        "<foreign:scripts xmlns:foreign='urn:review:foreign'/>",
        "<office:scripts xmlns:office='urn:review:foreign'/>",
        "<scripts/>",
        "<scripts xmlns='urn:review:foreign'/>",
    ] {
        let mut files = fixture("1.3", false, false);
        replace(&mut files, "content.xml", "<office:scripts/>", bad);
        assert_refused(&files, Some("unverified XML, active content, or content-derived statistics"));
    }
}

#[test]
fn independent_cache_and_parent_guards_refuse_nonempty_scripts_and_all_attributes() {
    for cache in [false, true] {
        for bad in [
            "<office:scripts>code</office:scripts>",
            "<office:scripts>&#160;</office:scripts>",
            "<office:scripts><![CDATA[x]]></office:scripts>",
            "<office:scripts><office:script/></office:scripts>",
            "<office:scripts><office:event-listeners/></office:scripts>",
            "<office:scripts><presentation:event-listener/></office:scripts>",
            "<office:scripts><office:scripts/></office:scripts>",
            "<office:scripts xml:id='passive'/>",
            "<office:scripts office:version='1.3'/>",
            "<office:scripts xmlns:f='urn:review:foreign' f:empty='true'/>",
            "<office:scripts empty='true'/>",
            "<office:scripts xml:base='Pictures/'/>",
        ] {
            let mut files = fixture("1.3", cache, cache);
            replace(&mut files, "content.xml", "<office:scripts/>", bad);
            assert_refused(&files, Some(if cache { "cache" } else { "unverified XML" }));
        }
    }
}

#[test]
fn independent_cache_profile_refuses_duplicate_and_misplaced_empty_scripts() {
    for bad in [
        "<office:scripts/><office:scripts/>",
        "<office:scripts/><office:script/>",
        "<office:scripts xmlns:office='urn:review:foreign'/>",
        "<scripts/>",
    ] {
        let mut files = fixture("1.3", true, false);
        replace(&mut files, "content.xml", "<office:scripts/>", bad);
        assert_refused(&files, Some("cache"));
    }
    for part in ["content.xml", "styles.xml"] {
        let mut files = fixture("1.3", false, true);
        replace(&mut files, "content.xml", "<office:scripts/>", "");
        let location = if part == "content.xml" { "<draw:text-box>" } else { "<office:styles/>" };
        replace(&mut files, part, location, &format!("{location}<office:scripts/>"));
        assert_refused(&files, Some("cache"));
    }
}

#[test]
fn independent_scripts_must_precede_every_other_root_element() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for earlier in ["</office:font-face-decls>", "</office:automatic-styles>", "</office:body>"] {
                let mut files = fixture(version, cache, cache);
                replace(&mut files, "content.xml", "<office:scripts/>", "");
                replace(&mut files, "content.xml", earlier, &format!("{earlier}<office:scripts/>"));
                assert_refused(&files, Some(if cache { "cache" } else { "unverified XML" }));
            }
        }
    }
}

#[test]
fn independent_processing_instructions_doctypes_and_malformed_qnames_refuse() {
    for cache in [false, true] {
        for bad in [
            "<office:scripts><?review refuse?></office:scripts>",
            "<!DOCTYPE scripts><office:scripts/>",
            "<office:scripts>",
            "<missing:scripts/>",
            "<office:scripts/><office:scripts></different:scripts>",
            "<office:scripts xml:id='a' xml:id='b'/>",
        ] {
            let mut files = fixture("1.3", cache, cache);
            replace(&mut files, "content.xml", "<office:scripts/>", bad);
            assert_refused(&files, None);
        }
    }
}

#[test]
fn independent_all_standard_font_enums_and_decoded_alias_values_preserve() {
    for version in ["1.2", "1.3"] {
        for generic in ["roman", "swiss", "modern", "decorative", "script", "system"] {
            for pitch in ["fixed", "variable"] {
                let mut files = fixture(version, true, false);
                for part in ["content.xml", "styles.xml"] {
                    replace(&mut files, part, "style:font-family-generic='swiss'", &format!("style:font-family-generic='{generic}'"));
                    replace(&mut files, part, "style:font-pitch='variable'", &format!("style:font-pitch='{pitch}'"));
                }
                assert_exact(&files);
            }
        }
    }
    let mut aliases = fixture("1.3", false, true);
    for part in ["content.xml", "styles.xml"] {
        replace(
            &mut aliases,
            part,
            FONT,
            &format!(
                "<s:font-face xmlns:s='{STYLE}' s:name='ReviewFont' svg:font-family='BIZ UDPGothic' s:font-family-generic='sw&#105;ss' s:font-pitch='vari&#x61;ble'/>"
            ),
        );
    }
    assert_exact(&aliases);
}

#[test]
fn independent_cache_font_enums_are_exact_tokens_in_each_part() {
    for part in ["content.xml", "styles.xml"] {
        for (attribute, valid, bad_values) in [
            (
                "style:font-family-generic",
                "swiss",
                vec!["", "Swiss", "SWISS", " swiss", "swiss ", "swiss&#9;", "sans-serif", "unknown", "roman,swiss", "swiss&#160;"],
            ),
            (
                "style:font-pitch",
                "variable",
                vec!["", "Variable", "VARIABLE", " variable", "variable ", "variable&#10;", "proportional", "unknown", "fixed variable"],
            ),
        ] {
            for bad in bad_values {
                let mut files = fixture("1.3", true, false);
                replace(&mut files, part, &format!("{attribute}='{valid}'"), &format!("{attribute}='{bad}'"));
                assert_refused(&files, Some("cache unclassified attribute/value"));
            }
        }
    }
}

#[test]
fn independent_font_attributes_require_exact_expanded_names_and_owner_context() {
    for part in ["content.xml", "styles.xml"] {
        for replacement in [
            "font-family-generic='swiss'",
            "xmlns:f='urn:review:foreign' f:font-family-generic='swiss'",
            "xmlns:style='urn:review:foreign' style:font-family-generic='swiss'",
            "style:font-family-generic='swiss' xml:lang='en'",
            "style:font-family-generic='swiss' style:font-charset='unicode'",
            "style:font-family-generic='swiss' xlink:href='Pictures/absent.png'",
        ] {
            let mut files = fixture("1.3", true, false);
            replace(&mut files, part, "style:font-family-generic='swiss'", replacement);
            assert_refused(&files, Some("cache"));
        }
        let mut duplicate = fixture("1.3", false, true);
        replace(&mut duplicate, part, "style:font-pitch='variable'", &format!("style:font-pitch='variable' xmlns:s='{STYLE}' s:font-pitch='fixed'"));
        assert_refused(&duplicate, Some("duplicate expanded XML attribute"));
    }
    let mut misplaced = fixture("1.3", true, false);
    replace(
        &mut misplaced,
        "content.xml",
        "fo:keep-with-next='always'",
        "fo:keep-with-next='always' style:font-family-generic='swiss' style:font-pitch='fixed'",
    );
    assert_refused(&misplaced, Some("cache unclassified attribute/value"));
}

#[test]
fn independent_font_identifiers_references_family_grammar_and_leaf_rules_remain_closed() {
    for part in ["content.xml", "styles.xml"] {
        for name in ["", "   ", "Review&#x9;Font", "Review&#xA;Font", "Review&#xD;Font"] {
            let mut files = fixture("1.3", true, false);
            replace(&mut files, part, "style:name='ReviewFont'", &format!("style:name='{name}'"));
            assert_refused(&files, Some("cache font name grammar"));
        }
        for font in [
            "<style:font-face svg:font-family='BIZ UDPGothic'/>",
            "<style:font-face style:name='ReviewFont' svg:font-family='BIZ UDPGothic, serif'/>",
            "<style:font-face style:name='ReviewFont' svg:font-family='https://example.invalid/font'/>",
            "<style:font-face style:name='ReviewFont'><svg:font-face-src><svg:font-face-uri xlink:href='Pictures/font.ttf'/></svg:font-face-src></style:font-face>",
            "<style:font-face style:name='ReviewFont'><office:binary-data>AAAA</office:binary-data></style:font-face>",
            "<style:font-face style:name='ReviewFont'>content</style:font-face>",
        ] {
            let mut files = fixture("1.3", true, false);
            replace(&mut files, part, FONT, font);
            assert_refused(&files, Some("cache"));
        }
        let mut duplicate = fixture("1.3", true, false);
        replace(&mut duplicate, part, FONT, &format!("{FONT}{FONT}"));
        assert_refused(&duplicate, Some("cache ambiguous definition"));
    }
    let mut missing = fixture("1.3", true, true);
    replace(&mut missing, "content.xml", "style:font-name='ReviewFont'", "style:font-name='MissingFont'");
    assert_refused(&missing, Some("cache unresolved font reference"));
}

#[test]
fn independent_passive_admission_does_not_relax_cache_dependencies_or_no_edit_policy() {
    let mut referenced = fixture("1.3", true, true);
    replace(
        &mut referenced,
        "styles.xml",
        "<office:styles/>",
        &format!("<office:styles><draw:fill-image draw:name='Unused' xlink:href='{THUMB}'/></office:styles>"),
    );
    assert_refused(&referenced, Some("cache unsafe part reference"));
    let mut unknown = fixture("1.3", true, true);
    replace(&mut unknown, "content.xml", "fo:keep-with-next='always'", "fo:keep-with-next='always' fo:letter-spacing='1pt'");
    assert_refused(&unknown, Some("cache unclassified attribute/value"));
    for cache in [false, true] {
        let current = import(&package(&fixture("1.3", cache, cache))).unwrap();
        let unchanged = current.clone();
        assert!(export(&current).is_err(), "no supported text edit still requires original recovery");
        assert_eq!(current, unchanged);
    }
}
