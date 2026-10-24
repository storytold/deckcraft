//! Check the immutable writer sources and keep PDF reading out of production exports.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const SHARED_REV: &str = "9f75d3dc19b4b514c4f3d4847ed1ec1b0e86b4ef";
const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

pub fn run(root: &Path) -> Result<(), String> {
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root)
        .args(["metadata", "--locked", "--format-version", "1"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let metadata: Value = serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    validate(&metadata)?;
    // Metadata merges target features even with --filter-platform. Check Cargo's
    // selected feature graph separately so native compression never enables threads on WASM.
    let output =
        Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into())).arg("-vV").output().map_err(|e| format!("rustc host: {e}"))?;
    if !output.status.success() {
        return Err(format!("rustc host: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let version = String::from_utf8(output.stdout).map_err(|e| format!("rustc host: {e}"))?;
    let host = version.lines().find_map(|line| line.strip_prefix("host: ")).ok_or("rustc did not report its host target")?;
    for (target, parallel) in [(host, true), ("wasm32-unknown-unknown", false)] {
        let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .current_dir(root)
            .args([
                "tree",
                "--locked",
                "--target",
                target,
                "--edges",
                "normal,build",
                "--package",
                "deckcraft-pdf",
                "--depth",
                "2",
                "--no-dedupe",
                "--prefix",
                "none",
                "--format",
                "{p}|{f}",
            ])
            .output()
            .map_err(|e| format!("cargo tree ({target}): {e}"))?;
        if !output.status.success() {
            return Err(format!("cargo tree ({target}): {}", String::from_utf8_lossy(&output.stderr)));
        }
        validate_target_features(&String::from_utf8_lossy(&output.stdout), parallel).map_err(|e| format!("{target}: {e}"))?;
    }
    println!("PDF writer: pinned craft-pdf/Krilla; native-only parallel compression; parser assertions are development-only");
    Ok(())
}

fn validate(metadata: &Value) -> Result<(), String> {
    let packages = metadata.get("packages").and_then(Value::as_array).ok_or("cargo metadata has no packages")?;
    let nodes = metadata.get("resolve").and_then(|r| r.get("nodes")).and_then(Value::as_array).ok_or("cargo metadata has no resolved nodes")?;
    let one_package = |name: &str| -> Result<&Value, String> {
        let matches: Vec<_> = packages.iter().filter(|p| p.get("name").and_then(Value::as_str) == Some(name)).collect();
        if matches.len() != 1 {
            return Err(format!("{name}: expected one resolved package, found {}", matches.len()));
        }
        matches.first().copied().ok_or_else(|| format!("missing {name}"))
    };
    let source = format!("git+https://github.com/storytold/craft-libs?rev={SHARED_REV}#{SHARED_REV}");
    for (name, version, expected_source, expected_features) in [
        ("craft-pdf", "0.1.0", source.as_str(), vec!["write", "write-simple-text"]),
        ("krilla", "0.8.2", source.as_str(), vec!["raster-images", "rayon", "simple-text"]),
        ("hayro-syntax", "0.7.2", REGISTRY, vec!["std"]),
    ] {
        let package = one_package(name)?;
        if package.get("version").and_then(Value::as_str) != Some(version) || package.get("source").and_then(Value::as_str) != Some(expected_source) {
            return Err(format!("{name}: expected {version} from {expected_source}; got {package}"));
        }
        let id = package.get("id").and_then(Value::as_str).ok_or_else(|| format!("{name}: missing package id"))?;
        let node = nodes.iter().find(|n| n.get("id").and_then(Value::as_str) == Some(id)).ok_or_else(|| format!("{name}: no resolved node"))?;
        let features = node.get("features").and_then(Value::as_array).ok_or_else(|| format!("{name}: no resolved features"))?;
        let actual: BTreeSet<_> = features.iter().filter_map(Value::as_str).collect();
        let expected: BTreeSet<_> = expected_features.into_iter().collect();
        if actual != expected {
            return Err(format!("{name}: expected features {expected:?}; got {features:?}"));
        }
    }
    // Metadata includes development dependencies. Follow only normal/build edges from
    // the production exporter so a parser accidentally promoted into it cannot hide there.
    let by_id: HashMap<_, _> = packages.iter().filter_map(|p| p.get("id").and_then(Value::as_str).map(|id| (id, p))).collect();
    let root = one_package("deckcraft-pdf")?.get("id").and_then(Value::as_str).ok_or("deckcraft-pdf has no id")?;
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let package = by_id.get(id).ok_or_else(|| format!("unresolved package {id}"))?;
        let name = package.get("name").and_then(Value::as_str).ok_or("resolved package has no name")?;
        if name == "hayro" || name.starts_with("hayro-") {
            return Err(format!("unexpected production reader dependency {name}"));
        }
        let node = nodes.iter().find(|n| n.get("id").and_then(Value::as_str) == Some(id)).ok_or_else(|| format!("no resolved node for {id}"))?;
        for dep in node.get("deps").and_then(Value::as_array).ok_or_else(|| format!("no dependency edges for {id}"))? {
            let kinds = dep.get("dep_kinds").and_then(Value::as_array).ok_or("dependency has no kinds")?;
            if kinds.is_empty() {
                return Err("dependency has no kinds".into());
            }
            if kinds.iter().any(|k| k.get("kind").and_then(Value::as_str) != Some("dev")) {
                pending.push(dep.get("pkg").and_then(Value::as_str).ok_or("dependency has no package id")?);
            }
        }
    }
    Ok(())
}

fn validate_target_features(tree: &str, parallel: bool) -> Result<(), String> {
    let mut expected = BTreeSet::from(["raster-images", "simple-text"]);
    if parallel {
        expected.insert("rayon");
    }
    let mut found = false;
    for (package, features) in tree.lines().filter_map(|line| line.split_once('|')).filter(|(package, _)| package.starts_with("krilla v")) {
        found = true;
        let actual: BTreeSet<_> = features.split(',').collect();
        if actual != expected {
            return Err(format!("{package}: expected features {expected:?}; got {actual:?}"));
        }
    }
    if !found {
        return Err("missing Krilla in the exporter's target dependency graph".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata() -> Value {
        let source = format!("git+https://github.com/storytold/craft-libs?rev={SHARED_REV}#{SHARED_REV}");
        json!({
            "packages": [
                {"id":"app", "name":"deckcraft-pdf"},
                {"id":"writer", "name":"craft-pdf", "version":"0.1.0", "source":source},
                {"id":"backend", "name":"krilla", "version":"0.8.2", "source":source},
                {"id":"parser", "name":"hayro-syntax", "version":"0.7.2", "source":REGISTRY}
            ],
            "resolve":{"nodes":[
                {"id":"app", "deps":[{"pkg":"writer", "dep_kinds":[{"kind":null}]}, {"pkg":"parser", "dep_kinds":[{"kind":"dev"}]}]},
                {"id":"writer", "features":["write","write-simple-text"], "deps":[{"pkg":"backend", "dep_kinds":[{"kind":null}]}]},
                {"id":"backend", "features":["raster-images","rayon","simple-text"], "deps":[]},
                {"id":"parser", "features":["std"], "deps":[]}
            ]}
        })
    }

    #[test]
    fn parallel_compression_is_required_only_on_native_targets() {
        let serial = "krilla v0.8.2 (pinned)|raster-images,simple-text\n";
        let parallel = "krilla v0.8.2 (pinned)|raster-images,rayon,simple-text\n";
        assert!(validate_target_features(serial, false).is_ok());
        assert!(validate_target_features(parallel, true).is_ok());
        assert!(validate_target_features(serial, true).is_err());
        assert!(validate_target_features(parallel, false).is_err());
        assert!(validate_target_features("", false).is_err());
    }

    #[test]
    fn rejects_unpinned_or_duplicate_writer_sources() {
        assert!(validate(&metadata()).is_ok());
        for source in [Value::Null, json!(REGISTRY), json!("git+https://github.com/storytold/craft-libs#other")] {
            let mut m = metadata();
            m["packages"][1]["source"] = source;
            assert!(validate(&m).is_err());
        }
        let mut m = metadata();
        let duplicate = m["packages"][2].clone();
        m["packages"].as_array_mut().unwrap().push(duplicate);
        assert!(validate(&m).is_err());
    }

    #[test]
    fn rejects_import_features_and_missing_text_shaping() {
        for (node, features) in [
            (1, json!([])),
            (1, json!(["write"])),
            (1, json!(["write", "write-simple-text", "write-pdf"])),
            (2, json!(["raster-images", "rayon", "simple-text", "pdf"])),
            (2, json!(["raster-images"])),
            (3, json!(["std", "images"])),
        ] {
            let mut m = metadata();
            m["resolve"]["nodes"][node]["features"] = features;
            assert!(validate(&m).is_err());
        }
    }

    #[test]
    fn rejects_dev_parser_promoted_to_production_directly_or_transitively() {
        for kind in [Value::Null, json!("build")] {
            let mut m = metadata();
            m["resolve"]["nodes"][0]["deps"][1]["dep_kinds"][0]["kind"] = kind;
            assert!(validate(&m).is_err());
        }
        let mut m = metadata();
        m["resolve"]["nodes"][2]["deps"] = json!([{"pkg":"parser", "dep_kinds":[{"kind":null}]}]);
        assert!(validate(&m).is_err());
    }

    #[test]
    fn rejects_incomplete_dependency_metadata() {
        let mut m = metadata();
        m["resolve"]["nodes"][2]["deps"] = json!([{"pkg":"missing", "dep_kinds":[{"kind":null}]}]);
        assert!(validate(&m).is_err());
        let mut m = metadata();
        m["resolve"] = Value::Null;
        assert!(validate(&m).is_err());
        let mut m = metadata();
        m["packages"].as_array_mut().unwrap().retain(|p| p["name"] != "hayro-syntax");
        assert!(validate(&m).is_err());
    }
}
