//! Contributor-original synthetic ODF minimum/normal paragraph policies; no assets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    Presentation, ShapeKind, TextBody,
    text::{LineHeight, Spacing},
};
use deckcraft_odp::{MIME, export, import};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    sync::Arc,
};

const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0'";

fn style(name: &str, parent: Option<&str>, attrs: &str) -> String {
    let parent = parent.map(|name| format!(" style:parent-style-name='{name}'")).unwrap_or_default();
    format!(
        "<style:style style:name='{name}' style:family='paragraph'{parent}><style:paragraph-properties {attrs}/><style:text-properties fo:font-family='BIZ UDPGothic' fo:font-size='17pt'/></style:style>"
    )
}
fn frame(paragraphs: &str) -> String {
    format!("<draw:frame svg:width='240pt' svg:height='120pt'><draw:text-box>{paragraphs}</draw:text-box></draw:frame>")
}
fn files(version: &str, common: &str, automatic: &str, shapes: &str) -> BTreeMap<String, Vec<u8>> {
    let content = format!(
        "<office:document-content {NS} office:version='{version}'><office:automatic-styles>{automatic}</office:automatic-styles><office:body><office:presentation><draw:page draw:name='Minimum policy' draw:master-page-name='Master'>{shapes}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='{version}'><office:styles>{common}</office:styles><office:automatic-styles><style:page-layout style:name='Paper'><style:page-layout-properties fo:page-width='600pt' fo:page-height='400pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='Master' style:page-layout-name='Paper'/></office:master-styles></office:document-styles>"
    );
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='{version}'><manifest:file-entry manifest:full-path='/' manifest:media-type='{MIME}' manifest:version='{version}'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='styles.xml' manifest:media-type='text/xml'/></manifest:manifest>"
    );
    BTreeMap::from([
        ("mimetype".into(), MIME.as_bytes().to_vec()),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
        ("META-INF/manifest.xml".into(), manifest.into_bytes()),
    ])
}
fn package(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("mimetype", options).unwrap();
    archive.write_all(MIME.as_bytes()).unwrap();
    for (name, data) in files.iter().filter(|(name, _)| name.as_str() != "mimetype") {
        archive.start_file(name, options).unwrap();
        archive.write_all(data).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        assert!(files.insert(entry.name().into(), data).is_none());
    }
    files
}
fn replace(files: &mut BTreeMap<String, Vec<u8>>, part: &str, old: &str, new: &str) {
    let value = String::from_utf8(files[part].clone()).unwrap();
    assert!(value.contains(old));
    files.insert(part.into(), value.replace(old, new).into_bytes());
}
fn body(presentation: &Presentation) -> &TextBody {
    presentation.slides[0].shapes[0].text.as_ref().unwrap()
}
fn edit(presentation: &mut Presentation) {
    Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "Actually edited".into();
}
fn clean(presentation: &Presentation) {
    assert!(!presentation.tags.iter().any(|(key, _)| key == "odp:warning" || key == "odp:semantic-audit"), "{:?}", presentation.tags);
}
fn source(presentation: &Presentation) -> &[u8] {
    presentation.media.iter().find(|media| media.content_type == SOURCE).unwrap().data.as_slice()
}
fn policy(presentation: &Presentation, index: usize) -> Option<LineHeight> {
    body(presentation).paragraphs[index].props.line_height
}
fn emit(version: &str, input: &BTreeMap<String, Vec<u8>>, output: &[u8]) {
    if let Ok(directory) = std::env::var("DECKCRAFT_MINIMUM_LINE_HEIGHT_XML_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        for (stage, members) in [("input", input.clone()), ("output", parts(output))] {
            for (name, bytes) in members.iter().filter(|(name, _)| name.ends_with(".xml")) {
                std::fs::write(std::path::Path::new(&directory).join(format!("{version}-{stage}-{}", name.replace('/', "-"))), bytes).unwrap();
            }
        }
    }
}
fn guarded(files: &BTreeMap<String, Vec<u8>>) {
    let bytes = package(files);
    let mut presentation = import(&bytes).expect("unsupported spacing remains recoverable");
    assert!(presentation.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert_eq!(source(&presentation), bytes);
    edit(&mut presentation);
    let snapshot = presentation.clone();
    assert!(export(&presentation).is_err(), "unsupported source semantics must not be laundered by native rewrite");
    assert_eq!(presentation, snapshot);
    assert_eq!(source(&presentation), bytes);
}

#[test]
fn minimum_normal_and_empty_paragraph_defaults_survive_native_and_actual_odp_edits_both_versions() {
    for version in ["1.2", "1.3"] {
        let common = style("Base", None, "style:line-height-at-least='25pt'");
        let automatic = format!("{}{}", style("P", Some("Base"), ""), style("Normal", Some("Base"), "fo:line-height='normal'"));
        let input = files(
            version,
            &common,
            &automatic,
            &frame("<text:p text:style-name='P'>Original</text:p><text:p text:style-name='P'/><text:p text:style-name='Normal'>Reset</text:p>"),
        );
        let bytes = package(&input);
        let mut presentation = import(&bytes).unwrap();
        clean(&presentation);
        assert_eq!(policy(&presentation, 0), Some(LineHeight::Minimum { points: 25.0 }));
        assert_eq!(policy(&presentation, 1), Some(LineHeight::Minimum { points: 25.0 }));
        assert_eq!(policy(&presentation, 2), Some(LineHeight::Normal));
        assert!(body(&presentation).paragraphs.iter().all(|paragraph| paragraph.props.line_spacing.is_none()));
        edit(&mut presentation);
        let native = deckcraft_format::save(&presentation).unwrap();
        let restored = deckcraft_format::load(&native).unwrap();
        assert_eq!(restored, presentation, "native persistence keeps the complete model and original source");
        let output = export(&restored).unwrap();
        assert_eq!(source(&restored), bytes);
        let reopened = import(&output).unwrap();
        clean(&reopened);
        assert_eq!(body(&reopened).paragraphs[0].text(), "Actually edited");
        for index in 0..3 {
            assert_eq!(policy(&reopened, index), policy(&restored, index));
            let props = &body(&reopened).paragraphs[index].def_props;
            assert_eq!(props.font.as_deref(), Some("BIZ UDPGothic"));
            assert_eq!(props.size, Some(17.0));
        }
        let xml = String::from_utf8(parts(&output)["content.xml"].clone()).unwrap();
        assert!(xml.contains("style:line-height-at-least=\"25pt\""));
        assert!(xml.contains("fo:line-height=\"normal\""));
        assert!(!xml.contains("fo:line-height=\"25pt\""));
        emit(version, &input, &output);
    }
}

#[test]
fn effective_odf_attribute_inheritance_keeps_normal_disable_and_explicit_zero() {
    let common = format!(
        "<style:default-style style:family='paragraph'><style:paragraph-properties style:line-height-at-least='18pt'/></style:default-style>{}{}",
        style("Parent", None, "style:line-height-at-least='28pt'"),
        style("NormalParent", None, "fo:line-height='normal'")
    );
    let automatic = format!(
        "{}{}{}",
        style("Inherited", Some("Parent"), ""),
        style("Zero", Some("Parent"), "style:line-height-at-least='0pt'"),
        style("Disabled", Some("NormalParent"), "style:line-height-at-least='45pt'")
    );
    let presentation = import(&package(&files("1.3", &common, &automatic, &frame("<text:p>Default</text:p><text:p text:style-name='Inherited'>Parent</text:p><text:p text:style-name='Zero'>Zero</text:p><text:p text:style-name='Disabled'>Disabled</text:p>")))).unwrap();
    clean(&presentation);
    assert_eq!(
        (0..4).map(|index| policy(&presentation, index)).collect::<Vec<_>>(),
        [
            Some(LineHeight::Minimum { points: 18.0 }),
            Some(LineHeight::Minimum { points: 28.0 }),
            Some(LineHeight::Minimum { points: 0.0 }),
            Some(LineHeight::Normal)
        ]
    );
}

#[test]
fn normal_disables_valid_minimum_and_signed_additional_without_inventing_combination_order() {
    for additional in ["-5pt", "0pt", "2mm", "1000000pt", "-1000000pt"] {
        let automatic = style("P", None, &format!("fo:line-height='normal' style:line-height-at-least='20pt' style:line-spacing='{additional}'"));
        let presentation = import(&package(&files("1.3", "", &automatic, &frame("<text:p text:style-name='P'>Normal</text:p>")))).unwrap();
        clean(&presentation);
        assert_eq!(policy(&presentation, 0), Some(LineHeight::Normal));
        let reopened = import(&export(&presentation).unwrap()).unwrap();
        clean(&reopened);
        assert_eq!(policy(&reopened, 0), Some(LineHeight::Normal));
    }
}

#[test]
fn additional_and_mixed_effective_combinations_keep_used_and_unused_export_guards() {
    for attrs in [
        "style:line-spacing='-2pt'",
        "style:line-spacing='0pt'",
        "style:line-height-at-least='10pt' style:line-spacing='3pt'",
        "fo:line-height='120%' style:line-height-at-least='10pt'",
        "fo:line-height='20pt' style:line-spacing='3pt'",
    ] {
        let automatic = style("P", None, attrs);
        guarded(&files("1.3", "", &automatic, &frame("<text:p text:style-name='P'>Original</text:p>")));
        guarded(&files("1.3", "", &automatic, &frame("<text:p>Original</text:p>")));
    }
    for (parent, child) in [
        ("style:line-height-at-least='24pt'", "fo:line-height='120%'"),
        ("fo:line-height='20pt'", "style:line-height-at-least='10pt'"),
        ("style:line-height-at-least='24pt'", "style:line-spacing='-2pt'"),
    ] {
        guarded(&files(
            "1.3",
            &style("Base", None, parent),
            &style("P", Some("Base"), child),
            &frame("<text:p text:style-name='P'>Original</text:p>"),
        ));
    }
}

#[test]
fn unused_missing_wrong_family_and_cyclic_paragraph_chains_remain_contextually_blocking() {
    let shape = frame("<text:p>Original</text:p>");
    for common in [
        style("Unused", Some("Missing"), "style:line-height-at-least='24pt'"),
        format!(
            "<style:style style:name='WrongFamily' style:family='text'/>{}",
            style("Unused", Some("WrongFamily"), "style:line-height-at-least='24pt'")
        ),
        format!("{}{}", style("A", Some("B"), "style:line-height-at-least='24pt'"), style("B", Some("A"), "")),
    ] {
        guarded(&files("1.3", &common, "", &shape));
    }
}

#[test]
fn minimum_and_disabled_modifier_values_reject_malformed_and_postconversion_overflow() {
    for value in ["-1pt", "NaNpt", "infpt", "1e309pt", "1000001pt", "14000in", "999999cm", "12%", "normal", ""] {
        let input = files(
            "1.3",
            "",
            &style("P", None, &format!("style:line-height-at-least='{value}'")),
            &frame("<text:p text:style-name='P'>Original</text:p>"),
        );
        assert!(import(&package(&input)).is_err(), "invalid minimum must not become an inherited/default policy: {value}");
    }
    for (attr, value) in [("style:line-height-at-least", "-1pt"), ("style:line-spacing", "NaNpt"), ("style:line-spacing", "14000in")] {
        let input = files(
            "1.3",
            "",
            &style("P", None, &format!("fo:line-height='normal' {attr}='{value}'")),
            &frame("<text:p text:style-name='P'>Original</text:p>"),
        );
        assert!(import(&package(&input)).is_err(), "normal does not acknowledge malformed disabled values");
    }
    for (value, expected) in [("1000000pt", 1_000_000.0), ("0", 0.0), ("1in", 72.0), ("2.54cm", 72.0)] {
        let presentation = import(&package(&files(
            "1.3",
            "",
            &style("P", None, &format!("style:line-height-at-least='{value}'")),
            &frame("<text:p text:style-name='P'>Original</text:p>"),
        )))
        .unwrap();
        clean(&presentation);
        let Some(LineHeight::Minimum { points }) = policy(&presentation, 0) else { panic!("minimum") };
        assert!((points - expected).abs() < 1e-9);
    }
}

#[test]
fn legacy_numeric_only_invalid_values_remain_recoverable_without_acknowledging_new_modifiers() {
    for (value, expected) in [
        ("NaN%", None),
        ("NaNpt", None),
        ("garbage", None),
        ("0%", Some(Spacing::Pct(0.0))),
        ("-20%", Some(Spacing::Pct(-0.2))),
        ("0pt", Some(Spacing::Pts(0.0))),
        ("-2pt", Some(Spacing::Pts(-2.0))),
    ] {
        for unrelated_minimum in [false, true] {
            let common = if unrelated_minimum { style("Unrelated", None, "style:line-height-at-least='24pt'") } else { String::new() };
            let input = files(
                "1.3",
                &common,
                &style("P", None, &format!("fo:line-height='{value}'")),
                &frame("<text:p text:style-name='P'>Original</text:p>"),
            );
            let bytes = package(&input);
            let presentation = import(&bytes).expect("old numeric-only source values remain recoverable");
            assert_eq!(body(&presentation).paragraphs[0].props.line_spacing, expected);
            assert_eq!(policy(&presentation, 0), None);
            assert!(presentation.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("semantic audit")));
            assert_eq!(source(&presentation), bytes);
            assert!(export(&presentation).is_err(), "unchanged legacy-invalid source must not silently rewrite");
        }
    }
    let inherited_minimum = style("Base", None, "style:line-height-at-least='24pt'");
    let input =
        files("1.3", &inherited_minimum, &style("P", Some("Base"), "fo:line-height='NaN%'"), &frame("<text:p text:style-name='P'>Original</text:p>"));
    assert!(import(&package(&input)).is_err(), "a modifier in this actual chain keeps new-policy validation strict");
}

#[test]
fn foreign_namespace_and_wrong_bags_cannot_mint_minimum_policy() {
    for properties in [
        "<style:text-properties style:line-height-at-least='24pt'/>",
        "<style:graphic-properties style:line-height-at-least='24pt'/>",
        "<style:paragraph-properties xmlns:q='urn:minimum:foreign' q:line-height-at-least='24pt'/>",
    ] {
        let automatic = format!("<style:style style:name='P' style:family='paragraph'>{properties}</style:style>");
        let input = files("1.3", "", &automatic, &frame("<text:p text:style-name='P'>Original</text:p>"));
        let presentation = import(&package(&input)).unwrap();
        assert_eq!(policy(&presentation, 0), None);
        guarded(&input);
    }
    let mut aliases =
        files("1.3", "", &style("P", None, "style:line-height-at-least='24pt'"), &frame("<text:p text:style-name='P'>Original</text:p>"));
    replace(&mut aliases, "content.xml", "xmlns:style=", "xmlns:q=");
    replace(&mut aliases, "content.xml", "<style:", "<q:");
    replace(&mut aliases, "content.xml", "</style:", "</q:");
    replace(&mut aliases, "content.xml", " style:", " q:");
    let presentation = import(&package(&aliases)).unwrap();
    clean(&presentation);
    assert_eq!(policy(&presentation, 0), Some(LineHeight::Minimum { points: 24.0 }));
}

#[test]
fn nonparagraph_family_defaults_keep_explicit_unmapped_spacing_guards() {
    for family in ["graphic", "presentation", "table-cell"] {
        for attrs in ["style:line-height-at-least='24pt'", "fo:line-height='normal' style:line-spacing='-2pt'"] {
            let common = format!("<style:default-style style:family='{family}'><style:paragraph-properties {attrs}/></style:default-style>");
            guarded(&files("1.3", &common, "", &frame("<text:p>Original</text:p>")));
        }
    }
}

#[test]
fn writer_refuses_invalid_or_dual_native_policy_without_mutating_original_source() {
    let input = files("1.3", "", &style("P", None, ""), &frame("<text:p text:style-name='P'>Original</text:p>"));
    let mut presentation = import(&package(&input)).unwrap();
    clean(&presentation);
    for points in [f64::NAN, f64::INFINITY, -1.0, 1_000_001.0] {
        Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].props.line_height =
            Some(LineHeight::Minimum { points });
        let before = source(&presentation).to_vec();
        let error = export(&presentation).unwrap_err().to_string();
        assert!(error.contains("invalid or conflicting paragraph line-height policy"), "{error}");
        assert_eq!(source(&presentation), before);
    }
    let props = &mut Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].props;
    props.line_height = Some(LineHeight::Normal);
    props.line_spacing = Some(Spacing::Pts(20.0));
    let snapshot = presentation.clone();
    assert!(export(&presentation).unwrap_err().to_string().contains("invalid or conflicting paragraph line-height policy"));
    assert_eq!(presentation, snapshot);
}

#[test]
fn explicit_table_paragraph_minimum_and_latin_typing_defaults_survive_rewrite() {
    let automatic = style("P", None, "style:line-height-at-least='24pt'");
    let table = "<draw:frame svg:width='240pt' svg:height='120pt'><table:table table:name='Original table'><table:table-column/><table:table-row><table:table-cell><text:p text:style-name='P'>Cell</text:p><text:p text:style-name='P'/></table:table-cell></table:table-row></table:table></draw:frame>";
    let presentation = import(&package(&files("1.3", "", &automatic, table))).unwrap();
    clean(&presentation);
    let output = export(&presentation).unwrap();
    let reopened = import(&output).unwrap();
    clean(&reopened);
    for deck in [&presentation, &reopened] {
        let ShapeKind::Table(table) = &deck.slides[0].shapes[0].kind else { panic!("table") };
        for paragraph in &table.rows[0].cells[0].text.paragraphs {
            assert_eq!(paragraph.props.line_height, Some(LineHeight::Minimum { points: 24.0 }));
            assert_eq!(paragraph.def_props.font.as_deref(), Some("BIZ UDPGothic"));
            assert_eq!(paragraph.def_props.size, Some(17.0));
        }
    }
}

#[test]
fn distinct_latin_paragraph_end_font_or_size_refuses_instead_of_overwriting_defaults() {
    let input = files("1.3", "", &style("P", None, ""), &frame("<text:p text:style-name='P'>Original</text:p><text:p text:style-name='P'/>"));
    for font in [false, true] {
        let mut presentation = import(&package(&input)).unwrap();
        let end = &mut Arc::make_mut(&mut presentation.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[1].end_props;
        if font {
            end.font = Some("BIZ UDPMincho".into());
        } else {
            end.size = Some(31.0);
        }
        let snapshot = presentation.clone();
        assert!(export(&presentation).unwrap_err().to_string().contains("distinct paragraph-end Latin font or size"));
        assert_eq!(presentation, snapshot);
    }
}

fn notes_input(shared_attrs: &str, override_attrs: &str) -> BTreeMap<String, Vec<u8>> {
    let common = format!("{}<style:style style:name='NotesBody' style:family='presentation'/>", style("SharedNotes", None, shared_attrs));
    let automatic = style("LocalNotes", Some("SharedNotes"), override_attrs);
    let notes_frame = |paragraph: &str| {
        format!(
            "<draw:frame presentation:class='notes' presentation:style-name='NotesBody' draw:text-style-name='SharedNotes' svg:x='20pt' svg:y='100pt' svg:width='300pt' svg:height='180pt'><draw:text-box>{paragraph}</draw:text-box></draw:frame>"
        )
    };
    let shapes = format!(
        "{}<presentation:notes>{}</presentation:notes>",
        frame("<text:p>Slide</text:p>"),
        notes_frame("<text:p text:style-name='LocalNotes'>Spoken</text:p>")
    );
    let mut input = files("1.3", &common, &automatic, &shapes);
    replace(
        &mut input,
        "styles.xml",
        "<style:master-page style:name='Master' style:page-layout-name='Paper'/>",
        &format!(
            "<style:master-page style:name='Master' style:page-layout-name='Paper'><presentation:notes>{}</presentation:notes></style:master-page>",
            notes_frame("<text:p text:style-name='SharedNotes'/>")
        ),
    );
    input
}

#[test]
fn structured_notes_local_style_reconstruction_preserves_shared_and_local_minimum_ownership() {
    for (shared, local, expected_shared, expected_local) in [
        ("style:line-height-at-least='24pt'", "", Some(LineHeight::Minimum { points: 24.0 }), None),
        (
            "style:line-height-at-least='24pt'",
            "style:line-height-at-least='31pt'",
            Some(LineHeight::Minimum { points: 24.0 }),
            Some(LineHeight::Minimum { points: 31.0 }),
        ),
        (
            "style:line-height-at-least='24pt'",
            "style:line-height-at-least='24pt'",
            Some(LineHeight::Minimum { points: 24.0 }),
            Some(LineHeight::Minimum { points: 24.0 }),
        ),
        ("style:line-height-at-least='24pt'", "fo:line-height='normal'", Some(LineHeight::Minimum { points: 24.0 }), Some(LineHeight::Normal)),
        ("fo:line-height='normal'", "", Some(LineHeight::Normal), None),
        ("fo:line-height='normal'", "fo:line-height='normal'", Some(LineHeight::Normal), Some(LineHeight::Normal)),
        ("", "fo:line-height='normal'", None, Some(LineHeight::Normal)),
    ] {
        let input = notes_input(shared, local);
        let presentation = import(&package(&input)).unwrap();
        clean(&presentation);
        assert_eq!(presentation.slides[0].notes.paragraphs[0].props.line_height, expected_local);
        let master = &presentation.notes_masters[0].master;
        assert_eq!(master.shapes[0].text.as_ref().unwrap().paragraphs[0].props.line_height, expected_shared);
        let native = deckcraft_format::load(&deckcraft_format::save(&presentation).unwrap()).unwrap();
        assert_eq!(native, presentation);
        let reopened = import(&export(&native).unwrap()).unwrap();
        clean(&reopened);
        assert_eq!(reopened.slides[0].notes.paragraphs[0].props.line_height, expected_local);
        assert_eq!(reopened.notes_masters[0].master.shapes[0].text.as_ref().unwrap().paragraphs[0].props.line_height, expected_shared);
    }
}

#[test]
fn dormant_local_notes_modifier_keeps_shared_normal_preview_and_precise_source_only_guard() {
    for local in ["style:line-height-at-least='31pt'", "style:line-spacing='-2pt'", "style:line-height-at-least='31pt' style:line-spacing='2pt'"] {
        let bytes = package(&notes_input("fo:line-height='normal'", local));
        let presentation = import(&bytes).unwrap();
        assert!(
            presentation.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("line-height") && value.contains("ownership")),
            "{:?}",
            presentation.tags
        );
        assert_eq!(presentation.slides[0].notes.paragraphs[0].props.line_height, None, "inherited Normal must not become a pinned local Normal");
        let slide = &presentation.slides[0];
        let page = slide.notes_page.as_ref().unwrap();
        let frame = page.shapes.iter().find(|shape| Some(shape.id) == page.body_shape).unwrap();
        let ctx = deckcraft_model::resolve::Ctx::for_notes(&presentation, slide).unwrap();
        // The notes renderer/writer transiently attaches canonical notes and
        // inherits the matching master paragraph before ordinary resolution.
        let mut prepared = slide.notes.paragraphs[0].clone();
        let parent =
            presentation.notes_master_for(slide).unwrap().shapes.iter().find(|shape| shape.ph_type() == Some(deckcraft_model::PhType::Body)).unwrap();
        let parent = parent
            .text
            .as_ref()
            .unwrap()
            .paragraphs
            .iter()
            .find(|paragraph| paragraph.level == prepared.level)
            .or_else(|| parent.text.as_ref().unwrap().paragraphs.first())
            .unwrap();
        prepared.props.inherit(&parent.props);
        assert_eq!(deckcraft_model::resolve::para(&ctx, frame, &prepared).line_height, Some(LineHeight::Normal));
        let native = deckcraft_format::load(&deckcraft_format::save(&presentation).unwrap()).unwrap();
        assert_eq!(native, presentation);
        assert_eq!(source(&native), bytes);
        for edit_notes in [false, true] {
            let mut edited = native.clone();
            if edit_notes {
                Arc::make_mut(&mut edited.slides[0]).notes.paragraphs[0].runs[0].text = "Edited notes".into();
            } else {
                edit(&mut edited);
            }
            let snapshot = edited.clone();
            assert!(export(&edited).is_err(), "dependent source ownership must not be laundered by an unrelated or notes edit");
            assert_eq!(edited, snapshot);
            assert_eq!(source(&edited), bytes);
        }
    }
}

#[test]
fn new_policy_context_cannot_enter_unproved_source_patch_lane_with_another_passive_property() {
    for new_attrs in ["style:line-height-at-least='24pt'", "fo:line-height='normal'"] {
        guarded(&files(
            "1.3",
            "",
            &style("P", None, &format!("{new_attrs} fo:keep-with-next='always'")),
            &frame("<text:p text:style-name='P'>Original</text:p>"),
        ));
    }
    let common = style("Base", None, "style:line-height-at-least='24pt'");
    let automatic = format!("{}{}", style("Unused", Some("Base"), "fo:line-height='120%'"), style("P", None, "fo:keep-with-next='always'"));
    guarded(&files("1.3", &common, &automatic, &frame("<text:p text:style-name='P'>Original</text:p>")));
    let default =
        "<style:default-style style:family='paragraph'><style:paragraph-properties style:line-height-at-least='24pt'/></style:default-style>";
    guarded(&files("1.3", default, &style("Unused", None, "fo:line-height='120%'"), &frame("<text:p>Original</text:p>")));

    let original = files(
        "1.3",
        "",
        &style("P", None, "fo:line-height='120%' fo:keep-with-next='always'"),
        &frame("<text:p text:style-name='P'>Original</text:p>"),
    );
    let mut presentation = import(&package(&original)).unwrap();
    edit(&mut presentation);
    let output = export(&presentation).expect("old numeric line-height plus passive property remains source-preservable");
    let mut expected = original;
    // Changed text is encoded explicitly to preserve ODF whitespace semantics;
    // only that proved body replacement may differ from the original XML bytes.
    replace(
        &mut expected,
        "content.xml",
        ">Original</text:p>",
        ">Actually<dcpatch:s xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>edited</text:p>",
    );
    assert_eq!(parts(&output), expected, "old source-preserving lane retains all payloads and XML gaps exactly");
    let reopened = import(&output).unwrap();
    assert_eq!(body(&reopened).paragraphs[0].text(), "Actually edited");
}
