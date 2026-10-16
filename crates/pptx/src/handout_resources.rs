//! Unused typed handout layouts. No visible handout object is restored from metadata.
use super::{Exp, Out};
use crate::{
    PptxError,
    notes_codec::{self, digest, invalid},
    notes_records::{Dependency, MediaBinding},
    opc,
    xml::El,
};
use deckcraft_model::{Layout, LayoutId, Master, MediaId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    projection: String,
    relationships: String,
    layouts: Vec<Layout>,
    media: Vec<MediaBinding>,
    slides: Vec<(u32, String)>,
    dependencies: Vec<Dependency>,
}

fn validate(master: &Master, layouts: &[Layout]) -> Result<(), PptxError> {
    // Bound before creating the one temporary typed resource owner. Core shapes,
    // text and backgrounds are never copied into the canonical unused ledger.
    let mut bytes = crate::notes_budget::Counter::new(notes_codec::MAX_BYTES);
    bytes.value(layouts)?;
    bytes.value(&master.theme)?;
    let owner = Master { theme: master.theme.clone(), layouts: layouts.to_vec(), ..Default::default() };
    crate::notes_projection::validate_master(&owner)
}

pub(crate) fn write(x: &mut Exp<'_>, out: &mut Out, master: &Master, core: &[u8]) -> Result<Vec<u8>, PptxError> {
    if master.layouts.is_empty() {
        return Ok(core.to_vec());
    }
    validate(master, &master.layouts)?;
    if x.p.slides.len() > 10_000 {
        return Err(invalid("handout resource slide owner count exceeds limit"));
    }
    let mut slide_ids = HashSet::new();
    if x.p.slides.iter().any(|slide| !slide_ids.insert(slide.id)) {
        return Err(invalid("duplicate handout source slide identity"));
    }
    let slides: Vec<_> =
        x.p.slides
            .iter()
            .map(|slide| x.slide_parts.get(&slide.id).map(|part| (slide.id.0, part.clone())).ok_or_else(|| invalid("missing handout resource slide")))
            .collect::<Result<_, _>>()?;
    let slide_ids = slides.iter().map(|(id, _)| (*id, *id)).collect();
    crate::notes_projection::validate_slide_links(&master.layouts, &slide_ids)?;
    let mut needed = HashSet::new();
    for layout in &master.layouts {
        crate::notes_media::visit::layout(layout, &mut |id| {
            needed.insert(*id);
        });
    }
    if needed.len() > 10_000 {
        return Err(invalid("handout layout media count exceeds limit"));
    }
    // Stable traversal order, independent of hash randomization.
    let mut needed: Vec<_> = needed.into_iter().collect();
    needed.sort_by_key(|id| id.0);
    let mut media = Vec::new();
    let mut bytes = 0usize;
    let mut metadata = crate::notes_budget::Counter::new(notes_codec::MAX_BYTES);
    for id in needed {
        let item = x.p.media(id).ok_or_else(|| invalid("missing handout layout media"))?;
        metadata.value(&item.name)?;
        metadata.value(&item.content_type)?;
        metadata.value(&item.link)?;
        if item.content_type.is_empty() || !item.content_type.contains('/') || item.content_type == "application/vnd.deckcraft.odp-source" {
            return Err(invalid("invalid handout layout media type"));
        }
        bytes = bytes.checked_add(item.data.len()).ok_or_else(|| invalid("handout layout media size overflow"))?;
        if item.data.len() > 32 * 1024 * 1024 || bytes > 128 * 1024 * 1024 {
            return Err(invalid("handout layout media size limit"));
        }
        let kind = if item.content_type.starts_with("image/") {
            "image"
        } else if item.content_type.starts_with("audio/") {
            "audio"
        } else if item.content_type.starts_with("video/") {
            "video"
        } else {
            "package"
        };
        let name = item.name.clone();
        let content_type = item.content_type.clone();
        let link = item.link.clone();
        let dependency = if item.data.is_empty() {
            None
        } else {
            let dependency =
                Dependency { part: String::new(), content_type: content_type.clone(), bytes: item.data.len(), sha256: digest(&item.data) };
            let part = x.media_part(id).ok_or_else(|| invalid("cannot emit handout layout media"))?;
            Some(Dependency { part, ..dependency })
        };
        if dependency.is_none() && link.is_none() {
            return Err(invalid("empty handout layout media has no link"));
        }
        let relation = if dependency.is_none() {
            out.rels.add_new(kind, link.as_deref().ok_or_else(|| invalid("missing external handout layout media"))?, true)
        } else {
            x.media_rel(out, id, kind).ok_or_else(|| invalid("cannot bind handout layout media"))?
        };
        media.push(MediaBinding {
            original: id.0,
            relation,
            name,
            content_type,
            original_link: link.clone(),
            external: dependency.is_none().then_some(link).flatten(),
            dependency,
        });
    }
    let root = crate::xml::parse(core)?.root;
    let record = Record {
        projection: notes_codec::projection(&root)?,
        relationships: digest(&out.rels.to_xml()),
        layouts: master.layouts.clone(),
        media,
        slides,
        dependencies: super::notes::dependency_closure(x, out, None)?,
    };
    super::notes::append_extension(root, "handout-layouts", &record)
}

fn actual_dependencies(pkg: &opc::Package, part: &crate::read::Part) -> Result<Vec<Dependency>, PptxError> {
    let mut pending: Vec<_> = part
        .rels
        .items
        .iter()
        .filter(|rel| !rel.external && !matches!(rel.kind.as_str(), "slide" | "notesMaster"))
        .map(|rel| rel.target.clone())
        .collect();
    let mut seen = HashSet::new();
    let mut bytes = 0usize;
    let mut result = Vec::new();
    while let Some(part) = pending.pop() {
        if !seen.insert(opc::norm(&part)) {
            continue;
        }
        if seen.len() > 50_000 {
            return Err(invalid("handout dependency count limit"));
        }
        let data = pkg.get(&part).ok_or_else(|| invalid("handout dependency was deleted"))?;
        bytes = bytes.checked_add(data.len()).ok_or_else(|| invalid("handout dependency byte overflow"))?;
        if bytes > 128 * 1024 * 1024 {
            return Err(invalid("handout dependency byte limit"));
        }
        let name = pkg.original_name(&part).ok_or_else(|| invalid("handout dependency identity missing"))?;
        result.push(Dependency {
            part: name.to_string(),
            content_type: pkg.content_type(&part).ok_or_else(|| invalid("handout dependency content type missing"))?,
            bytes: data.len(),
            sha256: digest(data),
        });
        let rels = opc::rels_path(name);
        if pkg.get(&rels).is_some() {
            pending.push(rels);
            for rel in pkg.rels(name).items {
                if !rel.external && !matches!(rel.kind.as_str(), "slide" | "notesMaster") {
                    pending.push(rel.target);
                }
            }
        }
    }
    result.sort_by(|a, b| a.part.cmp(&b.part));
    Ok(result)
}

fn allocate(imp: &mut crate::read::Imp<'_>) -> Result<u32, PptxError> {
    imp.next = imp.next.checked_add(1).ok_or_else(|| invalid("handout resource identity space exhausted"))?;
    Ok(imp.next)
}

pub(crate) fn restore(imp: &mut crate::read::Imp<'_>, master: &mut Master, part: &crate::read::Part, root: &El) -> Result<(), PptxError> {
    let Some(record) = notes_codec::read_record::<Record>(root, "handout-layouts")? else {
        return Ok(());
    };
    let relationships =
        opc::RelsOut { items: part.rels.items.iter().map(|rel| (rel.id.clone(), rel.ty.clone(), rel.target_raw.clone(), rel.external)).collect() };
    if notes_codec::projection(root)? != record.projection || digest(&relationships.to_xml()) != record.relationships {
        return Err(invalid("handout core or relationships changed; unused resource metadata is stale"));
    }
    if actual_dependencies(imp.pkg, part)? != record.dependencies {
        return Err(invalid("handout actual dependency closure changed"));
    }
    if record.media.len() > 10_000 || record.slides.len() > 10_000 {
        return Err(invalid("handout resource owner limit"));
    }
    validate(master, &record.layouts)?;
    let mut shape_count = 0usize;
    for layout in &record.layouts {
        deckcraft_model::walk(&layout.shapes, &mut |_, _| shape_count = shape_count.saturating_add(1));
    }
    let identities = shape_count
        .checked_add(record.layouts.len())
        .and_then(|n| n.checked_add(record.media.len()))
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| invalid("handout resource identity count overflow"))?;
    imp.next.checked_add(identities).ok_or_else(|| invalid("handout resource identity space exhausted"))?;
    let mut slides = HashMap::new();
    let mut actual_slides = HashSet::new();
    for (original, part) in &record.slides {
        let actual = imp.slide_by_part.get(&opc::norm(part)).ok_or_else(|| invalid("handout slide target was deleted"))?;
        if !actual_slides.insert(*actual) {
            return Err(invalid("duplicate actual handout slide owner"));
        }
        if slides.insert(*original, actual.0).is_some() {
            return Err(invalid("duplicate handout original slide identity"));
        }
    }
    if slides.len() != imp.slide_by_part.len() {
        return Err(invalid("handout slide owner set changed"));
    }
    let dependencies: HashMap<_, _> = record.dependencies.iter().map(|dependency| (opc::norm(&dependency.part), dependency)).collect();
    let mut media = HashMap::new();
    let mut relations = HashSet::new();
    for binding in &record.media {
        if binding.content_type.is_empty() || !binding.content_type.contains('/') || binding.content_type == "application/vnd.deckcraft.odp-source" {
            return Err(invalid("invalid handout layout media type"));
        }
        if media.contains_key(&MediaId(binding.original)) || !relations.insert(&binding.relation) {
            return Err(invalid("duplicate handout media binding"));
        }
        let mut matching = part.rels.items.iter().filter(|rel| rel.id == binding.relation);
        let rel = matching.next().ok_or_else(|| invalid("missing handout layout relationship"))?;
        if matching.next().is_some() {
            return Err(invalid("ambiguous handout layout relationship"));
        }
        let id = if let Some(expected) = &binding.dependency {
            if expected.bytes > 32 * 1024 * 1024 {
                return Err(invalid("handout layout media size limit"));
            }
            if rel.external
                || opc::norm(&rel.target) != opc::norm(&expected.part)
                || dependencies.get(&opc::norm(&expected.part)).copied() != Some(expected)
            {
                return Err(invalid("handout layout media dependency mismatch"));
            }
            imp.media_for(&rel.target).ok_or_else(|| invalid("missing handout layout media part"))?
        } else {
            let external = binding.external.as_deref().ok_or_else(|| invalid("missing external handout layout media"))?;
            if !rel.external || rel.target != external || binding.original_link.as_deref() != Some(external) {
                return Err(invalid("external handout layout relationship changed"));
            }
            // URL equality does not establish native identity. Keep distinct
            // original resources and avoid renaming a visible core media item.
            let id = MediaId(allocate(imp)?);
            imp.media.push(deckcraft_model::MediaItem {
                id,
                name: binding.name.clone(),
                content_type: binding.content_type.clone(),
                data: Arc::new(Vec::new()),
                link: Some(external.to_string()),
            });
            id
        };
        let item = imp.media.iter_mut().find(|item| item.id == id).ok_or_else(|| invalid("missing imported handout media"))?;
        if binding.dependency.is_some() && item.content_type != binding.content_type {
            return Err(invalid("handout media type changed"));
        }
        item.content_type.clone_from(&binding.content_type);
        item.name.clone_from(&binding.name);
        item.link.clone_from(&binding.original_link);
        media.insert(MediaId(binding.original), id);
    }
    let mut layouts = record.layouts;
    for layout in &mut layouts {
        layout.id = LayoutId(allocate(imp)?);
        crate::notes_projection::remap_shape_ids(&mut layout.shapes, &mut || allocate(imp))?;
        let mut missing = false;
        crate::notes_media::visit_mut::layout(layout, &mut |id| {
            if let Some(new) = media.get(id) {
                *id = *new;
            } else {
                missing = true;
            }
        });
        if missing {
            return Err(invalid("unbound handout layout media"));
        }
        crate::notes_projection::remap_slide_links(layout, &slides)?;
    }
    validate(master, &layouts)?;
    master.layouts = layouts;
    Ok(())
}
