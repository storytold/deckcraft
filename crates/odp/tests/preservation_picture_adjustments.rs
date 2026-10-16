//! Contributor-original source-retention fixtures for bounded raster-picture adjustments.
//! BIZ UDPGothic names the existing OFL craft-fonts face; no font bytes are added here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{Presentation, ShapeKind, style::PictureFill};
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const MANIFEST: &str = "META-INF/manifest.xml";
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const THUMB: &str = "Thumbnails/thumbnail.png";
const THUMB_OWNER: &str = "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xlink='http://www.w3.org/1999/xlink'";
const FONT: &str =
    "<style:font-face style:name='OpenFace' svg:font-family='BIZ UDPGothic' style:font-family-generic='swiss' style:font-pitch='variable'/>";
const COUNTS: &str = "<meta:document-statistic meta:word-count='4' meta:character-count='24' meta:non-whitespace-character-count='22' meta:sentence-count='2' meta:syllable-count='6' meta:page-count='1' meta:paragraph-count='2'/>";

fn fixture(version: &str, preview: bool, statistics: bool, scripts: &str) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='{version}'>{scripts}<office:font-face-decls>{FONT}</office:font-face-decls><office:automatic-styles><style:style style:name='P' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/><style:text-properties style:font-name='OpenFace' fo:font-size='24pt'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Original page' draw:master-page-name='M' xml:id='page'><draw:frame xml:id='local' svg:width='240pt' svg:height='120pt'><draw:text-box><text:p text:style-name='P' xml:id='one'>Original one</text:p><text:p text:style-name='P' xml:id='two'>Original two</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:font-face-decls>{FONT}</office:font-face-decls><office:styles/><office:automatic-styles><style:page-layout style:name='PM'><style:page-layout-properties fo:page-width='720pt' fo:page-height='405pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='M' style:page-layout-name='PM'/></office:master-styles></office:document-styles>"
    );
    let statistic = if statistics { COUNTS } else { "" };
    let meta = format!(
        "<office:document-meta {NS} office:version='{version}'><office:meta><dc:title>Original scalar title</dc:title>{statistic}</office:meta></office:document-meta>"
    );
    // A versioned document-settings root without a child is schema-valid in both versions.
    let settings = format!("<office:document-settings {NS} office:version='{version}'/>");
    let png =
        deckcraft_render::Image { width: 2, height: 2, pixels: vec![37, 109, 181, 255, 211, 73, 29, 255, 97, 181, 53, 255, 229, 193, 113, 255] }
            .to_png();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("meta.xml".into(), meta.into_bytes()),
        ("settings.xml".into(), settings.into_bytes()),
        ("Pictures/keep.png".into(), png.clone()),
    ]);
    if preview {
        files.insert(THUMB.into(), png);
    }
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><!--root survivor--><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/>\r\n"
    );
    for name in files.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name.ends_with(".png") { "image/png" } else { "text/xml" };
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>\n<!--survivor gap-->\t"));
    }
    manifest.push_str("</manifest:manifest>");
    files.insert(MANIFEST.into(), manifest.into_bytes());
    files
}
fn zip(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("mimetype", options).unwrap();
    writer.write_all(MIME.as_bytes()).unwrap();
    for (name, bytes) in files.iter().filter(|(name, _)| name.as_str() != "mimetype") {
        writer.start_file(name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        assert!(files.insert(member.name().into(), bytes).is_none());
    }
    files
}
fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(files[part].clone()).unwrap();
    assert!(text.contains(old), "missing mutation {old}");
    files.insert(part.into(), text.replace(old, new).into_bytes());
}
fn edit(presentation: &mut Presentation) {
    let text = Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap();
    text.paragraphs[0].runs[0].text = "Edited".into();
    text.paragraphs[1].runs[0].text = "Changed".into();
}
fn without_source_ranges(mut presentation: Presentation) -> Presentation {
    for source in presentation.media.iter_mut().filter(|media| media.content_type == SOURCE) {
        source.data = Arc::new(Vec::new());
    }
    for slide in &mut presentation.slides {
        let slide = Arc::make_mut(slide);
        slide.tags.retain(|(key, _)| key != "odp:source-range");
        for shape in &mut slide.shapes {
            shape.tags.retain(|(key, _)| key != "odp:source-range");
        }
    }
    presentation
}
fn preserved(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    let source = zip(files);
    let mut desired = import(&source).expect("public original fixture imports");
    assert!(export(&desired).is_err(), "an actual edit is required");
    edit(&mut desired);
    let before_export = desired.clone();
    let output = export(&desired).expect("bounded picture adjustments permit supported text edits");
    assert_eq!(desired, before_export, "export must not mutate the model");
    assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    let after = parts(&output);
    let mut expected = files.clone();
    expected.remove(THUMB);
    for (name, bytes) in &mut expected {
        if name == "content.xml" {
            *bytes = String::from_utf8(bytes.clone()).unwrap().replace("Original one", "Edited").replace("Original two", "Changed").into_bytes();
        } else if name == MANIFEST {
            *bytes = String::from_utf8(bytes.clone()).unwrap().replace(THUMB_OWNER, "").into_bytes();
        } else if name == "meta.xml" {
            let mut value = String::from_utf8(bytes.clone()).unwrap();
            for attribute in [
                " meta:word-count='4'",
                " meta:character-count='24'",
                " meta:non-whitespace-character-count='22'",
                " meta:sentence-count='2'",
                " meta:syllable-count='6'",
            ] {
                value = value.replace(attribute, "");
            }
            *bytes = value.into_bytes();
        }
    }
    assert_eq!(after, expected, "every surviving payload and all unedited XML gaps must be exact");
    let mut reopened = import(&output).expect("output reimports");
    assert_eq!(reopened.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), output);
    assert_eq!(without_source_ranges(desired), without_source_ranges(reopened.clone()), "only source bytes/ranges may change");
    Arc::make_mut(&mut reopened.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Again".into();
    let next_output = export(&reopened).expect("fresh output profile still permits a second edit");
    let mut next_expected = after.clone();
    replace(&mut next_expected, "content.xml", "Edited", "Again");
    assert_eq!(parts(&next_output), next_expected, "fresh cache proof must not change additional payloads");
    after
}
fn refused(files: &BTreeMap<String, Vec<u8>>) {
    refused_reason(files, None);
}
fn refused_reason(files: &BTreeMap<String, Vec<u8>>, expected_reason: Option<&str>) {
    let source = zip(files);
    let mut presentation = import(&source).unwrap_or_else(|_| import(&zip(&fixture("1.3", true, true, "<office:scripts/>"))).unwrap());
    edit(&mut presentation);
    // Import-rejected hostile XML must also hit the export package proof, never a warning bypass.
    presentation.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(source);
    let before = presentation.clone();
    let error = export(&presentation).expect_err("unproved container/dependency must refuse").to_string();
    assert!(error.contains("source-preserving text export refused:") || error.contains("XML"), "unexpected error: {error}");
    assert!(!error.contains("changes outside plain slide text"), "the package proof must refuse before native comparison: {error}");
    assert!(
        !error.contains("source has unsupported content beyond passive text properties")
            && !error.contains("source audit contains non-preservable semantics"),
        "the package proof must refuse before warning/audit eligibility: {error}"
    );
    if let Some(reason) = expected_reason {
        assert!(error.ends_with(reason), "expected exact package-proof reason {reason}: {error}");
    }
    assert_eq!(presentation, before, "refusal must leave the model/source unchanged");
}
fn emit(version: &str, before: &BTreeMap<String, Vec<u8>>, after: &BTreeMap<String, Vec<u8>>) {
    if let Ok(directory) = std::env::var("DECKCRAFT_PICTURE_ADJUSTMENTS_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (stage, files) in [("source", before), ("output", after)] {
            for name in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                let filename = format!("{version}-{stage}-{}", name.replace('/', "-"));
                std::fs::write(std::path::Path::new(&directory).join(filename), &files[name]).unwrap();
            }
        }
    }
}

const PICTURE_KEYS: &[(&str, &str)] = &[
    ("draw:blue", "0%"),
    ("draw:green", "0%"),
    ("draw:red", "0%"),
    ("style:mirror", "none"),
    ("draw:color-mode", "standard"),
    ("draw:image-opacity", "40%"),
    ("draw:luminance", "10%"),
    ("draw:contrast", "-20%"),
];
fn picture_fixture(version: &str, preview: bool, statistics: bool, owner: &str, family: &str, attributes: &str) -> BTreeMap<String, Vec<u8>> {
    let mut files = fixture(version, preview, statistics, "<office:scripts/>");
    let bag = format!("<style:graphic-properties {attributes}/>");
    let named = format!("<style:style style:name='PictureControls' style:family='{family}'>{bag}</style:style>");
    match owner {
        "common" => replace(&mut files, "styles.xml", "<office:styles/>", &format!("<office:styles>{named}</office:styles>")),
        "default" => replace(
            &mut files,
            "styles.xml",
            "<office:styles/>",
            &format!("<office:styles><style:default-style style:family='{family}'>{bag}</style:default-style></office:styles>"),
        ),
        "content-auto" => replace(&mut files, "content.xml", "<office:automatic-styles>", &format!("<office:automatic-styles>{named}")),
        "styles-auto" => replace(&mut files, "styles.xml", "<office:automatic-styles>", &format!("<office:automatic-styles>{named}")),
        _ => panic!("unknown contributor-original style owner"),
    }
    let reference = if family == "graphic" && matches!(owner, "common" | "content-auto") { " draw:style-name='PictureControls'" } else { "" };
    replace(
        &mut files,
        "content.xml",
        "</draw:page>",
        &format!(
            "<draw:frame xml:id='picture' svg:x='300pt' svg:y='20pt' svg:width='80pt' svg:height='80pt'{reference}><draw:image xlink:href='Pictures/keep.png' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame></draw:page>"
        ),
    );
    files
}
fn combined(mode: &str) -> String {
    format!(
        "draw:red='0%' draw:green='0%' draw:blue='0%' style:mirror='none' draw:color-mode='{mode}' draw:image-opacity='40%' draw:luminance='10%' draw:contrast='-20%'"
    )
}
fn picture(p: &Presentation) -> &PictureFill {
    p.slides[0]
        .shapes
        .iter()
        .find_map(|shape| match &shape.kind {
            ShapeKind::Picture { fill } => Some(fill),
            _ => None,
        })
        .expect("original raster picture")
}
fn picture_pixels(p: &Presentation) -> Vec<u8> {
    let slide = &p.slides[0];
    let shape = slide.shapes.iter().find(|shape| matches!(shape.kind, ShapeKind::Picture { .. })).unwrap();
    let image = deckcraft_render::render_shape(p, slide, shape.id, 0.5).unwrap();
    assert!(image.width > 0 && image.height > 0);
    assert!(image.pixels.chunks_exact(4).any(|pixel| pixel[3] != 0), "the oracle must render the original raster");
    image.pixels
}

#[test]
fn each_control_and_absence_preserve_all_scopes_families_versions_and_cache_paths() {
    for version in ["1.2", "1.3"] {
        for (preview, statistics) in [(false, false), (true, false), (false, true), (true, true)] {
            for owner in ["common", "default", "content-auto", "styles-auto"] {
                for family in ["graphic", "presentation"] {
                    preserved(&picture_fixture(version, preview, statistics, owner, family, ""));
                    for (key, value) in PICTURE_KEYS {
                        preserved(&picture_fixture(version, preview, statistics, owner, family, &format!("{key}='{value}'")));
                    }
                }
            }
        }
    }
}

#[test]
fn combined_controls_keep_picture_model_rendering_and_exact_source_bytes_and_emit_schema_parts() {
    for version in ["1.2", "1.3"] {
        for mode in ["standard", "greyscale", "mono"] {
            let files = picture_fixture(version, true, true, "content-auto", "graphic", &combined(mode));
            let original = import(&zip(&files)).unwrap();
            let fill = picture(&original);
            assert_eq!(fill.alpha, Some(0.4));
            assert_eq!(fill.adjust.brightness, 0.1);
            assert_eq!(fill.adjust.contrast, -0.2);
            assert_eq!(fill.adjust.grayscale, mode == "greyscale");
            assert_eq!(fill.adjust.bilevel, (mode == "mono").then_some(0.5));
            assert_eq!(fill.crop, [0.0; 4]);
            let original_picture = fill.clone();
            let before_pixels = picture_pixels(&original);
            let after = preserved(&files);
            assert_eq!(after["Pictures/keep.png"], files["Pictures/keep.png"]);
            let reopened = import(&zip(&after)).unwrap();
            assert_eq!(picture(&reopened), &original_picture);
            assert_eq!(picture_pixels(&reopened), before_pixels, "independent text changes must not alter picture pixels");
            if mode == "standard" {
                emit(version, &files, &after);
            }
        }
    }
}

#[test]
fn neutral_unsupported_operations_have_identical_pixels_to_absent_controls() {
    let absent = picture_fixture("1.3", true, true, "common", "graphic", "");
    let neutral = picture_fixture(
        "1.3",
        true,
        true,
        "common",
        "graphic",
        "draw:blue='0%' draw:green='0%' draw:red='0%' style:mirror='none' draw:color-mode='standard'",
    );
    let a = import(&zip(&absent)).unwrap();
    let b = import(&zip(&neutral)).unwrap();
    assert_eq!(picture(&a), picture(&b));
    assert_eq!(picture_pixels(&a), picture_pixels(&b));
    preserved(&neutral);
}

#[test]
fn modeled_adjustments_have_visible_native_effects_without_rewriting_the_raster() {
    let original = picture_fixture("1.3", true, true, "common", "graphic", "");
    let before = picture_pixels(&import(&zip(&original)).unwrap());
    for attributes in
        ["draw:image-opacity='40%'", "draw:luminance='25%'", "draw:contrast='-50%'", "draw:color-mode='greyscale'", "draw:color-mode='mono'"]
    {
        let files = picture_fixture("1.3", true, true, "common", "graphic", attributes);
        let p = import(&zip(&files)).unwrap();
        assert_ne!(picture_pixels(&p), before, "modeled picture adjustment must affect this original colored raster: {attributes}");
        let after = preserved(&files);
        assert_eq!(after["Pictures/keep.png"], original["Pictures/keep.png"]);
        assert_eq!(picture_pixels(&import(&zip(&after)).unwrap()), picture_pixels(&p));
    }
}

#[test]
fn percentage_endpoints_fractions_and_image_opacity_precedence_remain_native() {
    for version in ["1.2", "1.3"] {
        for key in ["draw:image-opacity", "draw:luminance", "draw:contrast"] {
            for value in ["0%", "00%", "0.5%", "99.25%", "100%", "100.000%"] {
                let files = picture_fixture(version, true, true, "content-auto", "graphic", &format!("{key}='{value}'"));
                let imported = import(&zip(&files)).unwrap();
                let n = value.trim_end_matches('%').parse::<f64>().unwrap() / 100.0;
                let fill = picture(&imported);
                match key {
                    "draw:image-opacity" => assert_eq!(fill.alpha, Some(n)),
                    "draw:luminance" => assert_eq!(fill.adjust.brightness, n),
                    "draw:contrast" => assert_eq!(fill.adjust.contrast, n),
                    _ => panic!("original percentage owner"),
                }
                preserved(&files);
            }
        }
        for value in ["-0%", "-0.5%", "-99.25%", "-100%", "-100.000%"] {
            preserved(&picture_fixture(version, true, true, "common", "graphic", &format!("draw:contrast='{value}'")));
        }
        let files = picture_fixture(version, true, true, "common", "graphic", "draw:opacity='75%' draw:image-opacity='25%'");
        assert_eq!(picture(&import(&zip(&files)).unwrap()).alpha, Some(0.25));
        preserved(&files);
        // Exactly 64 bytes belongs to the admitted bounded grammar; the next byte refuses.
        let at_bound = format!("0.{}%", "0".repeat(61));
        assert_eq!(at_bound.len(), 64);
        preserved(&picture_fixture(version, true, true, "common", "graphic", &format!("draw:contrast='{at_bound}'")));
        let over_bound = format!("0.{}%", "0".repeat(62));
        refused_reason(
            &picture_fixture(version, true, true, "common", "graphic", &format!("draw:contrast='{over_bound}'")),
            Some("cache unclassified attribute/value"),
        );
    }
}

#[test]
fn aliases_entities_attribute_order_comments_and_xml_gaps_are_exact_survivors() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            let mut files = picture_fixture(version, cache, cache, "content-auto", "graphic", "");
            replace(
                &mut files,
                "content.xml",
                "<style:graphic-properties />",
                "<s:graphic-properties xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' s:mirror = 'n&#111;ne' d:blue = \"0&#37;\" d:green='0%' d:red='0%' d:color-mode='greysc&#97;le' d:image-opacity='4&#48;%' d:luminance='1&#48;%' d:contrast='-2&#48;%'> <!--original raster-property gap--> \n </s:graphic-properties>",
            );
            preserved(&files);
        }
    }
}

#[test]
fn nonneutral_rgb_noncanonical_zero_and_directional_or_unknown_mirrors_remain_denied() {
    for key in ["draw:blue", "draw:green", "draw:red"] {
        for value in ["1%", "-1%", "100%", "-100%", "00%", "0.0%", "-0%", "+0%", "0", "0% ", " 0%", "0&#9;%", "0% 0%", "file:///color"] {
            refused_reason(
                &picture_fixture("1.3", true, true, "content-auto", "graphic", &format!("{key}='{value}'")),
                Some("cache unclassified attribute/value"),
            );
        }
    }
    for value in [
        "vertical",
        "horizontal",
        "horizontal-on-even",
        "horizontal-on-odd",
        "vertical horizontal",
        "horizontal vertical",
        "None",
        "none ",
        "",
        "none none",
        "file:///mirror",
    ] {
        refused_reason(
            &picture_fixture("1.3", true, true, "common", "graphic", &format!("style:mirror='{value}'")),
            Some("cache unclassified attribute/value"),
        );
    }
}

#[test]
fn unsupported_modes_and_bad_percentage_types_ranges_and_lexical_forms_remain_denied() {
    for mode in ["watermark", "gray", "grayscale", "Standard", "MONO", " standard", "mono ", "mono greyscale", "", "Pictures/keep.png"] {
        refused_reason(
            &picture_fixture("1.3", true, true, "content-auto", "graphic", &format!("draw:color-mode='{mode}'")),
            Some("cache unclassified attribute/value"),
        );
    }
    for version in ["1.2", "1.3"] {
        for key in ["draw:image-opacity", "draw:luminance", "draw:contrast"] {
            for value in [
                "",
                "0",
                "+0%",
                "1e1%",
                "NaN%",
                "inf%",
                "101%",
                "100.01%",
                "000%",
                "0.%",
                ".5%",
                "0.5.0%",
                " 0%",
                "0% ",
                "0%&#9;",
                "0% 1%",
                "1pt",
                "file:///percent",
            ] {
                refused_reason(
                    &picture_fixture(version, true, true, "content-auto", "graphic", &format!("{key}='{value}'")),
                    Some("cache unclassified attribute/value"),
                );
            }
            if key != "draw:contrast" {
                for value in ["-0%", "-1%", "-100%"] {
                    refused_reason(
                        &picture_fixture(version, true, true, "content-auto", "graphic", &format!("{key}='{value}'")),
                        Some("cache unclassified attribute/value"),
                    );
                }
            } else {
                refused_reason(
                    &picture_fixture(version, true, true, "common", "graphic", "draw:contrast='-100.01%'"),
                    Some("cache unclassified attribute/value"),
                );
            }
        }
    }
}

#[test]
fn foreign_unqualified_rebound_and_duplicate_attribute_identities_remain_closed() {
    for (key, value) in PICTURE_KEYS {
        let (prefix, local) = key.split_once(':').unwrap();
        let uri =
            if prefix == "draw" { "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" } else { "urn:oasis:names:tc:opendocument:xmlns:style:1.0" };
        for bad in [
            format!("{local}='{value}'"),
            format!("xmlns:f='urn:deckcraft:test:foreign' f:{local}='{value}'"),
            format!("xmlns:{prefix}='urn:deckcraft:test:foreign' {key}='{value}'"),
        ] {
            refused_reason(&picture_fixture("1.3", true, true, "common", "graphic", &bad), Some("cache XML identity/depth"));
        }
        let duplicate = format!("xmlns:a='{uri}' {key}='{value}' a:{local}='{value}'");
        refused_reason(&picture_fixture("1.3", true, true, "content-auto", "graphic", &duplicate), Some("duplicate expanded XML attribute"));
    }
}

#[test]
fn wrong_bags_families_direct_owners_parts_and_children_remain_denied() {
    for (key, value) in PICTURE_KEYS {
        for owner in ["common", "default", "content-auto", "styles-auto"] {
            let part = if owner == "content-auto" { "content.xml" } else { "styles.xml" };
            let mut files = picture_fixture("1.3", true, true, owner, "graphic", &format!("{key}='{value}'"));
            replace(&mut files, part, "style:family='graphic'", "style:family='drawing-page'");
            replace(&mut files, part, "style:graphic-properties", "style:drawing-page-properties");
            refused_reason(&files, Some("cache unclassified attribute/value"));
            let mut files = picture_fixture("1.3", true, true, owner, "paragraph", &format!("{key}='{value}'"));
            refused_reason(&files, Some("cache unclassified XML context"));
            replace(&mut files, part, "style:graphic-properties", "style:paragraph-properties");
            refused_reason(&files, Some("cache unclassified attribute/value"));
        }
        let mut direct = picture_fixture("1.3", true, true, "common", "graphic", "");
        replace(&mut direct, "content.xml", "xml:id='picture'", &format!("xml:id='picture' {key}='{value}'"));
        refused_reason(&direct, Some("cache unclassified attribute/value"));
        let mut direct = picture_fixture("1.3", true, true, "common", "graphic", "");
        replace(&mut direct, "content.xml", "xlink:type='simple'", &format!("xlink:type='simple' {key}='{value}'"));
        refused_reason(&direct, Some("cache unclassified attribute/value"));
    }
    for bag in [
        "<style:graphic-properties><style:default-style style:family='graphic'/></style:graphic-properties>",
        "<style:graphic-properties><draw:red/></style:graphic-properties>",
        "<f:graphic-properties xmlns:f='urn:deckcraft:test:foreign' draw:red='0%'/>",
    ] {
        let mut files = picture_fixture("1.3", true, true, "common", "graphic", "");
        replace(&mut files, "styles.xml", "<style:graphic-properties />", bag);
        refused(&files);
    }
    let mut files = picture_fixture("1.3", true, true, "common", "graphic", "");
    replace(&mut files, "styles.xml", "style:name='PictureControls'", "style:name='PictureControls' draw:blue='0%'");
    refused_reason(&files, Some("cache unclassified attribute/value"));
    let mut files = picture_fixture("1.3", true, true, "common", "graphic", "");
    replace(&mut files, "meta.xml", "<office:meta>", "<office:meta><style:graphic-properties draw:blue='0%'/>");
    refused_reason(&files, Some("cache unclassified XML context"));
}

#[test]
fn neutral_controls_do_not_hide_gamma_clip_unknown_scalars_references_or_active_content() {
    for extra in [
        "draw:gamma='NaN%'",
        "fo:clip='inherit'",
        "fo:clip='rect(1pt, 0pt, 0pt, 0pt)'",
        "draw:color-inversion='false'",
        "style:protect='none'",
        "draw:auto-grow-width='false'",
        "draw:fit-to-contour='false'",
        "xlink:href='Thumbnails/thumbnail.png'",
        "xml:base='file:///'",
    ] {
        refused_reason(
            &picture_fixture("1.3", true, true, "content-auto", "graphic", &format!("draw:blue='0%' style:mirror='none' {extra}")),
            Some("cache unclassified attribute/value"),
        );
    }
    let mut files =
        picture_fixture("1.3", true, true, "content-auto", "graphic", "draw:blue='0%' draw:fill='bitmap' draw:fill-image-name='MissingRaster'");
    refused_reason(&files, Some("cache unresolved typed reference"));
    replace(&mut files, "content.xml", "draw:fill='bitmap' draw:fill-image-name='MissingRaster'", "draw:fill='none'");
    replace(&mut files, "content.xml", "xml:id='page'>", "xml:id='page'><office:forms/>");
    refused_reason(&files, Some("cache unclassified XML context"));
}

#[test]
fn actual_image_references_still_require_retained_raster_members_and_matching_owners() {
    for bad in [
        "Thumbnails/thumbnail.png",
        "../Pictures/keep.png",
        "Pictures/missing.png",
        "file:///original.png",
        "https://example.invalid/original.png",
        "Pictures/keep.png#part",
        "Pictures/%6beep.png",
        "Pictures//keep.png",
    ] {
        let mut files = picture_fixture("1.3", true, true, "content-auto", "graphic", &combined("standard"));
        replace(&mut files, "content.xml", "xlink:href='Pictures/keep.png'", &format!("xlink:href='{bad}'"));
        refused(&files);
    }
    let mut files = picture_fixture("1.3", true, true, "common", "graphic", &combined("standard"));
    replace(
        &mut files,
        MANIFEST,
        "manifest:full-path='Pictures/keep.png' manifest:media-type='image/png'",
        "manifest:full-path='Pictures/keep.png' manifest:media-type='text/plain'",
    );
    refused(&files);
    let mut files = picture_fixture("1.3", true, true, "common", "graphic", &combined("standard"));
    files.insert("Pictures/keep.png".into(), b"contributor-original non-raster payload".to_vec());
    refused(&files);
    let mut files = picture_fixture("1.3", true, true, "common", "graphic", &combined("standard"));
    replace(&mut files, "content.xml", "<draw:image xlink:href=", "<draw:image xml:base='file:///' xlink:href=");
    refused_reason(&files, Some("cache unclassified attribute/value"));
}

#[test]
fn native_picture_adjust_crop_media_flip_and_geometry_edits_still_refuse_without_mutation() {
    // ODP has no engine dev-dependency. Root's existing engine context owns command coverage.
    for change in ["alpha", "brightness", "contrast", "grayscale", "bilevel", "crop", "media", "flip", "geometry"] {
        let files = picture_fixture("1.3", true, true, "content-auto", "graphic", &combined("standard"));
        let source = zip(&files);
        let mut desired = import(&source).unwrap();
        edit(&mut desired);
        let shape = Arc::make_mut(&mut desired.slides[0]).shapes.iter_mut().find(|shape| matches!(shape.kind, ShapeKind::Picture { .. })).unwrap();
        if change == "flip" {
            shape.xfrm.as_mut().unwrap().flip_h = true;
        } else if change == "geometry" {
            shape.xfrm.as_mut().unwrap().w += 1.0;
        } else {
            let ShapeKind::Picture { fill } = &mut shape.kind else { panic!("original picture kind") };
            match change {
                "alpha" => fill.alpha = Some(0.8),
                "brightness" => fill.adjust.brightness = 0.5,
                "contrast" => fill.adjust.contrast = 0.5,
                "grayscale" => fill.adjust.grayscale = true,
                "bilevel" => fill.adjust.bilevel = Some(0.5),
                "crop" => fill.crop[0] = 0.1,
                "media" => fill.media = deckcraft_model::MediaId(u32::MAX),
                _ => panic!("original native-change case"),
            }
        }
        let before = desired.clone();
        let error = export(&desired).expect_err("picture changes are not plain-text edits").to_string();
        assert!(error.ends_with("changes outside plain slide text are unsupported"), "source proof must pass before native-change refusal: {error}");
        assert_eq!(desired, before);
        assert_eq!(desired.media.iter().find(|item| item.content_type == SOURCE).unwrap().data.as_slice(), source);
    }
}

#[test]
fn image_adjustments_do_not_exempt_independent_master_field_source_warnings() {
    let mut files = picture_fixture("1.3", true, true, "content-auto", "graphic", &combined("standard"));
    replace(
        &mut files,
        "styles.xml",
        "<office:styles/>",
        "<office:styles><style:default-style style:family='drawing-page'><style:drawing-page-properties presentation:display-header='true'/></style:default-style></office:styles>",
    );
    let mut desired = import(&zip(&files)).unwrap();
    assert!(desired.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("regular master placeholders or inherited visibility")));
    edit(&mut desired);
    let before = desired.clone();
    let error = export(&desired).expect_err("unsupported independent owner semantics must still refuse").to_string();
    assert!(
        error.ends_with("source has unsupported content beyond passive text properties"),
        "picture classification must not bypass warning policy: {error}"
    );
    assert_eq!(desired, before);
}
