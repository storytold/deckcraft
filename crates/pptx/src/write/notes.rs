//! One standards-valid notes master, page-local editable projection, validated native metadata.
use std::collections::HashSet;

use deckcraft_model::{MediaId, theme::Theme};

use super::{Exp, Out, clr_map, csld, root_open, theme};
use crate::{
    PptxError,
    notes_codec::{self, digest, invalid},
    notes_projection::{PageProjection, Plan},
    notes_records::{Counters, Dependency, MasterRecord, MediaBinding, PageRecord, Resources, SlideBinding},
    opc::{self, NS_A, NS_R},
    xml::{A, El, W},
};

/// Rebuild one projection from unpublished restored typed state. Seed existing media
/// part paths so relationship ids/targets reproduce the actual serialized package.
/// No package is finished, recursive export is not used, and no disk output occurs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn expected_page(
    p: &deckcraft_model::Presentation,
    index: usize,
    resource_digest: &str,
    media_parts: std::collections::HashMap<MediaId, String>,
    slide_parts: std::collections::HashMap<deckcraft_model::SlideId, String>,
    notes_part: &str,
    slide_part: &str,
    counters: &Counters,
    seed_dependencies: &std::collections::HashMap<String, Dependency>,
) -> Result<(String, String, Vec<Dependency>), PptxError> {
    counters.validate()?;
    let projected = crate::notes_projection::project_one(p, index)?;
    let mut x = Exp {
        p,
        pkg: Default::default(),
        media_parts,
        n_image: counters.image,
        n_media: counters.media,
        n_chart: counters.chart,
        n_diagram: counters.diagram,
        n_tags: counters.tags,
        n_chartex: counters.chartex,
        n_kept: counters.kept,
        slide_parts,
        table_styles: Vec::new(),
        placeholder: counters.placeholder.clone(),
        error: None,
        table_hashes: Default::default(),
    };
    // Existing media bytes remain authoritative in the already validated input.
    // Reuse their small witnesses instead of cloning/re-hashing payloads per page.
    let mut seeds = seed_dependencies.clone();
    for (id, part) in &x.media_parts {
        if let Some(dependency) = seed_dependencies.get(&opc::norm(part)) {
            x.table_hashes.insert(*id, dependency.sha256.clone());
        }
    }
    if let Some(part) = &counters.placeholder {
        let bytes = crate::png::transparent_1x1();
        seeds
            .insert(opc::norm(part), Dependency { part: part.clone(), content_type: "image/png".into(), bytes: bytes.len(), sha256: digest(&bytes) });
    }
    let mut out = Out::new(notes_part);
    out.rels.add("notesMaster", &opc::relative(notes_part, MASTER_PART), false);
    out.rels.add("slide", &opc::relative(notes_part, slide_part), false);
    let xml = page_projection(&mut x, &mut out, &projected, index, resource_digest, Some(&seeds))?;
    if let Some(error) = x.error.take() {
        return Err(error);
    }
    let root = crate::xml::parse(&xml)?.root;
    let record = notes_codec::read_record::<PageRecord>(&root, "page")?.ok_or_else(|| invalid("expected notes dependency record is missing"))?;
    Ok((notes_codec::projection(&root)?, digest(&out.rels.to_xml()), record.dependencies))
}

pub(super) const MASTER_PART: &str = "ppt/notesMasters/notesMaster1.xml";
const THEME_PART: &str = "ppt/theme/notesTheme.xml";
const CT_THEME_OVERRIDE: &str = "application/vnd.openxmlformats-officedocument.themeOverride+xml";

/// The standard master is neutral; all actual visible notes objects are on the
/// independently regenerated page. Keep its witness derived from writer code.
fn neutral_master() -> Vec<u8> {
    let mut w = W::new();
    root_open(&mut w, "p:notesMaster", A::new());
    w.open("p:cSld", A::new().a("name", "DeckCraft notes projection"));
    w.open0("p:spTree");
    super::shapes::tree_header(&mut w);
    w.close("p:spTree");
    w.close("p:cSld");
    clr_map(&mut w, &[]);
    w.empty("p:hf", A::new().a("hdr", "0").a("ftr", "0").a("dt", "0").a("sldNum", "0"));
    w.empty0("p:notesStyle");
    w.close("p:notesMaster");
    w.finish()
}

pub(crate) fn expected_master_projection() -> Result<String, PptxError> {
    notes_codec::projection(&crate::xml::parse(&neutral_master())?.root)
}

pub(super) struct Prepared {
    pub plan: Plan,
    pub resource_digest: String,
    media_dependencies: std::collections::HashMap<String, Dependency>,
}

/// Call after regular slide part names have been installed in Exp::slide_parts.
/// The returned RID registers the single standard Notes Master in presentation.xml.
pub(super) fn prepare(x: &mut Exp<'_>, presentation: &mut Out, plan: Plan) -> Result<(Prepared, String), PptxError> {
    let mut out = Out::new(MASTER_PART);
    out.rels.add("theme", &opc::relative(MASTER_PART, THEME_PART), false);
    let neutral = Theme::default();
    let mut theme_out = Out::new(THEME_PART);
    let theme_xml = theme::theme_xml(x, &mut theme_out, &neutral);
    let theme_dep = Dependency { part: THEME_PART.into(), content_type: super::CT_THEME.into(), bytes: theme_xml.len(), sha256: digest(&theme_xml) };
    x.pkg.xml_part(THEME_PART, super::CT_THEME, theme_xml);
    x.pkg.rels(THEME_PART, &theme_out.rels);

    let mut needed = Vec::new();
    let mut seen = HashSet::new();
    let mut collect = |id: &MediaId| {
        if seen.insert(*id) {
            needed.push(*id);
        }
    };
    for master in x.p.notes_master.iter().map(|master| master.as_ref()).chain(x.p.notes_masters.iter().map(|notes| notes.master.as_ref())) {
        crate::notes_media::visit::master(master, &mut collect);
    }
    for (slide, page) in x.p.slides.iter().zip(&plan.pages) {
        if let Some(page) = page {
            crate::notes_media::visit::shapes(&page.shapes, &mut collect);
            crate::notes_media::visit::text(&slide.notes, &mut collect);
            if let Some(native) = &page.native {
                crate::notes_media::visit::shapes(&native.shapes, &mut collect);
                if let Some(deckcraft_model::Background::Fill { fill }) = &native.background {
                    crate::notes_media::visit::fill(fill, &mut collect);
                }
            }
            if let Some(master) = x.p.notes_master_for(slide).or_else(|| x.p.first_master()) {
                crate::notes_media::visit::theme(&master.theme, &mut collect);
            }
        }
    }
    if needed.len() > 10_000 {
        return Err(invalid("notes media count exceeds limit"));
    }
    let mut media = Vec::new();
    let mut media_bytes = 0usize;
    for id in needed {
        let item = x.p.media(id).ok_or_else(|| invalid("missing notes media dependency"))?;
        if item.content_type == "application/vnd.deckcraft.odp-source" {
            return Err(invalid("retained original source bytes cannot be emitted as notes media"));
        }
        if x.p.media.iter().filter(|item| item.id == id).count() != 1 {
            return Err(invalid("ambiguous notes media identity"));
        }
        if item.content_type.is_empty() || !item.content_type.contains('/') {
            return Err(invalid("notes media requires an explicit valid content type"));
        }
        media_bytes = media_bytes.checked_add(item.data.len()).ok_or_else(|| invalid("notes media bytes overflow"))?;
        if item.data.len() > 32 * 1024 * 1024 || media_bytes > 128 * 1024 * 1024 {
            return Err(invalid("notes media budget exceeded"));
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
        let external = item.link.clone();
        let dependency = if !item.data.is_empty() {
            let bytes = item.data.len();
            let sha256 = digest(&item.data);
            let part = x.media_part(id).ok_or_else(|| invalid("cannot emit notes media dependency"))?;
            Some(Dependency { part, content_type: content_type.clone(), bytes, sha256 })
        } else {
            None
        };
        if dependency.is_none() && external.is_none() {
            return Err(invalid("empty notes media has no external link"));
        }
        let relation = x.media_rel(&mut out, id, kind).ok_or_else(|| invalid("cannot bind notes media dependency"))?;
        media.push(MediaBinding {
            original: id.0,
            relation,
            name,
            content_type,
            original_link: external.clone(),
            external: dependency.is_none().then_some(external).flatten(),
            dependency,
        });
    }
    let mut slides = Vec::new();
    for (index, slide) in x.p.slides.iter().enumerate() {
        let part = x.slide_parts.get(&slide.id).cloned().ok_or_else(|| invalid("missing notes owning slide part"))?;
        let notes = plan.pages.get(index).and_then(Option::as_ref).map(|_| format!("ppt/notesSlides/notesSlide{}.xml", index + 1));
        slides.push(SlideBinding { original: slide.id.0, part, notes });
    }
    let media_dependencies =
        media.iter().filter_map(|binding| binding.dependency.as_ref()).map(|dependency| (opc::norm(&dependency.part), dependency.clone())).collect();
    let mut resources =
        Resources { legacy: x.p.notes_master.clone(), named: x.p.notes_masters.clone(), size: plan.size, slides, media, table_sources: Vec::new() };
    // Visible tables have their sole canonical text/value cache in current core.
    // Truly unused originals remain sole native resources in this closed record.
    let visible: HashSet<_> = plan
        .pages
        .iter()
        .flatten()
        .flat_map(|page| &page.tables)
        .filter_map(|binding| binding.master.map(|master| (master, binding.original_shape)))
        .collect();
    for master in resources.legacy.iter().chain(resources.named.iter().map(|notes| &notes.master)) {
        deckcraft_model::walk(&master.shapes, &mut |shape, _| {
            if matches!(shape.kind, deckcraft_model::ShapeKind::Table(_)) {
                resources.table_sources.push(crate::notes_records::TableSource {
                    master: master.id.0,
                    layout: None,
                    shape: shape.id.0,
                    projected: visible.contains(&(master.id.0, shape.id.0)),
                });
            }
        });
        for layout in &master.layouts {
            deckcraft_model::walk(&layout.shapes, &mut |shape, _| {
                if matches!(shape.kind, deckcraft_model::ShapeKind::Table(_)) {
                    resources.table_sources.push(crate::notes_records::TableSource {
                        master: master.id.0,
                        layout: Some(layout.id.0),
                        shape: shape.id.0,
                        projected: false,
                    });
                }
            });
        }
    }
    for master in resources.legacy.iter_mut().chain(resources.named.iter_mut().map(|notes| &mut notes.master)) {
        let master = std::sync::Arc::make_mut(master);
        strip_tables(&mut master.shapes, &visible.iter().filter(|(id, _)| *id == master.id.0).map(|(_, shape)| *shape).collect())?;
    }
    let resource_digest = digest(&serde_json::to_vec(&resources).map_err(invalid)?);
    let core = neutral_master();
    let root = crate::xml::parse(&core)?.root;
    let record = MasterRecord { projection: notes_codec::projection(&root)?, relationships: digest(&out.rels.to_xml()), theme: theme_dep, resources };
    let xml = append_extension(root, "master", &record)?;
    x.pkg.xml_part(MASTER_PART, super::CT_NOTES_MASTER, xml);
    x.pkg.rels(MASTER_PART, &out.rels);
    let rid = presentation.rels.add("notesMaster", &opc::relative(&presentation.name, MASTER_PART), false);
    Ok((Prepared { plan, resource_digest, media_dependencies }, rid))
}

/// Write after the caller adds standard notesMaster and owning slide relationships.
pub(super) fn page(x: &mut Exp<'_>, out: &mut Out, prepared: &Prepared, index: usize) -> Result<Vec<u8>, PptxError> {
    let projected = prepared.plan.pages.get(index).and_then(Option::as_ref).ok_or_else(|| invalid("missing notes projection page"))?;
    page_projection(x, out, projected, index, &prepared.resource_digest, Some(&prepared.media_dependencies))
}

fn page_projection(
    x: &mut Exp<'_>,
    out: &mut Out,
    projected: &PageProjection,
    index: usize,
    resource_digest: &str,
    seeds: Option<&std::collections::HashMap<String, Dependency>>,
) -> Result<Vec<u8>, PptxError> {
    let counters = Counters {
        image: x.n_image,
        media: x.n_media,
        chart: x.n_chart,
        diagram: x.n_diagram,
        tags: x.n_tags,
        chartex: x.n_chartex,
        kept: x.n_kept,
        placeholder: x.placeholder.clone(),
    };
    counters.validate()?;
    let slide = x.p.slides.get(index).ok_or_else(|| invalid("missing notes owning slide"))?;
    let source_theme = x.p.notes_master_for(slide).or_else(|| x.p.first_master()).map(|master| master.theme.clone()).unwrap_or_default();
    let theme_name = format!("ppt/theme/notesThemeOverride{}.xml", index + 1);
    let mut theme_out = Out::new(&theme_name);
    let theme_xml = theme_override(x, &mut theme_out, &source_theme)?;
    let theme_dep =
        Dependency { part: theme_name.clone(), content_type: CT_THEME_OVERRIDE.into(), bytes: theme_xml.len(), sha256: digest(&theme_xml) };
    let theme_relationships = digest(&theme_out.rels.to_xml());
    x.pkg.xml_part(&theme_name, CT_THEME_OVERRIDE, theme_xml);
    x.pkg.rels(&theme_name, &theme_out.rels);
    out.rels.add("themeOverride", &opc::relative(&out.name, &theme_name), false);

    let mut w = W::new();
    root_open(&mut w, "p:notes", A::new().a("showMasterSp", "0"));
    let ids = csld(&mut w, x, out, "", projected.background.as_ref(), &projected.shapes, &[]);
    w.open0("p:clrMapOvr");
    let color_map = x.p.notes_master_for(slide).or_else(|| x.p.first_master()).map(|master| master.color_map.as_slice()).unwrap_or_default();
    let mut map = W::new();
    map.open("p:wrapper", A::new().a("xmlns:p", crate::opc::NS_P));
    clr_map(&mut map, color_map);
    map.close("p:wrapper");
    if let Some(mapping) = crate::xml::parse(&map.finish())?.root.elements().next() {
        let mut mapping = mapping.clone();
        mapping.name = "a:overrideClrMapping".into();
        w.raw(&mapping.to_xml());
    }
    w.close("p:clrMapOvr");
    w.close("p:notes");
    let core = w.finish();
    let root = crate::xml::parse(&core)?.root;
    let dependencies = dependency_closure(x, out, seeds)?;
    let mut original = projected.native.clone();
    if let Some(page) = &mut original {
        strip_tables(
            &mut page.shapes,
            &projected.tables.iter().filter(|binding| binding.master.is_none()).map(|binding| binding.original_shape).collect(),
        )?;
    }
    let mut tables = projected.tables.clone();
    for binding in &mut tables {
        binding.projected_shape =
            ids.get(deckcraft_model::ShapeId(binding.projected_shape)).ok_or_else(|| invalid("projected notes table was not written"))?;
    }
    let record = PageRecord {
        projection: notes_codec::projection(&root)?,
        relationships: digest(&out.rels.to_xml()),
        resources: resource_digest.into(),
        original_slide: slide.id.0,
        body_file_id: ids.get(projected.body).ok_or_else(|| invalid("canonical notes body was not written"))?,
        original,
        body_style: projected.body_style.clone(),
        theme: theme_dep,
        theme_relationships,
        counters,
        tables,
        dependencies,
    };
    append_extension(root, "page", &record)
}

/// Every current core dependency, including shared images/placeholder posters and
/// transitive chart/tag/diagram/theme parts. Never traverse back into the live owner
/// slide or the separately validated single notes master.
pub(crate) fn dependency_closure(
    x: &Exp<'_>,
    owner: &Out,
    seeds: Option<&std::collections::HashMap<String, Dependency>>,
) -> Result<Vec<Dependency>, PptxError> {
    let mut pending = Vec::new();
    for (_, ty, target, external) in &owner.rels.items {
        let kind = ty.rsplit('/').next().unwrap_or("");
        if !external && !matches!(kind, "slide" | "notesMaster") {
            pending.push(opc::resolve(&opc::dir_of(&owner.name), target));
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut bytes_seen = 0usize;
    while let Some(part) = pending.pop() {
        if !seen.insert(opc::norm(&part)) {
            continue;
        }
        if seen.len() > 50_000 {
            return Err(invalid("notes dependency closure exceeds package limits"));
        }
        // Seeds contain only already validated immutable ordinary media parts.
        // They have no emitted side relationships and need hashing only once.
        if let Some(seed) = seeds.and_then(|seeds| seeds.get(&opc::norm(&part))) {
            bytes_seen = bytes_seen.checked_add(seed.bytes).ok_or_else(|| invalid("notes dependency byte overflow"))?;
            if bytes_seen > 128 * 1024 * 1024 {
                return Err(invalid("notes dependency closure exceeds byte limit"));
            }
            out.push(seed.clone());
            continue;
        }
        let (name, bytes) =
            x.pkg.entries().find(|(name, _)| opc::norm(name) == opc::norm(&part)).ok_or_else(|| invalid("missing emitted notes dependency"))?;
        bytes_seen = bytes_seen.checked_add(bytes.len()).ok_or_else(|| invalid("notes dependency byte overflow"))?;
        if bytes_seen > 128 * 1024 * 1024 {
            return Err(invalid("notes dependency closure exceeds byte limit"));
        }
        out.push(Dependency {
            part: name.to_string(),
            content_type: x.pkg.content_type_for(name).ok_or_else(|| invalid("notes dependency has no content type"))?,
            bytes: bytes.len(),
            sha256: digest(bytes),
        });
        let rels_name = opc::rels_path(name);
        if let Some((_, rels)) = x.pkg.entries().find(|(name, _)| opc::norm(name) == opc::norm(&rels_name)) {
            pending.push(rels_name);
            for relation in opc::rels_from_bytes(rels, name).items {
                if !relation.external && !matches!(relation.kind.as_str(), "slide" | "notesMaster") {
                    pending.push(relation.target);
                }
            }
        }
    }
    out.sort_by(|a, b| a.part.cmp(&b.part));
    Ok(out)
}

pub(crate) fn append_extension<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq>(
    mut root: El,
    kind: &str,
    record: &T,
) -> Result<Vec<u8>, PptxError> {
    let mut extra = W::new();
    // A wrapper supplies the standard prefix for the standalone fragment parser.
    extra.open("p:wrapper", A::new().a("xmlns:p", crate::opc::NS_P));
    notes_codec::write_record(&mut extra, kind, record)?;
    extra.close("p:wrapper");
    let doc = crate::xml::parse(&extra.finish())?;
    root.children.extend(doc.root.children);
    let mut w = W::new();
    w.raw(&root.to_xml());
    Ok(w.finish())
}

fn theme_override(x: &mut Exp<'_>, out: &mut Out, theme: &Theme) -> Result<Vec<u8>, PptxError> {
    let base = theme::theme_xml(x, out, theme);
    let doc = crate::xml::parse(&base)?;
    let elements = doc.root.child("themeElements").ok_or_else(|| invalid("missing notes theme elements"))?;
    let mut w = W::new();
    w.open("a:themeOverride", A::new().a("xmlns:a", NS_A).a("xmlns:r", NS_R));
    for child in elements.elements() {
        w.raw(&child.to_xml());
    }
    w.close("a:themeOverride");
    Ok(w.finish())
}

fn strip_tables(shapes: &mut [deckcraft_model::Shape], visible: &HashSet<u32>) -> Result<(), PptxError> {
    for shape in shapes {
        if visible.contains(&shape.id.0)
            && let deckcraft_model::ShapeKind::Table(table) = &mut shape.kind
        {
            crate::write::table_projection::notes::strip(table, 0)?;
        }
        if let Some(children) = shape.children_mut() {
            strip_tables(children, visible)?;
        }
    }
    Ok(())
}
