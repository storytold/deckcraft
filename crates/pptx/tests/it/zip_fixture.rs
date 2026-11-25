//! Ordered ZIP helpers for the in-memory PPTX fixtures in this integration binary.

use std::io::{Cursor, Read, Seek, Write};

use deckcraft_model::defaults;

fn payload_first_entry(archive: &mut zip::ZipArchive<impl Read + Seek>, index: usize) -> (String, Vec<u8>) {
    let mut entry = archive.by_index(index).expect("entry");
    let mut data = Vec::new();
    entry.read_to_end(&mut data).expect("read");
    (entry.name().expect("ZIP member name").into_owned(), data)
}

pub(super) fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    (0..archive.len()).map(|index| payload_first_entry(&mut archive, index)).collect()
}

pub(super) fn zip_of(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in files {
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// `bytes` with `edit` applied to every slide part.
pub(super) fn edit_slides(bytes: &[u8], edit: impl Fn(String) -> String) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        let name = f.name().expect("ZIP member name").to_string();
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            d = edit(String::from_utf8(d).expect("utf8")).into_bytes();
        }
        out.start_file(name, o).expect("start");
        out.write_all(&d).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

pub(super) fn replace_part(bytes: Vec<u8>, part: &str, replacement: &[u8]) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut found = false;
    for i in 0..src.len() {
        let mut f = src.by_index(i).expect("entry");
        let name = f.name().expect("ZIP member name").to_string();
        let mut data = vec![];
        f.read_to_end(&mut data).expect("read");
        if name == part {
            data = replacement.to_vec();
            found = true;
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).expect("start");
        out.write_all(&data).expect("write");
    }
    assert!(found, "the export has no {part}");
    out.finish().expect("finish").into_inner()
}

/// An exported deck with `parts` added (or replaced) and `edit` applied to every XML part.
pub(super) fn with_parts(parts: &[(&str, Vec<u8>)], edit: impl Fn(&str, String) -> String) -> Vec<u8> {
    let base = deckcraft_pptx::export(&defaults::new_presentation(None)).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(base)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let name = f.name().expect("ZIP member name").to_string();
        if parts.iter().any(|(n, _)| *n == name) {
            continue;
        }
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        if name.ends_with(".xml") || name.ends_with(".rels") {
            d = edit(&name, String::from_utf8(d).expect("utf8")).into_bytes();
        }
        out.start_file(name, o).expect("start");
        out.write_all(&d).expect("write");
    }
    for (name, data) in parts {
        out.start_file(*name, o).expect("start");
        out.write_all(data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

pub(super) fn corpus_parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut parts = std::collections::BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).expect("entry");
        if f.is_dir() {
            continue;
        }
        let name = f.name().expect("ZIP member name").trim_start_matches('/').to_string();
        let mut data = vec![];
        f.read_to_end(&mut data).expect("read");
        parts.insert(name, data);
    }
    parts
}
