//! Open Packaging Conventions: the zip container, part names, relationships and content types.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use crate::PptxError;
use crate::xml::{self, A, W};

/// Largest single part we decompress (media included).
pub const MAX_PART: u64 = 512 * 1024 * 1024;
/// Largest XML part we parse.
pub const MAX_XML_PART: usize = 96 * 1024 * 1024;
/// Total decompressed bytes accepted for one package.
pub const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
/// Total bytes we are willing to decompress for one package, counting parts we then discard
/// (too large, or failing part-way): bounds inflate work, not just what is kept.
pub const MAX_INFLATE_WORK: u64 = 2 * MAX_TOTAL;
/// Maximum number of zip entries.
pub const MAX_ENTRIES: usize = 50_000;

pub const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub const NS_C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
pub const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
pub const NS_P14: &str = "http://schemas.microsoft.com/office/powerpoint/2010/main";
pub const NS_P15: &str = "http://schemas.microsoft.com/office/powerpoint/2012/main";
pub const NS_P159: &str = "http://schemas.microsoft.com/office/powerpoint/2015/09/main";
pub const NS_A14: &str = "http://schemas.microsoft.com/office/drawing/2010/main";
/// Modern (threaded) comments, Office 2018/8 schema.
pub const NS_P188: &str = "http://schemas.microsoft.com/office/powerpoint/2018/8/main";

pub const RT_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
pub const RT_MEDIA: &str = "http://schemas.microsoft.com/office/2007/relationships/media";
/// A 3-D model part (`.glb`). Office relationship, not an ECMA-376 base type.
pub const RT_MODEL3D: &str = "http://schemas.microsoft.com/office/2017/06/relationships/model3d";
pub const RT_CORE: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";

/// The files of a package, keyed by normalised part name (no leading slash, lowercase).
pub struct Package {
    parts: HashMap<String, (String, Vec<u8>)>,
    ct_cache: std::sync::OnceLock<(HashMap<String, String>, HashMap<String, String>)>,
}

impl Package {
    pub fn open(bytes: &[u8]) -> Result<Self, PptxError> {
        Self::open_within(bytes, MAX_PART, MAX_TOTAL, MAX_INFLATE_WORK)
    }
    /// [`Package::open`] with explicit limits: per part, kept in total, and inflated in total.
    fn open_within(bytes: &[u8], max_part: u64, max_total: u64, max_work: u64) -> Result<Self, PptxError> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| PptxError::NotPptx(format!("{e}")))?;
        if zip.len() > MAX_ENTRIES {
            return Err(PptxError::NotPptx(format!("too many entries ({})", zip.len())));
        }
        let mut parts = HashMap::new();
        let mut total: u64 = 0;
        let mut work: u64 = 0;
        for i in 0..zip.len() {
            if work >= max_work {
                log::warn!("pptx: decompression budget spent; skipping the remaining {} entries", zip.len() - i);
                break;
            }
            let mut f = match zip.by_index(i) {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("pptx: skipping unreadable zip entry {i}: {e}");
                    continue;
                }
            };
            if f.is_dir() {
                continue;
            }
            let name = f.name().trim_start_matches('/').to_string();
            let declared = f.size();
            if declared > max_part {
                log::warn!("pptx: skipping oversized part {name} ({declared} bytes)");
                continue;
            }
            let mut data = Vec::with_capacity(declared.min(16 * 1024 * 1024) as usize);
            let cap = max_part.min(max_total.saturating_sub(total)).min(max_work.saturating_sub(work));
            let read = (&mut f).take(cap.saturating_add(1)).read_to_end(&mut data);
            // Charge what was inflated whether or not the part is kept.
            work = work.saturating_add(data.len() as u64);
            match read {
                Ok(_) if data.len() as u64 > cap => {
                    log::warn!("pptx: part {name} exceeds size limits; skipped");
                    continue;
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!("pptx: could not read {name}: {e}");
                    continue;
                }
            }
            total = total.saturating_add(data.len() as u64);
            // A name kept with `\` would reach code that splits paths on `/`.
            parts.insert(norm(&name), (name.replace('\\', "/"), data));
        }
        Ok(Package { parts, ct_cache: std::sync::OnceLock::new() })
    }
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.parts.get(&norm(name)).map(|(_, d)| d.as_slice())
    }
    /// An XML part's bytes, when it is within the XML size limit.
    pub fn xml_bytes(&self, name: &str) -> Option<&[u8]> {
        let b = self.get(name)?;
        if b.len() > MAX_XML_PART {
            log::warn!("pptx: XML part {name} too large");
            return None;
        }
        Some(b)
    }
    /// Parse an XML part.
    pub fn xml(&self, name: &str) -> Option<xml::Doc> {
        let b = self.get(name)?;
        if b.len() > MAX_XML_PART {
            log::warn!("pptx: XML part {name} too large");
            return None;
        }
        match xml::parse(b) {
            Ok(d) => Some(d),
            Err(e) => {
                log::warn!("pptx: {name}: {e}");
                None
            }
        }
    }
    /// Relationships of a part (`ppt/slides/slide1.xml` → `ppt/slides/_rels/slide1.xml.rels`).
    pub fn rels(&self, part: &str) -> Rels {
        let rp = rels_path(part);
        let Some(doc) = self.xml(&rp) else { return Rels::default() };
        rels_of(&doc, part)
    }
    /// Content types: (extension defaults, part overrides).
    pub fn content_type(&self, part: &str) -> Option<String> {
        let (overrides, defaults) = self.ct_cache.get_or_init(|| {
            let mut ov = HashMap::new();
            let mut def = HashMap::new();
            if let Some(doc) = self.xml("[Content_Types].xml") {
                for o in doc.root.children_named("Override") {
                    if let (Some(n), Some(ct)) = (o.attr("PartName"), o.attr("ContentType")) {
                        ov.insert(n.to_ascii_lowercase(), ct.to_string());
                    }
                }
                for d in doc.root.children_named("Default") {
                    if let (Some(e), Some(ct)) = (d.attr("Extension"), d.attr("ContentType")) {
                        def.insert(e.to_ascii_lowercase(), ct.to_string());
                    }
                }
            }
            (ov, def)
        });
        let p = format!("/{}", norm(part));
        if let Some(ct) = overrides.get(&p) {
            return Some(ct.clone());
        }
        let ext = part.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        defaults.get(&ext).cloned()
    }
    pub fn original_name(&self, name: &str) -> Option<&str> {
        self.parts.get(&norm(name)).map(|(n, _)| n.as_str())
    }
}

/// The relationships in a parsed rels part, with internal targets resolved against `part`'s folder.
fn rels_of(doc: &xml::Doc, part: &str) -> Rels {
    let base = dir_of(part);
    let mut out = Rels::default();
    for r in doc.root.children_named("Relationship") {
        let (Some(id), Some(ty), Some(target)) = (r.attr("Id"), r.attr("Type"), r.attr("Target")) else { continue };
        let external = r.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("external"));
        let target_raw = target.to_string();
        let target = if external { target_raw.clone() } else { resolve(&base, target) };
        out.items.push(Rel {
            id: id.to_string(),
            kind: ty.rsplit('/').next().unwrap_or(ty).to_string(),
            ty: ty.to_string(),
            target_raw,
            target,
            external,
        });
    }
    out
}

/// The relationships of `part` from the bytes of its rels part (kept parts carried by a chart).
pub fn rels_from_bytes(bytes: &[u8], part: &str) -> Rels {
    match xml::parse(bytes) {
        Ok(doc) => rels_of(&doc, part),
        Err(e) => {
            log::warn!("pptx: relationships of {part}: {e}");
            Rels::default()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Rel {
    pub id: String,
    /// Last segment of the relationship type (`slide`, `image`, `media`…).
    pub kind: String,
    /// Full relationship type URI.
    pub ty: String,
    /// `Target` as written in the rels part (relative path or URL).
    pub target_raw: String,
    /// Absolute part name for internal targets; the URL for external ones.
    pub target: String,
    pub external: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Rels {
    pub items: Vec<Rel>,
}

impl Rels {
    pub fn get(&self, id: &str) -> Option<&Rel> {
        self.items.iter().find(|r| r.id == id)
    }
    pub fn target(&self, id: &str) -> Option<&str> {
        self.get(id).map(|r| r.target.as_str())
    }
    pub fn first_of(&self, kind: &str) -> Option<&Rel> {
        self.items.iter().find(|r| r.kind == kind)
    }
    pub fn all_of<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Rel> + 'a {
        self.items.iter().filter(move |r| r.kind == kind)
    }
}

pub fn norm(name: &str) -> String {
    name.trim_start_matches('/').replace('\\', "/").to_ascii_lowercase()
}

/// A part a chart may carry along beside its own `chartN.xml` (workbook, style, drawing, picture).
/// Chart documents themselves are rewritten, and nothing outside these folders is copied: a relationship
/// must not be able to replace `ppt/slides/slide1.xml` on save.
pub fn sidecar_part(name: &str) -> bool {
    let n = norm(name);
    let segs: Vec<&str> = n.split('/').filter(|s| !s.is_empty()).collect();
    if segs.iter().any(|s| *s == "." || *s == "..") {
        return false;
    }
    let (Some(root), Some(area)) = (segs.first().copied(), segs.get(1).copied()) else { return false };
    if root != "ppt" || !matches!(area, "embeddings" | "charts" | "drawings" | "media") {
        return false;
    }
    let file = segs.last().copied().unwrap_or("");
    let stem = file.strip_prefix("chart").unwrap_or("");
    let num = stem.strip_suffix(".xml.rels").or_else(|| stem.strip_suffix(".xml")).unwrap_or("");
    if stem != file && !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

pub fn dir_of(part: &str) -> String {
    let p = part.trim_start_matches('/');
    match p.rfind('/') {
        Some(i) => p.get(..i).unwrap_or("").to_string(),
        None => String::new(),
    }
}

pub fn file_of(part: &str) -> &str {
    part.rsplit('/').next().unwrap_or(part)
}

pub fn rels_path(part: &str) -> String {
    let p = part.trim_start_matches('/');
    let dir = dir_of(p);
    let file = file_of(p);
    if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b.get(i).copied().unwrap_or(0);
        if c == b'%'
            && let Some(h) = b
                .get(i + 1..i + 3)
                .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(h);
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolve a relative target against a base directory into a part name.
pub fn resolve(base: &str, target: &str) -> String {
    let target = percent_decode(target.split('#').next().unwrap_or("")).replace('\\', "/");
    let target = target.as_str();
    let mut segs: Vec<&str> = if target.starts_with('/') { vec![] } else { base.split('/').filter(|s| !s.is_empty()).collect() };
    for s in target.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    segs.join("/")
}

/// Relative target from the directory of `from` to part `to` (both absolute part names).
pub fn relative(from: &str, to: &str) -> String {
    let fd_s = dir_of(from);
    let fd: Vec<&str> = fd_s.split('/').filter(|s| !s.is_empty()).collect();
    let td_s = dir_of(to);
    let td: Vec<&str> = td_s.split('/').filter(|s| !s.is_empty()).collect();
    let common = fd.iter().zip(td.iter()).take_while(|(a, b)| a == b).count();
    let mut out = String::new();
    for _ in common..fd.len() {
        out.push_str("../");
    }
    for s in td.iter().skip(common) {
        out.push_str(s);
        out.push('/');
    }
    out.push_str(file_of(to));
    out
}

// ---------------------------------------------------------------------------------------------
// Writing

/// Relationships being built for one part.
#[derive(Default)]
pub struct RelsOut {
    pub items: Vec<(String, String, String, bool)>,
}

impl RelsOut {
    /// Add (or reuse) a relationship; `kind` is a full type URI or a suffix of the standard base.
    pub fn add(&mut self, kind: &str, target: &str, external: bool) -> String {
        let ty = if kind.starts_with("http") { kind.to_string() } else { format!("{RT_BASE}{kind}") };
        if let Some((id, ..)) = self.items.iter().find(|(_, t, tg, e)| *t == ty && tg == target && *e == external) {
            return id.clone();
        }
        let id = format!("rId{}", self.items.len() + 1);
        self.items.push((id.clone(), ty, target.to_string(), external));
        id
    }
    /// Always add a new relationship even if an identical one exists.
    pub fn add_new(&mut self, kind: &str, target: &str, external: bool) -> String {
        let ty = if kind.starts_with("http") { kind.to_string() } else { format!("{RT_BASE}{kind}") };
        let id = format!("rId{}", self.items.len() + 1);
        self.items.push((id.clone(), ty, target.to_string(), external));
        id
    }
    pub fn to_xml(&self) -> Vec<u8> {
        let mut w = W::new();
        w.open("Relationships", A::new().a("xmlns", "http://schemas.openxmlformats.org/package/2006/relationships"));
        for (id, ty, target, ext) in &self.items {
            let mut a = A::new().a("Id", id).a("Type", ty).a("Target", target);
            if *ext {
                a = a.a("TargetMode", "External");
            }
            w.empty("Relationship", a);
        }
        w.close("Relationships");
        w.finish()
    }
}

/// A package being written.
pub struct PackageOut {
    files: Vec<(String, Vec<u8>, bool)>,
    defaults: Vec<(String, String)>,
    overrides: Vec<(String, String)>,
}

impl Default for PackageOut {
    fn default() -> Self {
        PackageOut {
            files: vec![],
            defaults: vec![
                ("rels".into(), "application/vnd.openxmlformats-package.relationships+xml".into()),
                ("xml".into(), "application/xml".into()),
            ],
            overrides: vec![],
        }
    }
}

impl PackageOut {
    /// Add an XML part with its content type override.
    pub fn xml_part(&mut self, name: &str, content_type: &str, data: Vec<u8>) {
        self.overrides.push((format!("/{name}"), content_type.to_string()));
        self.files.push((name.to_string(), data, true));
    }
    pub fn rels(&mut self, part: &str, rels: &RelsOut) {
        if rels.items.is_empty() {
            return;
        }
        let path = rels_path(part);
        if let Some(existing) = self.files.iter_mut().find(|(name, ..)| *name == path) {
            existing.1 = rels.to_xml();
        } else {
            self.files.push((path, rels.to_xml(), true));
        }
    }
    /// Add a part with no content type of its own (a relationships part).
    pub fn raw(&mut self, name: &str, data: Vec<u8>) {
        self.files.push((name.to_string(), data, true));
    }
    /// Add a binary part; its extension gets a Default content type.
    pub fn binary(&mut self, name: &str, content_type: &str, data: Vec<u8>) {
        let ext = name.rsplit('.').next().unwrap_or("bin").to_ascii_lowercase();
        if !self.defaults.iter().any(|(e, _)| *e == ext) {
            self.defaults.push((ext, content_type.to_string()));
        } else if !self.defaults.iter().any(|(e, c)| *e == ext && c == content_type) {
            self.overrides.push((format!("/{name}"), content_type.to_string()));
        }
        self.files.push((name.to_string(), data, false));
    }
    pub fn finish(self) -> Result<Vec<u8>, PptxError> {
        let mut ct = W::new();
        ct.open("Types", A::new().a("xmlns", "http://schemas.openxmlformats.org/package/2006/content-types"));
        for (e, c) in &self.defaults {
            ct.empty("Default", A::new().a("Extension", e).a("ContentType", c));
        }
        for (p, c) in &self.overrides {
            ct.empty("Override", A::new().a("PartName", p).a("ContentType", c));
        }
        ct.close("Types");
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let err = |e: &dyn std::fmt::Display| PptxError::Write(e.to_string());
        zw.start_file("[Content_Types].xml", deflate).map_err(|e| err(&e))?;
        zw.write_all(&ct.finish()).map_err(|e| err(&e))?;
        let mut written = std::collections::HashSet::new();
        for (name, data, compress) in &self.files {
            // Zip entry names must be unique: a second part under a name already written is
            // dropped rather than producing a package readers reject or resolve differently.
            if !written.insert(norm(name)) {
                log::warn!("pptx: part {name} written twice; keeping the first");
                continue;
            }
            let large = data.len() as u64 >= u32::MAX as u64 / 2;
            let opts = if *compress { deflate } else { stored }.large_file(large);
            zw.start_file(name.as_str(), opts).map_err(|e| err(&e))?;
            zw.write_all(data).map_err(|e| err(&e))?;
        }
        let cur = zw.finish().map_err(|e| err(&e))?;
        Ok(cur.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_keeps_encoded_hash_and_strips_fragment() {
        assert_eq!(resolve("ppt/slides", "../media/a%23b.png"), "ppt/media/a#b.png");
        assert_eq!(resolve("ppt/slides", "../media/a.png#frag"), "ppt/media/a.png");
    }

    #[test]
    fn resolve_percent_decode_requires_two_hex_digits() {
        assert_eq!(resolve("ppt/slides", "../media/a%+5b.png"), "ppt/media/a%+5b.png");
        assert_eq!(resolve("ppt/slides", "../media/a%20b.png"), "ppt/media/a b.png");
    }

    fn zip_of(entries: &[(&str, usize)]) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, len) in entries {
            zw.start_file(*name, o).unwrap();
            zw.write_all(&vec![b'x'; *len]).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn discarded_parts_count_against_the_inflate_budget() {
        // Kept bytes cap at 100: b, c and d are inflated past what's left and discarded. That
        // work is charged, so after d the budget (130) is spent and e is not read, although it
        // would have fit.
        let z = zip_of(&[("a", 80), ("b", 80), ("c", 80), ("d", 80), ("e", 5)]);
        let p = Package::open_within(&z, 1000, 100, 130).unwrap();
        assert!(p.get("a").is_some());
        assert!(p.get("b").is_none() && p.get("d").is_none());
        assert!(p.get("e").is_none(), "the inflate budget stops reading");
        // With room to spare the same small part is kept.
        let p = Package::open_within(&z, 1000, 100, 10_000).unwrap();
        assert!(p.get("e").is_some());
    }

    #[test]
    fn backslash_zip_entry_names_are_found() {
        let z = zip_of(&[("PPT\\Slides\\Slide1.xml", 3)]);
        let p = Package::open(&z).unwrap();
        assert!(p.get("ppt/slides/slide1.xml").is_some());
        assert_eq!(p.original_name("ppt/slides/slide1.xml"), Some("PPT/Slides/Slide1.xml"));
    }

    #[test]
    fn xml_bytes_applies_the_xml_limit() {
        let z = zip_of(&[("small.xml", 10)]);
        let p = Package::open(&z).unwrap();
        assert_eq!(p.xml_bytes("small.xml").map(<[u8]>::len), Some(10));
        assert!(p.xml_bytes("missing.xml").is_none());
    }

    #[test]
    fn resolve_backslash_target() {
        assert_eq!(resolve("ppt/slides", "..\\media\\image1.png"), "ppt/media/image1.png");
        assert_eq!(resolve("ppt/slides", "\\ppt\\media\\a.png"), "ppt/media/a.png");
    }

    #[test]
    fn path_resolution() {
        assert_eq!(resolve("ppt/slides", "../media/image1.png"), "ppt/media/image1.png");
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt/slides", "/ppt/media/a%20b.png"), "ppt/media/a b.png");
        assert_eq!(resolve("", "../../../x.xml"), "x.xml");
        assert_eq!(rels_path("ppt/presentation.xml"), "ppt/_rels/presentation.xml.rels");
        assert_eq!(rels_path("[Content_Types].xml"), "_rels/[Content_Types].xml.rels");
        assert_eq!(relative("ppt/slides/slide1.xml", "ppt/media/image1.png"), "../media/image1.png");
        assert_eq!(relative("ppt/presentation.xml", "ppt/slides/slide1.xml"), "slides/slide1.xml");
        assert_eq!(relative("ppt/slides/slide1.xml", "ppt/slides/slide2.xml"), "slide2.xml");
    }

    #[test]
    fn sidecar_parts_stay_inside_chart_folders() {
        assert!(sidecar_part("ppt/embeddings/Microsoft_Excel_Worksheet1.xlsx"));
        assert!(sidecar_part("ppt/charts/style1.xml"));
        assert!(sidecar_part("ppt/drawings/chartDrawing1.xml"));
        assert!(sidecar_part("ppt/media/image1.png"));
        assert!(!sidecar_part("ppt/charts/chart1.xml"));
        assert!(!sidecar_part("ppt/charts/_rels/chart1.xml.rels"));
        assert!(!sidecar_part("ppt/slides/slide1.xml"));
        assert!(!sidecar_part("ppt/embeddings/../../ppt/slides/slide1.xml"));
        assert!(!sidecar_part("/etc/passwd"));
    }
}
