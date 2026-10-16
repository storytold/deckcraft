//! Independent, contributor-original R05.1 ownership and adversarial source fixtures.
//! Expected pixels are interior solid-shape/background samples; no font is involved.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::*;
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:xlink='http://www.w3.org/1999/xlink'";
const RED: [u8; 4] = [192, 32, 48, 255];
const BLUE: [u8; 4] = [32, 64, 192, 255];
const GREEN: [u8; 4] = [32, 160, 64, 255];

fn zip(content: &str, styles: &str) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in [
        ("mimetype", MIME),
        ("content.xml", content),
        ("styles.xml", styles),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(value.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    z.by_name(name).unwrap().read_to_string(&mut out).unwrap();
    out
}
fn rect(name: &str, style: &str, x: u32) -> String {
    format!("<draw:rect xml:id='{name}' draw:name='{name}' draw:style-name='{style}' svg:x='{x}pt' svg:y='10pt' svg:width='20pt' svg:height='20pt'/>")
}
fn style(name: &str, family: &str, paint: &str) -> String {
    let properties = if family == "graphic" { "graphic" } else { "drawing-page" };
    format!(
        "<style:style style:name='{name}' style:family='{family}'><style:{properties}-properties draw:fill='solid' draw:fill-color='{paint}' draw:stroke='none'/></style:style>"
    )
}
fn master(name: &str, props: &str, artwork: &str) -> String {
    format!("<style:master-page style:name='{name}' style:page-layout-name='Physical' {props}>{artwork}</style:master-page>")
}
fn page(name: &str, owner: &str, props: &str, content: &str) -> String {
    format!("<draw:page draw:name='{name}' draw:master-page-name='{owner}' {props}>{content}</draw:page>")
}
fn package(pages: &str, masters: &str, common: &str, automatic: &str) -> Vec<u8> {
    zip(
        &format!(
            "<office:document-content {NS} office:version='1.3'><office:automatic-styles>{automatic}</office:automatic-styles><office:body><office:presentation>{pages}</office:presentation></office:body></office:document-content>"
        ),
        &format!(
            "<office:document-styles {NS} office:version='1.3'><office:styles>{common}</office:styles><office:automatic-styles><style:page-layout style:name='Physical'><style:page-layout-properties fo:page-width='120pt' fo:page-height='80pt'/></style:page-layout></office:automatic-styles><office:master-styles>{masters}</office:master-styles></office:document-styles>"
        ),
    )
}
fn fixture() -> Vec<u8> {
    package(
        &[
            page("First", "A", "", &rect("local-one", "Blue", 70)),
            page("Second", "A", "", &rect("local-two", "Blue", 70)),
            page("Third", "B", "", &rect("local-three", "Blue", 70)),
        ]
        .concat(),
        &[master("A", "draw:style-name='Paint'", &rect("shared-a", "Red", 10)), master("B", "draw:style-name='Paint'", &rect("shared-b", "Red", 10))]
            .concat(),
        &[style("Red", "graphic", "#c02030"), style("Blue", "graphic", "#2040c0"), style("Paint", "drawing-page", "#f0f0f0")].concat(),
        "",
    )
}
fn pixel(p: &Presentation, slide: usize, x: u32, y: u32) -> [u8; 4] {
    deckcraft_render::render_slide(p, slide, &Default::default()).pixel(x, y)
}
fn no_warnings(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}
fn guarded(bytes: &[u8]) {
    if let Ok(p) = import(bytes) {
        assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"), "unsupported or ambiguous source silently accepted");
    }
}

#[test]
fn edit_reopened_shared_artwork_changes_two_a_pages_and_never_equal_looking_b() {
    let initial = import(&fixture()).unwrap();
    no_warnings(&initial);
    let mut p = import(&export(&initial).unwrap()).unwrap();
    no_warnings(&p);
    assert_eq!(p.masters.len(), 2, "equal artwork is not shared source identity");
    for i in 0..3 {
        assert_eq!(p.slides[i].shapes.len(), 1, "shared artwork must not be copied locally");
        assert_eq!(pixel(&p, i, 15, 15), RED);
        assert_eq!(pixel(&p, i, 75, 15), BLUE);
        assert!(p.master_for(&p.slides[i]).unwrap().1.unwrap().shapes.is_empty());
    }
    let a = p.master_for(&p.slides[0]).unwrap().0.id;
    assert_eq!(p.master_for(&p.slides[1]).unwrap().0.id, a);
    assert_ne!(p.master_for(&p.slides[2]).unwrap().0.id, a);
    let m = Arc::make_mut(p.masters.iter_mut().find(|m| m.id == a).unwrap());
    m.shapes[0].fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(32, 160, 64))));
    m.shapes[0].xfrm.as_mut().unwrap().x = 40.0;
    p = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    let bytes = export(&p).unwrap();
    let content = part(&bytes, "content.xml");
    let styles = part(&bytes, "styles.xml");
    assert!(!content.contains("shared-a"));
    assert!(!content.contains("shared-b"));
    assert_eq!(styles.matches("draw:name=\"shared-a\"").count(), 1);
    assert_eq!(styles.matches("draw:name=\"shared-b\"").count(), 1);
    let p = import(&bytes).unwrap();
    no_warnings(&p);
    for i in 0..2 {
        assert_eq!(pixel(&p, i, 45, 15), GREEN);
        assert_eq!(pixel(&p, i, 15, 15), [240, 240, 240, 255]);
        assert_eq!(pixel(&p, i, 75, 15), BLUE);
    }
    assert_eq!(pixel(&p, 2, 15, 15), RED);
    assert_eq!(pixel(&p, 2, 45, 15), [240, 240, 240, 255]);
}

#[test]
fn reopened_absent_equal_and_clear_paint_have_distinct_live_ownership() {
    let pages =
        [page("Absent", "A", "", ""), page("Equal", "A", "draw:style-name='Equal'", ""), page("Clear", "A", "draw:style-name='Clear'", "")].concat();
    let common = [
        style("Paint", "drawing-page", "#c02030"),
        style("Equal", "drawing-page", "#c02030"),
        "<style:style style:name='Clear' style:family='drawing-page'><style:drawing-page-properties draw:fill='none'/></style:style>".into(),
    ]
    .concat();
    let p = import(&package(&pages, &master("A", "draw:style-name='Paint'", ""), &common, "")).unwrap();
    let mut p = import(&export(&p).unwrap()).unwrap();
    no_warnings(&p);
    assert!(p.slides[0].background.is_none());
    assert!(p.slides[1].background.is_some());
    assert!(matches!(p.slides[2].background, Some(Background::Fill { fill: Fill::None })));
    Arc::make_mut(&mut p.masters[0]).background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(32, 160, 64))) });
    assert_eq!(pixel(&p, 0, 60, 40), GREEN);
    assert_eq!(pixel(&p, 1, 60, 40), RED);
    assert_ne!(pixel(&p, 2, 60, 40), GREEN);
    assert!(matches!(resolve::background(&resolve::Ctx::for_slide(&p, &p.slides[2]).unwrap(), Some(&p.slides[2])).0, Fill::None));
}

#[test]
fn explicit_page_reference_to_master_style_is_still_owned_local_paint() {
    let pages =
        [page("Absent", "A", "", ""), page("SameStyle", "A", "draw:style-name='Paint'", ""), page("ViaParent", "A", "draw:style-name='Child'", "")]
            .concat();
    let common = style("Paint", "drawing-page", "#c02030")
        + "<style:style style:name='Child' style:family='drawing-page' style:parent-style-name='Paint'><style:drawing-page-properties/></style:style>";
    let p = import(&package(&pages, &master("A", "draw:style-name='Paint'", ""), &common, "")).unwrap();
    let mut p = import(&export(&p).unwrap()).unwrap();
    no_warnings(&p);
    assert!(p.slides[0].background.is_none());
    assert!(p.slides[1].background.is_some(), "explicit page style usage proves ownership even when it is also the master's style");
    assert!(p.slides[2].background.is_some(), "style-parent ancestry is distinct from native master inheritance");
    Arc::make_mut(&mut p.masters[0]).background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(32, 160, 64))) });
    assert_eq!(pixel(&p, 0, 60, 40), GREEN);
    assert_eq!(pixel(&p, 1, 60, 40), RED);
    assert_eq!(pixel(&p, 2, 60, 40), RED);
}

#[test]
fn native_layout_background_is_preserved_with_shared_regular_artwork() {
    let mut p = import(&fixture()).unwrap();
    let m = Arc::make_mut(&mut p.masters[0]);
    m.background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(192, 32, 48))) });
    m.layouts[0].background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(32, 64, 192))) });
    assert_eq!(pixel(&p, 0, 60, 50), BLUE);
    // This arc may keep layout paint guarded or materialize it like layout art;
    // a successful export must preserve the actual resolved page paint.
    if let Ok(bytes) = export(&p) {
        let p = import(&bytes).unwrap();
        no_warnings(&p);
        assert_eq!(pixel(&p, 0, 60, 50), BLUE, "layout paint disappeared under shared-master inheritance");
    }
}

#[test]
fn implicit_custom_theme_master_background_is_serialized_as_visible_paint() {
    let mut p = import(&fixture()).unwrap();
    let m = Arc::make_mut(&mut p.masters[0]);
    m.background = None;
    m.theme.colors.set(SchemeSlot::Lt1, Rgba::rgb(192, 32, 48));
    assert_eq!(pixel(&p, 0, 60, 50), RED);
    let p = import(&export(&p).unwrap()).unwrap();
    no_warnings(&p);
    assert_eq!(pixel(&p, 0, 60, 50), RED, "implicit Bg1 must not become the importer's default white");
    assert_eq!(pixel(&p, 1, 60, 50), RED);
    assert!(p.slides[0].background.is_none(), "master paint must remain shared");
}

#[test]
fn inherited_master_background_color_map_remaps_are_preserved_or_refused() {
    for layout_map in [false, true] {
        for explicit_master_paint in [false, true] {
            let mut p = import(&fixture()).unwrap();
            let m = Arc::make_mut(&mut p.masters[0]);
            m.background = explicit_master_paint.then(|| Background::Fill { fill: Fill::solid(ColorRef::scheme(SchemeSlot::Bg1)) });
            m.theme.colors.set(SchemeSlot::Lt1, Rgba::rgb(192, 32, 48));
            m.theme.colors.set(SchemeSlot::Accent1, Rgba::rgb(32, 160, 64));
            let map = vec![("bg1".into(), "accent1".into())];
            if layout_map {
                m.layouts[0].color_map = Some(map);
            } else {
                Arc::make_mut(&mut p.slides[0]).color_map = Some(map);
            }
            assert_eq!(pixel(&p, 0, 60, 50), GREEN);
            if let Ok(bytes) = export(&p) {
                let reopened = import(&bytes).unwrap();
                no_warnings(&reopened);
                assert_eq!(pixel(&reopened, 0, 60, 50), GREEN, "page/layout map resolved through the wrong master owner");
            }
            // A literal local background does not depend on the inherited master's
            // color map. Refusing it would mistake override presence for effect.
            let layout = p.slides[0].layout;
            for slide in p.slides.iter_mut().filter(|slide| slide.layout == layout) {
                Arc::make_mut(slide).background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(32, 64, 192))) });
            }
            let reopened = import(&export(&p).unwrap()).unwrap();
            assert_eq!(pixel(&reopened, 0, 60, 50), BLUE);
        }
    }
}

#[test]
fn same_named_content_automatic_style_cannot_repaint_styles_master() {
    let source = package(
        &page("One", "A", "", &rect("local", "Art", 70)),
        &master("A", "", &rect("shared", "Art", 10)),
        &style("Art", "graphic", "#c02030"),
        &style("Art", "graphic", "#2040c0"),
    );
    if let Ok(p) = import(&source)
        && !p.tags.iter().any(|(key, _)| key == "odp:warning")
    {
        assert_eq!(pixel(&p, 0, 15, 15), RED, "content automatic name leaked into styles.xml master scope");
        assert_eq!(pixel(&p, 0, 75, 15), BLUE);
    }
}

#[test]
fn duplicate_same_scope_style_and_automatic_parent_are_guarded() {
    let pages = page("One", "A", "", "");
    let owners = master("A", "draw:style-name='Paint'", "");
    guarded(&package(&pages, &owners, &[style("Paint", "drawing-page", "#c02030"), style("Paint", "drawing-page", "#2040c0")].concat(), ""));
    let automatic = "<style:style style:name='Paint' style:family='drawing-page' style:parent-style-name='Parent'><style:drawing-page-properties/></style:style><style:style style:name='Parent' style:family='drawing-page'><style:drawing-page-properties draw:fill='solid' draw:fill-color='#c02030'/></style:style>";
    guarded(&package(&pages, &owners, "", automatic));
}

#[test]
fn unused_background_master_and_presentation_template_are_guarded() {
    let pages = page("One", "A", "", "");
    guarded(&package(
        &pages,
        &[master("A", "", ""), master("Unused", "draw:style-name='Paint'", "")].concat(),
        &style("Paint", "drawing-page", "#c02030"),
        "",
    ));
    let template = "<style:presentation-page-layout style:name='UnusedTemplate'><presentation:placeholder presentation:object='title' svg:x='1pt' svg:y='1pt' svg:width='20pt' svg:height='10pt'/></style:presentation-page-layout>";
    guarded(&package(&pages, &master("A", "", ""), "", template));
}

#[test]
fn source_shapes_keep_group_hidden_layer_and_unique_owner_ids() {
    let art = format!(
        "<draw:layer-set><draw:layer draw:name='Invisible' draw:display='none'/></draw:layer-set><draw:g xml:id='group' draw:name='group'>{}{}</draw:g>",
        rect("visible", "Red", 10),
        rect("hidden", "Red", 45).replace("draw:name='hidden'", "draw:name='hidden' draw:layer='Invisible'")
    );
    let p = import(&package(&page("One", "A", "", ""), &master("A", "", &art), &style("Red", "graphic", "#c02030"), "")).unwrap();
    let p = import(&export(&p).unwrap()).unwrap();
    no_warnings(&p);
    assert!(p.slides[0].shapes.is_empty());
    let m = p.master_for(&p.slides[0]).unwrap().0;
    assert_eq!(m.shapes.len(), 1);
    let ShapeKind::Group { children, .. } = &m.shapes[0].kind else {
        panic!("group lost");
    };
    assert_eq!(children.len(), 2);
    assert!(!children[0].hidden);
    assert!(children[1].hidden);
    assert_ne!(children[0].id, children[1].id);
    assert_ne!(children[0].id, m.shapes[0].id);
    assert_eq!(pixel(&p, 0, 15, 15), RED);
    assert_ne!(pixel(&p, 0, 50, 15), RED);
}

#[test]
fn local_layer_definition_cannot_hide_shared_master_artwork() {
    let local = "<draw:layer-set><draw:layer draw:name='SharedLayer' draw:display='none'/></draw:layer-set>";
    let art = rect("shared", "Red", 10).replace("draw:name='shared'", "draw:name='shared' draw:layer='SharedLayer'");
    let p = import(&package(&page("One", "A", "", local), &master("A", "", &art), &style("Red", "graphic", "#c02030"), "")).unwrap();
    no_warnings(&p);
    assert_eq!(pixel(&p, 0, 15, 15), RED, "page-local layer leaked into master scope");
}

#[test]
fn different_master_local_layers_do_not_override_default_layers_for_other_owners() {
    let layer = |display: &str| format!("<draw:layer-set><draw:layer draw:name='ArtLayer' draw:display='{display}'/></draw:layer-set>");
    let art = |name: &str, x| rect(name, "Red", x).replace(&format!("draw:name='{name}'"), &format!("draw:name='{name}' draw:layer='ArtLayer'"));
    let owners = layer("always") + &master("A", "", &art("a", 10)) + &master("B", "", &(layer("none") + &art("b", 10)));
    let pages = page("One", "A", "", &art("local-a", 70)) + &page("Two", "B", "", &art("local-b", 70));
    let p = import(&package(&pages, &owners, &style("Red", "graphic", "#c02030"), "")).unwrap();
    no_warnings(&p);
    for p in [&p, &import(&export(&p).unwrap()).unwrap()] {
        assert_eq!(pixel(p, 0, 15, 15), RED, "B's page-local master layers contaminated A's default layers");
        assert_ne!(pixel(p, 1, 15, 15), RED, "B's own hidden master layer was lost");
        for i in 0..2 {
            assert_eq!(pixel(p, i, 75, 15), RED, "regular local pages must use their default layer set");
        }
    }
}

#[test]
fn duplicate_document_layer_defaults_and_master_containers_are_rejected() {
    let defaults = "<draw:layer-set><draw:layer draw:name='Visible' draw:display='always'/></draw:layer-set><draw:layer-set><draw:layer draw:name='Hidden' draw:display='none'/></draw:layer-set>";
    let bytes = package(&page("One", "A", "", ""), &(defaults.to_string() + &master("A", "", "")), "", "");
    assert!(import(&bytes).is_err(), "two default layer sets must not select the first silently");
    let bytes = package(&page("One", "A", "", ""), &master("A", "", ""), "", "");
    let styles = part(&bytes, "styles.xml").replace("</office:master-styles>", "</office:master-styles><office:master-styles/>");
    assert!(import(&zip(&part(&bytes, "content.xml"), &styles)).is_err(), "two structural master containers must not be merged silently");
}

#[test]
fn invalid_visibility_missing_parents_and_duplicate_owner_names_refuse() {
    let pages = page("One", "A", "", "");
    for common in [
        "<style:style style:name='Paint' style:family='drawing-page'><style:drawing-page-properties presentation:background-objects-visible='TRUE'/></style:style>",
        "<style:style style:name='Paint' style:family='drawing-page' style:parent-style-name='Missing'/>",
        "<style:style style:name='Paint' style:family='drawing-page' style:parent-style-name='Paint'/>",
    ] {
        assert!(import(&package(&pages, &master("A", "draw:style-name='Paint'", ""), common, "")).is_err());
    }
    assert!(import(&package(&pages, &[master("A", "", ""), master("A", "", "")].concat(), "", "")).is_err());
    assert!(import(&package(&page("One", "Missing", "", ""), &master("A", "", ""), "", "")).is_err());
}

#[test]
fn nested_master_definition_cannot_masquerade_as_a_structural_master_owner() {
    // Known namespace/name alone is not proof that a definition belongs to the
    // document's office:master-styles inventory.
    guarded(&package(&page("One", "Nested", "", ""), "", &master("Nested", "", ""), ""));
}

#[test]
fn missing_graphic_style_and_missing_graphic_parent_never_silently_default() {
    for common in ["", "<style:style style:name='Missing' style:family='graphic' style:parent-style-name='Absent'/>"] {
        guarded(&package(&page("One", "A", "", ""), &master("A", "", &rect("art", "Missing", 10)), common, ""));
    }
}

#[test]
fn implicit_master_visibility_defaults_stay_guarded() {
    let default = "<style:default-style style:family='drawing-page'><style:drawing-page-properties presentation:background-objects-visible='false'/></style:default-style>";
    guarded(&package(
        &page("One", "A", "", ""),
        &master("A", "", &rect("art", "Red", 10)),
        &[default.into(), style("Red", "graphic", "#c02030")].concat(),
        "",
    ));
}

#[test]
fn native_shared_master_placeholders_are_not_silently_dropped() {
    let mut p = import(&fixture()).unwrap();
    Arc::make_mut(&mut p.masters[0]).shapes.push(Shape {
        id: ShapeId(70_001),
        ph: Some(Placeholder { kind: PhType::Title, ..Default::default() }),
        text: Some(TextBody::from_text("Owned master placeholder")),
        xfrm: Some(Xfrm::new(2.0, 35.0, 90.0, 20.0)),
        ..Default::default()
    });
    assert!(export(&p).is_err(), "R05.2 ownership is unsupported; filtering placeholders loses native data");
}

#[test]
fn cross_owner_numeric_ids_have_unique_xml_ids_and_local_animation_targets() {
    let mut p = import(&fixture()).unwrap();
    let id = p.masters[0].shapes[0].id;
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes[0].id = id;
    slide.animations.push(Animation { shape: id, ..Default::default() });
    let bytes = export(&p).unwrap();
    let content = part(&bytes, "content.xml");
    let styles = part(&bytes, "styles.xml");
    let xml = format!("{content}{styles}");
    let ids: Vec<_> = xml.split("xml:id=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "owner-qualified output IDs must be globally unique");
    assert!(content.contains(&format!("smil:targetElement=\"slide{}-shape{}\"", p.slides[0].id.0, id.0)));
    let p = import(&bytes).unwrap();
    no_warnings(&p);
    assert_eq!(p.slides[0].animations[0].shape, p.slides[0].shapes[0].id);
    assert_ne!(p.slides[0].animations[0].shape, p.master_for(&p.slides[0]).unwrap().0.shapes[0].id);
}

#[test]
fn duplicate_source_xml_ids_inside_styles_part_are_rejected() {
    let a = rect("first-art", "Red", 10).replace("xml:id='first-art'", "xml:id='duplicate'");
    let b = rect("second-art", "Red", 10).replace("xml:id='second-art'", "xml:id='duplicate'");
    let owners = [master("A", "", &a), master("B", "", &b)].concat();
    let pages = [page("One", "A", "", ""), page("Two", "B", "", "")].concat();
    assert!(import(&package(&pages, &owners, &style("Red", "graphic", "#c02030"), "")).is_err(), "xml:id uniqueness is mandatory within styles.xml");
}

#[test]
fn equal_source_xml_ids_in_separate_xml_parts_keep_distinct_owners() {
    let shared = rect("shared", "Red", 10).replace("xml:id='shared'", "xml:id='part-local'");
    let local = rect("local", "Blue", 70).replace("xml:id='local'", "xml:id='part-local'");
    let source = package(
        &page("One", "A", "", &local),
        &master("A", "", &shared),
        &[style("Red", "graphic", "#c02030"), style("Blue", "graphic", "#2040c0")].concat(),
        "",
    );
    let p = import(&source).unwrap();
    no_warnings(&p);
    let master_art = &p.master_for(&p.slides[0]).unwrap().0.shapes[0];
    let local_art = &p.slides[0].shapes[0];
    assert_ne!(master_art.id, local_art.id);
    for shape in [master_art, local_art] {
        assert!(shape.tags.iter().any(|(key, value)| key == "odp:id" && value == "part-local"));
    }
    assert!(master_art.tags.iter().any(|(key, value)| key == "odp:source-part" && value == "styles.xml"));
    assert!(local_art.tags.iter().any(|(key, value)| key == "odp:source-part" && value == "content.xml"));
    let p = import(&export(&p).unwrap()).unwrap();
    no_warnings(&p);
    assert_eq!(pixel(&p, 0, 15, 15), RED);
    assert_eq!(pixel(&p, 0, 75, 15), BLUE);
}

#[test]
fn many_source_masters_and_deep_shared_groups_are_bounded() {
    let owners: String = (0..10_001).map(|i| master(&format!("M{i}"), "", "")).collect();
    assert!(import(&package(&page("One", "M0", "", ""), &owners, "", "")).is_err());
    let mut art = rect("art", "Red", 10);
    for _ in 0..70 {
        art = format!("<draw:g>{art}</draw:g>");
    }
    assert!(import(&package(&page("One", "A", "", ""), &master("A", "", &art), &style("Red", "graphic", "#c02030"), "")).is_err());
}

#[test]
fn repeated_scoped_style_copies_charge_actual_key_bytes_before_cloning() {
    // 600 KiB of legal (<8192-byte) style keys, copied into 256 referenced master
    // inventories, exceeds the cumulative 128 MiB work/allocation budget.
    // The package itself is small and has no shape/media/font expansion.
    let common: String = (0..100).map(|i| style(&format!("S{i}{}", "x".repeat(6000)), "graphic", "#c02030")).collect();
    let owners: String = (0..256).map(|i| master(&format!("M{i}"), "", "")).collect();
    let pages: String = (0..256).map(|i| page(&format!("P{i}"), &format!("M{i}"), "", "")).collect();
    assert!(import(&package(&pages, &owners, &common, "")).is_err(), "repeated scoped style-map work escaped cumulative accounting");
}

#[test]
fn repeated_page_property_value_copies_are_cumulatively_precharged() {
    // These legal resource-name strings are inactive under draw:fill='none'.
    // They still cost 24 KiB whenever an effective/local property map is cloned.
    // Four thousand pages exceed 128 MiB across preflight and page conversion;
    // only charging the number of properties misses this amplification.
    let name = "x".repeat(8000);
    let common = format!(
        "<style:style style:name='Local' style:family='drawing-page'><style:drawing-page-properties draw:fill='none' draw:fill-gradient-name='{name}' draw:fill-hatch-name='{name}' draw:fill-image-name='{name}'/></style:style>"
    );
    let pages: String = (0..4000).map(|i| page(&format!("P{i}"), "A", "draw:style-name='Local'", "")).collect();
    let error = import(&package(&pages, &master("A", "", ""), &common, "")).unwrap_err().to_string();
    assert!(error.contains("budget") || error.contains("limit"), "expected cumulative budget refusal, got {error}");
}

#[test]
fn zero_pages_and_one_empty_master_are_safe() {
    assert!(import(&package("", "", "", "")).is_ok());
    let p = import(&package(&page("One", "A", "", ""), &master("A", "", ""), "", "")).unwrap();
    no_warnings(&p);
    assert_eq!(p.masters.len(), 1);
    assert_eq!(p.masters[0].layouts.len(), 1);
    assert!(p.masters[0].shapes.is_empty());
}

#[test]
fn regular_artwork_internal_hyperlink_still_targets_the_correct_page() {
    let linked = format!("<draw:a xlink:href='#Third'>{}</draw:a>", rect("linked", "Red", 10));
    let bytes = package(
        &[page("First", "A", "", ""), page("Third", "B", "", "")].concat(),
        &[master("A", "", &linked), master("B", "", "")].concat(),
        &style("Red", "graphic", "#c02030"),
        "",
    );
    let p = import(&bytes).unwrap();
    let p = import(&export(&p).unwrap()).unwrap();
    no_warnings(&p);
    let click = &p.master_for(&p.slides[0]).unwrap().0.shapes[0].click.as_ref().unwrap().action;
    assert_eq!(click, &text::Action::Slide { slide: p.slides[1].id });
}

#[test]
fn one_notes_owner_cannot_be_duplicated_across_two_distinct_regular_masters() {
    let mut p = import(&fixture()).unwrap();
    p.notes_masters.push(notes::NotesMaster { master: Arc::new(Master { id: MasterId(80_000), ..Default::default() }), ..Default::default() });
    for slide in &mut p.slides {
        Arc::make_mut(slide).notes_page = Some(notes::NotesPage { master: Some(MasterId(80_000)), ..Default::default() });
    }
    assert!(export(&p).is_err(), "ODF pairing would duplicate a live notes owner");
}

#[test]
fn ordinary_master_notes_pairing_keeps_distinct_shared_ids_and_local_bodies() {
    let notes_body = |text: &str| {
        format!(
            "<draw:frame presentation:class='notes' presentation:user-transformed='true' svg:x='10pt' svg:y='30pt' svg:width='100pt' svg:height='30pt'><draw:text-box><text:p>{text}</text:p></draw:text-box></draw:frame>"
        )
    };
    let note_page = |text: &str| format!("<presentation:notes>{}</presentation:notes>", notes_body(text));
    let notes_master = format!("<presentation:notes style:page-layout-name='Physical'>{}</presentation:notes>", notes_body(""));
    let owners = [master("A", "", &(rect("a", "Red", 10) + &notes_master)), master("B", "", &(rect("b", "Red", 10) + &notes_master))].concat();
    let pages = [
        page("One", "A", "", &note_page("One body")),
        page("Two", "A", "", &note_page("Two body")),
        page("Three", "B", "", &note_page("Three body")),
    ]
    .concat();
    let p = import(&package(&pages, &owners, &style("Red", "graphic", "#c02030"), "")).unwrap();
    let p = import(&export(&p).unwrap()).unwrap();
    no_warnings(&p);
    assert_eq!(p.masters.len(), 2);
    assert_eq!(p.notes_masters.len(), 2);
    let note_id = |i: usize| p.slides[i].notes_page.as_ref().unwrap().master.unwrap();
    assert_eq!(note_id(0), note_id(1));
    assert_ne!(note_id(0), note_id(2));
    for (i, expected) in ["One body", "Two body", "Three body"].iter().enumerate() {
        assert_eq!(p.slides[i].notes_text(), *expected);
    }
}
