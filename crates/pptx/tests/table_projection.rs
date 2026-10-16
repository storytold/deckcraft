//! Contributor-original enhanced tables and hostile edited core projections.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{table::*, text::TextBody, *};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};
fn fixture(nested: bool) -> Presentation {
    let mut p = Presentation::default();
    let mut table = Table::new(2, 2, 360., 36.);
    table.style = "grid".into();
    table.first_row = false;
    table.rows[0].cells[0].text = TextBody::from_text("12.5");
    table.rows[0].cells[0].data = Some(Arc::new(
        CellConstant::from_display(CellValue::Number { kind: CellNumberKind::Float, lexical: "12.5".into() }, &table.rows[0].cells[0].text).unwrap(),
    ));
    table.set_cell_formula(0, 1, "=[.A1]*2").unwrap();
    let format = NumberFormat {
        style: NumberStyle::Currency {
            decimal_places: 2,
            min_decimal_places: 2,
            grouping: true,
            code: "EUR".into(),
            symbol: "€".into(),
            symbol_after: true,
            space: true,
        },
        locale: NumberLocale::FrBe,
        color: Some([171, 35, 54, 255]),
        ..Default::default()
    };
    let cell = &mut table.rows[0].cells[0];
    cell.text = TextBody::from_text(&format.format_value(cell.data.as_ref().unwrap().value()).unwrap().text);
    cell.data = Some(Arc::new(CellConstant::from_display(cell.data.as_ref().unwrap().value().clone(), &cell.text).unwrap()));
    cell.number_format = Some(Arc::new(format));
    table.rows[1].cells[0].text = TextBody::from_text("Before");
    if nested {
        let cell = &mut table.rows[1].cells[0];
        let mut child = Table::new(1, 1, 180., 30.);
        child.style = "grid".into();
        child.cell_mut(0, 0).unwrap().text = TextBody::from_text("Child");
        cell.append_nested_table(child).unwrap();
        cell.append_text_block(TextBody::from_text("After")).unwrap();
    }
    Arc::make_mut(&mut p.slides[0]).shapes =
        vec![Shape { id: ShapeId(700), xfrm: Some(Xfrm::new(40., 50., 360., 72.)), kind: ShapeKind::Table(table), ..Default::default() }];
    p.fix_next_id();
    p
}
fn parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut file = zip.by_index(i).unwrap();
            let name = file.name().to_string();
            let mut bytes = vec![];
            file.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn edit(bytes: &[u8], mut change: impl FnMut(&str, &mut Vec<u8>) -> bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, mut bytes) in parts(bytes) {
        if !change(&name, &mut bytes) {
            continue;
        }
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("missing restored table") };
    table
}
fn flat_nodes(xml: &str) -> Vec<deckcraft_pptx::XmlEl> {
    let doc = deckcraft_pptx::parse_xml(xml.as_bytes()).unwrap();
    doc.root.find("graph").unwrap().children_named("node").cloned().collect()
}
fn flat_member_value(xml: &str, name: &str) -> (u32, String) {
    let nodes = flat_nodes(xml);
    let id =
        nodes.iter().flat_map(|node| node.children_named("member")).find(|member| member.attr("name") == Some(name)).unwrap().u32("ref").unwrap();
    let value = nodes.iter().find(|node| node.u32("id") == Some(id)).unwrap().text();
    (id, value)
}
fn flat_replace_scalar(xml: &str, id: u32, value: &str) -> String {
    let mut xml = xml.to_string();
    let tag = format!("<dc:node id=\"{id}\" ");
    let start = xml.find(&tag).unwrap();
    let begin = xml[start..].find('>').unwrap() + start + 1;
    let end = xml[begin..].find("</dc:node>").unwrap() + begin;
    xml.replace_range(begin..end, value);
    xml
}
fn flat_scalar_id(xml: &str, value: &str) -> u32 {
    flat_nodes(xml).iter().find(|node| node.elements().next().is_none() && node.text() == value).unwrap().u32("id").unwrap()
}

#[test]
fn typed_values_formulas_numberformats_roundtrip_all_package_kinds() {
    let p = fixture(false);
    let before = p.clone();
    for ext in ["pptx", "potx", "ppsx"] {
        let bytes = deckcraft_pptx::export_as(&p, ext).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(table(&back), table(&p));
        assert_eq!(table(&back).cell(0, 1).unwrap().formula.as_ref().unwrap().outcome(), &FormulaOutcome::Number { lexical: "25".into() });
    }
    assert_eq!(p, before);
}
#[test]
fn nested_tables_and_ordered_text_are_standard_editable_objects() {
    let p = fixture(true);
    let bytes = deckcraft_pptx::export(&p).unwrap();
    emit_completion_package("nested-tables.pptx", &bytes);
    let xml = String::from_utf8(parts(&bytes).into_iter().find(|(n, _)| n == "ppt/slides/slide1.xml").unwrap().1).unwrap();
    assert!(xml.contains("<p:grpSp>"));
    assert!(xml.matches("<a:tbl>").count() >= 2);
    assert!(xml.find(">Before<").unwrap() < xml.find(">Child<").unwrap());
    assert!(xml.find(">Child<").unwrap() < xml.find(">After<").unwrap());
    let back = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(table(&back), table(&p));
}
#[test]
fn foreign_cell_edit_refuses_stale_formula_authority() {
    let bytes = deckcraft_pptx::export(&fixture(false)).unwrap();
    let changed = edit(&bytes, |name, data| {
        if name == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            *data = xml.replacen(">25<", ">26<", 1).into_bytes();
        }
        true
    });
    let error = deckcraft_pptx::import(&changed).unwrap_err().to_string();
    assert!(error.contains("table") || error.contains("formula"));
}
#[test]
fn forged_geometry_with_current_core_unchanged_is_refused() {
    let bytes = deckcraft_pptx::export(&fixture(false)).unwrap();
    let changed = edit(&bytes, |name, data| {
        if name == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            let id = flat_scalar_id(&xml, "40.0");
            *data = flat_replace_scalar(&xml, id, "41.0").into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn deleted_child_table_cannot_be_resurrected() {
    let bytes = deckcraft_pptx::export(&fixture(true)).unwrap();
    let changed = edit(&bytes, |name, data| {
        if name == "ppt/slides/slide1.xml" {
            let mut xml = String::from_utf8(data.clone()).unwrap();
            let start = xml.find("<p:graphicFrame>").unwrap();
            let end = xml[start..].find("</p:graphicFrame>").unwrap() + start + "</p:graphicFrame>".len();
            xml.replace_range(start..end, "");
            *data = xml.into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn stripping_extension_preserves_current_core_as_ordinary_editable_objects() {
    let bytes = deckcraft_pptx::export(&fixture(true)).unwrap();
    let changed = edit(&bytes, |name, data| {
        if name == "ppt/slides/slide1.xml" {
            let mut xml = String::from_utf8(data.clone()).unwrap();
            let start = xml.find("<p:extLst><p:ext uri=\"urn:deckcraft:pptx:table:1\"").unwrap();
            let end = xml[start..].find("</p:extLst>").unwrap() + start + "</p:extLst>".len();
            xml.replace_range(start..end, "");
            *data = xml.replacen(">Child<", ">Edited child<", 1).into_bytes();
        }
        true
    });
    let p = deckcraft_pptx::import(&changed).unwrap();
    assert!(matches!(p.slides[0].shapes[0].kind, ShapeKind::Group { .. }));
    let mut found = false;
    walk(&p.slides[0].shapes, &mut |s, _| {
        if let ShapeKind::Table(t) = &s.kind {
            found |= t.rows.iter().flat_map(|r| &r.cells).any(|c| c.text.text() == "Edited child");
            assert!(t.rows.iter().flat_map(|r| &r.cells).all(|c| c.data.is_none() && c.formula.is_none()));
        }
    });
    assert!(found);
}
#[test]
fn rich_canonical_text_occurs_only_in_standard_projection() {
    let bytes = deckcraft_pptx::export(&fixture(true)).unwrap();
    let xml = String::from_utf8(parts(&bytes).into_iter().find(|(n, _)| n == "ppt/slides/slide1.xml").unwrap().1).unwrap();
    for word in ["Before", "Child", "After"] {
        assert_eq!(xml.matches(&format!(">{word}<")).count(), 1);
    }
    assert!(!xml.contains("name=\"display\""));
    assert!(!xml.contains("name=\"displayBaseline\""));
}
#[test]
fn table_extension_namespace_spoof_is_refused() {
    let bytes = deckcraft_pptx::export(&fixture(false)).unwrap();
    let changed = edit(&bytes, |name, data| {
        if name == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            *data = xml.replacen("xmlns:dc=\"urn:deckcraft:pptx:table:1\"", "xmlns:dc=\"urn:foreign:table\"", 1).into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn all_formula_profiles_retain_typed_authority() {
    for profile in [FORMULA_PROFILE, FORMULA_PROFILE_V1_FORMATTED, FORMULA_PROFILE_V2] {
        let mut p = fixture(false);
        let ShapeKind::Table(source_table) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
        let body = TextBody::from_text("25");
        source_table.cell_mut(0, 1).unwrap().formula = Some(Arc::new(
            CellFormula::from_profile_display(profile, "=[.A1]*2".into(), FormulaOutcome::Number { lexical: "25".into() }, &body).unwrap(),
        ));
        let bytes = deckcraft_pptx::export(&p).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(table(&back).cell(0, 1).unwrap().formula.as_ref().unwrap().profile(), profile);
    }
}
#[test]
fn slide_master_layout_handout_and_notes_table_owners_roundtrip() {
    let mut p = fixture(true);
    let original = p.slides[0].shapes[0].clone();
    let master = Arc::make_mut(&mut p.masters[0]);
    master.shapes.push(original.clone());
    master.layouts[0].shapes.push(original.clone());
    p.handout_master = Some(Arc::new(Master { id: MasterId(810), shapes: vec![original.clone()], ..Default::default() }));
    p.notes_master = Some(Arc::new(Master { id: MasterId(820), shapes: vec![original.clone()], ..Default::default() }));
    Arc::make_mut(&mut p.slides[0]).notes = TextBody::from_text("Speaker body");
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(table(&back), table(&p));
    assert!(back.masters[0].shapes.iter().any(|s| matches!(s.kind, ShapeKind::Table(_))));
    assert!(back.masters[0].layouts[0].shapes.iter().any(|s| matches!(s.kind, ShapeKind::Table(_))));
    assert!(back.handout_master.as_ref().unwrap().shapes.iter().any(|s| matches!(s.kind, ShapeKind::Table(_))));
    assert!(back.notes_master.as_ref().unwrap().shapes.iter().any(|s| matches!(s.kind, ShapeKind::Table(_))));
}
#[test]
fn second_export_and_import_keeps_native_table_text_and_formulas() {
    let p = fixture(true);
    let once = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    let twice = deckcraft_pptx::import(&deckcraft_pptx::export(&once).unwrap()).unwrap();
    assert_eq!(table(&twice), table(&p));
}
#[test]
fn notes_bridge_has_single_visible_cell_text_authority_and_keeps_unused_resource() {
    let mut p = fixture(true);
    let original = p.slides[0].shapes[0].clone();
    p.notes_master = Some(Arc::new(Master { id: MasterId(820), shapes: vec![original.clone()], ..Default::default() }));
    let mut unused = original.clone();
    unused.id = ShapeId(991);
    let ShapeKind::Table(unused_table) = &mut unused.kind else { panic!("table") };
    unused_table.cell_mut(1, 0).unwrap().text = TextBody::from_text("Unused sole text");
    p.notes_masters.push(notes::NotesMaster {
        master: Arc::new(Master { id: MasterId(990), shapes: vec![unused], ..Default::default() }),
        ..Default::default()
    });
    let mut second = (*p.slides[0]).clone();
    second.id = SlideId(910);
    p.slides.push(Arc::new(second));
    for slide in &mut p.slides {
        Arc::make_mut(slide).notes = TextBody::from_text("Speaker body");
    }
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let entries = parts(&bytes);
    let master = String::from_utf8(entries.iter().find(|(name, _)| name == "ppt/notesMasters/notesMaster1.xml").unwrap().1.clone()).unwrap();
    assert!(!master.contains(">Before<"));
    assert!(master.contains(">Unused sole text<"));
    for index in 1..=2 {
        let xml =
            String::from_utf8(entries.iter().find(|(name, _)| name == &format!("ppt/notesSlides/notesSlide{index}.xml")).unwrap().1.clone()).unwrap();
        assert_eq!(xml.matches(">Before<").count(), 1);
    }
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let ShapeKind::Table(shared) = &back.notes_master.as_ref().unwrap().shapes[0].kind else { panic!("shared table") };
    assert_eq!(shared, table(&p));
    let changed = edit(&bytes, |name, data| {
        if name == "ppt/notesSlides/notesSlide1.xml" {
            let xml = String::from_utf8(data.clone()).unwrap();
            *data = xml.replacen(">Before<", ">Foreign cell<", 1).into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&changed).is_err());
    let deleted = edit(&bytes, |name, _| name != "ppt/notesSlides/notesSlide1.xml");
    assert!(deckcraft_pptx::import(&deleted).is_err());
}
#[test]
fn flat_metadata_cycles_multiple_owners_and_detached_values_are_refused() {
    let bytes = deckcraft_pptx::export(&fixture(false)).unwrap();
    for mode in 0..3 {
        let changed = edit(&bytes, |name, data| {
            if name == "ppt/slides/slide1.xml" {
                let xml = String::from_utf8(data.clone()).unwrap();
                let (source, _) = flat_member_value(&xml, "source");
                let (bodies, _) = flat_member_value(&xml, "bodies");
                let mut xml = match mode {
                    0 => xml.replacen(&format!("<dc:member name=\"source\" ref=\"{source}\"/>"), "<dc:member name=\"source\" ref=\"0\"/>", 1),
                    1 => xml.replacen(
                        &format!("<dc:member name=\"bodies\" ref=\"{bodies}\"/>"),
                        &format!("<dc:member name=\"bodies\" ref=\"{source}\"/>"),
                        1,
                    ),
                    _ => xml,
                };
                if mode == 2 {
                    let nodes = flat_nodes(&xml);
                    let extra = format!("<dc:node id=\"{}\" kind=\"string\">Detached authority</dc:node></dc:graph>", nodes.len());
                    xml = xml.replacen("</dc:graph>", &extra, 1);
                }
                *data = xml.into_bytes();
            }
            true
        });
        assert!(deckcraft_pptx::import(&changed).is_err());
    }
}
#[test]
fn maximum_nested_table_depth_roundtrips_with_shallow_metadata_xml() {
    let mut child = Table::new(1, 1, 100., 24.);
    child.style = "grid".into();
    child.cell_mut(0, 0).unwrap().text = TextBody::from_text("Deepest visible cell");
    for _ in 0..MAX_NESTED_TABLE_DEPTH {
        let mut parent = Table::new(1, 1, 200., 24.);
        parent.style = "grid".into();
        parent.cell_mut(0, 0).unwrap().append_nested_table(child).unwrap();
        child = parent;
    }
    let mut p = fixture(false);
    Arc::make_mut(&mut p.slides[0]).shapes[0].kind = ShapeKind::Table(child);
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(table(&back), table(&p));
    let xml = parts(&bytes).into_iter().find(|(name, _)| name == "ppt/slides/slide1.xml").unwrap().1;
    let doc = deckcraft_pptx::parse_xml(&xml).unwrap();
    assert!(doc.root.find("graph").is_some());
}
#[test]
fn fresh_media_and_slide_link_identities_are_bound_to_current_standard_parts() {
    let mut p = fixture(true);
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([39, 119, 181, 255])))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let media = p.add_media("original-table-art.png", "image/png", png.into_inner()).unwrap();
    let mut other = (*p.slides[0]).clone();
    other.id = SlideId(860);
    other.shapes.clear();
    p.slides.push(Arc::new(other));
    let ShapeKind::Table(source) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
    source.background = Some(Fill::Picture(style::PictureFill { media, ..Default::default() }));
    source.cell_mut(1, 0).unwrap().text.paragraphs[0].runs[0].props.link = Some(text::Hyperlink {
        action: text::Action::Slide { slide: SlideId(860) },
        tooltip: "Current slide link".into(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: None,
        sound_name: None,
    });
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let Fill::Picture(fill) = table(&back).background.as_ref().unwrap() else { panic!("picture") };
    assert_eq!(back.media(fill.media).unwrap().data.as_slice(), p.media(media).unwrap().data.as_slice());
    let text::Action::Slide { slide } = &table(&back).cell(1, 0).unwrap().text.paragraphs[0].runs[0].props.link.as_ref().unwrap().action else {
        panic!("slide link")
    };
    assert_eq!(*slide, back.slides[1].id);
    let changed = edit(&bytes, |name, data| {
        if name.starts_with("ppt/media/") && name.ends_with(".png") {
            *data.last_mut().unwrap() ^= 1;
        }
        true
    });
    assert!(deckcraft_pptx::import(&changed).is_err());
    let twice = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
    assert!(table(&twice).cell(1, 0).unwrap().text.paragraphs[0].runs[0].props.link.is_some());
}
#[test]
fn combined_notes_groups_and_maximum_nested_table_metadata_roundtrip() {
    let mut child = Table::new(1, 1, 100., 24.);
    child.style = "grid".into();
    child.cell_mut(0, 0).unwrap().text = TextBody::from_text("Combined deepest cell");
    for _ in 0..MAX_NESTED_TABLE_DEPTH {
        let mut parent = Table::new(1, 1, 200., 24.);
        parent.style = "grid".into();
        parent.cell_mut(0, 0).unwrap().append_nested_table(child).unwrap();
        child = parent;
    }
    let mut drawing = Shape { id: ShapeId(1200), xfrm: Some(Xfrm::new(0., 0., 200., 500.)), kind: ShapeKind::Table(child), ..Default::default() };
    for index in 0..14 {
        drawing = Shape {
            id: ShapeId(1300 + index),
            xfrm: Some(Xfrm::new(0., 0., 200., 500.)),
            kind: ShapeKind::Group { children: vec![drawing], child: Xfrm::new(0., 0., 200., 500.) },
            ..Default::default()
        };
    }
    let mut p = fixture(false);
    p.notes_master = Some(Arc::new(Master { id: MasterId(1400), shapes: vec![drawing], ..Default::default() }));
    Arc::make_mut(&mut p.slides[0]).notes = TextBody::from_text("Combined speaker");
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let restored = deckcraft_pptx::import(&bytes).unwrap();
    let mut found = false;
    walk(&restored.notes_master.as_ref().unwrap().shapes, &mut |shape, _| {
        if let ShapeKind::Table(t) = &shape.kind {
            found = true;
            assert_eq!(
                t.at_path(&TablePath((0..MAX_NESTED_TABLE_DEPTH).map(|_| TablePathStep { row: 0, col: 0, child: 1 }).collect()))
                    .unwrap()
                    .cell(0, 0)
                    .unwrap()
                    .text
                    .text(),
                "Combined deepest cell"
            );
        }
    });
    assert!(found);
}

fn emit_completion_package(name: &str, bytes: &[u8]) {
    if let Ok(directory) = std::env::var("DECKCRAFT_COMPLETION_QA_DIR") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
}
