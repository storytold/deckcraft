//! Interface language for the application shell.
//!
//! The ribbon tabs, File menu, title-bar controls, status bar and Preferences read this catalog.
//! Ribbon groups, the other dialogs and command ids stay in English. Japanese is the translation
//! here. Strings are written from the meaning of the English label. They are not copied from
//! another product's localization.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in preferences. `System` is resolved when the interface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanguageChoice {
    #[default]
    System,
    En,
    Ja,
}

impl LanguageChoice {
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::En, Self::Ja];

    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Ja => "ja",
        }
    }

    /// `system`, `en` (and `en-*`), or `ja` / `ja-JP`.
    pub fn parse(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::En);
        }
        if tag == "ja" || tag.starts_with("ja-") {
            return Some(Self::Ja);
        }
        None
    }

    pub fn resolve(self, system: Option<&str>) -> Resolved {
        match self {
            Self::En => Resolved::En,
            Self::Ja => Resolved::Ja,
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
    Ja,
}

impl Resolved {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ja => "ja",
        }
    }
}

/// Map an OS locale tag to a catalog. Unknown tags are English.
pub fn resolve_tag(tag: &str) -> Resolved {
    let tag = normalize(tag);
    if tag == "ja" || tag.starts_with("ja-") {
        return Resolved::Ja;
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
    // Tests pin System to English. A machine whose OS language is Japanese would otherwise
    // rename the labels the UI tests look up. Tag mapping is covered on its own.
    #[cfg(test)]
    {
        let _ = tag;
        None
    }
    #[cfg(not(test))]
    tag
}

/// `(id, English, Japanese)`. Ids are sorted so lookup can binary-search.
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.account", "Account", "アカウント"),
    ("nav.back", "Back", "戻る"),
    ("nav.close", "Close", "閉じる"),
    ("nav.export", "Export", "エクスポート"),
    ("nav.home", "Home", "ホーム"),
    ("nav.info", "Info", "情報"),
    ("nav.new", "New", "新規"),
    ("nav.open", "Open", "開く"),
    ("nav.options", "Options", "オプション"),
    ("nav.print", "Print", "印刷"),
    ("nav.save", "Save", "保存"),
    ("nav.save_as", "Save As", "名前を付けて保存"),
    ("pref.author", "Your name (comments):", "名前（コメント）:"),
    ("pref.autocorrect", "AutoCorrect as you type", "入力しながら自動修正する"),
    ("pref.cancel", "Cancel", "キャンセル"),
    ("pref.dark", "Dark appearance", "ダーク表示"),
    ("pref.done", "Done", "完了"),
    ("pref.editing", "Editing", "編集"),
    ("pref.english", "English", "English"),
    ("pref.general", "General", "全般"),
    ("pref.ja", "日本語", "日本語"),
    ("pref.language", "Interface language", "表示言語"),
    ("pref.quotes", "Replace straight quotes with smart quotes", "直線の引用符を曲がった引用符に置き換える"),
    ("pref.size", "Interface size", "表示の大きさ"),
    ("pref.smart_guides", "Show smart guides", "スマートガイドを表示する"),
    ("pref.snap", "Snap objects to grid", "オブジェクトをグリッドに合わせる"),
    ("pref.system", "System", "システムに合わせる"),
    ("pref.title", "Preferences", "環境設定"),
    ("status.comments", "Comments", "コメント"),
    ("status.fit", "Fit slide to current window", "スライドをウィンドウに合わせる"),
    ("status.normal", "Normal", "標準"),
    ("status.notes", "Notes", "ノート"),
    ("status.reading", "Reading View", "閲覧表示"),
    ("status.slide_master", "Slide Master", "スライドマスター"),
    ("status.slide_of", "Slide {n} of {m}", "スライド {n}/{m}"),
    ("status.slideshow", "Slide Show", "スライドショー"),
    ("status.sorter", "Slide Sorter", "スライド一覧"),
    ("status.zoom_in", "Zoom In", "拡大"),
    ("status.zoom_out", "Zoom Out", "縮小"),
    ("tab.animations", "Animations", "アニメーション"),
    ("tab.chart_design", "Chart Design", "グラフのデザイン"),
    ("tab.design", "Design", "デザイン"),
    ("tab.draw", "Draw", "描画"),
    ("tab.file", "File", "ファイル"),
    ("tab.home", "Home", "ホーム"),
    ("tab.insert", "Insert", "挿入"),
    ("tab.layout", "Layout", "レイアウト"),
    ("tab.picture_format", "Picture Format", "画像の書式"),
    ("tab.playback", "Playback", "再生"),
    ("tab.record", "Record", "記録"),
    ("tab.review", "Review", "校閲"),
    ("tab.shape_format", "Shape Format", "図形の書式"),
    ("tab.slide_master", "Slide Master", "スライドマスター"),
    ("tab.slideshow", "Slide Show", "スライドショー"),
    ("tab.table_design", "Table Design", "テーブルのデザイン"),
    ("tab.transitions", "Transitions", "切り替え"),
    ("tab.view", "View", "表示"),
    ("title.appearance", "Light/Dark appearance", "ライト/ダーク表示"),
    ("title.comments", "Comments", "コメント"),
    ("title.edited", "Edited", "編集済み"),
    ("title.home", "Home", "ホーム"),
    ("title.play", "Play from Start", "最初から再生"),
    ("title.present", "Present", "発表"),
    ("title.redo", "Redo", "やり直し"),
    ("title.save", "Save", "保存"),
    ("title.search", "Search commands", "コマンドを検索"),
    ("title.undo", "Undo", "元に戻す"),
];

/// The shell string `id` in `lang`. An unknown id is returned unchanged so a missed call site
/// shows up as the id rather than a blank label.
pub fn text(lang: Resolved, id: &'static str) -> &'static str {
    match CATALOG.binary_search_by_key(&id, |row| row.0) {
        Ok(i) => CATALOG
            .get(i)
            .map(|row| match lang {
                Resolved::En => row.1,
                Resolved::Ja => row.2,
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
        LanguageChoice::Ja => "pref.ja",
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
        for (id, en, ja) in CATALOG {
            assert!(*id > previous, "{id} is out of order after {previous}");
            previous = id;
            assert!(!en.is_empty() && !ja.is_empty(), "{id}");
            assert_eq!(holes(en), holes(ja), "{id}");
            let same = ["pref.english", "pref.ja"];
            if en == ja {
                assert!(same.contains(id), "{id} has no Japanese text");
            }
        }
        assert!(CATALOG.len() >= 60, "the shell catalog shrank");
    }

    #[test]
    fn japanese_tags_resolve_and_other_tags_do_not() {
        for tag in ["ja", "ja-JP", "ja_JP.UTF-8", "ja-JP-mac", "JA-jp"] {
            assert_eq!(resolve_tag(tag), Resolved::Ja, "{tag}");
        }
        for tag in ["en-BY", "en-US", "zh-CN", "zh-TW", "ru-BY", "C", ""] {
            assert_eq!(resolve_tag(tag), Resolved::En, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("ja-JP")), Resolved::Ja);
        assert_eq!(LanguageChoice::System.resolve(Some("zh-CN")), Resolved::En);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::En);
        assert_eq!(LanguageChoice::En.resolve(Some("ja-JP")), Resolved::En);
        assert_eq!(LanguageChoice::Ja.resolve(None), Resolved::Ja);
    }

    #[test]
    fn the_language_command_accepts_the_three_choices() {
        assert_eq!(LanguageChoice::parse("system"), Some(LanguageChoice::System));
        assert_eq!(LanguageChoice::parse("en"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("en-GB"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("ja"), Some(LanguageChoice::Ja));
        assert_eq!(LanguageChoice::parse("ja-JP"), Some(LanguageChoice::Ja));
        assert_eq!(LanguageChoice::parse("zh-Hans"), None);
        assert_eq!(LanguageChoice::parse("fr"), None);
    }

    #[test]
    fn a_saved_choice_round_trips_and_an_unknown_code_falls_back() {
        let raw = serde_json::to_string(&LanguageChoice::Ja).expect("serialize");
        assert_eq!(raw, "\"ja\"");
        let back: LanguageChoice = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, LanguageChoice::Ja);
        let unknown: LanguageChoice = serde_json::from_str("\"zh-Hans\"").expect("deserialize");
        assert_eq!(unknown, LanguageChoice::System);
    }

    #[test]
    fn placeholders_keep_their_names() {
        let ja = text(Resolved::Ja, "status.slide_of");
        assert_eq!(fill(ja, &[("n", "2"), ("m", "9")]), "スライド 2/9");
        let en = text(Resolved::En, "status.slide_of");
        assert_eq!(fill(en, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    fn tab_titles_follow_the_language_and_keep_the_english_id() {
        assert_eq!(tab_label(Resolved::Ja, "File"), "ファイル");
        assert_eq!(tab_label(Resolved::Ja, "Home"), "ホーム");
        assert_eq!(tab_label(Resolved::En, "Slide Show"), "Slide Show");
        assert_eq!(tab_label(Resolved::Ja, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::Ja, "tab.file", "Alt+F"), "ファイル (Alt+F)");
    }
}
