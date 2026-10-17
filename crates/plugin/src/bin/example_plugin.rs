//! A reference DeckCraft add-in, and the one the plugin host's tests drive.
//!
//! ```text
//! deckcraft-example-plugin --manifest > my-plugins/example/plugin.json
//! deckcraft-cli --plugin my-plugins/example run --sample --cmd 'plugins.run={"plugin":"org.deckcraft.example","command":"agenda"}' --export out.png
//! ```
//!
//! A plugin reads JSON-RPC requests from standard input, one per line, and writes one response
//! line per request to standard output; it exits when its input ends. See `docs/plugins.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::io::{BufRead, Write};

use serde_json::{Value as Json, json};

fn manifest() -> Json {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "deckcraft-example-plugin".into());
    json!({
        "id": "org.deckcraft.example",
        "name": "Example add-in",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Reference add-in: a few commands",
        "author": "DeckCraft contributors",
        "homepage": "https://github.com/storytold/deckcraft/blob/main/docs/plugins.md",
        "apps": ["deckcraft"],
        "run": {"command": exe},
        "commands": [
            {"id": "agenda", "label": "Agenda Slide", "description": "Adds a slide titled Agenda listing the slides' titles", "params": "{title?: string = \"Agenda\"}"},
            {"id": "stamp", "label": "Stamp", "description": "Adds a text box saying which slide this is"},
            {"id": "broken", "label": "Broken", "description": "Adds a slide, then runs a command that fails (the engine undoes both)"}
        ],
        "ui": {
            "ribbon": [
                {"tab": "Example", "group": "Slides", "buttons": [
                    {"command": "agenda", "icon": "plus"},
                    {"command": "agenda", "label": "Contents", "size": "small", "params": {"title": "Contents"}},
                    {"command": "stamp", "size": "small"}
                ]},
                {"tab": "Insert", "group": "Example", "buttons": [{"command": "stamp"}]}
            ],
            "menus": [
                {"menu": "slide", "command": "stamp", "label": "Stamp (Example)"},
                {"menu": "shape", "command": "agenda", "label": "Agenda Slide (Example)"}
            ]
        }
    })
}

fn main() {
    if std::env::args().any(|a| a == "--manifest") {
        let _ = writeln!(std::io::stdout(), "{}", serde_json::to_string_pretty(&manifest()).unwrap_or_default());
        return;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Json>(&line) else { continue };
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Json::Null);
        let Some(id) = msg.get("id").cloned() else {
            if method == "shutdown" {
                break;
            }
            continue;
        };
        let reply = match method {
            "initialize" => Ok(json!({"protocolVersion": 1})),
            "commands/run" => command(&params),
            other => Err(format!("unknown method {other}")),
        };
        let out = match reply {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(message) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": message}}),
        };
        if writeln!(stdout, "{out}").and_then(|_| stdout.flush()).is_err() {
            break;
        }
    }
}

fn command(p: &Json) -> Result<Json, String> {
    let ctx = p.get("context").cloned().unwrap_or(Json::Null);
    let slide = ctx.get("slide").and_then(Json::as_u64).unwrap_or(0);
    match p.get("command").and_then(Json::as_str).unwrap_or("") {
        "agenda" => {
            let title = p.get("params").and_then(|q| q.get("title")).and_then(Json::as_str).unwrap_or("Agenda").to_string();
            let titles: Vec<String> = ctx
                .get("titles")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_str)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect();
            let body = if titles.is_empty() { "Nothing yet".to_string() } else { titles.join("\n") };
            Ok(json!({
                "commands": [{"command": "slide.new", "params": {"layout": "titleAndContent", "title": title, "body": body}}],
                "message": format!("Added {title} ({} slides listed)", titles.len()),
            }))
        }
        "stamp" => Ok(json!({"commands": [
            {"command": "insert.textBox", "params": {"rect": [24, 24, 240, 40], "text": format!("Slide {}", slide + 1)}}
        ]})),
        "broken" => Ok(json!({"commands": [
            {"command": "slide.new", "params": {"layout": "blank"}},
            {"command": "no.suchCommand", "params": {}}
        ]})),
        other => Err(format!("unknown command {other}")),
    }
}
