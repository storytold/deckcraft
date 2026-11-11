//! Interface text only: presentation content, file names and command identifiers stay intact.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// User interface language selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    /// Follow the host operating system or browser locale.
    #[default]
    System,
    /// Brazilian Portuguese (`pt-BR`).
    PtBr,
    /// English fallback (`en`).
    En,
}

impl Language {
    /// All available language choices in presentation order.
    pub const ALL: [Self; 3] = [Self::System, Self::PtBr, Self::En];

    /// Human-readable display label for preferences and menus.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => tr("System language"),
            Self::PtBr => "Português (Brasil)",
            Self::En => "English",
        }
    }

    /// Match a BCP 47 locale tag (e.g. `pt-BR`, `pt_PT`, `en-US`) to a supported language.
    pub fn from_locale(locale: &str) -> Self {
        let language = locale.split(['-', '_', '.', '@']).next().unwrap_or("");
        if language.eq_ignore_ascii_case("pt") { Self::PtBr } else { Self::En }
    }

    /// Machine-readable language code for serialization or command options.
    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::PtBr => "pt-BR",
            Self::En => "en",
        }
    }

    /// Parse a language code string into a [`Language`] option.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_ascii_lowercase().as_str() {
            "system" | "auto" => Some(Self::System),
            "pt-br" | "pt_br" | "pt" => Some(Self::PtBr),
            "en" | "en-us" => Some(Self::En),
            _ => None,
        }
    }

    /// Resolve [`Language::System`] to a concrete language using the provided system locale.
    pub fn resolve(self, locale: Option<&str>) -> Self {
        if self == Self::System { Self::from_locale(locale.unwrap_or("")) } else { self }
    }
}

thread_local! {
    static CURRENT: Cell<Language> = const { Cell::new(Language::En) };
}

/// Returns the currently active user interface language for the current thread.
pub fn current() -> Language {
    CURRENT.get()
}

/// Set the currently active user interface language for the current thread.
pub fn set_current(language: Language) {
    CURRENT.set(language);
}

/// Convenience alias to set the active user interface language.
pub fn set_language(language: Language) {
    set_current(language);
}

const CATALOG_PT_BR: &str = include_str!("i18n/pt-br.tsv");
static PT_BR: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Lazily parsed Brazilian Portuguese translation map from `pt-br.tsv`.
fn catalog() -> &'static HashMap<String, String> {
    PT_BR.get_or_init(|| {
        CATALOG_PT_BR
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('\t'))
            .map(|(source, text)| (source.replace("\\n", "\n"), text.replace("\\n", "\n")))
            .collect()
    })
}

/// Exact lookup at an explicit interface boundary; unknown text falls back to English.
pub fn tr(text: &str) -> &str {
    if CURRENT.get() == Language::PtBr { catalog().get(text).map(String::as_str).unwrap_or(text) } else { text }
}

/// Substitute already-formatted values without interpreting their contents as interface text.
pub fn format(source: &str, values: &[&str]) -> String {
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
            let key = &tail[..end];
            let val = if let Ok(i) = key.parse::<usize>() {
                values.get(i).copied()
            } else {
                let v = values.get(index).copied();
                index += 1;
                v
            };
            out.push_str(val.unwrap_or(""));
            rest = tail.get(end + 1..).unwrap_or("");
        } else {
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(&rest.replace("}}", "}"));
    out
}

/// Substitute an integer count into singular/plural forms depending on the active language.
pub fn count(n: usize, english: &str, singular: &str, plural: &str) -> String {
    let word = if CURRENT.get() == Language::PtBr { if n == 1 { singular } else { plural } } else { english };
    std::format!("{n} {word}")
}

/// Formats the AutoRecover startup notification with the number of recovered presentations.
pub fn recovered(n: usize) -> String {
    let amount =
        count(n, if n == 1 { "unsaved presentation" } else { "unsaved presentations" }, "apresentação não salva", "apresentações não salvas");
    format("Recovered {} from the last session", &[&amount])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translations() {
        set_current(Language::PtBr);
        for (source, expected) in [
            ("Cut", "Recortar"),
            ("Copy", "Copiar"),
            ("Paste", "Colar"),
            ("Grid & Guides", "Grade e Guias"),
            ("Format Shape", "Formatar Forma"),
            ("Animation Pane", "Painel de Animação"),
            ("Home", "Página Inicial"),
            ("Insert", "Inserir"),
            ("New\nSlide", "Novo\nSlide"),
            ("Add\nAnimation", "Adicionar\nAnimação"),
            ("Account", "Conta"),
            ("Options", "Opções"),
            ("Preset gradients", "Gradientes predefinidos"),
            ("Inserted 1 slide", "Inserido 1 slide"),
            ("Distribute Vertically", "Distribuir Verticalmente"),
            ("File (Alt+F)", "Arquivo (Alt+F)"),
            ("SmartArt Design", "Design do SmartArt"),
            ("Zoom", "Zoom"),
            ("Zoom…", "Zoom…"),
            ("Zoom on", "Zoom ativado"),
            ("Interface Language", "Idioma da Interface"),
            ("Interface Language: System", "Idioma da interface: Sistema"),
            ("Interface Language: Português (Brasil)", "Idioma da interface: Português (Brasil)"),
            ("Interface Language: English", "Idioma da interface: Inglês"),
            ("Save to the page", "Salvar na página"),
            ("Click the preview to zoom.", "Clique na visualização para aplicar zoom."),
            ("Choose a slide to show it, or press G to return.", "Escolha um slide para exibi-lo ou pressione G para voltar."),
            ("The audience window has closed.", "A janela do público foi fechada."),
            ("The audience window is not responding.", "A janela do público não está respondendo."),
            ("Return to the audience window to resume the connection.", "Retorne à janela do público para retomar a conexão."),
            ("Waiting for the audience window…", "Aguardando a janela do público…"),
            ("(missing)", "(ausente)"),
        ] {
            assert_eq!(tr(source), expected);
        }
        set_current(Language::En);
    }

    #[test]
    fn test_translation_formatting() {
        set_current(Language::PtBr);
        assert_eq!(format("Slide {0} of {1}", &["1", "5"]), "Slide 1 de 5");
        assert_eq!(tr("Exported {path}").replace("{path}", "test.pdf"), "Exportado test.pdf");
        assert_eq!(tr("Saved {path}").replace("{path}", "test.deckcraft"), "Salvo em test.deckcraft");
        assert_eq!(format("Still opening {0}", &["deck.pptx"]), "Ainda abrindo deck.pptx");
        assert_eq!(format("Stopped opening {0}", &["deck.pptx"]), "Cancelada a abertura de deck.pptx");
        assert_eq!(format("Inserted {} slides", &["3"]), "Inseridos 3 slides");
        assert_eq!(
            tr("Couldn't save AutoRecover information: {e}").replace("{e}", "disk full"),
            "Não foi possível salvar informações de AutoRecuperação: disk full"
        );
        assert_eq!(format("First {} slides", &["48"]), "Primeiros 48 slides");
        assert_eq!(format("{0} — this file can't be found", &["talk.pptx"]), "talk.pptx — este arquivo não foi encontrado");
        assert_eq!(format("No themes or templates match “{0}”.", &["xyz"]), "Nenhum tema ou modelo corresponde a “xyz”.");
        set_current(Language::En);
    }

    #[test]
    fn test_english_fallback() {
        set_current(Language::En);
        assert_eq!(tr("Cut"), "Cut");
        assert_eq!(tr("Home"), "Home");
        assert_eq!(tr("Inserted 1 slide"), "Inserted 1 slide");
        assert_eq!(format("Inserted {} slides", &["3"]), "Inserted 3 slides");
        assert_eq!(tr("File (Alt+F)"), "File (Alt+F)");
        assert_eq!(tr("SmartArt Design"), "SmartArt Design");
        assert_eq!(tr("Zoom on"), "Zoom on");
    }

    #[test]
    fn test_unique_catalog_keys() {
        let mut seen = std::collections::HashSet::new();
        for line in CATALOG_PT_BR.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            if let Some((key, _)) = line.split_once('\t') {
                assert!(seen.insert(key), "duplicate translation key in catalog: {key}");
            }
        }
    }

    #[test]
    fn test_format_and_count() {
        set_current(Language::PtBr);
        assert_eq!(recovered(1), "Recuperadas 1 apresentação não salva da última sessão");
        assert_eq!(recovered(3), "Recuperadas 3 apresentações não salvas da última sessão");
        set_current(Language::En);
    }

    #[test]
    fn test_catalog_integrity() {
        assert!(catalog().len() > 1000);
        for (k, v) in catalog().iter() {
            assert!(!k.is_empty(), "empty key found");
            assert!(!v.is_empty(), "empty translation found for key: {k}");
        }
    }

    #[test]
    fn test_language_resolution() {
        assert_eq!(Language::from_locale("pt-BR"), Language::PtBr);
        assert_eq!(Language::from_locale("pt_PT"), Language::PtBr);
        assert_eq!(Language::from_locale("en-US"), Language::En);
        assert_eq!(Language::from_code("pt-br"), Some(Language::PtBr));
        assert_eq!(Language::from_code("en"), Some(Language::En));
        assert_eq!(Language::PtBr.code(), "pt-BR");
        assert_eq!(Language::En.code(), "en");
    }
}
