//! Table styles: our style ids ↔ table style GUIDs, and `tableStyles.xml` definitions for the
//! styles we write (generated from theme accents, matching how DeckCraft draws them).

use crate::opc::NS_A;
use crate::xml::{A, W};

/// Well-known built-in table style GUIDs (format identifiers) and our equivalent ids.
const KNOWN: &[(&str, &str)] = &[
    ("{2D5ABB26-0587-4C30-8999-92F81FD0307C}", "none"),
    ("{5940675A-B579-460E-94D1-54222C63F5DA}", "grid"),
    ("{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}", "medium2-accent1"),
    ("{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}", "medium2-tx1"),
    ("{21E4AEA4-8DFA-4A89-87EB-49C32662AFE8}", "medium2-accent2"),
    ("{F5AB1C69-6EDB-4FF4-983F-18BD219EF322}", "medium2-accent3"),
    ("{00A15C55-8517-42AA-B614-E9B94910E393}", "medium2-accent4"),
    ("{7DF18680-E054-41AD-8BC1-D1AEF772440D}", "medium2-accent5"),
    ("{93296810-A885-4BE3-A3E7-6D5BEEA58F35}", "medium2-accent6"),
];

/// GUIDs we write as built-in references (no definition needed).
const BUILTIN_WRITE: [&str; 3] = ["none", "grid", "medium2-accent1"];

fn fnv(s: &str, seed: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A stable GUID derived from text (version-4 layout).
pub fn guid(text: &str) -> String {
    let a = fnv(text, 0x5163_7261_6674);
    let b = fnv(text, 0x7461_626c_6573);
    format!(
        "{{{:08X}-{:04X}-4{:03X}-8{:03X}-{:012X}}}",
        (a >> 32) as u32,
        (a >> 16) as u16,
        (a & 0x0fff) as u16,
        (b >> 48) as u16 & 0x0fff,
        b & 0xffff_ffff_ffff
    )
}

fn own_guid(style: &str) -> String {
    guid(&format!("deckcraft-table-style:{style}"))
}

/// Is `g` the GUID DeckCraft writes for one of its own styles?
pub fn is_own_guid(g: &str) -> bool {
    let up = g.to_ascii_uppercase();
    deckcraft_model::table::table_styles().iter().any(|(id, _)| own_guid(id) == up)
}

/// Table style GUID → our style id (or the GUID itself when unknown).
pub fn style_from_guid(g: &str) -> String {
    let up = g.to_ascii_uppercase();
    if let Some((_, s)) = KNOWN.iter().find(|(k, _)| *k == up) {
        return s.to_string();
    }
    for (id, _) in deckcraft_model::table::table_styles() {
        if own_guid(&id) == up {
            return id;
        }
    }
    if g.is_empty() { "none".into() } else { g.to_string() }
}

/// GUID to write for our style id, and whether a definition must be written.
pub fn guid_for_style(style: &str) -> (String, bool) {
    if style.starts_with('{') {
        return (style.to_string(), false);
    }
    if BUILTIN_WRITE.contains(&style)
        && let Some((g, _)) = KNOWN.iter().find(|(_, s)| *s == style)
    {
        return (g.to_string(), false);
    }
    let known = deckcraft_model::table::table_styles().iter().any(|(id, _)| id == style);
    if known { (own_guid(style), true) } else { ("{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}".into(), false) }
}

fn clr(w: &mut W, slot: &str, mods: &[(&str, i32)]) {
    if mods.is_empty() {
        w.val("a:schemeClr", slot);
        return;
    }
    w.open("a:schemeClr", A::new().a("val", slot));
    for (n, v) in mods {
        w.val(n, v);
    }
    w.close("a:schemeClr");
}

fn solid(w: &mut W, slot: &str, mods: &[(&str, i32)]) {
    w.open0("a:fill");
    w.open0("a:solidFill");
    clr(w, slot, mods);
    w.close("a:solidFill");
    w.close("a:fill");
}

fn borders(w: &mut W, b: Option<(&str, &[(&str, i32)], i32)>) {
    w.open0("a:tcBdr");
    if let Some((slot, mods, width)) = b {
        for side in ["a:left", "a:right", "a:top", "a:bottom", "a:insideH", "a:insideV"] {
            w.open0(side);
            w.open("a:ln", A::new().a("w", width).a("cmpd", "sng"));
            w.open0("a:solidFill");
            clr(w, slot, mods);
            w.close("a:solidFill");
            w.close("a:ln");
            w.close(side);
        }
    }
    w.close("a:tcBdr");
}

fn txt(w: &mut W, bold: bool, color: Option<&str>) {
    w.open("a:tcTxStyle", A::new().o("b", bold.then_some("on")));
    w.open("a:fontRef", A::new().a("idx", "minor"));
    w.val("a:prstClr", "black");
    w.close("a:fontRef");
    match color {
        Some(c) => w.val("a:schemeClr", c),
        None => w.val("a:schemeClr", "dk1"),
    }
    w.close("a:tcTxStyle");
}

/// Write one `a:tblStyle` for our style id.
fn style_def(w: &mut W, id: &str, name: &str) {
    let kind = id.split('-').next().unwrap_or("medium2");
    let acc = match id.rsplit('-').next().unwrap_or("accent1") {
        "tx1" => "dk1",
        a if a.starts_with("accent") => a,
        _ => "accent1",
    };
    let acc: &str = acc;
    w.open("a:tblStyle", A::new().a("styleId", own_guid(id)).a("styleName", name));
    let part =
        |w: &mut W, tag: &str, bold: bool, text: Option<&str>, fill: Option<(&str, &[(&str, i32)])>, bdr: Option<(&str, &[(&str, i32)], i32)>| {
            w.open0(tag);
            if bold || text.is_some() {
                txt(w, bold, text);
            }
            w.open0("a:tcStyle");
            borders(w, bdr);
            if let Some((s, m)) = fill {
                solid(w, s, m);
            }
            w.close("a:tcStyle");
            w.close(tag);
        };
    match kind {
        "grid" => {
            part(w, "a:wholeTbl", false, Some("dk1"), None, Some(("dk1", &[], 9525)));
        }
        "light1" => {
            part(w, "a:wholeTbl", false, Some("dk1"), None, None);
            part(w, "a:band1H", false, None, Some((acc, &[("a:tint", 20000)])), None);
            part(w, "a:band1V", false, None, Some((acc, &[("a:tint", 20000)])), None);
            part(w, "a:lastCol", true, None, None, None);
            part(w, "a:firstCol", true, None, None, None);
            part(w, "a:lastRow", true, None, None, None);
            part(w, "a:firstRow", true, None, None, Some((acc, &[], 12700)));
        }
        "light2" => {
            part(w, "a:wholeTbl", false, Some("dk1"), None, Some((acc, &[], 12700)));
            // Schema order: … lastCol, firstCol, lastRow, …, firstRow.
            part(w, "a:firstCol", true, None, None, None);
            part(w, "a:lastRow", true, None, None, None);
            part(w, "a:firstRow", true, Some("lt1"), Some((acc, &[])), Some((acc, &[], 12700)));
        }
        "dark1" => {
            part(w, "a:wholeTbl", false, Some("lt1"), Some((acc, &[("a:shade", 75000)])), Some(("lt1", &[], 12700)));
            part(w, "a:band1H", false, None, Some((acc, &[("a:shade", 60000)])), None);
            part(w, "a:band1V", false, None, Some((acc, &[("a:shade", 60000)])), None);
            part(w, "a:firstCol", true, None, None, None);
            part(w, "a:lastRow", true, None, None, None);
            part(w, "a:firstRow", true, Some("lt1"), Some(("dk1", &[])), None);
        }
        "medium4" => {
            part(w, "a:wholeTbl", false, Some("dk1"), Some((acc, &[("a:tint", 20000)])), Some((acc, &[], 9525)));
            part(w, "a:band1H", false, None, Some((acc, &[("a:tint", 40000)])), None);
            part(w, "a:band1V", false, None, Some((acc, &[("a:tint", 40000)])), None);
            part(w, "a:lastCol", true, None, None, None);
            part(w, "a:firstCol", true, None, None, None);
            part(w, "a:lastRow", true, None, None, None);
            part(w, "a:firstRow", true, None, Some((acc, &[("a:tint", 40000)])), None);
        }
        _ => {
            // medium2
            part(w, "a:wholeTbl", false, Some("dk1"), Some((acc, &[("a:tint", 20000)])), Some(("lt1", &[], 12700)));
            part(w, "a:band1H", false, None, Some((acc, &[("a:tint", 40000)])), None);
            part(w, "a:band1V", false, None, Some((acc, &[("a:tint", 40000)])), None);
            part(w, "a:lastCol", true, Some("lt1"), Some((acc, &[])), None);
            part(w, "a:firstCol", true, Some("lt1"), Some((acc, &[])), None);
            part(w, "a:lastRow", true, Some("lt1"), Some((acc, &[])), None);
            part(w, "a:firstRow", true, Some("lt1"), Some((acc, &[])), None);
        }
    }
    w.close("a:tblStyle");
}

/// `ppt/tableStyles.xml` with definitions for the styles in `used`: ours, generated, and those the
/// opened file defined (`defs`), written back as they were read.
pub fn table_styles_xml(used: &[String], defs: &[deckcraft_model::table::TableStyleDef]) -> Vec<u8> {
    let mut w = W::new();
    w.open("a:tblStyleLst", A::new().a("xmlns:a", NS_A).a("def", "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"));
    let names = deckcraft_model::table::table_styles();
    // Indexes instead of scans: written ids (as given and ASCII lower-case), and the file's
    // definitions by lower-case id (the first wins).
    let mut done: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut done_lower: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut def_of: std::collections::HashMap<String, &deckcraft_model::table::TableStyleDef> = std::collections::HashMap::new();
    for d in defs {
        def_of.entry(d.id.to_ascii_lowercase()).or_insert(d);
    }
    for s in used {
        if !done.insert(s.as_str()) {
            continue;
        }
        done_lower.insert(s.to_ascii_lowercase());
        if let Some(d) = def_of.get(&s.to_ascii_lowercase())
            && crate::xml::parse(d.raw.as_bytes()).is_ok_and(|x| x.root.is("tblStyle"))
        {
            w.raw(&d.raw);
            continue;
        }
        let (_, define) = guid_for_style(s);
        if !define {
            continue;
        }
        let name = names.iter().find(|(id, _)| id == s).map(|(_, n)| format!("DeckCraft {n}")).unwrap_or_else(|| s.clone());
        style_def(&mut w, s, &name);
    }
    // Styles the file defined but no table uses still belong to the deck.
    for d in defs {
        if done_lower.contains(&d.id.to_ascii_lowercase()) {
            continue;
        }
        if crate::xml::parse(d.raw.as_bytes()).is_ok_and(|x| x.root.is("tblStyle")) {
            done_lower.insert(d.id.to_ascii_lowercase());
            w.raw(&d.raw);
        }
    }
    w.close("a:tblStyleLst");
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_parts_follow_schema_order() {
        const ORDER: [&str; 14] = [
            "tblBg", "wholeTbl", "band1H", "band2H", "band1V", "band2V", "lastCol", "firstCol", "lastRow", "seCell", "swCell", "firstRow", "neCell",
            "nwCell",
        ];
        let used: Vec<String> = deckcraft_model::table::table_styles().into_iter().map(|(id, _)| id).collect();
        let d = crate::xml::parse(&table_styles_xml(&used, &[])).unwrap();
        let mut n = 0;
        for st in d.root.children_named("tblStyle") {
            n += 1;
            let pos: Vec<usize> = st.elements().map(|e| ORDER.iter().position(|o| *o == e.local()).unwrap()).collect();
            assert!(pos.windows(2).all(|w| w[0] < w[1]), "{:?}: {pos:?}", st.attr("styleName"));
        }
        assert!(n > 30);
    }

    #[test]
    fn guids_round_trip() {
        for (id, _) in deckcraft_model::table::table_styles() {
            let (g, _) = guid_for_style(&id);
            assert_eq!(style_from_guid(&g), id, "{id} via {g}");
            assert_eq!(g.len(), 38);
        }
        assert_eq!(style_from_guid("{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"), "medium2-accent1");
        assert_eq!(style_from_guid("{ABCDEF00-0000-0000-0000-000000000000}"), "{ABCDEF00-0000-0000-0000-000000000000}");
    }
}
