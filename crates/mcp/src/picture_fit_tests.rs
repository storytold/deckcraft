//! `insert_picture` passes `fit` to `insert.picture`, and `inspect_slide` and `review.pictures`
//! report what the picture looks like as drawn.

use deckcraft_engine::cmd::base64_encode;
use serde_json::{Value, json};

use crate::{Headless, Server};

/// The `result` of calling tool `name` with `args`.
fn call(s: &mut Server, name: &str, args: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string();
    let out = s.handle_line(&line).expect("a response");
    serde_json::from_str::<Value>(&out).expect("json")["result"].clone()
}

/// The JSON a successful call of tool `name` returns.
fn ok(s: &mut Server, name: &str, args: Value) -> Value {
    let res = call(s, name, args);
    assert_eq!(res["isError"], false, "{name} failed: {res}");
    let text =
        res["content"].as_array().unwrap().iter().find(|c| c["type"] == "text").map(|c| c["text"].as_str().unwrap().to_string()).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

/// A 400 × 300 px picture into a 420 × 150 pt box: `fit: "contain"` gives it a 200 × 150 pt box,
/// centred, and without `fit` the box stretches it, which `review.pictures` reports.
#[test]
fn insert_picture_passes_fit_to_the_engine() {
    let mut s = Server::new(Box::new(Headless::new()));
    ok(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    let png = deckcraft_render::Image { width: 400, height: 300, pixels: vec![255; 400 * 300 * 4] }.to_png();
    let data = base64_encode(&png);
    let picture = |s: &mut Server, y: u32, fit: Option<&str>| {
        let mut args = json!({"data": data, "name": "picture.png", "x": 40, "y": y, "w": 420, "h": 150});
        if let Some(fit) = fit {
            args["fit"] = json!(fit);
        }
        ok(s, "insert_picture", args)["id"].as_u64().unwrap()
    };
    let contained = picture(&mut s, 40, Some("contain"));
    let stretched = picture(&mut s, 300, None);
    let slide = ok(&mut s, "inspect_slide", json!({}));
    let shapes = slide["shapes"].as_array().unwrap();
    let entry = |id: u64| shapes.iter().find(|e| e["id"].as_u64() == Some(id)).cloned().unwrap_or_else(|| panic!("{slide}"));
    assert_eq!(entry(contained)["box"], json!([150.0, 40.0, 200.0, 150.0]), "{slide}");
    assert_eq!(entry(contained)["aspect"], json!({"source": 1.333, "shown": 1.333}), "{slide}");
    assert_eq!(entry(stretched)["aspect"], json!({"source": 1.333, "shown": 2.8}), "{slide}");
    let found = ok(&mut s, "run_command", json!({"command": "review.pictures", "params": {}}));
    let kinds: Vec<(u64, &str)> = found
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["shape"].as_u64().unwrap(), e["kind"].as_str().unwrap()))
        .filter(|(id, _)| [contained, stretched].contains(id))
        .collect();
    assert_eq!(kinds, [(stretched, "stretched"), (stretched, "lowResolution")], "{found}");

    // A fit the engine doesn't know is refused with `fit` named, and nothing is inserted.
    let r = call(&mut s, "insert_picture", json!({"data": data, "name": "picture.png", "fit": "fill"}));
    assert_eq!(r["isError"], true, "{r}");
    assert!(r["content"][0]["text"].as_str().unwrap_or_default().contains("`fit` must be stretch, contain or cover"), "{r}");
    assert_eq!(ok(&mut s, "inspect_slide", json!({}))["shapes"].as_array().unwrap().len(), shapes.len());
}
