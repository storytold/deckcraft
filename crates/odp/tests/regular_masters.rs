//! Contributor-original regular-master sharing, paint ownership and refusal fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use deckcraft_model::*;
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0'";
fn shape(name: &str, style: &str) -> String {
    format!(
        "<draw:rect draw:name='{name}' draw:style-name='{style}' svg:x='20pt' svg:y='20pt' svg:width='80pt' svg:height='40pt'><text:p>{name}</text:p></draw:rect>"
    )
}
fn package(content: &str, styles: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", content),
        ("styles.xml", styles),
        (
            "META-INF/manifest.xml",
            "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='application/vnd.oasis.opendocument.presentation'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/><manifest:file-entry manifest:full-path='styles.xml' manifest:media-type='text/xml'/></manifest:manifest>",
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn source(extra_page: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page draw:name='Inherited' draw:master-page-name='A'>{}</draw:page><draw:page draw:name='Explicit' draw:master-page-name='A' draw:style-name='EqualA'>{}</draw:page><draw:page draw:name='Other' draw:master-page-name='B'>{}</draw:page>{extra_page}</office:presentation></office:body></office:document-content>",
        shape("LocalOne", "Art"),
        shape("LocalTwo", "Art"),
        shape("LocalThree", "Art")
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles><style:style style:name='Art' style:family='graphic'><style:graphic-properties draw:fill='solid' draw:fill-color='#234567' draw:stroke='none'/><style:text-properties fo:font-family='BIZ UDPGothic'/></style:style><style:style style:name='PaintA' style:family='drawing-page'><style:drawing-page-properties draw:fill='solid' draw:fill-color='#abcdef'/></style:style><style:style style:name='PaintB' style:family='drawing-page'><style:drawing-page-properties draw:fill='solid' draw:fill-color='#fedcba'/></style:style><style:style style:name='EqualA' style:family='drawing-page'><style:drawing-page-properties draw:fill='solid' draw:fill-color='#abcdef'/></style:style><style:style style:name='Clear' style:family='drawing-page'><style:drawing-page-properties draw:fill='none'/></style:style></office:styles><office:automatic-styles><style:page-layout style:name='Physical'><style:page-layout-properties fo:page-width='320pt' fo:page-height='180pt'/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name='A' style:page-layout-name='Physical' draw:style-name='PaintA'>{}</style:master-page><style:master-page style:name='B' style:page-layout-name='Physical' draw:style-name='PaintB'>{}</style:master-page></office:master-styles></office:document-styles>",
        shape("SharedA", "Art"),
        shape("SharedB", "Art")
    );
    package(&content, &styles)
}
fn text_in(p: &Presentation, slide: usize) -> Vec<String> {
    p.slides[slide].shapes.iter().filter_map(|shape| shape.text.as_ref().map(TextBody::text)).collect()
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}
fn no_warning(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}

#[test]
fn two_a_slides_share_one_live_master_and_b_remains_independent_after_native_and_odp() {
    let mut p = import(&source("")).unwrap();
    no_warning(&p);
    assert_eq!(p.masters.len(), 2);
    assert_eq!(p.master_for(&p.slides[0]).unwrap().0.id, p.master_for(&p.slides[1]).unwrap().0.id);
    assert_ne!(p.master_for(&p.slides[0]).unwrap().0.id, p.master_for(&p.slides[2]).unwrap().0.id);
    for i in 0..3 {
        assert_eq!(p.slides[i].shapes.len(), 1);
        assert!(!text_in(&p, i).iter().any(|text| text.starts_with("Shared")));
    }
    assert_eq!(p.masters[0].shapes.len(), 1);
    assert!(p.masters[0].layouts[0].show_master_shapes);
    assert!(p.masters[0].shapes[0].tags.iter().any(|(key, value)| key == "odp:source-part" && value == "styles.xml"));
    let before: Vec<_> = (0..3).map(|i| deckcraft_render::render_slide(&p, i, &Default::default()).pixels).collect();
    let id = p.master_for(&p.slides[0]).unwrap().0.id;
    let master = Arc::make_mut(p.masters.iter_mut().find(|master| master.id == id).unwrap());
    master.shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].text = "EditedShared".into();
    master.shapes[0].fill = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(231, 42, 89))));
    master.shapes[0].xfrm.as_mut().unwrap().x += 40.0;
    p = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
    assert_eq!(p.masters.len(), 2);
    for (i, old) in before.iter().enumerate() {
        assert_eq!(deckcraft_render::render_slide(&p, i, &Default::default()).pixels == *old, i == 2);
    }
    let output = export(&p).unwrap();
    assert!(!part(&output, "content.xml").contains("EditedShared"));
    assert_eq!(part(&output, "styles.xml").matches("EditedShared").count(), 1);
    let reopened = import(&output).unwrap();
    no_warning(&reopened);
    assert_eq!(reopened.masters.len(), 2);
    assert_eq!(reopened.master_for(&reopened.slides[0]).unwrap().0.id, reopened.master_for(&reopened.slides[1]).unwrap().0.id);
    assert_eq!(reopened.master_for(&reopened.slides[0]).unwrap().0.shapes[0].text.as_ref().unwrap().text(), "EditedShared");
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_MASTER_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml", "META-INF/manifest.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(name.replace('/', "-")), part(&output, name)).unwrap();
        }
    }
}

#[test]
fn absent_equal_local_and_explicit_no_fill_keep_distinct_background_ownership() {
    let mut p = import(&source("<draw:page draw:name='Clear' draw:master-page-name='A' draw:style-name='Clear'/>")).unwrap();
    no_warning(&p);
    assert!(p.slides[0].background.is_none());
    assert!(p.slides[1].background.is_some());
    assert!(matches!(p.slides[3].background, Some(Background::Fill { fill: Fill::None })));
    let master = Arc::make_mut(&mut p.masters[0]);
    master.background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(1, 2, 3))) });
    let encoded = export(&p).unwrap();
    let reopened = import(&encoded).unwrap();
    no_warning(&reopened);
    assert!(reopened.slides[0].background.is_none());
    assert!(reopened.slides[1].background.is_some());
    assert!(matches!(reopened.slides[3].background, Some(Background::Fill { fill: Fill::None })));
    let resolved = |index: usize| {
        let ctx = resolve::Ctx::for_slide(&reopened, reopened.slides[index].as_ref()).unwrap();
        resolve::background(&ctx, Some(reopened.slides[index].as_ref())).0
    };
    assert_ne!(resolved(0), resolved(1));
    assert!(matches!(resolved(3), Fill::None));
}

#[test]
fn duplicate_missing_invalid_and_cyclic_master_references_refuse() {
    let original = source("");
    let content = part(&original, "content.xml");
    let styles = part(&original, "styles.xml");
    for changed in [
        styles.replace("style:name='B'", "style:name='A'"),
        styles.replace("style:page-layout-name='Physical'", "style:page-layout-name='Missing'"),
        styles.replace("fo:page-width='320pt'", "fo:page-width='NaNpt'"),
        styles.replace(
            "style:name='PaintA' style:family='drawing-page'",
            "style:name='PaintA' style:family='drawing-page' style:parent-style-name='PaintA'",
        ),
    ] {
        assert!(import(&package(&content, &changed)).is_err());
    }
    assert!(import(&package(&content.replace("draw:master-page-name='A'", "draw:master-page-name='Missing'"), &styles)).is_err());
}

#[test]
fn unsupported_regular_placeholders_stay_guarded_instead_of_claiming_live_ownership() {
    let original = source("");
    let content = part(&original, "content.xml");
    let styles = part(&original, "styles.xml").replace("draw:name='SharedA'", "draw:name='SharedA' presentation:class='title'");
    let p = import(&package(&content, &styles)).unwrap();
    assert!(p.tags.iter().any(|(key, value)| key == "odp:warning" && value.contains("regular master")));
    assert!(export(&p).is_err());
    let original = source("");
    let content = part(&original, "content.xml");
    let styles = part(&original, "styles.xml");
    for style in [
        styles.replace("draw:fill-color='#abcdef'/>", "draw:fill-color='#abcdef' presentation:display-footer='false'/>"),
        styles.replace(
            "</office:master-styles>",
            "<style:master-page style:name='UnusedPaint' style:page-layout-name='Physical' draw:style-name='PaintA'/></office:master-styles>",
        ),
    ] {
        let p = import(&package(&content, &style)).unwrap();
        assert!(p.tags.iter().any(|(key, _)| key == "odp:warning"));
        assert!(export(&p).is_err());
    }
    let styles = styles.replace("</office:styles>", "<style:presentation-page-layout style:name='UnusedTemplate'><presentation:placeholder presentation:object='title' svg:x='1pt' svg:y='1pt' svg:width='80pt' svg:height='20pt'/></style:presentation-page-layout></office:styles>");
    let p = import(&package(&content, &styles)).unwrap();
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
    assert_eq!(p.presentation_templates.len(), 1);
    let back = import(&export(&p).unwrap()).unwrap();
    assert_eq!(back.presentation_templates[0].name, "UnusedTemplate");
    assert_eq!(back.presentation_templates[0].slots[0].role, PresentationRole::Title);
}

#[test]
fn shared_native_master_cannot_split_across_notes_owners_or_dangling_layouts() {
    let mut p = import(&source("")).unwrap();
    p.notes_masters = vec![
        notes::NotesMaster { master: Arc::new(Master { id: MasterId(80_000), ..Default::default() }), ..Default::default() },
        notes::NotesMaster { master: Arc::new(Master { id: MasterId(80_001), ..Default::default() }), ..Default::default() },
    ];
    for (i, id) in [80_000, 80_001].into_iter().enumerate() {
        Arc::make_mut(&mut p.slides[i]).notes_page = Some(notes::NotesPage { master: Some(MasterId(id)), ..Default::default() });
    }
    assert!(export(&p).unwrap_err().to_string().contains("multiple notes masters"));
    let mut p = import(&source("")).unwrap();
    Arc::make_mut(&mut p.slides[0]).layout = LayoutId(99_999);
    assert!(export(&p).is_err());
    let mut p = import(&source("")).unwrap();
    let duplicate = p.masters[0].shapes[0].clone();
    Arc::make_mut(&mut p.masters[0]).shapes.push(duplicate);
    assert!(export(&p).is_err());
    let mut p = import(&source("")).unwrap();
    Arc::make_mut(&mut p.masters[0]).shapes[0].ph = Some(Placeholder { kind: PhType::Title, ..Default::default() });
    assert!(export(&p).unwrap_err().to_string().contains("R05.2"));
}

fn themed_background() -> Presentation {
    let mut p = import(&source("")).unwrap();
    let master = Arc::make_mut(&mut p.masters[0]);
    master.background = None;
    master.theme.colors.set(SchemeSlot::Lt1, Rgba::rgb(13, 29, 47));
    master.theme.colors.set(SchemeSlot::Dk1, Rgba::rgb(201, 37, 91));
    p
}
fn corner(p: &Presentation, index: usize) -> Vec<u8> {
    deckcraft_render::render_slide(p, index, &Default::default()).pixels.get(..4).unwrap().to_vec()
}

#[test]
fn implicit_custom_theme_paint_is_saved_once_on_the_shared_master() {
    let p = themed_background();
    assert_eq!(corner(&p, 0), vec![13, 29, 47, 255]);
    let output = export(&p).unwrap();
    let mut reopened = import(&output).unwrap();
    no_warning(&reopened);
    assert!(reopened.slides[0].background.is_none());
    assert_eq!(corner(&reopened, 0), vec![13, 29, 47, 255]);
    let explicit = corner(&reopened, 1);
    Arc::make_mut(&mut reopened.masters[0]).background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(7, 8, 9))) });
    assert_eq!(corner(&reopened, 0), vec![7, 8, 9, 255]);
    assert_eq!(corner(&reopened, 1), explicit);
}

#[test]
fn only_effective_inherited_color_remaps_refuse_and_local_paint_stays_owned() {
    let mut changed = themed_background();
    Arc::make_mut(&mut changed.slides[0]).color_map = Some(vec![("bg1".into(), "dk1".into())]);
    assert_eq!(corner(&changed, 0), vec![201, 37, 91, 255]);
    assert!(export(&changed).unwrap_err().to_string().contains("color-map"));

    let mut layout = themed_background();
    Arc::make_mut(&mut layout.masters[0]).layouts[0].color_map = Some(vec![("bg1".into(), "dk1".into())]);
    assert!(export(&layout).unwrap_err().to_string().contains("color-map"));

    let mut unrelated = themed_background();
    Arc::make_mut(&mut unrelated.slides[0]).color_map = Some(vec![("tx1".into(), "dk2".into())]);
    let reopened = import(&export(&unrelated).unwrap()).unwrap();
    no_warning(&reopened);
    assert_eq!(corner(&reopened, 0), corner(&unrelated, 0));

    let mut equal_color = themed_background();
    Arc::make_mut(&mut equal_color.masters[0]).theme.colors.set(SchemeSlot::Dk1, Rgba::rgb(13, 29, 47));
    Arc::make_mut(&mut equal_color.slides[0]).color_map = Some(vec![("bg1".into(), "dk1".into())]);
    let reopened = import(&export(&equal_color).unwrap()).unwrap();
    no_warning(&reopened);
    assert_eq!(corner(&reopened, 0), corner(&equal_color, 0));

    let mut local = themed_background();
    Arc::make_mut(&mut local.slides[1]).color_map = Some(vec![("bg1".into(), "dk1".into())]);
    let reopened = import(&export(&local).unwrap()).unwrap();
    no_warning(&reopened);
    assert!(reopened.slides[1].background.is_some());
    assert_eq!(corner(&reopened, 1), corner(&local, 1));

    let mut layout_local = themed_background();
    let layout = &mut Arc::make_mut(&mut layout_local.masters[0]).layouts[0];
    layout.color_map = Some(vec![("bg1".into(), "dk1".into())]);
    layout.background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(5, 17, 29))) });
    let reopened = import(&export(&layout_local).unwrap()).unwrap();
    no_warning(&reopened);
    assert_eq!(corner(&reopened, 0), vec![5, 17, 29, 255]);
    assert_eq!(corner(&reopened, 1), corner(&layout_local, 1));
}

#[test]
fn zero_background_style_reference_uses_the_resolvers_first_fill_for_remap_guard() {
    let mut p = themed_background();
    let master = Arc::make_mut(&mut p.masters[0]);
    master.background = Some(Background::Ref { idx: 0, color: ColorRef::rgb(Rgba::rgb(99, 88, 77)) });
    master.theme.format.fills = vec![Fill::solid(ColorRef::scheme(SchemeSlot::Bg1))];
    assert_eq!(corner(&p, 0), vec![13, 29, 47, 255]);
    Arc::make_mut(&mut p.slides[0]).color_map = Some(vec![("bg1".into(), "dk1".into())]);
    assert_eq!(corner(&p, 0), vec![201, 37, 91, 255]);
    assert!(export(&p).unwrap_err().to_string().contains("color-map"));
}
