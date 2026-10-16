//! Independent contributor-original package-level authority and geometry checks.
//! Staged review fixtures only; root owns compilation and runtime acceptance.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    table::*,
    text::{Anchor, TextBody},
    *,
};
use deckcraft_pptx::{XmlEl, parse_xml};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

fn fixture() -> Presentation {
    let mut p = Presentation {
        masters: vec![Arc::new(Master {
            id: MasterId(10),
            layouts: vec![Layout { id: LayoutId(20), show_master_shapes: true, ..Default::default() }],
            ..Default::default()
        })],
        ..Default::default()
    };
    let mut t = Table::new(1, 1, 240., 60.);
    t.style = "grid".into();
    t.first_row = false;
    let c = t.cell_mut(0, 0).unwrap();
    c.text = TextBody::from_text("Independent canonical cell");
    c.data = Some(Arc::new(CellConstant::from_display(CellValue::String { value: "Independent canonical cell".into() }, &c.text).unwrap()));
    p.slides = vec![Arc::new(Slide {
        id: SlideId(30),
        layout: LayoutId(20),
        shapes: vec![Shape {
            id: ShapeId(300),
            name: "Independent enhanced table".into(),
            xfrm: Some(Xfrm::new(30., 40., 240., 60.)),
            kind: ShapeKind::Table(t),
            ..Default::default()
        }],
        ..Default::default()
    })];
    p.fix_next_id();
    p
}
fn tagged() -> Presentation {
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).shapes[0].tags = vec![("independent-key".into(), "ORIGINAL_TAG_VALUE".into())];
    p
}
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let n = f.name().to_string();
            let mut d = vec![];
            f.read_to_end(&mut d).unwrap();
            (n, d)
        })
        .collect()
}
fn edit(bytes: &[u8], mut change: impl FnMut(&str, &mut Vec<u8>) -> bool) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, mut d) in entries(bytes) {
        if change(&n, &mut d) {
            z.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&d).unwrap();
        }
    }
    z.finish().unwrap().into_inner()
}
fn xml_part(bytes: &[u8], part: &str) -> String {
    String::from_utf8(entries(bytes).into_iter().find(|(n, _)| n == part).unwrap().1).unwrap()
}
fn replace(bytes: &[u8], part: &str, old: &str, new: &str) -> Vec<u8> {
    let mut hits = 0;
    let out = edit(bytes, |n, d| {
        if n == part {
            let s = String::from_utf8(d.clone()).unwrap();
            assert!(s.contains(old));
            *d = s.replacen(old, new, 1).into_bytes();
            hits += 1;
        }
        true
    });
    assert_eq!(hits, 1);
    out
}
fn nodes(xml: &str) -> Vec<XmlEl> {
    parse_xml(xml.as_bytes()).unwrap().root.find("graph").unwrap().children_named("node").cloned().collect()
}
fn node_at(nodes: &[XmlEl], id: u32) -> &XmlEl {
    nodes.iter().find(|n| n.u32("id") == Some(id)).unwrap()
}
fn member(nodes: &[XmlEl], owner: u32, key: &str) -> u32 {
    node_at(nodes, owner).children_named("member").find(|m| m.attr("name") == Some(key)).unwrap().u32("ref").unwrap()
}
fn scalar_path(xml: &str, path: &[&str]) -> u32 {
    let ns = nodes(xml);
    path.iter().fold(0, |id, key| member(&ns, id, key))
}
fn scalar_text(xml: &str, id: u32) -> String {
    node_at(&nodes(xml), id).text()
}
fn scalar_value_id(xml: &str, value: &str) -> u32 {
    nodes(xml).iter().find(|n| n.elements().next().is_none() && n.text() == value).unwrap().u32("id").unwrap()
}
fn replace_scalar(xml: &str, id: u32, value: &str) -> String {
    let mut s = xml.to_owned();
    let prefix = format!("<dc:node id=\"{id}\" ");
    let start = s.find(&prefix).unwrap();
    let begin = start + s[start..].find('>').unwrap() + 1;
    let end = begin + s[begin..].find("</dc:node>").unwrap();
    s.replace_range(begin..end, value);
    s
}
fn set_scalar_path(xml: &str, path: &[&str], value: &str) -> String {
    let ns = nodes(xml);
    let (last, parent) = path.split_last().unwrap();
    let owner = parent.iter().fold(0, |id, key| member(&ns, id, key));
    if let Some(binding) = node_at(&ns, owner).children_named("member").find(|m| m.attr("name") == Some(*last)) {
        return replace_scalar(xml, binding.u32("ref").unwrap(), value);
    }
    // Default zero/false transforms are deliberately omitted by native serde.
    // Append one owned scalar with a forward ref so this remains a valid ledger.
    let mut out = xml.to_owned();
    let prefix = format!("<dc:node id=\"{owner}\" ");
    let start = out.find(&prefix).unwrap();
    let end = start + out[start..].find("</dc:node>").unwrap();
    let id = ns.len();
    out.insert_str(end, &format!("<dc:member name=\"{last}\" ref=\"{id}\"/>"));
    let kind = if matches!(value, "true" | "false") { "boolean" } else { "number" };
    let extra = format!("<dc:node id=\"{id}\" kind=\"{kind}\">{value}</dc:node></dc:graph>");
    out.replacen("</dc:graph>", &extra, 1)
}
fn mutate_table_scalar(bytes: &[u8], path: &[&str], value: &str) -> Vec<u8> {
    edit(bytes, |n, d| {
        if n == "ppt/slides/slide1.xml" {
            let s = String::from_utf8(d.clone()).unwrap();
            *d = set_scalar_path(&s, path, value).into_bytes();
        }
        true
    })
}
fn table_shapes(p: &Presentation) -> Vec<&Shape> {
    let mut out = vec![];
    walk(&p.slides[0].shapes, &mut |s, _| {
        if matches!(s.kind, ShapeKind::Table(_)) {
            out.push(s)
        }
    });
    out
}
fn strip_table_extensions(bytes: &[u8]) -> Vec<u8> {
    edit(bytes, |n, d| {
        if n.ends_with(".xml") {
            let mut xml = String::from_utf8(d.clone()).unwrap();
            let open = "<p:extLst><p:ext uri=\"urn:deckcraft:pptx:table:1\"";
            while let Some(start) = xml.find(open) {
                let end = start + xml[start..].find("</p:extLst>").unwrap() + "</p:extLst>".len();
                xml.replace_range(start..end, "");
            }
            *d = xml.into_bytes();
        }
        true
    })
}

#[test]
fn independently_changed_standard_tag_payload_cannot_restore_stale_native_tag() {
    let original = deckcraft_pptx::export(&tagged()).unwrap();
    let changed = replace(&original, "ppt/tags/tag1.xml", "ORIGINAL_TAG_VALUE", "FOREIGN_TAG_VALUE");
    assert!(deckcraft_pptx::import(&changed).is_err(), "known table metadata must invalidate when its ordinary tag dependency changes");
    let core = deckcraft_pptx::import(&strip_table_extensions(&changed)).unwrap();
    assert_eq!(table_shapes(&core)[0].tags[0].1, "FOREIGN_TAG_VALUE");
}
#[test]
fn deleted_standard_tag_dependency_cannot_be_resurrected_from_table_metadata() {
    let bytes = deckcraft_pptx::export(&tagged()).unwrap();
    let changed = edit(&bytes, |n, _| n != "ppt/tags/tag1.xml");
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn standard_tag_dependency_content_type_is_bound() {
    let bytes = deckcraft_pptx::export(&tagged()).unwrap();
    let changed = replace(
        &bytes,
        "[Content_Types].xml",
        "application/vnd.openxmlformats-officedocument.presentationml.tags+xml",
        "application/x-independent-foreign-tag",
    );
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn metadata_only_tag_forgery_is_checked_against_independently_generated_tag_bytes() {
    let bytes = deckcraft_pptx::export(&tagged()).unwrap();
    let changed = edit(&bytes, |n, d| {
        if n == "ppt/slides/slide1.xml" {
            let s = String::from_utf8(d.clone()).unwrap();
            *d = replace_scalar(&s, scalar_value_id(&s, "ORIGINAL_TAG_VALUE"), "FORGED_NATIVE_TAG").into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn prior_shape_tags_and_two_enhanced_tables_keep_writer_global_sidepart_identity() {
    let mut p = tagged();
    let mut second = p.slides[0].shapes[0].clone();
    second.id = ShapeId(500);
    second.tags[0].1 = "SECOND_ORIGINAL_TAG".into();
    let ordinary = Shape {
        id: ShapeId(200),
        xfrm: Some(Xfrm::new(0., 0., 10., 10.)),
        tags: vec![("earlier".into(), "Earlier ordinary object".into())],
        ..Default::default()
    };
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.shapes.insert(0, ordinary);
    slide.shapes.push(second);
    p.fix_next_id();
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let tables = table_shapes(&back);
    assert_eq!(tables.len(), 2);
    assert_eq!(tables[0].tags[0].1, "ORIGINAL_TAG_VALUE");
    assert_eq!(tables[1].tags[0].1, "SECOND_ORIGINAL_TAG");
    let again = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
    assert_eq!(table_shapes(&again)[1].tags[0].1, "SECOND_ORIGINAL_TAG");
}
#[test]
fn forged_matching_dependency_hashes_do_not_replace_fresh_generated_byte_comparison() {
    let original = deckcraft_pptx::export(&tagged()).unwrap();
    let mut foreign = tagged();
    Arc::make_mut(&mut foreign.slides[0]).shapes[0].tags[0].1 = "FOREIGN_TAG_VALUE".into();
    let foreign = deckcraft_pptx::export(&foreign).unwrap();
    let old = xml_part(&original, "ppt/slides/slide1.xml");
    let new = xml_part(&foreign, "ppt/slides/slide1.xml");
    let ns = nodes(&old);
    // If dependencies are not yet in the production record, this still exercises
    // the real stale side-part attack; after repair it also reaches regeneration.
    let dependency_root = node_at(&ns, 0).children_named("member").find(|m| m.attr("name") == Some("dependencies")).map(|m| m.u32("ref").unwrap());
    let mut replacements = vec![];
    if let Some(id) = dependency_root {
        let new_ns = nodes(&new);
        let new_id = member(&new_ns, 0, "dependencies");
        let before = node_at(&ns, id).children_named("item").next().unwrap().u32("ref").unwrap();
        let after = node_at(&new_ns, new_id).children_named("item").next().unwrap().u32("ref").unwrap();
        for key in ["bytes", "sha256"] {
            let old_id = member(&ns, before, key);
            let new_id = member(&new_ns, after, key);
            replacements.push((old_id, scalar_text(&new, new_id)));
        }
    }
    let foreign_tag = xml_part(&foreign, "ppt/tags/tag1.xml");
    let changed = edit(&original, |n, d| {
        if n == "ppt/tags/tag1.xml" {
            *d = foreign_tag.as_bytes().to_vec();
        }
        if n == "ppt/slides/slide1.xml" {
            let mut s = String::from_utf8(d.clone()).unwrap();
            for (id, value) in &replacements {
                s = replace_scalar(&s, *id, value);
            }
            *d = s.into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&changed).is_err(), "stored dependency hashes must not authorize different source tags");
}
#[test]
fn metadata_only_simple_table_rotation_cannot_invent_unprojected_native_geometry() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    let changed = mutate_table_scalar(&bytes, &["source", "xfrm", "rot"], "45.0");
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn metadata_only_simple_table_flip_cannot_invent_unprojected_native_geometry() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    for key in ["flipH", "flipV"] {
        let changed = mutate_table_scalar(&bytes, &["source", "xfrm", key], "true");
        assert!(deckcraft_pptx::import(&changed).is_err());
    }
}
#[test]
fn legitimate_rotated_typed_table_keeps_transform_in_standard_editable_core() {
    let mut p = fixture();
    let x = Arc::make_mut(&mut p.slides[0]).shapes[0].xfrm.as_mut().unwrap();
    x.rot = 45.;
    x.flip_h = true;
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let core = strip_table_extensions(&bytes);
    let xml = xml_part(&core, "ppt/slides/slide1.xml");
    assert!(xml.contains("rot=\"2700000\""), "standard projection must actually carry the 45-degree transform");
    assert!(xml.contains("flipH=\"1\""));
    assert!(deckcraft_pptx::import(&core).is_ok());
    let back = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(table_shapes(&back)[0].xfrm, p.slides[0].shapes[0].xfrm);
}
#[test]
fn foreign_core_geometry_with_rewritten_saved_projection_hash_is_still_refused() {
    let original = deckcraft_pptx::export(&fixture()).unwrap();
    let mut moved = fixture();
    Arc::make_mut(&mut moved.slides[0]).shapes[0].xfrm.as_mut().unwrap().x = 37.;
    let moved = deckcraft_pptx::export(&moved).unwrap();
    let moved_xml = xml_part(&moved, "ppt/slides/slide1.xml");
    let moved_hash = scalar_text(&moved_xml, scalar_path(&moved_xml, &["projection"]));
    let changed = edit(&original, |n, d| {
        if n == "ppt/slides/slide1.xml" {
            let old = String::from_utf8(d.clone()).unwrap();
            let mut s = old.replacen("x=\"381000\"", "x=\"469900\"", 1);
            assert_ne!(s, old);
            let id = scalar_path(&s, &["projection"]);
            s = replace_scalar(&s, id, &moved_hash);
            *d = s.into_bytes();
        }
        true
    });
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn flat_metadata_namespace_scoped_node_and_edge_spoofs_are_refused() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    for (old, new) in [
        ("<dc:node id=\"0\"", "<dc:node xmlns:dc=\"urn:independent:foreign\" id=\"0\""),
        ("<dc:member name=\"source\"", "<dc:member xmlns:dc=\"urn:independent:foreign\" name=\"source\""),
    ] {
        let changed = replace(&bytes, "ppt/slides/slide1.xml", old, new);
        assert!(deckcraft_pptx::import(&changed).is_err());
    }
}
#[test]
fn flat_ledger_rejects_noncanonical_reference_lexicals_and_out_of_range_ids() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    for value in ["4294967295", "18446744073709551615", "01", "+1"] {
        let changed = edit(&bytes, |n, d| {
            if n == "ppt/slides/slide1.xml" {
                let s = String::from_utf8(d.clone()).unwrap();
                let ns = nodes(&s);
                let id = member(&ns, 0, "source");
                let old = format!("name=\"source\" ref=\"{id}\"");
                *d = s.replacen(&old, &format!("name=\"source\" ref=\"{value}\""), 1).into_bytes();
            }
            true
        });
        assert!(deckcraft_pptx::import(&changed).is_err());
    }
}
#[test]
fn duplicated_ordinary_shape_ids_in_enhanced_projection_are_refused() {
    let mut p = fixture();
    let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
    let mut child = Table::new(1, 1, 100., 25.);
    child.style = "grid".into();
    child.cell_mut(0, 0).unwrap().text = TextBody::from_text("Child identity witness");
    t.cell_mut(0, 0).unwrap().data = None;
    t.cell_mut(0, 0).unwrap().append_nested_table(child).unwrap();
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let changed = replace(&bytes, "ppt/slides/slide1.xml", "id=\"301\"", "id=\"300\"");
    assert!(deckcraft_pptx::import(&changed).is_err());
}

#[test]
fn nested_geometry_matches_independent_margin_scale_anchor_and_merge_physics() {
    let mut p = fixture();
    let mut t = Table::new(2, 2, 300., 120.);
    t.cols = vec![180., 120.];
    t.style = "grid".into();
    t.first_row = false;
    // Parent row one is merged across both columns: width300 less 10+20 margins.
    // A child width540 scales to .5, producing h30 from native h60.
    assert!(t.merge(1, 0, 1, 1));
    let c = t.cell_mut(1, 0).unwrap();
    c.text = TextBody::default();
    c.margins = Some([10., 20., 15., 25.]);
    c.anchor = Some(Anchor::Bottom);
    let mut child = Table::new(1, 2, 540., 60.);
    child.style = "grid".into();
    child.first_row = false;
    child.cols = vec![320., 220.];
    child.cell_mut(0, 0).unwrap().text = TextBody::from_text("Nested left");
    child.cell_mut(0, 1).unwrap().text = TextBody::from_text("Nested right");
    c.append_nested_table(child).unwrap();
    Arc::make_mut(&mut p.slides[0]).shapes[0].kind = ShapeKind::Table(t);
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let xml = xml_part(&strip_table_extensions(&bytes), "ppt/slides/slide1.xml");
    let doc = parse_xml(xml.as_bytes()).unwrap();
    let mut groups = vec![];
    doc.root.find_all("grpSp", &mut groups);
    let nested = groups.iter().find(|g| g.path(&["grpSpPr", "xfrm", "ext"]).is_some_and(|e| e.i64("cx") == Some(270 * 12700))).unwrap();
    let xf = nested.path(&["grpSpPr", "xfrm"]).unwrap();
    let offset = xf.child("off").unwrap();
    let size = xf.child("ext").unwrap();
    let child = xf.child("chExt").unwrap();
    assert_eq!(offset.i64("x"), Some(10 * 12700));
    // Row0=120, free parent space=120-(15+30+25)=50: y=120+15+50=185.
    assert_eq!(offset.i64("y"), Some(185 * 12700));
    assert_eq!(size.i64("cy"), Some(30 * 12700));
    assert_eq!(child.i64("cx"), Some(540 * 12700));
    assert_eq!(child.i64("cy"), Some(60 * 12700));
    let root_grid = doc.root.find("tbl").unwrap();
    let row = root_grid.children_named("tr").nth(1).unwrap();
    assert_eq!(row.children_named("tc").next().unwrap().u32("gridSpan"), Some(2));
    assert_eq!(row.children_named("tc").nth(1).unwrap().bool("hMerge"), Some(true));
    assert!(xml.contains("Nested left") && xml.contains("Nested right"));
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let ShapeKind::Table(actual) = &table_shapes(&back)[0].kind else { panic!("table") };
    let ShapeKind::Table(expected) = &p.slides[0].shapes[0].kind else { panic!("table") };
    assert_eq!(actual, expected);
}

#[test]
fn transformed_table_wrapper_keeps_native_owner_connector_and_typed_body_bindings() {
    let mut p = fixture();
    let original = Arc::make_mut(&mut p.slides[0]);
    let x = original.shapes[0].xfrm.as_mut().unwrap();
    x.rot = 90.;
    x.flip_v = true;
    original.shapes.push(Shape {
        id: ShapeId(400),
        xfrm: Some(Xfrm::new(0., 0., 50., 50.)),
        kind: ShapeKind::Connector { start: Some((ShapeId(300), 2)), end: None },
        ..Default::default()
    });
    p.fix_next_id();
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let doc = parse_xml(xml_part(&strip_table_extensions(&bytes), "ppt/slides/slide1.xml").as_bytes()).unwrap();
    let group = doc.root.find("grpSp").unwrap();
    assert_eq!(group.path(&["nvGrpSpPr", "cNvPr"]).unwrap().u32("id"), Some(300));
    let inner = group.child("graphicFrame").unwrap();
    assert_ne!(inner.path(&["nvGraphicFramePr", "cNvPr"]).unwrap().u32("id"), Some(300));
    assert_eq!(doc.root.find("stCxn").unwrap().u32("id"), Some(300));
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let table = table_shapes(&back)[0];
    assert_eq!(table.xfrm, p.slides[0].shapes[0].xfrm);
    let ShapeKind::Table(t) = &table.kind else { panic!("table") };
    assert_eq!(t.cell(0, 0).unwrap().text.text(), "Independent canonical cell");
    assert!(t.cell(0, 0).unwrap().data.is_some());
    let ShapeKind::Connector { start: Some((target, 2)), .. } = &back.slides[0].shapes[1].kind else { panic!("connector") };
    assert_eq!(*target, table.id);
}
#[test]
fn hostile_generated_dependency_counter_refuses_before_regeneration() {
    let bytes = deckcraft_pptx::export(&tagged()).unwrap();
    let changed = mutate_table_scalar(&bytes, &["counters", "tags"], "4294967295");
    assert!(deckcraft_pptx::import(&changed).is_err());
}

fn xml_group_point(group: &XmlEl, point: (f64, f64)) -> (f64, f64) {
    // Independent CT_GroupTransform2D reconstruction, intentionally not using
    // Xfrm::affine/group_child_affine or production expected-projection helpers.
    let x = group.path(&["grpSpPr", "xfrm"]).unwrap();
    let off = x.child("off").unwrap();
    let size = x.child("ext").unwrap();
    let child_off = x.child("chOff").unwrap();
    let child_size = x.child("chExt").unwrap();
    let pt = |e: &XmlEl, key: &str| e.i64(key).unwrap() as f64 / 12700.;
    let w = pt(size, "cx");
    let h = pt(size, "cy");
    let mut px = (point.0 - pt(child_off, "x")) * w / pt(child_size, "cx") - w / 2.;
    let mut py = (point.1 - pt(child_off, "y")) * h / pt(child_size, "cy") - h / 2.;
    if x.bool("flipH") == Some(true) {
        px = -px;
    }
    if x.bool("flipV") == Some(true) {
        py = -py;
    }
    let angle = x.i64("rot").unwrap_or(0) as f64 / 60000. * std::f64::consts::PI / 180.;
    (pt(off, "x") + w / 2. + px * angle.cos() - py * angle.sin(), pt(off, "y") + h / 2. + px * angle.sin() + py * angle.cos())
}
#[test]
fn mismatched_native_box_retains_affine_pivot_and_intrinsic_overflow_for_simple_and_nested_tables() {
    for nested in [false, true] {
        for (rotation, flip_h, flip_v) in [(0., false, false), (90., false, false), (0., true, false), (0., false, true), (90., true, true)] {
            let mut p = fixture();
            let owner = &mut Arc::make_mut(&mut p.slides[0]).shapes[0];
            owner.xfrm = Some(Xfrm { rot: rotation, flip_h, flip_v, ..Xfrm::new(30., 40., 100., 20.) });
            if nested {
                let ShapeKind::Table(t) = &mut owner.kind else { panic!("table") };
                let c = t.cell_mut(0, 0).unwrap();
                c.data = None;
                let mut child = Table::new(1, 1, 100., 30.);
                child.style = "grid".into();
                child.cell_mut(0, 0).unwrap().text = TextBody::from_text("Pivot child");
                c.append_nested_table(child).unwrap();
            }
            let bytes = deckcraft_pptx::export(&p).unwrap();
            let core = strip_table_extensions(&bytes);
            let doc = parse_xml(xml_part(&core, "ppt/slides/slide1.xml").as_bytes()).unwrap();
            let group = doc.root.find("grpSp").unwrap();
            let x = group.path(&["grpSpPr", "xfrm"]).unwrap();
            assert_eq!(x.child("ext").unwrap().i64("cx"), Some(100 * 12700));
            assert_eq!(x.child("ext").unwrap().i64("cy"), Some(20 * 12700));
            assert_eq!(x.child("chExt").unwrap().i64("cx"), Some(100 * 12700));
            assert_eq!(x.child("chExt").unwrap().i64("cy"), Some(20 * 12700));
            let intrinsic = group.child("graphicFrame").unwrap().child("xfrm").unwrap().child("ext").unwrap();
            assert_eq!(intrinsic.i64("cx"), Some(240 * 12700));
            assert!(intrinsic.i64("cy").unwrap() >= 60 * 12700);
            for point in [(0., 0.), (240., 0.), (0., 60.), (240., 60.)] {
                let mut px = point.0 - 50.;
                let mut py = point.1 - 10.;
                if flip_h {
                    px = -px;
                }
                if flip_v {
                    py = -py;
                }
                let expected = if rotation == 90. { (80. - py, 50. + px) } else { (80. + px, 50. + py) };
                let actual = xml_group_point(group, point);
                assert!(
                    (actual.0 - expected.0).abs() < 1e-7 && (actual.1 - expected.1).abs() < 1e-7,
                    "nested={nested}, angle={rotation}, flips={flip_h}/{flip_v}, point={point:?}: {actual:?} != {expected:?}"
                );
            }
            let back = deckcraft_pptx::import(&bytes).unwrap();
            assert_eq!(table_shapes(&back)[0].xfrm, p.slides[0].shapes[0].xfrm);
        }
    }
}
#[test]
fn source_box_only_metadata_change_cannot_change_the_unprojected_rotation_pivot() {
    let mut p = fixture();
    Arc::make_mut(&mut p.slides[0]).shapes[0].xfrm.as_mut().unwrap().rot = 90.;
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let changed = mutate_table_scalar(&bytes, &["source", "xfrm", "w"], "100.0");
    assert!(deckcraft_pptx::import(&changed).is_err());
}
#[test]
fn anonymous_array_edge_bomb_stops_at_edge_budget_before_owner_vector_collection() {
    let bytes = deckcraft_pptx::export(&fixture()).unwrap();
    let changed = edit(&bytes, |n, d| {
        if n == "ppt/slides/slide1.xml" {
            let mut xml = String::from_utf8(d.clone()).unwrap();
            let ns = nodes(&xml);
            let id = member(&ns, 0, "ids");
            let old = node_at(&ns, id).to_xml();
            let edges = format!("<dc:item ref=\"{}\"/>", ns.len()).repeat(200_001);
            let node = format!("<dc:node id=\"{id}\" kind=\"array\">{edges}</dc:node>");
            assert!(xml.contains(&old));
            xml = xml.replacen(&old, &node, 1);
            *d = xml.into_bytes();
        }
        true
    });
    let error = deckcraft_pptx::import(&changed).unwrap_err().to_string();
    assert!(error.contains("metadata graph edge limit"), "must stop before allocating the full unowned edge sequence: {error}");
}
#[test]
fn notes_anonymous_array_edge_bomb_uses_the_same_bounded_wire_contract() {
    let mut p = fixture();
    p.notes_master = Some(Arc::new(Master { id: MasterId(800), ..Default::default() }));
    Arc::make_mut(&mut p.slides[0]).notes = TextBody::from_text("Independent bounded notes");
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let changed = edit(&bytes, |n, d| {
        if n == "ppt/notesSlides/notesSlide1.xml" {
            let mut xml = String::from_utf8(d.clone()).unwrap();
            let ns = nodes(&xml);
            let id = member(&ns, 0, "tables");
            let old = node_at(&ns, id).to_xml();
            let edges = format!("<dc:item ref=\"{}\"/>", ns.len()).repeat(200_001);
            let node = format!("<dc:node id=\"{id}\" kind=\"array\">{edges}</dc:node>");
            assert!(xml.contains(&old));
            xml = xml.replacen(&old, &node, 1);
            *d = xml.into_bytes();
        }
        true
    });
    let error = deckcraft_pptx::import(&changed).unwrap_err().to_string();
    assert!(error.contains("metadata graph edge limit"), "notes must stop before collecting all anonymous edges: {error}");
}
