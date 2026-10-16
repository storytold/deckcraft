//! Bounded expanded-name comment XML, independent of caller-chosen namespace prefixes.
use crate::PptxError;
use quick_xml::{
    events::Event,
    name::{QName, ResolveResult},
    reader::NsReader,
};
use std::collections::BTreeMap;
pub(crate) type Result<T> = std::result::Result<T, PptxError>;
pub(crate) const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub(crate) const PS: &str = "http://purl.oclc.org/ooxml/presentationml/main";
pub(crate) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub(crate) const AS: &str = "http://purl.oclc.org/ooxml/drawingml/main";
pub(crate) const M: &str = "http://schemas.microsoft.com/office/powerpoint/2018/8/main";
pub(crate) const PC: &str = "http://schemas.microsoft.com/office/powerpoint/2013/main/command";
pub(crate) const AC: &str = "http://schemas.microsoft.com/office/drawing/2013/main/command";
pub(crate) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const XML: &str = "http://www.w3.org/XML/1998/namespace";
const MAX: usize = 64 * 1024 * 1024;
const MAX_ATTRIBUTE: usize = 8192;
fn error(message: &str) -> PptxError {
    PptxError::Xml(format!("comments: {message}"))
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Name {
    pub ns: String,
    pub local: String,
}
#[derive(Clone, Debug)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}
#[derive(Clone, Debug)]
pub(crate) struct Element {
    pub name: Name,
    pub attrs: BTreeMap<Name, String>,
    pub children: Vec<Node>,
}
impl Element {
    pub fn is(&self, ns: &str, local: &str) -> bool {
        self.name.ns == ns && self.name.local == local
    }
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(&Name { ns: String::new(), local: name.into() }).map(String::as_str)
    }
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(el) => Some(el),
            Node::Text(_) => None,
        })
    }
    pub fn one(&self, ns: &str, name: &str) -> Result<Option<&Element>> {
        let mut matching = self.elements().filter(|el| el.is(ns, name));
        let first = matching.next();
        if matching.next().is_some() {
            return Err(error("duplicate singleton element"));
        }
        Ok(first)
    }
    pub fn scalar(&self) -> Result<String> {
        let mut out = String::new();
        for node in &self.children {
            match node {
                Node::Element(_) => return Err(error("structured scalar")),
                Node::Text(text) => {
                    let length = out.len().checked_add(text.len()).ok_or_else(|| error("scalar length overflow"))?;
                    if length > 1024 * 1024 {
                        return Err(error("scalar exceeds limit"));
                    }
                    out.try_reserve(text.len()).map_err(|_| error("cannot allocate scalar"))?;
                    out.push_str(text);
                }
            }
        }
        Ok(out)
    }
    pub fn legacy(&self) -> crate::xml::El {
        fn name(name: &Name) -> String {
            let prefix = match name.ns.as_str() {
                A | AS => "a",
                P | PS => "p",
                M => "p188",
                PC => "pc",
                AC => "ac",
                R => "r",
                XML => "xml",
                "http://schemas.openxmlformats.org/officeDocument/2006/math" => "m",
                "http://schemas.microsoft.com/office/drawing/2010/main" => "a14",
                "http://schemas.microsoft.com/office/powerpoint/2010/main" => "p14",
                "" => return name.local.clone(),
                _ => "foreign",
            };
            format!("{prefix}:{}", name.local)
        }
        crate::xml::El {
            name: name(&self.name),
            attrs: self
                .attrs
                .iter()
                .map(|(n, v)| (name(n), v.clone()))
                .chain([
                    ("xmlns:a".into(), A.into()),
                    ("xmlns:p".into(), P.into()),
                    ("xmlns:p188".into(), M.into()),
                    ("xmlns:r".into(), R.into()),
                    ("xmlns:m".into(), "http://schemas.openxmlformats.org/officeDocument/2006/math".into()),
                    ("xmlns:a14".into(), "http://schemas.microsoft.com/office/drawing/2010/main".into()),
                    ("xmlns:p14".into(), "http://schemas.microsoft.com/office/powerpoint/2010/main".into()),
                ])
                .collect(),
            children: self
                .children
                .iter()
                .map(|node| match node {
                    Node::Element(el) => crate::xml::Node::El(el.legacy()),
                    Node::Text(text) => crate::xml::Node::Text(text.clone()),
                })
                .collect(),
        }
    }
}
#[derive(Default)]
pub(crate) struct Budget {
    bytes: usize,
    nodes: usize,
    source: usize,
    pub model_modern: deckcraft_model::comments::Budget,
    pub model_legacy: deckcraft_model::comments::Budget,
}
impl Budget {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(|| error("allocation accounting overflow"))?;
        self.nodes = self.nodes.checked_add(1).ok_or_else(|| error("work accounting overflow"))?;
        if self.bytes > MAX || self.nodes > 1_000_000 {
            return Err(error("XML allocation/work limit exceeded"));
        }
        Ok(())
    }
}
fn expanded(reader: &NsReader<&[u8]>, q: QName<'_>, attribute: bool) -> Result<Name> {
    let (ns, local) = if attribute { reader.resolver().resolve_attribute(q) } else { reader.resolver().resolve_element(q) };
    let ns = match ns {
        ResolveResult::Bound(ns) => {
            if ns.as_ref().len() > 4096 {
                return Err(error("namespace exceeds limit"));
            }
            std::str::from_utf8(ns.as_ref()).map_err(|_| error("non-UTF8 namespace"))?.to_string()
        }
        ResolveResult::Unbound => String::new(),
        ResolveResult::Unknown(_) => return Err(error("unbound namespace")),
    };
    if local.as_ref().len() > 1024 {
        return Err(error("local name exceeds limit"));
    }
    let local = std::str::from_utf8(local.as_ref()).map_err(|_| error("non-UTF8 local name"))?.to_string();
    if ns.len() > 4096 || local.len() > 1024 {
        return Err(error("name exceeds limit"));
    }
    Ok(Name { ns, local })
}
pub(crate) fn parse(bytes: &[u8], budget: &mut Budget) -> Result<Element> {
    budget.source = budget.source.checked_add(bytes.len()).ok_or_else(|| error("source size overflow"))?;
    if bytes.len() > 32 * 1024 * 1024 || budget.source > MAX {
        return Err(error("XML source exceeds limit"));
    }
    let mut decoded = String::new();
    let source = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes.starts_with(&[0xff, 0xfe]);
        let body = bytes.get(2..).ok_or_else(|| error("short UTF16 source"))?;
        if body.len() % 2 != 0 {
            return Err(error("malformed UTF16"));
        }
        let words = body.as_chunks::<2>().0.iter().map(|pair| if little { u16::from_le_bytes(*pair) } else { u16::from_be_bytes(*pair) });
        for ch in char::decode_utf16(words) {
            let ch = ch.map_err(|_| error("malformed UTF16"))?;
            if decoded.len().saturating_add(ch.len_utf8()) > 32 * 1024 * 1024 {
                return Err(error("decoded XML exceeds limit"));
            }
            decoded.try_reserve(ch.len_utf8()).map_err(|_| error("cannot allocate decoded XML"))?;
            decoded.push(ch);
        }
        let start = decoded.strip_prefix("<?xml").and_then(|rest| rest.find("?>").and_then(|at| at.checked_add(7))).unwrap_or(0);
        decoded.get(start..).ok_or_else(|| error("bad encoding declaration"))?.as_bytes()
    } else {
        bytes
    };
    let mut reader = NsReader::from_reader(source);
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    fn append(stack: &mut [Element], root: &mut Option<Element>, element: Element) -> Result<()> {
        if let Some(parent) = stack.last_mut() {
            parent.children.try_reserve(1).map_err(|_| error("cannot allocate XML children"))?;
            parent.children.push(Node::Element(element));
        } else if root.replace(element).is_some() {
            return Err(error("multiple XML roots"));
        }
        Ok(())
    }
    let mut at_start = true;
    loop {
        let event = reader.read_event().map_err(|e| PptxError::Xml(e.to_string()))?;
        let first_event = std::mem::replace(&mut at_start, false);
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                if stack.len() >= 64 {
                    return Err(error("XML nesting exceeds limit"));
                }
                budget.charge(256)?;
                let name = expanded(&reader, start.name(), false)?;
                budget.charge(name.ns.len().saturating_add(name.local.len()).saturating_mul(2))?;
                let mut attrs = BTreeMap::new();
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(|e| PptxError::Xml(e.to_string()))?;
                    if attribute.key.as_ref() == b"xmlns" || attribute.key.as_ref().starts_with(b"xmlns:") {
                        continue;
                    }
                    // One decoded byte may occupy six bytes as an XML entity.
                    // Bound both forms so escaping never shortens the supported profile.
                    if attrs.len() >= 128 || attribute.value.len() > MAX_ATTRIBUTE * 6 {
                        return Err(error("XML attributes exceed limit"));
                    }
                    let name = expanded(&reader, attribute.key, true)?;
                    let value = attribute
                        .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|e| PptxError::Xml(e.to_string()))?;
                    if value.len() > MAX_ATTRIBUTE {
                        return Err(error("decoded XML attribute exceeds limit"));
                    }
                    budget.charge(
                        128usize.saturating_add(value.len().saturating_mul(2)).saturating_add(name.ns.len()).saturating_add(name.local.len()),
                    )?;
                    if attrs.insert(name, value.into_owned()).is_some() {
                        return Err(error("duplicate expanded attribute"));
                    }
                }
                let element = Element { name, attrs, children: Vec::new() };
                if matches!(event, Event::Start(_)) {
                    stack.try_reserve(1).map_err(|_| error("cannot allocate XML stack"))?;
                    stack.push(element);
                } else {
                    append(&mut stack, &mut root, element)?;
                }
            }
            Event::End(_) => {
                let element = stack.pop().ok_or_else(|| error("unbalanced XML"))?;
                append(&mut stack, &mut root, element)?;
            }
            Event::Text(text) => {
                let text = text.xml_content(quick_xml::XmlVersion::Implicit1_0).map_err(|e| PptxError::Xml(e.to_string()))?;
                push_text(&mut stack, text.as_ref(), budget)?;
            }
            Event::CData(text) => {
                let text = text.xml_content(quick_xml::XmlVersion::Implicit1_0).map_err(|e| PptxError::Xml(e.to_string()))?;
                push_text(&mut stack, text.as_ref(), budget)?;
            }
            Event::GeneralRef(reference) => {
                let text = if reference.is_char_ref() {
                    reference
                        .resolve_char_ref()
                        .map_err(|e| PptxError::Xml(e.to_string()))?
                        .ok_or_else(|| error("bad character reference"))?
                        .to_string()
                } else {
                    match reference.decode().map_err(|e| PptxError::Xml(e.to_string()))?.as_ref() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        _ => return Err(error("unknown entity")),
                    }
                };
                push_text(&mut stack, &text, budget)?;
            }
            Event::Decl(_) if !first_event => {
                return Err(error("XML declaration must occur only at document start"));
            }
            Event::DocType(_) => return Err(error("document types are unsupported")),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(error("truncated XML"));
    }
    root.ok_or_else(|| error("missing root"))
}
fn push_text(stack: &mut [Element], text: &str, budget: &mut Budget) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        budget.charge(text.len().saturating_mul(2).saturating_add(64))?;
        parent.children.try_reserve(1).map_err(|_| error("cannot allocate XML text"))?;
        parent.children.push(Node::Text(text.into()));
    } else if !text.trim().is_empty() {
        return Err(error("text outside root"));
    }
    Ok(())
}

pub(crate) struct Xml(pub String);
impl Xml {
    pub fn new() -> Self {
        Self(String::new())
    }
    pub fn raw(&mut self, text: &str) -> Result<()> {
        let length = self.0.len().checked_add(text.len()).ok_or_else(|| error("XML output overflow"))?;
        if length > MAX {
            return Err(error("XML output exceeds limit"));
        }
        self.0.try_reserve(text.len()).map_err(|_| error("cannot allocate XML output"))?;
        self.0.push_str(text);
        Ok(())
    }
    pub fn escaped(&mut self, text: &str, attribute: bool) -> Result<()> {
        for ch in text.chars() {
            let fixed = match ch {
                '&' => Some("&amp;"),
                '<' => Some("&lt;"),
                '>' => Some("&gt;"),
                '"' if attribute => Some("&quot;"),
                '\r' => Some("&#13;"),
                '\n' if attribute => Some("&#10;"),
                '\t' if attribute => Some("&#9;"),
                _ => None,
            };
            if let Some(fixed) = fixed {
                self.raw(fixed)?;
            } else {
                if !matches!(ch, '\t' | '\n') && (ch < ' ' || matches!(ch, '\u{FFFE}' | '\u{FFFF}')) {
                    return Err(error("invalid XML character"));
                }
                let mut bytes = [0u8; 4];
                self.raw(ch.encode_utf8(&mut bytes))?;
            }
        }
        Ok(())
    }
    pub fn attr(&mut self, key: &str, value: &str) -> Result<()> {
        self.raw(" ")?;
        self.raw(key)?;
        self.raw("=\"")?;
        self.escaped(value, true)?;
        self.raw("\"")
    }
}

/// Injective expanded-name XML projection. Relationship identities bind their actual target,
/// type and external flag, independent of caller prefixes and relationship allocation order.
pub(crate) fn body_projection(body: &Element, rels: &crate::opc::Rels) -> Result<String> {
    fn node(el: &Element, rels: &crate::opc::Rels, out: &mut Xml, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(error("body projection nesting exceeds limit"));
        }
        out.raw("<e")?;
        out.attr("ns", &el.name.ns)?;
        out.attr("name", &el.name.local)?;
        out.raw(">")?;
        for (name, value) in &el.attrs {
            out.raw("<a")?;
            out.attr("ns", &name.ns)?;
            out.attr("name", &name.local)?;
            let value = if name.ns == R {
                let rel = rels.get(value).ok_or_else(|| error("rich body relationship target missing"))?;
                serde_json::to_string(&(&rel.ty, &rel.target, rel.external)).map_err(|_| error("relationship projection failed"))?
            } else {
                value.clone()
            };
            out.attr("value", &value)?;
            out.raw("/>")?;
        }
        for child in &el.children {
            match child {
                Node::Element(child) => node(child, rels, out, depth + 1)?,
                Node::Text(text) => {
                    out.raw("<t>")?;
                    out.escaped(text, false)?;
                    out.raw("</t>")?;
                }
            }
        }
        out.raw("</e>")
    }
    let mut out = Xml::new();
    node(body, rels, &mut out, 0)?;
    if out.0.len() > 8 * 1024 * 1024 {
        return Err(error("body projection exceeds limit"));
    }
    Ok(out.0)
}
