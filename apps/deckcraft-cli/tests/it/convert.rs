//! `deckcraft-cli convert` writes the format OUT's extension names, or fails without writing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli(args: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).args(args).output().unwrap()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn convert_rejects_output_formats_it_cannot_write() {
    let dir = temp_dir("convert-unsupported");
    let input = dir.join("in.pptx");
    let out = Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).args(["run", "--sample", "--export"]).arg(&input).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    for name in ["out.odp", "out.bmp", "out.svg", "out.outline", "out"] {
        let target = dir.join(name);
        let out = cli(&[Path::new("convert"), &input, &target]);
        assert!(!out.status.success(), "{name}: convert should fail");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("unsupported output format") && err.contains(".pptx"), "{name}: {err}");
        assert!(!target.exists(), "{name}: nothing is written");
    }

    let target = dir.join("out.deckcraft");
    let out = cli(&[Path::new("convert"), &input, &target]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read(&target).unwrap().starts_with(b"PK\x03\x04"));

    let target = dir.join("out.txt");
    let out = cli(&[Path::new("convert"), &input, &target]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!std::fs::read(&target).unwrap().starts_with(b"PK"), "an outline is text, not a zip");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_rejects_unsupported_save_and_export_paths_before_running() {
    let dir = temp_dir("run-unsupported");
    for flag in ["--save", "--export"] {
        let target = dir.join("deck.odp");
        let out = Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).args(["run", "--sample", flag]).arg(&target).output().unwrap();
        assert!(!out.status.success(), "{flag}: run should fail");
        assert!(String::from_utf8_lossy(&out.stderr).contains("unsupported output format .odp"), "{flag}");
        assert!(!target.exists(), "{flag}: nothing is written");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
