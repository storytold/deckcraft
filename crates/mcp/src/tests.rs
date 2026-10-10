//! An agent builds a small deck entirely through MCP and checks its work only through MCP.

use serde_json::{Value, json};

use crate::{Headless, Server};

fn call(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let out = s.handle_line(&line).expect("a response");
    serde_json::from_str(&out).expect("json")
}

fn tool(s: &mut Server, name: &str, args: Value) -> Value {
    let r = call(s, 9, "tools/call", json!({"name": name, "arguments": args}));
    let res = &r["result"];
    assert_eq!(res["isError"], false, "{name} failed: {res}");
    let text =
        res["content"].as_array().unwrap().iter().find(|c| c["type"] == "text").map(|c| c["text"].as_str().unwrap().to_string()).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

#[test]
fn handshake_and_tool_list() {
    let mut s = Server::new(Box::new(Headless::new()));
    let r = call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
    assert_eq!(r["result"]["serverInfo"]["name"], "deckcraft");
    let tools = call(&mut s, 2, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["run_command", "render_slide", "add_slide", "add_shape", "pointer", "inspect_slide"] {
        assert!(names.contains(&n), "{n}");
    }
    // Garbage never panics.
    assert!(s.handle_line("not json").is_some());
    assert!(s.handle_line("{}").is_some());
}

#[test]
fn agent_builds_a_deck() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let doc = tool(&mut s, "inspect_document", json!({}));
    let title_id = {
        let sl = tool(&mut s, "inspect_slide", json!({}));
        sl["shapes"][0]["id"].as_u64().unwrap()
    };
    let _ = doc;
    tool(&mut s, "set_text", json!({"id": title_id, "text": "Quarterly Review"}));
    tool(&mut s, "add_slide", json!({"layout": "titleAndContent", "title": "Highlights", "body": "Revenue up\n\tNew markets\nCosts down"}));
    let shape = tool(&mut s, "add_shape", json!({"preset": "roundRect", "x": 600, "y": 300, "w": 200, "h": 100, "text": "Goal", "fill": "accent2"}));
    let sid = shape["id"].as_u64().unwrap();
    tool(&mut s, "run_command", json!({"command": "shape.rotate", "params": {"id": sid, "deg": 10}}));
    // Drag the shape with the pointer.
    tool(
        &mut s,
        "pointer",
        json!({"events": [{"kind": "down", "x": 700, "y": 350}, {"kind": "drag", "x": 650, "y": 330}, {"kind": "up", "x": 650, "y": 330}]}),
    );
    tool(&mut s, "insert_chart", json!({"type": "pie", "categories": ["A", "B"], "series": [{"name": "S", "values": [3, 5]}]}));
    let sl = tool(&mut s, "inspect_slide", json!({}));
    assert_eq!(sl["title"], "Highlights");
    let shapes = sl["shapes"].as_array().unwrap();
    let goal = shapes.iter().find(|x| x["id"].as_u64() == Some(sid)).unwrap();
    assert_eq!(goal["text"], "Goal");
    assert_eq!(goal["rotation"], 10.0);
    assert!(goal["box"][0].as_f64().unwrap() < 600.0, "moved left: {goal}");
    assert!(shapes.iter().any(|x| x["kind"] == "chart"));
    let doc = tool(&mut s, "inspect_document", json!({}));
    assert_eq!(doc["slides"].as_array().unwrap().len(), 2);
    assert_eq!(doc["theme"], "Harbor");
    // Look at it.
    let r = call(&mut s, 10, "tools/call", json!({"name": "render_slide", "arguments": {"scale": 0.25}}));
    assert_eq!(r["result"]["content"][0]["type"], "image");
    // Keyboard: select the shape, delete it, undo.
    tool(&mut s, "select", json!({"ids": [sid]}));
    tool(&mut s, "key", json!({"key": "Delete"}));
    let n1 = tool(&mut s, "inspect_slide", json!({}))["shapes"].as_array().unwrap().len();
    tool(&mut s, "key", json!({"key": "z", "mods": {"cmd": true}}));
    let n2 = tool(&mut s, "inspect_slide", json!({}))["shapes"].as_array().unwrap().len();
    assert_eq!(n2, n1 + 1);
}

#[test]
fn set_text_unknown_id_is_error() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let title_id = tool(&mut s, "inspect_slide", json!({}))["shapes"][0]["id"].as_u64().unwrap();
    tool(&mut s, "select", json!({"ids": [title_id]}));
    // -1 and 2^32 are not shape ids; they must not fall back to the selection.
    for bad in [json!(999999), json!(-1), json!(4294967296u64)] {
        let r = call(&mut s, 1, "tools/call", json!({"name": "set_text", "arguments": {"id": bad, "text": "x"}}));
        assert_eq!(r["result"]["isError"], true, "{bad}: {r}");
    }
    // A real id, and a just-created shape, still work; omitting id still uses the selection.
    tool(&mut s, "set_text", json!({"id": title_id, "text": "Real"}));
    let sid = tool(&mut s, "add_shape", json!({"preset": "rect", "x": 10, "y": 10, "w": 50, "h": 50}))["id"].as_u64().unwrap();
    tool(&mut s, "set_text", json!({"id": sid, "text": "Fresh"}));
    tool(&mut s, "select", json!({"ids": [title_id]}));
    tool(&mut s, "set_text", json!({"text": "Selected"}));
    let sl = tool(&mut s, "inspect_slide", json!({}));
    let text_of = |id: u64| sl["shapes"].as_array().unwrap().iter().find(|x| x["id"].as_u64() == Some(id)).unwrap()["text"].clone();
    assert_eq!(text_of(sid), "Fresh");
    assert_eq!(text_of(title_id), "Selected");
}

#[test]
fn invalid_id_never_falls_back_to_selection() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let title_id = tool(&mut s, "inspect_slide", json!({}))["shapes"][0]["id"].as_u64().unwrap();
    tool(&mut s, "select", json!({"ids": [title_id]}));
    for cmd in ["text.edit", "text.get", "shape.inspect", "table.selectCells", "arrange.reorder"] {
        for bad in [json!(-1), json!(1.5), json!("7"), json!(4294967296u64)] {
            let r = call(&mut s, 1, "tools/call", json!({"name": "run_command", "arguments": {"command": cmd, "params": {"id": bad, "index": 0}}}));
            assert_eq!(r["result"]["isError"], true, "{cmd} {bad}: {r}");
        }
    }
    // Absent id still means the selection, and a real id still works.
    tool(&mut s, "run_command", json!({"command": "text.get", "params": {}}));
    tool(&mut s, "run_command", json!({"command": "text.get", "params": {"id": title_id}}));
    tool(&mut s, "run_command", json!({"command": "shape.inspect", "params": {"id": title_id}}));
}

#[test]
fn errors_are_reported_not_panics() {
    let mut s = Server::new(Box::new(Headless::new()));
    let r = call(&mut s, 1, "tools/call", json!({"name": "run_command", "arguments": {"command": "no.such"}}));
    assert_eq!(r["result"]["isError"], true);
    let r = call(&mut s, 2, "tools/call", json!({"name": "screenshot", "arguments": {}}));
    assert_eq!(r["result"]["isError"], true);
    let r = call(&mut s, 3, "tools/call", json!({"name": "add_shape", "arguments": {"preset": "rect"}}));
    assert_eq!(r["result"]["isError"], true);
}

#[test]
fn export_tool_lists_pdf_and_its_options() {
    let mut s = Server::new(Box::new(Headless::new()));
    let tools = call(&mut s, 1, "tools/list", json!({}));
    let export = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "export").unwrap().clone();
    assert!(export["description"].as_str().unwrap().contains("pdf"), "{export}");
    let props = &export["inputSchema"]["properties"];
    let formats = props["format"]["enum"].as_array().unwrap();
    for f in ["png", "jpeg", "pptx", "deckcraft", "outline", "pdf"] {
        assert!(formats.iter().any(|x| x == f), "format {f}: {export}");
    }
    assert_eq!(props["layout"]["enum"], json!(["slides", "notes", "handouts"]));
    assert_eq!(props["perPage"]["enum"], json!([1, 2, 3, 4, 6, 9]));
    for p in ["dpi", "slides", "includeHidden", "textLayer", "frame"] {
        assert!(props.get(p).is_some(), "{p}: {export}");
    }
    // What the schema describes is accepted.
    let dir = std::env::temp_dir().join(format!("deckcraft-mcp-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    tool(&mut s, "new_presentation", json!({}));
    for layout in ["slides", "notes", "handouts"] {
        let path = dir.join(format!("{layout}.pdf"));
        tool(&mut s, "export", json!({"path": path.to_string_lossy(), "format": "pdf", "layout": layout, "perPage": 3}));
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"), "{layout}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
