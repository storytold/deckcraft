//! ASCII FBX: `Name: value, value, … { children }`, with `;` comments to the end of the line.
//!
//! Values are numbers, quoted strings, bare words (`T`, `Y`…) and array markers (`*24`, whose
//! values follow as an `a:` child). A value list continues across lines after a comma.

use super::{Node, Prop};
use crate::{Error, Result, invalid};

const FORMAT: &str = "FBX";
const MAX_VALUES: usize = 256 << 20;
const MAX_NODES: usize = 4 << 20;

#[derive(Debug, PartialEq)]
enum Tok<'a> {
    /// `Name:`
    Key(&'a str),
    Str(&'a str),
    Word(&'a str),
    Comma,
    Open,
    Close,
}

struct Lexer<'a> {
    s: &'a str,
    at: usize,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Result<Option<Tok<'a>>> {
        let b = self.s.as_bytes();
        loop {
            match b.get(self.at) {
                None => return Ok(None),
                Some(c) if c.is_ascii_whitespace() => self.at += 1,
                Some(b';') => {
                    while b.get(self.at).is_some_and(|&c| c != b'\n') {
                        self.at += 1;
                    }
                }
                Some(_) => break,
            }
        }
        let start = self.at;
        let c = b.get(start).copied().unwrap_or(b' ');
        self.at += 1;
        Ok(Some(match c {
            b',' => Tok::Comma,
            b'{' => Tok::Open,
            b'}' => Tok::Close,
            b'"' => {
                let end = self.s.get(self.at..).and_then(|r| r.find('"')).ok_or_else(|| invalid(FORMAT, "a string is not closed"))?;
                let v = self.s.get(self.at..self.at + end).unwrap_or_default();
                self.at += end + 1;
                Tok::Str(v)
            }
            _ => {
                while b.get(self.at).is_some_and(|&c| !(c.is_ascii_whitespace() || matches!(c, b',' | b'{' | b'}' | b':' | b'"' | b';'))) {
                    self.at += 1;
                }
                let w = self.s.get(start..self.at).unwrap_or_default();
                if b.get(self.at) == Some(&b':') {
                    self.at += 1;
                    Tok::Key(w)
                } else {
                    Tok::Word(w)
                }
            }
        }))
    }
}

struct Parser<'a> {
    lex: Lexer<'a>,
    peeked: Option<Tok<'a>>,
    values: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn peek(&mut self) -> Result<Option<&Tok<'a>>> {
        if self.peeked.is_none() {
            self.peeked = self.lex.next()?;
        }
        Ok(self.peeked.as_ref())
    }

    fn take(&mut self) -> Result<Option<Tok<'a>>> {
        match self.peeked.take() {
            Some(t) => Ok(Some(t)),
            None => self.lex.next(),
        }
    }

    fn value(word: &str) -> Prop {
        if let Ok(i) = word.parse::<i64>() {
            Prop::Int(i)
        } else if let Some(f) = word.parse::<f64>().ok().filter(|f| f.is_finite()) {
            Prop::Float(f)
        } else {
            Prop::Str(word.to_string())
        }
    }

    /// Nodes until `}` (when `nested`) or the end of the text.
    fn list(&mut self, nested: bool, depth: usize) -> Result<Vec<Node>> {
        if depth > crate::MAX_DEPTH {
            return Err(invalid(FORMAT, "nodes nest too deeply"));
        }
        let mut out = Vec::new();
        loop {
            match self.take()? {
                None if nested => return Err(invalid(FORMAT, "a `{` is not closed")),
                None => return Ok(out),
                Some(Tok::Close) if nested => return Ok(out),
                Some(Tok::Key(name)) => {
                    self.nodes += 1;
                    if self.nodes > MAX_NODES {
                        return Err(Error::TooLarge("too many FBX nodes".into()));
                    }
                    let node = self.node(name, depth)?;
                    out.push(node);
                }
                Some(t) => return Err(invalid(FORMAT, format!("unexpected {t:?}"))),
            }
        }
    }

    fn node(&mut self, name: &str, depth: usize) -> Result<Node> {
        let mut props = Vec::new();
        // Values: v (, v)*
        let mut expect_value = true;
        loop {
            match self.peek()? {
                Some(Tok::Str(_) | Tok::Word(_)) if expect_value => {
                    self.values += 1;
                    if self.values > MAX_VALUES {
                        return Err(Error::TooLarge("FBX arrays hold too many values".into()));
                    }
                    match self.take()? {
                        Some(Tok::Str(s)) => props.push(Prop::Str(s.to_string())),
                        Some(Tok::Word(w)) if w.starts_with('*') => {}
                        Some(Tok::Word(w)) => props.push(Self::value(w)),
                        _ => {}
                    }
                    expect_value = false;
                }
                Some(Tok::Comma) => {
                    self.take()?;
                    expect_value = true;
                }
                _ => break,
            }
        }
        let children = if self.peek()? == Some(&Tok::Open) {
            self.take()?;
            self.list(true, depth + 1)?
        } else {
            Vec::new()
        };
        Ok(Node { name: name.to_string(), props, children })
    }
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Node>> {
    let text = String::from_utf8_lossy(bytes);
    let mut p = Parser { lex: Lexer { s: &text, at: 0 }, peeked: None, values: 0, nodes: 0 };
    p.list(false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_values_and_arrays() {
        let text = "; FBX 7.4.0 project file\nFBXHeaderExtension:  {\n FBXVersion: 7400\n}\nObjects:  {\n Geometry: 123, \"Geometry::Box\", \"Mesh\" {\n  Vertices: *6 {\n   a: 0,0,0,\n1.5,-2,3e1\n  }\n  Shading: T\n }\n}\n";
        let nodes = parse(text.as_bytes()).expect("parse");
        assert_eq!(nodes.len(), 2);
        let geom = &nodes[1].children[0];
        assert_eq!(geom.props, vec![Prop::Int(123), Prop::Str("Geometry::Box".into()), Prop::Str("Mesh".into())]);
        let a = &geom.children[0].children[0];
        assert_eq!(a.name, "a");
        assert_eq!(a.props.len(), 6);
        assert_eq!(a.props[5], Prop::Float(30.0));
        assert_eq!(geom.children[1].props, vec![Prop::Str("T".into())]);
    }

    #[test]
    fn broken_text_fails_cleanly() {
        for bad in ["A: {", "A: \"open", "}", "A: 1 B", "A: { B: { C: { }"] {
            assert!(parse(bad.as_bytes()).is_err(), "{bad}");
        }
        let deep = "A: {".repeat(200);
        assert!(parse(deep.as_bytes()).is_err());
    }
}
