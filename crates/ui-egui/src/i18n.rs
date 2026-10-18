//! Interface language for the application shell.
//!
//! The ribbon tabs, File menu, title-bar controls, status bar and Preferences read this catalog.
//! Ribbon groups, the other dialogs and command ids stay in English. Vietnamese is the translation
//! here. Strings are written from the meaning of the English label. They are not copied from
//! another product's localization.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in preferences. `System` is resolved when the interface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanguageChoice {
    #[default]
    System,
    En,
    Vi,
}

impl LanguageChoice {
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::En, Self::Vi];

    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Vi => "vi",
        }
    }

    /// `system`, `en` (and `en-*`), or `vi` / `vi-VN`.
    pub fn parse(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::En);
        }
        if tag == "vi" || tag.starts_with("vi-") {
            return Some(Self::Vi);
        }
        None
    }

    pub fn resolve(self, system: Option<&str>) -> Resolved {
        match self {
            Self::En => Resolved::En,
            Self::Vi => Resolved::Vi,
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
    Vi,
}

impl Resolved {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Vi => "vi",
        }
    }
}

/// Map an OS locale tag to a catalog. Unknown tags are English.
pub fn resolve_tag(tag: &str) -> Resolved {
    let tag = normalize(tag);
    if tag == "vi" || tag.starts_with("vi-") {
        return Resolved::Vi;
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
    // Tests pin System to English. A machine whose OS language is Vietnamese would otherwise
    // rename the labels the UI tests look up. Tag mapping is covered on its own.
    #[cfg(test)]
    {
        let _ = tag;
        None
    }
    #[cfg(not(test))]
    tag
}

/// `(id, English, Vietnamese)`. Ids are sorted so lookup can binary-search.
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.account", "Account", "Tài khoản"),
    ("nav.back", "Back", "Quay lại"),
    ("nav.close", "Close", "Đóng"),
    ("nav.export", "Export", "Xuất"),
    ("nav.home", "Home", "Trang chủ"),
    ("nav.info", "Info", "Thông tin"),
    ("nav.new", "New", "Mới"),
    ("nav.open", "Open", "Mở"),
    ("nav.options", "Options", "Lựa chọn"),
    ("nav.print", "Print", "In"),
    ("nav.save", "Save", "Lưu"),
    ("nav.save_as", "Save As", "Lưu với tên khác"),
    ("pref.author", "Your name (comments):", "Tên của bạn (bình luận):"),
    ("pref.autocorrect", "AutoCorrect as you type", "Tự sửa khi gõ"),
    ("pref.cancel", "Cancel", "Hủy"),
    ("pref.dark", "Dark appearance", "Giao diện tối"),
    ("pref.done", "Done", "Xong"),
    ("pref.editing", "Editing", "Soạn thảo"),
    ("pref.english", "English", "English"),
    ("pref.general", "General", "Chung"),
    ("pref.language", "Interface language", "Ngôn ngữ giao diện"),
    ("pref.quotes", "Replace straight quotes with smart quotes", "Đổi dấu nháy thẳng thành dấu nháy thông minh"),
    ("pref.size", "Interface size", "Cỡ giao diện"),
    ("pref.smart_guides", "Show smart guides", "Hiện đường căn thông minh"),
    ("pref.snap", "Snap objects to grid", "Bám đối tượng vào lưới"),
    ("pref.system", "System", "Theo hệ thống"),
    ("pref.title", "Preferences", "Tùy chọn"),
    ("pref.vi", "Tiếng Việt", "Tiếng Việt"),
    ("status.comments", "Comments", "Bình luận"),
    ("status.fit", "Fit slide to current window", "Vừa trang chiếu vào cửa sổ này"),
    ("status.normal", "Normal", "Thường"),
    ("status.notes", "Notes", "Ghi chú"),
    ("status.reading", "Reading View", "Chế độ đọc"),
    ("status.slide_master", "Slide Master", "Trang chiếu gốc"),
    ("status.slide_of", "Slide {n} of {m}", "Trang chiếu {n} trên {m}"),
    ("status.slideshow", "Slide Show", "Trình chiếu"),
    ("status.sorter", "Slide Sorter", "Sắp xếp trang chiếu"),
    ("status.zoom_in", "Zoom In", "Phóng to"),
    ("status.zoom_out", "Zoom Out", "Thu nhỏ"),
    ("tab.animations", "Animations", "Hoạt hình"),
    ("tab.chart_design", "Chart Design", "Thiết kế biểu đồ"),
    ("tab.design", "Design", "Thiết kế"),
    ("tab.draw", "Draw", "Vẽ"),
    ("tab.file", "File", "Tệp"),
    ("tab.home", "Home", "Trang chủ"),
    ("tab.insert", "Insert", "Chèn"),
    ("tab.layout", "Layout", "Bố cục"),
    ("tab.picture_format", "Picture Format", "Định dạng ảnh"),
    ("tab.playback", "Playback", "Phát lại"),
    ("tab.record", "Record", "Ghi"),
    ("tab.review", "Review", "Xem lại"),
    ("tab.shape_format", "Shape Format", "Định dạng hình"),
    ("tab.slide_master", "Slide Master", "Trang chiếu gốc"),
    ("tab.slideshow", "Slide Show", "Trình chiếu"),
    ("tab.table_design", "Table Design", "Thiết kế bảng"),
    ("tab.transitions", "Transitions", "Chuyển tiếp"),
    ("tab.view", "View", "Xem"),
    ("title.appearance", "Light/Dark appearance", "Giao diện sáng hoặc tối"),
    ("title.comments", "Comments", "Bình luận"),
    ("title.edited", "Edited", "Đã sửa"),
    ("title.home", "Home", "Trang chủ"),
    ("title.play", "Play from Start", "Phát từ đầu"),
    ("title.present", "Present", "Trình bày"),
    ("title.redo", "Redo", "Làm lại"),
    ("title.save", "Save", "Lưu"),
    ("title.search", "Search commands", "Tìm lệnh"),
    ("title.undo", "Undo", "Hoàn tác"),
];

/// The shell string `id` in `lang`. An unknown id is returned unchanged so a missed call site
/// shows up as the id rather than a blank label.
pub fn text(lang: Resolved, id: &'static str) -> &'static str {
    match CATALOG.binary_search_by_key(&id, |row| row.0) {
        Ok(i) => CATALOG
            .get(i)
            .map(|row| match lang {
                Resolved::En => row.1,
                Resolved::Vi => row.2,
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
        LanguageChoice::Vi => "pref.vi",
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
        for (id, en, vn) in CATALOG {
            assert!(*id > previous, "{id} is out of order after {previous}");
            previous = id;
            assert!(!en.is_empty() && !vn.is_empty(), "{id}");
            assert_eq!(holes(en), holes(vn), "{id}");
            let same = ["pref.english", "pref.vi"];
            if en == vn {
                assert!(same.contains(id), "{id} has no Vietnamese text");
            }
        }
        assert!(CATALOG.len() >= 60, "the shell catalog shrank");
    }

    #[test]
    fn vietnamese_tags_resolve_and_other_tags_do_not() {
        for tag in ["vi", "vi-VN", "vi_VN.UTF-8", "VI-vn"] {
            assert_eq!(resolve_tag(tag), Resolved::Vi, "{tag}");
        }
        for tag in ["id", "id-ID", "tr", "tr-TR", "en-US", "zh-Hans", "vie", ""] {
            assert_eq!(resolve_tag(tag), Resolved::En, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("vi-VN")), Resolved::Vi);
        assert_eq!(LanguageChoice::System.resolve(Some("tr-TR")), Resolved::En);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::En);
        assert_eq!(LanguageChoice::En.resolve(Some("vi-VN")), Resolved::En);
        assert_eq!(LanguageChoice::Vi.resolve(None), Resolved::Vi);
    }

    #[test]
    fn the_language_command_accepts_the_three_choices() {
        assert_eq!(LanguageChoice::parse("system"), Some(LanguageChoice::System));
        assert_eq!(LanguageChoice::parse("en"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("en-GB"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("vi"), Some(LanguageChoice::Vi));
        assert_eq!(LanguageChoice::parse("vi-VN"), Some(LanguageChoice::Vi));
        assert_eq!(LanguageChoice::parse("id"), None);
        assert_eq!(LanguageChoice::parse("tr"), None);
        assert_eq!(LanguageChoice::parse("zh-Hans"), None);
        assert_eq!(LanguageChoice::parse("vie"), None);
    }

    #[test]
    fn a_saved_choice_round_trips_and_an_unknown_code_falls_back() {
        let raw = serde_json::to_string(&LanguageChoice::Vi).expect("serialize");
        assert_eq!(raw, "\"vi\"");
        let back: LanguageChoice = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, LanguageChoice::Vi);
        let unknown: LanguageChoice = serde_json::from_str("\"tr\"").expect("deserialize");
        assert_eq!(unknown, LanguageChoice::System);
    }

    #[test]
    fn placeholders_keep_their_names() {
        let vn = text(Resolved::Vi, "status.slide_of");
        assert_eq!(fill(vn, &[("n", "2"), ("m", "9")]), "Trang chiếu 2 trên 9");
        let en = text(Resolved::En, "status.slide_of");
        assert_eq!(fill(en, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    fn tab_titles_follow_the_language_and_keep_the_english_id() {
        assert_eq!(tab_label(Resolved::Vi, "File"), "Tệp");
        assert_eq!(tab_label(Resolved::Vi, "Home"), "Trang chủ");
        assert_eq!(tab_label(Resolved::En, "Slide Show"), "Slide Show");
        assert_eq!(tab_label(Resolved::Vi, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::Vi, "tab.file", "Alt+F"), "Tệp (Alt+F)");
    }
}
