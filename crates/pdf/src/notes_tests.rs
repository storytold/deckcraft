//! Contributor-original structured notes PDF fixtures.
use super::*;
use deckcraft_model::{
    notes::{NotesFields, NotesMaster, NotesPage},
    text::{Hyperlink, RunKind, TextBody},
    *,
};
use hayro_syntax::object::{Array, Dict, Object, Stream};
use std::sync::Arc;

fn text(value: &str) -> TextBody {
    let mut text = TextBody::from_text(value);
    for paragraph in &mut text.paragraphs {
        for run in &mut paragraph.runs {
            run.props.font = Some("Ubuntu".into());
            run.props.size = Some(14.0);
        }
    }
    text
}
fn shape(id: u32, x: f64, y: f64, w: f64, h: f64) -> Shape {
    Shape { id: ShapeId(id), xfrm: Some(Xfrm::new(x, y, w, h)), ..Default::default() }
}
fn fill(r: u8, g: u8, b: u8) -> deckcraft_model::Fill {
    deckcraft_model::Fill::solid(ColorRef::rgb(Rgba::rgb(r, g, b)))
}
fn fixture() -> Presentation {
    let mut p = deckcraft_model::defaults::new_presentation(None);
    p.slide_size = deckcraft_model::Size::new(200.0, 100.0);
    let first = Arc::make_mut(&mut p.slides[0]);
    first.background = Some(Background::Fill { fill: fill(220, 20, 30) });
    first.shapes = vec![Shape { text: Some(text("Live slide")), ..shape(900, 10.0, 10.0, 170.0, 60.0) }];
    first.notes = text("Canonical rich notes");
    first.notes.paragraphs[0].runs[0].props.bold = Some(true);
    first.notes.paragraphs[0].props.background = Some(fill(240, 220, 100));
    let body = Shape { ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }), ..shape(901, 20.0, 220.0, 260.0, 100.0) };
    let thumbnail = Shape { ph: Some(Placeholder { kind: PhType::SlideImage, ..Default::default() }), ..shape(902, 20.0, 50.0, 200.0, 100.0) };
    first.notes_page =
        Some(NotesPage { master: Some(MasterId(910)), body_shape: Some(body.id), shapes: vec![thumbnail, body], ..Default::default() });
    let mut header = text("placeholder");
    header.paragraphs[0].runs[0].kind = RunKind::Field { field: "header".into() };
    header.paragraphs[0].runs[0].text.clear();
    p.notes_masters.push(NotesMaster {
        master: Arc::new(Master {
            id: MasterId(910),
            background: Some(Background::Fill { fill: fill(20, 160, 60) }),
            shapes: vec![
                Shape { fill: Some(fill(250, 200, 10)), ..shape(911, 0.0, 0.0, 10.0, 10.0) },
                Shape {
                    ph: Some(Placeholder { kind: PhType::Header, ..Default::default() }),
                    text: Some(header),
                    ..shape(912, 20.0, 170.0, 260.0, 40.0)
                },
            ],
            ..Default::default()
        }),
        size: deckcraft_model::Size::new(300.0, 400.0),
        fields: NotesFields { header: Some("Live master header".into()), display_header: Some(true), ..Default::default() },
    });
    p.fix_next_id();
    p
}
fn options() -> PdfOptions {
    PdfOptions { layout: PageLayout::Notes, dpi: 72.0, paper: (612.0, 792.0), ..Default::default() }
}
fn artwork(pdf: &hayro_syntax::Pdf) -> Vec<u8> {
    let resources = pdf.pages()[0].raw().get::<Dict<'_>>(b"Resources").unwrap();
    let objects = resources.get::<Dict<'_>>(b"XObject").unwrap();
    let stream = objects.keys().find_map(|name| objects.get::<Stream<'_>>(name)).unwrap();
    let data = stream.decoded().unwrap().into_owned();
    assert_eq!(data.len(), 300 * 400 * 3);
    data
}
fn extract(pdf: &hayro_syntax::Pdf) -> String {
    use hayro_syntax::content::ops::TypedInstruction;
    let page = &pdf.pages()[0];
    let fonts = page.raw().get::<Dict<'_>>(b"Resources").unwrap().get::<Dict<'_>>(b"Font").unwrap();
    let mut maps = HashMap::new();
    for name in fonts.keys() {
        let font = fonts.get::<Dict<'_>>(&name).unwrap();
        let stream = font.get::<Stream<'_>>(b"ToUnicode").unwrap().decoded().unwrap().into_owned();
        let cmap = std::str::from_utf8(&stream).unwrap();
        let mut map = HashMap::<u16, String>::new();
        let mut active = false;
        for line in cmap.lines() {
            if line.ends_with("beginbfchar") {
                active = true;
            } else if line == "endbfchar" {
                active = false;
            } else if active {
                let parts: Vec<_> = line.split_whitespace().collect();
                let cid = u16::from_str_radix(parts[0].trim_matches(['<', '>']), 16).unwrap();
                let units: Vec<_> = parts[1]
                    .trim_matches(['<', '>'])
                    .as_bytes()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|v| u16::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap())
                    .collect();
                map.insert(cid, String::from_utf16(&units).unwrap());
            }
        }
        maps.insert(name.to_vec(), map);
    }
    let mut extracted = String::new();
    let mut current = Vec::new();
    let mut operations = page.typed_operations();
    while let Some(operation) = operations.next() {
        let mut append = |bytes: &[u8]| {
            for pair in bytes.as_chunks::<2>().0 {
                extracted.push_str(maps[&current].get(&u16::from_be_bytes([pair[0], pair[1]])).unwrap());
            }
        };
        match operation {
            TypedInstruction::TextFont(value) => current = value.0.to_vec(),
            TypedInstruction::ShowText(value) => append(value.0.as_bytes()),
            TypedInstruction::ShowTexts(values) => {
                for value in values.0.iter::<Object<'_>>() {
                    if let Object::String(value) = value {
                        append(value.as_bytes());
                    }
                }
            }
            _ => {}
        }
    }
    extracted
}
#[test]
fn structured_notes_pdf_preserves_master_background_rich_body_fields_and_live_preview_edit() {
    let mut p = fixture();
    let bytes = export(&p, &options()).unwrap();
    let pdf = hayro_syntax::Pdf::new(bytes).unwrap();
    assert_eq!(pdf.pages()[0].render_dimensions(), (300.0, 400.0));
    let pixels = artwork(&pdf);
    let at = |pixels: &[u8], x: usize, y: usize| pixels[(y * 300 + x) * 3..(y * 300 + x) * 3 + 3].to_vec();
    assert_eq!(at(&pixels, 290, 390), [20, 160, 60]);
    assert_eq!(at(&pixels, 5, 5), [250, 200, 10]);
    assert_eq!(at(&pixels, 210, 140), [220, 20, 30]);
    let extracted = extract(&pdf);
    for value in ["Canonical rich notes", "Live master header", "Live slide"] {
        assert!(extracted.contains(value), "{extracted:?}");
    }
    Arc::make_mut(&mut p.slides[0]).background = Some(Background::Fill { fill: fill(10, 40, 220) });
    let pdf = hayro_syntax::Pdf::new(export(&p, &options()).unwrap()).unwrap();
    assert_eq!(at(&artwork(&pdf), 210, 140), [10, 40, 220]);
}
#[test]
fn structured_notes_pdf_uses_each_page_size_and_resolves_notes_and_preview_links() {
    let mut p = fixture();
    let mut second = (*p.slides[0]).clone();
    second.id = SlideId(950);
    second.notes_page.as_mut().unwrap().size = Some(deckcraft_model::Size::new(400.0, 300.0));
    p.slides.push(Arc::new(second));
    let link = Hyperlink {
        action: ClickAction::Slide { slide: SlideId(950) },
        tooltip: String::new(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: None,
        sound_name: None,
    };
    let first = Arc::make_mut(&mut p.slides[0]);
    first.notes.paragraphs[0].runs[0].props.link = Some(link.clone());
    first.shapes[0].click = Some(link);
    for text_layer in [true, false] {
        let pdf = hayro_syntax::Pdf::new(export(&p, &PdfOptions { text_layer, ..options() }).unwrap()).unwrap();
        assert_eq!(pdf.pages()[0].render_dimensions(), (300.0, 400.0));
        assert_eq!(pdf.pages()[1].render_dimensions(), (400.0, 300.0));
        let page = &pdf.pages()[0];
        let annotations = page.raw().get::<Array<'_>>(b"Annots").unwrap();
        let links: Vec<_> = annotations.iter::<Dict<'_>>().collect();
        assert!(links.len() >= 2, "canonical notes and live preview both have links");
        for link in links {
            assert!(link.get::<Array<'_>>(b"Dest").is_some());
        }
        let fonts = page.raw().get::<Dict<'_>>(b"Resources").unwrap().get::<Dict<'_>>(b"Font");
        assert_eq!(fonts.is_some_and(|fonts| !fonts.is_empty()), text_layer);
    }
}
#[test]
fn invalid_structured_notes_references_refuse_pdf_export() {
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap().master = Some(MasterId(999));
    assert!(matches!(export(&p, &options()), Err(PdfError::Write(_))));
}

#[test]
fn live_preview_overlay_retains_group_mapping_rotation_and_mirroring() {
    let p = fixture();
    let ctx = deckcraft_model::resolve::Ctx::for_slide(&p, &p.slides[0]).unwrap();
    let mut thumbnail = shape(980, 10.0, 5.0, 80.0, 40.0);
    thumbnail.ph = Some(Placeholder { kind: PhType::SlideImage, ..Default::default() });
    let x = thumbnail.xfrm.as_mut().unwrap();
    x.rot = 90.0;
    x.flip_h = true;
    let group = Shape {
        kind: ShapeKind::Group { children: vec![thumbnail], child: Xfrm::new(0.0, 0.0, 100.0, 50.0) },
        ..shape(981, 100.0, 50.0, 200.0, 100.0)
    };
    let views = thumbnail_views(&ctx, &[group], p.slide_size);
    assert_eq!(views.len(), 1);
    let (view, _) = views[0];
    let center = view * kurbo::Point::new(100.0, 50.0);
    let corner = view * kurbo::Point::new(0.0, 0.0);
    assert!((center.x - 200.0).abs() < 1e-9 && (center.y - 100.0).abs() < 1e-9);
    assert!((corner.x - 240.0).abs() < 1e-9 && (corner.y - 180.0).abs() < 1e-9);
}
