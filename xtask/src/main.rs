//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json). External tools (`cargo`, `curl`, `tar`) are
//! invoked through `std::process::Command`.

mod assets;
mod bench;
mod ico;
mod layers;
mod parity;
mod pdf_deps;
mod stats;
mod version;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  assets          check that every icon/image/font/asset is attributed in ATTRIBUTION.md
  pdf-deps        verify pinned writer sources and the production feature boundary
  layers          enforce the crate dependency layering (plan/architecture.md §3)
  wasm            cargo check --target wasm32-unknown-unknown for the wasm-safe crates (one call)
  test            workspace tests (cargo nextest when installed, else cargo test), then doctests
  ci              fmt --check, pdf-deps, clippy -D warnings, test, doctest, assets, layers, wasm
                  (stops at first failure)
  stats [--exact] count tests and lines per crate (--exact: ask the test harness via `-- --list`)
  bench [--save] [DECK...]
                  time open, render, save and export of the sample (and DECKs) in release, and
                  compare with docs/benchmarks/baseline.json (--save writes it)
  parity          recompute the PowerPoint feature-parity summary in docs/parity-checklist.md
  ico <out.ico> <in.png>...
                  pack square PNGs (<= 256 px) into a Windows .ico (see packaging/icons.sh)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("assets") => assets::run(&root()),
        Some("layers") => cmd_layers(),
        Some("pdf-deps") => pdf_deps::run(&root()),
        Some("wasm") => cmd_wasm(),
        Some("test") => cmd_test().and_then(|()| cmd_doctest()),
        Some("ci") => cmd_ci(),
        Some("stats") => stats::run(&root(), rest.contains(&"--exact")),
        Some("parity") => parity::run(&root()),
        Some("bench") => bench::run(&root(), &rest),
        Some("ico") => ico::run(&rest),
        Some("version") => version::run(&root(), &rest),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask has a parent dir").to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (plan/architecture.md §3)\n");
    println!("{:<28} {:<14} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<14} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// Workspace packages that must build for wasm32: all L0–L5 crates plus
/// the egui shell.
fn wasm_set() -> Result<Vec<String>, String> {
    let crates = layers::from_metadata(&metadata()?)?;
    Ok(crates
        .into_iter()
        .filter(|c| match layers::classify(&c.name) {
            Some(layers::Class::Layer(l)) => l <= 5 || layers::short_name(&c.name) == "ui-egui",
            Some(layers::Class::Standalone) => true,
            _ => false,
        })
        .map(|c| c.name)
        .collect())
}

/// One `cargo check` for the whole set: each crate is checked once, and
/// `--keep-going` still reports every failing crate. Features unify across the
/// set, so each crate is checked with the union of the features the set enables.
fn cmd_wasm() -> Result<(), String> {
    let set = wasm_set()?;
    if set.is_empty() {
        return Err("no wasm-safe crates found in the workspace metadata".into());
    }
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown", "--keep-going"]);
    for pkg in &set {
        c.args(["-p", pkg]);
    }
    run(c, &format!("cargo check --target wasm32-unknown-unknown --keep-going -p {}", set.join(" -p ")))?;
    println!("\nwasm32-unknown-unknown check: ok ({} crates: {})", set.len(), set.join(", "));
    Ok(())
}

/// Test targets for the test build. Examples are left out so they aren't
/// linked; clippy `--all-targets` still compiles them.
const TEST_TARGETS: [&str; 4] = ["--workspace", "--lib", "--bins", "--tests"];

fn nextest_available() -> bool {
    cargo().args(["nextest", "--version"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn cmd_test() -> Result<(), String> {
    let mut c = cargo();
    let tool = if nextest_available() {
        c.args(["nextest", "run"]);
        "nextest run"
    } else {
        eprintln!("note: cargo-nextest is not installed, running cargo test (cargo install cargo-nextest --locked)");
        c.arg("test");
        "test"
    };
    c.args(TEST_TARGETS);
    run(c, &format!("cargo {tool} {}", TEST_TARGETS.join(" ")))
}

/// nextest does not run doctests.
fn cmd_doctest() -> Result<(), String> {
    let mut c = cargo();
    c.args(["test", "--workspace", "--doc"]);
    run(c, "cargo test --workspace --doc")
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        ("pdf-deps", Box::new(|| pdf_deps::run(&root()))),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--keep-going", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets --keep-going -- -D warnings")
            }),
        ),
        ("test", Box::new(cmd_test)),
        ("doctest", Box::new(cmd_doctest)),
        ("assets", Box::new(|| assets::run(&root()))),
        ("layers", Box::new(cmd_layers)),
        ("wasm", Box::new(cmd_wasm)),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            for (n, _) in steps.iter().skip(done.len() + 1) {
                println!("  skip  {n}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}
