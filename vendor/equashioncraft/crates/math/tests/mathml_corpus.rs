//! MathML over the whole corpus (`corpus/equations.tsv`, `corpus/latex.tsv`, `corpus/omml/*.xml`
//! and the template palette): the output is well-formed XML with the MathML namespace on the
//! root, has no DOCTYPE and no named entities, and `from_mathml(to_mathml(m))` is stable.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use equashioncraft_math::{Math, Node, Seq, from_linear, from_mathml, from_omml, templates, to_mathml, to_mathml_display};
use quick_xml::Reader;
use quick_xml::events::Event;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Every equation of the corpus: (name, tree, from linear text).
fn corpus() -> Vec<(String, Math, bool)> {
    let mut out = Vec::new();
    let eq = std::fs::read_to_string(corpus_dir().join("equations.tsv")).unwrap();
    for l in eq.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = l.split('\t').collect();
        let mut m = from_linear(f[1]);
        m.para = f[2].trim() == "1";
        out.push((f[0].to_string(), m, true));
    }
    let lx = std::fs::read_to_string(corpus_dir().join("latex.tsv")).unwrap();
    for l in lx.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let src = l.split('\t').next().unwrap();
        out.push((format!("latex.tsv {src}"), from_linear(src), true));
    }
    let mut files: Vec<_> = std::fs::read_dir(corpus_dir().join("omml")).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    for p in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "xml")) {
        let x = std::fs::read_to_string(&p).unwrap();
        out.push((p.file_name().unwrap().to_string_lossy().to_string(), from_omml(&x), false));
    }
    for t in templates() {
        out.push((format!("template {}", t.id), t.math.clone(), false));
    }
    // Constructs the corpus files do not happen to use.
    for src in [r"\text{if } x>0", r"{}_a^b X", r"\underline{x}", r"\mathbf{v}+\boldsymbol{w}"] {
        out.push((format!("extra {src}"), from_linear(src), true));
    }
    assert!(out.len() > 100, "{} items", out.len());
    out
}

/// Parse with quick-xml (end names checked); panics on any error. Returns the element names.
fn well_formed(name: &str, x: &str) -> Vec<String> {
    let mut r = Reader::from_str(x);
    let mut names = Vec::new();
    let mut depth = 0i32;
    let mut roots = 0;
    loop {
        match r.read_event() {
            Ok(Event::Start(b)) => {
                if depth == 0 {
                    roots += 1;
                    let ns = b.attributes().flatten().find(|a| a.key.as_ref() == b"xmlns").map(|a| a.value.to_vec());
                    assert_eq!(ns.as_deref(), Some(&b"http://www.w3.org/1998/Math/MathML"[..]), "{name}: root namespace");
                }
                depth += 1;
                names.push(String::from_utf8_lossy(b.name().as_ref()).to_string());
            }
            Ok(Event::Empty(b)) => {
                assert!(depth > 0, "{name}: empty root");
                names.push(String::from_utf8_lossy(b.name().as_ref()).to_string());
            }
            Ok(Event::End(_)) => depth -= 1,
            Ok(Event::GeneralRef(g)) => assert!(g.is_char_ref(), "{name}: named entity &{};", String::from_utf8_lossy(&g)),
            Ok(Event::DocType(_)) => panic!("{name}: DOCTYPE"),
            Ok(Event::Decl(_)) => panic!("{name}: XML declaration"),
            Ok(Event::Text(t)) => assert!(depth > 0 || t.iter().all(u8::is_ascii_whitespace), "{name}: text outside the root"),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => panic!("{name}: not well-formed at {}: {e}\n{x}", r.buffer_position()),
        }
    }
    assert_eq!(depth, 0, "{name}: unclosed");
    assert_eq!(roots, 1, "{name}: one root");
    assert_eq!(names.first().map(String::as_str), Some("math"), "{name}");
    assert!(names.iter().all(|n| !n.contains(':')), "{name}: prefixed element");
    names
}

#[test]
fn every_corpus_item_is_well_formed_mathml() {
    let mut seen = std::collections::BTreeSet::new();
    for (name, m, _) in corpus() {
        let x = to_mathml(&m);
        assert!(x.starts_with("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\""), "{name}: {x}");
        let block = to_mathml_display(&m, true);
        assert!(block.starts_with("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\">"), "{name}");
        // No named entity at all, not even the five XML ones.
        assert!(!x.contains("&amp;") && !x.contains("&lt;") && !x.contains("&gt;") && !x.contains("&quot;") && !x.contains("&apos;"), "{name}: {x}");
        for n in well_formed(&name, &x) {
            seen.insert(n);
        }
        well_formed(&name, &block);
    }
    for want in [
        "mfrac",
        "msqrt",
        "mroot",
        "msup",
        "msub",
        "msubsup",
        "munderover",
        "munder",
        "mover",
        "mtable",
        "mtr",
        "mtd",
        "mi",
        "mn",
        "mo",
        "mtext",
        "mspace",
        "mrow",
        "menclose",
    ] {
        assert!(seen.contains(want), "no corpus item uses <{want}>; seen {seen:?}");
    }
    assert!(!seen.contains("mfenced"));
}

/// The tree with what MathML does not carry removed: OMML-only properties, typed ASCII spaces.
fn norm(s: &Seq) -> Seq {
    let mut out: Seq = Vec::new();
    for n in s {
        let n = match n.bare().clone() {
            Node::Text(t) => Node::Text(t.replace(' ', "")),
            Node::Frac { num, den } => Node::Frac { num: norm(&num), den: norm(&den) },
            Node::Rad { deg, body } => Node::Rad { deg: deg.map(|d| norm(&d)), body: norm(&body) },
            Node::Script { base, sub, sup } => Node::Script { base: norm(&base), sub: sub.map(|d| norm(&d)), sup: sup.map(|d| norm(&d)) },
            Node::Nary { op, sub, sup, body } => Node::Nary { op, sub: sub.map(|d| norm(&d)), sup: sup.map(|d| norm(&d)), body: norm(&body) },
            Node::Delim { open, close, sep, items } => Node::Delim { open, close, sep, items: items.iter().map(norm).collect() },
            Node::Matrix { rows } => Node::Matrix { rows: rows.iter().map(|r| r.iter().map(norm).collect()).collect() },
            Node::Func { name, body } => Node::Func { name, body: norm(&body) },
            Node::Accent { ch, body } => Node::Accent { ch, body: norm(&body) },
            Node::Limit { lower, base, lim } => Node::Limit { lower, base: norm(&base), lim: norm(&lim) },
            Node::EqArr { rows } => Node::EqArr { rows: rows.iter().map(norm).collect() },
            Node::PreScript { sub, sup, base } => Node::PreScript { sub: norm(&sub), sup: norm(&sup), base: norm(&base) },
            Node::Bar { top, body } => Node::Bar { top, body: norm(&body) },
            Node::GroupChr { ch, top, body } => Node::GroupChr { ch, top, body: norm(&body) },
            Node::Boxed { border, body } => Node::Boxed { border, body: norm(&body) },
            other => other,
        };
        match (out.last_mut(), n) {
            (_, Node::Text(t)) if t.is_empty() => {}
            (Some(Node::Text(a)), Node::Text(b)) => a.push_str(&b),
            (_, n) => out.push(n),
        }
    }
    out
}

#[test]
fn from_mathml_of_to_mathml_is_stable() {
    for (name, m, typed) in corpus() {
        let x1 = to_mathml(&m);
        let back = from_mathml(&x1).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(back.para, m.para, "{name}");
        let x2 = to_mathml(&back);
        assert_eq!(x1, x2, "{name}: not a fixed point");
        // Typed equations come back as the same tree (up to typed spaces).
        if typed {
            assert_eq!(norm(&back.body), norm(&m.body), "{name}\n{x1}");
        }
    }
}
