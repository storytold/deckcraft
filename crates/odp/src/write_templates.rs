//! Emit each presentation-template owner exactly once; pages carry only a name reference.
use super::*;
use deckcraft_model::presentation_template::{self, PresentationTemplate};

fn append(out: &mut String, value: &str) -> Result<()> {
    if out.len().checked_add(value.len()).is_none_or(|size| size as u64 > crate::MAX_PART) {
        return Err(OdpError("presentation template XML exceeds package part limit".into()));
    }
    out.push_str(value);
    Ok(())
}

/// Stream attribute escaping, preserving XML-normalized whitespace via references.
fn attribute(out: &mut String, value: &str) -> Result<()> {
    for character in value.chars() {
        match character {
            '&' => append(out, "&amp;")?,
            '<' => append(out, "&lt;")?,
            '>' => append(out, "&gt;")?,
            '"' => append(out, "&quot;")?,
            '\'' => append(out, "&apos;")?,
            '\t' => append(out, "&#9;")?,
            '\n' => append(out, "&#10;")?,
            '\r' => append(out, "&#13;")?,
            other => {
                let mut utf8 = [0; 4];
                append(out, other.encode_utf8(&mut utf8))?;
            }
        }
    }
    Ok(())
}

fn length(out: &mut String, name: &str, value: f64) -> Result<()> {
    // Model validation bounds finite operands. f64 Display's decimal form is bounded
    // by its finite precision/exponent range; one tiny temporary cannot grow with input.
    let value = value.to_string();
    if value.len() > 384 || value.contains(['e', 'E']) {
        return Err(OdpError("presentation template coordinate has no bounded decimal encoding".into()));
    }
    append(out, " svg:")?;
    append(out, name)?;
    append(out, "=\"")?;
    append(out, &value)?;
    append(out, "pt\"")
}
fn emit_template(template: &PresentationTemplate, out: &mut String) -> Result<()> {
    append(out, "<style:presentation-page-layout style:name=\"")?;
    attribute(out, &template.name)?;
    append(out, "\"")?;
    if let Some(label) = &template.display_name {
        append(out, " style:display-name=\"")?;
        attribute(out, label)?;
        append(out, "\"")?;
    }
    append(out, ">")?;
    for slot in &template.slots {
        append(out, "<presentation:placeholder presentation:object=\"")?;
        append(out, slot.role.odf_token())?;
        append(out, "\"")?;
        length(out, "x", slot.bounds.x)?;
        length(out, "y", slot.bounds.y)?;
        length(out, "width", slot.bounds.w)?;
        length(out, "height", slot.bounds.h)?;
        append(out, "/>")?;
    }
    append(out, "</style:presentation-page-layout>")
}

pub(super) struct TemplateExportPlan<'a> {
    names: BTreeMap<PresentationTemplateId, &'a str>,
    definitions: String,
}
impl<'a> TemplateExportPlan<'a> {
    pub(super) fn build(p: &'a Presentation) -> Result<Self> {
        let stats = presentation_template::validate(p).map_err(|error| OdpError(error.to_string()))?;
        let index = p.presentation_templates.len().checked_mul(128).ok_or_else(|| OdpError("presentation template export index overflow".into()))?;
        if stats.logical_bytes.checked_add(index).is_none_or(|bytes| bytes as u64 > crate::MAX_PART) {
            return Err(OdpError("presentation template export index exceeds budget".into()));
        }
        let mut plan = Self { names: BTreeMap::new(), definitions: String::new() };
        for template in &p.presentation_templates {
            plan.names.insert(template.id, template.name.as_str());
            emit_template(template, &mut plan.definitions)?;
        }
        Ok(plan)
    }
    /// Call exactly once in office:styles, alongside the other common definitions.
    pub(super) fn emit(&self, out: &mut String) -> Result<()> {
        append(out, &self.definitions)
    }
    /// Append inside the page opening tag. Lookup is indexed, never a resource scan.
    pub(super) fn page_attrs(&self, slide: &Slide, out: &mut String) -> Result<()> {
        let Some(id) = slide.presentation_template else {
            return Ok(());
        };
        let name = self.names.get(&id).ok_or_else(|| OdpError("dangling presentation template selection during export".into()))?;
        append(out, " presentation:presentation-page-layout-name=\"")?;
        attribute(out, name)?;
        append(out, "\"")
    }
}
