//! DrawingML paragraph defaults apply to every list level, before level-specific overrides.

use std::io::{Cursor, Read, Write};

#[test]
fn default_paragraph_language_and_format_inherit_into_each_level() {
    let bytes = deckcraft_pptx::export(&deckcraft_model::defaults::new_presentation(None)).expect("export");
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(vec![]));
    for i in 0..source.len() {
        let mut file = source.by_index(i).expect("part");
        let mut bytes = vec![];
        file.read_to_end(&mut bytes).expect("read");
        if file.name() == "ppt/presentation.xml" {
            let xml = String::from_utf8(bytes).expect("xml");
            let start = xml.find("<p:defaultTextStyle").expect("default style");
            let end = xml[start..].find("</p:defaultTextStyle>").expect("end") + start + "</p:defaultTextStyle>".len();
            let style = r#"<p:defaultTextStyle>
              <a:defPPr marL="127000"><a:defRPr lang="en-US" sz="2200" b="1"/></a:defPPr>
              <a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr>
              <a:lvl2pPr marL="254000"><a:defRPr lang="ja-JP" b="0"/></a:lvl2pPr>
            </p:defaultTextStyle>"#;
            bytes = format!("{}{style}{}", &xml[..start], &xml[end..]).into_bytes();
        }
        out.start_file(file.name(), zip::write::SimpleFileOptions::default()).expect("part");
        out.write_all(&bytes).expect("write");
    }
    let p = deckcraft_pptx::import(&out.finish().expect("zip").into_inner()).expect("import");
    let q = deckcraft_pptx::import(&deckcraft_pptx::export(&p).expect("save")).expect("reopen");
    for deck in [&p, &q] {
        for level in 0..9 {
            let s = deck.default_text_style.level(level).expect("level defaults");
            assert_eq!(s.run.lang.as_deref(), Some(if level == 1 { "ja-JP" } else { "en-US" }));
            assert_eq!(s.run.size, Some(if level == 0 { 18.0 } else { 22.0 }));
            assert_eq!(s.run.bold, Some(level != 1));
            assert_eq!(s.para.margin_left, Some(if level == 1 { 20.0 } else { 10.0 }));
        }
    }
}
