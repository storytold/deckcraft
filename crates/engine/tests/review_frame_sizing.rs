//! Independent engine-command boundaries for source-retained ODF frame constraints.
use deckcraft_engine::{DocState, Session, cmd::file};
use deckcraft_model::text::AutoFit;
use deckcraft_odp::{MIME, import};
use serde_json::json;
use std::{collections::BTreeMap, io::Cursor, io::Read, io::Write};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'";
type Parts = BTreeMap<String, Vec<u8>>;

fn fixture(properties: &str, width: u32, height: u32, cache: bool) -> (Vec<u8>, Parts) {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='EngineFrame' style:family='graphic'><style:graphic-properties {properties}/></style:style><style:style style:name='Keep' style:family='paragraph'><style:paragraph-properties fo:keep-together='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Engine review'><draw:frame xml:id='frame' draw:style-name='EngineFrame' svg:width='{width}pt' svg:height='{height}pt'><draw:text-box><text:p text:style-name='Keep'>Original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let statistic = if cache { "<meta:document-statistic meta:page-count='1' meta:character-count='8'/>" } else { "" };
    let meta = format!(
        "<office:document-meta {NS} office:version='1.3'><office:meta><!--immutable frame source-->{statistic}</office:meta></office:document-meta>"
    );
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='meta.xml' manifest:media-type='text/xml'/></manifest:manifest>"
    );
    let parts: Parts = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        ("META-INF/manifest.xml".into(), manifest.into_bytes()),
    ]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in std::iter::once("mimetype").chain(parts.keys().map(String::as_str).filter(|name| *name != "mimetype")) {
        zip.start_file(name, stored).unwrap();
        zip.write_all(&parts[name]).unwrap();
    }
    (zip.finish().unwrap().into_inner(), parts)
}

fn unpack(bytes: &[u8]) -> Parts {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = Parts::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        assert!(parts.insert(member.name().into(), bytes).is_none());
    }
    parts
}

fn open(source: &[u8]) -> Session {
    let presentation = file::open_presentation("independent-frame.odp", source).unwrap();
    let baseline = file::save_bytes(&presentation, "odp").unwrap_err().to_string();
    assert!(baseline.contains("no supported text edits"), "baseline must pass cache/audit/ownership proof: {baseline}");
    let mut session = Session::new();
    session.add_document(DocState::new(presentation, None, "Independent frame review".into()));
    session
}

fn change(session: &mut Session, text: &str) {
    let id = session.active().unwrap().doc.slides[0].shapes[0].id.0;
    session.execute("text.edit", &json!({"id":id})).unwrap();
    session.execute("text.selectAll", &json!({})).unwrap();
    session.execute("text.insert", &json!({"text":text})).unwrap();
    assert_eq!(session.active().unwrap().doc.slides[0].shapes[0].text.as_ref().unwrap().text(), text);
}

fn recovery(session: &mut Session, source: &[u8]) {
    assert_eq!(file::retained_odp_source(&session.active().unwrap().doc).unwrap(), source);
    let first = session.execute("file.sourceBytes", &json!({})).unwrap();
    assert_eq!(first["name"], "original.odp");
    assert!(!first["data"].as_str().unwrap().is_empty());
    let native = file::save_bytes(&session.active().unwrap().doc, "deckcraft").unwrap();
    let reopened = deckcraft_format::load(&native).unwrap();
    assert_eq!(file::retained_odp_source(&reopened).unwrap(), source);
}

fn refuse_changed_geometry_or_scale(session: &mut Session, source: &[u8]) {
    let frozen = session.active().unwrap().doc.clone();
    let error = file::save_bytes(&frozen, "odp").unwrap_err().to_string();
    assert!(error.contains("changes outside plain slide text"), "native fitting changes must remain outside text preservation: {error}");
    assert_eq!(*session.active().unwrap().doc, *frozen);
    let native = file::save_bytes(&frozen, "deckcraft").unwrap();
    let reopened = deckcraft_format::load(&native).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].xfrm, frozen.slides[0].shapes[0].xfrm);
    assert_eq!(reopened.slides[0].shapes[0].text, frozen.slides[0].shapes[0].text);
    assert!(file::save_bytes(&reopened, "odp").unwrap_err().to_string().contains("changes outside plain slide text"));
    recovery(session, source);
}

#[test]
fn short_engine_command_edits_export_when_geometry_and_body_stay_unchanged() {
    for cache in [false, true] {
        for properties in [
            "draw:auto-grow-height='false' draw:fit-to-size='false' fo:wrap-option='wrap' fo:min-height='0pt' fo:min-width='10pt'",
            "style:shrink-to-fit='true' draw:auto-grow-height='false' draw:fit-to-size='false' fo:wrap-option='no-wrap'",
        ] {
            let (source, parts) = fixture(properties, 400, 200, cache);
            let mut session = open(&source);
            let original = session.active().unwrap().doc.clone();
            change(&mut session, "Revised");
            let after = &session.active().unwrap().doc.slides[0].shapes[0];
            assert_eq!(after.xfrm, original.slides[0].shapes[0].xfrm);
            assert_eq!(after.text.as_ref().unwrap().body, original.slides[0].shapes[0].text.as_ref().unwrap().body);
            let frozen = session.active().unwrap().doc.clone();
            let output = file::save_bytes(&frozen, "odp").unwrap();
            assert_eq!(*session.active().unwrap().doc, *frozen);
            let mut expected = parts;
            let content = expected.get_mut("content.xml").unwrap();
            *content = String::from_utf8(content.clone()).unwrap().replace(">Original<", ">Revised<").into_bytes();
            if cache {
                let meta = expected.get_mut("meta.xml").unwrap();
                *meta = String::from_utf8(meta.clone()).unwrap().replace(" meta:character-count='8'", "").into_bytes();
            }
            assert_eq!(unpack(&output), expected);
            let reopened = import(&output).unwrap();
            assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "Revised");
            recovery(&mut session, &source);
        }
    }
}

#[test]
fn actual_engine_autogrow_height_growth_refuses_text_only_export_and_keeps_recovery() {
    for cache in [false, true] {
        let (source, _) = fixture("draw:auto-grow-height='true' fo:wrap-option='wrap' fo:min-height='0pt'", 100, 12, cache);
        let mut session = open(&source);
        let original = session.active().unwrap().doc.clone();
        let before = original.slides[0].shapes[0].xfrm.unwrap();
        change(&mut session, &"Expansion needs wrapping ".repeat(60));
        let after = session.active().unwrap().doc.slides[0].shapes[0].xfrm.unwrap();
        assert!(after.h > before.h, "real engine command must grow geometry: {before:?} -> {after:?}");
        assert_eq!(after.w, before.w);
        refuse_changed_geometry_or_scale(&mut session, &source);
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(*session.active().unwrap().doc, *original);
    }
}

#[test]
fn actual_engine_autogrow_height_shrink_cannot_authenticate_unmodeled_minima() {
    for cache in [false, true] {
        let (source, _) = fixture("draw:auto-grow-height='true' fo:wrap-option='wrap' fo:min-height='180pt' fo:min-width='300pt'", 400, 240, cache);
        let mut session = open(&source);
        let before = session.active().unwrap().doc.slides[0].shapes[0].xfrm.unwrap();
        change(&mut session, "Small");
        let after = session.active().unwrap().doc.slides[0].shapes[0].xfrm.unwrap();
        assert!(
            after.h < before.h && after.h < 180.0,
            "minimum constraints remain source-only; native shrink must not pass preservation: {before:?} -> {after:?}"
        );
        refuse_changed_geometry_or_scale(&mut session, &source);
    }
}

#[test]
fn actual_engine_shrink_scale_is_persisted_but_refuses_plain_text_source_export() {
    for cache in [false, true] {
        let (source, _) = fixture("style:shrink-to-fit='true' draw:fit-to-size='false' fo:wrap-option='wrap'", 80, 18, cache);
        let mut session = open(&source);
        let geometry = session.active().unwrap().doc.slides[0].shapes[0].xfrm;
        change(&mut session, &"Saved shrinking text ".repeat(80));
        let after = &session.active().unwrap().doc.slides[0].shapes[0];
        assert_eq!(after.xfrm, geometry);
        let Some(AutoFit::Shrink { font_scale, line_reduction }) = after.text.as_ref().unwrap().body.autofit else {
            panic!("expected native shrink mode")
        };
        assert!(
            font_scale.is_some_and(|scale| scale < 1.0) || line_reduction > 0.0,
            "actual engine layout must store a changed fit: {font_scale:?}/{line_reduction}"
        );
        refuse_changed_geometry_or_scale(&mut session, &source);
    }
}
