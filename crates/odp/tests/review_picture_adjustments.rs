//! Independent W12 contributor-original raster/source-retention adversaries.
//! Pixel arrays are original; Liberation Sans names an existing openly licensed face.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{Presentation, ShapeKind, style::PictureFill};
use deckcraft_odp::{MIME, export, import};
use std::{collections::BTreeMap, io::Cursor, io::Read, io::Write, sync::Arc};

type Parts = BTreeMap<String, Vec<u8>>;
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MANIFEST: &str = "META-INF/manifest.xml";
const RASTER: &str = "Pictures/review.png";
const THUMB: &str = "Thumbnails/thumbnail.png";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:xlink='http://www.w3.org/1999/xlink' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";
const FONT: &str = "<office:font-face-decls><style:font-face style:name='Open' svg:font-family='Liberation Sans'/></office:font-face-decls>";
const NEUTRAL: &str = "draw:blue='0%' draw:green='0%' draw:red='0%' style:mirror='none'";

fn replace(parts: &mut Parts, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(parts[part].clone()).unwrap();
    assert!(text.contains(old), "missing independent mutation {old}");
    parts.insert(part.into(), text.replace(old, new).into_bytes());
}

fn fixture(version: &str, properties: &str, cache: bool) -> Parts {
    let content = format!(
        "<office:document-content {NS} office:version='{version}'><office:scripts/>{FONT}<office:automatic-styles><style:style style:name='Image' style:family='graphic'><style:graphic-properties {properties}/></style:style><style:style style:name='Keep' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/><style:text-properties style:font-name='Open' fo:font-size='16pt'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Independent raster' draw:master-page-name='Master' xml:id='page'><draw:frame xml:id='picture' draw:style-name='Image' svg:x='20pt' svg:y='20pt' svg:width='80pt' svg:height='80pt'><draw:image xlink:href='Pictures/review.png' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame><!--between real objects--><draw:frame xml:id='editable' svg:x='160pt' svg:y='20pt' svg:width='200pt' svg:height='160pt'><draw:text-box><!--before real text--><text:p text:style-name='Keep'>First original</text:p><text:p text:style-name='Keep'>Second original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'>{FONT}<office:styles/><office:automatic-styles><style:page-layout style:name='Page'><style:page-layout-properties fo:page-width='400pt' fo:page-height='240pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='Master' style:page-layout-name='Page'/></office:master-styles></office:document-styles>"
    );
    let counts = if cache { "<meta:document-statistic meta:word-count='4' meta:page-count='1'/>" } else { "" };
    let raster = deckcraft_render::Image {
        width: 3,
        height: 2,
        pixels: vec![23, 71, 163, 255, 207, 83, 31, 255, 43, 179, 97, 255, 223, 193, 73, 255, 117, 43, 149, 255, 13, 157, 181, 255],
    }
    .to_png();
    let mut parts: Parts = BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        (
            "meta.xml".into(),
            format!(
                "<office:document-meta {NS} office:version='{version}'><office:meta><!--literal gap Ω-->{counts}</office:meta></office:document-meta>"
            )
            .into_bytes(),
        ),
        ("settings.xml".into(), format!("<office:document-settings {NS} office:version='{version}'/>").into_bytes()),
        (RASTER.into(), raster.clone()),
    ]);
    if cache {
        parts.insert(THUMB.into(), raster);
    }
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/><!--owner gap-->\r\n"
    );
    for name in parts.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name.ends_with(".png") { "image/png" } else { "text/xml" };
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>\n<!--retained gap-->\t"));
    }
    manifest.push_str("</manifest:manifest>");
    parts.insert(MANIFEST.into(), manifest.into_bytes());
    parts
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
    let mut parts = Parts::new();
    for index in 0..zip.len() {
        let mut member = zip.by_index(index).unwrap();
        let mut payload = Vec::new();
        member.read_to_end(&mut payload).unwrap();
        assert!(parts.insert(member.name().into(), payload).is_none());
    }
    parts
}

fn assert_member_map(actual: &Parts, expected: &Parts) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>(), "complete surviving inventory must match");
    for (name, wanted) in expected {
        let found = &actual[name];
        assert!(
            found == wanted,
            "member {name} differs: actual {} bytes, expected {} bytes, first differing byte offset {:?}",
            found.len(),
            wanted.len(),
            found.iter().zip(wanted).position(|(a, b)| a != b),
        );
    }
}

fn edit(p: &mut Presentation, first: &str, second: &str) {
    let text = Arc::make_mut(&mut p.slides[0]).shapes[1].text.as_mut().unwrap();
    text.paragraphs[0].runs[0].text = first.into();
    text.paragraphs[1].runs[0].text = second.into();
}

fn picture(p: &Presentation) -> &PictureFill {
    let ShapeKind::Picture { fill } = &p.slides[0].shapes[0].kind else { panic!("actual native raster picture required") };
    fill
}

fn picture_pixels(p: &Presentation) -> (u32, u32, Vec<u8>) {
    let image = deckcraft_render::render_shape(p, &p.slides[0], p.slides[0].shapes[0].id, 1.0).unwrap();
    (image.width, image.height, image.pixels)
}

fn normalize(mut p: Presentation) -> Presentation {
    p.media.iter_mut().filter(|media| media.content_type == SOURCE).for_each(|media| media.data = Arc::new(Vec::new()));
    for slide in &mut p.slides {
        let slide = Arc::make_mut(slide);
        slide.tags.retain(|(key, _)| key != "odp:source-range");
        for shape in &mut slide.shapes {
            shape.tags.retain(|(key, _)| key != "odp:source-range");
        }
    }
    p
}

fn preserved(parts: &Parts, cache: bool) -> Parts {
    let source = package(parts);
    let immutable_source = source.clone();
    let mut current = import(&source).expect("independent original raster fixture imports");
    assert_eq!(current.slides[0].shapes.len(), 2);
    let unchanged = export(&current).unwrap_err().to_string();
    assert!(unchanged.contains("no supported text edits"), "must reach passive audit eligibility: {unchanged}");
    let original_picture = picture(&current).clone();
    assert_eq!(current.media.iter().find(|media| media.id == original_picture.media).unwrap().data.as_slice(), parts[RASTER]);
    let pixels = picture_pixels(&current);
    edit(&mut current, "FirstRevised", "SecondRevised");
    let current = deckcraft_format::load(&deckcraft_format::save(&current).unwrap()).unwrap();
    assert_eq!(picture_pixels(&current), pixels, "native save/load and independent text edits must retain actual image render");
    let frozen = current.clone();
    let output = export(&current).expect("closed image controls permit real text edits");
    assert_eq!(current, frozen, "export is observational");
    assert_eq!(source, immutable_source, "input bytes are immutable");
    assert_eq!(current.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    let mut expected = parts.clone();
    replace(&mut expected, "content.xml", ">First original<", ">FirstRevised<");
    replace(&mut expected, "content.xml", ">Second original<", ">SecondRevised<");
    if cache {
        expected.remove(THUMB);
        replace(&mut expected, MANIFEST, "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>", "");
        replace(&mut expected, "meta.xml", " meta:word-count='4'", "");
    }
    let after = unpack(&output);
    assert_member_map(&after, &expected);
    assert_eq!(after[RASTER], parts[RASTER]);
    assert_eq!(after["styles.xml"], parts["styles.xml"]);
    let mut reopened = import(&output).unwrap();
    assert_eq!(picture(&reopened), &original_picture);
    assert_eq!(picture_pixels(&reopened), pixels, "output picture render must be identical");
    assert_eq!(normalize(current), normalize(reopened.clone()), "only original-source bytes and lexical ranges may change");
    edit(&mut reopened, "ThirdRevised", "FourthRevised");
    replace(&mut expected, "content.xml", ">FirstRevised<", ">ThirdRevised<");
    replace(&mut expected, "content.xml", ">SecondRevised<", ">FourthRevised<");
    assert_member_map(&unpack(&export(&reopened).unwrap()), &expected);
    after
}

fn refused(parts: &Parts) -> String {
    let source = package(parts);
    let mut current = import(&source).unwrap_or_else(|_| import(&package(&fixture("1.3", NEUTRAL, true))).unwrap());
    edit(&mut current, "FirstRevised", "SecondRevised");
    current.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(source.clone());
    let frozen = current.clone();
    let error = export(&current).expect_err("unproved cache profile must refuse before text comparison").to_string();
    assert!(error.contains("source-preserving text export refused:") || error.contains("XML"), "unexpected failure {error}");
    assert!(!error.contains("changes outside plain slide text"), "must refuse at package proof: {error}");
    assert!(
        !error.contains("source has unsupported content beyond passive text properties")
            && !error.contains("source audit contains non-preservable semantics"),
        "must refuse before warning/audit gate: {error}"
    );
    assert_eq!(current, frozen);
    assert_eq!(current.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    error
}

#[test]
fn independent_live_raster_all_eight_controls_both_versions_cache_routes_and_modes() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for mode in ["standard", "greyscale", "mono"] {
                let attributes = format!(
                    "{NEUTRAL} draw:color-mode='{mode}' draw:image-opacity='62.5%' draw:opacity='90%' draw:luminance='12.5%' draw:contrast='-37.5%'"
                );
                let parts = fixture(version, &attributes, cache);
                let native = import(&package(&parts)).unwrap();
                let fill = picture(&native);
                assert_eq!(fill.alpha, Some(0.625), "image opacity keeps existing precedence");
                assert_eq!(fill.adjust.brightness, 0.125);
                assert_eq!(fill.adjust.contrast, -0.375);
                assert_eq!(fill.adjust.grayscale, mode == "greyscale");
                assert_eq!(fill.adjust.bilevel, (mode == "mono").then_some(0.5));
                assert_eq!(fill.crop, [0.0; 4]);
                assert!(picture_pixels(&native).2.chunks_exact(4).any(|pixel| pixel[3] > 0), "render must contain the actual raster");
                let after = preserved(&parts, cache);
                if cache
                    && mode == "standard"
                    && let Ok(directory) = std::env::var("DECKCRAFT_REVIEW_PICTURE_XML_DIR")
                {
                    std::fs::create_dir_all(&directory).unwrap();
                    for (stage, map) in [("source", &parts), ("output", &after)] {
                        for part in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                            let name = format!("{version}-{stage}-{}", part.replace('/', "-"));
                            std::fs::write(std::path::Path::new(&directory).join(name), &map[part]).unwrap();
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn independent_decimal_boundary_exact_neutrality_and_decoded_aliases() {
    for key in ["draw:image-opacity", "draw:luminance", "draw:contrast"] {
        for value in ["0%", "00%", "09.125%", "99.999%", "100%", "100.000%"] {
            preserved(&fixture("1.3", &format!("{key}='{value}'"), true), true);
        }
        let at_limit = format!("0.{}%", "0".repeat(61));
        assert_eq!(at_limit.len(), 64);
        preserved(&fixture("1.3", &format!("{key}='{at_limit}'"), true), true);
    }
    for value in ["-0%", "-00%", "-09.125%", "-100.000%"] {
        preserved(&fixture("1.3", &format!("draw:contrast='{value}'"), true), true);
    }
    let aliases = "xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' d:red='&#48;%' d:green=\"0&#37;\" d:blue = '0%' s:mirror='n&#111;ne' d:image-opacity='6&#50;.5%' d:luminance='00%' d:contrast='-00.5%' d:color-mode='greysc&#97;le'";
    preserved(&fixture("1.3", aliases, true), true);
}

#[test]
fn independent_common_default_and_both_auto_scopes_remain_exact() {
    let controls = format!("{NEUTRAL} draw:image-opacity='75%' draw:luminance='5%' draw:contrast='-5%' draw:color-mode='greyscale'");
    for version in ["1.2", "1.3"] {
        for family in ["graphic", "presentation"] {
            for scope in ["common", "default", "content-auto", "styles-auto"] {
                // Keep a live raster with these same controls while challenging unused
                // definition traversal and exact per-part/family ownership independently.
                let mut parts = fixture(version, &controls, true);
                let named = format!("<style:style style:name='Other' style:family='{family}'><style:graphic-properties {controls}/></style:style>");
                match scope {
                    "common" => replace(&mut parts, "styles.xml", "<office:styles/>", &format!("<office:styles>{named}</office:styles>")),
                    "default" => replace(
                        &mut parts,
                        "styles.xml",
                        "<office:styles/>",
                        &format!(
                            "<office:styles><style:default-style style:family='{family}'><style:graphic-properties {controls}/></style:default-style></office:styles>"
                        ),
                    ),
                    "content-auto" => replace(&mut parts, "content.xml", "</office:automatic-styles>", &format!("{named}</office:automatic-styles>")),
                    "styles-auto" => replace(&mut parts, "styles.xml", "</office:automatic-styles>", &format!("{named}</office:automatic-styles>")),
                    _ => panic!("unknown independent fixture scope"),
                }
                preserved(&parts, true);
            }
        }
    }
}

#[test]
fn independent_channel_mirror_mode_and_percentage_lexical_rejections() {
    for key in ["draw:blue", "draw:green", "draw:red"] {
        for value in ["1%", "-1%", "-0%", "+0%", "00%", "0.0%", "0", " 0%", "0% ", "0% 0%"] {
            refused(&fixture("1.3", &format!("{key}='{value}'"), true));
        }
    }
    for value in ["horizontal", "vertical", "horizontal vertical", "horizontal-on-even", "horizontal-on-odd", "NONE", "none ", " none", "none none"] {
        refused(&fixture("1.3", &format!("style:mirror='{value}'"), true));
    }
    for value in ["watermark", "grayscale", "Standard", "MONO", "greyscale ", " standard", "standard mono", ""] {
        refused(&fixture("1.3", &format!("draw:color-mode='{value}'"), true));
    }
    for key in ["draw:image-opacity", "draw:luminance", "draw:contrast"] {
        for value in [
            "",
            "100.001%",
            "101%",
            "000%",
            "0100%",
            "+0%",
            "+1%",
            "--1%",
            "1e1%",
            "NaN%",
            "inf%",
            "-inf%",
            ".5%",
            "1.%",
            "1",
            "1pt",
            "1%%",
            " 1%",
            "1% ",
            "1%&#9;",
            "1% 2%",
            "1,5%",
            "１２%",
            "https://example.invalid/",
            "Pictures/review.png",
        ] {
            refused(&fixture("1.3", &format!("{key}='{value}'"), true));
        }
        let too_long = format!("0.{}%", "0".repeat(62));
        assert_eq!(too_long.len(), 65);
        refused(&fixture("1.3", &format!("{key}='{too_long}'"), true));
    }
    for version in ["1.2", "1.3"] {
        for key in ["draw:image-opacity", "draw:luminance"] {
            for value in ["-0%", "-1%", "-100%"] {
                refused(&fixture(version, &format!("{key}='{value}'"), true));
            }
        }
    }
}

#[test]
fn independent_namespace_bag_family_direct_owner_and_leaf_rejections() {
    for properties in [
        "image-opacity='50%'",
        "xmlns:draw='urn:review:foreign' draw:image-opacity='50%'",
        "xmlns:x='urn:review:foreign' x:image-opacity='50%'",
        "xmlns:style='urn:review:foreign' style:mirror='none'",
        "xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' draw:image-opacity='50%' d:image-opacity='50%'",
    ] {
        refused(&fixture("1.3", properties, true));
    }
    for (old, new) in [
        ("style:family='graphic'", "style:family='paragraph'"),
        ("style:family='graphic'", "style:family='text'"),
        ("style:graphic-properties", "style:paragraph-properties"),
        ("style:graphic-properties", "style:text-properties"),
        ("style:graphic-properties", "style:drawing-page-properties"),
        ("<draw:frame xml:id='picture'", "<draw:frame draw:image-opacity='50%' xml:id='picture'"),
        ("<draw:image xlink:href=", "<draw:image draw:image-opacity='50%' xlink:href="),
    ] {
        let mut parts = fixture("1.3", "draw:image-opacity='50%'", true);
        replace(&mut parts, "content.xml", old, new);
        refused(&parts);
    }
    for leaf in [
        "<style:graphic-properties draw:image-opacity='50%'><draw:image xlink:href='Pictures/review.png'/></style:graphic-properties>",
        "<style:graphic-properties draw:image-opacity='50%'>nonempty</style:graphic-properties>",
        "<style:graphic-properties draw:image-opacity='50%'/><style:graphic-properties draw:image-opacity='50%'/>",
    ] {
        let mut parts = fixture("1.3", "draw:image-opacity='50%'", true);
        replace(&mut parts, "content.xml", "<style:graphic-properties draw:image-opacity='50%'/>", leaf);
        refused(&parts);
    }
    for (family, bag) in
        [("paragraph", "style:paragraph-properties"), ("text", "style:text-properties"), ("drawing-page", "style:drawing-page-properties")]
    {
        let mut parts = fixture("1.3", "draw:image-opacity='50%'", true);
        replace(&mut parts, "content.xml", "style:family='graphic'", &format!("style:family='{family}'"));
        replace(&mut parts, "content.xml", "style:graphic-properties", bag);
        refused(&parts);
    }
    let mut page = fixture("1.3", "", true);
    replace(&mut page, "styles.xml", "fo:page-width='400pt'", "draw:image-opacity='50%' fo:page-width='400pt'");
    refused(&page);
    let mut metadata = fixture("1.3", "", true);
    replace(
        &mut metadata,
        "meta.xml",
        "</office:meta>",
        "<style:style style:name='Other' style:family='graphic'><style:graphic-properties draw:image-opacity='50%'/></style:style></office:meta>",
    );
    refused(&metadata);
}

#[test]
fn independent_neutral_controls_do_not_hide_deferred_controls_or_unresolved_unused_dependencies() {
    for extra in ["draw:gamma='NaN%'", "fo:clip='inherit'", "fo:clip='rect(1pt, 0pt, 0pt, 0pt)'", "style:protect='none'", "draw:unknown-control='0%'"]
    {
        refused(&fixture("1.3", &format!("{NEUTRAL} {extra}"), true));
    }
    for unused in [
        "<style:style style:name='Unused' style:family='graphic'><style:graphic-properties draw:red='1%'/></style:style>",
        "<style:style style:name='Unused' style:family='graphic'><style:graphic-properties draw:fill='bitmap' draw:fill-image-name='Absent'/></style:style>",
        "<style:style style:name='Unused' style:family='paragraph'><style:text-properties style:font-name='Absent'/></style:style>",
        "<style:style style:name='Unused' style:family='graphic' style:parent-style-name='Absent'><style:graphic-properties draw:blue='0%'/></style:style>",
        "<style:style style:name='Image' style:family='graphic'><style:graphic-properties draw:blue='0%'/></style:style>",
    ] {
        let mut parts = fixture("1.3", NEUTRAL, true);
        replace(&mut parts, "content.xml", "</office:automatic-styles>", &format!("{unused}</office:automatic-styles>"));
        refused(&parts);
    }
    let mut cycle = fixture("1.3", NEUTRAL, true);
    replace(
        &mut cycle,
        "styles.xml",
        "<office:styles/>",
        "<office:styles><style:style style:name='Cycle' style:family='graphic' style:parent-style-name='Cycle'><style:graphic-properties draw:blue='0%'/></style:style></office:styles>",
    );
    refused(&cycle);
}

#[test]
fn independent_image_resource_and_active_content_closure_stays_mandatory() {
    for path in [
        "Pictures/missing.png",
        "Thumbnails/thumbnail.png",
        "../Pictures/review.png",
        "Pictures/../review.png",
        "Pictures//review.png",
        "Pictures/review.png#fragment",
        "Pictures/review.png?query",
        "Pictures/%72eview.png",
        "https://example.invalid/image.png",
        "file:///image.png",
    ] {
        let mut parts = fixture("1.3", NEUTRAL, true);
        replace(&mut parts, "content.xml", "xlink:href='Pictures/review.png'", &format!("xlink:href='{path}'"));
        refused(&parts);
    }
    let mut mime = fixture("1.3", NEUTRAL, true);
    replace(
        &mut mime,
        MANIFEST,
        "manifest:full-path='Pictures/review.png' manifest:media-type='image/png'",
        "manifest:full-path='Pictures/review.png' manifest:media-type='image/jpeg'",
    );
    refused(&mime);
    let mut corrupt = fixture("1.3", NEUTRAL, true);
    corrupt.insert(RASTER.into(), b"contributor-original non-raster".to_vec());
    refused(&corrupt);
    for replacement in ["<office:scripts><office:script/></office:scripts>", "<office:scripts xlink:href='Pictures/review.png'/>"] {
        let mut parts = fixture("1.3", NEUTRAL, true);
        replace(&mut parts, "content.xml", "<office:scripts/>", replacement);
        refused(&parts);
    }
    let mut child = fixture("1.3", NEUTRAL, true);
    replace(
        &mut child,
        "content.xml",
        "xlink:actuate='onLoad'/>",
        "xlink:actuate='onLoad'><draw:image xlink:href='Pictures/review.png'/></draw:image>",
    );
    refused(&child);
}

#[test]
fn independent_supported_text_edits_never_authorize_picture_or_media_changes() {
    for cache in [false, true] {
        for mutation in 0..9 {
            let source = package(&fixture("1.3", &format!("{NEUTRAL} draw:image-opacity='75%' draw:contrast='-20%'"), cache));
            let mut current = import(&source).unwrap();
            edit(&mut current, "FirstRevised", "SecondRevised");
            let shape = &mut Arc::make_mut(&mut current.slides[0]).shapes[0];
            let ShapeKind::Picture { fill } = &mut shape.kind else { panic!("actual picture required") };
            match mutation {
                0 => fill.crop[0] = 0.01,
                1 => fill.alpha = Some(0.5),
                2 => fill.adjust.brightness = 0.1,
                3 => fill.adjust.contrast = 0.3,
                4 => fill.adjust.grayscale = true,
                5 => fill.adjust.bilevel = Some(0.5),
                6 => shape.xfrm.as_mut().unwrap().flip_h = true,
                7 => shape.xfrm.as_mut().unwrap().w += 1.0,
                _ => current.media.iter_mut().find(|media| media.id == fill.media).unwrap().data = Arc::new(vec![1, 2, 3]),
            }
            let frozen = current.clone();
            let error = export(&current).unwrap_err().to_string();
            assert!(error.contains("changes outside plain slide text"), "native boundary mutation {mutation}: {error}");
            assert_eq!(current, frozen);
            assert_eq!(current.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
        }
    }
}

#[test]
fn independent_no_cache_path_keeps_existing_scalar_policy_and_audit_warnings() {
    // No preview/statistic means no closed cache profile. This intentionally tests the
    // pre-existing source-retention route, not new scalar rejection in an unused profile.
    preserved(&fixture("1.3", "draw:red='3%' draw:gamma='75%' style:mirror='horizontal'", false), false);
    for cache in [false, true] {
        let mut parts = fixture("1.3", NEUTRAL, cache);
        replace(
            &mut parts,
            "content.xml",
            "<style:paragraph-properties fo:keep-with-next='always'/>",
            "<style:paragraph-properties fo:keep-with-next='always' style:line-spacing='1pt'/>",
        );
        let source = package(&parts);
        let mut current = import(&source).unwrap();
        edit(&mut current, "FirstRevised", "SecondRevised");
        let frozen = current.clone();
        let error = export(&current).unwrap_err().to_string();
        assert!(
            error.contains("source audit contains non-preservable semantics")
                || error.contains("source has unsupported content beyond passive text properties"),
            "image controls must not exempt independent line-height warnings/audit: {error}"
        );
        assert_eq!(current, frozen);
    }
}
