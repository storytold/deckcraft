//! The Equation tab's galleries: structure templates (fractions, scripts, radicals, integrals,
//! large operators, brackets, functions, accents, limits and logs, operators, matrices), symbol
//! sets and ready-made equations.
//!
//! The data is WordCraft's (`math_gallery.rs`, v0.5.0), exported once as equaSHion trees so a host
//! needs neither WordCraft's linear-format parser nor its model: each template is a [`Math`]
//! whose empty places are empty sequences, the form [`crate::Editor::insert_template`] takes.
//! `gallery.json` is that export (compact JSON of [`Galleries`]).

use std::sync::OnceLock;

use serde::Deserialize;

use crate::Math;

/// One template or ready-made equation.
#[derive(Debug, Clone, Deserialize)]
pub struct GalleryTemplate {
    pub id: String,
    pub label: String,
    /// The equation in Word's linear format (`⬚` marks an empty place), for tooltips.
    pub linear: String,
    pub math: Math,
}

/// A titled run of templates inside a gallery.
#[derive(Debug, Clone, Deserialize)]
pub struct Section {
    pub name: String,
    pub templates: Vec<GalleryTemplate>,
}

/// One structure gallery (a button of the Equation tab) with its sections.
#[derive(Debug, Clone, Deserialize)]
pub struct Gallery {
    pub id: String,
    pub label: String,
    /// The picture on the gallery's ribbon button.
    pub picture: Math,
    pub sections: Vec<Section>,
}

/// A named set of symbols (the Symbols gallery's categories).
#[derive(Debug, Clone, Deserialize)]
pub struct SymbolSet {
    pub name: String,
    pub chars: String,
}

/// Everything the Equation tab offers.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Galleries {
    pub structures: Vec<Gallery>,
    pub symbols: Vec<SymbolSet>,
    /// Ready-made equations (Insert › Equation), grouped by subject.
    pub builtins: Vec<GalleryTemplate>,
    /// The picture on the ready-made equations button.
    #[serde(rename = "equationPicture")]
    pub equation_picture: Math,
    /// TeX-style names of the symbols that have one (`α` is `alpha`), for tooltips.
    #[serde(rename = "symbolNames")]
    pub symbol_names: std::collections::HashMap<String, String>,
}

/// The galleries, parsed once.
pub fn galleries() -> &'static Galleries {
    static G: OnceLock<Galleries> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("gallery.json")).unwrap_or_default())
}

impl Galleries {
    /// A structure gallery by id.
    pub fn structure(&self, id: &str) -> Option<&Gallery> {
        self.structures.iter().find(|g| g.id == id)
    }

    /// A symbol's name (`alpha` for `α`), when it has one.
    pub fn symbol_name(&self, c: char) -> Option<&str> {
        self.symbol_names.get(c.to_string().as_str()).map(String::as_str)
    }

    /// Every structure template.
    pub fn templates(&self) -> impl Iterator<Item = &GalleryTemplate> {
        self.structures.iter().flat_map(|g| g.sections.iter().flat_map(|s| s.templates.iter()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Editor, run_text, to_linear};

    #[test]
    fn the_galleries_load_with_every_group() {
        let g = galleries();
        let ids: Vec<&str> = g.structures.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            ["fraction", "script", "radical", "integral", "largeOperator", "bracket", "function", "accent", "limitLog", "operator", "matrix"]
        );
        assert_eq!(g.templates().count(), 222);
        assert_eq!(g.symbols.len(), 8);
        assert_eq!(g.builtins.len(), 14);
    }

    #[test]
    fn template_ids_are_unique_and_none_is_empty() {
        let g = galleries();
        let mut seen = std::collections::HashSet::new();
        for t in g.templates().chain(g.builtins.iter()) {
            assert!(seen.insert(t.id.as_str()), "duplicate id {}", t.id);
            assert!(!t.math.body.is_empty(), "{} has no body", t.id);
            assert!(!to_linear(&t.math).is_empty(), "{} has no text", t.id);
        }
    }

    #[test]
    fn every_template_inserts_into_an_empty_equation() {
        for t in galleries().templates() {
            let mut e = Editor::new(Math::new(vec![]));
            e.insert_template(&t.math);
            assert!(!e.math().body.is_empty(), "{} inserted nothing", t.id);
        }
    }

    #[test]
    fn buttons_have_pictures_and_symbols_have_names() {
        let g = galleries();
        assert!(g.structures.iter().all(|s| !s.picture.body.is_empty()), "every gallery has a picture");
        assert!(!g.equation_picture.body.is_empty());
        assert_eq!(g.symbol_name('α'), Some("alpha"));
    }

    #[test]
    fn a_ready_made_equation_reads_back() {
        let pythagoras = galleries().builtins.iter().find(|t| t.id == "pythagoras").expect("pythagoras");
        assert_eq!(run_text(&pythagoras.math), "a^2+b^2=c^2");
    }
}
