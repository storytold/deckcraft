//! Interface language for the application shell.
//!
//! The catalog covers the ribbon tabs, the File (Backstage) menu, the title bar, the status bar
//! and the Preferences dialog. Ribbon groups, the other dialogs, command ids and presentation text
//! stay in English. English is the source: every key has an English row, and a key without a
//! translation falls back to it.
//!
//! Traditional Chinese here uses Taiwan vocabulary and was written from the English meanings.
//! It is not copied from another product's localization, and there is no Simplified Chinese
//! catalog: a Simplified locale or code stays in English until the user picks a language.

use std::sync::OnceLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in `ui.json`. `System` is resolved against the operating system each time
/// the shell is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanguageChoice {
    #[default]
    System,
    English,
    TraditionalChinese,
}

impl LanguageChoice {
    /// The picker's order: the automatic choice first, then the named languages.
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::English, Self::TraditionalChinese];

    /// The code saved to `ui.json` and taken by `app.language`.
    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "en",
            Self::TraditionalChinese => "zh-Hant",
        }
    }

    /// A saved or requested code, or an operating-system tag. `None` means the value is unknown.
    ///
    /// Traditional Chinese tags (`zh-Hant`, `zh-TW`, `zh-HK`, `zh-MO`, with optional region and
    /// encoding suffixes) select [`Self::TraditionalChinese`]. `zh-Hans`, `zh-CN`, `zh-SG` and
    /// bare `zh` select [`Self::English`]: there is no Simplified catalog. Other tags select
    /// nothing, so a saved value can fall back to [`Self::System`].
    pub fn from_tag(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag.is_empty() {
            return None;
        }
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::English);
        }
        if is_traditional(&tag) {
            return Some(Self::TraditionalChinese);
        }
        if is_simplified(&tag) {
            return Some(Self::English);
        }
        None
    }

    /// The language to draw with. `System` uses `system_tag`; when that is missing (the web
    /// build, or a test) it stays English.
    pub fn resolve(self, system_tag: Option<&str>) -> Resolved {
        self.resolve_with(system_tag, choice_available(self))
    }

    /// The resolver with the build's availability passed in, so the web fallback is testable on
    /// native builds too.
    fn resolve_with(self, system_tag: Option<&str>, available: bool) -> Resolved {
        let resolved = match self {
            Self::English => Resolved::English,
            Self::TraditionalChinese => Resolved::TraditionalChinese,
            Self::System => system_tag.map(resolve_tag).unwrap_or(Resolved::English),
        };
        // The web build has no installed fonts and bundles no CJK face: a saved or explicit
        // Traditional Chinese choice must not draw missing-glyph boxes there.
        if resolved == Resolved::TraditionalChinese && !available { Resolved::English } else { resolved }
    }
}

impl Serialize for LanguageChoice {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for LanguageChoice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_tag(&raw).unwrap_or(Self::System))
    }
}

/// The catalog actually used for drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    English,
    TraditionalChinese,
}

impl Resolved {
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::TraditionalChinese => "zh-Hant",
        }
    }
}

/// Map an operating-system locale to a catalog. Only Traditional Chinese tags leave English.
pub fn resolve_tag(tag: &str) -> Resolved {
    if is_traditional(&normalize(tag)) { Resolved::TraditionalChinese } else { Resolved::English }
}

/// The operating system's locale tag, read once. The web build has no desktop locale here, so
/// `System` stays English there until the user picks a language explicitly.
pub fn system_tag() -> Option<String> {
    static TAG: OnceLock<Option<String>> = OnceLock::new();
    TAG.get_or_init(read_system_tag).clone()
}

#[cfg(all(not(target_arch = "wasm32"), not(test)))]
fn read_system_tag() -> Option<String> {
    sys_locale::get_locale().filter(|tag| !tag.trim().is_empty())
}

#[cfg(any(target_arch = "wasm32", test))]
fn read_system_tag() -> Option<String> {
    // Tests pin `System` to English so an operating system set to Traditional Chinese does not
    // rename the labels existing UI tests look up. The tag mapping has its own tests.
    None
}

fn normalize(raw: &str) -> String {
    let trimmed = raw.trim();
    let head = trimmed.split(['.', '@']).next().unwrap_or(trimmed);
    head.replace('_', "-").to_ascii_lowercase()
}

fn is_traditional(tag: &str) -> bool {
    matches!(tag, "zh-hant" | "zh-tw" | "zh-hk" | "zh-mo")
        || tag.starts_with("zh-hant-")
        || tag.starts_with("zh-tw-")
        || tag.starts_with("zh-hk-")
        || tag.starts_with("zh-mo-")
}

fn is_simplified(tag: &str) -> bool {
    matches!(tag, "zh" | "zh-hans" | "zh-cn" | "zh-sg") || tag.starts_with("zh-hans-") || tag.starts_with("zh-cn-") || tag.starts_with("zh-sg-")
}

/// Whether the Preferences picker offers `choice` in this build.
///
/// The web build bundles no CJK face (the asset policy excludes Noto CJK and Source Han), and it
/// has no installed fonts to borrow, so Traditional Chinese could only draw boxes there. The
/// entry stays visible but disabled, in English. Desktop builds offer all three choices.
pub fn choice_available(choice: LanguageChoice) -> bool {
    !cfg!(target_arch = "wasm32") || choice != LanguageChoice::TraditionalChinese
}

/// The label of a picker choice. Language names stay in their own language where the build can
/// draw them; the web build names the unavailable one in English.
pub fn choice_label(lang: Resolved, choice: LanguageChoice) -> &'static str {
    match choice {
        LanguageChoice::System => text(lang, "pref.system"),
        LanguageChoice::English => "English",
        LanguageChoice::TraditionalChinese => {
            if cfg!(target_arch = "wasm32") {
                "Traditional Chinese"
            } else {
                "繁體中文"
            }
        }
    }
}

/// `(key, English, Traditional Chinese)`. Keys are sorted for the binary search in [`text`].
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.about", "About", "關於"),
    ("nav.back", "Back", "返回"),
    ("nav.close", "Close", "關閉"),
    ("nav.export", "Export", "匯出"),
    ("nav.home", "Home", "首頁"),
    ("nav.info", "Info", "資訊"),
    ("nav.new", "New", "新增"),
    ("nav.open", "Open", "開啟"),
    ("nav.options", "Options", "選項"),
    ("nav.print", "Print", "列印"),
    ("nav.save", "Save", "儲存"),
    ("nav.save_as", "Save As", "另存新檔"),
    ("pref.author", "Your name (comments):", "您的名稱（註解）："),
    ("pref.autocorrect", "AutoCorrect as you type", "輸入時自動校正"),
    ("pref.autocorrect_options", "AutoCorrect Options…", "自動校正選項…"),
    ("pref.cancel", "Cancel", "取消"),
    ("pref.centimeters", "Centimeters", "公分"),
    ("pref.dark", "Dark appearance", "深色外觀"),
    ("pref.done", "Done", "完成"),
    ("pref.editing", "Editing", "編輯"),
    ("pref.general", "General", "一般"),
    ("pref.inches", "Inches", "英吋"),
    ("pref.language", "Interface language", "介面語言"),
    ("pref.measurement_units", "Measurement units:", "量測單位："),
    ("pref.quotes", "Replace straight quotes with smart quotes", "以智慧引號取代直引號"),
    ("pref.size", "Interface size", "介面大小"),
    ("pref.smart_guides", "Show smart guides", "顯示智慧參考線"),
    ("pref.snap", "Snap objects to grid", "將物件對齊格線"),
    ("pref.system", "System", "跟隨系統"),
    ("pref.title", "Preferences", "偏好設定"),
    ("status.comments", "Comments", "註解"),
    ("status.fit", "Fit slide to current window", "投影片符合視窗大小"),
    ("status.language", "Language", "語言"),
    ("status.normal", "Normal", "標準"),
    ("status.not_checked", " (not checked)", "（未檢查）"),
    ("status.notes", "Notes", "備忘稿"),
    ("status.reading", "Reading View", "閱讀模式"),
    ("status.slide_master", "Slide Master", "投影片母片"),
    ("status.slide_of", "Slide {n} of {m}", "第 {n} 張，共 {m} 張"),
    ("status.slideshow", "Slide Show", "投影片放映"),
    ("status.sorter", "Slide Sorter", "投影片瀏覽"),
    ("status.zoom", "Zoom", "縮放"),
    ("status.zoom_in", "Zoom In", "放大"),
    ("status.zoom_level", "Zoom level", "縮放比例"),
    ("status.zoom_out", "Zoom Out", "縮小"),
    ("tab.animations", "Animations", "動畫"),
    ("tab.chart_design", "Chart Design", "圖表設計"),
    ("tab.design", "Design", "設計"),
    ("tab.draw", "Draw", "繪圖"),
    ("tab.file", "File", "檔案"),
    ("tab.handout_master", "Handout Master", "講義母片"),
    ("tab.home", "Home", "常用"),
    ("tab.insert", "Insert", "插入"),
    ("tab.layout", "Layout", "版面配置"),
    ("tab.notes_master", "Notes Master", "備忘稿母片"),
    ("tab.picture_format", "Picture Format", "圖片格式"),
    ("tab.playback", "Playback", "播放"),
    ("tab.record", "Record", "錄製"),
    ("tab.review", "Review", "校閱"),
    ("tab.shape_format", "Shape Format", "圖案格式"),
    ("tab.slide_master", "Slide Master", "投影片母片"),
    ("tab.slideshow", "Slide Show", "投影片放映"),
    ("tab.smartart_design", "SmartArt Design", "SmartArt 設計"),
    ("tab.table_design", "Table Design", "表格設計"),
    ("tab.transitions", "Transitions", "轉場"),
    ("tab.view", "View", "檢視"),
    ("title.appearance", "Light/Dark appearance", "淺色/深色外觀"),
    ("title.comments", "Comments", "註解"),
    ("title.edited", "Edited", "已編輯"),
    ("title.present", "Present", "放映"),
    ("title.search", "Search commands", "搜尋命令"),
];

/// The shell string `key` in `lang`. An unknown key is returned unchanged, so a missed call site
/// shows its key rather than a blank label.
pub fn text(lang: Resolved, key: &'static str) -> &'static str {
    match CATALOG.binary_search_by(|row| row.0.cmp(key)) {
        Ok(i) => CATALOG.get(i).map_or(key, |row| match lang {
            Resolved::English => row.1,
            Resolved::TraditionalChinese => row.2,
        }),
        Err(_) => key,
    }
}

/// `"{label} ({shortcut})"`: the shortcut chord stays in Latin.
pub fn with_shortcut(lang: Resolved, key: &'static str, shortcut: &str) -> String {
    format!("{} ({shortcut})", text(lang, key))
}

/// Replace `{name}` placeholders in `template`. A name missing from `vars` is left as written.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(rest.get(..start).unwrap_or(""));
        let after = rest.get(start + 1..).unwrap_or("");
        let Some(end) = after.find('}') else {
            out.push_str(rest.get(start..).unwrap_or(""));
            return out;
        };
        let name = after.get(..end).unwrap_or("");
        match vars.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => out.push_str(value),
            None => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = after.get(end + 1..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// A ribbon tab's visible title from the English name stored in [`crate::UiState::tab`]. Contextual
/// tab names are catalogued too; an unknown name is returned unchanged.
pub fn ribbon_tab(lang: Resolved, name: &'static str) -> &'static str {
    let key = match name {
        "File" => "tab.file",
        "Home" => "tab.home",
        "Insert" => "tab.insert",
        "Draw" => "tab.draw",
        "Design" => "tab.design",
        "Transitions" => "tab.transitions",
        "Animations" => "tab.animations",
        "Slide Show" => "tab.slideshow",
        "Record" => "tab.record",
        "Review" => "tab.review",
        "View" => "tab.view",
        "Handout Master" => "tab.handout_master",
        "Notes Master" => "tab.notes_master",
        "Slide Master" => "tab.slide_master",
        "Shape Format" => "tab.shape_format",
        "Picture Format" => "tab.picture_format",
        "Table Design" => "tab.table_design",
        "Layout" => "tab.layout",
        "SmartArt Design" => "tab.smartart_design",
        "Chart Design" => "tab.chart_design",
        "Playback" => "tab.playback",
        _ => return name,
    };
    text(lang, key)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// The placeholders in a template, in order, without their braces.
    fn placeholders(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let after = rest.get(start + 1..).unwrap_or("");
            let Some(end) = after.find('}') else { break };
            out.push(after.get(..end).unwrap_or(""));
            rest = after.get(end + 1..).unwrap_or("");
        }
        out
    }

    #[test]
    fn the_catalog_has_unique_sorted_keys_and_keeps_placeholders() {
        let mut previous = "";
        for (key, english, traditional) in CATALOG {
            assert!(*key > previous, "{key} is out of order after {previous}");
            previous = key;
            assert!(!english.is_empty(), "{key} has no English source");
            assert!(!traditional.is_empty(), "{key} has no Traditional Chinese text");
            assert_eq!(placeholders(english), placeholders(traditional), "{key} changes its placeholders");
        }
        // The shell is deliberately small; a large drop means a call site lost its row.
        assert!(CATALOG.len() >= 70, "the shell catalog shrank to {} strings", CATALOG.len());
    }

    #[test]
    fn traditional_locales_resolve_and_simplified_stays_english() {
        for tag in ["zh-Hant", "zh-TW", "zh-HK", "zh-MO", "zh_TW.UTF-8", "zh-Hant-HK", "ZH-tw", "zh-HK-mac"] {
            assert_eq!(resolve_tag(tag), Resolved::TraditionalChinese, "{tag}");
        }
        for tag in ["zh", "zh-Hans", "zh-CN", "zh-SG", "zh_CN.UTF-8", "ja-JP", "ko-KR", "en-US", ""] {
            assert_eq!(resolve_tag(tag), Resolved::English, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("zh-TW")), Resolved::TraditionalChinese);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-HK")), Resolved::TraditionalChinese);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-MO")), Resolved::TraditionalChinese);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-Hant")), Resolved::TraditionalChinese);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-Hans")), Resolved::English);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-CN")), Resolved::English);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::English);
        assert_eq!(LanguageChoice::English.resolve(Some("zh-TW")), Resolved::English);
        assert_eq!(LanguageChoice::TraditionalChinese.resolve(None), Resolved::TraditionalChinese);
    }

    #[test]
    fn an_unavailable_traditional_choice_falls_back_to_english() {
        assert_eq!(LanguageChoice::TraditionalChinese.resolve_with(None, false), Resolved::English);
        assert_eq!(LanguageChoice::TraditionalChinese.resolve_with(None, true), Resolved::TraditionalChinese);
    }

    #[test]
    fn the_language_command_accepts_its_documented_codes_and_locale_aliases() {
        for (code, choice) in [
            ("system", LanguageChoice::System),
            ("en", LanguageChoice::English),
            ("en-GB", LanguageChoice::English),
            ("zh-Hant", LanguageChoice::TraditionalChinese),
            ("zh-TW", LanguageChoice::TraditionalChinese),
            ("zh-HK", LanguageChoice::TraditionalChinese),
            ("zh-MO", LanguageChoice::TraditionalChinese),
        ] {
            assert_eq!(LanguageChoice::from_tag(code), Some(choice), "{code}");
        }
        // There is no Simplified catalog, so an explicit Simplified tag stays English.
        assert_eq!(LanguageChoice::from_tag("zh-Hans"), Some(LanguageChoice::English));
        assert_eq!(LanguageChoice::from_tag("zh-CN"), Some(LanguageChoice::English));
        assert_eq!(LanguageChoice::from_tag("ja"), None);
        assert_eq!(LanguageChoice::from_tag(""), None);
    }

    #[test]
    fn a_saved_choice_round_trips_and_an_unknown_value_falls_back_to_system() {
        let saved = serde_json::to_string(&LanguageChoice::TraditionalChinese).unwrap();
        assert_eq!(saved, "\"zh-Hant\"");
        let back: LanguageChoice = serde_json::from_str(&saved).unwrap();
        assert_eq!(back, LanguageChoice::TraditionalChinese);
        let simplified: LanguageChoice = serde_json::from_str("\"zh-CN\"").unwrap();
        assert_eq!(simplified, LanguageChoice::English, "an explicit Hans code stays English");
        let unknown: LanguageChoice = serde_json::from_str("\"zh-XX\"").unwrap();
        assert_eq!(unknown, LanguageChoice::System, "an unknown saved value falls back to System");
    }

    #[test]
    fn the_slide_counter_keeps_named_placeholders() {
        let traditional = text(Resolved::TraditionalChinese, "status.slide_of");
        assert_eq!(fill(traditional, &[("n", "2"), ("m", "9")]), "第 2 張，共 9 張");
        let english = text(Resolved::English, "status.slide_of");
        assert_eq!(fill(english, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    fn ribbons_and_shortcuts_follow_the_language() {
        assert_eq!(ribbon_tab(Resolved::TraditionalChinese, "File"), "檔案");
        assert_eq!(ribbon_tab(Resolved::TraditionalChinese, "Slide Show"), "投影片放映");
        assert_eq!(ribbon_tab(Resolved::English, "Slide Show"), "Slide Show");
        assert_eq!(ribbon_tab(Resolved::TraditionalChinese, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::TraditionalChinese, "tab.file", "Alt+F"), "檔案 (Alt+F)");
    }
}
