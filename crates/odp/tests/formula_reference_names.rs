//! Contributor-original, fontless ODF 1.3 §19.171 equation-reference fixtures.
//! This slice supports Unicode alphanumeric names plus underscore, not every ODF name.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_geom::{KurboShape as _, Rect};
use deckcraft_model::{Geom, Presentation, ShapeKind};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

fn equation(name: &str, formula: &str) -> String {
    // All callers supply our own literal XML-safe strings, never external input.
    format!("<draw:equation draw:name='{name}' draw:formula='{formula}'/>")
}

fn package(equations: &str, path: &str, modifiers: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' office:version='1.3'><office:automatic-styles><style:style style:name='red' style:family='graphic'><style:graphic-properties draw:fill='solid' draw:fill-color='#ff0000' draw:stroke='none'/></style:style><style:style style:name='white' style:family='drawing-page'><style:drawing-page-properties draw:fill='solid' draw:fill-color='#ffffff'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:style-name='white'><draw:custom-shape draw:style-name='red' svg:x='20pt' svg:y='30pt' svg:width='200pt' svg:height='100pt'><draw:enhanced-geometry svg:viewBox='0 0 100 100' draw:modifiers='{modifiers}' draw:enhanced-path='{path}'>{equations}</draw:enhanced-geometry></draw:custom-shape></draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", content.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn rectangle(right: &str) -> String {
    format!("M 0 0 L {right} 0 {right} 100 0 100 Z N")
}

fn editable(bytes: &[u8]) -> Presentation {
    let p = import(bytes).unwrap();
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
    assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Shape));
    assert!(matches!(p.slides[0].shapes[0].geom, Geom::Custom { .. }));
    assert!(p.slides[0].shapes[0].text.is_none(), "fixture must remain fontless");
    p
}

fn assert_rectangle(p: &Presentation, width: f64) {
    let shape = &p.slides[0].shapes[0];
    let x = shape.xfrm.as_ref().unwrap();
    assert_eq!((x.x, x.y, x.w, x.h), (20.0, 30.0, 200.0, 100.0));
    let g = deckcraft_render::shape_geometry(shape, x.w, x.h);
    assert_eq!(g.paths.len(), 1);
    assert_eq!(g.paths[0].path.bounding_box(), Rect::new(0.0, 0.0, width, 100.0));
    assert!((g.paths[0].path.area().abs() - width * 100.0).abs() < 1e-8);
    let image = deckcraft_render::render_slide(p, 0, &deckcraft_render::RenderOpts { size: Some((240, 160)), ..Default::default() });
    assert_eq!(image.pixel(60, 80), [255, 0, 0, 255]);
    assert_eq!(image.pixel(180, 80), [255, 255, 255, 255]);
    if width == 150.0 {
        assert_eq!(image.pixel(160, 80), [255, 0, 0, 255]);
        assert_eq!(image.pixel(140, 80), [255, 0, 0, 255]);
    } else {
        assert_eq!(width, 100.0);
        assert_eq!(image.pixel(140, 80), [255, 255, 255, 255]);
    }
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}

#[test]
fn unicode_and_chained_mixed_names_match_literal_geometry_and_pixels() {
    let literal = editable(&package("", &rectangle("75"), ""));
    assert_rectangle(&literal, 150.0);
    let expected = &literal.slides[0].shapes[0].geom;
    for name in ["補正", "幅2", "α", "коррекция", "guide_幅2"] {
        let direct = editable(&package(&equation(name, "75"), &rectangle(&format!("?{name}")), ""));
        assert_eq!(&direct.slides[0].shapes[0].geom, expected, "{name}");
        assert_rectangle(&direct, 150.0);
        let equations = format!("{}{}", equation(name, "75"), equation("終点", &format!("max(?{name},50)")));
        let chained = editable(&package(&equations, &rectangle("?終点"), ""));
        assert_eq!(&chained.slides[0].shapes[0].geom, expected, "chain {name}");
        assert_rectangle(&chained, 150.0);
    }
}

#[test]
fn unicode_geometry_survives_native_and_odp_reopen_and_real_path_edit() {
    let p = editable(&package(&equation("補正", "75"), &rectangle("?補正"), ""));
    let native = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    assert_eq!(native.slides[0].shapes[0].geom, p.slides[0].shapes[0].geom);
    assert_rectangle(&native, 150.0);
    let bytes = export(&native).unwrap();
    let reopened = editable(&bytes);
    assert_rectangle(&reopened, 150.0);
    let content = part(&bytes, "content.xml");
    assert!(content.contains("svg:viewBox=\"0 0 1000 1000\""));
    assert!(content.contains("750"), "normalized path must contain the evaluated right edge");
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_FORMULA_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(name), part(&bytes, name)).unwrap();
        }
    }

    let mut edited = native;
    let Geom::Custom { paths } = &mut Arc::make_mut(&mut edited.slides[0]).shapes[0].geom else { panic!("custom path") };
    assert_eq!((paths[0].w, paths[0].h), (100.0, 100.0));
    paths[0].d = "M 0 0 L 50 0 L 50 100 L 0 100 Z".into();
    assert_rectangle(&edited, 100.0);
    let edited_bytes = export(&edited).unwrap();
    assert_rectangle(&editable(&edited_bytes), 100.0);
    assert_ne!(part(&edited_bytes, "content.xml"), content);
    let native_edited = deckcraft_format::load(&deckcraft_format::save(&edited).unwrap()).unwrap();
    assert_rectangle(&native_edited, 100.0);
}

#[test]
fn exact_case_and_distinct_unicode_names_do_not_alias() {
    let equations = format!("{}{}{}{}", equation("幅", "25"), equation("巾", "75"), equation("Guide", "25"), equation("guide", "75"));
    for formula in ["?巾", "?guide", "?幅 + ?巾 - ?幅", "?Guide + ?guide - ?Guide"] {
        let equations = format!("{equations}{}", equation("結果", formula));
        assert_rectangle(&editable(&package(&equations, &rectangle("?結果"), "")), 150.0);
    }
}

fn assert_guarded(equations: &str, path: &str, modifiers: &str) -> Presentation {
    let source = package(equations, path, modifiers);
    let p = import(&source).unwrap();
    assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Opaque { .. }));
    assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), source);
    assert!(export(&p).is_err(), "unsupported geometry must not become a fresh editable export");
    p
}

fn assert_formula_limit(equations: &str, path: &str, expected: &str) {
    let p = assert_guarded(equations, path, "");
    assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains(expected)), "expected {expected}: {:?}", p.tags);
}

#[test]
fn duplicate_equation_names_are_refused_instead_of_rebound() {
    for name in ["補正", "guide"] {
        let equations = format!("{}{}", equation(name, "25"), equation(name, "75"));
        assert_guarded(&equations, &rectangle(&format!("?{name}")), "");
    }
}

#[test]
fn unknown_cyclic_bare_and_unsupported_names_remain_recoverable() {
    for (equations, reference) in [
        (equation("補正", "75"), "?未知"),
        (format!("{}{}", equation("補正", "?循環"), equation("循環", "?補正")), "?補正"),
        (equation("補正", "75"), "?"),
        (equation("幅-右", "75"), "?幅-右"),
        (equation("e\u{301}", "75"), "?e\u{301}"),
    ] {
        assert_guarded(&equations, &rectangle(reference), "");
    }
    assert_guarded(&equation("結果", "?GUIDE"), &rectangle("?結果"), "");
}

#[test]
fn numeric_modifier_indices_keep_their_ascii_integer_guard() {
    assert_rectangle(&editable(&package(&equation("補正", "$0"), &rectangle("?補正"), "75")), 150.0);
    for reference in ["$１", "$١", "$0幅", "$1", "$999999999999999999999999999999999999999999999999"] {
        assert_guarded(&equation("補正", reference), &rectangle("?補正"), "75");
        assert_guarded("", &rectangle(reference), "75");
    }
}

#[test]
fn unicode_references_retain_depth_equation_and_fuel_limits() {
    let mut equations = String::new();
    for i in 0..40 {
        equations.push_str(&equation(&format!("段{i}"), &format!("?段{}", i + 1)));
    }
    equations.push_str(&equation("段40", "75"));
    assert_formula_limit(&equations, &rectangle("?段0"), "geometry formula complexity exceeds limit");

    let equations = (0..257).map(|i| equation(&format!("幅{i}"), "75")).collect::<String>();
    assert_formula_limit(&equations, &rectangle("?幅0"), "too many geometry equations");

    // Each attribute stays below the XML 8192-byte cap. Repeated evaluation of
    // this individually bounded expression must exhaust the shared formula fuel.
    let expensive = format!("1{}", "+1".repeat(200));
    let path = format!("M 0 0 {} N", "L ?幅 0 ".repeat(600));
    assert!(path.len() < 8192);
    assert_formula_limit(&equation("幅", &expensive), &path, "geometry formula operation budget exceeded");
}

#[test]
fn oversized_reference_path_is_refused_by_the_xml_attribute_budget() {
    // The XML boundary intentionally rejects this before formula substitution.
    // Its deeper 4096-reference guard is tested directly inside read_formula.rs.
    let path = format!("M 0 0 {} N", "L ?補正 0 ".repeat(4097));
    assert!(path.len() > 8192);
    let error = import(&package(&equation("補正", "75"), &path, "")).unwrap_err();
    assert_eq!(error.0, "XML attributes exceed size limit");
}

#[test]
fn nonfinite_excessive_and_malformed_equation_values_stay_guarded() {
    for formula in ["1/0", "sqrt(-1)", "1e999", "1000000000", "999999999*999999999", "1e+", ".e3"] {
        assert_guarded(&equation("補正", formula), &rectangle("?補正"), "");
    }
}

#[test]
fn ascii_arithmetic_functions_and_lazy_if_keep_their_existing_results() {
    for formula in [
        "75",
        "+7.5e+1",
        "1+2*37",
        "-(25)+100",
        "abs(-75)",
        "sqrt(5625)",
        "min(75,100)",
        "max(25,75)",
        "75+sin(0)+tan(0)+atan(0)+atan2(0,1)",
        "75*cos(0)",
        "if(1,75,1/0)",
        "if(0,sqrt(-1),75)",
        "if(1,75,?guide)",
    ] {
        let p = editable(&package(&equation("guide", formula), &rectangle("?guide"), ""));
        assert_rectangle(&p, 150.0);
    }
    let equations = format!("{}{}", equation("幅", "75"), equation("結果", "if(1,?幅,?結果)"));
    assert_rectangle(&editable(&package(&equations, &rectangle("?結果"), "")), 150.0);
}
