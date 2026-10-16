//! OMML (ECMA-376 Part 1, §22.1) reader and writer.

use crate::xml::{self, XEl};
use crate::{Extra, Math, Node, Seq, Style};

pub const OMML_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// Joins two `m:oMath` children of one `m:oMathPara` (or sibling `m:oMath` elements) in a body;
/// written back verbatim, which restores the two elements.
pub const OMATH_BREAK: &str = "</m:oMath><m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">";

fn is_math(e: &XEl) -> bool {
    e.math
}

/// Children of `e`'s property container (`<local>Pr`) that the tree does not model.
fn extra_pr(e: &XEl, modeled: &[&str]) -> Vec<(String, String)> {
    let Some(pr) = child(e, &format!("{}Pr", e.local())) else { return vec![] };
    let mut v: Vec<(String, String)> = pr
        .els()
        .filter(|c| !(is_math(c) && modeled.contains(&c.local())))
        .map(|c| (if is_math(c) { c.local().to_string() } else { c.name.clone() }, extra_xml(c)))
        .collect();
    sort_pr(&mut v, &format!("{}Pr", e.local()));
    v
}

/// Schema order, as the writer emits it, so a re-read tree equals the one that was written.
fn sort_pr(v: &mut [(String, String)], tag: &str) {
    let order = pr_order(tag);
    v.sort_by_key(|(k, _)| order.iter().position(|o| o == k).unwrap_or(order.len()));
}

/// Verbatim XML of a preserved property; the redundant `xmlns:m` is dropped because `to_omml`
/// declares it on the root.
fn extra_xml(c: &XEl) -> String {
    let x = raw_xml(c);
    let decl = format!(" xmlns:m=\"{OMML_NS}\"");
    if c.name.starts_with("m:") { x.replacen(&decl, "", 1) } else { x }
}

fn keep(node: Node, extra: Extra) -> Node {
    if extra.is_empty() { node } else { Node::Props { extra, node: Box::new(node) } }
}

/// What a structural element carries beyond the tree: unmodelled properties, its own attributes,
/// and children that are neither its property container nor one of `kids`.
fn ex(e: &XEl, modeled: &[&str], kids: &[&str]) -> Extra {
    let prc = format!("{}Pr", e.local());
    let tail = e.els().filter(|c| !(is_math(c) && (c.local() == prc || kids.contains(&c.local())))).map(extra_xml).collect();
    Extra { pr: extra_pr(e, modeled), side: vec![], attrs: xml::start_extras(e, OMML_NS), tail }
}

/// Schema order of property children, so merged output stays valid.
fn pr_order(tag: &str) -> &'static [&'static str] {
    match tag {
        "fPr" => &["type", "ctrlPr"],
        "radPr" => &["degHide", "ctrlPr"],
        "sSubSupPr" => &["alnScr", "ctrlPr"],
        "naryPr" => &["chr", "limLoc", "grow", "subHide", "supHide", "ctrlPr"],
        "dPr" => &["begChr", "sepChr", "endChr", "grow", "shp", "ctrlPr"],
        "mPr" => &["baseJc", "plcHide", "rSp", "cSp", "rSpRule", "cGpRule", "cGp", "mcs", "ctrlPr"],
        "accPr" => &["chr", "ctrlPr"],
        "barPr" => &["pos", "ctrlPr"],
        "groupChrPr" => &["chr", "pos", "vertJc", "ctrlPr"],
        "eqArrPr" => &["baseJc", "maxDist", "objDist", "rSp", "rSpRule", "ctrlPr"],
        "boxPr" => &["opEmu", "noBreak", "diff", "brk", "aln", "ctrlPr"],
        "borderBoxPr" => &["hideTop", "hideBot", "hideLeft", "hideRight", "strikeH", "strikeV", "strikeBLTR", "strikeTLBR", "ctrlPr"],
        "rPr" => &["lit", "nor", "scr", "sty", "br", "aln"],
        _ => &["ctrlPr"],
    }
}

fn child<'a>(e: &'a XEl, local: &str) -> Option<&'a XEl> {
    e.els().find(|c| is_math(c) && c.local() == local)
}

fn push_text(out: &mut Seq, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Node::Text(t)) = out.last_mut() {
        t.push_str(s);
    } else {
        out.push(Node::Text(s.to_string()));
    }
}

fn push_styled(out: &mut Seq, s: &str, style: Option<Style>) {
    let Some(style) = style else { return push_text(out, s) };
    if s.is_empty() {
        return;
    }
    if let Some(Node::Styled { text, style: st }) = out.last_mut()
        && *st == style
    {
        text.push_str(s);
        return;
    }
    out.push(Node::Styled { text: s.to_string(), style });
}

fn run_style(e: &XEl) -> Option<Style> {
    let pr = child(e, "rPr")?;
    if let Some(n) = child(pr, "nor")
        && !matches!(n.attr_local("val"), Some("0") | Some("false") | Some("off"))
    {
        return Some(Style::Normal);
    }
    match child(pr, "sty")?.attr_local("val") {
        Some("p") => Some(Style::Plain),
        Some("b") => Some(Style::Bold),
        Some("bi") => Some(Style::BoldItalic),
        _ => None,
    }
}

/// The text of a sequence made only of text runs, if it is one.
fn plain_text(s: &Seq) -> Option<String> {
    let mut out = String::new();
    for n in s {
        match n {
            Node::Text(t) | Node::Styled { text: t, .. } => out.push_str(t),
            _ => return None,
        }
    }
    Some(out)
}

fn seq_of(e: &XEl, local: &str) -> Seq {
    child(e, local).map(convert_children).unwrap_or_default()
}

fn opt_seq(e: &XEl, local: &str) -> Option<Seq> {
    child(e, local).map(convert_children)
}

/// Like [`opt_seq`], but a part flagged hidden is dropped only when it is empty: hidden content
/// is kept so that an edit does not lose it.
fn hidable_seq(e: &XEl, props: &str, flag_name: &str, local: &str) -> Option<Seq> {
    let s = opt_seq(e, local);
    if flag(e, props, flag_name) { s.filter(|q| !q.is_empty()) } else { s }
}

fn val_of(e: &XEl, props: &str, local: &str) -> Option<String> {
    child(e, props).and_then(|p| child(p, local)).map(|c| c.attr_local("val").unwrap_or("").to_string())
}

fn flag(e: &XEl, props: &str, local: &str) -> bool {
    match child(e, props).and_then(|p| child(p, local)) {
        Some(c) => !matches!(c.attr_local("val"), Some("0") | Some("false") | Some("off")),
        None => false,
    }
}

fn convert_children(e: &XEl) -> Seq {
    let mut out = Seq::new();
    for c in e.els() {
        convert_el(c, &mut out);
    }
    out
}

/// Structural children of each modelled element (the property container is implied).
fn kids_of(local: &str) -> Option<&'static [&'static str]> {
    Some(match local {
        "f" => &["num", "den"],
        "rad" => &["deg", "e"],
        "sSup" => &["e", "sup"],
        "sSub" => &["e", "sub"],
        "sSubSup" => &["e", "sub", "sup"],
        "nary" => &["sub", "sup", "e"],
        "d" | "eqArr" | "acc" | "bar" | "groupChr" | "box" | "borderBox" => &["e"],
        "m" => &["mr"],
        "func" => &["fName", "e"],
        "limLow" | "limUpp" => &["e", "lim"],
        "sPre" => &["sub", "sup", "e"],
        _ => return None,
    })
}

/// True when the tree cannot hold everything in `e` in place: attributes or foreign children on
/// an inner container (`m:num`, `m:e`, `m:mr`, ...), a repeated container, or a foreign child
/// ahead of a modelled one (the tail is written last). Such an element is kept verbatim.
fn needs_raw(e: &XEl, kids: &[&str]) -> bool {
    let prc = format!("{}Pr", e.local());
    let mut foreign_seen = false;
    let mut seen: Vec<&str> = vec![];
    let repeats = matches!(e.local(), "d" | "eqArr" | "m");
    for c in e.els() {
        let modeled = is_math(c) && (c.local() == prc || kids.contains(&c.local()));
        if !modeled {
            foreign_seen = true;
            continue;
        }
        if foreign_seen || (!repeats && seen.contains(&c.local())) || !xml::start_extras(c, OMML_NS).is_empty() {
            return true;
        }
        seen.push(c.local());
        if c.local() == "mr" && (c.els().any(|g| !(is_math(g) && g.local() == "e")) || c.els().any(|g| !xml::start_extras(g, OMML_NS).is_empty())) {
            return true;
        }
    }
    false
}

fn convert_el(e: &XEl, out: &mut Seq) {
    if !is_math(e) {
        out.push(raw(e));
        return;
    }
    if let Some(kids) = kids_of(e.local())
        && needs_raw(e, kids)
    {
        out.push(raw(e));
        return;
    }
    match e.local() {
        "oMath" | "oMathPara" => {
            for c in e.els() {
                convert_el(c, out);
            }
        }
        // Paragraph properties are lifted into `Math::{para, jc}` by `convert_top`.
        "oMathParaPr" => {}
        "r" => {
            let mut s = String::new();
            let mut other = false;
            let mut side = vec![];
            for c in e.els() {
                if is_math(c) && c.local() == "t" {
                    s.push_str(&c.text());
                } else if is_math(c) && c.local() == "rPr" {
                } else if is_math(c) {
                    other = true;
                } else {
                    side.push(extra_xml(c));
                }
            }
            if other {
                out.push(raw(e));
                return;
            }
            let style = run_style(e);
            let mut pr = vec![];
            if let Some(rp) = child(e, "rPr") {
                for c in rp.els() {
                    let modeled = is_math(c)
                        && match c.local() {
                            "nor" => style == Some(Style::Normal),
                            "sty" => matches!(style, Some(Style::Plain | Style::Bold | Style::BoldItalic)),
                            _ => false,
                        };
                    if !modeled {
                        pr.push((if is_math(c) { c.local().to_string() } else { c.name.clone() }, extra_xml(c)));
                    }
                }
            }
            sort_pr(&mut pr, "rPr");
            if pr.is_empty() && side.is_empty() {
                push_styled(out, &s, style);
            } else {
                let node = match style {
                    Some(style) => Node::Styled { text: s, style },
                    None => Node::Text(s),
                };
                out.push(Node::Props { extra: Extra { pr, side, ..Default::default() }, node: Box::new(node) });
            }
        }
        "f" => out.push(keep(Node::Frac { num: seq_of(e, "num"), den: seq_of(e, "den") }, ex(e, &[], &["num", "den"]))),
        "rad" => {
            let deg = hidable_seq(e, "radPr", "degHide", "deg");
            let hide: &[&str] = if flag(e, "radPr", "degHide") && deg.is_none() { &["degHide"] } else { &[] };
            out.push(keep(Node::Rad { deg, body: seq_of(e, "e") }, ex(e, hide, &["deg", "e"])));
        }
        "sSup" => out.push(keep(Node::Script { base: seq_of(e, "e"), sub: None, sup: Some(seq_of(e, "sup")) }, ex(e, &[], &["e", "sup"]))),
        "sSub" => out.push(keep(Node::Script { base: seq_of(e, "e"), sub: Some(seq_of(e, "sub")), sup: None }, ex(e, &[], &["e", "sub"]))),
        "sSubSup" => out.push(keep(
            Node::Script { base: seq_of(e, "e"), sub: Some(seq_of(e, "sub")), sup: Some(seq_of(e, "sup")) },
            ex(e, &[], &["e", "sub", "sup"]),
        )),
        "nary" => {
            // No `m:chr` means the default integral; an empty `m:chr` means no operator (NUL).
            let op = match val_of(e, "naryPr", "chr") {
                Some(v) => v.chars().next().unwrap_or('\0'),
                None => '\u{222B}',
            };
            let sub = hidable_seq(e, "naryPr", "subHide", "sub");
            let sup = hidable_seq(e, "naryPr", "supHide", "sup");
            let mut hide = vec![];
            if val_of(e, "naryPr", "chr").is_none_or(|v| v.chars().count() <= 1) {
                hide.push("chr");
            }
            if sub.is_none() {
                hide.push("subHide");
            }
            if sup.is_none() {
                hide.push("supHide");
            }
            let mut pr = ex(e, &hide, &["sub", "sup", "e"]);
            // The writer supplies the usual limit placement; only a different one is preserved.
            let usual = if matches!(op, '\u{222B}'..='\u{2233}') { "subSup" } else { "undOvr" };
            if val_of(e, "naryPr", "limLoc").as_deref() == Some(usual) {
                pr.pr.retain(|(k, _)| k != "limLoc");
            }
            out.push(keep(Node::Nary { op, sub, sup, body: seq_of(e, "e") }, pr));
        }
        "d" => {
            let open = val_of(e, "dPr", "begChr").unwrap_or_else(|| "(".into());
            let close = val_of(e, "dPr", "endChr").unwrap_or_else(|| ")".into());
            let sep = val_of(e, "dPr", "sepChr").unwrap_or_else(|| "|".into());
            let mut items: Vec<Seq> = e.els().filter(|c| is_math(c) && c.local() == "e").map(convert_children).collect();
            if items.is_empty() {
                items.push(Seq::new());
            }
            out.push(keep(Node::Delim { open, close, sep, items }, ex(e, &["begChr", "endChr", "sepChr"], &["e"])));
        }
        "m" => {
            let rows = e
                .els()
                .filter(|r| is_math(r) && r.local() == "mr")
                .map(|r| r.els().filter(|c| is_math(c) && c.local() == "e").map(convert_children).collect::<Vec<_>>())
                .collect();
            out.push(keep(Node::Matrix { rows }, ex(e, &[], &["mr"])));
        }
        "func" => {
            let fname = child(e, "fName");
            let runs_only = fname.is_some_and(|f| f.els().all(|c| is_math(c) && c.local() == "r"));
            let nseq = seq_of(e, "fName");
            let bare: Seq = nseq.iter().map(|n| n.bare().clone()).collect();
            let plain = plain_text(&bare).filter(|_| runs_only);
            // A structured name (Word writes `lim` as a limLow, `log_b` as a sSub) keeps its XML
            // verbatim next to a linear-text name, so the func wrapper survives.
            let structured = fname.is_some() && plain.is_none();
            let n = match plain {
                Some(n) => n.trim().to_string(),
                None => crate::to_linear(&Math::new(nseq)).trim().to_string(),
            };
            let mut gen_run = String::new();
            run(&mut gen_run, &n, Some(Style::Plain), &Extra::default(), false);
            let mut raw_runs = String::new();
            if let Some(f) = fname {
                for c in f.els() {
                    raw_runs.push_str(&extra_xml(c));
                }
            }
            let raw_runs = raw_runs.replace(&format!(" xmlns:m=\"{OMML_NS}\""), "");
            let mut x = ex(e, &[], &["fName", "e"]);
            x.side = if raw_runs == gen_run && !structured { vec![] } else { vec![raw_runs] };
            out.push(keep(Node::Func { name: n, body: seq_of(e, "e") }, x));
        }
        "acc" => {
            // An empty or multi-character `m:chr` cannot be a single `char`; its XML is kept verbatim.
            let v = val_of(e, "accPr", "chr");
            let single = v.as_deref().is_some_and(|s| s.chars().count() == 1);
            let ch = v.and_then(|s| s.chars().next()).unwrap_or('\u{0302}');
            let modeled: &[&str] = if single || val_of(e, "accPr", "chr").is_none() { &["chr"] } else { &[] };
            out.push(keep(Node::Accent { ch, body: seq_of(e, "e") }, ex(e, modeled, &["e"])));
        }
        "limLow" | "limUpp" => {
            out.push(keep(Node::Limit { lower: e.local() == "limLow", base: seq_of(e, "e"), lim: seq_of(e, "lim") }, ex(e, &[], &["e", "lim"])))
        }
        "eqArr" => {
            let mut rows: Vec<Seq> = e.els().filter(|c| is_math(c) && c.local() == "e").map(convert_children).collect();
            if rows.is_empty() {
                rows.push(Seq::new());
            }
            out.push(keep(Node::EqArr { rows }, ex(e, &[], &["e"])));
        }
        "sPre" => {
            out.push(keep(Node::PreScript { sub: seq_of(e, "sub"), sup: seq_of(e, "sup"), base: seq_of(e, "e") }, ex(e, &[], &["sub", "sup", "e"])))
        }
        "bar" => {
            out.push(keep(Node::Bar { top: val_of(e, "barPr", "pos").as_deref() == Some("top"), body: seq_of(e, "e") }, ex(e, &["pos"], &["e"])))
        }
        "groupChr" => {
            let ch = val_of(e, "groupChrPr", "chr").and_then(|s| s.chars().next()).unwrap_or('\u{23DF}');
            let top = val_of(e, "groupChrPr", "pos").as_deref() == Some("top");
            out.push(keep(Node::GroupChr { ch, top, body: seq_of(e, "e") }, ex(e, &["chr", "pos"], &["e"])));
        }
        "box" => out.push(keep(Node::Boxed { border: false, body: seq_of(e, "e") }, ex(e, &[], &["e"]))),
        "borderBox" => out.push(keep(Node::Boxed { border: true, body: seq_of(e, "e") }, ex(e, &[], &["e"]))),
        // A stray run-properties element carries no content.
        "rPr" => {}
        _ => out.push(raw(e)),
    }
}

fn raw_xml(e: &XEl) -> String {
    let mut s = String::new();
    xml::write_el(e, true, &mut s, 0);
    s
}

fn raw(e: &XEl) -> Node {
    Node::Raw(raw_xml(e))
}

/// Separate the second and later `m:oMath` of a document from what came before.
fn join_omath(e: &XEl, m: &mut Math, seen: &mut usize) {
    if is_math(e) && e.local() == "oMath" {
        // Bare declarations are not an attribute worth carrying; the Raw nodes bring their own.
        let plain = e.attrs.iter().any(|(k, _)| k != "xmlns" && !k.starts_with("xmlns:"));
        let attrs = if plain { xml::start_extras(e, OMML_NS) } else { String::new() };
        if *seen > 0 {
            let mut brk = OMATH_BREAK.to_string();
            if !attrs.is_empty() {
                brk.pop();
                brk.push_str(&attrs);
                brk.push('>');
            }
            m.body.push(Node::Raw(brk));
        } else {
            m.attrs = attrs;
        }
        *seen += 1;
    }
}

/// Top level: wrapper elements that are not OMML (for example `a14:m`) are looked through.
fn convert_top(e: &XEl, m: &mut Math, depth: usize, seen: &mut usize) {
    if is_math(e) && e.local() == "oMathPara" {
        m.para = true;
        if let Some(jc) = val_of(e, "oMathParaPr", "jc") {
            m.jc = Some(jc);
        }
        for c in e.els() {
            join_omath(c, m, seen);
            convert_el(c, &mut m.body);
        }
    } else if is_math(e) || depth > crate::XML_DEPTH {
        join_omath(e, m, seen);
        convert_el(e, &mut m.body);
    } else {
        for c in e.els() {
            convert_top(c, m, depth + 1, seen);
        }
    }
}

/// Parse OMML (an `m:oMath`, `m:oMathPara`, or a bare fragment). Never fails: unknown elements
/// become [`Node::Raw`] and malformed input yields what could be read.
pub fn from_omml(xml_src: &str) -> Math {
    let roots = xml::parse(xml_src);
    let mut m = Math::default();
    let mut seen = 0usize;
    for r in &roots {
        convert_top(r, &mut m, 0, &mut seen);
    }
    m
}

// ---- writer -------------------------------------------------------------------------------

fn val_s(tag: &str, v: &str) -> (String, String) {
    let mut out = String::new();
    out.push_str("<m:");
    out.push_str(tag);
    out.push_str(" m:val=\"");
    xml::escape_attr(v, &mut out);
    out.push_str("\"/>");
    (tag.to_string(), out)
}

fn val(out: &mut String, tag: &str, v: &str) {
    out.push_str(&val_s(tag, v).1);
}

/// Merge generated property children with the preserved ones (preserved win on a name clash,
/// they are never ones the tree models) in schema order. `always` writes the container even
/// when it is empty.
fn pr(out: &mut String, tag: &str, generated: Vec<(String, String)>, x: &Extra, always: bool) {
    let mut items: Vec<(String, String)> = generated.into_iter().filter(|(k, _)| !x.pr.iter().any(|(xk, _)| xk == k)).collect();
    items.extend(x.pr.iter().cloned());
    if items.is_empty() && !always {
        return;
    }
    let order = pr_order(tag);
    items.sort_by_key(|(k, _)| order.iter().position(|o| o == k).unwrap_or(order.len()));
    out.push_str("<m:");
    out.push_str(tag);
    out.push('>');
    for (_, xml) in &items {
        out.push_str(xml);
    }
    out.push_str("</m:");
    out.push_str(tag);
    out.push('>');
}

/// True while the verbatim `m:fName` content still stands for `name` (as plain text or as the
/// linear form of its structure).
fn name_matches(raw: &str, name: &str) -> bool {
    let m = from_omml(&format!("<m:oMath>{raw}</m:oMath>"));
    let bare: Seq = m.body.iter().map(|n| n.bare().clone()).collect();
    plain_text(&bare).is_some_and(|t| t.trim() == name) || crate::to_linear(&m).trim() == name
}

fn run(out: &mut String, text: &str, style: Option<Style>, x: &Extra, force: bool) {
    if text.is_empty() && !force {
        return;
    }
    out.push_str("<m:r>");
    let mut pre = vec![];
    match style {
        Some(Style::Plain) => pre.push(val_s("sty", "p")),
        Some(Style::Bold) => pre.push(val_s("sty", "b")),
        Some(Style::BoldItalic) => pre.push(val_s("sty", "bi")),
        Some(Style::Normal) => pre.push(("nor".to_string(), "<m:nor/>".to_string())),
        None => {}
    }
    pr(out, "rPr", pre, x, false);
    for s in &x.side {
        out.push_str(s);
    }
    out.push_str("<m:t xml:space=\"preserve\">");
    xml::escape(text, out);
    out.push_str("</m:t></m:r>");
}

fn wrap(out: &mut String, tag: &str, s: &[Node], depth: usize) {
    out.push_str("<m:");
    out.push_str(tag);
    out.push('>');
    write_seq(out, s, depth);
    out.push_str("</m:");
    out.push_str(tag);
    out.push('>');
}

fn write_seq(out: &mut String, s: &[Node], depth: usize) {
    if depth > crate::XML_DEPTH {
        return;
    }
    for n in s {
        write_node(out, n, depth + 1, &Extra::default());
    }
}

fn open(out: &mut String, tag: &str, x: &Extra) {
    out.push_str("<m:");
    out.push_str(tag);
    out.push_str(&x.attrs);
    out.push('>');
}

fn close(out: &mut String, tag: &str, x: &Extra) {
    for t in &x.tail {
        out.push_str(t);
    }
    out.push_str("</m:");
    out.push_str(tag);
    out.push('>');
}

fn write_node(out: &mut String, n: &Node, depth: usize, x: &Extra) {
    match n {
        Node::Props { extra, node } => {
            // Bounded: `Props` wrappers are only built by the reader, one per element.
            let mut inner = node.as_ref();
            let mut ex = extra;
            let mut guard = 0;
            while let Node::Props { extra: e2, node: n2 } = inner {
                if guard > 8 {
                    break;
                }
                guard += 1;
                inner = n2;
                ex = e2;
            }
            write_node(out, inner, depth, ex)
        }
        Node::Text(t) => run(out, t, None, x, !x.is_empty()),
        Node::Styled { text, style } => run(out, text, Some(*style), x, !x.is_empty()),
        Node::Frac { num, den } => {
            open(out, "f", x);
            pr(out, "fPr", vec![], x, false);
            wrap(out, "num", num, depth);
            wrap(out, "den", den, depth);
            close(out, "f", x);
        }
        Node::Rad { deg, body } => {
            open(out, "rad", x);
            let pre = if deg.is_none() { vec![val_s("degHide", "1")] } else { vec![] };
            pr(out, "radPr", pre, x, true);
            wrap(out, "deg", deg.as_deref().unwrap_or(&[]), depth);
            wrap(out, "e", body, depth);
            close(out, "rad", x);
        }
        Node::Script { base, sub, sup } => match (sub, sup) {
            (None, None) => write_seq(out, base, depth),
            (None, Some(p)) => {
                open(out, "sSup", x);
                pr(out, "sSupPr", vec![], x, false);
                wrap(out, "e", base, depth);
                wrap(out, "sup", p, depth);
                close(out, "sSup", x);
            }
            (Some(b), None) => {
                open(out, "sSub", x);
                pr(out, "sSubPr", vec![], x, false);
                wrap(out, "e", base, depth);
                wrap(out, "sub", b, depth);
                close(out, "sSub", x);
            }
            (Some(b), Some(p)) => {
                open(out, "sSubSup", x);
                pr(out, "sSubSupPr", vec![], x, false);
                wrap(out, "e", base, depth);
                wrap(out, "sub", b, depth);
                wrap(out, "sup", p, depth);
                close(out, "sSubSup", x);
            }
        },
        Node::Nary { op, sub, sup, body } => {
            open(out, "nary", x);
            let opstr = if *op == '\0' { String::new() } else { op.to_string() };
            let mut pre = vec![val_s("chr", &opstr), val_s("limLoc", if matches!(op, '\u{222B}'..='\u{2233}') { "subSup" } else { "undOvr" })];
            if sub.is_none() {
                pre.push(val_s("subHide", "1"));
            }
            if sup.is_none() {
                pre.push(val_s("supHide", "1"));
            }
            pr(out, "naryPr", pre, x, true);
            wrap(out, "sub", sub.as_deref().unwrap_or(&[]), depth);
            wrap(out, "sup", sup.as_deref().unwrap_or(&[]), depth);
            wrap(out, "e", body, depth);
            close(out, "nary", x);
        }
        Node::Delim { open: o, close: c, sep, items } => {
            open(out, "d", x);
            let mut pre = vec![val_s("begChr", o)];
            if items.len() > 1 || sep != "|" {
                pre.push(val_s("sepChr", sep));
            }
            pre.push(val_s("endChr", c));
            pr(out, "dPr", pre, x, true);
            if items.is_empty() {
                wrap(out, "e", &[], depth);
            }
            for it in items {
                wrap(out, "e", it, depth);
            }
            close(out, "d", x);
        }
        Node::Matrix { rows } => {
            open(out, "m", x);
            pr(out, "mPr", vec![], x, false);
            for r in rows {
                open(out, "mr", &Extra::default());
                for c in r {
                    wrap(out, "e", c, depth);
                }
                close(out, "mr", &Extra::default());
            }
            close(out, "m", x);
        }
        Node::Func { name, body } => {
            open(out, "func", x);
            pr(out, "funcPr", vec![], x, false);
            open(out, "fName", &Extra::default());
            match x.side.first() {
                // Verbatim name run, valid only while it still carries the name.
                Some(raw) if name_matches(raw, name) => out.push_str(raw),
                _ => run(out, name, Some(Style::Plain), &Extra::default(), false),
            }
            close(out, "fName", &Extra::default());
            wrap(out, "e", body, depth);
            close(out, "func", x);
        }
        Node::Accent { ch, body } => {
            open(out, "acc", x);
            pr(out, "accPr", vec![val_s("chr", &ch.to_string())], x, true);
            wrap(out, "e", body, depth);
            close(out, "acc", x);
        }
        Node::Limit { lower, base, lim } => {
            let tag = if *lower { "limLow" } else { "limUpp" };
            open(out, tag, x);
            pr(out, if *lower { "limLowPr" } else { "limUppPr" }, vec![], x, false);
            wrap(out, "e", base, depth);
            wrap(out, "lim", lim, depth);
            close(out, tag, x);
        }
        Node::EqArr { rows } => {
            open(out, "eqArr", x);
            pr(out, "eqArrPr", vec![], x, false);
            if rows.is_empty() {
                wrap(out, "e", &[], depth);
            }
            for r in rows {
                wrap(out, "e", r, depth);
            }
            close(out, "eqArr", x);
        }
        Node::PreScript { sub, sup, base } => {
            open(out, "sPre", x);
            pr(out, "sPrePr", vec![], x, false);
            wrap(out, "sub", sub, depth);
            wrap(out, "sup", sup, depth);
            wrap(out, "e", base, depth);
            close(out, "sPre", x);
        }
        Node::Bar { top, body } => {
            open(out, "bar", x);
            pr(out, "barPr", vec![val_s("pos", if *top { "top" } else { "bot" })], x, true);
            wrap(out, "e", body, depth);
            close(out, "bar", x);
        }
        Node::GroupChr { ch, top, body } => {
            open(out, "groupChr", x);
            pr(out, "groupChrPr", vec![val_s("chr", &ch.to_string()), val_s("pos", if *top { "top" } else { "bot" })], x, true);
            wrap(out, "e", body, depth);
            close(out, "groupChr", x);
        }
        Node::Boxed { border, body } => {
            let tag = if *border { "borderBox" } else { "box" };
            open(out, tag, x);
            pr(out, if *border { "borderBoxPr" } else { "boxPr" }, vec![], x, false);
            wrap(out, "e", body, depth);
            close(out, tag, x);
        }
        Node::Raw(s) => out.push_str(s),
    }
}

/// Serialise to a standalone element carrying `xmlns:m`: `m:oMath`, or `m:oMathPara` (with its
/// `m:oMathParaPr` as a sibling before the `m:oMath`, as the schema requires) for display math.
pub fn to_omml(m: &Math) -> String {
    let mut out = String::new();
    if m.para {
        out.push_str("<m:oMathPara xmlns:m=\"");
        out.push_str(OMML_NS);
        out.push_str("\">");
        if let Some(jc) = &m.jc {
            out.push_str("<m:oMathParaPr>");
            val(&mut out, "jc", jc);
            out.push_str("</m:oMathParaPr>");
        }
        out.push_str("<m:oMath");
        out.push_str(&m.attrs);
        out.push('>');
    } else {
        out.push_str("<m:oMath xmlns:m=\"");
        out.push_str(OMML_NS);
        out.push('"');
        out.push_str(&m.attrs);
        out.push('>');
    }
    write_seq(&mut out, &m.body, 0);
    out.push_str("</m:oMath>");
    if m.para {
        out.push_str("</m:oMathPara>");
    }
    out
}
