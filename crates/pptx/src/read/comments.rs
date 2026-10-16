//! Namespace-aware actual comment parts, complete visible import, and bound restoration.
use super::*;
use crate::comments_codec::{self as codec, BodyCodec, DecodedBody, EncodedBody, WireMap};
use deckcraft_model::{
    ShapeId,
    comments::{self, CommentExtension, CommentProjection},
};
use std::collections::BTreeMap;
fn error(s: impl std::fmt::Display) -> PptxError {
    PptxError::Unsupported(format!("comments: {s}"))
}
fn shell<'a>(pkg: &'a Package) -> Imp<'a> {
    Imp {
        pkg,
        media: Vec::new(),
        media_by_part: HashMap::new(),
        next: 1,
        slide_by_part: HashMap::new(),
        custom_show_names: HashMap::new(),
        table_style_ids: Vec::new(),
        vml_previews: HashMap::new(),
        error: None,
        table_pending: Vec::new(),
        table_bytes: 0,
        table_hashes: HashMap::new(),
    }
}
pub(crate) fn effective_body(xml: &str, part: &str, rels: &opc::RelsOut) -> Result<deckcraft_model::text::TextBody, PptxError> {
    let pkg = Package::comment_rels(part, rels.to_xml());
    let mut imp = shell(&pkg);
    let doc=crate::xml::parse(format!("<x xmlns:p188=\"{}\" xmlns:a=\"{}\" xmlns:r=\"{}\" xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\">{xml}</x>",crate::comments_dom::M,crate::comments_dom::A,crate::comments_dom::R).as_bytes())?;
    let part = Part::new(&pkg, part, &doc);
    let tx = doc.root.child("txBody").ok_or_else(|| error("encoded body missing"))?;
    Ok(dml::text_body(&mut imp, &part, tx))
}
struct Rich<'a, 'b> {
    imp: &'a mut Imp<'b>,
    part: &'a Part,
}
impl BodyCodec for Rich<'_, '_> {
    fn encode(&mut self, _: &deckcraft_model::text::TextBody) -> Result<EncodedBody, PptxError> {
        Err(error("reader cannot encode"))
    }
    fn decode(&mut self, body: &El) -> Result<DecodedBody, PptxError> {
        let count = body.children_named("p").count();
        if count > 10_000 {
            return Err(error("rich body paragraph limit exceeded"));
        }
        fn preflight(el: &El, charge: &mut usize) -> Result<(), PptxError> {
            let add = if matches!(el.local(), "r" | "fld") {
                let pieces = el.child("t").map(|t| t.text().bytes().filter(|b| matches!(b, b'\n' | 11)).count().saturating_add(1)).unwrap_or(1);
                let props = el.child("rPr").map(|p| p.to_xml().len()).unwrap_or(0);
                pieces
                    .checked_mul(std::mem::size_of::<deckcraft_model::text::Run>().saturating_add(props.saturating_mul(8)))
                    .ok_or_else(|| error("rich body expansion overflow"))?
            } else if el.is("br") {
                std::mem::size_of::<deckcraft_model::text::Run>()
            } else {
                0
            };
            *charge = charge.checked_add(add).ok_or_else(|| error("rich body expansion overflow"))?;
            if *charge > 16 * 1024 * 1024 {
                return Err(error("rich body expanded allocation exceeds limit"));
            }
            for child in el.elements() {
                preflight(child, charge)?;
            }
            Ok(())
        }
        preflight(body, &mut 0)?;
        let structural_unmapped = rich_source_unmapped(body)?;
        let decoded = dml::text_body(self.imp, self.part, body);
        let unmapped = structural_unmapped || rich_references_unmapped(body, self.imp, self.part)?;
        Ok(DecodedBody { body: decoded, unmapped })
    }
    fn projection(&self, body: &crate::comments_dom::Element) -> Result<Option<String>, PptxError> {
        crate::comments_dom::body_projection(body, &self.part.rels).map(Some)
    }
}
pub(super) struct State {
    legacy: Option<codec::Authors>,
    modern: Option<codec::Authors>,
    budget: crate::comments_dom::Budget,
    pub projection: CommentProjection,
    maps: BTreeMap<String, WireMap>,
    owners: BTreeMap<String, usize>,
    parts: BTreeMap<String, String>,
    warnings: Vec<&'static str>,
}
impl State {
    pub fn new(imp: &mut Imp, pres: &Part) -> Result<Self, PptxError> {
        let mut state = Self {
            legacy: None,
            modern: None,
            budget: Default::default(),
            projection: CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: Vec::new() },
            maps: BTreeMap::new(),
            owners: BTreeMap::new(),
            parts: BTreeMap::new(),
            warnings: Vec::new(),
        };
        let mut ids = codec::IdPool::reserve(&mut imp.next, 20_000)?;
        for (kind, modern) in [("commentAuthors", false), ("authors", true)] {
            let rels = pres.rels.all_of(kind).collect::<Vec<_>>();
            if rels.len() > 1 {
                return Err(error("ambiguous author part relationship"));
            }
            if let Some(rel) = rels.first() {
                if rel.external {
                    return Err(error("external author relationship"));
                }
                let Some(bytes) = imp.pkg.xml_bytes(&rel.target) else {
                    state.warnings.push(
                        "Referenced comment author part is missing or oversized; available standard fallback and original package are retained",
                    );
                    continue;
                };
                let authors = codec::read_authors(bytes, &rel.target, &mut ids, &mut state.budget)?;
                if authors.unmapped {
                    state.warnings.push("Comment author extensions are retained in the original package; current typed author metadata represents only the public core identity");
                }
                if authors.modern != modern {
                    return Err(error("author content lane differs from relationship"));
                }
                if modern {
                    state.modern = Some(authors);
                } else {
                    state.legacy = Some(authors);
                }
            }
        }
        Ok(state)
    }
    pub fn page(
        &mut self,
        imp: &mut Imp,
        p: &Presentation,
        slide: &mut Slide,
        part: &Part,
        file_slide_id: u32,
        index: usize,
        file_shapes: &HashMap<u32, ShapeId>,
    ) -> Result<(), PptxError> {
        let links = part.rels.all_of("comments").collect::<Vec<_>>();
        if links.is_empty() {
            return Ok(());
        }
        if links.len() > 2 {
            return Err(error("ambiguous comment part relationships"));
        }
        let owner = format!("slide:{index}");
        let native_to_wire = file_shapes.iter().map(|(wire, native)| (*native, *wire)).collect();
        let map = crate::write::comments::wire_map(p, slide, &owner, file_slide_id, &native_to_wire)?;
        let mut selected = None;
        let mut lanes = std::collections::BTreeSet::new();
        for rel in links {
            if rel.external {
                return Err(error("external comment relationship"));
            }
            let Some(bytes) = imp.pkg.xml_bytes(&rel.target) else {
                self.warnings.push("Referenced comment part is missing or oversized; available standard fallback and original package are retained");
                continue;
            };
            let doc = crate::xml::parse(bytes)?;
            let modern = crate::comments_dom::parse(bytes, &mut crate::comments_dom::Budget::default())?.is(crate::comments_dom::M, "cmLst");
            let authors = if modern { self.modern.as_ref() } else { self.legacy.as_ref() }.ok_or_else(|| error("comment author part missing"))?;
            let mut count = 0usize;
            fn census(el: &El, count: &mut usize) -> Result<(), PptxError> {
                if matches!(el.local(), "cm" | "reply") {
                    *count = count.checked_add(1).ok_or_else(|| error("message overflow"))?;
                }
                for child in el.elements() {
                    census(child, count)?;
                }
                Ok(())
            }
            census(&doc.root, &mut count)?;
            let mut ids = codec::IdPool::reserve(&mut imp.next, count)?;
            let source_part = Part::new(imp.pkg, &rel.target, &doc);
            let read = codec::read_page(bytes, authors, &map, &mut Rich { imp, part: &source_part }, &mut ids, &mut self.budget)?;
            if !lanes.insert(read.modern) {
                return Err(error("duplicate comment lane"));
            }
            if read.retain_source {
                self.warnings.extend(read.issues);
            }
            self.parts.insert(read.projection.key.clone(), rel.target.clone());
            self.projection.owners.push(read.projection);
            if selected.is_none() || read.modern {
                selected = Some((read.modern, read.comments));
            }
        }
        if let Some((_, items)) = selected {
            slide.comments = items;
        }
        self.owners.insert(owner.clone(), index);
        self.maps.insert(owner, map);
        Ok(())
    }
    pub fn finish(self, imp: &mut Imp, p: &mut Presentation, pres: &Part, original: &[u8]) -> Result<(), PptxError> {
        let extension_rel = pres.rels.items.iter().filter(|rel| rel.ty == crate::comments_extension::REL).collect::<Vec<_>>();
        if extension_rel.len() > 1 {
            return Err(error("duplicate comment sidecar relationship"));
        }
        let mut warnings = self.warnings;
        if let Some(rel) = extension_rel.first() {
            let restored = (|| -> Result<(), PptxError> {
                if !warnings.is_empty() {
                    return Err(error("incomplete source projection cannot restore metadata"));
                }
                if rel.external || imp.pkg.content_type(&rel.target).as_deref() != Some(crate::comments_extension::MIME) {
                    return Err(error("invalid comment sidecar package declaration"));
                }
                let bytes = imp.pkg.get(&rel.target).ok_or_else(|| error("comment sidecar missing"))?;
                let extension = CommentExtension::decode(bytes).map_err(error)?;
                let mut candidate = p.clone();
                let mut media_map = BTreeMap::new();
                for binding in &extension.media {
                    let bytes = imp.pkg.get(&binding.part).ok_or_else(|| error("declared embedded resource missing"))?;
                    if bytes.len() as u64 != binding.size || imp.pkg.content_type(&binding.part).as_deref() != Some(&binding.content_type) {
                        return Err(error("embedded resource declaration differs"));
                    }
                    let id = imp.media_for(&binding.part).ok_or_else(|| error("embedded resource cannot load"))?;
                    media_map.insert(binding.id, id);
                }
                candidate.media = imp.media.clone();
                let mut maps = self.maps.clone();
                let mut shape_maps = BTreeMap::new();
                for owner in &extension.owners {
                    let source = self.maps.get(&owner.key).ok_or_else(|| error("missing actual comment owner"))?;
                    let aliases = extension.shape_bindings.get(&owner.key).ok_or_else(|| error("missing shape aliases"))?;
                    let mut expected = source.clone();
                    expected.shapes.clear();
                    let mut remap = BTreeMap::new();
                    for (native, wire) in aliases {
                        let wire = wire.parse::<u32>().map_err(error)?;
                        let mut matches = source.shapes.iter().filter(|(_, s)| s.id == wire);
                        let (actual, shape) = matches.next().ok_or_else(|| error("shape alias has no actual owner"))?;
                        if matches.next().is_some() {
                            return Err(error("shape alias is ambiguous"));
                        }
                        expected.shapes.insert(*native, shape.clone());
                        remap.insert(*native, *actual);
                    }
                    maps.insert(owner.key.clone(), expected);
                    shape_maps.insert(owner.key.clone(), remap);
                }
                let expected = crate::write::comments::expected(&extension, &maps, &self.parts, &candidate, &media_map)?;
                let mut actual = self.projection.clone();
                actual.owners.sort_by(|a, b| a.key.cmp(&b.key));
                let mut expected = expected;
                expected.owners.sort_by(|a, b| a.key.cmp(&b.key));
                let mut extension = extension;
                extension.projection.owners.sort_by(|a, b| a.key.cmp(&b.key));
                extension.restore(&actual, &mut candidate, &self.owners, &media_map, &shape_maps, |_, _| Ok(expected)).map_err(error)?;
                let mut slide_map = BTreeMap::new();
                for (native, part) in &extension.slide_bindings {
                    let actual = imp.slide_by_part.get(&opc::norm(part)).ok_or_else(|| error("slide alias has no actual owner"))?;
                    slide_map.insert(*native, *actual);
                }
                for slide in &mut candidate.slides {
                    comments::resources::map_slides(&mut Arc::make_mut(slide).comments, &slide_map).map_err(error)?;
                }
                comments::validate(&candidate, true).map_err(error)?;
                *p = candidate;
                Ok(())
            })();
            if let Err(e) = restored {
                log::warn!("{e}");
                warnings.push("Comment extension did not match the independently parsed standard parts; standard comments remain authoritative and original package is retained");
            }
        }
        if !warnings.is_empty() {
            for warning in warnings {
                if !p.tags.iter().any(|(key, value)| key == "pptx:comment-warning" && value == warning) {
                    p.tags.push(("pptx:comment-warning".into(), warning.into()));
                }
            }
            let id = MediaId(imp.next.checked_add(1).ok_or_else(|| error("identity space exhausted"))?);
            imp.next = id.0;
            imp.media.push(MediaItem {
                id,
                name: "source.pptx".into(),
                content_type: "application/vnd.deckcraft.pptx-source".into(),
                data: Arc::new(original.to_vec()),
                link: None,
            });
        }
        p.media = imp.media.clone();
        Ok(())
    }
}

// Audit the actual source rather than comparing reserialized XML. This profile follows the
// production read::dml mappings. Being in a known namespace does not establish support.
// `body` comes from comments_dom::Element::legacy, which gives admitted namespaces stable
// prefixes and rejects duplicate expanded attributes. Namespace declarations themselves
// carry no additional typography; the separate expanded-name guard verifies all nodes.
fn rich_source_unmapped(body: &El) -> Result<bool, PptxError> {
    fn fill(name: &str) -> bool {
        matches!(name, "a:noFill" | "a:solidFill" | "a:gradFill" | "a:blipFill" | "a:pattFill" | "a:grpFill")
    }
    fn color(name: &str) -> bool {
        matches!(name, "a:srgbClr" | "a:schemeClr" | "a:sysClr" | "a:prstClr" | "a:scrgbClr" | "a:hslClr")
    }
    fn paragraph_props(name: &str) -> bool {
        name == "a:pPr"
            || matches!(
                name,
                "a:lvl1pPr" | "a:lvl2pPr" | "a:lvl3pPr" | "a:lvl4pPr" | "a:lvl5pPr" | "a:lvl6pPr" | "a:lvl7pPr" | "a:lvl8pPr" | "a:lvl9pPr"
            )
    }
    fn run_props(name: &str) -> bool {
        matches!(name, "a:rPr" | "a:defRPr" | "a:endParaRPr")
    }
    // Each supported child maps to a typed slot. Mutually exclusive alternatives share a
    // slot, so the first/last-child behavior of dml cannot silently discard a second value.
    fn child(parent: &str, name: &str) -> Option<(&'static str, usize)> {
        Some(match parent {
            "p188:txBody" => match name {
                "a:bodyPr" => ("body", 1),
                "a:lstStyle" => ("list", 1),
                "a:p" => ("paragraph", 100_000),
                _ => return None,
            },
            "a:bodyPr" => match name {
                "a:noAutofit" | "a:spAutoFit" | "a:normAutofit" => ("autofit", 1),
                "a:prstTxWarp" => ("warp", 1),
                _ => return None,
            },
            "a:lstStyle" if paragraph_props(name) && name != "a:pPr" => {
                return Some((
                    match name {
                        "a:lvl1pPr" => "level1",
                        "a:lvl2pPr" => "level2",
                        "a:lvl3pPr" => "level3",
                        "a:lvl4pPr" => "level4",
                        "a:lvl5pPr" => "level5",
                        "a:lvl6pPr" => "level6",
                        "a:lvl7pPr" => "level7",
                        "a:lvl8pPr" => "level8",
                        _ => "level9",
                    },
                    1,
                ));
            }
            "a:p" => match name {
                "a:pPr" => ("props", 1),
                "a:endParaRPr" => ("end", 1),
                "a:r" | "a:br" | "a:fld" | "a14:m" => ("run", 100_000),
                _ => return None,
            },
            "a:r" | "a:fld" => match name {
                "a:rPr" => ("props", 1),
                "a:t" => ("text", 1),
                _ => return None,
            },
            "a:br" => match name {
                "a:rPr" => ("props", 1),
                _ => return None,
            },
            parent if paragraph_props(parent) => match name {
                "a:lnSpc" => ("line", 1),
                "a:spcBef" => ("before", 1),
                "a:spcAft" => ("after", 1),
                "a:buNone" | "a:buChar" | "a:buAutoNum" | "a:buBlip" => ("bullet", 1),
                "a:buFont" => ("bulletfont", 1),
                "a:buClr" => ("bulletcolor", 1),
                "a:buSzPct" => ("bulletsize", 1),
                "a:tabLst" => ("tabs", 1),
                "a:defRPr" => ("default", 1),
                _ => return None,
            },
            parent if run_props(parent) => match name {
                "a:ln" => ("line", 1),
                name if fill(name) => ("fill", 1),
                "a:effectLst" => ("effects", 1),
                "a:highlight" => ("highlight", 1),
                "a:uFill" => ("underline", 1),
                "a:latin" => ("latin", 1),
                "a:ea" => ("ea", 1),
                "a:cs" => ("cs", 1),
                "a:sym" => ("symbol", 1),
                "a:hlinkClick" => ("click", 1),
                "a:hlinkMouseOver" => ("hover", 1),
                _ => return None,
            },
            "a:prstTxWarp" => match name {
                "a:avLst" => ("guides", 1),
                _ => return None,
            },
            "a:avLst" => match name {
                "a:gd" => ("guide", 16),
                _ => return None,
            },
            "a:lnSpc" | "a:spcBef" | "a:spcAft" => match name {
                "a:spcPct" | "a:spcPts" => ("spacing", 1),
                _ => return None,
            },
            "a:tabLst" => match name {
                "a:tab" => ("tab", 64),
                _ => return None,
            },
            "a:buBlip" => match name {
                "a:blip" => ("blip", 1),
                _ => return None,
            },
            "a:solidFill" | "a:fgClr" | "a:bgClr" | "a:gs" | "a:buClr" | "a:highlight" | "a:clrFrom" | "a:clrTo" | "a:outerShdw" | "a:innerShdw"
            | "a:prstShdw" | "a:glow"
                if color(name) =>
            {
                ("color", 1)
            }
            "a:duotone" if color(name) => ("color", 2),
            parent if color(parent) => {
                let local = name.strip_prefix("a:")?;
                deckcraft_color::ColorTransform::from_xml(local, Some(0))?;
                ("transform", 32)
            }
            "a:gradFill" => match name {
                "a:gsLst" => ("stops", 1),
                "a:lin" | "a:path" => ("shape", 1),
                "a:tileRect" => ("tile", 1),
                _ => return None,
            },
            "a:gsLst" => match name {
                "a:gs" => ("stop", 256),
                _ => return None,
            },
            "a:path" => match name {
                "a:fillToRect" => ("focus", 1),
                _ => return None,
            },
            "a:pattFill" => match name {
                "a:fgClr" => ("fg", 1),
                "a:bgClr" => ("bg", 1),
                _ => return None,
            },
            "a:blipFill" => match name {
                "a:blip" => ("blip", 1),
                "a:srcRect" => ("crop", 1),
                "a:stretch" | "a:tile" => ("mode", 1),
                _ => return None,
            },
            "a:blip" => match name {
                "a:alphaModFix" => ("alpha", 1),
                "a:lum" => ("lum", 1),
                "a:grayscl" => ("gray", 1),
                "a:biLevel" => ("bilevel", 1),
                "a:alphaInv" => ("invert", 1),
                "a:duotone" => ("duotone", 1),
                "a:clrChange" => ("clear", 1),
                "a:extLst" => ("artistic", 1),
                _ => return None,
            },
            "a:stretch" => match name {
                "a:fillRect" => ("rect", 1),
                _ => return None,
            },
            "a:clrChange" => match name {
                "a:clrFrom" => ("from", 1),
                "a:clrTo" => ("to", 1),
                _ => return None,
            },
            "a:extLst" => match name {
                "a:ext" => ("extension", 1),
                _ => return None,
            },
            "a:ext" => match name {
                "a14:imgProps" => ("image", 1),
                _ => return None,
            },
            "a14:imgProps" => match name {
                "a14:imgLayer" => ("layer", 1),
                _ => return None,
            },
            "a14:imgLayer" => match name {
                "a14:imgEffect" => ("effect", 1),
                _ => return None,
            },
            "a14:imgEffect" if name.strip_prefix("a14:artistic").is_some_and(|suffix| suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())) => {
                ("artistic", 1)
            }
            "a:ln" => match name {
                name if fill(name) => ("fill", 1),
                "a:prstDash" | "a:custDash" => ("dash", 1),
                "a:round" | "a:bevel" | "a:miter" => ("join", 1),
                "a:headEnd" => ("head", 1),
                "a:tailEnd" => ("tail", 1),
                _ => return None,
            },
            "a:custDash" => match name {
                "a:ds" => ("dash", 64),
                _ => return None,
            },
            "a:effectLst" => match name {
                "a:outerShdw" | "a:prstShdw" => ("outer", 1),
                "a:innerShdw" => ("inner", 1),
                "a:glow" => ("glow", 1),
                "a:reflection" => ("reflection", 1),
                "a:softEdge" => ("soft", 1),
                _ => return None,
            },
            "a:uFill" => match name {
                "a:solidFill" => ("fill", 1),
                _ => return None,
            },
            "a:hlinkClick" | "a:hlinkMouseOver" => match name {
                "a:snd" => ("sound", 1),
                _ => return None,
            },
            _ => return None,
        })
    }
    fn attributes(name: &str) -> &'static [&'static str] {
        match name {
            "a:bodyPr" => &[
                "lIns",
                "tIns",
                "rIns",
                "bIns",
                "anchor",
                "anchorCtr",
                "wrap",
                "vert",
                "rot",
                "numCol",
                "spcCol",
                "rtlCol",
                "upright",
                "spcFirstLastPara",
            ],
            name if paragraph_props(name) => {
                &["algn", "marL", "marR", "indent", "rtl", "fontAlgn", "eaLnBrk", "latinLnBrk", "hangingPunct", "defTabSz", "lvl"]
            }
            name if run_props(name) => &["lang", "sz", "b", "i", "u", "strike", "baseline", "cap", "spc", "kern", "noProof"],
            // fld@id is an object alias, regenerated by write::dml::paragraph; it does not
            // establish message/author ownership or change the stored field type/text.
            "a:fld" => &["id", "type"],
            "a:t" => &["xml:space"],
            "a:normAutofit" => &["fontScale", "lnSpcReduction"],
            "a:prstTxWarp" => &["prst"],
            "a:gd" => &["name", "fmla"],
            "a:spcPct" | "a:spcPts" | "a:buSzPct" | "a:prstDash" | "a:srgbClr" | "a:schemeClr" | "a:prstClr" => &["val"],
            "a:sysClr" => &["val", "lastClr"],
            "a:scrgbClr" => &["r", "g", "b"],
            "a:hslClr" => &["hue", "sat", "lum"],
            "a:buChar" => &["char"],
            "a:buAutoNum" => &["type", "startAt"],
            "a:buFont" | "a:latin" | "a:ea" | "a:cs" | "a:sym" => &["typeface"],
            "a:tab" => &["pos", "algn"],
            "a:gradFill" | "a:blipFill" => &["rotWithShape"],
            "a:gs" => &["pos"],
            "a:lin" => &["ang", "scaled"],
            "a:path" => &["path"],
            "a:tileRect" | "a:fillToRect" | "a:srcRect" | "a:fillRect" => &["l", "t", "r", "b"],
            "a:pattFill" => &["prst"],
            "a:blip" | "a14:imgLayer" => &["r:embed", "r:link"],
            "a:alphaModFix" => &["amt"],
            "a:lum" => &["bright", "contrast"],
            "a:biLevel" => &["thresh"],
            "a:tile" => &["tx", "ty", "sx", "sy", "flip", "algn"],
            "a:ext" => &["uri"],
            "a:ln" => &["w", "cap", "cmpd"],
            "a:ds" => &["d", "sp"],
            "a:headEnd" | "a:tailEnd" => &["type", "w", "len"],
            "a:outerShdw" | "a:innerShdw" | "a:prstShdw" => &["blurRad", "dist", "dir", "sx", "sy", "kx", "ky", "algn", "rotWithShape"],
            "a:glow" | "a:softEdge" => &["rad"],
            "a:reflection" => &["blurRad", "stA", "endA", "endPos", "dist"],
            "a:hlinkClick" | "a:hlinkMouseOver" => &["r:id", "action", "tooltip", "highlightClick", "endSnd"],
            // The exact optional source label is represented by Hyperlink::sound_name.
            "a:snd" => &["r:embed", "r:link", "name"],
            name if name.strip_prefix("a:").and_then(|local| deckcraft_color::ColorTransform::from_xml(local, Some(0))).is_some() => {
                if matches!(name, "a:comp" | "a:inv" | "a:gray") { &[] } else { &["val"] }
            }
            _ => &[],
        }
    }
    fn exception(el: &El, name: &str, value: &str) -> bool {
        // The native Reflection type holds the remaining five properties. These four
        // fixed values are emitted by write::dml::effects and define that exact profile;
        // different source values are unrepresented and must retain their source.
        el.name == "a:reflection"
            && matches!((name, value), ("dir", "5400000") | ("sy", "-100000") | ("algn", "bl") | ("rotWithShape", "0" | "false"))
    }
    fn special(el: &El, parent: &str) -> bool {
        if el.attr("r:embed").is_some_and(|value| !value.is_empty()) && el.attr("r:link").is_some_and(|value| !value.is_empty()) {
            return true;
        }
        if el.name == "a:blip" && parent == "a:buBlip" && el.elements().next().is_some() {
            return true;
        }
        if el.name == "a:buChar" && el.attr("char").is_some_and(|text| text.chars().count() > 4) {
            return true;
        }
        if el.name == "a:avLst" {
            let mut seen = std::collections::BTreeSet::new();
            for guide in el.elements() {
                let name = guide.attr("name").unwrap_or("");
                let index = if name == "adj" { Some(1) } else { name.strip_prefix("adj").and_then(|value| value.parse::<usize>().ok()) };
                if index.is_some_and(|index| !seen.insert(index)) {
                    return true;
                }
            }
        }
        if el.name == "a:gd" {
            let name = el.attr("name").unwrap_or("");
            if !(name == "adj"
                || name.strip_prefix("adj").and_then(|value| value.parse::<usize>().ok()).is_some_and(|index| (1..=8).contains(&index)))
            {
                return true;
            }
            if !el
                .attr("fmla")
                .and_then(|value| value.trim().strip_prefix("val"))
                .and_then(|value| value.trim().parse::<f64>().ok())
                .is_some_and(f64::is_finite)
            {
                return true;
            }
        }
        if el.name == "a:ext" && el.attr("uri") != Some("{BEBA8EAE-BF5A-486C-A8C5-ECC9F3942E4B}") {
            return true;
        }
        if el.name == "a:duotone" && el.elements().count() != 2 {
            return true;
        }
        if el.name == "a:clrChange" {
            // blip_fill represents only a chromakey RGB. Its writer emits the same RGB
            // with zero alpha as clrTo; any other replacement color/effect would be lost.
            let from = el.child("clrFrom").and_then(dml::color);
            let to = el.child("clrTo").and_then(dml::color);
            match (from, to) {
                (Some(from), Some(to))
                    if matches!(from.base, deckcraft_model::style::ColorBase::Rgb { .. })
                        && from.mods.is_empty()
                        && to.base == from.base
                        && to.mods == [deckcraft_color::ColorTransform::Alpha(0)] => {}
                _ => return true,
            }
        }
        false
    }
    let mut stack = vec![(body, "", 0usize)];
    let mut work = 0usize;
    while let Some((el, parent, depth)) = stack.pop() {
        work = work.checked_add(1).ok_or_else(|| error("rich source audit work overflow"))?;
        if work > 1_000_000 || depth > 64 {
            return Err(error("rich source audit exceeds limit"));
        }
        if special(el, parent) {
            return Ok(true);
        }
        let admitted = attributes(&el.name);
        for (name, value) in &el.attrs {
            if name == "xmlns" || name.starts_with("xmlns:") {
                continue;
            }
            if !admitted.contains(&name.as_str()) && !exception(el, name, value) {
                return Ok(true);
            }
            if name == "xml:space" && !matches!(value.as_str(), "preserve" | "default") {
                return Ok(true);
            }
        }
        // dml::paragraph stores the complete OMML children in RunKind::Math. Do not
        // audit the preserved payload as if it had been flattened into normal runs.
        // The independent namespace guard still audits this subtree before this call.
        if el.name == "a14:m" {
            if el.children.iter().any(|node| matches!(node, crate::xml::Node::Text(text) if !text.trim().is_empty())) {
                return Ok(true);
            }
            continue;
        }
        let mut slots = std::collections::BTreeMap::<&str, usize>::new();
        for node in &el.children {
            match node {
                crate::xml::Node::Text(text) => {
                    if el.name != "a:t" && !text.trim().is_empty() {
                        return Ok(true);
                    }
                }
                crate::xml::Node::El(next) => {
                    let Some((slot, maximum)) = child(&el.name, &next.name) else { return Ok(true) };
                    let count = slots.entry(slot).or_default();
                    *count = count.checked_add(1).ok_or_else(|| error("rich source audit child overflow"))?;
                    if *count > maximum {
                        return Ok(true);
                    }
                    stack.try_reserve(1).map_err(|_| error("cannot allocate rich source audit"))?;
                    stack.push((next, el.name.as_str(), depth + 1));
                }
            }
        }
    }
    Ok(false)
}

// Structural support alone is insufficient: an admitted hyperlink/action may have failed
// the actual decoder, or a picture/sound relationship may name no usable embedded bytes.
// This runs after text_body, so parser-success checks reuse already imported media aliases.
fn rich_references_unmapped(body: &El, imp: &mut Imp<'_>, part: &Part) -> Result<bool, PptxError> {
    let mut pending = vec![(body, 0usize, false)];
    let mut work = 0usize;
    while let Some((el, depth, raw_math)) = pending.pop() {
        work = work.checked_add(1).ok_or_else(|| error("rich reference audit work overflow"))?;
        if work > 1_000_000 || depth > 64 {
            return Err(error("rich reference audit exceeds limit"));
        }
        let raw_math = raw_math || el.name == "a14:m";
        if raw_math && el.attrs.iter().any(|(name, _)| name.starts_with("r:")) {
            // Raw OMML text is preserved, but no typed media/slide walker binds its rel IDs.
            return Ok(true);
        }
        if matches!(el.name.as_str(), "a:blip" | "a14:imgLayer" | "a:snd") {
            let rid = el.attr("r:embed").or_else(|| el.attr("r:link")).filter(|id| !id.is_empty());
            let Some(rid) = rid else { return Ok(true) };
            let mut rels = part.rels.items.iter().filter(|rel| rel.id == rid);
            let Some(rel) = rels.next() else { return Ok(true) };
            let kind = if el.name == "a:snd" { "audio" } else { "image" };
            if rels.next().is_some()
                || rel.kind != kind
                || rel.external
                || rel.target.is_empty()
                || imp.pkg.get(&rel.target).is_none_or(|bytes| bytes.is_empty() || bytes.len() > comments::MAX_ATTACHMENT_BYTES)
            {
                return Ok(true);
            }
            if dml::media_ref(imp, part, el).is_none() {
                return Ok(true);
            }
        }
        if matches!(el.name.as_str(), "a:hlinkClick" | "a:hlinkMouseOver") {
            let action = el.attr("action").unwrap_or("");
            // These exact simple actions are emitted/read directly. A custom-show query
            // has only the two explicitly decoded fields; additional keys are not stored.
            let simple = matches!(
                action,
                "" | "ppaction://hlinksldjump"
                    | "ppaction://program"
                    | "ppaction://media"
                    | "ppaction://hlinkfile"
                    | "ppaction://hlinkshowjump?jump=nextslide"
                    | "ppaction://hlinkshowjump?jump=previousslide"
                    | "ppaction://hlinkshowjump?jump=firstslide"
                    | "ppaction://hlinkshowjump?jump=lastslide"
                    | "ppaction://hlinkshowjump?jump=endshow"
                    | "ppaction://hlinkshowjump?jump=lastslideviewed"
            );
            let custom = action.strip_prefix("ppaction://customshow?").is_some_and(|query| {
                let mut id = false;
                let mut returns = false;
                query.split('&').all(|field| {
                    if let Some(value) = field.strip_prefix("id=") {
                        let fresh = !id;
                        id = true;
                        fresh && !value.is_empty()
                    } else if let Some(value) = field.strip_prefix("return=") {
                        let fresh = !returns;
                        returns = true;
                        fresh && matches!(value, "true" | "false" | "1" | "0")
                    } else {
                        false
                    }
                }) && id
            });
            if !simple && !custom {
                return Ok(true);
            }
            if let Some(rid) = el.attr("r:id").filter(|value| !value.is_empty()) {
                let mut rels = part.rels.items.iter().filter(|rel| rel.id == rid);
                let Some(rel) = rels.next() else { return Ok(true) };
                if rels.next().is_some() || !matches!(rel.kind.as_str(), "hyperlink" | "slide") || rel.target.is_empty() {
                    return Ok(true);
                }
            }
            let Some(link) = dml::hyperlink(imp, part, el) else { return Ok(true) };
            if el.child("snd").is_some() && link.sound.is_none() {
                return Ok(true);
            }
            let sound_name = el.child("snd").and_then(|sound| sound.attr("name"));
            if link.sound_name.as_deref() != sound_name {
                return Ok(true);
            }
        }
        for child in el.elements() {
            pending.try_reserve(1).map_err(|_| error("cannot allocate rich reference audit"))?;
            pending.push((child, depth + 1, raw_math));
        }
    }
    Ok(false)
}

#[cfg(test)]
mod rich_source_audit_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    fn audit(content: &str) -> bool {
        let source = format!(
            "<p188:txBody xmlns:p188=\"{}\" xmlns:a=\"{}\" xmlns:r=\"{}\" xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">{content}</p188:txBody>",
            crate::comments_dom::M,
            crate::comments_dom::A,
            crate::comments_dom::R,
        );
        let source = crate::comments_dom::parse(source.as_bytes(), &mut crate::comments_dom::Budget::default()).unwrap();
        rich_source_unmapped(&source.legacy()).unwrap()
    }
    #[test]
    fn mapped_attributes_and_generated_defaults_remain_clean() {
        assert!(!audit(
            r#"<a:bodyPr rot="60000" lIns="12700" numCol="2"><a:normAutofit fontScale="80000"/></a:bodyPr><a:lstStyle><a:lvl1pPr><a:buAutoNum type="arabicPeriod" startAt="4"/><a:defRPr lang="fr-FR"/></a:lvl1pPr></a:lstStyle><a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-GB" b="1" i="0" noProof="1"><a:effectLst><a:reflection blurRad="0" stA="100000" endA="0" endPos="100000" dist="0" dir="5400000" sy="-100000" algn="bl" rotWithShape="0"/></a:effectLst></a:rPr><a:t xml:space="preserve"> body </a:t></a:r><a:fld id="{01234567-89AB-CDEF-0123-000000000001}" type="slidenum"><a:rPr/><a:t>3</a:t></a:fld></a:p>"#
        ));
    }
    #[test]
    fn known_namespace_does_not_admit_unsupported_attributes_or_children() {
        for content in [
            r#"<a:p><a:r><a:rPr altLang="fr-FR"/><a:t>body</a:t></a:r></a:p>"#,
            r#"<a:p><a:pPr><a:buSzPts val="1200"/></a:pPr></a:p>"#,
            r#"<a:bodyPr compatLnSpc="1"/><a:p/>"#,
            r#"<a:p><a:r><a:rPr><a:latin typeface="Original Open Font" pitchFamily="34"/></a:rPr><a:t>body</a:t></a:r></a:p>"#,
            r#"<a:p><a:r><a:rPr><a:effectLst><a:reflection dir="0"/></a:effectLst></a:rPr><a:t>body</a:t></a:r></a:p>"#,
            r#"<a:p><a:fld id="original" type="slidenum"><a:pPr algn="ctr"/><a:t>3</a:t></a:fld></a:p>"#,
        ] {
            assert!(audit(content), "unsupported source accepted: {content}");
        }
    }
    #[test]
    fn duplicated_slots_and_production_truncation_require_source_retention() {
        for content in [
            r#"<a:p><a:r><a:rPr b="1"/><a:rPr b="0"/><a:t>body</a:t></a:r></a:p>"#.to_string(),
            r#"<a:p><a:pPr><a:buNone/><a:buChar char="*"/></a:pPr></a:p>"#.to_string(),
            r#"<a:p><a:pPr><a:buChar char="12345"/></a:pPr></a:p>"#.to_string(),
            format!("<a:p><a:pPr><a:tabLst>{}</a:tabLst></a:pPr></a:p>", "<a:tab pos=\"0\"/>".repeat(65)),
            r#"<a:bodyPr><a:prstTxWarp prst="textArchUp"><a:avLst><a:gd name="adj9" fmla="val 123"/></a:avLst></a:prstTxWarp></a:bodyPr><a:p/>"#
                .to_string(),
            r#"<a:bodyPr><a:prstTxWarp prst="textArchUp"><a:avLst><a:gd name="adj" fmla="*/ 1 2 3"/></a:avLst></a:prstTxWarp></a:bodyPr><a:p/>"#
                .to_string(),
        ] {
            assert!(audit(&content));
        }
    }
    #[test]
    fn complete_raw_math_is_not_misclassified_as_flattened_text() {
        assert!(!audit(r#"<a:p><a14:m><m:oMath><m:r><m:rPr><m:sty m:val="i"/></m:rPr><m:t>x</m:t></m:r></m:oMath></a14:m></a:p>"#));
        assert!(audit(r#"<a:p><a14:m>unrepresented wrapper text<m:oMath/></a14:m></a:p>"#));
    }
    #[test]
    fn artistic_effect_name_and_original_media_profile_is_bounded_and_exact() {
        assert!(!audit(
            r#"<a:p><a:r><a:rPr><a:blipFill rotWithShape="1"><a:blip r:embed="image"><a:extLst><a:ext uri="{BEBA8EAE-BF5A-486C-A8C5-ECC9F3942E4B}"><a14:imgProps><a14:imgLayer r:embed="original"><a14:imgEffect><a14:artisticBlur/></a14:imgEffect></a14:imgLayer></a14:imgProps></a:ext></a:extLst></a:blip><a:stretch><a:fillRect/></a:stretch></a:blipFill></a:rPr><a:t>body</a:t></a:r></a:p>"#
        ));
        assert!(audit(
            r#"<a:p><a:r><a:rPr><a:blipFill><a:blip r:embed="image"><a:extLst><a:ext uri="unknown"><a14:imgProps/></a:ext></a:extLst></a:blip></a:blipFill></a:rPr><a:t>body</a:t></a:r></a:p>"#
        ));
    }
}
