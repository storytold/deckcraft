//! The desktop app's settings files, `ui.json` ([`UiState`]) and `prefs.json`
//! ([`deckcraft_engine::Prefs`]), in the per-user settings folder. They are files on disk that a
//! crash, a full disk or a hand edit can leave damaged, so reading never fails and never trusts
//! them: an unreadable, oversized or malformed file gives the defaults, and the values of a
//! well-formed one are brought into range ([`UiState::sanitized`], `Prefs::sanitized`). Writing
//! goes through a temporary file and a rename, so a crash mid-write leaves the old file.

use std::path::Path;

use deckcraft_engine::Prefs;

use crate::UiState;

pub const UI_FILE: &str = "ui.json";
pub const PREFS_FILE: &str = "prefs.json";
/// Larger settings files are not settings.
pub const MAX_BYTES: u64 = 4 << 20;

/// The JSON in `path`, when it is a regular file of a sane size that holds a `T`.
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        log::warn!("{}: not a settings file; using the defaults", path.display());
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("{}: {e}; using the defaults", path.display());
            None
        }
    }
}

/// The saved UI state and preferences in `dir` (each `None` when missing or unusable).
pub fn load(dir: &Path) -> (Option<UiState>, Option<Prefs>) {
    (read::<UiState>(&dir.join(UI_FILE)).map(UiState::sanitized), read::<Prefs>(&dir.join(PREFS_FILE)).map(Prefs::sanitized))
}

/// Write `bytes` to `path` through a temporary file beside it.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Save both files into `dir` (created when missing). Failures are logged, not fatal.
pub fn save(dir: &Path, ui: &UiState, prefs: &Prefs) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("{}: {e}; settings not saved", dir.display());
        return;
    }
    for (name, json) in [(UI_FILE, serde_json::to_vec_pretty(ui)), (PREFS_FILE, serde_json::to_vec_pretty(prefs))] {
        let path = dir.join(name);
        if let Err(e) = json.map_err(std::io::Error::other).and_then(|b| write_atomic(&path, &b)) {
            log::warn!("{}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("deckcraft-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn damaged_settings_files_give_the_defaults() {
        let d = dir("damaged");
        let cases: [&[u8]; 6] = [b"", b"{", b"\xFF\xFE garbage", b"[1, 2, 3]", b"{\"zoom\": \"big\"}", b"null"];
        for bytes in cases {
            std::fs::write(d.join(UI_FILE), bytes).unwrap();
            std::fs::write(d.join(PREFS_FILE), bytes).unwrap();
            let (ui, prefs) = load(&d);
            // Prefs has no `zoom`, so that case is a valid (default) Prefs.
            assert!(ui.is_none(), "{:?}", String::from_utf8_lossy(bytes));
            assert!(prefs.is_none() || bytes.starts_with(b"{\"zoom"), "{:?}", String::from_utf8_lossy(bytes));
        }
        // A folder where a file should be, and an oversized file.
        std::fs::remove_file(d.join(UI_FILE)).unwrap();
        std::fs::create_dir(d.join(UI_FILE)).unwrap();
        std::fs::write(d.join(PREFS_FILE), vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(matches!(load(&d), (None, None)));
        // Nothing there at all.
        assert!(matches!(load(&d.join("missing")), (None, None)));
    }

    #[test]
    fn out_of_range_values_are_brought_into_range() {
        let d = dir("ranges");
        let recent: Vec<String> = (0..5000).map(|i| format!("/tmp/deck{i}.pptx")).collect();
        let ui = serde_json::json!({
            "zoom": 1.0e30, "sorterZoom": -4.0, "uiScale": 1.0e9, "notesHeight": 1.0e30,
            "thumbsWidth": -1.0e30, "paneWidth": 3.4e38, "tab": "x".repeat(10_000), "pane": "y".repeat(500),
            "recent": recent,
        });
        let prefs = serde_json::json!({"gridSpacing": 0.0, "duplicateOffset": 1.0e300, "undoLimit": u64::MAX, "recoveryMinutes": -5.0, "author": "a".repeat(100_000)});
        std::fs::write(d.join(UI_FILE), ui.to_string()).unwrap();
        std::fs::write(d.join(PREFS_FILE), prefs.to_string()).unwrap();
        let (ui, prefs) = load(&d);
        let (ui, prefs) = (ui.unwrap(), prefs.unwrap());
        assert_eq!(ui.zoom, Some(*crate::ZOOM_RANGE.end()));
        assert_eq!((ui.sorter_zoom, ui.ui_scale), (0.5, 3.0));
        assert!(ui.notes_height <= 2000.0 && ui.thumbs_width >= 60.0 && ui.pane_width <= 520.0);
        assert_eq!((ui.tab.as_str(), ui.pane.as_deref()), ("Home", None));
        assert_eq!(ui.recent.len(), crate::RECENT_MAX);
        assert_eq!(prefs.grid_spacing, Prefs::default().grid_spacing);
        assert_eq!(prefs.duplicate_offset, 1000.0);
        assert_eq!(prefs.undo_limit, deckcraft_engine::MAX_UNDO_LIMIT);
        assert_eq!(prefs.recovery_minutes, 0.1);
        assert_eq!(prefs.author.chars().count(), 256);
        // A zero or negative zoom means "fit".
        std::fs::write(d.join(UI_FILE), r#"{"zoom": 0}"#).unwrap();
        assert_eq!(load(&d).0.unwrap().zoom, None);
    }

    #[test]
    fn saving_round_trips_and_leaves_no_temporary_file() {
        let d = dir("save");
        let ui = UiState { zoom: Some(1.5), recent: vec!["/a.pptx".into()], ..Default::default() };
        let prefs = Prefs { author: "Ann".into(), units: deckcraft_geom::Unit::Centimeters, ..Default::default() };
        save(&d.join("nested"), &ui, &prefs);
        let (u, p) = load(&d.join("nested"));
        assert_eq!(u.unwrap().zoom, Some(1.5));
        let p = p.unwrap();
        assert_eq!((p.author.as_str(), p.units), ("Ann", deckcraft_geom::Unit::Centimeters));
        let names: Vec<String> =
            std::fs::read_dir(d.join("nested")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
        // A settings folder that is a file: nothing saved, no panic.
        std::fs::write(d.join("blocked"), "file").unwrap();
        save(&d.join("blocked"), &ui, &prefs);
    }

    /// The window opens with the worst `ui.json` values once they are sanitized; a zoom of
    /// 10,000 % used to panic in egui and 1e30 exhausted memory at start-up.
    #[test]
    fn the_window_opens_with_hostile_saved_state() {
        let hostile = UiState {
            zoom: Some(1.0e30),
            notes_height: 1.0e30,
            thumbs_width: -1.0e30,
            pane: Some("format".into()),
            pane_width: 1.0e30,
            ..Default::default()
        };
        for ui in [hostile.clone().sanitized(), UiState { zoom: Some(1.0e4), ..Default::default() }] {
            let mut app = crate::SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
            // The second case is not sanitized: the canvas keeps a stray zoom in range itself.
            app.ui = ui;
            let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
                |ui, app: &mut crate::SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            h.run_steps(4);
            assert!(h.state().canvas_scale <= *crate::ZOOM_RANGE.end() + 1e-3, "{}", h.state().canvas_scale);
        }
    }
}
