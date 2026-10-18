//! Interface language for the application shell.
//!
//! The ribbon tabs, File menu, title-bar controls, status bar and Preferences read this catalog.
//! Ribbon groups, the other dialogs and command ids stay in English. Indonesian is the translation
//! here. Strings are written from the meaning of the English label. They are not copied from
//! another product's localization.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in preferences. `System` is resolved when the interface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanguageChoice {
    #[default]
    System,
    En,
    Id,
}

impl LanguageChoice {
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::En, Self::Id];

    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Id => "id",
        }
    }

    /// `system`, `en` (and `en-*`), or `id` / `id-ID`.
    pub fn parse(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::En);
        }
        if tag == "id" || tag.starts_with("id-") {
            return Some(Self::Id);
        }
        None
    }

    pub fn resolve(self, system: Option<&str>) -> Resolved {
        match self {
            Self::En => Resolved::En,
            Self::Id => Resolved::Id,
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
    Id,
}

impl Resolved {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Id => "id",
        }
    }
}

/// Map an OS locale tag to a catalog. Unknown tags are English.
pub fn resolve_tag(tag: &str) -> Resolved {
    let tag = normalize(tag);
    if tag == "id" || tag.starts_with("id-") {
        return Resolved::Id;
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
    // Tests pin System to English. A machine whose OS language is Indonesian would otherwise
    // rename the labels the UI tests look up. Tag mapping is covered on its own.
    #[cfg(test)]
    {
        let _ = tag;
        None
    }
    #[cfg(not(test))]
    tag
}

/// `(id, English, Indonesian)`. Ids are sorted so lookup can binary-search.
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.account", "Account", "Akun"),
    ("nav.back", "Back", "Kembali"),
    ("nav.close", "Close", "Tutup"),
    ("nav.export", "Export", "Ekspor"),
    ("nav.home", "Home", "Beranda"),
    ("nav.info", "Info", "Informasi"),
    ("nav.new", "New", "Baru"),
    ("nav.open", "Open", "Buka"),
    ("nav.options", "Options", "Opsi"),
    ("nav.print", "Print", "Cetak"),
    ("nav.save", "Save", "Simpan"),
    ("nav.save_as", "Save As", "Simpan sebagai"),
    ("pref.author", "Your name (comments):", "Nama Anda (komentar):"),
    ("pref.autocorrect", "AutoCorrect as you type", "Koreksi otomatis saat mengetik"),
    ("pref.cancel", "Cancel", "Batal"),
    ("pref.dark", "Dark appearance", "Tampilan gelap"),
    ("pref.done", "Done", "Selesai"),
    ("pref.editing", "Editing", "Pengeditan"),
    ("pref.english", "English", "English"),
    ("pref.general", "General", "Umum"),
    ("pref.id", "Bahasa Indonesia", "Bahasa Indonesia"),
    ("pref.language", "Interface language", "Bahasa antarmuka"),
    ("pref.quotes", "Replace straight quotes with smart quotes", "Ganti tanda kutip lurus dengan tanda kutip cerdas"),
    ("pref.size", "Interface size", "Ukuran antarmuka"),
    ("pref.smart_guides", "Show smart guides", "Tampilkan panduan cerdas"),
    ("pref.snap", "Snap objects to grid", "Lekatkan objek ke kisi"),
    ("pref.system", "System", "Menurut sistem"),
    ("pref.title", "Preferences", "Preferensi"),
    ("status.comments", "Comments", "Komentar"),
    ("status.fit", "Fit slide to current window", "Paskan salindia ke jendela ini"),
    ("status.normal", "Normal", "Biasa"),
    ("status.notes", "Notes", "Catatan"),
    ("status.reading", "Reading View", "Tampilan baca"),
    ("status.slide_master", "Slide Master", "Master salindia"),
    ("status.slide_of", "Slide {n} of {m}", "Salindia {n} dari {m}"),
    ("status.slideshow", "Slide Show", "Tayangan"),
    ("status.sorter", "Slide Sorter", "Pengurut salindia"),
    ("status.zoom_in", "Zoom In", "Perbesar"),
    ("status.zoom_out", "Zoom Out", "Perkecil"),
    ("tab.animations", "Animations", "Animasi"),
    ("tab.chart_design", "Chart Design", "Desain bagan"),
    ("tab.design", "Design", "Desain"),
    ("tab.draw", "Draw", "Gambar"),
    ("tab.file", "File", "Berkas"),
    ("tab.home", "Home", "Beranda"),
    ("tab.insert", "Insert", "Sisipkan"),
    ("tab.layout", "Layout", "Tata letak"),
    ("tab.picture_format", "Picture Format", "Format gambar"),
    ("tab.playback", "Playback", "Pemutaran"),
    ("tab.record", "Record", "Rekam"),
    ("tab.review", "Review", "Tinjau"),
    ("tab.shape_format", "Shape Format", "Format bentuk"),
    ("tab.slide_master", "Slide Master", "Master salindia"),
    ("tab.slideshow", "Slide Show", "Tayangan"),
    ("tab.table_design", "Table Design", "Desain tabel"),
    ("tab.transitions", "Transitions", "Transisi"),
    ("tab.view", "View", "Tampilan"),
    ("title.appearance", "Light/Dark appearance", "Tampilan terang atau gelap"),
    ("title.comments", "Comments", "Komentar"),
    ("title.edited", "Edited", "Diubah"),
    ("title.home", "Home", "Beranda"),
    ("title.play", "Play from Start", "Putar dari awal"),
    ("title.present", "Present", "Sajikan"),
    ("title.redo", "Redo", "Ulangi"),
    ("title.save", "Save", "Simpan"),
    ("title.search", "Search commands", "Cari perintah"),
    ("title.undo", "Undo", "Urungkan"),
];

/// The shell string `id` in `lang`. An unknown id is returned unchanged so a missed call site
/// shows up as the id rather than a blank label.
pub fn text(lang: Resolved, id: &'static str) -> &'static str {
    match CATALOG.binary_search_by_key(&id, |row| row.0) {
        Ok(i) => CATALOG
            .get(i)
            .map(|row| match lang {
                Resolved::En => row.1,
                Resolved::Id => row.2,
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
        LanguageChoice::Id => "pref.id",
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
        for (id, en, idn) in CATALOG {
            assert!(*id > previous, "{id} is out of order after {previous}");
            previous = id;
            assert!(!en.is_empty() && !idn.is_empty(), "{id}");
            assert_eq!(holes(en), holes(idn), "{id}");
            let same = ["pref.english", "pref.id"];
            if en == idn {
                assert!(same.contains(id), "{id} has no Indonesian text");
            }
        }
        assert!(CATALOG.len() >= 60, "the shell catalog shrank");
    }

    #[test]
    fn indonesian_tags_resolve_and_other_tags_do_not() {
        for tag in ["id", "id-ID", "id_ID.UTF-8", "ID-id"] {
            assert_eq!(resolve_tag(tag), Resolved::Id, "{tag}");
        }
        for tag in ["ms", "ms-MY", "it", "it-IT", "en-US", "cs", "jv", "ido", "in", ""] {
            assert_eq!(resolve_tag(tag), Resolved::En, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("id-ID")), Resolved::Id);
        assert_eq!(LanguageChoice::System.resolve(Some("ms-MY")), Resolved::En);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::En);
        assert_eq!(LanguageChoice::En.resolve(Some("id-ID")), Resolved::En);
        assert_eq!(LanguageChoice::Id.resolve(None), Resolved::Id);
    }

    #[test]
    fn the_language_command_accepts_the_three_choices() {
        assert_eq!(LanguageChoice::parse("system"), Some(LanguageChoice::System));
        assert_eq!(LanguageChoice::parse("en"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("en-GB"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("id"), Some(LanguageChoice::Id));
        assert_eq!(LanguageChoice::parse("id-ID"), Some(LanguageChoice::Id));
        assert_eq!(LanguageChoice::parse("ms"), None);
        assert_eq!(LanguageChoice::parse("it"), None);
        assert_eq!(LanguageChoice::parse("cs"), None);
        assert_eq!(LanguageChoice::parse("zh-Hans"), None);
        assert_eq!(LanguageChoice::parse("ido"), None);
    }

    #[test]
    fn a_saved_choice_round_trips_and_an_unknown_code_falls_back() {
        let raw = serde_json::to_string(&LanguageChoice::Id).expect("serialize");
        assert_eq!(raw, "\"id\"");
        let back: LanguageChoice = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, LanguageChoice::Id);
        let unknown: LanguageChoice = serde_json::from_str("\"ms\"").expect("deserialize");
        assert_eq!(unknown, LanguageChoice::System);
    }

    #[test]
    fn placeholders_keep_their_names() {
        let idn = text(Resolved::Id, "status.slide_of");
        assert_eq!(fill(idn, &[("n", "2"), ("m", "9")]), "Salindia 2 dari 9");
        let en = text(Resolved::En, "status.slide_of");
        assert_eq!(fill(en, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    fn tab_titles_follow_the_language_and_keep_the_english_id() {
        assert_eq!(tab_label(Resolved::Id, "File"), "Berkas");
        assert_eq!(tab_label(Resolved::Id, "Home"), "Beranda");
        assert_eq!(tab_label(Resolved::En, "Slide Show"), "Slide Show");
        assert_eq!(tab_label(Resolved::Id, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::Id, "tab.file", "Alt+F"), "Berkas (Alt+F)");
    }
}
