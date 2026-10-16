//! Ordinary structured notes import and atomic validated DeckCraft semantic restoration.
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use deckcraft_model::{
    Background, LayoutId, Master, MasterId, MediaId, PhType, Presentation, Shape, SlideId,
    notes::{NotesDateTime, NotesFields, NotesMaster, NotesPage},
    text::{RunKind, TextBody},
};

use super::{Imp, Part, background, color_map, dml, load, shapes, theme};
use crate::{
    PptxError,
    notes_codec::{self, digest, invalid},
    notes_projection::{self, MAX_PAGES, MAX_SHAPES},
    notes_records::{Dependency, MasterRecord, PageRecord},
    opc::{self, Package},
    xml::{El, Node},
};

struct CorePage {
    index: usize,
    part: Part,
    root: El,
    context: shapes::IdCtx,
    record: Option<PageRecord>,
}

/// Root integration: call once after slides/regular masters have been read, in place
/// of both the old per-slide read_notes and old first-notes-master-only block.
/// It publishes a restored candidate only after every binding/projection validates.
pub(super) fn read_all(imp: &mut Imp<'_>, p: &mut Presentation, pres: &Part, slide_parts: &[(u32, String, SlideId)]) -> Result<(), PptxError> {
    if slide_parts.len() > MAX_PAGES {
        return Err(invalid("notes slide count exceeds limit"));
    }
    let masters: Vec<_> = pres.rels.all_of("notesMaster").collect();
    if masters.len() > 1 {
        return Err(invalid("package has multiple core notes masters"));
    }
    let mut master_data = None;
    let mut core_base = None;
    if let Some(relation) = masters.first() {
        if relation.external {
            return Err(invalid("external notes master relationship"));
        }
        let doc = load(imp.pkg, &relation.target).ok_or_else(|| invalid("missing notes master part"))?;
        let part = Part::new(imp.pkg, &relation.target, &doc);
        let (master, fields) = core_master(imp, &part, &doc.root)?;
        let record = notes_codec::read_record::<MasterRecord>(&doc.root, "master")?;
        let master = Arc::new(master);
        core_base = Some(master.clone());
        p.notes_master = None;
        p.notes_masters.push(NotesMaster { master, size: p.notes_size, fields: fields.clone() });
        master_data = Some((part, doc.root, fields, record));
    }
    let mut pages = Vec::new();
    for (_, slide_part, id) in slide_parts {
        let Some(index) = p.slides.iter().position(|slide| slide.id == *id) else { continue };
        let rels = imp.pkg.rels(slide_part);
        let related: Vec<_> = rels.all_of("notesSlide").collect();
        if related.len() > 1 {
            return Err(invalid("slide has multiple notes parts"));
        }
        let Some(relation) = related.first() else { continue };
        if relation.external {
            return Err(invalid("external notes slide relationship"));
        }
        let doc = load(imp.pkg, &relation.target).ok_or_else(|| invalid("missing notes slide part"))?;
        let part = Part::new(imp.pkg, &relation.target, &doc);
        let owners: Vec<_> = part.rels.all_of("slide").collect();
        if owners.len() != 1 || owners.first().is_none_or(|owner| owner.external || opc::norm(&owner.target) != opc::norm(slide_part)) {
            return Err(invalid("notes live preview does not point to its owning slide"));
        }
        let master_rels: Vec<_> = part.rels.all_of("notesMaster").collect();
        if let Some((master, ..)) = &master_data {
            if master_rels.len() != 1 || master_rels.first().is_none_or(|owner| owner.external || opc::norm(&owner.target) != opc::norm(&master.name))
            {
                return Err(invalid("notes page does not point to the registered single master"));
            }
        } else if !master_rels.is_empty() {
            return Err(invalid("unregistered notes master"));
        }
        let record = notes_codec::read_record::<PageRecord>(&doc.root, "page")?;
        if record.is_some() {
            raw_tree(&doc.root)?;
        }
        let mut context = shapes::IdCtx { fresh: true, ..Default::default() };
        let (mut page, notes) = core_page(imp, &part, &doc.root, &mut context)?;
        page.master = core_base.as_ref().map(|master| master.id);
        if let Some(relation) = part.rels.first_of("themeOverride") {
            let base = core_base.as_deref().cloned().unwrap_or_default();
            let mut master = base;
            master.id = MasterId(allocate(imp)?);
            master.theme = read_override(imp, &relation.target, &master.theme)?;
            if let Some(mapping) = doc.root.path(&["clrMapOvr", "overrideClrMapping"]) {
                master.color_map = color_map(mapping);
            }
            page.master = Some(master.id);
            p.notes_masters.push(NotesMaster {
                master: Arc::new(master),
                size: p.notes_size,
                fields: master_data.as_ref().map(|(_, _, fields, _)| fields.clone()).unwrap_or_default(),
            });
        }
        let slide = p.slides.get_mut(index).ok_or_else(|| invalid("notes owning slide disappeared"))?;
        let slide = Arc::make_mut(slide);
        slide.notes = notes;
        slide.notes_page = Some(page);
        pages.push(CorePage { index, part, root: doc.root, context, record });
    }
    crate::write::table_projection::restore_all(imp, p)?;
    let Some((part, root, _, Some(record))) = master_data else {
        if pages.iter().any(|page| page.record.is_some()) {
            return Err(invalid("orphan notes semantic page record"));
        }
        return Ok(());
    };
    raw_tree(&root)?;
    if record.projection != notes_codec::projection(&root)? || record.relationships != relationships_digest(&part) {
        return Err(invalid("standard notes-master projection changed; semantic metadata is stale"));
    }
    if crate::write::notes::expected_master_projection()? != notes_codec::projection(&root)? {
        return Err(invalid("native notes metadata does not reproduce the actual neutral master"));
    }
    let mut validated_dependencies = HashMap::new();
    dependency(imp.pkg, &record.theme, &mut validated_dependencies)?;
    let resource_digest = digest(&serde_json::to_vec(&record.resources).map_err(invalid)?);
    if !deckcraft_model::notes::valid_size(record.resources.size) || record.resources.named.len() > MAX_PAGES {
        return Err(invalid("invalid native notes resource limits"));
    }
    let mut slide_map = HashMap::new();
    let mut slide_names = HashMap::new();
    let mut original_ids = HashSet::new();
    if record.resources.slides.len() != p.slides.len() {
        return Err(invalid("slide set changed; notes semantic metadata is stale"));
    }
    for binding in &record.resources.slides {
        if !original_ids.insert(binding.original) {
            return Err(invalid("duplicate original notes slide identity"));
        }
        let id = imp.slide_by_part.get(&opc::norm(&binding.part)).copied().ok_or_else(|| invalid("notes-bound slide was deleted or renamed"))?;
        slide_map.insert(binding.original, id.0);
        slide_names.insert(id, binding.part.clone());
        let actual = imp.pkg.rels(&binding.part);
        let notes: Vec<_> = actual.all_of("notesSlide").collect();
        match &binding.notes {
            Some(expected) if notes.len() == 1 && notes.first().is_some_and(|rel| !rel.external && opc::norm(&rel.target) == opc::norm(expected)) => {
            }
            None if notes.is_empty() => {}
            _ => return Err(invalid("notes page set changed; semantic metadata is stale")),
        }
    }
    let mut media = HashMap::new();
    let mut media_parts = HashMap::new();
    let mut seed_dependencies = HashMap::new();
    let mut media_bytes = 0usize;
    if record.resources.media.len() > 10_000 {
        return Err(invalid("notes media count limit"));
    }
    for binding in &record.resources.media {
        if media.contains_key(&MediaId(binding.original)) {
            return Err(invalid("duplicate original notes media identity"));
        }
        let relation = unique_relation(&part, &binding.relation)?;
        let id = if let Some(expected) = &binding.dependency {
            if relation.external || opc::norm(&relation.target) != opc::norm(&expected.part) {
                return Err(invalid("notes media relationship changed"));
            }
            dependency(imp.pkg, expected, &mut validated_dependencies)?;
            seed_dependencies.insert(opc::norm(&expected.part), expected.clone());
            media_bytes = media_bytes.checked_add(expected.bytes).ok_or_else(|| invalid("notes media byte overflow"))?;
            if expected.bytes > 32 * 1024 * 1024 || media_bytes > 128 * 1024 * 1024 {
                return Err(invalid("notes media budget exceeded"));
            }
            let id = imp.media_for(&relation.target).ok_or_else(|| invalid("notes media part is missing"))?;
            media_parts.insert(id, relation.target.clone());
            id
        } else {
            let link = binding.external.as_deref().ok_or_else(|| invalid("missing external notes media binding"))?;
            if !relation.external || relation.target != link {
                return Err(invalid("external notes media relationship changed"));
            }
            imp.media_link(link)
        };
        if let Some(item) = imp.media.iter_mut().find(|item| item.id == id) {
            if binding.dependency.is_some() && item.content_type != binding.content_type {
                return Err(invalid("notes media content type changed"));
            }
            if binding.dependency.is_none() {
                item.content_type.clone_from(&binding.content_type);
            }
            item.name.clone_from(&binding.name);
            item.link.clone_from(&binding.original_link);
        }
        media.insert(MediaId(binding.original), id);
    }
    // Every page must match before an unpublished candidate is reconstructed.
    for page in &pages {
        let record = page.record.as_ref().ok_or_else(|| invalid("notes semantic page record was deleted"))?;
        record.counters.validate()?;
        if record.resources != resource_digest
            || record.projection != notes_codec::projection(&page.root)?
            || record.relationships != relationships_digest(&page.part)
        {
            return Err(invalid("standard notes-page projection changed; semantic metadata is stale"));
        }
        dependency(imp.pkg, &record.theme, &mut validated_dependencies)?;
        if record.dependencies.len() > 50_000 {
            return Err(invalid("notes generated dependency count exceeds limit"));
        }
        let mut names = HashSet::new();
        for expected in &record.dependencies {
            if !names.insert(opc::norm(&expected.part)) {
                return Err(invalid("duplicate notes generated dependency"));
            }
            dependency(imp.pkg, expected, &mut validated_dependencies)?;
        }
        let override_part = Part { name: record.theme.part.clone(), rels: imp.pkg.rels(&record.theme.part), ns: vec![], inner_ns: vec![] };
        if relationships_digest(&override_part) != record.theme_relationships {
            return Err(invalid("notes theme relationships changed"));
        }
        if slide_map.get(&record.original_slide).copied() != p.slides.get(page.index).map(|slide| slide.id.0) {
            return Err(invalid("notes semantic record moved to another slide"));
        }
    }
    let mut candidate = p.clone();
    candidate.media = imp.media.clone();
    candidate.notes_master = record.resources.legacy.clone();
    candidate.notes_masters = record.resources.named.clone();
    candidate.notes_size = record.resources.size;
    // Attach original page skeletons before deriving their structural table owner set.
    // Canonical table text/cache is restored only from the independently verified core.
    for page in &pages {
        let record = page.record.as_ref().ok_or_else(|| invalid("missing notes table binding record"))?;
        let slide = candidate.slides.get_mut(page.index).ok_or_else(|| invalid("missing notes table slide"))?;
        Arc::make_mut(slide).notes_page = record.original.clone();
    }
    restore_tables(&mut candidate, p, &pages, &record.resources.table_sources)?;
    let mut master_map = HashMap::new();
    if let Some(master) = &mut candidate.notes_master {
        let master = Arc::make_mut(master);
        remap_master(imp, master, &media, &slide_map, &mut master_map)?;
    }
    for notes in &mut candidate.notes_masters {
        if !deckcraft_model::notes::valid_size(notes.size) || !deckcraft_model::notes::valid_fields(&notes.fields) {
            return Err(invalid("invalid native notes master fields or size"));
        }
        notes_projection::validate_fields(&notes.fields)?;
        remap_master(imp, Arc::make_mut(&mut notes.master), &media, &slide_map, &mut master_map)?;
    }
    for page in &pages {
        let record = page.record.as_ref().ok_or_else(|| invalid("missing notes page record"))?;
        let current = candidate.slides.get(page.index).ok_or_else(|| invalid("missing notes owning slide"))?;
        let core_body = p.slides.get(page.index).and_then(|slide| slide.notes_page.as_ref()).and_then(|page| page.body_shape);
        if page.context.lookup(record.body_file_id) != core_body {
            return Err(invalid("canonical notes body binding changed"));
        }
        let mut native = current.notes_page.clone();
        if let Some(native) = &mut native {
            notes_projection::validate_shapes(&native.shapes)?;
            if let Some(id) = native.master {
                native.master = Some(*master_map.get(&id).ok_or_else(|| invalid("native notes master is missing"))?);
            }
            let ids = notes_projection::remap_shape_ids(&mut native.shapes, &mut || allocate(imp))?;
            if let Some(body) = native.body_shape {
                native.body_shape = Some(*ids.get(&body).ok_or_else(|| invalid("native canonical body is missing"))?);
            }
            rewrite_media_page(native, &media)?;
            notes_projection::remap_slide_links(native, &slide_map)?;
        }
        let mut notes = record.body_style.restore(&current.notes)?;
        rewrite_media_text(&mut notes, &media)?;
        notes_projection::remap_slide_links(&mut notes, &slide_map)?;
        let slide = candidate.slides.get_mut(page.index).ok_or_else(|| invalid("missing notes owning slide"))?;
        let slide = Arc::make_mut(slide);
        slide.notes = notes;
        slide.notes_page = native;
        if let Some(error) = candidate.notes_page_error(candidate.slides.get(page.index).ok_or_else(|| invalid("missing notes slide"))?) {
            return Err(invalid(error));
        }
    }
    deckcraft_model::table::validate_constants(&candidate).map_err(invalid)?;
    // Independently derive the expected core projection from restored typed semantics.
    // Trusting only a payload-supplied digest would permit invented hidden state.
    for page in &pages {
        let slide = candidate.slides.get(page.index).ok_or_else(|| invalid("missing projection slide"))?;
        let slide_part = slide_names.get(&slide.id).ok_or_else(|| invalid("missing projection owner"))?;
        let record = page.record.as_ref().ok_or_else(|| invalid("missing notes page dependency record"))?;
        let (expected, relations, dependencies) = crate::write::notes::expected_page(
            &candidate,
            page.index,
            &resource_digest,
            media_parts.clone(),
            slide_names.clone(),
            &page.part.name,
            slide_part,
            &record.counters,
            &seed_dependencies,
        )?;
        if expected != notes_codec::projection(&page.root)? || relations != relationships_digest(&page.part) || dependencies != record.dependencies {
            return Err(invalid(format!(
                "native notes metadata does not reproduce the actual standard projection (page {}, core {}, relationships {}, dependencies {})",
                page.index,
                expected == notes_codec::projection(&page.root)?,
                relations == relationships_digest(&page.part),
                dependencies == record.dependencies
            )));
        }
    }
    candidate.next_id = imp.next;
    candidate.fix_next_id();
    *p = candidate;
    Ok(())
}

fn restore_tables(
    candidate: &mut Presentation,
    core: &Presentation,
    pages: &[CorePage],
    sources: &[crate::notes_records::TableSource],
) -> Result<(), PptxError> {
    let mut declared = HashMap::new();
    for source in sources {
        if source.layout.is_some() && source.projected {
            return Err(invalid("unused notes layout table cannot acquire a projected authority"));
        }
        if declared.insert((source.master, source.layout, source.shape), source.projected).is_some() {
            return Err(invalid("duplicate original notes table source"));
        }
    }
    let mut actual_sources = HashSet::new();
    for master in candidate.notes_master.iter().map(Arc::as_ref).chain(candidate.notes_masters.iter().map(|notes| notes.master.as_ref())) {
        deckcraft_model::walk(&master.shapes, &mut |shape, _| {
            if matches!(shape.kind, deckcraft_model::ShapeKind::Table(_)) {
                actual_sources.insert((master.id.0, None, shape.id.0));
            }
        });
        for layout in &master.layouts {
            deckcraft_model::walk(&layout.shapes, &mut |shape, _| {
                if matches!(shape.kind, deckcraft_model::ShapeKind::Table(_)) {
                    actual_sources.insert((master.id.0, Some(layout.id.0), shape.id.0));
                }
            });
        }
    }
    if declared.len() != actual_sources.len() || declared.keys().any(|key| !actual_sources.contains(key)) {
        return Err(invalid("notes table source classification is incomplete"));
    }
    let mut restored = HashSet::new();
    for page in pages {
        let record = page.record.as_ref().ok_or_else(|| invalid("missing notes table bridge"))?;
        let expected = notes_projection::project_one(candidate, page.index)?;
        let wanted: HashSet<_> = expected.tables.iter().map(|binding| (binding.master, binding.original_shape)).collect();
        let provided: HashSet<_> = record.tables.iter().map(|binding| (binding.master, binding.original_shape)).collect();
        if wanted != provided || provided.len() != record.tables.len() {
            return Err(invalid("notes table semantic owner binding changed"));
        }
        for binding in &record.tables {
            let id = page.context.lookup(binding.projected_shape).ok_or_else(|| invalid("projected notes table was deleted"))?;
            let drawings = core
                .slides
                .get(page.index)
                .and_then(|slide| slide.notes_page.as_ref())
                .ok_or_else(|| invalid("core notes table page was deleted"))?;
            let shape = deckcraft_model::find_shape(&drawings.shapes, id).ok_or_else(|| invalid("core notes table owner was deleted"))?;
            let deckcraft_model::ShapeKind::Table(current) = &shape.kind else {
                return Err(invalid("core notes table changed kind"));
            };
            let target = if let Some(master) = binding.master {
                if declared.get(&(master, None, binding.original_shape)) != Some(&true) {
                    return Err(invalid("unused notes table cannot acquire a projected text authority"));
                }
                if !restored.insert((master, None, binding.original_shape)) {
                    continue;
                }
                let target = candidate
                    .notes_master
                    .iter_mut()
                    .find(|m| m.id.0 == master)
                    .or_else(|| candidate.notes_masters.iter_mut().find(|m| m.master.id.0 == master).map(|m| &mut m.master))
                    .ok_or_else(|| invalid("original notes table master is missing"))?;
                deckcraft_model::find_shape_mut(&mut Arc::make_mut(target).shapes, deckcraft_model::ShapeId(binding.original_shape))
                    .ok_or_else(|| invalid("original shared notes table was deleted"))?
            } else {
                let slide = candidate.slides.get_mut(page.index).ok_or_else(|| invalid("original notes table page is missing"))?;
                let page = Arc::make_mut(slide).notes_page.as_mut().ok_or_else(|| invalid("original local notes table page is missing"))?;
                deckcraft_model::find_shape_mut(&mut page.shapes, deckcraft_model::ShapeId(binding.original_shape))
                    .ok_or_else(|| invalid("original local notes table was deleted"))?
            };
            let deckcraft_model::ShapeKind::Table(target) = &mut target.kind else {
                return Err(invalid("original notes table changed kind"));
            };
            crate::write::table_projection::notes::restore(target, current, &binding.bodies)?;
        }
    }
    if declared.iter().any(|(key, projected)| *projected != restored.contains(key)) {
        return Err(invalid("projected notes table has no current core authority"));
    }
    Ok(())
}

fn allocate(imp: &mut Imp<'_>) -> Result<u32, PptxError> {
    imp.next = imp.next.checked_add(1).ok_or_else(|| invalid("notes identity space exhausted"))?;
    Ok(imp.next)
}

fn unique_relation<'a>(part: &'a Part, id: &str) -> Result<&'a opc::Rel, PptxError> {
    let mut found = part.rels.items.iter().filter(|relation| relation.id == id);
    let relation = found.next().ok_or_else(|| invalid("missing notes relationship"))?;
    if found.next().is_some() {
        return Err(invalid("duplicate notes relationship identity"));
    }
    Ok(relation)
}

fn dependency(pkg: &Package, expected: &Dependency, validated: &mut HashMap<String, Dependency>) -> Result<(), PptxError> {
    let key = opc::norm(&expected.part);
    if let Some(previous) = validated.get(&key) {
        return if previous == expected { Ok(()) } else { Err(invalid("inconsistent notes dependency witness")) };
    }
    if validated.len() >= 50_000 {
        return Err(invalid("notes dependency witness count exceeds limit"));
    }
    let bytes = pkg.get(&expected.part).ok_or_else(|| invalid("notes dependency was deleted"))?;
    if bytes.len() != expected.bytes
        || digest(bytes) != expected.sha256
        || pkg.content_type(&expected.part).as_deref() != Some(expected.content_type.as_str())
    {
        return Err(invalid("notes dependency changed"));
    }
    validated.insert(key, expected.clone());
    Ok(())
}

fn relationships_digest(part: &Part) -> String {
    let rels = opc::RelsOut {
        items: part
            .rels
            .items
            .iter()
            .map(|relation| (relation.id.clone(), relation.ty.clone(), relation.target_raw.clone(), relation.external))
            .collect(),
    };
    digest(&rels.to_xml())
}

fn remap_master(
    imp: &mut Imp<'_>,
    master: &mut Master,
    media: &HashMap<MediaId, MediaId>,
    slides: &HashMap<u32, u32>,
    masters: &mut HashMap<MasterId, MasterId>,
) -> Result<(), PptxError> {
    notes_projection::validate_master(master)?;
    let id = MasterId(allocate(imp)?);
    if masters.insert(master.id, id).is_some() {
        return Err(invalid("duplicate native notes master identity"));
    }
    master.id = id;
    notes_projection::remap_shape_ids(&mut master.shapes, &mut || allocate(imp))?;
    for layout in &mut master.layouts {
        layout.id = LayoutId(allocate(imp)?);
        notes_projection::remap_shape_ids(&mut layout.shapes, &mut || allocate(imp))?;
    }
    let mut missing = false;
    crate::notes_media::visit_mut::master(master, &mut |id| {
        if let Some(mapped) = media.get(id) {
            *id = *mapped;
        } else {
            missing = true;
        }
    });
    if missing {
        return Err(invalid("native notes master references an unbound media item"));
    }
    notes_projection::remap_slide_links(master, slides)
}

fn rewrite_media_page(page: &mut NotesPage, media: &HashMap<MediaId, MediaId>) -> Result<(), PptxError> {
    let mut missing = false;
    let mut rewrite = |id: &mut MediaId| {
        if let Some(mapped) = media.get(id) {
            *id = *mapped;
        } else {
            missing = true;
        }
    };
    crate::notes_media::visit_mut::shapes(&mut page.shapes, &mut rewrite);
    if let Some(Background::Fill { fill }) = &mut page.background {
        crate::notes_media::visit_mut::fill(fill, &mut rewrite);
    }
    if missing {
        return Err(invalid("native notes page references an unbound media item"));
    }
    Ok(())
}

fn rewrite_media_text(text: &mut TextBody, media: &HashMap<MediaId, MediaId>) -> Result<(), PptxError> {
    let mut missing = false;
    crate::notes_media::visit_mut::text(text, &mut |id| {
        if let Some(mapped) = media.get(id) {
            *id = *mapped;
        } else {
            missing = true;
        }
    });
    if missing {
        return Err(invalid("native canonical notes reference unbound media"));
    }
    Ok(())
}

fn core_master(imp: &mut Imp<'_>, part: &Part, root: &El) -> Result<(Master, NotesFields), PptxError> {
    let mut master = Master { id: MasterId(allocate(imp)?), name: "Notes Master".into(), ..Default::default() };
    if let Some(relation) = part.rels.first_of("theme")
        && let Some(doc) = load(imp.pkg, &relation.target)
    {
        let theme_part = Part::new(imp.pkg, &relation.target, &doc);
        master.theme = theme::read_theme(imp, &theme_part, &doc.root);
    }
    let mut context = shapes::IdCtx { fresh: true, ..Default::default() };
    if let Some(common) = root.child("cSld") {
        master.background = background(imp, part, common);
        if let Some(tree) = common.child("spTree") {
            master.shapes = shapes::sp_tree(imp, part, &mut context, tree, 0);
        }
    }
    if let Some(mapping) = root.child("clrMap") {
        master.color_map = color_map(mapping);
    }
    if let Some(style) = root.child("notesStyle") {
        master.body_style = dml::list_style(imp, part, style);
    }
    let fields = fields(&master.shapes, root.child("hf"));
    Ok((master, fields))
}

fn core_page(imp: &mut Imp<'_>, part: &Part, root: &El, context: &mut shapes::IdCtx) -> Result<(NotesPage, TextBody), PptxError> {
    let common = root.child("cSld").ok_or_else(|| invalid("notes page lacks common slide data"))?;
    let mut page =
        NotesPage { background: background(imp, part, common), show_master_shapes: root.bool("showMasterSp").unwrap_or(true), ..Default::default() };
    if let Some(tree) = common.child("spTree") {
        page.shapes = shapes::sp_tree(imp, part, context, tree, 0);
    }
    let mut bodies = Vec::new();
    deckcraft_model::walk(&page.shapes, &mut |shape, _| {
        if shape.ph_type() == Some(PhType::Body) {
            bodies.push(shape.id);
        }
    });
    if bodies.len() > 1 {
        return Err(invalid("ambiguous primary notes body frames"));
    }
    let mut text = TextBody::default();
    if let Some(id) = bodies.first().copied() {
        let shape = deckcraft_model::find_shape_mut(&mut page.shapes, id).ok_or_else(|| invalid("missing notes body frame"))?;
        text = shape.text.take().unwrap_or_default();
        page.body_shape = Some(id);
    }
    page.fields = fields(&page.shapes, None);
    Ok((page, text))
}

fn fields(shapes: &[Shape], flags: Option<&El>) -> NotesFields {
    let mut fields = NotesFields::default();
    if let Some(flags) = flags {
        fields.display_header = flags.bool("hdr").or(Some(true));
        fields.display_footer = flags.bool("ftr").or(Some(true));
        fields.display_date_time = flags.bool("dt").or(Some(true));
        fields.display_page_number = flags.bool("sldNum").or(Some(true));
    }
    deckcraft_model::walk(shapes, &mut |shape, _| {
        let text = shape.text.as_ref().map(TextBody::text).unwrap_or_default();
        match shape.ph_type() {
            Some(PhType::Header) => {
                if shape.text.is_some() {
                    fields.header = Some(text);
                }
                if flags.is_none() {
                    fields.display_header = Some(!shape.hidden);
                }
            }
            Some(PhType::Footer) => {
                if shape.text.is_some() {
                    fields.footer = Some(text);
                }
                if flags.is_none() {
                    fields.display_footer = Some(!shape.hidden);
                }
            }
            Some(PhType::Date) => {
                let current =
                    shape.text.as_ref().into_iter().flat_map(|text| &text.paragraphs).flat_map(|paragraph| &paragraph.runs).find_map(|run| {
                        if let RunKind::Field { field } = &run.kind {
                            field.starts_with("datetime").then(|| NotesDateTime::Current { format: field.clone(), lang: run.props.lang.clone() })
                        } else {
                            None
                        }
                    });
                if shape.text.is_some() {
                    fields.date_time = Some(current.unwrap_or(NotesDateTime::Fixed { text }));
                }
                if flags.is_none() {
                    fields.display_date_time = Some(!shape.hidden);
                }
            }
            Some(PhType::SlideNum) if flags.is_none() => fields.display_page_number = Some(!shape.hidden),
            _ => {}
        }
    });
    fields
}

fn read_override(imp: &mut Imp<'_>, path: &str, base: &deckcraft_model::Theme) -> Result<deckcraft_model::Theme, PptxError> {
    let doc = load(imp.pkg, path).ok_or_else(|| invalid("missing notes theme override"))?;
    let part = Part::new(imp.pkg, path, &doc);
    let mut root = doc.root.clone();
    root.name = "a:theme".into();
    root.children = vec![Node::El(El { name: "a:themeElements".into(), attrs: vec![], children: doc.root.children.clone() })];
    let read = theme::read_theme(imp, &part, &root);
    let mut out = base.clone();
    if doc.root.child("clrScheme").is_some() {
        out.colors = read.colors;
    }
    if doc.root.child("fontScheme").is_some() {
        out.fonts = read.fonts;
    }
    if doc.root.child("fmtScheme").is_some() {
        out.format = read.format;
    }
    Ok(out)
}

fn raw_tree(root: &El) -> Result<(), PptxError> {
    fn visit(el: &El, ids: &mut HashSet<u32>, count: &mut usize, depth: usize) -> Result<(), PptxError> {
        if depth > 96 {
            return Err(invalid("notes raw projection nesting limit"));
        }
        if el.local() == "cNvPr" {
            *count = count.checked_add(1).ok_or_else(|| invalid("notes raw shape count overflow"))?;
            let id = notes_codec::exact_attr(el, "id").and_then(|id| id.parse::<u32>().ok()).ok_or_else(|| invalid("invalid raw shape identity"))?;
            if *count > MAX_SHAPES + 1 || !ids.insert(id) {
                return Err(invalid("duplicate or excessive raw shape identities"));
            }
        }
        if el.local() == "extLst" {
            return Ok(());
        }
        if el.local() == "AlternateContent" {
            // Our supported modern object writer intentionally repeats the same
            // cNvPr identity in mutually exclusive Choice/Fallback branches.
            // The complete branch XML still participates in projection hashing.
            if let Some(branch) = el.child("Choice").or_else(|| el.child("Fallback")) {
                visit(branch, ids, count, depth + 1)?;
            }
            return Ok(());
        }
        for child in el.elements() {
            visit(child, ids, count, depth + 1)?;
        }
        Ok(())
    }
    visit(root, &mut HashSet::new(), &mut 0, 0)
}
