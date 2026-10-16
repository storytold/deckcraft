//! Contributor-original source-retention fixtures for bounded source-only gamma instructions.
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
    assert_eq!(
        desired.tags.iter().filter(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING).count(),
        1,
        "source-only gamma warning must remain distinct and unique"
    );
    assert!(export(&desired).is_err(), "an actual edit is required");
    edit(&mut desired);
    let before_export = desired.clone();
    let output = export(&desired).expect("typed source-only gamma permits otherwise eligible text edits");
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
    if let Ok(directory) = std::env::var("DECKCRAFT_GAMMA_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (stage, files) in [("source", before), ("output", after)] {
            for name in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                let filename = format!("{version}-{stage}-{}", name.replace('/', "-"));
                std::fs::write(std::path::Path::new(&directory).join(filename), &files[name]).unwrap();
            }
        }
    }
}

const GAMMA_WARNING: &str =
    "ODF picture gamma has no native rendering mapping; source instructions are retained; edited export requires verified source preservation.";
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
fn typed_gamma_survives_all_owners_families_versions_and_cache_routes_with_two_real_edits() {
    for version in ["1.2", "1.3"] {
        for (preview, statistics) in [(false, false), (true, false), (false, true), (true, true)] {
            for owner in ["common", "default", "content-auto", "styles-auto"] {
                for family in ["graphic", "presentation"] {
                    for value in ["0%", "100%", "125.5%", "-50.25%", "-0%"] {
                        preserved(&picture_fixture(version, preview, statistics, owner, family, &format!("draw:gamma='{value}'")));
                    }
                }
            }
        }
    }
}

#[test]
fn gamma_instruction_and_raster_bytes_survive_while_native_preview_stays_approximate() {
    for version in ["1.2", "1.3"] {
        let absent = import(&zip(&picture_fixture(version, true, true, "content-auto", "graphic", ""))).unwrap();
        for value in ["0%", "100%", "125.5%", "-50.25%"] {
            let files = picture_fixture(version, true, true, "content-auto", "graphic", &format!("draw:gamma='{value}' draw:image-opacity='40%'"));
            let before = import(&zip(&files)).unwrap();
            assert_eq!(picture(&before).adjust, picture(&absent).adjust, "there is no native gamma field; this is not a gamma identity claim");
            assert_eq!(picture(&before).alpha, Some(0.4));
            let pixels = picture_pixels(&before);
            let after = preserved(&files);
            assert_eq!(after["Pictures/keep.png"], files["Pictures/keep.png"]);
            let reopened = import(&zip(&after)).unwrap();
            assert_eq!(picture(&reopened), picture(&before));
            assert_eq!(picture_pixels(&reopened), pixels, "approximate native picture preview stays stable around an independent text edit");
            assert!(String::from_utf8(after["content.xml"].clone()).unwrap().contains(&format!("draw:gamma='{value}'")));
            if value == "125.5%" {
                emit(version, &files, &after);
            }
        }
    }
}

#[test]
fn scalar_syntax_is_bounded_without_float_domain_or_spelling_rewrites() {
    for cache in [false, true] {
        for value in ["000000%".to_string(), format!("{}%", "9".repeat(63)), format!("-{}%", "9".repeat(62)), "-000.125%".to_string()] {
            assert!(value.len() <= 64);
            preserved(&picture_fixture("1.3", cache, cache, "common", "graphic", &format!("draw:gamma='{value}'")));
        }
    }
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for value in [
                "",
                "%",
                "-%",
                "+1%",
                "--1%",
                "NaN%",
                "inf%",
                "1e2%",
                ".5%",
                "1.%",
                "1.2.3%",
                "1",
                "1%%",
                "1% ",
                " 1%",
                "1%&#9;",
                "1% 2%",
                "１%",
                "1pt",
                "file:///gamma",
            ] {
                refused(&picture_fixture(version, cache, cache, "content-auto", "graphic", &format!("draw:gamma='{value}'")));
            }
            let over_bound = format!("{}%", "9".repeat(64));
            assert_eq!(over_bound.len(), 65);
            refused(&picture_fixture(version, cache, cache, "common", "graphic", &format!("draw:gamma='{over_bound}'")));
        }
    }
}

#[test]
fn aliases_entities_whitespace_comments_and_style_gaps_survive_as_original_bytes() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            let mut files = picture_fixture(version, cache, cache, "content-auto", "graphic", "");
            replace(
                &mut files,
                "content.xml",
                "<style:graphic-properties />",
                "<s:graphic-properties xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' d:gamma = \"-1&#50;5.5&#37;\"> <!--retained source-only instruction gap--> \r\n </s:graphic-properties>",
            );
            preserved(&files);
        }
    }
}

fn without_passive_audit(mut files: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    replace(&mut files, "content.xml", "<style:paragraph-properties fo:keep-with-next='always'/>", "");
    files
}

#[test]
fn gamma_alone_warns_and_blocks_native_odp_export_before_and_after_text_edit_and_native_save() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for value in ["0%", "100%", "125.5%", "-50.25%"] {
                let files =
                    without_passive_audit(picture_fixture(version, cache, cache, "content-auto", "graphic", &format!("draw:gamma='{value}'")));
                let source = zip(&files);
                let mut p = import(&source).unwrap();
                assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING));
                assert!(!p.tags.iter().any(|(key, _)| key == "odp:semantic-audit"), "gamma is independent of audit coverage v1");
                assert!(export(&p).is_err(), "native export must never silently omit gamma, including conventional values");
                edit(&mut p);
                let before = p.clone();
                let error = export(&p).unwrap_err().to_string();
                assert!(
                    error.ends_with("source audit contains non-preservable semantics"),
                    "the existing passive audit gate remains required: {error}"
                );
                assert_eq!(p, before);
                let native = deckcraft_format::save(&p).unwrap();
                let reopened = deckcraft_format::load(&native).unwrap();
                assert_eq!(reopened, p);
                assert!(reopened.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING));
                assert_eq!(reopened.media.iter().find(|item| item.content_type == SOURCE).unwrap().data.as_slice(), source);
                assert_eq!(picture_pixels(&reopened), picture_pixels(&p));
                assert!(export(&reopened).is_err());
            }
        }
    }
}

#[test]
fn absent_gamma_does_not_gain_the_new_diagnostic_or_native_export_guard() {
    let files = without_passive_audit(picture_fixture("1.3", false, false, "content-auto", "graphic", ""));
    let p = import(&zip(&files)).unwrap();
    assert!(!p.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING));
    let output = export(&p).expect("otherwise represented original raster/text fixture still supports native export");
    let reopened = import(&output).unwrap();
    assert!(!reopened.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING));
}

#[test]
fn gamma_only_in_metadata_settings_or_manifest_still_warns_and_refuses_native_routing() {
    for part in ["meta.xml", "settings.xml", MANIFEST] {
        let mut files = without_passive_audit(picture_fixture("1.3", false, false, "content-auto", "graphic", ""));
        match part {
            "meta.xml" => replace(&mut files, part, "<office:meta>", "<office:meta draw:gamma='100%'>"),
            "settings.xml" => replace(&mut files, part, "office:version='1.3'/>", "office:version='1.3' draw:gamma='100%'/>"),
            MANIFEST => replace(
                &mut files,
                part,
                "<manifest:manifest ",
                "<manifest:manifest xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' d:gamma='100%' ",
            ),
            _ => panic!("original fixed part"),
        }
        let mut p = import(&zip(&files)).unwrap();
        assert!(
            p.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING),
            "wrong part must not silently take native export: {part}"
        );
        edit(&mut p);
        let before = p.clone();
        assert!(export(&p).is_err());
        assert_eq!(p, before);
    }
}

#[test]
fn malformed_owners_parts_families_default_auto_and_property_children_refuse_even_without_caches() {
    for cache in [false, true] {
        for family in ["paragraph", "text", "drawing-page"] {
            refused(&picture_fixture("1.3", cache, cache, "content-auto", family, "draw:gamma='100%'"));
        }
        for owner in ["common", "default", "content-auto", "styles-auto"] {
            let part = if owner == "content-auto" { "content.xml" } else { "styles.xml" };
            let mut files = picture_fixture("1.3", cache, cache, owner, "graphic", "draw:gamma='100%'");
            replace(&mut files, part, "style:graphic-properties", "style:paragraph-properties");
            refused(&files);
        }
        for new_bag in [
            "<style:graphic-properties draw:gamma='100%'><text:p>Original property child</text:p></style:graphic-properties>",
            "<style:graphic-properties draw:gamma='100%'>Original nonleaf text</style:graphic-properties>",
            "<style:graphic-properties draw:gamma='100%'>&#160;</style:graphic-properties>",
        ] {
            let mut files = picture_fixture("1.3", cache, cache, "content-auto", "graphic", "");
            replace(&mut files, "content.xml", "<style:graphic-properties />", new_bag);
            refused(&files);
        }
        for (part, old, new) in [
            ("content.xml", "style:name='PictureControls'", "style:name='PictureControls' draw:gamma='100%'"),
            ("content.xml", "xml:id='picture'", "xml:id='picture' draw:gamma='100%'"),
            ("content.xml", "xlink:type='simple'", "xlink:type='simple' draw:gamma='100%'"),
            ("meta.xml", "<office:meta>", "<office:meta draw:gamma='100%'>"),
            ("settings.xml", "office:version='1.3'/>", "office:version='1.3' draw:gamma='100%'/>"),
        ] {
            let mut files = picture_fixture("1.3", cache, cache, "content-auto", "graphic", "");
            replace(&mut files, part, old, new);
            refused(&files);
        }
        let mut default_auto = picture_fixture("1.3", cache, cache, "styles-auto", "graphic", "draw:gamma='100%'");
        replace(
            &mut default_auto,
            "styles.xml",
            "<style:style style:name='PictureControls' style:family='graphic'>",
            "<style:default-style style:family='graphic'>",
        );
        replace(&mut default_auto, "styles.xml", "</style:style>", "</style:default-style>");
        refused(&default_auto);
    }
}

#[test]
fn all_standard_gamma_occurrences_are_scanned_including_unused_invalid_entries_after_many_valid_ones() {
    for cache in [false, true] {
        let mut files = picture_fixture("1.3", cache, cache, "content-auto", "graphic", "draw:gamma='125.5%'");
        let mut unused = String::new();
        for i in 0..129 {
            unused.push_str(&format!(
                "<style:style style:name='UnusedGamma{i}' style:family='graphic'><style:graphic-properties draw:gamma='100%'/></style:style>"
            ));
        }
        unused.push_str(
            "<style:style style:name='LastInvalidGamma' style:family='graphic'><style:graphic-properties draw:gamma='not-a-percent'/></style:style>",
        );
        replace(&mut files, "styles.xml", "<office:styles/>", &format!("<office:styles>{unused}</office:styles>"));
        refused(&files);
        let mut files = picture_fixture("1.3", cache, cache, "content-auto", "graphic", "draw:gamma='125.5%'");
        replace(
            &mut files,
            "styles.xml",
            "<office:styles/>",
            "<office:styles><style:default-style style:family='graphic'><style:graphic-properties draw:gamma='bad'/></style:default-style></office:styles>",
        );
        refused(&files);
    }
}

#[test]
fn exact_expanded_qnames_foreign_unqualified_rebound_and_duplicate_spellings_stay_guarded() {
    for cache in [false, true] {
        for attributes in [
            "gamma='100%'",
            "xmlns:f='urn:deckcraft:test:foreign' f:gamma='100%'",
            "xmlns:draw='urn:deckcraft:test:foreign' draw:gamma='100%'",
            "xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' draw:gamma='100%' d:gamma='125.5%'",
        ] {
            refused(&picture_fixture("1.3", cache, cache, "common", "graphic", attributes));
        }
    }
}

#[test]
fn stale_removed_or_forged_warning_tags_cannot_become_a_source_retention_capability() {
    for remove_all in [false, true] {
        let files = picture_fixture("1.3", true, true, "content-auto", "graphic", "draw:gamma='125.5%'");
        let mut p = import(&zip(&files)).unwrap();
        edit(&mut p);
        p.tags.retain(|(key, value)| !(key == "odp:warning" && (remove_all || value == GAMMA_WARNING)));
        for i in 0..1100 {
            p.tags.push(("contributor-original-diagnostic-filler".into(), i.to_string()));
        }
        let before = p.clone();
        assert!(export(&p).is_err(), "older/stale models without reconstructed source-only warnings fail closed");
        assert_eq!(p, before);
    }
    let files = picture_fixture("1.3", false, false, "content-auto", "graphic", "");
    let mut p = import(&zip(&files)).unwrap();
    edit(&mut p);
    p.tags.push(("odp:warning".into(), GAMMA_WARNING.into()));
    assert!(export(&p).is_err(), "a forged gamma warning without source evidence grants nothing");
    let invalid = picture_fixture("1.3", false, false, "common", "graphic", "draw:gamma='bad'");
    p.media.iter_mut().find(|item| item.content_type == SOURCE).unwrap().data = Arc::new(zip(&invalid));
    let before = p.clone();
    assert!(export(&p).is_err(), "fresh source evidence outranks forged/stale diagnostics");
    assert_eq!(p, before);
}

#[test]
fn gamma_does_not_hide_unsafe_raster_references_active_children_or_independent_source_warnings() {
    for cache in [false, true] {
        for path in
            ["Thumbnails/thumbnail.png", "../Pictures/keep.png", "Pictures/missing.png", "file:///gamma.png", "https://example.invalid/gamma.png"]
        {
            let mut files = picture_fixture("1.3", cache, cache, "content-auto", "graphic", "draw:gamma='125.5%'");
            replace(&mut files, "content.xml", "xlink:href='Pictures/keep.png'", &format!("xlink:href='{path}'"));
            refused(&files);
        }
        let mut files = picture_fixture("1.3", cache, cache, "common", "graphic", "draw:gamma='125.5%'");
        replace(&mut files, "content.xml", "xml:id='page'>", "xml:id='page'><office:forms/>");
        refused(&files);
        let mut files = picture_fixture("1.3", cache, cache, "content-auto", "graphic", "draw:gamma='125.5%'");
        replace(
            &mut files,
            "styles.xml",
            "<office:styles/>",
            "<office:styles><style:default-style style:family='drawing-page'><style:drawing-page-properties presentation:display-header='true'/></style:default-style></office:styles>",
        );
        let mut p = import(&zip(&files)).unwrap();
        assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING));
        assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("regular master placeholders or inherited visibility")));
        edit(&mut p);
        assert!(export(&p).unwrap_err().to_string().ends_with("source has unsupported content beyond passive text properties"));
    }
}

#[test]
fn gamma_preservation_still_refuses_native_picture_and_geometry_changes() {
    for change in ["alpha", "crop", "brightness", "contrast", "media", "flip", "geometry"] {
        let files = picture_fixture("1.3", true, true, "content-auto", "graphic", "draw:gamma='125.5%'");
        let mut p = import(&zip(&files)).unwrap();
        edit(&mut p);
        let shape = Arc::make_mut(&mut p.slides[0]).shapes.iter_mut().find(|shape| matches!(shape.kind, ShapeKind::Picture { .. })).unwrap();
        if change == "flip" {
            shape.xfrm.as_mut().unwrap().flip_h = true;
        } else if change == "geometry" {
            shape.xfrm.as_mut().unwrap().h += 1.0;
        } else {
            let ShapeKind::Picture { fill } = &mut shape.kind else { panic!("original picture") };
            match change {
                "alpha" => fill.alpha = Some(0.5),
                "crop" => fill.crop[0] = 0.1,
                "brightness" => fill.adjust.brightness = 0.25,
                "contrast" => fill.adjust.contrast = -0.25,
                "media" => fill.media = deckcraft_model::MediaId(u32::MAX),
                _ => panic!("original native mutation"),
            }
        }
        let before = p.clone();
        let error = export(&p).unwrap_err().to_string();
        assert!(
            error.ends_with("changes outside plain slide text are unsupported"),
            "gamma source proof must pass before native comparison: {error}"
        );
        assert_eq!(p, before);
    }
}

#[test]
fn gamma_does_not_admit_shape_paragraph_slide_or_nontext_metadata_edits() {
    for change in ["shape-owner", "paragraph-owner", "slide-owner", "title"] {
        let files = picture_fixture("1.3", false, false, "content-auto", "graphic", "draw:gamma='125.5%'");
        let mut p = import(&zip(&files)).unwrap();
        edit(&mut p);
        match change {
            "shape-owner" => {
                Arc::make_mut(&mut p.slides[0]).shapes.pop();
            }
            "paragraph-owner" => {
                Arc::make_mut(&mut p.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs.pop();
            }
            "slide-owner" => {
                p.slides.push(p.slides[0].clone());
            }
            "title" => p.props.title = "Contributor-original changed title".into(),
            _ => panic!("original owner mutation"),
        }
        let before = p.clone();
        let error = export(&p).unwrap_err().to_string();
        assert!(error.contains("source-preserving text export refused:"));
        assert_eq!(p, before);
    }
}
