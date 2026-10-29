//! `deckcraft-cli mcp --allow-commands LIST`: through the MCP protocol, only listed commands run,
//! whether asked for directly (`run_command`), in a `batch`, through a dedicated tool
//! (`add_slide`, `save_presentation`…) or through `call_app`; gestures, which change the
//! presentation without naming a command, are refused; and `list_commands` shows only what may run.
//! With `--automation-write-root`, the write root itself is not a file to save.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use deckcraft_engine::command_access::CommandAllowlist;
use deckcraft_engine::file_access::AutomationRoots;
use deckcraft_mcp::{Headless, Server};
use serde_json::{Value, json};

/// A headless server with a new presentation, held to `list`.
fn allowed(list: &str) -> Server {
    let list = Arc::new(CommandAllowlist::parse(list).unwrap());
    Server::new(Box::new(Headless::with_document().with_command_allowlist(Some(list))))
}

/// `p` as text, the way tools receive paths.
fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// `tools/call` -> (isError, the first text block).
fn call(server: &mut Server, name: &str, args: Value) -> (bool, String) {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string();
    let reply: Value = serde_json::from_str(&server.handle_line(&line).unwrap()).unwrap();
    let result = &reply["result"];
    let text = result["content"].as_array().unwrap().iter().find_map(|c| c["text"].as_str()).unwrap_or_default().to_string();
    (result["isError"] == true, text)
}

/// `tools/call` that must succeed; returns its text.
fn ok(server: &mut Server, name: &str, args: Value) -> String {
    let (error, text) = call(server, name, args.clone());
    assert!(!error, "{name} {args}: {text}");
    text
}

/// `tools/call` that must be refused with `needle` in the message.
fn refused(server: &mut Server, name: &str, args: Value, needle: &str) {
    let (error, text) = call(server, name, args.clone());
    assert!(error, "{name} {args} should be refused: {text}");
    assert!(text.contains(needle), "{name}: expected `{needle}` in: {text}");
}

/// The number of slides in the open presentation, read without a command list in the way.
fn slides(server: &mut Server) -> usize {
    let doc: Value = serde_json::from_str(&ok(server, "inspect_document", json!({}))).unwrap();
    doc["slides"].as_array().map_or(0, Vec::len)
}

const EDITING: &str = "slide.*, text.*, format.*, insert.textBox, edit.select, document.inspect, file.new";

/// Listed commands run, through any tool; the rest are refused and change nothing.
#[test]
fn listed_commands_run_and_the_rest_are_refused() {
    let mut srv = allowed(EDITING);
    ok(&mut srv, "add_slide", json!({"title": "Allowed"}));
    let before = slides(&mut srv);
    ok(&mut srv, "run_command", json!({"command": "slide.new", "params": {"title": "Also allowed"}}));
    assert_eq!(slides(&mut srv), before + 1);
    refused(&mut srv, "run_command", json!({"command": "file.open", "params": {"path": "x.pptx"}}), "file.open");
    refused(&mut srv, "open_presentation", json!({"path": "x.pptx"}), "file.open");
    refused(&mut srv, "save_presentation", json!({"path": "x.deckcraft"}), "file.save");
    refused(&mut srv, "export", json!({"path": "x.pdf"}), "file.export");
    refused(&mut srv, "insert_chart", json!({"type": "column"}), "insert.chart");
    refused(&mut srv, "add_shape", json!({"preset": "rect", "x": 10, "y": 10, "w": 50, "h": 50}), "shape.insert");
    refused(
        &mut srv,
        "call_app",
        json!({"method": "engine.execute", "params": {"command": "insert.chart", "params": {"type": "column"}}}),
        "insert.chart",
    );
    refused(&mut srv, "call_app", json!({"method": "app.save", "params": {"path": "x.deckcraft"}}), "file.save");
    assert_eq!(slides(&mut srv), before + 1, "nothing refused changed the presentation");
}

/// A batch stops at the first command off the list; the ones before it ran.
#[test]
fn a_batch_stops_at_the_first_command_off_the_list() {
    let mut srv = allowed(EDITING);
    let before = slides(&mut srv);
    let (error, text) = call(
        &mut srv,
        "batch",
        json!({"commands": [
            {"command": "slide.new", "params": {"title": "One"}},
            {"command": "insert.chart", "params": {"type": "column"}},
            {"command": "slide.new", "params": {"title": "Never"}}
        ]}),
    );
    assert!(error && text.contains("insert.chart") && text.contains("--allow-commands"), "{text}");
    assert_eq!(slides(&mut srv), before + 1);
    // keepGoing doesn't let the refused one through either.
    ok(
        &mut srv,
        "batch",
        json!({"keepGoing": true, "commands": [{"command": "insert.chart", "params": {"type": "column"}}, {"command": "slide.new", "params": {"title": "Two"}}]}),
    );
    assert_eq!(slides(&mut srv), before + 2);
}

/// Gestures change the presentation without naming a command: refused while a list is in force,
/// even when every command behind them is on the list.
#[test]
fn gestures_are_not_a_way_around_the_list() {
    let mut srv = allowed("slide.*, text.*, format.*, shape.*, insert.*, edit.*, file.*, document.inspect");
    for (tool, args) in [
        ("pointer", json!({"events": [{"kind": "down", "x": 100, "y": 100}, {"kind": "up", "x": 200, "y": 200}]})),
        ("key", json!({"key": "s", "mods": {"cmd": true}})),
        ("type_text", json!({"text": "hello"})),
        ("select_tool", json!({"tool": "shape", "preset": "rect"})),
    ] {
        refused(&mut srv, tool, args, "--allow-commands");
    }
    refused(&mut srv, "call_app", json!({"method": "ui.key", "params": {"key": "delete"}}), "--allow-commands");
}

/// `list_commands` shows only the commands on the list.
#[test]
fn list_commands_shows_only_what_may_run() {
    let mut srv = allowed("format.*, slide.new");
    let list: Value = serde_json::from_str(&ok(&mut srv, "list_commands", json!({}))).unwrap();
    let ids: Vec<&str> = list.as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    assert!(ids.contains(&"format.bold") && ids.contains(&"slide.new"), "{ids:?}");
    assert!(ids.iter().all(|id| id.starts_with("format.") || *id == "slide.new"), "{ids:?}");
}

/// Without a list nothing changes: any command, gestures too.
#[test]
fn without_a_list_nothing_changes() {
    let mut srv = Server::new(Box::new(Headless::with_document()));
    ok(&mut srv, "run_command", json!({"command": "slide.new", "params": {}}));
    ok(&mut srv, "select_tool", json!({"tool": "select"}));
    let list: Value = serde_json::from_str(&ok(&mut srv, "list_commands", json!({}))).unwrap();
    assert!(list.as_array().unwrap().len() > 200);
}

/// A write root named like a file: saving to the root's own path would write the temporary
/// sibling next to the root, outside it. The save is refused and nothing appears beside it.
#[test]
fn the_write_root_itself_is_not_a_destination() {
    let base: PathBuf = std::env::temp_dir().join(format!("dc-mcp-rootname-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("out.pptx");
    std::fs::create_dir_all(&root).unwrap();
    let roots = AutomationRoots::new(Some(&root), Some(&root)).unwrap();
    let mut srv = Server::new(Box::new(Headless::with_document().with_automation_roots(roots)));
    ok(&mut srv, "new_presentation", json!({}));
    refused(&mut srv, "save_presentation", json!({"path": s(&root)}), "the write root itself is not a file to write");
    refused(&mut srv, "export", json!({"path": s(&root)}), "the write root itself is not a file to write");
    ok(&mut srv, "save_presentation", json!({"path": s(&root.join("deck.pptx"))}));
    let mut names: Vec<_> = std::fs::read_dir(&base).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["out.pptx"], "nothing beside the root");
    assert!(root.join("deck.pptx").is_file());
    let _ = std::fs::remove_dir_all(&base);
}

/// Drawing a slide only reads the deck, but writing the picture to a file is an export: without
/// `file.export` on the list a `path` given through `call_app` is refused and nothing is written
/// (`render_slide` itself takes no `path`). With it, the file is written as before.
#[test]
fn rendering_to_a_file_needs_file_export() {
    let dir: PathBuf = std::env::temp_dir().join(format!("dc-mcp-render-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("slide.png");

    let mut srv = allowed(EDITING);
    ok(&mut srv, "render_slide", json!({"slide": 0}));
    refused(&mut srv, "render_slide", json!({"slide": 0, "path": s(&png)}), "unknown argument");
    refused(&mut srv, "call_app", json!({"method": "ui.render", "params": {"path": s(&png)}}), "file.export");
    refused(&mut srv, "call_app", json!({"method": "app.renderSlide", "params": {"path": s(&png)}}), "file.export");
    assert!(!png.exists(), "nothing written without file.export");

    let mut srv = allowed(&format!("{EDITING}, file.export"));
    ok(&mut srv, "call_app", json!({"method": "ui.render", "params": {"path": s(&png)}}));
    assert!(png.is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
