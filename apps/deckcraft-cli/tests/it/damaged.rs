//! Opening a `.pptx` that lost slides says so on stderr instead of saving a shorter deck in silence (#203).

use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).args(args).output().unwrap()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `deck` with `ppt/slides/slide2.xml` cut off.
fn without_slide2(deck: &[u8]) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(Cursor::new(deck)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let mut data = vec![];
        f.read_to_end(&mut data).unwrap();
        if f.name().unwrap() == "ppt/slides/slide2.xml" {
            data = b"<p:sld".to_vec();
        }
        out.start_file(f.name().unwrap(), options).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn a_pptx_with_an_unreadable_slide_is_reported_on_stderr() {
    let dir = temp_dir("lost-slide");
    let good = dir.join("good.pptx");
    let made = cli(&["run", "--sample", "--save", good.to_str().unwrap()]);
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let broken = dir.join("broken.pptx");
    std::fs::write(&broken, without_slide2(&std::fs::read(&good).unwrap())).unwrap();

    let ok = cli(&["info", good.to_str().unwrap()]);
    assert!(ok.status.success() && !String::from_utf8_lossy(&ok.stderr).contains("warning"), "{}", String::from_utf8_lossy(&ok.stderr));

    let bad = cli(&["info", broken.to_str().unwrap()]);
    assert!(bad.status.success(), "{}", String::from_utf8_lossy(&bad.stderr));
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("warning") && err.contains("1 slide couldn't be read and left out"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}
