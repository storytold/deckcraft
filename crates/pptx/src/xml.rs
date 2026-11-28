//! Bounded XML DOM wrapper over [`craft_drawingml::xmlx::parse`] for callers that read kept
//! XML fragments (such as 3-D shape settings).

use crate::PptxError;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    El(El),
    Text(String),
}

fn local_of(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

impl El {
    pub fn local(&self) -> &str {
        local_of(&self.name)
    }

    pub fn prefix(&self) -> &str {
        self.name.split_once(':').map_or("", |(p, _)| p)
    }

    pub fn is(&self, local: &str) -> bool {
        self.local() == local
    }

    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        self.write_xml(&mut out);
        out
    }

    fn write_xml(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            craft_xml::push_esc_attr(out, v);
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for c in &self.children {
            match c {
                Node::El(e) => e.write_xml(out),
                Node::Text(t) => craft_xml::push_esc(out, t),
            }
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push('>');
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name || local_of(k) == name).map(|(_, v)| v.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|n| match n {
            Node::El(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    pub fn child(&self, local: &str) -> Option<&El> {
        self.elements().find(|e| e.local() == local)
    }

    pub fn children_named<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.elements().filter(move |e| e.local() == local)
    }

    pub fn path(&self, path: &[&str]) -> Option<&El> {
        path.iter().try_fold(self, |cur, name| cur.child(name))
    }

    pub fn f64(&self, name: &str) -> Option<f64> {
        self.attr(name).and_then(|v| v.trim().parse::<f64>().ok())
    }

    pub fn find(&self, local: &str) -> Option<&El> {
        self.find_bounded(local, 0)
    }

    fn find_bounded(&self, local: &str, depth: usize) -> Option<&El> {
        if depth > 64 {
            return None;
        }
        for c in self.elements() {
            if c.local() == local {
                return Some(c);
            }
            if let Some(found) = c.find_bounded(local, depth + 1) {
                return Some(found);
            }
        }
        None
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        self.push_text(&mut out);
        out
    }

    fn push_text(&self, out: &mut String) {
        for c in &self.children {
            match c {
                Node::El(e) => e.push_text(out),
                Node::Text(t) => out.push_str(t),
            }
        }
    }
}

/// Attribute list builder for lightweight XML serialization (OMML fragments).
#[derive(Default)]
pub(crate) struct A(Vec<(String, String)>);

impl A {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn a(mut self, k: &str, v: impl std::fmt::Display) -> Self {
        self.0.push((k.to_owned(), v.to_string()));
        self
    }

    pub(crate) fn o(self, k: &str, v: Option<impl std::fmt::Display>) -> Self {
        match v {
            Some(v) => self.a(k, v),
            None => self,
        }
    }
}

/// Lightweight XML string writer for OMML fragments.
#[derive(Default)]
pub(crate) struct W(String);

impl W {
    pub(crate) fn new() -> Self {
        Self(String::new())
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }

    pub(crate) fn open(&mut self, tag: &str, attrs: A) {
        self.0.push('<');
        self.0.push_str(tag);
        for (k, v) in &attrs.0 {
            self.0.push(' ');
            self.0.push_str(k);
            self.0.push_str("=\"");
            craft_xml::push_esc_attr(&mut self.0, v);
            self.0.push('"');
        }
        self.0.push('>');
    }

    pub(crate) fn open0(&mut self, tag: &str) {
        self.0.push('<');
        self.0.push_str(tag);
        self.0.push('>');
    }

    pub(crate) fn close(&mut self, tag: &str) {
        self.0.push_str("</");
        self.0.push_str(tag);
        self.0.push('>');
    }

    pub(crate) fn empty(&mut self, tag: &str, attrs: A) {
        self.0.push('<');
        self.0.push_str(tag);
        for (k, v) in &attrs.0 {
            self.0.push(' ');
            self.0.push_str(k);
            self.0.push_str("=\"");
            craft_xml::push_esc_attr(&mut self.0, v);
            self.0.push('"');
        }
        self.0.push_str("/>");
    }

    pub(crate) fn empty0(&mut self, tag: &str) {
        self.0.push('<');
        self.0.push_str(tag);
        self.0.push_str("/>");
    }

    pub(crate) fn elt(&mut self, tag: &str, text: &str) {
        self.open0(tag);
        craft_xml::push_esc(&mut self.0, text);
        self.close(tag);
    }
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub root: El,
}

fn from_craft_el(el: craft_xml::El) -> El {
    let children = el
        .children
        .into_iter()
        .filter_map(|n| match n {
            craft_xml::Node::El(c) => Some(Node::El(from_craft_el(c))),
            craft_xml::Node::Text(t) | craft_xml::Node::CData(t) => Some(Node::Text(t)),
            craft_xml::Node::Pi { .. } | craft_xml::Node::Comment(_) => None,
        })
        .collect();
    El { name: el.name, attrs: el.attrs, children }
}

pub fn parse(bytes: &[u8]) -> Result<Doc, PptxError> {
    let root = craft_drawingml::xmlx::parse(bytes).map_err(|e| PptxError::Xml(e.to_string()))?;
    Ok(Doc { root: from_craft_el(root) })
}
