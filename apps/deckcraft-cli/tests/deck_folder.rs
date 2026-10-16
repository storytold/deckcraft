//! `deckcraft-cli convert` and `info` with `.deck` folders.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn cli(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).args(args).output().expect("run deckcraft-cli");
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `info` without the file path and the title taken from it.
fn info(path: &str) -> Value {
    let mut v: Value = serde_json::from_str(&cli(&["info", path])).expect("info JSON");
    if let Some(o) = v.as_object_mut() {
        o.remove("path");
        o.remove("title");
    }
    v
}

#[test]
fn convert_to_a_folder_and_back() {
    let dir = std::env::temp_dir().join(format!("deckcraft-cli-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let p = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let (zip, deck, back) = (p("a.deckcraft"), p("a.deck"), p("b.deckcraft"));
    cli(&["run", "--sample", "--save", &zip]);
    cli(&["convert", &zip, &deck]);
    assert!(Path::new(&deck).join("deck.json").is_file());
    assert!(Path::new(&deck).join("slides.order").is_file());
    cli(&["convert", &deck, &back]);
    let a = info(&zip);
    assert_eq!(info(&deck), a);
    assert_eq!(info(&back), a);
    // Converting again onto the same folder leaves it as it was.
    let before = std::fs::read(Path::new(&deck).join("slides.order")).expect("slides.order");
    cli(&["convert", &back, &deck]);
    assert_eq!(std::fs::read(Path::new(&deck).join("slides.order")).expect("slides.order"), before);
    let _ = std::fs::remove_dir_all(&dir);
}
