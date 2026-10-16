//! Contributor-original notes layouts; all previews remain editable live slide references.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    notes::{NotesDateTime, NotesFields},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
use zip::write::SimpleFileOptions;

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:presentation='urn:oasis:names:tc:opendocument:xmlns:presentation:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0'";

fn fixture(pages: &str, masters: &str, styles: &str, declarations: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles/><office:body><office:presentation>{declarations}{pages}</office:presentation></office:body></office:document-content>"
    );
    let styles = format!(
        "<office:document-styles {NS} office:version='1.3'><office:styles>{styles}</office:styles><office:automatic-styles><style:page-layout style:name='Slide'><style:page-layout-properties fo:page-width='400pt' fo:page-height='300pt'/></style:page-layout><style:page-layout style:name='Portrait'><style:page-layout-properties fo:page-width='400pt' fo:page-height='600pt' fo:margin='0pt'/></style:page-layout><style:page-layout style:name='Landscape'><style:page-layout-properties fo:page-width='600pt' fo:page-height='400pt' fo:margin='0pt'/></style:page-layout></office:automatic-styles><office:master-styles>{masters}</office:master-styles></office:document-styles>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("mimetype", MIME),
        ("content.xml", &content),
        ("styles.xml", &styles),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/> "),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(data.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn body(text: &str) -> String {
    format!(
        "<draw:frame presentation:class='notes' presentation:user-transformed='true' svg:x='40pt' svg:y='320pt' svg:width='320pt' svg:height='220pt'><draw:text-box>{text}</draw:text-box></draw:frame>"
    )
}
fn thumbnail(ordinal: Option<usize>) -> String {
    let number = ordinal.map(|n| format!(" draw:page-number='{n}'")).unwrap_or_default();
    format!("<draw:page-thumbnail presentation:class='page' svg:x='40pt' svg:y='40pt' svg:width='320pt' svg:height='240pt'{number}/>")
}
fn master(name: &str, layout: &str, extra: &str) -> String {
    format!(
        "<style:master-page style:name='{name}' style:page-layout-name='Slide'><presentation:notes style:page-layout-name='{layout}'>{}{}{extra}</presentation:notes></style:master-page>",
        thumbnail(None),
        body("")
    )
}
fn page(master: &str, ordinal: usize, text: &str, extra: &str) -> String {
    format!(
        "<draw:page draw:master-page-name='{master}'><presentation:notes>{}{}{extra}</presentation:notes></draw:page>",
        body(text),
        thumbnail(Some(ordinal))
    )
}
fn no_warnings(p: &Presentation) {
    assert!(!p.tags.iter().any(|(key, _)| key == "odp:warning"), "{:?}", p.tags);
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}
fn dump(label: &str, bytes: &[u8]) {
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_NOTES_OUTPUT") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(format!("{label}-{name}")), part(bytes, name)).unwrap();
        }
    }
}

#[test]
fn empty_notes_keep_distinct_layout_frames_and_live_owning_slide_thumbnail() {
    let deck = import(&fixture(&page("M", 1, "", ""), &master("M", "Portrait", ""), "", "")).unwrap();
    no_warnings(&deck);
    assert_eq!(deck.notes_masters.len(), 1);
    assert!(deck.slides[0].notes.is_empty());
    let layout = deck.slides[0].notes_page.as_ref().unwrap();
    assert_eq!(layout.shapes.len(), 2);
    assert!(layout.shapes[0].text.is_none());
    assert_eq!(layout.body_shape, Some(layout.shapes[0].id));
    assert_eq!(layout.shapes[1].ph_type(), Some(PhType::SlideImage));
    assert_eq!(deck.notes_page_size(&deck.slides[0]), Size::new(400.0, 600.0));
    let bytes = export(&deck).unwrap();
    dump("empty", &bytes);
    assert!(part(&bytes, "content.xml").contains("<draw:page-thumbnail"));
    let reopened = import(&bytes).unwrap();
    no_warnings(&reopened);
    assert_eq!(reopened.slides[0].notes_page.as_ref().unwrap().shapes.len(), 2);
    assert!(reopened.slides[0].notes.is_empty());
}

#[test]
fn canonical_notes_edit_does_not_stale_or_concatenate_other_text_frames() {
    let extra = "<draw:frame svg:x='10pt' svg:y='280pt' svg:width='80pt' svg:height='20pt'><draw:text-box><text:p>Independent label</text:p></draw:text-box></draw:frame>";
    let mut deck = import(&fixture(&page("M", 1, "<text:p>Before</text:p>", extra), &master("M", "Portrait", ""), "", "")).unwrap();
    assert_eq!(deck.slides[0].notes_text(), "Before");
    Arc::make_mut(&mut deck.slides[0]).notes = TextBody::from_text("After canonical edit");
    let bytes = export(&deck).unwrap();
    dump("edit", &bytes);
    let reopened = import(&bytes).unwrap();
    no_warnings(&reopened);
    assert_eq!(reopened.slides[0].notes_text(), "After canonical edit");
    let page = reopened.slides[0].notes_page.as_ref().unwrap();
    assert!(find_shape(&page.shapes, page.body_shape.unwrap()).unwrap().text.is_none());
    assert_eq!(page.shapes.last().unwrap().text.as_ref().unwrap().text(), "Independent label");
}

#[test]
fn distinct_notes_masters_and_page_size_overrides_survive_reorder() {
    let pages =
        format!("{}{}{}", page("A", 1, "<text:p>A</text:p>", ""), page("B", 2, "<text:p>B</text:p>", ""), page("A", 3, "<text:p>C</text:p>", ""));
    let masters = format!("{}{}", master("A", "Portrait", ""), master("B", "Landscape", ""));
    let mut deck = import(&fixture(&pages, &masters, "", "")).unwrap();
    no_warnings(&deck);
    assert_eq!(deck.notes_masters.len(), 2);
    assert_eq!(deck.slides[0].notes_page.as_ref().unwrap().master, deck.slides[2].notes_page.as_ref().unwrap().master);
    assert_ne!(deck.slides[0].notes_page.as_ref().unwrap().master, deck.slides[1].notes_page.as_ref().unwrap().master);
    Arc::make_mut(&mut deck.slides[2]).notes_page.as_mut().unwrap().size = Some(Size::new(500.0, 700.0));
    deck.slides.swap(0, 1);
    let bytes = export(&deck).unwrap();
    dump("masters", &bytes);
    let reopened = import(&bytes).unwrap();
    no_warnings(&reopened);
    assert_eq!(reopened.notes_page_size(&reopened.slides[0]), Size::new(600.0, 400.0));
    assert_eq!(reopened.notes_page_size(&reopened.slides[1]), Size::new(400.0, 600.0));
    assert_eq!(reopened.notes_page_size(&reopened.slides[2]), Size::new(500.0, 700.0));
    let content = part(&bytes, "content.xml");
    for ordinal in 1..=3 {
        assert!(content.contains(&format!("draw:page-number=\"{ordinal}\"")));
    }
    assert_eq!(reopened.slides.iter().map(|s| s.notes_text()).collect::<Vec<_>>(), ["B", "A", "C"]);
}

#[test]
fn header_footer_and_date_field_markers_remain_live_and_use_page_values() {
    let fields = "<draw:frame presentation:class='header' svg:width='100pt' svg:height='20pt'><draw:text-box><text:p><presentation:header/></text:p></draw:text-box></draw:frame><draw:frame presentation:class='footer' svg:y='560pt' svg:width='100pt' svg:height='20pt'><draw:text-box><text:p><presentation:footer/></text:p></draw:text-box></draw:frame><draw:frame presentation:class='date-time' svg:x='200pt' svg:width='100pt' svg:height='20pt'><draw:text-box><text:p><presentation:date-time/></text:p></draw:text-box></draw:frame>";
    let mut deck = import(&fixture(&page("M", 1, "", ""), &master("M", "Portrait", fields), "", "")).unwrap();
    let notes = Arc::make_mut(&mut deck.slides[0]).notes_page.as_mut().unwrap();
    notes.fields = NotesFields {
        header: Some("Own header".into()),
        footer: Some(String::new()),
        date_time: Some(NotesDateTime::Fixed { text: "Own date".into() }),
        display_header: Some(true),
        display_footer: Some(false),
        display_date_time: Some(true),
        ..Default::default()
    };
    let bytes = export(&deck).unwrap();
    dump("fields", &bytes);
    assert!(part(&bytes, "styles.xml").contains("<presentation:header/>"));
    let reopened = import(&bytes).unwrap();
    no_warnings(&reopened);
    let fields = reopened.notes_fields(&reopened.slides[0]);
    assert_eq!(fields.header.as_deref(), Some("Own header"));
    assert_eq!(fields.footer.as_deref(), Some(""));
    assert_eq!(fields.date_time, Some(NotesDateTime::Fixed { text: "Own date".into() }));
    let master = &reopened.notes_masters[0].master;
    let field = master.shapes.iter().find(|s| s.ph_type() == Some(PhType::Header)).unwrap().text.as_ref().unwrap();
    assert!(matches!(&field.paragraphs[0].runs[0].kind,deckcraft_model::text::RunKind::Field{field} if field=="header"));
}

#[test]
fn foreign_thumbnail_ordinals_and_duplicate_bodies_reject() {
    for ordinal in [0, 2, usize::MAX] {
        let page = format!(
            "<draw:page draw:master-page-name='M'><presentation:notes>{}{}</presentation:notes></draw:page>",
            body(""),
            thumbnail(Some(ordinal))
        );
        assert!(import(&fixture(&page, &master("M", "Portrait", ""), "", "")).is_err());
    }
    let page = page("M", 1, "", &body("<text:p>Second body</text:p>"));
    assert!(import(&fixture(&page, &master("M", "Portrait", ""), "", "")).unwrap_err().0.contains("multiple primary notes"));
}

#[test]
fn unsupported_page_geometry_and_missing_field_references_remain_guarded() {
    let missing = page("M", 1, "", "").replace("<presentation:notes>", "<presentation:notes presentation:use-header-name='missing'>");
    assert!(import(&fixture(&missing, &master("M", "Portrait", ""), "", "")).is_err());
    let source = fixture(&page("M", 1, "", ""), &master("M", "Portrait", ""), "", "");
    // Repackage a contributor-original layout with nonzero page margins.
    let mut zip = zip::ZipArchive::new(Cursor::new(&source)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if name == "styles.xml" {
            data = String::from_utf8(data).unwrap().replace("fo:margin='0pt'", "fo:margin='20pt'").into_bytes();
        }
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    let deck = import(&out.finish().unwrap().into_inner()).unwrap();
    assert!(deck.tags.iter().any(|(key, message)| key == "odp:warning" && message.contains("notes page margins")));
    assert!(export(&deck).is_err());
}

#[test]
fn grouped_canonical_body_uses_the_live_notes_value() {
    let mut page = page("M", 1, "<text:p>Old</text:p>", "");
    page = page.replace("<presentation:notes>", "<presentation:notes><draw:g>").replace("</presentation:notes>", "</draw:g></presentation:notes>");
    let mut deck = import(&fixture(&page, &master("M", "Portrait", ""), "", "")).unwrap();
    Arc::make_mut(&mut deck.slides[0]).notes = TextBody::from_text("New grouped body");
    let bytes = export(&deck).unwrap();
    dump("group", &bytes);
    let reopened = import(&bytes).unwrap();
    no_warnings(&reopened);
    assert_eq!(reopened.slides[0].notes_text(), "New grouped body");
}

#[test]
fn inherited_notes_geometry_stays_live_after_master_edit_with_local_override_control() {
    let inherited =
        page("M", 1, "<text:p>Inherited</text:p>", "").replace("presentation:user-transformed='true'", "presentation:user-transformed='false'");
    let explicit = page("M", 2, "<text:p>Local</text:p>", "");
    let mut deck = import(&fixture(&format!("{inherited}{explicit}"), &master("M", "Portrait", ""), "", "")).unwrap();
    no_warnings(&deck);
    let frame = |deck: &Presentation, index: usize| {
        let page = deck.slides[index].notes_page.as_ref().unwrap();
        find_shape(&page.shapes, page.body_shape.unwrap()).unwrap().xfrm
    };
    assert!(frame(&deck, 0).is_none());
    assert_eq!(frame(&deck, 1).unwrap().y, 320.0);
    let master = Arc::make_mut(&mut deck.notes_masters[0].master);
    let body = master.shapes.iter_mut().find(|shape| shape.ph_type() == Some(PhType::Body)).unwrap();
    body.xfrm.as_mut().unwrap().y = 350.0;
    let bytes = export(&deck).unwrap();
    dump("geometry", &bytes);
    let mut reopened = import(&bytes).unwrap();
    no_warnings(&reopened);
    assert!(frame(&reopened, 0).is_none());
    assert_eq!(frame(&reopened, 1).unwrap().y, 320.0);
    let master = Arc::make_mut(&mut reopened.notes_masters[0].master);
    let body = master.shapes.iter_mut().find(|shape| shape.ph_type() == Some(PhType::Body)).unwrap();
    assert_eq!(body.xfrm.unwrap().y, 350.0);
    body.xfrm.as_mut().unwrap().y = 370.0;
    let bytes = export(&reopened).unwrap();
    let content = part(&bytes, "content.xml");
    assert!(content.contains("presentation:user-transformed=\"false\""));
    assert!(content.contains("svg:y=\"370pt\""), "inherited geometry should follow the edited master");
    let reopened = import(&bytes).unwrap();
    assert!(frame(&reopened, 0).is_none());
    assert_eq!(frame(&reopened, 1).unwrap().y, 320.0);
}

#[test]
fn links_in_canonical_independent_and_master_notes_text_use_live_slide_ids() {
    let linked = "<text:p><text:a xmlns:xlink='http://www.w3.org/1999/xlink' xlink:href='#Destination'>Go</text:a></text:p>";
    let extra =
        format!("<draw:frame svg:x='10pt' svg:y='280pt' svg:width='80pt' svg:height='20pt'><draw:text-box>{linked}</draw:text-box></draw:frame>");
    let pages = format!("{}{}", page("M", 1, linked, &extra), page("M", 2, "", "").replace("<draw:page ", "<draw:page draw:name='Destination' "));
    let mut deck = import(&fixture(&pages, &master("M", "Portrait", &extra), "", "")).unwrap();
    let target = deck.slides[1].id;
    let check = |text: &TextBody, target: SlideId| {
        assert!(
            matches!(&text.paragraphs[0].runs[0].props.link.as_ref().unwrap().action, deckcraft_model::text::Action::Slide { slide } if *slide == target)
        );
    };
    check(&deck.slides[0].notes, target);
    check(deck.slides[0].notes_page.as_ref().unwrap().shapes.last().unwrap().text.as_ref().unwrap(), target);
    check(deck.notes_masters[0].master.shapes.last().unwrap().text.as_ref().unwrap(), target);
    deck.slides.swap(0, 1);
    let reopened = import(&export(&deck).unwrap()).unwrap();
    no_warnings(&reopened);
    let target = reopened.slides[0].id;
    check(&reopened.slides[1].notes, target);
    check(reopened.slides[1].notes_page.as_ref().unwrap().shapes.last().unwrap().text.as_ref().unwrap(), target);
    check(reopened.notes_masters[0].master.shapes.last().unwrap().text.as_ref().unwrap(), target);
}

#[test]
fn hidden_notes_background_and_master_shapes_are_distinct_explicit_overrides() {
    let styles = "<style:style style:name='Hidden' style:family='drawing-page'><style:drawing-page-properties presentation:background-visible='false' presentation:background-objects-visible='false'/></style:style><style:style style:name='Painted' style:family='drawing-page'><style:drawing-page-properties draw:fill='solid' draw:fill-color='#112233'/></style:style>";
    let page = page("M", 1, "", "").replace("<presentation:notes>", "<presentation:notes draw:style-name='Hidden'>");
    let master = master("M", "Portrait", "").replace("<presentation:notes ", "<presentation:notes draw:style-name='Painted' ");
    let deck = import(&fixture(&page, &master, styles, "")).unwrap();
    no_warnings(&deck);
    let page = deck.slides[0].notes_page.as_ref().unwrap();
    assert!(matches!(page.background, Some(Background::Fill { fill: Fill::None })));
    assert!(!page.show_master_shapes);
    assert!(matches!(deck.notes_masters[0].master.background, Some(Background::Fill { fill: Fill::Solid { .. } })));
    let reopened = import(&export(&deck).unwrap()).unwrap();
    no_warnings(&reopened);
    let page = reopened.slides[0].notes_page.as_ref().unwrap();
    assert!(matches!(page.background, Some(Background::Fill { fill: Fill::None })));
    assert!(!page.show_master_shapes);
    assert!(matches!(reopened.notes_masters[0].master.background, Some(Background::Fill { fill: Fill::Solid { .. } })));
}

#[test]
fn missing_notes_master_style_cannot_silently_drop_its_paint() {
    let master = master("M", "Portrait", "").replace("<presentation:notes ", "<presentation:notes draw:style-name='MissingMasterPaint' ");
    let source = fixture(&page("M", 1, "", ""), &master, "", "");
    assert!(import(&source).unwrap_err().0.contains("missing required ODF style in its package part"));
}
