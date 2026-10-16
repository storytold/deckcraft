//! Contributor-original source-retention fixtures for bounded graphic frame sizing controls.
//! BIZ UDPGothic names the existing OFL craft-fonts face; no font bytes are added here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{Presentation, text::AutoFit};
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
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xlink='http://www.w3.org/1999/xlink'";
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
    let png = deckcraft_render::Image { width: 1, height: 1, pixels: vec![37, 109, 181, 255] }.to_png();
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
    let output = export(&desired).expect("bounded frame controls permit supported text edits");
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
    if let Ok(directory) = std::env::var("DECKCRAFT_FRAME_SIZING_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (stage, files) in [("source", before), ("output", after)] {
            for name in [MANIFEST, "content.xml", "styles.xml", "meta.xml", "settings.xml"] {
                let filename = format!("{version}-{stage}-{}", name.replace('/', "-"));
                std::fs::write(std::path::Path::new(&directory).join(filename), &files[name]).unwrap();
            }
        }
    }
}

const FRAME_KEYS: &[&str] = &["draw:auto-grow-height", "draw:fit-to-size", "style:shrink-to-fit", "fo:wrap-option", "fo:min-height", "fo:min-width"];
fn sizing_fixture(
    version: &str,
    preview: bool,
    statistics: bool,
    owner: &str,
    family: &str,
    attributes: &str,
    apply: bool,
) -> BTreeMap<String, Vec<u8>> {
    let mut files = fixture(version, preview, statistics, "<office:scripts/>");
    let bag = format!("<style:graphic-properties {attributes}/>");
    let named = format!("<style:style style:name='Sizing' style:family='{family}'>{bag}</style:style>");
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
    if apply {
        assert_eq!(family, "graphic");
        assert!(matches!(owner, "common" | "content-auto"), "automatic styles retain their exact package scopes");
        replace(&mut files, "content.xml", "xml:id='local'", "xml:id='local' draw:style-name='Sizing'");
    }
    files
}
fn combined_controls(grow: &str, shrink: &str, wrap: &str) -> String {
    format!(
        "draw:auto-grow-height='{grow}' draw:fit-to-size='false' style:shrink-to-fit='{shrink}' fo:wrap-option='{wrap}' fo:min-height='0pt' fo:min-width='12pt'"
    )
}

#[test]
fn each_control_preserves_text_edits_under_all_owners_families_versions_and_cache_paths() {
    for version in ["1.2", "1.3"] {
        for (preview, statistics) in [(false, false), (true, false), (false, true), (true, true)] {
            for owner in ["common", "default", "content-auto", "styles-auto"] {
                for family in ["graphic", "presentation"] {
                    for attributes in [
                        "",
                        "draw:auto-grow-height='false'",
                        "draw:auto-grow-height='true'",
                        "draw:fit-to-size='false'",
                        "style:shrink-to-fit='false'",
                        "style:shrink-to-fit='true'",
                        "fo:wrap-option='wrap'",
                        "fo:wrap-option='no-wrap'",
                        "fo:min-height='12pt'",
                        "fo:min-width='12pt'",
                    ] {
                        let apply = family == "graphic" && matches!(owner, "common" | "content-auto");
                        preserved(&sizing_fixture(version, preview, statistics, owner, family, attributes, apply));
                    }
                }
            }
        }
    }
}

#[test]
fn combined_fitting_controls_reimport_current_body_mapping_and_emit_schema_parts() {
    for version in ["1.2", "1.3"] {
        for grow in ["false", "true"] {
            for shrink in ["false", "true"] {
                for wrap in ["wrap", "no-wrap"] {
                    let files = sizing_fixture(version, true, true, "content-auto", "graphic", &combined_controls(grow, shrink, wrap), true);
                    let p = import(&zip(&files)).unwrap();
                    let body = &p.slides[0].shapes[0].text.as_ref().unwrap().body;
                    let expected = if shrink == "true" {
                        AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 }
                    } else if grow == "true" {
                        AutoFit::Shape
                    } else {
                        AutoFit::None
                    };
                    assert_eq!(body.autofit, Some(expected), "current native mapping; not a claim about all conflicting consumer policies");
                    assert_eq!(body.wrap, Some(wrap == "wrap"));
                    let after = preserved(&files);
                    if grow == "false" && shrink == "false" && wrap == "wrap" {
                        emit(version, &files, &after);
                    }
                }
            }
        }
    }
}

#[test]
fn absolute_minimum_lengths_zero_and_existing_point_bound_are_preserved_without_modeling_constraints() {
    for key in ["fo:min-height", "fo:min-width"] {
        for value in ["0pt", "0cm", "1pt", "2pc", "1in", "2.54cm", "25.4mm", "10000000pt"] {
            let files = sizing_fixture("1.3", true, true, "content-auto", "graphic", &format!("{key}='{value}'"), true);
            preserved(&files);
        }
    }
}

#[test]
fn aliases_entities_comments_attribute_order_and_source_gaps_remain_exact() {
    for version in ["1.2", "1.3"] {
        for cache in [false, true] {
            let mut files = sizing_fixture(version, cache, cache, "content-auto", "graphic", "", true);
            replace(
                &mut files,
                "content.xml",
                "<style:graphic-properties />",
                "<s:graphic-properties xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:f='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' f:min-width = '1&#50;pt' f:wrap-option = \"no-wrap\" s:shrink-to-fit='f&#97;lse' d:fit-to-size='false' f:min-height='0pt' d:auto-grow-height='tr&#117;e'> <!--retained sizing gap--> \n </s:graphic-properties>",
            );
            preserved(&files);
        }
    }
}

#[test]
fn stretch_true_legacy_fit_tokens_and_invalid_boolean_or_wrap_forms_remain_refused() {
    for value in ["true", "all", "shrink-to-fit", "0", "1", "False", "", "false ", " false", "false true", "urn:fit:false"] {
        let files = sizing_fixture("1.3", true, true, "content-auto", "graphic", &format!("draw:fit-to-size='{value}'"), true);
        refused_reason(&files, Some("cache unclassified attribute/value"));
    }
    for key in ["draw:auto-grow-height", "style:shrink-to-fit"] {
        for value in ["0", "1", "True", "FALSE", "", " ", "true ", " false", "true false", "true&#9;", "file:///fit"] {
            refused_reason(
                &sizing_fixture("1.3", true, true, "content-auto", "graphic", &format!("{key}='{value}'"), true),
                Some("cache unclassified attribute/value"),
            );
        }
    }
    for value in ["", "wrap ", "Wrap", "nowrap", "true", "wrap no-wrap", "Thumbnails/thumbnail.png"] {
        refused_reason(
            &sizing_fixture("1.3", true, true, "content-auto", "graphic", &format!("fo:wrap-option='{value}'"), true),
            Some("cache unclassified attribute/value"),
        );
    }
}

#[test]
fn negative_nonfinite_exponent_percent_units_and_over_bound_minimum_lengths_refuse() {
    for key in ["fo:min-height", "fo:min-width"] {
        for value in [
            "",
            "0",
            "-0pt",
            "-1pt",
            "+1pt",
            ".5cm",
            "NaNpt",
            "infpt",
            "1e3pt",
            "10000001pt",
            "138889in",
            "50%",
            "12px",
            "1em",
            "12pt ",
            " 12pt",
            "1pt 2pt",
            "file:///length",
        ] {
            refused_reason(
                &sizing_fixture("1.3", true, true, "content-auto", "graphic", &format!("{key}='{value}'"), true),
                Some("cache unclassified attribute/value"),
            );
        }
    }
}

#[test]
fn unqualified_foreign_rebound_and_duplicate_attribute_identities_refuse() {
    for key in FRAME_KEYS {
        let (prefix, local) = key.split_once(':').unwrap();
        let uri = match prefix {
            "draw" => "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0",
            "style" => "urn:oasis:names:tc:opendocument:xmlns:style:1.0",
            "fo" => "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0",
            _ => panic!("unknown original attribute namespace"),
        };
        let value = match *key {
            "fo:wrap-option" => "wrap",
            "fo:min-height" | "fo:min-width" => "12pt",
            _ => "false",
        };
        for bad in [
            format!("{local}='{value}'"),
            format!("xmlns:f='urn:deckcraft:test:foreign' f:{local}='{value}'"),
            format!("xmlns:{prefix}='urn:deckcraft:test:foreign' {key}='{value}'"),
        ] {
            refused_reason(&sizing_fixture("1.3", true, true, "common", "graphic", &bad, false), Some("cache XML identity/depth"));
        }
        let duplicate = format!("xmlns:a='{uri}' {key}='{value}' a:{local}='{value}'");
        refused_reason(&sizing_fixture("1.3", true, true, "content-auto", "graphic", &duplicate, false), Some("duplicate expanded XML attribute"));
    }
}

#[test]
fn wrong_bags_families_default_children_and_direct_frame_or_textbox_attributes_refuse() {
    for key in FRAME_KEYS {
        let value = match *key {
            "fo:wrap-option" => "wrap",
            "fo:min-height" | "fo:min-width" => "12pt",
            _ => "false",
        };
        for owner in ["common", "default", "content-auto", "styles-auto"] {
            let part = if owner == "content-auto" { "content.xml" } else { "styles.xml" };
            let mut wrong_bag = sizing_fixture("1.3", true, true, owner, "graphic", &format!("{key}='{value}'"), false);
            replace(&mut wrong_bag, part, "style:family='graphic'", "style:family='drawing-page'");
            replace(&mut wrong_bag, part, "style:graphic-properties", "style:drawing-page-properties");
            refused_reason(&wrong_bag, Some("cache unclassified attribute/value"));
            let mut wrong_family = sizing_fixture("1.3", true, true, owner, "graphic", &format!("{key}='{value}'"), false);
            replace(&mut wrong_family, part, "style:family='graphic'", "style:family='paragraph'");
            refused_reason(&wrong_family, Some("cache unclassified XML context"));
        }
        for (old, new) in [
            ("<draw:text-box>".to_string(), format!("<draw:text-box {key}='{value}'>")),
            ("xml:id='local'".to_string(), format!("xml:id='local' {key}='{value}'")),
        ] {
            let mut direct = sizing_fixture("1.3", true, true, "content-auto", "graphic", "", false);
            replace(&mut direct, "content.xml", &old, &new);
            refused_reason(&direct, Some("cache unclassified attribute/value"));
        }
    }
    for bag in [
        "<style:graphic-properties><style:default-style style:family='graphic'/></style:graphic-properties>",
        "<style:graphic-properties><draw:auto-grow-height/></style:graphic-properties>",
        "<f:graphic-properties xmlns:f='urn:deckcraft:test:foreign' draw:auto-grow-height='false'/>",
    ] {
        let mut files = sizing_fixture("1.3", true, true, "content-auto", "graphic", "", false);
        replace(&mut files, "content.xml", "<style:graphic-properties />", bag);
        refused(&files);
    }
    let mut owner_attribute = sizing_fixture("1.3", true, true, "content-auto", "graphic", "", false);
    replace(&mut owner_attribute, "content.xml", "style:name='Sizing'", "style:name='Sizing' draw:auto-grow-height='false'");
    refused_reason(&owner_attribute, Some("cache unclassified attribute/value"));
    let mut wrong_part = sizing_fixture("1.3", true, true, "content-auto", "graphic", "", false);
    replace(&mut wrong_part, "meta.xml", "<office:meta>", "<office:meta><style:graphic-properties draw:auto-grow-height='false'/>");
    refused_reason(&wrong_part, Some("cache unclassified XML context"));
}

#[test]
fn retained_controls_cannot_hide_unknown_properties_active_content_or_resource_dependencies() {
    for extra in [
        "draw:auto-grow-width='true'",
        "draw:fit-to-contour='false'",
        "fo:max-height='12pt'",
        "draw:color-inversion='false'",
        "xlink:href='Thumbnails/thumbnail.png'",
        "xml:base='file:///'",
    ] {
        let files = sizing_fixture("1.3", true, true, "content-auto", "graphic", &format!("draw:auto-grow-height='false' {extra}"), true);
        refused_reason(&files, Some("cache unclassified attribute/value"));
    }
    let mut files = sizing_fixture(
        "1.3",
        true,
        true,
        "content-auto",
        "graphic",
        "draw:auto-grow-height='false' draw:fill='bitmap' draw:fill-image-name='MissingRaster'",
        true,
    );
    refused_reason(&files, Some("cache unresolved typed reference"));
    replace(&mut files, "content.xml", "draw:fill='bitmap' draw:fill-image-name='MissingRaster'", "draw:fill='none'");
    replace(&mut files, "content.xml", "xml:id='page'>", "xml:id='page'><office:forms/>");
    refused_reason(&files, Some("cache unclassified XML context"));
}

#[test]
fn native_geometry_scale_and_body_changes_still_refuse_the_plain_text_preservation_lane() {
    // Engine is not an ODP dev-dependency. These are imported-model regressions for the
    // derivative fields engine fitting can change; root's engine suite owns command coverage.
    for change in ["grow-height", "grow-width", "font-scale", "wrap", "autofit"] {
        let attributes = if change == "font-scale" { combined_controls("false", "true", "wrap") } else { combined_controls("true", "false", "wrap") };
        let files = sizing_fixture("1.3", true, true, "content-auto", "graphic", &attributes, true);
        let source = zip(&files);
        let mut desired = import(&source).unwrap();
        edit(&mut desired);
        let shape = &mut Arc::make_mut(&mut desired.slides[0]).shapes[0];
        match change {
            "grow-height" => shape.xfrm.as_mut().unwrap().h += 1.0,
            "grow-width" => shape.xfrm.as_mut().unwrap().w += 1.0,
            "font-scale" => shape.text.as_mut().unwrap().body.autofit = Some(AutoFit::Shrink { font_scale: Some(0.8), line_reduction: 0.1 }),
            "wrap" => shape.text.as_mut().unwrap().body.wrap = Some(false),
            "autofit" => shape.text.as_mut().unwrap().body.autofit = Some(AutoFit::None),
            _ => panic!("unknown original native-change case"),
        }
        let before = desired.clone();
        let error = export(&desired).expect_err("body/geometry changes are not new supported edit kinds").to_string();
        assert!(
            error.ends_with("changes outside plain slide text are unsupported"),
            "source profile should pass before native-change refusal: {error}"
        );
        assert_eq!(desired, before);
        assert_eq!(desired.media.iter().find(|m| m.content_type == SOURCE).unwrap().data.as_slice(), source);
    }
}

#[test]
fn source_master_field_warnings_are_not_exempted_by_frame_sizing_classification() {
    let mut files = sizing_fixture("1.3", true, true, "content-auto", "graphic", &combined_controls("false", "false", "wrap"), true);
    replace(
        &mut files,
        "styles.xml",
        "<office:styles/>",
        "<office:styles><style:default-style style:family='drawing-page'><style:drawing-page-properties xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' presentation:display-header='true'/></style:default-style></office:styles>",
    );
    let source = zip(&files);
    let mut desired = import(&source).unwrap();
    assert!(desired.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("regular master placeholders or inherited visibility")));
    edit(&mut desired);
    let before = desired.clone();
    let error = export(&desired).expect_err("unsupported master ownership still requires refusal").to_string();
    assert!(
        error.ends_with("source has unsupported content beyond passive text properties"),
        "the cache profile passes but the warning remains: {error}"
    );
    assert_eq!(desired, before);
}
