//! [`deckcraft_model::math`] tree → Office Math (OMML).

use deckcraft_model::math::{Arg, ColJc, FracKind, LimLoc, MAX_DEPTH, MNode, MRun, MScr, MSty, Math, MathJc, ScriptKind, parse_linear};

use crate::xml::{A, W};

const NS_M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// Write an equation inside `<a14:m>`: `m:oMath` inside `m:oMathPara` (or `m:oMath` when inline).
pub fn write_equation(w: &mut W, linear: &str, display: bool, math: &Math) {
    if display {
        w.open("m:oMathPara", A::new().a("xmlns:m", NS_M));
        let jc = match math.jc {
            MathJc::CenterGroup => None,
            MathJc::Center => Some("center"),
            MathJc::Left => Some("left"),
            MathJc::Right => Some("right"),
        };
        if let Some(jc) = jc {
            w.open0("m:oMathParaPr");
            val(w, "m:jc", jc);
            w.close("m:oMathParaPr");
        }
        w.open0("m:oMath");
    } else {
        w.open("m:oMath", A::new().a("xmlns:m", NS_M));
    }
    let parsed;
    let nodes: &Arg = if math.nodes.is_empty() {
        parsed = parse_linear(linear);
        &parsed
    } else {
        &math.nodes
    };
    arg_inner(w, nodes, 0);
    w.close("m:oMath");
    if display {
        w.close("m:oMathPara");
    }
}

/// Writes a self-closing element with an `m:val` attribute.
fn val(w: &mut W, name: &str, v: &str) {
    w.empty(name, A::new().a("m:val", v));
}

/// Writes a math argument element `name` wrapping the inner nodes.
fn arg(w: &mut W, name: &str, a: &[MNode], depth: usize) {
    if a.is_empty() {
        w.empty0(name);
        return;
    }
    w.open0(name);
    arg_inner(w, a, depth + 1);
    w.close(name);
}

/// Recursively writes a slice of math nodes within recursion depth limit.
fn arg_inner(w: &mut W, a: &[MNode], depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for n in a {
        node(w, n, depth);
    }
}

/// Writes an OMML character property element.
fn char_prop(w: &mut W, name: &str, c: Option<char>) {
    let s = c.map(|c| c.to_string()).unwrap_or_default();
    val(w, name, &s);
}

/// Serializes an `MNode::Matrix` into an `m:m` OMML element.
fn write_matrix(w: &mut W, n: &MNode, depth: usize) {
    let MNode::Matrix { rows, col_jc } = n else { return };
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
    w.open0("m:m");
    w.open0("m:mPr");
    w.open0("m:mcs");
    for c in 0..cols {
        let jc = match col_jc.get(c).copied().unwrap_or_default() {
            ColJc::Center => "center",
            ColJc::Left => "left",
            ColJc::Right => "right",
        };
        w.open0("m:mc");
        w.open0("m:mcPr");
        val(w, "m:count", "1");
        val(w, "m:mcJc", jc);
        w.close("m:mcPr");
        w.close("m:mc");
    }
    w.close("m:mcs");
    w.close("m:mPr");
    for row in rows {
        w.open0("m:mr");
        for c in 0..cols {
            arg(w, "m:e", row.get(c).map(|v| v.as_slice()).unwrap_or(&[]), depth);
        }
        w.close("m:mr");
    }
    w.close("m:m");
}

/// Serializes an `MNode::Nary` into an `m:nary` OMML element.
fn write_nary(w: &mut W, n: &MNode, depth: usize) {
    let MNode::Nary { chr, lim_loc, grow, sub_hide, sup_hide, sub, sup, e } = n else { return };
    w.open0("m:nary");
    w.open0("m:naryPr");
    if *chr != '∫' {
        char_prop(w, "m:chr", Some(*chr));
    }
    match lim_loc {
        Some(LimLoc::UndOvr) => val(w, "m:limLoc", "undOvr"),
        Some(LimLoc::SubSup) => val(w, "m:limLoc", "subSup"),
        None => {}
    }
    if *grow {
        val(w, "m:grow", "1");
    }
    if *sub_hide {
        val(w, "m:subHide", "1");
    }
    if *sup_hide {
        val(w, "m:supHide", "1");
    }
    w.close("m:naryPr");
    arg(w, "m:sub", sub, depth);
    arg(w, "m:sup", sup, depth);
    arg(w, "m:e", e, depth);
    w.close("m:nary");
}

/// Serializes a single `MNode` into its OMML XML element.
#[allow(clippy::cognitive_complexity)]
fn node(w: &mut W, n: &MNode, depth: usize) {
    match n {
        MNode::Run(r) => run(w, r),
        MNode::Frac { kind, num, den } => {
            w.open0("m:f");
            if *kind != FracKind::Bar {
                w.open0("m:fPr");
                match kind {
                    FracKind::Bar => {}
                    FracKind::Skewed => val(w, "m:type", "skw"),
                    FracKind::Linear => val(w, "m:type", "lin"),
                    FracKind::NoBar => val(w, "m:type", "noBar"),
                }
                w.close("m:fPr");
            }
            arg(w, "m:num", num, depth);
            arg(w, "m:den", den, depth);
            w.close("m:f");
        }
        MNode::Script { kind, base, sub, sup } => {
            let el = match kind {
                ScriptKind::Sup => "m:sSup",
                ScriptKind::Sub => "m:sSub",
                ScriptKind::SubSup => "m:sSubSup",
                ScriptKind::Pre => "m:sPre",
            };
            w.open0(el);
            if *kind == ScriptKind::Pre {
                arg(w, "m:sub", sub, depth);
                arg(w, "m:sup", sup, depth);
                arg(w, "m:e", base, depth);
            } else {
                arg(w, "m:e", base, depth);
                if matches!(kind, ScriptKind::Sub | ScriptKind::SubSup) {
                    arg(w, "m:sub", sub, depth);
                }
                if matches!(kind, ScriptKind::Sup | ScriptKind::SubSup) {
                    arg(w, "m:sup", sup, depth);
                }
            }
            w.close(el);
        }
        MNode::Rad { deg, deg_hide, e } => {
            w.open0("m:rad");
            if *deg_hide {
                w.open0("m:radPr");
                val(w, "m:degHide", "1");
                w.close("m:radPr");
            }
            arg(w, "m:deg", deg, depth);
            arg(w, "m:e", e, depth);
            w.close("m:rad");
        }
        MNode::Nary { .. } => write_nary(w, n, depth),
        MNode::Delim { beg, end, sep, grow, shp_match, elems } => {
            w.open0("m:d");
            if *beg != Some('(') || *sep != Some('|') || *end != Some(')') || !grow || *shp_match {
                w.open0("m:dPr");
                if *beg != Some('(') {
                    char_prop(w, "m:begChr", *beg);
                }
                if *sep != Some('|') {
                    char_prop(w, "m:sepChr", *sep);
                }
                if *end != Some(')') {
                    char_prop(w, "m:endChr", *end);
                }
                if !grow {
                    val(w, "m:grow", "0");
                }
                if *shp_match {
                    val(w, "m:shp", "match");
                }
                w.close("m:dPr");
            }
            for e in elems {
                arg(w, "m:e", e, depth);
            }
            w.close("m:d");
        }
        MNode::Func { name, e } => {
            w.open0("m:func");
            arg(w, "m:fName", name, depth);
            arg(w, "m:e", e, depth);
            w.close("m:func");
        }
        MNode::Lim { upper, e, lim } => {
            let el = if *upper { "m:limUpp" } else { "m:limLow" };
            w.open0(el);
            arg(w, "m:e", e, depth);
            arg(w, "m:lim", lim, depth);
            w.close(el);
        }
        MNode::Acc { chr, e } => {
            w.open0("m:acc");
            w.open0("m:accPr");
            char_prop(w, "m:chr", Some(*chr));
            w.close("m:accPr");
            arg(w, "m:e", e, depth);
            w.close("m:acc");
        }
        MNode::Bar { top, e } => {
            w.open0("m:bar");
            w.open0("m:barPr");
            val(w, "m:pos", if *top { "top" } else { "bot" });
            w.close("m:barPr");
            arg(w, "m:e", e, depth);
            w.close("m:bar");
        }
        MNode::BorderBox { hide, strike, e } => {
            w.open0("m:borderBox");
            if hide.iter().any(|x| *x) || strike.iter().any(|x| *x) {
                w.open0("m:borderBoxPr");
                for (on, name) in hide.iter().zip(["m:hideTop", "m:hideBot", "m:hideLeft", "m:hideRight"]) {
                    if *on {
                        val(w, name, "1");
                    }
                }
                for (on, name) in strike.iter().zip(["m:strikeH", "m:strikeV", "m:strikeBLTR", "m:strikeTLBR"]) {
                    if *on {
                        val(w, name, "1");
                    }
                }
                w.close("m:borderBoxPr");
            }
            arg(w, "m:e", e, depth);
            w.close("m:borderBox");
        }
        MNode::Boxed { e } => {
            w.open0("m:box");
            arg(w, "m:e", e, depth);
            w.close("m:box");
        }
        MNode::Phasor { e } => {
            w.open0("m:borderBox");
            w.open0("m:borderBoxPr");
            val(w, "m:hideTop", "1");
            val(w, "m:hideLeft", "1");
            val(w, "m:hideRight", "1");
            val(w, "m:strikeBLTR", "1");
            w.close("m:borderBoxPr");
            arg(w, "m:e", e, depth);
            w.close("m:borderBox");
        }
        MNode::GroupChr { chr, top, e } => {
            w.open0("m:groupChr");
            w.open0("m:groupChrPr");
            char_prop(w, "m:chr", Some(*chr));
            val(w, "m:pos", if *top { "top" } else { "bot" });
            val(w, "m:vertJc", if *top { "bot" } else { "top" });
            w.close("m:groupChrPr");
            arg(w, "m:e", e, depth);
            w.close("m:groupChr");
        }
        MNode::EqArr { rows } => {
            w.open0("m:eqArr");
            if rows.iter().any(|r| deckcraft_model::math::has_number_mark(r)) {
                w.open0("m:eqArrPr");
                val(w, "m:maxDist", "1");
                w.close("m:eqArrPr");
            }
            for r in rows {
                arg(w, "m:e", r, depth);
            }
            w.close("m:eqArr");
        }
        MNode::Matrix { .. } => write_matrix(w, n, depth),
        MNode::Phant { show, zero_wid, zero_asc, zero_desc, e } => {
            w.open0("m:phant");
            w.open0("m:phantPr");
            val(w, "m:show", if *show { "1" } else { "0" });
            for (on, name) in [(*zero_wid, "m:zeroWid"), (*zero_asc, "m:zeroAsc"), (*zero_desc, "m:zeroDesc")] {
                if on {
                    val(w, name, "1");
                }
            }
            w.close("m:phantPr");
            arg(w, "m:e", e, depth);
            w.close("m:phant");
        }
    }
}

/// Serializes an `MRun` into an `m:r` run element.
fn run(w: &mut W, r: &MRun) {
    w.open0("m:r");
    let sty = r.sty.map(|s| match s {
        MSty::Plain => "p",
        MSty::Bold => "b",
        MSty::Italic => "i",
        MSty::BoldItalic => "bi",
    });
    let scr = match r.scr {
        MScr::Roman => None,
        MScr::Script => Some("script"),
        MScr::Fraktur => Some("fraktur"),
        MScr::DoubleStruck => Some("double-struck"),
        MScr::SansSerif => Some("sans-serif"),
        MScr::Monospace => Some("monospace"),
    };
    if sty.is_some() || scr.is_some() || r.nor || r.lit || r.brk.is_some() {
        w.open0("m:rPr");
        if r.lit {
            w.empty0("m:lit");
        }
        if r.nor {
            w.empty0("m:nor");
        }
        if let Some(s) = scr {
            val(w, "m:scr", s);
        }
        if let Some(s) = sty {
            val(w, "m:sty", s);
        }
        match r.brk {
            Some(0) => w.empty0("m:brk"),
            Some(n) => w.empty("m:brk", A::new().a("m:alnAt", n.to_string())),
            None => {}
        }
        w.close("m:rPr");
    }
    if r.size.is_some() || r.color.is_some() || r.font.is_some() {
        let a = A::new().o("sz", r.size.map(|s| (s.clamp(1.0, 4000.0) * 100.0).round() as i64));
        if r.color.is_none() && r.font.is_none() {
            w.empty("a:rPr", a);
        } else {
            w.open("a:rPr", a);
            if let Some(c) = r.color {
                w.open0("a:solidFill");
                w.empty("a:srgbClr", A::new().a("val", c.hex()));
                w.close("a:solidFill");
            }
            if let Some(f) = &r.font {
                w.empty("a:latin", A::new().a("typeface", f));
            }
            w.close("a:rPr");
        }
    }
    w.elt("m:t", &r.text);
    w.close("m:r");
}
