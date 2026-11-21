//! `deckcraft-cli run` and `mcp` browse the Library folders the desktop app keeps in its
//! preferences, and only those preferences.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-cli-library-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The binary with its per-user settings folder inside `home`, holding a `prefs.json` that lists
/// `folders`.
fn cli(home: &Path, folders: &[&Path]) -> Command {
    let settings = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/DeckCraft")
    } else if cfg!(windows) {
        home.join("AppData").join("DeckCraft")
    } else {
        home.join(".config").join("deckcraft")
    };
    std::fs::create_dir_all(&settings).unwrap();
    let list: Vec<String> = folders.iter().map(|f| f.to_string_lossy().into_owned()).collect();
    std::fs::write(settings.join("prefs.json"), json!({"libraryFolders": list, "author": "Someone Else"}).to_string()).unwrap();
    let mut c = Command::new(crate::deckcraft_cli());
    c.env("HOME", home).env("XDG_CONFIG_HOME", home.join(".config")).env("APPDATA", home.join("AppData")).env_remove("DECKCRAFT_NO_PREFS");
    c
}

fn folders_from_run(c: &mut Command) -> Value {
    let out = c.args(["run", "--cmd", "library.folders", "--print"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let answer = text.lines().find_map(|l| l.split_once("library.folders → ")).map(|(_, a)| a.to_string()).unwrap_or_default();
    serde_json::from_str(&answer).unwrap_or(Value::Null)
}

#[test]
fn run_browses_the_apps_library_folders_and_no_other_preference() {
    let base = tmpdir("run");
    let (home, lib) = (base.join("home"), base.join("decks"));
    std::fs::create_dir_all(&lib).unwrap();
    let lib = lib.canonicalize().unwrap();
    let folders = folders_from_run(&mut cli(&home, &[&lib]));
    assert_eq!(folders[0]["path"], json!(lib.to_string_lossy()), "{folders}");
    // The author (another preference) isn't taken: a comment is the CLI's default author's.
    let out = cli(&home, &[&lib]).args(["run", "--cmd", r#"comment.add={"text":"hi"}"#, "--cmd", "comment.list", "--print"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("\"author\"") && !text.contains("Someone Else"), "{text}");
    // DECKCRAFT_NO_PREFS leaves them out, as it does for the app.
    let none = folders_from_run(cli(&home, &[&lib]).env("DECKCRAFT_NO_PREFS", "1"));
    assert_eq!(none, json!([]));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn mcp_browses_them_too_inside_its_read_roots() {
    let base = tmpdir("mcp");
    let (home, lib, other) = (base.join("home"), base.join("decks"), base.join("other"));
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let ask = |args: &[&str]| -> Value {
        let mut child = cli(&home, &[&lib]).arg("mcp").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        {
            let stdin = child.stdin.as_mut().unwrap();
            writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#).unwrap();
            let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "run_command", "arguments": {"command": "library.folders", "params": {}}}});
            writeln!(stdin, "{call}").unwrap();
        }
        let out = child.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let reply: Value = text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v["id"] == json!(2)).unwrap_or_default();
        let content = reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string();
        serde_json::from_str(&content).unwrap_or(Value::Null)
    };
    let free = ask(&[]);
    assert_eq!(free.as_array().map(Vec::len), Some(1), "{free}");
    assert!(free[0].get("error").is_none(), "{free}");
    // Under a read root that doesn't hold it, the folder is listed with why it can't be read.
    let root = format!("--automation-read-root={}", other.to_string_lossy());
    let confined = ask(&[&root]);
    assert!(confined[0]["error"].as_str().is_some_and(|e| e.contains("outside the read root")), "{confined}");
    let _ = std::fs::remove_dir_all(base);
}
