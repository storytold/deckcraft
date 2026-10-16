//! Fresh contributor-original cache deletion and dependency-closure adversaries.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xlink='http://www.w3.org/1999/xlink'";
const STAT: &str = "<meta:document-statistic meta:word-count='2' meta:page-count='1'/>";
const THUMB: &str = "Thumbnails/thumbnail.png";

fn png() -> Vec<u8> {
    deckcraft_render::Image { width: 1, height: 1, pixels: vec![51, 101, 151, 255] }.to_png()
}
fn fixture(stat: &str) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Local' draw:master-page-name='M' xml:id='page'><draw:frame svg:width='100pt' svg:height='40pt' xml:id='local'><draw:text-box><text:p text:style-name='P'>Original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='720pt' fo:page-height='405pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
    );
    let meta = format!(
        "<office:document-meta {NS} office:version='1.3'><office:meta><!--literal UTF8: Ω--><dc:title>Thumbnails/thumbnail.png and meta:word-count are prose</dc:title>{stat}</office:meta></office:document-meta>"
    );
    let mut files = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        (THUMB.into(), png()),
    ]);
    manifest(&mut files);
    files
}
fn manifest(files: &mut BTreeMap<String, Vec<u8>>) {
    let mut xml = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}'/><!--manifest immutable gap-->"
    );
    for name in
        files.keys().filter(|name| name.as_str() != "mimetype" && name.as_str() != "META-INF/manifest.xml" && !name.starts_with("Configurations2/"))
    {
        let mime = if name.ends_with('/') {
            ""
        } else if name.ends_with(".png") {
            "image/png"
        } else {
            "text/xml"
        };
        xml.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>"));
    }
    xml.push_str("<!--trailing gap--></manifest:manifest>");
    files.insert("META-INF/manifest.xml".into(), xml.into_bytes());
}
fn zip(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    out.start_file("mimetype", options).unwrap();
    out.write_all(MIME.as_bytes()).unwrap();
    for (name, bytes) in files {
        if name == "mimetype" {
            continue;
        }
        if name.ends_with('/') {
            out.add_directory(name, options).unwrap();
        } else {
            out.start_file(name, options).unwrap();
            out.write_all(bytes).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = BTreeMap::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        files.insert(file.name().into(), bytes);
    }
    files
}
fn replace(files: &mut BTreeMap<String, Vec<u8>>, name: &str, old: &str, new: &str) {
    let text = String::from_utf8(files[name].clone()).unwrap();
    assert!(text.contains(old));
    files.insert(name.into(), text.replace(old, new).into_bytes());
}
fn edit(source: &[u8]) -> Presentation {
    let mut p = import(source).unwrap();
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Changed".into();
    p
}
fn refused(files: &BTreeMap<String, Vec<u8>>) {
    let source = zip(files);
    // Force cache validation even when an upstream ordinary import rejects the adversary.
    let mut p = match import(&source) {
        Ok(_) => edit(&source),
        Err(_) => edit(&zip(&fixture(STAT))),
    };
    p.media.iter_mut().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data = Arc::new(source);
    let error = export(&p).unwrap_err().to_string();
    assert!(
        error.contains("cache") || error.contains("manifest"),
        "must fail cache/manifest proof, not hide behind a later native mismatch: {error}"
    );
}

#[test]
fn independent_alias_ordinals_and_paired_leaves_preserve_every_surviving_byte() {
    let statistic = "<m:document-statistic\t m:word-count = '&#50;' xmlns:m='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'\r\n m:page-count=\"&#49;\" xmlns:z='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'  z:character-count = '8'\n z:paragraph-count='1'\t z:sentence-count='0' z:syllable-count='2' z:non-whitespace-character-count='8'> \n </m:document-statistic>";
    let mut files = fixture(statistic);
    files.insert("Thumbnails/".into(), Vec::new());
    files.insert("Configurations2/empty/".into(), Vec::new());
    files.insert("Pictures/keep.png".into(), png());
    files.insert(
        "settings.xml".into(),
        format!("<office:document-settings {NS} office:version='1.3'><office:settings/></office:document-settings>").into_bytes(),
    );
    manifest(&mut files);
    let owner = "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>";
    let paired = "<file-entry xmlns='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:full-path='Thumbnails/thumbnail.pn&#103;' manifest:media-type = \"image/png\">\r\n </file-entry>";
    replace(&mut files, "META-INF/manifest.xml", owner, paired);
    let source = zip(&files);
    let desired = edit(&source);
    let output = export(&desired).unwrap();
    let after = parts(&output);
    assert_eq!(after.len() + 1, files.len());
    assert!(!after.contains_key(THUMB));
    for (name, bytes) in &files {
        if name == THUMB {
            continue;
        }
        let expected = match name.as_str() {
            "content.xml" => String::from_utf8(bytes.clone()).unwrap().replace(">Original<", ">Changed<").into_bytes(),
            "META-INF/manifest.xml" => String::from_utf8(bytes.clone()).unwrap().replace(paired, "").into_bytes(),
            "meta.xml" => {
                let mut text = String::from_utf8(bytes.clone()).unwrap();
                for attr in [
                    "\t m:word-count = '&#50;'",
                    "  z:character-count = '8'",
                    "\t z:sentence-count='0'",
                    " z:syllable-count='2'",
                    " z:non-whitespace-character-count='8'",
                ] {
                    text = text.replace(attr, "");
                }
                text.into_bytes()
            }
            _ => bytes.clone(),
        };
        assert_eq!(after[name], expected, "rewrote surviving bytes in {name}");
    }
    assert_eq!(import(&output).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Changed");
    let backup = desired.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap();
    assert_eq!(backup.data.as_slice(), source);
    assert_eq!(import(&backup.data).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Original");
}

#[test]
fn independent_part_reference_closure_includes_unused_entity_aliased_resources() {
    for uri in [
        "Thumbnails/thumbnail.pn&#103;",
        "./Thumbnails/thumbnail.png",
        "././Thumbnails/thumbnail.png",
        "Thumbnails%2Fthumbnail.png",
        "/Thumbnails/thumbnail.png",
        "Thumbnails/thumbnail.png?q=1",
        "Thumbnails/thumbnail.png#x",
        "https://example.invalid/thumbnail.png",
        "Thumbnails\\thumbnail.png",
    ] {
        let mut files = fixture(STAT);
        replace(
            &mut files,
            "styles.xml",
            "<office:styles/>",
            &format!("<office:styles><draw:fill-image draw:name='Unused' xlink:href='{uri}'/></office:styles>"),
        );
        refused(&files);
    }
    let mut valid = fixture(STAT);
    valid.insert("Pictures/keep.png".into(), png());
    manifest(&mut valid);
    replace(
        &mut valid,
        "styles.xml",
        "<office:styles/>",
        "<office:styles><draw:fill-image draw:name='Unused' xlink:href='./Pictures/keep.png'/></office:styles>",
    );
    let output = parts(&export(&edit(&zip(&valid))).unwrap());
    assert_eq!(output["Pictures/keep.png"], valid["Pictures/keep.png"]);
    assert!(!output.contains_key(THUMB));
}

#[test]
fn independent_wrong_part_statistic_and_all_count_dependency_kinds_refuse() {
    for part in ["settings.xml", "styles.xml", "content.xml"] {
        let mut files = fixture("");
        if part == "settings.xml" {
            files.insert(
                part.into(),
                format!("<office:document-settings {NS} office:version='1.3'><office:settings>{STAT}</office:settings></office:document-settings>")
                    .into_bytes(),
            );
            manifest(&mut files);
        } else if part == "styles.xml" {
            replace(&mut files, part, "<office:styles/>", &format!("<office:styles>{STAT}</office:styles>"));
        } else {
            replace(&mut files, part, "<office:presentation>", &format!("<office:presentation>{STAT}"));
        }
        refused(&files);
    }
    for field in
        ["word-count", "character-count", "non-whitespace-character-count", "sentence-count", "syllable-count", "variable-get", "user-field-get"]
    {
        let mut files = fixture(STAT);
        replace(&mut files, "content.xml", ">Original<", &format!("><text:{field}>1</text:{field}><"));
        refused(&files);
    }
    let mut files = fixture(STAT);
    replace(&mut files, "meta.xml", "</office:meta>", "<meta:user-defined meta:name='count'>meta:word-count</meta:user-defined></office:meta>");
    refused(&files);
}

#[test]
fn independent_style_family_scope_and_late_unknown_definitions_fail_closed() {
    for definition in [
        "<style:style style:name='U' style:family='text'><style:text-properties style:font-name='Missing'/></style:style>",
        "<style:style style:name='T' style:family='text'/><style:style style:name='U' style:family='paragraph' style:parent-style-name='T'/>",
        "<style:default-style style:family='text'><style:graphic-properties draw:fill='none'/></style:default-style>",
        "<style:style style:name='U' style:family='text'><style:text-properties fo:letter-spacing='0pt'/></style:style>",
    ] {
        let mut files = fixture(STAT);
        replace(&mut files, "styles.xml", "<office:styles/>", &format!("<office:styles>{definition}</office:styles>"));
        refused(&files);
    }
    let mut files = fixture(STAT);
    let mut definitions = (0..180)
        .map(|index| {
            format!(
                "<style:style style:name='U{index}' style:family='paragraph'><style:paragraph-properties fo:keep-together='always'/></style:style>"
            )
        })
        .collect::<String>();
    definitions.push_str("<style:default-style style:family='graphic'><style:graphic-properties draw:shadow='none'/></style:default-style>");
    replace(&mut files, "styles.xml", "<office:styles/>", &format!("<office:styles>{definitions}</office:styles>"));
    refused(&files);
    let mut files = fixture(STAT);
    replace(
        &mut files,
        "content.xml",
        "<office:automatic-styles>",
        "<office:font-face-decls><style:font-face style:name='OnlyContent' svg:font-family='Example'/></office:font-face-decls><office:automatic-styles>",
    );
    replace(
        &mut files,
        "styles.xml",
        "<office:styles/>",
        "<office:styles><style:style style:name='U' style:family='text'><style:text-properties style:font-name='OnlyContent'/></style:style></office:styles>",
    );
    refused(&files);
}

#[test]
fn independent_owner_and_count_namespace_spoofs_cannot_mint_cache_permission() {
    for stat in [
        "<meta:document-statistic xmlns:f='urn:foreign' f:word-count='2'/>",
        "<meta:document-statistic meta:word-count='2' xml:id='counter'/>",
        "<meta:document-statistic meta:word-count='2'><text:span/></meta:document-statistic>",
        "<meta:document-statistic meta:word-count='2'><![CDATA[ ]]></meta:document-statistic>",
        "<meta:document-statistic meta:word-count='2'/><meta:document-statistic/>",
    ] {
        refused(&fixture(stat));
    }
    for owner in [
        "<manifest:file-entry xmlns:manifest='urn:spoof' manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>",
        "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png' xml:id='thumb'/>",
        "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'><!--opaque--></manifest:file-entry>",
    ] {
        let mut files = fixture(STAT);
        replace(
            &mut files,
            "META-INF/manifest.xml",
            "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>",
            owner,
        );
        refused(&files);
    }
}

#[test]
fn independent_cache_exceptions_never_authorize_no_edit_or_foreign_native_changes() {
    let source = zip(&fixture(STAT));
    let original = import(&source).unwrap();
    assert!(export(&original).is_err());
    let mut desired = edit(&source);
    Arc::make_mut(&mut desired.slides[0]).shapes[0].xfrm.as_mut().unwrap().w += 1.;
    assert!(export(&desired).is_err());
    assert_eq!(desired.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
    let mut files = fixture(STAT);
    replace(
        &mut files,
        "styles.xml",
        "<style:master-page style:name='M'",
        "<style:master-page presentation:presentation-page-layout-name='Unsupported' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' style:name='M'",
    );
    refused(&files);
}
