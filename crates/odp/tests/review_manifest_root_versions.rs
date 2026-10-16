//! Independent contributor-original fixtures for the closed manifest root-owner extension.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const MANIFEST: &str = "META-INF/manifest.xml";
const THUMB: &str = "Thumbnails/thumbnail.png";
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MNS: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'";

fn fixture(version: &str, owner_version: Option<&str>, preview: bool) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='{version}'><office:automatic-styles><style:style style:name='Pass' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Review' draw:master-page-name='Master'><draw:frame svg:width='120pt' svg:height='45pt'><draw:text-box><text:p text:style-name='Pass'>Before</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:styles/><office:automatic-styles><style:page-layout style:name='Paper'><style:page-layout-properties fo:page-width='360pt' fo:page-height='240pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='Master' style:page-layout-name='Paper'/></office:master-styles></office:document-styles>"
    );
    let meta = format!(
        "<office:document-meta {NS} office:version='{version}'><office:meta><!--retained metadata gap--><meta:document-statistic meta:word-count='1' meta:page-count='1'/></office:meta></office:document-meta>"
    );
    let settings = format!("<office:document-settings {NS} office:version='{version}'></office:document-settings>");
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        ("settings.xml".into(), settings.into_bytes()),
    ]);
    if preview {
        files.insert(THUMB.into(), deckcraft_render::Image { width: 1, height: 1, pixels: vec![73, 29, 157, 255] }.to_png());
    }
    let optional = owner_version.map_or(String::new(), |v| format!(" pkg:version='{v}'"));
    let mut xml = format!(
        "<pkg:manifest xmlns:pkg='{MNS}' pkg:version='{version}'>\n<!--before root owner-->\n<pkg:file-entry pkg:full-path='/' pkg:media-type='{MIME}'{optional}/>\n<!--after root owner-->\n"
    );
    for name in files.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name == THUMB { "image/png" } else { "text/xml" };
        xml.push_str(&format!("<pkg:file-entry pkg:full-path='{name}' pkg:media-type='{mime}'/>\n"));
    }
    xml.push_str("<!--retained trailing gap--></pkg:manifest>");
    files.insert(MANIFEST.into(), xml.into_bytes());
    files
}

fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, old: &str, new: &str) {
    let xml = String::from_utf8(files[part].clone()).unwrap();
    assert!(xml.contains(old), "mutation must change {part}: {old}");
    files.insert(part.into(), xml.replace(old, new).into_bytes());
}

fn package(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("mimetype", options).unwrap();
    writer.write_all(MIME.as_bytes()).unwrap();
    for (name, data) in files.iter().filter(|(name, _)| name.as_str() != "mimetype") {
        writer.start_file(name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn unpack(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        files.insert(file.name().into(), data);
    }
    files
}

fn edit(source: &[u8]) -> Presentation {
    let mut presentation = import(source).unwrap();
    Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "After".into();
    presentation
}

fn assert_export_exact(files: &BTreeMap<String, Vec<u8>>) {
    let source = package(files);
    let presentation = edit(&source);
    let unchanged = presentation.clone();
    let output = export(&presentation).expect("closed root-owner version package must export");
    assert_eq!(presentation, unchanged, "successful export mutates neither model nor recovery source");
    let mut expected = files.clone();
    replace(&mut expected, "content.xml", ">Before</text:p>", ">After</text:p>");
    replace(&mut expected, "meta.xml", " meta:word-count='1'", "");
    if expected.remove(THUMB).is_some() {
        replace(&mut expected, MANIFEST, &format!("<pkg:file-entry pkg:full-path='{THUMB}' pkg:media-type='image/png'/>"), "");
    }
    assert_eq!(unpack(&output), expected, "every payload and all surviving manifest gaps must be byte exact");
    let reopened = import(&output).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "After");
    assert_eq!(presentation.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), source);
}

fn assert_refused(files: &BTreeMap<String, Vec<u8>>) {
    let source = package(files);
    // Even XML that import rejects must reach source-package proof when attached to a
    // separately valid baseline. Refusal cannot depend on a missing ordinary text edit.
    let mut presentation = match import(&source) {
        Ok(_) => edit(&source),
        Err(_) => edit(&package(&fixture("1.3", Some("1.3"), false))),
    };
    presentation.media.iter_mut().find(|m| m.content_type == SOURCE).unwrap().data = Arc::new(source.clone());
    let unchanged = presentation.clone();
    for _ in 0..2 {
        let error = export(&presentation).unwrap_err().to_string();
        assert!(error.contains("cache") || error.contains("manifest") || error.contains("XML"), "wrong refusal: {error}");
        assert_eq!(presentation, unchanged, "failed export must preserve every model field");
        assert_eq!(presentation.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), source);
    }
}

#[test]
fn both_versions_and_absent_owner_versions_preserve_exact_payloads() {
    for version in ["1.2", "1.3"] {
        for owner in [None, Some(version)] {
            for preview in [false, true] {
                assert_export_exact(&fixture(version, owner, preview));
            }
        }
    }
}

#[test]
fn root_version_alias_entities_attribute_order_and_whitespace_are_retained() {
    for version in ["1.2", "1.3"] {
        let mut files = fixture(version, Some(version), true);
        replace(
            &mut files,
            MANIFEST,
            &format!("<pkg:file-entry pkg:full-path='/' pkg:media-type='{MIME}' pkg:version='{version}'/>"),
            &format!(
                "<entry:file-entry xmlns:entry='{MNS}' xmlns:v='{MNS}'\n v:version = \"&#49;.{}\"\t entry:media-type='{MIME}' entry:full-path='&#x2f;'> \r\n </entry:file-entry>",
                if version == "1.2" { "2" } else { "3" }
            ),
        );
        assert_export_exact(&files);
    }
}

#[test]
fn default_element_namespace_does_not_change_standard_attribute_identity() {
    let mut files = fixture("1.3", Some("1.3"), false);
    replace(
        &mut files,
        MANIFEST,
        &format!("<pkg:file-entry pkg:full-path='/' pkg:media-type='{MIME}' pkg:version='1.3'/>"),
        &format!("<file-entry xmlns='{MNS}' pkg:version='1.3' pkg:media-type='{MIME}' pkg:full-path='/'/>"),
    );
    assert_export_exact(&files);
}

#[test]
fn malformed_unknown_and_manifest_mismatched_owner_versions_refuse() {
    for value in ["", " ", "1.3 ", " 1.3", "1.03", "1.4", "1.2", "1.3&#x9;", "&#x31;.2"] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(
            &mut files,
            MANIFEST,
            &format!("pkg:media-type='{MIME}' pkg:version='1.3'"),
            &format!("pkg:media-type='{MIME}' pkg:version='{value}'"),
        );
        assert_refused(&files);
    }
}

#[test]
fn root_owner_requires_exact_path_and_presentation_mime() {
    for (path, mime) in [
        ("./", MIME),
        ("//", MIME),
        ("/ ", MIME),
        ("content.xml", MIME),
        ("/", "text/xml"),
        ("/", "application/vnd.oasis.opendocument.presentation-template"),
    ] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(
            &mut files,
            MANIFEST,
            &format!("pkg:full-path='/' pkg:media-type='{MIME}'"),
            &format!("pkg:full-path='{path}' pkg:media-type='{mime}'"),
        );
        assert_refused(&files);
    }
}

#[test]
fn every_nonroot_owner_still_rejects_the_optional_third_attribute() {
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml", THUMB] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(&mut files, MANIFEST, &format!("pkg:full-path='{part}'"), &format!("pkg:version='1.3' pkg:full-path='{part}'"));
        assert_refused(&files);
    }
}

#[test]
fn foreign_unqualified_and_rebound_versions_never_count_as_standard_version() {
    for attribute in [
        "xmlns:f='urn:review:foreign' f:version='1.3'",
        "version='1.3'",
        "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' office:version='1.3'",
    ] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(&mut files, MANIFEST, &format!("pkg:media-type='{MIME}' pkg:version='1.3'"), &format!("pkg:media-type='{MIME}' {attribute}"));
        assert_refused(&files);
    }
    let mut files = fixture("1.3", Some("1.3"), true);
    replace(&mut files, MANIFEST, "<pkg:file-entry pkg:full-path='/'", "<pkg:file-entry xmlns:pkg='urn:review:foreign' pkg:full-path='/'");
    assert_refused(&files);
}

#[test]
fn standard_version_does_not_license_a_fourth_attribute_or_child() {
    for extra in ["pkg:size='0'", "xmlns:f='urn:review:foreign' f:version='1.3'", "version='1.3'", "xml:lang='en'"] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(
            &mut files,
            MANIFEST,
            &format!("pkg:media-type='{MIME}' pkg:version='1.3'"),
            &format!("pkg:media-type='{MIME}' pkg:version='1.3' {extra}"),
        );
        assert_refused(&files);
    }
    for child in ["nonwhitespace", "<pkg:encryption-data/>", "<pkg:file-entry pkg:full-path='nested' pkg:media-type='text/xml'/>"] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(
            &mut files,
            MANIFEST,
            &format!("pkg:media-type='{MIME}' pkg:version='1.3'/>"),
            &format!("pkg:media-type='{MIME}' pkg:version='1.3'>{child}</pkg:file-entry>"),
        );
        assert_refused(&files);
    }
}

#[test]
fn duplicate_root_owners_and_duplicate_expanded_version_attributes_refuse() {
    for duplicate in [
        format!("<pkg:file-entry pkg:full-path='&#47;' pkg:media-type='{MIME}' pkg:version='1.3'/>"),
        format!("<other:file-entry xmlns:other='{MNS}' other:full-path='/' other:media-type='{MIME}'/>"),
    ] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(&mut files, MANIFEST, "<!--after root owner-->", &duplicate);
        assert_refused(&files);
    }
    let mut files = fixture("1.3", Some("1.3"), true);
    replace(
        &mut files,
        MANIFEST,
        &format!("pkg:media-type='{MIME}' pkg:version='1.3'"),
        &format!("pkg:media-type='{MIME}' pkg:version='1.3' xmlns:other='{MNS}' other:version='1.3'"),
    );
    assert_refused(&files);
}

#[test]
fn owner_version_requires_all_fixed_part_versions_but_absence_keeps_old_profile() {
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(&mut files, part, "office:version='1.3'", "office:version='1.2'");
        assert_refused(&files);
        let mut legacy = fixture("1.3", None, true);
        replace(&mut legacy, part, "office:version='1.3'", "office:version='1.2'");
        assert_export_exact(&legacy);
    }
}

#[test]
fn fixed_part_version_aliases_and_entities_are_semantic_but_missing_or_foreign_refuse() {
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        let mut files = fixture("1.2", Some("1.2"), true);
        replace(&mut files, part, "office:version='1.2'", "xmlns:v='urn:oasis:names:tc:opendocument:xmlns:office:1.0' v:version='&#49;.2'");
        assert_export_exact(&files);
        for replacement in ["", "xmlns:v='urn:review:foreign' v:version='1.2'", "version='1.2'"] {
            let mut files = fixture("1.2", Some("1.2"), true);
            replace(&mut files, part, "office:version='1.2'", replacement);
            assert_refused(&files);
        }
    }
}

#[test]
fn optional_version_does_not_weaken_thumbnail_or_unknown_dependency_proofs() {
    let mut corrupt = fixture("1.3", Some("1.3"), true);
    corrupt.insert(THUMB.into(), b"not a PNG".to_vec());
    assert_refused(&corrupt);
    let mut referenced = fixture("1.3", Some("1.3"), true);
    replace(
        &mut referenced,
        "styles.xml",
        "<office:styles/>",
        &format!("<office:styles><draw:fill-image draw:name='Unused' xlink:href='{THUMB}'/></office:styles>"),
    );
    assert_refused(&referenced);
    let mut unknown = fixture("1.3", Some("1.3"), true);
    replace(
        &mut unknown,
        "styles.xml",
        "<office:styles/>",
        "<office:styles><style:style style:name='Unknown' style:family='text'><style:text-properties fo:letter-spacing='1pt'/></style:style></office:styles>",
    );
    assert_refused(&unknown);
    let source = package(&fixture("1.3", Some("1.3"), true));
    let unchanged = import(&source).unwrap();
    assert!(export(&unchanged).is_err(), "root version must not license no-edit cache deletion");
}

#[test]
fn schema_valid_empty_settings_roots_preserve_all_source_bytes() {
    for version in ["1.2", "1.3"] {
        for owner in [None, Some(version)] {
            let mut files = fixture(version, owner, true);
            replace(
                &mut files,
                "settings.xml",
                "></office:document-settings>",
                ">\r\n<!--empty settings document has no configuration content-->\t</office:document-settings>",
            );
            assert_export_exact(&files);
        }
    }
}

#[test]
fn legacy_single_empty_settings_child_keeps_existing_profile_behavior() {
    // Compatibility only: an empty office:settings child is outside the OASIS schema's
    // one-or-more configuration requirement, but the previous closed profile allowed it.
    for version in ["1.2", "1.3"] {
        for owner in [None, Some(version)] {
            let mut files = fixture(version, owner, true);
            replace(&mut files, "settings.xml", "></office:document-settings>", "><office:settings/></office:document-settings>");
            assert_export_exact(&files);
        }
    }
}

#[test]
fn empty_settings_root_exception_does_not_admit_settings_semantics_or_other_roots() {
    for inside in [
        "not whitespace",
        "<office:settings/><office:settings/>",
        "<office:settings/><alias:settings xmlns:alias='urn:oasis:names:tc:opendocument:xmlns:office:1.0'/>",
        "<office:meta/>",
        "<f:settings xmlns:f='urn:review:foreign'/>",
        "<office:settings xmlns:office='urn:review:foreign'/>",
        "<settings/>",
        "<office:settings>not whitespace</office:settings>",
        "<office:settings office:version='1.3'/>",
        "<office:settings xmlns:config='urn:oasis:names:tc:opendocument:xmlns:config:1.0'><config:config-item-set config:name='Unclassified'><config:config-item config:name='Entry' config:type='string'>value</config:config-item></config:config-item-set></office:settings>",
    ] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(&mut files, "settings.xml", "></office:document-settings>", &format!(">{inside}</office:document-settings>"));
        assert_refused(&files);
    }
    for extra in ["office:mimetype='text/xml'", "xmlns:f='urn:review:foreign' f:version='1.3'", "version='1.3'"] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(&mut files, "settings.xml", "office:version='1.3'", &format!("office:version='1.3' {extra}"));
        assert_refused(&files);
    }
    let mut foreign_root = fixture("1.3", Some("1.3"), true);
    replace(
        &mut foreign_root,
        "settings.xml",
        "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0'",
        "xmlns:office='urn:review:foreign'",
    );
    assert_refused(&foreign_root);
    let mut empty_meta = fixture("1.3", Some("1.3"), true);
    replace(
        &mut empty_meta,
        "meta.xml",
        "<office:meta><!--retained metadata gap--><meta:document-statistic meta:word-count='1' meta:page-count='1'/></office:meta>",
        "",
    );
    assert_refused(&empty_meta);
}
