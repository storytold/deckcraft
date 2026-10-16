//! Source-bound, shrinking cache edits; never serialize an XML tree.
use super::{charge, refuse};
use crate::{Result, xml::El};
use quick_xml::{Reader, events::Event};
use std::ops::Range;

fn space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

/// Attribute intervals include their preceding XML whitespace, and retain quote spelling.
pub(super) fn attributes(bytes: &[u8], el: &El, budget: &mut usize) -> Result<Vec<(usize, Range<usize>)>> {
    let (_, range) = el.source_provenance().ok_or_else(|| refuse("cache XML has no source"))?;
    let src = bytes.get(range.clone()).ok_or_else(|| refuse("cache source range"))?;
    charge(budget, src.len())?;
    let mut reader = Reader::from_reader(src);
    let start = match reader.read_event().map_err(|_| refuse("cache opening tag"))? {
        Event::Start(e) | Event::Empty(e) => e,
        _ => return Err(refuse("cache opening tag")),
    };
    let raw_qname = start.name();
    let raw_name = std::str::from_utf8(raw_qname.as_ref()).map_err(|_| refuse("cache source name encoding"))?;
    if el.resolve_qname(raw_name).is_none_or(|(ns, local)| ns != el.expanded_name.namespace.as_deref() || local != el.expanded_name.local.as_ref()) {
        return Err(refuse("cache source element identity"));
    }
    let opening_end = usize::try_from(reader.buffer_position()).map_err(|_| refuse("cache opening extent"))?;
    let raw = src.get(..opening_end).ok_or_else(|| refuse("cache opening extent"))?;
    let mut cursor = 1usize;
    while raw.get(cursor).is_some_and(|c| !space(*c) && !matches!(*c, b'/' | b'>')) {
        cursor += 1;
    }
    let mut result = Vec::new();
    for (ordinal, attr) in start.attributes().enumerate() {
        let attr = attr.map_err(|_| refuse("cache attribute syntax"))?;
        let begin = cursor;
        while raw.get(cursor).is_some_and(|c| space(*c)) {
            cursor += 1;
        }
        if begin == cursor {
            return Err(refuse("cache attribute separator"));
        }
        let name_start = cursor;
        while raw.get(cursor).is_some_and(|c| !space(*c) && *c != b'=') {
            cursor += 1;
        }
        if raw.get(name_start..cursor) != Some(attr.key.as_ref()) {
            return Err(refuse("cache attribute identity"));
        }
        while raw.get(cursor).is_some_and(|c| space(*c)) {
            cursor += 1;
        }
        if raw.get(cursor) != Some(&b'=') {
            return Err(refuse("cache attribute equals"));
        }
        cursor += 1;
        while raw.get(cursor).is_some_and(|c| space(*c)) {
            cursor += 1;
        }
        let quote = *raw.get(cursor).ok_or_else(|| refuse("cache attribute quote"))?;
        if !matches!(quote, b'\'' | b'"') {
            return Err(refuse("cache attribute quote"));
        }
        cursor += 1;
        let value_start = cursor;
        while raw.get(cursor).is_some_and(|c| *c != quote) {
            cursor += 1;
        }
        if raw.get(value_start..cursor) != Some(attr.value.as_ref()) || raw.get(cursor) != Some(&quote) {
            return Err(refuse("cache attribute value"));
        }
        cursor += 1;
        if let Some(expanded) = el.expanded_attrs.iter().find(|a| a.index == ordinal) {
            let name = std::str::from_utf8(attr.key.as_ref()).map_err(|_| refuse("cache attribute name encoding"))?;
            // Default element namespaces never apply to unqualified attributes.
            let identity = if name.contains(':') { el.resolve_qname(name) } else { Some((None, name)) };
            if identity.is_none_or(|(ns, local)| ns != expanded.name.namespace.as_deref() || local != expanded.name.local.as_ref()) {
                return Err(refuse("cache source attribute expanded identity"));
            }
            let value = attr
                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                .map_err(|_| refuse("cache attribute encoding"))?;
            if el.attr(&expanded.projected_key) != Some(value.as_ref()) {
                return Err(refuse("cache parsed attribute mismatch"));
            }
        } else if attr.key.as_ref() != b"xmlns" && !attr.key.as_ref().starts_with(b"xmlns:") {
            return Err(refuse("cache unbound attribute"));
        }
        charge(budget, 48)?;
        result.push((ordinal, range.start + begin..range.start + cursor));
    }
    while raw.get(cursor).is_some_and(|c| space(*c)) {
        cursor += 1;
    }
    if !matches!(raw.get(cursor..), Some(b">") | Some(b"/>")) {
        return Err(refuse("cache opening tail"));
    }
    Ok(result)
}

pub(super) fn empty_leaf(bytes: &[u8], el: &El, budget: &mut usize) -> Result<()> {
    attributes(bytes, el, budget)?;
    let (_, range) = el.source_provenance().ok_or_else(|| refuse("cache source missing"))?;
    let mut reader = Reader::from_reader(bytes.get(range).ok_or_else(|| refuse("cache source extent"))?);
    let mut elements = 0usize;
    loop {
        match reader.read_event().map_err(|_| refuse("cache leaf syntax"))? {
            Event::Start(_) | Event::Empty(_) => {
                elements += 1;
                if elements > 1 {
                    return Err(refuse("cache leaf child"));
                }
            }
            Event::End(_) => {}
            Event::Text(t) if t.as_ref().iter().all(|c| space(*c)) => {}
            Event::Eof => break,
            _ => return Err(refuse("cache leaf opaque content")),
        }
    }
    if elements != 1 {
        return Err(refuse("cache leaf identity"));
    }
    Ok(())
}

pub(super) fn remove(bytes: &[u8], mut ranges: Vec<Range<usize>>, budget: &mut usize) -> Result<Vec<u8>> {
    ranges.sort_by_key(|r| r.start);
    let mut end = 0usize;
    let mut removed = 0usize;
    for r in &ranges {
        if r.start < end || r.start > r.end || r.end > bytes.len() {
            return Err(refuse("overlapping cache edits"));
        }
        removed = removed.checked_add(r.len()).ok_or_else(|| refuse("cache edit extent"))?;
        end = r.end;
    }
    let size = bytes.len().checked_sub(removed).ok_or_else(|| refuse("cache edit size"))?;
    charge(budget, size)?;
    let mut output = Vec::with_capacity(size);
    end = 0;
    for r in ranges {
        output.extend_from_slice(bytes.get(end..r.start).ok_or_else(|| refuse("cache edit gap"))?);
        end = r.end;
    }
    output.extend_from_slice(bytes.get(end..).ok_or_else(|| refuse("cache edit tail"))?);
    Ok(output)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn equal_values_do_not_authorize_a_different_expanded_attribute() {
        let source = b"<meta:document-statistic xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' meta:word-count='1'/>";
        let el = crate::xml::parse_part(source, crate::xml::SourcePart::Metadata).unwrap();
        let changed = std::str::from_utf8(source).unwrap().replace("meta:word-count", "meta:page-count");
        assert!(attributes(changed.as_bytes(), &el, &mut 0).is_err());
        assert!(attributes(source, &el, &mut 0).is_ok());
    }
}
