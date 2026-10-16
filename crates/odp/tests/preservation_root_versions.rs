//! Contributor-original synthetic packages for the optional manifest root-owner version.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const MANIFEST: &str = "META-INF/manifest.xml";
const THUMB: &str = "Thumbnails/thumbnail.png";
const THUMB_OWNER: &str = "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/'";
const COUNTS: &str = "<meta:document-statistic meta:word-count='4' meta:character-count='24' meta:non-whitespace-character-count='22' meta:sentence-count='2' meta:syllable-count='6' meta:page-count='1' meta:paragraph-count='2'/>";

fn owner(path: &str, mime: &str) -> String {
    format!("<manifest:file-entry manifest:full-path='{path}' manifest:media-type='{mime}'/>")
}
fn root_owner(version: Option<&str>) -> String {
    let version = version.map(|v| format!(" manifest:version='{v}'")).unwrap_or_default();
    format!("<manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}'{version}/>")
}
fn fixture(version: &str, root_version: Option<&str>, preview: bool) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='{version}'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Original page' draw:master-page-name='M' xml:id='page'><draw:frame xml:id='local' svg:width='240pt' svg:height='120pt'><draw:text-box><text:p text:style-name='P' xml:id='one'>Original one</text:p><text:p text:style-name='P' xml:id='two'>Original two</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='720pt' fo:page-height='405pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
    );
    let meta = format!(
        "<office:document-meta {NS} office:version='{version}'><office:meta><dc:title>Original scalar title</dc:title>{COUNTS}</office:meta></office:document-meta>"
    );
    let settings = format!("<office:document-settings {NS} office:version='{version}'></office:document-settings>");
    let png = deckcraft_render::Image { width: 1, height: 1, pixels: vec![23, 101, 173, 255] }.to_png();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        ("settings.xml".into(), settings.into_bytes()),
        ("Pictures/keep.png".into(), png.clone()),
        ("Pictures/".into(), Vec::new()),
        ("Configurations2/".into(), Vec::new()),
    ]);
    if preview {
        files.insert(THUMB.into(), png);
    }
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'>\n<!--before root-->{}<!--after root-->\r\n",
        root_owner(root_version)
    );
    for name in files.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name == "Configurations2/" {
            "application/vnd.sun.xml.ui.configuration"
        } else if name.ends_with('/') {
            ""
        } else if name.ends_with(".png") {
            "image/png"
        } else {
            "text/xml"
        };
        manifest.push_str(&owner(name, mime));
        manifest.push_str("\n<!--survivor gap-->\t");
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
        if name.ends_with('/') {
            writer.add_directory(name, options).unwrap();
        } else {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        files.insert(member.name().into(), bytes);
    }
    files
}
fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(files[part].clone()).unwrap();
    assert!(text.contains(old), "missing mutation {old}");
    files.insert(part.into(), text.replace(old, new).into_bytes());
}
fn edit(p: &mut Presentation) {
    let text = Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap();
    text.paragraphs[0].runs[0].text = "Edited".into();
    text.paragraphs[1].runs[0].text = "Changed".into();
}
fn preserved(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    let source = zip(files);
    let mut desired = import(&source).expect("synthetic public package imports");
    assert!(export(&desired).is_err(), "no edit must still refuse cache invalidation");
    edit(&mut desired);
    let output = export(&desired).expect("proved root-owner version allows a real two-text edit");
    let after = parts(&output);
    assert_eq!(after.len() + usize::from(files.contains_key(THUMB)), files.len());
    assert!(!after.contains_key(THUMB));
    for (name, bytes) in files {
        let original = String::from_utf8_lossy(bytes);
        let expected = match name.as_str() {
            THUMB => continue,
            "content.xml" => original.replace("Original one", "Edited").replace("Original two", "Changed").into_bytes(),
            "meta.xml" => original
                .replace(" meta:word-count='4'", "")
                .replace(" meta:character-count='24'", "")
                .replace(" meta:non-whitespace-character-count='22'", "")
                .replace(" meta:sentence-count='2'", "")
                .replace(" meta:syllable-count='6'", "")
                .into_bytes(),
            MANIFEST => original.replace(THUMB_OWNER, "").into_bytes(),
            _ => bytes.clone(),
        };
        assert_eq!(after[name], expected, "unexpected bytes in {name}");
    }
    let reopened = import(&output).expect("edited output reimports");
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "Edited\nChanged");
    assert_eq!(desired.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
    // Retained structural statistics still trigger a fresh closed proof on the next edit.
    let mut second = reopened;
    Arc::make_mut(&mut second.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Again".into();
    let second_output = export(&second).expect("fresh output cache proof remains valid");
    assert_eq!(parts(&second_output)[MANIFEST], after[MANIFEST]);
    assert_eq!(import(&second_output).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Again\nChanged");
    after
}
fn refused(files: &BTreeMap<String, Vec<u8>>) {
    refused_with_earlier_reason(files, None);
}
fn refused_with_earlier_reason(files: &BTreeMap<String, Vec<u8>>, earlier_reason: Option<&str>) {
    let source = zip(files);
    let mut p = import(&source).unwrap_or_else(|_| import(&zip(&fixture("1.3", Some("1.3"), true))).unwrap());
    edit(&mut p);
    // Ensure even import-rejected XML reaches the raw package/cache proof on export.
    p.media.iter_mut().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data = Arc::new(source);
    let error = export(&p).expect_err("unproved root owner/package must refuse").to_string();
    if let Some(reason) = earlier_reason {
        assert!(error.ends_with(reason), "must fail the exact earlier owner proof: {error}");
        return;
    }
    assert!(
        error.contains("cache") || error.contains("manifest") || error.contains("duplicate expanded XML attribute"),
        "must fail the package/cache/XML proof before native mismatch: {error}"
    );
}
fn emit(version: &str, before: &BTreeMap<String, Vec<u8>>, after: &BTreeMap<String, Vec<u8>>) {
    if let Ok(directory) = std::env::var("DECKCRAFT_ROOT_VERSION_XML_DIR") {
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
fn absent_and_matching_root_versions_12_13_preserve_bytes_and_both_cache_proofs() {
    for version in ["1.2", "1.3"] {
        for root_version in [None, Some(version)] {
            for preview in [false, true] {
                let files = fixture(version, root_version, preview);
                let after = preserved(&files);
                if root_version.is_some() && preview {
                    emit(version, &files, &after);
                }
            }
        }
    }
}

#[test]
fn root_owner_aliases_entities_attribute_order_and_paired_leaf_are_byte_exact() {
    for version in ["1.2", "1.3"] {
        let mut files = fixture(version, Some(version), true);
        let lexical = format!(
            "<file-entry xmlns='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' xmlns:m='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'\r\n m:version = '1.&#{};'\t m:media-type = \"application/vnd.oasis.opendocument.presentatio&#110;\" m:full-path = '&#47;'> \n </file-entry>",
            if version == "1.2" { "50" } else { "51" }
        );
        replace(&mut files, MANIFEST, &root_owner(Some(version)), &lexical);
        replace(
            &mut files,
            MANIFEST,
            &format!("manifest:version='{version}'"),
            &format!("manifest:version='1.&#{};'", if version == "1.2" { "50" } else { "51" }),
        );
        for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
            replace(
                &mut files,
                part,
                &format!("office:version='{version}'"),
                &format!(
                    "xmlns:o='urn:oasis:names:tc:opendocument:xmlns:office:1.0' o:version='1.&#{};'",
                    if version == "1.2" { "50" } else { "51" }
                ),
            );
        }
        preserved(&files);
    }
}

#[test]
fn invalid_unknown_empty_whitespace_uri_and_mismatched_root_versions_refuse() {
    for manifest_version in ["1.2", "1.3"] {
        for bad in ["", " ", "1.1", "1.4", "1.20", " 1.2", "1.3 ", "1.&#50;&#32;", "urn:version:1.2", "Thumbnails/thumbnail.png"] {
            refused(&fixture(manifest_version, Some(bad), true));
        }
        let other = if manifest_version == "1.2" { "1.3" } else { "1.2" };
        refused(&fixture(manifest_version, Some(other), true));
    }
}

#[test]
fn present_root_version_requires_every_fixed_xml_root_to_match() {
    for version in ["1.2", "1.3"] {
        let other = if version == "1.2" { "1.3" } else { "1.2" };
        for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
            for replacement in [format!("office:version='{other}'"), String::new()] {
                let mut files = fixture(version, Some(version), true);
                replace(&mut files, part, &format!("office:version='{version}'"), &replacement);
                refused(&files);
            }
        }
    }
}

#[test]
fn absent_root_owner_version_keeps_existing_independent_fixed_root_versions() {
    for version in ["1.2", "1.3"] {
        for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
            let mut files = fixture(version, None, true);
            let other = if version == "1.2" { "1.3" } else { "1.2" };
            replace(&mut files, part, &format!("office:version='{version}'"), &format!("office:version='{other}'"));
            preserved(&files);
        }
    }
}

#[test]
fn thumbnail_picture_fixed_part_and_directory_owners_keep_exactly_two_attributes() {
    for version in ["1.2", "1.3"] {
        for (path, mime) in [
            (THUMB, "image/png"),
            ("Pictures/keep.png", "image/png"),
            ("content.xml", "text/xml"),
            ("styles.xml", "text/xml"),
            ("meta.xml", "text/xml"),
            ("settings.xml", "text/xml"),
            ("Pictures/", ""),
            ("Configurations2/", "application/vnd.sun.xml.ui.configuration"),
        ] {
            let mut files = fixture(version, Some(version), true);
            let original = owner(path, mime);
            replace(&mut files, MANIFEST, &original, &original.replace("/>", &format!(" manifest:version='{version}'/>")));
            if path == "Configurations2/" {
                refused_with_earlier_reason(&files, Some("source-preserving text export refused: unverified configuration directory owner"));
            } else {
                refused(&files);
            }
        }
    }
}

#[test]
fn foreign_same_local_version_owner_identity_and_duplicate_expanded_attributes_refuse() {
    for bad in [
        format!("<manifest:file-entry xmlns:x='urn:contributor:foreign' manifest:full-path='/' manifest:media-type='{MIME}' x:version='1.3'/>"),
        format!("<x:file-entry xmlns:x='urn:contributor:foreign' manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3'/>"),
        format!("<manifest:file-entry xmlns:x='urn:contributor:foreign' x:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3'/>"),
        format!(
            "<manifest:file-entry xmlns:x='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3' x:version='1.3'/>"
        ),
    ] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(&mut files, MANIFEST, &root_owner(Some("1.3")), &bad);
        refused(&files);
    }
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        let mut files = fixture("1.3", Some("1.3"), true);
        replace(&mut files, part, "office:version='1.3'", "xmlns:x='urn:contributor:foreign' x:version='1.3'");
        refused(&files);
    }
}

#[test]
fn extra_root_attributes_children_duplicate_owners_and_wrong_mime_refuse() {
    for version in [None, Some("1.3")] {
        let root = root_owner(version);
        for bad in [
            root.replace("/>", " manifest:size='0'/>"),
            root.replace("/>", " manifest:preferred-view-mode='presentation'/>"),
            root.replace("/>", " xml:id='root'/>"),
            root.replace("/>", "><manifest:file-entry manifest:full-path='nested' manifest:media-type='text/xml'/></manifest:file-entry>"),
            root.replace("/>", ">not whitespace</manifest:file-entry>"),
            format!("{root}{}", root_owner(None)),
            format!("{}{}", root_owner(None), root_owner(Some("1.3"))),
            root.replace(MIME, "application/vnd.oasis.opendocument.text"),
        ] {
            let mut files = fixture("1.3", version, true);
            replace(&mut files, MANIFEST, &root, &bad);
            refused(&files);
        }
    }
}

#[test]
fn zero_settings_children_and_legacy_empty_child_stay_closed() {
    for version in ["1.2", "1.3"] {
        let mut legacy = fixture(version, Some(version), true);
        replace(&mut legacy, "settings.xml", "</office:document-settings>", "<office:settings/></office:document-settings>");
        preserved(&legacy);
        for child in [
            "<office:settings/><office:settings/>",
            "not whitespace",
            "<office:unknown/>",
            "<office:settings office:version='1.3'/>",
            "<office:settings>not whitespace</office:settings>",
            "<office:settings><office:unknown/></office:settings>",
        ] {
            let mut files = fixture(version, Some(version), true);
            replace(&mut files, "settings.xml", "</office:document-settings>", &format!("{child}</office:document-settings>"));
            refused(&files);
        }
    }
}

#[test]
fn matching_owner_version_does_not_broaden_xml_or_cache_dependencies() {
    for version in ["1.2", "1.3"] {
        for (part, old, new) in [
            ("content.xml", "<office:body>", "<office:scripts><office:script/></office:scripts><office:body>"),
            (
                "styles.xml",
                "<office:styles/>",
                "<office:styles><style:style style:name='Unused' style:family='text'><style:text-properties fo:text-shadow='none'/></style:style></office:styles>",
            ),
            ("meta.xml", "meta:word-count='4'", "meta:word-count='4' meta:unknown-count='0'"),
            (
                "settings.xml",
                "</office:document-settings>",
                "<office:settings xmlns:config='urn:oasis:names:tc:opendocument:xmlns:config:1.0'><config:config-item-set config:name='Unknown'/></office:settings></office:document-settings>",
            ),
        ] {
            let mut files = fixture(version, Some(version), true);
            replace(&mut files, part, old, new);
            refused(&files);
        }
        let mut files = fixture(version, Some(version), true);
        replace(
            &mut files,
            "styles.xml",
            "<office:styles/>",
            "<office:styles xmlns:xlink='http://www.w3.org/1999/xlink'><draw:fill-image draw:name='Unused' xlink:href='Thumbnails/thumbnail.png'/></office:styles>",
        );
        refused(&files);
    }
}
