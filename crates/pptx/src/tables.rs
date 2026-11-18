//! Table style XML uses the shared writer; app XML admission remains unchanged.
pub use craft_drawingml::table::styles::{guid, guid_for_style, is_own_guid, style_from_guid};

pub fn table_styles_xml(used: &[String], defs: &[deckcraft_model::table::TableStyleDef]) -> Vec<u8> {
    craft_drawingml_table::table::styles::table_styles_xml_with(used, defs.iter().map(|d| (d.id.as_str(), d.raw.as_str())), "DeckCraft", |raw| {
        crate::xml::parse(raw.as_bytes()).is_ok_and(|x| x.root.is("tblStyle"))
    })
}

#[cfg(test)]
#[path = "tables_legacy.rs"]
mod legacy;

#[cfg(test)]
fn own_guid(style: &str) -> String {
    guid(&format!("deckcraft-table-style:{style}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_writer_matches_legacy_bytes_and_admission() {
        use deckcraft_model::table::TableStyleDef;
        let mut used: Vec<String> = deckcraft_model::table::table_styles().into_iter().map(|(id, _)| id).collect();
        used.extend(["unknown<&\"".into(), "Custom".into(), "Custom".into(), "CUSTOM".into()]);
        for raw in [
            "<a:tblStyle xmlns:a=\"urn:test\"/>",
            "<tblStyle>", // Existing lenient partial-root policy.
            "<tblStyle></other><trailing/>",
            "<!--prefix--><tblStyle name=\"&amp;\"/>",
            "<notAStyle/>",
            "",
            "<tblStyle><child/></tblStyle>",
        ] {
            let defs = vec![
                TableStyleDef { id: "custom".into(), raw: raw.into(), ..Default::default() },
                TableStyleDef { id: "CUSTOM".into(), raw: "<tblStyle second=\"1\"/>".into(), ..Default::default() },
                TableStyleDef { id: "unused".into(), raw: "<tblStyle unused=\"1\"/>".into(), ..Default::default() },
            ];
            for selection in [&used[..], &[][..]] {
                assert_eq!(table_styles_xml(selection, &defs), legacy::table_styles_xml(selection, &defs), "{raw:?}");
            }
        }
        for depth in [255, 256, 257] {
            let raw = format!("{}{}", "<tblStyle>".repeat(depth), "</tblStyle>".repeat(depth));
            let defs = [TableStyleDef { id: "custom".into(), raw, ..Default::default() }];
            assert_eq!(table_styles_xml(&used, &defs), legacy::table_styles_xml(&used, &defs));
        }
    }

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
