//! Contributor-original ODF notes-preview protection safety regressions.
//! BIZ UDPGothic is an existing open font-family name; no font assets are added.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{PhType, Presentation, Shape};
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MANIFEST: &str = "META-INF/manifest.xml";
const WARNING: &str = "ODF frame protection is not fully represented natively; original source is retained and interchange export is blocked.";
const GAMMA: &str =
    "ODF picture gamma has no native rendering mapping; source instructions are retained; edited export requires verified source preservation.";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'";
const FONT: &str = "<office:font-face-decls><style:font-face style:name='OpenFace' svg:font-family='BIZ UDPGothic'/></office:font-face-decls>";
type Parts = BTreeMap<String, Vec<u8>>;

fn thumbnail(extra: &str) -> String {
    format!("<draw:page-thumbnail presentation:class='page' svg:x='40pt' svg:y='40pt' svg:width='320pt' svg:height='240pt'{extra}/>")
}
fn notes_body(text: &str) -> String {
    format!(
        "<draw:frame presentation:class='notes' presentation:user-transformed='true' svg:x='40pt' svg:y='320pt' svg:width='320pt' svg:height='220pt'><draw:text-box>{text}</draw:text-box></draw:frame>"
    )
}
fn fixture(version: &str, attributes: &str, passive: bool, cache: bool) -> Parts {
    let keep = if passive { "<style:paragraph-properties fo:keep-with-next='always'/>" } else { "" };
    let content = format!(
        "<office:document-content {NS} office:version='{version}'>{FONT}<office:automatic-styles><style:style style:name='ProtectedPreview' style:family='graphic'><style:graphic-properties {attributes}/></style:style><style:style style:name='OriginalText' style:family='paragraph'>{keep}<style:text-properties style:font-name='OpenFace' fo:font-size='18pt'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Contributor original protection guard' draw:master-page-name='OriginalMaster' xml:id='original-page'><draw:frame xml:id='original-text' svg:x='20pt' svg:y='20pt' svg:width='300pt' svg:height='80pt'><draw:text-box><text:p text:style-name='OriginalText'>Original local text</text:p></draw:text-box></draw:frame><presentation:notes>{}{}</presentation:notes></draw:page></office:presentation></office:body></office:document-content>",
        notes_body("<text:p>Contributor original speaker note</text:p>"),
        thumbnail(" draw:page-number='1' draw:style-name='ProtectedPreview'")
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'>{FONT}<office:styles/><office:automatic-styles><style:page-layout style:name='OriginalSlide'><style:page-layout-properties fo:page-width='400pt' fo:page-height='300pt'/></style:page-layout><style:page-layout style:name='OriginalNotes'><style:page-layout-properties fo:page-width='400pt' fo:page-height='600pt' fo:margin='0pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='OriginalMaster' style:page-layout-name='OriginalSlide'><presentation:notes style:page-layout-name='OriginalNotes'>{}{}</presentation:notes></style:master-page></office:master-styles></office:document-styles>",
        thumbnail(""),
        notes_body("")
    );
    let counts = if cache { "<meta:document-statistic meta:word-count='3' meta:page-count='1' meta:paragraph-count='2'/>" } else { "" };
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        (
            "meta.xml".into(),
            format!("<office:document-meta {NS} office:version='{version}'><office:meta>{counts}</office:meta></office:document-meta>").into_bytes(),
        ),
        ("settings.xml".into(), format!("<office:document-settings {NS} office:version='{version}'/>").into_bytes()),
    ]);
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/>"
    );
    for name in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='text/xml'/>"));
    }
    manifest.push_str("</manifest:manifest>");
    files.insert(MANIFEST.into(), manifest.into_bytes());
    files
}
fn zip(files: &Parts) -> Vec<u8> {
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
fn parts(bytes: &[u8]) -> Parts {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut result: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert!(result.insert(entry.name().into(), bytes).is_none());
    }
    result
}
fn replace(files: &mut Parts, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(files[part].clone()).unwrap();
    assert!(text.contains(old), "original fixture mutation target exists");
    files.insert(part.into(), text.replace(old, new).into_bytes());
}
fn preview(p: &Presentation) -> &Shape {
    let page = p.slides[0].notes_page.as_ref().expect("actual local notes page");
    let previews: Vec<_> = page.shapes.iter().filter(|shape| shape.ph_type() == Some(PhType::SlideImage)).collect();
    assert_eq!(previews.len(), 1);
    previews[0]
}
fn warning(p: &Presentation) -> usize {
    p.tags.iter().filter(|(key, value)| key == "odp:warning" && value == WARNING).count()
}
fn source(p: &Presentation) -> &[u8] {
    let items: Vec<_> = p.media.iter().filter(|item| item.content_type == SOURCE).collect();
    assert_eq!(items.len(), 1);
    items[0].data.as_slice()
}
fn edit(p: &mut Presentation) {
    let shape = Arc::make_mut(&mut p.slides[0]).shapes.iter_mut().find(|shape| shape.text.is_some()).unwrap();
    shape.text.as_mut().unwrap().paragraphs[0].runs[0].text = "Edited original local text".into();
}
fn refused(p: &Presentation) {
    let before = p.clone();
    let error = export(p).expect_err("source protection must never silently disappear").to_string();
    assert!(!error.is_empty());
    assert_eq!(p, &before, "diagnostic/export refusal cannot mutate source or model");
}
fn emit(version: &str, label: &str, files: &Parts) {
    if let Ok(directory) = std::env::var("DECKCRAFT_PROTECTION_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for name in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
            std::fs::write(std::path::Path::new(&directory).join(format!("{version}-{label}-{}", name.replace('/', "-"))), &files[name]).unwrap();
        }
    }
}

#[test]
fn actual_notes_preview_geometric_protection_warns_and_refuses_native_and_edited_export() {
    for version in ["1.2", "1.3"] {
        for token in ["position", "size", "position size", "size position"] {
            let files = fixture(version, &format!("style:protect='{token}'"), false, false);
            let original = zip(&files);
            let mut p = import(&original).unwrap();
            assert_eq!(warning(&p), 1);
            assert_eq!(
                p.tags.iter().filter(|(key, _)| key == "odp:warning").count(),
                1,
                "protection diagnostic is independent of paragraph audit v1"
            );
            assert!(!preview(&p).locked, "diagnostic-only change does not assign an imprecise native lock");
            assert_eq!(source(&p), original);
            refused(&p);
            edit(&mut p);
            refused(&p);
            assert_eq!(source(&p), original);
            assert_eq!(zip(&files), original, "original fixture input is immutable");
        }
    }
}

#[test]
fn absent_and_exact_none_allow_native_export_and_emit_twenty_schema_source_parts() {
    for version in ["1.2", "1.3"] {
        for attributes in ["", "style:protect='none'", "style:protect='n&#111;ne'"] {
            let files = fixture(version, attributes, false, false);
            let original = zip(&files);
            let p = import(&original).unwrap();
            assert_eq!(warning(&p), 0);
            assert!(!preview(&p).locked);
            assert_eq!(source(&p), original);
            let before = p.clone();
            let output = export(&p).expect("known unprotected state adds no new warning/guard");
            assert_eq!(p, before);
            let reopened = import(&output).unwrap();
            assert_eq!(warning(&reopened), 0);
            assert!(!preview(&reopened).locked);
            assert!(!parts(&output).is_empty());
        }
        emit(version, "protected-source", &fixture(version, "style:protect='position'", false, false));
        emit(version, "none-source", &fixture(version, "style:protect='none'", false, false));
    }
}

#[test]
fn content_protection_invalid_scalars_and_noncanonical_none_are_never_exempted() {
    for token in [
        "content",
        "content position",
        "position content size",
        "",
        "None",
        "NONE",
        "none ",
        " none",
        "none none",
        "none content",
        "true",
        "false",
        "0",
        "1",
        "none&#9;",
        "file:///protect",
    ] {
        let files = fixture("1.3", &format!("style:protect='{token}'"), false, false);
        let mut p = import(&zip(&files)).unwrap();
        assert_eq!(warning(&p), 1, "every standard-expanded nonexempt value is diagnosed: {token}");
        edit(&mut p);
        refused(&p);
    }
}

#[test]
fn old_passive_no_cache_retention_cannot_be_used_to_preserve_protection_under_the_new_guard() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            let files = fixture(version, "style:protect='position'", true, cache);
            let original = zip(&files);
            let mut p = import(&original).unwrap();
            assert_eq!(warning(&p), 1);
            assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.starts_with("ODF semantic audit v1 found")));
            edit(&mut p);
            refused(&p);
            assert_eq!(source(&p), original);
        }
    }
}

#[test]
fn protection_warning_and_source_survive_native_save_reopen_before_and_after_real_text_edit() {
    for version in ["1.2", "1.3"] {
        let original = zip(&fixture(version, "style:protect='position'", false, false));
        let mut p = import(&original).unwrap();
        for edited in [false, true] {
            if edited {
                edit(&mut p);
            }
            let before = p.clone();
            let bytes = deckcraft_format::save(&p).unwrap();
            assert_eq!(p, before);
            let reopened = deckcraft_format::load(&bytes).unwrap();
            assert_eq!(reopened, p);
            assert_eq!(warning(&reopened), 1);
            assert_eq!(source(&reopened), original);
            assert!(!preview(&reopened).locked);
            refused(&reopened);
        }
    }
}

#[test]
fn unused_inherited_and_late_occurrences_still_publish_one_reserved_warning() {
    let mut files = fixture("1.3", "style:protect='none'", false, false);
    let mut definitions = String::new();
    for i in 0..129 {
        definitions.push_str(&format!(
            "<style:style style:name='UnusedNone{i}' style:family='graphic'><style:graphic-properties style:protect='none'/></style:style>"
        ));
    }
    definitions.push_str("<style:style style:name='UnusedProtected' style:family='graphic'><style:graphic-properties style:protect='position'/></style:style><style:style style:name='UnusedInherited' style:family='graphic' style:parent-style-name='UnusedProtected'/>");
    replace(&mut files, "styles.xml", "<office:styles/>", &format!("<office:styles>{definitions}</office:styles>"));
    let p = import(&zip(&files)).unwrap();
    assert_eq!(warning(&p), 1);
    refused(&p);
}

#[test]
fn standard_protect_none_on_wrong_part_owner_family_or_nonleaf_bag_is_diagnosed() {
    for (part, old, new) in [
        ("content.xml", "xml:id='original-text'", "xml:id='original-text' style:protect='none'"),
        ("content.xml", "<style:graphic-properties />", "<style:paragraph-properties style:protect='none'/>"),
        (
            "content.xml",
            "<style:graphic-properties />",
            "<style:graphic-properties style:protect='none'><text:p>Original child</text:p></style:graphic-properties>",
        ),
        ("meta.xml", "<office:meta>", "<office:meta style:protect='none'>"),
        ("settings.xml", "office:version='1.3'/>", "office:version='1.3' style:protect='none'/>"),
    ] {
        let mut files = fixture("1.3", "", false, false);
        replace(&mut files, part, old, new);
        let p = import(&zip(&files)).unwrap();
        assert_eq!(warning(&p), 1);
        refused(&p);
    }
    let mut files = fixture("1.3", "", false, false);
    replace(
        &mut files,
        MANIFEST,
        "<manifest:manifest ",
        "<manifest:manifest xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' s:protect='none' ",
    );
    let p = import(&zip(&files)).unwrap();
    assert_eq!(warning(&p), 1);
    refused(&p);
    for family in ["paragraph", "text", "drawing-page"] {
        let mut files = fixture("1.3", "", false, false);
        replace(
            &mut files,
            "styles.xml",
            "<office:styles/>",
            &format!(
                "<office:styles><style:style style:name='UnusedWrongFamily' style:family='{family}'><style:graphic-properties style:protect='none'/></style:style></office:styles>"
            ),
        );
        let p = import(&zip(&files)).unwrap();
        assert_eq!(warning(&p), 1);
        refused(&p);
    }
}

#[test]
fn namespace_aliases_and_entity_nonempty_tokens_are_detected_without_prefix_matching() {
    for version in ["1.2", "1.3"] {
        let files = fixture(version, "xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' s:protect='pos&#105;tion'", false, false);
        let p = import(&zip(&files)).unwrap();
        assert_eq!(warning(&p), 1);
        refused(&p);
    }
}

#[test]
fn foreign_unqualified_rebound_and_duplicate_names_keep_their_independent_guards() {
    for attributes in [
        "protect='position'",
        "xmlns:f='urn:deckcraft:test:foreign' f:protect='position'",
        "xmlns:s='urn:deckcraft:test:foreign' s:protect='position'",
        "xmlns:style='urn:deckcraft:test:foreign' style:protect='position'",
    ] {
        let files = fixture("1.3", attributes, false, false);
        let p = import(&zip(&files)).unwrap();
        assert_eq!(warning(&p), 0, "foreign/unqualified same-local names are not standard protection");
        assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
        refused(&p);
    }
    let files = fixture("1.3", "xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' style:protect='none' s:protect='none'", false, false);
    assert!(import(&zip(&files)).unwrap_err().to_string().contains("duplicate expanded XML attribute"));
}

#[test]
fn mixed_gamma_and_old_passive_audit_do_not_allowlist_the_new_protection_warning() {
    for cache in [false, true] {
        let files = fixture("1.3", "style:protect='position' draw:gamma='125.5%'", true, cache);
        let mut p = import(&zip(&files)).unwrap();
        assert_eq!(warning(&p), 1);
        assert_eq!(p.tags.iter().filter(|(key, value)| key == "odp:warning" && value == GAMMA).count(), 1);
        assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.starts_with("ODF semantic audit v1 found")));
        edit(&mut p);
        refused(&p);
    }
}

#[test]
fn old_warning_stripped_native_with_retained_source_cannot_silently_export() {
    for version in ["1.2", "1.3"] {
        let original = zip(&fixture(version, "style:protect='position'", false, false));
        let mut p = import(&original).unwrap();
        p.tags.retain(|(key, value)| !(key == "odp:warning" && value == WARNING));
        assert_eq!(warning(&p), 0);
        edit(&mut p);
        for i in 0..1100 {
            p.tags.push(("contributor-original-filler".into(), i.to_string()));
        }
        let bytes = deckcraft_format::save(&p).unwrap();
        let reopened = deckcraft_format::load(&bytes).unwrap();
        assert_eq!(warning(&reopened), 0, "the legacy model truly lacks the new guard tag");
        assert_eq!(source(&reopened), original);
        refused(&reopened);
    }
}

#[test]
fn missing_source_or_import_marker_does_not_turn_a_protection_diagnostic_into_permission() {
    for loss in ["source", "import-marker", "warning-and-source", "warning-and-import-marker"] {
        let mut p = import(&zip(&fixture("1.3", "style:protect='position'", false, false))).unwrap();
        edit(&mut p);
        if loss.contains("source") {
            p.media.retain(|item| item.content_type != SOURCE);
        }
        if loss.contains("import-marker") {
            p.tags.retain(|(key, _)| key != "odp:imported");
        }
        if loss.starts_with("warning-and-") {
            p.tags.retain(|(key, value)| !(key == "odp:warning" && value == WARNING));
        }
        refused(&p);
    }
}
