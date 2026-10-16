//! Independent contributor-original passive frame-constraint package fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::Presentation;
use deckcraft_odp::{MIME, export, import};
use std::{collections::BTreeMap, io::Cursor, io::Read, io::Write, sync::Arc};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
type Parts = BTreeMap<String, Vec<u8>>;

fn fixture(properties: &str, cache: bool) -> Parts {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='Frame' style:family='graphic'><style:graphic-properties {properties}/></style:style><style:style style:name='Keep' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Independent'><draw:frame xml:id='frame' draw:style-name='Frame' svg:width='200pt' svg:height='120pt'><draw:text-box><!--before changed body--><text:p xml:id='paragraph' text:style-name='Keep'>Original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let counts = if cache { "<meta:document-statistic meta:page-count='1' meta:word-count='1'/>" } else { "" };
    let metadata =
        format!("<office:document-meta {NS} office:version='1.3'><office:meta><!--metadata sentinel-->{counts}</office:meta></office:document-meta>");
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='1.3'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='meta.xml' manifest:media-type='text/xml'/></manifest:manifest>"
    );
    BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("meta.xml".into(), metadata.into_bytes()),
        ("META-INF/manifest.xml".into(), manifest.into_bytes()),
    ])
}

fn package(parts: &Parts) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in std::iter::once("mimetype").chain(parts.keys().map(String::as_str).filter(|name| *name != "mimetype")) {
        zip.start_file(name, stored).unwrap();
        zip.write_all(&parts[name]).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn unpack(bytes: &[u8]) -> Parts {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Parts::new();
    for index in 0..zip.len() {
        let mut member = zip.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        assert!(out.insert(member.name().into(), bytes).is_none());
    }
    out
}

fn replace(parts: &mut Parts, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(parts[part].clone()).unwrap();
    assert!(text.contains(old), "fixture replacement absent");
    parts.insert(part.into(), text.replace(old, new).into_bytes());
}

fn edit(p: &mut Presentation, value: &str) {
    Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = value.into();
}

fn preserved(parts: &Parts, cache: bool) {
    let source = package(parts);
    let mut current = import(&source).unwrap();
    let unchanged = export(&current).unwrap_err().to_string();
    assert!(unchanged.contains("no supported text edits"), "eligible audit/passive baseline required: {unchanged}");
    let geometry = current.slides[0].shapes[0].xfrm;
    let body = current.slides[0].shapes[0].text.as_ref().unwrap().body.clone();
    edit(&mut current, "Revised");
    let current = deckcraft_format::load(&deckcraft_format::save(&current).unwrap()).unwrap();
    let frozen = current.clone();
    let output = export(&current).unwrap();
    assert_eq!(current, frozen);
    let mut expected = parts.clone();
    // Only the supported changed paragraph body is regenerated; the preceding comment survives.
    replace(&mut expected, "content.xml", ">Original<", ">Revised<");
    if cache {
        replace(&mut expected, "meta.xml", " meta:word-count='1'", "");
    }
    assert_eq!(unpack(&output), expected);
    assert_eq!(current.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    let mut reopened = import(&output).unwrap();
    assert_eq!(reopened.slides[0].shapes[0].xfrm, geometry);
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().body, body);
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().text(), "Revised");
    edit(&mut reopened, "Again");
    replace(&mut expected, "content.xml", ">Revised<", ">Again<");
    assert_eq!(unpack(&export(&reopened).unwrap()), expected, "fresh output proof must preserve the same constraints");
}

fn refused(parts: &Parts) -> String {
    let mut current = import(&package(parts)).unwrap_or_else(|_| import(&package(&fixture("", false))).unwrap());
    current.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(package(parts));
    edit(&mut current, "Revised");
    let frozen = current.clone();
    let error = export(&current).expect_err("unproved frame properties must refuse").to_string();
    assert_eq!(current, frozen);
    error
}

#[test]
fn aliases_entities_absolute_minima_and_unused_definitions_keep_exact_survivors() {
    let properties = "xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:f='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' d:auto-grow-height=\"tr&#117;e\" d:fit-to-size='f&#97;lse' s:shrink-to-fit='false' f:wrap-option='no-wrap' f:min-height='9999999.5pt' f:min-width='0cm'";
    for cache in [false, true] {
        let mut parts = fixture(properties, cache);
        let styles = format!(
            "<office:document-styles {NS} office:version='1.3'><office:styles><style:default-style style:family='presentation'><style:graphic-properties fo:min-width='1pc' style:shrink-to-fit='false'/></style:default-style></office:styles><office:automatic-styles><style:style style:name='Unused' style:family='graphic'><style:graphic-properties draw:auto-grow-height='false' fo:min-height='10000000pt'/></style:style></office:automatic-styles></office:document-styles>"
        );
        parts.insert("styles.xml".into(), styles.into_bytes());
        replace(
            &mut parts,
            "META-INF/manifest.xml",
            "</manifest:manifest>",
            "<manifest:file-entry manifest:full-path='styles.xml' manifest:media-type='text/xml'/></manifest:manifest>",
        );
        preserved(&parts, cache);
    }
}

#[test]
fn no_overflow_model_edit_retains_each_allowed_flag_and_length_unit() {
    for cache in [false, true] {
        for properties in [
            "",
            "draw:auto-grow-height='true'",
            "draw:auto-grow-height='false' draw:fit-to-size='false' style:shrink-to-fit='true' fo:wrap-option='wrap'",
            "fo:min-width='1pt' fo:min-height='1pc'",
            "fo:min-width='1in' fo:min-height='1cm'",
            "fo:min-width='1mm' fo:min-height='0pt'",
        ] {
            preserved(&fixture(properties, cache), cache);
        }
    }
}

#[test]
fn cache_profile_never_coerces_boolean_wrap_stretch_or_length_values() {
    for property in ["draw:auto-grow-height", "style:shrink-to-fit"] {
        for value in ["1", "0", "TRUE", "False", " true", "false ", "true false", "https://example.invalid/"] {
            refused(&fixture(&format!("{property}='{value}'"), true));
        }
    }
    for value in ["true", "all", "shrink-to-fit", "none", "False", "0", "false true", "false "] {
        refused(&fixture(&format!("draw:fit-to-size='{value}'"), true));
    }
    for value in ["nowrap", "WRAP", "no-wrap ", "wrap no-wrap", "false", ""] {
        refused(&fixture(&format!("fo:wrap-option='{value}'"), true));
    }
    for property in ["fo:min-height", "fo:min-width"] {
        for value in ["-0pt", "+1pt", "-1pt", "1e1pt", "NaNpt", "infpt", "10000000.01pt", "1px", "10%", "1pt ", ".5pt", "1.pt", ""] {
            refused(&fixture(&format!("{property}='{value}'"), true));
        }
    }
}

#[test]
fn exact_namespace_leaf_and_family_owners_are_not_replaced_by_local_names() {
    for properties in [
        "auto-grow-height='false'",
        "xmlns:draw='urn:review:drawing' draw:auto-grow-height='false'",
        "xmlns:fo='urn:review:format' fo:min-width='0pt'",
        "xmlns:s='urn:review:style' s:shrink-to-fit='false'",
        "xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' draw:auto-grow-height='false' d:auto-grow-height='false'",
    ] {
        refused(&fixture(properties, true));
    }
    for (from, to) in [
        ("style:family='graphic'", "style:family='paragraph'"),
        ("style:graphic-properties", "style:paragraph-properties"),
        ("style:graphic-properties", "style:drawing-page-properties"),
        ("style:graphic-properties", "style:text-properties"),
        ("<draw:frame xml:id=", "<draw:frame draw:auto-grow-height='false' xml:id="),
        ("<draw:text-box>", "<draw:text-box fo:min-width='0pt'>"),
    ] {
        let mut parts = fixture("draw:auto-grow-height='false'", true);
        replace(&mut parts, "content.xml", from, to);
        refused(&parts);
    }
    let mut child = fixture("draw:auto-grow-height='false'", true);
    replace(
        &mut child,
        "content.xml",
        "<style:graphic-properties draw:auto-grow-height='false'/>",
        "<style:graphic-properties draw:auto-grow-height='false'><draw:auto-grow-height/></style:graphic-properties>",
    );
    refused(&child);
}

#[test]
fn false_flags_cannot_hide_unresolved_fonts_images_active_content_or_unused_bad_sizing() {
    for sibling in [
        "<style:style style:name='Hidden' style:family='graphic'><style:graphic-properties draw:auto-grow-width='false'/></style:style>",
        "<style:style style:name='Hidden' style:family='graphic'><style:graphic-properties draw:fit-to-contour='false'/></style:style>",
        "<style:style style:name='Hidden' style:family='graphic'><style:graphic-properties fo:max-height='100pt'/></style:style>",
        "<style:style style:name='Hidden' style:family='graphic'><style:graphic-properties draw:fill='bitmap' draw:fill-image-name='Missing'/></style:style>",
        "<style:style style:name='Hidden' style:family='paragraph'><style:text-properties style:font-name='Missing'/></style:style>",
        "<style:style style:name='Hidden' style:family='graphic'><style:graphic-properties draw:fit-to-size='true'/></style:style>",
    ] {
        let mut parts = fixture("draw:auto-grow-height='false' draw:fit-to-size='false' style:shrink-to-fit='false'", true);
        replace(&mut parts, "content.xml", "</office:automatic-styles>", &format!("{sibling}</office:automatic-styles>"));
        refused(&parts);
    }
    for cache in [false, true] {
        let mut parts = fixture("draw:auto-grow-height='false'", cache);
        replace(&mut parts, "content.xml", "<office:body>", "<office:scripts><office:script/></office:scripts><office:body>");
        refused(&parts);
        let mut parts = fixture("draw:auto-grow-height='false'", cache);
        replace(
            &mut parts,
            "content.xml",
            "<draw:text-box>",
            "<draw:text-box xmlns:xlink='http://www.w3.org/1999/xlink' xlink:href='https://example.invalid/'>",
        );
        refused(&parts);
    }
}

#[test]
fn source_retained_minima_do_not_authorize_geometry_wrap_inset_or_fit_changes() {
    for cache in [false, true] {
        for change in [0, 1, 2, 3] {
            let source = package(&fixture("fo:min-height='100pt' fo:min-width='200pt' draw:fit-to-size='false'", cache));
            let mut current = import(&source).unwrap();
            edit(&mut current, "Revised");
            let shape = &mut Arc::make_mut(&mut current.slides[0]).shapes[0];
            match change {
                0 => shape.xfrm.as_mut().unwrap().h += 1.0,
                1 => shape.text.as_mut().unwrap().body.wrap = Some(false),
                2 => shape.text.as_mut().unwrap().body.inset_l = Some(1.0),
                _ => shape.text.as_mut().unwrap().body.autofit = Some(deckcraft_model::text::AutoFit::Shape),
            }
            let frozen = current.clone();
            let error = export(&current).unwrap_err().to_string();
            assert!(error.contains("changes outside plain slide text"), "{error}");
            assert_eq!(current, frozen);
            assert_eq!(current.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
        }
    }
}

#[test]
fn comments_inside_the_changed_paragraph_still_refuse_lexical_patch_ownership() {
    for cache in [false, true] {
        let mut parts = fixture("draw:auto-grow-height='false' draw:fit-to-size='false'", cache);
        replace(&mut parts, "content.xml", ">Original<", "><!--inside changed body-->Original<");
        let error = refused(&parts);
        assert!(error.contains("source text contains unverified lexical content"), "{error}");
    }
}
