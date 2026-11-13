//! A small, bounded XML DOM over quick-xml for SVG documents.
//!
//! Elements are matched by local name, so prefixed documents (`svg:rect`) read the same as plain
//! ones. Depth and node count are capped because picture data is hostile input.

use std::sync::Arc;

use quick_xml::XmlVersion;
use quick_xml::events::Event;

/// Maximum element nesting accepted.
pub const MAX_DEPTH: usize = 64;
/// Maximum number of elements accepted.
pub const MAX_NODES: usize = 200_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    /// Local name (`rect`, `linearGradient`).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Arc<Node>>,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// Parse the document and return its root element.
pub fn parse(bytes: &[u8]) -> Option<Arc<Node>> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let mut reader = quick_xml::Reader::from_reader(bytes);
    {
        let c = reader.config_mut();
        c.trim_text(true);
        c.check_end_names = false;
        c.allow_unmatched_ends = true;
        c.expand_empty_elements = false;
    }
    let mut stack: Vec<Node> = vec![];
    let mut root: Option<Node> = None;
    let mut nodes = 0usize;
    let mut buf = Vec::new();
    loop {
        let ev = reader.read_event_into(&mut buf).ok()?;
        match ev {
            Event::Start(b) => {
                nodes += 1;
                if nodes > MAX_NODES || stack.len() >= MAX_DEPTH {
                    return None;
                }
                stack.push(element(&b, &reader)?);
            }
            Event::Empty(b) => {
                nodes += 1;
                if nodes > MAX_NODES {
                    return None;
                }
                let node = element(&b, &reader)?;
                push(&mut stack, &mut root, node);
            }
            Event::End(_) => {
                if let Some(node) = stack.pop() {
                    push(&mut stack, &mut root, node);
                }
            }
            Event::Eof => break,
            // Text, comments, declarations and processing instructions carry nothing we draw.
            _ => {}
        }
        buf.clear();
    }
    while let Some(node) = stack.pop() {
        push(&mut stack, &mut root, node);
    }
    root.map(Arc::new)
}

fn push(stack: &mut [Node], root: &mut Option<Node>, node: Node) {
    match stack.last_mut() {
        Some(parent) => parent.kids.push(Arc::new(node)),
        None => {
            if root.is_none() {
                *root = Some(node);
            }
        }
    }
}

fn element(b: &quick_xml::events::BytesStart, reader: &quick_xml::Reader<&[u8]>) -> Option<Node> {
    let full = String::from_utf8_lossy(b.name().as_ref()).into_owned();
    let name = full.rsplit_once(':').map(|(_, l)| l).unwrap_or(&full).to_string();
    let mut attrs = Vec::new();
    for a in b.attributes().with_checks(false).flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        // Namespace declarations are not attributes the reader consults.
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit_once(':').map(|(_, l)| l).unwrap_or(&key).to_string();
        let value = a
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned());
        attrs.push((local, value));
    }
    Some(Node { name, attrs, kids: vec![] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_prefixed_elements_and_skips_namespace_declarations() {
        let root = parse(b"<svg:svg xmlns:svg='urn:s' viewBox='0 0 4 2'><svg:rect width='2' height='1'/></svg:svg>").expect("parses");
        assert_eq!(root.name, "svg");
        assert_eq!(root.attr("viewBox"), Some("0 0 4 2"));
        assert_eq!(root.kids.len(), 1);
        assert_eq!(root.kids[0].name, "rect");
        assert_eq!(root.kids[0].attr("width"), Some("2"));
        assert!(root.attr("xmlns:svg").is_none());
    }

    #[test]
    fn refuses_broken_or_oversized_documents() {
        assert!(parse(b"<svg><rect>").is_some(), "unclosed elements still read");
        assert!(parse(b"not xml at all").is_none());
        let deep = "<g>".repeat(MAX_DEPTH + 2);
        assert!(parse(format!("<svg>{deep}</svg>").as_bytes()).is_none());
    }
}
