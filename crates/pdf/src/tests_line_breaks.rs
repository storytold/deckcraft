//! Breaks occupy one logical character even though their stored run.text is empty.
use super::*;
use deckcraft_model::text::{Bullet, RunProps, TextBody};
use deckcraft_model::{Master, PhType, Placeholder, Shape, ShapeId, Xfrm};
use hayro_syntax::{
    content::ops::TypedInstruction,
    object::{Dict, Object, Stream},
};
use std::sync::Arc;

fn body(text: &str) -> TextBody {
    let mut body = TextBody::default();
    let props = RunProps { font: Some("Ubuntu".into()), size: Some(20.0), ..Default::default() };
    deckcraft_model::edit::insert(&mut body, (0, 0), text, Some(props));
    for paragraph in &mut body.paragraphs {
        paragraph.props.bullet = Some(Bullet::None);
    }
    body
}

fn presentation(text: &str) -> Presentation {
    let mut pres = Presentation::default();
    Arc::make_mut(&mut pres.slides[0]).shapes =
        vec![Shape { id: ShapeId(124), xfrm: Some(Xfrm::new(20.0, 20.0, 600.0, 400.0)), text: Some(body(text)), ..Default::default() }];
    pres
}

// The same bounded two-byte-CID/bfchar fixture decoding used by tests.rs. The
// actual content strings and emitted ToUnicode map are checked, not PDF metadata.
fn page_text(bytes: Vec<u8>) -> String {
    let pdf = hayro_syntax::Pdf::new(bytes).unwrap();
    assert_eq!(pdf.pages().len(), 1);
    let page = &pdf.pages()[0];
    let resources = page.raw().get::<Dict<'_>>(b"Resources").unwrap();
    let Some(fonts) = resources.get::<Dict<'_>>(b"Font") else { return String::new() };
    assert!(!fonts.is_empty() && fonts.len() <= 8, "bounded generated fixture");
    eprintln!("line-break PDF fixture emitted {} fonts", fonts.len());
    let mut mappings = HashMap::new();
    for name in fonts.keys() {
        let font = fonts.get::<Dict<'_>>(&name).unwrap();
        let cmap = font.get::<Stream<'_>>(b"ToUnicode").unwrap().decoded().unwrap().into_owned();
        let cmap = std::str::from_utf8(&cmap).unwrap();
        assert!(cmap.len() < 64 * 1024);
        let mut mapping = HashMap::<u16, String>::new();
        let mut in_char_map = false;
        for line in cmap.lines() {
            if line.ends_with("beginbfchar") {
                in_char_map = true;
            } else if line == "endbfchar" {
                in_char_map = false;
            } else if in_char_map {
                let parts: Vec<_> = line.split_whitespace().collect();
                assert_eq!(parts.len(), 2);
                let cid = u16::from_str_radix(parts[0].trim_matches(['<', '>']), 16).unwrap();
                let destination = parts[1].trim_matches(['<', '>']);
                assert!(!destination.is_empty() && destination.len().is_multiple_of(4));
                let units: Vec<_> = destination
                    .as_bytes()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|chunk| u16::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap())
                    .collect();
                assert!(mapping.len() < 1024);
                mapping.insert(cid, String::from_utf16(&units).unwrap());
            }
        }
        mappings.insert(name.to_vec(), mapping);
    }
    let mut extracted = String::new();
    let mut current_font: Option<Vec<u8>> = None;
    let append = |bytes: &[u8], mapping: &HashMap<u16, String>, extracted: &mut String| {
        assert!(bytes.len() < 4096);
        assert!(bytes.len().is_multiple_of(2));
        for pair in bytes.as_chunks::<2>().0 {
            extracted.push_str(mapping.get(&u16::from_be_bytes(*pair)).unwrap());
        }
    };
    let mut operations = page.typed_operations();
    while let Some(operation) = operations.next() {
        match operation {
            TypedInstruction::TextFont(value) => {
                assert!(mappings.contains_key(value.0.as_ref()));
                current_font = Some(value.0.to_vec());
            }
            TypedInstruction::ShowText(value) => append(value.0.as_bytes(), mappings.get(current_font.as_ref().unwrap()).unwrap(), &mut extracted),
            TypedInstruction::ShowTexts(values) => {
                for value in values.0.iter::<Object<'_>>() {
                    if let Object::String(value) = value {
                        append(value.as_bytes(), mappings.get(current_font.as_ref().unwrap()).unwrap(), &mut extracted);
                    }
                }
            }
            _ => (),
        }
    }
    assert!(extracted.len() < 8192);
    extracted
}

#[test]
fn line_breaks_preserve_actual_pdf_text_and_native_character_ranges() {
    for text in ["ALPHABETA", "ALPHA\u{b}BETA", "\u{b}ALPHA\u{b}\u{b}BETA\u{b}", "Café e\u{301}\u{b}Ελληνικά", "\u{b}\u{b}"] {
        let pres = presentation(text);
        let placed = deckcraft_render::place_slide(&pres, 0);
        let placed = &placed.texts[0];
        assert_eq!(placed.body.paragraphs.len(), 1);
        assert_eq!(placed.body.paragraphs[0].text(), text);
        assert_eq!(placed.layout.lines.len(), text.chars().filter(|&c| c == '\u{b}').count() + 1);
        for run in &placed.layout.runs {
            assert!(!run.glyphs.is_empty());
            assert!(run.glyphs.iter().all(|glyph| glyph.0 != 0));
            let logical: String = text.chars().skip(run.chars.0).take(run.chars.1 - run.chars.0).collect();
            assert!(!logical.contains('\u{b}'));
        }
        let before = pres.clone();
        eprintln!("line-break source {text:?}; native faces {:?}", placed.layout.runs.iter().map(|r| r.face.family.as_str()).collect::<Vec<_>>());
        let bytes = export(&pres, &PdfOptions { dpi: 48.0, ..Default::default() }).unwrap();
        assert_eq!(page_text(bytes), text.replace('\u{b}', ""));
        assert_eq!(pres, before, "export keeps source model unchanged");
    }
}

#[test]
fn line_breaks_preserve_master_pieces_and_notes_handout_pdf_text() {
    for header_text in ["HeadA\u{b}HeadB", "Café\u{b}\u{b}Ελληνικά\u{b}"] {
        let mut pres = presentation("SlideA\u{b}SlideB");
        pres.notes_size = deckcraft_model::Size::new(612.0, 792.0);
        let master = Arc::new(Master {
            shapes: vec![Shape {
                id: ShapeId(125),
                xfrm: Some(Xfrm::new(20.0, 20.0, 500.0, 150.0)),
                ph: Some(Placeholder { kind: PhType::Header, ..Default::default() }),
                text: Some(body(header_text)),
                ..Default::default()
            }],
            ..Default::default()
        });
        let pieces = master_text_pieces(&pres, &master, (612.0, 792.0), 1);
        let pieces_text: String = pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(pieces_text, header_text.replace('\u{b}', ""));
        assert!(pieces.len() >= 2);
        assert!(pieces.first().unwrap().at.1 < pieces.last().unwrap().at.1);
        pres.notes_master = Some(master.clone());
        pres.handout_master = Some(master);
        for layout in [PageLayout::Notes, PageLayout::Handouts { per_page: 3 }] {
            let bytes = export(&pres, &PdfOptions { layout, dpi: 48.0, ..Default::default() }).unwrap();
            let text = page_text(bytes);
            assert_eq!(text.matches("SlideASlideB").count(), 1, "{layout:?}: {text:?}");
            assert_eq!(text.matches(&header_text.replace('\u{b}', "")).count(), 1, "{layout:?}: {text:?}");
        }
    }
}
