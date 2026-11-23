//! What typed text means in a number field, and how far a scrub moves a value (from FigCraft).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::{Rel, Typed, eval, parse, stepped};
use crate::widgets::{scrub_amount, scrubbed};

#[test]
fn typed_text_is_a_number_arithmetic_or_an_edit_relative_to_the_value() {
    use Typed::{Rel as R, Value as V};
    // (text, in a percentage field, meaning)
    let rows: Vec<(&str, bool, Option<Typed>)> = vec![
        ("12", false, Some(V(12.0))),
        (" 12.5 ", false, Some(V(12.5))),
        ("12,5", false, Some(V(12.5))),
        (".5", false, Some(V(0.5))),
        ("-3", false, Some(V(-3.0))),
        ("- 3", false, Some(V(-3.0))),
        ("10+5", false, Some(V(15.0))),
        ("10 - 3", false, Some(V(7.0))),
        ("2*3+4", false, Some(V(10.0))),
        ("2+3*4", false, Some(V(14.0))),
        ("(2+3)*4", false, Some(V(20.0))),
        ("100/(2+3)", false, Some(V(20.0))),
        ("-(4+1)", false, Some(V(-5.0))),
        ("((1+2)*(3+4))/7", false, Some(V(3.0))),
        ("2*-3", false, Some(V(-6.0))),
        ("10--2", false, Some(V(12.0))),
        ("12px", false, Some(V(12.0))),
        ("45°", false, Some(V(45.0))),
        ("2x", false, Some(V(2.0))),
        ("300ms", false, Some(V(300.0))),
        ("10px + 2px", false, Some(V(12.0))),
        ("+10", false, Some(R(Rel::Add(10.0)))),
        ("+ 2*3", false, Some(R(Rel::Add(6.0)))),
        ("*2", false, Some(R(Rel::Mul(2.0)))),
        ("/4", false, Some(R(Rel::Div(4.0)))),
        ("*(1+1)", false, Some(R(Rel::Mul(2.0)))),
        ("50%", false, Some(R(Rel::Mul(0.5)))),
        ("+10%", false, Some(R(Rel::Mul(1.1)))),
        ("-10%", false, Some(R(Rel::Mul(0.9)))),
        ("50%", true, Some(V(50.0))),
        ("+10%", true, Some(R(Rel::Add(10.0)))),
        ("50% + 10%", true, Some(V(60.0))),
        ("", false, None),
        ("   ", false, None),
        ("abc", false, None),
        ("1/0", false, None),
        ("/0", false, Some(R(Rel::Div(0.0)))),
        ("(1+2", false, None),
        ("1+2)", false, None),
        ("1+", false, None),
        ("*", false, None),
        ("1 2", false, None),
        ("1..2", false, None),
        ("50%%", false, None),
        ("5%+1", false, None),
        ("1e999", false, None),
        ("٣", false, None),
    ];
    for (text, pct, want) in rows {
        let got = parse(text, pct);
        let same = match (got, want) {
            (Some(V(a)), Some(V(b)))
            | (Some(R(Rel::Add(a))), Some(R(Rel::Add(b))))
            | (Some(R(Rel::Mul(a))), Some(R(Rel::Mul(b))))
            | (Some(R(Rel::Div(a))), Some(R(Rel::Div(b)))) => (a - b).abs() < 1e-9,
            (None, None) => true,
            _ => false,
        };
        assert!(same, "{text:?} (percentage field: {pct}): {got:?} (wanted {want:?})");
    }
    // Relative edits: the value they give, and nothing where there is none.
    assert_eq!(Rel::Add(10.0).apply(5.0), Some(15.0));
    assert_eq!(Rel::Mul(2.0).apply(5.0), Some(10.0));
    assert_eq!(Rel::Div(4.0).apply(10.0), Some(2.5));
    assert_eq!(Rel::Div(0.0).apply(10.0), None);
    assert_eq!(Rel::Mul(f64::MAX).apply(f64::MAX), None);
    // `eval`: every unit is a unit; without a value `+4` is 4.
    for (text, cur, want) in [
        ("12", None, Some(12.0)),
        ("10+5", None, Some(15.0)),
        ("*2", Some(8.0), Some(16.0)),
        ("+4", Some(8.0), Some(12.0)),
        ("+4", None, Some(4.0)),
        ("*2", None, None),
        ("-3", None, Some(-3.0)),
        ("50%", None, Some(50.0)),
        ("1/0", None, None),
        ("abc", None, None),
    ] {
        assert_eq!(eval(text, cur), want, "eval({text:?}, {cur:?})");
    }
    // Junk never panics: long, deep and odd text.
    for junk in ["(".repeat(5000), "1+".repeat(5000), format!("{}1{}", "(".repeat(40), ")".repeat(40)), "−1".into(), "1\u{0}2".into(), "é%".into()]
    {
        let _ = parse(&junk, false);
        let _ = parse(&junk, true);
    }
}

#[test]
fn a_scrub_moves_a_step_a_point_ten_with_shift_and_a_tenth_with_alt() {
    // (points dragged, the field's step, Shift, Alt) → how far the value moves
    let rows = [
        ((25.0, 1.0, false, false), 25.0),
        ((-25.0, 1.0, false, false), -25.0),
        ((10.0, 1.0, true, false), 100.0),
        ((20.0, 1.0, false, true), 2.0),
        ((20.0, 1.0, true, true), 20.0),
        ((10.0, 0.1, false, false), 1.0),
        ((10.0, 0.1, true, false), 10.0),
        ((10.0, 10.0, false, false), 100.0),
        ((0.0, 1.0, true, false), 0.0),
    ];
    for ((dx, step, shift, alt), want) in rows {
        let got = scrub_amount(dx, step, shift, alt);
        assert!((got - want).abs() < 1e-9, "{dx} points, step {step}, shift {shift}, alt {alt}: {got} (wanted {want})");
    }
    // The value shown: whole steps from where the scrub began (it keeps its fraction), tenths
    // with Alt, and no dust from adding tenths.
    for ((from, amount, quantum), want) in [
        ((12.5, 3.4, 1.0), 15.5),
        ((12.5, -3.6, 1.0), 8.5),
        ((10.0, 0.34, 0.1), 10.3),
        ((0.1, 0.2, 0.1), 0.3),
        ((5.0, 7.0, 0.0), 5.0),
        ((5.0, f64::NAN, 1.0), 5.0),
    ] {
        assert_eq!(scrubbed(from, amount, quantum), want, "from {from}, moved {amount}, quantum {quantum}");
    }
    assert_eq!(stepped(0.1, 1.0, 0.2), 0.3);
    assert_eq!(stepped(1.0, 1.0, f64::INFINITY), 1.0);
}
