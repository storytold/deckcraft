//! The Add-ins window: the install confirmation, My Add-ins and a plug-in command's form.

use deckcraft_engine::Session;
use deckcraft_engine::plugin::craft_plugin::bundle::Bundle;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

use super::{Page, obtain, open, run_command};
use crate::{Services, SlideApp};

/// `(module (memory (export "memory") 1) (func (export "craft_run") (param i32) (result i32) i32.const 0))`:
/// a plug-in that does nothing and succeeds.
const NOTHING: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic, version
    0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01, 0x7f, // type: (i32) -> i32
    0x03, 0x02, 0x01, 0x00, // function 0 has type 0
    0x05, 0x03, 0x01, 0x00, 0x01, // one memory, one page
    0x07, 0x16, 0x02, 0x06, b'm', b'e', b'm', b'o', b'r', b'y', 0x02, 0x00, 0x09, b'c', b'r', b'a', b'f', b't', b'_', b'r', b'u', b'n', 0x00,
    0x00, // exports
    0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x00, 0x0b, // code: i32.const 0
];

fn bundle(params: Value) -> String {
    let manifest = json!({
        "id": "io.example.window", "name": "Window Test", "version": "1.0.0", "main": "plugin.wasm",
        "apps": {"deckcraft": {"api": 6}}, "permissions": {"access": "write"},
        "commands": [{"id": "go", "label": "Go…", "params": params}]
    });
    deckcraft_engine::cmd::base64_encode(&Bundle { manifest: manifest.to_string(), wasm: NOTHING.to_vec(), assets: vec![] }.encode())
}

fn harness() -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::with_new(), Services::default()),
    );
    h.run_steps(3);
    h
}

#[test]
fn a_file_is_reviewed_then_installed_from_the_confirmation() {
    let mut h = harness();
    {
        let app = h.state_mut();
        open(app, Page::Get);
        let mut f = app.plugins_form.take().unwrap();
        obtain(app, &mut f, json!({"data": bundle(json!([])), "name": "window.craftplugin"}));
        assert!(f.review.is_some(), "{}", f.message);
        app.plugins_form = Some(f);
    }
    h.run_steps(3);
    assert!(h.query_by_label_contains("in no catalog").is_some(), "the confirmation says the file is in no catalog");
    assert!(h.query_by_label_contains("Can edit the presentation").is_some(), "and what it may do");
    assert!(h.state().session.plugins.list().is_empty(), "nothing is installed before the confirmation");
    h.query_by_label("Install").expect("the Install button").click();
    h.run_steps(3);
    assert_eq!(h.state().session.plugins.list().len(), 1);
    assert_eq!(h.state().plugins_form.as_ref().map(|f| f.page), Some(Page::Installed));
}

#[test]
fn a_command_with_parameters_asks_for_them_before_it_runs() {
    let mut h = harness();
    let params = json!([{"name": "count", "label": "How many", "type": "number", "default": 3}]);
    h.state_mut().run("plugin.install", json!({"data": bundle(params)})).unwrap();
    run_command(h.state_mut(), "io.example.window", "go");
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("pluginParams"));
    assert!(h.query_by_label("How many").is_some(), "the field is labelled from the manifest");
    h.query_by_label("Run").expect("the Run button").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "the form closes after the run");
    let last = h.state().session.plugins.last.clone().expect("it ran");
    assert_eq!((last.0.as_str(), last.1.as_str(), last.2["count"].as_f64()), ("io.example.window", "go", Some(3.0)));
}

#[test]
fn get_add_ins_opens_the_window_on_its_get_page() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.run("app.addins", json!({"page": "get"})).unwrap();
    assert_eq!(app.dialog.as_ref().map(|d| d.id.as_str()), Some("plugins"));
    assert_eq!(app.plugins_form.as_ref().map(|f| f.page), Some(Page::Get));
    assert!(app.run("app.addins", json!({"page": "elsewhere"})).is_err());
}
