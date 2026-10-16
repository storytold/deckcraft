//! The function names the parsers and the editor know. One place, so a name added here is
//! typed, parsed and printed the same way everywhere.

/// Names LaTeX spells as commands (`\sin`); the LaTeX writer prints them as `\name`.
pub(crate) const LATEX: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "log", "ln", "exp", "lim", "max", "min", "det",
    "gcd",
];

/// Names with no LaTeX command; the LaTeX writer prints them as `\operatorname{name}`. They
/// are typed with a parenthesis (`floor(x)`) unless they are also in [`BARE`].
pub(crate) const OPNAMES: &[&str] = &[
    // hyperbolic and the inverse trigonometric / hyperbolic functions LaTeX has no command for
    "csch", "sech", "coth", "arccsc", "arcsec", "arccot", "arcsinh", "arccosh", "arctanh", // rounding
    "abs", "floor", "ceil", "round", "sign", // number theory
    "lcm", "nPr", "nCr", // statistics
    "mean", "median", "stdev", "stdevp", "var", "cov", "corr", "total", "count", "quartile", "quantile", "mad", // matrices
    "trace", "rank", "rref",
];

/// Functions that take an unparenthesised argument (`sin x`, `sech x`).
pub(crate) const BARE: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "log", "ln", "exp", "csch", "sech", "coth",
    "arccsc", "arcsec", "arccot", "arcsinh", "arccosh", "arctanh",
];

/// True for any name that becomes a function node when typed with its argument.
pub(crate) fn is_func(name: &str) -> bool {
    LATEX.contains(&name) || OPNAMES.contains(&name)
}

/// True for a name that is only a function once a `(` follows it (not a bare-argument one).
pub(crate) fn is_call_only(name: &str) -> bool {
    OPNAMES.contains(&name) && !BARE.contains(&name)
}
