use std::io::{Cursor, Read, Write};

use deckcraft_model::Presentation;

/// Rewrite the chart part's XML through `f` and re-import the deck.
pub(super) fn reimport(bytes: &[u8], f: impl Fn(&str) -> String) -> Presentation {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP member name").to_string();
        let mut data = vec![];
        entry.read_to_end(&mut data).expect("read");
        if name.starts_with("ppt/charts/chart") && name.ends_with(".xml") && !name.contains("_rels") {
            let xml = String::from_utf8(data).expect("chart xml");
            data = f(&xml).into_bytes();
        }
        out.start_file(&name, opts).expect("start");
        out.write_all(&data).expect("write");
    }
    let patched = out.finish().expect("finish").into_inner();
    deckcraft_pptx::import(&patched).expect("import")
}
