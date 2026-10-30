//! `deckcraft-cli mcp --allow-commands` at startup, run as the real binary: a flag without its
//! list, a repeated flag, an unknown command, an oversized `@FILE` or one that is a stream, and
//! `--connect` all stop the server before it serves; a good list starts it held to that list.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

/// Run the CLI with `args`; `output()` gives the child no stdin, so a server that starts ends.
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).args(args).output().unwrap()
}

/// A fresh, empty folder for one test.
fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-cli-allow-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The run must fail with `needle` on stderr, without ever having served.
fn refused(args: &[&str], needle: &str) {
    let out = run(args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{args:?} should not start: {stderr}");
    assert!(stderr.contains(needle), "{args:?}: expected `{needle}` in: {stderr}");
    assert!(!stderr.contains("commands allowed"), "{args:?} served: {stderr}");
}

/// A flag without its list, or given twice, never starts the server.
#[test]
fn a_flag_without_its_list_never_starts_the_server() {
    refused(&["mcp", "--allow-commands"], "--allow-commands requires a list");
    refused(&["mcp", "--sample", "--allow-commands"], "--allow-commands requires a list");
    refused(&["mcp", "--allow-commands", "--sample"], "--allow-commands requires a list");
    refused(&["mcp", "--allow-commands", "slide.*", "--allow-commands", "text.*"], "duplicate --allow-commands");
    refused(&["mcp", "--allow-commands", ""], "the command list is empty");
}

/// `--connect` before or after the flag doesn't get past the checks.
#[test]
fn connect_is_not_a_way_around_the_checks() {
    refused(&["mcp", "--connect", "7990", "--allow-commands"], "--allow-commands requires a list");
    refused(&["mcp", "--allow-commands", "--connect", "7990"], "--allow-commands requires a list");
    refused(&["mcp", "--connect", "7990", "--allow-commands", "slide.*"], "a running app is not held to it");
}

/// Unknown ids and malformed patterns stop the server at startup.
#[test]
fn unknown_commands_and_patterns_are_refused() {
    refused(&["mcp", "--allow-commands", "slide.neww"], "unknown command");
    refused(&["mcp", "--allow-commands", "nope.*"], "matches no command");
    refused(&["mcp", "--allow-commands", "*"], "--allow-commands");
    refused(&["mcp", "--allow-commands", "slide*"], "--allow-commands");
}

/// An oversized `@FILE`, a folder, `/dev/zero` and a FIFO are refused without blocking.
#[test]
fn oversized_and_streamed_list_files_are_refused() {
    let dir = tmpdir("files");
    let big = dir.join("big.txt");
    // Valid ids up front, so only the size can be the reason.
    let mut text = String::from("slide.new\n");
    text.push_str(&"#".repeat(256 * 1024));
    std::fs::write(&big, text).unwrap();
    refused(&["mcp", "--allow-commands", &format!("@{}", big.display())], "too long");
    refused(&["mcp", "--allow-commands", &format!("@{}", dir.display())], "not a regular file");
    #[cfg(unix)]
    {
        refused(&["mcp", "--allow-commands", "@/dev/zero"], "not a regular file");
        let fifo = dir.join("list");
        assert!(Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()), "mkfifo");
        refused(&["mcp", "--allow-commands", &format!("@{}", fifo.display())], "not a regular file");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A good `@FILE` list starts the server held to it: a listed command runs, another is refused.
#[test]
fn a_good_list_starts_the_server_held_to_it() {
    let dir = tmpdir("good");
    std::fs::write(dir.join("list.txt"), "# what the agent may do\nslide.new\nfile.new, document.inspect\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_deckcraft-cli"))
        .args(["mcp", "--allow-commands", &format!("@{}", dir.join("list.txt").display())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        let lines = [
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
            serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "new_presentation", "arguments": {}}}),
            serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "add_slide", "arguments": {"title": "Hi"}}}),
            serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "export", "arguments": {"path": "x.pdf"}}}),
        ];
        for l in lines {
            writeln!(stdin, "{l}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("commands allowed: document.inspect, file.new, slide.new"), "{stderr}");
    let replies: Vec<Value> = String::from_utf8(out.stdout).unwrap().lines().skip(1).map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    assert_eq!(replies[1]["result"]["isError"], false, "{}", replies[1]);
    assert_eq!(replies[2]["result"]["isError"], true, "{}", replies[2]);
    assert!(replies[2].to_string().contains("file.export"), "{}", replies[2]);
    assert!(!std::path::Path::new("x.pdf").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
