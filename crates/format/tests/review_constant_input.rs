//! Independent contributor-original native constant/input-boundary fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_format::{MIMETYPE, load, save};
use deckcraft_model::{
    table::{CellConstant, CellNumberKind, CellValue},
    *,
};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

fn package(json: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body) in [("mimetype", MIMETYPE), ("presentation.json", json)] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn manifest(presentation: &str, extra: &str) -> String {
    let comma = if extra.is_empty() { "" } else { "," };
    format!(r#"{{"format":"deckcraft","version":1,"generator":"independent","presentation":{presentation}{comma}{extra}}}"#)
}
fn cell(constant: &str, positional: bool) -> String {
    let text = serde_json::to_string(&TextBody::from_text("shown")).unwrap();
    if positional {
        // Original Cell sequence fields retain their historical indexes; data is appended.
        format!("[{text},null,[null,null,null,null],null,null,null,null,1,1,false,false,null,{constant}]")
    } else {
        format!(r#"{{"text":{text},"gridSpan":1,"rowSpan":1,"data":{constant}}}"#)
    }
}
fn table_presentation(cells: &str) -> String {
    format!(r#"{{"slides":[{{"id":7,"shapes":[{{"id":9,"kind":{{"type":"table","cols":[80],"rows":[{{"height":30,"cells":[{cells}]}}]}}}}]}}]}}"#)
}
fn constant() -> &'static str {
    r#"{"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"9007199254740993"},"display":["shown"]}"#
}
fn only_cell(p: &Presentation) -> &Cell {
    let ShapeKind::Table(table) = &p.slides[0].shapes[0].kind else { panic!("expected table") };
    &table.rows[0].cells[0]
}
fn damage(json: &str) -> String {
    load(&package(json)).unwrap_err().to_string()
}

#[test]
fn map_and_positional_cells_keep_exact_numeric_authority_and_display() {
    for positional in [false, true] {
        let p = load(&package(&manifest(&table_presentation(&cell(constant(), positional)), ""))).unwrap();
        let cell = only_cell(&p);
        assert_eq!(cell.text.text(), "shown");
        assert_eq!(cell.data.as_ref().unwrap().value(), &CellValue::Number { kind: CellNumberKind::Float, lexical: "9007199254740993".into() });
        assert!(cell.data.as_ref().unwrap().matches_display(&cell.text).unwrap());
        let p = load(&save(&p).unwrap()).unwrap();
        assert_eq!(only_cell(&p).data.as_ref().unwrap().value(), cell.data.as_ref().unwrap().value());
        let positional_root = format!(r#"["deckcraft",1,"independent",{}]"#, table_presentation(&crate::cell(constant(), positional)));
        let root = load(&package(&positional_root)).unwrap();
        assert_eq!(only_cell(&root).data.as_ref().unwrap().value(), cell.data.as_ref().unwrap().value());
    }
}

#[test]
fn historical_positional_cell_without_trailing_data_keeps_its_old_fields() {
    let text = serde_json::to_string(&TextBody::from_text("legacy")).unwrap();
    let old = format!("[{text},null,[null,null,null,null],null,null,[1,2,3,4],null,2,3,true,false,null]");
    let cell: Cell = serde_json::from_str(&old).unwrap();
    assert!(cell.data.is_none());
    assert_eq!(cell.margins, Some([1.0, 2.0, 3.0, 4.0]));
    assert_eq!((cell.grid_span, cell.row_span, cell.h_merge, cell.v_merge), (2, 3, true, false));
    assert_eq!(cell.text.text(), "legacy");
}

#[test]
fn unicode_stored_string_is_independent_of_rich_display_through_native_save() {
    let mut p = Presentation::default();
    let mut table = Table::new(1, 1, 80.0, 30.0);
    let cell = &mut table.rows[0].cells[0];
    cell.text = TextBody::from_text("é漢\nshown\u{b}ب");
    cell.data = Some(Arc::new(CellConstant::from_display(CellValue::String { value: "e\u{301}\t漢\r\nب".into() }, &cell.text).unwrap()));
    Arc::make_mut(&mut p.slides[0]).shapes = vec![Shape { id: ShapeId(70), kind: ShapeKind::Table(table), ..Default::default() }];
    let p = load(&save(&p).unwrap()).unwrap();
    assert_eq!(only_cell(&p).text.text(), "é漢\nshown\u{b}ب");
    assert_eq!(only_cell(&p).data.as_ref().unwrap().value(), &CellValue::String { value: "e\u{301}\t漢\r\nب".into() });
}

#[test]
fn typed_metadata_and_decoded_unknown_data_display_keys_disable_legacy_null_repair() {
    let legacy = r#"{"slides":[{"id":7,"shapes":[{"id":9,"xfrm":{"x":null,"y":0,"w":10,"h":10}}]}]}"#;
    assert!(load(&package(&manifest(legacy, r#""label":"data display value","ordinaryValue":{"value":0}"#))).is_ok());
    let fake_keys = serde_json::to_string(r#"\"d\u0061ta\": {"display":[]} value"#).unwrap();
    assert!(load(&package(&manifest(legacy, &format!(r#""database":{{"value":0}},"label":{fake_keys}"#)))).is_ok());
    for extra in [r#""data":null"#, r#""unknown":{"d\u0061ta":null}"#, r#""unknown":{"displ\u0061y":[]}"#] {
        assert!(load(&package(&manifest(legacy, extra))).is_err(), "decoded metadata keys must conservatively stop numeric repair");
    }
    let mut p: serde_json::Value = serde_json::from_str(&table_presentation(&cell(constant(), true))).unwrap();
    p["slides"][0]["shapes"][0]["xfrm"] = serde_json::json!({"x":null,"y":0,"w":10,"h":10});
    assert!(load(&package(&manifest(&p.to_string(), ""))).is_err(), "positional constants also carry required display keys");
}

#[test]
fn malformed_constants_cannot_be_resurrected_by_numeric_null_repair() {
    for malformed in [
        r#"{"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":null},"display":["shown"]}"#,
        r#"{"value":{"kind":"boolean","value":true}}"#,
        r#"{"value":{"kind":"boolean","value":null},"display":["shown"]}"#,
        r#"{"value":{"kind":"number","numberKind":{"kind":"float"},"lexical":"1e-999"},"display":["shown"]}"#,
        r#"{"value":{"kind":"boolean","value":true},"display":null}"#,
    ] {
        for positional in [false, true] {
            assert!(load(&package(&manifest(&table_presentation(&cell(malformed, positional)), ""))).is_err());
        }
    }
}

#[test]
fn aggregate_empty_baselines_are_bounded_for_maps_positional_and_unknown_owners() {
    let display = format!("[{}]", vec![r#""""#; 1024].join(","));
    let record = format!(r#"{{"value":{{"kind":"boolean","value":true}},"display":{display}}}"#);
    for positional in [false, true] {
        // Ordinary model grammar and the same 1024-element record are valid alone.
        assert!(load(&package(&manifest(&table_presentation(&cell(&record, positional)), ""))).is_ok());
        let cells = vec![cell(&record, positional); 640].join(",");
        let presentation = table_presentation(&cells);
        let error = damage(&manifest(&presentation, ""));
        assert!(error.contains("metadata") && error.contains("budget"), "{error}");
        let error = damage(&format!(r#"["deckcraft",1,"independent",{presentation}]"#));
        assert!(error.contains("metadata") && error.contains("budget"), "positional root reset accounting: {error}");
    }
    let records = vec![record.clone(); 400].join(",");
    assert!(load(&package(&manifest("{}", &format!(r#""futureHandout":{{"layouts":[{{"display":[{records}]}}]}}"#)))).is_ok());
    let records = vec![record; 640].join(",");
    let error = damage(&manifest("{}", &format!(r#""futureHandout":{{"layouts":[{{"display":[{records}]}}]}}"#)));
    assert!(error.contains("metadata") && error.contains("budget"), "{error}");
}

#[test]
fn escaped_duplicate_and_nested_trigger_subtrees_share_one_metadata_counter() {
    let array = format!("[{}]", vec!["null"; 400_000].join(","));
    assert!(load(&package(&manifest("{}", &format!(r#""ordinary":{array}"#)))).is_ok());
    for extra in [
        format!(r#""value":{array},"value":{array},"value":{array}"#),
        format!(r#""future":{{"d\u0061ta":[{array},{array},{array}]}}"#),
        format!(r#""future":{{"val\u0075e":{{"nested":[{array},{array},{array}]}}}}"#),
    ] {
        let error = damage(&manifest("{}", &extra));
        assert!(error.contains("metadata") && error.contains("budget"), "duplicate/escaped/nested scope reset: {error}");
    }
}

#[test]
fn raw_token_and_key_limits_cover_unknown_values_before_model_ignore() {
    let exact = serde_json::to_string(&"x".repeat(1024 * 1024)).unwrap();
    assert!(load(&package(&manifest("{}", &format!(r#""future":{exact}"#)))).is_ok());
    let too_long = serde_json::to_string(&"x".repeat(1024 * 1024 + 1)).unwrap();
    assert!(damage(&manifest("{}", &format!(r#""future":{too_long}"#))).contains("string"));
    for (raw_key, accepted) in
        [("x".repeat(1024), true), ("x".repeat(1025), false), (r#"\u0061"#.repeat(170), true), (r#"\u0061"#.repeat(171), false)]
    {
        assert_eq!(load(&package(&manifest("{}", &format!(r#""{raw_key}":0"#)))).is_ok(), accepted);
    }
    let escaped = serde_json::to_string(&"\\\" escaped quote: data display value \u{1f680}").unwrap();
    assert!(load(&package(&manifest("{}", &format!(r#""future":{escaped}"#)))).is_ok());
}

#[test]
fn unknown_unmarked_values_have_global_work_depth_and_trailing_document_limits() {
    let many = format!(r#""unknown":[{}]"#, vec!["0"; 2_000_001].join(","));
    assert!(damage(&manifest("{}", &many)).contains("work"));
    // The native parser now admits the documented combined group/table profile
    // using bounded iterative reconstruction, with a lexical ceiling of 512.
    let within = format!(r#""unknown":{}0{}"#, "[".repeat(140), "]".repeat(140));
    assert!(load(&package(&manifest("{}", &within))).is_ok());
    let deep = format!(r#""unknown":{}0{}"#, "[".repeat(513), "]".repeat(513));
    assert!(load(&package(&manifest("{}", &deep))).is_err());
    for tail in [" {}", " true", " trailing"] {
        assert!(load(&package(&(manifest("{}", "") + tail))).is_err());
    }
    assert!(load(&package(&manifest("{}", r#""unknown":{"display":[}"#))).is_err());
}

#[test]
fn zip_declared_json_size_is_rejected_before_inflation_or_full_parse() {
    let mut bytes = package(&manifest("{}", ""));
    let central = bytes
        .windows(4)
        .enumerate()
        .filter(|(_, sig)| *sig == b"PK\x01\x02")
        .map(|(i, _)| i)
        .find(|&i| {
            let n = u16::from_le_bytes(bytes[i + 28..i + 30].try_into().unwrap()) as usize;
            bytes.get(i + 46..i + 46 + n) == Some(b"presentation.json".as_slice())
        })
        .unwrap();
    bytes[central + 24..central + 28].copy_from_slice(&(128u32 * 1024 * 1024 + 1).to_le_bytes());
    let error = load(&bytes).unwrap_err().to_string();
    assert!(error.contains("presentation.json is too large"), "declared size must win before CRC/EOF/serde: {error}");
}

#[test]
fn native_save_enforces_the_same_raw_token_profile_as_load() {
    let mut p = Presentation::default();
    p.props.title = "x".repeat(1024 * 1024 + 1);
    assert!(save(&p).is_err());
    p.props.title = "x".repeat(1024 * 1024);
    let bytes = save(&p).unwrap();
    assert_eq!(load(&bytes).unwrap().props.title, p.props.title);
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut json = String::new();
    zip.by_name("presentation.json").unwrap().read_to_string(&mut json).unwrap();
    assert!(json.contains(&p.props.title));
}

fn boundary_table(stale: bool) -> Shape {
    let mut table = Table::new(1, 1, 80.0, 30.0);
    let cell = &mut table.rows[0].cells[0];
    cell.text = TextBody::from_text("original display");
    cell.data = Some(Arc::new(CellConstant::from_display(CellValue::String { value: "independent stored value".into() }, &cell.text).unwrap()));
    if stale {
        cell.text = TextBody::from_text("edited display");
    }
    Shape { id: ShapeId(500), hidden: true, kind: ShapeKind::Table(table), ..Default::default() }
}
fn boundary_group(shape: Shape) -> Shape {
    Shape {
        id: ShapeId(501),
        hidden: true,
        kind: ShapeKind::Group { children: vec![shape], child: Xfrm::new(0.0, 0.0, 80.0, 30.0) },
        ..Default::default()
    }
}
fn boundary_master(id: u32, shape: Shape, in_layout: bool) -> Arc<Master> {
    let mut master = Master {
        id: MasterId(id),
        layouts: vec![Layout { id: LayoutId(id + 1), show_master_shapes: false, ..Default::default() }],
        ..Default::default()
    };
    if in_layout {
        master.layouts[0].shapes.push(boundary_group(shape));
    } else {
        master.shapes.push(boundary_group(shape));
    }
    Arc::new(master)
}
fn boundary_fixture(owner: usize, stale: bool) -> Presentation {
    let mut p = Presentation {
        masters: vec![Arc::new(Master {
            id: MasterId(100),
            layouts: vec![Layout { id: LayoutId(101), ..Default::default() }],
            ..Default::default()
        })],
        slides: vec![Arc::new(Slide { id: SlideId(200), layout: LayoutId(101), hidden: true, show_master_shapes: false, ..Default::default() })],
        notes_master: None,
        ..Default::default()
    };
    p.notes_masters.clear();
    p.handout_master = None;
    let shape = boundary_table(stale);
    match owner {
        0 => Arc::make_mut(&mut p.slides[0]).shapes.push(shape),
        1 => Arc::make_mut(&mut p.slides[0]).shapes.push(boundary_group(boundary_group(shape))),
        2 | 3 => p.masters.push(boundary_master(300, shape, owner == 3)),
        4 | 5 => p.notes_master = Some(boundary_master(310, shape, owner == 5)),
        6 | 7 => p.notes_masters.push(notes::NotesMaster { master: boundary_master(320, shape, owner == 7), ..Default::default() }),
        8 | 9 => p.handout_master = Some(boundary_master(330, shape, owner == 9)),
        _ => {
            Arc::make_mut(&mut p.slides[0]).notes_page =
                Some(notes::NotesPage { shapes: vec![boundary_group(shape)], show_master_shapes: false, ..Default::default() })
        }
    }
    p.fix_next_id();
    p
}
fn boundary_cells(p: &Presentation) -> Vec<&Cell> {
    fn shapes<'a>(items: &'a [Shape], out: &mut Vec<&'a Cell>) {
        for item in items {
            match &item.kind {
                ShapeKind::Table(table) => out.extend(table.rows.iter().flat_map(|row| &row.cells)),
                ShapeKind::Group { children, .. } => shapes(children, out),
                _ => {}
            }
        }
    }
    let mut cells = Vec::new();
    for slide in &p.slides {
        shapes(&slide.shapes, &mut cells);
        if let Some(page) = &slide.notes_page {
            shapes(&page.shapes, &mut cells);
        }
    }
    for master in
        p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|notes| &notes.master))
    {
        shapes(&master.shapes, &mut cells);
        for layout in &master.layouts {
            shapes(&layout.shapes, &mut cells);
        }
    }
    cells
}
fn boundary_owner_pointers(p: &Presentation) -> Vec<usize> {
    p.slides
        .iter()
        .map(|slide| Arc::as_ptr(slide) as usize)
        .chain(
            p.masters
                .iter()
                .chain(p.notes_master.iter())
                .chain(p.handout_master.iter())
                .chain(p.notes_masters.iter().map(|notes| &notes.master))
                .map(|master| Arc::as_ptr(master) as usize),
        )
        .collect()
}
fn boundary_manifest(p: &Presentation) -> String {
    manifest(&serde_json::to_string(p).unwrap(), "")
}

#[test]
fn every_hidden_unreferenced_nested_owner_lowers_stale_data_without_mutating_save_source() {
    for owner in 0..11 {
        for stale in [false, true] {
            let p = boundary_fixture(owner, stale);
            let old = p.clone();
            let pointers = boundary_owner_pointers(&p);
            let mut reconciled = p.clone();
            assert_eq!(deckcraft_model::table::reconcile_constants(&mut reconciled).unwrap(), stale);
            let detached = pointers.iter().zip(boundary_owner_pointers(&reconciled)).filter(|(a, b)| **a != *b).count();
            assert_eq!(detached, usize::from(stale), "owner {owner}");
            let saved = save(&p).unwrap();
            assert_eq!(p, old, "save mutated caller at owner {owner}");
            assert_eq!(boundary_owner_pointers(&p), pointers);
            for bytes in [saved, package(&boundary_manifest(&p))] {
                let back = load(&bytes).unwrap();
                let cells = boundary_cells(&back);
                assert_eq!(cells.len(), 1, "owner {owner} lost a hidden/unreferenced table");
                assert_eq!(cells[0].text.text(), if stale { "edited display" } else { "original display" });
                assert_eq!(cells[0].data.is_none(), stale, "owner {owner}");
                if !stale {
                    assert_eq!(cells[0].data.as_ref().unwrap().value(), boundary_cells(&p)[0].data.as_ref().unwrap().value());
                }
            }
            assert!(boundary_cells(&p)[0].data.is_some());
        }
    }
}

#[test]
fn covered_and_ragged_typed_cells_refuse_before_native_normalization_can_remove_them() {
    for case in 0..5 {
        let mut p = boundary_fixture(0, true);
        let slide = Arc::make_mut(&mut p.slides[0]);
        let ShapeKind::Table(table) = &mut slide.shapes[0].kind else { panic!("table") };
        match case {
            0 => table.rows[0].cells[0].h_merge = true,
            1 => table.rows[0].cells[0].v_merge = true,
            2 => table.cols.clear(),
            3 => table.rows[0].cells.insert(0, Cell::new("ordinary anchor")),
            _ => table.cols.push(40.0),
        }
        let original = p.clone();
        let pointers = boundary_owner_pointers(&p);
        assert!(save(&p).is_err(), "case {case}");
        assert_eq!(p, original);
        assert_eq!(boundary_owner_pointers(&p), pointers);
        assert!(load(&package(&boundary_manifest(&p))).is_err(), "case {case}: repair must not hide raw typed metadata");
        let mut candidate = p.clone();
        assert!(deckcraft_model::table::reconcile_constants(&mut candidate).is_err());
        assert_eq!(boundary_owner_pointers(&candidate), pointers);
    }
}

#[test]
fn invalid_late_owner_prevents_earlier_stale_owner_detachment() {
    let mut p = boundary_fixture(0, true);
    let mut table = boundary_table(false);
    let ShapeKind::Table(t) = &mut table.kind else { panic!("table") };
    t.rows[0].cells[0].h_merge = true;
    p.handout_master = Some(boundary_master(350, table, true));
    let original = p.clone();
    let pointers = boundary_owner_pointers(&p);
    assert!(deckcraft_model::table::reconcile_constants(&mut p).is_err());
    assert_eq!(p, original);
    assert_eq!(boundary_owner_pointers(&p), pointers);
    assert!(save(&p).is_err());
}

#[test]
fn repeated_master_attachments_have_shared_aggregate_metadata_and_display_byte_limits() {
    let mut p = boundary_fixture(0, false);
    p.slides.clear();
    p.masters.clear();
    let mut table = boundary_table(false);
    let ShapeKind::Table(t) = &mut table.kind else { panic!("table") };
    let cell = &mut t.rows[0].cells[0];
    cell.text = TextBody::from_text("");
    cell.data = Some(Arc::new(CellConstant::from_display(CellValue::String { value: "x".repeat(65_536) }, &cell.text).unwrap()));
    let owner = boundary_master(400, table, false);
    p.masters = vec![owner.clone(); 520];
    assert!(deckcraft_model::table::validate_constants(&p).unwrap_err().to_string().contains("metadata"));
    assert!(save(&p).is_err());
    assert!(Arc::ptr_eq(&p.masters[0], &owner));
    let mut table = boundary_table(true);
    let ShapeKind::Table(t) = &mut table.kind else { panic!("table") };
    t.rows[0].cells[0].text = TextBody::from_text(&"x".repeat(65_536));
    let owner = boundary_master(410, table, false);
    p.masters = vec![owner.clone(); 513];
    assert!(deckcraft_model::table::validate_constants(&p).unwrap_err().to_string().contains("current display bytes"));
    assert!(save(&p).is_err());
    assert!(Arc::ptr_eq(&p.masters[0], &owner));
}

#[test]
fn oversized_stale_shared_owners_obey_the_complete_native_profile_before_cow() {
    let mut p = boundary_fixture(0, false);
    p.slides.clear();
    p.masters.clear();
    let mut table = boundary_table(true);
    let ShapeKind::Table(t) = &mut table.kind else { panic!("table") };
    // Unsupported current display becomes stale without walking its oversized run.
    // Save must still enforce the complete JSON profile before cloning this owner.
    t.rows[0].cells[0].text = TextBody::from_text(&"x".repeat(1024 * 1024 + 1));
    let owner = boundary_master(430, table, false);
    p.masters = vec![owner.clone(); 4];
    let before = boundary_owner_pointers(&p);
    let error = save(&p).unwrap_err().to_string();
    assert!(error.contains("token") || error.contains("size limit"), "{error}");
    assert_eq!(boundary_owner_pointers(&p), before);
    assert!(Arc::ptr_eq(&owner, &p.masters[0]));
    assert!(boundary_cells(&p).iter().all(|cell| cell.data.is_some()));
    // A display just beyond the per-record limit but within the native profile can
    // legitimately become literal text; the source's stale attachment stays intact.
    let mut p = boundary_fixture(0, true);
    let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
    t.rows[0].cells[0].text = TextBody::from_text(&"x".repeat(65_537));
    let back = load(&save(&p).unwrap()).unwrap();
    assert!(boundary_cells(&back)[0].data.is_none());
    assert_eq!(boundary_cells(&back)[0].text.text().len(), 65_537);
    assert!(boundary_cells(&p)[0].data.is_some());
}

#[test]
fn native_stale_lowering_reports_one_fixed_nonprivate_load_diagnostic() {
    let mut p = boundary_fixture(0, true);
    let other = boundary_fixture(8, true);
    p.handout_master = other.handout_master;
    let (back, messages) = deckcraft_format::load_checked(&package(&boundary_manifest(&p))).unwrap();
    assert!(boundary_cells(&back).iter().all(|cell| cell.data.is_none()));
    assert_eq!(messages, vec!["Stale stored table values were cleared; displayed text was retained"]);
    assert!(!messages[0].contains("independent stored value"));
    assert!(!messages[0].contains("edited display"));
    let (_, messages) = deckcraft_format::load_checked(&save(&p).unwrap()).unwrap();
    assert!(messages.is_empty(), "save has already explicitly lowered the stale records");
    let (_, messages) = deckcraft_format::load_checked(&package(&boundary_manifest(&boundary_fixture(0, false)))).unwrap();
    assert!(messages.is_empty(), "current constants are not repairs");
}
