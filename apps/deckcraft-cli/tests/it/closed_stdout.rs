use std::fs::{self, File};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("deckcraft-closed-stdout-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn closed_stdout() -> Stdio {
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    Stdio::from(OwnedFd::from(writer))
}

fn run(dir: &Path, args: &[&str], stdout: Stdio) -> Output {
    Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).current_dir(dir).args(args).stdout(stdout).stderr(Stdio::piped()).output().unwrap()
}

#[test]
fn broken_pipe_does_not_skip_later_commands_or_save() {
    let dir = Scratch::new("save");
    let out = run(
        &dir.0,
        &[
            "run",
            "--cmd",
            r#"slide.new={"layout":"titleOnly","title":"First"}"#,
            "--cmd",
            r#"slide.new={"layout":"titleOnly","title":"Later"}"#,
            "--print",
            "--save",
            "out.deckcraft",
        ],
        closed_stdout(),
    );
    assert!(out.status.success(), "exit {:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let saved = dir.0.join("out.deckcraft");
    assert!(saved.is_file(), "requested save was skipped");
    let info = Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).arg("info").arg(saved).output().unwrap();
    assert!(info.status.success(), "{}", String::from_utf8_lossy(&info.stderr));
    let data: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    let slides = data["slides"].as_array().unwrap();
    assert!(slides.iter().any(|slide| slide["title"] == "Later"), "later command did not persist: {data}");
}

#[test]
fn broken_pipe_does_not_hide_a_later_command_failure() {
    let dir = Scratch::new("command-failure");
    let out = run(
        &dir.0,
        &["run", "--cmd", r#"slide.new={"title":"First"}"#, "--cmd", r#"slide.go={"index":999}"#, "--print", "--save", "out.deckcraft"],
        closed_stdout(),
    );
    assert!(!out.status.success(), "a later command failure returned success");
    assert!(!dir.0.join("out.deckcraft").exists(), "failed run unexpectedly saved");
    assert!(String::from_utf8_lossy(&out.stderr).contains("no slide 999"), "missing command failure diagnostic");
}

#[test]
fn broken_pipe_does_not_hide_a_save_failure() {
    let dir = Scratch::new("save-failure");
    fs::create_dir(dir.0.join("out.deckcraft")).unwrap();
    let out = run(&dir.0, &["run", "--cmd", r#"slide.new={"title":"First"}"#, "--print", "--save", "out.deckcraft"], closed_stdout());
    assert!(!out.status.success(), "a failed save returned success");
    assert!(!out.stderr.is_empty(), "missing save failure diagnostic");
}

#[test]
fn broken_pipe_does_not_skip_later_rendered_slides() {
    let dir = Scratch::new("render-all");
    let baseline = run(&dir.0, &["render", "--sample", "--all", "--scale", "0.1", "normal"], Stdio::piped());
    assert!(baseline.status.success(), "{}", String::from_utf8_lossy(&baseline.stderr));
    let closed = run(&dir.0, &["render", "--sample", "--all", "--scale", "0.1", "closed"], closed_stdout());
    assert!(closed.status.success(), "{}", String::from_utf8_lossy(&closed.stderr));
    let names = |folder: &str| {
        let mut files: Vec<_> = fs::read_dir(dir.0.join(folder)).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        files.sort();
        files
    };
    let expected = names("normal");
    assert!(expected.len() > 1, "sample must have multiple slides");
    assert_eq!(names("closed"), expected, "rendering stopped after stdout closed");
}

#[test]
fn other_stdout_write_errors_fail() {
    let dir = Scratch::new("write-failure");
    let path = dir.0.join("readonly");
    fs::write(&path, b"").unwrap();
    let readonly = File::open(path).unwrap();
    let out = run(&dir.0, &["run", "--cmd", r#"slide.new={"title":"First"}"#, "--print"], Stdio::from(readonly));
    assert!(!out.status.success(), "a non-broken stdout error returned success");
    assert!(String::from_utf8_lossy(&out.stderr).contains("can't write to stdout"));
}
