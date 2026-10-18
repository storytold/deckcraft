//! Updating cells of an existing workbook (`.xlsx`) in place, for a chart whose data changed: the
//! workbook PowerPoint opens for Edit Data keeps its other sheets, formulas and formatting.
//!
//! Only the parts that change are rewritten, and within them only the cells that change (plus the
//! name of a table column whose header cell changes); every other zip entry is copied with its
//! compressed bytes. Anything this can't do safely returns `None`, and the caller writes a
//! workbook of its own instead.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Write};

use quick_xml::events::{BytesStart, Event};

use crate::opc::{self, Package};
use crate::xml::{esc, esc_attr};

/// A cell's new content.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Blank,
}

/// Cell `index` (0-based) of the range a chart formula names (`Sheet1!$B$2:$B$5`) gets `value`.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub formula: String,
    pub index: usize,
    pub value: Value,
}

const MAX_COL: u32 = 16_384;
const MAX_ROW: u32 = 1_048_576;

/// Children of `workbook` that follow `calcPr` (ECMA-376 Part 1, §18.2.27).
const AFTER_CALC_PR: [&str; 9] = [
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

/// A one-column or one-row range of one sheet, cells as (column, row), 1-based.
#[derive(Debug, PartialEq)]
struct Range {
    sheet: String,
    from: (u32, u32),
    to: (u32, u32),
}

impl Range {
    /// The `i`-th cell, in reading order.
    fn cell(&self, i: usize) -> Option<(u32, u32)> {
        let i = u32::try_from(i).ok()?;
        let ((c1, r1), (c2, r2)) = (self.from, self.to);
        if c1 == c2 {
            let r = r1.checked_add(i)?;
            (r <= r2).then_some((c1, r))
        } else if r1 == r2 {
            let c = c1.checked_add(i)?;
            (c <= c2).then_some((c, r1))
        } else {
            None
        }
    }
}

/// `$B$12` → (2, 12).
fn cell_ref(s: &str) -> Option<(u32, u32)> {
    let s = s.strip_prefix('$').unwrap_or(s);
    let letters = s.bytes().take_while(u8::is_ascii_alphabetic).count();
    if !(1..=3).contains(&letters) {
        return None;
    }
    let digits = s.get(letters..)?;
    let digits = digits.strip_prefix('$').unwrap_or(digits);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let row: u32 = digits.parse().ok()?;
    let col = s.get(..letters)?.bytes().fold(0u32, |n, b| n * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1));
    ((1..=MAX_COL).contains(&col) && (1..=MAX_ROW).contains(&row)).then_some((col, row))
}

/// Column letters of a 1-based column (1 → A, 27 → AA).
fn col_name(mut n: u32) -> String {
    let mut s = String::new();
    while n > 0 && s.len() < 4 {
        let r = (n - 1) % 26;
        s.insert(0, char::from(b'A' + r as u8));
        n = (n - 1) / 26;
    }
    s
}

/// A chart formula naming one range of one sheet: `Sheet1!$B$2:$B$5`, `'Q1 data'!$A$1`.
/// Unions, names, functions and references to other workbooks are refused.
fn parse_range(f: &str) -> Option<Range> {
    let f = f.trim();
    let f = f.strip_prefix('=').unwrap_or(f);
    let (sheet, cells) = match f.strip_prefix('\'') {
        Some(rest) => {
            // A quoted name; `''` stands for one quote.
            let mut name = String::new();
            let mut it = rest.char_indices().peekable();
            let mut end = None;
            while let Some((i, ch)) = it.next() {
                if ch != '\'' {
                    name.push(ch);
                } else if it.peek().is_some_and(|(_, n)| *n == '\'') {
                    name.push('\'');
                    it.next();
                } else {
                    end = Some(i + 1);
                    break;
                }
            }
            (name, rest.get(end?..)?.strip_prefix('!')?)
        }
        None => {
            let (s, c) = f.split_once('!')?;
            (s.to_string(), c)
        }
    };
    if sheet.is_empty() || sheet.contains(['[', ']']) || cells.contains(['!', ',', '(', ')', ' ']) {
        return None;
    }
    let (a, b) = cells.split_once(':').unwrap_or((cells, cells));
    let ((c1, r1), (c2, r2)) = (cell_ref(a)?, cell_ref(b)?);
    let r = Range { sheet, from: (c1.min(c2), r1.min(r2)), to: (c1.max(c2), r1.max(r2)) };
    (r.from.0 == r.to.0 || r.from.1 == r.to.1).then_some(r)
}

fn fmt_num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

fn pos(r: &quick_xml::Reader<&[u8]>) -> usize {
    usize::try_from(r.buffer_position()).unwrap_or(usize::MAX)
}

/// Was the tag ending at `end` an empty one (`<row r="3"/>`)?
fn ends_empty(xml: &[u8], end: usize) -> bool {
    xml.get(end.saturating_sub(2)..end).is_some_and(|s| s == b"/>")
}

fn local_is(e: &BytesStart, local: &str) -> bool {
    e.local_name().as_ref() == local.as_bytes()
}

/// An attribute's value, unescaped, by its name as written.
fn attr(e: &BytesStart, key: &str) -> Option<String> {
    let a = e.attributes().with_checks(false).flatten().find(|a| a.key.as_ref() == key.as_bytes())?;
    let raw = String::from_utf8_lossy(&a.value).into_owned();
    Some(quick_xml::escape::unescape(&raw).map(|v| v.into_owned()).unwrap_or(raw))
}

/// The prefix of an element name with its colon (`x:`), or nothing.
fn prefix_of(e: &BytesStart) -> String {
    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    name.rsplit_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default()
}

/// A start tag written again: attributes in `set` take their new (escaped) value, or go when it
/// is `None`; the others keep their original text.
fn retag(e: &BytesStart, set: &[(&str, Option<String>)], empty: bool) -> String {
    let mut s = format!("<{}", String::from_utf8_lossy(e.name().as_ref()));
    let mut done: HashSet<&str> = HashSet::new();
    for a in e.attributes().with_checks(false).flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        match set.iter().find(|(k, _)| *k == key) {
            Some((k, v)) => {
                done.insert(k);
                if let Some(v) = v {
                    s.push_str(&format!(" {key}=\"{v}\""));
                }
            }
            None => s.push_str(&format!(" {key}=\"{}\"", String::from_utf8_lossy(&a.value).replace('"', "&quot;"))),
        }
    }
    for (k, v) in set {
        if let (false, Some(v)) = (done.contains(k), v) {
            s.push_str(&format!(" {k}=\"{v}\""));
        }
    }
    s.push_str(if empty { "/>" } else { ">" });
    s
}

/// A cell element with its new content. `orig` is the cell it replaces, whose other attributes
/// (style first) it keeps.
fn cell_xml(p: &str, at: (u32, u32), orig: Option<&BytesStart>, v: &Value) -> String {
    let t = match v {
        Value::Text(_) => Some("inlineStr".to_string()),
        Value::Number(_) | Value::Blank => None,
    };
    let empty = *v == Value::Blank;
    let mut s = match orig {
        // A new value drops the old type and its value metadata.
        Some(e) => retag(e, &[("t", t), ("cm", None), ("vm", None)], empty),
        None => {
            let r = format!("{}{}", col_name(at.0), at.1);
            let t = t.map(|t| format!(" t=\"{t}\"")).unwrap_or_default();
            format!("<{p}c r=\"{r}\"{t}{}", if empty { "/>" } else { ">" })
        }
    };
    match v {
        Value::Number(n) => s.push_str(&format!("<{p}v>{}</{p}v></{p}c>", fmt_num(*n))),
        Value::Text(t) => s.push_str(&format!("<{p}is><{p}t xml:space=\"preserve\">{}</{p}t></{p}is></{p}c>", esc(t))),
        Value::Blank => {}
    }
    s
}

fn row_xml(p: &str, row: u32, cells: &[((u32, u32), &Value)]) -> String {
    let mut s = format!("<{p}row r=\"{row}\">");
    for ((r, c), v) in cells {
        s.push_str(&cell_xml(p, (*c, *r), None, v));
    }
    s.push_str(&format!("</{p}row>"));
    s
}

/// The pending cells before (`row`, `col`), taken out of `pending`.
fn take_before(pending: &mut BTreeMap<(u32, u32), &Value>, row: u32, col: u32) -> Vec<((u32, u32), Value)> {
    let keys: Vec<(u32, u32)> = pending.range((row, 0)..(row, col)).map(|(k, _)| *k).collect();
    keys.into_iter().filter_map(|k| pending.remove(&k).map(|v| (k, v.clone()))).collect()
}

/// The pending rows before `row` (all of them for `None`), as XML.
fn rows_before(p: &str, pending: &mut BTreeMap<(u32, u32), &Value>, row: Option<u32>) -> String {
    let keys: Vec<(u32, u32)> = pending.keys().copied().filter(|(r, _)| row.is_none_or(|row| *r < row)).collect();
    let mut s = String::new();
    let mut i = 0;
    while let Some(&(r, _)) = keys.get(i) {
        let mut cells = vec![];
        while let Some(&k) = keys.get(i).filter(|k| k.0 == r) {
            if let Some(v) = pending.remove(&k) {
                cells.push((k, v));
            }
            i += 1;
        }
        s.push_str(&row_xml(p, r, &cells));
    }
    s
}

fn cells_xml(p: &str, cells: &[((u32, u32), Value)]) -> String {
    cells.iter().map(|((r, c), v)| cell_xml(p, (*c, *r), None, v)).collect()
}

/// A start tag written as an empty one (`<row r="5"/>`) opened instead (`<row r="5">`).
fn opened(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let t = s.trim_end();
    t.strip_suffix("/>").map(|t| format!("{t}>")).unwrap_or_else(|| t.to_string())
}

/// A worksheet with `cells` (keyed by (row, column)) written. Returns the new XML and whether a
/// formula was replaced; `None` when a cell can't be placed safely.
fn patch_sheet(xml: &[u8], cells: &BTreeMap<(u32, u32), &Value>) -> Option<(Vec<u8>, bool)> {
    let mut r = quick_xml::Reader::from_reader(xml);
    r.config_mut().trim_text(false);
    let mut out: Vec<u8> = Vec::with_capacity(xml.len() + 256);
    let mut copied = 0usize;
    let mut pending = cells.clone();
    let mut p = String::new();
    let mut in_data = false;
    let mut row: Option<u32> = None;
    let mut dropped = false;
    // Copy the original up to `at`, then `insert`.
    let emit = |out: &mut Vec<u8>, copied: &mut usize, at: usize, insert: &str| -> Option<()> {
        out.extend_from_slice(xml.get(*copied..at)?);
        out.extend_from_slice(insert.as_bytes());
        *copied = at;
        Some(())
    };
    loop {
        let start = pos(&r);
        let ev = r.read_event().ok()?;
        let end = pos(&r);
        match ev {
            Event::Eof => break,
            Event::Start(e) if !in_data && local_is(&e, "sheetData") => {
                in_data = true;
                p = prefix_of(&e);
            }
            Event::Empty(e) if !in_data && local_is(&e, "sheetData") => {
                p = prefix_of(&e);
                let rows = rows_before(&p, &mut pending, None);
                let tag = opened(xml.get(start..end)?);
                emit(&mut out, &mut copied, start, &format!("{tag}{rows}</{p}sheetData>"))?;
                copied = end;
            }
            Event::End(e) if in_data && row.is_none() && e.local_name().as_ref() == b"sheetData" => {
                let rows = rows_before(&p, &mut pending, None);
                emit(&mut out, &mut copied, start, &rows)?;
                in_data = false;
            }
            Event::Start(e) | Event::Empty(e) if in_data && row.is_none() && local_is(&e, "row") => {
                let n: u32 = attr(&e, "r")?.trim().parse().ok()?;
                let rows = rows_before(&p, &mut pending, Some(n));
                emit(&mut out, &mut copied, start, &rows)?;
                if ends_empty(xml, end) {
                    let cells = take_before(&mut pending, n, u32::MAX);
                    if !cells.is_empty() {
                        let tag = opened(xml.get(start..end)?);
                        emit(&mut out, &mut copied, start, &format!("{tag}{}</{p}row>", cells_xml(&p, &cells)))?;
                        copied = end;
                    }
                } else {
                    row = Some(n);
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"row" => {
                if let Some(n) = row.take() {
                    let cells = take_before(&mut pending, n, u32::MAX);
                    emit(&mut out, &mut copied, start, &cells_xml(&p, &cells))?;
                }
            }
            ev @ (Event::Start(_) | Event::Empty(_)) if row.is_some() => {
                let (Event::Start(e) | Event::Empty(e)) = &ev else { continue };
                if !local_is(e, "c") {
                    continue;
                }
                let n = row?;
                let (col, at_row) = cell_ref(attr(e, "r")?.trim())?;
                if at_row != n {
                    return None;
                }
                let before = take_before(&mut pending, n, col);
                emit(&mut out, &mut copied, start, &cells_xml(&p, &before))?;
                let mut stop = end;
                if matches!(ev, Event::Start(_)) {
                    // Read to the end of the cell, watching for formulas.
                    let mut depth = 0usize;
                    loop {
                        match r.read_event().ok()? {
                            Event::Start(c) => {
                                if local_is(&c, "f") {
                                    dropped |= pending.contains_key(&(n, col));
                                    if attr(&c, "ref").is_some() && pending.contains_key(&(n, col)) {
                                        return None;
                                    }
                                }
                                depth += 1;
                            }
                            Event::Empty(c) if local_is(&c, "f") && pending.contains_key(&(n, col)) => {
                                if attr(&c, "ref").is_some() {
                                    return None;
                                }
                                dropped = true;
                            }
                            Event::End(_) if depth == 0 => break,
                            Event::End(_) => depth -= 1,
                            Event::Eof => return None,
                            _ => {}
                        }
                    }
                    stop = pos(&r);
                }
                if let Some(v) = pending.remove(&(n, col)) {
                    emit(&mut out, &mut copied, start, &cell_xml(&p, (col, n), Some(e), v))?;
                    copied = stop;
                }
            }
            _ => {}
        }
    }
    if !pending.is_empty() {
        return None;
    }
    out.extend_from_slice(xml.get(copied..)?);
    Some((out, dropped))
}

/// Does this worksheet hold a formula?
fn has_formula(xml: &[u8]) -> bool {
    let mut r = quick_xml::Reader::from_reader(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if local_is(&e, "f") => return true,
            Ok(Event::Eof) | Err(_) => return false,
            _ => {}
        }
    }
}

/// `workbook.xml` with `fullCalcOnLoad="1"` on `calcPr`, which is added when missing.
fn full_calc_on_load(xml: &[u8]) -> Option<Vec<u8>> {
    let mut r = quick_xml::Reader::from_reader(xml);
    let mut depth = 0usize;
    let mut p = String::new();
    let mut at: Option<(usize, usize, String)> = None;
    loop {
        let start = pos(&r);
        let ev = r.read_event().ok()?;
        let end = pos(&r);
        match ev {
            Event::Start(e) | Event::Empty(e) if depth == 1 && local_is(&e, "calcPr") => {
                let empty = ends_empty(xml, end);
                at = Some((start, end, retag(&e, &[("fullCalcOnLoad", Some("1".into()))], empty)));
                break;
            }
            Event::Start(e) | Event::Empty(e) if depth == 1 && AFTER_CALC_PR.iter().any(|n| local_is(&e, n)) => {
                at = Some((start, start, format!("<{p}calcPr fullCalcOnLoad=\"1\"/>")));
                break;
            }
            Event::Start(e) => {
                if depth == 0 {
                    p = prefix_of(&e);
                }
                depth += 1;
            }
            Event::End(_) if depth == 1 => {
                at = Some((start, start, format!("<{p}calcPr fullCalcOnLoad=\"1\"/>")));
                break;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    let (from, to, tag) = at?;
    let mut out = xml.get(..from)?.to_vec();
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(xml.get(to..)?);
    Some(out)
}

/// The XML without the elements `drop` picks (with their content).
fn remove_elements(xml: &[u8], drop: impl Fn(&BytesStart) -> bool) -> Option<Vec<u8>> {
    let mut r = quick_xml::Reader::from_reader(xml);
    let mut out = Vec::with_capacity(xml.len());
    let mut copied = 0usize;
    loop {
        let start = pos(&r);
        match r.read_event().ok()? {
            Event::Eof => break,
            Event::Empty(e) if drop(&e) => {
                out.extend_from_slice(xml.get(copied..start)?);
                copied = pos(&r);
            }
            Event::Start(e) if drop(&e) => {
                let name = e.name().as_ref().to_vec();
                r.read_to_end(quick_xml::name::QName(&name)).ok()?;
                out.extend_from_slice(xml.get(copied..start)?);
                copied = pos(&r);
            }
            _ => {}
        }
    }
    out.extend_from_slice(xml.get(copied..)?);
    Some(out)
}

/// `tableColumn` elements renamed: index in the table → new name.
fn rename_columns(xml: &[u8], names: &BTreeMap<usize, String>) -> Option<Vec<u8>> {
    let mut r = quick_xml::Reader::from_reader(xml);
    let mut out = Vec::with_capacity(xml.len() + 64);
    let mut copied = 0usize;
    let mut i = 0usize;
    loop {
        let start = pos(&r);
        let ev = r.read_event().ok()?;
        let end = pos(&r);
        match ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if local_is(&e, "tableColumn") => {
                if let Some(name) = names.get(&i) {
                    let empty = ends_empty(xml, end);
                    out.extend_from_slice(xml.get(copied..start)?);
                    out.extend_from_slice(retag(&e, &[("name", Some(esc_attr(name)))], empty).as_bytes());
                    copied = end;
                }
                i += 1;
            }
            _ => {}
        }
    }
    out.extend_from_slice(xml.get(copied..)?);
    Some(out)
}

/// Table header cells among the edits of one sheet: tables to rename columns in (part → column
/// index → new name). `None` when a header can't take its new value: Excel wants unique, non-empty
/// text names, and formulas that name the old column would break.
fn table_renames(pkg: &Package, sheet: &str, cells: &BTreeMap<(u32, u32), &Value>) -> Option<Vec<(String, BTreeMap<usize, String>)>> {
    let mut out = vec![];
    for rel in pkg.rels(sheet).all_of("table") {
        let Some(doc) = pkg.xml(&rel.target) else { continue };
        let t = &doc.root;
        let Some(range) = t.attr("ref").and_then(parse_area) else { continue };
        if t.attr("headerRowCount").is_some_and(|n| n.trim() == "0") {
            continue;
        }
        let columns: Vec<String> = t
            .child("tableColumns")
            .map(|c| c.children_named("tableColumn").map(|c| c.attr("name").unwrap_or("").to_string()).collect())
            .unwrap_or_default();
        let mut names = columns.clone();
        let mut renames = BTreeMap::new();
        for (&(_, col), v) in cells.range((range.0.1, range.0.0)..=(range.0.1, range.1.0)) {
            let i = usize::try_from(col.checked_sub(range.0.0)?).ok()?;
            let Value::Text(new) = v else { return None };
            if new.trim().is_empty() {
                return None;
            }
            let old = columns.get(i)?;
            if old != new {
                // Structured references (`Table1[Series 1]`, `[@[Series 1]]`) name the column.
                let used = |s: &str| [format!("[{}]", esc(old)), format!("[@{}]", esc(old))].iter().any(|n| s.contains(n.as_str()));
                if xl_xml(pkg).any(|x| used(&x)) {
                    return None;
                }
            }
            *names.get_mut(i)? = new.clone();
            renames.insert(i, new.clone());
        }
        if renames.is_empty() {
            continue;
        }
        let mut seen = HashSet::new();
        if !names.iter().all(|n| seen.insert(n.to_lowercase())) {
            return None;
        }
        out.push((rel.target.clone(), renames));
    }
    Some(out)
}

/// `A1:D5` → ((1, 1), (4, 5)).
fn parse_area(r: &str) -> Option<((u32, u32), (u32, u32))> {
    let (a, b) = r.trim().split_once(':').unwrap_or((r.trim(), r.trim()));
    let ((c1, r1), (c2, r2)) = (cell_ref(a)?, cell_ref(b)?);
    Some(((c1.min(c2), r1.min(r2)), (c1.max(c2), r1.max(r2))))
}

/// The XML parts of the workbook (`xl/…`), as text, shared strings left out.
fn xl_xml(pkg: &Package) -> impl Iterator<Item = String> + '_ {
    pkg.names()
        .filter(|n| n.starts_with("xl/") && n.ends_with(".xml") && !n.ends_with("sharedstrings.xml"))
        .filter_map(|n| pkg.get(n).map(|b| String::from_utf8_lossy(b).into_owned()))
}

/// The workbook `xlsx` with `edits` applied, every other cell and part as it was. Formulas
/// elsewhere are recalculated when the workbook opens. `None` when an edit can't be placed safely
/// (a range that isn't one row or column of one sheet, a cell holding a shared or array formula,
/// a table header that can't take the new name, an unreadable part…).
pub fn patch(xlsx: &[u8], edits: &[Edit]) -> Option<Vec<u8>> {
    let pkg = Package::open(xlsx).ok()?;
    let book = pkg.rels("").first_of("officeDocument")?.target.clone();
    let book_rels = pkg.rels(&book);
    let book_doc = pkg.xml(&book)?;
    let mut sheets: HashMap<String, String> = HashMap::new();
    for s in book_doc.root.child("sheets")?.children_named("sheet") {
        if let (Some(name), Some(target)) = (s.attr("name"), s.attr("r:id").and_then(|id| book_rels.target(id))) {
            sheets.insert(name.to_lowercase(), target.to_string());
        }
    }
    let mut by_sheet: BTreeMap<String, BTreeMap<(u32, u32), &Value>> = BTreeMap::new();
    for e in edits {
        let range = parse_range(&e.formula)?;
        let part = sheets.get(&range.sheet.to_lowercase())?;
        let (col, row) = range.cell(e.index)?;
        by_sheet.entry(part.clone()).or_default().insert((row, col), &e.value);
    }

    let mut changed: HashMap<String, Vec<u8>> = HashMap::new();
    let mut removed: HashSet<String> = HashSet::new();
    let mut dropped_formula = false;
    for (part, cells) in &by_sheet {
        for (table, names) in table_renames(&pkg, part, cells)? {
            changed.insert(opc::norm(&table), rename_columns(pkg.get(&table)?, &names)?);
        }
        let (xml, dropped) = patch_sheet(pkg.get(part)?, cells)?;
        dropped_formula |= dropped;
        changed.insert(opc::norm(part), xml);
    }
    if changed.is_empty() {
        return Some(xlsx.to_vec());
    }
    // The calculation chain lists formula cells; one that lost its formula would make Excel
    // repair the file. It is optional: Excel rebuilds it.
    if dropped_formula && let Some(chain) = book_rels.first_of("calcChain") {
        let target = chain.target.clone();
        let id = chain.id.clone();
        let rels = opc::rels_path(&book);
        changed.insert(
            opc::norm(&rels),
            remove_elements(pkg.get(&rels)?, |e| local_is(e, "Relationship") && attr(e, "Id").as_deref() == Some(id.as_str()))?,
        );
        let ct = "[Content_Types].xml";
        let part = format!("/{}", opc::norm(&target));
        changed.insert(
            opc::norm(ct),
            remove_elements(pkg.get(ct)?, |e| local_is(e, "Override") && attr(e, "PartName").is_some_and(|n| n.to_ascii_lowercase() == part))?,
        );
        removed.insert(opc::norm(&target));
    }
    let formulas = book_rels.all_of("worksheet").any(|s| {
        let n = opc::norm(&s.target);
        changed.get(&n).map(|x| has_formula(x)).unwrap_or_else(|| pkg.get(&s.target).is_some_and(has_formula))
    });
    if formulas {
        changed.insert(opc::norm(&book), full_calc_on_load(pkg.get(&book)?)?);
    }
    rezip(xlsx, &changed, &removed)
}

/// The zip again: entries in `changed` get new content, those in `removed` go, the others are
/// copied as they were (compressed bytes, dates).
fn rezip(xlsx: &[u8], changed: &HashMap<String, Vec<u8>>, removed: &HashSet<String>) -> Option<Vec<u8>> {
    let mut zin = zip::ZipArchive::new(Cursor::new(xlsx)).ok()?;
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::with_capacity(xlsx.len() + 1024)));
    let mut written = HashSet::new();
    for i in 0..zin.len() {
        let f = zin.by_index_raw(i).ok()?;
        let name = opc::norm(f.name());
        if removed.contains(&name) {
            continue;
        }
        match changed.get(&name) {
            Some(data) => {
                let mut opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
                if let Some(t) = f.last_modified() {
                    opts = opts.last_modified_time(t);
                }
                let entry = f.name().to_string();
                drop(f);
                zw.start_file(entry, opts).ok()?;
                zw.write_all(data).ok()?;
                written.insert(name);
            }
            None => zw.raw_copy_file(f).ok()?,
        }
    }
    if changed.keys().any(|k| !written.contains(k)) {
        return None;
    }
    Some(zw.finish().ok()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let r = parse_range("Sheet1!$B$2:$B$5").unwrap();
        assert_eq!((r.sheet.as_str(), r.from, r.to), ("Sheet1", (2, 2), (2, 5)));
        assert_eq!(r.cell(3), Some((2, 5)));
        assert_eq!(r.cell(4), None);
        let r = parse_range("'Q1 ''data'''!C1:A1").unwrap();
        assert_eq!((r.sheet.as_str(), r.from, r.to), ("Q1 'data'", (1, 1), (3, 1)));
        assert_eq!(r.cell(2), Some((3, 1)));
        assert_eq!(parse_range("=Sheet1!$A$1").unwrap().cell(0), Some((1, 1)));
        for bad in ["Sheet1!$A$1:$B$2", "[1]Sheet1!$A$1", "(Sheet1!$A$1,Sheet1!$A$3)", "Sheet1!Name", "A1", "Sheet1!$XFE$1", "Sheet1!A0"] {
            assert!(parse_range(bad).is_none(), "{bad}");
        }
        assert_eq!(col_name(1), "A");
        assert_eq!(col_name(28), "AB");
        assert_eq!(cell_ref("$AB$10"), Some((28, 10)));
    }

    fn cells<'a>(list: &[((u32, u32), &'a Value)]) -> BTreeMap<(u32, u32), &'a Value> {
        list.iter().copied().collect()
    }

    #[test]
    fn sheet_cells_are_replaced_and_inserted_in_order() {
        let xml = br#"<?xml version="1.0"?><worksheet xmlns="m"><sheetData><row r="1" spans="1:3"><c r="A1" t="s"><v>0</v></c><c r="C1" s="2"><v>7</v></c></row><row r="3"/><row r="5"><c r="B5"><f>SUM(B1:B4)</f><v>3</v></c></row></sheetData><mergeCells count="0"/></worksheet>"#;
        let (n, t, b) = (Value::Number(2.5), Value::Text("Q <1>".into()), Value::Blank);
        let (out, dropped) = patch_sheet(xml, &cells(&[((1, 2), &n), ((1, 3), &b), ((2, 1), &t), ((3, 2), &n), ((5, 2), &n), ((6, 1), &n)])).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(dropped);
        assert_eq!(
            out,
            concat!(
                r#"<?xml version="1.0"?><worksheet xmlns="m"><sheetData><row r="1" spans="1:3"><c r="A1" t="s"><v>0</v></c>"#,
                r#"<c r="B1"><v>2.5</v></c><c r="C1" s="2"/></row>"#,
                r#"<row r="2"><c r="A2" t="inlineStr"><is><t xml:space="preserve">Q &lt;1&gt;</t></is></c></row>"#,
                r#"<row r="3"><c r="B3"><v>2.5</v></c></row><row r="5"><c r="B5"><v>2.5</v></c></row>"#,
                r#"<row r="6"><c r="A6"><v>2.5</v></c></row></sheetData><mergeCells count="0"/></worksheet>"#
            )
        );
        // Prefixed elements and an empty sheet.
        let xml = br#"<x:worksheet xmlns:x="m"><x:sheetData/></x:worksheet>"#;
        let (out, dropped) = patch_sheet(xml, &cells(&[((2, 1), &n)])).unwrap();
        assert!(!dropped);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#"<x:worksheet xmlns:x="m"><x:sheetData><x:row r="2"><x:c r="A2"><x:v>2.5</x:v></x:c></x:row></x:sheetData></x:worksheet>"#
        );
    }

    #[test]
    fn unsafe_cells_are_refused() {
        let n = Value::Number(1.0);
        // A shared formula's anchor: other cells use it.
        let shared =
            br#"<worksheet><sheetData><row r="2"><c r="B2"><f t="shared" ref="B2:B4" si="0">A2*2</f><v>2</v></c></row></sheetData></worksheet>"#;
        assert!(patch_sheet(shared, &cells(&[((2, 2), &n)])).is_none());
        // Rows or cells without their position.
        let implicit = br#"<worksheet><sheetData><row><c><v>1</v></c></row></sheetData></worksheet>"#;
        assert!(patch_sheet(implicit, &cells(&[((1, 1), &n)])).is_none());
        // No sheet data to put the cell in.
        assert!(patch_sheet(b"<worksheet/>", &cells(&[((1, 1), &n)])).is_none());
    }

    #[test]
    fn calc_pr_is_set_or_added() {
        let set = full_calc_on_load(br#"<workbook><sheets/><calcPr calcId="191029"/></workbook>"#).unwrap();
        assert_eq!(String::from_utf8(set).unwrap(), r#"<workbook><sheets/><calcPr calcId="191029" fullCalcOnLoad="1"/></workbook>"#);
        let added = full_calc_on_load(br#"<x:workbook><x:sheets><x:sheet/></x:sheets><x:extLst/></x:workbook>"#).unwrap();
        assert_eq!(
            String::from_utf8(added).unwrap(),
            r#"<x:workbook><x:sheets><x:sheet/></x:sheets><x:calcPr fullCalcOnLoad="1"/><x:extLst/></x:workbook>"#
        );
        let last = full_calc_on_load(br#"<workbook><sheets></sheets></workbook>"#).unwrap();
        assert_eq!(String::from_utf8(last).unwrap(), r#"<workbook><sheets></sheets><calcPr fullCalcOnLoad="1"/></workbook>"#);
    }

    #[test]
    fn elements_are_removed_and_columns_renamed() {
        let rels = br#"<Relationships><Relationship Id="rId1" Target="a"/><Relationship Id="rId2" Target="b"/></Relationships>"#;
        let out = remove_elements(rels, |e| attr(e, "Id").as_deref() == Some("rId2")).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), r#"<Relationships><Relationship Id="rId1" Target="a"/></Relationships>"#);
        let table = br#"<table ref="A1:C3"><tableColumns count="3"><tableColumn id="1" name=" "/><tableColumn id="2" name="S1"/><tableColumn id="3" name="S2"></tableColumn></tableColumns></table>"#;
        let names = BTreeMap::from([(1, "Sales & co".to_string()), (2, "Costs".to_string())]);
        let out = String::from_utf8(rename_columns(table, &names).unwrap()).unwrap();
        assert!(out.contains(r#"<tableColumn id="2" name="Sales &amp; co"/><tableColumn id="3" name="Costs"></tableColumn>"#), "{out}");
    }
}
