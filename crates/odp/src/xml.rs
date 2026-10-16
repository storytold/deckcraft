//! Bounded, namespace-aware XML reader. Prefixes in input documents are arbitrary.
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};

use quick_xml::{
    events::Event,
    name::{QName, ResolveResult},
    reader::NsReader,
};

use crate::{OdpError, Result};

pub const NS: &[(&str, &str)] = &[
    ("xml", "http://www.w3.org/XML/1998/namespace"),
    ("office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
    ("style", "urn:oasis:names:tc:opendocument:xmlns:style:1.0"),
    ("draw", "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"),
    ("text", "urn:oasis:names:tc:opendocument:xmlns:text:1.0"),
    ("presentation", "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"),
    ("svg", "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"),
    ("fo", "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"),
    ("xlink", "http://www.w3.org/1999/xlink"),
    ("chart", "urn:oasis:names:tc:opendocument:xmlns:chart:1.0"),
    ("table", "urn:oasis:names:tc:opendocument:xmlns:table:1.0"),
    ("number", "urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0"),
    ("dc", "http://purl.org/dc/elements/1.1/"),
    ("meta", "urn:oasis:names:tc:opendocument:xmlns:meta:1.0"),
    ("smil", "urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0"),
    ("anim", "urn:oasis:names:tc:opendocument:xmlns:animation:1.0"),
    ("manifest", "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"),
    ("loext", "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0"),
];

/// Full namespace identity is independent of the canonical reader prefix registry.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExpandedName {
    /// Unqualified attributes have no namespace, even under a default namespace.
    pub namespace: Option<Arc<str>>,
    pub local: Arc<str>,
}

#[derive(Debug)]
pub struct ExpandedAttribute {
    pub name: ExpandedName,
    /// Lookup key in `El::attrs`; foreign keys remain distinct when local names agree.
    pub projected_key: String,
    /// Source attribute ordinal, including preceding namespace declarations.
    pub index: usize,
}

/// Additional identity/scope metadata is bounded separately from XML source and model data.
const MAX_IDENTITY_BYTES: usize = 32 * 1024 * 1024;
#[derive(Default)]
struct Identities {
    strings: BTreeSet<Arc<str>>,
    bytes: usize,
}
impl Identities {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_IDENTITY_BYTES {
            return Err(OdpError("XML namespace identity metadata exceeds size limit".into()));
        }
        Ok(())
    }
    fn intern(&mut self, value: &str) -> Result<Arc<str>> {
        if let Some(existing) = self.strings.get(value) {
            return Ok(existing.clone());
        }
        self.charge(value.len().saturating_add(64))?;
        let value: Arc<str> = value.into();
        self.strings.insert(value.clone());
        Ok(value)
    }
}

#[derive(Default, Debug)]
pub struct El {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub expanded_name: ExpandedName,
    pub expanded_attrs: Vec<ExpandedAttribute>,
    pub children: Vec<Node>,
    source: Option<Source>,
    identity_metadata_bytes: usize,
}

/// Shared source bytes and persistent namespace scopes avoid copying an ancestor's XML or
/// namespace strings into every descendant. Ranges include the element's own tags.
#[derive(Debug)]
struct Source {
    bytes: Arc<[u8]>,
    range: Range<usize>,
    scope: Option<Arc<Scope>>,
    part: Option<SourcePart>,
}

/// Package provenance is separate from XML names. Unnamed/generated XML has no package
/// authority; embedded parts deliberately cannot grant content.xml editing ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourcePart {
    Content,
    Styles,
    Metadata,
    Embedded,
    Manifest,
}

impl SourcePart {
    pub fn label(self) -> &'static str {
        match self {
            Self::Content => "content.xml",
            Self::Styles => "styles.xml",
            Self::Metadata => "meta.xml",
            Self::Embedded => "embedded",
            Self::Manifest => "META-INF/manifest.xml",
        }
    }
}

#[derive(Debug)]
struct Scope {
    parent: Option<Arc<Scope>>,
    declarations: Vec<(String, Arc<str>)>,
}

#[derive(Debug)]
pub enum Node {
    El(El),
    Text(String),
}

impl El {
    /// Charged once for the parsed part, on its root only. Callers add this to their
    /// cumulative package import budget; child elements return zero.
    pub fn identity_metadata_bytes(&self) -> usize {
        self.identity_metadata_bytes
    }
    pub fn source_start(&self) -> usize {
        self.source.as_ref().map(|source| source.range.start).unwrap_or(0)
    }
    /// Compare a parsed part's immutable source without copying or hashing it. Callers
    /// charge the complete comparison once per part, rather than once per descendant.
    pub(crate) fn source_bytes_match(&self, bytes: &[u8]) -> bool {
        self.source.as_ref().is_some_and(|source| source.bytes.as_ref() == bytes)
    }
    pub fn source_provenance(&self) -> Option<(SourcePart, Range<usize>)> {
        let source = self.source.as_ref()?;
        Some((source.part?, source.range.clone()))
    }
    /// Resolve a QName-valued field in its original source scope. Namespace lookup
    /// does not classify the referenced feature as supported. Returned URI/local
    /// strings borrow shared bindings and the input; no URI is copied or truncated.
    pub fn resolve_qname<'a>(&'a self, value: &'a str) -> Option<(Option<&'a str>, &'a str)> {
        let value = value.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
        let (prefix, local) = value.split_once(':').map(|(prefix, local)| (Some(prefix), local)).unwrap_or((None, value));
        if !ncname(local) || prefix.is_some_and(|prefix| !ncname(prefix)) {
            return None;
        }
        if prefix == Some("xml") {
            return Some((Some("http://www.w3.org/XML/1998/namespace"), local));
        }
        let mut scope = self.source.as_ref().and_then(|source| source.scope.as_deref());
        for _ in 0..64 {
            let Some(current) = scope else { break };
            if let Some((_, uri)) = current.declarations.iter().rev().find(|(name, _)| name == prefix.unwrap_or("")) {
                return if uri.is_empty() { if prefix.is_none() { Some((None, local)) } else { None } } else { Some((Some(uri.as_ref()), local)) };
            }
            scope = current.parent.as_deref();
        }
        if prefix.is_none() { Some((None, local)) } else { None }
    }
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }
    pub fn elements(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|n| match n {
            Node::El(e) => Some(e),
            _ => None,
        })
    }
    pub fn child(&self, name: &str) -> Option<&El> {
        self.elements().find(|e| e.name == name)
    }
    pub fn descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a El>) {
        for e in self.elements() {
            if e.name == name {
                out.push(e);
            }
            e.descendants(name, out);
        }
    }

    /// Namespace declarations are structural; foreign content may carry semantics we cannot edit.
    pub fn foreign_content(&self) -> bool {
        self.foreign_content_in(None)
    }

    fn foreign_content_in(&self, parent: Option<&El>) -> bool {
        self.name.starts_with("unknown:")
            || (self.name.starts_with("loext:") && !parent.is_some_and(|p| self.supported_paragraph_fill(p)))
            || (self.attrs.keys().any(|name| name.starts_with("unknown:") || name.starts_with("loext:"))
                && !crate::table_number_styles::supported_extension(self))
            || self.elements().any(|child| child.foreign_content_in(Some(self)))
    }

    /// Parsing a namespace does not imply support for its features. Only this exact, empty
    /// paragraph fill wrapper is represented by the text reader; all other loext semantics
    /// continue to trigger the foreign-content export guard.
    pub(crate) fn supported_paragraph_fill(&self, parent: &El) -> bool {
        let valid_color = |value: &str| value.strip_prefix('#').is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()));
        self.name == "loext:graphic-properties"
            && matches!(parent.name.as_str(), "style:style" | "style:default-style")
            && parent.attr("style:family") == Some("paragraph")
            && self.attrs.keys().all(|name| matches!(name.as_str(), "draw:fill" | "draw:fill-color"))
            && self.children.iter().all(|node| match node {
                Node::Text(text) => text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')),
                Node::El(_) => false,
            })
            && self.attr("draw:fill-color").is_none_or(valid_color)
            && match self.attr("draw:fill") {
                Some("none") => true,
                Some("solid") => self.attr("draw:fill-color").is_some_and(valid_color),
                _ => false,
            }
    }
}

/// XML1.0 NCName character classes, excluding the QName separator colon.
fn ncname(value: &str) -> bool {
    fn start(c: char) -> bool {
        matches!(c, 'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
            | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
            | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
    }
    let mut chars = value.chars();
    chars.next().is_some_and(start)
        && chars.all(|c| start(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}'))
}

fn expanded_name(
    reader: &NsReader<&[u8]>,
    q: QName<'_>,
    attribute: bool,
    identities: &mut Identities,
    scope: Option<&Scope>,
) -> Result<ExpandedName> {
    let (ns, local) = if attribute { reader.resolver().resolve_attribute(q) } else { reader.resolver().resolve_element(q) };
    let namespace = match ns {
        ResolveResult::Bound(_) => {
            let prefix_end = q.as_ref().iter().position(|byte| *byte == b':');
            let prefix = prefix_end.and_then(|end| q.as_ref().get(..end)).unwrap_or(b"");
            if prefix == b"xml" {
                Some(identities.intern("http://www.w3.org/XML/1998/namespace")?)
            } else {
                // Resolve by the short lexical prefix in persistent scope, then clone
                // the interned URI handle. A long inherited URI is not rescanned or
                // allocated for each descendant/attribute.
                let mut current = scope;
                let mut found = None;
                while let Some(scope) = current {
                    if let Some((_, uri)) = scope.declarations.iter().rev().find(|(p, _)| p.as_bytes() == prefix) {
                        found = Some(uri.clone());
                        break;
                    }
                    current = scope.parent.as_deref();
                }
                Some(found.ok_or_else(|| OdpError("missing XML namespace scope".into()))?)
            }
        }
        ResolveResult::Unbound => None,
        ResolveResult::Unknown(_) => return Err(OdpError("unbound XML namespace prefix".into())),
    };
    let local = std::str::from_utf8(local.as_ref()).map_err(|_| OdpError("invalid XML name encoding".into()))?;
    if local.len() > 8192 {
        return Err(OdpError("XML name exceeds size limit".into()));
    }
    Ok(ExpandedName { namespace, local: identities.intern(local)? })
}

fn projected_name(name: &ExpandedName) -> String {
    let prefix = name.namespace.as_deref().and_then(|uri| NS.iter().find(|(_, u)| *u == uri).map(|(p, _)| *p)).unwrap_or("unknown");
    format!("{prefix}:{}", name.local)
}

pub fn parse(bytes: &[u8]) -> Result<El> {
    parse_source(bytes, None)
}

pub(crate) fn parse_part(bytes: &[u8], part: SourcePart) -> Result<El> {
    parse_source(bytes, Some(part))
}

fn parse_source(bytes: &[u8], part: Option<SourcePart>) -> Result<El> {
    if bytes.len() as u64 > crate::MAX_PART {
        return Err(OdpError("XML source exceeds part size limit".into()));
    }
    let source: Arc<[u8]> = bytes.into();
    let mut reader = NsReader::from_reader(bytes);
    let mut stack: Vec<El> = Vec::new();
    let mut root = None;
    let mut nodes = 0;
    let mut expanded_text = 0usize;
    let mut identities = Identities::default();
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|e| OdpError(e.to_string()))?;
        match event {
            Event::Start(ref b) | Event::Empty(ref b) => {
                nodes += 1;
                if nodes > 200_000 || stack.len() >= 64 {
                    return Err(OdpError("XML exceeds nesting or element limit".into()));
                }
                identities.charge(
                    std::mem::size_of::<ExpandedName>()
                        .saturating_add(std::mem::size_of::<Vec<ExpandedAttribute>>())
                        // Conservative alignment allowance for the new provenance slot.
                        .saturating_add(std::mem::size_of::<usize>())
                        .saturating_add(std::mem::size_of::<usize>()),
                )?;
                let mut declarations = Vec::new();
                // Namespace bindings apply to the element's own name and attributes,
                // regardless of declaration order in the source start tag.
                for (index, a) in b.attributes().enumerate() {
                    if index >= 128 {
                        return Err(OdpError("XML attributes exceed size limit".into()));
                    }
                    let a = a.map_err(|e| OdpError(e.to_string()))?;
                    let prefix = if a.key.as_ref() == b"xmlns" { Some(b"".as_slice()) } else { a.key.as_ref().strip_prefix(b"xmlns:") };
                    let Some(prefix) = prefix else { continue };
                    let value =
                        a.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder()).map_err(|e| OdpError(e.to_string()))?;
                    if a.key.as_ref().len() > 8192 || value.len() > 8192 {
                        return Err(OdpError("XML attributes exceed size limit".into()));
                    }
                    identities.charge(
                        prefix
                            .len()
                            .saturating_add(std::mem::size_of::<Scope>())
                            .saturating_add(std::mem::size_of::<(String, Arc<str>)>().saturating_mul(2)),
                    )?;
                    let prefix = std::str::from_utf8(prefix).map_err(|_| OdpError("invalid namespace prefix".into()))?.to_string();
                    declarations.push((prefix, identities.intern(&value)?));
                }
                let inherited = stack.last().and_then(|p| p.source.as_ref()).and_then(|s| s.scope.clone());
                let scope = if declarations.is_empty() { inherited } else { Some(Arc::new(Scope { parent: inherited, declarations })) };
                let identity = expanded_name(&reader, b.name(), false, &mut identities, scope.as_deref())?;
                let mut el = El { name: projected_name(&identity), expanded_name: identity, ..Default::default() };
                let mut attribute_names = BTreeSet::new();
                for (index, a) in b.attributes().enumerate() {
                    if index >= 128 {
                        return Err(OdpError("XML attributes exceed size limit".into()));
                    }
                    let a = a.map_err(|e| OdpError(e.to_string()))?;
                    let value = a
                        .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|e| OdpError(e.to_string()))?
                        .into_owned();
                    if a.key.as_ref().len() > 8192 || value.len() > 8192 {
                        return Err(OdpError("XML attributes exceed size limit".into()));
                    }
                    if a.key.as_ref() == b"xmlns" || a.key.as_ref().starts_with(b"xmlns:") {
                        continue;
                    }
                    let expanded = expanded_name(&reader, a.key, true, &mut identities, scope.as_deref())?;
                    // Interned URI pointer identity avoids comparing a long shared URI
                    // repeatedly while detecting duplicate expanded attribute names.
                    let namespace_id = expanded.namespace.as_ref().map(|uri| Arc::as_ptr(uri) as *const u8 as usize);
                    if !attribute_names.insert((namespace_id, expanded.local.clone())) {
                        return Err(OdpError("duplicate expanded XML attribute".into()));
                    }
                    let mut key = projected_name(&expanded);
                    if el.attrs.contains_key(&key) {
                        // Keep projections distinct even if a permissive XML tokenizer accepts
                        // a nonconforming local name containing our separator.
                        key.push_str(&format!("@{index}"));
                        while el.attrs.contains_key(&key) {
                            key.push('@');
                        }
                    }
                    identities.charge(key.len().saturating_add(std::mem::size_of::<ExpandedAttribute>().saturating_mul(2)))?;
                    el.expanded_attrs.push(ExpandedAttribute { name: expanded, projected_key: key.clone(), index });
                    el.attrs.insert(key, value);
                }
                if el.name == "text:s" {
                    let count = el.attr("text:c").unwrap_or("1").parse::<usize>().map_err(|_| OdpError("invalid space count".into()))?;
                    expanded_text = expanded_text.saturating_add(count);
                    if expanded_text > 8 * 1024 * 1024 {
                        return Err(OdpError("expanded text exceeds size limit".into()));
                    }
                }
                if el.attrs.len() > 128 || el.attrs.values().any(|v| v.len() > 8192) {
                    return Err(OdpError("XML attributes exceed size limit".into()));
                }
                el.source = Some(Source { bytes: source.clone(), range: start..reader.buffer_position() as usize, scope, part });
                if matches!(event, Event::Start(_)) {
                    stack.push(el);
                } else {
                    append(&mut stack, &mut root, el)?;
                }
            }
            Event::End(_) => {
                let mut el = stack.pop().ok_or_else(|| OdpError("unmatched XML end".into()))?;
                if let Some(source) = &mut el.source {
                    source.range.end = reader.buffer_position() as usize;
                }
                append(&mut stack, &mut root, el)?;
            }
            Event::Text(t) => {
                if let Some(p) = stack.last_mut() {
                    p.children.push(Node::Text(t.xml_content(quick_xml::XmlVersion::Implicit1_0).map_err(|e| OdpError(e.to_string()))?.into_owned()));
                }
            }
            Event::CData(t) => {
                if let Some(p) = stack.last_mut() {
                    p.children.push(Node::Text(t.xml_content(quick_xml::XmlVersion::Implicit1_0).map_err(|e| OdpError(e.to_string()))?.into_owned()));
                }
            }
            Event::GeneralRef(r) => {
                let value = if r.is_char_ref() {
                    r.resolve_char_ref().map_err(|e| OdpError(e.to_string()))?.map(String::from)
                } else {
                    quick_xml::escape::resolve_predefined_entity(&r.decode().map_err(|e| OdpError(e.to_string()))?).map(String::from)
                }
                .ok_or_else(|| OdpError("unknown XML entity".into()))?;
                if let Some(p) = stack.last_mut() {
                    p.children.push(Node::Text(value));
                }
            }
            Event::DocType(_) => return Err(OdpError("XML document types are not supported".into())),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(OdpError("truncated XML".into()));
    }
    let mut root = root.ok_or_else(|| OdpError("missing XML root".into()))?;
    root.identity_metadata_bytes = identities.bytes;
    Ok(root)
}

/// Preserve a source element, including unknown namespaces, QName attribute values, comments
/// and processing instructions. Inherited bindings become local declarations so the fragment
/// remains valid when moved into a newly written package. No external resource is resolved.
#[cfg(test)]
fn source_fragment(el: &El) -> Result<String> {
    source_fragment_limited(el, crate::MAX_PART as usize)
}

/// Check the caller's cumulative budget before allocating a self-contained fragment.
pub(crate) fn source_fragment_limited(el: &El, available: usize) -> Result<String> {
    let source = el.source.as_ref().ok_or_else(|| OdpError("element has no source fragment".into()))?;
    let bytes = source.bytes.get(source.range.clone()).ok_or_else(|| OdpError("invalid XML source range".into()))?;
    let original = std::str::from_utf8(bytes).map_err(|_| OdpError("source XML is not UTF-8".into()))?;
    let mut scopes = Vec::new();
    let mut scope = source.scope.as_deref();
    while let Some(current) = scope {
        if scopes.len() >= 64 {
            return Err(OdpError("namespace scope exceeds nesting limit".into()));
        }
        scopes.push(current);
        scope = current.parent.as_deref();
    }
    let mut bindings = BTreeMap::new();
    for scope in scopes.into_iter().rev() {
        for (prefix, uri) in &scope.declarations {
            bindings.insert(prefix.as_str(), uri.as_ref());
        }
    }
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let first = reader.read_event().map_err(|e| OdpError(e.to_string()))?;
    let (Event::Start(first) | Event::Empty(first)) = first else { return Err(OdpError("invalid source element".into())) };
    let insert = 1usize.saturating_add(first.name().as_ref().len());
    for attr in first.attributes() {
        let attr = attr.map_err(|e| OdpError(e.to_string()))?;
        if attr.key.as_ref() == b"xmlns" {
            bindings.remove("");
        } else if let Some(prefix) = attr.key.as_ref().strip_prefix(b"xmlns:") {
            let prefix = std::str::from_utf8(prefix).map_err(|_| OdpError("invalid namespace prefix".into()))?;
            bindings.remove(prefix);
        }
    }
    let expanded = bindings
        .iter()
        .fold(original.len(), |size, (prefix, uri)| size.saturating_add(prefix.len()).saturating_add(uri.len().saturating_mul(6)).saturating_add(12));
    if expanded as u64 > crate::MAX_PART || expanded > available {
        return Err(OdpError("preserved XML fragment exceeds part size limit".into()));
    }
    let mut out = String::with_capacity(expanded);
    out.push_str(original.get(..insert).ok_or_else(|| OdpError("invalid XML source prefix".into()))?);
    for (prefix, uri) in bindings {
        out.push_str(" xmlns");
        if !prefix.is_empty() {
            out.push(':');
            out.push_str(prefix);
        }
        out.push_str("=\"");
        out.push_str(&esc(uri));
        out.push('"');
    }
    out.push_str(original.get(insert..).ok_or_else(|| OdpError("invalid XML source suffix".into()))?);
    Ok(out)
}

fn append(stack: &mut [El], root: &mut Option<El>, el: El) -> Result<()> {
    if let Some(p) = stack.last_mut() {
        p.children.push(Node::El(el));
    } else if root.replace(el).is_some() {
        return Err(OdpError("multiple XML roots".into()));
    }
    Ok(())
}

pub fn esc(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}
pub fn header(root: &str) -> String {
    let mut s = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><{root} office:version=\"1.3\"");
    for (p, u) in NS {
        s.push_str(&format!(" xmlns:{p}=\"{u}\""));
    }
    s.push('>');
    s
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    #[test]
    fn package_provenance_retains_complete_ranges_and_unnamed_xml_has_no_authority() {
        let source = b"<?xml version='1.0'?><x:p xmlns:x='urn:test'><x:s a='>'>A &amp; B</x:s><!--note--><x:e/></x:p>";
        let parsed = super::parse_part(source, super::SourcePart::Content).unwrap();
        let (part, range) = parsed.source_provenance().unwrap();
        assert_eq!(part, super::SourcePart::Content);
        assert_eq!(source.get(range), Some(&source[21..]));
        let child = parsed.elements().next().unwrap();
        let (_, range) = child.source_provenance().unwrap();
        assert_eq!(source.get(range), Some(b"<x:s a='>'>A &amp; B</x:s>".as_slice()));
        let alternate = super::parse_part(source, super::SourcePart::Styles).unwrap();
        assert_eq!(alternate.elements().next().unwrap().source_provenance().unwrap().0, super::SourcePart::Styles);
        let unnamed = super::parse(source).unwrap();
        assert!(unnamed.source_provenance().is_none());
        assert!(unnamed.elements().all(|child| child.source_provenance().is_none()));
    }
    use super::*;

    #[test]
    fn expanded_names_preserve_foreign_namespaces_and_unqualified_attributes() {
        let root = parse(br#"<r xmlns='urn:default' xmlns:a='urn:first' xmlns:b='urn:second' a:flag='one' b:flag='two' flag='three' xml:id='id'/>"#)
            .unwrap();
        assert_eq!(root.expanded_name.namespace.as_deref(), Some("urn:default"));
        assert_eq!(root.expanded_attrs.len(), 4);
        assert_eq!(root.expanded_attrs[0].name.namespace.as_deref(), Some("urn:first"));
        assert_eq!(root.expanded_attrs[1].name.namespace.as_deref(), Some("urn:second"));
        assert_eq!(root.expanded_attrs[2].name.namespace, None);
        assert_eq!(root.attrs.len(), 4);
        assert_eq!(root.attr(&root.expanded_attrs[0].projected_key), Some("one"));
        assert_eq!(root.attr(&root.expanded_attrs[1].projected_key), Some("two"));
        assert_eq!(root.attr(&root.expanded_attrs[2].projected_key), Some("three"));
        assert_eq!(root.attr("xml:id"), Some("id"));
    }

    #[test]
    fn duplicate_expanded_foreign_attributes_and_unbound_prefixes_reject() {
        for bytes in [br#"<r xmlns:a='urn:foreign' xmlns:b='urn:foreign' a:flag='1' b:flag='2'/>"#.as_slice(), b"<a:r/>", b"<r a:flag='1'/>"] {
            assert!(parse(bytes).is_err());
        }
    }

    #[test]
    fn identity_strings_are_shared_and_source_offsets_are_part_local() {
        let uri = format!("urn:long:{}", "x".repeat(7000));
        let bytes = format!("<r xmlns:a='{uri}'><a:item a:flag='1'/><a:item a:flag='2'/></r>");
        let root = parse(bytes.as_bytes()).unwrap();
        let a = root.elements().next().unwrap();
        let b = root.elements().nth(1).unwrap();
        assert!(Arc::ptr_eq(a.expanded_name.namespace.as_ref().unwrap(), b.expanded_name.namespace.as_ref().unwrap()));
        assert!(Arc::ptr_eq(&a.expanded_name.local, &b.expanded_name.local));
        assert!(a.source_start() < b.source_start());
        assert_eq!(bytes.get(a.source_start()..).unwrap().chars().next(), Some('<'));
    }

    #[test]
    fn repeated_long_namespace_aliases_share_one_identity_and_binding_storage() {
        let uri = format!("urn:long:{}", "x".repeat(7000));
        let declarations = (0..80).map(|index| format!(" xmlns:a{index}='{uri}'")).collect::<String>();
        let bytes = format!("<r{declarations}><a0:item a79:flag='yes'/></r>");
        let root = parse(bytes.as_bytes()).unwrap();
        let child = root.elements().next().unwrap();
        let scope = root.source.as_ref().unwrap().scope.as_ref().unwrap();
        let bound_uri = &scope.declarations[0].1;
        assert!(scope.declarations.iter().all(|(_, uri)| Arc::ptr_eq(uri, bound_uri)));
        assert!(Arc::ptr_eq(child.expanded_name.namespace.as_ref().unwrap(), bound_uri));
        assert!(Arc::ptr_eq(child.expanded_attrs[0].name.namespace.as_ref().unwrap(), bound_uri));
        assert!(root.identity_metadata_bytes() < bytes.len() / 2);
        assert_eq!(child.identity_metadata_bytes(), 0);
    }

    #[test]
    fn qname_values_resolve_source_aliases_defaults_rebinding_and_unbound_prefixes() {
        let root =
            parse(br#"<r xmlns='urn:default' xmlns:a='urn:chart' xmlns:b='urn:chart'><item xmlns:a='urn:foreign'><inner xmlns=''/></item></r>"#)
                .unwrap();
        let child = root.elements().next().unwrap();
        let inner = child.elements().next().unwrap();
        assert_eq!(root.resolve_qname("a:scatter"), Some((Some("urn:chart"), "scatter")));
        assert_eq!(root.resolve_qname("b:scatter"), root.resolve_qname("a:scatter"));
        assert_eq!(child.resolve_qname("a:scatter"), Some((Some("urn:foreign"), "scatter")));
        assert_eq!(child.resolve_qname("scatter"), Some((Some("urn:default"), "scatter")));
        assert_eq!(inner.resolve_qname("scatter"), Some((None, "scatter")));
        assert_eq!(inner.resolve_qname("xml:id"), Some((Some("http://www.w3.org/XML/1998/namespace"), "id")));
        assert_eq!(root.resolve_qname("  b:scatter\t"), root.resolve_qname("b:scatter"));
        for invalid in ["missing:scatter", "a:b:scatter", ":scatter", "a:", "a:scat ter", "1a:scatter", "", "a:1scatter"] {
            assert_eq!(root.resolve_qname(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn namespace_identity_budget_fails_closed_before_allocation() {
        let mut identities = Identities { bytes: MAX_IDENTITY_BYTES, ..Default::default() };
        assert!(identities.intern("urn:new").is_err());
        assert!(identities.strings.is_empty());
    }

    #[test]
    fn source_fragments_keep_unknown_namespaces_qnames_and_lexical_content() {
        let bytes = br#"<o:root xmlns:o="urn:own" xmlns:v="urn:extension" xmlns:q="urn:attribute-values"><o:holder><v:object q:type="q:Kind" xml:id="object1"><!--keep--><![CDATA[<raw>]]><?instruction value?></v:object></o:holder></o:root>"#;
        let root = parse(bytes).unwrap();
        let object = root.elements().next().unwrap().elements().next().unwrap();
        assert_eq!(object.attr("xml:id"), Some("object1"));
        let fragment = source_fragment(object).unwrap();
        assert!(fragment.contains("xmlns:v=\"urn:extension\""));
        assert!(fragment.contains("xmlns:q=\"urn:attribute-values\""));
        assert!(fragment.contains("q:type=\"q:Kind\""));
        assert!(fragment.contains("<!--keep--><![CDATA[<raw>]]><?instruction value?>"));
        parse(fragment.as_bytes()).unwrap();
        assert!(Arc::ptr_eq(&root.source.as_ref().unwrap().bytes, &object.source.as_ref().unwrap().bytes));
    }

    #[test]
    fn source_fragments_respect_namespace_rebindings_and_default_namespaces() {
        let root =
            parse(br#"<r xmlns="urn:outer" xmlns:v="urn:old"><h xmlns:v="urn:new"><v:item v:flag="yes"><child xmlns="urn:inner"/></v:item></h></r>"#)
                .unwrap();
        let object = root.elements().next().unwrap().elements().next().unwrap();
        let fragment = source_fragment(object).unwrap();
        assert!(fragment.contains("xmlns=\"urn:outer\""));
        assert!(fragment.contains("xmlns:v=\"urn:new\""));
        assert!(!fragment.contains("urn:old"));
        assert!(fragment.contains("<child xmlns=\"urn:inner\"/>"));
        parse(fragment.as_bytes()).unwrap();
    }

    #[test]
    fn source_fragment_does_not_duplicate_local_declarations_or_retain_siblings() {
        let root = parse(br#"<r xmlns:v="urn:old"><before/><v:item xmlns:v="urn:new" note="a&amp;b"/><after/></r>"#).unwrap();
        let object = root.elements().nth(1).unwrap();
        let fragment = source_fragment(object).unwrap();
        assert_eq!(fragment.matches("xmlns:v=").count(), 1);
        assert!(!fragment.contains("before") && !fragment.contains("after"));
        assert!(fragment.contains("note=\"a&amp;b\""));
        parse(fragment.as_bytes()).unwrap();
    }
}
