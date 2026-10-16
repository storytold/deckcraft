//! Contributor-original fixtures for the deliberately narrow source-patch writer.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{Presentation, text::Run};
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'";
const PROPS: &str = "<style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:text-align-last='center'/></style:style>";
fn content(paragraphs: &str) -> String {
    format!(
        "<?xml version='1.0' encoding='UTF-8'?><office:document-content {NS} office:version='1.3'><office:automatic-styles>{PROPS}</office:automatic-styles><office:body><office:presentation><draw:page draw:name='Original page' draw:master-page-name='M' xml:id='page-a'><draw:frame draw:name='Original shape' xml:id='frame-a' svg:x='12pt' svg:y='24pt' svg:width='240pt' svg:height='120pt'><draw:text-box>{paragraphs}</draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    )
}
fn styles(common: &str, artwork: &str) -> String {
    format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles>{common}</office:styles><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='960pt' fo:page-height='540pt' style:print-orientation='landscape'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'>{artwork}</style:master-page></office:master-styles></office:document-styles>"
    )
}
fn zip_files(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", options).unwrap();
    zip.write_all(MIME.as_bytes()).unwrap();
    for (name, bytes) in files {
        if name == "mimetype" {
            continue;
        }
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn fixture(paragraphs: &str, extras: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let mut files = BTreeMap::from([("content.xml".into(), content(paragraphs).into_bytes()), ("styles.xml".into(), styles("", "").into_bytes())]);
    let mut owners = BTreeMap::from([("content.xml", "text/xml"), ("styles.xml", "text/xml")]);
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}'/>"
    );
    for (name, mime, bytes) in extras {
        files.insert((*name).into(), bytes.to_vec());
        owners.insert(name, mime);
    }
    for (name, mime) in owners {
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>"));
    }
    manifest.push_str("</manifest:manifest>");
    files.insert("META-INF/manifest.xml".into(), manifest.into_bytes());
    zip_files(&files)
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut file = zip.by_index(index).unwrap();
            let name = file.name().into();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn edit(p: &mut Presentation, shape: usize, paragraph: usize, value: &str) {
    let paragraph = &mut Arc::make_mut(&mut p.slides[0]).shapes[shape].text.as_mut().unwrap().paragraphs[paragraph];
    let props = paragraph.runs.first().unwrap().props.clone();
    paragraph.runs = vec![Run::with(value, props)];
}
fn edited(source: &[u8]) -> Presentation {
    let mut p = import(source).unwrap();
    edit(&mut p, 0, 0, "Edited text");
    p
}
fn replace_source(p: &mut Presentation, bytes: Vec<u8>) {
    p.media.iter_mut().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data = Arc::new(bytes);
}
fn encode(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            ' ' => "<dcpatch:s xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>".into(),
            '\t' => "<dcpatch:tab xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>".into(),
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            _ => c.to_string(),
        })
        .collect()
}

#[test]
fn real_text_edits_preserve_all_other_payload_and_xml_bytes() {
    let styles = styles("<style:style style:name='T' style:family='text'><style:text-properties fo:text-transform='uppercase'/></style:style>", "");
    let metadata = "<office:document-meta xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' office:version='1.3'><office:meta><dc:title>Contributor original</dc:title></office:meta></office:document-meta>";
    let raster = deckcraft_render::Image { width: 1, height: 1, pixels: vec![23, 97, 181, 255] }.to_png();
    let source = fixture(
        "<text:p text:style-name='P' xml:id='p-a'>Original one</text:p><text:p text:style-name='P' xml:id='p-b'><text:span text:style-name='T'>Original two</text:span></text:p>",
        &[
            ("styles.xml", "text/xml", styles.as_bytes()),
            ("meta.xml", "text/xml", metadata.as_bytes()),
            ("Pictures/original.png", "image/png", &raster),
        ],
    );
    let mut p = import(&source).unwrap();
    let first = "  Ω < & >\tlast  ";
    let second = "Second edit";
    edit(&mut p, 0, 0, first);
    edit(&mut p, 0, 1, second);
    let output = export(&p).expect("strict preservation should export proven local text edits");
    assert_ne!(output, source);
    let before = parts(&source);
    let after = parts(&output);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    for (name, bytes) in &before {
        if name != "content.xml" {
            assert_eq!(bytes, &after[name], "changed {name}");
        }
    }
    let expected =
        String::from_utf8(before["content.xml"].clone()).unwrap().replace("Original one", &encode(first)).replace("Original two", &encode(second));
    assert_eq!(after["content.xml"], expected.as_bytes());
    let reopened = import(&output).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), format!("{first}\n{second}"));
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
    if let Ok(directory) = std::env::var("DECKCRAFT_PRESERVATION_QA_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (name, bytes) in after {
            if name.ends_with(".xml") {
                std::fs::write(std::path::Path::new(&directory).join(name.replace('/', "_")), bytes).unwrap();
            }
        }
    }
}

#[test]
fn adjacent_identical_text_runs_are_equivalent_but_no_edits_still_refuse() {
    let source = fixture("<text:p text:style-name='P'>Original text</text:p>", &[]);
    let p = import(&source).unwrap();
    assert!(export(&p).is_err());
    let mut p = p;
    let paragraph = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0];
    let props = paragraph.runs[0].props.clone();
    paragraph.runs = vec![Run::with("Original", props.clone()), Run::with(" text", props.clone())];
    assert!(export(&p).is_err());
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs =
        vec![Run::with("New ", props.clone()), Run::with("value", props)];
    let output = export(&p).unwrap();
    assert_eq!(import(&output).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "New value");
}

#[test]
fn any_nontext_edit_or_forged_provenance_refuses_the_entire_export() {
    let source = fixture("<text:p text:style-name='P'>Original text</text:p>", &[]);
    for variant in 0..10 {
        let mut p = edited(&source);
        let slide = Arc::make_mut(&mut p.slides[0]);
        match variant {
            0 => slide.name.push('!'),
            1 => slide.shapes[0].xfrm.as_mut().unwrap().w += 1.0,
            2 => slide.shapes[0].text.as_mut().unwrap().body.inset_l = Some(18.0),
            3 => slide.shapes[0].text.as_mut().unwrap().paragraphs[0].def_props.bold = Some(true),
            4 => slide.shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].props.bold = Some(true),
            5 => slide.notes = deckcraft_model::text::TextBody::from_text("Changed notes"),
            6 => slide.shapes[0].tags.iter_mut().find(|(k, _)| k == "odp:source-range").unwrap().1 = "v1:0:10".into(),
            7 => slide.shapes[0].tags.iter_mut().find(|(k, _)| k == "odp:source-part").unwrap().1 = "styles.xml".into(),
            8 => slide.shapes[0].text.as_mut().unwrap().paragraphs.push(Default::default()),
            _ => p.next_id += 1,
        }
        assert!(export(&p).is_err(), "variant {variant}");
    }
}

#[test]
fn mixed_nested_field_list_and_lexical_containers_stay_guarded() {
    for paragraph in [
        "<text:p text:style-name='P'><text:span>First</text:span><text:span>second</text:span></text:p>",
        "<text:p text:style-name='P'><text:span><text:span>Nested</text:span></text:span></text:p>",
        "<text:p text:style-name='P'>Before<text:page-number>1</text:page-number></text:p>",
        "<text:list><text:list-item><text:p text:style-name='P'>Listed</text:p></text:list-item></text:list>",
        "<text:p text:style-name='P'>Original<!-- retained comment -->text</text:p>",
        "<text:p text:style-name='P'>Original<?custom data?>text</text:p>",
        "<text:p text:style-name='P'>Original<text:tab text:tab-ref='1'/>text</text:p>",
    ] {
        let source = fixture(paragraph, &[]);
        let p = edited(&source);
        assert!(export(&p).is_err(), "accepted {paragraph}");
    }
}

#[test]
fn master_artwork_never_grants_local_edit_ownership_even_for_one_copy() {
    for pages in 1..=2 {
        let source = fixture("<text:p text:style-name='P'>Local</text:p>", &[]);
        let mut files = parts(&source);
        let original = String::from_utf8(files["content.xml"].clone()).unwrap();
        let page = "<draw:page draw:master-page-name='M'/>";
        let content = original.replace(
            original.split("<draw:page").nth(1).unwrap().split("</draw:page>").next().map(|v| format!("<draw:page{v}</draw:page>")).unwrap().as_str(),
            &page.repeat(pages),
        );
        files.insert("content.xml".into(), content.into_bytes());
        let styles =
            styles("", "<draw:frame svg:width='100pt' svg:height='40pt'><draw:text-box><text:p>Master text</text:p></draw:text-box></draw:frame>");
        files.insert("styles.xml".into(), styles.into_bytes());
        let mut p = import(&zip_files(&files)).unwrap();
        assert_eq!(p.slides.len(), pages);
        assert_eq!(p.masters.len(), 1);
        assert_eq!(p.masters[0].shapes.len(), 1);
        assert_eq!(p.masters[0].layouts.len(), 1);
        let layout = &p.masters[0].layouts[0];
        assert!(layout.shapes.is_empty());
        assert!(layout.show_master_shapes);
        for slide in &p.slides {
            assert_eq!(slide.layout, layout.id);
            assert!(slide.show_master_shapes);
            assert!(slide.shapes.is_empty(), "master artwork must not become local copies");
        }
        let shape = &p.masters[0].shapes[0];
        assert_eq!(shape.tags.iter().find(|(key, _)| key == "odp:source-part").unwrap().1, "styles.xml");
        assert!(shape.tags.iter().find(|(key, _)| key == "odp:source-range").unwrap().1.starts_with("v1:"));
        assert_eq!(shape.text.as_ref().unwrap().text(), "Master text");
        let paragraph = &mut Arc::make_mut(&mut p.masters[0]).shapes[0].text.as_mut().unwrap().paragraphs[0];
        paragraph.runs = vec![Run::with("Changed master copy", paragraph.runs[0].props.clone())];
        assert_eq!(p.masters[0].shapes[0].text.as_ref().unwrap().text(), "Changed master copy");
        assert!(export(&p).is_err(), "pages {pages}");
    }
}

#[test]
fn a_missing_named_master_remains_an_import_error() {
    let source = fixture("<text:p text:style-name='P'>Original</text:p>", &[]);
    let mut files = parts(&source);
    let content = String::from_utf8(files["content.xml"].clone()).unwrap().replace("draw:master-page-name='M'", "draw:master-page-name='Missing'");
    files.insert("content.xml".into(), content.into_bytes());
    assert!(import(&zip_files(&files)).unwrap_err().to_string().contains("master"));
}

#[test]
fn cache_active_external_and_unknown_parts_refuse() {
    let paragraphs = "<text:p text:style-name='P'>Original text</text:p>";
    let meta = "<office:document-meta xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'><office:meta><meta:document-statistic meta:word-count='2'/></office:meta></office:document-meta>";
    for extra in [
        ("Thumbnails/thumbnail.png", "image/png", b"\x89PNG\r\n\x1a\n".as_slice()),
        ("unknown.bin", "application/octet-stream", b"passive-looking data".as_slice()),
        ("META-INF/documentsignatures.xml", "text/xml", b"<signature/>".as_slice()),
        ("meta.xml", "text/xml", meta.as_bytes()),
        ("Pictures/fake.png", "image/png", b"not a PNG".as_slice()),
    ] {
        let source = fixture(paragraphs, &[extra]);
        let p = edited(&source);
        assert!(export(&p).is_err(), "accepted {}", extra.0);
    }
    for content_fragment in [
        "<text:p text:style-name='P'><text:a xlink:href='https://example.invalid'>Original</text:a></text:p>",
        "<text:p text:style-name='P'>Original<office:event-listeners/></text:p>",
        "<text:p text:style-name='P'>Original<text:script/></text:p>",
    ] {
        let source = fixture(content_fragment, &[]);
        assert!(export(&edited(&source)).is_err());
    }
}

#[test]
fn package_ambiguity_corruption_and_oversized_claims_fail_before_patching() {
    let source = fixture("<text:p text:style-name='P'>Original text</text:p>", &[]);
    let end = source.len() - 22;
    let cd = u32::from_le_bytes(source[end + 16..end + 20].try_into().unwrap()) as usize;
    let name_len = u16::from_le_bytes(source[cd + 28..cd + 30].try_into().unwrap()) as usize;
    let first_len = 46 + name_len;
    let mut duplicate = source[..end].to_vec();
    duplicate.extend_from_slice(&source[cd..cd + first_len]);
    duplicate.extend_from_slice(&source[end..]);
    let new_end = duplicate.len() - 22;
    let count = u16::from_le_bytes(source[end + 10..end + 12].try_into().unwrap()) + 1;
    duplicate[new_end + 8..new_end + 10].copy_from_slice(&count.to_le_bytes());
    duplicate[new_end + 10..new_end + 12].copy_from_slice(&count.to_le_bytes());
    let size = u32::from_le_bytes(source[end + 12..end + 16].try_into().unwrap()) + first_len as u32;
    duplicate[new_end + 12..new_end + 16].copy_from_slice(&size.to_le_bytes());
    let mut wrong_local_name = source.clone();
    wrong_local_name[30] = b'x';
    let mut crc = source.clone();
    let text = crc.windows(b"Original text".len()).position(|s| s == b"Original text").unwrap();
    crc[text] = b'X';
    let mut large = source.clone();
    large[cd + 24..cd + 28].copy_from_slice(&(33u32 * 1024 * 1024).to_le_bytes());
    let mut too_many = source.clone();
    too_many[end + 8..end + 10].copy_from_slice(&10_001u16.to_le_bytes());
    too_many[end + 10..end + 12].copy_from_slice(&10_001u16.to_le_bytes());
    for bytes in [duplicate, wrong_local_name, crc, large, too_many] {
        let mut p = edited(&source);
        replace_source(&mut p, bytes);
        assert!(export(&p).unwrap_err().to_string().contains("ZIP32"));
    }
}

#[test]
fn encoding_bounds_and_non_xml_characters_never_publish_partial_edits() {
    let source = fixture("<text:p text:style-name='P'>Original text</text:p>", &[]);
    for value in ["", "new\nparagraph", "new\rtext", "new\u{b}break", "bad\0text", "bad\u{1}text"] {
        let mut p = edited(&source);
        edit(&mut p, 0, 0, value);
        assert!(export(&p).is_err(), "accepted {value:?}");
    }
    let mut p = edited(&source);
    edit(&mut p, 0, 0, &" ".repeat(500_000));
    assert!(export(&p).is_err());
}

#[test]
fn cumulative_patch_growth_and_inherited_run_string_clones_are_precharged() {
    let source = fixture("<text:p text:style-name='P'>One</text:p><text:p text:style-name='P'>Two</text:p>", &[]);
    let mut p = import(&source).unwrap();
    let spaces = " ".repeat(240_000);
    edit(&mut p, 0, 0, &spaces);
    edit(&mut p, 0, 1, &spaces);
    assert!(export(&p).unwrap_err().to_string().contains("encoded text exceeds budget"));

    let mut files = parts(&fixture("<text:p text:style-name='P'>Original</text:p>", &[]));
    let content = String::from_utf8(files["content.xml"].clone()).unwrap().replace(
        "<style:paragraph-properties fo:text-align-last='center'/>",
        &format!("<style:paragraph-properties fo:text-align-last='center'/><style:text-properties fo:font-family='{}'/>", "F".repeat(8192)),
    );
    files.insert("content.xml".into(), content.into_bytes());
    let mut p = import(&zip_files(&files)).unwrap();
    let paragraph = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0];
    assert_eq!(paragraph.runs[0].props.font.as_ref().unwrap().len(), 8192);
    paragraph.runs = vec![Run::with("X", paragraph.runs[0].props.clone()); 4096];
    assert!(export(&p).unwrap_err().to_string().contains("edited text budget exceeded"));
}

#[test]
fn an_ascii_source_with_non_utf8_declaration_cannot_receive_utf8_text() {
    let source = fixture("<text:p text:style-name='P'>Original</text:p>", &[]);
    let mut files = parts(&source);
    let content = String::from_utf8(files["content.xml"].clone()).unwrap().replace("encoding='UTF-8'", "encoding='ISO-8859-1'");
    files.insert("content.xml".into(), content.into_bytes());
    let mut p = import(&zip_files(&files)).unwrap();
    edit(&mut p, 0, 0, "Ω edited");
    assert!(export(&p).unwrap_err().to_string().contains("UTF-8"));
}
