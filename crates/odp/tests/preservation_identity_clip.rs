//! Contributor-original identity-clip retention fixtures; no nonzero crop or gamma fidelity claim.
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

fn base_fixture(version: &str, preview: bool, statistics: bool, scripts: &str) -> BTreeMap<String, Vec<u8>> {
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
    let text = Arc::make_mut(&mut presentation.slides[0]).shapes.iter_mut().find_map(|shape| shape.text.as_mut()).unwrap();
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
    for master in &mut presentation.masters {
        for shape in &mut Arc::make_mut(master).shapes {
            shape.tags.retain(|(key, _)| key != "odp:source-range");
        }
    }
    presentation
}
fn preserved(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    let original_files = files.clone();
    let source = zip(files);
    let mut desired = import(&source).expect("public original fixture imports");
    assert!(
        desired.tags.iter().any(|(key, _)| key == "odp:semantic-audit"),
        "the independent local text frame must trigger the existing audit route"
    );
    let before_pixels = picture_pixels(&desired);
    assert!(!before_pixels.is_empty());
    for fill in pictures(&desired) {
        assert_eq!(fill.crop, [0.0; 4]);
        assert_eq!(fill.alpha, Some(0.4), "the actual raster fixture must use explicit mapped opacity");
    }
    assert!(export(&desired).is_err(), "an actual edit is required");
    edit(&mut desired);
    let native = deckcraft_format::load(&deckcraft_format::save(&desired).unwrap()).unwrap();
    assert_eq!(native, desired, "native save/load must retain source bytes, picture and supported text edits");
    desired = native;
    let before_export = desired.clone();
    let output = export(&desired).expect("identity clipping permits supported text edits");
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
    assert_eq!(*files, original_files, "fixture input members must remain unchanged");
    assert_eq!(after["Pictures/keep.png"], files["Pictures/keep.png"]);
    assert_eq!(after["styles.xml"], files["styles.xml"], "style part must retain exact bytes");
    let mut reopened = import(&output).expect("output reimports");
    assert_eq!(picture_pixels(&reopened), before_pixels, "independent text edits must not change raster pixels");
    for fill in pictures(&reopened) {
        assert_eq!(fill.crop, [0.0; 4]);
    }
    assert_eq!(reopened.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), output);
    assert_eq!(without_source_ranges(desired), without_source_ranges(reopened.clone()), "only source bytes/ranges may change");
    Arc::make_mut(&mut reopened.slides[0]).shapes.iter_mut().find_map(|shape| shape.text.as_mut()).unwrap().paragraphs[0].runs[0].text =
        "Again".into();
    let next_output = export(&reopened).expect("fresh output profile still permits a second edit");
    let mut next_expected = after.clone();
    replace(&mut next_expected, "content.xml", "Edited", "Again");
    assert_eq!(parts(&next_output), next_expected, "fresh cache proof must not change additional payloads");
    assert_eq!(picture_pixels(&import(&next_output).unwrap()), before_pixels);
    assert_eq!(*files, original_files);
    assert_eq!(reopened.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), output);
    after
}
fn refused(files: &BTreeMap<String, Vec<u8>>) {
    refused_reason(files, None);
}
fn refused_reason(files: &BTreeMap<String, Vec<u8>>, expected_reason: Option<&str>) {
    let source = zip(files);
    let mut presentation =
        import(&source).unwrap_or_else(|_| import(&zip(&fixture("1.3", true, true, "content-auto", "graphic", "fo:clip='auto'"))).unwrap());
    edit(&mut presentation);
    // Import-rejected hostile XML must also hit the export package proof, never a warning bypass.
    presentation.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(source);
    let before = presentation.clone();
    let error = match export(&presentation) {
        Err(error) => error.to_string(),
        Ok(output) => panic!("unproved container/dependency unexpectedly exported {} bytes", output.len()),
    };
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
    if let Ok(directory) = std::env::var("DECKCRAFT_IDENTITY_CLIP_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (stage, files) in [("source", before), ("output", after)] {
            for name in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                let filename = format!("{version}-{stage}-{}", name.replace('/', "-"));
                std::fs::write(std::path::Path::new(&directory).join(filename), &files[name]).unwrap();
            }
        }
    }
}

fn fixture(version: &str, preview: bool, statistics: bool, owner: &str, family: &str, attributes: &str) -> BTreeMap<String, Vec<u8>> {
    let mut files = base_fixture(version, preview, statistics, "<office:scripts/>");
    // Active lanes use mapped opacity as a reachability marker. The separately guarded
    // styles-auto presentation definition remains unused beside a marked graphic picture.
    let bag = format!("<style:graphic-properties draw:image-opacity='40%' {attributes}/>");
    let named = format!("<style:style style:name='IdentityClip' style:family='{family}'>{bag}</style:style>");
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
        _ => panic!("unknown original fixture owner"),
    }
    let reference = if family == "presentation" {
        // Presentation style ownership on regular/master pictures is independently guarded.
        // Exercise retained definitions beside a live graphic picture, without claiming
        // an active presentation-style mapping.
        let marker =
            "<style:style style:name='RasterMarker' style:family='graphic'><style:graphic-properties draw:image-opacity='40%'/></style:style>";
        if String::from_utf8_lossy(&files["styles.xml"]).contains("<office:styles/>") {
            replace(&mut files, "styles.xml", "<office:styles/>", &format!("<office:styles>{marker}</office:styles>"));
        } else {
            replace(&mut files, "styles.xml", "<office:styles>", &format!("<office:styles>{marker}"));
        }
        " draw:style-name='RasterMarker'".into()
    } else if owner == "default" {
        String::new()
    } else {
        " draw:style-name='IdentityClip'".into()
    };
    let picture = format!(
        "<draw:frame xml:id='picture' svg:x='300pt' svg:y='20pt' svg:width='80pt' svg:height='80pt'{reference}><draw:image xlink:href='Pictures/keep.png' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame>"
    );
    if owner == "styles-auto" {
        // Graphic styles-auto reaches a live master picture. Presentation styles-auto
        // remains a retained definition beside that picture, as documented above.
        replace(
            &mut files,
            "styles.xml",
            "<style:master-page style:name='M' style:page-layout-name='PM'/>",
            &format!("<style:master-page style:name='M' style:page-layout-name='PM'>{picture}</style:master-page>"),
        );
    } else {
        replace(&mut files, "content.xml", "</draw:page>", &format!("{picture}</draw:page>"));
    }
    files
}
fn pictures(p: &Presentation) -> Vec<&PictureFill> {
    p.masters
        .iter()
        .flat_map(|master| &master.shapes)
        .chain(p.slides.iter().flat_map(|slide| &slide.shapes))
        .filter_map(|shape| match &shape.kind {
            ShapeKind::Picture { fill } => Some(fill),
            _ => None,
        })
        .collect()
}
fn picture_pixels(p: &Presentation) -> Vec<Vec<u8>> {
    // Isolate each real imported picture from independently edited local text. A temporary
    // slide also lets the same oracle render the live styles-auto master picture.
    p.masters
        .iter()
        .flat_map(|master| &master.shapes)
        .chain(p.slides.iter().flat_map(|slide| &slide.shapes))
        .filter(|shape| matches!(shape.kind, ShapeKind::Picture { .. }))
        .map(|shape| {
            let mut slide = (*p.slides[0]).clone();
            slide.shapes = vec![shape.clone()];
            let image = deckcraft_render::render_shape(p, &slide, shape.id, 0.5).unwrap();
            assert!(image.width > 0 && image.height > 0);
            assert!(image.pixels.chunks_exact(4).any(|pixel| pixel[3] != 0), "render must exercise the original raster");
            image.pixels
        })
        .collect()
}

#[test]
fn identity_and_absence_preserve_actual_pictures_all_versions_scopes_families_and_cache_paths() {
    for version in ["1.2", "1.3"] {
        for (preview, statistics) in [(false, false), (true, false), (false, true), (true, true)] {
            for owner in ["common", "default", "content-auto", "styles-auto"] {
                for family in ["graphic", "presentation"] {
                    for value in [None, Some("auto"), Some("rect(0pt,0pt,0pt,0pt)"), Some("rect( 0cm , 0mm, 0in, 0pc )")] {
                        let attr = value.map_or_else(String::new, |value| format!("fo:clip='{value}'"));
                        preserved(&fixture(version, preview, statistics, owner, family, &attr));
                    }
                }
            }
        }
    }
}

#[test]
fn each_zero_unit_at_each_edge_is_an_identity_and_the_schema_emission_has_twenty_parts() {
    for version in ["1.2", "1.3"] {
        for unit in ["cm", "mm", "in", "pt", "pc"] {
            for edge in 0..4 {
                let mut lengths = ["0pt".to_string(), "0pt".to_string(), "0pt".to_string(), "0pt".to_string()];
                lengths[edge] = format!("0{unit}");
                let attr = format!("fo:clip='rect({})'", lengths.join(", "));
                preserved(&fixture(version, true, true, "content-auto", "graphic", &attr));
            }
        }
        let files = fixture(version, true, true, "content-auto", "graphic", "fo:clip='rect( 0cm , 0mm, 0in, 0pc )'");
        let after = preserved(&files);
        emit(version, &files, &after);
    }
}

#[test]
fn zero_clip_and_whole_auto_have_identical_native_models_and_pixels_to_absence() {
    for version in ["1.2", "1.3"] {
        for owner in ["common", "default", "content-auto", "styles-auto"] {
            for family in ["graphic", "presentation"] {
                let absent = import(&zip(&fixture(version, true, true, owner, family, ""))).unwrap();
                for value in ["auto", "rect(0cm, 0mm, 0in, 0pc)"] {
                    let neutral = import(&zip(&fixture(version, true, true, owner, family, &format!("fo:clip='{value}'")))).unwrap();
                    assert_eq!(pictures(&absent), pictures(&neutral));
                    assert_eq!(picture_pixels(&absent), picture_pixels(&neutral));
                }
            }
        }
    }
}

#[test]
fn aliases_entities_whitespace_comments_and_style_bytes_are_exact_survivors() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            let mut files = fixture(version, cache, cache, "content-auto", "graphic", "fo:clip='auto'");
            replace(
                &mut files,
                "content.xml",
                "<style:graphic-properties draw:image-opacity='40%' fo:clip='auto'/>",
                "<s:graphic-properties xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:f='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' f:clip = 'rect(0p&#116;, 0mm, 0cm, 0pc)' d:image-opacity = \"4&#48;%\"> <!--identity property gap--> \n </s:graphic-properties>",
            );
            preserved(&files);
        }
    }
}

#[test]
fn decoded_value_byte_bound_is_inclusive_and_not_a_rendering_dimension_bound() {
    for version in ["1.2", "1.3"] {
        let exact = format!("rect({}0pt,0pt,0pt,0pt)", " ".repeat(107));
        assert_eq!(exact.len(), 128);
        preserved(&fixture(version, true, true, "common", "graphic", &format!("fo:clip='{exact}'")));
        let over = format!("rect({}0pt,0pt,0pt,0pt)", " ".repeat(108));
        assert_eq!(over.len(), 129);
        refused_reason(&fixture(version, true, true, "common", "graphic", &format!("fo:clip='{over}'")), Some("cache unclassified attribute/value"));
    }
}

#[test]
fn nonzero_signed_fractional_auto_operand_and_malformed_rect_grammars_refuse_on_cache_paths() {
    for version in ["1.2", "1.3"] {
        for value in [
            "",
            "Auto",
            "AUTO",
            " auto",
            "auto ",
            "auto auto",
            "inherit",
            "none",
            "rect()",
            "rect(0pt,0pt,0pt)",
            "rect(0pt,0pt,0pt,0pt,0pt)",
            "rect(0pt,0pt,0pt,0pt,)",
            "rect(,0pt,0pt,0pt)",
            "rect(0pt 0pt 0pt 0pt)",
            "rect(0pt;0pt;0pt;0pt)",
            "rect(0pt,,0pt,0pt)",
            "rect(0pt,0pt,0pt,0pt",
            "rect(0pt,0pt,0pt,0pt))",
            "Rect(0pt,0pt,0pt,0pt)",
            " rect(0pt,0pt,0pt,0pt)",
            "rect(0pt,0pt,0pt,0pt) ",
            "rect(0pt,0pt,0pt,0pt)auto",
            "rect(0pt,0pt,0pt,0pt)rect(0pt,0pt,0pt,0pt)",
            "rect(auto,0pt,0pt,0pt)",
            "rect(0pt,auto,0pt,0pt)",
            "rect(0pt,0pt,auto,0pt)",
            "rect(0pt,0pt,0pt,auto)",
            "rect(auto,auto,auto,auto)",
            "rect(autoautoautoauto)",
            "rect(0pt,0pt,0ptauto)",
            "rect(0pt,0pt,0pt,1pt)",
            "rect(0pt,0pt,0pt,-0pt)",
            "rect(0pt,0pt,0pt,+0pt)",
            "rect(0pt,0pt,0pt,0.0pt)",
            "rect(0pt,0pt,0pt,.0pt)",
            "rect(0pt,0pt,0pt,00pt)",
            "rect(0pt,0pt,0pt,0.pt)",
            "rect(0pt,0pt,0pt,0)",
            "rect(0pt,0pt,0pt,0px)",
            "rect(0pt,0pt,0pt,0em)",
            "rect(0pt,0pt,0pt,0%)",
            "rect(0pt,0pt,0pt,0PT)",
            "rect(0pt,0pt,0pt,0e0pt)",
            "rect(0pt,0pt,0pt,NaNpt)",
            "rect(0pt,0pt,0pt,infpt)",
            "rect(0pt,0pt,0pt,calc(0pt))",
            "url(Pictures/keep.png)",
            "rect(0pt,0pt,0pt,0 pt)",
            "rect(0pt,0pt,0pt,0pt&#9;)",
            "rect(0pt,0pt,0pt,0pt&#10;)",
            "rect(0pt,0pt,0pt,0pt&#13;)",
            "rect(0pt,0pt,0pt,0pt&#160;)",
            "rect(0pt,0pt,0pt,0pt&#8195;)",
        ] {
            refused_reason(
                &fixture(version, true, true, "content-auto", "graphic", &format!("fo:clip='{value}'")),
                Some("cache unclassified attribute/value"),
            );
        }
        for edge in 0..4 {
            let mut lengths = ["0pt"; 4];
            lengths[edge] = "1pt";
            refused_reason(
                &fixture(version, true, true, "common", "graphic", &format!("fo:clip='rect({})'", lengths.join(","))),
                Some("cache unclassified attribute/value"),
            );
        }
    }
}

#[test]
fn foreign_unqualified_rebound_and_duplicate_expanded_clip_identities_refuse() {
    for version in ["1.2", "1.3"] {
        for attr in ["clip='auto'", "xmlns:f='urn:deckcraft:test:foreign' f:clip='auto'", "xmlns:fo='urn:deckcraft:test:foreign' fo:clip='auto'"] {
            refused_reason(&fixture(version, true, true, "common", "graphic", attr), Some("cache XML identity/depth"));
        }
        let attr = "xmlns:a='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' fo:clip='auto' a:clip='auto'";
        refused_reason(&fixture(version, true, true, "content-auto", "graphic", attr), Some("duplicate expanded XML attribute"));
    }
}

#[test]
fn clip_remains_closed_on_other_bags_families_direct_elements_and_parts() {
    for owner in ["common", "default", "content-auto", "styles-auto"] {
        let part = if owner == "content-auto" { "content.xml" } else { "styles.xml" };
        for (family, bag) in
            [("drawing-page", "style:drawing-page-properties"), ("paragraph", "style:paragraph-properties"), ("text", "style:text-properties")]
        {
            let mut files = fixture("1.3", true, true, owner, "graphic", "fo:clip='auto'");
            replace(&mut files, part, "style:family='graphic'", &format!("style:family='{family}'"));
            replace(&mut files, part, "style:graphic-properties", bag);
            replace(&mut files, part, "draw:image-opacity='40%' ", "");
            refused(&files);
        }
    }
    for owner in ["xml:id='picture'", "xlink:type='simple'"] {
        let mut files = fixture("1.3", true, true, "content-auto", "graphic", "");
        replace(&mut files, "content.xml", owner, &format!("{owner} fo:clip='auto'"));
        refused_reason(&files, Some("cache unclassified attribute/value"));
    }
    let mut files = fixture("1.3", true, true, "common", "graphic", "");
    replace(&mut files, "styles.xml", "style:name='IdentityClip'", "style:name='IdentityClip' fo:clip='auto'");
    refused_reason(&files, Some("cache unclassified attribute/value"));
    let mut files = fixture("1.3", true, true, "common", "graphic", "");
    replace(&mut files, "styles.xml", "<style:page-layout-properties ", "<style:page-layout-properties fo:clip='auto' ");
    refused_reason(&files, Some("cache unclassified attribute/value"));
    let mut files = fixture("1.3", true, true, "common", "graphic", "");
    replace(&mut files, "meta.xml", "<office:meta>", "<office:meta><style:graphic-properties fo:clip='auto'/>");
    refused_reason(&files, Some("cache unclassified XML context"));
    let mut files = fixture("1.3", true, true, "common", "graphic", "fo:clip='auto'");
    replace(&mut files, "styles.xml", "fo:clip='auto'/>", "fo:clip='auto'><draw:image xlink:href='Pictures/keep.png'/></style:graphic-properties>");
    refused_reason(&files, Some("cache unclassified XML context"));
}

#[test]
fn identity_clip_does_not_hide_gamma_unknown_effects_typed_references_or_active_content() {
    for extra in [
        "draw:gamma='NaN%'",
        "draw:gamma='+1%'",
        "draw:gamma='1e2%'",
        "draw:color-inversion='false'",
        "style:protect='none'",
        "xml:base='file:///'",
        "xlink:href='Thumbnails/thumbnail.png'",
    ] {
        refused_reason(
            &fixture("1.3", true, true, "content-auto", "graphic", &format!("fo:clip='auto' {extra}")),
            Some("cache unclassified attribute/value"),
        );
    }
    let mut files = fixture("1.3", true, true, "content-auto", "graphic", "fo:clip='auto' draw:fill='bitmap' draw:fill-image-name='MissingRaster'");
    refused_reason(&files, Some("cache unresolved typed reference"));
    replace(&mut files, "content.xml", "draw:fill='bitmap' draw:fill-image-name='MissingRaster'", "draw:fill='none'");
    replace(&mut files, "content.xml", "xml:id='page'>", "xml:id='page'><office:forms/>");
    refused_reason(&files, Some("cache unclassified XML context"));
}

#[test]
fn identity_clip_keeps_raster_path_mime_payload_and_owner_proofs() {
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
        let mut files = fixture("1.3", true, true, "content-auto", "graphic", "fo:clip='auto'");
        replace(&mut files, "content.xml", "xlink:href='Pictures/keep.png'", &format!("xlink:href='{bad}'"));
        refused(&files);
    }
    let mut files = fixture("1.3", true, true, "common", "graphic", "fo:clip='auto'");
    replace(
        &mut files,
        MANIFEST,
        "manifest:full-path='Pictures/keep.png' manifest:media-type='image/png'",
        "manifest:full-path='Pictures/keep.png' manifest:media-type='text/plain'",
    );
    refused(&files);
    let mut files = fixture("1.3", true, true, "common", "graphic", "fo:clip='auto'");
    files.insert("Pictures/keep.png".into(), b"contributor-original non-raster payload".to_vec());
    refused(&files);
    let mut files = fixture("1.3", true, true, "common", "graphic", "fo:clip='auto'");
    replace(&mut files, "content.xml", "<draw:image xlink:href=", "<draw:image xml:base='file:///' xlink:href=");
    refused_reason(&files, Some("cache unclassified attribute/value"));
}

#[test]
fn local_text_edits_do_not_allow_native_picture_changes_on_any_cache_path() {
    for cache in [false, true] {
        for change in ["alpha", "brightness", "contrast", "grayscale", "bilevel", "crop", "media", "flip", "geometry"] {
            let files = fixture("1.3", cache, cache, "content-auto", "graphic", "fo:clip='auto'");
            let source = zip(&files);
            let mut desired = import(&source).unwrap();
            edit(&mut desired);
            let shape =
                Arc::make_mut(&mut desired.slides[0]).shapes.iter_mut().find(|shape| matches!(shape.kind, ShapeKind::Picture { .. })).unwrap();
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
            let error = export(&desired).expect_err("picture changes are not local plain-text edits").to_string();
            assert!(error.ends_with("changes outside plain slide text are unsupported"), "source proof must precede native-change refusal: {error}");
            assert_eq!(desired, before);
            assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
        }
    }
}

#[test]
fn identity_clip_does_not_exempt_independent_master_warnings() {
    let mut files = fixture("1.3", true, true, "content-auto", "graphic", "fo:clip='auto'");
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
    let error = export(&desired).expect_err("independent owner semantics must still refuse").to_string();
    assert!(
        error.ends_with("source has unsupported content beyond passive text properties"),
        "clip classification must not bypass warning policy: {error}"
    );
    assert_eq!(desired, before);
}

#[test]
fn identity_does_not_short_circuit_unused_definitions_parent_closure_or_foreign_bags() {
    let mut files = fixture("1.3", true, true, "common", "graphic", "fo:clip='auto'");
    replace(
        &mut files,
        "styles.xml",
        "</office:styles>",
        "<style:style style:name='UnusedGamma' style:family='graphic'><style:graphic-properties draw:gamma='NaN%'/></style:style></office:styles>",
    );
    refused_reason(&files, Some("cache unclassified attribute/value"));
    let mut files = fixture("1.3", true, true, "common", "graphic", "fo:clip='auto'");
    replace(&mut files, "styles.xml", "style:name='IdentityClip'", "style:name='IdentityClip' style:parent-style-name='MissingParent'");
    refused_reason(&files, Some("cache unresolved typed reference"));
    let mut files = fixture("1.3", true, true, "common", "graphic", "fo:clip='auto'");
    replace(&mut files, "styles.xml", "style:name='IdentityClip'", "style:name='IdentityClip' style:parent-style-name='IdentityClip'");
    refused_reason(&files, Some("cache style inheritance cycle"));
    for bad in ["<graphic-properties fo:clip='auto'/>", "<f:graphic-properties xmlns:f='urn:deckcraft:test:foreign' fo:clip='auto'/>"] {
        let mut files = fixture("1.3", true, true, "common", "graphic", "fo:clip='auto'");
        replace(&mut files, "styles.xml", "<style:graphic-properties draw:image-opacity='40%' fo:clip='auto'/>", bad);
        refused_reason(&files, Some("cache XML identity/depth"));
    }
}

#[test]
fn live_graphic_master_picture_changes_still_refuse_after_local_text_edit() {
    let files = fixture("1.3", true, true, "styles-auto", "graphic", "fo:clip='auto'");
    let source = zip(&files);
    let mut desired = import(&source).unwrap();
    edit(&mut desired);
    let shape = Arc::make_mut(&mut desired.masters[0]).shapes.iter_mut().find(|shape| matches!(shape.kind, ShapeKind::Picture { .. })).unwrap();
    let ShapeKind::Picture { fill } = &mut shape.kind else { panic!("original graphic master picture") };
    fill.crop[0] = 0.1;
    let before = desired.clone();
    let error = export(&desired).expect_err("master crop is not a local text edit").to_string();
    assert!(error.ends_with("changes outside plain slide text are unsupported"), "master guard must survive clip classification: {error}");
    assert_eq!(desired, before);
    assert_eq!(desired.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
}

#[test]
fn presentation_style_on_master_remains_independently_unsupported() {
    let mut files = fixture("1.3", true, true, "styles-auto", "presentation", "fo:clip='auto'");
    replace(&mut files, "styles.xml", "draw:style-name='RasterMarker'", "presentation:style-name='IdentityClip'");
    let mut desired = import(&zip(&files)).unwrap();
    assert!(desired.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("regular master placeholders or inherited visibility")));
    edit(&mut desired);
    let before = desired.clone();
    let error = export(&desired).expect_err("identity clip must not expand master presentation ownership").to_string();
    assert!(
        error.ends_with("source has unsupported content beyond passive text properties"),
        "existing master warning remains authoritative: {error}"
    );
    assert_eq!(desired, before);
}
