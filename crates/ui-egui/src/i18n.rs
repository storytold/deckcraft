//! Interface language for the application shell.
//!
//! The ribbon tabs, File menu, title-bar controls, status bar and Preferences read this catalog.
//! Ribbon groups, the other dialogs and command ids stay in English. Traditional Chinese, in
//! Taiwan vocabulary, is the translation here. Strings are written from the meaning of the English
//! label. They are not copied from another product's localization.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in preferences. `System` is resolved when the interface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanguageChoice {
    #[default]
    System,
    En,
    ZhHant,
}

impl LanguageChoice {
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::En, Self::ZhHant];

    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::ZhHant => "zh-Hant",
        }
    }

    /// `system`, `en` (and `en-*`), or `zh-Hant` / `zh-TW` / `zh-HK` / `zh-MO`.
    pub fn parse(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::En);
        }
        if is_traditional(&tag) {
            return Some(Self::ZhHant);
        }
        None
    }

    pub fn resolve(self, system: Option<&str>) -> Resolved {
        match self {
            Self::En => Resolved::En,
            Self::ZhHant => Resolved::ZhHant,
            Self::System => system.map(resolve_tag).unwrap_or(Resolved::En),
        }
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
        Ok(Self::parse(&raw).unwrap_or(Self::System))
    }
}

/// The language actually used to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    En,
    ZhHant,
}

impl Resolved {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhHant => "zh-Hant",
        }
    }
}

/// Map an OS locale tag to a catalog. Unknown tags, including Simplified Chinese, are English.
pub fn resolve_tag(tag: &str) -> Resolved {
    let tag = normalize(tag);
    if is_traditional(&tag) {
        return Resolved::ZhHant;
    }
    Resolved::En
}

/// `zh-Hant` (and `zh-Hant-*`), plus the Traditional locales `zh-TW`, `zh-HK` and `zh-MO`.
fn is_traditional(tag: &str) -> bool {
    tag == "zh-hant"
        || tag.starts_with("zh-hant-")
        || tag == "zh-tw"
        || tag.starts_with("zh-tw-")
        || tag == "zh-hk"
        || tag.starts_with("zh-hk-")
        || tag == "zh-mo"
        || tag.starts_with("zh-mo-")
}

fn normalize(raw: &str) -> String {
    let trimmed = raw.trim();
    let head = trimmed.split(['.', '@']).next().unwrap_or(trimmed);
    head.replace('_', "-").to_ascii_lowercase()
}

/// The operating system's locale tag, read once. The web build has no system tag here, so
/// `System` stays English until the user picks a language.
pub fn system_tag() -> Option<String> {
    static CACHED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    CACHED.get_or_init(read_system_tag).clone()
}

fn read_system_tag() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    let tag = sys_locale::get_locale();
    #[cfg(target_arch = "wasm32")]
    let tag: Option<String> = None;
    // Tests pin System to English. A machine whose OS language is Traditional Chinese would
    // otherwise rename the labels the UI tests look up. Tag mapping is covered on its own.
    #[cfg(test)]
    {
        let _ = tag;
        None
    }
    #[cfg(not(test))]
    tag
}

/// `(id, English, Traditional Chinese)`. Ids are sorted so lookup can binary-search.
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.account", "Account", "帳戶"),
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
    ("pref.author", "Your name (comments):", "你的姓名（註解）："),
    ("pref.autocorrect", "AutoCorrect as you type", "輸入時自動校正"),
    ("pref.cancel", "Cancel", "取消"),
    ("pref.dark", "Dark appearance", "深色外觀"),
    ("pref.done", "Done", "完成"),
    ("pref.editing", "Editing", "編輯"),
    ("pref.english", "English", "English"),
    ("pref.general", "General", "一般"),
    ("pref.language", "Interface language", "介面語言"),
    ("pref.quotes", "Replace straight quotes with smart quotes", "將直引號換成彎引號"),
    ("pref.size", "Interface size", "介面大小"),
    ("pref.smart_guides", "Show smart guides", "顯示智慧參考線"),
    ("pref.snap", "Snap objects to grid", "將物件對齊格線"),
    ("pref.system", "System", "跟隨系統"),
    ("pref.title", "Preferences", "偏好設定"),
    ("pref.zh_hant", "繁體中文", "繁體中文"),
    ("status.comments", "Comments", "註解"),
    ("status.fit", "Fit slide to current window", "讓投影片符合目前視窗"),
    ("status.normal", "Normal", "標準"),
    ("status.notes", "Notes", "備忘稿"),
    ("status.reading", "Reading View", "閱讀檢視"),
    ("status.slide_master", "Slide Master", "投影片母片"),
    ("status.slide_of", "Slide {n} of {m}", "投影片 {n}/{m}"),
    ("status.slideshow", "Slide Show", "投影片放映"),
    ("status.sorter", "Slide Sorter", "投影片瀏覽"),
    ("status.zoom_in", "Zoom In", "放大"),
    ("status.zoom_out", "Zoom Out", "縮小"),
    ("tab.animations", "Animations", "動畫"),
    ("tab.chart_design", "Chart Design", "圖表設計"),
    ("tab.design", "Design", "設計"),
    ("tab.draw", "Draw", "繪圖"),
    ("tab.file", "File", "檔案"),
    ("tab.home", "Home", "常用"),
    ("tab.insert", "Insert", "插入"),
    ("tab.layout", "Layout", "版面配置"),
    ("tab.picture_format", "Picture Format", "圖片格式"),
    ("tab.playback", "Playback", "播放"),
    ("tab.record", "Record", "錄製"),
    ("tab.review", "Review", "校閱"),
    ("tab.shape_format", "Shape Format", "圖形格式"),
    ("tab.slide_master", "Slide Master", "投影片母片"),
    ("tab.slideshow", "Slide Show", "投影片放映"),
    ("tab.table_design", "Table Design", "表格設計"),
    ("tab.transitions", "Transitions", "轉場"),
    ("tab.view", "View", "檢視"),
    ("title.appearance", "Light/Dark appearance", "淺色/深色外觀"),
    ("title.comments", "Comments", "註解"),
    ("title.edited", "Edited", "已編輯"),
    ("title.home", "Home", "常用"),
    ("title.play", "Play from Start", "從頭放映"),
    ("title.present", "Present", "放映"),
    ("title.redo", "Redo", "重做"),
    ("title.save", "Save", "儲存"),
    ("title.search", "Search commands", "搜尋命令"),
    ("title.undo", "Undo", "復原"),
];

/// The shell string `id` in `lang`. An unknown id is returned unchanged so a missed call site
/// shows up as the id rather than a blank label.
pub fn text(lang: Resolved, id: &'static str) -> &'static str {
    match CATALOG.binary_search_by_key(&id, |row| row.0) {
        Ok(i) => CATALOG
            .get(i)
            .map(|row| match lang {
                Resolved::En => row.1,
                Resolved::ZhHant => row.2,
            })
            .unwrap_or(id),
        Err(_) => id,
    }
}

/// `"{name} ({shortcut})"`, for tooltips that keep the key chord in Latin.
pub fn with_shortcut(lang: Resolved, id: &'static str, shortcut: &str) -> String {
    format!("{} ({shortcut})", text(lang, id))
}

/// Replace `{name}` placeholders. A name that is not in `vars` is left as written.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = rest.get(start + 1..).unwrap_or("");
        if let Some(end) = after.find('}') {
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
        } else {
            out.push_str(rest.get(start..).unwrap_or(""));
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// Ribbon and contextual tab titles. `english_name` is the id stored in [`crate::UiState::tab`].
pub fn tab_label(lang: Resolved, english_name: &'static str) -> &'static str {
    let id = match english_name {
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
        "Slide Master" => "tab.slide_master",
        "Shape Format" => "tab.shape_format",
        "Picture Format" => "tab.picture_format",
        "Table Design" => "tab.table_design",
        "Layout" => "tab.layout",
        "Chart Design" => "tab.chart_design",
        "Playback" => "tab.playback",
        _ => return english_name,
    };
    text(lang, id)
}

pub fn choice_label(lang: Resolved, choice: LanguageChoice) -> &'static str {
    let id = match choice {
        LanguageChoice::System => "pref.system",
        LanguageChoice::En => "pref.english",
        LanguageChoice::ZhHant => "pref.zh_hant",
    };
    text(lang, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holes(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            rest = rest.get(start + 1..).unwrap_or("");
            let Some(end) = rest.find('}') else { break };
            out.push(rest.get(..end).unwrap_or(""));
            rest = rest.get(end + 1..).unwrap_or("");
        }
        out
    }

    #[test]
    fn the_shell_catalog_is_sorted_and_complete() {
        let mut previous = "";
        for (id, en, zh) in CATALOG {
            assert!(*id > previous, "{id} is out of order after {previous}");
            previous = id;
            assert!(!en.is_empty() && !zh.is_empty(), "{id}");
            assert_eq!(holes(en), holes(zh), "{id}");
            let same = ["pref.english", "pref.zh_hant"];
            if en == zh {
                assert!(same.contains(id), "{id} has no Traditional Chinese text");
            }
        }
        assert!(CATALOG.len() >= 60, "the shell catalog shrank");
    }

    #[test]
    fn traditional_tags_resolve_and_simplified_tags_do_not() {
        for tag in ["zh-Hant", "zh-TW", "zh-HK", "zh-MO", "zh_TW.UTF-8", "zh-Hant-HK", "ZH-tw", "zh-HK-mac"] {
            assert_eq!(resolve_tag(tag), Resolved::ZhHant, "{tag}");
        }
        for tag in ["zh", "zh-Hans", "zh-CN", "zh-SG", "zh_CN.UTF-8", "ja-JP", "ko-KR", "en-US", ""] {
            assert_eq!(resolve_tag(tag), Resolved::En, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("zh-TW")), Resolved::ZhHant);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-HK")), Resolved::ZhHant);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-MO")), Resolved::ZhHant);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-CN")), Resolved::En);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::En);
        assert_eq!(LanguageChoice::En.resolve(Some("zh-TW")), Resolved::En);
        assert_eq!(LanguageChoice::ZhHant.resolve(None), Resolved::ZhHant);
    }

    #[test]
    fn the_language_command_accepts_the_three_choices() {
        assert_eq!(LanguageChoice::parse("system"), Some(LanguageChoice::System));
        assert_eq!(LanguageChoice::parse("en"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("en-GB"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("zh-Hant"), Some(LanguageChoice::ZhHant));
        assert_eq!(LanguageChoice::parse("zh-TW"), Some(LanguageChoice::ZhHant));
        assert_eq!(LanguageChoice::parse("zh-HK"), Some(LanguageChoice::ZhHant));
        assert_eq!(LanguageChoice::parse("zh-MO"), Some(LanguageChoice::ZhHant));
        assert_eq!(LanguageChoice::parse("zh-Hans"), None);
        assert_eq!(LanguageChoice::parse("zh-CN"), None);
        assert_eq!(LanguageChoice::parse("ja"), None);
    }

    #[test]
    fn a_saved_choice_round_trips_and_an_unknown_code_falls_back() {
        let raw = serde_json::to_string(&LanguageChoice::ZhHant).expect("serialize");
        assert_eq!(raw, "\"zh-Hant\"");
        let back: LanguageChoice = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, LanguageChoice::ZhHant);
        let unknown: LanguageChoice = serde_json::from_str("\"zh-Hans\"").expect("deserialize");
        assert_eq!(unknown, LanguageChoice::System);
    }

    #[test]
    fn placeholders_keep_their_names() {
        let zh = text(Resolved::ZhHant, "status.slide_of");
        assert_eq!(fill(zh, &[("n", "2"), ("m", "9")]), "投影片 2/9");
        let en = text(Resolved::En, "status.slide_of");
        assert_eq!(fill(en, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    fn tab_titles_follow_the_language_and_keep_the_english_id() {
        assert_eq!(tab_label(Resolved::ZhHant, "File"), "檔案");
        assert_eq!(tab_label(Resolved::ZhHant, "Home"), "常用");
        assert_eq!(tab_label(Resolved::En, "Slide Show"), "Slide Show");
        assert_eq!(tab_label(Resolved::ZhHant, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::ZhHant, "tab.file", "Alt+F"), "檔案 (Alt+F)");
    }
}
