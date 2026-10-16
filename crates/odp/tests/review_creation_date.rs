//! Independent contributor-original fixtures for passive historical timestamps.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{collections::BTreeMap, io::Cursor, io::Read, io::Write, sync::Arc};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/'";
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const DATE: &str = "2032-02-29T09:08:07Z";
const CREATION: &str = "<meta:creation-date>2032-02-29T09:08:07Z</meta:creation-date>";
type Parts = BTreeMap<String, Vec<u8>>;

fn fixture(metadata: &str, cache: bool) -> Parts {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='ReviewP' style:family='paragraph'><style:paragraph-properties fo:keep-together='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Independent'><draw:frame xml:id='frame' svg:width='160pt' svg:height='70pt'><draw:text-box><text:p text:style-name='ReviewP' xml:id='paragraph'>Before</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let counts = if cache { "<meta:document-statistic meta:page-count='1' meta:word-count='2'/>" } else { "" };
    let meta = format!(
        "<office:document-meta {NS} office:version='1.3'><office:meta><!-- independently authored -->{metadata}{counts}<!-- exact tail --></office:meta></office:document-meta>"
    );
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='meta.xml' manifest:media-type='text/xml'/></manifest:manifest>"
    );
    BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        ("META-INF/manifest.xml".into(), manifest.into_bytes()),
    ])
}

fn package(parts: &Parts) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in std::iter::once("mimetype").chain(parts.keys().map(String::as_str).filter(|name| *name != "mimetype")) {
        writer.start_file(name, options).unwrap();
        writer.write_all(&parts[name]).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn unpack(bytes: &[u8]) -> Parts {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = Parts::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert!(parts.insert(entry.name().into(), bytes).is_none());
    }
    parts
}

fn replace(parts: &mut Parts, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(parts[part].clone()).unwrap();
    assert!(text.contains(old));
    parts.insert(part.into(), text.replace(old, new).into_bytes());
}

fn edit(presentation: &mut Presentation) {
    Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "After & Ω".into();
}

fn preserved(parts: &Parts, cache: bool) -> Presentation {
    let original_parts = parts.clone();
    let source = package(parts);
    let baseline = import(&source).unwrap();
    assert!(baseline.tags.iter().any(|(key, _)| key == "odp:warning"), "fixture must enter source preservation");
    let unchanged_error = export(&baseline).expect_err("the unchanged source requires original recovery").to_string();
    assert!(
        unchanged_error.contains("no supported text edits"),
        "fixture must pass passive proof and audit before edit detection: {unchanged_error}"
    );
    let mut desired = baseline.clone();
    edit(&mut desired);
    let desired = deckcraft_format::load(&deckcraft_format::save(&desired).unwrap()).unwrap();
    let frozen = desired.clone();
    let output = export(&desired).unwrap();
    assert_eq!(desired, frozen, "export changed its input model");
    let actual = unpack(&output);
    let mut expected = parts.clone();
    // The supported patch writes each space as an explicitly bound ODF space node.
    // Fix the entire changed body independently; every surrounding byte stays exact.
    replace(
        &mut expected,
        "content.xml",
        ">Before<",
        ">After<dcpatch:s xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>&amp;<dcpatch:s xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>Ω<",
    );
    if cache {
        replace(&mut expected, "meta.xml", " meta:word-count='2'", "");
    }
    assert_eq!(actual, expected, "whole surviving part map differs");
    assert_eq!(*parts, original_parts);
    assert_eq!(baseline.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    let reopened = import(&output).unwrap();
    assert_eq!(reopened.props, baseline.props);
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "After & Ω");
    reopened
}

/// A fallback presentation keeps malformed synthetic sources on the passive export path.
fn refused(parts: &Parts) -> String {
    let source = package(parts);
    let mut desired = import(&source).unwrap_or_else(|_| import(&package(&fixture("", false))).unwrap());
    edit(&mut desired);
    desired.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(source);
    let frozen = desired.clone();
    let error = export(&desired).expect_err("unproved source semantics must refuse").to_string();
    assert_eq!(desired, frozen, "refusal changed its input model");
    error
}

#[test]
fn raw_namespace_aliases_comments_cdata_and_entity_spellings_survive_native_reopen() {
    let metadata = "<m:creation-date xmlns:m='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'>\r\n<![CDATA[2032-02-29T09:]]><!--split scalar-->08:&#48;7Z&#xD;&#xA;</m:creation-date>";
    for cache in [false, true] {
        let mut parts = fixture(metadata, cache);
        replace(&mut parts, "meta.xml", "office:document-meta", "o:document-meta");
        replace(&mut parts, "meta.xml", "office:meta", "o:meta");
        replace(&mut parts, "meta.xml", "office:version", "o:version");
        replace(&mut parts, "meta.xml", "xmlns:office=", "xmlns:o=");
        let reopened = preserved(&parts, cache);
        assert!(reopened.props.created.contains(DATE));
        assert!(reopened.props.modified.is_empty());
    }
}

#[test]
fn decoded_limit_is_global_across_hundreds_of_entity_comment_and_cdata_events() {
    // 19 date/time bytes + dot + 235 fractional digits + Z = 256 decoded bytes.
    let fraction: String = (0..235).map(|index| if index % 2 == 0 { "&#48;<!--gap-->" } else { "<![CDATA[0]]>" }).collect();
    let scalar = format!("2032-02-29T09:08:07.{fraction}Z");
    for cache in [false, true] {
        preserved(&fixture(&format!("<meta:creation-date>{scalar}</meta:creation-date>"), cache), cache);
        let error = refused(&fixture(&format!("<meta:creation-date>{scalar}&#32;</meta:creation-date>"), cache));
        assert!(error.contains("creation-date scalar size"), "{error}");
    }
}

#[test]
fn only_outer_xml_whitespace_is_accepted_without_lexical_rewriting() {
    for cache in [false, true] {
        for scalar in ["&#9;2032-02-29T09:08:07Z&#13;", "<![CDATA[ \t2032-02-29T09:08:07Z\r\n]]>"] {
            preserved(&fixture(&format!("<meta:creation-date>{scalar}</meta:creation-date>"), cache), cache);
        }
        for scalar in [
            "2032-02-29T09:&#9;08:07Z",
            "2032-02-29T09:08:07Z&#xA0;",
            "2032-02-29T09:08:07&#x85;Z",
            "2032-02-29t09:08:07Z",
            "2032-02-29T09:08:07z",
            "2032-02-29T09:08:07.Z",
        ] {
            let error = refused(&fixture(&format!("<meta:creation-date>{scalar}</meta:creation-date>"), cache));
            assert!(error.contains("creation-date"), "{error}");
        }
    }
}

#[test]
fn aliases_do_not_hide_duplicate_scalars_attributes_or_structured_content() {
    let alias = "<creation-date xmlns='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'>2032-02-29T09:08:07Z</creation-date>";
    for cache in [false, true] {
        preserved(&fixture(alias, cache), cache);
        for metadata in [
            format!("{CREATION}{alias}"),
            format!("<meta:creation-date id='x'>{DATE}</meta:creation-date>"),
            format!("<meta:creation-date xml:id='x'>{DATE}</meta:creation-date>"),
            format!("<meta:creation-date xmlns:a='http://www.w3.org/XML/1998/namespace' a:lang='en'>{DATE}</meta:creation-date>"),
            format!("<meta:creation-date xmlns:a='urn:review' a:value='x'>{DATE}</meta:creation-date>"),
            format!("<meta:creation-date>{DATE}<text:s/></meta:creation-date>"),
            format!("<meta:creation-date><meta:creation-date>{DATE}</meta:creation-date></meta:creation-date>"),
        ] {
            refused(&fixture(&metadata, cache));
        }
    }
}

#[test]
fn exact_namespace_owner_container_and_part_are_all_required() {
    for cache in [false, true] {
        for metadata in [
            format!("<creation-date>{DATE}</creation-date>"),
            format!("<meta:creation-date xmlns:meta='urn:review:metadata'>{DATE}</meta:creation-date>"),
            format!("<dc:description>{CREATION}</dc:description>"),
        ] {
            refused(&fixture(&metadata, cache));
        }
        let mut duplicate_owner = fixture(CREATION, cache);
        replace(
            &mut duplicate_owner,
            "meta.xml",
            "</office:meta>",
            "</office:meta><m:meta xmlns:m='urn:oasis:names:tc:opendocument:xmlns:office:1.0'/>",
        );
        refused(&duplicate_owner);
        let mut owner_attr = fixture(CREATION, cache);
        replace(&mut owner_attr, "meta.xml", "<office:meta>", "<office:meta xml:lang='en'>");
        refused(&owner_attr);
        let mut foreign_owner = fixture(CREATION, cache);
        replace(&mut foreign_owner, "meta.xml", "<office:meta>", "<office:meta xmlns:office='urn:review:office'>");
        refused(&foreign_owner);
        let mut wrong_part = fixture("", cache);
        replace(&mut wrong_part, "content.xml", "<office:body>", &format!("<office:meta>{CREATION}</office:meta><office:body>"));
        refused(&wrong_part);
        let mut wrong_root = fixture("", cache);
        replace(&mut wrong_root, "meta.xml", "<office:meta>", &format!("{CREATION}<office:meta>"));
        refused(&wrong_root);
    }
}

#[test]
fn document_modification_claims_refuse_even_empty_or_non_scalar_and_alias_encoded() {
    for cache in [false, true] {
        for modified in [
            "<dc:date/>",
            "<dc:date>invalid</dc:date>",
            "<dc:date><text:span/></dc:date>",
            "<date xmlns='http://purl.org/dc/elements/1.1/'>2032-02-29T09:08:07Z</date>",
            "<d:date xmlns:d='http://purl.org/dc/elements/1.1/' xml:lang='en'>2032-02-29T09:08:07Z</d:date>",
        ] {
            let error = refused(&fixture(&format!("{CREATION}{modified}"), cache));
            if !cache {
                assert!(error.contains("document modification date"), "{error}");
            }
        }
    }
}

#[test]
fn creation_admission_keeps_processing_instruction_doctype_and_reference_guards() {
    for cache in [false, true] {
        for metadata in [
            format!("<meta:creation-date>{DATE}<?review hidden?></meta:creation-date>"),
            format!("{CREATION}<dc:title xml:base='https://example.invalid/'>Title</dc:title>"),
            format!("{CREATION}<dc:title xmlns:xlink='http://www.w3.org/1999/xlink' xlink:href='https://example.invalid/'>Title</dc:title>"),
        ] {
            refused(&fixture(&metadata, cache));
        }
        let mut doctype = fixture(CREATION, cache);
        replace(&mut doctype, "meta.xml", "<office:document-meta", "<!DOCTYPE review><office:document-meta");
        refused(&doctype);
    }
}

#[test]
fn source_time_is_not_replaced_by_model_properties_and_refusal_is_immutable() {
    for cache in [false, true] {
        for (created, modified) in [("2033-01-01T00:00:00Z", ""), (DATE, "2033-01-01T00:00:00Z"), ("", "")] {
            let source = package(&fixture(CREATION, cache));
            let mut desired = import(&source).unwrap();
            edit(&mut desired);
            desired.props.created = created.into();
            desired.props.modified = modified.into();
            let frozen = desired.clone();
            let error = export(&desired).unwrap_err().to_string();
            assert!(error.contains("changes outside plain slide text"), "{error}");
            assert_eq!(desired, frozen);
            assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
        }
    }
}

#[test]
fn annotation_creation_time_remains_separate_from_document_modification_time() {
    let mut parts = fixture(CREATION, false);
    replace(
        &mut parts,
        "content.xml",
        "<draw:frame",
        "<office:annotation><dc:date>2024-02-29T24:00:00Z</dc:date><text:p>Independent note</text:p></office:annotation><draw:frame",
    );
    let baseline = import(&package(&parts)).unwrap();
    assert!(baseline.props.modified.is_empty());
    assert_eq!(baseline.slides[0].comments[0].date, "2024-02-29T24:00:00Z");
    assert_eq!(baseline.props.created, DATE);
    assert_eq!(baseline.slides[0].comments[0].comment_anchor(), deckcraft_model::comments::CommentAnchor::Unknown);
    assert!(
        baseline
            .tags
            .iter()
            .any(|(key, value)| key == "odp:warning" && value.starts_with("ODF annotation has no verified shape text or neutral floating holder"))
    );
    let mut desired = baseline.clone();
    edit(&mut desired);
    let desired = deckcraft_format::load(&deckcraft_format::save(&desired).unwrap()).unwrap();
    let frozen = desired.clone();
    let error = match export(&desired) {
        Err(error) => error,
        Ok(_) => panic!("an unproved source annotation owner must remain guarded"),
    };
    assert!(error.to_string().contains("unsupported content beyond passive text properties"), "{error}");
    assert_eq!(desired, frozen);
    assert_eq!(desired.props.created, DATE);
    assert!(desired.props.modified.is_empty());
    assert_eq!(desired.slides[0].comments[0].date, "2024-02-29T24:00:00Z");
    assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), package(&parts));
    // A separately authored native slide comment has a proven owner and can
    // interchange without turning its creation time into document modification.
    let mut authored = Presentation::default();
    authored.props.created = DATE.into();
    let mut comment = baseline.slides[0].comments[0].clone();
    comment.set_comment_anchor(deckcraft_model::comments::CommentAnchor::Slide);
    Arc::make_mut(&mut authored.slides[0]).comments = vec![comment];
    let original = authored.clone();
    let reopened = import(&export(&authored).unwrap()).unwrap();
    assert_eq!(reopened.props.created, DATE);
    assert!(reopened.props.modified.is_empty());
    assert_eq!(reopened.slides[0].comments[0].date, "2024-02-29T24:00:00Z");
    assert_eq!(reopened.slides[0].comments[0].text, "Independent note");
    assert_eq!(authored, original);
}
