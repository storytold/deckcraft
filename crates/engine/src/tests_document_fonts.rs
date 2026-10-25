use std::sync::Arc;

use deckcraft_model::text::{Align, Anchor, AutoFit, Bullet, TextBody};
use deckcraft_model::{ColorRef, EmbeddedFont, Fill, Presentation, Rgba, Shape, ShapeId, Xfrm};
use serde_json::json;

use crate::{DocState, Session, cmd};

const FAMILY: &str = "DeckCraft ScopeA";
const FONT_REFS: [&str; 4] = [FAMILY, "+mn-lt", "+mj-lt", FAMILY];

/// Derive two faces from the contributor-original CC0 fixture, without adding assets or
/// registering a font directly. The private test family also avoids other parallel tests.
fn font_bytes(advance: u16) -> Vec<u8> {
    let mut bytes = include_bytes!("../../fonts/tests/fixtures/legacy-regular.ttf").to_vec();
    for (from, to) in [
        (b"DeckCraft Legacy".to_vec(), FAMILY.as_bytes().to_vec()),
        ("DeckCraft Legacy".encode_utf16().flat_map(u16::to_be_bytes).collect(), FAMILY.encode_utf16().flat_map(u16::to_be_bytes).collect()),
    ] {
        assert_eq!(from.len(), to.len());
        let offsets: Vec<_> = bytes.windows(from.len()).enumerate().filter_map(|(i, s)| (s == from).then_some(i)).collect();
        assert!(!offsets.is_empty());
        for offset in offsets {
            bytes[offset..offset + to.len()].copy_from_slice(&to);
        }
    }
    let tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    let hmtx = (0..tables).map(|i| 12 + 16 * i).find(|&r| &bytes[r..r + 4] == b"hmtx").unwrap();
    let offset = u32::from_be_bytes(bytes[hmtx + 8..hmtx + 12].try_into().unwrap()) as usize;
    // Glyph 1 is A; its outline is unchanged, only its advance differs (1000 units/em).
    bytes[offset + 4..offset + 6].copy_from_slice(&advance.to_be_bytes());
    bytes
}

fn deck(advance: Option<u16>) -> String {
    let mut p = Presentation::default();
    for master in &mut p.masters {
        let master = Arc::make_mut(master);
        master.shapes.clear();
        for layout in &mut master.layouts {
            layout.shapes.clear();
        }
        for set in [&mut master.theme.fonts.major, &mut master.theme.fonts.minor] {
            set.latin = FAMILY.into();
            set.ea = FAMILY.into();
            set.cs = FAMILY.into();
        }
    }
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes.clear();
    slide.show_master_shapes = false;
    for (i, font) in FONT_REFS.iter().enumerate() {
        let mut body = TextBody::from_text("AAA");
        body.body.inset_l = Some(0.0);
        body.body.inset_r = Some(0.0);
        body.body.inset_t = Some(0.0);
        body.body.inset_b = Some(0.0);
        body.body.anchor = Some(Anchor::Top);
        body.body.autofit = Some(AutoFit::None);
        let para = &mut body.paragraphs[0];
        para.props.align = Some(Align::Left);
        para.props.bullet = Some(Bullet::None);
        if i == 3 {
            para.props.bullet = Some(Bullet::Char { char: "A".into() });
            para.props.bullet_font = Some(FAMILY.into());
            para.props.bullet_size = Some(1.0);
            para.props.margin_left = Some(40.0);
            para.props.indent = Some(-40.0);
        }
        let props = &mut para.runs[0].props;
        props.font = Some((*font).into());
        props.font_ea = Some(FAMILY.into());
        props.font_cs = Some(FAMILY.into());
        props.font_sym = Some(FAMILY.into());
        props.size = Some(40.0);
        props.bold = Some(false);
        props.italic = Some(false);
        props.fill = Some(Fill::solid(ColorRef::rgb(Rgba::BLACK)));
        slide.shapes.push(Shape {
            id: ShapeId(100 + i as u32),
            name: format!("font-reference-{i}"),
            xfrm: Some(Xfrm::new(20.0, 20.0 + 80.0 * i as f64, 400.0, 70.0)),
            fill: Some(Fill::None),
            text: Some(body),
            text_box: true,
            ..Default::default()
        });
    }
    if let Some(advance) = advance {
        let id = p.add_media("scope.ttf", "font/ttf", font_bytes(advance)).unwrap();
        p.embedded_fonts.push(EmbeddedFont { typeface: FAMILY.into(), regular: Some(id), ..Default::default() });
    }
    cmd::base64_encode(&cmd::file::save_bytes(&p, "pptx").unwrap())
}

fn open(s: &mut Session, name: &str, data: &str) -> usize {
    let result = s.execute("file.openBytes", &json!({"name": name, "data": data})).unwrap();
    assert_eq!(result["missingFonts"], json!([]), "{name}: {result}");
    let st = s.doc().unwrap();
    assert!(st.missing_fonts.is_empty());
    assert!(!st.is_dirty(), "installing an embedded font must not dirty {name}");
    s.active_index().unwrap()
}

fn assert_original_names(st: &DocState) {
    let p = &st.doc;
    let fonts = &p.masters[0].theme.fonts;
    for set in [&fonts.major, &fonts.minor] {
        assert_eq!(set.latin, FAMILY);
        assert_eq!(set.ea, FAMILY);
        assert_eq!(set.cs, FAMILY);
    }
    assert_eq!(p.embedded_fonts[0].typeface, FAMILY);
    assert_eq!(p.slides[0].shapes.len(), FONT_REFS.len());
    for (shape, expected) in p.slides[0].shapes.iter().zip(FONT_REFS) {
        let para = &shape.text.as_ref().unwrap().paragraphs[0];
        let props = &para.runs[0].props;
        assert_eq!(props.font.as_deref(), Some(expected));
        assert_eq!(props.font_ea.as_deref(), Some(FAMILY));
        assert_eq!(props.font_cs.as_deref(), Some(FAMILY));
        assert_eq!(props.font_sym.as_deref(), Some(FAMILY));
    }
    let bullet = &p.slides[0].shapes[3].text.as_ref().unwrap().paragraphs[0].props;
    assert_eq!(bullet.bullet_font.as_deref(), Some(FAMILY));
}

fn assert_rendered_advances(st: &DocState, advance: u16) -> Vec<u8> {
    assert_original_names(st);
    let placed = deckcraft_render::place_slide(&st.doc, 0);
    assert_eq!(placed.texts.len(), FONT_REFS.len());
    let expected = f64::from(advance) * 40.0 / 1000.0;
    for (i, text) in placed.texts.iter().enumerate() {
        let layout = &text.layout;
        assert_eq!(layout.lines.len(), 1, "reference {}", FONT_REFS[i]);
        let line = &layout.lines[0];
        assert!(
            (line.extent.1 - line.extent.0 - 3.0 * expected).abs() < 0.01,
            "reference {}: {:?}, expected {}",
            FONT_REFS[i],
            line.extent,
            3.0 * expected
        );
        let glyphs: Vec<_> = layout.runs.iter().filter(|r| !r.cells.is_empty()).flat_map(|r| &r.glyphs).collect();
        assert_eq!(glyphs.len(), 3);
        for pair in glyphs.windows(2) {
            assert!((pair[1].1 - pair[0].1 - expected).abs() < 0.01, "reference {}: {glyphs:?}", FONT_REFS[i]);
        }
        if i == 3 {
            let bullet = layout.runs.iter().find(|r| r.cells.is_empty()).expect("rendered bullet");
            assert_eq!(bullet.glyphs.len(), 1);
            let width = bullet.face.advance(bullet.glyphs[0].0) * bullet.size / bullet.face.upem;
            assert!((width - expected).abs() < 0.01, "embedded bullet face: {width} vs {expected}");
        }
    }
    let image = deckcraft_render::render_slide(&st.doc, 0, &deckcraft_render::RenderOpts::default());
    assert!(
        image.pixels.as_chunks::<4>().0.iter().any(|px| px[3] > 0 && px[0] < 64 && px[1] < 64 && px[2] < 64),
        "the embedded squares must actually be painted"
    );
    image.pixels
}

#[test]
fn opening_two_decks_isolates_embedded_font_metrics_and_preserves_saved_names() {
    let mut s = Session::new();
    let first = open(&mut s, "narrow.pptx", &deck(Some(700)));
    let narrow = assert_rendered_advances(s.doc().unwrap(), 700);
    let second = open(&mut s, "wide.pptx", &deck(Some(900)));
    let wide = assert_rendered_advances(s.doc().unwrap(), 900);
    assert_ne!(narrow, wide, "different advances must change the raster, not just a font lookup result");
    s.set_active(first);
    assert_eq!(assert_rendered_advances(s.doc().unwrap(), 700), narrow, "opening another deck must not change the first deck");

    for (document, advance) in [(first, 700), (second, 900)] {
        for format in ["deckcraft", "pptx"] {
            s.set_active(document);
            let saved = s.execute("file.saveBytes", &json!({"format": format})).unwrap();
            open(&mut s, &format!("roundtrip.{format}"), saved["data"].as_str().unwrap());
            assert_rendered_advances(s.doc().unwrap(), advance);
        }
    }
    s.set_active(first);
    assert_eq!(assert_rendered_advances(s.doc().unwrap(), 700), narrow);
    s.set_active(second);
    assert_eq!(assert_rendered_advances(s.doc().unwrap(), 900), wide);

    // A third document that names the same family but does not embed it must not borrow either
    // deck's private face, or silently stop reporting that the original family is missing.
    let result = s.execute("file.openBytes", &json!({"name": "unembedded.pptx", "data": deck(None)})).unwrap();
    assert_eq!(result["missingFonts"], json!([FAMILY]));
    assert_eq!(s.doc().unwrap().missing_fonts, vec![FAMILY.to_string()]);
    assert!(!s.doc().unwrap().is_dirty());
}

/// Append a replacement SFNT table; the original fixture's outlines and all other tables stay.
fn replace_table(bytes: &mut Vec<u8>, tag: &[u8; 4], table: &[u8]) {
    let tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    let record = (0..tables).map(|i| 12 + 16 * i).find(|&r| &bytes[r..r + 4] == tag).unwrap();
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    let offset = bytes.len() as u32;
    bytes.extend_from_slice(table);
    bytes[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
    bytes[record + 12..record + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
}

/// Remap the CC0 square to exactly the requested characters. In particular, the synthetic
/// Symbol face has only Greek Alpha: U+F041 must use Symbol's Unicode remapping, not a PUA cmap.
fn slot_font(family: &str, advance: u16, characters: &[u32], semibold: bool) -> Vec<u8> {
    let mut bytes = if semibold {
        include_bytes!("../../fonts/tests/fixtures/legacy-semibold.ttf").to_vec()
    } else {
        include_bytes!("../../fonts/tests/fixtures/legacy-regular.ttf").to_vec()
    };
    let style = if semibold { "Semibold" } else { "Regular" };
    let mut names = vec![0; 6 + 4 * 12];
    names[2..4].copy_from_slice(&4_u16.to_be_bytes());
    names[4..6].copy_from_slice(&(54_u16).to_be_bytes());
    for (i, (id, text)) in [(1_u16, family), (2, style), (16, family), (17, style)].into_iter().enumerate() {
        let value: Vec<_> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let offset = (names.len() - 54) as u16;
        let record = 6 + 12 * i;
        for (j, value) in [3_u16, 1, 0x0409, id, value.len() as u16, offset].into_iter().enumerate() {
            names[record + 2 * j..record + 2 * j + 2].copy_from_slice(&value.to_be_bytes());
        }
        names.extend_from_slice(&value);
    }
    replace_table(&mut bytes, b"name", &names);
    // One Windows UCS-4 format-12 subtable, with one single-character group per mapping.
    let mut cmap = Vec::new();
    for value in [0_u16, 1, 3, 10] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12_u32.to_be_bytes());
    cmap.extend_from_slice(&12_u16.to_be_bytes());
    cmap.extend_from_slice(&0_u16.to_be_bytes());
    for value in [16 + 12 * characters.len() as u32, 0, characters.len() as u32] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    for &character in characters {
        for value in [character, character, 1] {
            cmap.extend_from_slice(&value.to_be_bytes());
        }
    }
    replace_table(&mut bytes, b"cmap", &cmap);
    let mut hmtx = Vec::new();
    for value in [500_u16, 0, advance, 100] {
        hmtx.extend_from_slice(&value.to_be_bytes());
    }
    replace_table(&mut bytes, b"hmtx", &hmtx);
    bytes
}

#[test]
fn embedded_fonts_resolve_script_slots_symbol_remapping_and_bold_style() {
    const LATIN: &str = "DeckCraft Slot Latin";
    const EA: &str = "DeckCraft Slot EastAsian";
    const CS: &str = "DeckCraft Slot Complex";
    let mut p = Presentation::default();
    for master in &mut p.masters {
        let master = Arc::make_mut(master);
        master.shapes.clear();
        for layout in &mut master.layouts {
            layout.shapes.clear();
        }
        master.theme.fonts.minor.latin = LATIN.into();
        master.theme.fonts.minor.ea = EA.into();
        master.theme.fonts.minor.cs = CS.into();
        master.theme.fonts.minor.scripts.clear();
    }
    for (family, advance, characters, bold) in [
        (LATIN, 400, &[0x0041][..], false),
        (LATIN, 850, &[0x0041][..], true),
        (EA, 780, &[0x4E00][..], false),
        (CS, 930, &[0x0627][..], false),
        ("Symbol", 1050, &[0x0391][..], false),
    ] {
        let id = p.add_media(&format!("slot-{advance}.ttf"), "font/ttf", slot_font(family, advance, characters, bold)).unwrap();
        if bold {
            p.embedded_fonts.iter_mut().find(|font| font.typeface == family).unwrap().bold = Some(id);
        } else {
            p.embedded_fonts.push(EmbeddedFont { typeface: family.into(), regular: Some(id), ..Default::default() });
        }
    }
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes.clear();
    slide.show_master_shapes = false;
    for (i, text) in ["AA", "AA", "一一", "اا", "\u{F041}\u{F041}"].into_iter().enumerate() {
        let mut body = TextBody::from_text(text);
        body.body.autofit = Some(AutoFit::None);
        body.body.anchor = Some(Anchor::Top);
        let para = &mut body.paragraphs[0];
        para.props.align = Some(Align::Left);
        para.props.bullet = Some(Bullet::None);
        let props = &mut para.runs[0].props;
        props.font = Some(LATIN.into());
        props.font_ea = Some("+mn-ea".into());
        props.font_cs = Some("+mn-cs".into());
        props.font_sym = Some("Symbol".into());
        props.bold = Some(i == 1);
        props.italic = Some(false);
        props.size = Some(40.0);
        props.fill = Some(Fill::solid(ColorRef::rgb(Rgba::BLACK)));
        slide.shapes.push(Shape {
            id: ShapeId(200 + i as u32),
            name: format!("font-slot-{i}"),
            xfrm: Some(Xfrm::new(20.0, 20.0 + 80.0 * i as f64, 400.0, 70.0)),
            fill: Some(Fill::None),
            text: Some(body),
            text_box: true,
            ..Default::default()
        });
    }
    let mut s = Session::new();
    let data = cmd::base64_encode(&cmd::file::save_bytes(&p, "pptx").unwrap());
    open(&mut s, "font-slots.pptx", &data);
    let placed = deckcraft_render::place_slide(&s.doc().unwrap().doc, 0);
    assert_eq!(placed.texts.len(), 5);
    for ((text, advance), label) in placed.texts.iter().zip([400, 850, 780, 930, 1050]).zip(["regular", "bold", "EA", "CS", "Symbol"]) {
        let layout = &text.layout;
        assert_eq!(layout.lines.len(), 1, "{label}");
        let width = layout.lines[0].extent.1 - layout.lines[0].extent.0;
        let expected = 2.0 * f64::from(advance) * 40.0 / 1000.0;
        assert!((width - expected).abs() < 0.01, "{label}: width {width}, expected {expected}");
        let runs: Vec<_> = layout.runs.iter().filter(|run| !run.cells.is_empty()).collect();
        assert_eq!(runs.iter().map(|run| run.glyphs.len()).sum::<usize>(), 2, "{label}");
        assert!(runs.iter().flat_map(|run| &run.glyphs).all(|glyph| glyph.0 == 1), "{label}: draw the embedded square, not a fallback or .notdef");
        assert!(runs.iter().all(|run| !run.fake_bold), "{label}: select the embedded weight, not synthetic bold");
    }
    let image = deckcraft_render::render_slide(&s.doc().unwrap().doc, 0, &deckcraft_render::RenderOpts::default());
    // Every row, including the PUA Symbol row, must paint its two real embedded outlines.
    for row in 0..5 {
        let painted = (20 + 80 * row..90 + 80 * row)
            .any(|y| (20..420).any(|x| image.straight_at(x, y).is_some_and(|px| px[3] > 0 && px[0] < 64 && px[1] < 64 && px[2] < 64)));
        assert!(painted, "font slot row {row} must actually be rendered");
    }
}

#[test]
fn pasting_picture_keeps_source_document_embedded_font() {
    let mut s = Session::new();
    let source = open(&mut s, "source.pptx", &deck(Some(700)));
    let destination = open(&mut s, "destination.pptx", &deck(Some(900)));
    s.set_active(source);
    let st = s.doc().unwrap();
    let expected = deckcraft_render::render_shape(&st.doc, st.current_slide().unwrap(), ShapeId(100), 2.0).unwrap().to_png();
    s.execute("edit.select", &json!({"ids": [100]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.set_active(destination);
    let result = s.execute("edit.pasteSpecial", &json!({"as": "picture"})).unwrap();
    let id = ShapeId(result["id"].as_u64().or_else(|| result["ids"][0].as_u64()).unwrap() as u32);
    let st = s.doc().unwrap();
    let deckcraft_model::ShapeKind::Picture { fill } = &st.shape(id).unwrap().kind else { panic!("expected pasted picture") };
    assert_eq!(*st.doc.media(fill.media).unwrap().data, expected, "picture must retain the copied document's glyph advances");
}

#[test]
fn pasted_shapes_keep_source_only_embedded_families() {
    for (mode, id) in [("destinationTheme", 100), ("keepSource", 101)] {
        let mut s = Session::new();
        let source = open(&mut s, "source.pptx", &deck(Some(700)));
        s.execute("file.new", &json!({})).unwrap();
        let destination = s.active_index().unwrap();
        s.set_active(source);
        s.execute("edit.select", &json!({"ids": [id]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        s.set_active(destination);
        s.execute("edit.pasteSpecial", &json!({"as": mode})).unwrap();
        let placed = deckcraft_render::place_slide(&s.doc().unwrap().doc, 0);
        let line = &placed.texts.last().unwrap().layout.lines[0];
        assert!((line.extent.1 - line.extent.0 - 84.0).abs() < 0.01, "{mode}: pasted explicit font must keep its advances");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.embedded_fonts.is_empty(), "one undo removes both pasted shapes and their imported fonts");
        s.execute("edit.redo", &json!({})).unwrap();
        for format in ["deckcraft", "pptx"] {
            let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, format).unwrap();
            open(&mut s, &format!("pasted.{format}"), &cmd::base64_encode(&bytes));
            let placed = deckcraft_render::place_slide(&s.doc().unwrap().doc, 0);
            let line = &placed.texts.last().unwrap().layout.lines[0];
            assert!((line.extent.1 - line.extent.0 - 84.0).abs() < 0.01, "{mode}/{format}: retained source-only face must survive save and reopen");
        }
    }
}

#[test]
fn pasted_shapes_keep_the_destinations_existing_embedded_family() {
    let mut s = Session::new();
    let source = open(&mut s, "source.pptx", &deck(Some(700)));
    let destination = open(&mut s, "destination.pptx", &deck(Some(900)));
    s.set_active(source);
    s.execute("edit.select", &json!({"ids": [100]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.set_active(destination);
    s.execute("edit.paste", &json!({})).unwrap();
    let placed = deckcraft_render::place_slide(&s.doc().unwrap().doc, 0);
    let line = &placed.texts.last().unwrap().layout.lines[0];
    assert!((line.extent.1 - line.extent.0 - 108.0).abs() < 0.01, "an existing destination family must not be overwritten");
    assert_eq!(s.doc().unwrap().doc.embedded_fonts.len(), 1);
}

#[test]
fn rich_text_paste_fits_using_the_retained_embedded_font() {
    let mut s = Session::new();
    let source = open(&mut s, "source.pptx", &deck(Some(700)));
    let mut p = Presentation::default();
    let mut body = TextBody::from_text("");
    body.body.autofit = Some(AutoFit::Shape);
    body.body.wrap = Some(false);
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes.clear();
    slide.shapes.push(Shape {
        id: ShapeId(200),
        text: Some(body),
        text_box: true,
        xfrm: Some(Xfrm::new(20.0, 20.0, 30.0, 30.0)),
        ..Default::default()
    });
    s.add_document(DocState::new(p, None, "Destination".into()));
    let destination = s.active_index().unwrap();
    s.set_active(source);
    s.execute("text.edit", &json!({"id": 100})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.set_active(destination);
    s.execute("text.edit", &json!({"id": 200})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let fitted = s.doc().unwrap().shape(ShapeId(200)).unwrap().xfrm.unwrap();
    cmd::text::fit_text_box(&mut s, ShapeId(200)).unwrap();
    let refitted = s.doc().unwrap().shape(ShapeId(200)).unwrap().xfrm.unwrap();
    assert!((fitted.w - refitted.w).abs() < 0.01, "paste must fit once with the actual face, not retain fallback dimensions");
}

#[test]
fn pasting_shape_as_plain_text_does_not_import_its_font() {
    let mut s = Session::new();
    let source = open(&mut s, "source.pptx", &deck(Some(700)));
    s.execute("file.openBytes", &json!({"name": "unembedded.pptx", "data": deck(None)})).unwrap();
    let destination = s.active_index().unwrap();
    s.set_active(source);
    s.execute("edit.select", &json!({"ids": [100]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.set_active(destination);
    s.execute("text.edit", &json!({"id": 100})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.embedded_fonts.is_empty(), "plain text must not change the destination's existing family resolution");
    assert!(s.doc().unwrap().doc.font_aliases.is_empty());
}

#[test]
fn pasted_slide_fonts_follow_the_resulting_layout_not_the_current_slide() {
    let mut s = Session::new();
    let source = open(&mut s, "source.pptx", &deck(Some(700)));
    let source_layout = s.doc().unwrap().doc.slides[0].layout;
    let mut p = Presentation::default();
    let mut other = (*p.masters[0]).clone();
    other.id = deckcraft_model::MasterId(9001);
    other.theme.fonts.minor.latin = FAMILY.into();
    other.layouts[0].id = source_layout;
    other.shapes.clear();
    other.layouts[0].shapes.clear();
    let first = Arc::make_mut(&mut p.masters[0]);
    first.layouts[0].id = deckcraft_model::LayoutId(9000);
    first.shapes.clear();
    first.layouts[0].shapes.clear();
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.layout = deckcraft_model::LayoutId(9000);
    slide.shapes.clear();
    p.masters.push(Arc::new(other));
    s.add_document(DocState::new(p, None, "Multi-master destination".into()));
    let destination = s.active_index().unwrap();
    s.set_active(source);
    s.edit(|doc, _| {
        let slide = Arc::make_mut(&mut doc.slides[0]);
        slide.shapes.retain(|shape| shape.id == ShapeId(101));
        let props = &mut slide.shapes[0].text.as_mut().unwrap().paragraphs[0].runs[0].props;
        props.font_ea = None;
        props.font_cs = None;
        props.font_sym = None;
        Ok(())
    })
    .unwrap();
    s.execute("edit.copy", &json!({"scope": "slides"})).unwrap();
    s.set_active(destination);
    s.execute("edit.paste", &json!({})).unwrap();
    let placed = deckcraft_render::place_slide(&s.doc().unwrap().doc, 1);
    let line = &placed.texts[0].layout.lines[0];
    assert!((line.extent.1 - line.extent.0 - 84.0).abs() < 0.01, "the copied slide's retained layout resolves its theme font");
}

#[test]
fn pasted_placeholder_fonts_follow_the_resulting_nonplaceholder_style() {
    let mut s = Session::new();
    let source = open(&mut s, "source.pptx", &deck(Some(700)));
    s.edit(|doc, _| {
        let shape = &mut Arc::make_mut(&mut doc.slides[0]).shapes[0];
        shape.ph = Some(deckcraft_model::Placeholder { kind: deckcraft_model::PhType::Title, ..Default::default() });
        let props = &mut shape.text.as_mut().unwrap().paragraphs[0].runs[0].props;
        props.font = None;
        props.font_ea = None;
        props.font_cs = None;
        props.font_sym = None;
        Ok(())
    })
    .unwrap();
    let mut destination = Presentation::default();
    let mut level = deckcraft_model::text::LevelStyle::default();
    level.run.font = Some(FAMILY.into());
    destination.default_text_style.set(0, level);
    let master = Arc::make_mut(&mut destination.masters[0]);
    let mut title = deckcraft_model::text::LevelStyle::default();
    title.run.font = Some("Unembedded placeholder family".into());
    master.title_style.set(0, title);
    master.shapes.clear();
    master.layouts[0].shapes.clear();
    Arc::make_mut(&mut destination.slides[0]).shapes.clear();
    s.add_document(DocState::new(destination, None, "Destination".into()));
    let destination = s.active_index().unwrap();
    s.set_active(source);
    s.execute("edit.select", &json!({"ids": [100]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.set_active(destination);
    s.execute("edit.paste", &json!({})).unwrap();
    let p = &s.doc().unwrap().doc;
    assert!(p.slides[0].shapes[0].ph.is_none());
    let placed = deckcraft_render::place_slide(p, 0);
    let line = &placed.texts[0].layout.lines[0];
    assert!((line.extent.1 - line.extent.0 - 84.0).abs() < 0.01, "pasted nonplaceholder uses its resulting default font");
}

#[test]
fn pasted_text_retains_embedded_theme_script_family() {
    const ARABIC: &str = "DeckCraft Pasted Arabic";
    let mut source = Presentation::default();
    let id = source.add_media("arabic.ttf", "font/ttf", slot_font(ARABIC, 930, &[0x0627], false)).unwrap();
    source.embedded_fonts.push(EmbeddedFont { typeface: ARABIC.into(), regular: Some(id), ..Default::default() });
    let mut destination = source.clone();
    destination.embedded_fonts.clear();
    destination.media.clear();
    for p in [&mut source, &mut destination] {
        let master = Arc::make_mut(&mut p.masters[0]);
        master.shapes.clear();
        master.layouts[0].shapes.clear();
        master.theme.fonts.minor.cs.clear();
        master.theme.fonts.minor.scripts = vec![deckcraft_model::theme::ScriptFont { script: "Arab".into(), typeface: ARABIC.into() }];
        Arc::make_mut(&mut p.slides[0]).shapes.clear();
    }
    let mut body = TextBody::from_text("اا");
    body.body.autofit = Some(AutoFit::None);
    body.paragraphs[0].props.bullet = Some(Bullet::None);
    body.paragraphs[0].runs[0].props.size = Some(40.0);
    Arc::make_mut(&mut source.slides[0]).shapes.push(Shape {
        id: ShapeId(100),
        xfrm: Some(Xfrm::new(20.0, 20.0, 400.0, 70.0)),
        text: Some(body),
        text_box: true,
        ..Default::default()
    });
    let mut s = Session::new();
    s.add_document(DocState::new(source, None, "Source".into()));
    s.execute("edit.select", &json!({"ids": [100]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.add_document(DocState::new(destination, None, "Destination".into()));
    s.execute("edit.paste", &json!({})).unwrap();
    let placed = deckcraft_render::place_slide(&s.doc().unwrap().doc, 0);
    let line = &placed.texts[0].layout.lines[0];
    assert!((line.extent.1 - line.extent.0 - 74.4).abs() < 0.01, "script family must survive a source-only font transfer");
}

#[test]
fn rich_text_paste_does_not_import_replaced_paragraph_defaults() {
    let mut s = Session::new();
    let source = open(&mut s, "source.pptx", &deck(Some(700)));
    let mut destination_doc = (*s.doc().unwrap().doc).clone();
    destination_doc.embedded_fonts.clear();
    s.add_document(DocState::new(destination_doc, None, "Destination".into()));
    let destination = s.active_index().unwrap();
    for (index, family) in [(source, FAMILY), (destination, "Unembedded destination defaults")] {
        s.set_active(index);
        s.edit(|doc, _| {
            for master in &mut doc.masters {
                let master = Arc::make_mut(master);
                for set in [&mut master.theme.fonts.major, &mut master.theme.fonts.minor] {
                    set.latin = "Unembedded theme".into();
                    set.ea.clear();
                    set.cs.clear();
                }
            }
            let para = &mut Arc::make_mut(&mut doc.slides[0]).shapes[0].text.as_mut().unwrap().paragraphs[0];
            para.def_props.font = Some(family.into());
            para.runs[0].props.font = None;
            para.runs[0].props.font_ea = None;
            para.runs[0].props.font_cs = None;
            para.runs[0].props.font_sym = None;
            Ok(())
        })
        .unwrap();
    }
    s.set_active(source);
    s.execute("text.edit", &json!({"id": 100})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.set_active(destination);
    s.execute("text.edit", &json!({"id": 100})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.font_aliases.is_empty(), "discarded source paragraph defaults must not import a font");
    assert!(s.doc().unwrap().doc.embedded_fonts.is_empty());
}
