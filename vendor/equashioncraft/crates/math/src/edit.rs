//! Direct editing of a math tree: a caret that lives inside the tree, selection, typing with
//! structure keys (`^` `_` `/` `sqrt`), Greek names, arrow navigation in visual order, Backspace
//! that unwraps empty structures, and template insertion at the caret.
//!
//! The caret is a [`Path`] to a child sequence plus a position counted in *units*: one unit per
//! character of a text node and one per structure (a fraction is a single unit of its parent
//! sequence). Nothing here knows about layout or egui, and no input can make it panic: every
//! access is checked, nesting is capped at [`MAX_DEPTH`], and a stale caret is repaired on use.

use crate::{MAX_DEPTH, Math, Node, Seq};

/// Route from the root sequence to a child sequence: at each step the index of a node in the
/// current sequence and the index of one of that node's child sequences (see [`children`]).
pub type Path = Vec<(usize, usize)>;

/// A position in the tree: inside the sequence at `path`, after `pos` units.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caret {
    pub path: Path,
    pub pos: usize,
}

/// Navigation and deletion keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Backspace,
    Delete,
    /// Next place (cell, numerator, exponent, ...); with Shift the previous one.
    Tab,
    Undo,
    Redo,
}

impl Key {
    /// A key from its name (`left`, `Backspace`, ... any case).
    pub fn parse(name: &str) -> Option<Key> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "left" => Key::Left,
            "right" => Key::Right,
            "up" => Key::Up,
            "down" => Key::Down,
            "home" => Key::Home,
            "end" => Key::End,
            "backspace" => Key::Backspace,
            "delete" | "del" => Key::Delete,
            "tab" => Key::Tab,
            "undo" => Key::Undo,
            "redo" => Key::Redo,
            _ => return None,
        })
    }
}

/// The child sequences of a node in reading order. Layout and editing share this order: it is
/// what the second number of a [`Path`] step indexes.
pub fn children(n: &Node) -> Vec<&Seq> {
    use std::iter::once;
    match n {
        Node::Frac { num, den } => vec![num, den],
        Node::Rad { deg, body } => deg.iter().chain(once(body)).collect(),
        Node::Script { base, sub, sup } => once(base).chain(sub.iter()).chain(sup.iter()).collect(),
        Node::Nary { sub, sup, body, .. } => sub.iter().chain(sup.iter()).chain(once(body)).collect(),
        Node::Delim { items, .. } => items.iter().collect(),
        Node::Matrix { rows } => rows.iter().flatten().collect(),
        Node::Func { body, .. } | Node::Accent { body, .. } | Node::Bar { body, .. } | Node::GroupChr { body, .. } | Node::Boxed { body, .. } => {
            vec![body]
        }
        Node::Limit { base, lim, .. } => vec![base, lim],
        Node::EqArr { rows } => rows.iter().collect(),
        Node::PreScript { sub, sup, base } => vec![sub, sup, base],
        Node::Props { node, .. } => children(node),
        Node::Text(_) | Node::Styled { .. } | Node::Raw(_) => vec![],
    }
}

/// Mutable [`children`], same order.
pub fn children_mut(n: &mut Node) -> Vec<&mut Seq> {
    use std::iter::once;
    match n {
        Node::Frac { num, den } => vec![num, den],
        Node::Rad { deg, body } => deg.iter_mut().chain(once(body)).collect(),
        Node::Script { base, sub, sup } => once(base).chain(sub.iter_mut()).chain(sup.iter_mut()).collect(),
        Node::Nary { sub, sup, body, .. } => sub.iter_mut().chain(sup.iter_mut()).chain(once(body)).collect(),
        Node::Delim { items, .. } => items.iter_mut().collect(),
        Node::Matrix { rows } => rows.iter_mut().flatten().collect(),
        Node::Func { body, .. } | Node::Accent { body, .. } | Node::Bar { body, .. } | Node::GroupChr { body, .. } | Node::Boxed { body, .. } => {
            vec![body]
        }
        Node::Limit { base, lim, .. } => vec![base, lim],
        Node::EqArr { rows } => rows.iter_mut().collect(),
        Node::PreScript { sub, sup, base } => vec![sub, sup, base],
        Node::Props { node, .. } => children_mut(node),
        Node::Text(_) | Node::Styled { .. } | Node::Raw(_) => vec![],
    }
}

fn bare_mut(n: &mut Node) -> &mut Node {
    match n {
        Node::Props { node, .. } => bare_mut(node),
        other => other,
    }
}

/// The text a node carries, if it is a text-like leaf.
fn text_of(n: &Node) -> Option<&str> {
    match n.bare() {
        Node::Text(t) => Some(t),
        Node::Styled { text, .. } => Some(text),
        _ => None,
    }
}

fn node_units(n: &Node) -> usize {
    text_of(n).map_or(1, |t| t.chars().count())
}

/// A term that is one parenthesised group, as a fraction's numerator: the group's contents
/// (the fraction bar makes the parentheses redundant).
fn unwrap_paren(mut term: Vec<Node>) -> Vec<Node> {
    if let [only] = term.as_mut_slice()
        && let Node::Delim { open, close, items, .. } = bare_mut(only)
        && open == "("
        && close == ")"
        && items.len() == 1
    {
        return items.pop().unwrap_or_default();
    }
    term
}

/// Number of units in a sequence.
pub fn units(seq: &[Node]) -> usize {
    seq.iter().map(node_units).fold(0usize, usize::saturating_add)
}

/// Units before node `idx`.
fn start_of(seq: &[Node], idx: usize) -> usize {
    seq.iter().take(idx).map(node_units).fold(0usize, usize::saturating_add)
}

/// The node holding the unit that starts at `pos`, and the unit's offset inside it.
fn locate(seq: &[Node], pos: usize) -> Option<(usize, usize)> {
    let mut acc = 0usize;
    for (i, n) in seq.iter().enumerate() {
        let u = node_units(n);
        if pos < acc.saturating_add(u) {
            return Some((i, pos - acc));
        }
        acc = acc.saturating_add(u);
    }
    None
}

/// One entry per unit: the character of a text unit, `None` for a structure.
fn unit_chars(seq: &[Node]) -> Vec<Option<char>> {
    let mut out = vec![];
    for n in seq {
        match text_of(n) {
            Some(t) => out.extend(t.chars().map(Some)),
            None => out.push(None),
        }
    }
    out
}

/// The sequence at `path`.
pub fn seq_at<'a>(body: &'a Seq, path: &[(usize, usize)]) -> Option<&'a Seq> {
    if path.len() > MAX_DEPTH {
        return None;
    }
    let mut cur = body;
    for &(i, c) in path {
        cur = children(cur.get(i)?).get(c).copied()?;
    }
    Some(cur)
}

/// The sequence at `path`, mutable.
pub fn seq_at_mut<'a>(body: &'a mut Seq, path: &[(usize, usize)]) -> Option<&'a mut Seq> {
    if path.len() > MAX_DEPTH {
        return None;
    }
    let mut cur = body;
    for &(i, c) in path {
        let n = cur.get_mut(i)?;
        cur = children_mut(n).into_iter().nth(c)?;
    }
    Some(cur)
}

fn split_chars(t: &str, off: usize) -> (String, String) {
    (t.chars().take(off).collect(), t.chars().skip(off).collect())
}

/// Split a text-like node after `off` characters.
fn split_node(n: &Node, off: usize) -> Option<(Node, Node)> {
    match n {
        Node::Text(t) => {
            let (a, b) = split_chars(t, off);
            Some((Node::Text(a), Node::Text(b)))
        }
        Node::Styled { text, style } => {
            let (a, b) = split_chars(text, off);
            Some((Node::Styled { text: a, style: *style }, Node::Styled { text: b, style: *style }))
        }
        Node::Props { extra, node } => {
            let (a, b) = split_node(node, off)?;
            Some((Node::Props { extra: extra.clone(), node: Box::new(a) }, Node::Props { extra: extra.clone(), node: Box::new(b) }))
        }
        _ => None,
    }
}

/// Make unit position `pos` a boundary between nodes (splitting a text node if needed) and
/// return the index of the node that starts there.
fn boundary(seq: &mut Seq, pos: usize) -> usize {
    let mut acc = 0usize;
    let mut i = 0usize;
    while i < seq.len() {
        if acc >= pos {
            return i;
        }
        let u = seq.get(i).map_or(0, node_units);
        if acc.saturating_add(u) <= pos {
            acc = acc.saturating_add(u);
            i += 1;
            continue;
        }
        let off = pos - acc;
        if let Some((a, b)) = seq.get(i).and_then(|n| split_node(n, off)) {
            if let Some(slot) = seq.get_mut(i) {
                *slot = a;
            }
            seq.insert(i + 1, b);
            return i + 1;
        }
        return i + 1;
    }
    seq.len()
}

/// Join neighbouring plain text nodes and drop empty ones.
fn merge(seq: &mut Seq) {
    let old = std::mem::take(seq);
    for n in old {
        match n {
            Node::Text(t) if t.is_empty() => {}
            Node::Text(t) => {
                if let Some(Node::Text(prev)) = seq.last_mut() {
                    prev.push_str(&t);
                } else {
                    seq.push(Node::Text(t));
                }
            }
            other => seq.push(other),
        }
    }
}

/// Remove the blank text slots a template carries, so empty places are empty sequences.
fn strip_blanks(seq: &mut Seq, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    seq.retain(|n| !matches!(n, Node::Text(t) if t.is_empty()));
    for n in seq.iter_mut() {
        for child in children_mut(n) {
            strip_blanks(child, depth + 1);
        }
    }
}

/// Path (relative to `seq`) to the first empty child sequence, depth first.
fn first_empty(seq: &[Node], depth: usize) -> Option<Path> {
    if depth > MAX_DEPTH {
        return None;
    }
    for (i, n) in seq.iter().enumerate() {
        for (c, child) in children(n).into_iter().enumerate() {
            if units(child) == 0 {
                return Some(vec![(i, c)]);
            }
            if let Some(mut rest) = first_empty(child, depth + 1) {
                rest.insert(0, (i, c));
                return Some(rest);
            }
        }
    }
    None
}

/// [`first_empty`], except that a leading script with an empty base (`ₙC_r`: the n before the
/// letter) is a place of its own: the caret goes to its script, not to its base.
fn first_empty_skipping_prescript(nodes: &[Node]) -> Option<Path> {
    let lead = match nodes.first() {
        Some(n @ Node::Script { base, sub, sup }) if nodes.len() > 1 && base.is_empty() && (sub.is_some() || sup.is_some()) => n,
        _ => return first_empty(nodes, 0),
    };
    if let Some(c) = children(lead).iter().enumerate().skip(1).find_map(|(c, ch)| (units(ch) == 0).then_some(c)) {
        return Some(vec![(0, c)]);
    }
    let mut rest = first_empty(nodes.get(1..)?, 0)?;
    if let Some(first) = rest.first_mut() {
        first.0 += 1;
    }
    Some(rest)
}

/// Where Up / Down goes from child `c` of `node`, if the structure stacks its children.
fn vertical_neighbor(node: &Node, c: usize, up: bool) -> Option<usize> {
    match node.bare() {
        Node::Frac { .. } => match (c, up) {
            (0, false) => Some(1),
            (1, true) => Some(0),
            _ => None,
        },
        Node::Script { sub, sup, .. } => {
            let sub_i = sub.is_some().then_some(1usize);
            let sup_i = sup.is_some().then_some(1 + usize::from(sub.is_some()));
            if up {
                if Some(c) == sup_i {
                    None
                } else if Some(c) == sub_i {
                    sup_i.or(Some(0))
                } else {
                    sup_i
                }
            } else if Some(c) == sub_i {
                None
            } else if Some(c) == sup_i {
                sub_i.or(Some(0))
            } else {
                sub_i
            }
        }
        Node::Nary { sub, sup, .. } => {
            let sub_i = sub.is_some().then_some(0usize);
            let sup_i = sup.is_some().then_some(usize::from(sub.is_some()));
            if up {
                if Some(c) == sup_i { None } else { sup_i }
            } else if Some(c) == sub_i {
                None
            } else {
                sub_i
            }
        }
        Node::Limit { lower, .. } => match (c, up, *lower) {
            (0, false, true) | (0, true, false) => Some(1),
            (1, true, true) | (1, false, false) => Some(0),
            _ => None,
        },
        Node::Matrix { rows } => {
            let mut idx = 0usize;
            let mut at = None;
            for (r, row) in rows.iter().enumerate() {
                if c < idx + row.len() {
                    at = Some((r, c - idx));
                    break;
                }
                idx += row.len();
            }
            let (r, col) = at?;
            let tr = if up { r.checked_sub(1)? } else { r + 1 };
            let target = rows.get(tr)?;
            let tcol = col.min(target.len().checked_sub(1)?);
            let before: usize = rows.iter().take(tr).map(Vec::len).sum();
            Some(before + tcol)
        }
        Node::EqArr { rows } => {
            let t = if up { c.checked_sub(1)? } else { c + 1 };
            (t < rows.len()).then_some(t)
        }
        Node::Rad { deg: Some(_), .. } => match (c, up) {
            (1, true) => Some(0),
            (0, false) => Some(1),
            _ => None,
        },
        Node::PreScript { .. } => match (c, up) {
            (0, true) | (2, true) => Some(1),
            (1, false) | (2, false) => Some(0),
            _ => None,
        },
        _ => None,
    }
}

const GREEK: &[(&str, char)] = &[
    ("alpha", '\u{3B1}'),
    ("beta", '\u{3B2}'),
    ("gamma", '\u{3B3}'),
    ("delta", '\u{3B4}'),
    ("epsilon", '\u{3B5}'),
    ("zeta", '\u{3B6}'),
    ("eta", '\u{3B7}'),
    ("theta", '\u{3B8}'),
    ("iota", '\u{3B9}'),
    ("kappa", '\u{3BA}'),
    ("lambda", '\u{3BB}'),
    ("mu", '\u{3BC}'),
    ("nu", '\u{3BD}'),
    ("xi", '\u{3BE}'),
    ("omicron", '\u{3BF}'),
    ("pi", '\u{3C0}'),
    ("rho", '\u{3C1}'),
    ("sigma", '\u{3C3}'),
    ("tau", '\u{3C4}'),
    ("upsilon", '\u{3C5}'),
    ("phi", '\u{3C6}'),
    ("chi", '\u{3C7}'),
    ("psi", '\u{3C8}'),
    ("omega", '\u{3C9}'),
    ("Gamma", '\u{393}'),
    ("Delta", '\u{394}'),
    ("Theta", '\u{398}'),
    ("Lambda", '\u{39B}'),
    ("Xi", '\u{39E}'),
    ("Pi", '\u{3A0}'),
    ("Sigma", '\u{3A3}'),
    ("Phi", '\u{3A6}'),
    ("Psi", '\u{3A8}'),
    ("Omega", '\u{3A9}'),
    ("infty", '\u{221E}'),
];

/// Words that become a symbol when typed whole (the letters before the caret are exactly the
/// word). `pm` must not fire inside `bpm`, so there is no suffix matching for these.
const WORDS: &[(&str, char)] = &[
    ("pm", '\u{B1}'),
    ("times", '\u{D7}'),
    ("cdot", '\u{B7}'),
    ("leq", '\u{2264}'),
    ("geq", '\u{2265}'),
    ("neq", '\u{2260}'),
    ("approx", '\u{2248}'),
    ("partial", '\u{2202}'),
    ("nabla", '\u{2207}'),
    ("forall", '\u{2200}'),
    ("exists", '\u{2203}'),
];

/// Characters after which an unparenthesised function argument (`sin x`) is over.
const ARG_ENDERS: &str = "+\u{2212}=<>,\u{B1}\u{D7}\u{B7}";

/// Most characters one [`Editor::type_str`] call processes.
const MAX_TYPED: usize = 100_000;

/// Most single steps one Tab takes looking for the next place.
const MAX_TAB_STEPS: usize = 10_000;

/// What a typed `\name` becomes.
enum Cmd {
    /// A symbol or letter.
    Sym(char),
    /// A node with no places to fill (an operator word).
    Leaf(Node),
    /// A structure with empty places, and the place the caret goes to.
    Node(Node, usize),
    /// Accepted and dropped (`\left`, `\right`).
    Nothing,
}

/// The meaning of a typed LaTeX command name.
fn latex_cmd(name: &str) -> Option<Cmd> {
    use crate::linear;
    let plain = |t: &str| Node::Styled { text: t.into(), style: crate::Style::Plain };
    Some(match name {
        "frac" | "dfrac" | "tfrac" | "cfrac" => Cmd::Node(Node::Frac { num: vec![], den: vec![] }, 0),
        "sqrt" => Cmd::Node(Node::Rad { deg: None, body: vec![] }, 0),
        "left" | "right" => Cmd::Nothing,
        "infty" => Cmd::Sym('\u{221E}'),
        "lim" => Cmd::Node(Node::Limit { lower: true, base: vec![plain("lim")], lim: vec![] }, 1),
        _ => {
            if let Some((op, limits)) = linear::nary_op(name) {
                Cmd::Node(Node::Nary { op, sub: limits.then(Vec::new), sup: limits.then(Vec::new), body: vec![] }, 0)
            } else if linear::is_bare_func(name) {
                Cmd::Node(Node::Func { name: name.into(), body: vec![] }, 0)
            } else if linear::is_function_word(name) {
                Cmd::Leaf(plain(name))
            } else if let Some(ch) = linear::accent(name) {
                Cmd::Node(Node::Accent { ch, body: vec![] }, 0)
            } else {
                Cmd::Sym(linear::symbol(name)?)
            }
        }
    })
}

/// True when some longer command name starts with `name` (`in` is also the start of `int`).
fn has_longer_cmd(name: &str) -> bool {
    const EXTRA: [&str; 7] = ["frac", "dfrac", "tfrac", "cfrac", "sqrt", "left", "right"];
    crate::linear::command_names().chain(EXTRA).any(|n| n.len() > name.len() && n.starts_with(name))
}

/// A math tree being edited with a caret and an optional selection.
#[derive(Debug, Clone)]
pub struct Editor {
    math: Math,
    caret: Caret,
    /// Other end of the selection, a position in the caret's own sequence.
    anchor: Option<usize>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    /// A command or script just made a place: a `{` typed next opens its argument (LaTeX style).
    expect_brace: bool,
    /// Unclosed argument braces typed so far; a `}` closes the innermost one.
    brace_depth: u32,
    /// The radical's degree place was opened by a typed `[` after `\sqrt`, so a `]` closes it.
    deg_typed: bool,
}

/// Most edits kept for undo.
const MAX_HISTORY: usize = 200;

/// A state to go back to.
#[derive(Debug, Clone)]
struct Snap {
    math: Math,
    caret: Caret,
    anchor: Option<usize>,
}

impl Editor {
    /// Edit `math` with the caret at the end of the top row.
    pub fn new(math: Math) -> Editor {
        let mut e =
            Editor { math, caret: Caret::default(), anchor: None, undo: vec![], redo: vec![], expect_brace: false, brace_depth: 0, deg_typed: false };
        e.caret.pos = units(&e.math.body);
        e
    }

    pub fn math(&self) -> &Math {
        &self.math
    }

    pub fn math_mut(&mut self) -> &mut Math {
        &mut self.math
    }

    pub fn into_math(self) -> Math {
        self.math
    }

    pub fn caret(&self) -> &Caret {
        &self.caret
    }

    /// Replace the whole equation (the caret goes to the end).
    pub fn set_math(&mut self, m: Math) {
        *self = Editor::new(m);
    }

    /// The equation as LaTeX (see [`crate::to_latex`]).
    pub fn latex(&self) -> String {
        crate::to_latex(&self.math)
    }

    /// Replace the whole equation from typed text or LaTeX. Like [`Editor::set_math`] the caret
    /// goes to the end and the history starts over, so a host can load a value without leaving
    /// an undo step behind.
    pub fn set_latex(&mut self, src: &str) {
        self.set_math(crate::from_linear(src));
    }

    /// True when the caret is at the very start of the equation (no enclosing slot, position 0).
    /// A host uses this to hand an Up or Left key to the next field.
    pub fn at_start(&self) -> bool {
        self.caret.path.is_empty() && self.caret.pos == 0
    }

    /// True when the caret is at the very end of the top-level equation.
    pub fn at_end(&self) -> bool {
        self.caret.path.is_empty() && self.caret.pos >= units(&self.math.body)
    }

    /// Put the caret at the start of the top row, dropping any selection. A pending `\name` is
    /// left as typed.
    pub fn move_to_start(&mut self) {
        self.set_caret(Caret::default());
    }

    /// Put the caret at the end of the top row, dropping any selection. A pending `\name` is
    /// left as typed.
    pub fn move_to_end(&mut self) {
        let pos = units(&self.math.body);
        self.set_caret(Caret { path: vec![], pos });
    }

    /// True when nothing has been typed.
    pub fn is_blank(&self) -> bool {
        units(&self.math.body) == 0
    }

    /// The selected range in the caret's sequence, if any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.caret.pos).then(|| (a.min(self.caret.pos), a.max(self.caret.pos)))
    }

    /// The sequence the caret is in.
    pub fn seq(&self) -> &Seq {
        seq_at(&self.math.body, &self.caret.path).unwrap_or(&self.math.body)
    }

    fn seq_mut(&mut self) -> Option<&mut Seq> {
        seq_at_mut(&mut self.math.body, &self.caret.path)
    }

    fn len_here(&self) -> usize {
        units(self.seq())
    }

    /// Repair a caret that no longer points into the tree.
    fn fix(&mut self) {
        if seq_at(&self.math.body, &self.caret.path).is_none() {
            self.caret = Caret { path: vec![], pos: units(&self.math.body) };
            self.anchor = None;
        }
        let n = self.len_here();
        self.caret.pos = self.caret.pos.min(n);
        self.anchor = self.anchor.map(|a| a.min(n));
    }

    /// Put the caret at `c` (clamped into the tree), dropping any selection.
    pub fn set_caret(&mut self, c: Caret) {
        self.expect_brace = false;
        self.brace_depth = 0;
        self.caret = c;
        self.anchor = None;
        self.fix();
    }

    /// Move the caret to `c` keeping the selection anchor, when `c` is in the same sequence
    /// (a mouse drag). Otherwise the caret just moves there.
    pub fn drag_caret(&mut self, c: Caret) {
        self.fix();
        if c.path == self.caret.path {
            let anchor = self.anchor.unwrap_or(self.caret.pos);
            self.caret.pos = c.pos;
            self.anchor = Some(anchor);
            self.fix();
        } else {
            self.set_caret(c);
        }
    }

    /// Finish a `\name` still being typed (`\in` waits for a longer name): call before reading
    /// the result.
    pub fn flush(&mut self) {
        self.tracked(|e| e.flush_cmd());
    }

    /// The fixed end of the selection as a caret (the caret itself when nothing is selected).
    pub fn anchor_caret(&self) -> Caret {
        Caret { path: self.caret.path.clone(), pos: self.anchor.unwrap_or(self.caret.pos) }
    }

    /// Select from caret `a` to caret `b`, wherever they are: when they are in different rows
    /// the selection covers whole structures of the nearest row that holds both (a mouse drag
    /// or a shift-click).
    pub fn select_between(&mut self, a: &Caret, b: &Caret) {
        self.expect_brace = false;
        self.brace_depth = 0;
        let k = a.path.iter().zip(&b.path).take_while(|(x, y)| x == y).count();
        let Some(seq) = a.path.get(..k).and_then(|p| seq_at(&self.math.body, p)) else {
            self.set_caret(b.clone());
            return;
        };
        let end = units(seq);
        let spot = |c: &Caret| match c.path.get(k) {
            Some(&(i, _)) => (start_of(seq, i).min(end), true),
            None => (c.pos.min(end), false),
        };
        let ((pa, na), (pb, nb)) = (spot(a), spot(b));
        let (anchor, caret) = if pa < pb || (pa == pb && nb) { (pa, pb + usize::from(nb)) } else { (pa + usize::from(na), pb) };
        self.caret = Caret { path: a.path.get(..k).map(<[_]>::to_vec).unwrap_or_default(), pos: caret };
        self.anchor = (anchor != caret).then_some(anchor);
        self.fix();
    }

    /// Select the whole top row.
    pub fn select_all(&mut self) {
        self.caret = Caret { path: vec![], pos: units(&self.math.body) };
        self.anchor = Some(0);
    }

    fn can_nest(&self) -> bool {
        self.caret.path.len() + 8 < MAX_DEPTH
    }

    // ---------- typing ----------

    /// Type text: every character as if typed on the keyboard.
    pub fn type_str(&mut self, s: &str) {
        self.tracked(|e| {
            for c in s.chars().take(MAX_TYPED) {
                e.type_char_raw(c);
            }
        });
    }

    /// Type one character.
    pub fn type_char(&mut self, c: char) {
        self.tracked(|e| e.type_char_raw(c));
    }

    /// Run an edit and, when it changed the tree, make it one undo step.
    fn tracked(&mut self, f: impl FnOnce(&mut Self)) {
        let snap = Snap { math: self.math.clone(), caret: self.caret.clone(), anchor: self.anchor };
        f(self);
        if snap.math != self.math {
            self.undo.push(snap);
            if self.undo.len() > MAX_HISTORY {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn snap(&self) -> Snap {
        Snap { math: self.math.clone(), caret: self.caret.clone(), anchor: self.anchor }
    }

    fn restore(&mut self, s: Snap) {
        self.math = s.math;
        self.caret = s.caret;
        self.anchor = s.anchor;
        self.fix();
    }

    /// Step back one edit. False when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        let cur = self.snap();
        self.redo.push(cur);
        self.restore(prev);
        true
    }

    /// Redo an undone edit. False when there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        let cur = self.snap();
        self.undo.push(cur);
        self.restore(next);
        true
    }

    /// The selection as linear text (for Copy), or `None` with no selection.
    pub fn selected_linear(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        let mut tmp = self.clone();
        let body = tmp.take_range(a, b);
        let m = Math { body, ..Math::default() };
        Some(crate::to_linear(&m))
    }

    /// Cut: the selection as linear text, removed from the tree.
    pub fn cut(&mut self) -> Option<String> {
        let t = self.selected_linear()?;
        self.key(Key::Delete, false);
        Some(t)
    }

    fn type_char_raw(&mut self, c: char) {
        self.fix();
        if !c.is_ascii_alphabetic() {
            self.flush_cmd();
        }
        let expect = std::mem::take(&mut self.expect_brace);
        if c == '[' && expect && self.open_degree() {
            return;
        }
        if c == ']' && self.deg_typed && self.in_degree() {
            self.deg_typed = false;
            self.leave_right();
            self.expect_brace = true;
            return;
        }
        if c == '{' && expect {
            self.brace_depth = self.brace_depth.saturating_add(1);
            return;
        }
        if c == '}' && self.brace_depth > 0 && !self.in_brace_delim() {
            self.brace_depth -= 1;
            self.close_arg();
            return;
        }
        // `x^2` ends after its digits when anything but another digit, `.`, `^` or `_` follows
        // (`mv^2/2` is a fraction of `mv^2`, `x^2sin x` is `x^2` times `sin x`). Ported from
        // DeckCraft 7972cf5; not inside a typed LaTeX `{...}` argument.
        if !(c.is_ascii_digit() || matches!(c, '.' | '^' | '_') || c.is_whitespace() || c.is_control())
            && self.brace_depth == 0
            && self.selection().is_none()
            && self.script_slot_here()
            && self.caret.pos == self.len_here()
            && self.slot_is_digits()
        {
            self.exit_script_slot();
        }
        match c {
            '\\' => {
                self.leave_func_arg();
                self.insert_char('\\');
            }
            '^' => self.script(true),
            '_' => self.script(false),
            '/' => self.fraction(),
            '(' => {
                if !self.convert_call() {
                    self.open_delim("(", ")");
                }
            }
            '[' => self.open_delim("[", "]"),
            '{' => self.open_delim("{", "}"),
            ')' | ']' | '}' => {
                if !self.close_delim(c) {
                    self.insert_char(c);
                }
            }
            '-' => {
                if !self.replace_prev(&[('+', '\u{B1}')]) {
                    self.leave_func_arg();
                    self.insert_char('\u{2212}');
                }
            }
            '*' => {
                self.leave_func_arg();
                self.insert_char('\u{B7}');
            }
            '=' => {
                if !self.replace_prev(&[('<', '\u{2264}'), ('>', '\u{2265}'), ('!', '\u{2260}'), ('~', '\u{2248}')]) {
                    self.leave_func_arg();
                    self.insert_char('=');
                }
            }
            '>' => {
                if !self.replace_prev(&[('\u{2212}', '\u{2192}')]) {
                    self.leave_func_arg();
                    self.insert_char('>');
                }
            }
            c if c.is_whitespace() => {
                self.leave_nary_limit(false);
            }
            c if c.is_control() => {}
            c if ARG_ENDERS.contains(c) => {
                self.leave_func_arg();
                self.insert_char(c);
            }
            'h' if self.extend_func_name() => {}
            c => {
                self.insert_char(c);
                if c.is_ascii_alphabetic() && !self.latex_step() {
                    self.autoconvert();
                }
            }
        }
    }

    // ---------- typed LaTeX commands ----------

    /// The `\name` being typed right before the caret: where its backslash is and the name.
    fn pending_cmd(&self) -> Option<(usize, String)> {
        if self.selection().is_some() {
            return None;
        }
        let cls = unit_chars(self.seq());
        let pos = self.caret.pos.min(cls.len());
        let mut s = pos;
        while s > 0 && matches!(cls.get(s - 1), Some(Some(c)) if c.is_ascii_alphabetic()) {
            s -= 1;
        }
        if s == pos || s == 0 || cls.get(s - 1) != Some(&Some('\\')) {
            return None;
        }
        let name: String = cls.get(s..pos)?.iter().filter_map(|c| *c).collect();
        Some((s - 1, name))
    }

    /// After a letter of a `\name`: make it right away when nothing longer could be meant
    /// (`\alpha`, `\frac`), else wait for the next character. True while a command is pending.
    fn latex_step(&mut self) -> bool {
        let Some((bs, name)) = self.pending_cmd() else { return false };
        if latex_cmd(&name).is_some() && !has_longer_cmd(&name) {
            self.commit_cmd(bs, &name);
        }
        true
    }

    /// A character that cannot continue a command name ends a pending one (`\in` then a space).
    fn flush_cmd(&mut self) {
        self.flush_word(true);
    }

    /// [`Self::flush_cmd`], with or without turning a finished `inf` into the infinity sign.
    fn flush_word(&mut self, inf: bool) {
        if let Some((bs, name)) = self.pending_cmd() {
            if latex_cmd(&name).is_some() {
                self.commit_cmd(bs, &name);
            }
        } else if inf {
            self.flush_inf();
        }
    }

    /// The word `inf`, finished (by any other character, or by `flush`), becomes the infinity
    /// sign. It waits that long because `infty` starts with it.
    fn flush_inf(&mut self) {
        if self.selection().is_some() {
            return;
        }
        let cls = unit_chars(self.seq());
        let pos = self.caret.pos.min(cls.len());
        let mut s = pos;
        while s > 0 && matches!(cls.get(s - 1), Some(Some(c)) if c.is_ascii_alphabetic()) {
            s -= 1;
        }
        let run: String = cls.get(s..pos).unwrap_or(&[]).iter().filter_map(|c| *c).collect();
        if run == "inf" && (s == 0 || cls.get(s - 1) != Some(&Some('\\'))) {
            self.delete_range(s, pos);
            self.insert_char('\u{221E}');
        }
    }

    /// A `[` typed right after `\sqrt`: the radical gets a degree place, which a `]` closes.
    fn open_degree(&mut self) -> bool {
        if self.selection().is_some() || self.len_here() != 0 {
            return false;
        }
        let Some(&(i, c)) = self.caret.path.last() else { return false };
        let mut parent = self.caret.path.clone();
        parent.pop();
        let Some(Node::Rad { deg, body }) = seq_at_mut(&mut self.math.body, &parent).and_then(|s| s.get_mut(i)).map(bare_mut) else {
            return false;
        };
        if deg.is_some() || c != 0 || !body.is_empty() {
            return false;
        }
        *deg = Some(vec![]);
        self.deg_typed = true;
        true
    }

    /// The caret is in a radical's degree place.
    fn in_degree(&self) -> bool {
        let Some(&(i, c)) = self.caret.path.last() else { return false };
        let parent = self.caret.path.get(..self.caret.path.len() - 1).and_then(|p| seq_at(&self.math.body, p));
        c == 0 && matches!(parent.and_then(|s| s.get(i)).map(Node::bare), Some(Node::Rad { deg: Some(_), .. }))
    }

    /// Replace the typed `\name` (from `bs` to the caret) by what it stands for.
    fn commit_cmd(&mut self, bs: usize, name: &str) {
        let Some(cmd) = latex_cmd(name) else { return };
        let pos = self.caret.pos;
        if matches!(cmd, Cmd::Node(..)) && !self.can_nest() {
            return;
        }
        self.delete_range(bs, pos);
        match cmd {
            Cmd::Sym(c) => self.insert_char(c),
            Cmd::Nothing => {}
            Cmd::Leaf(node) => {
                if self.insert_nodes(vec![node]).is_some() {
                    self.caret.pos += 1;
                }
            }
            Cmd::Node(node, child) => {
                if let Some(i) = self.insert_nodes(vec![node]) {
                    self.caret.path.push((i, child));
                    self.caret.pos = 0;
                    self.expect_brace = true;
                }
            }
        }
    }

    /// The innermost place is a `{ }` delimiter pair's content (so a `}` closes that).
    fn in_brace_delim(&self) -> bool {
        let Some(&(i, _)) = self.caret.path.last() else { return false };
        let parent = self.caret.path.get(..self.caret.path.len() - 1).and_then(|p| seq_at(&self.math.body, p));
        matches!(parent.and_then(|s| s.get(i)).map(Node::bare), Some(Node::Delim { close, .. }) if close == "}")
    }

    /// A `}` that ends a LaTeX-style argument: on to the next place of a fraction, otherwise out
    /// of the structure. A big operator's limit stays put so `\sum_{i=1}^{n}` can add the other.
    fn close_arg(&mut self) {
        self.anchor = None;
        let Some(&(i, c)) = self.caret.path.last() else { return };
        let parent = self.caret.path.get(..self.caret.path.len() - 1).and_then(|p| seq_at(&self.math.body, p));
        match parent.and_then(|s| s.get(i)).map(Node::bare) {
            // The other limit may still follow (`\sum_{i=1}^{n}`), but the last one is done.
            Some(Node::Nary { sub, sup, .. }) if c + 1 < usize::from(sub.is_some()) + usize::from(sup.is_some()) => {}
            Some(Node::Frac { .. }) if c == 0 => {
                self.leave_right();
                self.expect_brace = true;
            }
            _ => self.leave_right(),
        }
    }

    /// The character just before the caret, when there is no selection.
    fn prev_char(&self) -> Option<char> {
        if self.selection().is_some() {
            return None;
        }
        let cls = unit_chars(self.seq());
        cls.get(self.caret.pos.checked_sub(1)?).copied().flatten()
    }

    /// If the character before the caret is one of `pairs`' first members, replace it by the
    /// second (`+` then `-` makes a plus-minus sign, `<` then `=` makes `<=` as one symbol).
    fn replace_prev(&mut self, pairs: &[(char, char)]) -> bool {
        let Some(p) = self.prev_char() else { return false };
        let Some(&(_, to)) = pairs.iter().find(|(from, _)| *from == p) else { return false };
        let pos = self.caret.pos;
        self.delete_range(pos - 1, pos);
        self.insert_char(to);
        true
    }

    /// The function node whose argument the caret is in.
    fn func_here(&self) -> Option<(usize, usize)> {
        let &(i, c) = self.caret.path.last()?;
        let parent = seq_at(&self.math.body, self.caret.path.get(..self.caret.path.len() - 1)?)?;
        matches!(parent.get(i)?.bare(), Node::Func { .. }).then_some((i, c))
    }

    /// The script node whose superscript or subscript the caret is in.
    fn script_slot_here(&self) -> bool {
        let Some(&(i, c)) = self.caret.path.last() else { return false };
        let Some(parent) = self.caret.path.get(..self.caret.path.len() - 1).and_then(|p| seq_at(&self.math.body, p)) else {
            return false;
        };
        c >= 1 && matches!(parent.get(i).map(Node::bare), Some(Node::Script { .. }))
    }

    /// The script slot the caret is in holds only digits (`x^2`): the next term ends it.
    fn slot_is_digits(&self) -> bool {
        let cls = unit_chars(self.seq());
        !cls.is_empty() && cls.iter().all(|c| matches!(c, Some(d) if d.is_ascii_digit() || *d == '.'))
    }

    /// Step out of the script slot the caret is in, to just after the script.
    fn exit_script_slot(&mut self) {
        if let Some((i, _)) = self.caret.path.pop() {
            self.caret.pos = start_of(self.seq(), i) + 1;
        }
    }

    /// A filled, unbraced limit of a big operator is over at a space (any limit) or at an
    /// operator (the upper limit only: `sum_i=1^n` keeps its `=`): the caret goes on to the
    /// operand, so `sum_(i=1)^n i^2` does not pour the summand into the upper limit. True when
    /// the caret moved.
    fn leave_nary_limit(&mut self, operator: bool) -> bool {
        if self.brace_depth > 0 || self.selection().is_some() {
            return false;
        }
        let Some(&(i, c)) = self.caret.path.last() else { return false };
        if self.len_here() == 0 || self.caret.pos != self.len_here() {
            return false;
        }
        let parent = self.caret.path.get(..self.caret.path.len() - 1).and_then(|p| seq_at(&self.math.body, p));
        let Some(Node::Nary { sub, sup, .. }) = parent.and_then(|s| s.get(i)).map(Node::bare) else { return false };
        let limits = usize::from(sub.is_some()) + usize::from(sup.is_some());
        let upper = sup.is_some() && c + 1 == limits;
        if c >= limits || (operator && !upper) {
            return false;
        }
        self.caret.path.pop();
        self.caret.path.push((i, limits));
        self.caret.pos = 0;
        true
    }

    /// Typing an operator ends an unparenthesised function argument (`sin x+1` is `sin x`, plus
    /// 1) and leaves a filled exponent or subscript (`x^2+1` is `x` squared, plus 1).
    fn leave_func_arg(&mut self) {
        for _ in 0..MAX_DEPTH {
            if self.selection().is_some() {
                return;
            }
            let n = self.len_here();
            if n == 0 || self.caret.pos != n {
                return;
            }
            if self.func_here().is_some() {
                self.leave_right();
            } else if self.script_slot_here() {
                // A digit exponent ends at the operator (`x^2+1`); a letter exponent keeps it
                // (`x^n+1` is `x^(n+1)`), as in DeckCraft 7972cf5.
                if !self.slot_is_digits() {
                    return;
                }
                self.exit_script_slot();
            } else {
                self.leave_nary_limit(true);
                return;
            }
        }
    }

    /// `h` right after a trigonometric name (empty argument) makes its hyperbolic form:
    /// `sinh`, `cosh`, `tanh`, `csch`, `sech`, `coth`, `arcsinh`, `arccosh`, `arctanh`.
    fn extend_func_name(&mut self) -> bool {
        if self.selection().is_some() || self.len_here() != 0 {
            return false;
        }
        let Some((i, _)) = self.func_here() else { return false };
        let mut parent = self.caret.path.clone();
        parent.pop();
        let Some(Node::Func { name, .. }) = seq_at_mut(&mut self.math.body, &parent).and_then(|s| s.get_mut(i)).map(bare_mut) else {
            return false;
        };
        if matches!(name.as_str(), "sin" | "cos" | "tan" | "csc" | "sec" | "cot" | "arcsin" | "arccos" | "arctan") {
            // `csc`, `sec` and `cot` take an `h` to become `csch`, `sech` and `coth`.
            name.push('h');
            true
        } else {
            false
        }
    }

    /// `(` `[` `{`: a delimiter pair with the caret inside (around the selection, if any).
    fn open_delim(&mut self, open: &str, close: &str) {
        if !self.can_nest() {
            self.insert_char(open.chars().next().unwrap_or('('));
            return;
        }
        let wrapped = self.selection();
        let items = match wrapped {
            Some((a, b)) => self.take_range(a, b),
            None => vec![],
        };
        let node = Node::Delim { open: open.into(), close: close.into(), sep: "|".into(), items: vec![items] };
        let Some(i) = self.insert_nodes(vec![node]) else { return };
        self.anchor = None;
        if wrapped.is_some() {
            self.caret.pos += 1;
        } else {
            self.caret.path.push((i, 0));
            self.caret.pos = 0;
        }
    }

    /// `)` `]` `}`: step out past the delimiter pair that this character closes, when the caret
    /// is at the end of its contents (possibly inside a root or script that ends there).
    fn close_delim(&mut self, ch: char) -> bool {
        if self.selection().is_some() {
            return false;
        }
        let mut path = self.caret.path.clone();
        let mut pos = self.caret.pos;
        for _ in 0..=MAX_DEPTH {
            let Some(seq) = seq_at(&self.math.body, &path) else { return false };
            if pos != units(seq) {
                return false;
            }
            let Some((i, c)) = path.pop() else { return false };
            let Some(parent) = seq_at(&self.math.body, &path) else { return false };
            let Some(node) = parent.get(i) else { return false };
            let after = start_of(parent, i) + 1;
            if let Node::Delim { close, .. } = node.bare()
                && close.chars().eq(std::iter::once(ch))
            {
                self.caret = Caret { path, pos: after };
                self.anchor = None;
                self.unwrap_frac_paren();
                return true;
            }
            if c + 1 != children(node).len() {
                return false;
            }
            pos = after;
        }
        false
    }

    /// A fraction's numerator or denominator, a radicand, a script or a limit that is just one
    /// `( )` group shows no parentheses (the structure already groups): typing `(x-1)` in a
    /// denominator reads like the numerator does, and `sqrt(b^2-4ac)` and `_(i=1)` match.
    fn unwrap_frac_paren(&mut self) {
        let Some(&(pi, _)) = self.caret.path.last() else { return };
        let Some(&(_, c)) = self.caret.path.last() else { return };
        let parent = self.caret.path.get(..self.caret.path.len() - 1).and_then(|p| seq_at(&self.math.body, p));
        let node = parent.and_then(|s| s.get(pi)).map(Node::bare);
        let in_script = matches!(node, Some(Node::Script { .. })) && c >= 1;
        let groups = match node {
            Some(Node::Frac { .. } | Node::Rad { .. } | Node::Func { .. }) => true,
            Some(Node::Script { .. }) => in_script,
            Some(Node::Nary { sub, sup, .. }) => c < usize::from(sub.is_some()) + usize::from(sup.is_some()),
            Some(Node::Limit { .. }) => c == 1,
            _ => false,
        };
        if !groups {
            return;
        }
        let last = node.is_some_and(|n| c + 1 == children(n).len());
        let ends = matches!(node, Some(Node::Rad { .. } | Node::Func { .. } | Node::Frac { .. })) && last;
        if let Some(seq) = self.seq_mut() {
            let term = std::mem::take(seq);
            *seq = unwrap_paren(term);
        }
        self.caret.pos = self.len_here();
        // `x^(n+1)`: the exponent is the group, so closing it also ends the exponent (DeckCraft
        // 7972cf5). The same for the argument of `sin(..)`, a radicand (`sqrt(x)+1`) and a
        // denominator (`1/(x-1)+2`): the closing parenthesis ends the structure, as in Desmos.
        if in_script || ends {
            self.exit_script_slot();
        }
    }

    /// Insert a symbol from the palette, replacing the selection. No name conversion.
    pub fn insert_symbol(&mut self, c: char) {
        self.tracked(|e| {
            e.fix();
            e.insert_char(c);
        });
    }

    fn take_range(&mut self, a: usize, b: usize) -> Vec<Node> {
        self.anchor = None;
        let out = match self.seq_mut() {
            Some(seq) => {
                let i = boundary(seq, a);
                let j = boundary(seq, b).min(seq.len());
                let i = i.min(j);
                seq.drain(i..j).collect()
            }
            None => vec![],
        };
        self.caret.pos = a;
        out
    }

    fn delete_range(&mut self, a: usize, b: usize) {
        let _ = self.take_range(a, b);
        if let Some(seq) = self.seq_mut() {
            merge(seq);
        }
    }

    fn drop_selection(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.delete_range(a, b);
        }
        self.anchor = None;
    }

    /// Insert nodes at the caret; the index of the first one.
    fn insert_nodes(&mut self, nodes: Vec<Node>) -> Option<usize> {
        let pos = self.caret.pos;
        let seq = self.seq_mut()?;
        let i = boundary(seq, pos).min(seq.len());
        seq.splice(i..i, nodes);
        Some(i)
    }

    fn insert_char(&mut self, c: char) {
        self.drop_selection();
        let pos = self.caret.pos;
        let Some(seq) = self.seq_mut() else { return };
        let i = boundary(seq, pos).min(seq.len());
        let prev = i.checked_sub(1).and_then(|p| seq.get_mut(p));
        if let Some(Node::Text(t)) = prev {
            t.push(c);
        } else if let Some(Node::Text(t)) = seq.get_mut(i) {
            t.insert(0, c);
        } else {
            seq.insert(i, Node::Text(c.to_string()));
        }
        self.caret.pos = pos + 1;
    }

    /// Where the term before the caret starts: just the last atom (`atom`, for a script base) or
    /// the whole run of letters, digits and structures (for a numerator).
    fn term_start(&self, atom: bool) -> usize {
        let cls = unit_chars(self.seq());
        let pos = self.caret.pos.min(cls.len());
        if pos == 0 {
            return 0;
        }
        if atom {
            return match cls.get(pos - 1) {
                Some(Some(c)) if c.is_ascii_digit() => {
                    let mut s = pos - 1;
                    while s > 0 && matches!(cls.get(s - 1), Some(Some(d)) if d.is_ascii_digit() || *d == '.') {
                        s -= 1;
                    }
                    s
                }
                _ => pos - 1,
            };
        }
        let mut s = pos;
        while s > 0 {
            match cls.get(s - 1) {
                Some(None) => s -= 1,
                Some(Some(c)) if c.is_alphanumeric() || *c == '.' || *c == '\u{2032}' => s -= 1,
                _ => break,
            }
        }
        s
    }

    /// `_` or `^` inside a big operator: in its empty body they add the limit (`int_0`), and
    /// in the end of one limit they jump to the other (`sum_i^n`, `int_0^1`).
    fn nary_limit(&mut self, sup: bool) -> bool {
        if self.selection().is_some() {
            return false;
        }
        let Some(&(i, c)) = self.caret.path.last() else { return false };
        let at_end = self.caret.pos == self.len_here();
        let empty = self.len_here() == 0;
        let mut parent = self.caret.path.clone();
        parent.pop();
        let Some(Node::Nary { sub, sup: up, .. }) = seq_at_mut(&mut self.math.body, &parent).and_then(|s| s.get_mut(i)).map(bare_mut) else {
            return false;
        };
        let body_idx = usize::from(sub.is_some()) + usize::from(up.is_some());
        let in_sub = sub.is_some() && c == 0;
        let in_sup = up.is_some() && c == usize::from(sub.is_some());
        let go = if c == body_idx && empty {
            true
        } else if sup {
            in_sub && at_end
        } else {
            in_sup && at_end
        };
        if !go {
            return false;
        }
        let idx = if sup {
            if up.is_none() {
                *up = Some(vec![]);
            }
            usize::from(sub.is_some())
        } else {
            if sub.is_none() {
                *sub = Some(vec![]);
            }
            0
        };
        self.caret.path.pop();
        self.caret.path.push((i, idx));
        self.caret.pos = self.len_here();
        true
    }

    fn script(&mut self, sup: bool) {
        self.script_raw(sup);
        self.expect_brace = true;
    }

    /// `_` right after `lim` (or `^` in an empty upper limit...): the place is already there, so
    /// typing it must not nest a script with an empty base inside it.
    fn redundant_script(&self, sup: bool) -> bool {
        if self.selection().is_some() || self.len_here() != 0 {
            return false;
        }
        let Some(&(i, c)) = self.caret.path.last() else { return false };
        let parent = self.caret.path.get(..self.caret.path.len() - 1).and_then(|p| seq_at(&self.math.body, p));
        match parent.and_then(|s| s.get(i)).map(Node::bare) {
            Some(Node::Limit { lower, .. }) => c == 1 && *lower != sup,
            Some(Node::Nary { sub, sup: up, .. }) => {
                if sup {
                    up.is_some() && c == usize::from(sub.is_some())
                } else {
                    sub.is_some() && c == 0
                }
            }
            _ => false,
        }
    }

    fn script_raw(&mut self, sup: bool) {
        if !self.can_nest() {
            return;
        }
        if self.redundant_script(sup) || self.nary_limit(sup) {
            return;
        }
        let base = if let Some((a, b)) = self.selection() {
            self.take_range(a, b)
        } else {
            if self.extend_script_before(sup) {
                return;
            }
            let (s, p) = (self.term_start(true), self.caret.pos);
            self.take_range(s, p)
        };
        let node = Node::Script { base, sub: (!sup).then(Vec::new), sup: sup.then(Vec::new) };
        let Some(i) = self.insert_nodes(vec![node]) else { return };
        self.caret.path.push((i, 1));
        self.caret.pos = 0;
        self.anchor = None;
    }

    /// The caret sits right after a script: add (or re-enter) the wanted part of it.
    fn extend_script_before(&mut self, sup: bool) -> bool {
        let pos = self.caret.pos;
        let Some(seq) = self.seq_mut() else { return false };
        let i = boundary(seq, pos).min(seq.len());
        let Some(p) = i.checked_sub(1) else { return false };
        let Some(Node::Script { sub, sup: up, .. }) = seq.get_mut(p).map(bare_mut) else { return false };
        if sup && up.is_none() {
            *up = Some(vec![]);
        } else if !sup && sub.is_none() {
            *sub = Some(vec![]);
        }
        let idx = if sup { 1 + usize::from(sub.is_some()) } else { 1 };
        self.caret.path.push((p, idx));
        self.caret.pos = self.len_here();
        self.anchor = None;
        true
    }

    fn fraction(&mut self) {
        if !self.can_nest() {
            return;
        }
        let num = if let Some((a, b)) = self.selection() {
            self.take_range(a, b)
        } else {
            let (s, p) = (self.term_start(false), self.caret.pos);
            unwrap_paren(self.take_range(s, p))
        };
        let filled = units(&num) > 0;
        let Some(i) = self.insert_nodes(vec![Node::Frac { num, den: vec![] }]) else { return };
        self.caret.path.push((i, usize::from(filled)));
        self.caret.pos = 0;
        self.anchor = None;
    }

    /// After a letter: turn a just-completed name (`alpha`, `sqrt`...) into its symbol.
    fn autoconvert(&mut self) {
        let cls = unit_chars(self.seq());
        let pos = self.caret.pos.min(cls.len());
        let mut s = pos;
        while s > 0 && matches!(cls.get(s - 1), Some(Some(c)) if c.is_ascii_alphabetic()) {
            s -= 1;
        }
        let run: String = cls.get(s..pos).unwrap_or(&[]).iter().filter_map(|c| *c).collect();
        if run.ends_with("sqrt") && self.can_nest() {
            self.delete_range(pos - 4, pos);
            if let Some(i) = self.insert_nodes(vec![Node::Rad { deg: None, body: vec![] }]) {
                self.caret.path.push((i, 0));
                self.caret.pos = 0;
            }
            return;
        }
        if self.convert_word(&run, pos) {
            return;
        }
        // `ipi` (as in `e^ipi`) is `i` then `pi` (DeckCraft 7972cf5); other short names only
        // convert as whole words.
        let short_ok = run.len() == 3 && run.ends_with("pi");
        let suffix = || GREEK.iter().filter(|(n, _)| (n.len() >= 4 || (short_ok && *n == "pi")) && run.ends_with(n)).max_by_key(|(n, _)| n.len());
        let exact = GREEK.iter().find(|(n, _)| *n == run);
        if let Some((name, sym)) = exact.or_else(suffix) {
            let n = name.chars().count();
            self.delete_range(pos - n, pos);
            self.insert_char(*sym);
        }
    }

    /// `floor(`, `nCr(`: a function name typed just before `(` becomes a function node with the
    /// caret in its argument, and the `(` is that argument's own.
    fn convert_call(&mut self) -> bool {
        if self.selection().is_some() || !self.can_nest() {
            return false;
        }
        let cls = unit_chars(self.seq());
        let pos = self.caret.pos.min(cls.len());
        let mut s = pos;
        while s > 0 && matches!(cls.get(s - 1), Some(Some(c)) if c.is_ascii_alphabetic()) {
            s -= 1;
        }
        let run: String = cls.get(s..pos).unwrap_or(&[]).iter().filter_map(|c| *c).collect();
        if !crate::funcs::is_call_only(&run) {
            return false;
        }
        self.delete_range(s, pos);
        let Some(i) = self.insert_nodes(vec![Node::Func { name: run, body: vec![] }]) else { return true };
        self.caret.path.push((i, 0));
        self.caret.pos = 0;
        true
    }

    /// A whole typed word (`pm`, `sin`, `sum`, `lim`...) becomes its symbol or structure.
    fn convert_word(&mut self, run: &str, pos: usize) -> bool {
        let n = run.chars().count();
        if let Some((_, sym)) = WORDS.iter().find(|(w, _)| *w == run) {
            self.delete_range(pos - n, pos);
            self.insert_char(*sym);
            return true;
        }
        let node = if crate::funcs::BARE.contains(&run) {
            Some((Node::Func { name: run.to_string(), body: vec![] }, 0))
        } else if let Some(&(_, op, limits)) =
            [("sum", '\u{2211}', true), ("prod", '\u{220F}', true), ("int", '\u{222B}', false)].iter().find(|(w, ..)| *w == run)
        {
            Some((Node::Nary { op, sub: limits.then(Vec::new), sup: limits.then(Vec::new), body: vec![] }, 0))
        } else if run == "lim" {
            Some((Node::Limit { lower: true, base: vec![Node::Styled { text: "lim".into(), style: crate::Style::Plain }], lim: vec![] }, 1))
        } else {
            None
        };
        let Some((node, child)) = node else { return false };
        if !self.can_nest() {
            return false;
        }
        self.delete_range(pos - n, pos);
        let Some(i) = self.insert_nodes(vec![node]) else { return true };
        self.caret.path.push((i, child));
        self.caret.pos = 0;
        true
    }

    // ---------- templates ----------

    /// Insert a palette structure at the caret. A script, fraction or accent takes the term
    /// before the caret (or the selection) as its base; other structures wrap the selection
    /// into their first blank. The caret lands in the first blank place.
    pub fn insert_template(&mut self, t: &Math) {
        self.tracked(|e| e.insert_template_raw(t));
    }

    fn insert_template_raw(&mut self, t: &Math) {
        self.fix();
        if !self.can_nest() {
            return;
        }
        let mut nodes = t.body.clone();
        strip_blanks(&mut nodes, 0);
        if nodes.is_empty() {
            return;
        }
        let single = nodes.len() == 1;
        // The part that takes what is before the caret, if the structure has one.
        let taker = match nodes.first().map(Node::bare) {
            Some(Node::Script { base, .. }) if single && base.is_empty() => Some((true, false)),
            Some(Node::Accent { body, .. } | Node::Bar { body, .. } | Node::GroupChr { body, .. }) if single && body.is_empty() => Some((true, true)),
            Some(Node::Frac { num, den }) if single && num.is_empty() && den.is_empty() => Some((false, false)),
            _ => None,
        };
        if let Some((atom, close)) = taker {
            let taken = if let Some((a, b)) = self.selection() {
                self.take_range(a, b)
            } else {
                let (s, p) = (self.term_start(atom), self.caret.pos);
                let t = self.take_range(s, p);
                if atom { t } else { unwrap_paren(t) }
            };
            let filled = units(&taken) > 0;
            if let Some(n) = nodes.first_mut()
                && let Some(first) = children_mut(bare_mut(n)).into_iter().next()
            {
                *first = taken;
            }
            let Some(i) = self.insert_nodes(nodes) else { return };
            if close && filled {
                self.caret.pos += 1;
            } else {
                // First place still blank: scripts go to their script, a fraction to its
                // denominator, an empty accent into its body.
                let c = usize::from(!(close && !filled) && (filled || atom));
                self.caret.path.push((i, c));
                self.caret.pos = 0;
            }
            self.anchor = None;
            return;
        }
        let selected = self.selection().map(|(a, b)| self.take_range(a, b)).unwrap_or_default();
        let target = first_empty_skipping_prescript(&nodes);
        let mut land = units(&selected);
        if let Some(rel) = &target
            && !selected.is_empty()
            && let Some(slot) = seq_at_mut(&mut nodes, rel)
        {
            *slot = selected;
        } else {
            land = 0;
        }
        let count = units(&nodes);
        let Some(i) = self.insert_nodes(nodes) else { return };
        match target {
            Some(mut rel) => {
                if let Some(first) = rel.first_mut() {
                    first.0 += i;
                }
                self.caret.path.extend(rel);
                self.caret.pos = land;
            }
            None => self.caret.pos += count,
        }
        self.anchor = None;
    }

    // ---------- keys ----------

    /// Press a navigation or deletion key; `shift` extends the selection (Left, Right, Home, End).
    pub fn key(&mut self, key: Key, shift: bool) {
        if !matches!(key, Key::Undo | Key::Redo) {
            let inf = !matches!(key, Key::Backspace | Key::Delete);
            self.tracked(|e| e.flush_word(inf));
        }
        if !matches!(key, Key::Backspace | Key::Delete) {
            self.expect_brace = false;
            self.brace_depth = 0;
        }
        match key {
            Key::Undo => {
                let _ = self.undo();
            }
            Key::Redo => {
                let _ = self.redo();
            }
            Key::Backspace | Key::Delete => self.tracked(|e| e.key_raw(key, shift)),
            _ => self.key_raw(key, shift),
        }
    }

    fn key_raw(&mut self, key: Key, shift: bool) {
        self.fix();
        match key {
            Key::Undo | Key::Redo => {}
            Key::Tab => {
                self.anchor = None;
                self.tab(shift);
            }
            Key::Backspace => self.backspace(),
            Key::Delete => self.delete_forward(),
            Key::Left | Key::Right | Key::Home | Key::End if shift => self.extend(key),
            Key::Left | Key::Right => {
                if let Some((a, b)) = self.selection() {
                    self.caret.pos = if key == Key::Left { a } else { b };
                    self.anchor = None;
                } else {
                    self.anchor = None;
                    if key == Key::Left { self.move_left() } else { self.move_right() }
                }
            }
            Key::Up | Key::Down => {
                self.anchor = None;
                self.move_vertical(key == Key::Up);
            }
            Key::Home | Key::End => {
                self.anchor = None;
                let to_start = key == Key::Home;
                let edge = if to_start { 0 } else { self.len_here() };
                if self.caret.pos == edge {
                    self.caret.path.clear();
                    self.caret.pos = if to_start { 0 } else { units(&self.math.body) };
                } else {
                    self.caret.pos = edge;
                }
            }
        }
        self.fix();
    }

    /// Move to the next (or previous) place of the tree: the next cell, the denominator after the
    /// numerator, out of the last place. Stays put at the ends.
    fn tab(&mut self, back: bool) {
        let start = self.caret.path.clone();
        let from_prescript = !back && self.in_prescript();
        for _ in 0..MAX_TAB_STEPS {
            let before = self.caret.clone();
            if back {
                self.move_left();
            } else {
                self.move_right();
            }
            if self.caret.path != start || self.caret == before {
                break;
            }
        }
        // Out of the n of `ₙC_r`: straight into the r.
        if from_prescript
            && self.caret.path.len() + 1 == start.len()
            && let Some((i, false, kids, true)) = self.unit_info(self.caret.pos)
            && kids >= 2
            && self.caret.path.len() < MAX_DEPTH
        {
            self.caret.path.push((i, 1));
            self.caret.pos = 0;
        }
    }

    /// The caret is in a script of a script node whose base is empty (the n of `ₙC_r`).
    fn in_prescript(&self) -> bool {
        let Some((&(i, c), parent)) = self.caret.path.split_last() else { return false };
        c > 0
            && seq_at(&self.math.body, parent)
                .and_then(|s| s.get(i))
                .is_some_and(|n| matches!(n.bare(), Node::Script { base, .. } if base.is_empty()))
    }

    fn extend(&mut self, key: Key) {
        let len = self.len_here();
        let pos = self.caret.pos;
        let to = match key {
            Key::Left => pos.saturating_sub(1),
            Key::Right => (pos + 1).min(len),
            Key::Home => 0,
            _ => len,
        };
        if self.anchor.is_none() {
            self.anchor = Some(pos);
        }
        self.caret.pos = to;
        if self.anchor == Some(to) {
            self.anchor = None;
        }
    }

    /// Facts about the unit at `pos`: (node index, is text, child count, is a script).
    fn unit_info(&self, pos: usize) -> Option<(usize, bool, usize, bool)> {
        let seq = self.seq();
        let (i, _) = locate(seq, pos)?;
        let n = seq.get(i)?;
        Some((i, text_of(n).is_some(), children(n).len(), matches!(n.bare(), Node::Script { .. })))
    }

    fn move_right(&mut self) {
        let pos = self.caret.pos;
        if pos < self.len_here() {
            match self.unit_info(pos) {
                Some((i, false, kids, script)) if kids > 0 && self.caret.path.len() < MAX_DEPTH => {
                    self.caret.path.push((i, 0));
                    self.caret.pos = 0;
                    if script {
                        // Over the base letter, not in front of it (that is where we came from).
                        self.caret.pos = self.len_here().min(1);
                    }
                    self.enter_lone_grid();
                }
                _ => self.caret.pos = pos + 1,
            }
        } else {
            self.leave_right();
        }
    }

    /// Caret at the start of a delimiter pair that holds only a grid: step into its first cell.
    fn enter_lone_grid(&mut self) {
        let grid = matches!(self.seq().as_slice(), [only] if matches!(only.bare(), Node::Matrix { .. } | Node::EqArr { .. }));
        if grid && self.caret.pos == 0 && self.caret.path.len() < MAX_DEPTH {
            self.caret.path.push((0, 0));
        }
    }

    fn move_left(&mut self) {
        let pos = self.caret.pos;
        if pos > 0 {
            match self.unit_info(pos - 1) {
                Some((i, false, kids, _)) if kids > 0 && self.caret.path.len() < MAX_DEPTH => {
                    self.caret.path.push((i, kids - 1));
                    self.caret.pos = self.len_here();
                    if self.only_grid_in_delim()
                        && let Some(k) = self.seq().first().map(|n| children(n).len()).filter(|k| *k > 0)
                    {
                        self.caret.path.push((0, k - 1));
                        self.caret.pos = self.len_here();
                    }
                }
                _ => self.caret.pos = pos - 1,
            }
        } else {
            self.leave_left();
        }
    }

    /// True when the caret's sequence holds just one grid (matrix or equation array) inside a
    /// delimiter pair, so the caret position before or after the grid looks the same as the
    /// grid's own edge and is skipped.
    fn only_grid_in_delim(&self) -> bool {
        let seq = self.seq();
        let grid = matches!(seq.as_slice(), [only] if matches!(only.bare(), Node::Matrix { .. } | Node::EqArr { .. }));
        let Some(&(i, _)) = self.caret.path.last() else { return false };
        let parent = self.caret.path.get(..self.caret.path.len() - 1).and_then(|p| seq_at(&self.math.body, p));
        grid && parent.and_then(|s| s.get(i)).is_some_and(|n| matches!(n.bare(), Node::Delim { .. }))
    }

    fn leave_right(&mut self) {
        let Some((i, c)) = self.caret.path.pop() else { return };
        let kids = self.seq().get(i).map_or(0, |n| children(n).len());
        if c + 1 < kids {
            self.caret.path.push((i, c + 1));
            self.caret.pos = 0;
        } else {
            self.caret.pos = start_of(self.seq(), i) + 1;
            if self.caret.pos == self.len_here() && self.only_grid_in_delim() {
                self.leave_right();
            }
        }
    }

    fn leave_left(&mut self) {
        let Some((i, c)) = self.caret.path.pop() else { return };
        if c > 0 {
            self.caret.path.push((i, c - 1));
            self.caret.pos = self.len_here();
        } else {
            self.caret.pos = start_of(self.seq(), i);
            if self.caret.pos == 0 && self.only_grid_in_delim() {
                self.leave_left();
            }
        }
    }

    fn move_vertical(&mut self, up: bool) {
        let mut path = self.caret.path.clone();
        let pos = self.caret.pos;
        while let Some((i, c)) = path.pop() {
            let next = seq_at(&self.math.body, &path).and_then(|s| s.get(i)).and_then(|n| vertical_neighbor(n, c, up));
            if let Some(nc) = next {
                path.push((i, nc));
                let n = seq_at(&self.math.body, &path).map_or(0, |s| units(s));
                self.caret = Caret { path, pos: pos.min(n) };
                return;
            }
        }
        self.caret = Caret { path: vec![], pos: if up { 0 } else { units(&self.math.body) } };
    }

    fn backspace(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.delete_range(a, b);
            return;
        }
        self.anchor = None;
        let pos = self.caret.pos;
        if pos > 0 {
            // Behind a structure with something in it: step inside first (the next Backspace
            // deletes from its end), so one key never wipes a whole fraction.
            if self.solid_structure(pos - 1) {
                let script = self.unit_info(pos - 1).is_some_and(|u| u.3);
                self.move_left();
                // A script has one visible tail, so the same key removes its last character
                // (stepping in with no visible change reads as a dead key).
                let n = self.len_here();
                if script && n > 0 && !self.solid_structure(n - 1) {
                    self.delete_range(n - 1, n);
                }
            } else {
                self.delete_range(pos - 1, pos);
            }
        } else {
            self.dissolve();
        }
    }

    /// True when the unit at `pos` is a structure that holds something (so deleting it would
    /// lose content).
    fn solid_structure(&self, pos: usize) -> bool {
        let Some((i, false, kids, _)) = self.unit_info(pos) else { return false };
        kids > 0 && self.caret.path.len() < MAX_DEPTH && self.seq().get(i).is_some_and(|n| children(n).iter().any(|k| units(k) > 0))
    }

    fn delete_forward(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.delete_range(a, b);
            return;
        }
        self.anchor = None;
        let pos = self.caret.pos;
        if pos < self.len_here() {
            if self.solid_structure(pos) {
                self.move_right();
            } else {
                self.delete_range(pos, pos + 1);
            }
        } else {
            let _ = self.remove_if_empty();
        }
    }

    /// The structure the caret is in, when every one of its places is empty: remove it.
    fn remove_if_empty(&mut self) -> bool {
        let Some(&(i, _)) = self.caret.path.last() else { return false };
        let mut parent_path = self.caret.path.clone();
        parent_path.pop();
        let empty = seq_at(&self.math.body, &parent_path).and_then(|s| s.get(i)).is_some_and(|n| children(n).iter().all(|k| units(k) == 0));
        if !empty {
            return false;
        }
        self.caret.path = parent_path;
        let start = start_of(self.seq(), i);
        if let Some(seq) = self.seq_mut() {
            if i < seq.len() {
                seq.remove(i);
            }
            merge(seq);
        }
        self.caret.pos = start;
        true
    }

    /// Backspace at the start of a place: remove an empty structure, drop an empty script part,
    /// unwrap a structure into its contents, or step out to the left.
    fn dissolve(&mut self) {
        let Some(&(i, c)) = self.caret.path.last() else { return };
        if self.remove_if_empty() {
            return;
        }
        let mut parent_path = self.caret.path.clone();
        parent_path.pop();
        let facts = seq_at(&self.math.body, &parent_path).and_then(|s| s.get(i)).map(|n| {
            let kids = children(n);
            (matches!(n.bare(), Node::Script { .. }), kids.get(c).is_none_or(|k| units(k) == 0), kids.iter().take(c).map(|k| units(k)).sum::<usize>())
        });
        let Some((script, this_empty, before)) = facts else { return };
        if script {
            if c > 0 && this_empty {
                self.drop_script_part(parent_path, i, c);
            } else {
                self.leave_left();
            }
        } else if c == 0 || this_empty {
            self.unwrap_node(parent_path, i, before);
        } else {
            self.leave_left();
        }
    }

    fn drop_script_part(&mut self, parent_path: Path, i: usize, c: usize) {
        let Some(seq) = seq_at_mut(&mut self.math.body, &parent_path) else { return };
        let Some(Node::Script { base, sub, sup }) = seq.get_mut(i).map(bare_mut) else { return };
        if sub.is_some() && c == 1 {
            *sub = None;
        } else {
            *sup = None;
        }
        let base_units = units(base);
        if sub.is_none() && sup.is_none() {
            let flat = std::mem::take(base);
            let start = start_of(seq, i);
            seq.splice(i..=i, flat);
            merge(seq);
            self.caret = Caret { path: parent_path, pos: start + base_units };
        } else {
            self.caret.path = parent_path;
            self.caret.path.push((i, 0));
            self.caret.pos = base_units;
        }
    }

    fn unwrap_node(&mut self, parent_path: Path, i: usize, before: usize) {
        let Some(seq) = seq_at_mut(&mut self.math.body, &parent_path) else { return };
        if i >= seq.len() {
            return;
        }
        let mut node = seq.remove(i);
        let mut flat: Vec<Node> = vec![];
        for k in children_mut(&mut node) {
            flat.extend(std::mem::take(k));
        }
        let start = start_of(seq, i);
        seq.splice(i..i, flat);
        merge(seq);
        self.caret = Caret { path: parent_path, pos: start + before };
    }

    /// The tree for display: every empty place holds a box character so it can be seen and
    /// clicked. The edited tree itself is untouched.
    pub fn display_math(&self) -> Math {
        fn fill(seq: &mut Seq, depth: usize) {
            if depth > MAX_DEPTH {
                return;
            }
            let len = seq.len();
            for (i, n) in seq.iter_mut().enumerate() {
                // A script with nothing before it and something after (the n of `ₙC_r`) has no
                // base to show: only its scripts are places.
                let prescript = i + 1 < len && matches!(n, Node::Script { base, sub, sup } if base.is_empty() && (sub.is_some() || sup.is_some()));
                for (c, child) in children_mut(n).into_iter().enumerate() {
                    if units(child) == 0 {
                        if !(prescript && c == 0) {
                            *child = vec![Node::Text("\u{25A1}".into())];
                        }
                    } else {
                        fill(child, depth + 1);
                    }
                }
            }
        }
        let mut m = self.math.clone();
        fill(&mut m.body, 0);
        m
    }

    /// True when the caret points at a real place of the tree (tests, debug).
    pub fn is_valid(&self) -> bool {
        seq_at(&self.math.body, &self.caret.path).is_some_and(|s| self.caret.pos <= units(s))
    }
}

#[cfg(test)]
mod tests;
