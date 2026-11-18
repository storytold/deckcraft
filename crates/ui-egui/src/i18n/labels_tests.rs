//! Labels built from a number or a name (#2014) are looked up with their `{placeholder}`s before
//! the value goes in, and dialog choices are looked up where they are drawn, so both show in the
//! interface language.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::*;
use crate::dialogs::Dialog;
use crate::{Services, SlideApp};

/// German rows for the labels these tests look at, in craft-i18n's format.
const DE: &str = "@id\tlanguage.name\tDeutsch
\tSlide {n}\tFolie {n}
\tAccept All ({n})\tAlle annehmen ({n})
\tNo synonyms for \"{word}\".\tKeine Synonyme für „{word}“.
\tFit\tEinpassen
\tWeb Page or File\tWebseite oder Datei
\tThis Document\tDieses Dokument
\tFind Next\tWeitersuchen
\tDeckCraft can't find “{text}”.\tDeckCraft findet „{text}“ nicht.
\tMatch {n} of {total} on slide {slide}.\tTreffer {n} von {total} auf Folie {slide}.
@plural\t{n} Column|{n} Columns\t{n} Spalte|{n} Spalten
@plural\t{name} ({n} slide)|{name} ({n} slides)\t{name} ({n} Folie)|{name} ({n} Folien)
";

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    )
}

#[test]
fn a_label_is_looked_up_before_its_values_are_filled_in() {
    let de = fixture_lang("de", DE);
    set_current(de);
    assert_eq!(fmt(tl!("Accept All ({n})"), &[("n", "4")]), "Alle annehmen (4)");
    // A value that looks like a placeholder is not filled in again.
    assert_eq!(fmt(tl!("No synonyms for \"{word}\"."), &[("word", "{n}")]), "Keine Synonyme für „{n}“.");
    // Counts take the language's plural form, also with a second placeholder.
    assert_eq!(tn(1, "{n} Column", "{n} Columns"), "1 Spalte");
    assert_eq!(tn(3, "{n} Column", "{n} Columns"), "3 Spalten");
    assert_eq!(fmt(&tn(2, "{name} ({n} slide)", "{name} ({n} slides)"), &[("name", "Kurz")]), "Kurz (2 Folien)");
    // Without a row, the English text comes back with the values in it.
    assert_eq!(fmt(tl!("Opening {name}…"), &[("name", "deck.pptx")]), "Opening deck.pptx…");
    set_current(Lang::EN);
    assert_eq!(tn(1, "{n} Column", "{n} Columns"), "1 Column");
    assert_eq!(fmt(tl!("Accept All ({n})"), &[("n", "4")]), "Accept All (4)");
}

/// The slide thumbnails' accessible names are built from the slide number.
#[test]
fn slide_thumbnails_are_named_in_the_interface_language() {
    use_fixture("de", DE);
    let mut h = harness(SlideApp::new(Session::with_new(), Services::default()));
    h.run_steps(3);
    assert!(h.query_by_label("Slide 1").is_some());
    h.state_mut().run("app.language", json!({"language": "de"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Slide 1").is_none(), "the English name is gone");
    assert!(h.query_by_label("Folie 1").is_some());
    h.state_mut().run("app.language", json!({"language": "en"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Slide 1").is_some());
}

/// Dialog choices show in the interface language; what they store stays the same.
#[test]
fn dialog_choices_are_drawn_in_the_interface_language() {
    use_fixture("de", DE);
    let mut h = harness(SlideApp::new(Session::with_new(), Services::default()));
    h.state_mut().run("app.language", json!({"language": "de"})).unwrap();
    h.state_mut().dialog = Some(Dialog::new("zoom"));
    h.run_steps(4);
    assert!(h.query_by_label("Fit").is_none());
    h.get_by_label("Einpassen").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.fields.get("mode")).map(String::as_str), Some("fit"), "the stored value stays English");

    h.state_mut().dialog = Some(Dialog::new("hyperlink"));
    h.run_steps(4);
    h.get_by_label("Webseite oder Datei");
    h.get_by_label("Dieses Dokument").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.fields.get("mode")).map(String::as_str), Some("slide"));
}

/// Find's messages carry the searched text or the match position: they are looked up first.
#[test]
fn find_reports_in_the_interface_language() {
    use_fixture("de", DE);
    let mut s = Session::with_new();
    s.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "Hello there"})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    let mut h = harness(SlideApp::new(s, Services::default()));
    h.state_mut().run("app.language", json!({"language": "de"})).unwrap();
    let status = |h: &Harness<'static, SlideApp>| h.state().dialog.as_ref().and_then(|d| d.fields.get("status").cloned());
    for (text, want) in [("zzz", "DeckCraft findet „zzz“ nicht."), ("there", "Treffer 1 von 1 auf Folie 1.")] {
        let mut d = Dialog::new("find");
        d.fields.insert("text".into(), text.into());
        h.state_mut().dialog = Some(d);
        h.run_steps(3);
        h.get_by_label("Weitersuchen").click();
        h.run_steps(3);
        assert_eq!(status(&h).as_deref(), Some(want));
    }
}

/// The strings the German smoke check on #2015 found in English: the closed Set Up Show choice,
/// the GitHub link, the dictionary mark in the Language list, the task pane title and the fixed
/// Accessibility Checker issues (#2235).
#[test]
fn strings_from_the_german_smoke_check_are_looked_up() {
    use_fixture(
        "de",
        "@id\tlanguage.name\tDeutsch
\tAll slides\tAlle Folien
\tSource on GitHub\tQuellcode auf GitHub
\t{name}  (dictionary)\t{name}  (Wörterbuch)
\tSpelling is checked for languages marked (dictionary).\tDie Rechtschreibung wird für Sprachen geprüft, die mit (Wörterbuch) markiert sind.
\tFormat Shape\tForm formatieren
\tMissing slide title\tFolientitel fehlt
",
    );
    let mut h = harness(SlideApp::new(Session::with_new(), Services::default()));
    h.state_mut().run("app.language", json!({"language": "de"})).unwrap();
    let show = |h: &mut Harness<'static, SlideApp>, id: &str| {
        h.state_mut().dialog = Some(Dialog::new(id));
        h.run_steps(4);
    };
    show(&mut h, "setupShow");
    assert!(h.query_by_value("All slides").is_none(), "the closed Slides choice");
    h.get_by_value("Alle Folien");
    show(&mut h, "about");
    h.get_by_label("Quellcode auf GitHub");
    show(&mut h, "language");
    assert!(h.query_all_by_label_contains("(dictionary)").next().is_none(), "the Language list's dictionary mark");
    assert!(h.query_all_by_label_contains("  (Wörterbuch)").next().is_some());
    show(&mut h, "accessibility");
    assert!(h.query_by_label_contains("Missing slide title").is_none(), "a fixed Accessibility Checker issue");
    assert!(h.query_all_by_label_contains("Folientitel fehlt").next().is_some());
    h.state_mut().dialog = None;
    h.state_mut().ui.pane = Some("format".into());
    h.run_steps(4);
    assert!(h.query_by_label("Format Shape").is_none(), "the task pane title");
    h.get_by_label("Form formatieren");
}
