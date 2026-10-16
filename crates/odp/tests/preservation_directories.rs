//! Contributor-original packages for the empty configuration-directory profile.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::text::Run;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";
const OWNER: &str = "<manifest:file-entry manifest:full-path='Configurations2/' manifest:media-type='application/vnd.sun.xml.ui.configuration'/>";

fn source(entries: &[(&str, &[u8])], owners: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Contributor original' draw:master-page-name='M'><draw:frame xml:id='local-text' svg:width='240pt' svg:height='120pt'><draw:text-box><text:p text:style-name='P'>Original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='960pt' fo:page-height='540pt' style:print-orientation='landscape'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
    );
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='styles.xml' manifest:media-type='text/xml'/>{owners}</manifest:manifest>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in [
        ("mimetype", MIME.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("styles.xml", styles.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
    ]
    .into_iter()
    .chain(entries.iter().copied())
    {
        if name.ends_with('/') && bytes.is_empty() {
            zip.add_directory(name, options).unwrap();
        } else {
            zip.start_file(name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}
fn members(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut members = BTreeMap::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        members.insert(file.name().into(), data);
    }
    members
}
fn edit(source: &[u8]) -> deckcraft_model::Presentation {
    let mut p = import(source).unwrap();
    let paragraph = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0];
    paragraph.runs = vec![Run::with("Edited", paragraph.runs[0].props.clone())];
    p
}
fn assert_preserved(source: &[u8]) {
    let p = edit(source);
    let output = export(&p).expect("bounded empty directories must permit a real text edit");
    assert_ne!(source, output);
    let before = members(source);
    let after = members(&output);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    for (name, bytes) in &before {
        if name == "content.xml" {
            assert_eq!(after[name], String::from_utf8(bytes.clone()).unwrap().replace(">Original<", ">Edited<").as_bytes());
        } else {
            assert_eq!(bytes, &after[name], "changed untouched member {name}");
        }
    }
    let reopened = import(&output).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "Edited");
    let recovered = p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap();
    assert_eq!(recovered.data.as_slice(), source);
    assert_eq!(import(&recovered.data).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Original");
}
fn assert_refused(source: &[u8]) {
    // Encrypted manifest owners can be rejected before a presentation exists.
    if import(source).is_ok() {
        assert!(export(&edit(source)).is_err());
    }
}

#[test]
fn zero_one_and_many_directories_preserve_all_payloads_and_manifest_bytes() {
    assert_preserved(&source(&[], ""));
    for entries in [
        vec![("Configurations2/", &b""[..])],
        vec![("Configurations2/toolbar/", &b""[..])],
        vec![
            ("Configurations2/", &b""[..]),
            ("Configurations2/toolbar/", &b""[..]),
            ("Configurations2/images/", &b""[..]),
            ("Configurations2/images/Bitmaps/", &b""[..]),
        ],
    ] {
        for owner in ["", OWNER] {
            assert_preserved(&source(&entries, owner));
        }
    }
}

#[test]
fn count_depth_and_path_limits_accept_the_boundary_and_refuse_the_next() {
    let paths: Vec<_> = (0..128).map(|index| format!("Configurations2/entry{index}/")).collect();
    let entries: Vec<_> = paths.iter().map(|path| (path.as_str(), &b""[..])).collect();
    assert_preserved(&source(&entries, OWNER));
    let mut too_many = entries;
    too_many.push(("Configurations2/entry128/", &[]));
    assert_refused(&source(&too_many, OWNER));
    let at_depth = format!("Configurations2/{}/", (0..7).map(|index| format!("d{index}")).collect::<Vec<_>>().join("/"));
    assert_preserved(&source(&[(&at_depth, &[])], OWNER));
    let over_depth = format!("{at_depth}d7/");
    assert_refused(&source(&[(&over_depth, &[])], OWNER));
    let at_length = format!("Configurations2/{}/", "a".repeat(1024 - "Configurations2/".len() - 1));
    assert_eq!(at_length.len(), 1024);
    assert_preserved(&source(&[(&at_length, &[])], OWNER));
    let over_length = format!("Configurations2/{}/", "a".repeat(1025 - "Configurations2/".len() - 1));
    assert_refused(&source(&[(&over_length, &[])], OWNER));
}

#[test]
fn configuration_files_are_forbidden_even_if_empty_xml_or_owned() {
    for (name, bytes) in [
        ("Configurations2/toolbar/custom.xml", &b"<configuration/>"[..]),
        ("Configurations2/toolbar/empty.xml", &b""[..]),
        ("Configurations2/toolbar/", &b"nonempty directory"[..]),
        ("Configurations2/", &b"nonempty root"[..]),
        ("Configurations2/config.xml", &b"<script:event-listener xmlns:script='urn:oasis:names:tc:opendocument:xmlns:script:1.0'/>"[..]),
    ] {
        assert_refused(&source(&[(name, bytes)], OWNER));
        let owners = format!("{OWNER}<manifest:file-entry manifest:full-path='{name}' manifest:media-type='text/xml'/>");
        assert_refused(&source(&[(name, bytes)], &owners));
    }
}

#[test]
fn only_the_unique_exact_configuration_root_owner_is_allowed() {
    let entries = [("Configurations2/toolbar/", &b""[..])];
    for owner in [
        OWNER.replace("application/vnd.sun.xml.ui.configuration", "text/xml"),
        OWNER.replace("application/vnd.sun.xml.ui.configuration", ""),
        format!("{OWNER}{OWNER}"),
        OWNER.replace("Configurations2/", "Configurations2/toolbar/"),
        format!("{OWNER}<manifest:file-entry manifest:full-path='Configurations2/toolbar/' manifest:media-type=''/>"),
        OWNER.replace("/>", " manifest:version='1.3'/>"),
        OWNER.replace("/>", " xmlns:f='urn:contributor:foreign' f:action='test'/>"),
        OWNER.replace("/>", "><manifest:encryption-data/></manifest:file-entry>"),
        OWNER.replace("/>", ">hidden text</manifest:file-entry>"),
    ] {
        assert_refused(&source(&entries, &owner));
    }
    assert_refused(&source(&[], OWNER)); // A directory owner cannot invent a missing scope.
    assert_refused(&source(&[("Configurations20/", &[])], OWNER));
}

#[test]
fn aliases_lookalike_prefixes_and_outside_parts_do_not_gain_an_exception() {
    for path in [
        "Configurations2/../other/",
        "Configurations2//toolbar/",
        "Configurations2/./toolbar/",
        "./Configurations2/toolbar/",
        "Configurations2/%2e%2e/",
        "Configurations2\\toolbar/",
        "configurations2/toolbar/",
        "Configurations20/toolbar/",
        "Configurations2/toolbar//",
    ] {
        assert_refused(&source(&[(path, &[])], OWNER));
    }
    assert_refused(&source(&[("Configurations2/", &[]), ("outside.bin", b"unverified")], OWNER));
}

#[test]
fn directory_caches_require_independent_proof_and_unknown_counts_still_refuse() {
    let original = source(&[("Configurations2/", &[])], OWNER);
    assert!(export(&import(&original).unwrap()).is_err());
    assert_refused(&source(&[("Configurations2/", &[]), ("Thumbnails/thumbnail.png", b"\x89PNG\r\n\x1a\n")], OWNER));
    let meta = b"<office:document-meta xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' office:version='1.3'><office:meta><meta:document-statistic meta:word-count='1'/></office:meta></office:document-meta>";
    let owners = format!("{OWNER}<manifest:file-entry manifest:full-path='meta.xml' manifest:media-type='text/xml'/>");
    let original = source(&[("Configurations2/", &[]), ("meta.xml", meta)], &owners);
    let output = export(&edit(&original)).expect("proved optional text count");
    let before = members(&original);
    let after = members(&output);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    assert_eq!(after["meta.xml"], std::str::from_utf8(meta).unwrap().replace(" meta:word-count='1'", "").as_bytes());
    for (name, bytes) in before.iter().filter(|(name, _)| !matches!(name.as_str(), "meta.xml" | "content.xml")) {
        assert_eq!(&after[name], bytes);
    }
    assert_eq!(import(&output).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Edited");
    let unknown = std::str::from_utf8(meta).unwrap().replace("meta:word-count='1'", "meta:word-count='1' meta:unknown-count='2'");
    assert_refused(&source(&[("Configurations2/", &[]), ("meta.xml", unknown.as_bytes())], &owners));
}
