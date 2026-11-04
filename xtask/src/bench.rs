//! `cargo xtask bench`: the performance baseline. Builds `deckcraft-cli` in release, runs its
//! `bench` on the built-in sample (and any decks given), and compares each step with the saved
//! baseline (`docs/benchmarks/baseline.json`) when there is one; `--save` writes a new baseline.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// A step this many times slower than the baseline is flagged.
const SLOWER: f64 = 1.5;

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let save = args.contains(&"--save");
    let decks: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with("--")).collect();
    let mut build = super::cargo();
    build.args(["build", "--release", "-q", "-p", "deckcraft-cli"]);
    super::run(build, "cargo build --release -p deckcraft-cli")?;
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), Into::into);
    let cli = target.join("release").join(if cfg!(windows) { "deckcraft-cli.exe" } else { "deckcraft-cli" });
    let mut results = vec![];
    let inputs: Vec<Option<&str>> = std::iter::once(None).chain(decks.iter().map(|d| Some(*d))).collect();
    for input in inputs {
        let mut c = Command::new(&cli);
        c.arg("bench").arg("--json");
        match input {
            Some(d) => c.arg(d),
            None => c.arg("--sample"),
        };
        let out = c.output().map_err(|e| format!("{}: {e}", cli.display()))?;
        if !out.status.success() {
            return Err(format!("bench {}: {}", input.unwrap_or("--sample"), String::from_utf8_lossy(&out.stderr).trim()));
        }
        let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("bench output: {e}"))?;
        results.push(v);
    }
    let baseline_path = root.join("docs/benchmarks/baseline.json");
    let baseline: Option<Value> = std::fs::read(&baseline_path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let mut slower = 0;
    for r in &results {
        let file = r["file"].as_str().unwrap_or("?");
        println!("\n{file}: {} slides", r["slides"]);
        let base = baseline.as_ref().and_then(|b| b["results"].as_array()?.iter().find(|x| x["file"] == r["file"]).cloned());
        for step in r["steps"].as_array().into_iter().flatten() {
            let name = step["step"].as_str().unwrap_or("?");
            let ms = step["medianMs"].as_f64().unwrap_or(0.0);
            let was = base.as_ref().and_then(|b| b["steps"].as_array()?.iter().find(|s| s["step"] == step["step"])?["medianMs"].as_f64());
            let cmp = match was {
                Some(w) if w > 0.5 => {
                    let k = ms / w;
                    if k > SLOWER {
                        slower += 1;
                    }
                    format!("  {k:>5.2}× baseline{}", if k > SLOWER { "  SLOWER" } else { "" })
                }
                _ => String::new(),
            };
            println!("  {name:<24} {ms:>9.1} ms{cmp}");
        }
        if let Some(p) = r["peakRssMiB"].as_f64() {
            println!("  peak memory              {p:>9.1} MiB");
        }
    }
    if save {
        if let Some(dir) = baseline_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let v = json!({"note": "cargo xtask bench --save; times depend on the machine, compare on the same one", "results": results});
        std::fs::write(&baseline_path, serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("\nwrote {}", baseline_path.display());
    } else if baseline.is_none() {
        println!("\nno baseline yet: `cargo xtask bench --save` writes docs/benchmarks/baseline.json");
    }
    if slower > 0 { Err(format!("{slower} step(s) more than {SLOWER}× slower than the baseline")) } else { Ok(()) }
}
