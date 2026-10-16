//! Contributor-original synthetic cache fixtures. BIZ UDPGothic is the OFL craft-fonts face.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{Presentation, text::Run};
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xlink='http://www.w3.org/1999/xlink'";
const COUNTS: &str = "<meta:document-statistic meta:word-count='4' meta:character-count=\"24\" meta:non-whitespace-character-count='20' meta:sentence-count='2' meta:syllable-count='6' meta:page-count='1' meta:paragraph-count='2'/>";
fn png() -> Vec<u8> {
    deckcraft_render::Image { width: 2, height: 2, pixels: vec![31, 97, 179, 255, 31, 97, 179, 255, 31, 97, 179, 255, 31, 97, 179, 255] }.to_png()
}
fn fixture(preview: bool, stat: &str) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/><style:text-properties fo:font-family='BIZ UDPGothic' fo:font-size='24pt' fo:color='#123456'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Original' draw:master-page-name='M' xml:id='page'><draw:frame svg:x='12pt' svg:y='24pt' svg:width='300pt' svg:height='150pt' xml:id='shape'><draw:text-box><text:p text:style-name='P' xml:id='one'>Original one</text:p><text:p text:style-name='P' xml:id='two'>Original two</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='400pt' fo:page-height='240pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
    );
    let meta = format!(
        "<office:document-meta {NS} office:version='1.3'><office:meta><dc:title>Literal Thumbnails/thumbnail.png</dc:title>{stat}</office:meta></office:document-meta>"
    );
    let mut files = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
    ]);
    if preview {
        files.insert("Thumbnails/thumbnail.png".into(), png());
    }
    manifest(&mut files);
    files
}
fn manifest(files: &mut BTreeMap<String, Vec<u8>>) {
    let mut m = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}'/>"
    );
    for name in files.keys().filter(|n| n.as_str() != "mimetype" && n.as_str() != "META-INF/manifest.xml") {
        if name.starts_with("Configurations2/") {
            continue;
        }
        let mime = if name.ends_with('/') {
            ""
        } else if name.ends_with(".png") {
            "image/png"
        } else {
            "text/xml"
        };
        m.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>"));
    }
    m.push_str("</manifest:manifest>");
    files.insert("META-INF/manifest.xml".into(), m.into_bytes());
}
fn zip(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file("mimetype", opt).unwrap();
    z.write_all(MIME.as_bytes()).unwrap();
    for (n, b) in files {
        if n == "mimetype" {
            continue;
        }
        if n.ends_with('/') {
            z.add_directory(n, opt).unwrap();
        } else {
            z.start_file(n, opt).unwrap();
            z.write_all(b).unwrap();
        }
    }
    z.finish().unwrap().into_inner()
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let n = f.name().into();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            (n, b)
        })
        .collect()
}
fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, a: &str, b: &str) {
    let value = String::from_utf8(files[part].clone()).unwrap();
    assert!(value.contains(a), "missing mutation {a}");
    files.insert(part.into(), value.replace(a, b).into_bytes());
}
fn edit(source: &[u8]) -> Presentation {
    let mut p = import(source).unwrap();
    for (i, value) in ["Edited first", "Changed second"].iter().enumerate() {
        let paragraph = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[i];
        let props = paragraph.runs[0].props.clone();
        assert_eq!(props.font.as_deref(), Some("BIZ UDPGothic"));
        paragraph.runs = vec![Run::with(*value, props)];
    }
    p
}
fn refuses(files: &BTreeMap<String, Vec<u8>>) {
    let source = zip(files);
    if let Ok(p) = import(&source) {
        let mut p = p;
        let para = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0];
        para.runs[0].text = "Edited".into();
        assert!(export(&p).is_err(), "unproved cache package exported");
    }
}

#[test]
fn both_caches_invalidate_exactly_and_render_real_two_text_edits() {
    let allowed = deckcraft_fonts::japanese_fonts().find(|f| f.family == "BIZ UDPGothic" && f.style == "Regular").expect("OFL craft-fonts BIZ face");
    let actual_face = deckcraft_fonts::FontDb::global().face("BIZ UDPGothic", "Regular");
    assert_eq!(actual_face.family, "BIZ UDPGothic");
    assert_eq!(actual_face.data(), allowed.bytes, "actual face must be the allowed source, without substitution");
    let mut files = fixture(true, COUNTS);
    files.insert("Configurations2/".into(), Vec::new());
    files.insert("Configurations2/toolpanel/".into(), Vec::new());
    files.insert("Thumbnails/".into(), Vec::new());
    files.insert("Pictures/original.png".into(), png());
    manifest(&mut files);
    let source = zip(&files);
    let p = edit(&source);
    let original = import(&source).unwrap();
    let out = export(&p).expect("proved optional caches");
    let after = parts(&out);
    if let Ok(directory) = std::env::var("DECKCRAFT_ODP_CACHE_QA_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for name in ["content.xml", "styles.xml", "meta.xml", "META-INF/manifest.xml"] {
            std::fs::write(std::path::Path::new(&directory).join(name.replace('/', "-")), &after[name]).unwrap();
        }
    }
    assert!(!after.contains_key("Thumbnails/thumbnail.png"));
    assert_eq!(after.len() + 1, files.len());
    for (n, b) in &files {
        if !matches!(n.as_str(), "content.xml" | "meta.xml" | "META-INF/manifest.xml" | "Thumbnails/thumbnail.png") {
            assert_eq!(after.get(n), Some(b), "changed untouched {n}");
        }
    }
    let expected_meta = String::from_utf8(files["meta.xml"].clone())
        .unwrap()
        .replace(" meta:word-count='4'", "")
        .replace(" meta:character-count=\"24\"", "")
        .replace(" meta:non-whitespace-character-count='20'", "")
        .replace(" meta:sentence-count='2'", "")
        .replace(" meta:syllable-count='6'", "");
    assert_eq!(after["meta.xml"], expected_meta.as_bytes());
    let expected_manifest = String::from_utf8(files["META-INF/manifest.xml"].clone())
        .unwrap()
        .replace("<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>", "");
    assert_eq!(after["META-INF/manifest.xml"], expected_manifest.as_bytes());
    let expected_content = String::from_utf8(files["content.xml"].clone())
        .unwrap()
        .replace("Original one", "Edited<dcpatch:s xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>first")
        .replace("Original two", "Changed<dcpatch:s xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>second");
    assert_eq!(after["content.xml"], expected_content.as_bytes());
    let reopened = import(&out).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "Edited first\nChanged second");
    let opts = deckcraft_render::RenderOpts { threads: 1, ..Default::default() };
    let image = deckcraft_render::render_slide(&p, 0, &opts);
    let actual = deckcraft_render::render_slide(&reopened, 0, &opts);
    assert_eq!(image.pixels, actual.pixels);
    assert_ne!(image.pixels, deckcraft_render::render_slide(&original, 0, &opts).pixels);
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
}

#[test]
fn thumbnail_only_and_statistics_only_and_empty_statistic_are_supported() {
    for (preview, stat) in [
        (true, ""),
        (false, COUNTS),
        (false, "<meta:document-statistic/>"),
        (false, "<meta:document-statistic meta:word-count='0'></meta:document-statistic>"),
    ] {
        let f = fixture(preview, stat);
        let source = zip(&f);
        assert!(export(&import(&source).unwrap()).is_err(), "no edit may invalidate caches");
        let out = parts(&export(&edit(&source)).unwrap());
        assert!(!out.contains_key("Thumbnails/thumbnail.png"));
        if !stat.is_empty() {
            assert!(!String::from_utf8_lossy(&out["meta.xml"]).contains("meta:word-count"));
        }
    }
}

#[test]
fn lexical_namespace_aliases_quotes_and_survivor_gaps_are_exact() {
    let stat = "<m:document-statistic xmlns:m='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'\n m:page-count = '1'\t m:word-count = \"&#49;2\"  m:paragraph-count='2'\r\n m:syllable-count='0' ></m:document-statistic>";
    let mut f = fixture(true, stat);
    replace(
        &mut f,
        "META-INF/manifest.xml",
        "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>",
        "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'> \n </manifest:file-entry>",
    );
    let after = parts(&export(&edit(&zip(&f))).unwrap());
    let expected =
        String::from_utf8(f["meta.xml"].clone()).unwrap().replace("\t m:word-count = \"&#49;2\"", "").replace("\r\n m:syllable-count='0'", "");
    assert_eq!(after["meta.xml"], expected.as_bytes());
}

#[test]
fn every_unused_style_and_unknown_scalar_looking_property_is_checked() {
    for bad in [
        "fo:letter-spacing='1pt'",
        "fo:text-shadow='none'",
        "fo:background-color='url(Thumbnails/thumbnail.png)'",
        "fo:font-size='NaNpt'",
        "fo:font-family='url&#40;Thumbnails/thumbnail.png&#41;'",
    ] {
        let mut f = fixture(true, COUNTS);
        replace(
            &mut f,
            "styles.xml",
            "<office:styles/>",
            &format!(
                "<office:styles><style:style style:name='Unused' style:family='text'><style:text-properties {bad}/></style:style></office:styles>"
            ),
        );
        refuses(&f);
    }
}

#[test]
fn dependency_scan_includes_unused_resources_metadata_settings_and_fields() {
    for bad in [
        "Thumbnails/thumbnail.png",
        "./Thumbnails/thumbnail.png",
        "Thumbnails/../Thumbnails/thumbnail.png",
        "Thumbnails%2fthumbnail.png",
        "file:///Thumbnails/thumbnail.png",
        "Thumbnails/thumbnail.png#x",
    ] {
        let mut f = fixture(true, COUNTS);
        replace(
            &mut f,
            "styles.xml",
            "<office:styles/>",
            &format!("<office:styles><draw:fill-image draw:name='Unused' xlink:href='{bad}'/></office:styles>"),
        );
        refuses(&f);
    }
    for (part, a, b) in [
        ("content.xml", "Original one", "<text:word-count>2</text:word-count>"),
        ("meta.xml", "</office:meta>", "<meta:user-defined meta:name='lookup'>meta:word-count</meta:user-defined></office:meta>"),
        (
            "styles.xml",
            "<office:styles/>",
            "<office:styles><style:style style:name='Unused' style:family='paragraph' style:parent-style-name='Missing'/></office:styles>",
        ),
    ] {
        let mut f = fixture(true, COUNTS);
        replace(&mut f, part, a, b);
        refuses(&f);
    }
    let mut f = fixture(true, COUNTS);
    f.insert("settings.xml".into(),format!("<office:document-settings {NS} xmlns:config='urn:oasis:names:tc:opendocument:xmlns:config:1.0' office:version='1.3'><office:settings><config:config-item-set config:name='x'/></office:settings></office:document-settings>").into_bytes());
    manifest(&mut f);
    refuses(&f);
}

#[test]
fn owner_and_png_guards_refuse_malformed_or_unproved_previews() {
    for variant in 0..9 {
        let mut f = fixture(true, COUNTS);
        match variant {
            0 => replace(
                &mut f,
                "META-INF/manifest.xml",
                "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>",
                "",
            ),
            1 => replace(&mut f, "META-INF/manifest.xml", "manifest:media-type='image/png'", "manifest:media-type='image/jpeg'"),
            2 => replace(
                &mut f,
                "META-INF/manifest.xml",
                "manifest:full-path='Thumbnails/thumbnail.png'",
                "xml:id='preview' manifest:full-path='Thumbnails/thumbnail.png'",
            ),
            3 => replace(
                &mut f,
                "META-INF/manifest.xml",
                "manifest:media-type='image/png'/>",
                "manifest:media-type='image/png'><!-- hidden --></manifest:file-entry>",
            ),
            4 => {
                f.insert("Thumbnails/other.png".into(), png());
                manifest(&mut f);
            }
            5 => {
                f.insert("Thumbnails/thumbnail.png".into(), b"not a PNG".to_vec());
            }
            6 => {
                let b = f.get_mut("Thumbnails/thumbnail.png").unwrap();
                b[16..20].copy_from_slice(&2049u32.to_be_bytes());
            }
            7 => {
                let b = f.get_mut("Thumbnails/thumbnail.png").unwrap();
                b[20..24].copy_from_slice(&0u32.to_be_bytes());
            }
            _ => {
                f.remove("Thumbnails/thumbnail.png");
            }
        }
        refuses(&f);
    }
}

#[test]
fn malformed_duplicate_misplaced_and_unknown_statistics_refuse() {
    for bad in [
        "<meta:document-statistic meta:word-count='-1'/>",
        "<meta:document-statistic meta:word-count='18446744073709551616'/>",
        "<meta:document-statistic meta:word-count='1.0'/>",
        "<meta:document-statistic meta:word-count='1' meta:unknown-count='2'/>",
        "<meta:document-statistic meta:word-count='1' xml:id='stat'/>",
        "<meta:document-statistic meta:word-count='1'><!--comment--></meta:document-statistic>",
        "<meta:document-statistic meta:word-count='1'><dc:title>x</dc:title></meta:document-statistic>",
        "<meta:document-statistic/><meta:document-statistic/>",
    ] {
        refuses(&fixture(true, bad));
    }
    let mut f = fixture(true, "");
    replace(&mut f, "styles.xml", "<office:styles/>", "<office:styles><meta:document-statistic/></office:styles>");
    refuses(&f);
}

#[test]
fn namespace_spoofing_signed_grammar_and_style_cycles_fail_closed() {
    for (part, a, b) in [
        ("content.xml", "fo:keep-with-next='always'", "xmlns:fo='urn:foreign' fo:keep-with-next='always'"),
        ("content.xml", "svg:width='300pt'", "svg:width='-300pt'"),
        ("content.xml", "fo:font-size='24pt'", "fo:font-size='100%'"),
        (
            "styles.xml",
            "<office:styles/>",
            "<office:styles><style:style style:name='A' style:family='paragraph' style:parent-style-name='B'/><style:style style:name='B' style:family='paragraph' style:parent-style-name='A'/></office:styles>",
        ),
    ] {
        let mut f = fixture(true, COUNTS);
        replace(&mut f, part, a, b);
        refuses(&f);
    }
}

#[test]
fn late_property_after_audit_detail_cap_and_definition_caps_are_checked() {
    let mut f = fixture(true, COUNTS);
    let mut unused = String::new();
    for i in 0..150 {
        unused.push_str(&format!(
            "<style:style style:name='U{i}' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style>"
        ));
    }
    unused.push_str("<style:style style:name='Bad' style:family='graphic'><style:graphic-properties draw:fill='hatch'/></style:style>");
    replace(&mut f, "styles.xml", "<office:styles/>", &format!("<office:styles>{unused}</office:styles>"));
    refuses(&f);
    let mut f = fixture(true, COUNTS);
    let mut unused = String::new();
    for i in 0..4100 {
        unused.push_str(&format!("<style:style style:name='U{i}' style:family='text'/>"));
    }
    replace(&mut f, "styles.xml", "<office:styles/>", &format!("<office:styles>{unused}</office:styles>"));
    refuses(&f);
}

#[test]
fn native_nontext_change_never_gets_a_cache_exception() {
    let source = zip(&fixture(true, COUNTS));
    let mut p = edit(&source);
    Arc::make_mut(&mut p.slides[0]).shapes[0].name.push('!');
    assert!(export(&p).is_err());
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
}
