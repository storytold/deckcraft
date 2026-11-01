//! Property tests over the shared corpus (`corpus/equations.tsv`, `corpus/omml/*.xml`): the
//! OMML and linear round trips are stable for every item.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use equashioncraft_math::{Math, from_linear, from_omml, to_linear, to_omml, unknown_commands};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// (id, linear, display) rows of `equations.tsv`.
fn rows() -> Vec<(String, String, bool)> {
    let text = std::fs::read_to_string(corpus_dir().join("equations.tsv")).expect("corpus/equations.tsv");
    let rows: Vec<_> = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 3, "malformed row: {l:?}");
            (f[0].to_string(), f[1].to_string(), f[2].trim() == "1")
        })
        .collect();
    assert!(rows.len() >= 40, "{} rows", rows.len());
    rows
}

/// (file name, OMML) of every `corpus/omml/*.xml`.
fn omml_samples() -> Vec<(String, String)> {
    let mut v: Vec<_> = std::fs::read_dir(corpus_dir().join("omml"))
        .expect("corpus/omml")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "xml"))
        .map(|p| (p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read_to_string(&p).unwrap()))
        .collect();
    v.sort();
    assert!(v.len() >= 3);
    v
}

#[test]
fn every_corpus_row_parses_without_unknown_commands() {
    let mut ids = std::collections::HashSet::new();
    for (id, lin, _) in rows() {
        assert!(ids.insert(id.clone()), "duplicate id {id}");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "id {id}");
        let m = from_linear(&lin);
        assert!(!m.body.is_empty(), "{id}");
        assert!(unknown_commands(&m).is_empty(), "{id}: {:?}", unknown_commands(&m));
    }
}

#[test]
fn to_omml_of_from_omml_is_idempotent() {
    let check = |name: &str, m: &Math| {
        let x0 = to_omml(m);
        let x1 = to_omml(&from_omml(&x0));
        assert_eq!(x0, x1, "{name}");
    };
    for (id, lin, display) in rows() {
        let mut m = from_linear(&lin);
        m.para = display;
        check(&id, &m);
    }
    for (name, x) in omml_samples() {
        let m = from_omml(&x);
        assert!(!m.body.is_empty(), "{name} parses to something");
        check(&name, &m);
    }
}

#[test]
fn from_linear_of_to_linear_is_stable() {
    let check = |name: &str, m: &Math| {
        let back = from_linear(&to_linear(m));
        assert_eq!(&back.body, &m.body, "{name}: {}", to_linear(m));
        // And the text itself is a fixed point.
        assert_eq!(to_linear(&back), to_linear(m), "{name}");
    };
    for (id, lin, _) in rows() {
        // The typed (LaTeX-ish) input is not itself the canonical linear text; its tree is.
        check(&id, &from_linear(&lin));
    }
    // OMML run properties (`m:rPr`, `a:rPr`, ...) have no linear form, so for the OMML samples
    // only the linear text is compared: it must be a fixed point.
    for (name, x) in omml_samples() {
        let lin = to_linear(&from_omml(&x));
        assert_eq!(to_linear(&from_linear(&lin)), lin, "{name}");
    }
}
