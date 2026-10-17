//! Add-ins in the window: their ribbon groups and tabs, the Developer tab with its macros bar,
//! the Add-ins window, and an Office add-in's task pane talking to the presentation.

use std::sync::{Arc, Mutex};

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value as Json, json};

use crate::office_pane::OfficeHost;
use crate::{Services, SlideApp};

fn harness(s: Session, services: Services) -> Harness<'static, SlideApp> {
    let app = SlideApp::new(s, services);
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

/// Add-ins are loaded process-wide: the tests that load one take turns.
static ADDINS: Mutex<()> = Mutex::new(());

/// Unloads the add-in however the test ends, so other tests never see it.
struct Unload(&'static str);

impl Drop for Unload {
    fn drop(&mut self) {
        deckcraft_engine::plugin::unload(self.0);
    }
}

#[test]
fn an_add_ins_buttons_and_the_developer_tab_with_its_macros_bar() {
    let _one_at_a_time = ADDINS.lock().unwrap_or_else(|e| e.into_inner());
    let m = deckcraft_engine::plugin::Manifest::parse(
        &json!({
            "id": "t.ui-addin", "name": "UI Test", "macros": {"language": "VBA"}, "run": {"command": "./not-started"},
            "commands": [{"id": "go", "label": "Go Now"}],
            "ui": {"ribbon": [{"tab": "Example", "group": "Mine", "buttons": [{"command": "go"}]}, {"tab": "Insert", "group": "Mine", "buttons": [{"command": "go", "label": "Go Here", "size": "small"}]}]}
        })
        .to_string(),
    )
    .unwrap();
    deckcraft_engine::plugin::install(m, std::env::temp_dir()).unwrap();
    let _unload = Unload("t.ui-addin");
    assert!(super::developer_visible());
    assert!(super::extra_tabs(&["Insert"]).contains(&"Example".to_string()));
    let mut s = Session::with_new();
    s.edit(|doc, _| {
        doc.vba_project = Some(Arc::new(b"project".to_vec()));
        Ok(())
    })
    .unwrap();
    let mut h = harness(s, Services::default());
    // The macros bar, until Enable Content.
    assert!(h.query_by_label("Enable Content").is_some(), "a presentation with macros opens with them disabled");
    h.get_by_label("Enable Content").click();
    h.run_steps(3);
    assert!(deckcraft_engine::cmd::developer::macros_allowed(&h.state().session));
    assert!(h.query_by_label("Enable Content").is_none());
    // The add-in's buttons: on Insert, and on its own tab.
    h.state_mut().ui.tab = "Insert".into();
    h.run_steps(3);
    assert!(h.query_by_label("Go Here").is_some());
    assert!(h.query_by_label("Add-ins").is_some());
    h.state_mut().ui.tab = "Example".into();
    h.run_steps(3);
    assert!(h.query_by_label("Go Now").is_some());
    // Pressing it runs the add-in's command (which can't start here) and says why.
    h.get_by_label("Go Now").click();
    h.run_steps(3);
    let said = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(said.contains("not-started") || said.contains("start"), "{said}");
    // Developer: Macro Security, and the Add-ins window listing the add-in.
    h.state_mut().ui.tab = "Developer".into();
    h.run_steps(3);
    h.get_by_label("Macro Security").click();
    h.run_steps(3);
    assert!(h.state().addins.security);
    h.state_mut().addins.security = false;
    h.get_by_label("Add-ins").click();
    h.run_steps(3);
    assert!(h.query_by_label("UI Test").is_some(), "the Add-ins window lists it");
    h.get_by_label("Deactivate").click();
    h.run_steps(3);
    assert!(!deckcraft_engine::plugin::get("t.ui-addin").unwrap().enabled());
    h.state_mut().ui.tab = "Example".into();
    h.run_steps(3);
    assert!(h.query_by_label("Go Now").is_none(), "an add-in turned off takes its buttons away");
}

#[test]
fn a_presentations_ribbon_buttons_show_on_their_tabs_and_run_their_macros() {
    use deckcraft_model::{Ribbon, RibbonButton, RibbonGroup, RibbonTab};
    // No macro add-in loaded meanwhile: the buttons show without one.
    let _one_at_a_time = ADDINS.lock().unwrap_or_else(|e| e.into_inner());
    let button = |id: &str, label: &str| RibbonButton {
        id: id.into(),
        label: label.into(),
        on_action: format!("Module1.{label}"),
        large: true,
        ..Default::default()
    };
    let group = |b| vec![RibbonGroup { id: "g".into(), label: "G".into(), buttons: vec![b] }];
    let mut s = Session::with_new();
    s.edit(|doc, _| {
        doc.ribbon = Some(Ribbon {
            tabs: vec![
                RibbonTab { id: "tTools".into(), label: "Tools".into(), groups: group(button("b1", "Go")), ..Default::default() },
                RibbonTab { builtin: Some("TabHome".into()), groups: group(button("b2", "Hello")), ..Default::default() },
            ],
        });
        Ok(())
    })
    .unwrap();
    let mut h = harness(s, Services::default());
    // On Home, after Home's own groups.
    assert!(h.query_by_label("Hello").is_some(), "the presentation's button on Home");
    assert!(h.query_by_label("Go").is_none());
    h.state_mut().ui.tab = "Tools".into();
    h.run_steps(3);
    assert_eq!(h.state().ui.tab, "Tools", "the presentation's own tab joins the tab row (an unknown one falls back to Home)");
    h.get_by_label("Go").click();
    h.run_steps(3);
    // Macros start disabled: the click says so instead of running.
    let said = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(said.contains("disabled"), "{said}");
    // Developer › Ribbon Buttons adds one more, saved in the presentation.
    super::open_buttons(h.state_mut(), Some("Module1.Report"));
    h.run_steps(3);
    h.get_by_label("Add").click();
    h.run_steps(3);
    let ribbon = h.state().session.active().unwrap().doc.ribbon.clone().unwrap();
    let added = ribbon.tabs.iter().find(|t| t.label == "Macros").and_then(|t| t.groups.first()).and_then(|g| g.buttons.first()).cloned();
    assert_eq!(added.map(|b| (b.label, b.on_action, b.image)), Some(("Report".into(), "Module1.Report".into(), Some("MacroPlay".into()))));
    h.state_mut().addins.buttons = None;
    h.state_mut().ui.tab = "Macros".into();
    h.run_steps(3);
    assert!(h.query_by_label("Report").is_some());
}

#[test]
fn record_macro_toggles_on_the_developer_tab() {
    let _one_at_a_time = ADDINS.lock().unwrap_or_else(|e| e.into_inner());
    let m = deckcraft_engine::plugin::Manifest::parse(
        &json!({"id": "t.ui-recorder", "name": "Recorder Test", "macros": {"language": "VBA"}, "run": {"command": "./not-started"}}).to_string(),
    )
    .unwrap();
    deckcraft_engine::plugin::install(m, std::env::temp_dir()).unwrap();
    let _unload = Unload("t.ui-recorder");
    let mut h = harness(Session::with_new(), Services::default());
    h.state_mut().ui.tab = "Developer".into();
    h.run_steps(3);
    assert!(h.query_by_label("Ribbon Buttons").is_some());
    h.get_by_label("Record Macro").click();
    h.run_steps(3);
    assert!(h.state().session.recording.is_some());
    // Stopping asks the add-in for the code; this one can't start, which the status says.
    h.get_by_label("Stop Recording").click();
    h.run_steps(3);
    assert!(h.state().session.recording.is_none());
    assert!(h.query_by_label("Record Macro").is_some());
    let said = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(said.contains("not-started") || said.contains("start"), "{said}");
}

/// A web view host that records what it was asked and lets the test post a page's messages.
#[derive(Clone, Default)]
struct FakeWeb(Arc<Mutex<Web>>);

#[derive(Default)]
struct Web {
    shown: Vec<(String, String)>,
    inbox: Vec<(String, String)>,
    replies: Vec<(String, String)>,
    hidden: Vec<String>,
    hidden_pages: Vec<(String, String)>,
    /// (page, entry point, argument).
    calls: Vec<(String, String, String)>,
}

impl OfficeHost for FakeWeb {
    fn show(&mut self, key: &str, url: &str, _rect: egui::Rect, _ppp: f32) {
        let mut w = self.0.lock().unwrap();
        if !w.shown.iter().any(|(k, _)| k == key) {
            w.shown.push((key.into(), url.into()));
        }
    }
    fn hide(&mut self, key: &str) {
        self.0.lock().unwrap().hidden.push(key.into());
    }
    fn close(&mut self, _key: &str) {}
    fn load_hidden(&mut self, key: &str, url: &str) {
        let mut w = self.0.lock().unwrap();
        if !w.hidden_pages.iter().any(|(k, _)| k == key) {
            w.hidden_pages.push((key.into(), url.into()));
        }
    }
    fn call(&mut self, key: &str, entry: &str, json: &str) {
        self.0.lock().unwrap().calls.push((key.into(), entry.into(), json.into()));
    }
    fn take_messages(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.0.lock().unwrap().inbox)
    }
    fn reply(&mut self, key: &str, json: &str) {
        self.0.lock().unwrap().replies.push((key.into(), json.into()));
    }
}

#[test]
fn an_office_add_ins_task_pane_reads_and_writes_the_presentation() {
    let _one_at_a_time = ADDINS.lock().unwrap_or_else(|e| e.into_inner());
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?>
<OfficeApp xmlns="http://schemas.microsoft.com/office/appforoffice/1.1" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="TaskPaneApp">
  <Id>0d5c7a1e-0000-4bcd-9eaf-0000000000aa</Id><Version>1.0.0.0</Version><ProviderName>Tests</ProviderName><DefaultLocale>en-US</DefaultLocale>
  <DisplayName DefaultValue="Pane Test"/><Description DefaultValue="Tests"/>
  <Hosts><Host Name="Presentation"/></Hosts>
  <DefaultSettings><SourceLocation DefaultValue="https://tests.example/pane.html"/></DefaultSettings>
  <Permissions>ReadWriteDocument</Permissions>
</OfficeApp>"#;
    let m = deckcraft_engine::plugin::office::parse_manifest(manifest, deckcraft_engine::plugin::HOST_ID).unwrap();
    let id = m.id.clone();
    deckcraft_engine::plugin::install(m, std::env::temp_dir()).unwrap();
    struct UnloadId(String);
    impl Drop for UnloadId {
        fn drop(&mut self) {
            deckcraft_engine::plugin::unload(&self.0);
        }
    }
    let _unload = UnloadId(id.clone());
    let web = FakeWeb::default();
    let services = Services { office: Some(Box::new(web.clone())), ..Default::default() };
    let mut h = harness(Session::with_new(), services);
    let _ = h.state_mut().run("plugins.run", json!({"plugin": id, "command": "taskpane"}));
    h.run_steps(3);
    let key = web.0.lock().unwrap().shown.first().cloned().map(|(k, url)| {
        assert_eq!(url, "https://tests.example/pane.html");
        k
    });
    let key = key.expect("the pane's page was shown");
    let slides = h.state().session.active().unwrap().doc.slides.len();
    // One context.sync(): add a slide, then count.
    let msg = json!({"id": 7, "actions": [
        {"op": "call", "path": [{"p": "presentation"}, {"p": "slides"}], "method": "add", "args": [{}]},
        {"op": "load", "path": [{"p": "presentation"}, {"p": "slides"}], "props": ["count"]}
    ]});
    web.0.lock().unwrap().inbox.push((key.clone(), msg.to_string()));
    h.run_steps(2);
    let (to, reply) = web.0.lock().unwrap().replies.last().cloned().unwrap();
    assert_eq!(to, key);
    let reply: Json = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["results"][1]["count"], json!(slides + 1), "{reply}");
    assert_eq!(h.state().session.active().unwrap().doc.slides.len(), slides + 1);
    // The File tab hides the page (a native web view would cover it).
    h.state_mut().open_backstage(crate::backstage::Page::Home);
    h.run_steps(2);
    assert!(web.0.lock().unwrap().hidden.contains(&key));
}

/// The add-in the dialog test talks to: this test binary started again by the plugin host, which
/// sets `CRAFT_PLUGIN_DIR` (a normal test run returns at once). libtest's own lines on standard
/// output aren't JSON, so the host skips them.
#[test]
fn serve_dialog_addin() {
    use std::io::{BufRead, Write};
    let Some(dir) = std::env::var_os("CRAFT_PLUGIN_DIR").map(std::path::PathBuf::from) else { return };
    if !dir.to_string_lossy().contains("deckcraft-ui-dialog-addin") {
        return;
    }
    let mut out = std::io::stdout();
    // Ends libtest's "test … " line, so the answers start on lines of their own.
    writeln!(out).unwrap();
    for line in std::io::stdin().lock().lines() {
        let Ok(msg) = serde_json::from_str::<Json>(&line.unwrap()) else { continue };
        let Some(id) = msg.get("id").cloned() else { break };
        let p = &msg["params"];
        let result = match (msg["method"].as_str().unwrap_or(""), p["command"].as_str().unwrap_or("")) {
            ("commands/run", "browse") => match p["params"]["id"].as_str() {
                None => json!({"dialog": {
                    "title": "Browse the cloud", "kind": "list", "prompt": "Documents",
                    "items": [{"id": "/docs/", "label": "docs", "detail": "Folder", "icon": "folder"}, {"id": "/a.pptx", "label": "a.pptx", "icon": "presentation"}],
                    "command": "browse", "okLabel": "Go there",
                    "buttons": [{"label": "Sign in", "command": "login"}]
                }}),
                Some("/docs/") => json!({"dialog": {
                    "title": "In docs", "kind": "list", "items": [{"id": "/docs/b.deckcraft", "label": "b.deckcraft", "detail": "2 KB"}], "command": "browse"
                }}),
                Some(_) => json!({"open": {"path": dir.join("b.deckcraft"), "label": "Test cloud · docs/b.deckcraft"}, "message": "fetched"}),
            },
            ("commands/run", "login") => match p["params"]["fields"].as_object() {
                None => json!({"dialog": {"title": "Sign in", "kind": "form", "command": "login", "fields": [
                    {"id": "user", "label": "User name"},
                    {"id": "pw", "label": "Password", "kind": "secret", "value": "hunter2"},
                    {"id": "keep", "label": "Stay signed in", "kind": "checkbox", "value": true}
                ]}}),
                Some(f) => json!({"message": format!("hello {} {} {}", f["user"].as_str().unwrap_or(""), f["pw"].as_str().unwrap_or(""), f["keep"])}),
            },
            _ => json!({}),
        };
        writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "result": result})).unwrap();
        out.flush().unwrap();
    }
}

#[test]
fn an_add_ins_dialogs_chain_and_open_files() {
    let _one_at_a_time = ADDINS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("deckcraft-ui-dialog-addin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The file the add-in hands over.
    let mut deck = Session::with_new();
    deck.execute("slide.new", &json!({})).unwrap();
    deck.execute("file.saveAs", &json!({"path": dir.join("b.deckcraft")})).unwrap();
    let m = deckcraft_engine::plugin::Manifest::parse(
        &json!({
            "id": "t.dialogs", "name": "Dialogs",
            "run": {"command": std::env::current_exe().unwrap(), "args": ["--exact", "addins::tests::serve_dialog_addin", "--nocapture", "--test-threads=1"]},
            "commands": [{"id": "browse", "label": "Browse"}, {"id": "login", "label": "Sign In"}],
            "ui": {"ribbon": [{"tab": "Cloud", "group": "Files", "buttons": [{"command": "browse", "icon": "folder"}]}]}
        })
        .to_string(),
    )
    .unwrap();
    deckcraft_engine::plugin::install(m, dir.clone()).unwrap();
    let _unload = Unload("t.dialogs");
    let mut h = harness(Session::with_new(), Services::default());
    h.state_mut().ui.tab = "Cloud".into();
    h.run_steps(3);
    // The ribbon button's answer is a dialog, drawn by DeckCraft.
    h.get_by_label("Browse").click();
    h.run_steps(3);
    assert!(h.query_by_label("Browse the cloud").is_some(), "the add-in's dialog shows");
    assert!(h.query_by_label("a.pptx").is_some());
    // OK without a choice says so and stays.
    h.get_by_label("Go there").click();
    h.run_steps(3);
    assert!(h.query_by_label("Choose an item first").is_some());
    // A folder, then OK: the add-in answers with the next dialog (dialogs chain).
    h.get_by_label("docs").click();
    h.run_steps(2);
    h.get_by_label("Go there").click();
    h.run_steps(3);
    assert!(h.query_by_label("In docs").is_some(), "the next dialog");
    assert!(h.query_by_label("Browse the cloud").is_none());
    // A file, then OK: the add-in answers `open`, which opens the file as File › Open does.
    let docs = h.state().session.documents().len();
    h.get_by_label("b.deckcraft").click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert!(h.state().addins.dialog.is_none(), "the dialog closed");
    for _ in 0..500 {
        h.run_steps(1);
        if h.state().opening.is_none() && h.state().session.documents().len() > docs {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(h.state().session.documents().len(), docs + 1);
    let opened = h.state().session.active().unwrap();
    assert_eq!(opened.path.as_deref(), Some(dir.join("b.deckcraft").to_string_lossy().as_ref()));
    assert_eq!(opened.location_label.as_deref(), Some("Test cloud · docs/b.deckcraft"), "the label is its location");
    // A dialog's extra button runs its command: a form, whose OK sends the fields.
    h.get_by_label("Browse").click();
    h.run_steps(3);
    h.get_by_label("Sign in").click();
    h.run_steps(3);
    assert!(h.query_by_label("Stay signed in").is_some(), "the form");
    h.get_by_label("User name").click();
    h.run_steps(2);
    h.get_by_label("User name").type_text("ada");
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(3);
    let said = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert_eq!(said, "hello ada hunter2 true");
    assert!(h.state().addins.dialog.is_none());
    // Cancel just closes.
    h.get_by_label("Browse").click();
    h.run_steps(3);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(h.state().addins.dialog.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_add_ins_dialog_is_bounded_and_filters_long_lists() {
    use deckcraft_engine::plugin::Dialog;
    let items: Vec<Json> = (0..30).map(|i| json!({"id": i.to_string(), "label": format!("Item {i}"), "icon": "no-such-icon"})).collect();
    let d = Dialog::parse(&json!({"kind": "list", "items": items, "command": "go", "multi": true})).unwrap();
    let mut pd = super::PluginDialog::new("t.x", d);
    assert_eq!(pd.visible().len(), 30);
    pd.filter = "item 2".into();
    assert_eq!(pd.visible().len(), 11, "Item 2 and Item 20–29");
    pd.click(3);
    pd.click(5);
    pd.click(3);
    assert_eq!(pd.selected_ids(), ["5"], "multi toggles");
    assert_eq!(pd.action(None).unwrap().unwrap().1, json!({"ids": ["5"]}));
    assert_eq!(super::item_icon("no-such-icon"), crate::icons::Icon::File, "unknown icons are a file");
    assert_eq!(super::item_icon("folder"), crate::icons::Icon::Folder);
    let form = Dialog::parse(&json!({"kind": "form", "command": "go", "fields": [{"id": "n", "label": "Count", "kind": "number"}]})).unwrap();
    let mut pf = super::PluginDialog::new("t.x", form);
    pf.values[0] = super::FieldValue::Text("lots".into());
    assert!(pf.action(None).unwrap_err().contains("Count"), "a number that isn't one doesn't run");
    pf.values[0] = super::FieldValue::Text("12".into());
    assert_eq!(pf.action(None).unwrap().unwrap(), ("go".to_string(), json!({"fields": {"n": 12}})));
    // A list's button runs with its own params plus the selected `id` (none without a selection).
    let list = Dialog::parse(&json!({"kind": "list", "items": [{"id": "/a"}, {"id": "/b"}], "command": "open",
        "buttons": [{"label": "Delete", "command": "remove", "params": {"account": "me"}}]}))
    .unwrap();
    let mut pl = super::PluginDialog::new("t.x", list);
    assert_eq!(pl.action(Some(0)).unwrap().unwrap(), ("remove".to_string(), json!({"account": "me"})));
    pl.click(1);
    assert_eq!(pl.action(Some(0)).unwrap().unwrap(), ("remove".to_string(), json!({"account": "me", "id": "/b"})));
    let msg = Dialog::parse(&json!({"kind": "message", "prompt": "Done"})).unwrap();
    assert_eq!(super::PluginDialog::new("t.x", msg).action(None).unwrap(), None, "a message's OK just closes");
}

#[test]
fn blocked_add_ins_say_why_and_offer_to_install_what_they_need() {
    let _one_at_a_time = ADDINS.lock().unwrap_or_else(|e| e.into_inner());
    let m = deckcraft_engine::plugin::Manifest::parse(
        r#"{"id": "t.ui-reports", "name": "Reports", "version": "1.4.0", "run": {"command": "./not-started"},
            "commands": [{"id": "go", "label": "Go"}],
            "ui": {"ribbon": [{"tab": "Insert", "group": "R", "buttons": [{"command": "go", "label": "Reports Button"}]}],
                   "menus": [{"menu": "shape", "command": "go"}]},
            "requires": {"addins": [
                {"id": "t.ui-vba", "version": "^0.1.4", "github": "sirmmo/craft-vba", "why": "Runs the report macros"},
                {"id": "t.ui-mqtt", "version": "*", "github": "sirmmo/craft-collab", "optional": true}]}}"#,
    )
    .unwrap();
    deckcraft_engine::plugin::install(m, std::env::temp_dir()).unwrap();
    let _unload = (Unload("t.ui-reports"), Unload("t.ui-vba"));
    assert!(super::groups("Insert").is_empty(), "a blocked add-in's ribbon group is absent");
    assert!(super::menu_items(deckcraft_engine::plugin::MenuKind::Shape).is_empty(), "and its menu items");
    let mut h = harness(Session::with_new(), Services::default());
    h.state_mut().addins.window = true;
    h.run_steps(3);
    assert!(h.query_by_label("Blocked · t.ui-reports").is_some());
    assert!(h.query_by_label("Needs craft-vba, which isn't installed").is_some());
    h.get_by_label("Install Missing").click();
    h.run_steps(3);
    let offer = h.state().addins.install.offer.as_ref().expect("Install Missing offers the missing add-ins");
    assert_eq!(offer.plugin, "t.ui-reports");
    let ticked: Vec<(&str, bool)> = offer.items.iter().map(|(m, on)| (m.id.as_str(), *on)).collect();
    assert_eq!(ticked, [("t.ui-vba", true), ("t.ui-mqtt", false)], "optional add-ins are offered unticked");
    assert!(h.query_by_label("Reports needs craft-vba ^0.1.4 (Runs the report macros). Install it too?").is_some());
    assert!(h.query_by_label("Reports can use craft-collab *. Install it too?").is_some());
    h.get_by_label("Not Now").click();
    h.run_steps(3);
    assert!(h.state().addins.install.offer.is_none());

    // Once the required add-in is there, the add-in unblocks; turning that one off asks first.
    let vba = deckcraft_engine::plugin::Manifest::parse(
        r#"{"id": "t.ui-vba", "name": "craft-vba", "version": "0.1.5", "run": {"command": "./not-started"}}"#,
    )
    .unwrap();
    deckcraft_engine::plugin::install(vba, std::env::temp_dir()).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Install Missing").is_none());
    assert_eq!(super::groups("Insert").len(), 1, "unblocked without a restart");
    let deactivate = h.query_all_by_label("Deactivate").last().expect("the second card's button");
    deactivate.click();
    h.run_steps(3);
    assert!(h.query_by_label("Reports needs craft-vba. Turn both off?").is_some());
    assert!(deckcraft_engine::plugin::get("t.ui-vba").unwrap().enabled(), "nothing is turned off before the answer");
    h.get_by_label("Turn Both Off").click();
    h.run_steps(3);
    assert!(!deckcraft_engine::plugin::get("t.ui-vba").unwrap().enabled());
    assert!(!deckcraft_engine::plugin::get("t.ui-reports").unwrap().enabled());
}

/// The `__craftOfficeRunFunction` calls made so far, as JSON.
fn run_calls(web: &FakeWeb) -> Vec<(String, Json)> {
    web.0
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|(_, e, _)| e == "__craftOfficeRunFunction")
        .map(|(k, _, j)| (k.clone(), serde_json::from_str(j).unwrap()))
        .collect()
}

#[test]
fn an_office_add_ins_function_command_runs_in_its_hidden_page() {
    let _one_at_a_time = ADDINS.lock().unwrap_or_else(|e| e.into_inner());
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?>
<OfficeApp xmlns="http://schemas.microsoft.com/office/appforoffice/1.1" xmlns:bt="http://schemas.microsoft.com/office/officeappbasictypes/1.0" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="TaskPaneApp">
  <Id>0d5c7a1e-0000-4bcd-9eaf-0000000000ab</Id><Version>1.0.0.0</Version><ProviderName>Tests</ProviderName><DefaultLocale>en-US</DefaultLocale>
  <DisplayName DefaultValue="Function Test"/><Description DefaultValue="Tests"/>
  <Hosts><Host Name="Presentation"/></Hosts>
  <DefaultSettings><SourceLocation DefaultValue="https://tests.example/pane.html"/></DefaultSettings>
  <Permissions>ReadWriteDocument</Permissions>
  <VersionOverrides xmlns="http://schemas.microsoft.com/office/taskpaneappversionoverrides" xsi:type="VersionOverridesV1_0">
    <Hosts><Host xsi:type="Presentation"><DesktopFormFactor>
      <FunctionFile resid="Commands.Url"/>
      <ExtensionPoint xsi:type="PrimaryCommandSurface"><OfficeTab id="TabHome"><Group id="T.Group"><Label resid="L"/>
        <Control xsi:type="Button" id="Tests.Write"><Label resid="L"/><Action xsi:type="ExecuteFunction"><FunctionName>writeValue</FunctionName></Action></Control>
      </Group></OfficeTab></ExtensionPoint>
    </DesktopFormFactor></Host></Hosts>
    <Resources>
      <bt:Urls><bt:Url id="Commands.Url" DefaultValue="https://tests.example/commands.html"/></bt:Urls>
      <bt:ShortStrings><bt:String id="L" DefaultValue="Write"/></bt:ShortStrings>
    </Resources>
  </VersionOverrides>
</OfficeApp>"#;
    let m = deckcraft_engine::plugin::office::parse_manifest(manifest, deckcraft_engine::plugin::HOST_ID).unwrap();
    let id = m.id.clone();
    let command = m.office.as_ref().unwrap().commands.keys().next().cloned().unwrap();
    deckcraft_engine::plugin::install(m, std::env::temp_dir()).unwrap();
    struct UnloadId(String);
    impl Drop for UnloadId {
        fn drop(&mut self) {
            deckcraft_engine::plugin::unload(&self.0);
        }
    }
    let _unload = UnloadId(id.clone());
    let web = FakeWeb::default();
    let services = Services { office: Some(Box::new(web.clone())), ..Default::default() };
    let mut h = harness(Session::with_new(), services);
    // Clicked twice before the page has loaded: the page is loaded once, nothing is called yet.
    for _ in 0..2 {
        h.state_mut().run("plugins.run", json!({"plugin": id, "command": command})).unwrap();
        h.run_steps(2);
    }
    let pages = web.0.lock().unwrap().hidden_pages.clone();
    assert_eq!(pages.len(), 1);
    let (key, url) = pages[0].clone();
    assert_eq!(url, "https://tests.example/commands.html");
    assert!(run_calls(&web).is_empty(), "not before the page is ready");
    // The page is ready: both calls, with the manifest's control id.
    web.0.lock().unwrap().inbox.push((key.clone(), json!({"event": "ready"}).to_string()));
    h.run_steps(2);
    let calls = run_calls(&web);
    assert_eq!(calls.len(), 2);
    assert_eq!(
        (calls[0].0.as_str(), calls[0].1["name"].as_str(), calls[0].1["control"].as_str()),
        (key.as_str(), Some("writeValue"), Some("Tests.Write"))
    );
    assert_eq!(h.state().office.calls.len(), 2);
    // functionDone ends a call; one with an error says so in the status bar. Events are never
    // answered as syncs.
    web.0.lock().unwrap().inbox.push((key.clone(), json!({"event": "functionDone", "call": calls[0].1["call"]}).to_string()));
    web.0.lock().unwrap().inbox.push((key.clone(), json!({"event": "functionDone", "call": calls[1].1["call"], "error": "no cell"}).to_string()));
    h.run_steps(2);
    assert!(h.state().office.calls.is_empty());
    let said = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(said.contains("writeValue") && said.contains("no cell"), "{said}");
    assert!(web.0.lock().unwrap().replies.is_empty());
    // A call that never completes is given up on after the time-out.
    h.state_mut().run("plugins.run", json!({"plugin": id, "command": command})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().office.calls.len(), 1);
    let now = crate::now_ms() + crate::office_pane::FUNCTION_TIMEOUT_MS + 1.0;
    crate::office_pane::expire_calls(h.state_mut(), now);
    assert!(h.state().office.calls.is_empty());
    let said = h.state().status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    assert!(said.contains("Function Test") && said.contains("didn't finish"), "{said}");
    // The selection changing is told to the ready pages.
    web.0.lock().unwrap().calls.clear();
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.run_steps(2);
    let events: Vec<(String, String, String)> = web.0.lock().unwrap().calls.iter().filter(|(_, e, _)| e == "__craftOfficeEvent").cloned().collect();
    assert_eq!(events, vec![(key.clone(), "__craftOfficeEvent".to_string(), json!({"type": "documentSelectionChanged"}).to_string())]);
}
