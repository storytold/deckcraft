//! `corpus/latex.tsv`: MathLive-style LaTeX inputs and the LaTeX `to_latex` prints for them.
//! The viSHual repo checks the same file against its LaTeX-to-expression converter.

use equashioncraft_math::{from_linear, to_latex};

#[test]
fn every_corpus_line_prints_as_recorded_and_is_stable() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../corpus/latex.tsv");
    let src = std::fs::read_to_string(path).expect("corpus/latex.tsv");
    let mut n = 0;
    for line in src.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let (input, want) = line.split_once('\t').expect("input<TAB>latex");
        let got = to_latex(&from_linear(input));
        assert_eq!(got, want, "input {input:?}");
        assert_eq!(to_latex(&from_linear(&got)), got, "not stable for {input:?}");
        n += 1;
    }
    assert!(n > 50, "corpus has {n} lines");
}
