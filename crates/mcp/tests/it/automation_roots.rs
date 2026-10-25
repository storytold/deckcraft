//! `deckcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir>`: through the MCP
//! protocol, files open and load only from inside the read root and are saved and exported only
//! inside the write root. Paths that leave a root (`..`, another folder, a sibling with the same
//! prefix, a link) are refused, and nothing is written outside.

use std::path::{Path, PathBuf};

use deckcraft_engine::file_access::AutomationRoots;
use deckcraft_mcp::{Headless, Server};
use serde_json::{Value, json};

/// A 1x1 PNG, so `insert_picture` has something real to read.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A fresh folder holding `in` (the read root, with `a.txt` and `pic.png`), `out` (the write root),
/// `outside` (with `secret.txt` and `pic.png`) and `in2`, and a server confined to `in` and `out`
/// (either may be left out). The server starts with a new presentation.
fn confined(tag: &str, read: bool, write: bool) -> (Server, PathBuf) {
    let base = std::env::temp_dir().join(format!("dc-mcp-roots-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    for d in ["in", "out", "outside", "in2"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    std::fs::write(base.join("in/a.txt"), "Inside text").unwrap();
    std::fs::write(base.join("in/pic.png"), PNG).unwrap();
    std::fs::write(base.join("outside/secret.txt"), "TOP SECRET").unwrap();
    std::fs::write(base.join("outside/pic.png"), PNG).unwrap();
    std::fs::write(base.join("in2/c.txt"), "Sibling").unwrap();
    let (in_dir, out_dir) = (base.join("in"), base.join("out"));
    let roots = AutomationRoots::new(read.then_some(in_dir.as_path()), write.then_some(out_dir.as_path())).unwrap();
    let mut server = Server::new(Box::new(Headless::with_document().with_automation_roots(roots)));
    ok(&mut server, "new_presentation", json!({}));
    (server, base)
}

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

fn ok(server: &mut Server, name: &str, args: Value) -> String {
    let (error, text) = call(server, name, args.clone());
    assert!(!error, "{name} {args}: {text}");
    text
}

fn refused(server: &mut Server, name: &str, args: Value, why: &str) {
    let (error, text) = call(server, name, args.clone());
    assert!(error && text.contains(why), "{name} {args} should be refused ({why}): {text}");
}

fn run(server: &mut Server, command: &str, params: Value) -> (bool, String) {
    call(server, "run_command", json!({"command": command, "params": params}))
}

/// Everything the open presentation says, as one string.
fn deck_text(server: &mut Server) -> String {
    format!("{} {}", ok(server, "inspect_document", json!({})), ok(server, "inspect_slide", json!({})))
}

#[test]
fn presentations_open_only_from_inside_the_read_root() {
    let (mut server, base) = confined("open", true, true);
    let sep = std::path::MAIN_SEPARATOR;
    ok(&mut server, "open_presentation", json!({"path": s(&base.join("in/a.txt"))}));
    assert!(deck_text(&mut server).contains("Inside text"));

    refused(&mut server, "open_presentation", json!({"path": s(&base.join("outside/secret.txt"))}), "outside the read root");
    refused(
        &mut server,
        "open_presentation",
        json!({"path": format!("{}{sep}..{sep}outside{sep}secret.txt", s(&base.join("in")))}),
        "outside the read root",
    );
    refused(&mut server, "open_presentation", json!({"path": s(&base.join("in2/c.txt"))}), "outside the read root");
    // The write root isn't readable: the two kinds of access are separate.
    std::fs::write(base.join("out/d.txt"), "d").unwrap();
    refused(&mut server, "open_presentation", json!({"path": s(&base.join("out/d.txt"))}), "outside the read root");
    // The same through the engine command and the control-channel method.
    let (error, text) = run(&mut server, "file.open", json!({"path": s(&base.join("outside/secret.txt"))}));
    assert!(error && text.contains("outside the read root"), "{text}");
    refused(&mut server, "call_app", json!({"method": "app.open", "params": {"path": s(&base.join("outside/secret.txt"))}}), "outside the read root");
    // Nothing from the refused files reached the deck.
    let text = deck_text(&mut server);
    assert!(!text.contains("TOP SECRET") && !text.contains("Sibling"), "{text}");
    assert!(text.contains("Inside text"), "the refused opens left the deck it had: {text}");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn pictures_media_and_other_path_reads_are_confined() {
    let (mut server, base) = confined("media", true, true);
    ok(&mut server, "insert_picture", json!({"path": s(&base.join("in/pic.png"))}));
    let before = ok(&mut server, "inspect_slide", json!({}));
    refused(&mut server, "insert_picture", json!({"path": s(&base.join("outside/pic.png"))}), "outside the read root");
    for command in ["insert.audio", "insert.video", "insert.picture", "picture.change", "insert.model3d"] {
        let (error, text) = run(&mut server, command, json!({"path": s(&base.join("outside/pic.png"))}));
        assert!(error && text.contains("outside the read root"), "{command}: {text}");
    }
    assert_eq!(ok(&mut server, "inspect_slide", json!({})), before, "refused reads change nothing");

    // Media inside the root gets as far as being read: a refusal there would name the root.
    for command in ["insert.audio", "insert.video"] {
        let (_, text) = run(&mut server, command, json!({"path": s(&base.join("in/pic.png"))}));
        assert!(!text.contains("automation"), "{command}: {text}");
    }
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn every_writing_command_is_refused_outside_the_write_root() {
    let (mut server, base) = confined("write", true, true);
    let outside = |name: &str| s(&base.join("outside").join(name));
    let inside = |name: &str| s(&base.join("out").join(name));
    ok(&mut server, "add_slide", json!({"title": "Second"}));

    refused(&mut server, "save_presentation", json!({"path": outside("x.deckcraft")}), "outside the write root");
    refused(&mut server, "export", json!({"path": outside("x.pptx")}), "outside the write root");
    refused(&mut server, "export", json!({"path": outside("x.pdf")}), "outside the write root");
    refused(&mut server, "export", json!({"path": outside("x.png")}), "outside the write root");
    refused(&mut server, "export", json!({"path": outside("x.txt")}), "outside the write root");
    refused(&mut server, "export", json!({"path": outside("all.png"), "all": true}), "outside the write root");
    refused(&mut server, "call_app", json!({"method": "ui.render", "params": {"path": outside("r.png")}}), "outside the write root");
    refused(&mut server, "call_app", json!({"method": "app.save", "params": {"path": outside("y.deckcraft")}}), "outside the write root");
    refused(&mut server, "call_app", json!({"method": "app.export", "params": {"path": outside("y.pdf")}}), "outside the write root");
    for command in ["file.save", "file.saveAs", "file.export"] {
        let (error, text) = run(&mut server, command, json!({"path": outside("z.deckcraft")}));
        assert!(error && text.contains("outside the write root"), "{command}: {text}");
    }
    // A sibling folder with the same prefix, and `..` out of the root.
    refused(&mut server, "save_presentation", json!({"path": s(&base.join("out2/x.deckcraft"))}), "outside the write root");
    let sep = std::path::MAIN_SEPARATOR;
    refused(
        &mut server,
        "save_presentation",
        json!({"path": format!("{}{sep}..{sep}outside{sep}x.deckcraft", s(&base.join("out")))}),
        "outside the write root",
    );
    // The read root isn't writable.
    refused(&mut server, "save_presentation", json!({"path": s(&base.join("in/x.deckcraft"))}), "outside the write root");

    let leftovers: Vec<_> = std::fs::read_dir(base.join("outside")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(leftovers.len(), 2, "nothing was written beside secret.txt and pic.png: {leftovers:?}");
    assert!(!base.join("out2").exists() && !base.join("in/x.deckcraft").exists());

    // Inside the write root all of it works.
    ok(&mut server, "save_presentation", json!({"path": inside("x.deckcraft")}));
    ok(&mut server, "export", json!({"path": inside("x.pptx")}));
    ok(&mut server, "export", json!({"path": inside("x.pdf")}));
    ok(&mut server, "export", json!({"path": inside("all.png"), "all": true}));
    ok(&mut server, "call_app", json!({"method": "ui.render", "params": {"path": inside("r.png")}}));
    for f in ["x.deckcraft", "x.pptx", "x.pdf", "all1.png", "all2.png", "r.png"] {
        assert!(base.join("out").join(f).exists(), "{f}");
    }
    let temporaries: Vec<_> = std::fs::read_dir(base.join("out"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".saving"))
        .collect();
    assert!(temporaries.is_empty(), "the temporary file is renamed away");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn one_root_grants_only_its_own_access() {
    let (mut read_only, base) = confined("read-only", true, false);
    ok(&mut read_only, "open_presentation", json!({"path": s(&base.join("in/a.txt"))}));
    refused(&mut read_only, "save_presentation", json!({"path": s(&base.join("out/x.deckcraft"))}), "write authority is absent");
    refused(&mut read_only, "export", json!({"path": s(&base.join("out/x.pdf"))}), "write authority is absent");
    refused(&mut read_only, "call_app", json!({"method": "ui.render", "params": {"path": s(&base.join("out/r.png"))}}), "write authority is absent");
    // Calls that return bytes instead of writing a path keep working.
    let (error, _) = call(&mut read_only, "render_slide", json!({"scale": 0.25}));
    assert!(!error);
    assert!(std::fs::read_dir(base.join("out")).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(base);

    let (mut write_only, base) = confined("write-only", false, true);
    refused(&mut write_only, "open_presentation", json!({"path": s(&base.join("in/a.txt"))}), "read authority is absent");
    refused(&mut write_only, "insert_picture", json!({"path": s(&base.join("in/pic.png"))}), "read authority is absent");
    ok(&mut write_only, "save_presentation", json!({"path": s(&base.join("out/x.deckcraft"))}));
    assert!(base.join("out/x.deckcraft").exists());
    let _ = std::fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn links_out_of_a_root_are_refused() {
    let (mut server, base) = confined("links", true, true);
    std::os::unix::fs::symlink(base.join("outside/secret.txt"), base.join("in/leak.txt")).unwrap();
    std::os::unix::fs::symlink(base.join("outside"), base.join("out/away")).unwrap();
    std::os::unix::fs::symlink(base.join("outside/gone.deckcraft"), base.join("out/dangling.deckcraft")).unwrap();
    refused(&mut server, "open_presentation", json!({"path": s(&base.join("in/leak.txt"))}), "outside the read root");
    refused(&mut server, "save_presentation", json!({"path": s(&base.join("out/away/x.deckcraft"))}), "outside the write root");
    refused(&mut server, "save_presentation", json!({"path": s(&base.join("out/dangling.deckcraft"))}), "a link that leads nowhere");
    assert!(!base.join("outside/x.deckcraft").exists() && !base.join("outside/gone.deckcraft").exists());
    assert!(!deck_text(&mut server).contains("TOP SECRET"));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn without_roots_nothing_is_confined() {
    let base = std::env::temp_dir().join(format!("dc-mcp-roots-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("a.txt"), "Anywhere").unwrap();
    let mut server = Server::new(Box::new(Headless::with_document().with_automation_roots(AutomationRoots::new(None, None).unwrap())));
    ok(&mut server, "open_presentation", json!({"path": s(&base.join("a.txt"))}));
    ok(&mut server, "save_presentation", json!({"path": s(&base.join("b.deckcraft"))}));
    assert!(base.join("b.deckcraft").exists());
    let _ = std::fs::remove_dir_all(base);
}
