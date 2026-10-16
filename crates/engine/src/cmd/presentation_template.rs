//! Editable template resources and page selection. These commands never apply
//! slot geometry or create/remove live slide objects.
use std::{mem::size_of, sync::Arc};

use deckcraft_model::{
    Presentation, PresentationTemplate, PresentationTemplateId, PresentationTemplateSlotId, SlideId,
    presentation_template::{
        self as templates, MAX_SLOTS_PER_TEMPLATE, MAX_TEMPLATE_BYTES, MAX_TEMPLATE_NAME_BYTES, MAX_TEMPLATE_SCAN_WORK, MAX_TEMPLATES,
        MAX_TOTAL_SLOTS, PresentationRole, PresentationTemplateBox, PresentationTemplateSlot, TemplateValidation,
    },
};
use serde_json::{Map, Value, json};

use super::{CommandSpec, NO_ID_SPACE, bad, cmd, has_doc};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "presentation.template.create",
            "Create Presentation Template",
            ["Design", "Templates"],
            None,
            "{name, displayName?: string, slots: [{role, bounds: {x,y,w,h} pt}]} → {id, slots: [slot ID]}; creates a resource without applying it",
            has_doc,
            create
        ),
        cmd!(
            "presentation.template.rename",
            "Rename Presentation Template",
            ["Design", "Templates"],
            None,
            "{id, name?: NCName, displayName?: string|null} — rename a resource without changing page objects",
            has_doc,
            rename
        ),
        cmd!(
            "presentation.template.addSlot",
            "Add Template Slot",
            ["Design", "Templates"],
            None,
            "{id, role, bounds: {x,y,w,h} pt, before?: slot ID} → {id, slot}; adds a resource slot without creating page objects",
            has_doc,
            add_slot
        ),
        cmd!(
            "presentation.template.setSlot",
            "Edit Template Slot",
            ["Design", "Templates"],
            None,
            "{id, slot, role?, bounds?: {x,y,w,h} pt} — edit a qualified slot without moving page objects",
            has_doc,
            set_slot
        ),
        cmd!(
            "presentation.template.reorderSlot",
            "Reorder Template Slot",
            ["Design", "Templates"],
            None,
            "{id, slot, index: final zero-based position} — reorder slots without changing their identities or page objects",
            has_doc,
            reorder_slot
        ),
        cmd!(
            "presentation.template.removeSlot",
            "Remove Template Slot",
            ["Design", "Templates"],
            None,
            "{id, slot} — remove a qualified resource slot without deleting page objects",
            has_doc,
            remove_slot
        ),
        cmd!(
            "presentation.template.delete",
            "Delete Presentation Template",
            ["Design", "Templates"],
            None,
            "{id} — remove an unused resource; explicitly clear page selections first",
            has_doc,
            delete
        ),
        cmd!(
            "slide.presentationTemplate",
            "Select Presentation Template",
            ["Design", "Templates"],
            None,
            "{slide?: slide ID (default current), template: template ID|null} — select only; never apply slots or change existing page objects",
            has_doc,
            select
        ),
    ]
}

fn object<'a>(cmd: &str, value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>> {
    let object = value.as_object().ok_or_else(|| bad(cmd, "parameters must be an object"))?;
    if object.len() > keys.len() || object.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(bad(cmd, "unknown parameter"));
    }
    Ok(object)
}

fn required<'a>(cmd: &str, values: &'a Map<String, Value>, key: &str) -> Result<&'a Value> {
    values.get(key).ok_or_else(|| bad(cmd, format!("missing `{key}`")))
}

fn id(cmd: &str, value: &Value) -> Result<u32> {
    value.as_u64().and_then(|id| u32::try_from(id).ok()).ok_or_else(|| bad(cmd, "ID must be an unsigned 32-bit integer"))
}

fn name(cmd: &str, value: &Value) -> Result<String> {
    let value = value.as_str().filter(|value| templates::valid_ncname(value)).ok_or_else(|| bad(cmd, "name must be a bounded XML NCName"))?;
    Ok(value.into())
}

fn label(cmd: &str, value: &Value) -> Result<String> {
    let value = value.as_str().ok_or_else(|| bad(cmd, "displayName must be a string"))?;
    if value.len() > MAX_TEMPLATE_NAME_BYTES
        || !value.chars().all(|c| matches!(c, '\t' | '\r' | '\n' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
    {
        return Err(bad(cmd, "displayName is too long or contains an invalid XML character"));
    }
    Ok(value.into())
}

fn role(cmd: &str, value: &Value) -> Result<PresentationRole> {
    value.as_str().and_then(PresentationRole::from_odf).ok_or_else(|| bad(cmd, "unsupported presentation role"))
}

fn bounds(cmd: &str, value: &Value) -> Result<PresentationTemplateBox> {
    let values = object(cmd, value, &["x", "y", "w", "h"])?;
    let number = |key| required(cmd, values, key)?.as_f64().ok_or_else(|| bad(cmd, "bounds require four numeric coordinates"));
    PresentationTemplateBox::new(number("x")?, number("y")?, number("w")?, number("h")?).map_err(|error| bad(cmd, error.to_string()))
}

fn validate(s: &Session, cmd: &str) -> Result<TemplateValidation> {
    templates::validate(&s.doc()?.doc).map_err(|error| bad(cmd, error.to_string()))
}

fn owner(s: &Session, cmd: &str, id: PresentationTemplateId) -> Result<usize> {
    s.doc()?.doc.presentation_templates.iter().position(|template| template.id == id).ok_or_else(|| bad(cmd, "no such presentation template"))
}

fn resource<'a>(s: &'a Session, cmd: &str, index: usize) -> Result<&'a PresentationTemplate> {
    s.doc()?.doc.presentation_templates.get(index).map(Arc::as_ref).ok_or_else(|| bad(cmd, "no such presentation template"))
}

fn slot_index(template: &PresentationTemplate, cmd: &str, id: PresentationTemplateSlotId) -> Result<usize> {
    template.slots.iter().position(|slot| slot.id == id).ok_or_else(|| bad(cmd, "no such slot in this template"))
}

fn unique_name(s: &Session, cmd: &str, name: &str, except: Option<usize>) -> Result<()> {
    if s.doc()?.doc.presentation_templates.iter().enumerate().any(|(index, template)| Some(index) != except && template.name == name) {
        return Err(bad(cmd, "duplicate presentation template name"));
    }
    Ok(())
}

/// Account growth before any template COW clone. The whole unpublished candidate
/// is independently validated again before Session can publish it.
fn preflight_growth(cmd: &str, stats: TemplateValidation, old_bytes: usize, new_bytes: usize, owners: usize, slots: usize) -> Result<()> {
    if stats.template_count.checked_add(owners).is_none_or(|count| count > MAX_TEMPLATES)
        || stats.slot_count.checked_add(slots).is_none_or(|count| count > MAX_TOTAL_SLOTS)
    {
        return Err(bad(cmd, "presentation template resource count exceeds limit"));
    }
    let growth = new_bytes.saturating_sub(old_bytes);
    let scratch = owners.saturating_mul(64).saturating_add(slots.saturating_mul(size_of::<PresentationTemplateSlotId>()));
    if stats.logical_bytes.checked_add(growth).and_then(|bytes| bytes.checked_add(scratch)).is_none_or(|bytes| bytes > MAX_TEMPLATE_BYTES)
        || stats.scan_work.checked_add(growth.saturating_mul(2)).is_none_or(|work| work > MAX_TEMPLATE_SCAN_WORK)
    {
        return Err(bad(cmd, "presentation template resource budget exceeded"));
    }
    Ok(())
}

fn allocation(s: &Session, cmd: &str, stats: TemplateValidation, count: usize) -> Result<u32> {
    let base = s.doc()?.doc.next_id.max(1).max(stats.max_id);
    let count = u32::try_from(count).map_err(|_| bad(cmd, NO_ID_SPACE))?;
    base.checked_add(count).ok_or_else(|| bad(cmd, NO_ID_SPACE))?;
    Ok(base)
}

fn consume_ids(doc: &mut Presentation, cmd: &str, base: u32, count: usize) -> Result<()> {
    doc.next_id = base;
    for _ in 0..count {
        doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?;
    }
    Ok(())
}

fn publish(s: &mut Session, cmd: &str, change: impl FnOnce(&mut Presentation) -> Result<Value>) -> Result<Value> {
    s.edit_resources(|doc, _| {
        let answer = change(doc)?;
        templates::validate(doc).map_err(|error| bad(cmd, error.to_string()))?;
        Ok(answer)
    })
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "presentation.template.create";
    let p = object(cmd, p, &["name", "displayName", "slots"])?;
    let name = name(cmd, required(cmd, p, "name")?)?;
    let display_name = p.get("displayName").map(|value| label(cmd, value)).transpose()?;
    let input = required(cmd, p, "slots")?.as_array().ok_or_else(|| bad(cmd, "slots must be an array"))?;
    if input.len() > MAX_SLOTS_PER_TEMPLATE {
        return Err(bad(cmd, "too many slots in one template"));
    }
    let stats = validate(s, cmd)?;
    unique_name(s, cmd, &name, None)?;
    let bytes = size_of::<PresentationTemplate>()
        .saturating_add(name.len())
        .saturating_add(display_name.as_ref().map_or(0, String::len))
        .saturating_add(input.len().saturating_mul(size_of::<PresentationTemplateSlot>()));
    preflight_growth(cmd, stats, 0, bytes, 1, input.len())?;
    let base = allocation(s, cmd, stats, input.len() + 1)?;
    let template_id = PresentationTemplateId(base.checked_add(1).ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
    let mut slots = Vec::new();
    slots.try_reserve_exact(input.len()).map_err(|_| bad(cmd, "cannot allocate template slots"))?;
    for (index, slot) in input.iter().enumerate() {
        let slot = object(cmd, slot, &["role", "bounds"])?;
        let offset = u32::try_from(index + 2).map_err(|_| bad(cmd, NO_ID_SPACE))?;
        slots.push(PresentationTemplateSlot {
            id: PresentationTemplateSlotId(base.checked_add(offset).ok_or_else(|| bad(cmd, NO_ID_SPACE))?),
            role: role(cmd, required(cmd, slot, "role")?)?,
            bounds: bounds(cmd, required(cmd, slot, "bounds")?)?,
        });
    }
    let ids: Vec<_> = slots.iter().map(|slot| slot.id).collect();
    let resource = PresentationTemplate { id: template_id, name, display_name, slots };
    publish(s, cmd, move |doc| {
        doc.presentation_templates.try_reserve(1).map_err(|_| bad(cmd, "cannot allocate presentation template"))?;
        consume_ids(doc, cmd, base, resource.slots.len() + 1)?;
        doc.presentation_templates.push(Arc::new(resource));
        Ok(json!({"id": template_id, "slots": ids}))
    })
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "presentation.template.rename";
    let p = object(cmd, p, &["id", "name", "displayName"])?;
    let id = PresentationTemplateId(id(cmd, required(cmd, p, "id")?)?);
    let name = p.get("name").map(|value| name(cmd, value)).transpose()?;
    let display_name = p.get("displayName").map(|value| if value.is_null() { Ok(None) } else { label(cmd, value).map(Some) }).transpose()?;
    let stats = validate(s, cmd)?;
    let index = owner(s, cmd, id)?;
    let current = resource(s, cmd, index)?;
    if let Some(name) = &name {
        unique_name(s, cmd, name, Some(index))?;
    }
    let next_name = name.as_deref().unwrap_or(&current.name);
    let next_label = display_name.as_ref().map_or(current.display_name.as_deref(), |value| value.as_deref());
    if next_name == current.name && next_label == current.display_name.as_deref() {
        return Ok(json!({"id": id}));
    }
    preflight_growth(
        cmd,
        stats,
        current.name.len().saturating_add(current.display_name.as_ref().map_or(0, String::len)),
        next_name.len().saturating_add(next_label.map_or(0, str::len)),
        0,
        0,
    )?;
    publish(s, cmd, move |doc| {
        let resource = doc.presentation_templates.get_mut(index).ok_or_else(|| bad(cmd, "template owner disappeared"))?;
        let resource = Arc::make_mut(resource);
        if let Some(name) = name {
            resource.name = name;
        }
        if let Some(display_name) = display_name {
            resource.display_name = display_name;
        }
        Ok(json!({"id": id}))
    })
}

fn add_slot(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "presentation.template.addSlot";
    let p = object(cmd, p, &["id", "role", "bounds", "before"])?;
    let id = PresentationTemplateId(id(cmd, required(cmd, p, "id")?)?);
    let role = role(cmd, required(cmd, p, "role")?)?;
    let bounds = bounds(cmd, required(cmd, p, "bounds")?)?;
    let before = p.get("before").map(|value| self::id(cmd, value).map(PresentationTemplateSlotId)).transpose()?;
    let stats = validate(s, cmd)?;
    let index = owner(s, cmd, id)?;
    let current = resource(s, cmd, index)?;
    if current.slots.len() >= MAX_SLOTS_PER_TEMPLATE {
        return Err(bad(cmd, "too many slots in one template"));
    }
    let at = before.map(|slot| slot_index(current, cmd, slot)).transpose()?.unwrap_or(current.slots.len());
    preflight_growth(cmd, stats, 0, size_of::<PresentationTemplateSlot>(), 0, 1)?;
    let base = allocation(s, cmd, stats, 1)?;
    let slot = PresentationTemplateSlot { id: PresentationTemplateSlotId(base.checked_add(1).ok_or_else(|| bad(cmd, NO_ID_SPACE))?), role, bounds };
    publish(s, cmd, move |doc| {
        consume_ids(doc, cmd, base, 1)?;
        let resource = doc.presentation_templates.get_mut(index).ok_or_else(|| bad(cmd, "template owner disappeared"))?;
        let resource = Arc::make_mut(resource);
        resource.slots.try_reserve(1).map_err(|_| bad(cmd, "cannot allocate template slot"))?;
        let slot_id = slot.id;
        resource.slots.insert(at, slot);
        Ok(json!({"id": id, "slot": slot_id}))
    })
}

fn set_slot(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "presentation.template.setSlot";
    let p = object(cmd, p, &["id", "slot", "role", "bounds"])?;
    let id = PresentationTemplateId(id(cmd, required(cmd, p, "id")?)?);
    let slot = PresentationTemplateSlotId(self::id(cmd, required(cmd, p, "slot")?)?);
    let role = p.get("role").map(|value| role(cmd, value)).transpose()?;
    let bounds = p.get("bounds").map(|value| bounds(cmd, value)).transpose()?;
    validate(s, cmd)?;
    let index = owner(s, cmd, id)?;
    let current = resource(s, cmd, index)?;
    let at = slot_index(current, cmd, slot)?;
    let current = current.slots.get(at).ok_or_else(|| bad(cmd, "no such slot in this template"))?;
    if role.is_none_or(|role| role == current.role) && bounds.is_none_or(|bounds| bounds == current.bounds) {
        return Ok(json!({"id": id, "slot": slot}));
    }
    publish(s, cmd, move |doc| {
        let resource = doc.presentation_templates.get_mut(index).ok_or_else(|| bad(cmd, "template owner disappeared"))?;
        let slot = Arc::make_mut(resource).slots.get_mut(at).ok_or_else(|| bad(cmd, "slot owner disappeared"))?;
        if let Some(role) = role {
            slot.role = role;
        }
        if let Some(bounds) = bounds {
            slot.bounds = bounds;
        }
        Ok(json!({"id": id, "slot": slot.id}))
    })
}

fn reorder_slot(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "presentation.template.reorderSlot";
    let p = object(cmd, p, &["id", "slot", "index"])?;
    let id = PresentationTemplateId(id(cmd, required(cmd, p, "id")?)?);
    let slot = PresentationTemplateSlotId(self::id(cmd, required(cmd, p, "slot")?)?);
    let to = required(cmd, p, "index")?
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| bad(cmd, "index must be an unsigned integer"))?;
    validate(s, cmd)?;
    let index = owner(s, cmd, id)?;
    let current = resource(s, cmd, index)?;
    let from = slot_index(current, cmd, slot)?;
    if to >= current.slots.len() {
        return Err(bad(cmd, "slot index is outside this template"));
    }
    if from == to {
        return Ok(json!({"id": id, "slot": slot, "index": to}));
    }
    publish(s, cmd, move |doc| {
        let resource = doc.presentation_templates.get_mut(index).ok_or_else(|| bad(cmd, "template owner disappeared"))?;
        let resource = Arc::make_mut(resource);
        let slot = resource.slots.remove(from);
        let slot_id = slot.id;
        resource.slots.insert(to, slot);
        Ok(json!({"id": id, "slot": slot_id, "index": to}))
    })
}

fn remove_slot(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "presentation.template.removeSlot";
    let p = object(cmd, p, &["id", "slot"])?;
    let id = PresentationTemplateId(id(cmd, required(cmd, p, "id")?)?);
    let slot = PresentationTemplateSlotId(self::id(cmd, required(cmd, p, "slot")?)?);
    validate(s, cmd)?;
    let index = owner(s, cmd, id)?;
    let at = slot_index(resource(s, cmd, index)?, cmd, slot)?;
    publish(s, cmd, move |doc| {
        let resource = doc.presentation_templates.get_mut(index).ok_or_else(|| bad(cmd, "template owner disappeared"))?;
        Arc::make_mut(resource).slots.remove(at);
        Ok(json!({"id": id, "slot": slot}))
    })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "presentation.template.delete";
    let p = object(cmd, p, &["id"])?;
    let id = PresentationTemplateId(id(cmd, required(cmd, p, "id")?)?);
    validate(s, cmd)?;
    let index = owner(s, cmd, id)?;
    if s.doc()?.doc.slides.iter().any(|slide| slide.presentation_template == Some(id)) {
        return Err(bad(cmd, "presentation template is selected by a page; clear those selections first"));
    }
    publish(s, cmd, move |doc| {
        doc.presentation_templates.remove(index);
        Ok(json!({"id": id}))
    })
}

fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "slide.presentationTemplate";
    let p = object(cmd, p, &["slide", "template"])?;
    let selected = required(cmd, p, "template")?;
    let template = if selected.is_null() { None } else { Some(PresentationTemplateId(id(cmd, selected)?)) };
    validate(s, cmd)?;
    if let Some(template) = template {
        owner(s, cmd, template)?;
    }
    let st = s.doc()?;
    let index = match p.get("slide") {
        Some(slide) => st.doc.slide_index(SlideId(id(cmd, slide)?)).ok_or_else(|| bad(cmd, "no such slide"))?,
        None => st.selection.slide,
    };
    let slide = st.doc.slides.get(index).ok_or_else(|| bad(cmd, "no current slide"))?;
    let slide_id = slide.id;
    if slide.presentation_template == template {
        return Ok(json!({"slide": slide_id, "template": template}));
    }
    publish(s, cmd, move |doc| {
        let slide = doc.slides.get_mut(index).ok_or_else(|| bad(cmd, "slide owner disappeared"))?;
        Arc::make_mut(slide).presentation_template = template;
        Ok(json!({"slide": slide_id, "template": template}))
    })
}
