//! Prints `input<TAB>to_latex(from_linear(input))` for each line of the file given as argument.
fn main() {
    let path = std::env::args().nth(1).unwrap_or_default();
    let src = std::fs::read_to_string(&path).unwrap_or_default();
    for l in src.lines().filter(|l| !l.is_empty()) {
        println!("{l}\t{}", equashion_math::to_latex(&equashion_math::from_linear(l)));
    }
}
