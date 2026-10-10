//! Tiny generic XML tree used by the OMML reader. Infallible: malformed input yields whatever
//! was parsed up to the error.

use crate::{MAX_DEPTH, MAX_NODES};

/// Deepest XML nesting kept: room for `MAX_DEPTH` levels of math nodes, each costing up to two
/// XML levels (`m:f` + `m:num`), plus the `oMathPara`/`oMath` wrappers.
pub const XML_DEPTH: usize = 2 * MAX_DEPTH + 8;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum XNode {
    El(XEl),
    Text(String),
}

#[derive(Debug, Clone, Default)]
pub struct XEl {
    /// Qualified name as written (`m:f`).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XNode>,
    /// `xmlns` declarations from ancestors that this element's subtree actually uses (bounded).
    pub inherited_ns: Arc<Vec<(String, String)>>,
    /// True when the element is in the OMML namespace (by URI; an unbound `m` or empty prefix
    /// counts, so bare fragments without declarations still read).
    pub math: bool,
    /// True when a needed inherited declaration was dropped to stay within the namespace byte
    /// budgets; serialising this element would then not be well-formed, so it is not emitted.
    pub ns_lost: bool,
}

impl XEl {
    pub fn prefix(&self) -> &str {
        self.name.split_once(':').map(|(p, _)| p).unwrap_or("")
    }
    pub fn local(&self) -> &str {
        self.name.split_once(':').map(|(_, l)| l).unwrap_or(&self.name)
    }
    pub fn attr_local(&self, local: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k.rsplit(':').next() == Some(local)).map(|(_, v)| v.as_str())
    }
    pub fn els(&self) -> impl Iterator<Item = &XEl> {
        self.children.iter().filter_map(|c| match c {
            XNode::El(e) => Some(e),
            XNode::Text(_) => None,
        })
    }
    pub fn text(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            match c {
                XNode::Text(t) => s.push_str(t),
                XNode::El(e) => s.push_str(&e.text()),
            }
        }
        s
    }
}

/// True for characters XML 1.0 can carry and that are not noncharacters.
pub fn xml_ok(c: char) -> bool {
    let u = c as u32;
    matches!(u, 0x9 | 0xA | 0xD | 0x20..=0xFDCF | 0xFDF0..=0x10FFFF) && (u & 0xFFFE) != 0xFFFE
}

/// Drop characters that cannot appear in XML (controls, noncharacters).
pub fn clean(s: &str) -> String {
    if s.chars().all(xml_ok) { s.to_string() } else { s.chars().filter(|&c| xml_ok(c)).collect() }
}

pub fn escape(s: &str, out: &mut String) {
    esc_into(s, out, false)
}

/// Attribute values: tab, CR and LF are written as character references, since a conforming
/// parser normalises literal ones to a space.
pub fn escape_attr(s: &str, out: &mut String) {
    esc_into(s, out, true)
}

fn esc_into(s: &str, out: &mut String, attr: bool) {
    for c in s.chars() {
        if !xml_ok(c) {
            continue;
        }
        match c {
            '\r' => out.push_str("&#13;"),
            '\n' if attr => out.push_str("&#10;"),
            '\t' if attr => out.push_str("&#9;"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

/// A name a conforming parser accepts: at most one colon, both parts non-empty NCNames.
fn valid_name(n: &str) -> bool {
    let mut parts = n.split(':');
    let ok = |p: &str| {
        let mut cs = p.chars();
        cs.next().is_some_and(|c| c.is_alphabetic() || c == '_') && cs.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
    };
    let (a, b, c) = (parts.next(), parts.next(), parts.next());
    c.is_none() && a.is_some_and(ok) && b.is_none_or(ok)
}

/// Serialise an element (with inherited namespace declarations it does not declare itself).
pub fn write_el(e: &XEl, with_inherited: bool, out: &mut String, depth: usize) {
    if depth > XML_DEPTH + 4 || !valid_name(&e.name) || (with_inherited && e.ns_lost) {
        return;
    }
    out.push('<');
    out.push_str(&e.name);
    if with_inherited {
        for (k, v) in e.inherited_ns.iter() {
            if !e.attrs.iter().any(|(ak, _)| ak == k) {
                out.push(' ');
                out.push_str(k);
                out.push_str("=\"");
                escape_attr(v, out);
                out.push('"');
            }
        }
    }
    for (k, v) in e.attrs.iter().filter(|(k, _)| valid_name(k)) {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape_attr(v, out);
        out.push('"');
    }
    if e.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &e.children {
        match c {
            XNode::Text(t) => escape(t, out),
            XNode::El(ch) => write_el(ch, false, out, depth + 1),
        }
    }
    out.push_str("</");
    out.push_str(&e.name);
    out.push('>');
}

/// The attributes of `e` as ` name="value"` text, for re-emission on a rewritten element whose
/// name is `m:*` under a root that declares `xmlns:m`. Declarations of the OMML namespace are
/// dropped (attributes in it are renamed to `m:`), a declaration an attribute needs is added from
/// the inherited set, and an attribute whose prefix is unbound is left out.
pub fn start_extras(e: &XEl, omml_ns: &str) -> String {
    if e.ns_lost {
        return String::new();
    }
    let lookup = |p: &str| -> Option<&str> {
        let key = format!("xmlns:{p}");
        e.attrs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
            .or_else(|| e.inherited_ns.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str()))
    };
    let mut decls = String::new();
    let mut attrs = String::new();
    for (k, v) in &e.attrs {
        if !valid_name(k) {
            continue;
        }
        let put = |name: &str, out: &mut String| {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            escape_attr(v, out);
            out.push('"');
        };
        if let Some(p) = k.strip_prefix("xmlns:") {
            if v != omml_ns && p != "m" {
                put(k, &mut decls);
            }
        } else if k == "xmlns" {
            // A default namespace would change the meaning of the rewritten element.
        } else if let Some((p, l)) = k.split_once(':') {
            if p == "xml" {
                put(k, &mut attrs);
            } else {
                match lookup(p) {
                    Some(ns) if ns == omml_ns => put(&format!("m:{l}"), &mut attrs),
                    Some(ns) => {
                        if !e.attrs.iter().any(|(ak, _)| ak.strip_prefix("xmlns:") == Some(p)) && !decls.contains(&format!(" xmlns:{p}=")) {
                            decls.push_str(&format!(" xmlns:{p}=\""));
                            escape_attr(ns, &mut decls);
                            decls.push('"');
                        }
                        put(k, &mut attrs);
                    }
                    None if p == "m" => put(k, &mut attrs),
                    None => {}
                }
            }
        } else {
            put(k, &mut attrs);
        }
    }
    decls + &attrs
}

fn start_el(b: &BytesStart, reader: &Reader<&[u8]>) -> XEl {
    let name = String::from_utf8_lossy(b.name().as_ref()).into_owned();
    let mut attrs = vec![];
    for a in b.attributes().with_checks(false).flatten().take(MAX_NS_DECLS + MAX_ATTRS + 64) {
        let k = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let v =
            a.decode_and_unescape_value(reader.decoder()).map(|c| c.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned());
        attrs.push((k, clean(&v)));
    }
    let mut el = XEl { name, attrs, children: vec![], inherited_ns: Arc::default(), math: false, ns_lost: false };
    cap_decls(&mut el);
    el
}

fn push_text(p: &mut XEl, s: &str) {
    let s = &clean(s);
    if s.is_empty() {
        return;
    }
    if let Some(XNode::Text(t)) = p.children.last_mut() {
        t.push_str(s);
    } else {
        p.children.push(XNode::Text(s.to_string()));
    }
}

/// Most `xmlns` declarations honoured on one element, and most distinct unbound prefixes tracked
/// per subtree. Both bound the work and the size of hostile namespace input.
const MAX_NS_DECLS: usize = 64;
const MAX_UNBOUND: usize = 64;
/// Most plain attributes kept on one element. Re-emission scans an element's attributes once per
/// attribute, so an unbounded count would be quadratic on hostile input.
const MAX_ATTRS: usize = 256;
/// Bytes of inherited declarations copied onto one element, and onto all elements of a document.
const MAX_INHERIT_ELEM: usize = 8 * 1024;
const MAX_INHERIT_DOC: usize = 16 * 1024 * 1024;

type Scope = HashMap<String, Vec<String>>;

/// Identical inherited-declaration sets are shared in memory; the document budget is still
/// charged for every copy that will be emitted.
type Interner = HashMap<Vec<(String, String)>, Arc<Vec<(String, String)>>>;

struct Budget {
    left: usize,
    sets: Interner,
}

fn is_decl(k: &str) -> bool {
    k == "xmlns" || k.starts_with("xmlns:")
}

/// Keep at most [`MAX_NS_DECLS`] namespace declarations on an element.
fn cap_decls(el: &mut XEl) {
    let mut seen = 0usize;
    let mut plain = 0usize;
    el.attrs.retain(|(k, _)| {
        if is_decl(k) {
            seen += 1;
            seen <= MAX_NS_DECLS
        } else {
            plain += 1;
            plain <= MAX_ATTRS
        }
    });
}

/// Record the prefixes this element's own name and attributes use.
fn note_uses(el: &XEl, unbound: &mut BTreeSet<String>) {
    let mut add = |p: &str| {
        if unbound.len() < MAX_UNBOUND || unbound.contains(p) {
            unbound.insert(p.to_string());
        }
    };
    match el.name.split_once(':') {
        Some((p, _)) => add(&format!("xmlns:{p}")),
        None => add("xmlns"),
    }
    for (k, _) in &el.attrs {
        if is_decl(k) {
            continue;
        }
        if let Some((p, _)) = k.split_once(':')
            && p != "xml"
        {
            add(&format!("xmlns:{p}"));
        }
    }
}

/// Namespace test by URI, falling back to the conventional spelling when the prefix is unbound.
fn resolve_math(el: &XEl, scope: &Scope) -> bool {
    let key = match el.name.split_once(':') {
        Some((p, _)) => format!("xmlns:{p}"),
        None => "xmlns".to_string(),
    };
    match scope.get(&key).and_then(|v| v.last()) {
        Some(uri) => uri == crate::OMML_NS,
        None => matches!(el.prefix(), "m" | ""),
    }
}

fn enter(el: &XEl, scope: &mut Scope) {
    for (k, v) in &el.attrs {
        if is_decl(k) {
            scope.entry(k.clone()).or_default().push(v.clone());
        }
    }
}

fn leave(el: &XEl, scope: &mut Scope) {
    for (k, _) in &el.attrs {
        if is_decl(k)
            && let Some(v) = scope.get_mut(k)
        {
            v.pop();
        }
    }
}

/// Finish an element: names not declared within its own subtree are resolved against the scope
/// of its ancestors (lazily, only for prefixes actually used) and the rest bubbles to the parent.
fn finish(el: &mut XEl, mut unbound: BTreeSet<String>, scope: &Scope, parent: Option<&mut BTreeSet<String>>, budget: &mut Budget) {
    for (k, _) in &el.attrs {
        if is_decl(k) {
            unbound.remove(k.as_str());
        }
    }
    // `scope` still holds this element's own declarations, which were just removed from `unbound`.
    let mut used = 0usize;
    let mut mine: Vec<(String, String)> = vec![];
    for k in &unbound {
        if let Some(v) = scope.get(k).and_then(|v| v.last()) {
            let cost = k.len() + v.len() + 4;
            if used + cost > MAX_INHERIT_ELEM {
                el.ns_lost = true;
                continue;
            }
            used += cost;
            mine.push((k.clone(), v.clone()));
        }
    }
    if !mine.is_empty() {
        // Charged per emitted copy: every element re-serialises its own inherited set, so the
        // document budget bounds the total bytes written, not the distinct sets stored.
        if used > budget.left {
            el.ns_lost = true;
        } else {
            budget.left -= used;
            if let Some(shared) = budget.sets.get(&mine) {
                el.inherited_ns = Arc::clone(shared);
            } else {
                let shared = Arc::new(mine.clone());
                budget.sets.insert(mine, Arc::clone(&shared));
                el.inherited_ns = shared;
            }
        }
    }
    if let Some(p) = parent {
        for k in unbound {
            if p.len() < MAX_UNBOUND || p.contains(&k) {
                p.insert(k);
            }
        }
    }
}

/// Parse into a forest of top-level elements.
pub fn parse(xml: &str) -> Vec<XEl> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<XEl> = vec![];
    let mut roots: Vec<XEl> = vec![];
    let mut overflow = 0usize;
    let mut nodes = 0usize;
    let mut unb: Vec<BTreeSet<String>> = vec![];
    let mut scope: Scope = HashMap::new();
    let mut budget = Budget { left: MAX_INHERIT_DOC, sets: HashMap::new() };
    let close = |stack: &mut Vec<XEl>, unb: &mut Vec<BTreeSet<String>>, scope: &mut Scope, roots: &mut Vec<XEl>, budget: &mut Budget| {
        if let Some(mut el) = stack.pop() {
            let mine = unb.pop().unwrap_or_default();
            finish(&mut el, mine, scope, unb.last_mut(), budget);
            leave(&el, scope);
            match stack.last_mut() {
                Some(p) => p.children.push(XNode::El(el)),
                None => roots.push(el),
            }
        }
    };
    while let Ok(ev) = reader.read_event() {
        if matches!(ev, Event::Start(_) | Event::Empty(_)) {
            nodes += 1;
            if nodes > MAX_NODES {
                break;
            }
        }
        match ev {
            Event::Start(b) => {
                if overflow > 0 || stack.len() >= XML_DEPTH {
                    overflow += 1;
                } else {
                    let mut el = start_el(&b, &reader);
                    enter(&el, &mut scope);
                    el.math = resolve_math(&el, &scope);
                    let mut u = BTreeSet::new();
                    note_uses(&el, &mut u);
                    unb.push(u);
                    stack.push(el);
                }
            }
            Event::Empty(b) => {
                if overflow == 0 && stack.len() < XML_DEPTH {
                    let mut el = start_el(&b, &reader);
                    enter(&el, &mut scope);
                    el.math = resolve_math(&el, &scope);
                    let mut u = BTreeSet::new();
                    note_uses(&el, &mut u);
                    finish(&mut el, u, &scope, unb.last_mut(), &mut budget);
                    leave(&el, &mut scope);
                    match stack.last_mut() {
                        Some(p) => p.children.push(XNode::El(el)),
                        None => roots.push(el),
                    }
                }
            }
            Event::End(_) => {
                if overflow > 0 {
                    overflow -= 1;
                } else {
                    close(&mut stack, &mut unb, &mut scope, &mut roots, &mut budget);
                }
            }
            Event::Text(t) => {
                if overflow == 0
                    && let Some(p) = stack.last_mut()
                {
                    let s = t.decode().map(|c| c.into_owned()).unwrap_or_default();
                    push_text(p, &s);
                }
            }
            Event::CData(t) => {
                if overflow == 0
                    && let Some(p) = stack.last_mut()
                {
                    push_text(p, &String::from_utf8_lossy(&t));
                }
            }
            Event::GeneralRef(r) => {
                if overflow == 0
                    && let Some(p) = stack.last_mut()
                {
                    let s = if r.is_char_ref() {
                        r.resolve_char_ref().ok().flatten().map(String::from).unwrap_or_default()
                    } else {
                        match r.decode().map(|c| c.into_owned()).unwrap_or_default().as_str() {
                            "amp" => "&".into(),
                            "lt" => "<".into(),
                            "gt" => ">".into(),
                            "quot" => "\"".into(),
                            "apos" => "'".into(),
                            _ => String::new(),
                        }
                    };
                    push_text(p, &s);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    while !stack.is_empty() {
        close(&mut stack, &mut unb, &mut scope, &mut roots, &mut budget);
    }
    roots
}
