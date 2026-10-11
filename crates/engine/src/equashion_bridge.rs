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
}
