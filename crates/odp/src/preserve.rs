//! Strict source-patch export: local plain slide text only, with unchanged package ownership.
use crate::{
    MAX_PART, MAX_TOTAL, MIME, OdpError, Result,
    xml::{self, El, Node, SourcePart},
};
use deckcraft_model::{
    Presentation, Shape,
    text::{Paragraph, RunKind},
};
use quick_xml::{Reader, events::Event};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};

#[path = "preserve_cache.rs"]
mod cache;
#[path = "preserve_metadata.rs"]
mod metadata;
#[path = "preserve_zip.rs"]
mod package;

const SOURCE: &str = "application/vnd.deckcraft.odp-source";
const MAX_TEXT: usize = 8 * 1024 * 1024;
fn refuse(reason: &str) -> OdpError {
    OdpError(format!("source-preserving text export refused: {reason}"))
}

struct Change {
    slide: usize,
    shape: usize,
    paragraph: usize,
    text: String,
}
struct Patch {
    range: Range<usize>,
    value: Vec<u8>,
}

pub(crate) fn export_text_edits(current: &Presentation) -> Result<Vec<u8>> {
    let mut sources = current.media.iter().filter(|media| media.content_type == SOURCE);
    let source = sources.next().ok_or_else(|| refuse("missing original source"))?;
    if sources.next().is_some()
        || source.link.is_some()
        || source.data.is_empty()
        || !current.tags.iter().any(|(key, value)| key == "odp:imported" && value == "1")
    {
        return Err(refuse("ambiguous original source"));
    }
    let package = package::Package::read(&source.data)?;
    let (documents, cache) = passive_package(&package)?;
    let mut effects = crate::source_effects::Report::default();
    let mut effects_budget = 0;
    for (name, document) in &documents {
        effects.scan(name, document, &mut effects_budget)?;
    }
    effects.validate_preservation()?;
    let baseline = crate::import(&source.data)?;
    let summary = crate::loss_report::ImportReport::default().summary();
    let warnings: Vec<_> = baseline.tags.iter().filter(|(key, _)| key == "odp:warning").collect();
    if warnings.is_empty()
        || warnings.iter().any(|(_, value)| value != summary && !(value == crate::source_effects::GAMMA_WARNING && effects.preservation_eligible()))
    {
        return Err(refuse("source has unsupported content beyond passive text properties"));
    }
    let mut audit = crate::loss_report::ImportReport::default();
    for (name, document) in &documents {
        let part = match name.as_str() {
            "content.xml" => crate::loss_report::Part::Content,
            "styles.xml" => crate::loss_report::Part::Styles,
            _ => crate::loss_report::Part::Metadata,
        };
        audit.audit(part, document);
    }
    if !audit.preservation_eligible() {
        return Err(refuse("source audit contains non-preservable semantics"));
    }
    let (expected, changes) = changes(&baseline, current)?;
    if changes.is_empty() {
        return Err(refuse("no supported text edits; use original recovery"));
    }
    let content = documents.get("content.xml").ok_or_else(|| refuse("missing content"))?;
    let original = package.files.get("content.xml").ok_or_else(|| refuse("missing content"))?;
    let patches = patches(&baseline, content, original, &changes)?;
    let edited = apply(original, patches)?;
    let output = package.write(&edited, cache.as_ref())?;
    let mut verified = crate::import(&output)?;
    // The new import has new source attachment bytes and fresh byte-range provenance. Only
    // those independently reconstructed fields may differ during the final semantic check.
    let mut desired = expected;
    align_provenance(&mut desired, &verified)?;
    let mut output_sources = verified.media.iter_mut().filter(|media| media.content_type == SOURCE);
    let actual = output_sources.next().ok_or_else(|| refuse("output source verification failed"))?;
    if output_sources.next().is_some() {
        return Err(refuse("output source verification failed"));
    }
    actual.data = source.data.clone();
    align_text_runs(&mut verified, &desired)?;
    if desired != verified {
        return Err(refuse("output does not reproduce every requested edit"));
    }
    Ok(output)
}

fn raster(name: &str, mime: &str, data: &[u8]) -> bool {
    if !name.starts_with("Pictures/") || name.trim_end_matches('/').split('/').count() != 2 {
        return false;
    }
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match mime {
        "image/png" => extension == "png" && data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => matches!(extension.as_str(), "jpg" | "jpeg") && data.starts_with(b"\xff\xd8\xff"),
        "image/gif" => extension == "gif" && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")),
        "image/webp" => extension == "webp" && data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"),
        "image/bmp" => extension == "bmp" && data.starts_with(b"BM"),
        "image/tiff" => matches!(extension.as_str(), "tif" | "tiff") && (data.starts_with(b"II\x2a\x00") || data.starts_with(b"MM\x00\x2a")),
        _ => false,
    }
}

fn passive_package(package: &package::Package) -> Result<(BTreeMap<String, El>, Option<cache::CachePlan>)> {
    let manifest_bytes = package.files.get("META-INF/manifest.xml").ok_or_else(|| refuse("missing manifest"))?;
    passive_lexical(manifest_bytes)?;
    let manifest = xml::parse_part(manifest_bytes, SourcePart::Manifest)?;
    if manifest.name != "manifest:manifest"
        || !matches!(manifest.attr("manifest:version"), Some("1.2" | "1.3"))
        || manifest.attrs.keys().any(|key| key != "manifest:version")
        || manifest.children.iter().any(|node| matches!(node, Node::Text(text) if !text.trim().is_empty()))
    {
        return Err(refuse("unsupported manifest"));
    }
    let mut owners = BTreeMap::new();
    for entry in manifest.elements() {
        let path = entry.attr("manifest:full-path").ok_or_else(|| refuse("missing manifest owner"))?;
        let mime = entry.attr("manifest:media-type").ok_or_else(|| refuse("missing manifest media type"))?;
        if entry.name != "manifest:file-entry"
            || entry.elements().next().is_some()
            || entry.children.iter().any(|n| matches!(n, Node::Text(t) if !t.trim().is_empty()))
            || entry.attrs.keys().any(|key| !matches!(key.as_str(), "manifest:full-path" | "manifest:media-type" | "manifest:version"))
            || owners.insert(path.to_string(), mime.to_string()).is_some()
        {
            return Err(refuse("ambiguous or protected manifest ownership"));
        }
        configuration_owner(entry, path, mime)?;
    }
    if owners.get("/").is_none_or(|mime| mime != MIME) {
        return Err(refuse("manifest MIME mismatch"));
    }
    let has_configuration_directories = configuration_directories(&package.files)?;
    let mut documents = BTreeMap::new();
    let mut budget = manifest_bytes.len().saturating_add(manifest.identity_metadata_bytes());
    for (name, bytes) in &package.files {
        if name == "mimetype" || name == "META-INF/manifest.xml" {
            continue;
        }
        if name.starts_with("Configurations2/") {
            // The complete prefix was proved to contain only bounded empty directories.
            continue;
        }
        if matches!(name.as_str(), "Pictures/" | "Thumbnails/") {
            if !bytes.is_empty() {
                return Err(refuse("invalid image directory"));
            }
            continue;
        }
        // Discovery grants no exception. The complete closed cache profile must prove
        // this exact optional member before passive validation or output can proceed.
        if name == "Thumbnails/thumbnail.png" {
            continue;
        }
        let mime = owners.get(name).ok_or_else(|| refuse("missing manifest file owner"))?;
        let source_part = match name.as_str() {
            "content.xml" => Some((SourcePart::Content, "office:document-content")),
            "styles.xml" => Some((SourcePart::Styles, "office:document-styles")),
            "meta.xml" => Some((SourcePart::Metadata, "office:document-meta")),
            "settings.xml" => Some((SourcePart::Metadata, "office:document-settings")),
            _ => None,
        };
        if let Some((part, root_name)) = source_part {
            if mime != "text/xml" {
                return Err(refuse("XML media-type mismatch"));
            }
            let root = xml::parse_part(bytes, part)?;
            passive_lexical(bytes)?;
            budget = budget.saturating_add(bytes.len()).saturating_add(root.identity_metadata_bytes());
            if budget as u64 > MAX_TOTAL {
                return Err(refuse("source XML budget exceeded"));
            }
            if root.name != root_name {
                return Err(refuse("XML document ownership mismatch"));
            }
            documents.insert(name.clone(), root);
        } else if !raster(name, mime, bytes) {
            return Err(refuse("package contains an unverified part, preview, or active content"));
        }
    }
    let cache = cache::plan(&package.files, &documents, &manifest, &mut budget)?;
    if package.files.contains_key("Thumbnails/thumbnail.png") && cache.as_ref().is_none_or(|plan| !plan.allows_thumbnail("Thumbnails/thumbnail.png"))
    {
        return Err(refuse("unverified optional thumbnail"));
    }
    if package.files.contains_key("Thumbnails/") && cache.is_none() {
        return Err(refuse("thumbnail directory has no verified cache scope"));
    }
    for (name, root) in &documents {
        passive_xml(root, None, &package.files, name, cache.as_ref())?;
    }
    for (name, mime) in owners {
        if name == "/" {
            continue;
        }
        if name == "Configurations2/" {
            if !has_configuration_directories {
                return Err(refuse("configuration directory owner has no directory scope"));
            }
            continue;
        }
        if name == "Pictures/" && mime.is_empty() && package.files.keys().any(|name| name.starts_with("Pictures/")) {
            continue;
        }
        if name == "Thumbnails/" && mime.is_empty() && cache.is_some() && package.files.keys().any(|name| name.starts_with("Thumbnails/")) {
            continue;
        }
        if name == "mimetype" || name.starts_with("META-INF/") || !package.files.contains_key(&name) {
            return Err(refuse("dangling or exempt manifest owner"));
        }
    }
    if !documents.contains_key("content.xml") {
        return Err(refuse("missing slide content"));
    }
    Ok((documents, cache))
}

fn configuration_owner(entry: &El, path: &str, mime: &str) -> Result<()> {
    if path.starts_with("Configurations2/")
        && (path != "Configurations2/"
            || mime != "application/vnd.sun.xml.ui.configuration"
            || entry.attrs.keys().any(|key| !matches!(key.as_str(), "manifest:full-path" | "manifest:media-type")))
    {
        return Err(refuse("unverified configuration directory owner"));
    }
    Ok(())
}

fn configuration_directories(files: &BTreeMap<String, Vec<u8>>) -> Result<bool> {
    let mut count = 0usize;
    for (name, bytes) in files.iter().filter(|(name, _)| name.starts_with("Configurations2/")) {
        count = count.saturating_add(1);
        // Raw ZIP preflight already proves canonical unique paths and zero-sized directory
        // records. Recheck the typed inventory before granting this prefix an exception.
        if !name.ends_with('/') || !bytes.is_empty() || name.len() > 1024 || name.trim_end_matches('/').split('/').count() > 8 || count > 128 {
            return Err(refuse("configuration scope must contain only bounded empty directories"));
        }
    }
    Ok(count != 0)
}

fn passive_lexical(bytes: &[u8]) -> Result<()> {
    std::str::from_utf8(bytes).map_err(|_| refuse("source XML must use UTF-8"))?;
    let mut reader = Reader::from_reader(bytes);
    loop {
        match reader.read_event().map_err(|_| refuse("invalid source XML"))? {
            Event::Eof => return Ok(()),
            Event::PI(_) | Event::DocType(_) => return Err(refuse("unverified XML processing instructions")),
            Event::Decl(declaration)
                if declaration.version().map_err(|_| refuse("invalid XML declaration"))?.as_ref() != b"1.0"
                    || declaration
                        .encoding()
                        .transpose()
                        .map_err(|_| refuse("invalid XML encoding"))?
                        .is_some_and(|encoding| !encoding.eq_ignore_ascii_case(b"UTF-8")) =>
            {
                return Err(refuse("source XML must use XML 1.0 UTF-8"));
            }
            _ => {}
        }
    }
}

/// The standard scripts container may be present even when a document has no scripts.
/// Prove that exact empty root child independently of the optional cache profile.
fn empty_scripts(el: &El, parent: Option<&El>, part: &str) -> bool {
    const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
    let standard =
        |element: &El, local: &str| element.expanded_name.namespace.as_deref() == Some(OFFICE) && element.expanded_name.local.as_ref() == local;
    part == "content.xml"
        && el.name == "office:scripts"
        && standard(el, "scripts")
        && el.attrs.is_empty()
        && el.expanded_attrs.is_empty()
        && el.children.iter().all(|node| matches!(node, Node::Text(text) if text.bytes().all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))))
        && parent.is_some_and(|owner| {
            owner.name == "office:document-content"
                && standard(owner, "document-content")
                && owner.elements().next().is_some_and(|first| standard(first, "scripts"))
                && owner.elements().filter(|child| standard(child, "scripts")).take(2).count() == 1
        })
}

fn passive_xml(el: &El, parent: Option<&El>, files: &BTreeMap<String, Vec<u8>>, part: &str, cache: Option<&cache::CachePlan>) -> Result<()> {
    metadata::validate(el, parent, part)?;
    if (el.name == "meta:document-statistic" && cache.is_none_or(|plan| !plan.allows_statistic(part, el)))
        || el.name.starts_with("unknown:")
        || el.name.starts_with("text:dde")
        || el.name.starts_with("table:dde")
        || el.name.starts_with("office:dde")
        || el.name.starts_with("text:database")
        || (el.name.starts_with("loext:") && !parent.is_some_and(|parent| el.supported_paragraph_fill(parent)))
        || el.attrs.keys().any(|key| {
            key.starts_with("unknown:")
                || key.starts_with("loext:")
                || matches!(
                    key.as_str(),
                    "office:dde-application" | "office:dde-topic" | "office:dde-item" | "office:automatic-update" | "draw:engine"
                )
        })
        || el.name == "office:scripts" && !empty_scripts(el, parent, part)
        || matches!(
            el.name.as_str(),
            "office:script"
                | "office:event-listeners"
                | "office:forms"
                | "office:binary-data"
                | "presentation:event-listener"
                | "text:script"
                | "text:execute-macro"
                | "draw:applet"
                | "draw:plugin"
                | "draw:floating-frame"
                | "draw:object"
                | "draw:object-ole"
                | "text:word-count"
                | "text:character-count"
                | "text:non-whitespace-character-count"
                | "text:ruby"
                | "style:ruby-properties"
        )
    {
        return Err(refuse("unverified XML, active content, or content-derived statistics"));
    }
    if let Some(href) = el.attr("xlink:href") {
        let local = href.strip_prefix("./").unwrap_or(href);
        if !href.starts_with('#') && !files.contains_key(local) {
            return Err(refuse("external or unresolved resource reference"));
        }
    }
    if el.attr("xml:base").is_some() {
        return Err(refuse("unverified resource base"));
    }
    for child in el.elements() {
        passive_xml(child, Some(el), files, part, cache)?;
    }
    Ok(())
}

fn plain_text(paragraph: &Paragraph, original: &Paragraph) -> Result<String> {
    let first = original.runs.first().ok_or_else(|| refuse("empty source paragraphs are unsupported"))?;
    if paragraph.runs.len() > 100_000 {
        return Err(refuse("text run budget exceeded"));
    }
    let mut size = 0usize;
    for run in &paragraph.runs {
        if run.kind != RunKind::Text || run.props != first.props {
            return Err(refuse("mixed run kinds or formatting are unsupported"));
        }
        size = size.checked_add(run.text.len()).ok_or_else(|| refuse("text budget exceeded"))?;
        if size > MAX_TEXT {
            return Err(refuse("text budget exceeded"));
        }
    }
    if paragraph.runs.is_empty() {
        return Err(refuse("empty edited paragraphs are unsupported"));
    }
    let mut text = String::with_capacity(size);
    for run in &paragraph.runs {
        text.push_str(&run.text);
    }
    Ok(text)
}

fn changes(baseline: &Presentation, current: &Presentation) -> Result<(Presentation, Vec<Change>)> {
    if baseline.slides.len() != current.slides.len() {
        return Err(refuse("slide ownership changed"));
    }
    let mut expected = baseline.clone();
    let mut edits = Vec::new();
    let mut budget = 0usize;
    for (slide_index, (before, after)) in baseline.slides.iter().zip(&current.slides).enumerate() {
        if before.shapes.len() != after.shapes.len() {
            return Err(refuse("shape ownership changed"));
        }
        for (shape_index, (before, after)) in before.shapes.iter().zip(&after.shapes).enumerate() {
            let (Some(before), Some(after)) = (&before.text, &after.text) else { continue };
            if before.paragraphs.len() != after.paragraphs.len() {
                return Err(refuse("paragraph ownership changed"));
            }
            for (paragraph_index, (before, after)) in before.paragraphs.iter().zip(&after.paragraphs).enumerate() {
                if before.runs == after.runs {
                    continue;
                }
                let old = plain_text(before, before)?;
                let text = plain_text(after, before)?;
                let run_weight = before.runs.first().map(|run| crate::read::run_weight(&run.props)).ok_or_else(|| refuse("missing source run"))?;
                budget = budget.saturating_add(text.len()).saturating_add(after.runs.len().saturating_mul(run_weight));
                if budget as u64 > MAX_PART {
                    return Err(refuse("edited text budget exceeded"));
                }
                let expected_slide = expected.slides.get_mut(slide_index).ok_or_else(|| refuse("slide correspondence failed"))?;
                let expected_shape =
                    Arc::make_mut(expected_slide).shapes.get_mut(shape_index).ok_or_else(|| refuse("shape correspondence failed"))?;
                let expected_paragraph = expected_shape
                    .text
                    .as_mut()
                    .and_then(|body| body.paragraphs.get_mut(paragraph_index))
                    .ok_or_else(|| refuse("paragraph correspondence failed"))?;
                expected_paragraph.runs = after.runs.clone();
                if old != text {
                    edits.push(Change { slide: slide_index, shape: shape_index, paragraph: paragraph_index, text });
                }
            }
        }
    }
    if expected != *current {
        return Err(refuse("changes outside plain slide text are unsupported"));
    }
    Ok((expected, edits))
}

fn provenance(tags: &[(String, String)]) -> Result<Option<(&str, &str)>> {
    let mut parts = tags.iter().filter(|(key, _)| key == "odp:source-part");
    let mut ranges = tags.iter().filter(|(key, _)| key == "odp:source-range");
    match (parts.next(), ranges.next()) {
        (None, None) => Ok(None),
        (Some((_, part)), Some((_, range))) if parts.next().is_none() && ranges.next().is_none() => Ok(Some((part, range))),
        _ => Err(refuse("ambiguous node provenance")),
    }
}

fn origin(el: &El, tags: &[(String, String)]) -> Result<bool> {
    let Some((part, range)) = el.source_provenance() else { return Err(refuse("missing XML provenance")) };
    Ok(provenance(tags)? == Some((part.label(), format!("v1:{}:{}", range.start, range.end).as_str())))
}

fn count_origins(p: &Presentation) -> Result<BTreeMap<(String, String), usize>> {
    fn add(shapes: &[Shape], out: &mut BTreeMap<(String, String), usize>, count: &mut usize, depth: usize) -> Result<()> {
        if depth >= 64 {
            return Err(refuse("source ownership nesting exceeded"));
        }
        for shape in shapes {
            *count = count.saturating_add(1);
            if *count > 100_000 {
                return Err(refuse("source ownership budget exceeded"));
            }
            if let Some((part, range)) = provenance(&shape.tags)? {
                if part.len() > 64 || range.len() > 64 {
                    return Err(refuse("invalid provenance payload"));
                }
                *out.entry((part.into(), range.into())).or_default() += 1;
            }
            if let deckcraft_model::ShapeKind::Group { children, .. } = &shape.kind {
                add(children, out, count, depth + 1)?;
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    let mut count = 0;
    for slide in &p.slides {
        add(&slide.shapes, &mut out, &mut count, 0)?;
        if let Some(notes) = &slide.notes_page {
            add(&notes.shapes, &mut out, &mut count, 0)?;
        }
    }
    for master in p
        .masters
        .iter()
        .map(Arc::as_ref)
        .chain(p.notes_master.as_deref())
        .chain(p.handout_master.as_deref())
        .chain(p.notes_masters.iter().map(|notes| notes.master.as_ref()))
    {
        add(&master.shapes, &mut out, &mut count, 0)?;
        for layout in &master.layouts {
            add(&layout.shapes, &mut out, &mut count, 0)?;
        }
    }
    Ok(out)
}

fn patches(baseline: &Presentation, content: &El, original: &[u8], changes: &[Change]) -> Result<Vec<Patch>> {
    let body = content.child("office:body").and_then(|body| body.child("office:presentation")).ok_or_else(|| refuse("missing presentation body"))?;
    let pages: Vec<_> = body.elements().filter(|page| page.name == "draw:page").collect();
    if pages.len() != baseline.slides.len() {
        return Err(refuse("page correspondence is ambiguous"));
    }
    let origins = count_origins(baseline)?;
    // Build one bounded source index; never scan the complete page tree per edit.
    let mut source_shapes = BTreeMap::new();
    for (index, page) in pages.iter().enumerate() {
        let slide = baseline.slides.get(index).ok_or_else(|| refuse("missing page owner"))?;
        if !origin(page, &slide.tags)? {
            return Err(refuse("page provenance mismatch"));
        }
        for shape in page.elements() {
            if let Some((part, range)) = shape.source_provenance() {
                source_shapes.insert((part.label(), range.start, range.end), (index, shape));
            }
        }
    }
    let mut out = Vec::with_capacity(changes.len());
    let mut rewritten = BTreeSet::new();
    let mut paragraph_index = BTreeMap::new();
    let mut encoded_bytes = 0usize;
    for change in changes {
        let shape =
            baseline.slides.get(change.slide).and_then(|slide| slide.shapes.get(change.shape)).ok_or_else(|| refuse("missing shape owner"))?;
        let Some((part, range)) = provenance(&shape.tags)? else { return Err(refuse("missing shape provenance")) };
        if part != "content.xml" || origins.get(&(part.into(), range.into())) != Some(&1) {
            return Err(refuse("source node has no unique local owner"));
        }
        let mut values = range.split(':');
        if values.next() != Some("v1") {
            return Err(refuse("unsupported provenance version"));
        }
        let start = values.next().and_then(|n| n.parse::<usize>().ok()).ok_or_else(|| refuse("invalid source range"))?;
        let end = values.next().and_then(|n| n.parse::<usize>().ok()).ok_or_else(|| refuse("invalid source range"))?;
        if values.next().is_some() {
            return Err(refuse("invalid source range"));
        }
        let (page_index, node) = source_shapes.get(&(part, start, end)).copied().ok_or_else(|| refuse("text is not owned by a local page node"))?;
        if page_index != change.slide || !origin(node, &shape.tags)? {
            return Err(refuse("source node ownership mismatch"));
        }
        let container = if node.name == "draw:frame" {
            if node.elements().count() != 1 {
                return Err(refuse("mixed frame content is unsupported"));
            }
            node.child("draw:text-box").ok_or_else(|| refuse("frame is not plain text"))?
        } else if matches!(node.name.as_str(), "draw:rect" | "draw:ellipse" | "draw:circle") {
            node
        } else {
            return Err(refuse("shape text container is unsupported"));
        };
        if let std::collections::btree_map::Entry::Vacant(entry) = paragraph_index.entry((start, end)) {
            let paragraphs: Vec<_> = container.elements().collect();
            if paragraphs.iter().any(|el| el.name != "text:p") {
                return Err(refuse("lists, fields, or mixed text containers are unsupported"));
            }
            entry.insert(paragraphs);
        }
        let paragraphs = paragraph_index.get(&(start, end)).ok_or_else(|| refuse("missing paragraph index"))?;
        if shape.text.as_ref().is_none_or(|text| text.paragraphs.len() != paragraphs.len()) {
            return Err(refuse("source paragraph correspondence is ambiguous"));
        }
        let paragraph = paragraphs.get(change.paragraph).copied().ok_or_else(|| refuse("missing source paragraph"))?;
        let leaf = leaf(paragraph)?;
        let (_, range) = leaf.source_provenance().ok_or_else(|| refuse("missing text provenance"))?;
        if !rewritten.insert((range.start, range.end)) {
            return Err(refuse("source text has multiple edits"));
        }
        let range = inner_range(original, range)?;
        let value = encode(&change.text, (MAX_PART as usize).saturating_sub(encoded_bytes))?;
        encoded_bytes = encoded_bytes.saturating_add(value.len());
        out.push(Patch { range, value });
    }
    Ok(out)
}

fn leaf(paragraph: &El) -> Result<&El> {
    let mut children = paragraph.elements();
    let first = children.next();
    let leaf = if first.is_some_and(|el| el.name == "text:span") {
        if children.next().is_some() || paragraph.children.iter().any(|node| matches!(node, Node::Text(text) if !text.trim().is_empty())) {
            return Err(refuse("mixed inline text is unsupported"));
        }
        first.ok_or_else(|| refuse("missing span"))?
    } else {
        paragraph
    };
    for child in leaf.elements() {
        if !matches!(child.name.as_str(), "text:s" | "text:tab")
            || child.elements().next().is_some()
            || child.children.iter().any(|node| matches!(node, Node::Text(text) if !text.is_empty()))
            || child.attrs.keys().any(|key| child.name != "text:s" || key != "text:c")
        {
            return Err(refuse("unverified inline semantics are unsupported"));
        }
    }
    Ok(leaf)
}

fn inner_range(original: &[u8], range: Range<usize>) -> Result<Range<usize>> {
    let bytes = original.get(range.clone()).ok_or_else(|| refuse("source text range is invalid"))?;
    let mut reader = Reader::from_reader(bytes);
    let first = reader.read_event().map_err(|_| refuse("invalid source text XML"))?;
    if !matches!(first, Event::Start(_)) {
        return Err(refuse("empty or self-closing source text is unsupported"));
    }
    let start = reader.buffer_position() as usize;
    let mut depth = 1usize;
    loop {
        let at = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| refuse("invalid source text XML"))?;
        match event {
            Event::Start(_) => {
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or_else(|| refuse("invalid source text depth"))?;
                if depth == 0 {
                    if reader.buffer_position() as usize != bytes.len() || at == start {
                        return Err(refuse("empty or ambiguous source text is unsupported"));
                    }
                    return Ok(range.start + start..range.start + at);
                }
            }
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) | Event::Empty(_) => {}
            _ => return Err(refuse("source text contains unverified lexical content")),
        }
    }
}

fn encode(text: &str, remaining: usize) -> Result<Vec<u8>> {
    if text.is_empty() {
        return Err(refuse("empty edited text is unsupported"));
    }
    const SPACE: &str = "<dcpatch:s xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>";
    const TAB: &str = "<dcpatch:tab xmlns:dcpatch=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"/>";
    let length = text.chars().try_fold(0usize, |n, c| {
        let size = match c {
            ' ' => SPACE.len(),
            '\t' => TAB.len(),
            '&' => 5,
            '<' | '>' => 4,
            '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}' => c.len_utf8(),
            _ => return Err(refuse("edited text contains unsupported XML characters or paragraph breaks")),
        };
        let next = n.checked_add(size).ok_or_else(|| refuse("encoded text exceeds budget"))?;
        if next > remaining {
            return Err(refuse("encoded text exceeds budget"));
        }
        Ok(next)
    })?;
    let mut out = String::with_capacity(length);
    for c in text.chars() {
        match c {
            ' ' => out.push_str(SPACE),
            '\t' => out.push_str(TAB),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    Ok(out.into_bytes())
}

fn apply(original: &[u8], mut patches: Vec<Patch>) -> Result<Vec<u8>> {
    patches.sort_by_key(|patch| patch.range.start);
    let mut length = original.len();
    let mut end = 0;
    let original_text = std::str::from_utf8(original).map_err(|_| refuse("source content is not UTF-8"))?;
    for patch in &patches {
        if patch.range.start < end
            || patch.range.end < patch.range.start
            || patch.range.end > original.len()
            || !original_text.is_char_boundary(patch.range.start)
            || !original_text.is_char_boundary(patch.range.end)
        {
            return Err(refuse("overlapping or invalid text patches"));
        }
        length = length
            .checked_sub(patch.range.len())
            .and_then(|n| n.checked_add(patch.value.len()))
            .ok_or_else(|| refuse("patched XML exceeds budget"))?;
        if length as u64 > MAX_PART {
            return Err(refuse("patched XML exceeds budget"));
        }
        end = patch.range.end;
    }
    let mut out = Vec::with_capacity(length);
    let mut end = 0;
    for patch in patches {
        out.extend_from_slice(original.get(end..patch.range.start).ok_or_else(|| refuse("invalid immutable XML interval"))?);
        out.extend_from_slice(&patch.value);
        end = patch.range.end;
    }
    out.extend_from_slice(original.get(end..).ok_or_else(|| refuse("invalid immutable XML tail"))?);
    let root = xml::parse_part(&out, SourcePart::Content)?;
    if root.identity_metadata_bytes().saturating_add(out.len()) as u64 > MAX_TOTAL {
        return Err(refuse("patched XML metadata exceeds budget"));
    }
    Ok(out)
}

fn align_tags(desired: &mut [(String, String)], actual: &[(String, String)]) -> Result<()> {
    match (provenance(desired)?, provenance(actual)?) {
        (None, None) => Ok(()),
        (Some((part, _)), Some((new_part, range))) if part == new_part && range.len() <= 64 && range.starts_with("v1:") => {
            let value = range.to_string();
            let (_, target) = desired.iter_mut().find(|(key, _)| key == "odp:source-range").ok_or_else(|| refuse("missing output provenance"))?;
            *target = value;
            Ok(())
        }
        _ => Err(refuse("output provenance ownership changed")),
    }
}

fn align_shapes(desired: &mut [Shape], actual: &[Shape], depth: usize) -> Result<()> {
    if depth >= 64 || desired.len() != actual.len() {
        return Err(refuse("output shape ownership changed"));
    }
    for (desired, actual) in desired.iter_mut().zip(actual) {
        align_tags(&mut desired.tags, &actual.tags)?;
        if let (deckcraft_model::ShapeKind::Group { children: desired, .. }, deckcraft_model::ShapeKind::Group { children: actual, .. }) =
            (&mut desired.kind, &actual.kind)
        {
            align_shapes(desired, actual, depth + 1)?;
        }
    }
    Ok(())
}

fn align_provenance(desired: &mut Presentation, actual: &Presentation) -> Result<()> {
    if desired.slides.len() != actual.slides.len()
        || desired.masters.len() != actual.masters.len()
        || desired.notes_masters.len() != actual.notes_masters.len()
    {
        return Err(refuse("output document ownership changed"));
    }
    for (desired, actual) in desired.slides.iter_mut().zip(&actual.slides) {
        let desired = Arc::make_mut(desired);
        align_tags(&mut desired.tags, &actual.tags)?;
        align_shapes(&mut desired.shapes, &actual.shapes, 0)?;
        if let (Some(desired), Some(actual)) = (&mut desired.notes_page, &actual.notes_page) {
            align_shapes(&mut desired.shapes, &actual.shapes, 0)?;
        }
    }
    fn master(desired: &mut deckcraft_model::Master, actual: &deckcraft_model::Master) -> Result<()> {
        if desired.layouts.len() != actual.layouts.len() {
            return Err(refuse("output layout ownership changed"));
        }
        align_shapes(&mut desired.shapes, &actual.shapes, 0)?;
        for (desired, actual) in desired.layouts.iter_mut().zip(&actual.layouts) {
            align_shapes(&mut desired.shapes, &actual.shapes, 0)?;
        }
        Ok(())
    }
    for (desired, actual) in desired.masters.iter_mut().zip(&actual.masters) {
        master(Arc::make_mut(desired), actual)?;
    }
    for (desired, actual) in desired.notes_masters.iter_mut().zip(&actual.notes_masters) {
        master(Arc::make_mut(&mut desired.master), &actual.master)?;
    }
    if let (Some(desired), Some(actual)) = (&mut desired.notes_master, &actual.notes_master) {
        master(Arc::make_mut(desired), actual)?;
    }
    if let (Some(desired), Some(actual)) = (&mut desired.handout_master, &actual.handout_master) {
        master(Arc::make_mut(desired), actual)?;
    }
    Ok(())
}

fn align_text_runs(actual: &mut Presentation, desired: &Presentation) -> Result<()> {
    for (actual, desired) in actual.slides.iter_mut().zip(&desired.slides) {
        for (actual, desired) in Arc::make_mut(actual).shapes.iter_mut().zip(&desired.shapes) {
            let (Some(actual), Some(desired)) = (&mut actual.text, &desired.text) else { continue };
            for (actual, desired) in actual.paragraphs.iter_mut().zip(&desired.paragraphs) {
                if actual.runs != desired.runs && plain_text(actual, desired)? == plain_text(desired, desired)? {
                    actual.runs = desired.runs.clone();
                }
            }
        }
    }
    Ok(())
}
