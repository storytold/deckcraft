//! An installed CJK font for the interface, after the embedded craft-fonts faces, when they leave
//! characters of the interface text without a glyph: a build without craft-fonts, one whose
//! craft-fonts checkout lacks the Chinese face, or one whose faces lack characters the
//! catalogs use would otherwise draw Chinese/Korean menus with boxes and `?`. Native only; wasm
//! has no system fonts to read.
//!
//! It looks for a short list of well-known files in the platform's font folders
//! ([`system_font_dirs`](crate::system_font_dirs)) rather than waiting for the full system font
//! scan, since the interface fonts are installed on the first frame.

use skrifa::MetadataProvider;
use skrifa::raw::FileRef;

use crate::CraftFont;

/// An installed font file chosen for the interface.
pub struct SystemUiFont {
    /// The face's family name, e.g. `NanumGothic` or `WenQuanYi Micro Hei`.
    pub family: String,
    /// The whole font file.
    pub bytes: Vec<u8>,
    /// The face's index in the file (0 unless it is a collection).
    pub index: u32,
}

/// Is `c` drawn from a CJK font: Han ideographs, kana, bopomofo, Hangul, CJK punctuation or a
/// full-width form? (Latin, Cyrillic and symbols come from the interface's Latin faces.)
pub fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{2E80}'..='\u{2FDF}' // radicals
        | '\u{3000}'..='\u{33FF}' // punctuation, kana, bopomofo, compatibility
        | '\u{3400}'..='\u{4DBF}' // extension A
        | '\u{4E00}'..='\u{9FFF}' // unified ideographs
        | '\u{AC00}'..='\u{D7AF}' // Hangul syllables
        | '\u{F900}'..='\u{FAFF}' // compatibility ideographs
        | '\u{FE30}'..='\u{FE4F}' // vertical forms
        | '\u{FF00}'..='\u{FFEF}' // full-width forms
        | '\u{20000}'..='\u{3FFFF}' // extensions B and later
    )
}

/// The distinct CJK characters of `texts` (see [`is_cjk`]) that none of the `embedded` faces
/// maps, in code point order. A craft-fonts face may lack characters the interface catalogs use
/// (an older or subset file), and those would draw as boxes or `?`.
pub fn ui_uncovered_cjk(texts: &[&str], embedded: &[&CraftFont]) -> Vec<char> {
    let faces: Vec<_> = embedded.iter().filter_map(|f| skrifa::FontRef::from_index(f.bytes, 0).ok()).map(|f| f.charmap()).collect();
    let chars: std::collections::BTreeSet<char> = texts.iter().flat_map(|t| t.chars()).filter(|c| is_cjk(*c)).collect();
    chars.into_iter().filter(|c| !faces.iter().any(|m| m.map(*c).is_some())).collect()
}

/// Do the embedded interface faces leave CJK characters of `texts` (the
/// interface catalogs and language names that can be on screen) without a glyph? Then an
/// installed CJK font goes after them.
pub fn ui_needs_system_cjk(texts: &[&str], embedded: &[&CraftFont]) -> bool {
    !ui_uncovered_cjk(texts, embedded).is_empty()
}

/// Files larger than this are skipped (the big system CJK collections are around 20–80 MB).
const MAX_FILE: u64 = 128 << 20;
/// Faces looked at in one collection.
const MAX_FACES: u32 = 64;
/// Entries looked at in one macOS font asset folder (each font is one `<hash>.asset` folder).
const MAX_ASSETS: usize = 8192;
/// Characters a face's coverage is measured on.
const MAX_NEEDED: usize = 4096;
/// The tables skrifa reads glyph outlines from: TrueType, CFF, CFF2 and VARC. A face with
/// outlines only in Apple's `hvgl` table has none of them, and the interface draws its characters
/// as blank space.
pub(crate) const OUTLINE_TABLES: [&[u8; 4]; 4] = [b"glyf", b"CFF ", b"CFF2", b"VARC"];

/// A character only a font for the language has: a simplified-only hanzi, kana, or Hangul.
fn probe(hans: bool, korean: bool) -> char {
    if hans {
        '页'
    } else if korean {
        '한'
    } else {
        'の'
    }
}

/// Font files to look for, relative to a font folder (or absolute), best first.
fn candidates(hans: bool, korean: bool) -> Vec<&'static str> {
    let (zh, ja, ko): (&[&str], &[&str], &[&str]) = if cfg!(windows) {
        (
            &["msyh.ttc", "simhei.ttf", "simsun.ttc", "Deng.ttf"],
            &["YuGothR.ttc", "meiryo.ttc", "msgothic.ttc"],
            &["NanumGothic.ttf", "malgun.ttf", "malgunsl.ttf", "gulim.ttc", "batang.ttc"],
        )
    } else if cfg!(target_os = "macos") {
        (
            &[
                "PingFang.ttc",
                "Hiragino Sans GB.ttc",
                "STHeiti Light.ttc",
                "STHeiti Medium.ttc",
                "/System/Library/PrivateFrameworks/FontServices.framework/Resources/Reserved/PingFangUI.ttc",
            ],
            &["ヒラギノ角ゴシック W3.ttc", "Hiragino Sans W3.ttc"],
            &["/System/Library/Fonts/AppleSDGothicNeo.ttc", "/System/Library/Fonts/Supplemental/AppleGothic.ttf"],
        )
    } else {
        (
            &[
                "wenquanyi/wqy-microhei/wqy-microhei.ttc",
                "truetype/wqy/wqy-microhei.ttc",
                "wenquanyi/wqy-zenhei/wqy-zenhei.ttc",
                "truetype/wqy/wqy-zenhei.ttc",
                "truetype/droid/DroidSansFallbackFull.ttf",
            ],
            &[
                "truetype/vlgothic/VL-Gothic-Regular.ttf",
                "TTF/VL-Gothic-Regular.ttf",
                "vlgothic/VL-Gothic-Regular.ttf",
                "truetype/vlgothic/VL-PGothic-Regular.ttf",
            ],
            &["truetype/nanum/NanumGothic.ttf", "truetype/nanum/NanumBarunGothic.ttf", "opentype/nanum/NanumGothic.otf"],
        )
    };
    if hans {
        zh.to_vec()
    } else if korean {
        ko.iter().chain(ja).chain(zh).copied().collect()
    } else {
        ja.iter().chain(zh).copied().collect()
    }
}

/// The installed CJK font for a Chinese (`hans`) or other interface that best covers `needed`
/// (the characters the embedded faces lack; empty: a character typical of the language), if one
/// is found. Never panics on damaged files.
pub fn system_cjk_ui_font(hans: bool, needed: &[char]) -> Option<SystemUiFont> {
    system_cjk_ui_font_for(hans, false, needed)
}

/// The installed CJK font for a Chinese, Korean or Japanese interface.
pub fn system_cjk_ui_font_for(hans: bool, korean: bool, needed: &[char]) -> Option<SystemUiFont> {
    let probe = [probe(hans, korean)];
    let needed = if needed.is_empty() { &probe[..] } else { needed.get(..MAX_NEEDED).unwrap_or(needed) };
    find_ui_font(&with_asset_data(crate::system_font_dirs()), &candidates(hans, korean), needed, hans)
}

/// `dirs` and, for each macOS font asset folder among them (`com_apple_MobileAsset_Font<N>`),
/// the `AssetData` folder of every `<hash>.asset` in it: macOS ships PingFang there.
/// One level, at most [`MAX_ASSETS`] entries per folder.
pub(crate) fn with_asset_data(dirs: Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    let mut out = Vec::with_capacity(dirs.len());
    for d in dirs {
        let assets = d.file_name().is_some_and(|n| n.to_string_lossy().starts_with("com_apple_MobileAsset_Font"));
        if assets && let Ok(entries) = std::fs::read_dir(&d) {
            let mut found: Vec<_> =
                entries.flatten().take(MAX_ASSETS).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "asset")).collect();
            found.sort();
            out.extend(found.into_iter().map(|p| p.join("AssetData")));
        }
        out.push(d);
    }
    out
}

/// The font file among `names` (tried in order, each in every folder of `dirs`) whose best face
/// maps all of `needed`, or failing that the one mapping the most of them (the earliest on a
/// tie). A file that maps none of them is never chosen.
pub(crate) fn find_ui_font(dirs: &[std::path::PathBuf], names: &[&str], needed: &[char], hans: bool) -> Option<SystemUiFont> {
    let mut tried = std::collections::HashSet::new();
    let mut best: Option<(usize, SystemUiFont)> = None;
    for path in names.iter().flat_map(|n| dirs.iter().map(move |d| d.join(n))) {
        if !tried.insert(path.clone()) {
            continue;
        }
        let Some(size) = std::fs::metadata(&path).ok().filter(std::fs::Metadata::is_file).map(|m| m.len()) else { continue };
        if size > MAX_FILE {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Some((index, family, covered)) = pick_face(&bytes, needed, hans) else { continue };
        if best.as_ref().is_some_and(|(b, _)| *b >= covered) {
            continue;
        }
        log::debug!("interface CJK fallback: {family} ({}), {covered} of {} characters", path.display(), needed.len());
        best = Some((covered, SystemUiFont { family, bytes, index }));
        if covered >= needed.len() {
            break;
        }
    }
    best.map(|(_, f)| f)
}

/// The face of a font file (or collection) to use and how many of `needed` it maps: the one
/// mapping the most, then preferring a Regular style and, for Chinese, a Simplified Chinese
/// family (`… SC`, `… GB`, as in a pan-CJK collection). Faces with none of the
/// [`OUTLINE_TABLES`] are skipped. `None` if the header isn't a font's or no face with outlines
/// maps any of `needed`.
pub(crate) fn pick_face(data: &[u8], needed: &[char], hans: bool) -> Option<(u32, String, usize)> {
    let magic = data.get(..4)?;
    if ![&[0, 1, 0, 0][..], b"OTTO", b"ttcf"].contains(&magic) {
        return None;
    }
    let count = match FileRef::new(data).ok()? {
        FileRef::Font(_) => 1,
        FileRef::Collection(c) => c.len().min(MAX_FACES),
    };
    let mut best: Option<((usize, bool, bool), u32, String)> = None;
    for i in 0..count {
        let Ok(f) = skrifa::FontRef::from_index(data, i) else { continue };
        if !OUTLINE_TABLES.iter().any(|t| f.table_data(skrifa::Tag::new(t)).is_some()) {
            continue;
        }
        let charmap = f.charmap();
        let covered = needed.iter().filter(|c| charmap.map(**c).is_some()).count();
        if covered == 0 {
            continue;
        }
        let Some((family, style)) = crate::fontdb::face_names(&f) else { continue };
        let regional = !hans || family.ends_with(" SC") || family.ends_with(" GB") || family.contains("YaHei");
        let score = (covered, style.eq_ignore_ascii_case("Regular"), regional);
        if best.as_ref().is_none_or(|(s, ..)| score > *s) {
            best = Some((score, i, family));
        }
    }
    best.map(|((covered, ..), i, family)| (i, family, covered))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn korean_interface_uses_korean_first_system_font_candidates() {
        let names = candidates(false, true);
        assert_eq!(
            names.first(),
            Some(&if cfg!(windows) {
                "NanumGothic.ttf"
            } else if cfg!(target_os = "macos") {
                "/System/Library/Fonts/AppleSDGothicNeo.ttc"
            } else {
                "truetype/nanum/NanumGothic.ttf"
            })
        );
        assert_eq!(probe(false, true), '한');
    }

    #[test]
    fn hans_takes_priority_when_both_hans_and_korean_flags_are_set() {
        assert_eq!(probe(true, true), '页');
        let names = candidates(true, true);
        assert_eq!(
            names.first(),
            Some(&if cfg!(windows) {
                "msyh.ttc"
            } else if cfg!(target_os = "macos") {
                "PingFang.ttc"
            } else {
                "wenquanyi/wqy-microhei/wqy-microhei.ttc"
            })
        );
    }

    #[test]
    fn korean_system_font_is_found_on_system() {
        let found = system_cjk_ui_font_for(false, true, &['한', '글', '홈']);
        if cfg!(windows) || cfg!(target_os = "macos") {
            assert!(found.is_some(), "System Korean font should be found on Windows/macOS");
            if let Some(font) = found {
                assert!(!font.bytes.is_empty());
            }
        }
    }
}
