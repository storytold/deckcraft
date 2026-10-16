//! Contributor-original paragraph-area fixtures; no external documents or assets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{text::Paragraph, *};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
use zip::write::SimpleFileOptions;

fn package(styles: &str, text: &str, shape_style: &str) -> Vec<u8> {
    package_with_shape(styles, text, shape_style, "frame")
}
fn package_with_shape(styles: &str, text: &str, shape_style: &str, shape: &str) -> Vec<u8> {
    let geometry = if shape == "custom-shape" { "<draw:enhanced-geometry draw:type='rectangle'/>" } else { "" };
    let content = format!(
        "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:ext='urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0'><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation><draw:page><draw:{shape} {shape_style} svg:width='300pt' svg:height='150pt'>{geometry}<draw:text-box>{text}</draw:text-box></draw:{shape}></draw:page></office:presentation></office:body></office:document-content>"
    );
    zip_document(&content)
}
fn zip_document(content: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", content),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn style(name: &str, parent: &str, properties: &str) -> String {
    format!("<style:style style:name='{name}' style:family='paragraph' style:parent-style-name='{parent}'>{properties}</style:style>")
}
fn background(hex: &str) -> Fill {
    let color = u32::from_str_radix(hex, 16).unwrap();
    Fill::solid(ColorRef::rgb(Rgba::rgb((color >> 16) as u8, (color >> 8) as u8, color as u8)))
}
fn paragraphs(deck: &Presentation) -> &[Paragraph] {
    &deck.slides[0].shapes[0].text.as_ref().unwrap().paragraphs
}
fn no_warnings(deck: &Presentation) {
    assert!(!deck.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", deck.tags);
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
    xml
}

#[test]
fn standard_default_parent_child_and_explicit_transparent_inherit() {
    let styles = format!(
        "<style:default-style style:family='paragraph'><style:paragraph-properties fo:background-color='#112233'/></style:default-style>{}{}{}",
        style("parent", "", "<style:paragraph-properties fo:background-color='#445566'/>"),
        style("child", "parent", "<style:paragraph-properties fo:text-align='center'/>"),
        style("clear", "child", "<style:paragraph-properties fo:background-color='transparent'/>"),
    );
    let deck =
        import(&package(&styles, "<text:p>Default</text:p><text:p text:style-name='child'>Inherited</text:p><text:p text:style-name='clear'/>", ""))
            .unwrap();
    no_warnings(&deck);
    assert_eq!(
        paragraphs(&deck).iter().map(|p| p.props.background.clone()).collect::<Vec<_>>(),
        [Some(background("112233")), Some(background("445566")), Some(Fill::None)]
    );
    assert!(paragraphs(&deck).iter().all(|p| p.def_props.highlight.is_none()));
}

#[test]
fn paragraph_and_run_backgrounds_do_not_depend_on_property_element_order() {
    let paragraph = "<style:paragraph-properties fo:background-color='#112233'/>";
    let text = "<style:text-properties fo:background-color='#abcdef'/>";
    for properties in [format!("{paragraph}{text}"), format!("{text}{paragraph}")] {
        let deck = import(&package(&style("p", "", &properties), "<text:p text:style-name='p'>Own text</text:p>", "")).unwrap();
        let p = &paragraphs(&deck)[0];
        assert_eq!(p.props.background, Some(background("112233")));
        assert_eq!(p.runs[0].props.highlight, Some(ColorRef::rgb(Rgba::rgb(171, 205, 239))));
        let reopened = import(&export(&deck).unwrap()).unwrap();
        assert_eq!(paragraphs(&reopened)[0].props.background, p.props.background);
        assert_eq!(paragraphs(&reopened)[0].runs[0].props.highlight, p.runs[0].props.highlight);
    }
}

#[test]
fn extension_defaults_inherit_and_none_ignores_inactive_color() {
    let styles = format!(
        "<style:default-style style:family='paragraph'><ext:graphic-properties draw:fill='solid' draw:fill-color='#112233'/></style:default-style>{}{}{}",
        style("parent", "", "<ext:graphic-properties draw:fill='solid' draw:fill-color='#445566'/>"),
        style("child", "parent", "<style:text-properties fo:font-size='24pt'/>"),
        style("clear", "child", "<ext:graphic-properties draw:fill='none' draw:fill-color='#ffffff'/>"),
    );
    let deck = import(&package(
        &styles,
        "<text:p>Default</text:p><text:p text:style-name='child'>Inherited</text:p><text:p text:style-name='clear'>Clear</text:p>",
        "",
    ))
    .unwrap();
    no_warnings(&deck);
    assert_eq!(
        paragraphs(&deck).iter().map(|p| p.props.background.clone()).collect::<Vec<_>>(),
        [Some(background("112233")), Some(background("445566")), Some(Fill::None)]
    );
}

#[test]
fn shape_text_style_overrides_global_default_and_paragraph_can_clear_it() {
    let styles = format!(
        "<style:default-style style:family='paragraph'><ext:graphic-properties draw:fill='solid' draw:fill-color='#112233'/></style:default-style>{}{}",
        style("shape", "", "<ext:graphic-properties draw:fill='solid' draw:fill-color='#445566'/>"),
        style("clear", "", "<style:paragraph-properties fo:background-color='transparent'/>"),
    );
    let deck =
        import(&package(&styles, "<text:p>Shape style</text:p><text:p text:style-name='clear'>Cleared</text:p>", "draw:text-style-name='shape'"))
            .unwrap();
    no_warnings(&deck);
    assert_eq!(paragraphs(&deck)[0].props.background, Some(background("445566")));
    assert_eq!(paragraphs(&deck)[1].props.background, Some(Fill::None));
}

#[test]
fn explicit_extension_mode_wins_same_style_standard_fallback() {
    for (mode, expected) in [("none", Fill::None), ("solid", background("445566"))] {
        let properties = format!(
            "<ext:graphic-properties draw:fill='{mode}' draw:fill-color='#445566'/><style:paragraph-properties fo:background-color='#112233'/>"
        );
        let deck = import(&package(&style("p", "", &properties), "<text:p text:style-name='p'>Own text</text:p>", "")).unwrap();
        no_warnings(&deck);
        assert_eq!(paragraphs(&deck)[0].props.background, Some(expected));
    }
}

#[test]
fn conflicting_painted_encodings_stay_guarded_but_cross_encoding_clear_is_safe() {
    let standard = "<style:paragraph-properties fo:background-color='#112233'/>";
    let extension = "<ext:graphic-properties draw:fill='solid' draw:fill-color='#445566'/>";
    for (parent, child, clear) in [
        (standard, extension, "<ext:graphic-properties draw:fill='none'/>"),
        (extension, standard, "<style:paragraph-properties fo:background-color='transparent'/>"),
    ] {
        let styles = format!("{}{}", style("parent", "", parent), style("child", "parent", child));
        assert!(import(&package(&styles, "<text:p text:style-name='child'>Conflict</text:p>", "")).is_err());
        let styles = format!("{}{}", style("parent", "", parent), style("child", "parent", clear));
        let deck = import(&package(&styles, "<text:p text:style-name='child'>Clear</text:p>", "")).unwrap();
        no_warnings(&deck);
        assert_eq!(paragraphs(&deck)[0].props.background, Some(Fill::None));
        let styles = format!("{}{}", style("shape", "", parent), style("child", "", child));
        let deck = import(&package(&styles, "<text:p text:style-name='child'>Conflict</text:p>", "draw:text-style-name='shape'")).unwrap();
        assert!(deck.tags.iter().any(|(key, message)| key == "odp:warning" && message.contains("paragraph background")));
        assert!(export(&deck).is_err());
    }
}

#[test]
fn paragraph_area_guards_cover_used_graphic_and_presentation_styles_on_custom_shapes() {
    for (family, attribute) in [("graphic", "draw:style-name"), ("presentation", "presentation:style-name")] {
        let styles = format!("<style:style style:name='g' style:family='{family}'><style:paragraph-properties fo:padding='2pt'/></style:style>");
        let deck = import(&package_with_shape(&styles, "<text:p>Own shape</text:p>", &format!("{attribute}='g'"), "custom-shape")).unwrap();
        assert!(deck.tags.iter().any(|(key, message)| key == "odp:warning" && message.contains("paragraph padding")));
        assert!(export(&deck).is_err());
    }
}

#[test]
fn shape_style_paragraph_areas_remain_distinct_from_text_highlights() {
    let paragraph = "<style:paragraph-properties fo:background-color='#112233'/>";
    let text = "<style:text-properties fo:background-color='#abcdef'/>";
    for (family, attribute) in [("graphic", "draw:style-name"), ("presentation", "presentation:style-name")] {
        for properties in [format!("{paragraph}{text}"), format!("{text}{paragraph}")] {
            let styles = format!("<style:style style:name='g' style:family='{family}'>{properties}</style:style>");
            let deck = import(&package(&styles, "<text:p>Own shape</text:p>", &format!("{attribute}='g'"))).unwrap();
            let p = &paragraphs(&deck)[0];
            assert_eq!(p.props.background, Some(background("112233")));
            assert_eq!(p.runs[0].props.highlight, Some(ColorRef::rgb(Rgba::rgb(171, 205, 239))));
            if family == "presentation" {
                // Paint imports correctly, but shared presentation-style ownership
                // still needs an editable resource mapping beyond these literal values.
                assert!(deck.tags.iter().any(|(key, message)| key == "odp:warning" && message.contains("presentation-style ownership")));
                assert!(export(&deck).is_err());
            } else {
                no_warnings(&deck);
                let reopened = import(&export(&deck).unwrap()).unwrap();
                assert_eq!(paragraphs(&reopened)[0].props.background, p.props.background);
                assert_eq!(paragraphs(&reopened)[0].runs[0].props.highlight, p.runs[0].props.highlight);
            }
        }
        let styles = format!("<style:default-style style:family='{family}'>{paragraph}</style:default-style>");
        let deck = import(&package(&styles, "<text:p>Default shape</text:p>", "")).unwrap();
        assert_eq!(paragraphs(&deck)[0].props.background, Some(background("112233")));
        assert!(paragraphs(&deck)[0].runs[0].props.highlight.is_none());
    }
}

#[test]
fn table_and_notes_paragraph_geometry_limits_stay_guarded() {
    let styles = style("p", "", "<style:paragraph-properties fo:background-color='#112233' fo:padding='2pt'/>");
    let content = part(&package(&styles, "", ""), "content.xml");
    let paragraph = "<text:p text:style-name='p'>Own text</text:p>";
    let table = format!(
        "<table:table><table:table-column/><table:table-row><table:table-cell>{paragraph}</table:table-cell></table:table-row></table:table>"
    );
    let notes = format!(
        "<presentation:notes><draw:frame presentation:class='notes' svg:width='300pt' svg:height='150pt'><draw:text-box>{paragraph}</draw:text-box></draw:frame></presentation:notes>"
    );
    for xml in [content.replace("<draw:text-box></draw:text-box>", &table), content.replace("</draw:page>", &format!("{notes}</draw:page>"))] {
        let deck = import(&zip_document(&xml)).unwrap();
        assert!(deck.tags.iter().any(|(key, message)| key == "odp:warning" && message.contains("paragraph padding")));
        assert!(export(&deck).is_err());
    }
}

#[test]
fn table_cell_defaults_and_parent_styles_supply_separate_paragraph_and_run_backgrounds() {
    let styles = format!(
        "<style:default-style style:family='table-cell'><style:table-cell-properties fo:background-color='#778899'/><style:paragraph-properties fo:background-color='#112233'/><style:text-properties fo:background-color='#abcdef'/></style:default-style><style:style style:name='parent' style:family='table-cell'><style:table-cell-properties fo:background-color='#223344'/><style:paragraph-properties fo:background-color='#445566'/></style:style><style:style style:name='child' style:family='table-cell' style:parent-style-name='parent'><style:text-properties fo:font-weight='bold'/></style:style><style:style style:name='clear-cell' style:family='table-cell' style:parent-style-name='child'><style:table-cell-properties fo:background-color='transparent'/><style:paragraph-properties fo:background-color='transparent'/></style:style>{}",
        style("clear-paragraph", "", "<style:paragraph-properties fo:background-color='transparent'/>"),
    );
    for cached in [false, true] {
        let final_cell = if cached {
            "<table:table-cell table:style-name='child' office:string-value='Cached'/>"
        } else {
            "<table:table-cell table:style-name='child'><text:p>Cached</text:p></table:table-cell>"
        };
        let cells = format!(
            "<table:table-cell><text:p>Default</text:p></table:table-cell><table:table-cell table:style-name='child'><text:p>Parent</text:p></table:table-cell><table:table-cell table:style-name='child'><text:p text:style-name='clear-paragraph'>Clear paragraph</text:p></table:table-cell><table:table-cell table:style-name='clear-cell'><text:p>Clear cell style</text:p></table:table-cell>{final_cell}"
        );
        let table =
            format!("<table:table><table:table-column table:number-columns-repeated='5'/><table:table-row>{cells}</table:table-row></table:table>");
        let content = part(&package(&styles, "", ""), "content.xml").replace("<draw:text-box></draw:text-box>", &table);
        let source = zip_document(&content);
        let deck = import(&source).unwrap();
        let reopened = if cached {
            // Stored string ownership is not represented yet. Keep checking its
            // imported paint/text, and require the new value-loss safeguard.
            assert!(deck.tags.iter().any(|(key, warning)| key == "odp:warning" && warning.starts_with("ODF typed cell values")));
            assert!(export(&deck).is_err());
            assert_eq!(
                deck.media.iter().find(|media| media.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(),
                source.as_slice()
            );
            None
        } else {
            no_warnings(&deck);
            Some(import(&export(&deck).unwrap()).unwrap())
        };
        for candidate in std::iter::once(&deck).chain(reopened.as_ref()) {
            let ShapeKind::Table(table) = &candidate.slides[0].shapes[0].kind else { panic!("expected table") };
            let expected = [background("112233"), background("445566"), Fill::None, Fill::None, background("445566")];
            let cell_fills = [background("778899"), background("223344"), background("223344"), Fill::None, background("223344")];
            assert_eq!(table.rows[0].cells.iter().map(|cell| cell.fill.clone()).collect::<Vec<_>>(), cell_fills.map(Some));
            for (cell, expected) in table.rows[0].cells.iter().zip(expected) {
                let paragraph = &cell.text.paragraphs[0];
                assert_eq!(paragraph.props.background, Some(expected));
                assert_eq!(paragraph.runs[0].props.highlight, Some(ColorRef::rgb(Rgba::rgb(171, 205, 239))));
            }
            assert_eq!(table.rows[0].cells[4].text.paragraphs[0].text(), "Cached");
        }
    }
}

#[test]
fn table_cell_fill_does_not_become_a_run_highlight_or_paragraph_area() {
    let styles =
        "<style:style style:name='cell' style:family='table-cell'><style:table-cell-properties fo:background-color='#112233'/></style:style>";
    let table = "<table:table><table:table-column/><table:table-row><table:table-cell table:style-name='cell'><text:p>Own text</text:p></table:table-cell></table:table-row></table:table>";
    let content = part(&package(styles, "", ""), "content.xml").replace("<draw:text-box></draw:text-box>", table);
    let deck = import(&zip_document(&content)).unwrap();
    no_warnings(&deck);
    let ShapeKind::Table(table) = &deck.slides[0].shapes[0].kind else { panic!("expected table") };
    assert_eq!(table.rows[0].cells[0].fill, Some(background("112233")));
    let paragraph = &table.rows[0].cells[0].text.paragraphs[0];
    assert!(paragraph.props.background.is_none());
    assert!(paragraph.runs[0].props.highlight.is_none());
}

#[test]
fn unsupported_or_malformed_extensions_remain_warned_and_export_refuses() {
    for properties in [
        "<ext:graphic-properties draw:fill='gradient' draw:fill-color='#112233'/>",
        "<ext:graphic-properties draw:fill='solid' draw:fill-color='#112233' draw:opacity='50%'/>",
        "<ext:graphic-properties draw:fill='solid'/>",
        "<ext:graphic-properties draw:fill='solid' draw:fill-color='#zzzzzz'/>",
        "<ext:graphic-properties draw:fill-color='#112233'/>",
        "<ext:graphic-properties draw:fill='none'><style:background-image/></ext:graphic-properties>",
        "<ext:graphic-properties draw:fill='none'>Unknown text</ext:graphic-properties>",
    ] {
        let deck = import(&package(&style("p", "", properties), "<text:p text:style-name='p'>Own text</text:p>", "")).unwrap();
        assert!(deck.tags.iter().any(|(key, _)| key == "odp:warning"), "{properties}");
        assert!(export(&deck).is_err(), "{properties}");
    }
}

#[test]
fn malformed_standard_background_and_background_cycles_reject() {
    for value in ["#1234", "red", "#zzzzzz", "", "#12345678"] {
        let properties = format!("<style:paragraph-properties fo:background-color='{value}'/>");
        assert!(import(&package(&style("p", "", &properties), "<text:p text:style-name='p'>Own text</text:p>", "")).is_err());
    }
    assert!(import(&package(&style("cycle", "cycle", ""), "<text:p text:style-name='cycle'>Own text</text:p>", "")).is_err());
    let duplicate = "<ext:graphic-properties draw:fill='none'/><ext:graphic-properties draw:fill='solid' draw:fill-color='#112233'/>";
    assert!(import(&package(&style("p", "", duplicate), "<text:p text:style-name='p'>Own text</text:p>", "")).is_err());
}

#[test]
fn unsupported_paragraph_area_geometry_warns_instead_of_becoming_safe() {
    for properties in [
        "<style:paragraph-properties fo:padding='2pt'/>",
        "<style:paragraph-properties fo:border='1pt solid #112233'/>",
        "<style:paragraph-properties style:join-border='true'/>",
        "<style:paragraph-properties style:shadow='1pt 2pt #112233'/>",
        "<style:paragraph-properties><style:background-image/></style:paragraph-properties>",
    ] {
        let deck = import(&package(&style("p", "", properties), "<text:p text:style-name='p'>Own text</text:p>", "")).unwrap();
        assert!(deck.tags.iter().any(|(key, _)| key == "odp:warning"), "{properties}");
        assert!(export(&deck).is_err());
    }
    let deck = import(&package(
        &style("p", "", "<style:paragraph-properties fo:padding='0pt' fo:border='none' style:join-border='false'/>"),
        "<text:p text:style-name='p'>Own text</text:p>",
        "",
    ))
    .unwrap();
    no_warnings(&deck);
}

#[test]
fn edited_backgrounds_export_as_standard_properties_and_reopen_editable() {
    let styles = style("p", "", "<ext:graphic-properties draw:fill='solid' draw:fill-color='#112233'/>");
    let mut deck = import(&package(&styles, "<text:p text:style-name='p'>Before</text:p><text:p/>", "")).unwrap();
    let body = Arc::make_mut(&mut deck.slides[0]).shapes[0].text.as_mut().unwrap();
    body.paragraphs[0].runs[0].text = "After edit".into();
    body.paragraphs[0].props.background = Some(background("abcdef"));
    body.paragraphs[1].props.background = Some(Fill::None);
    let bytes = export(&deck).unwrap();
    let xml = part(&bytes, "content.xml");
    assert!(xml.contains("fo:background-color=\"#ABCDEF\""));
    assert!(xml.contains("fo:background-color=\"transparent\""));
    assert!(!xml.contains("loext:graphic-properties") && !xml.contains("ext:graphic-properties"));
    let reopened = import(&bytes).unwrap();
    no_warnings(&reopened);
    assert_eq!(paragraphs(&reopened)[0].text(), "After edit");
    assert_eq!(paragraphs(&reopened)[0].props.background, Some(background("abcdef")));
    assert_eq!(paragraphs(&reopened)[1].props.background, Some(Fill::None));
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_BACKGROUND_OUTPUT") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(name), part(&bytes, name)).unwrap();
        }
    }
}

#[test]
fn writer_refuses_nonopaque_and_non_solid_paragraph_backgrounds() {
    let mut deck = import(&package("", "<text:p>Own text</text:p>", "")).unwrap();
    for fill in [Fill::solid(ColorRef::rgb(Rgba::BLACK).with(ColorTransform::Alpha(50000))), Fill::Group, Fill::Background] {
        Arc::make_mut(&mut deck.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0].props.background = Some(fill);
        assert!(export(&deck).unwrap_err().0.contains("paragraph background"));
    }
}
