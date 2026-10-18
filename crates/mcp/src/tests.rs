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
        let mut args = json!({"path": path.to_string_lossy(), "format": "pdf", "layout": layout, "paper": "a4", "orientation": "landscape"});
        // `perPage` is a handouts setting; the other layouts refuse it.
        if layout == "handouts" {
            args["perPage"] = json!(3);
        }
        tool(&mut s, "export", args);
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

/// #298: a tool name where a command id goes (`run_command`, an item of `batch`) runs that tool,
/// so add_shape with its fill and line can sit in a batch; and "no presentation open" names the
/// tools that open one.
#[test]
fn a_tool_name_as_a_command_id_runs_the_tool() {
    let mut s = Server::new(Box::new(Headless::new()));
    let r = call(&mut s, 1, "tools/call", json!({"name": "inspect_document", "arguments": {}}));
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r["result"]["isError"] == true && text.contains("new_presentation") && text.contains("open_presentation"), "{text}");
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    tool(&mut s, "add_slide", json!({"layout": "blank"}));
    let r = tool(&mut s, "run_command", json!({"command": "go_to_slide", "params": {"index": 0}}));
    assert_eq!(r["index"], 0, "{r}");
    let out = tool(
        &mut s,
        "batch",
        json!({"commands": [
            {"command": "go_to_slide", "params": {"index": 1}},
            {"command": "add_shape", "params": {"preset": "roundRect", "x": 10, "y": 10, "w": 100, "h": 50, "fill": "#336699", "text": "A"}},
            {"command": "shape.rename", "params": {"name": "Box A"}},
        ]}),
    );
    let items = out.as_array().unwrap();
    assert!(items.iter().all(|i| i["ok"] == true), "{out}");
    let id = items[1]["result"]["id"].as_u64().expect("the inserted shape's id");
    let sl = tool(&mut s, "inspect_slide", json!({}));
    assert_eq!(sl["index"], 1);
    let shape = sl["shapes"].as_array().unwrap().iter().find(|x| x["id"] == id).expect("shape on slide 1");
    assert_eq!(shape["name"], "Box A", "{shape}");
    assert_eq!(shape["text"], "A", "{shape}");
}

/// The tools that take `x`, `y`, `w`, `h` hand the engine a `rect`, not the four keys, which the
/// engine refuses (it found this: `insert_table` advertised them and then failed).
#[test]
fn tools_with_x_y_w_h_place_the_shape() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    tool(&mut s, "add_slide", json!({"layout": "blank"}));
    let t = tool(&mut s, "insert_table", json!({"rows": 2, "cols": 2, "x": 100, "y": 200, "w": 300, "h": 80, "data": [["a", "b"], ["c", "d"]]}));
    let id = t["id"].as_u64().expect("table id");
    let sl = tool(&mut s, "inspect_slide", json!({}));
    let table = sl["shapes"].as_array().unwrap().iter().find(|x| x["id"] == id).expect("the table");
    assert_eq!(table["box"], json!([100.0, 200.0, 300.0, 80.0]), "{table}");
    let bad = call(
        &mut s,
        5,
        "tools/call",
        json!({"name": "insert_picture", "arguments": {"data": "AAAA", "name": "x.png", "x": 1, "y": 2, "w": 3, "h": 4}}),
    );
    let text = bad["result"]["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("doesn't take"), "x/y/w/h must not reach the engine: {text}");
}

/// Phase-2 findings: `set_text` reaches a table cell with `cell`, and `list_commands` says what a
/// command acts on, so "disabled: nothing selected" no longer hides that `ids`/`id` work.
#[test]
fn set_text_takes_a_cell_and_list_commands_says_what_acts_on_what() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    tool(&mut s, "add_slide", json!({"layout": "blank"}));
    let t = tool(&mut s, "insert_table", json!({"rows": 2, "cols": 2, "x": 50, "y": 50, "w": 400, "h": 100}));
    let id = t["id"].as_u64().expect("table id");
    tool(&mut s, "set_text", json!({"id": id, "cell": [1, 0], "text": "pptx"}));
    let sl = tool(&mut s, "inspect_slide", json!({}));
    let table = sl["shapes"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap();
    assert_eq!(table["table"]["cells"][1][0], "pptx", "{table}");
    let cmds = tool(&mut s, "list_commands", json!({"filter": "shape.fill"}));
    let fill = cmds.as_array().unwrap().iter().find(|c| c["id"] == "shape.fill").unwrap();
    assert!(fill["actsOn"].as_str().unwrap_or("").contains("`ids`"), "{fill}");
    let cmds = tool(&mut s, "list_commands", json!({"filter": "table.style"}));
    let style = cmds.as_array().unwrap().iter().find(|c| c["id"] == "table.style").unwrap();
    assert!(style["actsOn"].as_str().unwrap_or("").contains("`id`"), "{style}");
}

/// Final-run findings: a command id given as a tool name is told to use run_command; the
/// instructions carry the diagram recipe so the common path needs no list_commands; and
/// list_commands {brief} answers ids and params only.
#[test]
fn discovery_costs_fewer_calls() {
    let mut s = Server::new(Box::new(Headless::new()));
    let init = call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
    let instr = init["result"]["instructions"].as_str().unwrap();
    for needle in [
        "shape.insert",
        "shape.connect",
        "animation.add",
        "slide.notes",
        "transition.set",
        "section.add",
        "insert.table",
        "design.palette",
        "placeholders",
        "cannot read images",
    ] {
        assert!(instr.contains(needle), "instructions should mention {needle}");
    }
    let r = call(&mut s, 2, "tools/call", json!({"name": "design.palette", "arguments": {}}));
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r["result"]["isError"] == true && text.contains("is a command") && text.contains("run_command"), "{text}");
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let brief = tool(&mut s, "list_commands", json!({"filter": "shape.connect", "brief": true}));
    let item = brief.as_array().unwrap().iter().find(|c| c["id"] == "shape.connect").unwrap();
    let mut keys: Vec<&str> = item.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["id", "params"], "{item}");
}

/// W6: inspect_slide answers animations as {count, clickSteps}; `animations: true` embeds the list.
#[test]
fn inspect_slide_summarises_animations_unless_asked() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let id = tool(&mut s, "add_shape", json!({"preset": "rect", "x": 10, "y": 10, "w": 100, "h": 50}))["id"].as_u64().unwrap();
    tool(&mut s, "run_command", json!({"command": "animation.add", "params": {"ids": [id], "effect": "fade", "start": "onClick"}}));
    tool(
        &mut s,
        "run_command",
        json!({"command": "animation.add", "params": {"ids": [id], "effect": "zoom", "class": "exit", "start": "afterPrevious"}}),
    );
    let sl = tool(&mut s, "inspect_slide", json!({}));
    assert_eq!(sl["animations"], json!({"count": 2, "clickSteps": 1}), "{sl}");
    let full = tool(&mut s, "inspect_slide", json!({"animations": true}));
    assert_eq!(full["animations"].as_array().map(Vec::len), Some(2), "{full}");
}

/// E5: a batch item can be named with `as`, and a later item's params can refer to its answer
/// with "$name" (its id) wherever an id goes, so a diagram is one batch: insert, connect, animate.
#[test]
fn batch_items_can_reference_earlier_results() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    tool(&mut s, "add_slide", json!({"layout": "blank"}));
    let out = tool(
        &mut s,
        "batch",
        json!({"commands": [
            {"command": "shape.insert", "params": {"preset": "roundRect", "rect": [50, 50, 150, 60], "text": "A", "fill": "accent1"}, "as": "a"},
            {"command": "shape.insert", "params": {"preset": "roundRect", "rect": [400, 50, 150, 60], "text": "B", "fill": "accent2"}, "as": "b"},
            {"command": "shape.connect", "params": {"from": "$a", "to": "$b"}, "as": "ab"},
            {"command": "animation.add", "params": {"ids": ["$a", "$b", "$ab"], "effect": "fade", "start": "onClick"}},
            {"command": "set_text", "params": {"id": "$b", "text": "B!"}},
        ]}),
    );
    let items = out.as_array().unwrap();
    assert!(items.iter().all(|i| i["ok"] == true), "{out}");
    let a = items[0]["result"]["id"].as_u64().unwrap();
    let b = items[1]["result"]["id"].as_u64().unwrap();
    assert!(items[2]["result"]["id"].as_u64().is_some(), "{out}");
    assert_eq!(items[3]["result"]["indices"], json!([0, 1, 2]), "{out}");
    let sl = tool(&mut s, "inspect_slide", json!({}));
    let shapes = sl["shapes"].as_array().unwrap();
    assert!(shapes.iter().any(|x| x["id"] == b && x["text"] == "B!"), "{sl}");
    assert!(shapes.iter().any(|x| x["id"] == a), "{sl}");
    let bad = call(
        &mut s,
        7,
        "tools/call",
        json!({"name": "batch", "arguments": {"commands": [{"command": "set_text", "params": {"id": "$nope", "text": "x"}}]}}),
    );
    let text = bad["result"]["content"][0]["text"].as_str().unwrap();
    assert!(bad["result"]["isError"] == true && text.contains("$nope"), "{text}");
}

/// A stand-in for the desktop app: records the control-channel calls and answers `ui.screenshot`
/// as the app does (a base64 PNG, or `{path, width, height}` once a path is given).
struct FakeApp {
    calls: std::rc::Rc<std::cell::RefCell<Vec<(String, Value)>>>,
}

impl crate::backend::Backend for FakeApp {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.calls.borrow_mut().push((method.to_string(), params.clone()));
        match method {
            "ui.screenshot" => Ok(match params.get("path").and_then(Value::as_str) {
                Some(p) => json!({"path": p, "width": 1280, "height": 800}),
                None => json!({"png": "iVBORw0KGgo=", "width": 1280, "height": 800}),
            }),
            other => Err(format!("unknown method `{other}`")),
        }
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        "stand-in app".into()
    }
}

/// W4: `screenshot` answered image content only, which an agent behind a text bridge cannot see.
/// With `path` the PNG is written there and the answer is text `{path, width, height}`; headless
/// still refuses clearly and writes nothing. (The connected backend is a loopback socket; the
/// stand-in checks the argument plumbing and the answer shape.)
#[test]
fn screenshot_with_a_path_answers_text_and_headless_refuses() {
    let mut s = Server::new(Box::new(Headless::new()));
    let path = std::env::temp_dir().join(format!("deckcraft-mcp-shot-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let r = call(&mut s, 1, "tools/call", json!({"name": "screenshot", "arguments": {"path": path.to_string_lossy()}}));
    let msg = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r["result"]["isError"] == true && msg.contains("desktop app") && msg.contains("export"), "{msg}");
    assert!(!path.exists(), "headless must write nothing");
    let calls = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
    let mut s = Server::new(Box::new(FakeApp { calls: calls.clone() }));
    let r = call(&mut s, 2, "tools/call", json!({"name": "screenshot", "arguments": {"path": "shots/window.png"}}));
    assert_eq!(r["result"]["isError"], false, "{r}");
    let content = r["result"]["content"].as_array().unwrap();
    assert!(content.iter().all(|c| c["type"] == "text"), "text, not an image: {r}");
    let v: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v, json!({"path": "shots/window.png", "width": 1280, "height": 800}));
    assert_eq!(calls.borrow().last().unwrap(), &("ui.screenshot".to_string(), json!({"path": "shots/window.png"})));
    // Without a path: image content, as before.
    let r = call(&mut s, 3, "tools/call", json!({"name": "screenshot", "arguments": {}}));
    assert_eq!(r["result"]["content"][0]["type"], "image", "{r}");
    assert_eq!(calls.borrow().last().unwrap().1, json!({"base64": true}));
    let tools = call(&mut s, 4, "tools/list", json!({}));
    let shot = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "screenshot").unwrap();
    assert!(shot["inputSchema"]["properties"].get("path").is_some() && shot["description"].as_str().unwrap().contains("path"), "{shot}");
}

/// A batch item named with a hyphen (`as: "ui-egui"`) can never be referred to as `$ui-egui`
/// (the reference stops at the hyphen), so the name is refused where it is declared, with the
/// rule, instead of a later item failing on a `$ui-egui` sent on as text.
#[test]
fn batch_refuses_a_name_a_reference_cannot_spell() {
    let mut s = Server::new(Box::new(Headless::new()));
    tool(&mut s, "new_presentation", json!({}));
    tool(&mut s, "add_slide", json!({"layout": "blank"}));
    let r = call(
        &mut s,
        9,
        "tools/call",
        json!({"name": "batch", "arguments": {"commands": [
            {"command": "shape.insert", "params": {"preset": "rect", "rect": [10, 10, 100, 50]}, "as": "ui-egui"},
            {"command": "shape.insert", "params": {"preset": "rect", "rect": [200, 10, 100, 50]}},
        ]}}),
    );
    let text = r["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(r["result"]["isError"] == true && text.contains("ui-egui") && text.contains("letters, digits and `_`"), "{text}");
    assert!(!text.contains("\"ok\":true"), "stops at the refused item: {text}");
    let sl = tool(&mut s, "inspect_slide", json!({}));
    assert_eq!(sl["shapes"].as_array().map(|a| a.len()), Some(0), "nothing ran: {sl}");
}

/// Y4: the `export` tool says that `all: true` numbers the files from 1 (the displayed slide
/// number) while `slide` is a 0-based index, and the answer carries `indices` beside `files`.
#[test]
fn export_all_says_files_are_numbered_from_one() {
    let mut s = Server::new(Box::new(Headless::new()));
    let tools = call(&mut s, 1, "tools/list", json!({}));
    let export = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "export").unwrap().clone();
    assert!(export["description"].as_str().unwrap().contains("numbered from 1"), "{export}");
    assert!(export["inputSchema"]["properties"]["all"]["description"].as_str().unwrap().contains("numbered from 1"), "{export}");
    let dir = std::env::temp_dir().join(format!("deckcraft-mcp-export-all-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    tool(&mut s, "new_presentation", json!({}));
    tool(&mut s, "add_slide", json!({"layout": "blank"}));
    let v = tool(&mut s, "export", json!({"path": dir.join("d.png").to_string_lossy(), "all": true, "scale": 0.25}));
    assert_eq!(v["indices"], json!([0, 1]), "{v}");
    assert_eq!(v["files"], json!([dir.join("d1.png").to_string_lossy(), dir.join("d2.png").to_string_lossy()]), "{v}");
    let _ = std::fs::remove_dir_all(&dir);
}
