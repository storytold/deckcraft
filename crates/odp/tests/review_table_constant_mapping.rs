//! Independent, contributor-original typed ODF scalar/display and owner-mapping fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    table::{CellConstant, CellNumberKind, CellValue},
    *,
};
use deckcraft_odp::{MIME, export, import};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:table='urn:oasis:names:tc:opendocument:xmlns:table:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:of='urn:oasis:names:tc:opendocument:xmlns:of:1.2'";
fn package(frames: &str) -> Vec<u8> {
    let xml = format!(
        "<office:document-content {NS} office:version='1.3'><office:body><office:presentation><draw:page>{frames}</draw:page></office:presentation></office:body></office:document-content>"
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in [
        ("mimetype", MIME),
        ("content.xml", xml.as_str()),
        ("META-INF/manifest.xml", "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0'/>"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn frame(attrs: &str, display: &str, columns: usize, rows: usize) -> String {
    format!(
        "<draw:frame svg:width='300pt' svg:height='100pt'><table:table><table:table-row table:number-rows-repeated='{rows}'><table:table-cell table:number-columns-repeated='{columns}' {attrs}>{display}</table:table-cell></table:table-row></table:table></draw:frame>"
    )
}
fn native(value: CellValue, body: TextBody) -> Presentation {
    let mut table = Table::new(1, 1, 100.0, 40.0);
    table.style = "none".into();
    table.rows[0].cells[0].data = Some(Arc::new(CellConstant::from_display(value, &body).unwrap()));
    table.rows[0].cells[0].text = body;
    let mut p = Presentation {
        masters: vec![Arc::new(Master {
            id: MasterId(100),
            layouts: vec![Layout { id: LayoutId(101), show_master_shapes: true, ..Default::default() }],
            ..Default::default()
        })],
        slides: vec![Arc::new(Slide {
            id: SlideId(200),
            layout: LayoutId(101),
            shapes: vec![Shape {
                id: ShapeId(300),
                xfrm: Some(Xfrm::new(10.0, 10.0, 100.0, 40.0)),
                kind: ShapeKind::Table(table),
                ..Default::default()
            }],
            ..Default::default()
        })],
        notes_master: None,
        ..Default::default()
    };
    p.notes_masters.clear();
    p.handout_master = None;
    p.fix_next_id();
    p
}
fn table(p: &Presentation) -> &Table {
    let ShapeKind::Table(t) = &p.slides[0].shapes[0].kind else { panic!("table") };
    t
}
fn table_mut(p: &mut Presentation) -> &mut Table {
    let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
    t
}
fn cell(p: &Presentation) -> &Cell {
    &table(p).rows[0].cells[0]
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}
fn no_warning(p: &Presentation) {
    assert!(!p.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", p.tags);
}
fn collect_cells(p: &Presentation) -> Vec<&Cell> {
    fn shapes<'a>(items: &'a [Shape], out: &mut Vec<&'a Cell>) {
        for s in items {
            match &s.kind {
                ShapeKind::Table(t) => out.extend(t.rows.iter().flat_map(|r| &r.cells)),
                ShapeKind::Group { children, .. } => shapes(children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for s in &p.slides {
        shapes(&s.shapes, &mut out);
        if let Some(n) = &s.notes_page {
            shapes(&n.shapes, &mut out);
        }
    }
    for m in p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|n| &n.master)) {
        shapes(&m.shapes, &mut out);
        for l in &m.layouts {
            shapes(&l.shapes, &mut out);
        }
    }
    out
}

#[test]
fn actual_odf_scalars_keep_authority_separate_from_exact_cached_unicode_display() {
    let display = "<text:p>e&#x301;漢<text:tab/>50%<text:line-break/>ب</text:p><text:p>Second<text:s text:c='2'/>line</text:p>";
    for (attrs, value) in [
        (
            "office:value-type='float' office:value='+009007199254740993.00e-0'",
            CellValue::Number { kind: CellNumberKind::Float, lexical: "+009007199254740993.00e-0".into() },
        ),
        ("office:value-type='percentage' office:value='0.5000'", CellValue::Number { kind: CellNumberKind::Percentage, lexical: "0.5000".into() }),
        ("office:value-type='boolean' office:boolean-value='0'", CellValue::Boolean { value: false }),
        (
            "office:value-type='string' office:string-value='stored&#x9;A&#xA;B&#xD;C &amp; &quot; &lt;'",
            CellValue::String { value: "stored\tA\nB\rC & \" <".into() },
        ),
        (
            "office:value-type='currency' office:value='-0.000' office:currency=''",
            CellValue::Number { kind: CellNumberKind::Currency { currency: Some("".into()) }, lexical: "-0.000".into() },
        ),
        (
            "office:value-type='currency' office:value='1E+2'",
            CellValue::Number { kind: CellNumberKind::Currency { currency: None }, lexical: "1E+2".into() },
        ),
    ] {
        let p = import(&package(&frame(attrs, display, 1, 1))).unwrap();
        no_warning(&p);
        let original = cell(&p).text.clone();
        assert_eq!(original.text(), "e\u{301}漢\t50%\u{b}ب\nSecond  line");
        assert_eq!(cell(&p).data.as_ref().unwrap().value(), &value);
        let p = deckcraft_format::load(&deckcraft_format::save(&p).unwrap()).unwrap();
        let p = import(&export(&p).unwrap()).unwrap();
        no_warning(&p);
        assert_eq!(cell(&p).data.as_ref().unwrap().value(), &value);
        assert_eq!(cell(&p).text.text(), original.text());
    }
}

#[test]
fn absent_display_fallback_uses_source_spelling_and_never_formats_currency_or_percentage() {
    for (attrs, text, value) in [
        (
            "office:value-type='float' office:value=' +01.2500 '",
            " +01.2500 ",
            CellValue::Number { kind: CellNumberKind::Float, lexical: "+01.2500".into() },
        ),
        ("office:value-type='boolean' office:boolean-value='1'", "1", CellValue::Boolean { value: true }),
        (
            "office:value-type='currency' office:value='0.5' office:currency='EUR'",
            "0.5",
            CellValue::Number { kind: CellNumberKind::Currency { currency: Some("EUR".into()) }, lexical: "0.5".into() },
        ),
    ] {
        let p = import(&package(&frame(attrs, "", 1, 1))).unwrap();
        no_warning(&p);
        assert_eq!(cell(&p).text.text(), text);
        assert_eq!(cell(&p).data.as_ref().unwrap().value(), &value);
    }
}

#[test]
fn repeated_typed_records_preserve_each_display_and_are_not_rebound_after_one_edit() {
    let p = import(&package(&frame("office:value-type='string' office:string-value='owned'", "<text:p>display</text:p>", 3, 2))).unwrap();
    no_warning(&p);
    let mut p = import(&export(&p).unwrap()).unwrap();
    no_warning(&p);
    let before = table(&p).rows[1].cells[1].data.as_ref().unwrap().clone();
    table_mut(&mut p).rows[0].cells[0].text = TextBody::from_text("changed");
    let output = export(&p).unwrap();
    let q = import(&output).unwrap();
    no_warning(&q);
    assert!(table(&q).rows[0].cells[0].data.is_none());
    assert_eq!(table(&q).rows[0].cells[0].text.text(), "changed");
    for (r, row) in table(&q).rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().enumerate() {
            if (r, c) != (0, 0) {
                assert_eq!(cell.data.as_ref().unwrap().value(), before.value());
                assert_eq!(cell.text.text(), "display");
            }
        }
    }
    assert!(table(&p).rows[0].cells[0].data.is_some(), "export must not mutate stale caller metadata");
}

#[test]
fn per_table_and_cross_table_metadata_amplification_refuse_before_repeat_attachment() {
    // Rich paragraph/run/cell structure itself counts toward 32 MiB. Two thousand
    // cells fit that plain budget, while attaching an 8 KiB value to each crosses it.
    let plain = frame("", "<text:p>cached</text:p>", 250, 8);
    import(&package(&plain)).expect("plain repeated-cell control must fit the independent table budget");
    let attrs = format!("office:value-type='string' office:string-value='{}'", "x".repeat(8192));
    let error = import(&package(&frame(&attrs, "<text:p>cached</text:p>", 250, 8))).unwrap_err().to_string();
    assert!(matches!(error.as_str(), "ODP: expanded table text exceeds limit" | "ODP: expanded nested table allocation exceeds limit"), "{error}");
    let plain = frame("", "<text:p>cached</text:p>", 400, 1).repeat(12);
    assert!(import(&package(&plain)).is_ok());
    let error = import(&package(&frame(&attrs, "<text:p>cached</text:p>", 400, 1).repeat(12))).unwrap_err().to_string();
    assert!(
        error.contains("metadata") || error.contains("total size") || error == "ODP: expanded nested table allocation exceeds total limit",
        "{error}"
    );
}

#[test]
fn supported_native_scalar_attribute_boundary_is_decoded_bytes_and_keeps_exact_whitespace() {
    for value in ["\t".repeat(8192), "漢".repeat(2730) + "xy"] {
        assert_eq!(value.len(), 8192);
        let p = native(CellValue::String { value: value.clone() }, TextBody::from_text("cached"));
        let bytes = export(&p).unwrap();
        let q = import(&bytes).unwrap();
        no_warning(&q);
        assert_eq!(cell(&q).data.as_ref().unwrap().value(), &CellValue::String { value });
    }
    let p = native(CellValue::String { value: "x".repeat(8193) }, TextBody::from_text("cached"));
    assert!(export(&p).unwrap_err().to_string().contains("attribute"));
    assert!(cell(&p).data.is_some());
}

#[test]
fn local_empty_placeholder_tables_and_group_ancestors_never_disappear_with_data() {
    for group in [false, true] {
        let mut p = native(CellValue::Boolean { value: true }, TextBody::from_text("cached"));
        let shape = Arc::make_mut(&mut p.slides[0]).shapes.remove(0);
        let mut shape = if group {
            Shape {
                id: ShapeId(301),
                kind: ShapeKind::Group { children: vec![shape], child: Xfrm::new(0.0, 0.0, 100.0, 40.0) },
                ..Default::default()
            }
        } else {
            shape
        };
        shape.ph = Some(Placeholder { kind: PhType::Body, ..Default::default() });
        shape.text = Some(TextBody::default());
        Arc::make_mut(&mut p.slides[0]).shapes.push(shape);
        match export(&p) {
            Err(e) => assert!(e.to_string().contains("placeholder")),
            Ok(bytes) => {
                let q = import(&bytes).unwrap();
                let cells = collect_cells(&q);
                assert_eq!(cells.len(), 1);
                assert_eq!(cells[0].data.as_ref().unwrap().value(), &CellValue::Boolean { value: true });
            }
        }
    }
}

#[test]
fn unrepresentable_current_display_characters_cannot_silently_rebind_a_baseline() {
    for text in ["a\0b", "a\u{1}b", "a\rb", "a\nb", "a\u{fffe}b"] {
        let body = TextBody { paragraphs: vec![Paragraph { runs: vec![Run::new(text)], ..Default::default() }], ..Default::default() };
        let p = native(CellValue::Boolean { value: true }, body);
        match export(&p) {
            Err(e) => assert!(e.to_string().contains("display") || e.to_string().contains("character"), "{e}"),
            Ok(bytes) => {
                let q = import(&bytes).unwrap();
                assert_eq!(cell(&q).text.paragraphs[0].runs.iter().map(Run::content).collect::<String>(), text);
                assert!(cell(&q).data.as_ref().unwrap().matches_display(&cell(&p).text).unwrap());
            }
        }
    }
}

#[test]
fn supported_soft_breaks_tabs_and_paragraph_boundaries_keep_current_baseline() {
    let body = TextBody {
        paragraphs: vec![
            Paragraph {
                runs: vec![Run::new("e\u{301}\t漢"), Run { kind: deckcraft_model::text::RunKind::Break, ..Run::new("ignored") }, Run::new("ب")],
                ..Default::default()
            },
            Paragraph::new("second"),
        ],
        ..Default::default()
    };
    let p = native(CellValue::Number { kind: CellNumberKind::Percentage, lexical: "0.50".into() }, body);
    let q = import(&export(&p).unwrap()).unwrap();
    no_warning(&q);
    assert_eq!(cell(&q).text.text(), cell(&p).text.text());
    assert!(cell(&q).data.as_ref().unwrap().matches_display(&cell(&p).text).unwrap());
}

#[test]
fn shared_regular_master_owns_one_typed_table_across_two_slides() {
    let mut p = native(CellValue::String { value: "master stored".into() }, TextBody::from_text("master display"));
    let shape = Arc::make_mut(&mut p.slides[0]).shapes.remove(0);
    let master = Arc::make_mut(&mut p.masters[0]);
    master.shapes.push(shape);
    master.preserve = true;
    let second = Slide { id: SlideId(201), layout: LayoutId(101), ..Default::default() };
    p.slides.push(Arc::new(second));
    let bytes = export(&p).unwrap();
    assert!(!part(&bytes, "content.xml").contains("master stored"));
    assert_eq!(part(&bytes, "styles.xml").matches("office:string-value=\"master stored\"").count(), 1);
    let q = import(&bytes).unwrap();
    no_warning(&q);
    assert_eq!(q.master_for(&q.slides[0]).unwrap().0.id, q.master_for(&q.slides[1]).unwrap().0.id);
    assert!(q.slides.iter().all(|s| s.shapes.is_empty()));
    assert_eq!(collect_cells(&q).len(), 1);
    assert_eq!(collect_cells(&q)[0].data.as_ref().unwrap().value(), &CellValue::String { value: "master stored".into() });
}

#[test]
fn local_notes_graphic_tables_are_mapped_but_canonical_placeholder_ancestors_refuse() {
    let mut p = native(CellValue::Boolean { value: false }, TextBody::from_text("note cache"));
    let shape = Arc::make_mut(&mut p.slides[0]).shapes.remove(0);
    Arc::make_mut(&mut p.slides[0]).notes_page = Some(notes::NotesPage { shapes: vec![shape], ..Default::default() });
    let q = import(&export(&p).unwrap()).unwrap();
    no_warning(&q);
    assert_eq!(collect_cells(&q).len(), 1);
    assert_eq!(collect_cells(&q)[0].data.as_ref().unwrap().value(), &CellValue::Boolean { value: false });
    let note = Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap();
    note.shapes[0].ph = Some(Placeholder { kind: PhType::Body, ..Default::default() });
    assert!(export(&p).unwrap_err().to_string().contains("notes placeholder"));
}

#[test]
fn attached_values_on_unemitted_owner_lanes_refuse_even_if_stale() {
    for owner in 0..4 {
        for stale in [false, true] {
            let mut p = native(CellValue::Boolean { value: true }, TextBody::from_text("before"));
            if stale {
                table_mut(&mut p).rows[0].cells[0].text = TextBody::from_text("after");
            }
            let shape = Arc::make_mut(&mut p.slides[0]).shapes.remove(0);
            match owner {
                0 => Arc::make_mut(&mut p.masters[0]).layouts[0].shapes.push(shape),
                1 => p.masters.push(Arc::new(Master {
                    id: MasterId(400),
                    shapes: vec![shape],
                    layouts: vec![Layout { id: LayoutId(401), ..Default::default() }],
                    ..Default::default()
                })),
                2 => p.handout_master = Some(Arc::new(Master { id: MasterId(410), shapes: vec![shape], ..Default::default() })),
                _ => {
                    p.notes_master = Some(Arc::new(Master {
                        id: MasterId(420),
                        layouts: vec![Layout { id: LayoutId(421), shapes: vec![shape], ..Default::default() }],
                        ..Default::default()
                    }))
                }
            }
            let e = export(&p).unwrap_err().to_string();
            assert!(e.contains("owner") || e.contains("representation") || e.contains("stored table"), "{e}");
        }
    }
}

#[test]
fn unsupported_scalar_formulas_and_orphan_attributes_remain_guarded_and_recoverable() {
    for attrs in [
        "office:value='1'",
        "office:value-type='float' office:value='1e-999'",
        "office:value-type='boolean' office:boolean-value='false' office:value='0'",
        "office:value-type='float' office:value='2' table:formula='of:=1+1'",
        "office:value-type='date' office:date-value='2026-02-30'",
    ] {
        let bytes = package(&frame(attrs, "<text:p>cached</text:p>", 1, 1));
        let p = import(&bytes).unwrap();
        assert!(p.tags.iter().any(|(k, _)| k == "odp:warning"));
        assert!(cell(&p).data.is_none());
        assert_eq!(cell(&p).text.text(), "cached");
        assert!(export(&p).is_err());
        assert_eq!(p.media.iter().find(|m| m.content_type == "application/vnd.deckcraft.odp-source").unwrap().data.as_slice(), bytes.as_slice());
    }
}
