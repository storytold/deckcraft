//! Contributor-original preservation fixtures for exact drawing-page visibility switches.
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
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xlink='http://www.w3.org/1999/xlink'";
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
    let output = export(&desired).expect("proved drawing-page switches permit supported text edits");
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
    if let Ok(directory) = std::env::var("DECKCRAFT_PAGE_SWITCHES_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (stage, files) in [("source", before), ("output", after)] {
            for name in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                let filename = format!("{version}-{stage}-{}", name.replace('/', "-"));
                std::fs::write(std::path::Path::new(&directory).join(filename), &files[name]).unwrap();
            }
        }
    }
}

const SWITCHES: &[&str] = &[
    "presentation:background-objects-visible",
    "presentation:background-visible",
    "presentation:display-date-time",
    "presentation:display-footer",
    "presentation:display-header",
    "presentation:display-page-number",
];
fn switches_fixture(version: &str, preview: bool, statistics: bool, owner: &str, attributes: &str) -> BTreeMap<String, Vec<u8>> {
    let mut files = fixture(version, preview, statistics, "<office:scripts/>");
    let bag = format!("<style:drawing-page-properties {attributes}/>");
    let named = format!("<style:style style:name='Switches' style:family='drawing-page'>{bag}</style:style>");
    match owner {
        "common" => replace(&mut files, "styles.xml", "<office:styles/>", &format!("<office:styles>{named}</office:styles>")),
        "default" => replace(
            &mut files,
            "styles.xml",
            "<office:styles/>",
            &format!("<office:styles><style:default-style style:family='drawing-page'>{bag}</style:default-style></office:styles>"),
        ),
        "content-auto" => replace(&mut files, "content.xml", "<office:automatic-styles>", &format!("<office:automatic-styles>{named}")),
        "styles-auto" => replace(&mut files, "styles.xml", "<office:automatic-styles>", &format!("<office:automatic-styles>{named}")),
        _ => panic!("unknown original test owner"),
    }
    files
}
fn combined(value: &str) -> String {
    SWITCHES.iter().map(|key| format!("{key}='{value}'")).collect::<Vec<_>>().join(" ")
}
fn warning_refused(files: &BTreeMap<String, Vec<u8>>) {
    let source = zip(files);
    let mut desired = import(&source).expect("known original owner semantics import with a retained warning");
    assert!(desired.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("regular master placeholders or inherited visibility")));
    edit(&mut desired);
    let before = desired.clone();
    let error = export(&desired).expect_err("unsupported editable master-field ownership remains blocked").to_string();
    assert!(
        error.ends_with("source has unsupported content beyond passive text properties"),
        "the cache grammar must pass before the unchanged warning gate: {error}"
    );
    assert_eq!(desired, before);
    assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
}

#[test]
fn each_switch_false_true_and_absent_survives_each_named_scope_version_and_cache_path() {
    for version in ["1.2", "1.3"] {
        for (preview, statistics) in [(false, false), (true, false), (false, true), (true, true)] {
            for owner in ["common", "content-auto", "styles-auto"] {
                preserved(&switches_fixture(version, preview, statistics, owner, ""));
                for key in SWITCHES {
                    for value in ["false", "true"] {
                        preserved(&switches_fixture(version, preview, statistics, owner, &format!("{key}='{value}'")));
                    }
                }
            }
        }
    }
}

#[test]
fn combined_switches_keep_all_bytes_and_emit_canonical_schema_parts() {
    for version in ["1.2", "1.3"] {
        for value in ["false", "true"] {
            let files = switches_fixture(version, true, true, "content-auto", &combined(value));
            let after = preserved(&files);
            if value == "false" {
                emit(version, &files, &after);
            }
        }
    }
}

#[test]
fn default_flags_pass_the_profile_but_do_not_bypass_master_field_or_visibility_warning() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            preserved(&switches_fixture(version, cache, cache, "default", ""));
            for key in SWITCHES {
                for value in ["false", "true"] {
                    let files = switches_fixture(version, cache, cache, "default", &format!("{key}='{value}'"));
                    if key.starts_with("presentation:background-") && value == "true" {
                        preserved(&files);
                    } else {
                        warning_refused(&files);
                    }
                }
            }
        }
    }
}

#[test]
fn used_local_background_controls_preserve_actual_model_values_and_text_edits() {
    for key in ["presentation:background-objects-visible", "presentation:background-visible"] {
        for value in ["false", "true"] {
            let mut files = switches_fixture("1.3", true, true, "content-auto", &format!("{key}='{value}'"));
            replace(&mut files, "content.xml", "draw:name='Original page'", "draw:name='Original page' draw:style-name='Switches'");
            if key == "presentation:background-objects-visible" {
                assert_eq!(import(&zip(&files)).unwrap().slides[0].show_master_shapes, value == "true");
            }
            preserved(&files);
        }
    }
}

#[test]
fn used_field_flags_and_inherited_false_backgrounds_still_refuse_after_cache_proof() {
    for key in SWITCHES {
        for value in ["false", "true"] {
            let mut files = switches_fixture("1.3", true, true, "common", &format!("{key}='{value}'"));
            replace(&mut files, "styles.xml", "style:master-page style:name='M'", "style:master-page style:name='M' draw:style-name='Switches'");
            if key.starts_with("presentation:background-") && value == "true" {
                preserved(&files);
            } else {
                warning_refused(&files);
            }
        }
    }
}

#[test]
fn namespace_aliases_entity_tokens_attribute_order_and_comments_survive_exactly() {
    for cache in [false, true] {
        let mut files = switches_fixture("1.3", cache, cache, "content-auto", &combined("true"));
        replace(
            &mut files,
            "content.xml",
            &format!("<style:drawing-page-properties {}/>", combined("true")),
            "<s:drawing-page-properties xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:p='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' p:display-page-number = 'tr&#117;e' p:display-header = \"false\" p:display-footer='true' p:display-date-time='false' p:background-visible='true' p:background-objects-visible='false'> <!--original gap--> \n </s:drawing-page-properties>",
        );
        preserved(&files);
    }
}

#[test]
fn exact_boolean_tokens_reject_numeric_case_whitespace_uri_and_lists() {
    for key in SWITCHES {
        for value in [
            "0",
            "1",
            "TRUE",
            "False",
            "",
            " ",
            "true ",
            " false",
            "true false",
            "urn:boolean:true",
            "Thumbnails/thumbnail.png",
            "true&#32;",
            "true&#9;",
        ] {
            let files = switches_fixture("1.3", true, true, "content-auto", &format!("{key}='{value}'"));
            refused_reason(&files, Some("cache unclassified attribute/value"));
        }
    }
}

#[test]
fn foreign_unqualified_rebound_and_duplicate_attribute_identities_refuse() {
    for key in SWITCHES {
        let local = key.strip_prefix("presentation:").unwrap();
        for attributes in [
            format!("xmlns:f='urn:deckcraft:test:foreign' f:{local}='true'"),
            format!("{local}='true'"),
            format!("xmlns:presentation='urn:deckcraft:test:foreign' presentation:{local}='true'"),
        ] {
            refused_reason(&switches_fixture("1.3", true, true, "common", &attributes), Some("cache XML identity/depth"));
        }
        let attributes = format!("xmlns:p='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' {key}='true' p:{local}='false'");
        refused_reason(&switches_fixture("1.3", true, true, "content-auto", &attributes), Some("duplicate expanded XML attribute"));
    }
}

#[test]
fn graphic_bags_wrong_families_and_default_child_spoofs_remain_denied() {
    for owner in ["common", "default", "content-auto", "styles-auto"] {
        for key in SWITCHES {
            let mut graphic = switches_fixture("1.3", true, true, owner, &format!("{key}='true'"));
            let part = if owner == "content-auto" { "content.xml" } else { "styles.xml" };
            replace(&mut graphic, part, "style:family='drawing-page'", "style:family='graphic'");
            replace(&mut graphic, part, "style:drawing-page-properties", "style:graphic-properties");
            refused_reason(&graphic, Some("cache unclassified attribute/value"));
            let mut wrong_family = switches_fixture("1.3", true, true, owner, &format!("{key}='true'"));
            replace(&mut wrong_family, part, "style:family='drawing-page'", "style:family='paragraph'");
            refused_reason(&wrong_family, Some("cache unclassified XML context"));
            let mut wrong_child = switches_fixture("1.3", true, true, owner, &format!("{key}='true'"));
            replace(&mut wrong_child, part, "style:drawing-page-properties", "style:graphic-properties");
            refused_reason(&wrong_child, Some("cache unclassified XML context"));
        }
    }
}

#[test]
fn property_flags_are_not_style_attributes_or_child_elements_or_foreign_property_bags() {
    let mut owner_attribute = switches_fixture("1.3", true, true, "content-auto", "");
    replace(&mut owner_attribute, "content.xml", "style:name='Switches'", "style:name='Switches' presentation:display-header='true'");
    refused_reason(&owner_attribute, Some("cache unclassified attribute/value"));
    for replacement in [
        "<style:drawing-page-properties><presentation:display-header/></style:drawing-page-properties>",
        "<style:drawing-page-properties><style:default-style style:family='drawing-page'/></style:drawing-page-properties>",
        "<f:drawing-page-properties xmlns:f='urn:deckcraft:test:foreign' presentation:display-header='true'/>",
        "<style:drawing-page-properties xmlns:style='urn:deckcraft:test:foreign' presentation:display-header='true'/>",
    ] {
        let mut files = switches_fixture("1.3", true, true, "content-auto", "");
        replace(&mut files, "content.xml", "<style:drawing-page-properties />", replacement);
        refused(&files);
    }
}

#[test]
fn false_flags_cannot_hide_unknown_scalars_resource_dependencies_or_active_content() {
    for attributes in [
        "presentation:background-visible='false' presentation:visibility='hidden'",
        "presentation:background-visible='false' draw:auto-grow-height='true'",
        "presentation:background-visible='false' xlink:href='Thumbnails/thumbnail.png'",
        "presentation:background-visible='false' xml:base='file:///'",
    ] {
        refused_reason(&switches_fixture("1.3", true, true, "content-auto", attributes), Some("cache unclassified attribute/value"));
    }
    let mut files = switches_fixture("1.3", true, true, "content-auto", &combined("false"));
    replace(&mut files, "content.xml", "xml:id='page'>", "xml:id='page'><office:forms/>");
    refused_reason(&files, Some("cache unclassified XML context"));
    let mut files = switches_fixture("1.3", true, true, "content-auto", &combined("false"));
    replace(&mut files, "content.xml", "style:name='Switches'", "style:name='Switches Name'");
    refused_reason(&files, Some("cache identifier grammar"));
}
