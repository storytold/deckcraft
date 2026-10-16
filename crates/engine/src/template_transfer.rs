//! Selected template-resource closure and atomic ownership transfer between documents.
//!
//! Numeric IDs, resource names and equal property values never prove shared ownership.
use std::{borrow::Cow, collections::HashSet, mem::size_of, sync::Arc};

use deckcraft_model::{
    Presentation, PresentationTemplate, PresentationTemplateId, PresentationTemplateSlot, PresentationTemplateSlotId, Slide,
    presentation_template::{self, MAX_TEMPLATE_BYTES, MAX_TEMPLATE_SCAN_WORK, MAX_TEMPLATE_SLIDE_SCAN, MAX_TEMPLATES, MAX_TOTAL_SLOTS},
};

use crate::{EngineError, Result};

#[derive(Clone, Debug, Default)]
pub(crate) struct TemplateClosure {
    // Sorted by source resource ID; snapshots survive deletion/new-document commands.
    resources: Vec<Arc<PresentationTemplate>>,
}

#[derive(Clone, Debug)]
struct Binding {
    source: Arc<PresentationTemplate>,
    destination: Arc<PresentationTemplate>,
}

/// Optional provenance retained by a compare session across separate accept operations.
#[derive(Clone, Debug, Default)]
pub(crate) struct TransferMap {
    // Sorted by source Arc address. Both strong snapshots are required for proof.
    bindings: Vec<Binding>,
}

impl TransferMap {
    pub(crate) fn pairs(&self) -> impl Iterator<Item = (&Arc<PresentationTemplate>, &Arc<PresentationTemplate>)> {
        self.bindings.iter().map(|binding| (&binding.source, &binding.destination))
    }

    pub(crate) fn maps(&self, source: &Arc<PresentationTemplate>, destination: &Arc<PresentationTemplate>) -> bool {
        self.binding(source).is_some_and(|binding| Arc::ptr_eq(&binding.destination, destination))
    }

    fn binding(&self, source: &Arc<PresentationTemplate>) -> Option<&Binding> {
        self.bindings
            .binary_search_by_key(&address(source), |binding| address(&binding.source))
            .ok()
            .and_then(|index| self.bindings.get(index))
            .filter(|binding| Arc::ptr_eq(&binding.source, source))
    }
}

fn address(owner: &Arc<PresentationTemplate>) -> usize {
    Arc::as_ptr(owner) as usize
}
fn invalid(reason: &str) -> EngineError {
    EngineError::Other(format!("presentation template transfer: {reason}"))
}

#[derive(Default)]
struct Budget {
    bytes: usize,
    work: usize,
}

impl Budget {
    fn bytes(&mut self, amount: usize) -> Result<()> {
        self.bytes = self.bytes.checked_add(amount).ok_or_else(|| invalid("byte accounting overflow"))?;
        if self.bytes > MAX_TEMPLATE_BYTES {
            return Err(invalid("byte budget exceeded"));
        }
        Ok(())
    }
    fn work(&mut self, amount: usize) -> Result<()> {
        self.work = self.work.checked_add(amount).ok_or_else(|| invalid("work accounting overflow"))?;
        if self.work > MAX_TEMPLATE_SCAN_WORK {
            return Err(invalid("scan budget exceeded"));
        }
        Ok(())
    }
    fn vector<T>(&mut self, count: usize) -> Result<Vec<T>> {
        self.bytes(count.checked_mul(size_of::<T>()).ok_or_else(|| invalid("vector accounting overflow"))?)?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| invalid("cannot allocate transfer index"))?;
        Ok(values)
    }
    fn text(&mut self, value: &str) -> Result<String> {
        self.bytes(value.len())?;
        self.work(value.len())?;
        let mut copy = String::new();
        copy.try_reserve_exact(value.len()).map_err(|_| invalid("cannot allocate resource text"))?;
        copy.push_str(value);
        Ok(copy)
    }
}

fn selected<'a>(resources: &[Arc<PresentationTemplate>], slides: impl IntoIterator<Item = &'a Slide>, budget: &mut Budget) -> Result<Vec<bool>> {
    let mut index = budget.vector::<(PresentationTemplateId, usize)>(resources.len())?;
    index.extend(resources.iter().enumerate().map(|(at, owner)| (owner.id, at)));
    index.sort_unstable_by_key(|entry| entry.0);
    let mut used = budget.vector::<bool>(resources.len())?;
    used.resize(resources.len(), false);
    let mut count = 0usize;
    for slide in slides {
        count = count.checked_add(1).ok_or_else(|| invalid("page count overflow"))?;
        if count > MAX_TEMPLATE_SLIDE_SCAN {
            return Err(invalid("too many transferred pages"));
        }
        budget.work(16)?;
        if let Some(id) = slide.presentation_template {
            let at = index
                .binary_search_by_key(&id, |entry| entry.0)
                .ok()
                .and_then(|at| index.get(at))
                .map(|entry| entry.1)
                .ok_or_else(|| invalid("transferred page selects an uncaptured template"))?;
            let flag = used.get_mut(at).ok_or_else(|| invalid("invalid source resource index"))?;
            *flag = true;
        }
    }
    Ok(used)
}

/// Capture only selected source owners, validating complete source template metadata first.
pub(crate) fn capture<'a>(source: &Presentation, slides: impl IntoIterator<Item = &'a Slide>) -> Result<TemplateClosure> {
    let stats = presentation_template::validate(source)?;
    let mut budget = Budget { bytes: stats.logical_bytes, work: stats.scan_work };
    let used = selected(&source.presentation_templates, slides, &mut budget)?;
    let mut resources = budget.vector::<Arc<PresentationTemplate>>(used.iter().filter(|used| **used).count())?;
    resources.extend(source.presentation_templates.iter().zip(used).filter(|(_, used)| *used).map(|(owner, _)| owner.clone()));
    resources.sort_unstable_by_key(|owner| owner.id);
    Ok(TemplateClosure { resources })
}

fn profile(resources: Vec<Arc<PresentationTemplate>>, slides: Vec<Arc<Slide>>) -> Presentation {
    Presentation { presentation_templates: resources, slides, ..Default::default() }
}

fn retained_bytes(owner: &PresentationTemplate, budget: &mut Budget) -> Result<()> {
    budget.bytes(size_of::<PresentationTemplate>())?;
    budget.bytes(owner.name.len())?;
    budget.bytes(owner.display_name.as_ref().map_or(0, String::len))?;
    budget
        .bytes(owner.slots.len().checked_mul(size_of::<PresentationTemplateSlot>()).ok_or_else(|| invalid("retained resource accounting overflow"))?)
}

fn validate_receipt(bindings: &[Binding]) -> Result<()> {
    if bindings.len() > MAX_TEMPLATES {
        return Err(invalid("too many retained transfer bindings"));
    }
    let mut budget = Budget::default();
    for binding in bindings {
        budget.bytes(size_of::<Binding>())?;
        retained_bytes(&binding.source, &mut budget)?;
        retained_bytes(&binding.destination, &mut budget)?;
    }
    Ok(())
}

fn fresh_name(source: &str, id: PresentationTemplateId, names: &HashSet<Cow<'_, str>>, budget: &mut Budget) -> Result<String> {
    budget.work(source.len())?;
    if !names.contains(source) {
        return budget.text(source);
    }
    // Generated ASCII names are valid NCNames and independent of arbitrary source length.
    for suffix in 0..=MAX_TEMPLATES {
        budget.work(64)?;
        let candidate = if suffix == 0 { format!("template_{}", id.0) } else { format!("template_{}_{suffix}", id.0) };
        if !names.contains(candidate.as_str()) {
            return budget.text(&candidate);
        }
    }
    Err(invalid("cannot allocate a unique resource name"))
}

/// Transfer one batch without preserving provenance between separate invocations.
pub(crate) fn remap(destination: &mut Presentation, closure: &TemplateClosure, slides: &mut [Slide]) -> Result<()> {
    remap_with(destination, closure, slides, &mut TransferMap::default())
}

/// Stage resources, identities, names, selected-page references and receipt atomically.
///
/// The caller inserts/replaces the returned-in-place slides in its surrounding transaction.
/// A comparison receipt only reuses a target while its exact owner Arc is still live.
pub(crate) fn remap_with(destination: &mut Presentation, closure: &TemplateClosure, slides: &mut [Slide], receipt: &mut TransferMap) -> Result<()> {
    let stats = presentation_template::validate(destination)?;
    validate_receipt(&receipt.bindings)?;
    let mut budget = Budget { bytes: stats.logical_bytes, work: stats.scan_work };
    let mut source_owners = budget.vector::<Arc<PresentationTemplate>>(closure.resources.len())?;
    source_owners.extend(closure.resources.iter().cloned());
    let source = profile(source_owners, vec![]);
    let source_stats = presentation_template::validate(&source)?;
    budget.work(source_stats.scan_work)?;
    let used = selected(&closure.resources, slides.iter(), &mut budget)?;
    if !used.iter().any(|used| *used) {
        return Ok(());
    }

    let mut live = budget.vector::<(usize, usize)>(destination.presentation_templates.len())?;
    live.extend(destination.presentation_templates.iter().enumerate().map(|(index, owner)| (address(owner), index)));
    live.sort_unstable_by_key(|entry| entry.0);
    let live_owner = |owner: &Arc<PresentationTemplate>| {
        live.binary_search_by_key(&address(owner), |entry| entry.0)
            .ok()
            .and_then(|at| live.get(at))
            .and_then(|entry| destination.presentation_templates.get(entry.1))
            .filter(|candidate| Arc::ptr_eq(candidate, owner))
    };
    let mut mapping = budget.vector::<(PresentationTemplateId, Arc<PresentationTemplate>)>(closure.resources.len())?;
    let mut pending = budget.vector::<Arc<PresentationTemplate>>(closure.resources.len())?;
    for (owner, used) in closure.resources.iter().zip(used) {
        if !used {
            continue;
        }
        let existing = live_owner(owner).or_else(|| {
            let candidate = &receipt.binding(owner)?.destination;
            live_owner(candidate).filter(|target| receipt.maps(owner, target))
        });
        if let Some(existing) = existing {
            mapping.push((owner.id, existing.clone()));
        } else {
            pending.push(owner.clone());
        }
    }
    let count = destination.presentation_templates.len().checked_add(pending.len()).ok_or_else(|| invalid("resource count overflow"))?;
    if count > MAX_TEMPLATES {
        return Err(invalid("too many destination template resources"));
    }
    let mut slot_count = stats.slot_count;
    let mut allocation_count = pending.len();
    for owner in &pending {
        slot_count = slot_count.checked_add(owner.slots.len()).ok_or_else(|| invalid("slot count overflow"))?;
        allocation_count = allocation_count.checked_add(owner.slots.len()).ok_or_else(|| invalid("ID count overflow"))?;
    }
    if slot_count > MAX_TOTAL_SLOTS {
        return Err(invalid("too many destination template slots"));
    }
    // The document's existing general allocator invariant remains binding; also protect stale template high-water values.
    let mut allocator = Presentation { next_id: destination.next_id.max(stats.max_id), ..Default::default() };
    let allocations = u32::try_from(allocation_count).map_err(|_| invalid("ID count overflow"))?;
    if allocations > 0 && allocator.next_id.max(1).checked_add(allocations).is_none() {
        return Err(invalid("document ID space exhausted"));
    }

    let mut owners = budget.vector::<Arc<PresentationTemplate>>(count)?;
    owners.extend(destination.presentation_templates.iter().cloned());
    // Keys borrow destination names; only newly created names require owned key copies.
    budget.bytes(count.checked_mul(64).ok_or_else(|| invalid("name index accounting overflow"))?)?;
    let mut names: HashSet<Cow<'_, str>> = HashSet::new();
    names.try_reserve(count).map_err(|_| invalid("cannot allocate resource name index"))?;
    for owner in &destination.presentation_templates {
        budget.work(owner.name.len())?;
        names.insert(Cow::Borrowed(&owner.name));
    }
    let mut bindings =
        budget.vector::<Binding>(receipt.bindings.len().checked_add(pending.len()).ok_or_else(|| invalid("binding count overflow"))?)?;
    bindings.extend(receipt.bindings.iter().cloned());
    for owner in &pending {
        budget.bytes(size_of::<PresentationTemplate>() + 2 * size_of::<usize>())?;
        let id = PresentationTemplateId(allocator.alloc_id().ok_or_else(|| invalid("document ID space exhausted"))?);
        let name = fresh_name(&owner.name, id, &names, &mut budget)?;
        names.insert(Cow::Owned(budget.text(&name)?));
        let display_name = owner.display_name.as_deref().map(|label| budget.text(label)).transpose()?;
        let mut slots = budget.vector::<PresentationTemplateSlot>(owner.slots.len())?;
        for slot in &owner.slots {
            let id = PresentationTemplateSlotId(allocator.alloc_id().ok_or_else(|| invalid("document ID space exhausted"))?);
            slots.push(PresentationTemplateSlot { id, role: slot.role, bounds: slot.bounds });
        }
        let copied = Arc::new(PresentationTemplate { id, name, display_name, slots });
        if let Some(binding) =
            receipt.bindings.binary_search_by_key(&address(owner), |binding| address(&binding.source)).ok().and_then(|at| bindings.get_mut(at))
        {
            binding.destination = copied.clone();
        } else {
            bindings.push(Binding { source: owner.clone(), destination: copied.clone() });
        }
        mapping.push((owner.id, copied.clone()));
        owners.push(copied);
    }
    drop(names);
    mapping.sort_unstable_by_key(|entry| entry.0);
    bindings.sort_unstable_by_key(|binding| address(&binding.source));
    validate_receipt(&bindings)?;

    let combined = destination.slides.len().checked_add(slides.len()).ok_or_else(|| invalid("page count overflow"))?;
    if combined > MAX_TEMPLATE_SLIDE_SCAN {
        return Err(invalid("too many destination and transferred page references"));
    }
    let mut projected = budget.vector::<Arc<Slide>>(combined)?;
    projected.extend(destination.slides.iter().cloned());
    let mut proxies = budget.vector::<Arc<Slide>>(mapping.len())?;
    for (_, owner) in &mapping {
        budget.bytes(size_of::<Slide>() + 2 * size_of::<usize>())?;
        proxies.push(Arc::new(Slide { presentation_template: Some(owner.id), ..Default::default() }));
    }
    budget.bytes(size_of::<Slide>() + 2 * size_of::<usize>())?;
    let plain = Arc::new(Slide::default());
    let mut selections = budget.vector::<Option<PresentationTemplateId>>(slides.len())?;
    for slide in slides.iter() {
        if let Some(id) = slide.presentation_template {
            let at = mapping.binary_search_by_key(&id, |entry| entry.0).map_err(|_| invalid("missing prepared resource mapping"))?;
            let proxy = proxies.get(at).ok_or_else(|| invalid("missing prepared page proxy"))?;
            selections.push(proxy.presentation_template);
            projected.push(proxy.clone());
        } else {
            selections.push(None);
            projected.push(plain.clone());
        }
    }
    let mut projected = profile(owners, projected);
    presentation_template::validate(&projected)?;
    // No fallible operations after publishing any caller-owned state.
    destination.presentation_templates = std::mem::take(&mut projected.presentation_templates);
    if allocation_count > 0 {
        destination.next_id = allocator.next_id;
    }
    for (slide, selected) in slides.iter_mut().zip(selections) {
        slide.presentation_template = selected;
    }
    receipt.bindings = bindings;
    Ok(())
}
