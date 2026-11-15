//! The pane against a made-up app whose Library holds pages, with a tab of its own: nothing here is
//! DeckCraft's.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use super::*;

/// A 16×9 PNG.
fn png() -> String {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 9, image::Rgba([40, 90, 200, 255])).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    craft_library::base64(bytes.get_ref())
}

#[derive(Default)]
struct Pages {
    folders: Vec<String>,
    acted: Vec<(String, Value)>,
    asked: Vec<String>,
    picker: bool,
}

impl Host for Pages {
    fn act(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.acted.push((id.to_string(), params.clone()));
        if id == "library.addFolder" {
            let path = params["path"].as_str().unwrap_or_default().to_string();
            self.folders.push(path.clone());
            return Ok(json!({"path": path, "added": true}));
        }
        Ok(json!({}))
    }
    fn ask(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.asked.push(id.to_string());
        let pages = json!([{"index": 0, "title": "Cover", "hidden": false}, {"index": 1, "title": "Terms", "hidden": true}]);
        match id {
            "library.list" if params.get("file").is_some() => Ok(json!([{"file": "/lib/contract.doc", "title": "Contract", "pages": pages}])),
            "library.list" => Ok(json!([{"file": "/lib/contract.doc", "folder": "/lib", "title": "contract"}])),
            "library.search" => Ok(json!([{"file": "/lib/contract.doc", "page": 1, "title": "Terms", "match": "title"}])),
            "library.thumbnail" => Ok(json!({"png": png(), "width": 16, "height": 9})),
            "library.shapes" => Ok(json!({"shapes": [{"name": "Signature block"}, {"name": "Seal"}, {"name": "Inner", "parent": "Seal"}]})),
            _ => Err(format!("no {id}")),
        }
    }
    fn folders(&self) -> Vec<String> {
        self.folders.clone()
    }
    fn pick_folder(&mut self) -> Option<Option<String>> {
        self.picker.then(|| Some("/picked".to_string()))
    }
    fn can_place(&self) -> bool {
        true
    }
    fn report(&mut self, _message: String) {}
    fn tab(&mut self, ui: &mut Ui, id: &str) {
        ui.label(format!("the app's own {id} tab"));
    }
}

const LABELS: Labels = Labels {
    nouns: Nouns { one: "page", many: "pages" },
    tabs: &[(ITEMS, "Pages"), ("stamps", "Stamps")],
    item: "Page",
    insert_item: "Insert page",
    insert_shapes: None,
    insert_element: Some("Insert element"),
    search_hint: "Search pages",
    add_folder_tip: "Show the pages of a folder of documents",
    empty: "Add a folder of documents to see their pages here.",
    not_in_browser: "Not in the browser.",
    aspect: 1.4,
};

fn harness(host: Pages) -> Harness<'static, (State, Pages)> {
    let mut h = Harness::builder().with_size(egui::vec2(320.0, 900.0)).build_ui_state(
        |ui, s: &mut (State, Pages)| {
            let pal = Palette::from_visuals(ui.visuals());
            show(ui, &mut s.0, &mut s.1, &LABELS, &pal);
        },
        (State::default(), host),
    );
    h.run_steps(4);
    h
}

#[test]
fn an_app_with_pages_lists_reads_and_inserts_them() {
    let mut h = harness(Pages { folders: vec!["/lib".into()], ..Default::default() });
    assert!(h.state().0.ready());
    assert!(h.query_by_label("Page 1: Cover").is_some() && h.query_by_label("Page 2: Terms").is_some());
    assert!(h.state().1.asked.iter().any(|a| a == "library.thumbnail"), "thumbnails drawn: {:?}", h.state().1.asked);
    h.get_by_label("Page 2: Terms").click();
    h.run_steps(2);
    assert_eq!(h.state().1.acted.last(), Some(&("library.insert".to_string(), json!({"file": "/lib/contract.doc", "page": 1}))));
    // Search speaks the app's nouns too.
    h.state_mut().0.query = "terms".into();
    for _ in 0..30 {
        h.step();
    }
    assert_eq!(h.state().0.hits(), Some(1));
    assert!(h.query_by_label("Page 1: Cover").is_none());
}

#[test]
fn insert_element_lists_top_level_names_and_inserts_one() {
    let mut h = harness(Pages { folders: vec!["/lib".into()], ..Default::default() });
    let at = h.get_by_label("Page 1: Cover").rect().center();
    h.event(egui::Event::PointerMoved(at));
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        h.step();
    }
    h.run_steps(2);
    assert!(h.query_by_label("Insert shapes on current slide").is_none(), "an app without that insert doesn't offer it");
    h.get_by_label("Insert element").click();
    h.run_steps(2);
    assert!(h.query_by_label("Inner").is_none(), "grouped shapes aren't offered on their own here");
    h.get_by_label("Seal").click();
    h.run_steps(2);
    assert_eq!(h.state().1.acted.last(), Some(&("library.insert".to_string(), json!({"file": "/lib/contract.doc", "page": 0, "names": ["Seal"]}))));
}

#[test]
fn the_apps_own_tab_is_drawn_by_the_app() {
    let mut h = harness(Pages { folders: vec!["/lib".into()], ..Default::default() });
    h.get_by_label("Stamps").click();
    h.run_steps(2);
    assert_eq!(h.state().0.tab, "stamps");
    assert!(h.query_by_label("the app's own stamps tab").is_some());
}

#[test]
fn without_folders_the_pane_says_what_to_do_and_adds_one() {
    let mut h = harness(Pages::default());
    assert!(h.query_by_label_contains("Add a folder of documents").is_some());
    // No picker: a path field.
    h.get_by_label("Add Folder…").click();
    h.run_steps(2);
    assert!(h.state().0.adding);
    // With a picker: the picked folder is added and shown.
    let mut h = harness(Pages { picker: true, ..Default::default() });
    h.get_by_label("Add Folder…").click();
    h.run_steps(2);
    assert_eq!(h.state().1.folders, vec!["/picked".to_string()]);
    assert_eq!(h.state().0.folder.as_deref(), Some("/picked"));
}

#[test]
fn thumbnails_decode_and_hostile_ones_fail_quietly() {
    assert!(decode(&unbase64(&png()).unwrap()).is_some_and(|i| i.size == [16, 9]));
    assert!(decode(b"not a png").is_none());
    assert_eq!(unbase64("Zm9vYmFy"), Some(b"foobar".to_vec()));
    assert_eq!(unbase64("Zg=="), Some(b"f".to_vec()));
    assert!(unbase64("**").is_none());
}
