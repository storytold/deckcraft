//! Independent contributor-original hostile package and edit-ownership regressions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::{Presentation, Shape, TextBody};
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'";
const STYLE: &str = "<style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style>";

fn content(text: &str, extra: &str) -> String {
    format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles>{STYLE}</office:automatic-styles><office:body><office:presentation><draw:page draw:name='Local' draw:master-page-name='M'><draw:frame draw:id='one' svg:width='144pt' svg:height='72pt'><draw:text-box><text:p text:style-name='P'>{text}</text:p><text:p text:style-name='P'>Second</text:p></draw:text-box></draw:frame>{extra}</draw:page></office:presentation></office:body></office:document-content>"
    )
}
fn fixture(text: &str, extra: &str, mut other: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let xml = content(text, extra);
    if !other.iter().any(|(name, _)| name == "styles.xml") {
        other.push((
            "styles.xml".into(),
            format!(
                "<office:document-styles {NS} office:version='1.3'><office:styles/><office:automatic-styles><style:page-layout style:name='Page'><style:page-layout-properties fo:page-width='720pt' fo:page-height='405pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='Page'/></office:master-styles></office:document-styles>"
            )
            .into_bytes(),
        ));
    }
    let mut owners = String::from(
        "<manifest:file-entry manifest:full-path='/' manifest:media-type='application/vnd.oasis.opendocument.presentation'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/>",
    );
    for (name, _) in &other {
        owners.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='text/xml'/>"));
    }
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'>{owners}</manifest:manifest>"
    );
    let mut parts = vec![
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), xml.into_bytes()),
        ("META-INF/manifest.xml".into(), manifest.into_bytes()),
    ];
    parts.append(&mut other);
    raw_zip(&parts)
}
fn crc(bytes: &[u8]) -> u32 {
    let mut state = !0u32;
    for byte in bytes {
        state ^= u32::from(*byte);
        for _ in 0..8 {
            state = (state >> 1) ^ (0xedb8_8320u32 & 0u32.wrapping_sub(state & 1));
        }
    }
    !state
}
fn raw_zip(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, bytes) in parts {
        let offset = out.len() as u32;
        let size = bytes.len() as u32;
        let checksum = crc(bytes);
        out.extend_from_slice(b"PK\x03\x04");
        for value in [20u16, 0x800, 0, 0, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in [checksum, size, size] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(bytes);
        central.extend_from_slice(b"PK\x01\x02");
        for value in [20u16, 20, 0x800, 0, 0, 0] {
            central.extend_from_slice(&value.to_le_bytes());
        }
        for value in [checksum, size, size] {
            central.extend_from_slice(&value.to_le_bytes());
        }
        for value in [name.len() as u16, 0, 0, 0, 0] {
            central.extend_from_slice(&value.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let offset = out.len() as u32;
    let size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    for value in [0u16, 0, parts.len() as u16, parts.len() as u16] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
fn members(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        out.insert(entry.name().to_string(), bytes);
    }
    out
}
fn edit(p: &mut Presentation, text: &str) {
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = text.into();
}
fn refused(bytes: &[u8]) {
    if let Ok(mut p) = import(bytes) {
        edit(&mut p, "Changed");
        assert!(export(&p).is_err(), "hostile package was exported");
    }
}

#[test]
fn review_preserves_payloads_and_xml_gaps_for_two_real_unicode_edits() {
    let source = fixture("Original", "", vec![]);
    let mut p = import(&source).unwrap();
    edit(&mut p, "  Café & <Ω>\t🙂  ");
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[1].runs[0].text = "Other edit".into();
    let output = export(&p).unwrap();
    assert_ne!(output, source);
    let before = members(&source);
    let after = members(&output);
    assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
    for (name, bytes) in &before {
        if name != "content.xml" {
            assert_eq!(after.get(name), Some(bytes));
        }
    }
    let xml = std::str::from_utf8(&after["content.xml"]).unwrap();
    assert!(xml.contains("draw:id='one'") && xml.contains(STYLE));
    let original = std::str::from_utf8(&before["content.xml"]).unwrap();
    let (prefix, tail) = original.split_once("Original").unwrap();
    let (gap, suffix) = tail.split_once("Second").unwrap();
    assert!(xml.starts_with(prefix) && xml.ends_with(suffix));
    let first_close = xml.find("</text:p>").unwrap();
    let second_edit = xml.find(">Other").unwrap() + 1;
    assert_eq!(xml.get(first_close..second_edit), Some(gap));
    let reopened = import(&output).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "  Café & <Ω>\t🙂  \nOther edit");
}

#[test]
fn review_no_edit_and_all_nontext_model_changes_refuse() {
    let p = import(&fixture("Original", "", vec![])).unwrap();
    assert!(export(&p).is_err());
    let mut cases = Vec::new();
    let mut v = p.clone();
    edit(&mut v, "New");
    v.props.title = "Changed title".into();
    cases.push(v);
    let mut v = p.clone();
    edit(&mut v, "New");
    Arc::make_mut(&mut v.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].props.margin_left = Some(20.0);
    cases.push(v);
    let mut v = p.clone();
    edit(&mut v, "New");
    Arc::make_mut(&mut v.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].props.bold = Some(true);
    cases.push(v);
    let mut v = p.clone();
    edit(&mut v, "New");
    Arc::make_mut(&mut v.slides[0]).notes = TextBody::from_text("Changed notes");
    cases.push(v);
    let mut v = p.clone();
    edit(&mut v, "New");
    Arc::make_mut(&mut v.slides[0]).shapes.push(Shape::default());
    cases.push(v);
    let mut v = p.clone();
    edit(&mut v, "New");
    Arc::make_mut(&mut v.media[0].data).push(0);
    cases.push(v);
    for changed in cases {
        assert!(export(&changed).is_err());
    }
}

#[test]
fn review_provenance_tampering_ranges_duplicates_and_source_owners_refuse() {
    let p = import(&fixture("Original", "", vec![])).unwrap();
    for range in ["v1:0:1", "v1:18446744073709551615:1", "v2:0:1", "v1:1:0", "v1:1:2:3"] {
        let mut v = p.clone();
        edit(&mut v, "New");
        Arc::make_mut(&mut v.slides[0]).shapes[0].tags.iter_mut().find(|(k, _)| k == "odp:source-range").unwrap().1 = range.into();
        assert!(export(&v).is_err());
    }
    let mut v = p.clone();
    edit(&mut v, "New");
    let shape = &mut Arc::make_mut(&mut v.slides[0]).shapes[0];
    let tag = shape.tags.iter().find(|(k, _)| k == "odp:source-range").unwrap().clone();
    shape.tags.push(tag);
    assert!(export(&v).is_err());
    let mut v = p.clone();
    edit(&mut v, "New");
    v.media.push(v.media[0].clone());
    assert!(export(&v).is_err());
}

#[test]
fn review_comment_pi_ids_and_nested_inline_semantics_are_never_overwritten() {
    for text in [
        "Or<!--retain-->iginal",
        "Or<?retain value?>iginal",
        "<text:span>Original<text:span>nested</text:span></text:span>",
        "<text:s xml:id='space'/>Original",
        "<text:span>Original</text:span><text:span>Other</text:span>",
    ] {
        refused(&fixture(text, "", vec![]));
    }
}

#[test]
fn review_plain_styled_span_keeps_lexical_owner_and_namespace_rebinding() {
    let source = fixture("<text:span xmlns:dcpatch='urn:other' text:style-name='P'>Original</text:span>", "", vec![]);
    let mut p = import(&source).unwrap();
    edit(&mut p, "A B\tC");
    let output = export(&p).unwrap();
    let parts = members(&output);
    let xml = std::str::from_utf8(&parts["content.xml"]).unwrap();
    assert!(xml.contains("<text:span xmlns:dcpatch='urn:other' text:style-name='P'>"));
    assert_eq!(import(&output).unwrap().slides[0].shapes[0].text.as_ref().unwrap().text(), "A B\tC\nSecond");
}

#[test]
fn review_rejects_newlines_controls_and_overexpanded_text() {
    let p = import(&fixture("Original", "", vec![])).unwrap();
    for text in ["A\rB", "A\nB", "A\0B", "A\u{b}B", "A\u{fffe}B"] {
        let mut v = p.clone();
        edit(&mut v, text);
        assert!(export(&v).is_err());
    }
    let mut v = p;
    edit(&mut v, &" ".repeat(500_000));
    assert!(export(&v).is_err());
}

#[test]
fn review_zip_duplicate_names_raw_aliases_and_manifest_conflicts_refuse() {
    let original = fixture("Original", "", vec![]);
    let inventory = members(&original);
    let base = || {
        vec![
            ("mimetype".into(), inventory["mimetype"].clone()),
            ("content.xml".into(), inventory["content.xml"].clone()),
            ("META-INF/manifest.xml".into(), inventory["META-INF/manifest.xml"].clone()),
            ("styles.xml".into(), inventory["styles.xml"].clone()),
        ]
    };
    for name in [
        "content.xml",
        "./content.xml",
        "Pictures/../content.xml",
        "/content.xml",
        "Pictures\\image.png",
        "Pictures/%2e%2e/content.xml",
        "Pictures//image.png",
        "Pictures/image.png:stream",
        "Pictures/image.png\0hidden",
    ] {
        let mut parts = base();
        parts.push((name.into(), Vec::new()));
        refused(&raw_zip(&parts));
    }
    let mut parts = base();
    parts[2].1 = String::from_utf8(parts[2].1.clone())
        .unwrap()
        .replace("</manifest:manifest>", "<manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/></manifest:manifest>")
        .into_bytes();
    refused(&raw_zip(&parts));
}

#[test]
fn review_zip_local_header_disagreement_crc_and_eocd_conflicts_refuse() {
    let source = fixture("Original", "", vec![]);
    let mut wrong_local = source.clone();
    wrong_local[30] = b'M';
    refused(&wrong_local);
    let mut wrong_crc = source.clone();
    wrong_crc[14] ^= 1;
    refused(&wrong_crc);
    let mut wrong_count = source.clone();
    let end = wrong_count.len() - 22;
    wrong_count[end + 10] = 2;
    refused(&wrong_count);
    let mut overlap = source.clone();
    let cd = u32::from_le_bytes(overlap[end + 16..end + 20].try_into().unwrap()) as usize;
    let second = cd + 46 + "mimetype".len();
    overlap[second + 42..second + 46].copy_from_slice(&0u32.to_le_bytes());
    refused(&overlap);
    let mut comment = source;
    let end = comment.len() - 22;
    comment[end + 20..end + 22].copy_from_slice(&22u16.to_le_bytes());
    comment.extend_from_slice(b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
    refused(&comment);
}

#[test]
fn review_active_external_opaque_preview_and_statistics_parts_refuse() {
    for extra in [
        "<office:scripts/>",
        "<draw:frame><draw:plugin xlink:href='#one'/></draw:frame>",
        "<draw:frame><draw:image xlink:href='https://example.invalid/private'/></draw:frame>",
    ] {
        refused(&fixture("Original", extra, vec![]));
    }
    for name in ["unknown.xml", "Thumbnails/thumbnail.png", "META-INF/documentsignatures.xml"] {
        refused(&fixture("Original", "", vec![(name.into(), b"opaque".to_vec())]));
    }
    let meta = format!(
        "<office:document-meta {NS} xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'><office:meta><meta:document-statistic meta:word-count='2'/></office:meta></office:document-meta>"
    );
    refused(&fixture("Original", "", vec![("meta.xml".into(), meta.into_bytes())]));
}

#[test]
fn review_inherited_master_text_cannot_acquire_local_page_ownership() {
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:master-styles><style:master-page style:name='Shared'><draw:frame svg:width='144pt' svg:height='72pt'><draw:text-box><text:p>Inherited</text:p></draw:text-box></draw:frame></style:master-page></office:master-styles></office:document-styles>"
    );
    let base = fixture("Original", "", vec![("styles.xml".into(), styles.into_bytes())]);
    let mut parts = members(&base);
    parts.insert("content.xml".into(), content("Original", "").replace("draw:master-page-name='M'", "draw:master-page-name='Shared'").into_bytes());
    let mut ordered = vec![("mimetype".into(), parts.remove("mimetype").unwrap())];
    ordered.extend(parts);
    let source = raw_zip(&ordered);
    let mut p = import(&source).unwrap();
    assert_eq!(p.slides[0].shapes.len(), 1, "shared artwork must not be copied into the local slide");
    assert_eq!(p.slides[0].shapes[0].text.as_ref().unwrap().text(), "Original\nSecond");
    let layout = p.slides[0].layout;
    let master = p.masters.iter().position(|master| master.layouts.iter().any(|candidate| candidate.id == layout)).unwrap();
    // A supported local text edit can preserve this master unchanged. Mutating the
    // actual master afterward must still fail the source-owner comparison.
    edit(&mut p, "Local edit");
    assert!(export(&p).is_ok());
    let inherited = Arc::make_mut(&mut p.masters[master])
        .shapes
        .iter_mut()
        .find(|shape| shape.text.as_ref().is_some_and(|text| text.text() == "Inherited"))
        .unwrap();
    assert!(inherited.tags.iter().any(|(key, value)| key == "odp:source-part" && value == "styles.xml"));
    inherited.text.as_mut().unwrap().paragraphs[0].runs[0].text = "Changed inherited".into();
    assert!(export(&p).is_err());
}

#[test]
fn review_missing_named_master_is_rejected_instead_of_silently_defaulted() {
    let source = fixture("Original", "", vec![]);
    let mut parts = members(&source);
    parts.insert(
        "styles.xml".into(),
        String::from_utf8(parts["styles.xml"].clone()).unwrap().replace("style:name='M'", "style:name='Other'").into_bytes(),
    );
    let mut ordered = vec![("mimetype".into(), parts.remove("mimetype").unwrap())];
    ordered.extend(parts);
    assert!(import(&raw_zip(&ordered)).unwrap_err().0.contains("missing named regular master"));
}

#[test]
fn review_raw_entry_limit_and_member_size_claims_are_bounded() {
    let source = fixture("Original", "", vec![]);
    let mut parts: Vec<_> = members(&source).into_iter().collect();
    let mime = parts.iter().position(|(name, _)| name == "mimetype").unwrap();
    parts.swap(0, mime);
    parts.extend((0..10_001).map(|i| (format!("Pictures/empty{i}.png"), Vec::new())));
    refused(&raw_zip(&parts));
    let mut oversized = source;
    let end = oversized.len() - 22;
    let cd = u32::from_le_bytes(oversized[end + 16..end + 20].try_into().unwrap()) as usize;
    let second = cd + 46 + "mimetype".len();
    let local = u32::from_le_bytes(oversized[second + 42..second + 46].try_into().unwrap()) as usize;
    oversized[second + 24..second + 28].copy_from_slice(&(32u32 * 1024 * 1024 + 1).to_le_bytes());
    oversized[local + 22..local + 26].copy_from_slice(&(32u32 * 1024 * 1024 + 1).to_le_bytes());
    refused(&oversized);
}
