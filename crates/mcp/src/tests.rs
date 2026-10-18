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

/// W5: a bare list_commands cost 95 KB (40 KB with brief) and an agent paid it twice per run. Bare
/// now answers the command families in under 6 KB; `id` answers one command's full entry; an
/// unknown `id` gets the engine's "commands named like it" hint.
#[test]
fn list_commands_bare_answers_families_and_id_answers_one_command() {
    let mut s = Server::new(Box::new(Headless::new()));
    let r = call(&mut s, 1, "tools/call", json!({"name": "list_commands", "arguments": {}}));
    assert_eq!(r["result"]["isError"], false, "{r}");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.len() < 6000, "bare list_commands is {} bytes", text.len());
    let families: Value = serde_json::from_str(text).unwrap();
    // Every family of every command (the part before the dot) is there, with its count and its
    // commands' names after the dot.
    let every = tool(&mut s, "list_commands", json!({"filter": ".", "brief": true}));
    let every = every.as_array().unwrap();
    assert!(every.len() > 200, "{}", every.len());
    for c in every {
        let id = c["id"].as_str().unwrap();
        let (fam, name) = id.split_once('.').unwrap();
        let f = families.as_array().unwrap().iter().find(|f| f["family"] == fam).unwrap_or_else(|| panic!("family {fam} missing: {text}"));
        let names: Vec<&str> = f["commands"].as_str().unwrap().split_whitespace().collect();
        assert!(names.contains(&name), "{id} missing from {f}");
        assert_eq!(f["count"].as_u64().unwrap() as usize, names.len(), "{f}");
    }
    let slide = families.as_array().unwrap().iter().find(|f| f["family"] == "slide").unwrap();
    assert!(slide["about"].as_str().is_some_and(|d| !d.is_empty()), "{slide}");
    // brief without a filter is still the families listing.
    let b = tool(&mut s, "list_commands", json!({"brief": true}));
    assert_eq!(b, families);
    // One command, in full.
    let one = tool(&mut s, "list_commands", json!({"id": "table.style"}));
    assert_eq!(one["id"], "table.style", "{one}");
    assert!(one["params"].as_str().is_some() && one["enabled"].is_boolean() && one["label"].is_string(), "{one}");
    assert!(one["actsOn"].as_str().unwrap_or("").contains("`id`"), "{one}");
    // An unknown id: the engine's hint, not an empty list.
    let r = call(&mut s, 2, "tools/call", json!({"name": "list_commands", "arguments": {"id": "table.styl"}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
    let msg = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(msg.contains("unknown command `table.styl`") && msg.contains("like it") && msg.contains("table.style"), "{msg}");
    let r = call(&mut s, 3, "tools/call", json!({"name": "list_commands", "arguments": {"id": "add_shape"}}));
    let msg = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r["result"]["isError"] == true && msg.contains("tool"), "{msg}");
    // A filter still answers matching commands as before.
    let cmds = tool(&mut s, "list_commands", json!({"filter": "table.style"}));
    assert_eq!(cmds.as_array().unwrap().len(), 1, "{cmds}");
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
