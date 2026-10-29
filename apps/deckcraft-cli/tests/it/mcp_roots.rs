//! `deckcraft-cli mcp` with the automation root flags, run as the real binary.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_deckcraft-cli"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().unwrap()
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Send `calls` (tool name, arguments) to `deckcraft-cli mcp <args>`; returns whether it exited
/// cleanly, its stderr, and the stdout lines after `initialize`, one per call.
fn mcp_session(args: &[&str], calls: &[(&str, Value)]) -> (bool, String, Vec<Value>) {
    let mut child = bin().arg("mcp").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#).unwrap();
        for (i, (name, arguments)) in calls.iter().enumerate() {
            let call = serde_json::json!({"jsonrpc": "2.0", "id": i + 2, "method": "tools/call", "params": {"name": name, "arguments": arguments}});
            writeln!(stdin, "{call}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let replies = stdout.lines().skip(1).map(|l| serde_json::from_str(l).unwrap()).collect();
    (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned(), replies)
}

fn text(reply: &Value) -> &str {
    reply["result"]["content"][0]["text"].as_str().unwrap()
}

#[test]
fn mcp_automation_roots_confine_the_server() {
    let base = tmpdir("roots");
    let (work, elsewhere) = (base.join("work"), base.join("elsewhere"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(work.join("in.txt"), "Inside text").unwrap();
    std::fs::write(elsewhere.join("secret.txt"), "TOP SECRET").unwrap();
    let work_s = work.to_str().unwrap();
    let secret = elsewhere.join("secret.txt").to_str().unwrap().to_string();

    // Both spellings of the flags; inside works, outside is refused with a readable message.
    let read_flag = format!("--automation-read-root={work_s}");
    let calls = [
        ("open_presentation", serde_json::json!({"path": work.join("in.txt").to_str().unwrap()})),
        ("open_presentation", serde_json::json!({"path": secret})),
        ("save_presentation", serde_json::json!({"path": work.join("saved.deckcraft").to_str().unwrap()})),
        ("save_presentation", serde_json::json!({"path": elsewhere.join("saved.deckcraft").to_str().unwrap()})),
    ];
    let (success, stderr, replies) = mcp_session(&[&read_flag, "--automation-write-root", work_s], &calls);
    assert!(success, "{stderr}");
    assert!(stderr.contains("files confined"), "{stderr}");
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    assert_eq!(replies[1]["result"]["isError"], true);
    assert!(text(&replies[1]).contains("outside the read root"), "{}", text(&replies[1]));
    assert_eq!(replies[2]["result"]["isError"], false, "{}", replies[2]);
    assert!(work.join("saved.deckcraft").exists());
    assert_eq!(replies[3]["result"]["isError"], true);
    assert!(text(&replies[3]).contains("outside the write root"), "{}", text(&replies[3]));
    assert!(!elsewhere.join("saved.deckcraft").exists());

    // A root left out grants none of its access.
    let save = [("new_presentation", serde_json::json!({})), calls[2].clone()];
    let (_, _, replies) = mcp_session(&[&read_flag], &save);
    assert_eq!(replies[1]["result"]["isError"], true);
    assert!(text(&replies[1]).contains("write authority is absent"), "{}", text(&replies[1]));

    // No flags: nothing is confined.
    let (success, _, replies) = mcp_session(&[], &calls[1..2]);
    assert!(success);
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn mcp_automation_root_flags_are_checked_up_front() {
    let base = tmpdir("roots-args");
    let dir = base.to_str().unwrap();
    let missing = base.join("missing");
    let fails = |args: &[&str], needle: &str| {
        let mut full = vec!["mcp"];
        full.extend_from_slice(args);
        let o = run(&full);
        assert!(!o.status.success(), "{args:?}");
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        assert!(err.contains(needle), "{args:?}: {err}");
    };
    // The flags confine the in-process server, so they cannot be combined with --connect.
    fails(&["--connect", "7979", "--automation-read-root", dir], "--connect");
    fails(&["--automation-write-root", dir, "--connect", "7979"], "--connect");
    fails(&["--automation-read-root", dir, "--automation-read-root", dir], "given twice");
    fails(&["--automation-write-root", dir, "--automation-write-root", dir], "given twice");
    fails(&["--automation-read-root"], "needs a folder");
    fails(&["--automation-write-root", "--sample"], "needs a folder");
    fails(&["--automation-read-root", missing.to_str().unwrap()], "cannot open the automation read root");
    fails(&["--automation-write-root="], "needs a non-empty folder");
    // A file is not a folder.
    std::fs::write(base.join("f.txt"), "x").unwrap();
    fails(&["--automation-write-root", base.join("f.txt").to_str().unwrap()], "cannot open the automation write root");
    let _ = std::fs::remove_dir_all(base);
}
