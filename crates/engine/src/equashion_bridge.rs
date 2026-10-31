//! The presentation's equations and the equaSHion editor's tree, through OMML.
//!
//! DeckCraft keeps an equation as an inline `Math` run: its OMML is the equation (kept as
//! PowerPoint wrote it and written back to PPTX unchanged), and the run text is its linear form.
//! The equation editor widget, the linear / LaTeX parser and the MathML reader work on
//! [`equashion_math::Math`]. The two meet in OMML: a run's OMML is read with equaSHion's OMML
//! import ([`to_editor`]) and the editor's tree is stored with its OMML export
//! ([`run_payload`]), so an equation crosses over exactly as it would through a saved file and
//! nothing here duplicates either converter. It lives in the engine (not ui-egui) because the
//! commands and paste need it too.

pub use equashion_math as eq;

/// A stored equation (a `Math` run's OMML) as the editor's tree. OMML import never fails:
/// unknown elements and properties are kept, so writing the tree back loses nothing.
pub fn to_editor(omml: &str) -> eq::Math {
    eq::from_omml(omml)
}

/// The editor's tree as OMML (`m:oMathPara` when it is a display equation), with `xmlns:m`.
pub fn editor_omml(m: &eq::Math) -> String {
    eq::to_omml(m)
}

/// What a `Math` run stores for the editor's tree: its OMML and the run text. The text is the
/// linear form of that OMML read back (so it always matches what the stored equation renders),
/// on one line and never empty, so the run keeps a character's width in the text.
pub fn run_payload(m: &eq::Math) -> (String, String) {
    let omml = editor_omml(m);
    let text = eq::run_text(&to_editor(&omml));
    (omml, text)
}

/// Presentation MathML (one `<math>` element) as the editor's tree, display from
/// `display="block"`. `None` when it is not MathML equaSHion reads, or holds nothing.
pub fn from_mathml(xml: &str) -> Option<eq::Math> {
    eq::from_mathml(xml).ok().filter(|m| !is_blank(m))
}

/// LaTeX (or equaSHion's typed linear form) as the editor's tree, read by equaSHion's parser
/// (`\frac{a}{b}`, `\sqrt{x}`, `\sum_{i=1}^{n}`, `x^2`). `None` when nothing is left.
pub fn from_latex(src: &str, display: bool) -> Option<eq::Math> {
    let src = src.trim();
    if src.is_empty() {
        return None;
    }
    let mut m = eq::from_linear(src);
    m.para = display;
    (!is_blank(&m)).then_some(m)
}

/// An OMML `m:oMath` / `m:oMathPara` as the editor's tree. `None` when nothing in it reads as
/// an equation (OMML import never fails, so an empty tree means it was not one).
pub fn from_omml(xml: &str) -> Option<eq::Math> {
    Some(to_editor(xml)).filter(|m| !is_blank(m))
}

/// The tree holds nothing visible.
fn is_blank(m: &eq::Math) -> bool {
    m.is_empty() || eq::to_linear(m).trim().is_empty()
}

/// Largest OMML or MathML clipboard text read as an equation, bytes.
pub const MAX_CLIP_MARKUP: usize = 1 << 20;
/// Largest delimited LaTeX clipboard text read as an equation, bytes.
pub const MAX_CLIP_LATEX: usize = 1 << 15;

/// The format of an equation on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipFormat {
    Omml,
    MathMl,
    Latex,
}

impl ClipFormat {
    pub fn name(self) -> &'static str {
        match self {
            ClipFormat::Omml => "OMML",
            ClipFormat::MathMl => "MathML",
            ClipFormat::Latex => "LaTeX",
        }
    }
}

/// Recognise an equation in pasted plain text. The whole text (trimmed) must be exactly one OMML
/// `m:oMath` / `m:oMathPara` element, one MathML `<math>` element, or LaTeX between explicit
/// delimiters: `$$..$$` or `\[..\]` (display), `$..$` or `\(..\)` (inline). `None` when it is not
/// such a payload (paste it as text); otherwise its format and the equation (its `para` flag
/// set for a display equation), or why it could not be read.
pub fn clipboard_equation(text: &str) -> Option<(ClipFormat, Result<eq::Math, String>)> {
    let t = text.trim();
    if let Some((qname, local)) = root_element(t) {
        let format = match local {
            "oMath" | "oMathPara" => ClipFormat::Omml,
            "math" => ClipFormat::MathMl,
            _ => return None,
        };
        if !whole_element(t, qname) {
            return None;
        }
        if t.len() > MAX_CLIP_MARKUP {
            return Some((format, Err(format!("the {} is too large", format.name()))));
        }
        let read = match format {
            ClipFormat::Omml => from_omml(t),
            _ => from_mathml(t),
        };
        return Some((format, read.ok_or_else(|| format!("not a {} equation DeckCraft can read", format.name()))));
    }
    let (inner, display) = latex_inner(t)?;
    if inner.len() > MAX_CLIP_LATEX {
        return Some((ClipFormat::Latex, Err("the LaTeX is too long".into())));
    }
    Some((ClipFormat::Latex, from_latex(inner, display).ok_or_else(|| "the LaTeX equation is empty".to_string())))
}

/// The qualified and local name of the first element, after an XML declaration, processing
/// instructions and comments.
fn root_element(t: &str) -> Option<(&str, &str)> {
    let mut rest = t;
    loop {
        let (open, close) = if rest.starts_with("<?") {
            ("<?", "?>")
        } else if rest.starts_with("<!--") {
            ("<!--", "-->")
        } else {
            break;
        };
        let body = rest.get(open.len()..)?;
        rest = body.get(body.find(close)? + close.len()..)?.trim_start();
    }
    let r = rest.strip_prefix('<')?;
    let len = r.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(r.len());
    let qname = r.get(..len).filter(|n| !n.is_empty())?;
    Some((qname, qname.rsplit(':').next().unwrap_or(qname)))
}

/// The text ends with the root element's end tag and holds no second element of that name at
/// the top: it is that one element.
fn whole_element(t: &str, qname: &str) -> bool {
    let end = format!("</{qname}>");
    match t.find(&end) {
        Some(i) => i + end.len() == t.len(),
        // An empty element (`<m:oMath/>`): its start tag ends the text.
        None => {
            let Some(start) = t.find(&format!("<{qname}")) else { return false };
            let tag_end = t.get(start..).and_then(|r| r.find('>')).map(|g| start + g + 1);
            t.ends_with("/>") && tag_end == Some(t.len())
        }
    }
}

/// The LaTeX between explicit delimiters spanning the whole text, and whether it is display.
/// Single `$` follows the usual Markdown rule (no space just inside either `$`), so prices and
/// other text with dollar signs stay text.
fn latex_inner(t: &str) -> Option<(&str, bool)> {
    let pick = |open: &str, close: &str, display: bool| -> Option<(&str, bool)> {
        let inner = t.strip_prefix(open)?.strip_suffix(close)?;
        if inner.contains(open) || inner.contains(close) || inner.trim().is_empty() {
            return None;
        }
        Some((inner, display))
    };
    if t.starts_with("$$") {
        return pick("$$", "$$", true).filter(|(i, _)| !i.contains('$'));
    }
    pick("\\[", "\\]", true)
        .or_else(|| pick("\\(", "\\)", false))
        .or_else(|| pick("$", "$", false).filter(|(i, _)| !i.starts_with(char::is_whitespace) && !i.ends_with(char::is_whitespace)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS_M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
    const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

    /// The equations the round trips are pinned on: (name, equaSHion linear format).
    const EQUATIONS: &[(&str, &str)] = &[
        ("fraction", "a/b"),
        ("sqrt", "√(x+1)"),
        ("root with degree", "√(3&x)"),
        ("sum with limits", "∑_(i=1)^n(i^2)"),
        ("matrix", "[■(1&0@0&1)]"),
        ("quadratic formula", "x=(-b±√(b^2-4ac))/2a"),
        ("scripts", "x_i^2+e^(iπ)"),
        ("integral", "∫_0^1(x dx)"),
        ("accents", "hat(x)+vec(A)"),
    ];

    #[test]
    fn editor_trees_round_trip_through_the_stored_omml() {
        for (name, lin) in EQUATIONS {
            for display in [false, true] {
                let mut m = eq::from_linear(lin);
                m.para = display;
                let (omml, text) = run_payload(&m);
                assert!(omml.contains("xmlns:m="), "{name}: {omml}");
                assert_eq!(omml.contains("oMathPara"), display, "{name}");
                let back = to_editor(&omml);
                assert_eq!(back.para, display, "{name}");
                assert_eq!(eq::to_linear(&back), eq::to_linear(&m), "{name}");
                assert_eq!(text, eq::run_text(&m), "{name}");
                // Stable: a second pass writes the same OMML.
                assert_eq!(run_payload(&back).0, omml, "{name}");
            }
        }
    }

    #[test]
    fn powerpoint_omml_keeps_its_run_formatting_through_the_editor() {
        // What the PPTX reader stores: inherited namespaces declared on the root, PowerPoint's
        // `a:rPr` inside each math run.
        let omml = format!(
            r#"<m:oMathPara xmlns:m="{NS_M}" xmlns:a="{NS_A}"><m:oMath><m:f><m:num><m:r><a:rPr lang="en-US" sz="2800" b="1"/><m:t>x</m:t></m:r></m:num><m:den><m:r><a:rPr lang="en-US" sz="2800" b="1"/><m:t>2</m:t></m:r></m:den></m:f></m:oMath></m:oMathPara>"#
        );
        let m = to_editor(&omml);
        assert!(m.para);
        assert_eq!(eq::to_linear(&m), "x/2");
        let (out, text) = run_payload(&m);
        assert_eq!(text, "x/2");
        assert!(out.contains("<m:f>") && out.matches(r#"sz="2800""#).count() == 2, "{out}");
        assert_eq!(to_editor(&out), m, "the tree survives a write and a read");
    }

    #[test]
    fn empty_and_hostile_omml_never_panic_and_keep_a_run_width() {
        for x in ["", "<m:oMath/>", "<<<", "<a><b/></a>", "<m:oMath><m:f><m:num>"] {
            let m = to_editor(x);
            let (_, text) = run_payload(&m);
            assert!(!text.is_empty(), "{x:?}");
        }
    }

    #[test]
    fn clipboard_detection_takes_only_whole_payloads() {
        for t in [
            "",
            "hello",
            "costs $5",
            "$5 and $6",
            "between $ 5 and 6 $",
            "a \\frac{1}{2} b",
            "\\alpha",
            "$$",
            "$ $",
            "$a$ and $b$",
            "<b>x</b>",
            "<mathx>y</mathx>",
            "<math><mi>x</mi></math> and more",
            "<m:oMath><m:r><m:t>a</m:t></m:r></m:oMath><m:oMath><m:r><m:t>b</m:t></m:r></m:oMath>",
            "<m:oMath><m:r><m:t>x</m:t></m:r>",
            "<!-- unterminated",
            "<?xml",
        ] {
            assert!(clipboard_equation(t).is_none(), "{t:?}");
        }
        let read = |t: &str| clipboard_equation(t).map(|(f, r)| (f, r.map(|m| (eq::to_linear(&m), m.para))));
        assert_eq!(read("  $x^2$\n"), Some((ClipFormat::Latex, Ok(("x^2".into(), false)))));
        assert_eq!(read("$$\\frac{a}{b}$$"), Some((ClipFormat::Latex, Ok(("a/b".into(), true)))));
        assert_eq!(read("\\[x\\]"), Some((ClipFormat::Latex, Ok(("x".into(), true)))));
        assert_eq!(read("\\(y\\)"), Some((ClipFormat::Latex, Ok(("y".into(), false)))));
        let para = format!("<?xml version=\"1.0\"?>\n<m:oMathPara xmlns:m=\"{NS_M}\"><m:oMath><m:r><m:t>y</m:t></m:r></m:oMath></m:oMathPara>");
        assert_eq!(read(&para), Some((ClipFormat::Omml, Ok(("y".into(), true)))));
        assert_eq!(read("<m:oMath><m:r><m:t>z</m:t></m:r></m:oMath>"), Some((ClipFormat::Omml, Ok(("z".into(), false)))));
        assert_eq!(read("<math display=\"block\"><mi>x</mi></math>"), Some((ClipFormat::MathMl, Ok(("x".into(), true)))));
        // Looks like an equation but cannot be read: the caller pastes text and says why.
        assert!(matches!(read("<m:oMath/>"), Some((ClipFormat::Omml, Err(_)))), "empty");
        assert!(matches!(read("<math></math>"), Some((ClipFormat::MathMl, Err(_)))), "empty");
        let long = format!("${}$", "x".repeat(MAX_CLIP_LATEX + 1));
        assert!(matches!(read(&long), Some((ClipFormat::Latex, Err(_)))));
        // equaSHion's own MathML and OMML copies read back.
        let src = eq::from_linear("\\sqrt{x}");
        let copied = eq::to_mathml_display(&src, true);
        assert!(matches!(clipboard_equation(&copied), Some((ClipFormat::MathMl, Ok(m))) if m.para && eq::to_linear(&m) == eq::to_linear(&src)));
        let copied = eq::to_omml(&src);
        assert!(matches!(clipboard_equation(&copied), Some((ClipFormat::Omml, Ok(m))) if eq::to_linear(&m) == eq::to_linear(&src)));
    }

    #[test]
    fn mathml_and_latex_become_equations() {
        let m = from_mathml(r#"<math display="block"><mfrac><mi>a</mi><mi>b</mi></mfrac></math>"#).unwrap();
        assert!(m.para);
        assert!(matches!(m.body.first().map(eq::Node::bare), Some(eq::Node::Frac { .. })), "{m:?}");
        let m = from_latex(r"\sqrt{x^2+1}", false).unwrap();
        assert!(!m.para);
        assert!(matches!(m.body.first().map(eq::Node::bare), Some(eq::Node::Rad { .. })), "{m:?}");
        assert!(from_mathml("<p>not math</p>").is_none());
        assert!(from_latex("   ", false).is_none());
    }
}
