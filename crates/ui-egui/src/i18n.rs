//! Interface text only: presentation content, file names and command identifiers stay intact.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    #[default]
    System,
    En,
    Et,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::System, Self::En, Self::Et];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => tr("System language"),
            Self::En => "English",
            Self::Et => "Eesti",
        }
    }

    pub fn from_locale(locale: &str) -> Self {
        let language = locale.split(['-', '_', '.', '@']).next().unwrap_or("");
        if language.eq_ignore_ascii_case("et") { Self::Et } else { Self::En }
    }

    pub fn resolve(self, locale: Option<&str>) -> Self {
        if self == Self::System { Self::from_locale(locale.unwrap_or("")) } else { self }
    }
}

thread_local! {
    static CURRENT: Cell<Language> = const { Cell::new(Language::En) };
}

pub fn current() -> Language {
    CURRENT.get()
}

pub fn set_current(language: Language) {
    CURRENT.set(language);
}

const CATALOG: &str = include_str!("i18n/et.tsv");
static ET: OnceLock<HashMap<String, String>> = OnceLock::new();

fn catalog() -> &'static HashMap<String, String> {
    ET.get_or_init(|| {
        CATALOG
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('\t'))
            .map(|(source, text)| (source.replace("\\n", "\n"), text.replace("\\n", "\n")))
            .collect()
    })
}

/// Exact lookup at an explicit interface boundary; unknown text falls back to English.
pub fn tr(text: &str) -> &str {
    if CURRENT.get() == Language::Et { catalog().get(text).map(String::as_str).unwrap_or(text) } else { text }
}

/// Context-specific interface label with the original English source as fallback.
pub fn tr_context<'a>(source: &'a str, key: &str) -> &'a str {
    if CURRENT.get() == Language::Et { catalog().get(key).map(String::as_str).unwrap_or(source) } else { source }
}

/// Resolve before rendering. Worker threads must not read the UI thread's language.
pub fn prompt_provider() -> fn(deckcraft_model::PhType) -> &'static str {
    if current() == Language::Et { estonian_prompt } else { english_prompt }
}

fn estonian_prompt(kind: deckcraft_model::PhType) -> &'static str {
    catalog().get(kind.prompt()).map(String::as_str).unwrap_or_else(|| kind.prompt())
}

fn english_prompt(kind: deckcraft_model::PhType) -> &'static str {
    kind.prompt()
}

/// Substitute already-formatted values without interpreting their contents as interface text.
pub fn format(source: &str, values: &[String]) -> String {
    let text = tr(source);
    let mut out = String::new();
    let mut rest = text;
    let mut index = 0;
    while let Some(start) = rest.find('{') {
        out.push_str(rest.get(..start).unwrap_or(""));
        let tail = rest.get(start + 1..).unwrap_or("");
        if let Some(after) = tail.strip_prefix('{') {
            out.push('{');
            rest = after;
        } else if let Some(end) = tail.find('}') {
            out.push_str(values.get(index).map(String::as_str).unwrap_or(""));
            index += 1;
            rest = tail.get(end + 1..).unwrap_or("");
        } else {
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(&rest.replace("}}", "}"));
    out
}

/// Estonian uses a singular noun for one and a partitive noun for other counts.
pub fn count(n: usize, english: &str, one: &str, other: &str) -> String {
    let word = if current() == Language::Et { if n == 1 { one } else { other } } else { english };
    format!("{n} {word}")
}

pub fn recovered(n: usize) -> String {
    let amount = count(n, if n == 1 { "unsaved presentation" } else { "unsaved presentations" }, "salvestamata esitlus", "salvestamata esitlust");
    format("Recovered {} from the last session", &[amount])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    fn placeholders(text: &str) -> Vec<&str> {
        let mut out = vec![];
        let mut rest = text;
        while let Some((_, tail)) = rest.split_once('{') {
            if let Some(tail) = tail.strip_prefix('{') {
                rest = tail;
                continue;
            }
            if let Some((field, tail)) = tail.split_once('}') {
                out.push(field);
                rest = tail;
            } else {
                break;
            }
        }
        out
    }

    #[test]
    fn all_command_menu_and_gallery_labels_are_translated() {
        let mut required = vec![];
        required.extend(crate::UI_COMMANDS.iter().map(|c| c.1));
        required.extend(deckcraft_engine::Session::new().commands().into_iter().map(|c| c.label));
        for (menu, items) in crate::menus::menu_tree() {
            required.push(menu);
            required.extend(items.into_iter().filter(|(label, _)| *label != "-").map(|(label, _)| label));
        }
        required.extend(deckcraft_geom::preset::CATALOG.iter().map(|shape| shape.label));
        required.extend(deckcraft_geom::preset::Category::ALL.iter().map(|category| category.label()));
        required.extend(deckcraft_model::anim::ANIMATIONS.iter().map(|effect| effect.1));
        required.extend(deckcraft_model::anim::TRANSITIONS.iter().flat_map(|effect| [effect.1, effect.2]));
        required.extend(deckcraft_model::chart::ChartType::MAIN.iter().map(|kind| kind.label()));
        required.extend(deckcraft_model::defaults::SLIDE_SIZES.iter().map(|size| size.0));
        required.extend(deckcraft_engine::cmd::file::PAPERS.iter().map(|paper| paper.1));
        required.extend(crate::backstage::SAVE_TYPES.iter().map(|kind| kind.1));
        required.extend(deckcraft_color::STANDARD_COLORS.iter().map(|color| color.1));
        required.extend(deckcraft_color::SchemeSlot::THEME.iter().map(|slot| slot.label()));
        required.extend(["@ribbon:Home", "@ribbon:Insert"]);
        let mut missing_options = vec![];
        for option in deckcraft_model::anim::TRANSITIONS
            .iter()
            .flat_map(|effect| effect.4)
            .chain(deckcraft_model::anim::ANIMATIONS.iter().flat_map(|effect| effect.5))
        {
            let label = crate::ribbon::option_label(option);
            if !label.is_empty() && !label.chars().all(|c| c.is_ascii_digit()) && !catalog().contains_key(&label) {
                missing_options.push(label);
            }
        }
        for effect in deckcraft_model::anim::TRANSITIONS {
            for option in effect.4 {
                let label = deckcraft_model::anim::transition_option_label(effect.0, option);
                if !label.is_empty() && !label.chars().all(|c| c.is_ascii_digit()) && !catalog().contains_key(&label) {
                    missing_options.push(label);
                }
            }
        }
        let missing: Vec<_> = required.into_iter().filter(|key| !catalog().contains_key(*key)).collect();
        assert!(missing.is_empty() && missing_options.is_empty(), "missing visible metadata: {missing:?}; effect options: {missing_options:?}");
    }

    #[test]
    fn every_literal_translation_call_has_a_catalog_entry() {
        for source in [
            include_str!("canvas.rs"),
            include_str!("backstage.rs"),
            include_str!("qat.rs"),
            include_str!("selpane.rs"),
            include_str!("notes.rs"),
            include_str!("eyedropper.rs"),
            include_str!("animtimeline.rs"),
            include_str!("credits.rs"),
            include_str!("dialogs.rs"),
            include_str!("fillui.rs"),
            include_str!("lib.rs"),
            include_str!("media.rs"),
            include_str!("menus.rs"),
            include_str!("panes.rs"),
            include_str!("ribbon.rs"),
            include_str!("show.rs"),
            include_str!("sorter.rs"),
            include_str!("status.rs"),
            include_str!("thumbs.rs"),
            include_str!("widgets.rs"),
            include_str!("../../../apps/deckcraft/src/main.rs"),
            include_str!("../../../apps/deckcraft-web/src/web.rs"),
        ] {
            for prefix in ["i18n::tr(\"", "i18n::format(\""] {
                let mut rest = source.split("#[cfg(test)]").next().unwrap_or(source);
                while let Some((_, tail)) = rest.split_once(prefix) {
                    let mut escaped = false;
                    let end = tail
                        .char_indices()
                        .find_map(|(index, c)| {
                            if escaped {
                                escaped = false;
                                None
                            } else if c == '\\' {
                                escaped = true;
                                None
                            } else if c == '\"' {
                                Some(index)
                            } else {
                                None
                            }
                        })
                        .unwrap();
                    let key: String = serde_json::from_str(&format!("\"{}\"", &tail[..end])).unwrap();
                    assert!(catalog().contains_key(&key), "missing literal translation: {key}");
                    rest = &tail[end + 1..];
                }
            }
        }
    }

    #[test]
    fn catalog_is_unique_complete_and_preserves_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (line_no, line) in CATALOG.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (source, text) = line.split_once('\t').unwrap();
            assert!(seen.insert(source), "duplicate on {}: {source}", line_no + 1);
            assert!(!text.trim().is_empty(), "empty translation: {source}");
            assert_eq!(placeholders(source), placeholders(text), "placeholder names/order/format: {source}");
        }
    }

    #[test]
    fn locale_selection_and_saved_settings() {
        for locale in ["et", "et-EE", "et_EE.UTF-8", "ET_ee@euro"] {
            assert_eq!(Language::from_locale(locale), Language::Et);
        }
        for locale in ["", "en-US", "fi-FI", "estonian"] {
            assert_eq!(Language::from_locale(locale), Language::En);
        }
        assert_eq!(Language::System.resolve(Some("et-EE")), Language::Et);
        assert_eq!(Language::En.resolve(Some("et-EE")), Language::En);
        let old: crate::UiState = serde_json::from_str("{}").unwrap();
        assert_eq!(old.interface_language, Language::System);
        let ui = crate::UiState { interface_language: Language::Et, ..old };
        let saved: crate::UiState = serde_json::from_str(&serde_json::to_string(&ui).unwrap()).unwrap();
        assert_eq!(saved.interface_language, Language::Et);
    }

    #[test]
    fn language_command_preserves_document_and_validates_input() {
        let mut app = crate::SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.run("insert.textBox", serde_json::json!({"text": "File {customer} / ÕÄÖÜ ŠŽ", "rect": [1, 2, 100, 40]})).unwrap();
        let before = serde_json::to_value(&app.session.active().unwrap().doc).unwrap();
        app.run("app.language", serde_json::json!({"language": "et"})).unwrap();
        assert_eq!(app.ui.interface_language, Language::Et);
        assert_eq!(tr("File"), "Fail");
        assert_eq!(before, serde_json::to_value(&app.session.active().unwrap().doc).unwrap());
        assert!(app.run("app.language", serde_json::json!({"language": "invalid"})).is_err());
        assert_eq!(app.ui.interface_language, Language::Et);
        for invalid in [serde_json::Value::Null, serde_json::json!(2), serde_json::json!(true), serde_json::json!([])] {
            assert!(app.run("app.language", serde_json::json!({"language": invalid})).is_err());
            assert_eq!(app.ui.interface_language, Language::Et);
        }
        set_current(Language::En);
    }

    #[test]
    fn formats_counts_and_worker_prompts_keep_the_selected_language() {
        set_current(Language::Et);
        assert_eq!(tr("File"), "Fail");
        assert_eq!(tr_context("Insert", "@ribbon:Insert"), "Lisamine");
        assert_eq!(tr("customer title {x}"), "customer title {x}");
        let user = "File {user} / ÕÄÖÜ.pptx".to_string();
        assert!(format("Saved {path}", std::slice::from_ref(&user)).ends_with(&user));
        for n in [0, 2, 11, 21, 101] {
            assert_eq!(count(n, "slides", "slaid", "slaidi"), format!("{n} slaidi"));
        }
        assert_eq!(count(1, "slide", "slaid", "slaidi"), "1 slaid");
        let provider = prompt_provider();
        let prompt = std::thread::spawn(move || provider(deckcraft_model::PhType::Title)).join().unwrap();
        assert_eq!(prompt, "Pealkirja lisamiseks klõpsa");
        set_current(Language::En);
        assert_eq!(tr("File"), "File");
        assert_eq!(count(2, "slides", "slaid", "slaidi"), "2 slides");
    }

    #[test]
    fn estonian_gradient_direction_updates_the_original_geometry() {
        use deckcraft_model::style::GradientShape;
        use egui_kittest::kittest::Queryable;
        set_current(Language::Et);
        let mut gradient = crate::fillui::default_gradient(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent1), 90.0);
        gradient.shape = GradientShape::Path { path: "circle".into(), focus: [0.5; 4] };
        let scheme = deckcraft_color::ColorScheme { name: "Test".into(), colors: [deckcraft_color::Rgba::BLACK; 12] };
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 700.0)).build_ui_state(
            move |ui, gradient| {
                if let Some(next) = crate::fillui::gradient_editor(ui, "review-gradient", gradient, &scheme) {
                    *gradient = next;
                }
            },
            gradient,
        );
        h.run_steps(3);
        for label in ["keskel", "ülal vasakul", "ülal paremal", "all vasakul", "all paremal"] {
            assert!(h.query_by_label(label).is_some());
        }
        h.get_by_label("ülal vasakul").click();
        h.run_steps(2);
        assert!(matches!(&h.state().shape, GradientShape::Path { path, focus } if path == "circle" && *focus == [0.0, 0.0, 1.0, 1.0]));
        if let Ok(dir) = std::env::var("DECKCRAFT_I18N_SHOTS") {
            std::fs::create_dir_all(&dir).unwrap();
            h.render().unwrap().save(format!("{dir}/et-gradient.png")).unwrap();
        }
        set_current(Language::En);
    }

    #[test]
    fn estonian_reviewed_file_pages_and_close_dialog() {
        use egui_kittest::kittest::Queryable;
        let mut app = crate::SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.run("app.language", serde_json::json!({"language": "et"})).unwrap();
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_ui_state(
            |ui, app: &mut crate::SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        for (page, labels) in [
            ("saveAs", vec![tr("DeckCraft Presentation (*.deckcraft)")]),
            ("print", vec![tr("Print All Slides"), tr("Full Page Slides"), tr("Portrait Orientation"), tr("Letter (8.5 × 11 in)")]),
        ] {
            h.state_mut().run("app.backstage", serde_json::json!({"page": page})).unwrap();
            if page == "saveAs" {
                h.state_mut().backstage.as_mut().unwrap().file_type = "deckcraft";
            }
            h.run_steps(3);
            if let Ok(dir) = std::env::var("DECKCRAFT_I18N_SHOTS") {
                std::fs::create_dir_all(&dir).unwrap();
                h.render().unwrap().save(format!("{dir}/et-{page}.png")).unwrap();
            }
            for label in labels {
                assert!(h.query_by_value(label).is_some(), "missing selected option: {label}");
            }
        }
        h.state_mut().ui.recent = vec!["/tmp/Review.pptx".into()];
        for (page, source) in [("open", "No recent presentations match “{}”."), ("new", "No themes or templates match “{}”.")] {
            h.state_mut().run("app.backstage", serde_json::json!({"page": page})).unwrap();
            let query = "ÕÄÖÜ {customer}".to_string();
            h.state_mut().backstage.as_mut().unwrap().query = query.clone();
            h.run_steps(3);
            assert!(h.query_by_label(&format(source, &[query])).is_some());
        }
        h.state_mut().backstage = None;
        h.state_mut().run("insert.textBox", serde_json::json!({"text": "ÕÄÖÜ {customer}", "rect": [1, 2, 100, 40]})).unwrap();
        let title = h.state().session.active().unwrap().title();
        h.state_mut().close_presentation();
        h.run_steps(3);
        let question = format("Do you want to save the changes you made to {what}?", &[format!("“{title}”")]);
        assert!(h.query_by_label(&question).is_some());
        h.get_by_label(tr("Cancel")).click();
        h.run_steps(2);
        assert!(h.state().session.active().unwrap().is_dirty());
        assert!(h.state().dialog.is_none());
        set_current(Language::En);
    }

    #[test]
    fn estonian_settings_and_interface_render() {
        let mut app = crate::SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.run("app.language", serde_json::json!({"language": "et"})).unwrap();
        app.run("app.preferences", serde_json::json!({})).unwrap();
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut crate::SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        use egui_kittest::kittest::Queryable;
        assert!(h.query_by_label("Kasutajaliidese keel:").is_some());
        assert_eq!(h.state().ui.interface_language, Language::Et);
        if let Ok(dir) = std::env::var("DECKCRAFT_I18N_SHOTS") {
            std::fs::create_dir_all(&dir).unwrap();
            h.render().unwrap().save(format!("{dir}/et-preferences.png")).unwrap();
        }
        h.state_mut().run("app.language", serde_json::json!({"language": "en"})).unwrap();
        h.run_steps(2);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        assert!(h.state().dialog.is_none());
        assert_eq!(h.state().ui.interface_language, Language::Et, "Escape restores the previous language");
        h.state_mut().run("app.preferences", serde_json::json!({})).unwrap();
        h.run_steps(2);
        h.state_mut().run("app.language", serde_json::json!({"language": "en"})).unwrap();
        h.run_steps(2);
        h.get_by_label("Done").click();
        h.run_steps(2);
        assert!(h.state().dialog.is_none());
        assert_eq!(h.state().ui.interface_language, Language::En, "Done keeps the selected language");
        set_current(Language::En);
    }
}
