//! An agent builds a small deck entirely through MCP and checks its work only through MCP.

use serde_json::{Value, json};

use crate::{Headless, Server};

fn call(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let out = s.handle_line(&line).expect("a response");
    serde_json::from_str(&out).expect("json")
}

#[test]
fn oversized_stdio_frame_is_drained_before_the_next_request() {
    let mut s = Server::new(Box::new(Headless::new()));
    let mut input = vec![b'x'; 16 * 1024 * 1024 + 1];
    input.extend_from_slice(b"\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n");
    let mut output = Vec::new();
    s.serve(std::io::Cursor::new(input), &mut output).unwrap();
    let replies: Vec<Value> = std::str::from_utf8(&output).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["error"]["code"], -32600);
    assert!(replies[0]["error"]["message"].as_str().unwrap().contains("16 MiB"));
    assert_eq!(replies[1]["id"], 1);
    assert_eq!(replies[1]["result"]["serverInfo"]["name"], "deckcraft");
}

#[test]
fn invalid_utf8_stdio_frame_gets_a_parse_error_and_does_not_stop_serving() {
    let mut s = Server::new(Box::new(Headless::new()));
    let mut input = b"\xff\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n".to_vec();
    let mut output = Vec::new();
    s.serve(std::io::Cursor::new(&mut input), &mut output).unwrap();
    let replies: Vec<Value> = std::str::from_utf8(&output).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert!(replies[0]["error"]["message"].as_str().unwrap().contains("UTF-8"));
    assert_eq!(replies[1]["result"]["serverInfo"]["name"], "deckcraft");
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

/// `batch {atomic: true}` runs engine `edit.batch`: every change to the presentation or none, in
/// one undo step.
#[test]
fn atomic_batch_makes_every_change_or_none() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let count = |s: &mut Server| tool(s, "inspect_slide", json!({}))["shapes"].as_array().unwrap().len();
    let rect = |x: i64| json!({"command": "shape.insert", "params": {"preset": "rect", "rect": [x, 300, 100, 80]}});
    let bad = json!({"command": "shape.rename", "params": {"id": 4_000_000, "name": "Nobody"}});
    let before = count(&mut s);
    let r = call(&mut s, 1, "tools/call", json!({"name": "batch", "arguments": {"commands": [rect(20), rect(140), bad.clone()], "atomic": true}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("step 2 (`shape.rename`)"), "{r}");
    assert_eq!(count(&mut s), before);
    let out = tool(&mut s, "batch", json!({"commands": [rect(20), rect(140)], "atomic": true}));
    assert_eq!(out.as_array().unwrap().len(), 2, "{out}");
    assert_eq!(out[0]["ok"], true, "{out}");
    assert!(out[1]["result"]["id"].is_u64(), "{out}");
    assert_eq!(count(&mut s), before + 2);
    assert_eq!(tool(&mut s, "inspect_document", json!({}))["undo"], "Batch Edit");
    tool(&mut s, "run_command", json!({"command": "edit.undo"}));
    assert_eq!(count(&mut s), before);
    // atomic and keepGoing together are refused before anything runs.
    let r = call(&mut s, 2, "tools/call", json!({"name": "batch", "arguments": {"commands": [rect(20)], "atomic": true, "keepGoing": true}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("leaves the presentation as it was"), "{r}");
    assert_eq!(count(&mut s), before);
    // Without atomic, the commands before the failing one stay.
    let r = call(&mut s, 3, "tools/call", json!({"name": "batch", "arguments": {"commands": [rect(20), rect(140), bad]}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    assert_eq!(count(&mut s), before + 2);
}

/// `batch` refuses an `atomic` that is not true, false or null before any command runs. Null is
/// the same as leaving it out: the commands run one by one.
#[test]
fn batch_refuses_an_atomic_that_is_not_a_boolean() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let count = |s: &mut Server| tool(s, "inspect_slide", json!({}))["shapes"].as_array().unwrap().len();
    let depth = |s: &mut Server| tool(s, "inspect_document", json!({}))["undoDepth"].clone();
    let rect = |x: i64| json!({"command": "shape.insert", "params": {"preset": "rect", "rect": [x, 300, 100, 80]}});
    let bad = json!({"command": "shape.rename", "params": {"id": 4_000_000, "name": "Nobody"}});
    let (before, undo) = (count(&mut s), depth(&mut s));
    for atomic in [json!("true"), json!(1)] {
        let r =
            call(&mut s, 1, "tools/call", json!({"name": "batch", "arguments": {"commands": [rect(20), rect(140), bad.clone()], "atomic": atomic}}));
        assert_eq!(r["result"]["isError"], true, "{r}");
        assert_eq!(count(&mut s), before, "atomic {atomic}: no command ran");
        assert_eq!(depth(&mut s), undo, "atomic {atomic}");
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("`atomic` takes true or false"), "{r}");
    }
    let r = call(&mut s, 2, "tools/call", json!({"name": "batch", "arguments": {"commands": [rect(20), rect(140), bad], "atomic": null}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    let steps: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(steps.as_array().map(Vec::len), Some(3), "{r}");
    assert_eq!(count(&mut s), before + 2);
}

/// #1151: `insert_chart` takes x, y, w and h like `insert_picture` and `insert_table`.
#[test]
fn insert_chart_takes_a_box() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let chart = json!({"type": "pie", "categories": ["A", "B"], "series": [{"name": "S", "values": [3, 5]}]});
    let box_of = |s: &mut Server, id: u64| {
        let sl = tool(s, "inspect_slide", json!({}));
        let shape = sl["shapes"].as_array().unwrap().iter().find(|x| x["id"].as_u64() == Some(id)).unwrap().clone();
        assert_eq!(shape["kind"], "chart");
        shape["box"].clone()
    };
    let mut args = chart.clone();
    for (k, v) in [("x", 50), ("y", 60), ("w", 300), ("h", 200)] {
        args[k] = json!(v);
    }
    let placed = tool(&mut s, "insert_chart", args)["id"].as_u64().unwrap();
    assert_eq!(box_of(&mut s, placed), json!([50.0, 60.0, 300.0, 200.0]));
    // without a full box the chart keeps its default place, as for pictures and tables
    let plain = tool(&mut s, "insert_chart", chart.clone())["id"].as_u64().unwrap();
    let default_box = box_of(&mut s, plain);
    let mut partial = chart.clone();
    partial["x"] = json!(50);
    let partial_id = tool(&mut s, "insert_chart", partial)["id"].as_u64().unwrap();
    assert_eq!(box_of(&mut s, partial_id), default_box);
    // a hostile box is answered, not a panic
    let mut odd = chart;
    for (k, v) in [("x", json!(-1e300)), ("y", json!(1e300)), ("w", json!(-5)), ("h", json!(0))] {
        odd[k] = v;
    }
    let r = call(&mut s, 11, "tools/call", json!({"name": "insert_chart", "arguments": odd}));
    assert!(r["result"].is_object(), "{r}");
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
fn unknown_tool_arguments_are_refused() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({}));
    let charts = |v: &Value| v["shapes"].as_array().unwrap().iter().filter(|sh| sh["kind"] == "chart").count();
    let before = charts(&tool(&mut s, "inspect_slide", json!({})));
    let r = call(&mut s, 2, "tools/call", json!({"name": "insert_chart", "arguments": {"type": "column", "data": [["x", "A"], ["q1", 1]]}}));
    assert_eq!(r["result"]["isError"], true);
    let msg = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(msg.contains("unknown argument \"data\"") && msg.contains("categories") && msg.contains("series"), "{msg}");
    assert_eq!(charts(&tool(&mut s, "inspect_slide", json!({}))), before, "a refused call inserts nothing");
    let r = call(&mut s, 3, "tools/call", json!({"name": "list_commands", "arguments": {"query": "theme", "limit": 1}}));
    assert_eq!(r["result"]["isError"], true);
    let msg = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(msg.contains("unknown arguments \"limit\", \"query\"") && msg.contains("filter"), "{msg}");
    // The command's own parameters stay inside `params`.
    tool(&mut s, "run_command", json!({"command": "slide.new", "params": {"layout": "blank"}}));
    let tools = call(&mut s, 4, "tools/list", json!({}));
    let chart = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "insert_chart").unwrap();
    assert_eq!(chart["inputSchema"]["additionalProperties"], false);
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

fn current_slide_title(s: &mut Server) -> Value {
    tool(s, "inspect_slide", json!({}))["title"].clone()
}

#[test]
fn go_to_slide_out_of_range_is_error() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    tool(&mut s, "add_slide", json!({"layout": "titleAndContent", "title": "Second", "body": "x"}));
    // Boundaries stay valid: first and last slide.
    tool(&mut s, "go_to_slide", json!({"index": 0}));
    tool(&mut s, "go_to_slide", json!({"index": 1}));
    assert_eq!(current_slide_title(&mut s), "Second");
    for bad in [json!(2), json!(99)] {
        let r = call(&mut s, 1, "tools/call", json!({"name": "go_to_slide", "arguments": {"index": bad}}));
        assert_eq!(r["result"]["isError"], true, "{bad}: {r}");
        assert_eq!(current_slide_title(&mut s), "Second", "failed go_to_slide must not move");
    }
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
    for p in ["dpi", "slides", "includeHidden", "textLayer", "frame", "paper", "orientation"] {
        assert!(props.get(p).is_some(), "{p}: {export}");
    }
    // What the schema describes is accepted.
    let dir = std::env::temp_dir().join(format!("deckcraft-mcp-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    tool(&mut s, "new_presentation", json!({}));
    for layout in ["slides", "notes", "handouts"] {
        let path = dir.join(format!("{layout}.pdf"));
        tool(
            &mut s,
            "export",
            json!({"path": path.to_string_lossy(), "format": "pdf", "layout": layout, "perPage": 3, "paper": "a4", "orientation": "landscape"}),
        );
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"), "{layout}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_rejects_unknown_format() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let path = std::env::temp_dir().join(format!("deckcraft-mcp-unknown-format-{}.docx", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let r = call(&mut s, 1, "tools/call", json!({"name": "export", "arguments": {"path": path.to_str().unwrap(), "format": "docx"}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("docx"), "{r}");
    assert!(!path.exists(), "nothing may be written for an unknown format");
}

#[test]
fn export_tool_lists_gif_and_its_options() {
    let mut s = Server::new(Box::new(Headless::new()));
    let tools = call(&mut s, 1, "tools/list", json!({}));
    let export = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "export").unwrap().clone();
    assert!(export["description"].as_str().unwrap().contains("gif"), "{export}");
    let props = &export["inputSchema"]["properties"];
    assert!(props["format"]["enum"].as_array().unwrap().iter().any(|x| x == "gif"), "{export}");
    assert_eq!(props["width"]["oneOf"][1]["enum"], json!(["small", "medium", "large", "extraLarge"]));
    for p in ["secondsPerSlide", "loop", "slides", "includeHidden"] {
        assert!(props.get(p).is_some(), "{p}: {export}");
    }
    // What the schema describes is accepted, and bad values are tool errors, not crashes.
    let dir = std::env::temp_dir().join(format!("deckcraft-mcp-gif-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    tool(&mut s, "new_presentation", json!({}));
    for width in [json!("small"), json!(200)] {
        let path = dir.join("deck.gif");
        tool(&mut s, "export", json!({"path": path.to_string_lossy(), "width": width, "secondsPerSlide": 2, "loop": false, "slides": [0]}));
        assert!(std::fs::read(&path).unwrap().starts_with(b"GIF89a"), "{width}");
    }
    let r = call(&mut s, 9, "tools/call", json!({"name": "export", "arguments": {"path": dir.join("bad.gif").to_string_lossy(), "width": 0}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}
