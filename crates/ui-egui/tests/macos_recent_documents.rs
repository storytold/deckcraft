//! Opening or saving a presentation is recorded with macOS recent documents.
//!
//! `NSDocumentController` is main-thread only. Cargo's default test harness parks that thread, so
//! this binary does the check in `main` instead of hopping over with `dispatch_sync`.

use std::path::Path;

fn main() {
    if let Err(err) = check() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn check() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let path = std::env::temp_dir().join(format!("deckcraft-recent-{}.deckcraft", std::process::id()));
    let png = std::env::temp_dir().join(format!("deckcraft-recent-{}.png", std::process::id()));
    write_pair(&path, &png)?;
    let outcome = observe(&path, &png);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&png);
    outcome
}

fn write_pair(path: &Path, png: &Path) -> Result<(), String> {
    std::fs::write(path, b"deckcraft").map_err(|e| e.to_string())?;
    std::fs::write(png, b"png").map_err(|e| e.to_string())?;
    Ok(())
}

fn observe(path: &Path, png: &Path) -> Result<(), String> {
    let want = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let want_s = path_string(&want)?;
    let png_s = path_string(png)?;
    let png_name = png.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    deckcraft_ui_egui::recent_docs::note_system_recent(&want_s);
    deckcraft_ui_egui::recent_docs::note_system_recent(&png_s);
    let listed = deckcraft_ui_egui::recent_docs::system_recent_paths();
    let found = listed.iter().any(|p| p == &want_s || std::fs::canonicalize(p).ok().as_ref() == Some(&want));
    if !found {
        return Err(format!("recent documents {listed:?} did not include {want_s}"));
    }
    if listed.iter().any(|p| Path::new(p).file_name().is_some_and(|n| n.to_string_lossy() == png_name)) {
        return Err(format!("an export was recorded: {png_name}"));
    }
    Ok(())
}

fn path_string(path: &Path) -> Result<String, String> {
    path.to_str().map(str::to_string).ok_or_else(|| format!("path is not utf-8: {}", path.display()))
}
