//! A batch (JSON-RPC or the `batch` tool) is bounded in entries and in the bytes of results it
//! gathers, so a short request cannot copy a large document into memory hundreds of times.

use deckcraft_mcp::{Headless, Server};
use serde_json::{Value, json};

fn server_with_notes(bytes: usize) -> Server {
    let mut server = Server::new(Box::new(Headless::with_document()));
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "run_command", "arguments": {"command": "slide.notes", "params": {"text": "n".repeat(bytes)}}}});
    let reply: Value = serde_json::from_str(&server.handle_line(&line.to_string()).unwrap()).unwrap();
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    server
}

#[test]
fn json_rpc_batches_are_bounded_in_entries_and_reply_bytes() {
    let mut server = server_with_notes(1 << 20);
    let ping = |i: usize| json!({"jsonrpc": "2.0", "id": i, "method": "ping"});
    let too_many: Vec<Value> = (0..1001).map(ping).collect();
    let reply: Value = serde_json::from_str(&server.handle_line(&Value::Array(too_many).to_string()).unwrap()).unwrap();
    assert_eq!(reply["error"]["code"], -32600, "{reply}");
    // 100 inspections of a slide with 1 MB of notes: about 64 are answered, the rest refused.
    let inspect = |i: usize| json!({"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": {"name": "inspect_slide", "arguments": {}}});
    let line = Value::Array((0..100).map(inspect).collect()).to_string();
    let reply = server.handle_line(&line).unwrap();
    assert!(reply.len() < (64 << 20) + (4 << 20), "{} bytes", reply.len());
    let replies: Vec<Value> = serde_json::from_str(&reply).unwrap();
    assert_eq!(replies.len(), 100, "every request is answered");
    assert!(replies.last().unwrap()["error"]["message"].as_str().unwrap().contains("64 MiB"));
}

#[test]
fn the_batch_tool_is_bounded_in_commands_and_result_bytes() {
    let mut server = server_with_notes(1 << 20);
    let call = |server: &mut Server, commands: Vec<Value>| {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "batch", "arguments": {"commands": commands, "keepGoing": true}}});
        let reply: Value = serde_json::from_str(&server.handle_line(&line.to_string()).unwrap()).unwrap();
        (reply["result"]["isError"] == true, reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string())
    };
    let (err, text) = call(&mut server, vec![json!({"command": "document.inspect"}); 1001]);
    assert!(err && text.contains("at most 1000"), "{text}");
    let (err, text) = call(&mut server, vec![json!({"command": "slide.inspect"}); 100]);
    assert!(err && text.contains("64 MiB"), "{}", &text[text.len().saturating_sub(200)..]);
    assert!(text.len() < (64 << 20) + (4 << 20));
}

#[test]
fn the_batch_tool_charges_the_escaped_reply() {
    // Quotes escape twice on the way out (`\\\"`), so 30 results of ~2 MiB serialize to ~120 MiB.
    let mut server = Server::new(Box::new(Headless::with_document()));
    let notes = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "run_command", "arguments": {"command": "slide.notes", "params": {"text": "\"".repeat(1 << 20)}}}});
    server.handle_line(&notes.to_string()).unwrap();
    let commands = vec![json!({"command": "slide.inspect"}); 30];
    let line = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "batch", "arguments": {"commands": commands, "keepGoing": true}}});
    let reply = server.handle_line(&line.to_string()).unwrap();
    assert!(reply.len() < (64 << 20) + (4 << 20), "{} bytes", reply.len());
    assert!(reply.contains("64 MiB"));
}

#[test]
fn the_batch_tool_charges_failures_too() {
    // Each unknown command's error echoes its ~1 MiB id; with keepGoing none of them is a result.
    let mut server = Server::new(Box::new(Headless::with_document()));
    let id = "x".repeat(1 << 20);
    let commands = vec![json!({"command": id}); 70];
    let line = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "batch", "arguments": {"commands": commands, "keepGoing": true}}});
    let reply = server.handle_line(&line.to_string()).unwrap();
    assert!(reply.len() < (64 << 20) + (4 << 20), "{} bytes", reply.len());
    assert!(reply.contains("64 MiB"));
}
