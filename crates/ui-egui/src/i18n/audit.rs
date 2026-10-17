//! Localisation audit: drives the interface through its main states in the pseudo-language (see
//! `pseudo.rs`) so every string that goes through the catalog is recorded, then checks the bundled
//! zh-Hans catalog has an entry for each. Run with `--nocapture` to print the keys the UI looked up
//! (`KEY<TAB>…`) and the ones still missing (`MISSING<TAB>…`).

use serde_json::json;

use super::pseudo;
use crate::{Services, SlideApp};

/// The ribbon's tabs (`ribbon::TABS`, plus the File tab).
const TABS: &[&str] = &["Home", "Insert", "Draw", "Design", "Transitions", "Animations", "Slide Show", "Record", "Review", "View"];
/// Every dialog `dialogs::title` names.
const DIALOGS: &[&str] = &[
    "table",
    "slideSize",
    "sizePosition",
    "headerFooter",
    "hyperlink",
    "setupShow",
    "zoom",
    "comment",
    "altText",
    "outline",
    "renameSection",
    "renameShape",
    "renameLayout",
    "paragraph",
    "tabs",
    "font",
    "about",
    "preferences",
    "autocorrect",
    "gridOptions",
    "export",
    "symbol",
    "photoAlbum",
    "replaceFonts",
    "equation",
    "dateTime",
    "spelling",
    "pasteSpecial",
    "find",
    "accessibility",
    "language",
    "customShow",
    "chartData",
    "trim",
    "smartart",
    "bullets",
    "action",
    "themeColors",
    "themeFonts",
];
const PANES: &[&str] = &["format", "animation", "selection", "comments", "designer", "background", "smartart"];
const BACKSTAGE: &[&str] = &["home", "new", "open", "info", "saveAs", "print", "export", "account"];

fn harness() -> egui_kittest::Harness<'static, SlideApp> {
    let app = SlideApp::new(deckcraft_engine::Session::with_new(), Services::default());
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1600.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

#[test]
fn the_catalog_covers_every_string_the_interface_looks_up() {
    pseudo::set(true);
    for tab in TABS {
        let mut h = harness();
        let _ = h.state_mut().run("view.tab", json!({"tab": tab}));
        h.run_steps(3);
    }
    for page in BACKSTAGE {
        let mut h = harness();
        let _ = h.state_mut().run("app.backstage", json!({"page": page, "on": true}));
        h.run_steps(3);
    }
    for id in DIALOGS {
        let mut h = harness();
        let _ = h.state_mut().run("app.dialog", json!({"id": id}));
        h.run_steps(3);
    }
    for pane in PANES {
        let mut h = harness();
        let _ = h.state_mut().run("view.pane", json!({"pane": pane}));
        h.run_steps(3);
    }
    let keys = pseudo::looked_up();
    pseudo::set(false);

    let lang = super::Lang::from_code("zh-hans").unwrap_or(super::Lang::EN);
    let missing: Vec<&String> = keys.iter().filter(|k| !super::has(lang, k)).collect();
    println!("LOOKED_UP_COUNT={}", keys.len());
    println!("MISSING_COUNT={}", missing.len());
    for k in &missing {
        println!("MISSING\t{}", k.replace('\n', "\\n"));
    }
    assert!(missing.is_empty(), "{} interface strings have no zh-hans catalog entry (see the MISSING lines)", missing.len());
}
