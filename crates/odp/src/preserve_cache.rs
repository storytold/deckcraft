//! Optional derived-cache removal after a complete, closed dependency proof.
//!
//! CachePlan is invocation-local: its parsed document map must remain alive and immutable
//! until all allows_statistic calls finish. The opaque address is compared, never dereferenced
//! or persisted. Output verification constructs a fresh parsed view and fresh dependency proof.
use super::refuse;
use crate::{
    MAX_PART, MAX_TOTAL, Result,
    xml::{self, El, SourcePart},
};
use quick_xml::{Reader, events::Event};
use std::{collections::BTreeMap, ops::Range};

#[path = "preserve_cache_lexical.rs"]
mod lexical;
#[path = "preserve_cache_profile.rs"]
mod profile;

const THUMBNAIL: &str = "Thumbnails/thumbnail.png";
const MANIFEST: &str = "META-INF/manifest.xml";
struct Statistic {
    address: usize,
    range: Range<usize>,
}
pub(super) struct CachePlan {
    thumbnail: bool,
    statistic: Option<Statistic>,
    replacements: BTreeMap<String, Vec<u8>>,
}

fn charge(budget: &mut usize, amount: usize) -> Result<()> {
    *budget = budget.checked_add(amount).ok_or_else(|| refuse("cache work budget"))?;
    if *budget > MAX_TOTAL as usize {
        return Err(refuse("cache work budget"));
    }
    Ok(())
}
/// Some(true) denotes an optional text-dependent count; false denotes retained structure.
fn stat_attr(key: &str) -> Option<bool> {
    match key {
        "meta:word-count" | "meta:character-count" | "meta:non-whitespace-character-count" | "meta:sentence-count" | "meta:syllable-count" => {
            Some(true)
        }
        "meta:page-count"
        | "meta:paragraph-count"
        | "meta:frame-count"
        | "meta:draw-count"
        | "meta:image-count"
        | "meta:object-count"
        | "meta:ole-object-count"
        | "meta:table-count"
        | "meta:cell-count"
        | "meta:row-count" => Some(false),
        _ => None,
    }
}
fn discover<'a>(el: &'a El, parent: &str, part: &str, found: &mut Option<&'a El>, budget: &mut usize, depth: usize) -> Result<()> {
    charge(budget, 64)?;
    if depth > 64 {
        return Err(refuse("cache discovery depth"));
    }
    if el.name == "meta:document-statistic" && (part != "meta.xml" || parent != "office:meta" || found.replace(el).is_some()) {
        return Err(refuse("cache statistic ownership"));
    }
    for child in el.elements() {
        discover(child, &el.name, part, found, budget, depth + 1)?;
    }
    Ok(())
}
fn lexical_document(bytes: &[u8], budget: &mut usize) -> Result<()> {
    if bytes.len() > MAX_PART as usize {
        return Err(refuse("cache XML part size"));
    }
    charge(budget, bytes.len())?;
    let mut reader = Reader::from_reader(bytes);
    loop {
        match reader.read_event().map_err(|_| refuse("cache XML lexical syntax"))? {
            Event::DocType(_) | Event::PI(_) => return Err(refuse("cache XML active lexical content")),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}
fn validate_sources(el: &El, bytes: &[u8], part: SourcePart, budget: &mut usize, depth: usize) -> Result<()> {
    if depth == 0 {
        charge(budget, bytes.len())?;
        if !el.source_bytes_match(bytes) {
            return Err(refuse("cache parsed part does not match immutable source bytes"));
        }
    }
    if depth > 64 || el.source_provenance().is_none_or(|(p, r)| p != part || r.end > bytes.len() || r.start >= r.end) {
        return Err(refuse("cache source part/range"));
    }
    lexical::attributes(bytes, el, budget)?;
    for child in el.elements() {
        validate_sources(child, bytes, part, budget, depth + 1)?;
    }
    Ok(())
}
fn source_part(name: &str) -> Result<SourcePart> {
    match name {
        "content.xml" => Ok(SourcePart::Content),
        "styles.xml" => Ok(SourcePart::Styles),
        "meta.xml" | "settings.xml" => Ok(SourcePart::Metadata),
        _ => Err(refuse("cache fixed XML part")),
    }
}
fn png(bytes: &[u8], budget: &mut usize) -> Result<()> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(8..16) != Some(b"\0\0\0\rIHDR") {
        return Err(refuse("cache thumbnail PNG header"));
    }
    let dimension = |range: Range<usize>| -> Result<u32> {
        let a: [u8; 4] =
            bytes.get(range).ok_or_else(|| refuse("cache thumbnail dimensions"))?.try_into().map_err(|_| refuse("cache thumbnail dimensions"))?;
        Ok(u32::from_be_bytes(a))
    };
    let w = dimension(16..20)?;
    let h = dimension(20..24)?;
    if w == 0 || h == 0 || w > 2048 || h > 2048 {
        return Err(refuse("cache thumbnail size"));
    }
    let rgba = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)).ok_or_else(|| refuse("cache thumbnail allocation"))?;
    charge(budget, rgba)?;
    let decoded = deckcraft_render::decode_image(bytes).ok_or_else(|| refuse("cache invalid thumbnail PNG"))?;
    if decoded.width() != w || decoded.height() != h || decoded.as_raw().len() != rgba {
        return Err(refuse("cache thumbnail decoded extent"));
    }
    Ok(())
}

pub(super) fn plan(
    files: &BTreeMap<String, Vec<u8>>,
    documents: &BTreeMap<String, El>,
    manifest: &El,
    budget: &mut usize,
) -> Result<Option<CachePlan>> {
    let mut statistic = None;
    for (part, document) in documents {
        discover(document, "", part, &mut statistic, budget, 0)?;
    }
    let preview = files.keys().any(|name| name.starts_with("Thumbnails/") && name != "Thumbnails/")
        || manifest.elements().any(|e| e.attr("manifest:full-path").is_some_and(|p| p.starts_with("Thumbnails/") && p != "Thumbnails/"));
    if !preview && statistic.is_none() {
        return Ok(None);
    }
    profile::Profile::validate(files, documents, manifest, budget)?;
    for (part, document) in documents {
        let bytes = files.get(part).ok_or_else(|| refuse("cache XML bytes missing"))?;
        lexical_document(bytes, budget)?;
        validate_sources(document, bytes, source_part(part)?, budget, 0)?;
    }
    let manifest_bytes = files.get(MANIFEST).ok_or_else(|| refuse("cache manifest missing"))?;
    lexical_document(manifest_bytes, budget)?;
    validate_sources(manifest, manifest_bytes, SourcePart::Manifest, budget, 0)?;
    let mut result = CachePlan { thumbnail: false, statistic: None, replacements: BTreeMap::new() };
    if preview {
        for (name, bytes) in files {
            if name.starts_with("Thumbnails/") && name != THUMBNAIL && !(name == "Thumbnails/" && bytes.is_empty()) {
                return Err(refuse("cache unknown preview inventory"));
            }
        }
        let image = files.get(THUMBNAIL).ok_or_else(|| refuse("cache thumbnail missing"))?;
        png(image, budget)?;
        let mut owner = None;
        for entry in manifest.elements() {
            let path = entry.attr("manifest:full-path").unwrap_or("");
            if path.starts_with("Thumbnails/") {
                if path == "Thumbnails/" && entry.attr("manifest:media-type") == Some("") && files.get(path).is_some_and(Vec::is_empty) {
                    lexical::empty_leaf(manifest_bytes, entry, budget)?;
                    continue;
                }
                if path != THUMBNAIL || entry.attr("manifest:media-type") != Some("image/png") || owner.replace(entry).is_some() {
                    return Err(refuse("cache thumbnail owner"));
                }
            }
        }
        let owner = owner.ok_or_else(|| refuse("cache thumbnail owner missing"))?;
        lexical::empty_leaf(manifest_bytes, owner, budget)?;
        let (_, range) = owner.source_provenance().ok_or_else(|| refuse("cache thumbnail owner source"))?;
        result.replacements.insert(MANIFEST.into(), lexical::remove(manifest_bytes, vec![range], budget)?);
        result.thumbnail = true;
    }
    if let Some(el) = statistic {
        let bytes = files.get("meta.xml").ok_or_else(|| refuse("cache statistic bytes"))?;
        lexical::empty_leaf(bytes, el, budget)?;
        let (_, range) = el.source_provenance().ok_or_else(|| refuse("cache statistic source"))?;
        let attrs = lexical::attributes(bytes, el, budget)?;
        let mut removals = Vec::new();
        for attr in &el.expanded_attrs {
            if stat_attr(&attr.projected_key) == Some(true) {
                let r = attrs.iter().find(|(i, _)| *i == attr.index).map(|(_, r)| r.clone()).ok_or_else(|| refuse("cache statistic ordinal"))?;
                charge(budget, 32)?;
                removals.push(r);
            }
        }
        if !removals.is_empty() {
            result.replacements.insert("meta.xml".into(), lexical::remove(bytes, removals, budget)?);
        }
        result.statistic = Some(Statistic { address: std::ptr::from_ref(el).addr(), range });
    }
    Ok(Some(result))
}
impl CachePlan {
    pub(super) fn allows_thumbnail(&self, name: &str) -> bool {
        self.thumbnail && name == THUMBNAIL
    }
    pub(super) fn allows_statistic(&self, part: &str, el: &El) -> bool {
        part == "meta.xml"
            && self.statistic.as_ref().is_some_and(|s| {
                s.address == std::ptr::from_ref(el).addr() && el.source_provenance().is_some_and(|(p, r)| p == SourcePart::Metadata && r == s.range)
            })
    }
    pub(super) fn replacement(&self, name: &str) -> Option<&[u8]> {
        self.replacements.get(name).map(Vec::as_slice)
    }
    pub(super) fn omits(&self, name: &str) -> bool {
        self.allows_thumbnail(name)
    }
    pub(super) fn verify_output(&self, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
        let mut budget = 0usize;
        let mut documents = BTreeMap::new();
        for name in ["content.xml", "styles.xml", "meta.xml", "settings.xml"] {
            if let Some(bytes) = files.get(name) {
                charge(&mut budget, bytes.len())?;
                let el = xml::parse_part(bytes, source_part(name)?)?;
                charge(&mut budget, el.identity_metadata_bytes())?;
                documents.insert(name.into(), el);
            }
        }
        let bytes = files.get(MANIFEST).ok_or_else(|| refuse("cache output manifest"))?;
        charge(&mut budget, bytes.len())?;
        let manifest = xml::parse_part(bytes, SourcePart::Manifest)?;
        charge(&mut budget, manifest.identity_metadata_bytes())?;
        profile::Profile::validate(files, &documents, &manifest, &mut budget)?;
        let mut stats = None;
        for (name, el) in &documents {
            discover(el, "", name, &mut stats, &mut budget, 0)?;
            lexical_document(files.get(name).ok_or_else(|| refuse("cache output XML bytes"))?, &mut budget)?;
        }
        lexical_document(bytes, &mut budget)?;
        if self.thumbnail && (files.contains_key(THUMBNAIL) || manifest.elements().any(|e| e.attr("manifest:full-path") == Some(THUMBNAIL))) {
            return Err(refuse("cache stale output thumbnail"));
        }
        if stats.is_some_and(|e| e.attrs.keys().any(|k| stat_attr(k) == Some(true))) {
            return Err(refuse("cache stale output counts"));
        }
        for (name, expected) in &self.replacements {
            if files.get(name) != Some(expected) {
                return Err(refuse("cache output replacement mismatch"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(clippy::expect_used)]
    fn fixture() -> (BTreeMap<String, Vec<u8>>, BTreeMap<String, El>, El) {
        let meta = b"<office:document-meta xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' office:version='1.3'><office:meta><meta:document-statistic meta:word-count='1'/></office:meta></office:document-meta>".to_vec();
        let manifest = b"<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='meta.xml' manifest:media-type='text/xml'/></manifest:manifest>".to_vec();
        let docs = BTreeMap::from([("meta.xml".into(), xml::parse_part(&meta, SourcePart::Metadata).expect("metadata"))]);
        let tree = xml::parse_part(&manifest, SourcePart::Manifest).expect("manifest");
        (BTreeMap::from([("meta.xml".into(), meta), (MANIFEST.into(), manifest)]), docs, tree)
    }
    #[test]
    #[allow(clippy::expect_used)]
    fn cumulative_budget_and_source_mismatch_fail_closed() {
        let (mut files, docs, manifest) = fixture();
        let mut exhausted = MAX_TOTAL as usize;
        assert!(plan(&files, &docs, &manifest, &mut exhausted).is_err());
        let mut budget = 0;
        assert!(plan(&files, &docs, &manifest, &mut budget).expect("original proof").is_some());
        let bytes = files.get_mut("meta.xml").expect("metadata");
        let index = bytes.windows(5).position(|w| w == b"'1'/>").expect("count value");
        bytes[index + 1] = b'2';
        assert!(plan(&files, &docs, &manifest, &mut 0).is_err());
        let (mut files, docs, manifest) = fixture();
        let original = String::from_utf8(files["meta.xml"].clone()).expect("UTF8");
        files.insert("meta.xml".into(), original.replace("meta:word-count", "meta:page-count").into_bytes());
        assert!(plan(&files, &docs, &manifest, &mut 0).is_err(), "equal-valued renamed source attribute");
    }
    #[test]
    #[allow(clippy::expect_used)]
    fn output_verification_freshly_profiles_and_requires_exact_shrink() {
        let (mut files, docs, manifest) = fixture();
        let p = plan(&files, &docs, &manifest, &mut 0).expect("proof").expect("cache");
        assert!(p.verify_output(&files).is_err(), "stale count");
        files.insert("meta.xml".into(), p.replacement("meta.xml").expect("shrink").to_vec());
        p.verify_output(&files).expect("fresh output proof");
        let bad = String::from_utf8(files["meta.xml"].clone()).expect("UTF8").replace("<office:meta>", "<office:meta xml:base='file:///' >");
        files.insert("meta.xml".into(), bad.into_bytes());
        assert!(p.verify_output(&files).is_err());
    }
    #[test]
    #[allow(clippy::expect_used)]
    fn statistic_permission_is_invocation_node_identity() {
        let bytes = b"<meta:document-statistic xmlns:meta='urn:oasis:names:tc:opendocument:xmlns:meta:1.0' meta:word-count='1'/>";
        let original = xml::parse_part(bytes, SourcePart::Metadata).expect("original");
        let other = xml::parse_part(bytes, SourcePart::Metadata).expect("lookalike");
        let p = CachePlan {
            thumbnail: false,
            statistic: Some(Statistic { address: std::ptr::from_ref(&original).addr(), range: 0..bytes.len() }),
            replacements: BTreeMap::new(),
        };
        assert!(p.allows_statistic("meta.xml", &original));
        assert!(!p.allows_statistic("meta.xml", &other));
        assert!(!p.allows_statistic("settings.xml", &original));
    }
}
