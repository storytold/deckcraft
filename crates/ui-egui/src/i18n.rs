//! Interface language for the application shell.
//!
//! The ribbon tabs, File menu, title-bar controls, status bar and Preferences read this catalog.
//! Ribbon groups, the other dialogs and command ids stay in English. Simplified Chinese is the
//! first translation; Traditional Chinese (`zh-Hant`, `zh-TW`, `zh-HK`, `zh-MO`) is a separate
//! catalog (#109) and stays English here.
//!
//! Strings are written from the meaning of the English label. They are not copied from another
//! product's localization.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in preferences. `System` is resolved when the interface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanguageChoice {
    #[default]
    System,
    En,
    ZhHans,
}

impl LanguageChoice {
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::En, Self::ZhHans];

    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::ZhHans => "zh-Hans",
        }
    }

    /// `system`, `en` (and `en-*`), or `zh-Hans` / `zh-CN` / `zh-SG`. Traditional Chinese tags
    /// are not accepted: this catalog does not cover them.
    pub fn parse(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::En);
        }
        if tag == "zh"
            || tag == "zh-hans"
            || tag.starts_with("zh-hans-")
            || tag == "zh-cn"
            || tag.starts_with("zh-cn-")
            || tag == "zh-sg"
            || tag.starts_with("zh-sg-")
        {
            return Some(Self::ZhHans);
        }
        None
    }

    pub fn resolve(self, system: Option<&str>) -> Resolved {
        match self {
            Self::En => Resolved::En,
            Self::ZhHans => Resolved::ZhHans,
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
    ZhHans,
}

impl Resolved {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhHans => "zh-Hans",
        }
    }
}

/// Map an OS locale tag to a catalog. Unknown tags, including Traditional Chinese, are English.
pub fn resolve_tag(tag: &str) -> Resolved {
    let tag = normalize(tag);
    if tag.starts_with("zh-hant") || tag.starts_with("zh-tw") || tag.starts_with("zh-hk") || tag.starts_with("zh-mo") {
        return Resolved::En;
    }
    if tag == "zh" || tag.starts_with("zh-hans") || tag.starts_with("zh-cn") || tag.starts_with("zh-sg") {
        return Resolved::ZhHans;
    }
    Resolved::En
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
    // Tests pin System to English. A machine whose OS language is Chinese would otherwise
    // rename the labels the UI tests look up. Tag mapping is covered on its own.
    #[cfg(test)]
    {
        let _ = tag;
        None
    }
    #[cfg(not(test))]
    tag
}

/// `(id, English, Simplified Chinese)`. Ids are sorted so lookup can binary-search.
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.account", "Account", "帐户"),
    ("nav.back", "Back", "返回"),
    ("nav.close", "Close", "关闭"),
    ("nav.export", "Export", "导出"),
    ("nav.home", "Home", "主页"),
    ("nav.info", "Info", "信息"),
    ("nav.new", "New", "新建"),
    ("nav.open", "Open", "打开"),
    ("nav.options", "Options", "选项"),
    ("nav.print", "Print", "打印"),
    ("nav.save", "Save", "保存"),
    ("nav.save_as", "Save As", "另存为"),
    ("pref.author", "Your name (comments):", "你的姓名（批注）："),
    ("pref.autocorrect", "AutoCorrect as you type", "键入时自动更正"),
    ("pref.cancel", "Cancel", "取消"),
    ("pref.dark", "Dark appearance", "深色外观"),
    ("pref.done", "Done", "完成"),
    ("pref.editing", "Editing", "编辑"),
    ("pref.english", "English", "English"),
    ("pref.general", "General", "常规"),
    ("pref.language", "Interface language", "界面语言"),
    ("pref.quotes", "Replace straight quotes with smart quotes", "将直引号替换为弯引号"),
    ("pref.size", "Interface size", "界面大小"),
    ("pref.smart_guides", "Show smart guides", "显示智能参考线"),
    ("pref.snap", "Snap objects to grid", "使对象对齐网格"),
    ("pref.system", "System", "跟随系统"),
    ("pref.title", "Preferences", "偏好设置"),
    ("pref.zh", "简体中文", "简体中文"),
    ("status.comments", "Comments", "批注"),
    ("status.fit", "Fit slide to current window", "使幻灯片适应当前窗口"),
    ("status.normal", "Normal", "普通"),
    ("status.notes", "Notes", "备注"),
    ("status.reading", "Reading View", "阅读视图"),
    ("status.slide_master", "Slide Master", "幻灯片母版"),
    ("status.slide_of", "Slide {n} of {m}", "幻灯片 {n}/{m}"),
    ("status.slideshow", "Slide Show", "幻灯片放映"),
    ("status.sorter", "Slide Sorter", "幻灯片浏览"),
    ("status.zoom_in", "Zoom In", "放大"),
    ("status.zoom_out", "Zoom Out", "缩小"),
    ("tab.animations", "Animations", "动画"),
    ("tab.chart_design", "Chart Design", "图表设计"),
    ("tab.design", "Design", "设计"),
    ("tab.draw", "Draw", "绘图"),
    ("tab.file", "File", "文件"),
    ("tab.home", "Home", "开始"),
    ("tab.insert", "Insert", "插入"),
    ("tab.layout", "Layout", "布局"),
    ("tab.picture_format", "Picture Format", "图片格式"),
    ("tab.playback", "Playback", "播放"),
    ("tab.record", "Record", "录制"),
    ("tab.review", "Review", "审阅"),
    ("tab.shape_format", "Shape Format", "形状格式"),
    ("tab.slide_master", "Slide Master", "幻灯片母版"),
    ("tab.slideshow", "Slide Show", "幻灯片放映"),
    ("tab.table_design", "Table Design", "表格设计"),
    ("tab.transitions", "Transitions", "切换"),
    ("tab.view", "View", "视图"),
    ("title.appearance", "Light/Dark appearance", "浅色/深色外观"),
    ("title.comments", "Comments", "批注"),
    ("title.edited", "Edited", "已编辑"),
    ("title.home", "Home", "开始"),
    ("title.play", "Play from Start", "从头放映"),
    ("title.present", "Present", "放映"),
    ("title.redo", "Redo", "重做"),
    ("title.save", "Save", "保存"),
    ("title.search", "Search commands", "搜索命令"),
    ("title.undo", "Undo", "撤销"),
];

/// The shell string `id` in `lang`. An unknown id is returned unchanged so a missed call site
/// shows up as the id rather than a blank label.
pub fn text(lang: Resolved, id: &'static str) -> &'static str {
    match CATALOG.binary_search_by_key(&id, |row| row.0) {
        Ok(i) => CATALOG
            .get(i)
            .map(|row| match lang {
                Resolved::En => row.1,
                Resolved::ZhHans => row.2,
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
        LanguageChoice::ZhHans => "pref.zh",
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
            let same = ["pref.english", "pref.zh"];
            if en == zh {
                assert!(same.contains(id), "{id} has no Chinese text");
            }
        }
        assert!(CATALOG.len() >= 60, "the shell catalog shrank");
    }

    #[test]
    fn simplified_chinese_tags_resolve_and_traditional_tags_do_not() {
        for tag in ["zh", "zh-Hans", "zh-Hans-CN", "zh-CN", "zh_CN.UTF-8", "zh-SG", "ZH-hans"] {
            assert_eq!(resolve_tag(tag), Resolved::ZhHans, "{tag}");
        }
        for tag in ["zh-Hant", "zh-TW", "zh-HK", "zh-MO", "zh_Hant_TW", "en-BY", "en-US", "ru-BY", "C", ""] {
            assert_eq!(resolve_tag(tag), Resolved::En, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("zh-CN")), Resolved::ZhHans);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-TW")), Resolved::En);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::En);
        assert_eq!(LanguageChoice::En.resolve(Some("zh-CN")), Resolved::En);
        assert_eq!(LanguageChoice::ZhHans.resolve(None), Resolved::ZhHans);
    }

    #[test]
    fn the_language_command_accepts_the_three_choices() {
        assert_eq!(LanguageChoice::parse("system"), Some(LanguageChoice::System));
        assert_eq!(LanguageChoice::parse("en"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("en-GB"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("zh-Hans"), Some(LanguageChoice::ZhHans));
        assert_eq!(LanguageChoice::parse("zh-CN"), Some(LanguageChoice::ZhHans));
        assert_eq!(LanguageChoice::parse("zh-TW"), None);
        assert_eq!(LanguageChoice::parse("fr"), None);
    }

    #[test]
    fn a_saved_choice_round_trips_and_an_unknown_code_falls_back() {
        let raw = serde_json::to_string(&LanguageChoice::ZhHans).expect("serialize");
        assert_eq!(raw, "\"zh-Hans\"");
        let back: LanguageChoice = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, LanguageChoice::ZhHans);
        let unknown: LanguageChoice = serde_json::from_str("\"zh-TW\"").expect("deserialize");
        assert_eq!(unknown, LanguageChoice::System);
    }

    #[test]
    fn placeholders_keep_their_names() {
        let zh = text(Resolved::ZhHans, "status.slide_of");
        assert_eq!(fill(zh, &[("n", "2"), ("m", "9")]), "幻灯片 2/9");
        let en = text(Resolved::En, "status.slide_of");
        assert_eq!(fill(en, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    fn tab_titles_follow_the_language_and_keep_the_english_id() {
        assert_eq!(tab_label(Resolved::ZhHans, "File"), "文件");
        assert_eq!(tab_label(Resolved::ZhHans, "Home"), "开始");
        assert_eq!(tab_label(Resolved::En, "Slide Show"), "Slide Show");
        assert_eq!(tab_label(Resolved::ZhHans, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::ZhHans, "tab.file", "Alt+F"), "文件 (Alt+F)");
    }
}
