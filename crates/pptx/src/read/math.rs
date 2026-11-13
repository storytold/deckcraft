//! Office Math (OMML, ECMA-376 Part 1 §22.1) → [`deckcraft_model::math`] tree.

use deckcraft_model::math::{Arg, ColJc, FracKind, LimLoc, MAX_DEPTH, MNode, MRun, MScr, MSty, Math, MathJc, Rgb, ScriptKind, merge_runs};

use crate::xml::El;

/// Read a math container (`a14:m`, `m:oMathPara`, or `m:oMath`) into a [`Math`] AST and `display` flag.
pub fn read_math_container(e: &El) -> (Math, bool) {
    if e.is("oMathPara") {
        (read_math_para(e), true)
    } else if e.is("oMath") {
        (read_omath(e, MathJc::CenterGroup), false)
    } else if let Some(mp) = e.child("oMathPara") {
        (read_math_para(mp), true)
    } else if let Some(om) = e.child("oMath") {
        (read_omath(om, MathJc::CenterGroup), false)
    } else {
        (Math::default(), true)
    }
}

/// Parses an `m:oMathPara` paragraph element into a Math model.
pub fn read_math_para(para: &El) -> Math {
    let jc = para_jc(para);
    let mut nodes = Vec::new();
    for om in para.children_named("oMath") {
        nodes.extend(arg(om, 0));
    }
    Math { nodes: merge_runs(nodes), omml: para.to_xml(), jc, linear: false }
}

/// An `m:oMath` element as an equation (structure plus its source XML).
pub fn read_omath(e: &El, jc: MathJc) -> Math {
    Math { nodes: merge_runs(arg(e, 0)), omml: e.to_xml(), jc, linear: false }
}

/// `m:oMathParaPr/m:jc`.
pub fn para_jc(para: &El) -> MathJc {
    match para.child("oMathParaPr").and_then(|p| p.child("jc")).and_then(|j| j.attr("val")) {
        Some("left") => MathJc::Left,
        Some("right") => MathJc::Right,
        Some("center") => MathJc::Center,
        _ => MathJc::CenterGroup,
    }
}

/// OMML on/off property: absent `m:val` = on.
fn on(e: &El) -> bool {
    match e.attr("val") {
        None => true,
        Some(v) => !matches!(v.trim(), "0" | "false" | "off" | "False" | "FALSE"),
    }
}

/// An on/off property `name` in the properties element `pr` (absent = `default`).
fn prop(pr: Option<&El>, name: &str, default: bool) -> bool {
    pr.and_then(|p| p.child(name)).map(on).unwrap_or(default)
}

/// A character property (`m:chr`, `m:begChr`…): `None` when absent, `Some(None)` when empty.
fn chr(pr: Option<&El>, name: &str) -> Option<Option<char>> {
    let c = pr?.child(name)?;
    match c.attr("val") {
        Some(v) => Some(v.chars().next()),
        None => Some(None),
    }
}

/// Reads the `val` attribute from child `name` of properties element `pr`.
fn val<'a>(pr: Option<&'a El>, name: &str) -> Option<&'a str> {
    pr.and_then(|p| p.child(name)).and_then(|c| c.attr("val"))
}

/// The argument held by child `name` of `e`.
fn sub_arg(e: &El, name: &str, depth: usize) -> Arg {
    e.child(name).map(|c| arg(c, depth + 1)).unwrap_or_default()
}

/// Read the math content of an element (`m:oMath`, `m:e`, `m:num`…).
fn arg(e: &El, depth: usize) -> Arg {
    let mut out = Vec::new();
    if depth > MAX_DEPTH {
        return out;
    }
    for k in e.elements() {
        node(k, depth, &mut out);
    }
    out
}

/// Reads a single math node element and appends it to `out`.
fn node(k: &El, depth: usize, out: &mut Arg) {
    let d = depth + 1;
    match k.local() {
        "r" => {
            if let Some(r) = run(k) {
                out.push(MNode::Run(r));
            }
        }
        "f" => {
            let pr = k.child("fPr");
            let kind = match val(pr, "type") {
                Some("skw") => FracKind::Skewed,
                Some("lin") => FracKind::Linear,
                Some("noBar") => FracKind::NoBar,
                _ => FracKind::Bar,
            };
            out.push(MNode::Frac { kind, num: sub_arg(k, "num", d), den: sub_arg(k, "den", d) });
        }
        "sSup" => out.push(MNode::Script { kind: ScriptKind::Sup, base: sub_arg(k, "e", d), sub: Vec::new(), sup: sub_arg(k, "sup", d) }),
        "sSub" => out.push(MNode::Script { kind: ScriptKind::Sub, base: sub_arg(k, "e", d), sub: sub_arg(k, "sub", d), sup: Vec::new() }),
        "sSubSup" => {
            out.push(MNode::Script { kind: ScriptKind::SubSup, base: sub_arg(k, "e", d), sub: sub_arg(k, "sub", d), sup: sub_arg(k, "sup", d) })
        }
        "sPre" => out.push(MNode::Script { kind: ScriptKind::Pre, base: sub_arg(k, "e", d), sub: sub_arg(k, "sub", d), sup: sub_arg(k, "sup", d) }),
        "rad" => {
            let pr = k.child("radPr");
            out.push(MNode::Rad { deg: sub_arg(k, "deg", d), deg_hide: prop(pr, "degHide", false), e: sub_arg(k, "e", d) });
        }
        "nary" => {
            let pr = k.child("naryPr");
            let chr = chr(pr, "chr").map(|c| c.unwrap_or('∫')).unwrap_or('∫');
            let lim_loc = match val(pr, "limLoc") {
                Some("undOvr") => Some(LimLoc::UndOvr),
                Some("subSup") => Some(LimLoc::SubSup),
                _ => None,
            };
            out.push(MNode::Nary {
                chr,
                lim_loc,
                grow: prop(pr, "grow", false),
                sub_hide: prop(pr, "subHide", false),
                sup_hide: prop(pr, "supHide", false),
                sub: sub_arg(k, "sub", d),
                sup: sub_arg(k, "sup", d),
                e: sub_arg(k, "e", d),
            });
        }
        "d" => {
            let pr = k.child("dPr");
            let beg = chr(pr, "begChr").unwrap_or(Some('('));
            let end = chr(pr, "endChr").unwrap_or(Some(')'));
            let sep = chr(pr, "sepChr").unwrap_or(Some('|'));
            let elems: Vec<Arg> = k.children_named("e").map(|e| arg(e, d)).collect();
            out.push(MNode::Delim {
                beg,
                end,
                sep,
                grow: prop(pr, "grow", true),
                shp_match: val(pr, "shp") == Some("match"),
                elems: if elems.is_empty() { vec![Vec::new()] } else { elems },
            });
        }
        "func" => out.push(MNode::Func { name: sub_arg(k, "fName", d), e: sub_arg(k, "e", d) }),
        "limLow" => out.push(MNode::Lim { upper: false, e: sub_arg(k, "e", d), lim: sub_arg(k, "lim", d) }),
        "limUpp" => out.push(MNode::Lim { upper: true, e: sub_arg(k, "e", d), lim: sub_arg(k, "lim", d) }),
        "acc" => {
            let c = chr(k.child("accPr"), "chr").map(|c| c.unwrap_or('\u{302}')).unwrap_or('\u{302}');
            out.push(MNode::Acc { chr: c, e: sub_arg(k, "e", d) });
        }
        "bar" => out.push(MNode::Bar { top: val(k.child("barPr"), "pos") == Some("top"), e: sub_arg(k, "e", d) }),
        "borderBox" => {
            let pr = k.child("borderBoxPr");
            let ht = prop(pr, "hideTop", false);
            let hb = prop(pr, "hideBot", false);
            let hl = prop(pr, "hideLeft", false);
            let hr = prop(pr, "hideRight", false);
            let sh = prop(pr, "strikeH", false);
            let sv = prop(pr, "strikeV", false);
            let bltr = prop(pr, "strikeBLTR", false);
            let tlbr = prop(pr, "strikeTLBR", false);
            if ht && hl && hr && !hb && bltr && !sh && !sv && !tlbr {
                out.push(MNode::Phasor { e: sub_arg(k, "e", d) });
            } else {
                out.push(MNode::BorderBox { hide: [ht, hb, hl, hr], strike: [sh, sv, bltr, tlbr], e: sub_arg(k, "e", d) });
            }
        }
        "box" => out.push(MNode::Boxed { e: sub_arg(k, "e", d) }),
        "groupChr" => {
            let pr = k.child("groupChrPr");
            let c = chr(pr, "chr").map(|c| c.unwrap_or('\u{23DF}')).unwrap_or('\u{23DF}');
            out.push(MNode::GroupChr { chr: c, top: val(pr, "pos") == Some("top"), e: sub_arg(k, "e", d) });
        }
        "eqArr" => out.push(MNode::EqArr { rows: k.children_named("e").map(|e| arg(e, d)).collect() }),
        "m" => {
            let mut col_jc = Vec::new();
            if let Some(mcs) = k.child("mPr").and_then(|p| p.child("mcs")) {
                for mc in mcs.children_named("mc") {
                    let pr = mc.child("mcPr");
                    let jc = match val(pr, "mcJc") {
                        Some("left") => ColJc::Left,
                        Some("right") => ColJc::Right,
                        _ => ColJc::Center,
                    };
                    let count = val(pr, "count").and_then(|c| c.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, 64);
                    for _ in 0..count {
                        if col_jc.len() < 64 {
                            col_jc.push(jc);
                        }
                    }
                }
            }
            let rows = k.children_named("mr").take(1000).map(|r| r.children_named("e").take(64).map(|e| arg(e, d)).collect()).collect();
            out.push(MNode::Matrix { rows, col_jc });
        }
        "phant" => {
            let pr = k.child("phantPr");
            out.push(MNode::Phant {
                show: prop(pr, "show", true),
                zero_wid: prop(pr, "zeroWid", false),
                zero_asc: prop(pr, "zeroAsc", false),
                zero_desc: prop(pr, "zeroDesc", false),
                e: sub_arg(k, "e", d),
            });
        }
        "oMath" if depth <= MAX_DEPTH => {
            for c in k.elements() {
                node(c, d, out);
            }
        }
        "AlternateContent" if depth <= MAX_DEPTH => {
            if let Some(c) = k.child("Choice").or_else(|| k.child("Fallback")) {
                for c in c.elements() {
                    node(c, d, out);
                }
            }
        }
        _ => {}
    }
}

/// Extracts concatenated text from all `m:t` children of a run.
fn run_text(r: &El) -> String {
    let mut s = String::new();
    for c in r.elements() {
        if c.is("t") {
            s.push_str(&c.text());
        }
    }
    s
}

/// Reads an `m:r` run element into an `MRun`.
fn run(r: &El) -> Option<MRun> {
    let text = run_text(r);
    if text.is_empty() {
        return None;
    }
    let mut out = MRun { text, ..Default::default() };
    for pr in r.children_named("rPr") {
        if pr.prefix() == "m" || pr.elements().any(|c| matches!(c.local(), "nor" | "lit" | "scr" | "sty" | "brk")) {
            out.sty = match pr.child("sty").and_then(|s| s.attr("val")) {
                Some("p") => Some(MSty::Plain),
                Some("b") => Some(MSty::Bold),
                Some("i") => Some(MSty::Italic),
                Some("bi") => Some(MSty::BoldItalic),
                _ => out.sty,
            };
            out.scr = match pr.child("scr").and_then(|s| s.attr("val")) {
                Some("script") => MScr::Script,
                Some("fraktur") => MScr::Fraktur,
                Some("double-struck") => MScr::DoubleStruck,
                Some("sans-serif") => MScr::SansSerif,
                Some("monospace") => MScr::Monospace,
                _ => out.scr,
            };
            out.nor |= pr.child("nor").is_some_and(on);
            out.lit |= pr.child("lit").is_some_and(on);
            if let Some(b) = pr.child("brk") {
                let aln = b.attr("alnAt").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
                out.brk = Some(aln.min(255) as u8);
            }
        } else {
            wrpr(pr, &mut out);
        }
    }
    Some(out)
}

/// Reads run formatting properties from `w:rPr` or `a:rPr`.
fn wrpr(pr: &El, out: &mut MRun) {
    if let Some(sz) = pr.child("sz").and_then(|s| s.attr("val")).and_then(|v| v.trim().parse::<f32>().ok()) {
        let pt = sz / 2.0;
        if pt.is_finite() && (1.0..=1638.0).contains(&pt) {
            out.size = Some(pt);
        }
    } else if let Some(sz) = pr.f64("sz") {
        let pt = (sz / 100.0) as f32;
        if pt.is_finite() && (1.0..=1638.0).contains(&pt) {
            out.size = Some(pt);
        }
    }
    if let Some(c) = pr.child("color").and_then(|c| c.attr("val")).or_else(|| pr.find("srgbClr").and_then(|c| c.attr("val"))).and_then(Rgb::parse) {
        out.color = Some(c);
    }
    if let Some(f) = pr.child("latin").and_then(|l| l.attr("typeface")).or_else(|| pr.child("rFonts").and_then(|r| r.attr("ascii"))) {
        let f = f.trim();
        if !f.is_empty() {
            out.font = Some(f.to_string());
        }
    }
}
