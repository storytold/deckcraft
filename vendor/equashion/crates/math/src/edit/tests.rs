use super::*;
use crate::{from_linear, template, to_linear};

fn ed() -> Editor {
    Editor::new(Math::default())
}

fn typed(s: &str) -> Editor {
    let mut e = ed();
    e.type_str(s);
    e
}

fn lin(e: &Editor) -> String {
    to_linear(e.math()).replace(' ', "")
}

fn tpl(id: &str) -> Math {
    template(id).map(|t| t.math).unwrap_or_default()
}

#[test]
fn x_caret_2_builds_a_superscript_with_the_caret_inside() {
    let e = typed("x^2");
    assert_eq!(lin(&e), "x^2");
    assert!(matches!(e.math().body.first(), Some(Node::Script { sup: Some(_), .. })));
    assert_eq!(e.caret().path.len(), 1, "caret is inside the superscript");
    assert_eq!(e.caret().pos, 1);
}

#[test]
fn right_arrow_leaves_the_superscript() {
    let mut e = typed("x^2");
    e.key(Key::Right, false);
    assert!(e.caret().path.is_empty());
    e.type_str("+1");
    assert_eq!(lin(&e), "x^2+1");
}

#[test]
fn underscore_makes_a_subscript_and_both_parts_combine() {
    let e = typed("a_1");
    assert_eq!(lin(&e), "a_1");
    let mut e = typed("x^2");
    e.key(Key::Right, false);
    e.type_str("_i");
    assert!(matches!(e.math().body.first(), Some(Node::Script { sub: Some(_), sup: Some(_), .. })), "{:?}", e.math().body);
    assert_eq!(e.math().body.len(), 1, "one script, not two");
}

#[test]
fn a_number_stays_together_as_the_base() {
    let e = typed("12^3");
    assert!(matches!(e.math().body.first(), Some(Node::Script { base, .. }) if base.len() == 1));
    let e = typed("ab^2");
    assert_eq!(e.math().body.len(), 2, "only b takes the exponent");
}

#[test]
fn slash_turns_the_preceding_term_into_a_numerator() {
    let mut e = typed("1+ab/");
    match e.math().body.get(1) {
        Some(Node::Frac { num, den }) => {
            assert_eq!(units(num), 2);
            assert!(den.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(e.caret().path, vec![(1, 1)], "caret is in the denominator");
    e.type_str("c");
    e.key(Key::Right, false);
    e.type_str("=2");
    assert_eq!(lin(&e).replace(['(', ')'], ""), "1+ab/c=2");
}

#[test]
fn slash_with_nothing_before_it_puts_the_caret_in_the_numerator() {
    let e = typed("/");
    assert_eq!(e.caret().path, vec![(0, 0)]);
}

#[test]
fn sqrt_typed_makes_a_root_with_the_caret_inside() {
    let mut e = typed("sqrt");
    assert!(matches!(e.math().body.first(), Some(Node::Rad { deg: None, .. })));
    assert_eq!(e.caret().path, vec![(0, 0)]);
    e.type_str("x+1");
    e.key(Key::Right, false);
    e.type_str("y");
    assert!(lin(&e).contains('√') && lin(&e).ends_with('y'), "{}", lin(&e));
}

#[test]
fn greek_names_become_symbols() {
    assert_eq!(lin(&typed("alpha")), "α");
    assert_eq!(lin(&typed("2pi r")), "2πr");
    assert_eq!(lin(&typed("Delta x")), "Δx");
    assert_eq!(lin(&typed("beta+theta")), "β+θ");
    // A name inside a longer word stays text; typing the middle of epsilon does not fire psi.
    assert_eq!(lin(&typed("epsilon")), "ε");
    assert_eq!(lin(&typed("menu")), "menu");
    assert_eq!(lin(&typed("a b")), "ab", "spaces are ignored");
}

#[test]
fn hyphen_is_a_real_minus() {
    assert_eq!(lin(&typed("a-b")), "a\u{2212}b");
}

#[test]
fn arrows_walk_the_tree_in_visual_order() {
    let mut e = typed("a/b");
    // Caret is in the denominator after b. Left twice: before b, then the end of the numerator.
    e.key(Key::Left, false);
    assert_eq!((e.caret().path.clone(), e.caret().pos), (vec![(0, 1)], 0));
    e.key(Key::Left, false);
    assert_eq!((e.caret().path.clone(), e.caret().pos), (vec![(0, 0)], 1));
    e.key(Key::Left, false);
    e.key(Key::Left, false);
    assert_eq!((e.caret().path.clone(), e.caret().pos), (vec![], 0));
    // And back: Right enters the numerator, crosses to the denominator, leaves.
    e.key(Key::Right, false);
    assert_eq!(e.caret().path, vec![(0, 0)]);
    e.key(Key::Right, false);
    e.key(Key::Right, false);
    assert_eq!(e.caret().path, vec![(0, 1)]);
    e.key(Key::Right, false);
    e.key(Key::Right, false);
    assert_eq!((e.caret().path.clone(), e.caret().pos), (vec![], 1));
}

#[test]
fn up_and_down_move_between_numerator_denominator_and_scripts() {
    let mut e = typed("a/b");
    e.key(Key::Up, false);
    assert_eq!(e.caret().path, vec![(0, 0)]);
    e.key(Key::Down, false);
    assert_eq!(e.caret().path, vec![(0, 1)]);
    let mut e = typed("x^2");
    e.key(Key::Down, false);
    assert_eq!(e.caret().path, vec![(0, 0)], "no subscript: down goes to the base");
    e.key(Key::Up, false);
    assert_eq!(e.caret().path, vec![(0, 1)]);
    // Up from the top row goes to the start, Down to the end.
    let mut e = typed("ab");
    e.key(Key::Up, false);
    assert_eq!(e.caret().pos, 0);
    e.key(Key::Down, false);
    assert_eq!(e.caret().pos, 2);
}

#[test]
fn matrix_cells_are_reached_with_up_and_down() {
    let mut e = ed();
    e.insert_template(&tpl("matrix2"));
    e.type_str("1");
    e.key(Key::Down, false);
    e.type_str("3");
    let m = lin(&e);
    assert!(m.contains('1') && m.contains('3'), "{m}");
    let at = e.caret().path.last().map(|p| p.1);
    assert_eq!(at, Some(2), "second row, first column");
}

#[test]
fn backspace_deletes_the_previous_atom_and_unwraps_empty_structures() {
    let mut e = typed("x^2");
    e.key(Key::Backspace, false);
    assert_eq!(e.caret().path.len(), 1);
    e.key(Key::Backspace, false);
    // The empty exponent is dropped and the base remains.
    assert!(e.caret().path.is_empty());
    assert_eq!(lin(&e), "x");
    assert_eq!(e.caret().pos, 1);
    e.key(Key::Backspace, false);
    assert!(e.is_blank());
    // Backspace behind a whole fraction steps into it; a second one deletes its last atom.
    let mut e = typed("a/b");
    e.key(Key::Right, false);
    e.key(Key::Backspace, false);
    assert!(!e.is_blank(), "one key never wipes a fraction");
    assert_eq!(e.caret().path, vec![(0, 1)]);
    e.key(Key::Backspace, false);
    assert_eq!(e.caret().path, vec![(0, 1)]);
    assert!(!e.is_blank());
    // Backspace in the denominator at its start unwraps the fraction.
    let mut e = typed("a/b");
    e.key(Key::Up, false);
    e.key(Key::Home, false);
    e.key(Key::Backspace, false);
    assert_eq!(lin(&e), "ab");
    assert!(e.caret().path.is_empty());
    assert_eq!(e.caret().pos, 0, "in front of a");
    // At the start of the denominator, Backspace only steps back into the numerator.
    let mut e = typed("a/b");
    e.key(Key::Left, false);
    e.key(Key::Backspace, false);
    assert_eq!(lin(&e).replace(['(', ')'], ""), "a/b");
    assert_eq!(e.caret().path, vec![(0, 0)]);
}

#[test]
fn backspace_in_a_fresh_structure_removes_it() {
    let mut e = ed();
    e.insert_template(&tpl("frac"));
    e.key(Key::Backspace, false);
    assert!(e.is_blank() && e.math().body.is_empty());
    let mut e = ed();
    e.insert_template(&tpl("sqrt"));
    e.key(Key::Backspace, false);
    assert!(e.math().body.is_empty());
}

#[test]
fn delete_removes_the_next_atom() {
    let mut e = typed("abc");
    e.key(Key::Home, false);
    e.key(Key::Delete, false);
    assert_eq!(lin(&e), "bc");
    // Delete in front of a structure steps into it instead of wiping it.
    let mut e = typed("a/b");
    e.key(Key::Home, false);
    e.key(Key::Home, false);
    e.key(Key::Delete, false);
    assert!(!e.is_blank());
    assert_eq!(e.caret().path, vec![(0, 0)]);
    // Delete at the end of an empty structure removes it.
    let mut e = ed();
    e.insert_template(&tpl("sqrt"));
    e.key(Key::Delete, false);
    assert!(e.math().body.is_empty());
}

#[test]
fn home_and_end_go_to_the_row_ends_then_the_whole_equation() {
    let mut e = typed("a+b/cd");
    e.key(Key::Home, false);
    assert_eq!(e.caret().pos, 0);
    assert_eq!(e.caret().path, vec![(1, 1)], "start of the denominator row");
    e.key(Key::Home, false);
    assert!(e.caret().path.is_empty());
    assert_eq!(e.caret().pos, 0);
    e.key(Key::End, false);
    assert_eq!(e.caret().pos, units(&e.math().body));
}

#[test]
fn shift_arrows_select_and_typing_replaces() {
    let mut e = typed("abc");
    e.key(Key::Left, true);
    e.key(Key::Left, true);
    assert_eq!(e.selection(), Some((1, 3)));
    e.type_str("x");
    assert_eq!(lin(&e), "ax");
    assert_eq!(e.selection(), None);
    // Shift+Home selects to the row start; plain Left collapses to its start.
    let mut e = typed("abc");
    e.key(Key::Home, true);
    assert_eq!(e.selection(), Some((0, 3)));
    e.key(Key::Left, false);
    assert_eq!((e.selection(), e.caret().pos), (None, 0));
    let mut e = typed("abc");
    e.select_all();
    e.key(Key::Backspace, false);
    assert!(e.is_blank());
}

#[test]
fn selection_is_wrapped_by_slash_caret_and_templates() {
    let mut e = typed("a+bc");
    e.key(Key::Left, true);
    e.key(Key::Left, true);
    e.type_str("/");
    assert!(matches!(e.math().body.get(1), Some(Node::Frac { num, .. }) if units(num) == 2));
    let mut e = typed("xy");
    e.key(Key::Left, true);
    e.type_str("^");
    assert!(matches!(e.math().body.get(1), Some(Node::Script { base, .. }) if units(base) == 1));
    let mut e = typed("ab");
    e.select_all();
    e.insert_template(&tpl("sqrt"));
    assert!(matches!(e.math().body.first(), Some(Node::Rad { body, .. }) if units(body) == 2));
    assert_eq!(e.caret().path, vec![(0, 0)]);
    assert_eq!(e.caret().pos, 2, "caret after the wrapped text");
}

#[test]
fn palette_structures_land_the_caret_in_the_first_blank() {
    let mut e = ed();
    e.insert_template(&tpl("frac"));
    assert_eq!(e.caret().path, vec![(0, 0)]);
    e.type_str("a");
    e.key(Key::Down, false);
    e.type_str("b");
    assert_eq!(lin(&e).replace(['(', ')'], ""), "a/b");
    // A superscript from the palette takes the letter before the caret.
    let mut e = typed("b");
    e.insert_template(&tpl("sup"));
    e.type_str("2");
    assert_eq!(lin(&e), "b^2");
    // An integral puts the caret in its lower limit.
    let mut e = ed();
    e.insert_template(&tpl("intlim"));
    e.type_str("0");
    e.key(Key::Right, false);
    e.type_str("1");
    assert!(lin(&e).contains('0') && lin(&e).contains('1'));
    // A fraction takes the term before it.
    let mut e = typed("x+12");
    e.insert_template(&tpl("frac"));
    assert!(matches!(e.math().body.get(1), Some(Node::Frac { num, .. }) if units(num) == 2));
    assert_eq!(e.caret().path, vec![(1, 1)]);
}

#[test]
fn every_template_inserts_and_accepts_typing() {
    for t in crate::templates() {
        let mut e = typed("x");
        e.insert_template(&t.math);
        assert!(e.is_valid(), "{}", t.id);
        e.type_str("y");
        assert!(e.is_valid(), "{}", t.id);
        assert!(lin(&e).contains('y'), "{}: {}", t.id, lin(&e));
        // Left to the start and Right to the end never get stuck.
        for _ in 0..50 {
            e.key(Key::Left, false);
        }
        assert_eq!((e.caret().path.len(), e.caret().pos), (0, 0), "{}", t.id);
        for _ in 0..50 {
            e.key(Key::Right, false);
        }
        assert_eq!(e.caret().path.len(), 0, "{}", t.id);
        assert_eq!(e.caret().pos, units(&e.math().body), "{}", t.id);
    }
}

#[test]
fn display_math_shows_a_box_in_every_empty_place_and_leaves_the_tree_alone() {
    let mut e = ed();
    e.insert_template(&tpl("frac"));
    let d = to_linear(&e.display_math());
    assert_eq!(d.matches('\u{25A1}').count(), 2, "{d}");
    assert!(e.math().body.iter().all(|n| children(n).iter().all(|k| k.is_empty())));
}

#[test]
fn set_caret_clamps_stale_positions() {
    let mut e = typed("ab");
    e.set_caret(Caret { path: vec![(7, 3), (1, 1)], pos: 99 });
    assert!(e.is_valid());
    e.set_caret(Caret { path: vec![], pos: 99 });
    assert_eq!(e.caret().pos, 2);
    let mut e = typed("a/b");
    e.set_caret(Caret { path: vec![(0, 1)], pos: 0 });
    e.drag_caret(Caret { path: vec![(0, 1)], pos: 1 });
    assert_eq!(e.selection(), Some((0, 1)));
    e.drag_caret(Caret { path: vec![], pos: 0 });
    assert_eq!(e.selection(), None, "dragging out of the row just moves the caret");
}

#[test]
fn loaded_equations_are_editable() {
    let mut e = Editor::new(from_linear("x^2+1"));
    e.key(Key::Left, false);
    e.type_str("0");
    assert_eq!(lin(&e), "x^2+01");
    e.key(Key::Home, false);
    e.key(Key::Home, false);
    e.key(Key::Right, false);
    assert!(e.is_valid());
}

#[test]
fn nesting_depth_is_capped() {
    let mut e = ed();
    for _ in 0..(MAX_DEPTH * 3) {
        e.type_str("x^");
    }
    for _ in 0..(MAX_DEPTH * 3) {
        e.type_str("/");
        e.insert_template(&tpl("frac"));
    }
    assert!(e.caret().path.len() <= MAX_DEPTH);
    assert!(e.is_valid());
    // The tree is walkable to its deepest place and back.
    for _ in 0..1000 {
        e.key(Key::Left, false);
    }
    assert!(e.is_valid());
}

#[test]
fn hostile_key_sequences_never_panic_and_keep_the_caret_valid() {
    let keys = [Key::Left, Key::Right, Key::Up, Key::Down, Key::Home, Key::End, Key::Backspace, Key::Delete];
    let texts = [
        "x", "^", "_", "/", "sqrt", "alpha", "2", "-", "(", " ", "pi", "\u{0}", "é", "\n", ")", "[", "}", "+-", "<=", "->", "sum", "int_", "sin",
        "sinh", "lim", "pm", "*", "arcsin",
    ];
    let ids: Vec<&'static str> = crate::templates().iter().map(|t| t.id).collect();
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..40 {
        let mut e = if round % 5 == 0 { Editor::new(from_linear("(a+b)^2/sqrt(x)")) } else { ed() };
        for step in 0..1500 {
            let r = next();
            match r % 10 {
                0..=3 => e.key(keys[(r >> 8) as usize % keys.len()], (r >> 20) & 1 == 1),
                4..=7 => e.type_str(texts[(r >> 8) as usize % texts.len()]),
                8 => {
                    let id = ids[(r >> 8) as usize % ids.len()];
                    e.insert_template(&tpl(id));
                }
                _ => match (r >> 8) % 4 {
                    0 => e.select_all(),
                    1 => e.set_caret(Caret { path: vec![((r >> 12) as usize % 5, (r >> 16) as usize % 4)], pos: (r >> 24) as usize % 9 }),
                    2 => e.drag_caret(Caret { path: e.caret().path.clone(), pos: (r >> 24) as usize % 9 }),
                    _ => {
                        let _ = e.display_math();
                    }
                },
            }
            assert!(e.is_valid(), "round {round} step {step}: {:?}", e.caret());
            assert!(e.caret().path.len() <= MAX_DEPTH);
        }
        // Whatever was built still prints and parses.
        let _ = crate::to_omml(e.math());
        let _ = to_linear(e.math());
    }
}

#[test]
fn key_names_parse() {
    assert_eq!(Key::parse("Left"), Some(Key::Left));
    assert_eq!(Key::parse(" backspace "), Some(Key::Backspace));
    assert_eq!(Key::parse("del"), Some(Key::Delete));
    assert_eq!(Key::parse("x"), None);
}

fn ty(s: &str) -> String {
    lin(&typed(s))
}

#[test]
fn brackets_pair_up_and_the_closing_one_steps_out() {
    let mut e = typed("(a+b");
    assert_eq!(e.caret().path.len(), 1, "caret is inside the new pair");
    assert!(matches!(e.math().body.first(), Some(Node::Delim { .. })));
    e.type_str(")+c");
    assert!(e.caret().path.is_empty(), "the closing bracket stepped out");
    assert_eq!(e.math().body.len(), 2, "{:?}", e.math().body);
    assert_eq!(ty("(a+b)+c"), "(a+b)+c");
    // A stray closing bracket is just a character.
    assert_eq!(ty("a)"), "a)");
    assert_eq!(ty("[x]"), "[x]");
}

#[test]
fn open_bracket_wraps_the_selection() {
    let mut e = typed("ab");
    e.key(Key::Left, true);
    e.key(Key::Left, true);
    e.type_str("(");
    assert!(matches!(e.math().body.first(), Some(Node::Delim { items, .. }) if units(&items[0]) == 2));
    assert!(e.caret().path.is_empty());
}

#[test]
fn slash_takes_a_parenthesised_group_without_its_parentheses() {
    let e = typed("(a+b)/");
    match e.math().body.first() {
        Some(Node::Frac { num, den }) => {
            assert_eq!(units(num), 3, "a+b, parentheses dropped");
            assert!(den.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(e.caret().path, vec![(0, 1)]);
}

#[test]
fn the_quadratic_formula_types_in_reading_order() {
    let mut e = ed();
    e.type_str("x=(-b+-sqrt(b^2");
    e.key(Key::Right, false);
    e.type_str("-4ac))/2a");
    // x = frac( -b ± sqrt(b^2 - 4ac), 2a )
    assert_eq!(e.math().body.len(), 2, "{:?}", e.math().body);
    let Some(Node::Frac { num, den }) = e.math().body.get(1) else { panic!("{:?}", e.math().body) };
    assert_eq!(units(den), 2, "2a is the denominator");
    let l = to_linear(&Math::new(num.clone())).replace(' ', "");
    assert!(l.contains('\u{B1}') && l.contains('\u{221A}') && l.contains("4ac"), "{l}");
    assert!(!l.starts_with('('), "no redundant parentheses: {l}");
    assert!(matches!(num.first(), Some(Node::Text(t)) if t.starts_with('\u{2212}')));
}

#[test]
fn plus_minus_and_symbol_words_convert() {
    assert_eq!(ty("a+-b"), "a\u{B1}b");
    assert_eq!(ty("pm"), "\u{B1}");
    assert_eq!(ty("bpm"), "bpm", "pm inside a word stays");
    assert_eq!(ty("a<=b"), "a\u{2264}b");
    assert_eq!(ty("a>=b"), "a\u{2265}b");
    assert_eq!(ty("a!=b"), "a\u{2260}b");
    assert_eq!(ty("x->y"), "x\u{2192}y");
    assert_eq!(ty("a*b"), "a\u{B7}b");
    assert_eq!(ty("2times3"), "2\u{D7}3");
}

#[test]
fn big_operators_and_functions_convert_when_typed() {
    let e = typed("sum");
    assert!(matches!(e.math().body.first(), Some(Node::Nary { op: '\u{2211}', sub: Some(_), sup: Some(_), .. })));
    assert_eq!(e.caret().path, vec![(0, 0)], "caret in the lower limit");
    let mut e = typed("int_0");
    e.key(Key::Right, false);
    assert!(matches!(e.math().body.first(), Some(Node::Nary { op: '\u{222B}', sub: Some(_), sup: None, .. })), "{:?}", e.math().body);
    let e = typed("int_0^1");
    assert!(
        matches!(e.math().body.first(), Some(Node::Nary { sub: Some(s), sup: Some(u), .. }) if units(s) == 1 && units(u) == 1),
        "{:?}",
        e.math().body
    );
    let e = typed("sin");
    assert!(matches!(e.math().body.first(), Some(Node::Func { name, .. }) if name == "sin"));
    assert!(matches!(typed("sinh").math().body.first(), Some(Node::Func { name, .. }) if name == "sinh"));
    assert!(matches!(typed("arcsin").math().body.first(), Some(Node::Func { name, .. }) if name == "arcsin"));
    assert!(matches!(typed("lim").math().body.first(), Some(Node::Limit { .. })));
}

#[test]
fn an_operator_ends_a_function_argument() {
    let e = typed("sinx+1");
    assert_eq!(e.math().body.len(), 2, "{:?}", e.math().body);
    assert!(matches!(e.math().body.first(), Some(Node::Func { body, .. }) if units(body) == 1));
    let e = typed("sin(x)+1");
    assert_eq!(e.math().body.len(), 2, "{:?}", e.math().body);
}

#[test]
fn right_after_the_last_matrix_cell_leaves_the_parentheses() {
    let mut e = ed();
    e.insert_template(&tpl("matrix2"));
    e.type_str("1");
    for k in ["2", "3", "4"] {
        e.key(Key::Right, false);
        e.type_str(k);
    }
    e.key(Key::Right, false);
    assert!(e.caret().path.is_empty(), "one Right is enough: {:?}", e.caret());
    e.type_str("+x");
    assert_eq!(e.math().body.len(), 2);
    // And back in: Left lands in the last cell.
    e.key(Key::Left, false);
    e.key(Key::Left, false);
    e.key(Key::Left, false);
    assert_eq!(e.caret().path.len(), 2, "{:?}", e.caret());
}

#[test]
fn plain_fence_characters_in_hostile_order_stay_valid() {
    let mut e = ed();
    for _ in 0..200 {
        e.type_str("([{");
    }
    for _ in 0..200 {
        e.type_str(")]}");
    }
    assert!(e.is_valid());
}

#[test]
fn backspace_after_a_filled_structure_steps_in_then_deletes_inside() {
    for src in ["x+a/b", "x+sqrtab", "x+(ab)"] {
        let mut e = typed(src);
        e.key(Key::Right, false);
        let before = lin(&e);
        e.key(Key::Backspace, false);
        assert_eq!(lin(&e), before, "{src}: first Backspace only enters");
        assert!(!e.caret().path.is_empty(), "{src}");
        e.key(Key::Backspace, false);
        assert_ne!(lin(&e), before, "{src}: second Backspace deletes an atom");
        assert!(lin(&e).starts_with("x+"), "{src}");
    }
}

#[test]
fn undo_and_redo_restore_tree_and_caret() {
    let mut e = typed("x^2");
    e.key(Key::Right, false);
    e.type_str("+1");
    assert!(e.undo());
    assert_eq!(lin(&e), "x^2");
    assert!(e.undo());
    assert!(e.is_blank());
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(lin(&e), "x^2");
    assert!(e.redo());
    assert_eq!(lin(&e), "x^2+1");
    assert!(!e.redo());
    // A wiped fraction comes back.
    let mut e = typed("a/b");
    e.select_all();
    e.key(Key::Backspace, false);
    assert!(e.is_blank());
    e.key(Key::Undo, false);
    assert_eq!(lin(&e).replace(['(', ')'], ""), "a/b");
    // A new edit drops the redo branch.
    e.type_str("c");
    assert!(!e.redo());
}

#[test]
fn cut_and_copy_use_linear_text() {
    let mut e = typed("ab+c");
    assert_eq!(e.selected_linear(), None);
    e.key(Key::Left, true);
    e.key(Key::Left, true);
    assert_eq!(e.selected_linear().as_deref(), Some("+c"));
    assert_eq!(e.cut().as_deref(), Some("+c"));
    assert_eq!(lin(&e), "ab");
    e.key(Key::Undo, false);
    assert_eq!(lin(&e), "ab+c");
}

#[test]
fn tab_walks_the_places_and_never_leaves_the_tree() {
    let mut e = typed("a/b");
    e.key(Key::Up, false);
    e.key(Key::Tab, false);
    assert_eq!(e.caret().path, vec![(0, 1)], "numerator to denominator");
    e.key(Key::Tab, false);
    assert!(e.caret().path.is_empty(), "out of the last place");
    e.key(Key::Tab, true);
    assert_eq!(e.caret().path, vec![(0, 1)], "Shift+Tab goes back");
    let mut e = typed("x^2");
    e.key(Key::Tab, false);
    e.type_str("+1");
    assert_eq!(lin(&e), "x^2+1", "Tab out of the exponent keeps typing in the tree");
    let mut e = typed("[1,2;3,4]");
    for _ in 0..50 {
        e.key(Key::Tab, false);
        e.key(Key::Tab, true);
        assert!(e.is_valid());
    }
}

#[test]
fn parentheses_around_a_denominator_are_dropped_like_the_numerator() {
    let e = typed("(x+1)/(x-1)");
    let Some(Node::Frac { num, den }) = e.math().body.first() else { panic!("{:?}", e.math().body) };
    assert!(!matches!(num.first(), Some(Node::Delim { .. })));
    assert!(!matches!(den.first(), Some(Node::Delim { .. })), "{den:?}");
    // Parentheses inside a longer denominator stay.
    let e = typed("a/(b)c");
    assert!(matches!(e.math().body.first(), Some(Node::Frac { .. })));
}

#[test]
fn hostile_keys_with_undo_never_panic() {
    let keys = [Key::Left, Key::Right, Key::Up, Key::Down, Key::Home, Key::End, Key::Backspace, Key::Delete, Key::Tab, Key::Undo, Key::Redo];
    let mut seed = 12345u64;
    let mut e = ed();
    for _ in 0..3000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let r = (seed >> 33) as usize;
        match r % 4 {
            0 => e.type_str(["x", "^", "_", "/", "(", ")", "sqrt", "alpha", "+", "-"][r / 4 % 10]),
            _ => e.key(keys[r / 4 % keys.len()], r.is_multiple_of(7)),
        }
        assert!(e.is_valid());
    }
}

#[test]
fn operators_leave_a_filled_exponent_or_subscript() {
    assert_eq!(lin(&typed("x^2+y^2=z^2")), "x^2+y^2=z^2");
    assert_eq!(lin(&typed("b^2-4ac")), "b^2−4ac");
    assert_eq!(lin(&typed("a_1+a_2")), "a_1+a_2");
    // An empty exponent keeps a leading minus: x^-1.
    assert_eq!(lin(&typed("x^-1")), "x^−1");
    // Limits of a big operator are not scripts: `=` stays in the limit.
    assert_eq!(typed("sumi=1").caret().path, vec![(0, 0)], "the lower limit keeps the =");
}

#[test]
fn typed_latex_commands_become_symbols_and_structures() {
    assert_eq!(ty("\\alpha"), "α");
    assert_eq!(ty("\\alpha\\beta"), "αβ");
    assert_eq!(ty("2\\pi r"), "2πr");
    assert_eq!(ty("a\\leq b"), "a≤b");
    assert_eq!(ty("a\\in b"), "a∈b");
    assert_eq!(ty("\\int_0^1"), ty("int_0^1"));
    assert!(matches!(typed("\\int").math().body.first(), Some(Node::Nary { op: '\u{222B}', .. })), "int is not eaten by in");
    assert!(matches!(typed("\\sqrt").math().body.first(), Some(Node::Rad { .. })));
    assert!(matches!(typed("\\sin").math().body.first(), Some(Node::Func { name, .. }) if name == "sin") || typed("\\sin ").math().body.len() == 1);
    assert!(matches!(typed("\\sin ").math().body.first(), Some(Node::Func { name, .. }) if name == "sin"));
    assert!(matches!(typed("\\sinh").math().body.first(), Some(Node::Func { name, .. }) if name == "sinh"));
    // An unknown command stays as typed.
    assert!(matches!(typed("\\foo").math().body.first(), Some(Node::Text(t)) if t == "\\foo"));
    assert_eq!(ty("\\left(x\\right)"), "(x)");
}

#[test]
fn braces_after_a_latex_command_are_its_arguments() {
    assert_eq!(ty("\\frac{a+b}{c}"), ty("(a+b)/c"));
    assert_eq!(ty("\\frac{1}{2}+x"), "1/2+x");
    assert_eq!(ty("\\sqrt{x+1}"), "√(x+1)");
    assert_eq!(ty("x^{2}+1"), "x^2+1");
    assert_eq!(ty("x_{ab}+1"), "x_(ab)+1");
    assert_eq!(ty("\\sum_{i=1}^{n}i"), ty("sum_i=1^n i"));
    assert_eq!(ty("e^{-x^{2}}"), ty("e^-x^2"));
    // A real brace pair is still a brace pair.
    assert!(matches!(typed("{a}").math().body.first(), Some(Node::Delim { open, .. }) if open == "{"));
    // Unmatched braces never panic or leave the caret invalid.
    for s in ["}", "\\frac}}", "\\frac{{{", "x^{", "\\sqrt}{"] {
        assert!(typed(s).is_valid(), "{s}");
    }
}

#[test]
fn a_limit_subscript_does_not_nest_an_empty_script() {
    for s in ["lim_x", "\\lim_x", "\\lim_{x\\to0}"] {
        let e = typed(s);
        let Some(Node::Limit { lim, .. }) = e.math().body.first() else { panic!("{s}: {:?}", e.math().body) };
        assert!(!lim.iter().any(|n| matches!(n.bare(), Node::Script { .. })), "{s}: stray script in {lim:?}");
        assert!(!to_linear(e.math()).contains("()_"), "{s}: {}", to_linear(e.math()));
    }
    assert_eq!(lin(&typed("\\lim_{x\\to0}")), lin(&typed("lim_x→0")));
    assert!(matches!(typed("\\lim_{x\\to0}").math().body.first(), Some(Node::Limit { lim, .. }) if units(lim) == 3));
    // The same for an operator whose limit places already exist.
    let e = typed("sum_i");
    assert!(matches!(e.math().body.first(), Some(Node::Nary { sub: Some(s), .. }) if units(s) == 1));
}

#[test]
fn select_between_covers_whole_structures_across_rows() {
    let mut e = typed("a+x/y");
    // From the start of the top row to inside the denominator: everything up to the fraction.
    let start = Caret { path: vec![], pos: 0 };
    let in_den = e.caret().clone();
    assert_eq!(in_den.path.len(), 1);
    e.select_between(&start, &in_den);
    assert!(e.caret().path.is_empty());
    assert_eq!(e.selection(), Some((0, units(&e.math().body))));
    // Backwards, from the denominator to the start.
    e.select_between(&in_den, &start);
    assert_eq!(e.selection(), Some((0, units(&e.math().body))));
    // Within one row it is a plain range.
    let mut e = typed("abcd");
    e.select_between(&Caret { path: vec![], pos: 1 }, &Caret { path: vec![], pos: 3 });
    assert_eq!(e.selection(), Some((1, 3)));
    // Stale carets never panic.
    e.select_between(&Caret { path: vec![(9, 9)], pos: 9 }, &Caret { path: vec![(8, 1), (1, 1)], pos: 99 });
    assert!(e.is_valid());
}

#[test]
fn typed_latex_root_degree() {
    let e = typed("\\sqrt[3]{x}");
    assert!(matches!(e.math().body.first(), Some(Node::Rad { deg: Some(d), body }) if units(d) == 1 && units(body) == 1), "{:?}", e.math().body);
    assert_eq!(ty("\\sqrt[3]{x}+1"), ty("\\sqrt[3]{x}") + "+1");
    // Without a command in front a `[` is a bracket as before, also inside a radical.
    let e = typed("sqrt[");
    assert!(matches!(e.math().body.first(), Some(Node::Rad { deg: None, .. })));
    // A stray `]` never panics.
    let e = typed("\\sqrt[]]");
    assert!(e.is_valid());
}

#[test]
fn parentheses_closing_a_radicand_or_script_are_not_kept() {
    let strip = |s: &str| ty(s).replace(' ', "");
    assert_eq!(strip("sqrt(b^2-4ac)"), "√(b^2−4ac)");
    let e = typed("sqrt(b^2-4ac)");
    assert!(matches!(e.math().body.first(), Some(Node::Rad { body, .. }) if !body.iter().any(|n| matches!(n.bare(), Node::Delim { .. }))));
    let e = typed("sum_(i=1)^n");
    assert!(
        matches!(e.math().body.first(), Some(Node::Nary { sub: Some(s), .. }) if !s.iter().any(|n| matches!(n.bare(), Node::Delim { .. }))),
        "{:?}",
        e.math().body
    );
    let e = typed("x^(n+1)");
    assert!(matches!(e.math().body.first(), Some(Node::Script { sup: Some(s), .. }) if !s.iter().any(|n| matches!(n.bare(), Node::Delim { .. }))));
    // Nested groups keep the inner ones, and the quadratic formula still closes properly.
    let e = typed("x=(-b+-sqrt(b^2-4ac))/2a");
    assert!(matches!(e.math().body.last(), Some(Node::Frac { .. })));
}

#[test]
fn backspace_after_an_exponent_deletes_its_last_character() {
    let mut e = typed("z^3");
    e.key(Key::Right, false);
    assert!(e.caret().path.is_empty());
    e.key(Key::Backspace, false);
    assert_eq!(lin(&e), "z^()");
    e.type_str("2");
    assert_eq!(lin(&e), "z^2");
}

#[test]
fn a_space_ends_an_upper_limit_and_inf_becomes_infinity() {
    let e = typed("sum_(i=1)^n i^2=1/2");
    let Some(Node::Nary { sup: Some(sup), body, .. }) = e.math().body.first().map(Node::bare) else { panic!("{:?}", e.math().body) };
    assert_eq!(units(sup), 1, "the upper limit is just n");
    assert!(units(body) >= 2, "the summand follows: {body:?}");
    // The braced form ends the last limit at its closing brace.
    let e = typed("\\sum_{i=1}^{n}i");
    assert!(matches!(e.math().body.first().map(Node::bare), Some(Node::Nary { sup: Some(s), .. }) if units(s) == 1));
    // `inf` converts once the word is over, and `infty` still works.
    let mut e = typed("x->inf");
    assert_eq!(lin(&e).chars().last(), Some('f'), "still being typed");
    e.flush();
    assert_eq!(lin(&e).chars().last(), Some('\u{221E}'));
    assert_eq!(lin(&typed("inf+1")), "\u{221E}+1");
    assert_eq!(lin(&typed("\\infty")), "\u{221E}");
    assert_eq!(lin(&typed("infty")), "\u{221E}");
    assert_eq!(lin(&typed("info")), "info");
    let mut e = typed("inf");
    e.key(Key::Backspace, false);
    assert_eq!(lin(&e), "in", "Backspace edits the word instead of converting it");
}

// --- Ported from DeckCraft 7972cf5 (crates/math/src/edit/tests.rs). ---
//
// All three failed against WordCraft dbfe1b1's edit model (`mv^2/2` gave `mv^(2/2)`, `e^ipi`
// stayed `e^(ipi)`, `x^n+1` gave `x^n` then `+1`, `x^(n+1)` left the caret in the exponent).
// Decision for each: DeckCraft's behaviour wins (MathLive-style exponent rules), ported into
// edit.rs with every WordCraft test still passing. Details in DIVERGENCE.md.

/// Decision: DeckCraft wins. A digits-only exponent ends at the next non-digit (`mv^2/2` is
/// `(mv^2)/2`, `x^2sin x` is `x^2·sin x`); multi-digit exponents keep going.
#[test]
fn a_single_digit_exponent_ends_at_the_next_term() {
    let e = typed("mv^2/2");
    assert!(matches!(e.math().body.first(), Some(Node::Frac { .. })), "{}", lin(&e));
    assert_eq!(e.caret().path.len(), 1, "caret in the denominator");
    let e = typed("x^2sin x");
    assert_eq!(e.math().body.len(), 2, "{}", lin(&e));
    assert!(matches!(e.math().body.get(1), Some(Node::Func { .. })));
    let e = typed("2^10");
    assert_eq!(lin(&e), "2^10");
    assert_eq!(e.caret().path.len(), 1, "multi-digit exponent keeps going");
}

/// Decision: DeckCraft wins. A letter exponent keeps operators (`x^n+1` is `x^(n+1)`), and the
/// three-letter run `ipi` converts its `pi` (`e^ipi` is `e^(iπ)`).
#[test]
fn letters_keep_going_in_an_exponent() {
    let e = typed("e^ipi");
    assert_eq!(lin(&e), "e^(iπ)");
    assert_eq!(e.caret().path.len(), 1);
    let e = typed("x^n+1");
    assert_eq!(lin(&e), "x^(n+1)");
    assert_eq!(e.caret().path.len(), 1, "operators stay in a letter exponent");
    let e = typed("2^n-1");
    assert_eq!(lin(&e), "2^(n−1)");
}

/// Decision: DeckCraft wins. WordCraft already dropped the parentheses; closing the group now
/// also takes the caret out of the exponent.
#[test]
fn a_parenthesised_exponent_is_the_group_itself() {
    let e = typed("x^(n+1)");
    let Some(Node::Script { sup: Some(s), .. }) = e.math().body.first() else { panic!("no script") };
    assert!(s.iter().all(|n| !matches!(n, Node::Delim { .. })), "no doubled fence");
    assert!(e.caret().path.is_empty(), "closing the group ends the exponent");
    assert_eq!(lin(&e), "x^(n+1)");
}

#[test]
fn typed_angle_floor_and_ceiling_commands_become_brackets() {
    let mut e = Editor::new(Math::default());
    e.type_str(r"\langle u,v\rangle+\lfloor x\rfloor+\lceil y\rceil");
    assert_eq!(crate::to_linear(e.math()).replace(' ', ""), "⟨u,v⟩+⌊x⌋+⌈y⌉");
}

#[test]
fn latex_round_trips_through_the_editor() {
    let mut e = Editor::new(Math::default());
    e.set_latex(r"\frac{a}{b}+\sqrt{x}");
    assert_eq!(e.latex(), r"\frac{a}{b}+\sqrt{x}");
    assert!(!e.undo(), "loading a value leaves no undo step");
}

#[test]
fn caret_edge_queries() {
    let mut e = Editor::new(crate::from_linear("x+1"));
    assert!(e.at_end() && !e.at_start());
    e.key(Key::Home, false);
    assert!(e.at_start() && !e.at_end());
    let mut f = Editor::new(crate::from_linear(r"\frac{a}{b}"));
    f.key(Key::Home, false);
    f.key(Key::Right, false);
    assert!(!f.at_start() && !f.at_end(), "inside the numerator is neither edge");
}

fn typed_linear(src: &str) -> String {
    to_linear(typed(src).math())
}

#[test]
fn closing_a_parenthesis_ends_the_radical_function_or_denominator() {
    assert_eq!(typed_linear("sqrt(x)+1"), "√(x)+1");
    assert_eq!(typed_linear("sqrt(x+1)*2"), "√(x+1)·2");
    assert_eq!(typed_linear("sin(x)+1"), "sin(x)+1");
    assert_eq!(typed_linear("1/(x-1)+2"), "1/(x−1)+2");
    // Without parentheses the structure keeps taking what is typed, as before.
    assert_eq!(typed_linear("sqrt x+1"), "√(x+1)");
}

#[test]
fn typed_call_names_become_function_nodes_at_the_parenthesis() {
    for name in ["floor", "nCr", "mean", "abs", "stdev", "rref"] {
        let e = typed(&format!("{name}(x"));
        assert!(matches!(e.math().body.first(), Some(Node::Func { name: n, .. }) if n == name), "{name}: {:?}", e.math().body);
        assert_eq!(e.caret().path.len(), 1, "{name}: the caret is in the argument");
    }
    // Bare-argument names convert as whole words, like sin.
    assert!(matches!(typed("sech").math().body.first(), Some(Node::Func { name, .. }) if name == "sech"));
    assert!(matches!(typed("arctanh").math().body.first(), Some(Node::Func { name, .. }) if name == "arctanh"));
    assert!(matches!(typed("arccot").math().body.first(), Some(Node::Func { name, .. }) if name == "arccot"));
    // A call-only name typed without a parenthesis is just letters; a plain "(" still nests.
    assert!(!matches!(typed("floor").math().body.first(), Some(Node::Func { .. })));
    assert!(matches!(typed("x(").math().body.get(1), Some(Node::Delim { .. })));
}

#[test]
fn choose_templates_have_a_place_where_n_and_r_go() {
    for (id, letter) in [("nCr", "C"), ("nPr", "P")] {
        let Some(t) = template(id) else { panic!("{id}") };
        let mut e = Editor::new(Math::default());
        e.insert_template(&t.math);
        e.type_str("5");
        e.key(Key::Tab, false);
        e.type_str("2");
        assert_eq!(e.latex(), format!("{{}}_5{letter}_2"), "{id}");
    }
}

#[test]
fn lcm_template_has_a_place_for_each_argument() {
    let Some(t) = template("lcm") else { panic!("lcm") };
    let mut e = Editor::new(Math::default());
    e.insert_template(&t.math);
    e.type_str("4");
    e.key(Key::Tab, false);
    e.type_str("6");
    assert_eq!(e.latex(), "\\operatorname{lcm}\\left(4,6\\right)");
}
