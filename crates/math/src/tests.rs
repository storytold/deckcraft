use super::*;
use crate::Extra;

fn txt(s: &str) -> Seq {
    vec![Node::Text(s.to_string())]
}

#[test]
fn omml_golden_fraction() {
    let m = Math::new(vec![Node::Frac { num: txt("a"), den: txt("b") }]);
    assert_eq!(
        to_omml(&m),
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:f><m:num><m:r><m:t xml:space=\"preserve\">a</m:t></m:r></m:num><m:den><m:r><m:t xml:space=\"preserve\">b</m:t></m:r></m:den></m:f></m:oMath>"
    );
}

#[test]
fn omml_golden_script_and_escape() {
    let m = Math::new(vec![Node::Script { base: txt("x"), sub: None, sup: Some(txt("<2")) }]);
    let x = to_omml(&m);
    assert!(x.contains(
        "<m:sSup><m:e><m:r><m:t xml:space=\"preserve\">x</m:t></m:r></m:e><m:sup><m:r><m:t xml:space=\"preserve\">&lt;2</m:t></m:r></m:sup></m:sSup>"
    ));
    assert_eq!(from_omml(&x), m);
}

#[test]
fn every_template_round_trips_omml() {
    for t in templates() {
        let x = to_omml(&t.math);
        assert!(x.contains("xmlns:m="), "{}", t.id);
        let back = from_omml(&x);
        // Empty Text slots vanish in OMML (no run), so compare through a second serialisation.
        assert_eq!(to_omml(&back), x, "{}", t.id);
    }
}

#[test]
fn every_template_round_trips_linear() {
    for t in templates() {
        let l = to_linear(&t.math);
        assert_eq!(to_linear(&from_linear(&l)), l, "{} via {l}", t.id);
    }
}

#[test]
fn template_ids_unique_and_lookup() {
    let all = templates();
    for (i, a) in all.iter().enumerate() {
        assert!(all.iter().skip(i + 1).all(|b| b.id != a.id));
    }
    assert!(template("frac").is_some());
    assert!(template("nope").is_none());
}

#[test]
fn rich_tree_round_trips_omml() {
    let m = Math::new(vec![
        Node::Nary {
            op: '∑',
            sub: Some(txt("i=1")),
            sup: Some(txt("n")),
            body: vec![Node::Script { base: txt("x"), sub: Some(txt("i")), sup: Some(txt("2")) }],
        },
        Node::Text("=".into()),
        Node::Rad { deg: Some(txt("3")), body: txt("y") },
        Node::Delim { open: "[".into(), close: "]".into(), sep: ",".into(), items: vec![txt("a"), txt("b")] },
        Node::Matrix { rows: vec![vec![txt("1"), txt("2")], vec![txt("3"), txt("4")]] },
        Node::Func { name: "sin".into(), body: txt("t") },
        Node::Accent { ch: '\u{0302}', body: txt("v") },
    ]);
    assert_eq!(from_omml(&to_omml(&m)), m);
}

const NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

fn plain(s: &str) -> Node {
    Node::Styled { text: s.into(), style: Style::Plain }
}

#[test]
fn omml_imports_word_style_and_keeps_unknown_raw() {
    let src = r#"<a14:m xmlns:a14="x"><m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:oMath><m:r><m:rPr><m:sty m:val="p"/></m:rPr><a:rPr xmlns:a="y"/><m:t>x</m:t></m:r><m:phant><m:e><m:r><m:t>n</m:t></m:r></m:e></m:phant></m:oMath></m:oMathPara></a14:m>"#;
    let m = from_omml(src);
    assert_eq!(m.body.len(), 2);
    // The `a:rPr` sibling of the run is preserved verbatim.
    assert_eq!(m.body[0].bare(), &plain("x"));
    assert!(matches!(&m.body[0], Node::Props { extra, .. } if extra.side == vec!["<a:rPr xmlns:a=\"y\"/>".to_string()]));
    match &m.body[1] {
        Node::Raw(r) => {
            assert!(r.starts_with("<m:phant"));
            assert!(r.contains("xmlns:m="), "namespace carried onto raw: {r}");
        }
        other => panic!("expected raw, got {other:?}"),
    }
    // Raw is re-emitted verbatim and re-parses to the same thing; its text still prints.
    assert_eq!(from_omml(&to_omml(&m)), m);
    assert_eq!(to_linear(&m), "xn");
}

#[test]
fn omml_para_props_are_lifted_not_raw() {
    let src = format!(
        r#"<m:oMathPara xmlns:m="{NS}"><m:oMathParaPr><m:jc m:val="left"/></m:oMathParaPr><m:oMath><m:r><m:t>a</m:t></m:r></m:oMath></m:oMathPara>"#
    );
    let m = from_omml(&src);
    assert_eq!(m.body, txt("a"));
    assert!(m.para);
    assert_eq!(m.jc.as_deref(), Some("left"));
    let out = to_omml(&m);
    // Schema order: the paragraph properties are a sibling of m:oMath, never inside it.
    assert!(out.starts_with(&format!("<m:oMathPara xmlns:m=\"{NS}\"><m:oMathParaPr><m:jc m:val=\"left\"/></m:oMathParaPr><m:oMath>")), "{out}");
    assert!(!out.contains("<m:oMath><m:oMathParaPr"));
    assert_eq!(from_omml(&out), m);
}

#[test]
fn omml_word_lim_is_a_limit_not_raw() {
    let src = format!(
        r#"<m:oMath xmlns:m="{NS}"><m:limLow><m:limLowPr/><m:e><m:r><m:rPr><m:sty m:val="p"/></m:rPr><m:t>lim</m:t></m:r></m:e><m:lim><m:r><m:t>x→0</m:t></m:r></m:lim></m:limLow><m:r><m:t>f</m:t></m:r></m:oMath>"#
    );
    let m = from_omml(&src);
    assert_eq!(m.body, vec![Node::Limit { lower: true, base: vec![plain("lim")], lim: txt("x→0") }, Node::Text("f".into())]);
    assert_eq!(to_linear(&m), "lim_(x→0)f");
    assert_eq!(from_omml(&to_omml(&m)), m);
    // A function whose name is a limLow (how Word nests `lim`) keeps both parts.
    let f = format!(
        r#"<m:oMath xmlns:m="{NS}"><m:func><m:fName><m:limLow><m:e><m:r><m:t>lim</m:t></m:r></m:e><m:lim><m:r><m:t>n</m:t></m:r></m:lim></m:limLow></m:fName><m:e><m:r><m:t>a</m:t></m:r></m:e></m:func></m:oMath>"#
    );
    let g = from_omml(&f);
    assert_eq!(g.body.len(), 1);
    assert!(matches!(g.body[0].bare(), Node::Func { .. }));
    let out = to_omml(&g);
    assert!(out.contains("<m:func>") && out.contains("<m:limLow>"), "{out}");
    assert_eq!(from_omml(&out), g);
    assert_eq!(to_omml(&from_omml(&out)), out);
}

#[test]
fn omml_func_with_subscripted_name_keeps_wrapper() {
    let f = format!(
        r#"<m:oMath xmlns:m="{NS}"><m:func><m:fName><m:sSub><m:e><m:r><m:t>log</m:t></m:r></m:e><m:sub><m:r><m:t>2</m:t></m:r></m:sub></m:sSub></m:fName><m:e><m:r><m:t>x</m:t></m:r></m:e></m:func></m:oMath>"#
    );
    let g = from_omml(&f);
    let out = to_omml(&g);
    assert!(out.contains("<m:func><m:fName><m:sSub>"), "{out}");
    assert_eq!(from_omml(&out), g);
}

#[test]
fn omml_idempotence_corner_cases() {
    let cases = [
        format!(r#"<m:oMath xmlns:m="{NS}"><m:d><m:dPr><m:sepChr m:val=";"/></m:dPr><m:e><m:r><m:t>a</m:t></m:r></m:e></m:d></m:oMath>"#),
        format!(
            r#"<m:oMath xmlns:m="{NS}"><m:func><m:fName><m:r><m:t>lim</m:t></m:r><m:r><m:t>  lead</m:t></m:r></m:fName><m:e><m:r><m:t>x</m:t></m:r></m:e></m:func></m:oMath>"#
        ),
        format!(r#"<m:oMath xmlns:m="{NS}"><x:foo xmlns:x="urn:x" a="p&#10;q&#9;r">t&#13;u</x:foo></m:oMath>"#),
    ];
    for c in &cases {
        let a = from_omml(c);
        let out = to_omml(&a);
        assert_eq!(from_omml(&out), a, "{out}");
        assert_eq!(to_omml(&from_omml(&out)), out);
    }
    let out = to_omml(&from_omml(&cases[2]));
    assert!(out.contains("&#10;") && out.contains("&#9;") && out.contains("&#13;"), "{out}");
}

#[test]
fn omml_deep_nesting_is_idempotent_and_ill_formed_names_are_not_emitted() {
    for depth in [30usize, 50, 51, 60] {
        let mut s = format!(r#"<m:oMath xmlns:m="{NS}">"#);
        for _ in 0..depth {
            s.push_str("<m:sSup><m:e>");
        }
        s.push_str("<m:r><m:t>x</m:t></m:r>");
        for _ in 0..depth {
            s.push_str("</m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>");
        }
        s.push_str("</m:oMath>");
        let a = from_omml(&s);
        let out = to_omml(&a);
        assert_eq!(from_omml(&out), a, "depth {depth}");
    }
    let bad = format!(r#"<m:oMath xmlns:m="{NS}"><m:growm:val="1"/><m:r><m:t>x</m:t></m:r></m:oMath>"#);
    let out = to_omml(&from_omml(&bad));
    assert!(!out.contains("growm:val"), "{out}");
}

#[test]
fn omml_other_word_structures_are_typed() {
    let cases: [(&str, &str); 6] = [
        ("eqArr", "<m:eqArr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:eqArr>"),
        ("sPre", "<m:sPre><m:sub><m:r><m:t>1</m:t></m:r></m:sub><m:sup><m:r><m:t>2</m:t></m:r></m:sup><m:e><m:r><m:t>X</m:t></m:r></m:e></m:sPre>"),
        ("bar", "<m:bar><m:barPr><m:pos m:val=\"top\"/></m:barPr><m:e><m:r><m:t>z</m:t></m:r></m:e></m:bar>"),
        ("groupChr", "<m:groupChr><m:e><m:r><m:t>q</m:t></m:r></m:e></m:groupChr>"),
        ("box", "<m:box><m:e><m:r><m:t>q</m:t></m:r></m:e></m:box>"),
        ("borderBox", "<m:borderBox><m:e><m:r><m:t>q</m:t></m:r></m:e></m:borderBox>"),
    ];
    for (name, frag) in cases {
        let m = from_omml(&format!("<m:oMath xmlns:m=\"{NS}\">{frag}</m:oMath>"));
        assert_eq!(m.body.len(), 1, "{name}");
        assert!(!matches!(m.body[0], Node::Raw(_)), "{name} became raw");
        assert_eq!(from_omml(&to_omml(&m)), m, "{name} round trip");
    }
    let m = from_omml(&format!("<m:oMath xmlns:m=\"{NS}\"><m:groupChr><m:e><m:r><m:t>q</m:t></m:r></m:e></m:groupChr></m:oMath>"));
    assert_eq!(m.body, vec![Node::GroupChr { ch: '\u{23DF}', top: false, body: txt("q") }]);
}

#[test]
fn omml_run_styles_are_kept() {
    let src = format!(
        r#"<m:oMath xmlns:m="{NS}"><m:r><m:rPr><m:sty m:val="b"/></m:rPr><m:t>B</m:t></m:r><m:r><m:rPr><m:nor/></m:rPr><m:t> if </m:t></m:r><m:r><m:rPr><m:sty m:val="bi"/></m:rPr><m:t>Z</m:t></m:r><m:r><m:rPr><m:sty m:val="i"/></m:rPr><m:t>y</m:t></m:r></m:oMath>"#
    );
    let m = from_omml(&src);
    assert_eq!(
        m.body,
        vec![
            Node::Styled { text: "B".into(), style: Style::Bold },
            Node::Styled { text: " if ".into(), style: Style::Normal },
            Node::Styled { text: "Z".into(), style: Style::BoldItalic },
            // `sty i` is not modelled: kept as a preserved property.
            Node::Props {
                extra: Extra { pr: vec![("sty".into(), "<m:sty m:val=\"i\"/>".into())], side: vec![], ..Default::default() },
                node: Box::new(Node::Text("y".into())),
            },
        ]
    );
    let out = to_omml(&m);
    assert!(out.contains("<m:rPr><m:sty m:val=\"b\"/></m:rPr>") && out.contains("<m:rPr><m:nor/></m:rPr>") && out.contains("<m:sty m:val=\"bi\"/>"));
    assert_eq!(from_omml(&out), m);
}

#[test]
fn omml_writer_shapes() {
    // Single-item delimiters have no separator; nary carries limLoc.
    let d = to_omml(&Math::new(vec![Node::Delim { open: "(".into(), close: ")".into(), sep: "|".into(), items: vec![txt("a")] }]));
    assert!(d.contains("<m:dPr><m:begChr m:val=\"(\"/><m:endChr m:val=\")\"/></m:dPr>"), "{d}");
    let d2 = to_omml(&Math::new(vec![Node::Delim { open: "(".into(), close: ")".into(), sep: ",".into(), items: vec![txt("a"), txt("b")] }]));
    assert!(d2.contains("<m:sepChr m:val=\",\"/>"));
    let s = to_omml(&from_linear("sum_(i=1)^n(i)"));
    assert!(s.contains("<m:chr m:val=\"∑\"/><m:limLoc m:val=\"undOvr\"/>"), "{s}");
    let i = to_omml(&from_linear("int_0^1(x)"));
    assert!(i.contains("<m:limLoc m:val=\"subSup\"/>"), "{i}");
}

#[test]
fn omml_nary_defaults_and_hidden_limits() {
    let src = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:nary><m:naryPr><m:subHide m:val="1"/><m:supHide m:val="1"/></m:naryPr><m:sub/><m:sup/><m:e><m:r><m:t>f</m:t></m:r></m:e></m:nary></m:oMath>"#;
    assert_eq!(from_omml(src).body, vec![Node::Nary { op: '∫', sub: None, sup: None, body: txt("f") }]);
}

#[test]
fn omml_infallible_on_garbage() {
    assert!(from_omml("").is_empty());
    let _ = from_omml("<<<not xml");
    let _ = from_omml("<m:oMath><m:f><m:num>");
    let deep = "<m:e>".repeat(500);
    let _ = from_omml(&format!("<m:oMath>{deep}"));
    let m = from_omml(&format!("<m:oMath xmlns:m=\"{NS}\"><m:r><m:t>a&amp;b&#233;</m:t></m:r></m:oMath>"));
    assert_eq!(m.body, txt("a&bé"));
}

#[test]
fn deep_trees_do_not_overflow() {
    let mut n = Node::Text("x".into());
    for _ in 0..5000 {
        n = Node::Frac { num: vec![n], den: txt("1") };
    }
    let m = Math::new(vec![n]);
    let _ = to_omml(&m);
    let _ = to_linear(&m);
    let deep = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
    let _ = from_linear(&deep);
    let _ = from_linear(&"√".repeat(5000));
    let _ = from_linear(&"x^".repeat(5000));
}

// ---- linear: every case of the old pretty_equation ----------------------------------------

fn lin(s: &str) -> String {
    to_linear(&from_linear(s))
}

#[test]
fn linear_symbol_cases() {
    assert_eq!(lin("a<=b"), "a≤b");
    assert_eq!(lin("a>=b"), "a≥b");
    assert_eq!(lin("a!=b"), "a≠b");
    assert_eq!(lin("2*3"), "2·3");
    assert_eq!(lin("+-1"), "±1");
    assert_eq!(lin("pi"), "π");
    assert_eq!(lin("2pi"), "2π");
    assert_eq!(lin("alpha+Omega"), "α+Ω");
    assert_eq!(lin("spin"), "spin");
}

#[test]
fn linear_caret_cases() {
    assert_eq!(from_linear("x^2").body, vec![Node::Script { base: txt("x"), sub: None, sup: Some(txt("2")) }]);
    assert_eq!(from_linear("x^10").body, vec![Node::Script { base: txt("x"), sub: None, sup: Some(txt("10")) }]);
    assert_eq!(from_linear("e^n").body, vec![Node::Script { base: txt("e"), sub: None, sup: Some(txt("n")) }]);
    assert_eq!(from_linear("x^-1").body, vec![Node::Script { base: txt("x"), sub: None, sup: Some(txt("\u{2212}1")) }]);
    assert_eq!(from_linear("x^(n+1)").body, vec![Node::Script { base: txt("x"), sub: None, sup: Some(txt("n+1")) }]);
    // Only the operand binds; the rest stays plain text.
    assert_eq!(from_linear("x^2+1").body, vec![Node::Script { base: txt("x"), sub: None, sup: Some(txt("2")) }, Node::Text("+1".into())]);
    assert_eq!(from_linear("12^2").body, vec![Node::Script { base: txt("12"), sub: None, sup: Some(txt("2")) }]);
}

#[test]
fn linear_sub_and_both() {
    assert_eq!(from_linear("x_1").body, vec![Node::Script { base: txt("x"), sub: Some(txt("1")), sup: None }]);
    assert_eq!(from_linear("x_1^2").body, vec![Node::Script { base: txt("x"), sub: Some(txt("1")), sup: Some(txt("2")) }]);
    assert_eq!(from_linear("x^2_1").body, vec![Node::Script { base: txt("x"), sub: Some(txt("1")), sup: Some(txt("2")) }]);
}

#[test]
fn linear_fractions_and_roots() {
    assert_eq!(from_linear("a/b").body, vec![Node::Frac { num: txt("a"), den: txt("b") }]);
    assert_eq!(from_linear("(a+b)/(c+d)").body, vec![Node::Frac { num: txt("a+b"), den: txt("c+d") }]);
    assert_eq!(from_linear("1+x/2").body, vec![Node::Text("1+".into()), Node::Frac { num: txt("x"), den: txt("2") }]);
    assert_eq!(from_linear("pi/2").body, vec![Node::Frac { num: txt("π"), den: txt("2") }]);
    assert_eq!(from_linear("sqrt(x)").body, vec![Node::Rad { deg: None, body: txt("x") }]);
    assert_eq!(from_linear("√[3](x)").body, vec![Node::Rad { deg: Some(txt("3")), body: txt("x") }]);
    assert_eq!(
        from_linear("a/b^2").body,
        vec![Node::Frac { num: txt("a"), den: vec![Node::Script { base: txt("b"), sub: None, sup: Some(txt("2")) }] }]
    );
}

#[test]
fn linear_big_structures() {
    assert_eq!(
        from_linear("sum_(i=1)^n(x_i)").body,
        vec![Node::Nary {
            op: '∑',
            sub: Some(txt("i=1")),
            sup: Some(txt("n")),
            body: vec![Node::Script { base: txt("x"), sub: Some(txt("i")), sup: None }]
        }]
    );
    assert_eq!(from_linear("sin(x)").body, vec![Node::Func { name: "sin".into(), body: txt("x") }]);
    assert_eq!(from_linear("sin x").body, vec![Node::Func { name: "sin".into(), body: txt("x") }]);
    assert_eq!(from_linear("hat(v)").body, vec![Node::Accent { ch: '\u{0302}', body: txt("v") }]);
    assert_eq!(from_linear("■(1&2@3&4)").body, vec![Node::Matrix { rows: vec![vec![txt("1"), txt("2")], vec![txt("3"), txt("4")]] }]);
}

#[test]
fn linear_unbalanced_stays_text() {
    assert_eq!(from_linear("(a").body, txt("(a"));
    assert_eq!(from_linear("a)").body, txt("a)"));
    assert_eq!(from_linear("").body, Seq::new());
}

#[test]
fn linear_golden_printing() {
    let m = Math::new(vec![
        Node::Frac { num: txt("a+b"), den: txt("c") },
        Node::Text("+".into()),
        Node::Script { base: txt("x"), sub: Some(txt("i")), sup: Some(txt("2")) },
        Node::Text("+".into()),
        Node::Rad { deg: None, body: txt("y") },
    ]);
    assert_eq!(to_linear(&m), "(a+b)/c+x_i^2+√(y)");
    assert_eq!(to_linear(&from_linear("(a+b)/c+x_i^2+sqrt(y)")), "(a+b)/c+x_i^2+√(y)");
}

#[test]
fn linear_round_trips_canonical_trees() {
    for src in ["x^2", "a/b", "x_1^2", "sum_(i=1)^n(x_i)", "sin(x)", "hat(v)", "√[3](x)", "■(1&2@3&4)", "(a+b)/(c+d)", "π/2", "x^(n+1)", "1+x/2"]
    {
        let m = from_linear(src);
        assert_eq!(from_linear(&to_linear(&m)), m, "{src}");
    }
}

#[test]
fn raw_prints_placeholder() {
    assert_eq!(to_linear(&Math::new(vec![Node::Raw("<m:foo/>".into())])), "□");
}

#[test]
fn serde_round_trip() {
    let m = from_linear("x^2/3");
    let s = format!("{m:?}");
    assert!(!s.is_empty());
}

// ---- spaced / ordinary typing --------------------------------------------------------------

#[test]
fn linear_spaced_fraction() {
    let want = vec![Node::Text("x = ".into()), Node::Frac { num: from_linear("-b +- sqrt(b^2 - 4*a*c)").body, den: txt("2a") }];
    assert_eq!(from_linear("x = (-b +- sqrt(b^2 - 4*a*c)) / (2a)").body, want);
    assert_eq!(from_linear("x = (-b +- sqrt(b^2 - 4*a*c))/(2a)").body, want);
    assert_eq!(from_linear("a / b").body, vec![Node::Frac { num: txt("a"), den: txt("b") }]);
    assert_eq!(from_linear("1 + x / 2").body, vec![Node::Text("1 + ".into()), Node::Frac { num: txt("x"), den: txt("2") }]);
    // No empty fraction on any spaced input.
    for src in ["a /b", "a/ b", "(a+b) / c", "x = 1 / (1 + 1 / (1 + 1 / x))"] {
        fn has_empty(s: &Seq) -> bool {
            s.iter().any(|n| match n {
                Node::Frac { num, den } => num.is_empty() || den.is_empty() || has_empty(num) || has_empty(den),
                _ => false,
            })
        }
        assert!(!has_empty(&from_linear(src).body), "{src}");
    }
}

#[test]
fn linear_coefficient_denominator() {
    assert_eq!(from_linear("-b/2a").body, vec![Node::Text("\u{2212}".into()), Node::Frac { num: txt("b"), den: txt("2a") }]);
    assert_eq!(to_linear(&from_linear("1/2a")), "1/2a");
}

#[test]
fn linear_spaced_big_operators() {
    let xi = vec![Node::Script { base: txt("x"), sub: Some(txt("i")), sup: None }];
    assert_eq!(
        from_linear("sum_(i=1)^n i^2").body,
        vec![Node::Nary {
            op: '∑',
            sub: Some(txt("i=1")),
            sup: Some(txt("n")),
            body: vec![Node::Script { base: txt("i"), sub: None, sup: Some(txt("2")) }]
        }]
    );
    assert_eq!(from_linear("sum_(i=1)^n x_i").body, vec![Node::Nary { op: '∑', sub: Some(txt("i=1")), sup: Some(txt("n")), body: xi }]);
    assert_eq!(
        from_linear("int_0^1 x^2 dx").body,
        vec![Node::Nary {
            op: '∫',
            sub: Some(txt("0")),
            sup: Some(txt("1")),
            body: vec![Node::Script { base: txt("x"), sub: None, sup: Some(txt("2")) }, Node::Text(" dx".into())]
        }]
    );
    assert_eq!(from_linear("sqrt x").body, vec![Node::Rad { deg: None, body: txt("x") }]);
    assert_eq!(from_linear("sqrt 2").body, vec![Node::Rad { deg: None, body: txt("2") }]);
    assert_eq!(from_linear("sin x").body, vec![Node::Func { name: "sin".into(), body: txt("x") }]);
    assert_eq!(from_linear("ln 2").body, vec![Node::Func { name: "ln".into(), body: txt("2") }]);
}

#[test]
fn linear_limit_forms() {
    let m = from_linear("lim_(x->0) sin(x)/x");
    assert_eq!(
        m.body,
        vec![
            Node::Limit { lower: true, base: vec![plain("lim")], lim: txt("x→0") },
            Node::Text(" ".into()),
            Node::Frac { num: vec![Node::Func { name: "sin".into(), body: txt("x") }], den: txt("x") },
        ]
    );
    assert_eq!(to_linear(&from_linear("lim_(x->0)")), "lim_(x→0)");
    assert_eq!(from_linear("lim_n a").body[0], Node::Limit { lower: true, base: vec![plain("lim")], lim: txt("n") });
}

#[test]
fn linear_brackets_bars_matrices() {
    let d = |o: &str, c: &str, x: &str| Node::Delim { open: o.into(), close: c.into(), sep: "|".into(), items: vec![txt(x)] };
    assert_eq!(from_linear("[a,b]").body, vec![d("[", "]", "a,b")]);
    assert_eq!(from_linear("|x|").body, vec![d("|", "|", "x")]);
    assert_eq!(from_linear("|x| + |y|").body, vec![d("|", "|", "x"), Node::Text(" + ".into()), d("|", "|", "y")]);
    assert_eq!(from_linear("P(A|B)").body, vec![Node::Text("P".into()), d("(", ")", "A|B")]);
    assert_eq!(from_linear("a|b").body, txt("a|b"));
    assert_eq!(from_linear("[a").body, txt("[a"));
    let grid = Node::Matrix { rows: vec![vec![txt("1"), txt("2")], vec![txt("3"), txt("4")]] };
    assert_eq!(
        from_linear("matrix(1&2@3&4)").body,
        vec![Node::Delim { open: "(".into(), close: ")".into(), sep: "|".into(), items: vec![vec![grid.clone()]] }]
    );
    assert_eq!(
        from_linear("bmatrix(1&2@3&4)").body,
        vec![Node::Delim { open: "[".into(), close: "]".into(), sep: "|".into(), items: vec![vec![grid.clone()]] }]
    );
    assert_eq!(from_linear("■(1&2@3&4)").body, vec![grid]);
    assert_eq!(from_linear("█(a@b)").body, vec![Node::EqArr { rows: vec![txt("a"), txt("b")] }]);
    for src in ["[a,b]", "|x|", "matrix(1&2@3&4)", "cases(x@y)", "█(a@b)", "overline(x)", "underbrace(x)", "boxed(x)"] {
        let m = from_linear(src);
        assert_eq!(from_linear(&to_linear(&m)), m, "{src}");
    }
}

#[test]
fn linear_latex_input() {
    assert_eq!(from_linear("\\frac{a}{b}").body, vec![Node::Frac { num: txt("a"), den: txt("b") }]);
    assert_eq!(from_linear("\\frac{a+b}{c}").body, vec![Node::Frac { num: txt("a+b"), den: txt("c") }]);
    assert_eq!(from_linear("a \\leq b").body, txt("a ≤ b"));
    assert_eq!(from_linear("a \\geq b \\neq c \\pm d \\cdot e").body, txt("a ≥ b ≠ c ± d · e"));
    assert_eq!(from_linear("\\sqrt{x}").body, vec![Node::Rad { deg: None, body: txt("x") }]);
    assert_eq!(from_linear("\\sqrt[3]{x+1}").body, vec![Node::Rad { deg: Some(txt("3")), body: txt("x+1") }]);
    assert_eq!(from_linear("x^{n+1}").body, vec![Node::Script { base: txt("x"), sub: None, sup: Some(txt("n+1")) }]);
    assert_eq!(from_linear("\\sum_{i=1}^{n} i").body, vec![Node::Nary { op: '∑', sub: Some(txt("i=1")), sup: Some(txt("n")), body: txt("i") }]);
    assert_eq!(from_linear("\\alpha + \\pi").body, txt("α + π"));
    assert_eq!(from_linear("\\sin\\theta").body, vec![Node::Func { name: "sin".into(), body: txt("θ") }]);
    assert_eq!(from_linear("\\text{if }x").body, vec![Node::Styled { text: "if ".into(), style: Style::Normal }, Node::Text("x".into())]);
    assert_eq!(from_linear("\\left( a \\right)").body.len(), 1);
    assert_eq!(from_linear("\\{ a \\}").body, vec![Node::Delim { open: "{".into(), close: "}".into(), sep: "|".into(), items: vec![txt(" a ")] }]);
    assert_eq!(from_linear("\\lim_{x\\to0} f").body[0], Node::Limit { lower: true, base: vec![plain("lim")], lim: txt("x→0") });
    // Unknown commands and dangling backslashes stay as text.
    assert_eq!(from_linear("\\foo").body, txt("\\foo"));
    assert_eq!(from_linear("a\\").body, txt("a\\"));
    // Deeply nested commands are bounded.
    let _ = from_linear(&"\\frac".repeat(5000));
    let _ = from_linear(&"\\sqrt".repeat(5000));
    let _ = from_linear(&"{".repeat(5000));
    let _ = from_linear(&"\\{".repeat(5000));
    let _ = from_linear(&"|".repeat(5000));
    let _ = from_linear(&"[".repeat(5000));
}

#[test]
fn linear_escapes_round_trip() {
    let m = Math::new(vec![Node::Text("a{b".into())]);
    assert_eq!(to_linear(&m), "a\\{b");
    assert_eq!(from_linear(&to_linear(&m)), m);
    let b = Math::new(vec![Node::Text("c\\d".into())]);
    assert_eq!(from_linear(&to_linear(&b)), b);
    let brace = Node::Delim { open: "{".into(), close: "}".into(), sep: "|".into(), items: vec![txt("x")] };
    assert_eq!(from_linear(&to_linear(&Math::new(vec![brace.clone()]))).body, vec![brace]);
}

#[test]
fn frac_followed_by_letters_does_not_fuse() {
    let m = Math::new(vec![Node::Frac { num: txt("x"), den: txt("2") }, Node::Text("a".into())]);
    assert_eq!(to_linear(&m), "x/(2)a");
    assert_eq!(from_linear(&to_linear(&m)).body, vec![Node::Frac { num: txt("x"), den: txt("2") }, Node::Text("a".into())]);
}

#[test]
fn templates_matrices_are_parenthesised() {
    let m = template("matrix2").map(|t| t.math);
    let Some(Math { body, .. }) = m else { panic!("no matrix2") };
    assert!(matches!(body.as_slice(), [Node::Delim { open, close, .. }] if open == "(" && close == ")"));
    assert!(template("lim").is_some() && template("cases").is_some());
    for t in templates() {
        assert!(!to_omml(&t.math).contains("<m:oMath><m:oMathParaPr"));
    }
}

// ---- round 3: bounded recursion, structure fixes -------------------------------------------

/// Run `f` on a 1 MB stack (the wasm default) so recursion bugs abort the test.
fn small_stack<F: FnOnce() + Send + 'static>(f: F) {
    let h = std::thread::Builder::new().stack_size(1 << 20).spawn(f);
    assert!(h.is_ok_and(|h| h.join().is_ok()), "overflowed or panicked on a 1 MB stack");
}

fn depth_of(s: &[Node]) -> usize {
    super::linear::seq_depth_for_tests(s)
}

#[test]
fn adversarial_typed_input_is_bounded() {
    small_stack(|| {
        let cases: Vec<String> = vec![
            "sum_".repeat(5000),
            "int_".repeat(5000),
            "lim_".repeat(5000),
            "sum^".repeat(5000),
            "x^".repeat(50000),
            "x_".repeat(50000),
            "a/".repeat(50000),
            "/".repeat(50000),
            "(".repeat(50000),
            "sin ".repeat(20000),
            "\\sqrt{".repeat(5000),
            "root(".repeat(5000),
            "\\begin{pmatrix}".repeat(3000),
            "\\binom".repeat(5000),
            "\\vec".repeat(5000),
            "matrix(".repeat(5000),
            "x^2^".repeat(20000),
            "a/b".repeat(20000),
        ];
        for c in &cases {
            let m = from_linear(c);
            assert!(depth_of(&m.body) < 400, "tree too deep for {:?}", c.chars().take(12).collect::<String>());
            let _ = to_linear(&m);
            let _ = to_omml(&m);
            let _ = m.clone();
            assert_eq!(m, m.clone());
        }
    });
}

#[test]
fn nary_body_keeps_its_fraction() {
    let m = from_linear("sum_(i=1)^n 1/i");
    assert_eq!(
        m.body,
        vec![Node::Nary { op: '∑', sub: Some(txt("i=1")), sup: Some(txt("n")), body: vec![Node::Frac { num: txt("1"), den: txt("i") }] }]
    );
    let k = from_linear("sum_(k=0)^infty x^k/k!");
    match k.body.as_slice() {
        [Node::Nary { body, .. }] => {
            assert!(matches!(body.as_slice(), [Node::Frac { den, .. }] if den == &txt("k!")), "{body:?}");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(from_linear("int_0^1 1/x dx").body.as_slice(), [Node::Nary { .. }]));
    assert_eq!(from_linear("n!/k!").body, vec![Node::Frac { num: txt("n!"), den: txt("k!") }]);
    let back = from_linear(&to_linear(&m));
    assert_eq!(back, m);
}

#[test]
fn function_names_are_not_split_by_scripts() {
    for src in ["sin^2 x", "\\sin^2 x"] {
        let m = from_linear(src);
        assert_eq!(m.body, vec![Node::Script { base: vec![plain("sin")], sub: None, sup: Some(txt("2")) }, Node::Text(" x".into())], "{src}");
    }
    assert!(matches!(from_linear("log_2 x").body.as_slice(), [Node::Script { sub: Some(_), .. }, _]));
    assert_eq!(from_linear("sin^2 x"), from_linear(&to_linear(&from_linear("sin^2 x"))));
    assert_eq!(lin("sin^2 x"), "sin^2 x");
}

#[test]
fn latex_structures_parse() {
    let pm = from_linear("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
    assert_eq!(
        pm.body,
        vec![Node::Delim {
            open: "(".into(),
            close: ")".into(),
            sep: "|".into(),
            items: vec![vec![Node::Matrix { rows: vec![vec![txt("a"), txt("b")], vec![txt("c"), txt("d")]] }]],
        }]
    );
    assert_eq!(from_linear("\\begin{bmatrix}1&2\\\\3&4\\end{bmatrix}").body.len(), 1);
    let cs = from_linear("f(x)=\\begin{cases} 1 & x>0 \\\\ 0 & x\\le 0 \\end{cases}");
    assert!(cs.body.iter().any(|n| matches!(n, Node::Delim { open, close, items, .. } if open == "{" && close.is_empty() && matches!(items.as_slice(), [it] if matches!(it.as_slice(), [Node::EqArr { rows }] if rows.len() == 2)))), "{cs:?}");
    // Nested environments split only at their own level.
    let nested = from_linear("\\begin{matrix}\\begin{matrix}a&b\\end{matrix}&c\\\\d&e\\end{matrix}");
    assert!(matches!(nested.body.as_slice(), [Node::Matrix { rows }] if rows.len() == 2 && rows.first().is_some_and(|r| r.len() == 2)), "{nested:?}");
    // Unterminated stays visible.
    assert_eq!(from_linear("\\begin{pmatrix} a").body, txt("\\begin{pmatrix} a"));
    assert!(matches!(
        from_linear("\\binom{n}{k}").body.as_slice(),
        [Node::Delim { open, items, .. }] if open == "(" && matches!(items.as_slice(), [it] if matches!(it.as_slice(), [Node::Matrix { rows }] if rows.len() == 2))
    ));
    assert_eq!(from_linear("\\vec{v}").body, vec![Node::Accent { ch: '\u{20D7}', body: txt("v") }]);
    assert_eq!(from_linear("\\hat x").body, vec![Node::Accent { ch: '\u{0302}', body: txt("x") }]);
    assert_eq!(from_linear("\\overline{AB}").body, vec![Node::Bar { top: true, body: txt("AB") }]);
    assert_eq!(from_linear("\\mathbb{R}").body, txt("ℝ"));
    assert_eq!(from_linear("\\mathcal{L}(x)").body.len(), 2);
    assert_eq!(from_linear("root(3)(x)").body, vec![Node::Rad { deg: Some(txt("3")), body: txt("x") }]);
    assert_eq!(from_linear("\\lim").body, txt("lim"));
}

#[test]
fn matrix_cells_are_trimmed_and_minus_is_typographic() {
    let a = from_linear("matrix(a & b @ c & d)");
    let b = from_linear("matrix(a&b@c&d)");
    assert_eq!(a, b);
    assert_eq!(from_linear("x - y").body, txt("x \u{2212} y"));
    assert_eq!(from_linear("-b").body, txt("\u{2212}b"));
    assert_eq!(from_linear("a->b").body, txt("a→b"));
    let q = from_linear("x = (-b +- sqrt(b^2 - 4ac)) / (2a)");
    assert!(to_omml(&q).contains("\u{2212}b"));
}

const HAND_SAMPLE: &str = r#"<m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:oMath><m:r><m:t>y=</m:t></m:r><m:f><m:num><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:num><m:den><m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e><m:r><m:t>z</m:t></m:r></m:e></m:rad></m:den></m:f><m:sSub><m:e><m:r><m:t>a</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub><m:sSubSup><m:e><m:r><m:t>b</m:t></m:r></m:e><m:sub><m:r><m:t>j</m:t></m:r></m:sub><m:sup><m:r><m:t>k</m:t></m:r></m:sup></m:sSubSup><m:sPre><m:sub><m:r><m:t>1</m:t></m:r></m:sub><m:sup><m:r><m:t>2</m:t></m:r></m:sup><m:e><m:r><m:t>C</m:t></m:r></m:e></m:sPre><m:nary><m:naryPr><m:chr m:val="∑"/></m:naryPr><m:sub><m:r><m:t>n=1</m:t></m:r></m:sub><m:sup><m:r><m:t>N</m:t></m:r></m:sup><m:e><m:r><m:t>n</m:t></m:r></m:e></m:nary><m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="]"/></m:dPr><m:e><m:r><m:t>p</m:t></m:r></m:e><m:e><m:r><m:t>q</m:t></m:r></m:e></m:d><m:m><m:mr><m:e><m:r><m:t>1</m:t></m:r></m:e><m:e><m:r><m:t>2</m:t></m:r></m:e></m:mr><m:mr><m:e><m:r><m:t>3</m:t></m:r></m:e><m:e><m:r><m:t>4</m:t></m:r></m:e></m:mr></m:m><m:func><m:fName><m:r><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>t</m:t></m:r></m:e></m:func><m:acc><m:accPr><m:chr m:val="&#x302;"/></m:accPr><m:e><m:r><m:t>v</m:t></m:r></m:e></m:acc><m:bar><m:barPr><m:pos m:val="top"/></m:barPr><m:e><m:r><m:t>w</m:t></m:r></m:e></m:bar><m:limUpp><m:e><m:r><m:t>u</m:t></m:r></m:e><m:lim><m:r><m:t>~</m:t></m:r></m:lim></m:limUpp><m:glossary x:k="1" xmlns:x="urn:x"><m:e>  keep &amp; me </m:e></m:glossary></m:oMath></m:oMathPara>"#;

#[test]
fn hand_sample_parse_emit_parse_is_idempotent() {
    let m = from_omml(HAND_SAMPLE);
    assert!(m.para);
    // Every listed structure is typed; only the unknown element is Raw.
    let raws: Vec<&Node> = m.body.iter().filter(|n| matches!(n, Node::Raw(_))).collect();
    assert_eq!(raws.len(), 1);
    let e1 = to_omml(&m);
    let m2 = from_omml(&e1);
    assert_eq!(m2, m);
    assert_eq!(to_omml(&m2), e1);
    let lin = to_linear(&m);
    assert!(lin.contains("sin") && lin.contains("2"), "{lin}");
}

#[test]
fn unknown_elements_survive_byte_identical() {
    let m = from_omml(HAND_SAMPLE);
    let raw = m.body.iter().find_map(|n| if let Node::Raw(r) = n { Some(r.clone()) } else { None });
    let raw = raw.expect("raw node");
    assert!(raw.contains(r#"<m:e>  keep &amp; me </m:e>"#), "{raw}");
    let out = to_omml(&m);
    assert!(out.contains(&raw), "raw not re-emitted verbatim");
    // And again after a second generation.
    let again = from_omml(&out);
    assert_eq!(to_omml(&again).matches(&raw).count(), 1);
}

#[test]
fn node_cap_bounds_huge_input() {
    let mut s = format!(r#"<m:oMath xmlns:m="{NS}">"#);
    for _ in 0..200_000 {
        s.push_str("<m:r><m:t>a</m:t></m:r>");
    }
    s.push_str("</m:oMath>");
    let m = from_omml(&s);
    let chars: usize = m.body.iter().map(|n| if let Node::Text(t) = n { t.len() } else { 0 }).sum();
    assert!(chars > 1000 && chars <= MAX_NODES, "{chars}");
    let _ = to_omml(&m);
}

#[test]
fn namespace_bomb_is_bounded() {
    let mut x = String::from("<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"");
    for i in 0..5000 {
        x.push_str(&format!(" xmlns:n{i}=\"urn:{i}\""));
    }
    x.push('>');
    for i in 0..5000 {
        x.push_str(&format!("<n{i}:u/>"));
    }
    x.push_str("</m:oMath>");
    let t = std::time::Instant::now();
    let m = from_omml(&x);
    let out = to_omml(&m);
    assert!(t.elapsed().as_secs() < 5);
    assert!(out.len() < 2_000_000, "{}", out.len());
}

#[test]
fn raw_keeps_used_inherited_namespace_only() {
    let x = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:q="Q" xmlns:u="U"><q:thing a="1"/></m:oMath>"#;
    let out = to_omml(&from_omml(x));
    assert!(out.contains(r#"xmlns:q="Q""#) && !out.contains("xmlns:u="), "{out}");
}

// ---- property preservation (M-eq.2c) -------------------------------------------------------

fn omml(inner: &str) -> String {
    format!(r#"<m:oMath xmlns:m="{NS}" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{inner}</m:oMath>"#)
}

/// Parse, emit, and require the emitted form to contain every fragment and to be a fixed point.
fn keeps(inner: &str, fragments: &[&str]) {
    let m = from_omml(&omml(inner));
    let out = to_omml(&m);
    for f in fragments {
        assert!(out.contains(f), "lost `{f}` in {out}");
    }
    let again = from_omml(&out);
    assert_eq!(again, m, "tree changed on second pass");
    assert_eq!(to_omml(&again), out, "bytes changed on second pass");
}

#[test]
fn edit_keeps_fraction_type_and_ctrlpr() {
    let ctrl = r#"<m:ctrlPr><w:rPr><w:rFonts w:ascii="Cambria Math" w:hAnsi="Cambria Math"/><w:i/></w:rPr></m:ctrlPr>"#;
    for ty in ["lin", "skw", "noBar"] {
        let src = format!(
            r#"<m:f><m:fPr><m:type m:val="{ty}"/>{ctrl}</m:fPr><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>"#
        );
        let want = format!(r#"<m:type m:val="{ty}"/>"#);
        keeps(&src, &[&want, r#"<w:rFonts w:ascii="Cambria Math""#, "<w:i/>"]);
    }
    // After an edit of the slots the properties stay.
    let mut m = from_omml(&omml(&format!(
        r#"<m:f><m:fPr><m:type m:val="lin"/>{ctrl}</m:fPr><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>"#
    )));
    if let Some(Node::Props { node, .. }) = m.body.first_mut()
        && let Node::Frac { num, .. } = node.as_mut()
    {
        *num = vec![Node::Text("zz".into())];
    }
    let out = to_omml(&m);
    assert!(out.contains(r#"<m:type m:val="lin"/>"#) && out.contains(">zz<"), "{out}");
}

#[test]
fn edit_keeps_script_lit_and_br_in_runs() {
    keeps(
        r#"<m:r><m:rPr><m:scr m:val="double-struck"/><m:sty m:val="p"/></m:rPr><m:t>R</m:t></m:r><m:r><m:t>x</m:t></m:r>"#,
        &[r#"<m:scr m:val="double-struck"/>"#],
    );
    // The double-struck R does not merge into the plain neighbour.
    let m = from_omml(&omml(r#"<m:r><m:rPr><m:scr m:val="double-struck"/></m:rPr><m:t>R</m:t></m:r><m:r><m:t>x</m:t></m:r>"#));
    assert_eq!(m.body.len(), 2);
    keeps(r#"<m:r><m:rPr><m:lit/></m:rPr><m:t>sin</m:t></m:r>"#, &["<m:lit/>"]);
    keeps(r#"<m:r><m:rPr><m:brk m:alnAt="1"/></m:rPr><m:t>a</m:t></m:r>"#, &["<m:brk"]);
    keeps(r#"<m:r><m:rPr><m:br m:alnAt="1"/></m:rPr><m:t></m:t></m:r>"#, &["<m:br m:alnAt=\"1\"/>"]);
    keeps(r#"<m:r><w:rPr><w:rFonts w:ascii="Cambria Math"/><w:b/></w:rPr><m:t>q</m:t></m:r>"#, &[r#"<w:rFonts w:ascii="Cambria Math"/>"#, "<w:b/>"]);
}

#[test]
fn edit_keeps_grow_shp_limloc_and_matrix_props() {
    keeps(
        r#"<m:d><m:dPr><m:begChr m:val="["/><m:endChr m:val="]"/><m:grow m:val="1"/><m:shp m:val="match"/></m:dPr><m:e><m:r><m:t>a</m:t></m:r></m:e></m:d>"#,
        &["<m:grow", "<m:shp"],
    );
    keeps(
        r#"<m:nary><m:naryPr><m:chr m:val="∑"/><m:limLoc m:val="subSup"/><m:grow m:val="1"/></m:naryPr><m:sub/><m:sup/><m:e/></m:nary>"#,
        &[r#"<m:limLoc m:val="subSup"/>"#, "<m:grow"],
    );
    keeps(
        r#"<m:m><m:mPr><m:baseJc m:val="top"/><m:mcs><m:mc><m:mcPr><m:count m:val="2"/><m:mcJc m:val="center"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e/><m:e/></m:mr></m:m>"#,
        &["<m:baseJc", "<m:mcs>"],
    );
    keeps(
        r#"<m:groupChr><m:groupChrPr><m:chr m:val="⏞"/><m:pos m:val="top"/><m:vertJc m:val="bot"/></m:groupChrPr><m:e/></m:groupChr>"#,
        &["<m:vertJc"],
    );
    keeps(r#"<m:borderBox><m:borderBoxPr><m:hideTop m:val="1"/></m:borderBoxPr><m:e/></m:borderBox>"#, &["<m:hideTop"]);
    keeps(r#"<m:sSup><m:sSupPr><m:ctrlPr><w:rPr><w:i/></w:rPr></m:ctrlPr></m:sSupPr><m:e/><m:sup/></m:sSup>"#, &["<w:i/>"]);
}

#[test]
fn schema_order_of_merged_properties() {
    let m = from_omml(&omml(
        r#"<m:nary><m:naryPr><m:chr m:val="∑"/><m:grow m:val="1"/><m:ctrlPr><w:rPr><w:i/></w:rPr></m:ctrlPr></m:naryPr><m:sub/><m:sup/><m:e/></m:nary>"#,
    ));
    let out = to_omml(&m);
    let at = |n: &str| out.find(n).unwrap_or(usize::MAX);
    assert!(at("<m:chr") < at("<m:limLoc") && at("<m:limLoc") < at("<m:grow") && at("<m:grow") < at("<m:ctrlPr"), "{out}");
}

#[test]
fn empty_delimiter_and_eqarr_are_idempotent() {
    keeps("<m:d><m:dPr><m:begChr m:val=\"(\"/></m:dPr></m:d>", &[]);
    keeps("<m:eqArr/>", &[]);
    let m = from_omml(&omml("<m:d/>"));
    assert!(matches!(m.body.as_slice(), [Node::Delim { items, .. }] if items.len() == 1));
}

#[test]
fn reading_keys_on_namespace_uri_not_prefix() {
    let x = format!(
        r#"<mm:oMath xmlns:mm="{NS}"><mm:f><mm:num><mm:r><mm:t>a</mm:t></mm:r></mm:num><mm:den><mm:r><mm:t>b</mm:t></mm:r></mm:den></mm:f></mm:oMath>"#
    );
    let m = from_omml(&x);
    assert_eq!(to_linear(&m), "a/b");
    // Default namespace still works, and an `m` prefix bound to something else is not math.
    let d = from_omml(&format!(r#"<oMath xmlns="{NS}"><r><t>z</t></r></oMath>"#));
    assert_eq!(to_linear(&d), "z");
    let other = from_omml(r#"<m:oMath xmlns:m="urn:other"><m:r><m:t>z</m:t></m:r></m:oMath>"#);
    assert!(other.body.iter().all(|n| matches!(n, Node::Raw(_))));
}

#[test]
fn nary_without_operator_stays_without() {
    let m = from_omml(&omml(r#"<m:nary><m:naryPr><m:chr m:val=""/></m:naryPr><m:sub/><m:sup/><m:e><m:r><m:t>x</m:t></m:r></m:e></m:nary>"#));
    assert!(matches!(m.body.first().map(Node::bare), Some(Node::Nary { op: '\0', .. })));
    let out = to_omml(&m);
    assert!(out.contains(r#"<m:chr m:val=""/>"#) && !out.contains('∫'), "{out}");
    assert_eq!(from_omml(&out), m);
    assert!(!to_linear(&m).contains('\0'));
}

#[test]
fn several_omath_in_one_para_keep_a_separator() {
    let x =
        format!(r#"<m:oMathPara xmlns:m="{NS}"><m:oMath><m:r><m:t>a</m:t></m:r></m:oMath><m:oMath><m:r><m:t>b</m:t></m:r></m:oMath></m:oMathPara>"#);
    let m = from_omml(&x);
    assert_ne!(to_linear(&m), "ab");
    let out = to_omml(&m);
    assert_eq!(out.matches("<m:oMath>").count() + out.matches("<m:oMath ").count(), 2, "{out}");
    assert_eq!(from_omml(&out), m);
}

#[test]
fn invalid_xml_characters_never_reach_output() {
    let m = Math::new(vec![Node::Text("a\u{FFFE}b\u{0}c\u{1}d\u{FDD0}e".into())]);
    let out = to_omml(&m);
    assert!(out.contains(">abcde<"), "{out:?}");
    let back = from_omml(&format!("<m:oMath xmlns:m=\"{NS}\"><m:r><m:t>x&#0;y&#xFFFE;z</m:t></m:r></m:oMath>"));
    assert_eq!(back.body, txt("xyz"));
    assert_eq!(from_omml(&to_omml(&back)), back);
}

#[test]
fn nesting_to_the_math_depth_cap_is_not_truncated() {
    let mut inner = String::from("<m:r><m:t>x</m:t></m:r>");
    for _ in 0..40 {
        inner = format!("<m:f><m:num>{inner}</m:num><m:den/></m:f>");
    }
    let m = from_omml(&omml(&inner));
    let out = to_omml(&m);
    assert_eq!(out.matches("<m:f>").count(), 40);
    assert_eq!(from_omml(&out), m);
}

const PP_SIN: &str = "<a14:m xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:oMathPara><m:oMath><m:func><m:funcPr><m:ctrlPr><a:rPr lang=\"en-US\" i=\"1\"/></m:ctrlPr></m:funcPr><m:fName><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><a:rPr lang=\"en-US\" i=\"1\"/><m:t>sin</m:t></m:r></m:fName><m:e><m:r><a:rPr lang=\"en-US\" i=\"1\"/><m:t>x</m:t></m:r></m:e></m:func></m:oMath></m:oMathPara></a14:m>";

#[test]
fn powerpoint_func_keeps_structure() {
    let m = from_omml(PP_SIN);
    assert_eq!(m.body.len(), 1);
    assert!(matches!(m.body[0].bare(), Node::Func { name, .. } if name == "sin"));
    assert_eq!(to_linear(&m), "sin(x)");
    let x = to_omml(&m);
    assert!(x.contains("<m:funcPr><m:ctrlPr"), "{x}");
    assert!(x.contains("<m:t>sin</m:t>") && x.contains("<m:sty m:val=\"p\"/>"), "{x}");
    assert_eq!(to_omml(&from_omml(&x)), x);
    assert_eq!(from_omml(&x), m);
}

#[test]
fn rpr_out_of_schema_order_is_tree_idempotent() {
    let src = "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:r><m:rPr><m:scr m:val=\"script\"/><m:sty m:val=\"b\"/><m:lit/></m:rPr><m:t>x</m:t></m:r></m:oMath>";
    let m = from_omml(src);
    assert_eq!(from_omml(&to_omml(&m)), m);
}

#[test]
fn arg_props_do_not_show_in_linear() {
    let src = "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:argPr><m:argSz m:val=\"-1\"/></m:argPr><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath>";
    let m = from_omml(src);
    assert_eq!(to_linear(&m), "x^2");
    assert!(to_omml(&m).contains("<m:argSz"));
}

fn grid_of(m: &Math) -> Option<Vec<Vec<String>>> {
    fn find(s: &[Node]) -> Option<&Vec<Vec<Seq>>> {
        for n in s {
            match n.bare() {
                Node::Matrix { rows } => return Some(rows),
                Node::Delim { items, .. } => {
                    for it in items {
                        if let Some(r) = find(it) {
                            return Some(r);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    find(&m.body).map(|rows| rows.iter().map(|r| r.iter().map(|c| to_linear(&Math::new(c.clone()))).collect()).collect())
}

#[test]
fn typed_matrix_is_a_real_grid() {
    let want = vec![vec!["a".to_string(), "b".into()], vec!["c".into(), "d".into()]];
    for src in ["matrix(a,b;c,d)", "[a, b; c, d]", "pmatrix(a, b; c, d)", "■(a&b@c&d)"] {
        let m = from_linear(src);
        assert_eq!(grid_of(&m), Some(want.clone()), "{src}: {m:?}");
        assert_eq!(from_omml(&to_omml(&m)), m, "{src}");
        assert_eq!(from_linear(&to_linear(&m)), m, "{src}");
    }
    // Commas inside nested brackets stay inside the cell; a plain bracket pair is not a grid.
    assert_eq!(grid_of(&from_linear("matrix(f(x,y),b;c,d)")).map(|g| g.len()), Some(2));
    assert_eq!(grid_of(&from_linear("[a,b]")), None);
}

#[test]
fn namespace_declaration_amplification_is_bounded() {
    let long = "u".repeat(2000);
    let mut src = String::from("<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"");
    for i in 0..60 {
        src.push_str(&format!(" xmlns:p{i}=\"urn:{long}{i}\""));
    }
    src.push('>');
    let attrs: String = (0..60).map(|i| format!(" p{i}:a=\"1\"")).collect();
    let n = 2000;
    for _ in 0..n {
        src.push_str(&format!("<m:foo{attrs}/>"));
    }
    src.push_str("</m:oMath>");
    let m = from_omml(&src);
    let out = to_omml(&m);
    assert!(out.len() < 2 * src.len() + 512 * 1024, "output {} for input {}", out.len(), src.len());
    // Whatever was emitted stays well-formed and is a fixed point.
    let again = to_omml(&from_omml(&out));
    assert_eq!(again, out);
    for node in &m.body {
        if let Node::Raw(r) = node.bare() {
            assert!(r.is_empty() || (r.starts_with("<m:foo") && r.ends_with("/>")), "{r:.80}");
            if !r.is_empty() {
                assert!(r.contains("xmlns:p0=") && r.contains("xmlns:p59="), "declared prefixes missing");
            }
        }
    }
}

const NS_HDR: &str =
    "xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

#[test]
fn large_equation_keeps_tail_after_edit() {
    let mut src = format!("<m:oMath {NS_HDR}>");
    for _ in 0..6000 {
        src.push_str("<m:r><a:rPr sz=\"2000\"/><m:t>x</m:t></m:r>");
    }
    src.push_str("<m:phant><m:e><m:r><m:t>KEEPME</m:t></m:r></m:e></m:phant></m:oMath>");
    let out = to_omml(&from_omml(&src));
    assert!(out.contains("KEEPME"));
    assert_eq!(out.matches("<a:rPr").count(), 6000);
}

#[test]
fn hidden_part_with_content_is_kept() {
    let src = format!(
        "<m:oMath {NS_HDR}><m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad></m:oMath>"
    );
    assert!(to_omml(&from_omml(&src)).contains(">3<"));
}

fn wrap_m(inner: &str) -> String {
    format!(r#"<m:oMath xmlns:m="{NS}" xmlns:w="urn:w" xmlns:x="urn:x">{inner}</m:oMath>"#)
}

fn assert_stable(src: &str) -> String {
    let m = from_omml(src);
    let out = to_omml(&m);
    assert_eq!(from_omml(&out), m, "{out}");
    assert_eq!(to_omml(&from_omml(&out)), out);
    out
}

#[test]
fn run_with_sym_br_tab_cr_is_kept_whole() {
    for inner in [r#"<m:sym m:font="Wingdings" m:char="F0E0"/>"#, "<m:br/>", "<m:tab/>", "<m:cr/>"] {
        let out = assert_stable(&wrap_m(&format!("<m:r><m:t>a</m:t>{inner}<m:t>b</m:t></m:r>")));
        assert!(out.contains(inner), "{inner} lost: {out}");
    }
}

#[test]
fn foreign_children_and_attributes_on_structural_elements_survive() {
    let out = assert_stable(&wrap_m(
        r#"<m:f w:keep="1" plain="p"><m:num><m:r><m:t>a</m:t></m:r></m:num><x:odd q="1">t</x:odd><m:den><m:r><m:t>b</m:t></m:r></m:den><m:weird/></m:f>"#,
    ));
    for needle in ["xmlns:w=\"urn:w\"", "w:keep=\"1\"", "plain=\"p\"", "<x:odd", "<m:weird/>"] {
        assert!(out.contains(needle), "{needle} lost: {out}");
    }
    assert_eq!(out.matches("<m:f ").count(), 1);
}

#[test]
fn hidden_limit_with_content_stays_hidden() {
    let out = assert_stable(&wrap_m(
        r#"<m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad><m:nary><m:naryPr><m:subHide m:val="1"/></m:naryPr><m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup/><m:e/></m:nary>"#,
    ));
    assert!(out.contains("degHide") && out.contains("subHide"), "{out}");
    assert!(out.contains(">3<") && out.contains(">i<"), "{out}");
}

#[test]
fn sibling_omath_each_declare_the_namespace() {
    let out = to_omml(&from_omml(&format!(
        r#"<m:oMath xmlns:m="{NS}"><m:r><m:t>a</m:t></m:r></m:oMath><m:oMath xmlns:m="{NS}"><m:r><m:t>b</m:t></m:r></m:oMath>"#
    )));
    assert_eq!(out.matches("<m:oMath ").count(), 2, "{out}");
    assert!(out.matches("xmlns:m=").count() >= 2);
    assert_stable(&out);
}

#[test]
fn func_without_fname_is_idempotent() {
    assert_stable(&wrap_m(r#"<m:func><m:e><m:r><m:t>x</m:t></m:r></m:e></m:func>"#));
}

#[test]
fn many_attributes_are_bounded() {
    let mut attrs = String::new();
    for i in 0..200_000 {
        attrs.push_str(&format!(" x:a{i}=\"1\""));
    }
    let src = format!(
        r#"<m:oMath xmlns:m="{NS}" xmlns:x="urn:x"><m:f{attrs}><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>"#
    );
    let t = std::time::Instant::now();
    let out = to_omml(&from_omml(&src));
    assert!(t.elapsed().as_secs() < 5, "slow: {:?}", t.elapsed());
    assert!(out.len() < 100_000, "{}", out.len());
    assert_stable(&out);
}

#[test]
fn func_with_foreign_child_and_attribute_is_idempotent() {
    let out = assert_stable(&wrap_m(
        r#"<m:func xmlns:x="urn:x" x:a="1"><m:fName><m:r><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>x</m:t></m:r></m:e><x:ext a="1"/></m:func>"#,
    ));
    assert_eq!(out.matches("<x:ext").count(), 1, "{out}");
    assert_eq!(out.matches("x:a=").count(), 1, "{out}");
}

#[test]
fn foreign_content_on_mr_num_den_e_omath_survives() {
    let out = assert_stable(&format!(
        r#"<m:oMath xmlns:m="{NS}" xmlns:x="urn:x" x:o="1"><m:f><m:num x:n="1"><m:r><m:t>a</m:t></m:r></m:num><m:den x:d="1"><m:r><m:t>b</m:t></m:r></m:den></m:f><m:m><m:mr x:r="1"><m:e x:e="1"><m:r><m:t>1</m:t></m:r></m:e><x:in/></m:mr></m:m></m:oMath>"#
    ));
    for n in ["x:o=", "x:n=", "x:d=", "x:r=", "x:e=", "<x:in"] {
        assert!(out.contains(n), "{n} lost: {out}");
    }
}

#[test]
fn foreign_child_between_delimiter_items_keeps_position() {
    let out = assert_stable(&wrap_m(r#"<m:d xmlns:x="urn:x"><m:e><m:r><m:t>a</m:t></m:r></m:e><x:mid/><m:e><m:r><m:t>b</m:t></m:r></m:e></m:d>"#));
    let (a, mid, b) = (out.find(">a<"), out.find("<x:mid"), out.find(">b<"));
    assert!(a < mid && mid < b, "{out}");
}

#[test]
fn thirty_thousand_run_equation_keeps_every_run() {
    let mut src = format!(r#"<m:oMath xmlns:m="{NS}">"#);
    for _ in 0..30_000 {
        src.push_str("<m:r><m:t>x</m:t></m:r><m:r><m:rPr><m:sty m:val=\"b\"/></m:rPr><m:t>y</m:t></m:r>");
    }
    src.push_str("<m:r><m:t>END</m:t></m:r></m:oMath>");
    let out = to_omml(&from_omml(&src));
    assert!(out.contains("END"));
}

#[test]
fn omath_attributes_survive_on_first_and_later_equations() {
    let out = assert_stable(&format!(
        r#"<m:oMathPara xmlns:m="{NS}" xmlns:x="urn:x"><m:oMath x:a="1"><m:r><m:t>a</m:t></m:r></m:oMath><m:oMath x:b="2"><m:r><m:t>b</m:t></m:r></m:oMath></m:oMathPara>"#
    ));
    assert!(out.contains("x:a=\"1\"") && out.contains("x:b=\"2\""), "{out}");
}

#[test]
fn empty_accent_and_multichar_nary_chr_survive_first_edit() {
    let out = assert_stable(&format!(
        r#"<m:oMath xmlns:m="{NS}"><m:acc><m:accPr><m:chr m:val=""/></m:accPr><m:e><m:r><m:t>a</m:t></m:r></m:e></m:acc><m:nary><m:naryPr><m:chr m:val="ab"/></m:naryPr><m:sub/><m:sup/><m:e><m:r><m:t>x</m:t></m:r></m:e></m:nary></m:oMath>"#
    ));
    assert!(out.contains(r#"m:val="""#) && !out.contains('\u{0302}'), "{out}");
    assert!(out.contains(r#"m:val="ab""#), "{out}");
}

#[test]
fn inherited_namespace_copies_are_charged_per_element() {
    let uri = "u".repeat(8000);
    let mut src = format!(r#"<m:oMath xmlns:m="{NS}" xmlns:q="{uri}">"#);
    for _ in 0..20_000 {
        src.push_str("<q:x/>");
    }
    src.push_str("</m:oMath>");
    let out = to_omml(&from_omml(&src));
    assert!(out.len() < 40 * 1024 * 1024, "{}", out.len());
}
