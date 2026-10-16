//! Exact presentation-template resources; no live shape/layout/style application.
use super::*;
use deckcraft_model::presentation_template::{
    MAX_SLOTS_PER_TEMPLATE, MAX_TEMPLATE_BYTES, MAX_TEMPLATE_NAME_BYTES, MAX_TEMPLATES, MAX_TOTAL_SLOTS, PresentationRole, PresentationTemplate,
    PresentationTemplateBox, PresentationTemplateSlot, valid_ncname,
};
use std::collections::BTreeSet;

pub(super) type SourceKey = (&'static str, usize, usize);

fn named(el: &El, prefix: &str, local: &str) -> bool {
    xml::NS
        .iter()
        .find(|(name, _)| *name == prefix)
        .is_some_and(|(_, uri)| el.expanded_name.namespace.as_deref() == Some(*uri) && el.expanded_name.local.as_ref() == local)
}
fn attribute<'a>(el: &'a El, prefix: &str, local: &str) -> Option<&'a str> {
    let uri = xml::NS.iter().find(|(name, _)| *name == prefix)?.1;
    el.expanded_attrs
        .iter()
        .find(|attr| attr.name.namespace.as_deref() == Some(uri) && attr.name.local.as_ref() == local)
        .and_then(|attr| el.attr(&attr.projected_key))
}
fn key(el: &El) -> Result<SourceKey> {
    let (part, range) = el.source_provenance().ok_or_else(|| OdpError("presentation template lacks package source identity".into()))?;
    Ok((part.label(), range.start, range.end))
}
fn space(value: &str) -> bool {
    value.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}
fn empty(el: &El) -> bool {
    el.children.iter().all(|node| matches!(node, Node::Text(value) if space(value)))
}
fn attrs(el: &El, allowed: &[(&str, &str)]) -> bool {
    el.expanded_attrs.iter().all(|attr| {
        allowed.iter().any(|(prefix, local)| {
            xml::NS
                .iter()
                .find(|(name, _)| name == prefix)
                .is_some_and(|(_, uri)| attr.name.namespace.as_deref() == Some(*uri) && attr.name.local.as_ref() == *local)
        })
    })
}

/// ODF decimal physical lengths only. px needs a resolution policy; percentages need a parent.
fn physical(value: &str) -> Option<f64> {
    if value.len() > 512 {
        return None;
    }
    let (number, factor) = [("pt", 1.), ("cm", 72. / 2.54), ("mm", 72. / 25.4), ("in", 72.), ("pc", 12.)]
        .into_iter()
        .find_map(|(unit, factor)| value.strip_suffix(unit).map(|number| (number, factor)))?;
    let digits = number.strip_prefix('-').unwrap_or(number);
    let mut decimal = false;
    let mut seen_digit = false;
    for byte in digits.bytes() {
        if byte.is_ascii_digit() {
            seen_digit = true;
        } else if byte == b'.' && !decimal {
            decimal = true;
        } else {
            return None;
        }
    }
    if !seen_digit {
        return None;
    }
    let value = number.parse::<f64>().ok()? * factor;
    value.is_finite().then_some(value)
}

struct Definition<'a> {
    el: &'a El,
    name: &'a str,
    label: Option<&'a str>,
    slots: Option<Vec<(PresentationRole, PresentationTemplateBox, SourceKey)>>,
}

struct Scan<'a, 'b> {
    definitions: Vec<Definition<'a>>,
    names: BTreeMap<&'a str, usize>,
    references: Vec<&'a El>,
    page_keys: BTreeSet<SourceKey>,
    local_bytes: usize,
    slots: usize,
    total: &'b mut u64,
    unhandled_templates: bool,
    unhandled_references: bool,
    regular_object_semantics: bool,
}
impl Scan<'_, '_> {
    fn charge(&mut self, amount: usize) -> Result<()> {
        self.local_bytes = self.local_bytes.checked_add(amount).ok_or_else(|| OdpError("presentation template budget overflow".into()))?;
        *self.total = self.total.checked_add(amount as u64).ok_or_else(|| OdpError("presentation template total budget overflow".into()))?;
        if self.local_bytes > MAX_TEMPLATE_BYTES || *self.total > crate::MAX_TOTAL {
            return Err(OdpError("presentation template inventory exceeds import budget".into()));
        }
        Ok(())
    }
}
impl<'a> Scan<'a, '_> {
    fn definition(&mut self, el: &'a El, admitted: bool) -> Result<()> {
        let name = attribute(el, "style", "name").ok_or_else(|| OdpError("presentation template is missing style:name".into()))?;
        if !valid_ncname(name) {
            return Err(OdpError("presentation template name is not a bounded NCName".into()));
        }
        if self.definitions.len() >= MAX_TEMPLATES {
            return Err(OdpError("too many presentation templates".into()));
        }
        self.charge(512usize.saturating_add(name.len()))?;
        if self.names.insert(name, self.definitions.len()).is_some() {
            return Err(OdpError("duplicate presentation template resource name".into()));
        }
        let label = attribute(el, "style", "display-name");
        if label.is_some_and(|label| label.len() > MAX_TEMPLATE_NAME_BYTES) {
            return Err(OdpError("presentation template display label exceeds limit".into()));
        }
        self.charge(label.map_or(0, str::len))?;
        let mut supported = admitted && attrs(el, &[("style", "name"), ("style", "display-name")]);
        let mut slots = Vec::new();
        let mut owner_slots = 0usize;
        for node in &el.children {
            let Node::El(slot) = node else {
                if matches!(node, Node::Text(value) if !space(value)) {
                    supported = false;
                }
                continue;
            };
            if !named(slot, "presentation", "placeholder") {
                supported = false;
                continue;
            }
            owner_slots = owner_slots.saturating_add(1);
            self.slots = self.slots.saturating_add(1);
            if owner_slots > MAX_SLOTS_PER_TEMPLATE || self.slots > MAX_TOTAL_SLOTS {
                return Err(OdpError("presentation template slot count exceeds limit".into()));
            }
            self.charge(std::mem::size_of::<PresentationTemplateSlot>().saturating_add(384))?;
            let role = attribute(slot, "presentation", "object").and_then(PresentationRole::from_odf);
            let bounds = attribute(slot, "svg", "x")
                .and_then(physical)
                .zip(attribute(slot, "svg", "y").and_then(physical))
                .zip(attribute(slot, "svg", "width").and_then(physical))
                .zip(attribute(slot, "svg", "height").and_then(physical))
                .and_then(|(((x, y), w), h)| PresentationTemplateBox::new(x, y, w, h).ok());
            if attrs(slot, &[("presentation", "object"), ("svg", "x"), ("svg", "y"), ("svg", "width"), ("svg", "height")])
                && empty(slot)
                && let (Some(role), Some(bounds)) = (role, bounds)
            {
                slots.push((role, bounds, key(slot)?));
            } else {
                supported = false;
            }
        }
        self.unhandled_templates |= !supported;
        self.definitions.push(Definition { el, name, label, slots: supported.then_some(slots) });
        Ok(())
    }
    fn visit(&mut self, el: &'a El, depth: usize, root_admitted: bool, direct_styles: bool, parent_template: bool, regular_page: bool) -> Result<()> {
        if depth > 64 {
            return Err(OdpError("presentation template XML nesting exceeds limit".into()));
        }
        self.charge(64usize.saturating_add(el.expanded_attrs.len().saturating_mul(64)))?;
        let template = named(el, "style", "presentation-page-layout");
        if template {
            self.definition(el, root_admitted && depth == 2 && direct_styles)?;
        }
        if named(el, "presentation", "placeholder") && !parent_template {
            self.unhandled_templates = true;
        }
        let regular_page = (regular_page || self.page_keys.contains(&key(el)?)) && !named(el, "presentation", "notes");
        if regular_page
            && el.expanded_attrs.iter().any(|attr| {
                attr.name.namespace.as_deref() == Some("urn:oasis:names:tc:opendocument:xmlns:presentation:1.0")
                    && matches!(attr.name.local.as_ref(), "class" | "placeholder" | "user-transformed" | "style-name" | "class-names")
            })
        {
            self.regular_object_semantics = true;
        }
        if attribute(el, "presentation", "presentation-page-layout-name").is_some() {
            self.charge(128)?;
            self.references.push(el);
            if !self.page_keys.contains(&key(el)?) {
                self.unhandled_references = true;
            }
        }
        let styles = root_admitted && depth == 1 && named(el, "office", "styles");
        for child in el.elements() {
            self.visit(child, depth + 1, root_admitted, styles, template, regular_page)?;
        }
        Ok(())
    }
}

/// Owned name bindings plus exact source owners. Flags require independent blocking warnings;
/// admitted template ownership must never acknowledge regular-page placeholder/style semantics.
pub(super) struct TemplateRead {
    pub(super) supported: BTreeSet<SourceKey>,
    pub(super) unhandled_templates: bool,
    pub(super) unhandled_references: bool,
    pub(super) regular_object_semantics: bool,
    pages: BTreeMap<SourceKey, Option<PresentationTemplateId>>,
}
impl TemplateRead {
    pub(super) fn build(content: &El, styles_doc: Option<&El>, pages: &[&El], p: &mut Presentation, total: &mut u64) -> Result<Self> {
        if !p.presentation_templates.is_empty() {
            return Err(OdpError("presentation template import target is not empty".into()));
        }
        let mut scan = Scan {
            definitions: Vec::new(),
            names: BTreeMap::new(),
            references: Vec::new(),
            page_keys: BTreeSet::new(),
            local_bytes: 0,
            slots: 0,
            total,
            unhandled_templates: false,
            unhandled_references: false,
            regular_object_semantics: false,
        };
        if pages.len() > 10_000 {
            return Err(OdpError("too many presentation template page bindings".into()));
        }
        scan.charge(pages.len().saturating_mul(256))?;
        for page in pages {
            scan.page_keys.insert(key(page)?);
        }
        for (root, expected) in styles_doc.into_iter().map(|root| (root, "document-styles")).chain(std::iter::once((content, "document-content"))) {
            let admitted = named(root, "office", expected)
                && root.source_provenance().is_some_and(|(part, _)| {
                    matches!((expected, part), ("document-content", xml::SourcePart::Content) | ("document-styles", xml::SourcePart::Styles))
                });
            if root.elements().filter(|el| named(el, "office", "styles")).take(2).count() > 1 {
                return Err(OdpError("multiple presentation template common-style containers".into()));
            }
            scan.visit(root, 0, admitted, false, false, false)?;
        }
        let mut result = Self {
            supported: BTreeSet::new(),
            unhandled_templates: scan.unhandled_templates,
            unhandled_references: scan.unhandled_references,
            regular_object_semantics: scan.regular_object_semantics,
            pages: BTreeMap::new(),
        };
        let mut bindings = BTreeMap::new();
        for definition in scan.definitions {
            let Some(slots) = definition.slots else {
                bindings.insert(definition.name, None);
                continue;
            };
            let owner = PresentationTemplateId(id(p)?);
            let mut native = Vec::new();
            for (role, bounds, source) in slots {
                native.push(PresentationTemplateSlot { id: PresentationTemplateSlotId(id(p)?), role, bounds });
                result.supported.insert(source);
            }
            result.supported.insert(key(definition.el)?);
            p.presentation_templates.push(Arc::new(PresentationTemplate {
                id: owner,
                name: definition.name.into(),
                display_name: definition.label.map(str::to_string),
                slots: native,
            }));
            bindings.insert(definition.name, Some(owner));
        }
        for page in pages {
            result.pages.insert(key(page)?, None);
        }
        for reference in scan.references {
            let name = attribute(reference, "presentation", "presentation-page-layout-name")
                .ok_or_else(|| OdpError("missing presentation template reference".into()))?;
            if !name.is_empty() && !valid_ncname(name) {
                return Err(OdpError("presentation template reference is not a bounded NCName".into()));
            }
            let selected = if name.is_empty() {
                None
            } else {
                *bindings.get(name).ok_or_else(|| OdpError("dangling presentation template reference".into()))?
            };
            let source = key(reference)?;
            if let Some(binding) = result.pages.get_mut(&source) {
                *binding = selected;
                if name.is_empty() || selected.is_some() {
                    result.supported.insert(source);
                } else {
                    result.unhandled_references = true;
                }
            }
        }
        deckcraft_model::presentation_template::validate(p).map_err(|error| OdpError(error.to_string()))?;
        Ok(result)
    }
    pub(super) fn selection(&self, page: &El) -> Result<Option<PresentationTemplateId>> {
        let source = key(page)?;
        let selection = self.pages.get(&source).copied().ok_or_else(|| OdpError("presentation template page owner was not indexed".into()))?;
        if selection.is_some() && !self.supported.contains(&source) {
            return Err(OdpError("presentation template selection lacks exact supported source ownership".into()));
        }
        Ok(selection)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    fn content() -> El {
        xml::parse_part(b"<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0'><office:styles><style:presentation-page-layout style:name='A'/><style:presentation-page-layout style:name='B'/></office:styles><office:body><office:presentation/></office:body></office:document-content>",xml::SourcePart::Content).unwrap()
    }
    #[test]
    fn import_checked_allocator_exhaustion_is_precise() {
        let root = content();
        let mut p = Presentation { next_id: 1_999_999, ..Default::default() };
        let error = TemplateRead::build(&root, None, &[], &mut p, &mut 0).err().unwrap().to_string();
        assert!(error.contains("too many objects"));
    }
    #[test]
    fn aggregate_import_budget_is_charged_before_template_model_allocation() {
        let root = content();
        let mut p = Presentation::default();
        let mut total = crate::MAX_TOTAL - 1;
        let error = TemplateRead::build(&root, None, &[], &mut p, &mut total).err().unwrap().to_string();
        assert!(error.contains("inventory exceeds import budget"));
        assert!(p.presentation_templates.is_empty());
    }
}
