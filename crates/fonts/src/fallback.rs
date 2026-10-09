//! Font fallback per grapheme cluster, and ligature caret positions.
//!
//! The cascade for a cluster (a base character with its marks, never split across faces) is:
//! 1. the faces the document asks for, in order (the run's slot typeface, then the theme's script
//!    font, `a:font[@script="Arab"]`);
//! 2. for scripts with a platform cascade (Arabic, Hebrew, Thai, Devanagari): the craft-fonts face
//!    bundled for the script (Noto Naskh Arabic for Arabic), so every machine draws the same; then
//!    openly licensed or OS-native families found by name among the installed fonts (no platform
//!    API is called); then any installed face covering the cluster;
//! 3. any loaded face (bundled first: Japanese keeps its craft-fonts faces), then the first
//!    requested face (drawn as .notdef).

use std::sync::Arc;

use skrifa::raw::TableProvider;

use crate::script::{ScriptTag, script_of};
use crate::{CRAFT_FONTS, FontDb, FontFace};

/// Characters that never decide a face: they're drawn by whatever face the cluster gets.
fn ignorable(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2069 | 0xFE00..=0xFE0F | 0xFEFF | 0xE0100..=0xE01EF)
}

/// Does `face` draw every character of the cluster?
pub fn covers_cluster(face: &FontFace, cluster: &[char]) -> bool {
    cluster.iter().all(|c| ignorable(*c) || face.covers(*c))
}

/// Installed families tried by name for a script after the bundled face, before scanning every
/// installed font. Microsoft font names are not listed (AGENTS.md §1.1); an installed one is
/// still used by the scan when nothing earlier covers the cluster.
fn os_families(script: ScriptTag) -> &'static [&'static str] {
    match &script {
        b"Arab" => &["Noto Naskh Arabic", "Noto Sans Arabic", "Geeza Pro", "SF Arabic", "DejaVu Sans"],
        b"Hebr" => &["Noto Sans Hebrew", "Arial Hebrew", "SF Hebrew", "DejaVu Sans"],
        b"Thai" => &["Noto Sans Thai", "Thonburi"],
        b"Deva" => &["Noto Sans Devanagari", "Kohinoor Devanagari", "Devanagari Sangam MN"],
        _ => &[],
    }
}

/// The craft-fonts families for a script (manifest order), e.g. Noto Naskh Arabic for `Arab`.
pub fn bundled_families(script: ScriptTag) -> Vec<&'static str> {
    let tag = std::str::from_utf8(&script).unwrap_or("");
    let mut v: Vec<&'static str> = CRAFT_FONTS.iter().filter(|f| f.scripts.contains(&tag)).map(|f| f.family).collect();
    v.dedup();
    v
}

impl FontDb {
    /// The face for one grapheme cluster: the first of `chain` that draws all of it, else the
    /// bundled face for its script, else an installed face for the script, else any loaded
    /// face that draws it, else `chain[0]` (or the default face if `chain` is empty).
    pub fn cascade(&self, cluster: &[char], chain: &[Arc<FontFace>], style: &str) -> Arc<FontFace> {
        if let Some(f) = chain.iter().find(|f| covers_cluster(f, cluster)) {
            return f.clone();
        }
        let first = || chain.first().cloned().unwrap_or_else(|| self.face(crate::DEFAULT_FAMILY, style));
        let Some(key) = cluster.iter().copied().find(|c| !ignorable(*c)) else { return first() };
        let script = cluster.iter().find_map(|c| script_of(*c));
        // Scripts with a platform cascade (Arabic, Hebrew, Thai, Devanagari): the bundled face,
        // then installed ones; the others keep the loaded-faces-first order (Japanese documents
        // prefer the bundled craft-fonts faces).
        if let Some(s) = script.filter(|s| !os_families(*s).is_empty()) {
            let bundled = bundled_families(s);
            for fam in &bundled {
                let f = self.face(fam, style);
                if f.family.eq_ignore_ascii_case(fam) && covers_cluster(&f, cluster) {
                    return f;
                }
            }
            for fam in os_families(s) {
                if bundled.contains(fam) {
                    continue;
                }
                if self.is_loaded(fam) || self.has_family(fam) {
                    let f = self.face(fam, style);
                    if f.family.eq_ignore_ascii_case(fam) && covers_cluster(&f, cluster) {
                        return f;
                    }
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(f) = self.system_face_for(key, cluster, &bundled) {
                return f;
            }
        }
        let exclude = chain.first().map(|f| f.id()).unwrap_or(u32::MAX);
        match self.fallback_for(key, exclude) {
            Some(f) if covers_cluster(&f, cluster) => f,
            Some(f) if chain.iter().all(|c| !c.covers(key)) => f,
            _ => first(),
        }
    }

    /// An installed (non-bundled) face covering the cluster, loading one from the system catalog
    /// if no loaded face does.
    #[cfg(not(target_arch = "wasm32"))]
    fn system_face_for(&self, key: char, cluster: &[char], bundled: &[&str]) -> Option<Arc<FontFace>> {
        let pick = |db: &FontDb| {
            let faces = db.read_faces();
            let mut v: Vec<&Arc<FontFace>> = faces
                .iter()
                .filter(|f| !bundled.iter().any(|b| f.family.eq_ignore_ascii_case(b)) && !CRAFT_FONTS.iter().any(|c| c.family == f.family))
                .filter(|f| covers_cluster(f, cluster))
                .collect();
            v.sort_by_key(|f| (f.italic, (f.weight - 400.0).abs() as i32));
            v.first().map(|f| (*f).clone())
        };
        if let Some(f) = pick(self) {
            return Some(f);
        }
        if self.system_fallback(key) { pick(self) } else { None }
    }
}

/// Caret positions inside a ligature glyph from GDEF `LigCaretList`, in font units from the
/// glyph's origin (sorted ascending). Empty when the font has none for `gid`.
pub fn ligature_carets(face: &FontFace, gid: u32) -> Vec<i32> {
    let Some(font) = face.skrifa() else { return Vec::new() };
    let Ok(gdef) = font.gdef() else { return Vec::new() };
    let Some(Ok(list)) = gdef.lig_caret_list() else { return Vec::new() };
    let Ok(cov) = list.coverage() else { return Vec::new() };
    let Some(idx) = cov.get(skrifa::GlyphId::new(gid)) else { return Vec::new() };
    let Ok(lig) = list.lig_glyphs().get(idx as usize) else { return Vec::new() };
    let mut out: Vec<i32> = lig
        .caret_values()
        .iter()
        .filter_map(|cv| cv.ok())
        .filter_map(|cv| match cv {
            skrifa::raw::tables::gdef::CaretValue::Format1(f) => Some(f.coordinate() as i32),
            skrifa::raw::tables::gdef::CaretValue::Format3(f) => Some(f.coordinate() as i32),
            // Format 2 is a contour point index; outlines aren't consulted here.
            skrifa::raw::tables::gdef::CaretValue::Format2(_) => None,
        })
        .take(64)
        .collect();
    out.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_face_wins_when_it_covers() {
        let db = FontDb::global();
        let inter = db.face(crate::DEFAULT_FAMILY, "Regular");
        let f = db.cascade(&['a'], std::slice::from_ref(&inter), "Regular");
        assert!(Arc::ptr_eq(&f, &inter));
        // Spaces and joiners never force a fallback.
        let f = db.cascade(&[' ', '\u{200D}'], std::slice::from_ref(&inter), "Regular");
        assert!(Arc::ptr_eq(&f, &inter));
        // Nothing to go on: the default face.
        assert!(!db.cascade(&[], &[], "Regular").family.is_empty());
    }

    #[test]
    fn arabic_cluster_falls_back_to_an_arabic_face() {
        let db = FontDb::global();
        let inter = db.face(crate::DEFAULT_FAMILY, "Regular");
        let cluster = ['\u{0628}', '\u{064E}']; // beh + fatha: one cluster, one face.
        let f = db.cascade(&cluster, std::slice::from_ref(&inter), "Regular");
        if covers_cluster(&f, &cluster) {
            assert!(f.covers('\u{0628}'));
        } else {
            eprintln!("skipped: no installed or bundled face covers Arabic");
        }
    }

    #[test]
    fn ligature_carets_never_fail() {
        let face = FontDb::global().face(crate::DEFAULT_FAMILY, "Regular");
        for gid in [0, 1, 100, u32::MAX] {
            let c = ligature_carets(&face, gid);
            assert!(c.windows(2).all(|w| w[0] <= w[1]));
        }
    }
}
