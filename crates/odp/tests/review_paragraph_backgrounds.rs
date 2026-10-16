//! Independent contributor-original regression cases for paragraph-area preservation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    text::{BodyProps, Spacing},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};
use zip::write::SimpleFileOptions;

fn blue() -> Fill {
    Fill::solid(ColorRef::rgb(Rgba::rgb(20, 100, 180)))
}
fn empty_body() -> TextBody {
    let mut body = TextBody::from_text("");
    body.body = BodyProps { inset_l: Some(0.0), inset_r: Some(0.0), inset_t: Some(0.0), inset_b: Some(0.0), ..Default::default() };
    body.paragraphs[0].props.background = Some(blue());
    body.paragraphs[0].props.line_spacing = Some(Spacing::Pts(24.0));
    body
}
fn shape() -> Shape {
    Shape {
        text: Some(empty_body()),
        text_box: true,
        xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)),
        fill: Some(Fill::None),
        line: Some(Line::none()),
        ..Default::default()
    }
}
fn synthetic(styles: &str, content: &str) -> Vec<u8> {
    synthetic_frame(styles, &format!("<draw:text-box>{content}</draw:text-box>"))
}
fn synthetic_frame(styles: &str, content: &str) -> Vec<u8> {
    let xml = format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:ext='urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation><draw:page><draw:frame svg:width='200pt' svg:height='100pt'>{content}</draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("mimetype", MIME),
        ("content.xml", xml.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn empty_placeholder_keeps_painted_area_after_odp_export() {
    let mut deck = Presentation::default();
    let mut sh = shape();
    sh.ph = Some(Placeholder::default());
    Arc::make_mut(&mut deck.slides[0]).shapes = vec![sh];
    let image = deckcraft_render::render_slide(&deck, 0, &Default::default());
    assert_eq!(image.pixel(200, 50), [20, 100, 180, 255]);
    let reopened = import(&export(&deck).unwrap()).unwrap();
    assert_eq!(reopened.slides[0].shapes.len(), 1, "painted empty placeholder is visible content");
    assert_eq!(reopened.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].props.background, Some(blue()));
}

#[test]
fn empty_notes_keep_painted_paragraph_after_odp_export() {
    let mut deck = Presentation::default();
    Arc::make_mut(&mut deck.slides[0]).notes = empty_body();
    let reopened = import(&export(&deck).unwrap()).unwrap();
    assert_eq!(reopened.slides[0].notes.paragraphs.len(), 1);
    assert_eq!(reopened.slides[0].notes.paragraphs[0].props.background, Some(blue()));
}

#[test]
fn empty_table_cell_paints_its_paragraph_before_and_after_odp_export() {
    let mut deck = Presentation::default();
    let mut table = Table::new(1, 1, 200.0, 100.0);
    table.rows[0].cells[0].text = empty_body();
    table.rows[0].cells[0].margins = Some([0.0; 4]);
    Arc::make_mut(&mut deck.slides[0]).shapes =
        vec![Shape { kind: ShapeKind::Table(table), xfrm: Some(Xfrm::new(40.0, 40.0, 200.0, 100.0)), ..Default::default() }];
    for candidate in [&deck, &import(&export(&deck).unwrap()).unwrap()] {
        let image = deckcraft_render::render_slide(candidate, 0, &Default::default());
        assert_eq!(image.pixel(200, 50), [20, 100, 180, 255], "empty cell paragraph background remains visible");
    }
}

#[test]
fn conflicting_inherited_standard_and_extension_paints_stay_guarded() {
    for (parent, child) in [
        ("<ext:graphic-properties draw:fill='solid' draw:fill-color='#112233'/>", "<style:paragraph-properties fo:background-color='#445566'/>"),
        ("<style:paragraph-properties fo:background-color='#112233'/>", "<ext:graphic-properties draw:fill='solid' draw:fill-color='#445566'/>"),
    ] {
        let styles = format!(
            "<style:style style:name='parent' style:family='paragraph'>{parent}</style:style><style:style style:name='child' style:family='paragraph' style:parent-style-name='parent'>{child}</style:style>"
        );
        let error = import(&synthetic(&styles, "<text:p text:style-name='child'>Own text</text:p>")).unwrap_err();
        assert!(error.0.contains("conflicting standard and extension paragraph background inheritance is not supported"));
    }
}

#[test]
fn default_paragraph_area_effects_guard_unnamed_paragraphs() {
    let styles = "<style:default-style style:family='paragraph'><style:paragraph-properties fo:background-color='#112233' fo:padding='2pt'/></style:default-style>";
    let deck = import(&synthetic(styles, "<text:p/>")).unwrap();
    assert!(deck.tags.iter().any(|(key, _)| key == "odp:warning"));
    assert!(export(&deck).is_err());
}

#[test]
fn duplicate_alias_attributes_cannot_replace_extension_fill_semantics() {
    let styles = "<style:style style:name='p' style:family='paragraph'><ext:graphic-properties xmlns:other='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' draw:fill='solid' other:fill='none' draw:fill-color='#112233'/></style:style>";
    let error = import(&synthetic(styles, "<text:p text:style-name='p'>Own text</text:p>")).unwrap_err();
    assert!(error.0.contains("duplicate expanded XML attribute"));
}

#[test]
fn horizontal_text_centering_does_not_shift_paragraph_column_area() {
    let mut deck = Presentation::default();
    let mut sh = shape();
    sh.text.as_mut().unwrap().body.anchor_ctr = Some(true);
    sh.text.as_mut().unwrap().paragraphs[0].runs = vec![Run::new("i")];
    Arc::make_mut(&mut deck.slides[0]).shapes = vec![sh];
    let image = deckcraft_render::render_slide(&deck, 0, &Default::default());
    assert_eq!(image.pixel(50, 50), [20, 100, 180, 255], "paragraph area starts at its column margin");
    assert_eq!(image.pixel(260, 50), [255; 4], "paragraph area ends at its column margin");
}

#[test]
fn identical_opaque_paragraph_area_remains_opaque_through_text_morph() {
    let mut deck = Presentation::default();
    let mut a = shape();
    a.id = ShapeId(11);
    let mut b = a.clone();
    b.id = ShapeId(10);
    Arc::make_mut(&mut deck.slides[0]).shapes = vec![a.clone()];
    let mut second = (*deck.slides[0]).clone();
    second.shapes = vec![b.clone()];
    deck.slides.push(Arc::new(second));
    let rect = a.xfrm.unwrap();
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let morph = deckcraft_render::TextMorph { old: a.id, new: b.id, from: rect, to: rect, chars: false, t };
        let image = deckcraft_render::render_blend(&deck, 0, 1, t, &[(b.clone(), false), (a.clone(), true)], &[morph], &[], &Default::default());
        assert_eq!(image.pixel(200, 50), [20, 100, 180, 255], "same opaque background must remain opaque at t={t}");
    }
}

#[test]
fn overlapping_opaque_paragraph_areas_remain_opaque_when_morph_geometry_changes() {
    let mut deck = Presentation::default();
    let mut a = shape();
    a.id = ShapeId(11);
    let mut b = a.clone();
    b.id = ShapeId(10);
    b.text.as_mut().unwrap().paragraphs[0].props.line_spacing = Some(Spacing::Pts(48.0));
    Arc::make_mut(&mut deck.slides[0]).shapes = vec![a.clone()];
    let mut second = (*deck.slides[0]).clone();
    second.shapes = vec![b.clone()];
    deck.slides.push(Arc::new(second));
    let rect = a.xfrm.unwrap();
    let morph = deckcraft_render::TextMorph { old: a.id, new: b.id, from: rect, to: rect, chars: false, t: 0.5 };
    let image = deckcraft_render::render_blend(&deck, 0, 1, 0.5, &[(b, false), (a, true)], &[morph], &[], &Default::default());
    assert_eq!(image.pixel(200, 50), [20, 100, 180, 255], "shared old/new coverage must remain opaque");
    let edge = image.pixel(200, 75);
    for (actual, expected) in edge.iter().zip([137u8, 177, 217, 255]) {
        assert!(actual.abs_diff(expected) <= 2, "new-only area fades halfway: {edge:?}");
    }
}

#[test]
fn annotation_on_empty_painted_paragraph_does_not_discard_its_frame() {
    let styles = "<style:style style:name='p' style:family='paragraph'><style:paragraph-properties fo:background-color='#1464B4'/></style:style>";
    let deck = import(&synthetic(
        styles,
        "<text:p text:style-name='p'><office:annotation office:name='comment'><text:p>Own note</text:p></office:annotation></text:p>",
    ))
    .unwrap();
    assert_eq!(deck.slides[0].shapes.len(), 1, "a commented paragraph area is visible content");
    assert_eq!(deck.slides[0].shapes[0].text.as_ref().unwrap().paragraphs[0].props.background, Some(blue()));
}

#[test]
fn identical_rotated_paragraph_areas_remain_opaque_at_morph_midpoint() {
    let mut deck = Presentation::default();
    let mut a = shape();
    a.id = ShapeId(11);
    a.text.as_mut().unwrap().body.rot = Some(30.0);
    let mut b = a.clone();
    b.id = ShapeId(10);
    Arc::make_mut(&mut deck.slides[0]).shapes = vec![a.clone()];
    let mut second = (*deck.slides[0]).clone();
    second.shapes = vec![b.clone()];
    deck.slides.push(Arc::new(second));
    let original = deckcraft_render::render_slide(&deck, 0, &Default::default());
    assert_eq!(original.pixel(212, 85), [20, 100, 180, 255]);
    let rect = a.xfrm.unwrap();
    let morph = deckcraft_render::TextMorph { old: a.id, new: b.id, from: rect, to: rect, chars: false, t: 0.5 };
    let image = deckcraft_render::render_blend(&deck, 0, 1, 0.5, &[(b, false), (a, true)], &[morph], &[], &Default::default());
    assert_eq!(image.pixel(212, 85), [20, 100, 180, 255], "unchanged rotated area must remain opaque");
}

#[test]
fn table_cell_style_paragraph_area_is_separate_from_character_highlighting() {
    let para = "<style:paragraph-properties fo:background-color='#1464B4'/>";
    let text = "<style:text-properties fo:background-color='#F0C814'/>";
    for properties in [format!("{para}{text}"), format!("{text}{para}")] {
        for (ancestors, parent, cell_fill) in [
            ("", "", None),
            (
                "<style:default-style style:family='table-cell'><style:table-cell-properties fo:background-color='#AABBCC'/></style:default-style><style:style style:name='base' style:family='table-cell'><style:table-cell-properties fo:background-color='#778899'/></style:style>",
                "base",
                Some(Fill::solid(ColorRef::rgb(Rgba::rgb(119, 136, 153)))),
            ),
        ] {
            let parent_attribute = if parent.is_empty() { String::new() } else { format!(" style:parent-style-name='{parent}'") };
            let styles = format!("{ancestors}<style:style style:name='cell' style:family='table-cell'{parent_attribute}>{properties}</style:style>");
            let content = "<table:table xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0'><table:table-column/><table:table-row><table:table-cell table:style-name='cell'><text:p>Own text</text:p></table:table-cell></table:table-row></table:table>";
            let deck = import(&synthetic_frame(&styles, content)).unwrap();
            assert!(!deck.tags.iter().any(|(key, _)| key == "odp:warning"));
            let ShapeKind::Table(table) = &deck.slides[0].shapes[0].kind else { panic!("expected editable table") };
            assert_eq!(table.rows[0].cells[0].fill, cell_fill, "cell paint comes only from inherited table-cell-properties");
            let paragraph = &table.rows[0].cells[0].text.paragraphs[0];
            assert_eq!(paragraph.props.background, Some(blue()));
            assert_eq!(paragraph.runs[0].props.highlight, Some(ColorRef::rgb(Rgba::rgb(240, 200, 20))));
            let reopened = import(&export(&deck).unwrap()).unwrap();
            let ShapeKind::Table(table) = &reopened.slides[0].shapes[0].kind else { panic!("expected editable table") };
            assert_eq!(table.rows[0].cells[0].fill, cell_fill);
            assert_eq!(table.rows[0].cells[0].text.paragraphs[0].props.background, Some(blue()));
        }
    }
}

#[test]
fn table_cell_inherited_background_cannot_hide_cross_encoding_conflicts() {
    let styles = "<style:style style:name='cell' style:family='table-cell'><style:paragraph-properties fo:background-color='#1464B4'/></style:style><style:style style:name='paragraph' style:family='paragraph'><ext:graphic-properties draw:fill='solid' draw:fill-color='#F0C814'/></style:style>";
    let content = "<table:table xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0'><table:table-column/><table:table-row><table:table-cell table:style-name='cell'><text:p text:style-name='paragraph'>Own text</text:p></table:table-cell></table:table-row></table:table>";
    match import(&synthetic_frame(styles, content)) {
        Ok(deck) => {
            assert!(deck.tags.iter().any(|(key, _)| key == "odp:warning"), "ambiguous cell/paragraph precedence must remain guarded");
            assert!(export(&deck).is_err());
        }
        Err(error) => assert!(error.0.contains("conflicting standard and extension paragraph background inheritance is not supported")),
    }
}
