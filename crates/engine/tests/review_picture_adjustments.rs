//! Independent W12 real-engine text/picture command and recovery boundaries.
use deckcraft_engine::{DocState, Session, cmd::file};
use deckcraft_model::{Presentation, ShapeKind};
use deckcraft_odp::{MIME, import};
use serde_json::json;
use std::{collections::BTreeMap, io::Cursor, io::Read, io::Write, sync::Arc};

type Parts = BTreeMap<String, Vec<u8>>;
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'";

fn fixture(cache: bool) -> (Vec<u8>, Parts) {
    fixture_with_text_properties(cache, "")
}

fn fixture_with_text_properties(cache: bool, text_properties: &str) -> (Vec<u8>, Parts) {
    let controls = "draw:red='0%' draw:green='0%' draw:blue='0%' style:mirror='none' draw:image-opacity='75%' draw:luminance='12.5%' draw:contrast='-25%' draw:color-mode='greyscale'";
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='Image' style:family='graphic'><style:graphic-properties {controls}/></style:style><style:style style:name='Text' style:family='graphic'><style:graphic-properties draw:auto-grow-height='false' draw:fit-to-size='false' fo:wrap-option='wrap'/></style:style><style:style style:name='Keep' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/>{text_properties}</style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Independent engine picture'><draw:frame xml:id='picture' draw:style-name='Image' svg:x='20pt' svg:y='20pt' svg:width='80pt' svg:height='80pt'><draw:image xlink:href='Pictures/engine.png'/></draw:frame><!--immutable gap--><draw:frame xml:id='text' draw:style-name='Text' svg:x='160pt' svg:y='20pt' svg:width='400pt' svg:height='200pt'><draw:text-box><text:p text:style-name='Keep'>Original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let statistic = if cache { "<meta:document-statistic meta:word-count='1' meta:page-count='1'/>" } else { "" };
    let meta =
        format!("<office:document-meta {NS} office:version='1.3'><office:meta><!--metadata gap-->{statistic}</office:meta></office:document-meta>");
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='meta.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='Pictures/engine.png' manifest:media-type='image/png'/></manifest:manifest>"
    );
    let raster = deckcraft_render::Image { width: 2, height: 1, pixels: vec![31, 139, 211, 255, 193, 83, 47, 255] }.to_png();
    let parts: Parts = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        ("META-INF/manifest.xml".into(), manifest.into_bytes()),
        ("Pictures/engine.png".into(), raster),
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
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = Parts::new();
    for index in 0..zip.len() {
        let mut member = zip.by_index(index).unwrap();
        let mut payload = Vec::new();
        member.read_to_end(&mut payload).unwrap();
        assert!(parts.insert(member.name().into(), payload).is_none());
    }
    parts
}

fn picture_pixels(p: &Presentation) -> Vec<u8> {
    assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Picture { .. }));
    deckcraft_render::render_shape(p, &p.slides[0], p.slides[0].shapes[0].id, 1.0).unwrap().pixels
}

fn open(source: &[u8]) -> Session {
    let p = file::open_presentation("original-engine-picture.odp", source).unwrap();
    let baseline = file::save_bytes(&p, "odp").unwrap_err().to_string();
    assert!(baseline.contains("no supported text edits"), "baseline must pass cache/audit proof: {baseline}");
    let mut session = Session::new();
    session.add_document(DocState::new(p, None, "Independent W12".into()));
    session
}

fn text_change(session: &mut Session) {
    let original = session.active().unwrap().doc.clone();
    let original_shape = &original.slides[0].shapes[1];
    let original_body = original_shape.text.as_ref().unwrap();
    assert_eq!(original_body.body.autofit, Some(deckcraft_model::text::AutoFit::None));
    replace_text_with_commands(session);
    let current = &session.active().unwrap().doc;
    let shape = &current.slides[0].shapes[1];
    let body = shape.text.as_ref().unwrap();
    assert_eq!(shape.xfrm, original_shape.xfrm, "eligible fixture must not refit geometry");
    assert_eq!(body.body, original_body.body, "eligible fixture must not change body settings");
    assert_eq!(body.list_style, original_body.list_style);
    assert_eq!(body.paragraphs.len(), original_body.paragraphs.len());
    for (after, before) in body.paragraphs.iter().zip(&original_body.paragraphs) {
        assert_eq!(after.props, before.props, "paragraph formatting stays unchanged");
        assert_eq!(after.level, before.level);
        assert_eq!(after.def_props, before.def_props, "paragraph defaults stay unchanged");
        assert_eq!(after.end_props, before.end_props, "paragraph end mark stays unchanged");
    }
    let mut expected = (*original).clone();
    Arc::make_mut(&mut expected.slides[0]).shapes[1].text.as_mut().unwrap().paragraphs[0].runs = body.paragraphs[0].runs.clone();
    assert_eq!(**current, expected, "actual commands may change only the existing paragraph's runs");
}

fn replace_text_with_commands(session: &mut Session) {
    let id = session.active().unwrap().doc.slides[0].shapes[1].id.0;
    session.execute("text.edit", &json!({"id": id})).unwrap();
    session.execute("text.selectAll", &json!({})).unwrap();
    session.execute("text.insert", &json!({"text": "Revised"})).unwrap();
    assert_eq!(session.active().unwrap().doc.slides[0].shapes[1].text.as_ref().unwrap().text(), "Revised");
}

fn recovery(session: &mut Session, source: &[u8]) {
    assert_eq!(file::retained_odp_source(&session.active().unwrap().doc).unwrap(), source);
    let queried = session.execute("file.sourceBytes", &json!({})).unwrap();
    assert_eq!(queried["name"], "original.odp");
    assert!(!queried["data"].as_str().unwrap().is_empty());
    let native = file::save_bytes(&session.active().unwrap().doc, "deckcraft").unwrap();
    let reopened = deckcraft_format::load(&native).unwrap();
    assert_eq!(file::retained_odp_source(&reopened).unwrap(), source, "native save/load retains exact recovery bytes");
}

#[test]
fn independent_engine_text_command_preserves_nondefault_picture_bytes_model_and_render() {
    for cache in [false, true] {
        let (source, mut expected) = fixture(cache);
        let mut session = open(&source);
        let original = session.active().unwrap().doc.clone();
        let pixels = picture_pixels(&original);
        text_change(&mut session);
        assert_eq!(session.active().unwrap().doc.slides[0].shapes[0], original.slides[0].shapes[0]);
        assert_eq!(picture_pixels(&session.active().unwrap().doc), pixels);
        let frozen = session.active().unwrap().doc.clone();
        let output = file::save_bytes(&frozen, "odp").unwrap();
        assert_eq!(*session.active().unwrap().doc, *frozen);
        let content = expected.get_mut("content.xml").unwrap();
        *content = String::from_utf8(content.clone()).unwrap().replace(">Original<", ">Revised<").into_bytes();
        if cache {
            let metadata = expected.get_mut("meta.xml").unwrap();
            *metadata = String::from_utf8(metadata.clone()).unwrap().replace(" meta:word-count='1'", "").into_bytes();
        }
        assert_eq!(unpack(&output), expected, "every raster/style/gap byte survives actual engine text commands");
        let reopened = import(&output).unwrap();
        assert_eq!(picture_pixels(&reopened), pixels);
        assert_eq!(reopened.slides[0].shapes[0].kind, original.slides[0].shapes[0].kind);
        recovery(&mut session, &source);
    }
}

#[test]
fn independent_engine_picture_commands_stay_outside_text_export_and_undo_recovers() {
    for cache in [false, true] {
        for (command, params) in [
            ("picture.adjust", json!({"brightness": 0.4})),
            ("picture.adjust", json!({"transparency": 0.5})),
            ("picture.crop", json!({"left": 0.1})),
            ("arrange.flipHorizontal", json!({})),
        ] {
            let (source, _) = fixture(cache);
            let mut session = open(&source);
            text_change(&mut session);
            let text_edited = session.active().unwrap().doc.clone();
            let picture = text_edited.slides[0].shapes[0].id.0;
            session.execute("edit.deselect", &json!({})).unwrap();
            session.execute("edit.select", &json!({"ids": [picture]})).unwrap();
            session.execute(command, &params).unwrap();
            assert_ne!(*session.active().unwrap().doc, *text_edited, "actual picture command must mutate the model");
            let frozen = session.active().unwrap().doc.clone();
            let error = file::save_bytes(&frozen, "odp").unwrap_err().to_string();
            assert!(error.contains("changes outside plain slide text"), "picture command {command}: {error}");
            assert_eq!(*session.active().unwrap().doc, *frozen);
            recovery(&mut session, &source);
            session.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(*session.active().unwrap().doc, *text_edited);
            assert!(file::save_bytes(&session.active().unwrap().doc, "odp").is_ok(), "undo restores text-only export eligibility");
        }
    }
}

#[test]
fn independent_engine_styled_full_replacement_materializes_end_mark_and_stays_guarded() {
    for cache in [false, true] {
        let (source, _) = fixture_with_text_properties(cache, "<style:text-properties fo:font-family='Liberation Sans' fo:font-size='16pt'/>");
        let mut session = open(&source);
        let original = session.active().unwrap().doc.clone();
        let before = &original.slides[0].shapes[1].text.as_ref().unwrap().paragraphs[0];
        assert_ne!(before.end_props, before.runs[0].props, "this fixture must exercise end-mark materialization");
        replace_text_with_commands(&mut session);
        let frozen = session.active().unwrap().doc.clone();
        let after = &frozen.slides[0].shapes[1].text.as_ref().unwrap().paragraphs[0];
        assert_eq!(after.end_props, before.runs[0].props, "full deletion seeds the empty paragraph's end mark from its deleted run");
        assert_eq!(after.props, before.props);
        assert_eq!(after.def_props, before.def_props);
        let mut expected = (*original).clone();
        let paragraph = &mut Arc::make_mut(&mut expected.slides[0]).shapes[1].text.as_mut().unwrap().paragraphs[0];
        paragraph.runs = after.runs.clone();
        paragraph.end_props = after.end_props.clone();
        assert_eq!(*frozen, expected, "the exact synthetic model delta is runs plus the derived paragraph end mark");
        let error = file::save_bytes(&frozen, "odp").unwrap_err().to_string();
        assert!(error.contains("changes outside plain slide text"), "end-mark materialization remains outside the source-patch contract: {error}");
        assert_eq!(*session.active().unwrap().doc, *frozen);
        recovery(&mut session, &source);
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(*session.active().unwrap().doc, *original, "undo restores the exact original styled model");
        assert!(file::save_bytes(&session.active().unwrap().doc, "odp").unwrap_err().to_string().contains("no supported text edits"));
        recovery(&mut session, &source);
    }
}

// W14 root integration regression: the actual engine path retains source-only gamma.
fn gamma_fixture(cache: bool, passive: bool) -> (Vec<u8>, Parts) {
    let (_, mut parts) = fixture(cache);
    let content = String::from_utf8(parts["content.xml"].clone()).unwrap();
    let content = content.replace("draw:red='0%'", "draw:red='0%' draw:gamma='125.5%'");
    let content = if passive { content } else { content.replace("fo:keep-with-next='always'", "") };
    parts.insert("content.xml".into(), content.into_bytes());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in std::iter::once("mimetype").chain(parts.keys().map(String::as_str).filter(|name| *name != "mimetype")) {
        writer.start_file(name, stored).unwrap();
        writer.write_all(&parts[name]).unwrap();
    }
    (writer.finish().unwrap().into_inner(), parts)
}

#[test]
fn engine_gamma_source_instruction_survives_actual_text_commands_and_native_reopen() {
    for cache in [false, true] {
        let (source, mut expected) = gamma_fixture(cache, true);
        let mut session = open(&source);
        let before = session.active().unwrap().doc.clone();
        let pixels = picture_pixels(&before);
        text_change(&mut session);
        let output = file::save_bytes(&session.active().unwrap().doc, "odp").unwrap();
        let content = String::from_utf8(expected["content.xml"].clone()).unwrap().replace(">Original<", ">Revised<");
        expected.insert("content.xml".into(), content.into_bytes());
        if cache {
            let meta = String::from_utf8(expected["meta.xml"].clone()).unwrap().replace(" meta:word-count='1'", "");
            expected.insert("meta.xml".into(), meta.into_bytes());
        }
        assert_eq!(unpack(&output), expected);
        assert_eq!(picture_pixels(&import(&output).unwrap()), pixels, "preview approximation remains stable; no native gamma identity claim");
        recovery(&mut session, &source);
        for format in ["pptx", "potx", "ppsx"] {
            assert!(file::save_bytes(&session.active().unwrap().doc, format).is_err());
        }
    }
}

#[test]
fn engine_gamma_only_and_legacy_missing_diagnostics_fail_closed_before_native_interchange() {
    const WARNING: &str =
        "ODF picture gamma has no native rendering mapping; source instructions are retained; edited export requires verified source preservation.";
    for passive in [false, true] {
        let (source, _) = gamma_fixture(false, passive);
        let mut presentation = import(&source).unwrap();
        assert!(presentation.tags.iter().any(|(key, value)| key == "odp:warning" && value == WARNING));
        let original = presentation.clone();
        let mut missing_provenance = presentation.clone();
        missing_provenance.tags.retain(|(key, _)| key != "odp:imported");
        for format in ["odp", "pptx", "potx", "ppsx"] {
            assert!(
                file::save_bytes(&missing_provenance, format).is_err(),
                "source-only gamma cannot borrow a diagnostic without imported provenance: {format}"
            );
        }
        if !passive {
            assert!(file::save_bytes(&presentation, "odp").is_err());
        }
        // Simulate a native file saved before the new source-only diagnostic existed.
        presentation.tags.retain(|(key, value)| !(key == "odp:warning" && value == WARNING));
        let legacy = deckcraft_format::load(&deckcraft_format::save(&presentation).unwrap()).unwrap();
        for format in ["odp", "pptx", "potx", "ppsx"] {
            assert!(file::save_bytes(&legacy, format).is_err(), "old retained source must never silently regenerate gamma away: {format}");
        }
        assert_eq!(file::retained_odp_source(&legacy).unwrap(), source);
        assert_eq!(original.media, legacy.media);
    }
}

// W16 diagnostic-only protection guard, including a genuinely imported notes preview.
fn protection_fixture(cache: bool, passive: bool) -> Vec<u8> {
    let (_, mut parts) = fixture(cache);
    let content = String::from_utf8(parts["content.xml"].clone()).unwrap();
    let content = content.replace("<office:automatic-styles>", "<office:automatic-styles><style:style style:name='Preview' style:family='graphic'><style:graphic-properties style:protect='position'/></style:style>");
    let content = content.replace("</draw:page>", "<presentation:notes xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'><draw:page-thumbnail draw:style-name='Preview' draw:page-number='1' svg:x='20pt' svg:y='20pt' svg:width='300pt' svg:height='180pt'/></presentation:notes></draw:page>");
    let content = if passive { content } else { content.replace("fo:keep-with-next='always'", "") };
    parts.insert("content.xml".into(), content.into_bytes());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in std::iter::once("mimetype").chain(parts.keys().map(String::as_str).filter(|name| *name != "mimetype")) {
        writer.start_file(name, stored).unwrap();
        writer.write_all(&parts[name]).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const PROTECTION_WARNING: &str =
    "ODF frame protection is not fully represented natively; original source is retained and interchange export is blocked.";

#[test]
fn engine_protection_warning_blocks_interchange_after_real_text_commands_and_native_reopen() {
    for cache in [false, true] {
        for passive in [false, true] {
            let source = protection_fixture(cache, passive);
            let p = file::open_presentation("original-protection.odp", &source).unwrap();
            assert_eq!(p.tags.iter().filter(|(key, value)| key == "odp:warning" && value == PROTECTION_WARNING).count(), 1);
            let page = p.slides[0].notes_page.as_ref().unwrap();
            let preview = page.shapes.iter().find(|shape| shape.ph_type() == Some(deckcraft_model::PhType::SlideImage)).unwrap();
            assert!(!preview.locked, "source protection is diagnosed, not invented as a native full lock");
            let mut session = Session::new();
            session.add_document(DocState::new(p, None, "Original protection".into()));
            text_change(&mut session);
            let before = session.active().unwrap().doc.clone();
            for format in ["odp", "pptx", "potx", "ppsx"] {
                assert!(
                    file::save_bytes(&before, format).is_err(),
                    "unrepresented protection must not disappear after an actual text edit: {format}"
                );
            }
            assert_eq!(*session.active().unwrap().doc, *before);
            recovery(&mut session, &source);
            let native = file::save_bytes(&before, "deckcraft").unwrap();
            let reopened = deckcraft_format::load(&native).unwrap();
            assert_eq!(reopened.tags, before.tags);
            assert_eq!(reopened.slides, before.slides);
            assert!(file::save_bytes(&reopened, "odp").is_err());
        }
    }
}

#[test]
fn engine_legacy_protected_source_cannot_bypass_native_guards_by_stripping_diagnostics_or_origin() {
    let source = protection_fixture(false, false);
    let original = import(&source).unwrap();
    for mutation in ["warning", "origin", "all_tags", "source", "source_and_origin", "source_type_and_origin"] {
        let mut legacy = original.clone();
        match mutation {
            "warning" => legacy.tags.retain(|(key, value)| !(key == "odp:warning" && value == PROTECTION_WARNING)),
            "origin" => legacy.tags.retain(|(key, _)| key != "odp:imported"),
            "all_tags" => legacy.tags.clear(),
            "source" => legacy.media.retain(|media| media.content_type != "application/vnd.deckcraft.odp-source"),
            "source_and_origin" => {
                legacy.media.retain(|media| media.content_type != "application/vnd.deckcraft.odp-source");
                legacy.tags.retain(|(key, _)| key != "odp:imported");
            }
            "source_type_and_origin" => {
                for media in &mut legacy.media {
                    if media.content_type == "application/vnd.deckcraft.odp-source" {
                        media.content_type = "application/octet-stream".into();
                    }
                }
                legacy.tags.retain(|(key, _)| key != "odp:imported");
            }
            _ => panic!("unknown original test mutation"),
        }
        let frozen = legacy.clone();
        for format in ["odp", "pptx", "potx", "ppsx"] {
            assert!(file::save_bytes(&legacy, format).is_err(), "remaining source/origin/diagnostic witness must fail closed: {mutation}, {format}");
        }
        assert_eq!(legacy, frozen);
    }
}
