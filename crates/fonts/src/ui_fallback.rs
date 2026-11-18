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
#[derive(Clone)]
pub struct SystemUiFont {
    /// Path of the chosen font file on disk.
    pub path: std::path::PathBuf,
    /// The face's family name, e.g. `NanumGothic` or `WenQuanYi Micro Hei`.
    pub family: String,
    /// The whole font file, shared across faces from the same collection path.
    pub bytes: std::sync::Arc<Vec<u8>>,
    /// The face's index in the file (0 unless it is a collection).
    pub index: u32,
}

/// Is `c` drawn from a CJK font: Han ideographs, kana, bopomofo, Hangul, CJK punctuation or a
/// full-width form? (Latin, Cyrillic and symbols come from the interface's Latin faces.)
pub(crate) fn is_cjk(c: char) -> bool {
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

/// The distinct CJK characters of `texts` (`is_cjk`) that none of the `embedded` faces
/// maps, in code point order. A craft-fonts face may lack characters the interface catalogs use
/// (an older or subset file), and those would draw as boxes or `?`.
pub fn ui_uncovered_cjk(texts: &[&str], embedded: &[&CraftFont]) -> Vec<char> {
    let faces: Vec<_> = embedded.iter().filter_map(|f| skrifa::FontRef::from_index(f.bytes, 0).ok()).map(|f| f.charmap()).collect();
    let chars: std::collections::BTreeSet<char> = texts.iter().flat_map(|t| t.chars()).filter(|c| is_cjk(*c)).collect();
    chars.into_iter().filter(|c| !faces.iter().any(|m| m.map(*c).is_some())).collect()
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

/// The CJK interface language a system font is chosen for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UiCjk {
    /// Simplified Chinese (`zh-hans`).
    Hans,
    /// Traditional Chinese (`zh-hant`).
    Hant,
    /// Korean.
    Korean,
    /// Japanese, or any other CJK interface.
    Other,
}

/// A character only a font for the language has: a simplified-only or traditional-only hanzi,
/// kana, or Hangul.
fn probe(mode: UiCjk) -> char {
    match mode {
        UiCjk::Hans => '页',
        UiCjk::Hant => '頁',
        UiCjk::Korean => '한',
        UiCjk::Other => 'の',
    }
}

/// Font files to look for, relative to a font folder (or absolute), best first.
fn candidates(mode: UiCjk) -> Vec<&'static str> {
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
    let hant: &[&str] = if cfg!(windows) { &["msjh.ttc", "msjhl.ttc", "mingliu.ttc"] } else { &[] };
    match mode {
        UiCjk::Hans => zh.to_vec(),
        UiCjk::Hant => hant.iter().chain(zh).copied().collect(),
        UiCjk::Korean => ko.iter().chain(ja).chain(zh).copied().collect(),
        UiCjk::Other => ja.iter().chain(zh).copied().collect(),
    }
}

/// The installed CJK font for a Chinese (Simplified or Traditional), Korean or Japanese interface
/// that best covers `needed`, reusing bytes from `file_cache` when a path is already loaded and
/// admitting a newly read file only within `max_new_bytes`.
pub fn system_cjk_ui_font_for(
    mode: UiCjk,
    needed: &[char],
    file_cache: &mut std::collections::HashMap<std::path::PathBuf, std::sync::Arc<Vec<u8>>>,
    max_new_bytes: usize,
) -> Option<SystemUiFont> {
    let probe = [probe(mode)];
    let needed = if needed.is_empty() { &probe[..] } else { needed.get(..MAX_NEEDED).unwrap_or(needed) };
    find_ui_font(&with_asset_data(crate::system_font_dirs()), &candidates(mode), needed, mode, file_cache, max_new_bytes)
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
/// tie). A file that maps none of them is never chosen. Files already present in `file_cache`
/// share their bytes without counting against `max_new_bytes`; a newly chosen file is recorded in
/// `file_cache`.
pub(crate) fn find_ui_font(
    dirs: &[std::path::PathBuf],
    names: &[&str],
    needed: &[char],
    mode: UiCjk,
    file_cache: &mut std::collections::HashMap<std::path::PathBuf, std::sync::Arc<Vec<u8>>>,
    max_new_bytes: usize,
) -> Option<SystemUiFont> {
    let mut tried = std::collections::HashSet::new();
    let mut best: Option<(usize, SystemUiFont)> = None;
    let file_cap = MAX_FILE.min(u64::try_from(max_new_bytes).unwrap_or(u64::MAX));
    for path in names.iter().flat_map(|n| dirs.iter().map(move |d| d.join(n))) {
        if !tried.insert(path.clone()) {
            continue;
        }
        let bytes = if let Some(cached) = file_cache.get(&path) {
            cached.clone()
        } else {
            let Some(size) = std::fs::metadata(&path).ok().filter(std::fs::Metadata::is_file).map(|m| m.len()) else { continue };
            if size > file_cap {
                continue;
            }
            let Ok(raw) = std::fs::read(&path) else { continue };
            if raw.len() as u64 > MAX_FILE || raw.len() > max_new_bytes {
                continue;
            }
            std::sync::Arc::new(raw)
        };
        let Some((index, family, covered)) = pick_face(&bytes, needed, mode) else { continue };
        if best.as_ref().is_some_and(|(b, _)| *b >= covered) {
            continue;
        }
        log::debug!("interface CJK fallback: {family} ({}), {covered} of {} characters", path.display(), needed.len());
        best = Some((covered, SystemUiFont { path, family, bytes, index }));
        if covered >= needed.len() {
            break;
        }
    }
    if let Some((_, font)) = &best {
        file_cache.entry(font.path.clone()).or_insert_with(|| font.bytes.clone());
    }
    best.map(|(_, f)| f)
}

/// The face of a font file (or collection) to use and how many of `needed` it maps: the one
/// mapping the most, then preferring a Regular style and, for Chinese, a family of the script
/// (Simplified: `… SC`, `… GB`; Traditional: `… TC`, `… HK`, as in a pan-CJK collection). Faces
/// with none of the [`OUTLINE_TABLES`] are skipped. `None` if the header isn't a font's or no
/// face with outlines maps any of `needed`.
pub(crate) fn pick_face(data: &[u8], needed: &[char], mode: UiCjk) -> Option<(u32, String, usize)> {
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
        let regional = match mode {
            UiCjk::Hans => family.ends_with(" SC") || family.ends_with(" GB") || family.contains("YaHei"),
            UiCjk::Hant => family.ends_with(" TC") || family.ends_with(" HK") || family.contains("JhengHei") || family.contains("MingLiU"),
            UiCjk::Korean | UiCjk::Other => true,
        };
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

    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;

    struct FaceSpec<'a> {
        family: &'a str,
        style: &'a str,
        chars: &'a [char],
        outline_table: Option<&'static [u8; 4]>,
    }

    fn push_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_be_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_be_bytes());
    }

    fn build_cmap_format4(chars: &[char]) -> Vec<u8> {
        let mut sorted: Vec<u16> = chars.iter().map(|&c| u16::try_from(c as u32).unwrap()).collect();
        sorted.sort_unstable();
        sorted.dedup();
        let seg_count = u16::try_from(sorted.len() + 1).unwrap();
        let seg_count_x2 = seg_count * 2;
        let max_pow2 = seg_count.ilog2();
        let search_range = (1u16 << max_pow2) * 2;
        let entry_selector = u16::try_from(max_pow2).unwrap();
        let range_shift = seg_count_x2 - search_range;
        let subtable_len = 16 + seg_count * 8;

        let mut out = Vec::new();
        // cmap header
        push_u16(&mut out, 0); // version
        push_u16(&mut out, 1); // numTables
        push_u16(&mut out, 3); // platformID (Windows)
        push_u16(&mut out, 1); // encodingID (Unicode BMP)
        push_u32(&mut out, 12); // subtable offset
        // format 4 subtable
        push_u16(&mut out, 4); // format
        push_u16(&mut out, subtable_len);
        push_u16(&mut out, 0); // language
        push_u16(&mut out, seg_count_x2);
        push_u16(&mut out, search_range);
        push_u16(&mut out, entry_selector);
        push_u16(&mut out, range_shift);
        for &c in &sorted {
            push_u16(&mut out, c);
        }
        push_u16(&mut out, 0xFFFF);
        push_u16(&mut out, 0); // reservedPad
        for &c in &sorted {
            push_u16(&mut out, c);
        }
        push_u16(&mut out, 0xFFFF);
        for (idx, &c) in sorted.iter().enumerate() {
            let gid = i32::try_from(idx + 1).unwrap();
            let delta = gid - i32::from(c);
            push_u16(&mut out, delta.rem_euclid(65536) as u16);
        }
        push_u16(&mut out, 1); // sentinel idDelta (0xFFFF + 1 = 0)
        for _ in 0..seg_count {
            push_u16(&mut out, 0); // idRangeOffset
        }
        out
    }

    fn build_name_table(family: &str, style: &str) -> Vec<u8> {
        let utf16_be = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(u16::to_be_bytes).collect() };
        let fam_bytes = utf16_be(family);
        let sty_bytes = utf16_be(style);
        let mut out = Vec::new();
        push_u16(&mut out, 0); // format
        push_u16(&mut out, 2); // count
        push_u16(&mut out, 6 + 2 * 12); // stringOffset
        // family record (nameID 1)
        push_u16(&mut out, 3);
        push_u16(&mut out, 1);
        push_u16(&mut out, 0x0409);
        push_u16(&mut out, 1);
        push_u16(&mut out, u16::try_from(fam_bytes.len()).unwrap());
        push_u16(&mut out, 0);
        // style record (nameID 2)
        push_u16(&mut out, 3);
        push_u16(&mut out, 1);
        push_u16(&mut out, 0x0409);
        push_u16(&mut out, 2);
        push_u16(&mut out, u16::try_from(sty_bytes.len()).unwrap());
        push_u16(&mut out, u16::try_from(fam_bytes.len()).unwrap());
        out.extend_from_slice(&fam_bytes);
        out.extend_from_slice(&sty_bytes);
        out
    }

    fn build_sfnt_at(spec: &FaceSpec<'_>, base_offset: usize) -> Vec<u8> {
        let mut tables: Vec<(&[u8; 4], Vec<u8>)> =
            vec![(b"cmap", build_cmap_format4(spec.chars)), (b"name", build_name_table(spec.family, spec.style))];
        if let Some(tag) = spec.outline_table {
            tables.push((tag, vec![0, 0, 0, 0]));
        }
        tables.sort_by_key(|(tag, _)| *tag);
        let num_tables = u16::try_from(tables.len()).unwrap();
        let max_pow2 = num_tables.ilog2();
        let search_range = (1u16 << max_pow2) * 16;
        let entry_selector = u16::try_from(max_pow2).unwrap();
        let range_shift = num_tables * 16 - search_range;

        let dir_len = 12 + tables.len() * 16;
        let mut data_offset = base_offset + dir_len;
        let mut out = Vec::new();
        push_u32(&mut out, 0x0001_0000);
        push_u16(&mut out, num_tables);
        push_u16(&mut out, search_range);
        push_u16(&mut out, entry_selector);
        push_u16(&mut out, range_shift);
        let mut body = Vec::new();
        for (tag, bytes) in tables {
            out.extend_from_slice(tag);
            push_u32(&mut out, 0); // checksum (not checked by skrifa)
            push_u32(&mut out, u32::try_from(data_offset).unwrap());
            push_u32(&mut out, u32::try_from(bytes.len()).unwrap());
            body.extend_from_slice(&bytes);
            while !body.len().is_multiple_of(4) {
                body.push(0);
            }
            data_offset = base_offset + dir_len + body.len();
        }
        out.extend_from_slice(&body);
        out
    }

    fn build_ttc(specs: &[FaceSpec<'_>]) -> Vec<u8> {
        let header_len = 12 + specs.len() * 4;
        let mut out = Vec::new();
        out.extend_from_slice(b"ttcf");
        push_u32(&mut out, 0x0001_0000);
        push_u32(&mut out, u32::try_from(specs.len()).unwrap());
        let mut offset = header_len;
        let mut faces = Vec::new();
        for spec in specs {
            let sfnt = build_sfnt_at(spec, offset);
            push_u32(&mut out, u32::try_from(offset).unwrap());
            offset += sfnt.len();
            faces.push(sfnt);
        }
        for sfnt in faces {
            out.extend_from_slice(&sfnt);
        }
        out
    }

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(label: &str) -> Self {
            static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("deckcraft-ui-fallback-test-{}-{n}-{label}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn korean_interface_uses_korean_first_system_font_candidates() {
        let names = candidates(UiCjk::Korean);
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
        assert_eq!(probe(UiCjk::Korean), '한');
    }

    #[test]
    fn simplified_chinese_uses_simplified_probe_and_candidates() {
        assert_eq!(probe(UiCjk::Hans), '页');
        let names = candidates(UiCjk::Hans);
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
    fn traditional_chinese_uses_a_traditional_probe_and_chinese_candidates() {
        assert_eq!(probe(UiCjk::Hant), '頁');
        let names = candidates(UiCjk::Hant);
        let zh = candidates(UiCjk::Hans);
        // Chinese faces only: no Japanese or Korean candidate (those lead the other modes).
        assert!(zh.iter().all(|n| names.contains(n)));
        assert!(names.len() <= zh.len() + 3);
        if cfg!(windows) {
            assert_eq!(names.first(), Some(&"msjh.ttc"));
        }
    }

    #[test]
    fn pick_face_skips_faces_without_outline_tables_and_unmapped_faces() {
        let no_outlines = build_sfnt_at(&FaceSpec { family: "HvglOnly", style: "Regular", chars: &['한', '글'], outline_table: None }, 0);
        assert_eq!(pick_face(&no_outlines, &['한'], UiCjk::Korean), None);

        let unmapped = build_sfnt_at(&FaceSpec { family: "HasGlyphs", style: "Regular", chars: &['あ'], outline_table: Some(b"glyf") }, 0);
        assert_eq!(pick_face(&unmapped, &['한'], UiCjk::Korean), None);

        let mixed = build_ttc(&[
            FaceSpec { family: "NoOutlines", style: "Regular", chars: &['한', '글'], outline_table: None },
            FaceSpec { family: "RealOutlines", style: "Regular", chars: &['한'], outline_table: Some(b"glyf") },
        ]);
        assert_eq!(pick_face(&mixed, &['한', '글'], UiCjk::Korean), Some((1, "RealOutlines".into(), 1)));
    }

    #[test]
    fn pick_face_prefers_regional_chinese_family_and_regular_style() {
        let pan_cjk = build_ttc(&[
            FaceSpec { family: "PanCJK TC", style: "Regular", chars: &['页', '頁', '中'], outline_table: Some(b"glyf") },
            FaceSpec { family: "PanCJK SC", style: "Bold", chars: &['页', '頁', '中'], outline_table: Some(b"glyf") },
            FaceSpec { family: "PanCJK SC", style: "Regular", chars: &['页', '頁', '中'], outline_table: Some(b"glyf") },
        ]);
        assert_eq!(pick_face(&pan_cjk, &['页', '中'], UiCjk::Hans), Some((2, "PanCJK SC".into(), 2)));
        assert_eq!(pick_face(&pan_cjk, &['頁', '中'], UiCjk::Hant), Some((0, "PanCJK TC".into(), 2)));
    }

    #[test]
    fn pick_face_rejects_damaged_or_truncated_inputs_without_panicking() {
        for bad in [&[][..], b"OT", b"NOTF\0\0\0\0", b"ttcf\0\x01\0\0\0\0\0\x02", b"\0\x01\0\0\0\x05"] {
            assert_eq!(pick_face(bad, &['한'], UiCjk::Korean), None);
        }
        let mut truncated = build_ttc(&[FaceSpec { family: "Test", style: "Regular", chars: &['한'], outline_table: Some(b"glyf") }]);
        truncated.truncate(20);
        assert_eq!(pick_face(&truncated, &['한'], UiCjk::Korean), None);
    }

    #[test]
    fn find_ui_font_picks_best_coverage_skips_zero_and_reuses_path_cache() {
        let dir = TempDir::new("find");
        let zero = build_sfnt_at(&FaceSpec { family: "Zero", style: "Regular", chars: &['あ'], outline_table: Some(b"glyf") }, 0);
        let partial = build_sfnt_at(&FaceSpec { family: "Partial", style: "Regular", chars: &['한'], outline_table: Some(b"glyf") }, 0);
        let ttc = build_ttc(&[
            FaceSpec { family: "Dual SC", style: "Regular", chars: &['한', '글', '页'], outline_table: Some(b"glyf") },
            FaceSpec { family: "Dual TC", style: "Regular", chars: &['한', '글', '頁'], outline_table: Some(b"glyf") },
        ]);
        std::fs::write(dir.0.join("zero.ttf"), &zero).unwrap();
        std::fs::write(dir.0.join("partial.ttf"), &partial).unwrap();
        std::fs::write(dir.0.join("dual.ttc"), &ttc).unwrap();

        let mut cache: HashMap<std::path::PathBuf, Arc<Vec<u8>>> = HashMap::new();
        // A file mapping none of `needed` is never chosen.
        assert!(find_ui_font(std::slice::from_ref(&dir.0), &["zero.ttf"], &['한'], UiCjk::Korean, &mut cache, usize::MAX).is_none());
        assert!(cache.is_empty());

        // `partial.ttf` maps 1 of 2; `dual.ttc` maps 2 of 2, so `dual.ttc` wins.
        let picked_hans =
            find_ui_font(std::slice::from_ref(&dir.0), &["zero.ttf", "partial.ttf", "dual.ttc"], &['한', '页'], UiCjk::Hans, &mut cache, usize::MAX)
                .unwrap();
        assert_eq!((picked_hans.family.as_str(), picked_hans.index), ("Dual SC", 0));
        assert_eq!(cache.len(), 1);

        // Even with zero remaining new-byte budget and the file removed from disk, `Hant` hits the
        // same cached `dual.ttc` bytes and resolves the Traditional face index.
        std::fs::remove_file(dir.0.join("dual.ttc")).unwrap();
        let picked_hant = find_ui_font(std::slice::from_ref(&dir.0), &["dual.ttc"], &['한', '頁'], UiCjk::Hant, &mut cache, 0).unwrap();
        assert_eq!((picked_hant.family.as_str(), picked_hant.index), ("Dual TC", 1));
        assert!(Arc::ptr_eq(&picked_hans.bytes, &picked_hant.bytes));

        // New files exceeding `max_new_bytes` are skipped.
        assert!(find_ui_font(std::slice::from_ref(&dir.0), &["partial.ttf"], &['한'], UiCjk::Korean, &mut cache, 8).is_none());
    }

    #[test]
    fn ui_uncovered_cjk_returns_only_unmapped_cjk_characters() {
        let font = build_sfnt_at(&FaceSpec { family: "Tiny", style: "Regular", chars: &['한', '글'], outline_table: Some(b"glyf") }, 0);
        let leaked = Box::leak(font.into_boxed_slice());
        let embedded = [CraftFont { family: "Tiny", style: "Regular", scripts: &["Kore"], bytes: leaked }];
        let uncovered = ui_uncovered_cjk(&["ABC 한글 홈 123"], &[&embedded[0]]);
        assert_eq!(uncovered, vec!['홈']);
    }
}
