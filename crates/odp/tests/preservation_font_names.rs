//! Contributor-original synthetic packages for exact opaque ODF font aliases.
//! BIZ UDPGothic names the existing OFL craft-fonts face; no font bytes are added here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const MANIFEST: &str = "META-INF/manifest.xml";
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const THUMB: &str = "Thumbnails/thumbnail.png";
const THUMB_OWNER: &str = "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xlink='http://www.w3.org/1999/xlink'";
const FONT: &str =
    "<style:font-face style:name='OpenFace' svg:font-family='BIZ UDPGothic' style:font-family-generic='swiss' style:font-pitch='variable'/>";
const COUNTS: &str = "<meta:document-statistic meta:word-count='4' meta:character-count='24' meta:non-whitespace-character-count='22' meta:sentence-count='2' meta:syllable-count='6' meta:page-count='1' meta:paragraph-count='2'/>";

fn fixture(version: &str, preview: bool, statistics: bool, scripts: &str) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='{version}'>{scripts}<office:font-face-decls>{FONT}</office:font-face-decls><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/><style:text-properties style:font-name='OpenFace' fo:font-size='24pt'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Original page' draw:master-page-name='M' xml:id='page'><draw:frame xml:id='local' svg:width='240pt' svg:height='120pt'><draw:text-box><text:p text:style-name='P' xml:id='one'>Original one</text:p><text:p text:style-name='P' xml:id='two'>Original two</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:font-face-decls>{FONT}</office:font-face-decls><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='720pt' fo:page-height='405pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
    );
    let statistic = if statistics { COUNTS } else { "" };
    let meta = format!(
        "<office:document-meta {NS} office:version='{version}'><office:meta><dc:title>Original scalar title</dc:title>{statistic}</office:meta></office:document-meta>"
    );
    // A versioned document-settings root without a child is schema-valid in both versions.
    let settings = format!("<office:document-settings {NS} office:version='{version}'/>");
    let png = deckcraft_render::Image { width: 1, height: 1, pixels: vec![37, 109, 181, 255] }.to_png();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        ("settings.xml".into(), settings.into_bytes()),
        ("Pictures/keep.png".into(), png.clone()),
    ]);
    if preview {
        files.insert(THUMB.into(), png);
    }
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><!--root survivor--><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/>\r\n"
    );
    for name in files.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name.ends_with(".png") { "image/png" } else { "text/xml" };
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>\n<!--survivor gap-->\t"));
    }
    manifest.push_str("</manifest:manifest>");
    files.insert(MANIFEST.into(), manifest.into_bytes());
    files
}
fn zip(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
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
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        assert!(files.insert(member.name().into(), bytes).is_none());
    }
    files
}
fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(files[part].clone()).unwrap();
    assert!(text.contains(old), "missing mutation {old}");
    files.insert(part.into(), text.replace(old, new).into_bytes());
}
fn edit(presentation: &mut Presentation) {
    let text = Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap();
    text.paragraphs[0].runs[0].text = "Edited".into();
    text.paragraphs[1].runs[0].text = "Changed".into();
}
fn without_source_ranges(mut presentation: Presentation) -> Presentation {
    for source in presentation.media.iter_mut().filter(|media| media.content_type == SOURCE) {
        source.data = Arc::new(Vec::new());
    }
    for slide in &mut presentation.slides {
        let slide = Arc::make_mut(slide);
        slide.tags.retain(|(key, _)| key != "odp:source-range");
        for shape in &mut slide.shapes {
            shape.tags.retain(|(key, _)| key != "odp:source-range");
        }
    }
    presentation
}
fn preserved(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    let source = zip(files);
    let mut desired = import(&source).expect("public original fixture imports");
    assert!(export(&desired).is_err(), "an actual edit is required");
    let props = &desired.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].runs[0].props;
    assert_eq!(props.font.as_deref(), Some("BIZ UDPGothic"));
    assert_eq!(props.font_ea.as_deref(), Some("BIZ UDPGothic"));
    assert_eq!(props.font_cs.as_deref(), Some("BIZ UDPGothic"));
    edit(&mut desired);
    let before_export = desired.clone();
    let output = export(&desired).expect("typed exact font aliases permit supported text edits");
    assert_eq!(desired, before_export, "export must not mutate the model");
    assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    let after = parts(&output);
    let mut expected = files.clone();
    expected.remove(THUMB);
    for (name, bytes) in &mut expected {
        if name == "content.xml" {
            *bytes = String::from_utf8(bytes.clone()).unwrap().replace("Original one", "Edited").replace("Original two", "Changed").into_bytes();
        } else if name == MANIFEST {
            *bytes = String::from_utf8(bytes.clone()).unwrap().replace(THUMB_OWNER, "").into_bytes();
        } else if name == "meta.xml" {
            let mut value = String::from_utf8(bytes.clone()).unwrap();
            for attribute in [
                " meta:word-count='4'",
                " meta:character-count='24'",
                " meta:non-whitespace-character-count='22'",
                " meta:sentence-count='2'",
                " meta:syllable-count='6'",
            ] {
                value = value.replace(attribute, "");
            }
            *bytes = value.into_bytes();
        }
    }
    assert_eq!(after, expected, "every surviving payload and all unedited XML gaps must be exact");
    let mut reopened = import(&output).expect("output reimports");
    assert_eq!(reopened.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), output);
    assert_eq!(without_source_ranges(desired), without_source_ranges(reopened.clone()), "only source bytes/ranges may change");
    Arc::make_mut(&mut reopened.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Again".into();
    let next_output = export(&reopened).expect("fresh output profile still permits a second edit");
    let mut next_expected = after.clone();
    replace(&mut next_expected, "content.xml", "Edited", "Again");
    assert_eq!(parts(&next_output), next_expected, "fresh cache proof must not change additional payloads");
    after
}
fn refused(files: &BTreeMap<String, Vec<u8>>) {
    refused_reason(files, None);
}
fn refused_reason(files: &BTreeMap<String, Vec<u8>>, expected_reason: Option<&str>) {
    let source = zip(files);
    let imported = import(&source);
    let import_error = imported.as_ref().err().map(ToString::to_string);
    let mut presentation = imported.unwrap_or_else(|_| import(&zip(&fixture("1.3", true, true, "<office:scripts/>"))).unwrap());
    edit(&mut presentation);
    // Import-rejected hostile XML must also hit the export package proof, never a warning bypass.
    presentation.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(source);
    let before = presentation.clone();
    let error = export(&presentation).expect_err("unproved container/dependency must refuse").to_string();
    assert!(
        error.contains("source-preserving text export refused:")
            || error.contains("XML")
            || expected_reason.is_some_and(|reason| error.ends_with(reason))
            || import_error.as_deref().is_some_and(|reason| error.ends_with(reason)),
        "unexpected error: {error}"
    );
    assert!(!error.contains("changes outside plain slide text"), "the package proof must refuse before native comparison: {error}");
    assert!(
        !error.contains("source has unsupported content beyond passive text properties")
            && !error.contains("source audit contains non-preservable semantics"),
        "the package proof must refuse before warning/audit eligibility: {error}"
    );
    if let Some(reason) = expected_reason {
        assert!(error.ends_with(reason), "expected exact package-proof reason {reason}: {error}");
    }
    assert_eq!(presentation, before, "refusal must leave the model/source unchanged");
}
fn emit(version: &str, before: &BTreeMap<String, Vec<u8>>, after: &BTreeMap<String, Vec<u8>>) {
    if let Ok(directory) = std::env::var("DECKCRAFT_FONT_NAMES_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (stage, files) in [("source", before), ("output", after)] {
            for name in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                let filename = format!("{version}-{stage}-{}", name.replace('/', "-"));
                std::fs::write(std::path::Path::new(&directory).join(filename), &files[name]).unwrap();
            }
        }
    }
}

fn attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('\'', "&apos;").replace('<', "&lt;").replace('"', "&quot;")
}
fn face(alias: &str) -> String {
    FONT.replace("style:name='OpenFace'", &format!("style:name='{}'", attribute(alias)))
}
fn named_fixture(version: &str, preview: bool, statistics: bool, alias: &str) -> BTreeMap<String, Vec<u8>> {
    let mut files = fixture(version, preview, statistics, "<office:scripts/>");
    for part in ["content.xml", "styles.xml"] {
        replace(&mut files, part, FONT, &face(alias));
    }
    let alias = attribute(alias);
    replace(
        &mut files,
        "content.xml",
        "style:font-name='OpenFace'",
        &format!("style:font-name='{alias}' style:font-name-asian='{alias}' style:font-name-complex='{alias}'"),
    );
    files
}
fn add_content_face(files: &mut BTreeMap<String, Vec<u8>>, declaration: &str) {
    replace(files, "content.xml", "</office:font-face-decls>", &format!("{declaration}</office:font-face-decls>"));
}

#[test]
fn exact_opaque_names_resolve_all_three_slots_with_each_cache_combination_and_version() {
    for version in ["1.2", "1.3"] {
        for (preview, statistics) in [(false, false), (true, false), (false, true), (true, true)] {
            let files = named_fixture(version, preview, statistics, "Deck Face 日本 العربية");
            let after = preserved(&files);
            if preview && statistics {
                emit(version, &files, &after);
            }
        }
    }
}

#[test]
fn spaces_unicode_punctuation_and_pathlike_aliases_preserve_without_resource_interpretation() {
    for alias in [
        " Leading Face",
        "Trailing Face ",
        "Deck\u{a0}Face",
        "Deck 日本",
        "Deck العربية",
        "Deck 'quoted' & composed",
        "Deck,Face",
        "alias:地方",
        "Thumbnails/thumbnail.png",
        "../font",
        "https://example.invalid/font",
        "url(font)",
    ] {
        for cache in [false, true] {
            preserved(&named_fixture("1.3", cache, cache, alias));
        }
    }
}

#[test]
fn utf8_byte_boundary_is_256_in_both_declarations_and_references() {
    for alias in ["F".repeat(256), "é".repeat(128), format!("{}a", "界".repeat(85))] {
        assert_eq!(alias.len(), 256);
        preserved(&named_fixture("1.3", true, true, &alias));
    }
    for alias in ["F".repeat(257), format!("{}a", "é".repeat(128)), format!("{}ab", "界".repeat(85))] {
        assert_eq!(alias.len(), 257);
        refused_reason(&named_fixture("1.3", true, true, &alias), Some("cache font name grammar"));
    }
    let mut files = named_fixture("1.3", true, true, "Valid Face");
    let too_long = "&#70;".repeat(257);
    for slot in ["style:font-name", "style:font-name-asian", "style:font-name-complex"] {
        let mut bad = files.clone();
        replace(&mut bad, "content.xml", &format!("{slot}='Valid Face'"), &format!("{slot}='{too_long}'"));
        refused_reason(&bad, Some("cache unresolved font reference"));
    }
    replace(&mut files, "content.xml", "style:name='Valid Face'", &format!("style:name='{too_long}'"));
    refused_reason(&files, Some("cache font name grammar"));
}

#[test]
fn attribute_entities_and_namespace_aliases_keep_exact_lexical_bytes() {
    for version in ["1.2", "1.3"] {
        let mut files = named_fixture(version, true, true, "Deck Face 日本 & 'quote'");
        for part in ["content.xml", "styles.xml"] {
            replace(
                &mut files,
                part,
                &format!("office:version='{version}'"),
                &format!("xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' office:version='{version}'"),
            );
            replace(&mut files, part, "style:name='Deck Face 日本 &amp; &apos;quote&apos;'", "s:name='Deck&#32;Face 日本 &#38; &#39;quote&#39;'");
        }
        for slot in ["font-name", "font-name-asian", "font-name-complex"] {
            replace(
                &mut files,
                "content.xml",
                &format!("style:{slot}='Deck Face 日本 &amp; &apos;quote&apos;'"),
                &format!("s:{slot}='Deck&#x20;Face 日本 &amp; &apos;quote&apos;'"),
            );
        }
        preserved(&files);
    }
}

#[test]
fn xml_raw_attribute_whitespace_normalizes_once_for_declarations_references_and_source_lexer() {
    // XML1.0 normalizes literal whitespace, including CRLF as one line ending, to spaces.
    // Character references to controls are deliberately tested separately and refused.
    for raw in ["Deck\tFace", "Deck\nFace", "Deck\rFace", "Deck\r\nFace"] {
        for (raw_declaration, raw_reference) in [(true, false), (false, true), (true, true)] {
            let mut files = named_fixture("1.3", true, true, "Deck Face");
            if raw_declaration {
                for part in ["content.xml", "styles.xml"] {
                    replace(&mut files, part, "style:name='Deck Face'", &format!("style:name='{raw}'"));
                }
            }
            if raw_reference {
                for slot in ["style:font-name", "style:font-name-asian", "style:font-name-complex"] {
                    replace(&mut files, "content.xml", &format!("{slot}='Deck Face'"), &format!("{slot}='{raw}'"));
                }
            }
            preserved(&files);
        }
        let mut duplicate = named_fixture("1.3", true, true, "Deck Face");
        add_content_face(&mut duplicate, &FONT.replace("style:name='OpenFace'", &format!("style:name='{raw}'")));
        refused_reason(&duplicate, Some("cache ambiguous definition"));
    }
}

#[test]
fn empty_whitespace_and_decoded_control_names_refuse_without_leaking_values() {
    for alias in ["", " ", "\u{a0}", " \u{2003} ", "\u{85}", "Face\u{7f}", "Face\u{9f}"] {
        refused_reason(&named_fixture("1.3", true, true, alias), Some("cache font name grammar"));
    }
    for control in ["&#9;", "&#10;", "&#13;", "&#x7f;", "&#x85;", "&#x9f;"] {
        let mut declaration = named_fixture("1.3", true, true, "Valid Face");
        replace(&mut declaration, "content.xml", "style:name='Valid Face'", &format!("style:name='Face{control}'"));
        refused_reason(&declaration, Some("cache font name grammar"));
        for slot in ["style:font-name", "style:font-name-asian", "style:font-name-complex"] {
            let mut reference = named_fixture("1.3", true, true, "Valid Face");
            replace(&mut reference, "content.xml", &format!("{slot}='Valid Face'"), &format!("{slot}='Face{control}'"));
            refused_reason(&reference, Some("cache unresolved font reference"));
        }
    }
    for invalid in ["&#0;", "&#x1f;", "&#xfffe;", "&#xffff;"] {
        let mut files = named_fixture("1.3", true, true, "Valid Face");
        replace(&mut files, "content.xml", "style:name='Valid Face'", &format!("style:name='Face{invalid}'"));
        refused(&files);
    }
    let mut files = named_fixture("1.3", true, true, "Valid Face");
    let bytes = files.get_mut("content.xml").unwrap();
    let location = bytes.windows(10).position(|value| value == b"Valid Face").unwrap();
    bytes[location] = 0xff;
    refused(&files);
}

#[test]
fn whitespace_case_and_unicode_normalization_variants_are_distinct_exact_keys() {
    for (declaration, different) in
        [("Face", " Face"), ("Face", "Face "), ("Face Face", "Face\u{a0}Face"), ("Face", "face"), ("Café Face", "Cafe\u{301} Face")]
    {
        let mut files = named_fixture("1.3", true, true, declaration);
        add_content_face(&mut files, &face(different));
        preserved(&files); // Both distinct definitions may coexist without collision.
        let mut mismatch = named_fixture("1.3", true, true, declaration);
        for slot in ["style:font-name", "style:font-name-asian", "style:font-name-complex"] {
            let mut bad = mismatch.clone();
            replace(&mut bad, "content.xml", &format!("{slot}='{}'", attribute(declaration)), &format!("{slot}='{}'", attribute(different)));
            refused_reason(&bad, Some("cache unresolved font reference"));
        }
        // An exact declaration/reference for the variant is equally valid.
        mismatch = named_fixture("1.3", true, true, different);
        preserved(&mismatch);
    }
}

#[test]
fn font_scopes_retain_styles_fallback_and_refuse_reverse_or_untyped_resolution() {
    let alias = "Scoped Face 日本";
    let mut styles_only = named_fixture("1.3", true, true, alias);
    replace(&mut styles_only, "content.xml", &face(alias), "");
    preserved(&styles_only); // The definition is encountered in styles.xml after content.xml.
    let mut content_only = named_fixture("1.3", true, true, alias);
    replace(&mut content_only, "styles.xml", &face(alias), "");
    preserved(&content_only);
    let unused = format!(
        "<office:styles><style:style style:name='Unused' style:family='text'><style:text-properties style:font-name='{}'/></style:style></office:styles>",
        attribute(alias)
    );
    replace(&mut content_only, "styles.xml", "<office:styles/>", &unused);
    refused_reason(&content_only, Some("cache unresolved font reference"));
    let mut untyped = named_fixture("1.3", true, true, "FontOnly");
    for part in ["content.xml", "styles.xml"] {
        replace(&mut untyped, part, &face("FontOnly"), "");
    }
    replace(
        &mut untyped,
        "content.xml",
        "<office:automatic-styles>",
        "<office:automatic-styles><style:style style:name='FontOnly' style:family='text'/>",
    );
    replace(&mut untyped, "content.xml", "xml:id='local'", "xml:id='FontOnly'");
    refused_reason(&untyped, Some("cache unresolved font reference"));
}

#[test]
fn duplicate_decoded_names_and_conflicting_cross_part_owners_remain_refused() {
    for duplicate in [face("Deck Face"), FONT.replace("style:name='OpenFace'", "style:name='Deck&#32;Face'")] {
        let mut files = named_fixture("1.3", true, true, "Deck Face");
        add_content_face(&mut files, &duplicate);
        refused_reason(&files, Some("cache ambiguous definition"));
    }
    for changed in [
        face("Deck Face").replace("style:font-pitch='variable'", "style:font-pitch='fixed'"),
        face("Deck Face").replace("style:font-family-generic='swiss'", "style:font-family-generic='roman'"),
    ] {
        let mut files = named_fixture("1.3", true, true, "Deck Face");
        replace(&mut files, "styles.xml", &face("Deck Face"), &changed);
        refused_reason(&files, Some("overlapping common and automatic ODF style identities are not supported"));
    }
}

#[test]
fn foreign_missing_and_wrong_context_font_definitions_and_references_remain_closed() {
    for replacement in ["xmlns:f='urn:deckcraft:test:foreign' f:name='Deck Face'", "name='Deck Face'", ""] {
        let mut files = named_fixture("1.3", true, true, "Deck Face");
        replace(&mut files, "content.xml", "style:name='Deck Face'", replacement);
        refused(&files);
    }
    for slot in ["font-name", "font-name-asian", "font-name-complex"] {
        let mut files = named_fixture("1.3", true, true, "Deck Face");
        replace(
            &mut files,
            "content.xml",
            &format!("style:{slot}='Deck Face'"),
            &format!("xmlns:f='urn:deckcraft:test:foreign' f:{slot}='Deck Face'"),
        );
        refused_reason(&files, Some("cache XML identity/depth"));
    }
    let mut files = named_fixture("1.3", true, true, "Deck Face");
    replace(&mut files, "content.xml", &face("Deck Face"), "");
    replace(&mut files, "content.xml", "<office:automatic-styles>", &format!("<office:automatic-styles>{}", face("Deck Face")));
    refused_reason(&files, Some("cache unclassified XML context"));
    let mut files = named_fixture("1.3", true, true, "Deck Face");
    replace(&mut files, "meta.xml", "<office:meta>", &format!("<office:meta>{}", face("Deck Face")));
    refused_reason(&files, Some("cache unclassified XML context"));
}

#[test]
fn family_resource_and_nonfont_identifier_grammars_do_not_inherit_font_alias_permission() {
    for (old, new, reason) in [
        ("style:name='P'", "style:name='Style Name 日本'", "cache identifier grammar"),
        ("xml:id='local'", "xml:id='Shape Name 日本'", "cache identifier grammar"),
        ("svg:font-family='BIZ UDPGothic'", "svg:font-family='日本 Family'", "cache unclassified attribute/value"),
        ("svg:font-family='BIZ UDPGothic'", "svg:font-family='url(font)'", "cache unclassified attribute/value"),
        ("style:font-pitch='variable'", "style:font-pitch='Deck Face'", "cache unclassified attribute/value"),
    ] {
        let mut files = named_fixture("1.3", true, true, "Deck Face");
        replace(&mut files, "content.xml", old, new);
        refused_reason(&files, Some(reason));
    }
    for extra in ["xlink:href='Thumbnails/thumbnail.png'", "style:font-charset='utf-8'", "xml:base='file:///'"] {
        let mut files = named_fixture("1.3", true, true, "Thumbnails/thumbnail.png");
        replace(&mut files, "content.xml", "style:font-pitch='variable'", &format!("style:font-pitch='variable' {extra}"));
        refused_reason(&files, Some("cache unclassified attribute/value"));
    }
    let mut files = named_fixture("1.3", true, true, "url(font)");
    let declaration = face("url(font)");
    replace(
        &mut files,
        "content.xml",
        &declaration,
        &declaration.replace("/>", "><svg:font-face-src><svg:font-face-uri xlink:href='Pictures/keep.png'/></svg:font-face-src></style:font-face>"),
    );
    refused_reason(&files, Some("cache unclassified XML context"));
}

#[test]
fn unused_references_and_definition_count_are_still_bounded_and_checked() {
    let mut files = named_fixture("1.3", true, true, "Deck Face");
    replace(
        &mut files,
        "content.xml",
        "<office:automatic-styles>",
        "<office:automatic-styles><style:style style:name='Unused' style:family='text'><style:text-properties style:font-name='Missing Face'/></style:style>",
    );
    refused_reason(&files, Some("cache unresolved font reference"));
    let mut files = named_fixture("1.3", true, true, "Deck Face");
    let mut declarations = String::new();
    for index in 0..4097 {
        declarations.push_str(&face(&format!("Unused Face {index}")));
    }
    add_content_face(&mut files, &declarations);
    refused_reason(&files, Some("cache definition count"));
}
