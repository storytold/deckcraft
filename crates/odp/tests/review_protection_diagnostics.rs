//! Independent W16 safety regressions; all XML and raster bytes are contributor-original.
//! Frame protection is source-only, not a native locking or preservation capability.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{PhType, Presentation};
use deckcraft_odp::{MIME, export, import, validate_source_diagnostics};
use std::{collections::BTreeMap, io::Cursor, io::Write, sync::Arc};

type Parts = BTreeMap<String, Vec<u8>>;
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MANIFEST: &str = "META-INF/manifest.xml";
const PROTECTION: &str = "ODF frame protection is not fully represented natively; original source is retained and interchange export is blocked.";
const GAMMA: &str =
    "ODF picture gamma has no native rendering mapping; source instructions are retained; edited export requires verified source preservation.";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'";

fn fixture(version: &str, properties: &str, notes: bool, passive: bool, cache: bool) -> Parts {
    let audit = if passive { "<style:paragraph-properties fo:keep-with-next='always'/>" } else { "" };
    let frame_style = if notes { "" } else { " draw:style-name='Protected'" };
    let notes = if notes {
        "<presentation:notes><draw:page-thumbnail draw:style-name='Protected' presentation:class='page' draw:page-number='1' svg:x='20pt' svg:y='20pt' svg:width='320pt' svg:height='240pt'/></presentation:notes>"
    } else {
        ""
    };
    let content = format!(
        "<office:document-content {NS} office:version='{version}'><office:automatic-styles><style:style style:name='Protected' style:family='graphic'><style:graphic-properties {properties}/></style:style><style:style style:name='Paragraph' style:family='paragraph'>{audit}<style:text-properties fo:font-size='16pt'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Original safety fixture' draw:master-page-name='Master'><draw:frame{frame_style} svg:x='30pt' svg:y='30pt' svg:width='240pt' svg:height='90pt'><draw:text-box><text:p text:style-name='Paragraph'>Original public text</text:p></draw:text-box></draw:frame>{notes}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:styles/><office:automatic-styles><style:page-layout style:name='Page'><style:page-layout-properties fo:page-width='400pt' fo:page-height='300pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='Master' style:page-layout-name='Page'/></office:master-styles></office:document-styles>"
    );
    let mut parts = Parts::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), format!("<office:document-meta {NS} office:version='{version}'><office:meta/></office:document-meta>").into_bytes()),
        ("settings.xml".into(), format!("<office:document-settings {NS} office:version='{version}'/>").into_bytes()),
    ]);
    if cache {
        parts.insert("Thumbnails/thumbnail.png".into(), deckcraft_render::Image { width: 1, height: 1, pixels: vec![39, 107, 173, 255] }.to_png());
    }
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/>"
    );
    for name in parts.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name.ends_with(".png") { "image/png" } else { "text/xml" };
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>"));
    }
    manifest.push_str("</manifest:manifest>");
    parts.insert(MANIFEST.into(), manifest.into_bytes());
    parts
}

fn replace(parts: &mut Parts, name: &str, old: &str, new: &str) {
    let text = String::from_utf8(parts[name].clone()).unwrap();
    assert!(text.contains(old), "independent mutation requires original spelling: {old}");
    parts.insert(name.into(), text.replace(old, new).into_bytes());
}

fn package(parts: &Parts) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in std::iter::once("mimetype").chain(parts.keys().map(String::as_str).filter(|name| *name != "mimetype")) {
        zip.start_file(name, options).unwrap();
        zip.write_all(&parts[name]).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn warning(p: &Presentation, value: &str) -> usize {
    p.tags.iter().filter(|(key, message)| key == "odp:warning" && message == value).count()
}

fn source_bytes(p: &Presentation) -> &[u8] {
    p.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice()
}

fn edit(p: &mut Presentation) {
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Revised public text".into();
}

fn assert_protected(parts: &Parts) -> Presentation {
    let bytes = package(parts);
    let p = import(&bytes).expect("independent synthetic protection fixture imports");
    assert_eq!(warning(&p, PROTECTION), 1, "one exact nonprivate protection diagnostic is mandatory");
    assert_eq!(source_bytes(&p), bytes);
    assert!(p.tags.iter().any(|(key, value)| key == "odp:imported" && value == "1"));
    validate_source_diagnostics(&p).expect("current complete diagnostic/provenance is valid");
    p
}

fn assert_refused(p: &Presentation) {
    let frozen = p.clone();
    let error = export(p).expect_err("source-only protection must block interchange export").to_string();
    assert!(error.contains("ODP"), "must return a handled format error");
    assert_eq!(p, &frozen, "refused export cannot mutate the editable document or original bytes");
}

#[test]
fn independent_real_notes_preview_protection_is_retained_and_edited_export_refuses() {
    for version in ["1.2", "1.3"] {
        for value in ["position", "size", "position size", "size position", "content", "content position size"] {
            let parts = fixture(version, &format!("style:protect='{value}'"), true, false, false);
            let mut p = assert_protected(&parts);
            assert_eq!(p.tags.iter().filter(|(key, _)| key == "odp:warning").count(), 1, "this reproduction has protection as its only blocker");
            let preview = &p.slides[0].notes_page.as_ref().unwrap().shapes[0];
            assert_eq!(preview.ph_type(), Some(PhType::SlideImage), "actual notes preview, not an unrelated frame");
            assert!(!preview.locked, "do not invent a mapping to the one native lock bit");
            let original_preview = preview.clone();
            assert_refused(&p);
            edit(&mut p);
            let reopened = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
            assert_eq!(reopened, p, "native persistence keeps editable state and all diagnostics");
            assert_eq!(source_bytes(&reopened), package(&parts), "native save/reopen supports exact source recovery");
            assert_eq!(reopened.slides[0].notes_page.as_ref().unwrap().shapes[0], original_preview);
            assert_refused(&reopened);
        }
    }
}

#[test]
fn independent_plain_frame_passive_old_no_cache_lane_now_refuses_protection() {
    for cache in [false, true] {
        for passive in [false, true] {
            let mut p = assert_protected(&fixture("1.3", "style:protect='size'", false, passive, cache));
            assert!(!p.slides[0].shapes[0].locked);
            assert_eq!(p.tags.iter().any(|(key, _)| key == "odp:semantic-audit"), passive);
            edit(&mut p);
            assert_refused(&p);
        }
    }
}

#[test]
fn independent_absent_and_exact_none_have_no_protection_diagnostic_or_lock_mapping() {
    for version in ["1.2", "1.3"] {
        for properties in ["", "style:protect='none'", "xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' s:protect='n&#111;ne'"] {
            for notes in [false, true] {
                let bytes = package(&fixture(version, properties, notes, false, false));
                let mut p = import(&bytes).unwrap();
                assert_eq!(warning(&p, PROTECTION), 0);
                assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "exact identity in a supported owner adds no warning");
                validate_source_diagnostics(&p).unwrap();
                assert!(!p.slides[0].shapes[0].locked);
                edit(&mut p);
                let output = export(&p).expect("ordinary native export remains available without protection");
                assert_eq!(import(&output).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "Revised public text");
                assert_eq!(source_bytes(&p), bytes);
            }
        }
    }
}

#[test]
fn independent_complete_scope_walk_covers_unused_named_defaults_and_both_graphic_families() {
    for family in ["graphic", "presentation"] {
        for scope in ["content-auto", "styles-auto", "common", "default"] {
            for value in ["none", "position", "content"] {
                let mut parts = fixture("1.3", "", false, false, false);
                let bag = format!(
                    "<s:graphic-properties xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' s:protect='{value}'> <!--original gap--> \r\n </s:graphic-properties>"
                );
                let named = format!("<style:style style:name='Unused' style:family='{family}'>{bag}</style:style>");
                match scope {
                    "content-auto" => replace(&mut parts, "content.xml", "</office:automatic-styles>", &format!("{named}</office:automatic-styles>")),
                    "styles-auto" => replace(&mut parts, "styles.xml", "</office:automatic-styles>", &format!("{named}</office:automatic-styles>")),
                    "common" => replace(&mut parts, "styles.xml", "<office:styles/>", &format!("<office:styles>{named}</office:styles>")),
                    _ => replace(
                        &mut parts,
                        "styles.xml",
                        "<office:styles/>",
                        &format!("<office:styles><style:default-style style:family='{family}'>{bag}</style:default-style></office:styles>"),
                    ),
                }
                let p = import(&package(&parts)).unwrap();
                assert_eq!(warning(&p, PROTECTION), usize::from(value != "none"), "scope={scope}, family={family}, value={value}");
                if value != "none" {
                    assert_refused(&p);
                }
            }
        }
    }
}

#[test]
fn independent_invalid_values_are_never_trimmed_case_folded_or_coerced_to_none() {
    for value in [
        "",
        "None",
        "NONE",
        " none",
        "none ",
        "none&#9;",
        "none position",
        "position position",
        "size size",
        "true",
        "0",
        "unknown",
        "file:///original",
        "content size",
        "position&#10;size",
    ] {
        let mut p = assert_protected(&fixture("1.3", &format!("style:protect='{value}'"), false, true, false));
        edit(&mut p);
        assert_refused(&p);
    }
}

#[test]
fn independent_wrong_owners_and_nonleaf_none_are_still_protection_evidence() {
    for body in [
        "<office:automatic-styles style:protect='none'/>",
        "<office:automatic-styles><style:style style:name='Unused' style:family='paragraph'><style:graphic-properties style:protect='none'/></style:style></office:automatic-styles>",
        "<office:automatic-styles><style:style style:name='Unused' style:family='graphic'><style:paragraph-properties style:protect='none'/></style:style></office:automatic-styles>",
        "<office:automatic-styles><style:default-style style:family='graphic'><style:graphic-properties style:protect='none'/></style:default-style></office:automatic-styles>",
        "<office:automatic-styles><style:style style:name='Unused' style:family='graphic'><style:graphic-properties style:protect='none'>payload</style:graphic-properties></style:style></office:automatic-styles>",
        "<office:automatic-styles><style:style style:name='Unused' style:family='graphic'><style:graphic-properties style:protect='none'><text:p/></style:graphic-properties></style:style></office:automatic-styles>",
    ] {
        let mut parts = fixture("1.3", "", false, false, false);
        // Put the candidate in styles.xml's root: every candidate has a complete walk,
        // including unused owners, without relying on visible-shape style resolution.
        replace(&mut parts, "styles.xml", "<office:styles/>", body);
        let p = assert_protected(&parts);
        assert_refused(&p);
    }
    let mut parts = fixture("1.3", "", false, false, false);
    replace(&mut parts, "content.xml", "<draw:frame ", "<draw:frame style:protect='none' ");
    assert_refused(&assert_protected(&parts));
}

#[test]
fn independent_standard_protection_scan_reaches_fixed_parts_and_parsed_embedded_documents() {
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml", MANIFEST, "Object/content.xml"] {
        let mut parts = fixture("1.3", "", false, false, false);
        match part {
            "content.xml" => replace(&mut parts, part, "<office:body>", "<office:body style:protect='none'>"),
            "styles.xml" => replace(&mut parts, part, "<office:styles/>", "<office:styles style:protect='none'/>"),
            "meta.xml" => replace(&mut parts, part, "<office:meta/>", "<office:meta style:protect='none'/>"),
            "settings.xml" => replace(&mut parts, part, "office:version='1.3'/>", "office:version='1.3' style:protect='none'/>"),
            MANIFEST => replace(
                &mut parts,
                part,
                "<manifest:manifest ",
                "<manifest:manifest xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' s:protect='none' ",
            ),
            _ => {
                parts.insert(
                    part.into(),
                    format!("<office:document-content {NS} office:version='1.3' style:protect='none'><office:body/></office:document-content>")
                        .into_bytes(),
                );
                replace(
                    &mut parts,
                    MANIFEST,
                    "</manifest:manifest>",
                    "<manifest:file-entry manifest:full-path='Object/content.xml' manifest:media-type='text/xml'/></manifest:manifest>",
                );
                replace(
                    &mut parts,
                    "content.xml",
                    "</draw:page>",
                    "<draw:frame svg:width='20pt' svg:height='20pt'><draw:object xlink:href='Object'/></draw:frame></draw:page>",
                );
            }
        }
        let p = assert_protected(&parts);
        let mut legacy = p.clone();
        legacy.tags.retain(|(key, _)| key != "odp:warning");
        assert!(validate_source_diagnostics(&legacy).is_err(), "fresh source reconstructs required warning in {part}");
        assert_refused(&legacy);
    }
}

#[test]
fn independent_foreign_lookalikes_are_not_standard_protection_and_keep_other_guards() {
    for properties in
        ["xmlns:s='urn:deckcraft:original:foreign' s:protect='position'", "xmlns:style='urn:deckcraft:original:foreign' style:protect='position'"]
    {
        let bytes = package(&fixture("1.3", properties, false, false, false));
        let p = import(&bytes).unwrap();
        assert_eq!(warning(&p, PROTECTION), 0, "lexical local name cannot borrow standard semantics");
        assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "independent foreign-semantic guard remains blocking");
        assert_eq!(source_bytes(&p), bytes);
        assert_refused(&p);
    }
    let duplicate =
        fixture("1.3", "xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' style:protect='none' s:protect='position'", false, false, false);
    assert!(import(&package(&duplicate)).unwrap_err().to_string().contains("duplicate expanded XML attribute"));
}

#[test]
fn independent_late_unused_protection_survives_audit_detail_caps_without_hiding_other_warnings() {
    let mut parts = fixture("1.3", "draw:gamma='125%'", false, true, false);
    let mut owners = String::new();
    for index in 0..180 {
        owners.push_str(&format!(
            "<style:style style:name='Public{index}' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style>"
        ));
    }
    owners.push_str("<style:style style:name='Last' style:family='graphic'><style:graphic-properties style:protect='content'/></style:style>");
    replace(&mut parts, "content.xml", "</office:automatic-styles>", &format!("{owners}</office:automatic-styles>"));
    let mut p = assert_protected(&parts);
    assert_eq!(warning(&p, GAMMA), 1, "protection does not suppress independent gamma diagnostics");
    assert!(
        p.tags.iter().any(|(key, value)| key == "odp:semantic-audit" && value.contains("omitted=") && !value.ends_with("omitted=0")),
        "fixture really exceeds audit detail cap"
    );
    assert!(
        p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("semantic audit v1")),
        "prior independent audit warning stays present"
    );
    edit(&mut p);
    assert_refused(&p);
    p.tags.retain(|(key, value)| !(key == "odp:warning" && value == PROTECTION));
    for index in 0..1100 {
        p.tags.push(("contributor-original-filler".into(), index.to_string()));
    }
    assert!(validate_source_diagnostics(&p).is_err(), "current tag count never truncates fresh required-warning proof");
    assert_refused(&p);
}

#[test]
fn independent_fresh_preflight_requires_both_effect_warnings_and_exact_import_origin() {
    let original = assert_protected(&fixture("1.3", "style:protect='position' draw:gamma='125%'", false, true, false));
    assert_eq!(warning(&original, GAMMA), 1);
    for mutation in ["protection", "gamma", "all-warnings", "imported", "imported-value"] {
        let mut p = original.clone();
        edit(&mut p);
        match mutation {
            "protection" => p.tags.retain(|(key, value)| !(key == "odp:warning" && value == PROTECTION)),
            "gamma" => p.tags.retain(|(key, value)| !(key == "odp:warning" && value == GAMMA)),
            "all-warnings" => p.tags.retain(|(key, _)| key != "odp:warning"),
            "imported" => p.tags.retain(|(key, _)| key != "odp:imported"),
            _ => p.tags.iter_mut().filter(|(key, _)| key == "odp:imported").for_each(|(_, value)| *value = "0".into()),
        }
        let frozen = p.clone();
        assert!(validate_source_diagnostics(&p).is_err(), "each freshly required warning must survive: {mutation}");
        assert_refused(&p);
        assert_eq!(p, frozen);
        let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        assert!(validate_source_diagnostics(&native).is_err(), "native persistence cannot repair missing proof: {mutation}");
        assert_refused(&native);
    }
}

#[test]
fn independent_protection_source_authority_adversaries_refuse_without_mutation() {
    let original = assert_protected(&fixture("1.3", "style:protect='size'", true, false, false));
    for mutation in ["empty", "linked", "duplicate", "corrupt", "retyped", "removed", "removed-origin"] {
        let mut p = original.clone();
        edit(&mut p);
        match mutation {
            "removed" | "removed-origin" => {
                p.media.retain(|media| media.content_type != SOURCE);
                if mutation == "removed-origin" {
                    p.tags.retain(|(key, _)| key != "odp:imported");
                }
            }
            "duplicate" => {
                let mut media = p.media.iter().find(|media| media.content_type == SOURCE).unwrap().clone();
                media.id = deckcraft_model::MediaId(100_000);
                p.media.push(media);
            }
            _ => {
                let media = p.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap();
                match mutation {
                    "empty" => media.data = Arc::new(Vec::new()),
                    "linked" => media.link = Some("https://example.invalid/original.odp".into()),
                    "retyped" => media.content_type = "application/octet-stream".into(),
                    _ => media.data = Arc::new(b"contributor-original invalid archive".to_vec()),
                }
            }
        }
        let frozen = p.clone();
        assert!(validate_source_diagnostics(&p).is_err(), "source authority must reject {mutation}");
        assert_refused(&p);
        assert_eq!(p, frozen);
    }
}

#[test]
fn independent_gamma_only_gate_and_none_profile_remain_closed() {
    for cache in [false, true] {
        let bytes = package(&fixture("1.3", "draw:gamma='125%'", false, false, cache));
        let mut p = import(&bytes).unwrap();
        assert_eq!(warning(&p, PROTECTION), 0);
        assert_eq!(warning(&p, GAMMA), 1);
        edit(&mut p);
        assert!(
            export(&p).unwrap_err().to_string().contains("source audit contains non-preservable semantics"),
            "gamma alone never becomes a preservation capability"
        );
    }
    // W16 does not admit even identity-none into the cache profile. It also must
    // not broaden the aggregate gamma condition to allow a protection warning.
    let mut p = import(&package(&fixture("1.3", "style:protect='none'", false, true, true))).unwrap();
    assert_eq!(warning(&p, PROTECTION), 0);
    edit(&mut p);
    assert!(export(&p).unwrap_err().to_string().contains("cache"), "existing cache grammar remains independently closed");
}
