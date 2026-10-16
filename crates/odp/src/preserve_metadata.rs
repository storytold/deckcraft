//! Passive historical creation time; modified-time claims require a separate edit policy.
use super::refuse;
use crate::{
    Result, annotation_metadata,
    xml::{El, Node, SourcePart},
};

const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const META: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";
const DC: &str = "http://purl.org/dc/elements/1.1/";

fn named(el: &El, namespace: &str, local: &str) -> bool {
    el.expanded_name.namespace.as_deref() == Some(namespace) && el.expanded_name.local.as_ref() == local
}

/// Validate one scalar without normalizing or regenerating any source bytes.
pub(super) fn creation(el: &El) -> Result<()> {
    if el.name != "meta:creation-date"
        || !named(el, META, "creation-date")
        || !el.attrs.is_empty()
        || !el.expanded_attrs.is_empty()
        || el.source_provenance().is_none_or(|(part, range)| part != SourcePart::Metadata || range.start >= range.end)
    {
        return Err(refuse("creation-date identity, attributes or source"));
    }
    let mut size = 0usize;
    for node in &el.children {
        let Node::Text(text) = node else { return Err(refuse("creation-date must be scalar text")) };
        size = size.checked_add(text.len()).ok_or_else(|| refuse("creation-date scalar size"))?;
        if size > annotation_metadata::MAX_DATE || !annotation_metadata::valid_xml_text(text) {
            return Err(refuse("creation-date scalar size or XML characters"));
        }
    }
    // Existing XML work budgets cover the source; this one transient copy is capped
    // before allocation even when entity expansion split it into many text nodes.
    let mut value = String::with_capacity(size);
    for node in &el.children {
        if let Node::Text(text) = node {
            value.push_str(text);
        }
    }
    if !annotation_metadata::valid_date(&value) {
        return Err(refuse("creation-date outside the supported XML Schema dateTime profile"));
    }
    Ok(())
}

pub(super) fn validate(el: &El, parent: Option<&El>, part: &str) -> Result<()> {
    if named(el, DC, "date") && parent.is_some_and(|owner| named(owner, OFFICE, "meta")) {
        return Err(refuse("document modification date requires an explicit edited-export policy"));
    }
    if named(el, OFFICE, "meta") && el.elements().any(|child| named(child, META, "creation-date")) {
        if part != "meta.xml"
            || el.name != "office:meta"
            || !el.attrs.is_empty()
            || !el.expanded_attrs.is_empty()
            || parent.is_none_or(|owner| {
                owner.name != "office:document-meta"
                    || !named(owner, OFFICE, "document-meta")
                    || owner.elements().filter(|child| named(child, OFFICE, "meta")).take(2).count() != 1
            })
        {
            return Err(refuse("creation-date document metadata ownership"));
        }
        // ODF permits repeated metadata; this closed profile deliberately refuses
        // ambiguous creation histories instead of choosing one value.
        if el.elements().filter(|child| named(child, META, "creation-date")).take(2).count() != 1 {
            return Err(refuse("duplicate creation-date outside the preservation profile"));
        }
    }
    if named(el, META, "creation-date") || el.name == "meta:creation-date" {
        let owner = parent.ok_or_else(|| refuse("creation-date owner missing"))?;
        if part != "meta.xml" || owner.name != "office:meta" || !named(owner, OFFICE, "meta") {
            return Err(refuse("creation-date must belong directly to document metadata"));
        }
        let (source, range) = el.source_provenance().ok_or_else(|| refuse("creation-date source missing"))?;
        let (owner_source, owner_range) = owner.source_provenance().ok_or_else(|| refuse("creation-date owner source missing"))?;
        if source != SourcePart::Metadata || source != owner_source || range.start < owner_range.start || range.end > owner_range.end {
            return Err(refuse("creation-date source ownership mismatch"));
        }
        creation(el)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn annotation_creation_timestamp_is_not_a_document_modification_claim() {
        let root = crate::xml::parse_part(
            b"<office:annotation xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:date>2026-10-16T12:00:00Z</dc:date></office:annotation>",
            SourcePart::Content,
        )
        .unwrap();
        assert!(validate(root.child("dc:date").unwrap(), Some(&root), "content.xml").is_ok());
    }

    #[test]
    fn creation_scalar_requires_metadata_source_provenance() {
        let bytes = b"<meta:creation-date xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0'>2026-10-16T12:00:00Z</meta:creation-date>";
        let wrong_part = crate::xml::parse_part(bytes, SourcePart::Content).unwrap();
        assert!(creation(&wrong_part).is_err());
        let correct_part = crate::xml::parse_part(bytes, SourcePart::Metadata).unwrap();
        assert!(creation(&correct_part).is_ok());
    }
}
