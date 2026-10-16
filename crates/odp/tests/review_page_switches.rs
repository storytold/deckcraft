//! Independent, contributor-original page-switch preservation regressions.
//! Fixtures contain no external document or bundled font/media assets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

type Files = BTreeMap<String, Vec<u8>>;
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MANIFEST: &str = "META-INF/manifest.xml";
const PRES: &str = "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0";
const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'";
const SWITCHES: [&str; 6] =
    ["background-visible", "background-objects-visible", "display-header", "display-footer", "display-date-time", "display-page-number"];
const STAT: &str = " meta:word-count='4'";

fn fixture(version: &str, cache: bool, owner: &str, bag: &str) -> Files {
    let style = format!("<style:style style:family='drawing-page' style:name='Visibility'>{bag}</style:style>");
    let mut common = String::new();
    let mut content_auto = String::new();
    let mut styles_auto = String::new();
    match owner {
        "common" => common = style,
        "content-auto" => content_auto = style,
        "styles-auto" => styles_auto = style,
        "default" => common = format!("<style:default-style style:family='drawing-page'>{bag}</style:default-style>"),
        _ => panic!("invalid synthetic fixture owner"),
    }
    let content = format!(
        "<?xml version='1.0'?>\r\n<office:document-content {NS} office:version='{version}'><!--content survivor--><office:automatic-styles>{content_auto}<style:style style:family='paragraph' style:name='Body'><style:paragraph-properties fo:text-align-last='center'/><style:text-properties fo:font-family='BIZ UDPGothic' fo:font-size='18pt'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Review' draw:master-page-name='Master' xml:id='page1'><draw:frame xml:id='frame1' svg:width='200pt' svg:height='90pt'><draw:text-box><text:p text:style-name='Body'>Signal alpha</text:p>\n<!--paragraph survivor-->\t<text:p text:style-name='Body'>Signal beta</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:styles>{common}</office:styles><office:automatic-styles>{styles_auto}<style:page-layout style:name='Paper'><style:page-layout-properties fo:page-width='640pt' fo:page-height='360pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='Master' style:page-layout-name='Paper'/></office:master-styles></office:document-styles>"
    );
    let meta = format!(
        "<office:document-meta {NS} office:version='{version}'><office:meta>{}</office:meta></office:document-meta>",
        if cache { "<meta:document-statistic meta:word-count='4' meta:page-count='1'/>" } else { "" }
    );
    let settings = format!("<office:document-settings {NS} office:version='{version}'/>");
    let mut files = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        ("settings.xml".into(), settings.into_bytes()),
    ]);
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/>"
    );
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        manifest.push_str(&format!("\r\n<!--manifest survivor--><manifest:file-entry manifest:media-type='text/xml' manifest:full-path='{part}'/>"));
    }
    manifest.push_str("</manifest:manifest>");
    files.insert(MANIFEST.into(), manifest.into_bytes());
    let property_part = if owner == "content-auto" { "content.xml" } else { "styles.xml" };
    assert!(
        String::from_utf8(files[property_part].clone()).unwrap().contains(bag),
        "the exact supplied property bytes must enter the source package"
    );
    files
}

fn bag(attributes: &str) -> String {
    format!("<style:drawing-page-properties {attributes}/>")
}

fn package(files: &Files) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    out.start_file("mimetype", options).unwrap();
    out.write_all(MIME.as_bytes()).unwrap();
    for (name, bytes) in files.iter().filter(|(name, _)| name.as_str() != "mimetype") {
        out.start_file(name, options).unwrap();
        out.write_all(bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn unpack(bytes: &[u8]) -> Files {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = Files::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert!(files.insert(entry.name().into(), bytes).is_none());
    }
    files
}

fn exact_members(bytes: &[u8], expected: &Files) {
    let actual = unpack(bytes);
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>(), "the complete surviving member inventory must be exact");
    for (part, bytes) in &actual {
        assert!(bytes == &expected[part], "all surviving bytes, XML gaps and flag spellings must be exact in {part}");
    }
}

fn mutate(files: &mut Files, part: &str, old: &str, new: &str) {
    let xml = String::from_utf8(files[part].clone()).unwrap();
    assert_eq!(xml.matches(old).count(), 1, "mutation must have exactly one target: {old}");
    files.insert(part.into(), xml.replacen(old, new, 1).into_bytes());
}

fn edit(model: &mut Presentation) {
    let text = Arc::make_mut(&mut model.slides[0]).shapes[0].text.as_mut().unwrap();
    assert_eq!(text.paragraphs.len(), 2);
    text.paragraphs[0].runs[0].text = "ReviewedAlpha".into();
    text.paragraphs[1].runs[0].text = "ReviewedBeta".into();
}

fn semantic(mut model: Presentation) -> Presentation {
    for media in &mut model.media {
        if media.content_type == SOURCE {
            media.data = Arc::new(Vec::new());
        }
    }
    for slide in &mut model.slides {
        let slide = Arc::make_mut(slide);
        slide.tags.retain(|(key, _)| key != "odp:source-range");
        for shape in &mut slide.shapes {
            shape.tags.retain(|(key, _)| key != "odp:source-range");
        }
    }
    model
}

fn check_preserved(files: &Files) {
    let source = package(files);
    let mut model = import(&source).expect("original synthetic package imports");
    assert!(model.tags.iter().any(|(key, _)| key == "odp:warning"), "the passive align-last fixture must select source preservation");
    let untouched = export(&model).expect_err("the source-preserving writer requires a real edit").to_string();
    assert!(
        untouched.contains("no supported text edits; use original recovery"),
        "the preservation proof must succeed before the no-edit guard: {untouched}"
    );
    edit(&mut model);
    let immutable = model.clone();
    let exported = export(&model).expect("supported text edit must preserve the proved flags");
    assert_eq!(model, immutable, "export cannot mutate source or desired model");
    assert_eq!(model.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), source);
    let mut expected = files.clone();
    mutate(&mut expected, "content.xml", "Signal alpha", "ReviewedAlpha");
    mutate(&mut expected, "content.xml", "Signal beta", "ReviewedBeta");
    if String::from_utf8(expected["meta.xml"].clone()).unwrap().contains(STAT) {
        mutate(&mut expected, "meta.xml", STAT, "");
    }
    exact_members(&exported, &expected);
    let mut reopened = import(&exported).expect("actual edited output reimports");
    assert_eq!(semantic(model), semantic(reopened.clone()), "all represented semantics survive the real text edit");
    assert_eq!(reopened.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), exported);
    Arc::make_mut(&mut reopened.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[1].runs[0].text = "FollowupBeta".into();
    mutate(&mut expected, "content.xml", "ReviewedBeta", "FollowupBeta");
    exact_members(&export(&reopened).expect("fresh source proof permits a second edit"), &expected);
}

/// If import rejects malformed/unsupported XML, attach it to a known clean public
/// model so export must independently prove the hostile package before comparison.
fn refused(files: &Files, reason: &str) {
    let source = package(files);
    let mut model = import(&source).unwrap_or_else(|_| import(&package(&fixture("1.3", true, "common", &bag("")))).unwrap());
    assert!(model.tags.iter().any(|(key, _)| key == "odp:warning"), "a hostile source must still select the source-preserving writer");
    edit(&mut model);
    model.media.iter_mut().find(|m| m.content_type == SOURCE).unwrap().data = Arc::new(source);
    let immutable = model.clone();
    let error = export(&model).expect_err("unproved source must refuse").to_string();
    assert!(error.contains(reason), "expected guard {reason:?}, got {error}");
    assert!(!error.contains("changes outside plain slide text"), "source proof must win over model comparison: {error}");
    assert_eq!(model, immutable, "failed proof cannot mutate the model");
}

#[test]
fn independent_mixed_switches_and_raw_entities_survive_both_versions_and_three_named_scopes() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for owner in ["common", "content-auto", "styles-auto"] {
                let attributes = SWITCHES
                    .iter()
                    .enumerate()
                    .map(|(i, local)| format!("presentation:{local}='{}'", if i % 2 == 0 { "tr&#x75;e" } else { "&#102;alse" }))
                    .collect::<Vec<_>>()
                    .join("\n\t");
                check_preserved(&fixture(version, cache, owner, &bag(&attributes)));
            }
        }
    }
}

#[test]
fn independent_default_namespace_property_element_and_locally_rebound_attribute_prefix_keep_raw_bytes() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            let property = format!(
                "<drawing-page-properties xmlns='{STYLE}' xmlns:local='{PRES}' local:display-header = \"&#x74;&#114;&#117;&#101;\" local:display-footer='false'><!--lexical survivor-->\r\n</drawing-page-properties>"
            );
            let mut files = fixture(version, cache, "common", &property);
            mutate(&mut files, "styles.xml", "<office:styles>", "<office:styles xmlns:local='urn:deckcraft:review:foreign'>");
            check_preserved(&files);
        }
    }
}

#[test]
fn independent_boolean_language_and_attribute_size_limits_remain_closed() {
    for version in ["1.2", "1.3"] {
        for local in SWITCHES {
            for token in [
                "1",
                "0",
                "True",
                "FALSE",
                "+true",
                "false,true",
                "true/false",
                "t rue",
                "true&#10;",
                "&#13;false",
                "true\t",
                "true\n",
                "true&#xA0;",
                "truе",
                "urn:review:true",
                "",
            ] {
                refused(
                    &fixture(version, true, "styles-auto", &bag(&format!("presentation:{local}='{token}'"))),
                    "cache unclassified attribute/value",
                );
            }
        }
    }
    for length in [8192, 8193] {
        let files = fixture("1.3", true, "content-auto", &bag(&format!("presentation:display-header='{}'", "t".repeat(length))));
        refused(&files, if length == 8192 { "cache unclassified attribute/value" } else { "XML attributes exceed size limit" });
    }
}

#[test]
fn independent_alias_duplicate_and_unqualified_attributes_do_not_gain_boolean_permission() {
    for local in SWITCHES {
        for (attribute, reason) in [
            (format!("xmlns='{PRES}' {local}='true'"), "cache XML identity/depth"),
            // Known standard namespaces have valid expanded identities, but their
            // same-local attributes still lack drawing-page property permission.
            (format!("xmlns:p='{STYLE}' p:{local}='false'"), "cache unclassified attribute/value"),
            (format!("xmlns:p='urn:deckcraft:review:foreign' p:{local}='false'"), "cache XML identity/depth"),
            (format!("xmlns:Presentation='{PRES}#wrong' Presentation:{local}='true'"), "cache XML identity/depth"),
            (format!("xml:{local}='true'"), "cache unclassified attribute/value"),
        ] {
            refused(&fixture("1.3", true, "common", &bag(&attribute)), reason);
        }
        let duplicate = format!("xmlns:a='{PRES}' xmlns:b='{PRES}' a:{local}='tr&#117;e' b:{local}='true'");
        refused(&fixture("1.2", true, "content-auto", &bag(&duplicate)), "duplicate expanded XML attribute");
        let duplicate = format!("presentation:{local}='false' presentation:{local}='false'");
        let files = fixture("1.3", true, "common", &bag(&duplicate));
        assert!(import(&package(&files)).is_err(), "raw duplicate attributes must be rejected during import");
        refused(&files, "attribute");
    }
}

#[test]
fn independent_family_container_and_part_restrictions_precede_warning_or_model_checks() {
    for family in ["graphic", "presentation", "text", "paragraph"] {
        let mut files = fixture("1.3", true, "common", &bag("presentation:display-footer='true'"));
        mutate(&mut files, "styles.xml", "style:family='drawing-page'", &format!("style:family='{family}'"));
        refused(&files, "cache unclassified XML context");
    }
    for family in ["graphic", "presentation"] {
        let property = "<style:graphic-properties presentation:display-page-number='false'/>";
        let mut files = fixture("1.3", true, "styles-auto", property);
        mutate(&mut files, "styles.xml", "style:family='drawing-page'", &format!("style:family='{family}'"));
        refused(&files, "cache unclassified attribute/value");
    }
    let mut files = fixture("1.3", true, "content-auto", &bag("presentation:display-footer='true'"));
    mutate(
        &mut files,
        "content.xml",
        "<style:style style:family='drawing-page' style:name='Visibility'>",
        "<style:default-style style:family='drawing-page'>",
    );
    mutate(
        &mut files,
        "content.xml",
        "</style:style><style:style style:family='paragraph'",
        "</style:default-style><style:style style:family='paragraph'",
    );
    refused(&files, "cache unclassified XML context");
    for part in ["meta.xml", "settings.xml"] {
        let mut files = fixture("1.3", true, "common", &bag(""));
        let extra = format!(
            "<office:automatic-styles><style:style style:family='drawing-page' style:name='ForeignPart'>{}</style:style></office:automatic-styles>",
            bag("presentation:display-header='true'")
        );
        if part == "meta.xml" {
            mutate(&mut files, part, "</office:meta>", &format!("</office:meta>{extra}"));
        } else {
            mutate(&mut files, part, "office:version='1.3'/>", &format!("office:version='1.3'>{extra}</office:document-settings>"));
        }
        refused(&files, "cache unclassified XML context");
    }
}

#[test]
fn independent_false_switches_do_not_skip_leaf_children_or_unused_definition_guards() {
    for child in [
        "<office:event-listeners/>",
        "<presentation:display-footer/>",
        "<style:drawing-page-properties/>",
        "<draw:image xlink:href='Thumbnails/thumbnail.png'/>",
    ] {
        let property = format!("<style:drawing-page-properties presentation:background-visible='false'>{child}</style:drawing-page-properties>");
        refused(&fixture("1.3", true, "styles-auto", &property), "cache unclassified XML context");
    }
    for attribute in [
        "draw:fill-gradient-name='Hidden'",
        "presentation:transition-style='none'",
        "draw:auto-grow-height='true'",
        "xml:base='file:///'",
        "xlink:href='Thumbnails/thumbnail.png'",
    ] {
        let property = bag(&format!("presentation:background-visible='false' {attribute}"));
        refused(&fixture("1.3", true, "styles-auto", &property), "cache unclassified attribute/value");
    }
    let mut files = fixture("1.3", true, "common", &bag("presentation:background-visible='false'"));
    mutate(&mut files, "styles.xml", "style:name='Visibility'", "style:name='Visibility' style:parent-style-name='Missing'");
    refused(&files, "cache unresolved typed reference");
    let mut files = fixture("1.3", true, "common", &bag("presentation:background-visible='false'"));
    mutate(&mut files, "content.xml", "xml:id='frame1'", "xml:id='page1'");
    refused(&files, "cache ambiguous definition");
}

#[test]
fn independent_false_flags_cannot_hide_thumbnail_dependencies_or_active_siblings() {
    let mut files = fixture("1.3", true, "common", &bag("presentation:background-visible='false'"));
    mutate(
        &mut files,
        "content.xml",
        "</draw:text-box></draw:frame>",
        "</draw:text-box></draw:frame><draw:frame svg:width='1pt' svg:height='1pt'><draw:image xlink:href='Thumbnails/thumbnail.png'/></draw:frame>",
    );
    refused(&files, "cache unsafe part reference");
    for cache in [false, true] {
        let mut files = fixture("1.3", cache, "common", &bag("presentation:background-visible='false'"));
        mutate(&mut files, "content.xml", "</draw:page>", "<office:forms/></draw:page>");
        refused(&files, if cache { "cache unclassified XML context" } else { "unverified XML, active content, or content-derived statistics" });
    }
}

#[test]
fn independent_default_and_used_field_flags_pass_profile_then_reach_the_unchanged_warning_gate() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for local in ["display-header", "display-footer", "display-date-time", "display-page-number"] {
                for value in ["false", "true"] {
                    for owner in ["default", "common", "content-auto"] {
                        let mut files = fixture(version, cache, owner, &bag(&format!("presentation:{local}='{value}'")));
                        match owner {
                            "common" => mutate(&mut files, "styles.xml", "style:name='Master'", "style:name='Master' draw:style-name='Visibility'"),
                            "content-auto" => {
                                mutate(&mut files, "content.xml", "draw:name='Review'", "draw:name='Review' draw:style-name='Visibility'")
                            }
                            _ => {}
                        }
                        let model = import(&package(&files)).expect("standard fields import with a retained warning");
                        assert!(
                            model
                                .tags
                                .iter()
                                .any(|(key, value)| key == "odp:warning" && value.contains("regular master placeholders or inherited visibility"))
                        );
                        refused(&files, "source has unsupported content beyond passive text properties");
                    }
                }
            }
        }
    }
}

#[test]
fn independent_used_background_values_remain_semantic_and_inherited_false_stays_guarded() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for local in ["background-visible", "background-objects-visible"] {
                for value in ["false", "true"] {
                    let mut files = fixture(version, cache, "content-auto", &bag(&format!("presentation:{local}='{value}'")));
                    mutate(&mut files, "content.xml", "draw:name='Review'", "draw:name='Review' draw:style-name='Visibility'");
                    if local == "background-objects-visible" {
                        assert_eq!(import(&package(&files)).unwrap().slides[0].show_master_shapes, value == "true");
                    }
                    check_preserved(&files);
                    for owner in ["common", "default"] {
                        let mut inherited = fixture(version, cache, owner, &bag(&format!("presentation:{local}='{value}'")));
                        if owner == "common" {
                            mutate(&mut inherited, "styles.xml", "style:name='Master'", "style:name='Master' draw:style-name='Visibility'");
                        }
                        if value == "false" {
                            refused(&inherited, "source has unsupported content beyond passive text properties");
                        } else {
                            check_preserved(&inherited);
                        }
                    }
                }
            }
        }
    }
}
