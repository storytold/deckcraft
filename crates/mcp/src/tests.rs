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

/// Ask Agent: both tools are listed for every client, and without the app they say how to get it
/// instead of failing obscurely.
#[test]
fn ask_agent_tools_are_listed_and_need_the_app() {
    let mut s = Server::new(Box::new(Headless::new()));
    let tools = call(&mut s, 1, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"wait_for_request") && names.contains(&"reply_to_request"), "{names:?}");
    for (name, args) in [("wait_for_request", json!({})), ("reply_to_request", json!({"id": 1, "text": "Done."}))] {
        let r = call(&mut s, 2, "tools/call", json!({"name": name, "arguments": args}));
        assert_eq!(r["result"]["isError"], true, "{name}: {r}");
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(text.contains("DeckCraft window") && text.contains("--connect"), "{name}: {text}");
    }
    // The method itself, through call_app, says the same.
    let r = call(&mut s, 3, "tools/call", json!({"name": "call_app", "arguments": {"method": "agent.listen"}}));
    assert!(r["result"]["content"][0]["text"].as_str().unwrap_or("").contains("DeckCraft window"), "{r}");
}

/// A stand-in for the app's Ask Agent side: hands out one request, records the calls.
struct FakeAgentApp {
    calls: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
    request: Option<Value>,
}

impl crate::Backend for FakeAgentApp {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.calls.lock().unwrap().push((method.to_string(), params.clone()));
        match method {
            "agent.listen" => Ok(self.request.take().unwrap_or(json!({"none": true}))),
            "agent.reply" => Ok(json!({"id": params["id"], "shown": true, "done": params["done"]})),
            other => Err(format!("unexpected {other}")),
        }
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        "fake app".into()
    }
}

#[test]
fn ask_agent_tools_carry_the_contract() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let request = json!({"id": 7, "instruction": "make this slide punchier", "selection": {"kind": "shapes", "slide": {"index": 1, "id": 257, "title": "Results"}, "shapes": [{"id": 300, "kind": "shape", "text": "Sales grew"}], "truncated": false}, "document": {"title": "Review.pptx", "saved": true, "slides": 4, "language": "en-US"}});
    let mut s = Server::new(Box::new(FakeAgentApp { calls: calls.clone(), request: Some(request) }));
    let r = tool(&mut s, "wait_for_request", json!({"timeout_s": 5}));
    assert_eq!(r["id"], 7);
    assert_eq!(r["selection"]["shapes"][0]["id"], 300);
    let blocked = call(&mut s, 3, "tools/call", json!({"name": "wait_for_request", "arguments": {}}));
    assert_eq!(blocked["result"]["isError"], true);
    tool(&mut s, "reply_to_request", json!({"id": 7, "text": "Done"}));
    assert_eq!(tool(&mut s, "wait_for_request", json!({})), json!({"none": true}));
    // The wait is bounded whatever the agent asks for.
    tool(&mut s, "wait_for_request", json!({"timeout_s": 5000}));
    tool(&mut s, "wait_for_request", json!({"timeout_s": -3}));
    let waits: Vec<Value> = calls.lock().unwrap().iter().filter(|(m, _)| m == "agent.listen").map(|(_, p)| p["timeoutMs"].clone()).collect();
    assert_eq!(waits, vec![json!(5000), json!(30000), json!(60000), json!(0)]);
    let r = tool(&mut s, "reply_to_request", json!({"id": 7, "text": "Shortened the title and the three bullets."}));
    assert_eq!(r["shown"], true);
    let (m, p) = calls.lock().unwrap().last().cloned().unwrap();
    assert_eq!((m.as_str(), p), ("agent.reply", json!({"id": 7, "text": "Shortened the title and the three bullets."})));
    tool(&mut s, "reply_to_request", json!({"id": 7, "text": "Halfway", "done": false}));
    assert_eq!(calls.lock().unwrap().last().unwrap().1["done"], false);
    for bad in [json!({"text": "x"}), json!({"id": "seven", "text": "x"}), json!({"id": 7})] {
        let r = call(&mut s, 3, "tools/call", json!({"name": "reply_to_request", "arguments": bad}));
        assert_eq!(r["result"]["isError"], true, "{r}");
    }
}

#[test]
fn ask_agent_schemas_are_the_contracts_common_ones() {
    let mut s = Server::new(Box::new(Headless::new()));
    let tools = call(&mut s, 1, "tools/list", json!({}));
    let ts = tools["result"]["tools"].as_array().unwrap().clone();
    let find = |n: &str| ts.iter().find(|t| t["name"] == n).unwrap().clone();
    let wait = find("wait_for_request");
    assert_eq!(wait["inputSchema"]["additionalProperties"], false, "{wait}");
    assert_eq!(wait["inputSchema"]["properties"]["agent"]["type"], "string", "{wait}");
    let reply = find("reply_to_request");
    assert_eq!(reply["inputSchema"]["required"], json!(["id"]), "{reply}");
    assert_eq!(reply["inputSchema"]["additionalProperties"], false, "{reply}");
    assert_eq!(reply["inputSchema"]["anyOf"].as_array().unwrap().len(), 2, "{reply}");
    for p in ["text", "format", "comment", "error", "done"] {
        assert!(reply["inputSchema"]["properties"].get(p).is_some(), "{p}: {reply}");
    }

    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut s = Server::new(Box::new(FakeAgentApp { calls: calls.clone(), request: Some(json!({"id": 7})) }));
    tool(&mut s, "wait_for_request", json!({"agent": "Review Bot", "timeout_s": 1}));
    let (m, p) = calls.lock().unwrap().last().cloned().unwrap();
    assert_eq!((m.as_str(), p["agent"].clone(), p["timeoutMs"].clone()), ("agent.listen", json!("Review Bot"), json!(1000)));
    tool(&mut s, "reply_to_request", json!({"id": 7, "error": "cannot reach the model"}));
    let (m, p) = calls.lock().unwrap().last().cloned().unwrap();
    assert_eq!(m, "agent.reply");
    assert_eq!(p["error"], "cannot reach the model");
    assert!(p.get("text").is_none(), "only the fields provided are forwarded: {p}");
    let r = call(&mut s, 3, "tools/call", json!({"name": "reply_to_request", "arguments": {"id": 7}}));
    assert_eq!(r["result"]["isError"], true, "{r}");
}

#[test]
fn ask_agent_bridge_attributes_commands_and_releases_after_reply() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut s = Server::new(Box::new(FakeAgentApp { calls: calls.clone(), request: Some(json!({"id": 9})) }));
    tool(&mut s, "wait_for_request", json!({}));
    s.backend().call("engine.execute", json!({"command": "shape.move", "params": {"dx": 5}})).ok();
    assert_eq!(calls.lock().unwrap().last().unwrap().1["request"], 9);
    assert!(s.backend().call("ui.text", json!({"text": "bypass"})).is_err());
    assert!(s.backend().call("agent.listen", json!({})).is_err());
    tool(&mut s, "reply_to_request", json!({"id": 9, "text": "Halfway", "done": false}));
    assert!(s.backend().call("ui.text", json!({"text": "bypass"})).is_err());
    tool(&mut s, "reply_to_request", json!({"id": 9, "text": "Done"}));
    s.backend().call("engine.execute", json!({"command": "shape.move", "params": {"dx": 5}})).ok();
    assert!(calls.lock().unwrap().last().unwrap().1.get("request").is_none());
}

struct ReconnectedAgent {
    refusal: &'static str,
    listens: usize,
    exchanges: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
}
impl crate::Backend for ReconnectedAgent {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.exchanges.lock().unwrap().push((method.into(), params));
        match method {
            "agent.listen" => {
                self.listens += 1;
                Ok(if self.listens == 2 { json!({"none": true}) } else { json!({"id": 7}) })
            }
            "engine.execute" if self.listens == 1 => Err(self.refusal.into()),
            "engine.execute" => Ok(json!({})),
            "agent.reply" if self.listens == 1 && self.refusal.starts_with("no Ask Agent request ") => Err(self.refusal.into()),
            "agent.reply" => Ok(json!({"shown": true, "done": true})),
            _ => Err("unexpected method".into()),
        }
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        "synthetic reconnect".into()
    }
}

#[test]
fn ask_agent_reconnect_releases_stale_ownership_but_blocks_untagged_edits() {
    for refusal in [
        "Ask Agent request 7 is not yours: listen for it first",
        "DeckCraft app at synthetic is not reachable: disconnected",
        "no Ask Agent request 7: it was never made, or the person dismissed it",
    ] {
        let exchanges = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = Server::new(Box::new(ReconnectedAgent { refusal, listens: 0, exchanges: exchanges.clone() }));
        tool(&mut s, "wait_for_request", json!({}));
        let edit = json!({"command": "shape.move", "params": {"dx": 5}});
        let lost = if refusal.starts_with("no Ask Agent request ") {
            s.backend().call("agent.reply", json!({"id": 7, "text": "Done", "done": true}))
        } else {
            s.backend().call("engine.execute", edit.clone())
        };
        assert!(lost.unwrap_err().contains("reacquire"));
        let count = exchanges.lock().unwrap().len();
        assert!(s.backend().call("engine.execute", edit.clone()).is_err());
        assert_eq!(exchanges.lock().unwrap().len(), count, "no untagged edit reaches the app after reconnect");
        assert_eq!(tool(&mut s, "wait_for_request", json!({})), json!({"none": true}));
        assert!(s.backend().call("engine.execute", edit.clone()).is_err(), "a timeout still has no ownership");
        tool(&mut s, "wait_for_request", json!({}));
        s.backend().call("engine.execute", edit).unwrap();
        assert_eq!(exchanges.lock().unwrap().last().unwrap().1["request"], 7);
        tool(&mut s, "reply_to_request", json!({"id": 7, "text": "Done"}));
    }
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
