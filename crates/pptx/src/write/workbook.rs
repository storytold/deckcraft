//! Bounded, transactional cell updates in an embedded XLSX. Unrelated entries stay byte-identical.
use crate::{
    opc,
    xml::{A, El, Node, W},
};
use deckcraft_model::chart::Chart;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

const LIMIT: usize = 32 << 20;
const CELLS: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Text(String),
    Number(f64),
    Blank,
}
type Writes = BTreeMap<(String, u32, u32), Value>;

fn address(s: &str) -> Option<(u32, u32)> {
    let s = s.replace('$', "");
    let at = s.find(|c: char| c.is_ascii_digit())?;
    let (col, row) = s.split_at(at);
    if col.is_empty() || col.len() > 3 || !col.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let col = col.bytes().fold(0u32, |a, c| a * 26 + u32::from(c.to_ascii_uppercase() - b'A' + 1));
    let row: u32 = row.parse().ok()?;
    (col <= 16384 && row > 0 && row <= 1048576).then_some((row, col))
}
fn cell_name(row: u32, mut col: u32) -> String {
    let mut letters = Vec::new();
    while col > 0 {
        letters.push(char::from(b'A' + ((col - 1) % 26) as u8));
        col = (col - 1) / 26;
    }
    format!("{}{row}", letters.into_iter().rev().collect::<String>())
}
fn range(f: &str) -> Option<(String, Vec<(u32, u32)>)> {
    let (sheet, refs) = f.trim().rsplit_once('!')?;
    if sheet.contains(['[', ']']) {
        return None;
    }
    let sheet = if sheet.starts_with('\'') && sheet.ends_with('\'') { sheet.get(1..sheet.len() - 1)?.replace("''", "'") } else { sheet.to_string() };
    let (a, b) = refs.split_once(':').unwrap_or((refs, refs));
    let (ar, ac) = address(a)?;
    let (br, bc) = address(b)?;
    if ar > br || ac > bc || (ar != br && ac != bc) {
        return None;
    }
    let count = usize::try_from(br - ar + bc - ac + 1).ok()?;
    if count > CELLS {
        return None;
    }
    Some((sheet, (0..count).map(|i| (ar + if ar != br { i as u32 } else { 0 }, ac + if ac != bc { i as u32 } else { 0 })).collect()))
}

fn qualified(parent: &El, local: &str) -> String {
    parent.name.rsplit_once(':').map_or_else(|| local.to_string(), |(prefix, _)| format!("{prefix}:{local}"))
}

fn reference(container: &mut El, values: Vec<Value>, writes: &mut Writes) -> Option<()> {
    // Literal series names do not have a corresponding spreadsheet cell.
    if container.is("tx") && container.child("strRef").is_none() {
        let Value::Text(text) = values.first()? else {
            return None;
        };
        let v = container.elements_mut().find(|e| e.is("v"))?;
        v.children = vec![Node::Text(text.clone())];
        return Some(());
    }
    let r = container.elements_mut().find(|e| e.is("strRef") || e.is("numRef"))?;
    let numeric = r.is("numRef");
    let (sheet, cells) = range(&r.child("f")?.text())?;
    // Resizing or indirect/name formulas need a separate dependency-aware editor.
    if cells.len() != values.len() {
        return None;
    }
    let mut normalized = Vec::new();
    for ((row, col), mut value) in cells.into_iter().zip(values) {
        if numeric && let Value::Text(s) = &value {
            value = Value::Number(s.parse::<f64>().ok().filter(|n| n.is_finite())?);
        }
        let key = (sheet.clone(), row, col);
        if writes.get(&key).is_some_and(|old| old != &value) || writes.len() >= CELLS {
            return None;
        }
        writes.insert(key, value.clone());
        normalized.push(value);
    }
    let cache_tag = qualified(r, if numeric { "numCache" } else { "strCache" });
    let format = r.child("numCache").and_then(|e| e.child("formatCode")).map(El::text).unwrap_or_else(|| "General".into());
    let mut w = W::frag();
    w.open0(&cache_tag);
    if numeric {
        w.elt(&qualified(r, "formatCode"), &format);
    }
    w.val(&qualified(r, "ptCount"), normalized.len());
    for (i, value) in normalized.iter().enumerate() {
        let text = match value {
            Value::Text(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Blank => continue,
        };
        w.open(&qualified(r, "pt"), A::new().a("idx", i));
        w.elt(&qualified(r, "v"), &text);
        w.close(&qualified(r, "pt"));
    }
    w.close(&cache_tag);
    r.children.retain(|n| !matches!(n,Node::El(e) if e.is("strCache") || e.is("numCache")));
    r.children.push(Node::El(crate::xml::parse(&w.finish()).ok()?.root));
    Some(())
}

fn unzip(bytes: &[u8]) -> Option<BTreeMap<String, (String, Vec<u8>)>> {
    if bytes.len() > LIMIT {
        return None;
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    if zip.len() > 2048 {
        return None;
    }
    let mut entries = BTreeMap::new();
    let mut total = 0usize;
    for i in 0..zip.len() {
        let f = zip.by_index(i).ok()?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        let key = opc::norm(&name);
        if name.starts_with('/') || key.split('/').any(|p| matches!(p, "" | "." | "..")) || key.starts_with("_xmlsignatures/") {
            return None;
        }
        let cap = LIMIT.saturating_sub(total).min(8 << 20);
        if f.size() > cap as u64 {
            return None;
        }
        let mut data = Vec::new();
        f.take(cap as u64 + 1).read_to_end(&mut data).ok()?;
        if data.len() > cap {
            return None;
        }
        total += data.len();
        if entries.insert(key, (name, data)).is_some() {
            return None;
        }
    }
    Some(entries)
}

fn patch_cells(sheet: &mut El, values: &BTreeMap<(u32, u32), Value>) -> Option<()> {
    // These ranges have additional owners (table column names / shared formula regions).
    // Leave them to a dependency-aware spreadsheet editor.
    if sheet.child("tableParts").is_some() || sheet.child("sheetProtection").is_some() {
        return None;
    }
    let data = sheet.elements_mut().find(|e| e.is("sheetData"))?;
    for row in data.children_named("row") {
        for cell in row.children_named("c") {
            if let Some(formula) = cell.child("f")
                && matches!(formula.attr("t"), Some("array" | "shared" | "dataTable"))
            {
                return None;
            }
        }
    }
    // Update rows in one pass; retain styles, metadata and all unrelated cells/formulas.
    let mut pending = values.clone();
    for row in data.elements_mut().filter(|e| e.is("row")) {
        for cell in row.elements_mut().filter(|e| e.is("c")) {
            let at = address(cell.attr("r")?)?;
            if let Some(value) = pending.remove(&at) {
                set_cell(cell, &value)?;
            }
        }
    }
    for ((row, col), value) in pending {
        let row_name = row.to_string();
        let pos = data.children.iter().position(|n| matches!(n,Node::El(e) if e.is("row") && e.attr("r")==Some(row_name.as_str())));
        let index = match pos {
            Some(i) => i,
            None => {
                data.children.push(Node::El(El { name: qualified(data, "row"), attrs: vec![("r".into(), row_name)], children: vec![] }));
                data.children.len() - 1
            }
        };
        let Node::El(row_el) = data.children.get_mut(index)? else {
            return None;
        };
        let mut cell = El { name: qualified(row_el, "c"), attrs: vec![("r".into(), cell_name(row, col))], children: vec![] };
        set_cell(&mut cell, &value)?;
        row_el.children.push(Node::El(cell));
    }
    for row in data.elements_mut().filter(|e| e.is("row")) {
        row.children.sort_by_key(|n| match n {
            Node::El(e) if e.is("c") => e.attr("r").and_then(address).map(|(_, c)| c).unwrap_or(u32::MAX),
            _ => u32::MAX,
        });
    }
    data.children.sort_by_key(|n| match n {
        Node::El(e) if e.is("row") => e.attr("r").and_then(|r| r.parse::<u32>().ok()).unwrap_or(u32::MAX),
        _ => u32::MAX,
    });
    Some(())
}
fn set_cell(cell: &mut El, value: &Value) -> Option<()> {
    // Never overwrite a computed cell or one member of an array/shared formula.
    if cell.child("f").is_some() {
        return None;
    }
    cell.children.retain(|n| !matches!(n,Node::El(e) if e.is("v") || e.is("is")));
    cell.attrs.retain(|(k, _)| k != "t");
    match value {
        Value::Text(s) => {
            cell.set_attr("t", "inlineStr");
            cell.children.push(Node::El(El {
                name: qualified(cell, "is"),
                attrs: vec![],
                children: vec![Node::El(El {
                    name: qualified(cell, "t"),
                    attrs: vec![("xml:space".into(), "preserve".into())],
                    children: vec![Node::Text(s.clone())],
                })],
            }));
        }
        Value::Number(n) => {
            cell.children.push(Node::El(El { name: qualified(cell, "v"), attrs: vec![], children: vec![Node::Text(n.to_string())] }));
        }
        Value::Blank => {}
    }
    Some(())
}

fn patch_workbook(bytes: &[u8], writes: Writes) -> Option<Vec<u8>> {
    let mut entries = unzip(bytes)?;
    let mut workbook = crate::xml::parse(&entries.get("xl/workbook.xml")?.1).ok()?.root;
    let rels = opc::rels_from_bytes(&entries.get("xl/_rels/workbook.xml.rels")?.1, "xl/workbook.xml");
    let mut sheets = BTreeMap::new();
    for s in workbook.child("sheets")?.children_named("sheet") {
        let r = rels.get(s.attr("r:id")?)?;
        if r.external {
            return None;
        }
        sheets.insert(s.attr("name")?.to_string(), opc::norm(&r.target));
    }
    let mut grouped: BTreeMap<String, BTreeMap<(u32, u32), Value>> = BTreeMap::new();
    for ((name, row, col), value) in writes {
        grouped.entry(sheets.get(&name)?.clone()).or_default().insert((row, col), value);
    }
    for (name, values) in grouped {
        let entry = entries.get_mut(&name)?;
        let mut sheet = crate::xml::parse(&entry.1).ok()?.root;
        patch_cells(&mut sheet, &values)?;
        entry.1 = sheet.to_xml().into_bytes();
    }
    if workbook.child("calcPr").is_none() {
        let after_calc = [
            "oleSize",
            "customWorkbookViews",
            "pivotCaches",
            "smartTagPr",
            "smartTagTypes",
            "webPublishing",
            "fileRecoveryPr",
            "webPublishObjects",
            "extLst",
        ];
        let at =
            workbook.children.iter().position(|n| matches!(n, Node::El(e) if after_calc.contains(&e.local()))).unwrap_or(workbook.children.len());
        workbook.children.insert(at, Node::El(El { name: qualified(&workbook, "calcPr"), attrs: vec![], children: vec![] }));
    }
    let calc = workbook.elements_mut().find(|e| e.is("calcPr"))?;
    calc.set_attr("fullCalcOnLoad", "1");
    calc.set_attr("forceFullCalc", "1");
    entries.get_mut("xl/workbook.xml")?.1 = workbook.to_xml().into_bytes();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (_, (name, bytes)) in entries {
        out.start_file(name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).ok()?;
        out.write_all(&bytes).ok()?;
    }
    Some(out.finish().ok()?.into_inner())
}

pub(super) fn synchronize(c: &Chart, original: &El) -> Option<Chart> {
    let id = original.child("externalData")?.attr("r:id")?;
    let rel = c.kept.rels.iter().find(|r| r.id == id && !r.external && r.rel_type.ends_with("/package"))?;
    let part = c.kept.parts.iter().position(|p| opc::norm(&p.name) == opc::norm(&rel.part) && p.name.to_ascii_lowercase().ends_with(".xlsx"))?;
    let mut root = original.clone();
    let mut writes = Writes::new();
    let plot = root.elements_mut().find(|e| e.is("chart"))?.elements_mut().find(|e| e.is("plotArea"))?;
    let mut order: Vec<_> = plot
        .elements()
        .filter(|g| g.local().ends_with("Chart"))
        .flat_map(|g| g.children_named("ser"))
        .enumerate()
        .map(|(i, s)| (s.child_val("order").or_else(|| s.child_val("idx")).and_then(|s| s.parse::<usize>().ok()).unwrap_or(i), i))
        .collect();
    if order.len() != c.series.len() {
        return None;
    }
    order.sort_by_key(|(o, _)| *o);
    let mut positions = vec![0; order.len()];
    for (logical, (_, physical)) in order.into_iter().enumerate() {
        *positions.get_mut(physical)? = logical;
    }
    let mut physical = 0;
    for group in plot.elements_mut().filter(|g| g.local().ends_with("Chart")) {
        for ser in group.elements_mut().filter(|s| s.is("ser")) {
            let series = c.series.get(*positions.get(physical)?)?;
            physical += 1;
            for (tag, values) in [
                ("tx", vec![Value::Text(series.name.clone())]),
                ("cat", c.categories.iter().cloned().map(Value::Text).collect()),
                ("val", series.values.iter().map(|v| v.filter(|n| n.is_finite()).map(Value::Number).unwrap_or(Value::Blank)).collect()),
            ] {
                reference(ser.elements_mut().find(|e| e.is(tag))?, values, &mut writes)?;
            }
        }
    }
    let bytes = patch_workbook(&c.kept.parts.get(part)?.bytes, writes)?;
    let mut out = c.clone();
    out.kept.parts.get_mut(part)?.bytes = bytes;
    out.raw = Some(root.to_xml());
    let mut expected = c.clone();
    expected.raw = None;
    expected.kept = Default::default();
    (crate::read::chart_from_el(&root)? == expected).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::chart::{ChartKept, ChartPart, ChartRel, ChartType};

    fn workbook(c: &Chart) -> Vec<u8> {
        let ns = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let mut w = W::new();
        w.open("worksheet", A::new().a("xmlns", ns));
        w.open0("sheetData");
        for row in 1..=c.categories.len() + 1 {
            w.open("row", A::new().a("r", row));
            for col in 1..=c.series.len() + 1 {
                let value = if row == 1 {
                    Value::Text(if col == 1 { "Category".into() } else { c.series[col - 2].name.clone() })
                } else if col == 1 {
                    Value::Text(c.categories[row - 2].clone())
                } else {
                    c.series[col - 2].values[row - 2].map(Value::Number).unwrap_or(Value::Blank)
                };
                let mut cell =
                    El { name: "c".into(), attrs: vec![("r".into(), cell_name(row as u32, col as u32)), ("s".into(), "0".into())], children: vec![] };
                set_cell(&mut cell, &value).unwrap();
                w.raw(&cell.to_xml());
            }
            w.close("row");
        }
        w.raw("<row r=\"99\"><c r=\"Z99\"><f>SUM(B2:B5)</f><v>15.5</v></c></row>");
        w.close("sheetData");
        w.close("worksheet");
        let main = w.finish();
        let parts=vec![
            ("[Content_Types].xml",r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/notes.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.as_bytes().to_vec()),
            ("_rels/.rels",format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{}officeDocument" Target="xl/workbook.xml"/></Relationships>"#,opc::RT_BASE).into_bytes()),
            ("xl/workbook.xml",format!(r#"<workbook xmlns="{ns}" xmlns:r="{}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" r:id="rId2"/></sheets></workbook>"#,opc::NS_R).into_bytes()),
            ("xl/_rels/workbook.xml.rels",format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{}worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{}worksheet" Target="worksheets/notes.xml"/></Relationships>"#,opc::RT_BASE,opc::RT_BASE).into_bytes()),
            ("xl/worksheets/sheet1.xml",main),
            ("xl/worksheets/notes.xml",format!(r#"<worksheet xmlns="{ns}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Unrelated notes</t></is></c></row></sheetData></worksheet>"#).into_bytes())];
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (n, b) in parts {
            out.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(&b).unwrap();
        }
        out.finish().unwrap().into_inner()
    }
    fn chart() -> Chart {
        let c = Chart::sample(ChartType::Column);
        let mut root = crate::xml::parse(&super::super::chart::chart_xml(&c).0).unwrap().root;
        root.children.push(Node::El(El { name: "c:externalData".into(), attrs: vec![("r:id".into(), "rIdWorkbook".into())], children: vec![] }));
        let mut read = crate::read::chart::read_chart_xml(root.to_xml().as_bytes()).unwrap();
        read.kept = ChartKept {
            rels: vec![ChartRel {
                id: "rIdWorkbook".into(),
                rel_type: format!("{}package", opc::RT_BASE),
                target: "../embeddings/source.xlsx".into(),
                part: "ppt/embeddings/source.xlsx".into(),
                external: false,
            }],
            parts: vec![ChartPart {
                name: "ppt/embeddings/source.xlsx".into(),
                content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
                bytes: workbook(&c),
                native_entry: None,
            }],
        };
        read
    }
    fn cell(root: &El, addr: &str) -> El {
        root.child("sheetData")
            .unwrap()
            .children_named("row")
            .flat_map(|r| r.children_named("c"))
            .find(|c| c.attr("r") == Some(addr))
            .unwrap()
            .clone()
    }

    #[test]
    fn chart_cache_and_workbook_change_together_without_touching_unrelated_content() {
        let mut c = chart();
        let original = c.kept.parts[0].bytes.clone();
        c.series[0].name = "更新 & <名称>".into();
        c.categories[0] = " 新类别 ".into();
        c.series[0].values[0] = Some(91.25);
        c.series[1].values[1] = None;
        let changed = super::super::chart_data::patched(&c).unwrap();
        assert!(changed.raw.as_ref().unwrap().contains("externalData"));
        let saved = unzip(&changed.kept.parts[0].bytes).unwrap();
        let before = unzip(&original).unwrap();
        assert_eq!(saved["xl/worksheets/notes.xml"].1, before["xl/worksheets/notes.xml"].1);
        let sheet = crate::xml::parse(&saved["xl/worksheets/sheet1.xml"].1).unwrap().root;
        let old = crate::xml::parse(&before["xl/worksheets/sheet1.xml"].1).unwrap().root;
        assert_eq!(cell(&sheet, "B1").child("is").unwrap().text(), "更新 & <名称>");
        assert_eq!(cell(&sheet, "A2").child("is").unwrap().text(), " 新类别 ");
        assert_eq!(cell(&sheet, "B2").child("v").unwrap().text(), "91.25");
        assert_eq!(cell(&sheet, "B2").attr("s"), Some("0"));
        assert!(cell(&sheet, "C3").child("v").is_none());
        assert_eq!(cell(&sheet, "Z99").to_xml(), cell(&old, "Z99").to_xml());
        assert!(String::from_utf8_lossy(&saved["xl/workbook.xml"].1).contains("fullCalcOnLoad=\"1\""));
        if let Some(dir) = std::env::var_os("DECKCRAFT_DIAGRAM_OUT") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            let mut p = deckcraft_model::Presentation::default();
            std::sync::Arc::make_mut(&mut p.slides[0]).shapes = vec![deckcraft_model::Shape {
                id: deckcraft_model::ShapeId(900),
                xfrm: Some(deckcraft_geom::Xfrm::new(40.0, 40.0, 850.0, 430.0)),
                kind: deckcraft_model::ShapeKind::Chart(Box::new(c)),
                ..Default::default()
            }];
            std::fs::write(dir.join("synced-chart.pptx"), crate::export(&p).unwrap()).unwrap();
            std::fs::write(dir.join("synced-chart.xlsx"), &changed.kept.parts[0].bytes).unwrap();
        }
    }
    #[test]
    fn formulas_conflicting_ranges_and_external_sources_fail_closed() {
        assert!(range("[external.xlsx]Sheet1!A1:A3").is_none());
        assert!(range("Sheet1!A1:B3").is_none());
        assert!(range("Sheet1!A1:A1048577").is_none());
        assert_eq!(range("'O''Brien Data'!$AA$2:$AA$3").unwrap().0, "O'Brien Data");
        let c = chart();
        let mut writes = Writes::new();
        writes.insert(("Sheet1".into(), 99, 26), Value::Number(1.0));
        assert!(patch_workbook(&c.kept.parts[0].bytes, writes).is_none());
        assert!(unzip(b"not a workbook").is_none());
        let mut reference_xml = crate::xml::parse(br#"<q:cat><q:strRef><q:f>Sheet1!A2</q:f><q:strCache/></q:strRef></q:cat>"#).unwrap().root;
        let mut conflicting = Writes::new();
        conflicting.insert(("Sheet1".into(), 2, 1), Value::Text("Already claimed".into()));
        assert!(reference(&mut reference_xml, vec![Value::Text("Different".into())], &mut conflicting).is_none());
    }

    #[test]
    fn prefixed_xml_and_missing_cells_keep_the_original_namespace() {
        let mut sheet =
            crate::xml::parse(br#"<s:worksheet xmlns:s="urn:synthetic"><s:sheetData><s:row r="1"><s:c r="A1"/></s:row></s:sheetData></s:worksheet>"#)
                .unwrap()
                .root;
        let values = BTreeMap::from([((1, 1), Value::Text(" x ".into())), ((1, 2), Value::Number(2.0)), ((2, 1), Value::Number(3.0))]);
        patch_cells(&mut sheet, &values).unwrap();
        assert_eq!(cell(&sheet, "A1").child("is").unwrap().name, "s:is");
        assert_eq!(cell(&sheet, "B1").name, "s:c");
        assert_eq!(cell(&sheet, "A2").child("v").unwrap().name, "s:v");
        assert!(sheet.child("sheetData").unwrap().children_named("row").all(|r| r.name == "s:row"));
        let mut r =
            crate::xml::parse(br#"<q:cat xmlns:q="urn:synthetic"><q:strRef><q:f>Sheet1!A2</q:f><q:strCache/></q:strRef></q:cat>"#).unwrap().root;
        reference(&mut r, vec![Value::Text("Replaced".into())], &mut Writes::new()).unwrap();
        let cache = r.child("strRef").unwrap().child("strCache").unwrap();
        assert_eq!(cache.name, "q:strCache");
        assert_eq!(cache.child("pt").unwrap().child("v").unwrap().name, "q:v");
        let mut protected = crate::xml::parse(br#"<worksheet><sheetData/><tableParts/></worksheet>"#).unwrap().root;
        assert!(patch_cells(&mut protected, &values).is_none());
        let mut array =
            crate::xml::parse(br#"<worksheet><sheetData><row r="1"><c r="Z1"><f t="array" ref="Z1:Z5">TEST()</f></c></row></sheetData></worksheet>"#)
                .unwrap()
                .root;
        assert!(patch_cells(&mut array, &values).is_none());
    }
}
