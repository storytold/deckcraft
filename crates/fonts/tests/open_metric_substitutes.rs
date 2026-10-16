//! Font selection uses attributed bundled bytes without loading application-private fonts.
use deckcraft_fonts::{CRAFT_FONTS, FontDb, bundled, system_font_dirs};

fn assert_bundled_substitute(requested: &str, expected: &str) {
    if !CRAFT_FONTS.iter().any(|font| font.family == expected) {
        eprintln!("skipped optional substitute check: {expected} is not bundled");
        return;
    }
    let db = FontDb::with_font_dirs(Vec::new());
    for style in ["Regular", "Bold", "Italic", "Bold Italic"] {
        for face in [db.face(requested, style), db.document_face(requested, style)] {
            assert_eq!(face.family, expected);
            assert!(CRAFT_FONTS.iter().any(|font| font.family == expected && font.bytes == face.data()));
        }
    }
}

#[test]
fn arial_requests_choose_bundled_liberation_sans_bytes() {
    assert_bundled_substitute("Arial", "Liberation Sans");
    assert_bundled_substitute("aRiAl", "Liberation Sans");
}

#[test]
fn calibri_requests_choose_bundled_carlito_bytes() {
    assert_bundled_substitute("Calibri", "Carlito");
}

#[test]
fn requested_bundled_family_keeps_its_own_face() {
    let db = FontDb::with_font_dirs(Vec::new());
    for font in CRAFT_FONTS.iter().filter(|font| matches!(font.family, "Inter" | "Liberation Sans" | "Carlito" | "Open Sans")) {
        let face = db.face(font.family, font.style);
        assert_eq!(face.family, font.family);
        assert!(bundled().into_iter().any(|bytes| bytes == face.data()));
    }
}

#[test]
fn default_scan_roots_exclude_application_bundles_and_cloud_caches() {
    let dirs = system_font_dirs();
    for dir in &dirs {
        let normalized = dir.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        assert!(!normalized.contains(".app/contents/"), "application bundle must not be a default font root");
        assert!(!normalized.contains("cloudfonts"), "application cloud cache must not be a default font root");
        assert!(!normalized.contains("group containers/ubf8t346g9.office"), "Office container must not be a default font root");
    }
    #[cfg(target_os = "macos")]
    {
        assert!(dirs.iter().any(|dir| dir == std::path::Path::new("/System/Library/Fonts")));
        assert!(dirs.iter().any(|dir| dir == std::path::Path::new("/Library/Fonts")));
    }
    #[cfg(windows)]
    assert!(dirs.iter().any(|dir| dir.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.eq_ignore_ascii_case("Fonts"))));
    #[cfg(target_arch = "wasm32")]
    assert!(dirs.is_empty());
}
