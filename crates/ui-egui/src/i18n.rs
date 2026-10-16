//! Interface language for the application shell.
//!
//! The ribbon tabs, File menu, title-bar controls, status bar and Preferences read this catalog.
//! Ribbon groups, the other dialogs and command ids stay in English. Korean is the translation
//! here. Strings are written from the meaning of the English label. They are not copied from
//! another product's localization.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in preferences. `System` is resolved when the interface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanguageChoice {
    #[default]
    System,
    En,
    Ko,
}

impl LanguageChoice {
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::En, Self::Ko];

    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Ko => "ko",
        }
    }

    /// `system`, `en` (and `en-*`), or `ko` / `ko-KR`.
    pub fn parse(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::En);
        }
        if tag == "ko" || tag.starts_with("ko-") {
            return Some(Self::Ko);
        }
        None
    }

    pub fn resolve(self, system: Option<&str>) -> Resolved {
        match self {
            Self::En => Resolved::En,
            Self::Ko => Resolved::Ko,
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
    Ko,
}

impl Resolved {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ko => "ko",
        }
    }
}

/// Map an OS locale tag to a catalog. Unknown tags are English.
pub fn resolve_tag(tag: &str) -> Resolved {
    let tag = normalize(tag);
    if tag == "ko" || tag.starts_with("ko-") {
        return Resolved::Ko;
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
    // Tests pin System to English. A machine whose OS language is Korean would otherwise
    // rename the labels the UI tests look up. Tag mapping is covered on its own.
    #[cfg(test)]
    {
        let _ = tag;
        None
    }
    #[cfg(not(test))]
    tag
}

/// `(id, English, Korean)`. Ids are sorted so lookup can binary-search.
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.account", "Account", "계정"),
    ("nav.back", "Back", "뒤로"),
    ("nav.close", "Close", "닫기"),
    ("nav.export", "Export", "내보내기"),
    ("nav.home", "Home", "홈"),
    ("nav.info", "Info", "정보"),
    ("nav.new", "New", "새로 만들기"),
    ("nav.open", "Open", "열기"),
    ("nav.options", "Options", "옵션"),
    ("nav.print", "Print", "인쇄"),
    ("nav.save", "Save", "저장"),
    ("nav.save_as", "Save As", "다른 이름으로 저장"),
    ("pref.author", "Your name (comments):", "이름(메모):"),
    ("pref.autocorrect", "AutoCorrect as you type", "입력하는 동안 자동으로 고침"),
    ("pref.cancel", "Cancel", "취소"),
    ("pref.dark", "Dark appearance", "어두운 화면"),
    ("pref.done", "Done", "완료"),
    ("pref.editing", "Editing", "편집"),
    ("pref.english", "English", "English"),
    ("pref.general", "General", "일반"),
    ("pref.ko", "한국어", "한국어"),
    ("pref.language", "Interface language", "표시 언어"),
    ("pref.quotes", "Replace straight quotes with smart quotes", "곧은 따옴표를 둥근 따옴표로 바꿈"),
    ("pref.size", "Interface size", "표시 크기"),
    ("pref.smart_guides", "Show smart guides", "스마트 안내선을 표시"),
    ("pref.snap", "Snap objects to grid", "개체를 격자에 맞춤"),
    ("pref.system", "System", "시스템 설정을 따름"),
    ("pref.title", "Preferences", "환경설정"),
    ("status.comments", "Comments", "메모"),
    ("status.fit", "Fit slide to current window", "슬라이드를 현재 창에 맞춤"),
    ("status.normal", "Normal", "기본"),
    ("status.notes", "Notes", "노트"),
    ("status.reading", "Reading View", "읽기 화면"),
    ("status.slide_master", "Slide Master", "슬라이드 마스터"),
    ("status.slide_of", "Slide {n} of {m}", "슬라이드 {n}/{m}"),
    ("status.slideshow", "Slide Show", "슬라이드 쇼"),
    ("status.sorter", "Slide Sorter", "슬라이드 모아 보기"),
    ("status.zoom_in", "Zoom In", "확대"),
    ("status.zoom_out", "Zoom Out", "축소"),
    ("tab.animations", "Animations", "애니메이션"),
    ("tab.chart_design", "Chart Design", "차트 디자인"),
    ("tab.design", "Design", "디자인"),
    ("tab.draw", "Draw", "그리기"),
    ("tab.file", "File", "파일"),
    ("tab.home", "Home", "홈"),
    ("tab.insert", "Insert", "삽입"),
    ("tab.layout", "Layout", "레이아웃"),
    ("tab.picture_format", "Picture Format", "그림 서식"),
    ("tab.playback", "Playback", "재생"),
    ("tab.record", "Record", "녹화"),
    ("tab.review", "Review", "검토"),
    ("tab.shape_format", "Shape Format", "도형 서식"),
    ("tab.slide_master", "Slide Master", "슬라이드 마스터"),
    ("tab.slideshow", "Slide Show", "슬라이드 쇼"),
    ("tab.table_design", "Table Design", "표 디자인"),
    ("tab.transitions", "Transitions", "전환"),
    ("tab.view", "View", "보기"),
    ("title.appearance", "Light/Dark appearance", "밝은 화면/어두운 화면"),
    ("title.comments", "Comments", "메모"),
    ("title.edited", "Edited", "편집됨"),
    ("title.home", "Home", "홈"),
    ("title.play", "Play from Start", "처음부터 재생"),
    ("title.present", "Present", "발표"),
    ("title.redo", "Redo", "다시 실행"),
    ("title.save", "Save", "저장"),
    ("title.search", "Search commands", "명령 검색"),
    ("title.undo", "Undo", "실행 취소"),
];

/// The shell string `id` in `lang`. An unknown id is returned unchanged so a missed call site
/// shows up as the id rather than a blank label.
pub fn text(lang: Resolved, id: &'static str) -> &'static str {
    match CATALOG.binary_search_by_key(&id, |row| row.0) {
        Ok(i) => CATALOG
            .get(i)
            .map(|row| match lang {
                Resolved::En => row.1,
                Resolved::Ko => row.2,
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
        LanguageChoice::Ko => "pref.ko",
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
        for (id, en, ko) in CATALOG {
            assert!(*id > previous, "{id} is out of order after {previous}");
            previous = id;
            assert!(!en.is_empty() && !ko.is_empty(), "{id}");
            assert_eq!(holes(en), holes(ko), "{id}");
            let same = ["pref.english", "pref.ko"];
            if en == ko {
                assert!(same.contains(id), "{id} has no Korean text");
            }
        }
        assert!(CATALOG.len() >= 60, "the shell catalog shrank");
    }

    #[test]
    fn korean_tags_resolve_and_other_tags_do_not() {
        for tag in ["ko", "ko-KR", "ko_KR.UTF-8", "ko-KP", "KO-kr"] {
            assert_eq!(resolve_tag(tag), Resolved::Ko, "{tag}");
        }
        for tag in ["en-BY", "en-US", "ja-JP", "zh-CN", "zh-TW", "ru-BY", "C", ""] {
            assert_eq!(resolve_tag(tag), Resolved::En, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("ko-KR")), Resolved::Ko);
        assert_eq!(LanguageChoice::System.resolve(Some("ja-JP")), Resolved::En);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::En);
        assert_eq!(LanguageChoice::En.resolve(Some("ko-KR")), Resolved::En);
        assert_eq!(LanguageChoice::Ko.resolve(None), Resolved::Ko);
    }

    #[test]
    fn the_language_command_accepts_the_three_choices() {
        assert_eq!(LanguageChoice::parse("system"), Some(LanguageChoice::System));
        assert_eq!(LanguageChoice::parse("en"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("en-GB"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("ko"), Some(LanguageChoice::Ko));
        assert_eq!(LanguageChoice::parse("ko-KR"), Some(LanguageChoice::Ko));
        assert_eq!(LanguageChoice::parse("ja"), None);
        assert_eq!(LanguageChoice::parse("zh-Hans"), None);
        assert_eq!(LanguageChoice::parse("fr"), None);
    }

    #[test]
    fn a_saved_choice_round_trips_and_an_unknown_code_falls_back() {
        let raw = serde_json::to_string(&LanguageChoice::Ko).expect("serialize");
        assert_eq!(raw, "\"ko\"");
        let back: LanguageChoice = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, LanguageChoice::Ko);
        let unknown: LanguageChoice = serde_json::from_str("\"ja\"").expect("deserialize");
        assert_eq!(unknown, LanguageChoice::System);
    }

    #[test]
    fn placeholders_keep_their_names() {
        let ko = text(Resolved::Ko, "status.slide_of");
        assert_eq!(fill(ko, &[("n", "2"), ("m", "9")]), "슬라이드 2/9");
        let en = text(Resolved::En, "status.slide_of");
        assert_eq!(fill(en, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    fn tab_titles_follow_the_language_and_keep_the_english_id() {
        assert_eq!(tab_label(Resolved::Ko, "File"), "파일");
        assert_eq!(tab_label(Resolved::Ko, "Home"), "홈");
        assert_eq!(tab_label(Resolved::En, "Slide Show"), "Slide Show");
        assert_eq!(tab_label(Resolved::Ko, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::Ko, "tab.file", "Alt+F"), "파일 (Alt+F)");
    }
}
