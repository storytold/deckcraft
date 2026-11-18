//! Plug-in runs against a real session, with modules assembled from WebAssembly text.

use serde_json::{Value, json};

use crate::Session;
use crate::plugin::craft_plugin::bundle::Bundle;

/// A module that calls `calls` (command, params JSON) in order. A refused or failed call fails the
/// run when `strict`; otherwise its message becomes the output and the run goes on. `tail` is WAT
/// run after the calls (a trap, an endless loop), before returning 0.
fn module(calls: &[(&str, String)], strict: bool, tail: &str) -> Vec<u8> {
    let mut data = String::new();
    let mut body = String::new();
    let mut at = 0usize;
    for (command, params) in calls {
        let (c, p) = (at, at + command.len());
        at = p + params.len();
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        data.push_str(&format!(r#"(data (i32.const {c}) "{}") (data (i32.const {p}) "{}")"#, esc(command), esc(params)));
        let on_error = if strict {
            "(return (i32.const 1))".to_string()
        } else {
            "(call $set_output (i32.const 8192) (call $reply_read (i32.const 8192) (call $reply_len)))".to_string()
        };
        body.push_str(&format!(
            "(if (i32.ne (call $call (i32.const {c}) (i32.const {}) (i32.const {p}) (i32.const {})) (i32.const 0)) (then {on_error}))\n",
            command.len(),
            params.len()
        ));
    }
    let wat = format!(
        r#"(module
          (import "craft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
          (import "craft" "reply_len" (func $reply_len (result i32)))
          (import "craft" "reply_read" (func $reply_read (param i32 i32) (result i32)))
          (import "craft" "set_output" (func $set_output (param i32 i32)))
          (memory (export "memory") 1)
          {data}
          (func (export "craft_run") (param $n i32) (result i32) {body} {tail} (i32.const 0)))"#
    );
    wat::parse_str(&wat).unwrap_or_else(|e| panic!("{e}\n{wat}"))
}

/// The `.craftplugin` of plug-in `id` for DeckCraft, asking for `access`.
fn bundle(id: &str, access: &str, wasm: Vec<u8>) -> Vec<u8> {
    let manifest = json!({
        "id": id, "name": "Test Plug-in", "version": "1.0.0", "main": "plugin.wasm",
        "apps": {"deckcraft": {"api": 6}},
        "permissions": {"access": access},
        "commands": [{"id": "go", "label": "Go"}]
    });
    Bundle { manifest: manifest.to_string(), wasm, assets: vec![] }.encode()
}

fn data(bytes: &[u8]) -> String {
    crate::cmd::base64_encode(bytes)
}

/// A session with one new presentation open.
fn session() -> Session {
    Session::with_new()
}

/// A session with plug-in `id` installed for the session (no store, no network).
fn with_plugin(id: &str, access: &str, wasm: Vec<u8>) -> Session {
    let mut s = session();
    let r = s.execute("plugin.install", &json!({"data": data(&bundle(id, access, wasm)), "name": "test.craftplugin"})).unwrap();
    assert_eq!(r["installed"], true);
    assert_eq!(r["stored"], false, "without a store, a plug-in is kept for the session");
    s
}

fn shapes(s: &Session) -> usize {
    s.doc().unwrap().shapes().len()
}

fn steps(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

fn last_label(s: &Session) -> Option<String> {
    s.doc().unwrap().history.undo.last().map(|e| e.label.clone())
}

fn call(command: &str, params: Value) -> (&str, String) {
    (command, params.to_string())
}

fn rect(x: f64) -> Value {
    json!({"preset": "rect", "rect": [x, 40, 80, 60]})
}

#[test]
fn every_allowed_command_exists_and_none_is_always_refused() {
    use crate::plugin::craft_plugin::{ALWAYS_DENIED, permissions::pattern_matches};
    for id in super::ALLOWED {
        assert!(crate::find_command(id).is_some(), "{id} isn't a command");
        assert!(!ALWAYS_DENIED.iter().any(|p| pattern_matches(p, id)), "{id}");
        for refused in ["plugin.", "file.", "show.", "view.", "window.", "edit.undo", "edit.redo", "edit.copy", "edit.cut", "edit.paste", "font.add"]
        {
            assert!(!id.starts_with(refused), "{id}");
        }
    }
    for refused in ["insert.picture", "insert.video", "insert.audio", "insert.photoAlbum", "picture.change", "smartArt.picture", "slide.reuse"] {
        assert!(!super::ALLOWED.contains(&refused), "{refused}");
    }
    for id in crate::command_specs().iter().map(|c| c.id).filter(|id| !super::ALLOWED.contains(id)) {
        assert!(super::policy().refuses(id), "{id} is refused unless it is allowed");
    }
}

#[test]
fn a_write_plugin_runs_real_commands_as_one_undo_step_named_after_it() {
    let wasm = module(
        &[
            call("shape.insert", rect(40.0)),
            call("shape.insert", rect(160.0)),
            call("shape.fill", json!({"color": "#CC3366"})),
            call("arrange.sendToBack", json!({})),
        ],
        true,
        "",
    );
    let mut s = with_plugin("io.example.pair", "write", wasm);
    s.execute("shape.insert", &rect(300.0)).unwrap();
    let (before, n) = (steps(&s), shapes(&s));

    let r = s.execute("plugin.run", &json!({"id": "io.example.pair"})).unwrap();

    assert_eq!(r["calls"], 4, "{r}");
    assert_eq!(shapes(&s), n + 2, "two shapes inserted");
    assert_eq!(steps(&s), before + 1, "four commands, one undo step");
    assert_eq!(last_label(&s).as_deref(), Some("Test Plug-in"));

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(shapes(&s), n, "one undo takes the whole run back");
    assert_eq!(steps(&s), before, "the presentation's own steps are still there");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(shapes(&s), n + 2);
    // The next command is a step of its own, with its own name.
    s.execute("shape.insert", &rect(400.0)).unwrap();
    assert_eq!(last_label(&s).as_deref(), Some("Shapes"));
}

#[test]
fn a_run_that_changes_nothing_records_no_step_and_keeps_redo() {
    let mut s = with_plugin("io.example.reader", "write", module(&[call("slide.inspect", json!({}))], true, ""));
    s.execute("shape.insert", &rect(40.0)).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let before = steps(&s);

    s.execute("plugin.run", &json!({"id": "io.example.reader"})).unwrap();

    assert_eq!(steps(&s), before);
    assert!(!s.doc().unwrap().history.redo.is_empty(), "redo survives a run that edited nothing");
}

#[test]
fn a_read_plugin_reads_and_its_edits_are_refused() {
    let wasm = module(&[call("slide.inspect", json!({})), call("shape.insert", rect(40.0))], false, "");
    let mut s = with_plugin("io.example.peek", "read", wasm);
    let (before, n) = (steps(&s), shapes(&s));

    let r = s.execute("plugin.run", &json!({"id": "io.example.peek"})).unwrap();

    let message = r["output"].as_str().unwrap();
    assert!(message.contains("may only read"), "{message}");
    assert_eq!(shapes(&s), n, "nothing was inserted");
    assert_eq!(steps(&s), before, "nothing to undo");
}

#[test]
fn a_read_plugin_may_move_the_selection() {
    let mut s = session();
    let id = s.execute("shape.insert", &rect(40.0)).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.deselect", &json!({})).unwrap();
    let wasm = module(&[call("edit.select", json!({"ids": [id]}))], true, "");
    let r = s.execute("plugin.install", &json!({"data": data(&bundle("io.example.pick", "read", wasm))})).unwrap();
    assert_eq!(r["installed"], true);
    let before = steps(&s);
    s.execute("plugin.run", &json!({"id": "io.example.pick"})).unwrap();
    assert_eq!(s.doc().unwrap().selection.shapes.len(), 1);
    assert_eq!(steps(&s), before, "selecting isn't an edit");
}

#[test]
fn refused_commands_and_file_paths_are_refused() {
    for (command, params, why) in [
        ("file.save", json!({"path": "/tmp/x.pptx"}), "isn't available to plugins"),
        ("file.open", json!({"path": "/etc/hosts"}), "isn't available to plugins"),
        ("file.export", json!({"path": "/tmp/x.pdf"}), "isn't available to plugins"),
        ("file.saveBytes", json!({}), "isn't available to plugins"),
        ("edit.undo", json!({}), "isn't available to plugins"),
        ("edit.redo", json!({}), "isn't available to plugins"),
        ("edit.copy", json!({}), "isn't available to plugins"),
        ("edit.paste", json!({}), "isn't available to plugins"),
        ("insert.picture", json!({"path": "/etc/hosts"}), "isn't available to plugins"),
        ("insert.video", json!({"path": "/tmp/v.mp4"}), "isn't available to plugins"),
        ("insert.audio", json!({"path": "/tmp/a.wav"}), "isn't available to plugins"),
        ("font.add", json!({"path": "/tmp/f.ttf"}), "isn't available to plugins"),
        ("design.browseTheme", json!({"path": "/tmp/t.thmx"}), "isn't available to plugins"),
        ("slide.reuse", json!({"path": "/tmp/d.pptx"}), "isn't available to plugins"),
        ("show.fromStart", json!({}), "isn't available to plugins"),
        ("show.setup", json!({}), "isn't available to plugins"),
        ("media.play", json!({}), "isn't available to plugins"),
        ("text.autocorrect", json!({}), "isn't available to plugins"),
        ("format.painter", json!({}), "isn't available to plugins"),
        ("view.slideMaster", json!({}), "isn't available to plugins"),
        ("window.next", json!({}), "isn't available to plugins"),
        ("plugin.list", json!({}), "isn't available to plugins"),
        ("transition.sound", json!({"path": "/etc/hosts"}), "can't name files"),
        ("shape.fill", json!({"picture": {"path": "/etc/hosts"}}), "can't name files"),
    ] {
        let wasm = module(&[call(command, params)], false, "");
        let mut s = with_plugin("io.example.sneaky", "write", wasm);
        let (before, n) = (steps(&s), shapes(&s));
        let r = s.execute("plugin.run", &json!({"id": "io.example.sneaky"})).unwrap();
        let message = r["output"].as_str().unwrap_or_default();
        assert!(message.contains(why), "{command}: {message}");
        assert_eq!((shapes(&s), steps(&s)), (n, before), "{command}");
        assert_eq!(s.documents().len(), 1, "{command}");
    }
}

#[test]
fn a_call_that_opens_a_dialog_is_refused() {
    // Date & Time with no keys asks the window for its dialog.
    let mut s = with_plugin("io.example.dialog", "write", module(&[call("insert.dateTime", json!({}))], false, ""));
    s.ui_requests.clear();
    let r = s.execute("plugin.run", &json!({"id": "io.example.dialog"})).unwrap();
    assert!(r["output"].as_str().unwrap_or_default().contains("opens a dialog"), "{r}");
    assert!(s.ui_requests.is_empty(), "no dialog is asked for");
}

#[test]
fn a_failing_plugin_leaves_the_presentation_and_history_unchanged() {
    for (name, tail) in [("trap", "(unreachable)"), ("fuel", "(loop $l (br $l))"), ("status", "(return (i32.const 3))")] {
        let wasm = module(&[call("shape.insert", rect(40.0)), call("edit.selectAll", json!({}))], true, tail);
        let mut s = with_plugin("io.example.broken", "write", wasm);
        s.execute("shape.insert", &rect(300.0)).unwrap();
        let (before, n, doc, sel) = (steps(&s), shapes(&s), s.doc().unwrap().doc.clone(), s.doc().unwrap().selection.clone());
        let mut params = json!({"id": "io.example.broken"});
        if name == "fuel" {
            params["fuel"] = json!(100_000);
        }
        let e = s.execute("plugin.run", &params).unwrap_err().to_string();
        assert!(e.contains("Test Plug-in"), "{name}: {e}");
        let st = s.doc().unwrap();
        assert!(std::sync::Arc::ptr_eq(&st.doc, &doc), "{name}: the presentation from before the run");
        assert_eq!((shapes(&s), steps(&s)), (n, before), "{name}");
        assert_eq!(s.doc().unwrap().selection, sel, "{name}");
        assert_eq!(last_label(&s).as_deref(), Some("Shapes"), "{name}");
        // The next command isn't named after the failed run either.
        s.execute("shape.insert", &rect(400.0)).unwrap();
        assert_eq!(last_label(&s).as_deref(), Some("Shapes"), "{name}");
    }
}

#[test]
fn settings_are_bounded() {
    let mut all = std::collections::BTreeMap::new();
    super::set_setting(&mut all, "p", "k", Some("[1,2]")).unwrap();
    assert_eq!(all["p"]["k"], json!([1, 2]));
    assert!(super::set_setting(&mut all, "p", &"k".repeat(200), Some("1")).is_err());
    assert!(super::set_setting(&mut all, "p", "big", Some(&format!("\"{}\"", "x".repeat(70_000)))).is_err());
    assert!(super::set_setting(&mut all, "p", "bad", Some("not json")).is_err());
    super::set_setting(&mut all, "p", "k", None).unwrap();
    assert!(all.is_empty());
}

#[test]
fn another_apps_plugin_is_refused() {
    let manifest = json!({
        "id": "io.example.other", "name": "Other", "version": "1.0.0", "main": "plugin.wasm",
        "apps": {"wordcraft": {"api": 6}}, "permissions": {"access": "write"}, "commands": [{"id": "go", "label": "Go"}]
    });
    let b = Bundle { manifest: manifest.to_string(), wasm: module(&[], true, ""), assets: vec![] }.encode();
    let mut s = session();
    let e = s.execute("plugin.install", &json!({"data": data(&b)})).unwrap_err().to_string();
    assert!(e.contains("can't be installed"), "{e}");
    assert!(s.plugins.list().is_empty());
}

#[test]
fn a_reviewed_bundle_installs_by_its_sha256_and_no_other() {
    let b = bundle("io.example.review", "write", module(&[], true, ""));
    let mut s = session();
    let request = json!({"data": data(&b), "name": "r.craftplugin"});
    let mut review = request.clone();
    review["review"] = json!(true);
    let info = s.execute("plugin.install", &review).unwrap();
    assert_eq!((info["installed"].clone(), info["kind"].clone(), info["inCatalog"].clone()), (json!(false), json!("command"), Value::Null));
    assert!(s.plugins.list().is_empty(), "a review installs nothing");
    let mut wrong = request.clone();
    wrong["sha256"] = json!("0".repeat(64));
    assert!(s.execute("plugin.install", &wrong).is_err());
    let mut right = request;
    right["sha256"] = info["sha256"].clone();
    assert_eq!(s.execute("plugin.install", &right).unwrap()["installed"], true);
}

#[test]
fn plugin_commands_record_no_undo_step() {
    let mut s = session();
    let b = bundle("io.example.quiet", "write", module(&[], true, ""));
    s.execute("plugin.install", &json!({"data": data(&b)})).unwrap();
    s.execute("plugin.list", &json!({})).unwrap();
    s.execute("plugin.remove", &json!({"id": "io.example.quiet"})).unwrap();
    assert_eq!(steps(&s), 0);
    assert!(s.ui_requests.is_empty(), "nothing asks for a window");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn install_list_and_remove_through_the_store() {
    let dir = std::env::temp_dir().join(format!("deckcraft-plugins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.craftplugin");
    std::fs::write(&file, bundle("io.example.stored", "write", module(&[call("shape.insert", rect(40.0))], true, ""))).unwrap();

    let mut s = session();
    s.plugins.open_store(&dir);
    let r = s.execute("plugin.install", &json!({"path": file.to_string_lossy()})).unwrap();
    assert_eq!((r["installed"].clone(), r["stored"].clone()), (json!(true), json!(true)));
    s.plugins.prefs.settings.insert("io.example.stored".into(), serde_json::Map::from_iter([("k".to_string(), json!(1))]));
    s.plugins.save_prefs();

    // Another session reads what is installed, and its settings.
    let mut t = session();
    t.plugins.open_store(&dir);
    let list = t.execute("plugin.list", &json!({})).unwrap();
    assert_eq!(list["plugins"][0]["id"], "io.example.stored", "{list}");
    assert_eq!(list["plugins"][0]["source"]["type"], "file");
    assert_eq!(t.plugins.prefs.settings["io.example.stored"]["k"], 1);
    let n = shapes(&t);
    t.execute("plugin.run", &json!({"id": "io.example.stored"})).unwrap();
    assert_eq!(shapes(&t), n + 1);

    assert_eq!(t.execute("plugin.remove", &json!({"id": "io.example.stored"})).unwrap()["removed"], "io.example.stored");
    assert!(t.execute("plugin.list", &json!({})).unwrap()["plugins"].as_array().unwrap().is_empty());
    assert!(!t.plugins.prefs.settings.contains_key("io.example.stored"), "its settings go with it");
    let mut u = session();
    u.plugins.open_store(&dir);
    assert!(u.plugins.list().is_empty(), "removed from the folder");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn catalogs_must_be_on_github_or_local() {
    let mut s = session();
    assert!(s.execute("plugin.catalog.add", &json!({"url": "https://example.com/index.json"})).is_err());
    let ok = "https://raw.githubusercontent.com/example/catalog/main/index.json";
    assert_eq!(s.execute("plugin.catalog.add", &json!({"url": ok})).unwrap()["catalogs"], json!([ok]));
    assert_eq!(s.execute("plugin.catalog.add", &json!({"url": ok, "remove": true})).unwrap()["catalogs"], json!([]));
    // No network in a headless session: a repository can't be fetched, and says so.
    let e = s.execute("plugin.install", &json!({"repo": "example/plugin"})).unwrap_err().to_string();
    assert!(e.contains("no network access"), "{e}");
}
