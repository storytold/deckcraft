use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    assert!(o.status.success(), "exit {:?}\nstderr: {}", o.status, String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    assert!(!o.status.success(), "exit {:?}\nstdout: {}", o.status, String::from_utf8_lossy(&o.stdout));
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const SUBCOMMANDS: [&str; 11] = ["info", "render", "render-transition", "convert", "run", "commands", "describe", "app", "mcp", "bench", "links"];

fn words(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn command_file_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-cli-command-file-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn inspected_titles(out: &str) -> Vec<String> {
    let answer = out.lines().last().unwrap().strip_prefix("document.inspect → ").unwrap();
    let doc: serde_json::Value = serde_json::from_str(answer).unwrap();
    doc["slides"].as_array().unwrap().iter().map(|s| s["title"].as_str().unwrap().to_string()).collect()
}

#[test]
fn command_file_runs_utf8_lines_in_order_and_skips_comments() {
    let dir = command_file_dir("lines");
    let file = dir.join("commands with spaces.txt");
    std::fs::write(
        &file,
        "\u{feff}# heading\r\n\r\n  # indented comment\r\n  slide.new={\"title\":\"Résumé = # 测试\"}  \r\nslide.new\r\nslide.new={\"title\":\"Last\"}",
    )
    .unwrap();
    let o = cli(&["run", "--print", "--cmd-file", &file.to_string_lossy(), "--cmd", "document.inspect"]);
    let out = stdout(&o);
    let commands: Vec<_> = out.lines().map(|l| l.split_once(" → ").unwrap().0).collect();
    assert_eq!(commands, ["slide.new", "slide.new", "slide.new", "document.inspect"]);
    let titles = inspected_titles(&out);
    assert_eq!(titles.len(), 4);
    assert_eq!(titles[1], "Résumé = # 测试");
    assert_eq!(titles[3], "Last");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn command_files_and_inline_commands_keep_argument_order() {
    let dir = command_file_dir("mixed");
    let first = dir.join("first.txt");
    let second = dir.join("second.txt");
    let saved = dir.join("result.deckcraft");
    std::fs::write(&first, "slide.new={\"title\":\"First file\"}\n").unwrap();
    std::fs::write(&second, "slide.new={\"title\":\"Second file\"}\n").unwrap();
    let o = cli(&[
        "run",
        "--cmd",
        "slide.new={\"title\":\"Inline first\"}",
        "--cmd-file",
        &first.to_string_lossy(),
        "--cmd",
        "slide.new={\"title\":\"Inline middle\"}",
        "--cmd-file",
        &second.to_string_lossy(),
        "--cmd",
        "document.inspect",
        "--print",
        "--save",
        &saved.to_string_lossy(),
    ]);
    let out = stdout(&o);
    let titles = inspected_titles(&out);
    assert_eq!(&titles[1..], ["Inline first", "First file", "Inline middle", "Second file"]);
    assert_eq!(out.lines().count(), 5);
    let reopened = cli(&["run", "--in", &saved.to_string_lossy(), "--cmd", "document.inspect", "--print"]);
    assert_eq!(inspected_titles(&stdout(&reopened)), titles);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failed_file_command_stops_later_commands_and_final_outputs() {
    let dir = command_file_dir("failure");
    let file = dir.join("commands.txt");
    let later = dir.join("later-in-file.deckcraft");
    let inline = dir.join("later-inline.deckcraft");
    let save = dir.join("saved.deckcraft");
    let export = dir.join("exported.png");
    let inline_command = format!("file.saveAs={}", serde_json::json!({"path": inline.to_string_lossy()}));
    for (command, message) in [("slide.new={", "bad JSON"), ("no.such.command", "unknown command"), ("slide.go={\"index\":999}", "slide")] {
        let later_command = format!("file.saveAs={}", serde_json::json!({"path": later.to_string_lossy()}));
        std::fs::write(&file, format!("# header\nslide.new\n{command}\n{later_command}\n")).unwrap();
        std::fs::write(&save, b"keep saved file").unwrap();
        std::fs::write(&export, b"keep exported file").unwrap();
        let o = cli(&[
            "run",
            "--print",
            "--cmd-file",
            &file.to_string_lossy(),
            "--cmd",
            &inline_command,
            "--save",
            &save.to_string_lossy(),
            "--export",
            &export.to_string_lossy(),
        ]);
        let err = stderr(&o);
        assert!(err.contains(&format!("{}:3:", file.display())) && err.contains(message), "{err}");
        assert_eq!(String::from_utf8_lossy(&o.stdout).lines().count(), 1, "only the first command ran");
        assert!(!later.exists() && !inline.exists(), "commands after the failure must not run");
        assert_eq!(std::fs::read(&save).unwrap(), b"keep saved file");
        assert_eq!(std::fs::read(&export).unwrap(), b"keep exported file");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn command_file_read_errors_name_the_path_and_utf8_line() {
    let dir = command_file_dir("read-errors");
    let missing = dir.join("missing.txt");
    let invalid = dir.join("invalid.txt");
    let save = dir.join("out.deckcraft");
    std::fs::write(&invalid, b"slide.new\n\xff\n").unwrap();
    for file in [&missing, &dir, &invalid] {
        let o = cli(&["run", "--cmd-file", &file.to_string_lossy(), "--save", &save.to_string_lossy()]);
        let err = stderr(&o);
        assert!(err.contains(&file.display().to_string()), "{err}");
        if file == &invalid {
            assert!(err.contains(&format!("{}:2:", invalid.display())) && err.contains("UTF-8"), "{err}");
        }
        assert!(!save.exists());
    }
    assert!(stderr(&cli(&["run", "--cmd-file"])).contains("--cmd-file needs a file"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn command_file_rejects_an_oversized_line_before_running_it() {
    let dir = command_file_dir("line-limit");
    let file = dir.join("commands.txt");
    let command = format!("slide.new={{\"title\":\"{}\"}}", "x".repeat(16 * 1024 * 1024));
    std::fs::write(&file, format!("{command}\nslide.new\n")).unwrap();

    let o = cli(&["run", "--print", "--cmd-file", &file.to_string_lossy()]);
    let err = stderr(&o);
    assert!(err.contains(&format!("{}:1:", file.display())) && err.contains("command line exceeds 16777216 byte limit"), "{err}");
    assert!(o.stdout.is_empty(), "the oversized command must not run");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn inline_commands_keep_their_existing_syntax_and_errors() {
    let o = cli(&["run", "--cmd", "slide.new", "--cmd", "slide.new={\"title\":\"Inline = #\"}", "--cmd", "document.inspect", "--print"]);
    let out = stdout(&o);
    let titles = inspected_titles(&out);
    assert_eq!(titles.len(), 3);
    assert_eq!(titles[2], "Inline = #");
    assert!(stderr(&cli(&["run", "--cmd", "slide.new={"])).starts_with("deckcraft-cli: slide.new: bad JSON:"));
    assert!(stderr(&cli(&["run", "--cmd", "no.such.command"])).contains("unknown command `no.such.command`"));
}

#[test]
fn an_unknown_subcommand_is_named_before_the_overview() {
    for args in [vec!["frobnicate"], vec!["help", "frobnicate"], vec!["--info", "--help"]] {
        let err = stderr(&cli(&args));
        assert!(err.contains("unknown subcommand `") && err.contains("\n\ndeckcraft-cli works on"), "{args:?}: {err}");
    }
}

#[test]
fn no_subcommand_shows_the_overview_and_fails() {
    assert!(stderr(&cli(&[])).starts_with("deckcraft-cli works on"));
}

#[test]
fn help_lists_every_subcommand_with_a_run_example() {
    for flag in ["help", "--help", "-h"] {
        let out = stdout(&cli(&[flag]));
        for sub in SUBCOMMANDS {
            assert!(out.contains(&format!("\n  {sub} ")), "{flag} lacks {sub}: {out}");
        }
        assert!(out.contains("deckcraft-cli run --cmd 'slide.new={\"title\":\"Q3 results\"}' --save q3.deckcraft"), "{out}");
    }
}

#[test]
fn every_subcommand_answers_help_and_does_nothing_else() {
    let dir = std::env::temp_dir().join(format!("dc-cli-help-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for sub in SUBCOMMANDS {
        for args in [vec![sub, "--help"], vec![sub, "-h", "out.png"], vec!["help", sub]] {
            let o = Command::new(env!("CARGO_BIN_EXE_deckcraft-cli")).current_dir(&dir).args(&args).output().unwrap();
            assert!(stdout(&o).starts_with(&format!("usage: deckcraft-cli {sub}")), "{args:?}");
        }
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "help wrote nothing");
    std::fs::remove_dir_all(&dir).unwrap();
    for args in [["help", "version"], ["--version", "--help"]] {
        assert!(stdout(&cli(&args)).starts_with("usage: deckcraft-cli --version"), "{args:?}");
    }
    for args in [["help", "help"], ["help", "--help"]] {
        assert!(stdout(&cli(&args)).starts_with("deckcraft-cli works on"), "{args:?}");
    }
}

#[test]
fn run_help_says_what_the_ids_do_not() {
    let out = words(&stdout(&cli(&["run", "--help"])));
    for fact in [
        "COMMAND is a command id, then `=`, then the command's parameters as one JSON object.",
        "points on a 960×540 slide",
        "Slide indices start at 0.",
        "Shape ids are looked up on the current slide only.",
        "A command that adds a shape (shape.insert, insert.textBox, insert.table…) selects it.",
        "format.* acts on the selected text or shapes",
        "--cmd-file FILE reads the same commands from a UTF-8 text file, one per line",
        "commands execute in the order those options appear.",
        "If a command fails, run stops there and saves nothing.",
    ] {
        assert!(out.contains(fact), "{fact}: {out}");
    }
}

#[test]
fn describe_spells_out_a_command() {
    assert_eq!(
        stdout(&cli(&["describe", "shape.insert"])),
        "shape.insert: Shapes, in Insert ▸ Illustrations.

Parameters:
  preset          rect|roundRect|ellipse|triangle|rightArrow|star5|… (see shape.presets)
  rect            [x,y,w,h] pt
  text, optional  string

Run it with: deckcraft-cli run --cmd 'shape.insert={…}'
"
    );
    assert_eq!(
        stdout(&cli(&["describe", "format.size"])),
        "format.size: Font Size, in Home ▸ Font.

Parameters:
  size  pt
Acts on the selected text or shapes.

Run it with: deckcraft-cli run --cmd 'format.size={…}'
"
    );
    assert!(stdout(&cli(&["describe", "edit.copy"])).contains("Answers with {text, html?}.\n\nRun it with: deckcraft-cli run --cmd edit.copy\n"));
    assert!(stdout(&cli(&["describe", "text.set"])).contains("Note: replaces the whole text, keeping the first run's formatting\n"));
    assert_eq!(
        stdout(&cli(&["describe", "shape.setDefault"])),
        "shape.setDefault: Set as Default Shape.\n\nTakes no parameters.\nActs on the selected shapes.\n\nRun it with: deckcraft-cli run --cmd shape.setDefault\n"
    );
}

#[test]
fn describe_shows_alternative_sets_of_parameters() {
    assert!(stdout(&cli(&["describe", "slide.go"])).contains("Parameters:\n  either:\n    index\n  or:\n    id\n"));
    let remove = stdout(&cli(&["describe", "animation.remove"]));
    assert!(remove.contains("  or:\n    ids, optional  (all of the shapes' animations)\n"), "{remove}");
    assert!(remove.ends_with("Run it with: deckcraft-cli run --cmd animation.remove\n") && !remove.contains("Note:"), "{remove}");
    assert!(stdout(&cli(&["describe", "shape.move"])).contains(
        "  either:
    dx, optional   pt (relative)
    dy, optional   pt (relative)
    ids, optional
  or:
    x, optional    pt (absolute)
    y, optional    pt (absolute)
    ids, optional
"
    ));
}

#[test]
fn commands_lists_one_line_per_command() {
    assert_eq!(stdout(&cli(&["commands", "format.size"])), "format.size — Font Size: {size: pt}\n");
    assert_eq!(stdout(&cli(&["commands", "slide.next"])), "slide.next — Next Slide\n");
    assert_eq!(stdout(&cli(&["commands", "shape.presets"])), "shape.presets — Shape Gallery → [{name, label, category}]\n");
}
