//! Independent contributor-original empty-directory boundary and ownership fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";
const OWNER: &str = "<m:file-entry m:full-path='Configurations2/' m:media-type='application/vnd.sun.xml.ui.configuration'/>";

fn fixture(entries: &[(&str, &[u8])], owners: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:master-page-name='M'><draw:frame xml:id='local' svg:width='100pt' svg:height='40pt'><draw:text-box><text:p text:style-name='P'>Original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles/><office:automatic-styles><style:page-layout style:name='Page'><style:page-layout-properties fo:page-width='720pt' fo:page-height='405pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='Page'/></office:master-styles></office:document-styles>"
    );
    let manifest = format!(
        "<m:manifest xmlns:m='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' m:version='1.3'><!--exact original manifest gap--><m:file-entry m:full-path='/' m:media-type='{MIME}'/><m:file-entry m:full-path='content.xml' m:media-type='text/xml'/><m:file-entry m:full-path='styles.xml' m:media-type='text/xml'/>{owners}</m:manifest>"
    );
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("mimetype", MIME.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("styles.xml", styles.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
    ]
    .into_iter()
    .chain(entries.iter().copied())
    {
        if name.ends_with('/') && data.is_empty() {
            writer.add_directory(name, options).unwrap();
        } else {
            writer.start_file(name, options).unwrap();
            writer.write_all(data).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}

fn members(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        parts.insert(file.name().into(), data);
    }
    parts
}

fn edited(source: &[u8]) -> Presentation {
    let mut p = import(source).unwrap();
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Edited".into();
    p
}

fn preserved(source: &[u8]) {
    let p = edited(source);
    let output = export(&p).unwrap();
    let before = members(source);
    let after = members(&output);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    for (name, bytes) in before {
        let expected = if name == "content.xml" { String::from_utf8(bytes).unwrap().replace(">Original<", ">Edited<").into_bytes() } else { bytes };
        assert_eq!(after[&name], expected, "changed {name}");
    }
    assert_eq!(import(&output).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Edited");
    let recovery = p.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap();
    assert_eq!(recovery.data.as_slice(), source);
    assert_eq!(import(&recovery.data).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Original");
}

/// Start from a valid editable state and replace only immutable source bytes.
/// This exercises preservation preflight even if ordinary import rejects the adversary first.
fn refused(source: Vec<u8>) {
    let mut p = edited(&fixture(&[("Configurations2/", &[])], OWNER));
    p.media.iter_mut().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data = Arc::new(source);
    assert!(export(&p).is_err(), "hostile directory package escaped preservation preflight");
}

fn u16_at(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()))
}
fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}
fn entry(bytes: &[u8], name: &str) -> Option<(usize, usize)> {
    let end = bytes.len() - 22;
    let mut at = u32_at(bytes, end + 16);
    for _ in 0..u16_at(bytes, end + 10) {
        assert_eq!(&bytes[at..at + 4], b"PK\x01\x02");
        let length = u16_at(bytes, at + 28);
        if &bytes[at + 46..at + 46 + length] == name.as_bytes() {
            return Some((at, u32_at(bytes, at + 42)));
        }
        at += 46 + length + u16_at(bytes, at + 30) + u16_at(bytes, at + 32);
    }
    None
}

#[test]
fn alias_prefixes_entities_and_unicode_directory_names_preserve_exact_original_bytes() {
    let entries = [("Configurations2/動作/empty.xml/", &b""[..]), ("Configurations2/scripts/events/", &b""[..])];
    for owner in
        [String::new(), OWNER.into(), OWNER.replace("Configurations2/", "Configurations&#50;/").replace("ui.configuration", "ui.configuratio&#110;")]
    {
        preserved(&fixture(&entries, &owner));
    }
}

#[test]
fn directory_path_limit_counts_utf8_bytes_and_physical_record_count() {
    let prefix = "Configurations2/";
    let available = 1024 - prefix.len() - 1;
    let path = format!("{prefix}{}{}/", "界".repeat(available / 3), "a".repeat(available % 3));
    assert_eq!(path.len(), 1024);
    preserved(&fixture(&[(&path, &[])], OWNER));
    let overflow = format!("{}a/", path.trim_end_matches('/'));
    assert_eq!(overflow.len(), 1025);
    refused(fixture(&[(&overflow, &[])], OWNER));
    let names = (0..127).map(|index| format!("Configurations2/d{index}/")).collect::<Vec<_>>();
    let mut entries = names.iter().map(|name| (name.as_str(), &b""[..])).collect::<Vec<_>>();
    entries.push(("Configurations2/", &[]));
    preserved(&fixture(&entries, OWNER));
    entries.push(("Configurations2/last/", &[]));
    refused(fixture(&entries, OWNER));
}

#[test]
fn namespace_aliases_cannot_hide_duplicate_child_or_active_root_owners() {
    let entries = [("Configurations2/toolbar/", &b""[..])];
    for extra in [
        OWNER.replace("Configurations2/", "Configurations&#50;/"),
        OWNER.replace("Configurations2/", "Configurations2/toolbar/"),
        OWNER.replace("m:media-type=", "xmlns:f='urn:review:foreign' f:media-type="),
        OWNER.replace("/>", " xmlns:f='urn:review:foreign' f:dispatch='script:run'/>"),
        OWNER.replace("/>", "><m:encryption-data/></m:file-entry>"),
        OWNER.replace("/>", " xml:base='https://example.invalid/'/>"),
        OWNER.replace("/>", " m:version='1.3'/>"),
    ] {
        refused(fixture(&entries, &format!("{OWNER}{extra}")));
        if !extra.contains("Configurations&#50;/") {
            refused(fixture(&entries, &extra));
        }
    }
    refused(fixture(&[], OWNER));
}

#[test]
fn configuration_scope_does_not_cover_files_siblings_or_descendant_manifest_owners() {
    for (name, payload) in [
        ("Configurations2/empty.xml", &b""[..]),
        ("Configurations2", &b""[..]),
        ("Configurations2x/", &b""[..]),
        ("Configurations2/", &b"data"[..]),
        ("Configurations2/script.xml", &b"<events/>"[..]),
    ] {
        let owner = format!("{OWNER}<m:file-entry m:full-path='{name}' m:media-type='text/xml'/>");
        refused(fixture(&[("Configurations2/valid/", &[]), (name, payload)], &owner));
        refused(fixture(&[("Configurations2/valid/", &[]), (name, payload)], OWNER));
    }
}

#[test]
fn raw_directory_metadata_cannot_bypass_method_crc_flags_symlink_or_duplicate_guards() {
    let original = fixture(&[("Configurations2/a/", &[]), ("Configurations2/b/", &[])], OWNER);
    let (central, local) = entry(&original, "Configurations2/b/").expect("original directory record");
    for mutation in 0..6 {
        let mut bytes = original.clone();
        match mutation {
            0 => {
                bytes[central + 10..central + 12].copy_from_slice(&8u16.to_le_bytes());
                bytes[local + 8..local + 10].copy_from_slice(&8u16.to_le_bytes());
            }
            1 => {
                bytes[central + 16..central + 20].copy_from_slice(&1u32.to_le_bytes());
                bytes[local + 14..local + 18].copy_from_slice(&1u32.to_le_bytes());
            }
            2 => {
                bytes[central + 8..central + 10].copy_from_slice(&1u16.to_le_bytes());
                bytes[local + 6..local + 8].copy_from_slice(&1u16.to_le_bytes());
            }
            3 => {
                bytes[central + 4..central + 6].copy_from_slice(&0x0314u16.to_le_bytes());
                bytes[central + 38..central + 42].copy_from_slice(&(0xa1ffu32 << 16).to_le_bytes());
            }
            4 => {
                let name = b"Configurations2/a/";
                bytes[central + 46..central + 46 + name.len()].copy_from_slice(name);
                bytes[local + 30..local + 30 + name.len()].copy_from_slice(name);
            }
            _ => {
                bytes[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
                bytes[local + 22..local + 26].copy_from_slice(&1u32.to_le_bytes());
            }
        }
        refused(bytes);
    }
}
