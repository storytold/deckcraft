//! `cargo run -p equashion-math --example typed -- 'sqrt(x)+1/2'`: type each argument into an
//! empty editor and print the resulting tree as linear text and LaTeX.
use equashion_math::{Editor, Math};

fn main() {
    for s in std::env::args().skip(1) {
        let mut e = Editor::new(Math::default());
        e.type_str(&s);
        println!("{s:24} lin={}  tex={}", equashion_math::to_linear(e.math()), e.latex());
    }
}
