//! Independent W13 identity-only source-retention regressions, using original XML and pixels.
//! Liberation Sans refers to the existing open font; no external asset is included.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{Presentation, ShapeKind, style::PictureFill};
use deckcraft_odp::{MIME, export, import};
use std::{collections::BTreeMap, io::Cursor, io::Read, io::Write, sync::Arc};

type Members = BTreeMap<String, Vec<u8>>;
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const RASTER: &str = "Pictures/independent-clip.png";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:xlink='http://www.w3.org/1999/xlink' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";
const FONT: &str =
    "<office:font-face-decls><style:font-face style:name='ReviewOpenFont' svg:font-family='Liberation Sans'/></office:font-face-decls>";

fn change(files: &mut Members, name: &str, old: &str, new: &str) {
    let original = String::from_utf8(files[name].clone()).unwrap();
    assert!(original.contains(old), "missing original test mutation {old}");
    files.insert(name.into(), original.replace(old, new).into_bytes());
}

fn fixture(version: &str, attribute: &str, cache: bool) -> Members {
    // The live image inherits the clip from a common graphic parent. The automatic child
    // supplies opacity, proving actual style application independently of source retention.
    let content = format!(
        "<office:document-content {NS} office:version='{version}'>{FONT}<office:automatic-styles><style:style style:name='LocalImage' style:family='graphic' style:parent-style-name='ClippingBase'><style:graphic-properties draw:image-opacity='75%'/></style:style><style:style style:name='ReviewedText' style:family='paragraph'><style:paragraph-properties fo:keep-with-next='always'/><style:text-properties style:font-name='ReviewOpenFont' fo:font-size='14pt'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Clip review' draw:master-page-name='ReviewMaster' xml:id='ReviewPage'><draw:frame xml:id='ReviewImage' draw:style-name='LocalImage' svg:x='8pt' svg:y='9pt' svg:width='12pt' svg:height='8pt'><draw:image xlink:href='{RASTER}' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame><!--unmodified object gap Ω--><draw:frame xml:id='ReviewText' svg:x='30pt' svg:y='20pt' svg:width='200pt' svg:height='70pt'><draw:text-box><text:p text:style-name='ReviewedText'>Review alpha</text:p><!--unmodified paragraph gap--><text:p text:style-name='ReviewedText'>Review beta</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'>{FONT}<office:styles><!--unmodified common gap--><style:style style:name='ClippingBase' style:family='graphic'><style:graphic-properties {attribute}/></style:style></office:styles><office:automatic-styles><style:page-layout style:name='ReviewLayout'><style:page-layout-properties fo:page-width='360pt' fo:page-height='200pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='ReviewMaster' style:page-layout-name='ReviewLayout'/></office:master-styles></office:document-styles>"
    );
    // Statistics alone selects the cache profile; there is deliberately no thumbnail.
    let statistic = if cache { "<meta:document-statistic meta:word-count='4' meta:page-count='1'/>" } else { "" };
    let png = deckcraft_render::Image { width: 3, height: 1, pixels: vec![19, 151, 103, 255, 193, 31, 137, 255, 67, 113, 223, 255] }.to_png();
    let mut files = Members::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        (
            "meta.xml".into(),
            format!("<office:document-meta {NS} office:version='{version}'><office:meta><!--statistics gap-->{statistic}</office:meta></office:document-meta>").into_bytes(),
        ),
        ("settings.xml".into(), format!("<office:document-settings {NS} office:version='{version}'/>").into_bytes()),
        (RASTER.into(), png),
    ]);
    let mut manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/><!--manifest survivor-->\r\n"
    );
    for name in files.keys().filter(|name| name.as_str() != "mimetype") {
        let mime = if name.ends_with(".png") { "image/png" } else { "text/xml" };
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path='{name}' manifest:media-type='{mime}'/>\n"));
    }
    manifest.push_str("</manifest:manifest>");
    files.insert("META-INF/manifest.xml".into(), manifest.into_bytes());
    files
}

fn pack(files: &Members) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in std::iter::once("mimetype").chain(files.keys().map(String::as_str).filter(|name| *name != "mimetype")) {
        writer.start_file(name, options).unwrap();
        writer.write_all(&files[name]).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn unpack(bytes: &[u8]) -> Members {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = Members::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut payload = Vec::new();
        member.read_to_end(&mut payload).unwrap();
        assert!(files.insert(member.name().into(), payload).is_none());
    }
    files
}

fn exact_members(actual: &Members, expected: &Members) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    for (name, wanted) in expected {
        let found = &actual[name];
        assert!(
            found == wanted,
            "member {name}: actual {} bytes, expected {} bytes, first difference {:?}",
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
    let ShapeKind::Picture { fill } = &p.slides[0].shapes[0].kind else { panic!("original live raster required") };
    fill
}

fn pixels(p: &Presentation) -> Vec<u8> {
    let rendered = deckcraft_render::render_shape(p, &p.slides[0], p.slides[0].shapes[0].id, 1.0).unwrap();
    assert!(rendered.pixels.chunks_exact(4).any(|rgba| rgba[3] > 0));
    rendered.pixels
}

fn normalize(mut p: Presentation) -> Presentation {
    for media in p.media.iter_mut().filter(|m| m.content_type == SOURCE) {
        media.data = Arc::new(Vec::new());
    }
    for slide in &mut p.slides {
        let slide = Arc::make_mut(slide);
        slide.tags.retain(|(key, _)| key != "odp:source-range");
        for shape in &mut slide.shapes {
            shape.tags.retain(|(key, _)| key != "odp:source-range");
        }
    }
    p
}

fn retained(files: &Members, cache: bool) {
    let input = files.clone();
    let original = pack(files);
    let mut current = import(&original).expect("original independent synthetic source");
    assert!(current.tags.iter().any(|(key, _)| key == "odp:semantic-audit"));
    assert_eq!(picture(&current).crop, [0.0; 4]);
    assert_eq!(picture(&current).alpha, Some(0.75));
    let raster_pixels = pixels(&current);
    let unchanged_error = export(&current).expect_err("requires a local edit").to_string();
    assert!(unchanged_error.ends_with("no supported text edits; use original recovery"), "{unchanged_error}");
    edit(&mut current, "ReviewChanged", "ReviewRevised");
    let saved = deckcraft_format::save(&current).unwrap();
    assert_eq!(deckcraft_format::load(&saved).unwrap(), current);
    let before = current.clone();
    let output = export(&current).expect("identity source clipping retains local text edits");
    assert_eq!(current, before);
    let mut expected = files.clone();
    change(&mut expected, "content.xml", ">Review alpha<", ">ReviewChanged<");
    change(&mut expected, "content.xml", ">Review beta<", ">ReviewRevised<");
    if cache {
        change(&mut expected, "meta.xml", " meta:word-count='4'", "");
    }
    exact_members(&unpack(&output), &expected);
    let mut reopened = import(&output).unwrap();
    assert_eq!(pixels(&reopened), raster_pixels);
    assert_eq!(picture(&reopened).crop, [0.0; 4]);
    assert_eq!(normalize(current.clone()), normalize(reopened.clone()));
    assert_eq!(current.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), original);
    assert_eq!(reopened.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), output);
    // A fresh invocation must re-prove the inherited retained style after cache shrinkage.
    edit(&mut reopened, "ReviewFinal", "ReviewLast");
    change(&mut expected, "content.xml", ">ReviewChanged<", ">ReviewFinal<");
    change(&mut expected, "content.xml", ">ReviewRevised<", ">ReviewLast<");
    let second = export(&reopened).unwrap();
    exact_members(&unpack(&second), &expected);
    assert_eq!(pixels(&import(&second).unwrap()), raster_pixels);
    assert_eq!(*files, input, "source members remain immutable");
    assert_eq!(current.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), original);
}

fn package_refused(files: &Members) {
    let source = pack(files);
    let mut desired = import(&source).unwrap_or_else(|_| import(&pack(&fixture("1.3", "fo:clip='auto'", true))).unwrap());
    edit(&mut desired, "Review changed", "Review revised");
    // Even XML rejected at import must be independently refused by the immutable package proof.
    desired.media.iter_mut().find(|m| m.content_type == SOURCE).unwrap().data = Arc::new(source.clone());
    let before = desired.clone();
    let error = match export(&desired) {
        Ok(output) => panic!("hostile cache source unexpectedly exported {} bytes", output.len()),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("source-preserving text export refused:")
            || error.contains("XML")
            || error.contains("external or unsafe package resource path")
            || error.contains("missing regular master style reference"),
        "{error}"
    );
    for later in [
        "changes outside plain slide text",
        "source has unsupported content beyond passive text properties",
        "source audit contains non-preservable semantics",
    ] {
        assert!(!error.contains(later), "package proof must refuse first: {error}");
    }
    assert_eq!(desired, before);
    assert_eq!(desired.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), source);
}

#[test]
fn independently_inherited_zero_permutations_survive_statistics_only_and_no_cache_routes() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for value in ["auto", "rect(0pc,0in,0mm,0cm)", "rect(  0pt , 0pc,0in ,0mm  )"] {
                retained(&fixture(version, &format!("fo:clip='{value}'"), cache), cache);
            }
        }
    }
}

#[test]
fn independently_decoded_length_boundary_aliases_and_xml_space_normalization_are_exact() {
    for version in ["1.2", "1.3"] {
        let exact = format!("rect({}0pt,0pt,0pt,0pt)", "&#32;".repeat(107));
        assert!(exact.len() > 128, "the bound applies to decoded bytes");
        retained(&fixture(version, &format!("fo:clip='{exact}'"), true), true);
        let over = format!("rect({}0pt,0pt,0pt,0pt)", "&#32;".repeat(108));
        package_refused(&fixture(version, &format!("fo:clip='{over}'"), true));
        retained(
            &fixture(
                version,
                "xmlns:other='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' other:clip=\"rect(&#x30;pt,0mm,0cm,0p&#99;)\"",
                true,
            ),
            true,
        );
        // XML normalizes literal attribute whitespace to ASCII spaces; character references
        // preserve tab/newline values, which are intentionally outside the decoded grammar.
        retained(&fixture(version, "fo:clip='rect(\t0pt,\n0mm,\r0cm,0pc)'", true), true);
        for whitespace in ["&#9;", "&#10;", "&#13;", "&#160;", "&#x2003;", "&#x200B;"] {
            package_refused(&fixture(version, &format!("fo:clip='rect({whitespace}0pt,0mm,0cm,0pc)'"), true));
        }
    }
}

#[test]
fn independently_each_edge_refuses_noncanonical_and_nonidentity_operands_on_cache_routes() {
    for version in ["1.2", "1.3"] {
        for edge in 0..4 {
            for bad in ["auto", "1mm", "-1pt", "-0pt", "+0pt", "0.00cm", "00in", "0e0pt", "0px", "0em", "0%", "0", "NaNpt", "0PT", "calc(0pt)"] {
                let mut operands = ["0pt"; 4];
                operands[edge] = bad;
                package_refused(&fixture(version, &format!("fo:clip='rect({})'", operands.join(",")), true));
            }
        }
        for bad in [
            "auto ",
            " auto",
            "AUTO",
            "rect(0pt,0pt,0pt)",
            "rect(0pt,0pt,0pt,0pt,)",
            "rect(0pt,0pt,0pt,0pt,0pt)",
            "rect(0pt 0pt 0pt 0pt)",
            "rect(0pt,,0pt,0pt)",
            "rect(0pt;0pt;0pt;0pt)",
            "rect(0pt,0pt,0pt,0pt)auto",
            "rect((0pt,0pt,0pt,0pt))",
        ] {
            package_refused(&fixture(version, &format!("fo:clip='{bad}'"), true));
        }
    }
}

#[test]
fn independently_identity_does_not_admit_other_expanded_names_bags_or_direct_owners() {
    for attributes in [
        "clip='auto'",
        "xmlns:fo='urn:deckcraft:independent:foreign' fo:clip='auto'",
        "xmlns:q='urn:deckcraft:independent:foreign' q:clip='auto'",
        "xmlns:q='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' fo:clip='auto' q:clip='auto'",
    ] {
        package_refused(&fixture("1.3", attributes, true));
    }
    for family in ["paragraph", "text", "drawing-page", "table-cell"] {
        let mut files = fixture("1.3", "fo:clip='auto'", true);
        change(
            &mut files,
            "styles.xml",
            "style:name='ClippingBase' style:family='graphic'",
            &format!("style:name='ClippingBase' style:family='{family}'"),
        );
        package_refused(&files);
    }
    for bag in ["style:text-properties", "style:paragraph-properties", "style:drawing-page-properties", "style:page-layout-properties"] {
        let mut files = fixture("1.3", "fo:clip='auto'", true);
        change(&mut files, "styles.xml", "<style:graphic-properties fo:clip='auto'/>", &format!("<{bag} fo:clip='auto'/>"));
        package_refused(&files);
    }
    for (part, owner) in [
        ("content.xml", "xml:id='ReviewImage'"),
        ("content.xml", "xlink:type='simple'"),
        ("styles.xml", "style:name='ClippingBase'"),
        ("styles.xml", "style:name='ReviewMaster'"),
    ] {
        let mut files = fixture("1.3", "fo:clip='auto'", true);
        change(&mut files, part, owner, &format!("{owner} fo:clip='auto'"));
        package_refused(&files);
    }
}

#[test]
fn independently_unused_clip_styles_cannot_hide_effects_cycles_duplicates_or_active_descendants() {
    for extra in
        ["draw:gamma='NaN%'", "draw:gamma='auto'", "draw:shadow='hidden'", "xlink:href='Pictures/independent-clip.png'", "xml:base='file:///'"]
    {
        let mut files = fixture("1.3", "fo:clip='auto'", true);
        let unused = format!(
            "<style:style style:name='UnusedClip' style:family='presentation'><style:graphic-properties fo:clip='auto' {extra}/></style:style>"
        );
        change(&mut files, "styles.xml", "</office:styles>", &format!("{unused}</office:styles>"));
        package_refused(&files);
    }
    for extra in [
        "<style:style style:name='UnusedClip' style:family='graphic' style:parent-style-name='Absent'><style:graphic-properties fo:clip='auto'/></style:style>",
        "<style:style style:name='UnusedClip' style:family='graphic' style:parent-style-name='UnusedClip'><style:graphic-properties fo:clip='auto'/></style:style>",
        "<style:style style:name='ClippingBase' style:family='graphic'><style:graphic-properties fo:clip='auto'/></style:style>",
        "<style:style style:name='UnusedClip' style:family='graphic'><style:graphic-properties fo:clip='auto'><office:scripts/></style:graphic-properties></style:style>",
    ] {
        let mut files = fixture("1.3", "fo:clip='auto'", true);
        change(&mut files, "styles.xml", "</office:styles>", &format!("{extra}</office:styles>"));
        package_refused(&files);
    }
    for (part, location, inserted) in [
        ("content.xml", "<office:presentation>", "<office:forms/>"),
        ("meta.xml", "<office:meta>", "<style:graphic-properties fo:clip='auto'/>"),
        ("settings.xml", "</office:document-settings>", "<style:graphic-properties fo:clip='auto'/>"),
    ] {
        let mut files = fixture("1.3", "fo:clip='auto'", true);
        if part == "settings.xml" {
            let original = String::from_utf8(files[part].clone()).unwrap();
            files.insert(part.into(), original.replace("/>", &format!(">{inserted}</office:document-settings>")).into_bytes());
        } else {
            change(&mut files, part, location, &format!("{location}{inserted}"));
        }
        package_refused(&files);
    }
}

#[test]
fn independently_identity_keeps_scope_typed_reference_and_resource_closure_checks() {
    for bad in [
        "../Pictures/independent-clip.png",
        "Pictures/absent.png",
        "Pictures/independent-clip.png#image",
        "Pictures/%69ndependent-clip.png",
        "file:///clip.png",
        "https://example.invalid/clip.png",
    ] {
        let mut files = fixture("1.3", "fo:clip='auto'", true);
        change(&mut files, "content.xml", &format!("xlink:href='{RASTER}'"), &format!("xlink:href='{bad}'"));
        package_refused(&files);
    }
    let mut files = fixture("1.3", "fo:clip='auto'", true);
    files.remove(RASTER);
    package_refused(&files);
    let mut files = fixture("1.3", "fo:clip='auto'", true);
    files.insert(RASTER.into(), b"original synthetic non-raster".to_vec());
    package_refused(&files);
    let mut files = fixture("1.3", "fo:clip='auto'", true);
    change(&mut files, "META-INF/manifest.xml", "manifest:media-type='image/png'", "manifest:media-type='image/jpeg'");
    package_refused(&files);
    let mut files = fixture("1.3", "fo:clip='auto'", true);
    // A name defined only in content-auto cannot satisfy a common parent-style reference.
    change(&mut files, "content.xml", "style:parent-style-name='ClippingBase'", "style:parent-style-name='LocalImage'");
    package_refused(&files);
    let mut files = fixture("1.3", "fo:clip='auto'", true);
    change(&mut files, "styles.xml", "fo:clip='auto'", "fo:clip='auto' draw:fill='bitmap' draw:fill-image-name='AbsentFill'");
    package_refused(&files);
}

#[test]
fn independently_text_edits_never_authorize_live_picture_or_media_changes() {
    for cache in [false, true] {
        for mutation in ["crop", "opacity", "brightness", "contrast", "grayscale", "geometry", "flip", "missing-media", "media-bytes"] {
            let source = pack(&fixture("1.3", "fo:clip='rect(0pt,0mm,0in,0pc)'", cache));
            let mut desired = import(&source).unwrap();
            edit(&mut desired, "Review changed", "Review revised");
            if mutation == "media-bytes" {
                let id = picture(&desired).media;
                desired.media.iter_mut().find(|m| m.id == id).unwrap().data =
                    Arc::new(deckcraft_render::Image { width: 1, height: 1, pixels: vec![173, 41, 89, 255] }.to_png());
            } else {
                let shape = &mut Arc::make_mut(&mut desired.slides[0]).shapes[0];
                if mutation == "geometry" {
                    shape.xfrm.as_mut().unwrap().w += 2.0;
                } else if mutation == "flip" {
                    shape.xfrm.as_mut().unwrap().flip_v = true;
                } else {
                    let ShapeKind::Picture { fill } = &mut shape.kind else { panic!("live original picture") };
                    match mutation {
                        "crop" => fill.crop[2] = 0.125,
                        "opacity" => fill.alpha = Some(0.5),
                        "brightness" => fill.adjust.brightness = 0.1,
                        "contrast" => fill.adjust.contrast = -0.1,
                        "grayscale" => fill.adjust.grayscale = true,
                        "missing-media" => fill.media = deckcraft_model::MediaId(u32::MAX),
                        _ => panic!("unknown original mutation"),
                    }
                }
            }
            let before = desired.clone();
            let error = export(&desired).expect_err("only local text edits are authorized by this path").to_string();
            assert!(error.ends_with("changes outside plain slide text are unsupported"), "{mutation}: {error}");
            assert_eq!(desired, before);
            assert_eq!(desired.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), source);
        }
    }
}
