//! Table styles: our style ids ↔ table style GUIDs, and `tableStyles.xml` definitions for the
//! styles we write (generated from theme accents, matching how DeckCraft draws them).

use crate::opc::NS_A;
use crate::xml::{A, W};

// Keep file-format identifiers in the shared DrawingML implementation. The local
// writer below still owns DeckCraft's table style definitions and XML policy.
pub use craft_drawingml::table::styles::{guid, guid_for_style, is_own_guid, style_from_guid};

fn own_guid(style: &str) -> String {
    guid(&format!("deckcraft-table-style:{style}"))
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
    fn shared_style_catalog_matches_deckcraft() {
        assert_eq!(deckcraft_model::table::table_styles(), craft_drawingml::table::table_styles());
        assert_eq!(style_from_guid(""), "none");
        let custom = "{abcdef00-0000-0000-0000-000000000000}";
        assert_eq!(style_from_guid(custom), custom);
        assert_eq!(guid_for_style(custom), (custom.to_string(), false));
        assert_eq!(guid_for_style("unknown"), guid_for_style("medium2-accent1"));
        for (id, _) in deckcraft_model::table::table_styles() {
            let generated = own_guid(&id);
            assert!(is_own_guid(&generated.to_ascii_lowercase()));
            if guid_for_style(&id).1 {
                assert_eq!(guid_for_style(&id).0, generated);
            }
        }
    }

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
