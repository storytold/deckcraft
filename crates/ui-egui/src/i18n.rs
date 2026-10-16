//! Interface language for the application shell.
//!
//! The ribbon tabs, File menu, title-bar controls, status bar and Preferences read this catalog.
//! Ribbon groups, the other dialogs and command ids stay in English. Czech is the translation
//! here. Strings are written from the meaning of the English label. They are not copied from
//! another product's localization.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The choice saved in preferences. `System` is resolved when the interface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanguageChoice {
    #[default]
    System,
    En,
    Cs,
}

impl LanguageChoice {
    pub const ALL: [LanguageChoice; 3] = [Self::System, Self::En, Self::Cs];

    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Cs => "cs",
        }
    }

    /// `system`, `en` (and `en-*`), or `cs` / `cs-CZ`.
    pub fn parse(raw: &str) -> Option<Self> {
        let tag = normalize(raw);
        if tag == "system" {
            return Some(Self::System);
        }
        if tag == "en" || tag.starts_with("en-") {
            return Some(Self::En);
        }
        if tag == "cs" || tag.starts_with("cs-") {
            return Some(Self::Cs);
        }
        None
    }

    pub fn resolve(self, system: Option<&str>) -> Resolved {
        match self {
            Self::En => Resolved::En,
            Self::Cs => Resolved::Cs,
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
    Cs,
}

impl Resolved {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Cs => "cs",
        }
    }
}

/// Map an OS locale tag to a catalog. Unknown tags are English.
pub fn resolve_tag(tag: &str) -> Resolved {
    let tag = normalize(tag);
    if tag == "cs" || tag.starts_with("cs-") {
        return Resolved::Cs;
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
    // Tests pin System to English. A machine whose OS language is Czech would otherwise
    // rename the labels the UI tests look up. Tag mapping is covered on its own.
    #[cfg(test)]
    {
        let _ = tag;
        None
    }
    #[cfg(not(test))]
    tag
}

/// `(id, English, Czech)`. Ids are sorted so lookup can binary-search.
const CATALOG: &[(&str, &str, &str)] = &[
    ("nav.account", "Account", "Účet"),
    ("nav.back", "Back", "Zpět"),
    ("nav.close", "Close", "Zavřít"),
    ("nav.export", "Export", "Exportovat"),
    ("nav.home", "Home", "Domů"),
    ("nav.info", "Info", "Informace"),
    ("nav.new", "New", "Nový"),
    ("nav.open", "Open", "Otevřít"),
    ("nav.options", "Options", "Možnosti"),
    ("nav.print", "Print", "Tisk"),
    ("nav.save", "Save", "Uložit"),
    ("nav.save_as", "Save As", "Uložit jako"),
    ("pref.author", "Your name (comments):", "Vaše jméno (komentáře):"),
    ("pref.autocorrect", "AutoCorrect as you type", "Opravovat při psaní"),
    ("pref.cancel", "Cancel", "Zrušit"),
    ("pref.cs", "Čeština", "Čeština"),
    ("pref.dark", "Dark appearance", "Tmavý vzhled"),
    ("pref.done", "Done", "Hotovo"),
    ("pref.editing", "Editing", "Úpravy"),
    ("pref.english", "English", "English"),
    ("pref.general", "General", "Obecné"),
    ("pref.language", "Interface language", "Jazyk rozhraní"),
    ("pref.quotes", "Replace straight quotes with smart quotes", "Nahradit rovné uvozovky oblými"),
    ("pref.size", "Interface size", "Velikost rozhraní"),
    ("pref.smart_guides", "Show smart guides", "Zobrazit chytrá vodítka"),
    ("pref.snap", "Snap objects to grid", "Přichytit objekty k mřížce"),
    ("pref.system", "System", "Podle systému"),
    ("pref.title", "Preferences", "Předvolby"),
    ("status.comments", "Comments", "Komentáře"),
    ("status.fit", "Fit slide to current window", "Přizpůsobit snímek aktuálnímu oknu"),
    ("status.normal", "Normal", "Normální"),
    ("status.notes", "Notes", "Poznámky"),
    ("status.reading", "Reading View", "Zobrazení pro čtení"),
    ("status.slide_master", "Slide Master", "Předloha snímků"),
    ("status.slide_of", "Slide {n} of {m}", "Snímek {n}/{m}"),
    ("status.slideshow", "Slide Show", "Prezentace"),
    ("status.sorter", "Slide Sorter", "Řazení snímků"),
    ("status.zoom_in", "Zoom In", "Přiblížit"),
    ("status.zoom_out", "Zoom Out", "Oddálit"),
    ("tab.animations", "Animations", "Animace"),
    ("tab.chart_design", "Chart Design", "Návrh grafu"),
    ("tab.design", "Design", "Návrh"),
    ("tab.draw", "Draw", "Kreslení"),
    ("tab.file", "File", "Soubor"),
    ("tab.home", "Home", "Domů"),
    ("tab.insert", "Insert", "Vložení"),
    ("tab.layout", "Layout", "Rozložení"),
    ("tab.picture_format", "Picture Format", "Formát obrázku"),
    ("tab.playback", "Playback", "Přehrávání"),
    ("tab.record", "Record", "Nahrávání"),
    ("tab.review", "Review", "Revize"),
    ("tab.shape_format", "Shape Format", "Formát obrazce"),
    ("tab.slide_master", "Slide Master", "Předloha snímků"),
    ("tab.slideshow", "Slide Show", "Prezentace"),
    ("tab.table_design", "Table Design", "Návrh tabulky"),
    ("tab.transitions", "Transitions", "Přechody"),
    ("tab.view", "View", "Zobrazení"),
    ("title.appearance", "Light/Dark appearance", "Světlý/tmavý vzhled"),
    ("title.comments", "Comments", "Komentáře"),
    ("title.edited", "Edited", "Upraveno"),
    ("title.home", "Home", "Domů"),
    ("title.play", "Play from Start", "Spustit od začátku"),
    ("title.present", "Present", "Prezentovat"),
    ("title.redo", "Redo", "Znovu"),
    ("title.save", "Save", "Uložit"),
    ("title.search", "Search commands", "Hledat příkazy"),
    ("title.undo", "Undo", "Zpět"),
];

/// The shell string `id` in `lang`. An unknown id is returned unchanged so a missed call site
/// shows up as the id rather than a blank label.
pub fn text(lang: Resolved, id: &'static str) -> &'static str {
    match CATALOG.binary_search_by_key(&id, |row| row.0) {
        Ok(i) => CATALOG
            .get(i)
            .map(|row| match lang {
                Resolved::En => row.1,
                Resolved::Cs => row.2,
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
        LanguageChoice::Cs => "pref.cs",
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
        for (id, en, cs) in CATALOG {
            assert!(*id > previous, "{id} is out of order after {previous}");
            previous = id;
            assert!(!en.is_empty() && !cs.is_empty(), "{id}");
            assert_eq!(holes(en), holes(cs), "{id}");
            let same = ["pref.english", "pref.cs"];
            if en == cs {
                assert!(same.contains(id), "{id} has no Czech text");
            }
        }
        assert!(CATALOG.len() >= 60, "the shell catalog shrank");
    }

    #[test]
    fn czech_tags_resolve_and_other_tags_do_not() {
        for tag in ["cs", "cs-CZ", "cs_CZ.UTF-8", "CS-cz"] {
            assert_eq!(resolve_tag(tag), Resolved::Cs, "{tag}");
        }
        for tag in ["sk", "sk-SK", "en-US", "de-DE", "pl", "zh-TW", ""] {
            assert_eq!(resolve_tag(tag), Resolved::En, "{tag}");
        }
        assert_eq!(LanguageChoice::System.resolve(Some("cs-CZ")), Resolved::Cs);
        assert_eq!(LanguageChoice::System.resolve(Some("sk-SK")), Resolved::En);
        assert_eq!(LanguageChoice::System.resolve(None), Resolved::En);
        assert_eq!(LanguageChoice::En.resolve(Some("cs-CZ")), Resolved::En);
        assert_eq!(LanguageChoice::Cs.resolve(None), Resolved::Cs);
    }

    #[test]
    fn the_language_command_accepts_the_three_choices() {
        assert_eq!(LanguageChoice::parse("system"), Some(LanguageChoice::System));
        assert_eq!(LanguageChoice::parse("en"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("en-GB"), Some(LanguageChoice::En));
        assert_eq!(LanguageChoice::parse("cs"), Some(LanguageChoice::Cs));
        assert_eq!(LanguageChoice::parse("cs-CZ"), Some(LanguageChoice::Cs));
        assert_eq!(LanguageChoice::parse("sk"), None);
        assert_eq!(LanguageChoice::parse("de"), None);
        assert_eq!(LanguageChoice::parse("zh-Hans"), None);
    }

    #[test]
    fn a_saved_choice_round_trips_and_an_unknown_code_falls_back() {
        let raw = serde_json::to_string(&LanguageChoice::Cs).expect("serialize");
        assert_eq!(raw, "\"cs\"");
        let back: LanguageChoice = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, LanguageChoice::Cs);
        let unknown: LanguageChoice = serde_json::from_str("\"sk\"").expect("deserialize");
        assert_eq!(unknown, LanguageChoice::System);
    }

    #[test]
    fn placeholders_keep_their_names() {
        let cs = text(Resolved::Cs, "status.slide_of");
        assert_eq!(fill(cs, &[("n", "2"), ("m", "9")]), "Snímek 2/9");
        let en = text(Resolved::En, "status.slide_of");
        assert_eq!(fill(en, &[("n", "2"), ("m", "9")]), "Slide 2 of 9");
        assert_eq!(fill("Slide {n} of {m}", &[("n", "1")]), "Slide 1 of {m}");
    }

    #[test]
    fn tab_titles_follow_the_language_and_keep_the_english_id() {
        assert_eq!(tab_label(Resolved::Cs, "File"), "Soubor");
        assert_eq!(tab_label(Resolved::Cs, "Home"), "Domů");
        assert_eq!(tab_label(Resolved::En, "Slide Show"), "Slide Show");
        assert_eq!(tab_label(Resolved::Cs, "Not a tab"), "Not a tab");
        assert_eq!(with_shortcut(Resolved::Cs, "tab.file", "Alt+F"), "Soubor (Alt+F)");
    }
}
