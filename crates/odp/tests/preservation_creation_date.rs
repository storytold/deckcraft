//! Contributor-original preservation fixtures for historical creation metadata.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/'";
const THUMB: &str = "Thumbnails/thumbnail.png";
const MANIFEST: &str = "META-INF/manifest.xml";
const THUMB_OWNER: &str = "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>";
const COUNTS: &str = "<meta:document-statistic meta:word-count='1' meta:character-count='8' meta:page-count='1'/>";

fn fixture(version: &str, creation: &str, cache: bool) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='{version}'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Page' draw:master-page-name='M' xml:id='page'><draw:frame xml:id='frame' svg:width='200pt' svg:height='80pt'><draw:text-box><text:p text:style-name='P' xml:id='body'>Original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='720pt' fo:page-height='405pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
    );
    let counts = if cache { COUNTS } else { "" };
    let meta = format!(
        "<office:document-meta {NS} office:version='{version}'><office:meta><!--before Ω--><dc:title>Own metadata</dc:title>{creation}<!--after-->{counts}</office:meta></office:document-meta>"
    );
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
    ]);
    if cache {
        files.insert(THUMB.into(), deckcraft_render::Image { width: 1, height: 1, pixels: vec![25, 85, 125, 255] }.to_png());
    }
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/>"
    );
    for name in files.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name.ends_with(".png") { "image/png" } else { "text/xml" };
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>"));
    }
    manifest.push_str("<!--tail--></manifest:manifest>");
    files.insert(MANIFEST.into(), manifest.into_bytes());
    files
}
fn zip(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(MIME.as_bytes()).unwrap();
    for (name, bytes) in files.iter().filter(|(name, _)| name.as_str() != "mimetype") {
        zip.start_file(name, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        files.insert(file.name().into(), bytes);
    }
    files
}
fn edit(p: &mut Presentation) {
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Changed".into();
}
fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, before: &str, after: &str) {
    let text = String::from_utf8(files[part].clone()).unwrap();
    assert!(text.contains(before));
    files.insert(part.into(), text.replace(before, after).into_bytes());
}
fn preserved(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let source = zip(files);
    let untouched = source.clone();
    let mut desired = import(&source).unwrap();
    let created = desired.props.created.clone();
    assert!(desired.props.modified.is_empty());
    assert!(export(&desired).is_err(), "unchanged source still requires original recovery");
    edit(&mut desired);
    desired = deckcraft_format::load(&deckcraft_format::save(&desired).unwrap()).unwrap();
    assert_eq!(desired.props.created, created);
    let output = export(&desired).unwrap();
    let after = parts(&output);
    assert_eq!(after.len() + usize::from(files.contains_key(THUMB)), files.len());
    for (name, bytes) in files {
        let expected = match name.as_str() {
            THUMB => {
                assert!(!after.contains_key(THUMB));
                continue;
            }
            "content.xml" => String::from_utf8(bytes.clone()).unwrap().replace(">Original<", ">Changed<").into_bytes(),
            MANIFEST => String::from_utf8(bytes.clone()).unwrap().replace(THUMB_OWNER, "").into_bytes(),
            "meta.xml" if files.contains_key(THUMB) => {
                String::from_utf8(bytes.clone()).unwrap().replace(" meta:word-count='1'", "").replace(" meta:character-count='8'", "").into_bytes()
            }
            _ => bytes.clone(),
        };
        assert_eq!(after[name], expected, "unexpected survivor bytes in {name}");
    }
    let reopened = import(&output).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "Changed");
    assert_eq!(reopened.props.created, created);
    assert!(reopened.props.modified.is_empty());
    let mut second = reopened;
    Arc::make_mut(&mut second.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Again".into();
    let next = parts(&export(&second).unwrap());
    for (name, bytes) in &after {
        let expected = if name == "content.xml" {
            String::from_utf8(bytes.clone()).unwrap().replace(">Changed<", ">Again<").into_bytes()
        } else {
            bytes.clone()
        };
        assert_eq!(next[name], expected, "fresh proof changed survivor {name}");
    }
    assert_eq!(source, untouched);
    let backup = desired.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap();
    assert_eq!(backup.data.as_slice(), source);
    assert_eq!(import(&backup.data).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Original");
    output
}
fn refused(files: &BTreeMap<String, Vec<u8>>) -> String {
    let source = zip(files);
    let mut desired = import(&source).unwrap_or_else(|_| import(&zip(&fixture("1.3", "", false))).unwrap());
    edit(&mut desired);
    desired.media.iter_mut().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data = Arc::new(source);
    export(&desired).expect_err("unproved timestamp semantics must refuse").to_string()
}

#[test]
fn creation_time_survives_real_edits_native_persistence_and_exact_survivor_maps() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            let files = fixture(version, "<meta:creation-date>2024-02-29T12:34:56.123+14:00</meta:creation-date>", cache);
            let output = preserved(&files);
            if let Ok(dir) = std::env::var("DECKCRAFT_ODP_CREATION_QA_DIR") {
                let dir = std::path::Path::new(&dir);
                std::fs::create_dir_all(dir).unwrap();
                for (kind, parts) in [("source", files), ("output", parts(&output))] {
                    for (name, bytes) in parts.into_iter().filter(|(name, _)| name.ends_with(".xml")) {
                        std::fs::write(dir.join(format!("{version}-{cache}-{kind}-{}", name.replace('/', "-"))), bytes).unwrap();
                    }
                }
            }
        }
    }
}

#[test]
fn bounded_datetime_lexicals_aliases_entities_comments_and_cdata_remain_raw() {
    for creation in [
        "",
        "<meta:creation-date>0001-01-01T00:00:00</meta:creation-date>",
        "<meta:creation-date>2024-02-29T24:00:00.000-14:00</meta:creation-date>",
        "<meta:creation-date>9999-12-31T23:59:59Z</meta:creation-date>",
        "<m:creation-date xmlns:m='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'> \t2026-10-16T00:00:00-00:00&#13;&#10;</m:creation-date>",
        "<creation-date xmlns='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'>&#50;026-10-16T00:00:00+00:00</creation-date>",
        "<meta:creation-date><![CDATA[2026-10-16T]]><!--retained scalar gap-->12:00:00Z</meta:creation-date>",
    ] {
        for cache in [false, true] {
            preserved(&fixture("1.3", creation, cache));
        }
    }
}

#[test]
fn empty_malformed_extended_and_oversized_creation_values_refuse_with_or_without_cache() {
    let oversized = format!("2026-10-16T00:00:00.{}Z", "0".repeat(256));
    for value in [
        "",
        " \t\r\n",
        "2026-10-16",
        "1900-02-29T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "0000-01-01T00:00:00Z",
        "-0001-01-01T00:00:00Z",
        "10000-01-01T00:00:00Z",
        "2026-10-16T24:00:00.1Z",
        "2026-10-16T12:60:00Z",
        "2026-10-16T12:00:60Z",
        "2026-10-16T12:00:00+14:01",
        "2026-10-16T12:00:00-15:00",
        &oversized,
    ] {
        for cache in [false, true] {
            let error = refused(&fixture("1.3", &format!("<meta:creation-date>{value}</meta:creation-date>"), cache));
            assert!(error.contains("creation-date"), "unexpected guard: {error}");
        }
    }
}

#[test]
fn decoded_datetime_byte_cap_is_checked_before_scalar_copy() {
    let exact = format!("2026-10-16T12:00:00.{}Z", "0".repeat(235));
    assert_eq!(exact.len(), 256);
    for cache in [false, true] {
        preserved(&fixture("1.3", &format!("<meta:creation-date>{exact}</meta:creation-date>"), cache));
        let oversized = format!("{exact} ");
        let error = refused(&fixture("1.3", &format!("<meta:creation-date>{oversized}</meta:creation-date>"), cache));
        assert!(error.contains("creation-date scalar size"), "{error}");
    }
}

#[test]
fn creation_metadata_cardinality_attributes_and_children_refuse() {
    for creation in [
        "<meta:creation-date>2026-10-16T00:00:00Z</meta:creation-date><meta:creation-date>2026-10-16T00:00:00Z</meta:creation-date>",
        "<meta:creation-date xml:lang='en'>2026-10-16T00:00:00Z</meta:creation-date>",
        "<meta:creation-date meta:custom='x'>2026-10-16T00:00:00Z</meta:creation-date>",
        "<meta:creation-date><text:span>2026-10-16T00:00:00Z</text:span></meta:creation-date>",
        "<meta:creation-date>2026-10-16T00:00:00Z&#0;</meta:creation-date>",
    ] {
        for cache in [false, true] {
            refused(&fixture("1.3", creation, cache));
        }
    }
}

#[test]
fn modification_time_is_not_silently_kept_fabricated_rewritten_or_removed() {
    for cache in [false, true] {
        for metadata in [
            "<dc:date>2026-10-16T12:00:00Z</dc:date>",
            "<meta:creation-date>2020-01-01T00:00:00Z</meta:creation-date><dc:date>2026-10-16T12:00:00Z</dc:date>",
            "<d:date xmlns:d='http://purl.org/dc/elements/1.1/'>2026-10-16T12:00:00Z</d:date>",
        ] {
            let error = refused(&fixture("1.3", metadata, cache));
            if !cache {
                assert!(error.contains("modification date"), "missing no-cache timestamp guard: {error}");
            }
        }
    }
}

#[test]
fn source_creation_property_changes_remain_outside_plain_text_edit_ownership() {
    let files = fixture("1.3", "<meta:creation-date>2020-01-01T00:00:00Z</meta:creation-date>", true);
    let mut desired = import(&zip(&files)).unwrap();
    edit(&mut desired);
    desired.props.created = "2026-10-16T00:00:00Z".into();
    assert!(export(&desired).unwrap_err().to_string().contains("changes outside plain slide text"));
}

#[test]
fn foreign_namespaces_wrong_owners_and_duplicate_meta_containers_do_not_gain_permission() {
    let value = "<meta:creation-date>2026-10-16T12:00:00Z</meta:creation-date>";
    for cache in [false, true] {
        refused(&fixture("1.3", "<meta:creation-date xmlns:meta='urn:foreign'>2026-10-16T12:00:00Z</meta:creation-date>", cache));
        let mut nested = fixture("1.3", "", cache);
        replace(&mut nested, "meta.xml", "<dc:title>Own metadata</dc:title>", &format!("<dc:title>Own metadata{value}</dc:title>"));
        refused(&nested);
        let mut duplicate = fixture("1.3", value, cache);
        replace(&mut duplicate, "meta.xml", "</office:meta>", "</office:meta><office:meta/>");
        refused(&duplicate);
        let mut wrong_part = fixture("1.3", "", cache);
        replace(&mut wrong_part, "styles.xml", "<office:styles/>", &format!("<office:styles>{value}</office:styles>"));
        refused(&wrong_part);
        let mut wrong_root = fixture("1.3", "", cache);
        replace(&mut wrong_root, "meta.xml", "</office:meta>", &format!("{value}</office:meta>"));
        replace(&mut wrong_root, "meta.xml", "<office:meta>", "<office:meta xml:id='unverified'>");
        refused(&wrong_root);
    }
}
