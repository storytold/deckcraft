//! Independent W14 contributor-original gamma diagnostics and exact-source adversaries.
//! Pixel arrays are original; Liberation Sans names an existing openly licensed face.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use deckcraft_model::{Presentation, ShapeKind, style::PictureFill};
use deckcraft_odp::{MIME, export, import, validate_source_diagnostics};
use std::{collections::BTreeMap, io::Cursor, io::Read, io::Write, sync::Arc};

type Parts = BTreeMap<String, Vec<u8>>;
const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MANIFEST: &str = "META-INF/manifest.xml";
const RASTER: &str = "Pictures/review.png";
const THUMB: &str = "Thumbnails/thumbnail.png";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:xlink='http://www.w3.org/1999/xlink' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";
const FONT: &str = "<office:font-face-decls><style:font-face style:name='Open' svg:font-family='Liberation Sans'/></office:font-face-decls>";
const GAMMA_WARNING: &str =
    "ODF picture gamma has no native rendering mapping; source instructions are retained; edited export requires verified source preservation.";

fn replace(parts: &mut Parts, part: &str, old: &str, new: &str) {
    let text = String::from_utf8(parts[part].clone()).unwrap();
    assert!(text.contains(old), "missing independent mutation {old}");
    parts.insert(part.into(), text.replace(old, new).into_bytes());
}

fn fixture(version: &str, properties: &str, preview: bool, statistics: bool, passive: bool) -> Parts {
    let audit = if passive { "<style:paragraph-properties fo:keep-with-next='always'/>" } else { "" };
    let content = format!(
        "<office:document-content {NS} office:version='{version}'><office:scripts/>{FONT}<office:automatic-styles><style:style style:name='Image' style:family='graphic'><style:graphic-properties {properties}/></style:style><style:style style:name='Keep' style:family='paragraph'>{audit}<style:text-properties style:font-name='Open' fo:font-size='16pt'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Independent raster' draw:master-page-name='Master' xml:id='page'><draw:frame xml:id='picture' draw:style-name='Image' svg:x='20pt' svg:y='20pt' svg:width='80pt' svg:height='80pt'><draw:image xlink:href='Pictures/review.png' xlink:type='simple' xlink:show='embed' xlink:actuate='onLoad'/></draw:frame><!--between real objects--><draw:frame xml:id='editable' svg:x='160pt' svg:y='20pt' svg:width='200pt' svg:height='160pt'><draw:text-box><!--before real text--><text:p text:style-name='Keep'>First original</text:p><text:p text:style-name='Keep'>Second original</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'>{FONT}<office:styles/><office:automatic-styles><style:page-layout style:name='Page'><style:page-layout-properties fo:page-width='400pt' fo:page-height='240pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='Master' style:page-layout-name='Page'/></office:master-styles></office:document-styles>"
    );
    let counts = if statistics { "<meta:document-statistic meta:word-count='4' meta:page-count='1'/>" } else { "" };
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
    if preview {
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

fn preserved(parts: &Parts) -> Parts {
    let source = package(parts);
    let immutable_source = source.clone();
    let mut current = import(&source).expect("independent original raster fixture imports");
    assert_eq!(current.slides[0].shapes.len(), 2);
    assert_eq!(current.tags.iter().filter(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING).count(), 1);
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
    if parts.contains_key(THUMB) {
        expected.remove(THUMB);
        replace(&mut expected, MANIFEST, "<manifest:file-entry manifest:full-path='Thumbnails/thumbnail.png' manifest:media-type='image/png'/>", "");
    }
    if String::from_utf8_lossy(&parts["meta.xml"]).contains(" meta:word-count='4'") {
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
    let mut current = import(&source).unwrap_or_else(|_| import(&package(&fixture("1.3", "draw:gamma='125%'", true, true, true))).unwrap());
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
fn independent_gamma_source_instruction_survives_all_cache_inventories_and_two_exports() {
    for version in ["1.2", "1.3"] {
        for (preview, statistics) in [(false, false), (true, false), (false, true), (true, true)] {
            for value in ["0%", "100%", "-0%", "-00125.0625%", "1000000%"] {
                let parts = fixture(version, &format!("draw:gamma='{value}' draw:image-opacity='62.5%'"), preview, statistics, true);
                let before = import(&package(&parts)).unwrap();
                assert_eq!(picture(&before).alpha, Some(0.625));
                assert_eq!(picture(&before).adjust, Default::default(), "gamma remains a source instruction with no native field or formula");
                assert!(picture_pixels(&before).2.chunks_exact(4).any(|pixel| pixel[3] > 0));
                let after = preserved(&parts);
                assert!(String::from_utf8(after["content.xml"].clone()).unwrap().contains(&format!("draw:gamma='{value}'")));
            }
        }
    }
}

#[test]
fn independent_gamma_scope_scan_keeps_alias_lexemes_and_unused_definitions() {
    for cache in [false, true] {
        for family in ["graphic", "presentation"] {
            for scope in ["common", "default", "content-auto", "styles-auto"] {
                let mut parts = fixture("1.3", "draw:gamma='-125.0625%'", cache, cache, true);
                let bag = "<s:graphic-properties xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' d:gamma = \"-001&#50;5.0625&#37;\"> <!--exact instruction gap--> \r\n </s:graphic-properties>";
                let named = format!("<style:style style:name='Other' style:family='{family}'>{bag}</style:style>");
                match scope {
                    "common" => replace(&mut parts, "styles.xml", "<office:styles/>", &format!("<office:styles>{named}</office:styles>")),
                    "default" => replace(
                        &mut parts,
                        "styles.xml",
                        "<office:styles/>",
                        &format!("<office:styles><style:default-style style:family='{family}'>{bag}</style:default-style></office:styles>"),
                    ),
                    "content-auto" => replace(&mut parts, "content.xml", "</office:automatic-styles>", &format!("{named}</office:automatic-styles>")),
                    "styles-auto" => replace(&mut parts, "styles.xml", "</office:automatic-styles>", &format!("{named}</office:automatic-styles>")),
                    _ => panic!("independent scope"),
                }
                preserved(&parts);
            }
        }
    }
}

#[test]
fn independent_gamma_scalar_is_syntax_only_bounded_and_never_float_coerced() {
    for cache in [false, true] {
        for value in [format!("{}%", "9".repeat(63)), format!("-{}%", "9".repeat(62)), "-000.000%".into()] {
            assert!(value.len() <= 64);
            preserved(&fixture("1.3", &format!("draw:gamma='{value}'"), cache, cache, true));
        }
        for value in [
            "",
            "%",
            "-%",
            "+0%",
            "--1%",
            "-+1%",
            ".25%",
            "25.%",
            "1e2%",
            "NaN%",
            "inf%",
            "-inf%",
            "1%%",
            "1% ",
            " 1%",
            "1%&#9;",
            "１%",
            "1% 2%",
            "1pt",
            "Pictures/review.png",
        ] {
            refused(&fixture("1.3", &format!("draw:gamma='{value}'"), cache, cache, true));
        }
        let over = format!("{}%", "9".repeat(64));
        assert_eq!(over.len(), 65);
        refused(&fixture("1.3", &format!("draw:gamma='{over}'"), cache, cache, true));
    }
}

#[test]
fn independent_gamma_only_warns_even_at_hundred_and_keeps_original_audit_requirement() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            for value in ["0%", "100%", "-125.5%"] {
                let source = package(&fixture(version, &format!("draw:gamma='{value}'"), cache, cache, false));
                let mut current = import(&source).unwrap();
                assert_eq!(current.tags.iter().filter(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING).count(), 1);
                assert!(!current.tags.iter().any(|(key, _)| key == "odp:semantic-audit"));
                assert!(export(&current).unwrap_err().to_string().contains("source audit contains non-preservable semantics"));
                edit(&mut current, "FirstRevised", "SecondRevised");
                let frozen = current.clone();
                assert!(export(&current).unwrap_err().to_string().contains("source audit contains non-preservable semantics"));
                assert_eq!(current, frozen);
                let reopened = deckcraft_format::load(&deckcraft_format::save(&current).unwrap()).unwrap();
                assert_eq!(reopened, current);
                assert_eq!(reopened.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
                assert!(export(&reopened).is_err());
            }
        }
    }
    let absent = import(&package(&fixture("1.3", "", false, false, false))).unwrap();
    assert!(!absent.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING));
    assert!(export(&absent).is_ok());
}

#[test]
fn independent_gamma_diagnostic_scan_reaches_fixed_parts_and_parsed_embedded_xml() {
    for part in ["content.xml", "styles.xml", "meta.xml", "settings.xml", MANIFEST, "Object/content.xml"] {
        let mut parts = fixture("1.3", "", false, false, false);
        match part {
            "content.xml" => replace(&mut parts, part, "<office:body>", "<office:body draw:gamma='100%'>"),
            "styles.xml" => replace(&mut parts, part, "<office:styles/>", "<office:styles draw:gamma='100%'/>"),
            "meta.xml" => replace(&mut parts, part, "<office:meta>", "<office:meta draw:gamma='100%'>"),
            "settings.xml" => replace(&mut parts, part, "office:version='1.3'/>", "office:version='1.3' draw:gamma='100%'/>"),
            MANIFEST => replace(
                &mut parts,
                part,
                "<manifest:manifest ",
                "<manifest:manifest xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' d:gamma='100%' ",
            ),
            _ => {
                parts.insert(
                    part.into(),
                    format!("<office:document-content {NS} office:version='1.3' draw:gamma='100%'><office:body/></office:document-content>")
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
        let source = package(&parts);
        let current = import(&source).unwrap();
        assert!(
            current.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING),
            "standard expanded gamma must be diagnosed in {part}"
        );
        assert!(export(&current).is_err());
        let mut legacy = current.clone();
        legacy.tags.retain(|(key, _)| key != "odp:warning");
        assert!(validate_source_diagnostics(&legacy).is_err(), "fresh source scan must reconstruct a missing warning from {part}");
        assert!(export(&legacy).is_err());
        assert_eq!(legacy.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice(), source);
    }
}

#[test]
fn independent_gamma_wrong_owner_leaf_family_and_exact_qname_do_not_borrow_a_valid_occurrence() {
    for cache in [false, true] {
        for (family, bag) in
            [("paragraph", "style:paragraph-properties"), ("text", "style:text-properties"), ("drawing-page", "style:drawing-page-properties")]
        {
            let mut parts = fixture("1.3", "draw:gamma='100%'", cache, cache, true);
            let unused = format!("<style:style style:name='Bad' style:family='{family}'><{bag} draw:gamma='100%'/></style:style>");
            replace(&mut parts, "content.xml", "</office:automatic-styles>", &format!("{unused}</office:automatic-styles>"));
            refused(&parts);
        }
        for attributes in [
            "gamma='100%'",
            "xmlns:draw='urn:review:foreign' draw:gamma='100%'",
            "xmlns:x='urn:review:foreign' x:gamma='100%'",
            "xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' draw:gamma='100%' d:gamma='125%'",
        ] {
            refused(&fixture("1.3", attributes, cache, cache, true));
        }
        for owner in [
            "<draw:frame draw:gamma='100%' svg:width='10pt' svg:height='10pt'><draw:image xlink:href='Pictures/review.png'/></draw:frame>",
            "<draw:frame svg:width='10pt' svg:height='10pt'><draw:image draw:gamma='100%' xlink:href='Pictures/review.png'/></draw:frame>",
        ] {
            let mut parts = fixture("1.3", "draw:gamma='100%'", cache, cache, true);
            replace(&mut parts, "content.xml", "</draw:page>", &format!("{owner}</draw:page>"));
            refused(&parts);
        }
        for bag in [
            "<style:graphic-properties draw:gamma='100%'>nonleaf</style:graphic-properties>",
            "<style:graphic-properties draw:gamma='100%'><draw:image xlink:href='Pictures/review.png'/></style:graphic-properties>",
        ] {
            let mut parts = fixture("1.3", "draw:gamma='100%'", cache, cache, true);
            replace(&mut parts, "content.xml", "<style:graphic-properties draw:gamma='100%'/>", bag);
            refused(&parts);
        }
        let mut parts = fixture("1.3", "draw:gamma='100%'", cache, cache, true);
        replace(
            &mut parts,
            "styles.xml",
            "<office:automatic-styles>",
            "<office:automatic-styles><style:default-style style:family='graphic'><style:graphic-properties draw:gamma='100%'/></style:default-style>",
        );
        refused(&parts);
    }
}

#[test]
fn independent_gamma_aggregate_never_truncates_invalid_unused_evidence_or_budget_checks() {
    for cache in [false, true] {
        let mut parts = fixture("1.3", "draw:gamma='125%'", cache, cache, true);
        let mut definitions = String::new();
        for index in 0..140 {
            definitions.push_str(&format!(
                "<style:style style:name='Valid{index}' style:family='graphic'><style:graphic-properties draw:gamma='100%'/></style:style>"
            ));
        }
        definitions.push_str("<style:style style:name='Last' style:family='graphic'><style:graphic-properties draw:gamma='bad'/></style:style>");
        replace(&mut parts, "styles.xml", "<office:styles/>", &format!("<office:styles>{definitions}</office:styles>"));
        let imported = import(&package(&parts)).unwrap();
        assert_eq!(imported.tags.iter().filter(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING).count(), 1);
        refused(&parts);
        let mut oversize = fixture("1.3", "draw:gamma='125%'", cache, cache, true);
        replace(&mut oversize, "content.xml", "draw:gamma='125%'", &format!("draw:gamma='{}%'", "9".repeat(8192)));
        assert!(import(&package(&oversize)).is_err(), "existing parser value budget remains enforced");
        refused(&oversize);
    }
}

#[test]
fn independent_legacy_diagnostics_and_source_identity_are_freshly_verified() {
    let source = package(&fixture("1.3", "draw:gamma='125%'", false, false, true));
    let original = import(&source).unwrap();
    for mutation in [
        "gamma-warning",
        "all-warning",
        "imported",
        "imported-value",
        "source-empty",
        "source-linked",
        "source-duplicate",
        "source-corrupt",
        "source-retyped",
        "source-removed",
    ] {
        let mut current = original.clone();
        edit(&mut current, "FirstRevised", "SecondRevised");
        match mutation {
            "gamma-warning" => current.tags.retain(|(key, value)| !(key == "odp:warning" && value == GAMMA_WARNING)),
            "all-warning" => current.tags.retain(|(key, _)| key != "odp:warning"),
            "imported" => current.tags.retain(|(key, _)| key != "odp:imported"),
            "imported-value" => current.tags.iter_mut().filter(|(key, _)| key == "odp:imported").for_each(|(_, value)| *value = "0".into()),
            "source-duplicate" => {
                let mut duplicate = current.media.iter().find(|media| media.content_type == SOURCE).unwrap().clone();
                duplicate.id = deckcraft_model::MediaId(100_000);
                current.media.push(duplicate);
            }
            "source-removed" => current.media.retain(|media| media.content_type != SOURCE),
            _ => {
                let media = current.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap();
                match mutation {
                    "source-empty" => media.data = Arc::new(Vec::new()),
                    "source-linked" => media.link = Some("https://example.invalid/source.odp".into()),
                    "source-corrupt" => media.data = Arc::new(b"contributor-original invalid package".to_vec()),
                    "source-retyped" => media.content_type = "application/octet-stream".into(),
                    _ => panic!("independent diagnostic mutation"),
                }
            }
        }
        for index in 0..1100 {
            current.tags.push(("contributor-original-diagnostic-filler".into(), index.to_string()));
        }
        let frozen = current.clone();
        assert!(validate_source_diagnostics(&current).is_err(), "fresh diagnostic validation must reject {mutation}");
        assert!(export(&current).is_err(), "ODP export must reject {mutation}");
        assert_eq!(current, frozen);
        let native = deckcraft_format::load(&deckcraft_format::save(&current).unwrap()).unwrap();
        assert!(export(&native).is_err(), "native roundtrip cannot repair forged source authority: {mutation}");
    }
}

#[test]
fn independent_gamma_proof_never_exempts_other_source_warnings_or_current_model_changes() {
    for cache in [false, true] {
        let mut parts = fixture("1.3", "draw:gamma='125%'", cache, cache, true);
        replace(
            &mut parts,
            "styles.xml",
            "<office:styles/>",
            "<office:styles><style:default-style style:family='drawing-page'><style:drawing-page-properties presentation:display-header='true'/></style:default-style></office:styles>",
        );
        let source = package(&parts);
        let mut current = import(&source).unwrap();
        assert!(current.tags.iter().any(|(key, value)| key == "odp:warning" && value == GAMMA_WARNING));
        assert!(
            current.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("regular master placeholders or inherited visibility"))
        );
        edit(&mut current, "FirstRevised", "SecondRevised");
        current.tags.retain(|(key, value)| key != "odp:warning" || value == GAMMA_WARNING);
        let frozen = current.clone();
        assert!(
            export(&current).unwrap_err().to_string().contains("source has unsupported content beyond passive text properties"),
            "fresh baseline warnings cannot be hidden by current-tag removal"
        );
        assert_eq!(current, frozen);
        for mutation in 0..4 {
            let mut current = import(&package(&fixture("1.3", "draw:gamma='125%'", cache, cache, true))).unwrap();
            edit(&mut current, "FirstRevised", "SecondRevised");
            match mutation {
                0 => current.props.title = "Original contributor changed title".into(),
                1 => Arc::make_mut(&mut current.slides[0]).shapes[0].xfrm.as_mut().unwrap().flip_h = true,
                2 => {
                    let ShapeKind::Picture { fill } = &mut Arc::make_mut(&mut current.slides[0]).shapes[0].kind else { panic!("actual raster") };
                    fill.adjust.brightness = 0.25;
                }
                _ => current.tags.push(("odp:warning".into(), "ODF unrelated contributor-original warning".into())),
            }
            let frozen = current.clone();
            assert!(export(&current).unwrap_err().to_string().contains("changes outside plain slide text"));
            assert_eq!(current, frozen);
        }
    }
}

#[test]
fn independent_forged_gamma_warning_source_swap_and_active_dependencies_grant_no_capability() {
    let mut no_gamma = import(&package(&fixture("1.3", "", false, false, true))).unwrap();
    edit(&mut no_gamma, "FirstRevised", "SecondRevised");
    no_gamma.tags.push(("odp:warning".into(), GAMMA_WARNING.into()));
    assert!(export(&no_gamma).unwrap_err().to_string().contains("changes outside plain slide text"));
    let mut invalid = fixture("1.3", "draw:gamma='125%'", false, false, true);
    replace(&mut invalid, "content.xml", "draw:gamma='125%'", "draw:gamma='bad'");
    no_gamma.media.iter_mut().find(|media| media.content_type == SOURCE).unwrap().data = Arc::new(package(&invalid));
    assert!(export(&no_gamma).unwrap_err().to_string().contains("outside the bounded retention contract"));
    for cache in [false, true] {
        for (old, new) in [
            ("<office:scripts/>", "<office:scripts><office:script/></office:scripts>"),
            ("xlink:href='Pictures/review.png'", "xlink:href='https://example.invalid/image.png'"),
            ("xlink:href='Pictures/review.png'", "xlink:href='Thumbnails/thumbnail.png'"),
        ] {
            let mut parts = fixture("1.3", "draw:gamma='125%'", cache, cache, true);
            replace(&mut parts, "content.xml", old, new);
            refused(&parts);
        }
    }
}
