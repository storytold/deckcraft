//! Audit a `.pptx` file for hidden information before sharing.
//!
//! [`audit_bytes`] scans the raw OPC package (the ZIP bytes) and reports what a
//! recipient of the file could learn: speaker notes, hidden slides, cropped
//! images that still carry their full pixel data, comment authors,
//! creator/last-modified-by in document properties, embedded media, macro and
//! embedded-object parts, and package parts nothing references.
//!
//! The audit is visibility only: it never modifies the file. Sanitizing (removing
//! what it finds) is a separate, future step. Cropped images are reported, never
//! stripped: removing the hidden pixels would destroy data.
//!
//! Detection notes (documented, not hidden):
//!
//! - Matching is by namespace URI, not by prefix, so a file that spells the
//!   PresentationML namespace with an unusual prefix is still scanned.
//! - A hidden slide is `show="false"` (W3C boolean) on the `p:sld` root of a
//!   slide part (`CT_Slide`, ECMA-376 Part 1 §19.3.1.38); omitted `show` means
//!   shown.
//! - Speaker notes are reported by presence of `ppt/notesSlides/*.xml` parts:
//!   the notes are the finding, whatever they say.
//! - Comment authors come from `ppt/commentAuthors.xml` (`p:cmAuthor`: `id`,
//!   `name`, `initials`); `p:cm` elements reference them by `authorId`.
//! - A `dc:creator` / `cp:lastModifiedBy` value is reported as-is; producers
//!   that anonymize are reported too, since the value is what a recipient sees.
//!
//! The audit fails closed: a part that cannot be read or parsed is an error,
//! never a silent pass.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use serde::Serialize;

#[cfg(test)]
mod tests;

// ECMA-376 Part 1 namespaces the audit understands.
const NS_P: &[u8] = b"http://schemas.openxmlformats.org/presentationml/2006/main";
const NS_A: &[u8] = b"http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_R: &[u8] = b"http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_DC: &[u8] = b"http://purl.org/dc/elements/1.1/";
const NS_CP: &[u8] = b"http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const NS_EP: &[u8] = b"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const NS_RELS: &[u8] = b"http://schemas.openxmlformats.org/package/2006/relationships";

/// Cap on ZIP entries read: a package is a handful of parts, not millions.
const MAX_PARTS: usize = 100_000;
/// Cap on decompressed bytes read from a single part (zip-bomb guard).
const MAX_PART_BYTES: u64 = 256 * 1024 * 1024;
/// Cap on total decompressed bytes across all scanned parts. Per-part caps
/// alone don't bound N parts that each compress well (zip-bomb guard).
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
/// Cap on XML elements walked in a single part.
const MAX_XML_ELEMENTS: u64 = 2_000_000;

/// What the audit found, as a stable machine-readable vocabulary.
///
/// The [`FindingKind::id`] strings are the crate's public contract: they appear
/// in CLI output and serialized reports. Do not rename them; add a new variant
/// instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingKind {
    /// A `ppt/notesSlides/*.xml` part exists: the deck carries speaker notes.
    NotesSlide,
    /// `show="false"` on the `p:sld` root of a slide part (`CT_Slide`,
    /// ECMA-376 Part 1 §19.3.1.38): the slide is hidden from the show.
    HiddenSlide,
    /// An `a:srcRect` crop on a `p:pic` whose `a:blip` still references the
    /// full image part: the crop only hides pixels (ECMA-376 Part 1
    /// §20.1.8.50 `CT_BlipFillProperties`, §20.1.8.51 `CT_RelativeRect`).
    CroppedImage,
    /// `name`/`initials` on a `p:cmAuthor` referenced by a `p:cm` comment
    /// (ECMA-376 Part 1 §19.4.1/§19.4.2).
    CommentAuthor,
    /// `dc:creator` / `cp:lastModifiedBy` in `docProps/core.xml`
    /// (ECMA-376 Part 2 §12.3.1).
    CorePropertiesAuthor,
    /// `Company` / `Manager` in `docProps/app.xml` (ECMA-376 Part 2 §12.3.2).
    ExtendedProperties,
    /// A part under `ppt/media/`: embedded audio, video or image data.
    EmbeddedMedia,
    /// A VBA macro project part (by relationship type, or `ppt/vbaProject.bin`):
    /// the deck carries macros.
    VbaProject,
    /// An embedded object part (by relationship type, or under
    /// `ppt/embeddings/`): an embedded (often OLE) object.
    OleEmbedding,
    /// A ZIP entry no relationship targets: leftover content nothing references.
    OrphanPart,
}

impl FindingKind {
    /// The stable machine-readable ID for this finding class.
    pub fn id(self) -> &'static str {
        match self {
            FindingKind::NotesSlide => "notes/slide",
            FindingKind::HiddenSlide => "hidden/slide",
            FindingKind::CroppedImage => "image/crop-retains-data",
            FindingKind::CommentAuthor => "author-pii/comment",
            FindingKind::CorePropertiesAuthor => "author-pii/core-properties",
            FindingKind::ExtendedProperties => "author-pii/extended-properties",
            FindingKind::EmbeddedMedia => "media/metadata",
            FindingKind::VbaProject => "active-content/vba",
            FindingKind::OleEmbedding => "active-content/ole",
            FindingKind::OrphanPart => "package/orphan-part",
        }
    }

    /// The human-readable title for this finding class.
    pub fn title(self) -> &'static str {
        match self {
            FindingKind::NotesSlide => "speaker notes present",
            FindingKind::HiddenSlide => "hidden slide",
            FindingKind::CroppedImage => "cropped image retains full pixel data",
            FindingKind::CommentAuthor => "author name on comment",
            FindingKind::CorePropertiesAuthor => "author name in document properties",
            FindingKind::ExtendedProperties => "organization name in extended properties",
            FindingKind::EmbeddedMedia => "embedded media",
            FindingKind::VbaProject => "VBA macro project",
            FindingKind::OleEmbedding => "embedded object",
            FindingKind::OrphanPart => "orphan package part",
        }
    }

    /// How much attention this class deserves: `warn` for identifying, hidden
    /// or executable content, `info` for leftovers worth knowing about.
    pub fn severity(self) -> &'static str {
        match self {
            FindingKind::OrphanPart | FindingKind::ExtendedProperties | FindingKind::EmbeddedMedia => "info",
            _ => "warn",
        }
    }
}

/// One thing the audit found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuditFinding {
    /// Stable finding-class ID (see [`FindingKind::id`]).
    pub id: String,
    /// Human-readable title (see [`FindingKind::title`]).
    pub title: String,
    /// `"warn"` or `"info"`.
    pub severity: String,
    /// Where it was found: the part name plus the element or attribute,
    /// e.g. `ppt/slides/slide2.xml <p:sld show="0">`.
    pub location: String,
    /// The matched value: an author name, a property value, a part name.
    pub detail: String,
}

impl AuditFinding {
    fn new(kind: FindingKind, location: String, detail: String) -> AuditFinding {
        AuditFinding { id: kind.id().to_string(), title: kind.title().to_string(), severity: kind.severity().to_string(), location, detail }
    }
}

/// Why the audit could not run.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    /// The bytes are not a readable ZIP archive.
    #[error("not a readable ZIP archive: {0}")]
    Zip(String),
    /// A package part needed for the audit could not be parsed as XML.
    #[error("part {0}: {1}")]
    Xml(String, String),
    /// A hostile-input guard tripped (too many parts, part too large, …).
    #[error("{0}")]
    Limit(String),
}

/// Scan the raw `.pptx` (OPC ZIP) bytes and return every finding.
///
/// Findings come back in a deterministic order (by class, location, detail).
/// Fails closed: unreadable or unparsable input is an [`AuditError`], never an
/// empty finding list that looks like a clean bill of health.
pub fn audit_bytes(bytes: &[u8]) -> Result<Vec<AuditFinding>, AuditError> {
    // One archive for the whole scan: re-opening it per part would re-parse
    // the central directory each time (quadratic on hostile input).
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| AuditError::Zip(e.to_string()))?;
    let names = part_names(&mut archive)?;
    let mut parts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut total: u64 = 0;
    for name in names.iter().filter(|n| is_scanned_part(n)) {
        let data = read_part(&mut archive, name, &mut total)?;
        parts.insert(name.clone(), data);
    }

    let mut findings = Vec::new();
    validate_presentation(&parts)?;
    scan_hidden_slides(&parts, &mut findings)?;
    scan_notes(&names, &mut findings);
    scan_cropped_images(&parts, &mut findings)?;
    scan_comments(&parts, &mut findings)?;
    scan_core_properties(&parts, &mut findings)?;
    scan_extended_properties(&parts, &mut findings)?;
    scan_media(&names, &mut findings);
    scan_active_content(&names, &parts, &mut findings)?;
    scan_orphans(&names, &parts, &mut findings)?;

    findings.sort_by(|a, b| (&a.id, &a.location, &a.detail).cmp(&(&b.id, &b.location, &b.detail)));
    Ok(findings)
}

/// The presentation part itself carries no v1 findings, but it is parsed so a
/// malformed package cannot pass the audit silently (fail-closed). The
/// presence check doubles as the not-a-presentation-package gate.
fn validate_presentation(parts: &BTreeMap<String, Vec<u8>>) -> Result<(), AuditError> {
    let Some(xml) = parts.get("ppt/presentation.xml") else {
        return Err(AuditError::Xml("ppt/presentation.xml".to_string(), "missing: not a presentation package".to_string()));
    };
    for_each_element(xml, "ppt/presentation.xml", &mut |_| Ok(()))?;
    Ok(())
}

/// Parts the audit parses as XML. Everything else is only listed by name.
fn is_scanned_part(name: &str) -> bool {
    name == "ppt/presentation.xml"
        || name == "ppt/commentAuthors.xml"
        || name == "docProps/core.xml"
        || name == "docProps/app.xml"
        || name.ends_with(".rels")
        || (name.starts_with("ppt/slides/") && name.ends_with(".xml"))
        || (name.starts_with("ppt/comments/") && name.ends_with(".xml"))
}

/// Every non-directory entry name in the ZIP, in archive order.
fn part_names(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Result<Vec<String>, AuditError> {
    let len = archive.len();
    if len > MAX_PARTS {
        return Err(AuditError::Limit(format!("too many ZIP entries: {len}")));
    }
    let mut names = Vec::with_capacity(len.min(1024));
    for i in 0..len {
        let file = archive.by_index(i).map_err(|e| AuditError::Zip(e.to_string()))?;
        let name = file.name().to_string();
        if !name.ends_with('/') {
            names.push(name);
        }
    }
    Ok(names)
}

/// Read one part, capped against decompression bombs, and charge it against
/// the cumulative budget. Fails closed: with a single archive for the whole
/// scan, a listed name that can't be read is an error, never a silent skip.
fn read_part(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, total: &mut u64) -> Result<Vec<u8>, AuditError> {
    let mut data = Vec::new();
    {
        let mut file = archive.by_name(name).map_err(|e| AuditError::Zip(format!("{name}: {e}")))?;
        let mut limited = (&mut file).take(MAX_PART_BYTES + 1);
        limited.read_to_end(&mut data).map_err(|e| AuditError::Zip(format!("{name}: {e}")))?;
    }
    if data.len() as u64 > MAX_PART_BYTES {
        return Err(AuditError::Limit(format!("part too large: {name}")));
    }
    *total = total.saturating_add(data.len() as u64);
    if *total > MAX_TOTAL_BYTES {
        return Err(AuditError::Limit(format!("package too large: {total} decompressed bytes over budget")));
    }
    Ok(data)
}

/// One element the walker saw: namespace URI, local name, decoded attributes.
#[derive(Clone)]
struct Element {
    ns: Vec<u8>,
    local: String,
    /// (namespace URI, local name, decoded value) per attribute.
    attrs: Vec<(Vec<u8>, String, String)>,
}

/// Call `f` for every start/empty element in `xml`, resolving names by
/// namespace URI so producer-chosen prefixes do not matter.
fn for_each_element(xml: &[u8], part: &str, f: &mut impl FnMut(&Element) -> Result<(), AuditError>) -> Result<(), AuditError> {
    let mut reader = NsReader::from_reader(xml);
    {
        let config = reader.config_mut();
        config.trim_text_start = false;
        config.trim_text_end = false;
        config.expand_empty_elements = false;
        // Reject well-tokenized but mismatched closing tags: a malformed part
        // must fail the audit, never produce a partial (possibly clean-looking)
        // finding list.
        config.check_end_names = true;
    }
    let mut buf = Vec::new();
    let mut count: u64 = 0;
    let mut depth: u32 = 0;
    // XML documents have exactly one root element: a second top-level
    // element (or none at all) is malformed input, not a scannable part.
    let mut root_seen = false;
    let mut check_root = |depth: u32, part: &str| -> Result<(), AuditError> {
        if depth == 0 {
            if root_seen {
                return Err(AuditError::Xml(part.to_string(), "multiple root elements".to_string()));
            }
            root_seen = true;
        }
        Ok(())
    };
    loop {
        buf.clear();
        let (resolution, event) = reader.read_resolved_event_into(&mut buf).map_err(|e| AuditError::Xml(part.to_string(), e.to_string()))?;
        // Copy the namespace URI out so the reader borrow ends here; the
        // attribute resolver below needs `&mut reader` again.
        let ns: Vec<u8> = match &resolution {
            ResolveResult::Bound(n) => n.0.to_vec(),
            _ => Vec::new(),
        };
        match event {
            Event::Start(ref e) => {
                check_root(depth, part)?;
                depth = depth.saturating_add(1);
                count += 1;
                if count > MAX_XML_ELEMENTS {
                    return Err(AuditError::Limit(format!("{part}: too many XML elements")));
                }
                f(&element_of(&mut reader, &ns, e, part)?)?;
            }
            Event::Empty(ref e) => {
                check_root(depth, part)?;
                count += 1;
                if count > MAX_XML_ELEMENTS {
                    return Err(AuditError::Limit(format!("{part}: too many XML elements")));
                }
                f(&element_of(&mut reader, &ns, e, part)?)?;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
            }
            // XML allows only whitespace, comments and processing
            // instructions outside the root element: anything else is
            // malformed input, not a scannable part.
            Event::Text(ref t) => {
                if depth == 0 && !t.as_ref().iter().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
                    return Err(AuditError::Xml(part.to_string(), "text outside the root element".to_string()));
                }
            }
            Event::CData(_) => {
                if depth == 0 {
                    return Err(AuditError::Xml(part.to_string(), "CDATA outside the root element".to_string()));
                }
            }
            Event::Eof => {
                if depth != 0 {
                    return Err(AuditError::Xml(part.to_string(), "unclosed elements at end of part".to_string()));
                }
                if !root_seen {
                    return Err(AuditError::Xml(part.to_string(), "no root element".to_string()));
                }
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Resolve one start tag into an [`Element`].
fn element_of(reader: &mut NsReader<&[u8]>, ns: &[u8], e: &BytesStart<'_>, part: &str) -> Result<Element, AuditError> {
    let ns = ns.to_vec();
    let local = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| AuditError::Xml(part.to_string(), err.to_string()))?;
        let key = attr.key;
        if key.as_ref() == b"xmlns" || key.as_ref().starts_with(b"xmlns:") {
            continue;
        }
        let (aresolution, alocal) = reader.resolver_mut().resolve_attribute(key);
        let ans = match aresolution {
            ResolveResult::Bound(ns) => ns.0.to_vec(),
            _ => Vec::new(),
        };
        let alocal = String::from_utf8_lossy(alocal.as_ref()).into_owned();
        let value = attr
            .normalized_value(XmlVersion::Explicit1_0)
            .map(|v| v.into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(&attr.value).into_owned());
        attrs.push((ans, alocal, value));
    }
    Ok(Element { ns, local, attrs })
}

/// The value of the attribute with namespace `ns` and local name `local`, if present.
fn attr<'a>(el: &'a Element, ns: &[u8], local: &str) -> Option<&'a str> {
    el.attrs.iter().find(|(ans, aname, _)| ans.as_slice() == ns && aname == local).map(|(_, _, v)| v.as_str())
}

/// The root element of an XML part.
fn root_element(xml: &[u8], part: &str) -> Result<Element, AuditError> {
    let mut root: Option<Element> = None;
    for_each_element(xml, part, &mut |el| {
        if root.is_none() {
            root = Some(el.clone());
        }
        Ok(())
    })?;
    root.ok_or_else(|| AuditError::Xml(part.to_string(), "no root element".to_string()))
}

/// Collect the text content of every element matching (`ns`, `local`).
fn texts_of(xml: &[u8], part: &str, ns: &[u8], local: &str) -> Result<Vec<String>, AuditError> {
    let mut reader = NsReader::from_reader(xml);
    {
        let config = reader.config_mut();
        config.trim_text_start = false;
        config.trim_text_end = false;
        config.expand_empty_elements = false;
        // See `for_each_element`: malformed parts fail the audit.
        config.check_end_names = true;
    }
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut depth: u32 = 0;
    let mut open: u32 = 0;
    let mut current = String::new();
    let mut count: u64 = 0;
    loop {
        buf.clear();
        let (resolution, event) = reader.read_resolved_event_into(&mut buf).map_err(|e| AuditError::Xml(part.to_string(), e.to_string()))?;
        match event {
            Event::Start(ref e) => {
                open = open.saturating_add(1);
                count += 1;
                if count > MAX_XML_ELEMENTS {
                    return Err(AuditError::Limit(format!("{part}: too many XML elements")));
                }
                let el_ns: &[u8] = match &resolution {
                    ResolveResult::Bound(n) => n.0,
                    _ => b"",
                };
                let el_local = e.local_name();
                let el_local = String::from_utf8_lossy(el_local.as_ref());
                if depth == 0 && el_ns == ns && el_local.as_ref() == local {
                    depth = 1;
                    current.clear();
                } else if depth > 0 {
                    depth = depth.saturating_add(1);
                }
            }
            Event::Empty(ref e) => {
                open = open.saturating_add(1);
                count += 1;
                if count > MAX_XML_ELEMENTS {
                    return Err(AuditError::Limit(format!("{part}: too many XML elements")));
                }
                let el_ns: &[u8] = match &resolution {
                    ResolveResult::Bound(n) => n.0,
                    _ => b"",
                };
                let el_local = e.local_name();
                let el_local = String::from_utf8_lossy(el_local.as_ref());
                if depth == 0 && el_ns == ns && el_local.as_ref() == local {
                    // An empty target element contributes its (empty) text.
                    out.push(String::new());
                } else if depth > 0 {
                    // Empty elements open and close immediately: no depth change.
                }
                open = open.saturating_sub(1);
            }
            Event::End(_) => {
                open = open.saturating_sub(1);
                if depth > 0 {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        out.push(std::mem::take(&mut current));
                    }
                }
            }
            Event::Text(ref t) => {
                if depth > 0 {
                    let decoded = t.xml10_content().map(|v| v.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(t.as_ref()).into_owned());
                    current.push_str(&decoded);
                }
            }
            Event::CData(ref t) => {
                if depth > 0 {
                    current.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            Event::Eof => {
                if open != 0 {
                    return Err(AuditError::Xml(part.to_string(), "unclosed elements at end of part".to_string()));
                }
                break;
            }
            _ => {}
        }
    }
    Ok(out)
}

/// `rId` -> resolved part name for one part's relationships.
fn rel_targets(parts: &BTreeMap<String, Vec<u8>>, rels_name: &str) -> Result<BTreeMap<String, String>, AuditError> {
    let mut map = BTreeMap::new();
    let Some(xml) = parts.get(rels_name) else { return Ok(map) };
    let source_dir = source_dir_of_rels(rels_name);
    for_each_element(xml, rels_name, &mut |el| {
        if el.ns.as_slice() == NS_RELS
            && el.local == "Relationship"
            && attr(el, b"", "TargetMode") != Some("External")
            && let (Some(id), Some(target)) = (attr(el, b"", "Id"), attr(el, b"", "Target"))
        {
            map.insert(id.to_string(), resolve_target(&source_dir, target));
        }
        Ok(())
    })?;
    Ok(map)
}

/// Slides hidden from the slide show: `show="false"` on the `p:sld` root
/// (`CT_Slide`, ECMA-376 Part 1 §19.3.1.38). Omitted `show` means shown.
fn scan_hidden_slides(parts: &BTreeMap<String, Vec<u8>>, findings: &mut Vec<AuditFinding>) -> Result<(), AuditError> {
    let mut slides: Vec<&String> = parts.keys().filter(|n| n.starts_with("ppt/slides/") && n.ends_with(".xml")).collect();
    slides.sort();
    for name in slides {
        let Some(xml) = parts.get(name) else { continue };
        let root = root_element(xml, name)?;
        if root.ns.as_slice() == NS_P && root.local == "sld" {
            let hidden = match attr(&root, b"", "show") {
                None => false,
                Some(v) => matches!(v.to_ascii_lowercase().as_str(), "false" | "0" | "off" | "no" | "f"),
            };
            if hidden {
                let show = attr(&root, b"", "show").unwrap_or("");
                findings.push(AuditFinding::new(
                    FindingKind::HiddenSlide,
                    format!("{name} <p:sld show=\"{show}\">"),
                    "slide hidden from the slide show".to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// Speaker notes: every `ppt/notesSlides/*.xml` part is a finding. The notes
/// are the leak vector, whatever they say.
fn scan_notes(names: &[String], findings: &mut Vec<AuditFinding>) {
    let mut notes: Vec<&String> = names.iter().filter(|n| n.starts_with("ppt/notesSlides/") && n.ends_with(".xml")).collect();
    notes.sort();
    for name in notes {
        findings.push(AuditFinding::new(FindingKind::NotesSlide, name.clone(), "speaker notes present".to_string()));
    }
}

/// The crop rectangle on an `a:srcRect`, if any edge is actually cropped.
/// Values are `ST_Percentage` (thousandths of a percent).
fn crop_description(el: &Element) -> Option<String> {
    let get = |n: &str| attr(el, b"", n).unwrap_or("0");
    let (l, t, r, b) = (get("l"), get("t"), get("r"), get("b"));
    // Positive offsets crop inward and hide pixels; zero or negative offsets
    // (PowerPoint writes negatives when crop handles are dragged outward)
    // hide nothing. Unparseable or non-finite values still report: fail closed.
    let crops = |v: &str| v.trim().trim_end_matches('%').parse::<f64>().map_or(true, |n| !n.is_finite() || n > 0.0);
    if [l, t, r, b].iter().any(|v| crops(v)) { Some(format!("l={l} t={t} r={r} b={b}")) } else { None }
}

/// Cropped pictures whose `a:blip` still references the full image part: the
/// crop only hides pixels (ECMA-376 Part 1 §20.1.8.50/§20.1.8.51). Reported,
/// never stripped.
fn scan_cropped_images(parts: &BTreeMap<String, Vec<u8>>, findings: &mut Vec<AuditFinding>) -> Result<(), AuditError> {
    let mut slides: Vec<&String> = parts.keys().filter(|n| n.starts_with("ppt/slides/") && n.ends_with(".xml")).collect();
    slides.sort();
    for name in slides {
        let Some(xml) = parts.get(name) else { continue };
        let targets = rel_targets(parts, &rels_of_part(name).unwrap_or_default())?;
        scan_slide_crops(xml, name, &targets, findings)?;
    }
    Ok(())
}

/// One slide part: pair each `p:pic`'s `a:blip` embed with its `a:srcRect`.
/// A dedicated walker (not `for_each_element`) because pairing needs nesting.
fn scan_slide_crops(xml: &[u8], part: &str, targets: &BTreeMap<String, String>, findings: &mut Vec<AuditFinding>) -> Result<(), AuditError> {
    let mut reader = NsReader::from_reader(xml);
    {
        let config = reader.config_mut();
        config.trim_text_start = false;
        config.trim_text_end = false;
        config.expand_empty_elements = false;
        config.check_end_names = true;
    }
    let mut buf = Vec::new();
    let mut count: u64 = 0;
    let mut depth: u32 = 0;
    let mut root_seen = false;
    // The innermost open `p:pic`, if any; pictures don't nest in practice.
    let mut in_pic = false;
    let mut pic_depth: u32 = 0;
    let mut embed: Option<String> = None;
    let mut crop: Option<String> = None;
    let mut check_root = |depth: u32, part: &str| -> Result<(), AuditError> {
        if depth == 0 {
            if root_seen {
                return Err(AuditError::Xml(part.to_string(), "multiple root elements".to_string()));
            }
            root_seen = true;
        }
        Ok(())
    };
    loop {
        buf.clear();
        let (resolution, event) = reader.read_resolved_event_into(&mut buf).map_err(|e| AuditError::Xml(part.to_string(), e.to_string()))?;
        let ns: Vec<u8> = match &resolution {
            ResolveResult::Bound(n) => n.0.to_vec(),
            _ => Vec::new(),
        };
        // Returns true when a `p:pic` just closed and its crop/blip pairing
        // is ready to report.
        let mut pic_closed = false;
        match event {
            Event::Start(ref e) => {
                check_root(depth, part)?;
                depth = depth.saturating_add(1);
                count += 1;
                if count > MAX_XML_ELEMENTS {
                    return Err(AuditError::Limit(format!("{part}: too many XML elements")));
                }
                let el = element_of(&mut reader, &ns, e, part)?;
                if el.ns.as_slice() == NS_P && el.local == "pic" && !in_pic {
                    in_pic = true;
                    pic_depth = depth;
                    embed = None;
                    crop = None;
                } else if in_pic && depth > pic_depth {
                    observe_pic_child(&el, &mut embed, &mut crop);
                }
            }
            Event::Empty(ref e) => {
                check_root(depth, part)?;
                count += 1;
                if count > MAX_XML_ELEMENTS {
                    return Err(AuditError::Limit(format!("{part}: too many XML elements")));
                }
                let el = element_of(&mut reader, &ns, e, part)?;
                if el.ns.as_slice() == NS_P && el.local == "pic" && !in_pic {
                    // An empty picture has no children: nothing to pair.
                } else if in_pic && depth >= pic_depth {
                    // An empty child fires at its parent's depth + 0, so
                    // compare against the picture's own depth.
                    observe_pic_child(&el, &mut embed, &mut crop);
                }
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if in_pic && depth < pic_depth {
                    pic_closed = true;
                }
            }
            Event::Text(ref t) => {
                if depth == 0 && !t.as_ref().iter().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
                    return Err(AuditError::Xml(part.to_string(), "text outside the root element".to_string()));
                }
            }
            Event::CData(_) => {
                if depth == 0 {
                    return Err(AuditError::Xml(part.to_string(), "CDATA outside the root element".to_string()));
                }
            }
            Event::Eof => {
                if depth != 0 {
                    return Err(AuditError::Xml(part.to_string(), "unclosed elements at end of part".to_string()));
                }
                if !root_seen {
                    return Err(AuditError::Xml(part.to_string(), "no root element".to_string()));
                }
                break;
            }
            _ => {}
        }
        if pic_closed {
            in_pic = false;
            if let (Some(rid), Some(c)) = (embed.take(), crop.take())
                && let Some(target) = targets.get(&rid)
            {
                findings.push(AuditFinding::new(
                    FindingKind::CroppedImage,
                    format!("{part} <p:pic>"),
                    format!("cropped {c}; full image retained in {target}"),
                ));
            }
        }
    }
    Ok(())
}

/// Inside a `p:pic`: remember the `a:blip` embed target and any `a:srcRect`
/// crop. Called for both start and empty elements within the picture.
fn observe_pic_child(el: &Element, embed: &mut Option<String>, crop: &mut Option<String>) {
    if el.ns.as_slice() == NS_A && el.local == "blip" {
        if let Some(rid) = attr(el, NS_R, "embed") {
            *embed = Some(rid.to_string());
        }
    } else if el.ns.as_slice() == NS_A && el.local == "srcRect" && crop.is_none() {
        *crop = crop_description(el);
    }
}

/// `p:cmAuthor` (`id`, `name`, `initials`) referenced by `p:cm` comments
/// via `authorId` (ECMA-376 Part 1 §19.4.1/§19.4.2).
fn scan_comments(parts: &BTreeMap<String, Vec<u8>>, findings: &mut Vec<AuditFinding>) -> Result<(), AuditError> {
    // id -> (name, initials)
    let mut authors: BTreeMap<String, (String, String)> = BTreeMap::new();
    if let Some(xml) = parts.get("ppt/commentAuthors.xml") {
        for_each_element(xml, "ppt/commentAuthors.xml", &mut |el| {
            if el.ns.as_slice() == NS_P
                && el.local == "cmAuthor"
                && let (Some(id), Some(name)) = (attr(el, b"", "id"), attr(el, b"", "name"))
            {
                let initials = attr(el, b"", "initials").unwrap_or("").to_string();
                authors.insert(id.to_string(), (name.to_string(), initials));
            }
            Ok(())
        })?;
    }
    let mut comment_parts: Vec<&String> = parts.keys().filter(|n| n.starts_with("ppt/comments/") && n.ends_with(".xml")).collect();
    comment_parts.sort();
    for name in comment_parts {
        let Some(xml) = parts.get(name) else { continue };
        for_each_element(xml, name, &mut |el| {
            if el.ns.as_slice() == NS_P
                && el.local == "cm"
                && let Some(author_id) = attr(el, b"", "authorId")
            {
                let detail = match authors.get(author_id) {
                    Some((author, initials)) if !author.is_empty() => {
                        if initials.is_empty() {
                            format!("author=\"{author}\"")
                        } else {
                            format!("author=\"{author}\" initials=\"{initials}\"")
                        }
                    }
                    _ => format!("authorId=\"{author_id}\" (unknown author)"),
                };
                findings.push(AuditFinding::new(FindingKind::CommentAuthor, format!("{name} <p:cm>"), detail));
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// `dc:creator` / `cp:lastModifiedBy` in `docProps/core.xml`.
fn scan_core_properties(parts: &BTreeMap<String, Vec<u8>>, findings: &mut Vec<AuditFinding>) -> Result<(), AuditError> {
    let Some(xml) = parts.get("docProps/core.xml") else { return Ok(()) };
    for (ns, local, element) in [(NS_DC, "creator", "dc:creator"), (NS_CP, "lastModifiedBy", "cp:lastModifiedBy")] {
        for value in texts_of(xml, "docProps/core.xml", ns, local)? {
            let value = value.trim();
            if !value.is_empty() {
                findings.push(AuditFinding::new(FindingKind::CorePropertiesAuthor, format!("docProps/core.xml <{element}>"), value.to_string()));
            }
        }
    }
    Ok(())
}

/// `Company` / `Manager` in `docProps/app.xml`: organizational, not personal,
/// but still identifying — reported at `info` severity.
fn scan_extended_properties(parts: &BTreeMap<String, Vec<u8>>, findings: &mut Vec<AuditFinding>) -> Result<(), AuditError> {
    let Some(xml) = parts.get("docProps/app.xml") else { return Ok(()) };
    for local in ["Company", "Manager"] {
        for value in texts_of(xml, "docProps/app.xml", NS_EP, local)? {
            let value = value.trim();
            if !value.is_empty() {
                findings.push(AuditFinding::new(FindingKind::ExtendedProperties, format!("docProps/app.xml <{local}>"), value.to_string()));
            }
        }
    }
    Ok(())
}

/// Parts under `ppt/media/`: embedded audio, video or images. Reported with
/// their part names so the sharer knows what media ships with the deck.
fn scan_media(names: &[String], findings: &mut Vec<AuditFinding>) {
    let mut media: Vec<&String> = names.iter().filter(|n| n.starts_with("ppt/media/")).collect();
    media.sort();
    for name in media {
        let kind = match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
            "mp4" | "mov" | "avi" | "wmv" | "m4v" | "webm" => "video",
            "mp3" | "wav" | "m4a" | "wma" | "ogg" | "mid" | "midi" => "audio",
            _ => "media",
        };
        findings.push(AuditFinding::new(FindingKind::EmbeddedMedia, name.clone(), format!("embedded {kind} part")));
    }
}

/// Relationship types that mark active content: the VBA project part and
/// embedded objects/packages (ECMA-376 Part 2 §11, plus the Office VBA extension).
const RT_VBA_PROJECT: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const RT_OLE_OBJECT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject";
const RT_PACKAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package";

/// Macro and embedded-object parts: reported as visibility, never blocked or
/// removed. Detection is by relationship type first — a reachable VBA or OLE
/// part may live at any valid package path — with the fixed names and
/// directories as fallback for parts whose relationships are missing.
fn scan_active_content(names: &[String], parts: &BTreeMap<String, Vec<u8>>, findings: &mut Vec<AuditFinding>) -> Result<(), AuditError> {
    // Parts already reported, so the two passes never double-report one part.
    let mut reported: BTreeSet<String> = BTreeSet::new();
    let mut report = |kind: FindingKind, location: String, detail: &str, findings: &mut Vec<AuditFinding>| {
        if reported.insert(location.clone()) {
            findings.push(AuditFinding::new(kind, location, detail.to_string()));
        }
    };
    for name in names {
        if name == "ppt/vbaProject.bin" {
            report(FindingKind::VbaProject, name.clone(), "VBA macro project present", findings);
        } else if name.starts_with("ppt/embeddings/") {
            report(FindingKind::OleEmbedding, name.clone(), "embedded object present", findings);
        }
    }
    for name in names.iter().filter(|n| n.ends_with(".rels")) {
        let Some(xml) = parts.get(name) else { continue };
        let source_dir = source_dir_of_rels(name);
        for_each_element(xml, name, &mut |el| {
            if el.ns.as_slice() == NS_RELS
                && el.local == "Relationship"
                && attr(el, b"", "TargetMode") != Some("External")
                && let Some(target) = attr(el, b"", "Target")
            {
                let resolved = resolve_target(&source_dir, target);
                match attr(el, b"", "Type").unwrap_or("") {
                    RT_VBA_PROJECT => report(FindingKind::VbaProject, resolved, "VBA macro project present (relationship type)", findings),
                    RT_OLE_OBJECT | RT_PACKAGE => {
                        report(FindingKind::OleEmbedding, resolved, "embedded object present (relationship type)", findings)
                    }
                    _ => {}
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// ZIP entries no relationship targets: content nothing references.
/// Reachability is traversed from the package root: only relationships in
/// reachable parts count, so a `.rels` file inside an orphaned subtree must
/// not rescue its targets from orphan status.
fn scan_orphans(names: &[String], parts: &BTreeMap<String, Vec<u8>>, findings: &mut Vec<AuditFinding>) -> Result<(), AuditError> {
    let name_set: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    let mut reachable: BTreeSet<String> = BTreeSet::new();
    reachable.insert("[Content_Types].xml".to_string());
    let mut stack = vec!["_rels/.rels".to_string()];
    while let Some(rels_name) = stack.pop() {
        if !name_set.contains(rels_name.as_str()) || !reachable.insert(rels_name.clone()) {
            continue;
        }
        let Some(xml) = parts.get(&rels_name) else { continue };
        let source_dir = source_dir_of_rels(&rels_name);
        let mut targets = Vec::new();
        for_each_element(xml, &rels_name, &mut |el| {
            if el.ns.as_slice() == NS_RELS
                && el.local == "Relationship"
                && let Some(target) = attr(el, b"", "Target")
                && attr(el, b"", "TargetMode") != Some("External")
            {
                targets.push(resolve_target(&source_dir, target));
            }
            Ok(())
        })?;
        for target in targets {
            if name_set.contains(target.as_str())
                && reachable.insert(target.clone())
                && let Some(rels) = rels_of_part(&target)
            {
                stack.push(rels);
            }
        }
    }
    for name in names {
        if !reachable.contains(name) {
            findings.push(AuditFinding::new(FindingKind::OrphanPart, name.clone(), "no relationship targets this part".to_string()));
        }
    }
    Ok(())
}

/// The directory of the part a `*.rels` file describes:
/// `ppt/_rels/presentation.xml.rels` describes `ppt/presentation.xml`, whose
/// directory is `ppt`; `_rels/.rels` describes the package root.
fn source_dir_of_rels(rels: &str) -> String {
    let without_ext = rels.strip_suffix(".rels").unwrap_or(rels);
    match without_ext.rfind("/_rels/") {
        Some(i) => without_ext[..i].to_string(),
        None => String::new(),
    }
}

/// The `*.rels` path for a package part (`ppt/slides/slide1.xml` →
/// `ppt/slides/_rels/slide1.xml.rels`), per OPC §12.2.4. `None` for parts that
/// have no relationships part (`[Content_Types].xml`).
fn rels_of_part(part: &str) -> Option<String> {
    if part == "[Content_Types].xml" {
        return None;
    }
    let (dir, file) = match part.rfind('/') {
        Some(i) => (&part[..i], &part[i + 1..]),
        None => ("", part),
    };
    if file.is_empty() {
        return None;
    }
    Some(if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") })
}

/// Resolve a relationship target against the source part's directory, lexically.
/// Absolute targets (`/ppt/...`) resolve from the package root.
fn resolve_target(source_dir: &str, target: &str) -> String {
    let joined = if let Some(abs) = target.strip_prefix('/') {
        abs.to_string()
    } else if source_dir.is_empty() {
        target.to_string()
    } else {
        format!("{source_dir}/{target}")
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}
