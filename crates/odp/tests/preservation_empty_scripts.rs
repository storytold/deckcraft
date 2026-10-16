//! Contributor-original fixtures for empty scripts containers and closed font declaration enums.
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
    edit(&mut desired);
    let before_export = desired.clone();
    let output = export(&desired).expect("bounded empty scripts/font enums permit supported text edits");
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
    let mut presentation = import(&source).unwrap_or_else(|_| import(&zip(&fixture("1.3", true, true, "<office:scripts/>"))).unwrap());
    edit(&mut presentation);
    // Import-rejected hostile XML must also hit the export package proof, never a warning bypass.
    presentation.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(source);
    let before = presentation.clone();
    let error = export(&presentation).expect_err("unproved container/dependency must refuse").to_string();
    assert!(error.contains("source-preserving text export refused:") || error.contains("XML"), "unexpected error: {error}");
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
    if let Ok(directory) = std::env::var("DECKCRAFT_EMPTY_SCRIPTS_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (stage, files) in [("source", before), ("output", after)] {
            for name in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                let filename = format!("{version}-{stage}-{}", name.replace('/', "-"));
                std::fs::write(std::path::Path::new(&directory).join(filename), &files[name]).unwrap();
            }
        }
    }
}

#[test]
fn empty_or_absent_scripts_preserve_real_edits_with_all_cache_combinations_and_versions() {
    for version in ["1.2", "1.3"] {
        for (preview, statistics) in [(false, false), (true, false), (false, true), (true, true)] {
            for scripts in
                ["", "<office:scripts/>", "<office:scripts> \r\n\t </office:scripts>", "<office:scripts><!--original comment--> \n </office:scripts>"]
            {
                let files = fixture(version, preview, statistics, scripts);
                let after = preserved(&files);
                if preview && statistics && scripts == "<office:scripts/>" {
                    emit(version, &files, &after);
                }
            }
        }
    }
}

#[test]
fn scripts_and_font_aliases_default_namespaces_entities_and_survivor_comments_are_exact() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            let scripts = "<scripts xmlns='urn:oasis:names:tc:opendocument:xmlns:office:1.0'><!--survivor--> &#32;\t </scripts>";
            let mut files = fixture(version, cache, cache, scripts);
            let font = "<s:font-face xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:v='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' s:font-pitch = 'vari&#97;ble' v:font-family = '&apos;BIZ UDPGothic&apos;' s:font-family-generic = 'swi&#115;s' s:name = 'Open&#70;ace'> \n </s:font-face>";
            for part in ["content.xml", "styles.xml"] {
                // Paired leaves contain whitespace nodes, so they cannot share the reader's
                // identical-empty-font exception across parts. Keep their owners distinct.
                let scoped_font = if part == "styles.xml" { font.replace("Open&#70;ace", "Styles&#70;ace") } else { font.into() };
                replace(&mut files, part, FONT, &scoped_font);
            }
            preserved(&files);
            replace(&mut files, "content.xml", scripts, "<o:scripts xmlns:o='urn:oasis:names:tc:opendocument:xmlns:office:1.0'/> ");
            preserved(&files);
        }
    }
}

#[test]
fn every_standard_font_enum_and_optional_absence_is_classified_only_on_font_faces() {
    for version in ["1.2", "1.3"] {
        for generic in [None, Some("roman"), Some("swiss"), Some("modern"), Some("decorative"), Some("script"), Some("system")] {
            for pitch in [None, Some("fixed"), Some("variable")] {
                let mut files = fixture(version, true, true, "<office:scripts/>");
                for part in ["content.xml", "styles.xml"] {
                    replace(
                        &mut files,
                        part,
                        " style:font-family-generic='swiss'",
                        &generic.map(|value| format!(" style:font-family-generic='{value}'")).unwrap_or_default(),
                    );
                    replace(
                        &mut files,
                        part,
                        " style:font-pitch='variable'",
                        &pitch.map(|value| format!(" style:font-pitch='{value}'")).unwrap_or_default(),
                    );
                }
                preserved(&files);
            }
        }
    }
}

#[test]
fn scripts_attributes_text_and_active_children_refuse_with_or_without_cache() {
    for cache in [false, true] {
        for scripts in [
            "<office:scripts xml:id='extra'/>",
            "<office:scripts xml:base='file:///'/>",
            "<office:scripts harmless='true'/>",
            "<office:scripts xmlns:f='urn:deckcraft:test:foreign' f:extra='true'/>",
            "<office:scripts>active</office:scripts>",
            "<office:scripts>&#160;</office:scripts>",
            "<office:scripts><![CDATA[active]]></office:scripts>",
            "<office:scripts><office:script/></office:scripts>",
            "<office:scripts><office:event-listeners/></office:scripts>",
            "<office:scripts><text:p/></office:scripts>",
            "<office:scripts><?active instruction?></office:scripts>",
        ] {
            refused(&fixture("1.3", cache, cache, scripts));
        }
    }
}

#[test]
fn duplicate_scripts_including_aliased_expanded_names_refuse_in_both_proof_paths() {
    for cache in [false, true] {
        for second in ["<office:scripts/>", "<o:scripts xmlns:o='urn:oasis:names:tc:opendocument:xmlns:office:1.0'/>"] {
            refused_reason(
                &fixture("1.3", cache, cache, &format!("<office:scripts/>{second}")),
                Some(if cache { "cache scripts root order" } else { "unverified XML, active content, or content-derived statistics" }),
            );
        }
    }
}

#[test]
fn foreign_rebound_and_unqualified_script_containers_never_gain_trust() {
    for cache in [false, true] {
        for scripts in [
            "<scripts/>",
            "<office:scripts xmlns:office='urn:deckcraft:test:foreign'/>",
            "<f:scripts xmlns:f='urn:deckcraft:test:foreign'/>",
            "<scripts xmlns='urn:deckcraft:test:foreign'/>",
        ] {
            refused(&fixture("1.3", cache, cache, scripts));
        }
    }
}

#[test]
fn empty_scripts_refuse_in_other_parts_or_below_the_content_root() {
    for cache in [false, true] {
        for (part, old, new) in [
            ("content.xml", "<office:presentation>", "<office:presentation><office:scripts/>"),
            ("content.xml", "<draw:text-box>", "<draw:text-box><office:scripts/>"),
            ("content.xml", "<office:automatic-styles>", "<office:automatic-styles><office:scripts/>"),
            ("styles.xml", "<office:styles/>", "<office:scripts/><office:styles/>"),
            ("meta.xml", "<office:meta>", "<office:scripts/><office:meta>"),
            ("settings.xml", "</office:document-settings>", "<office:scripts/></office:document-settings>"),
        ] {
            let mut files = fixture("1.3", cache, cache, "");
            if part == "settings.xml" {
                replace(&mut files, part, "office:version='1.3'/>", "office:version='1.3'></office:document-settings>");
            }
            replace(&mut files, part, old, new);
            refused(&files);
        }
        let mut files = fixture("1.3", cache, cache, "");
        replace(&mut files, "content.xml", "xml:id='page'>", "xml:id='page'><office:scripts/>");
        refused(&files);
    }
}

#[test]
fn scripts_after_fonts_styles_or_body_refuse_even_when_empty_and_unique() {
    for cache in [false, true] {
        for ending in ["</office:font-face-decls>", "</office:automatic-styles>", "</office:body>"] {
            let mut files = fixture("1.3", cache, cache, "");
            replace(&mut files, "content.xml", ending, &format!("{ending}<office:scripts/>"));
            refused_reason(
                &files,
                Some(if cache { "cache scripts root order" } else { "unverified XML, active content, or content-derived statistics" }),
            );
        }
        let files = fixture("1.3", cache, cache, "<!--before scripts--> \n <office:scripts/>");
        preserved(&files);
    }
}

#[test]
fn unknown_whitespace_case_uri_and_empty_font_enum_values_refuse() {
    for (attribute, original, values) in [
        (
            "style:font-family-generic",
            "swiss",
            vec!["", "Swiss", "swiss ", " swiss", "sans-serif", "swiss roman", "file:///font", "Thumbnails/thumbnail.png"],
        ),
        ("style:font-pitch", "variable", vec!["", "Variable", "variable ", " variable", "proportional", "fixed variable", "urn:pitch:fixed"]),
    ] {
        for value in values {
            for part in ["content.xml", "styles.xml"] {
                let mut files = fixture("1.3", true, true, "<office:scripts/>");
                replace(&mut files, part, &format!("{attribute}='{original}'"), &format!("{attribute}='{value}'"));
                refused_reason(&files, Some("cache unclassified attribute/value"));
            }
        }
    }
}

#[test]
fn foreign_unqualified_duplicate_and_rebound_font_attributes_refuse() {
    for part in ["content.xml", "styles.xml"] {
        for replacement in [
            "xmlns:f='urn:deckcraft:test:foreign' f:font-family-generic='swiss'",
            "font-family-generic='swiss'",
            "xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' s:font-family-generic='swiss' style:font-family-generic='swiss'",
            "xmlns:style='urn:deckcraft:test:foreign' style:font-family-generic='swiss'",
        ] {
            let mut files = fixture("1.3", true, true, "<office:scripts/>");
            replace(&mut files, part, "style:font-family-generic='swiss'", replacement);
            refused(&files);
        }
        let mut files = fixture("1.3", true, true, "<office:scripts/>");
        replace(&mut files, part, "style:font-pitch='variable'", "xmlns:f='urn:deckcraft:test:foreign' f:font-pitch='variable'");
        refused(&files);
    }
}

#[test]
fn font_children_extra_attributes_and_existing_identifier_and_reference_guards_remain_closed() {
    for part in ["content.xml", "styles.xml"] {
        for font in [
            "<style:font-face style:name='OpenFace' svg:font-family='BIZ UDPGothic' style:font-family-generic='swiss' style:font-pitch='variable'><svg:font-face-src><svg:font-face-uri xlink:href='Pictures/keep.png'/></svg:font-face-src></style:font-face>",
            "<style:font-face style:name='OpenFace' svg:font-family='BIZ UDPGothic' style:font-family-generic='swiss' style:font-pitch='variable' style:font-charset='utf-8'/>",
            "<style:font-face style:name='OpenFace' svg:font-family='BIZ UDPGothic' xlink:href='Thumbnails/thumbnail.png'/>",
            "<style:font-face style:name='   ' svg:font-family='BIZ UDPGothic'/>",
            "<style:font-face svg:font-family='BIZ UDPGothic'/>",
            "<style:font-face style:name='OpenFace' svg:font-family='BIZ UDPGothic'>opaque</style:font-face>",
        ] {
            let mut files = fixture("1.3", true, true, "<office:scripts/>");
            replace(&mut files, part, FONT, font);
            refused(&files);
        }
    }
    let mut files = fixture("1.3", true, true, "<office:scripts/>");
    replace(&mut files, "content.xml", "style:font-name='OpenFace'", "style:font-name='MissingFace'");
    refused(&files);
}

#[test]
fn font_enums_do_not_classify_other_property_owners_or_unrelated_active_content() {
    for mutation in
        ["style:font-family-generic='swiss'", "style:font-pitch='variable'", "xlink:href='Thumbnails/thumbnail.png'", "xml:base='file:///'"]
    {
        let mut files = fixture("1.3", true, true, "<office:scripts/>");
        replace(&mut files, "content.xml", "fo:keep-with-next='always'", &format!("fo:keep-with-next='always' {mutation}"));
        refused(&files);
    }
    let mut files = fixture("1.3", true, true, "<office:scripts/>");
    replace(&mut files, "content.xml", "<office:body>", "<office:body><office:forms/>");
    refused(&files);
}
