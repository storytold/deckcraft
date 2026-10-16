//! Actual ODF annotations, verified rich owner ranges and typed metadata restoration.
use super::*;
use crate::comments_codec::{self as codec, BodyCodec, DecodedBody, EncodedBody, ReadText, WireMap, WireText};
use deckcraft_model::comments::{self, CommentExtension, CommentProjection};
use std::collections::BTreeSet;
const HOLDER: &str = "DeckCraftCommentHolder";
pub(crate) fn holder<'a>(el: &'a El, styles: &Styles<'_>) -> Option<&'a El> {
    let annotation = codec::holder_annotation(el, HOLDER)?;
    let style = styles.get(&("graphic".into(), HOLDER.into()))?;
    if style.attrs.len() != 2
        || style.attr("style:family") != Some("graphic")
        || style.attr("style:name") != Some(HOLDER)
        || style.elements().count() != 1
    {
        return None;
    }
    let props = style.child("style:graphic-properties")?;
    if props.attrs.len() != 2 || props.attr("draw:fill") != Some("none") || props.attr("draw:stroke") != Some("none") || !props.children.is_empty() {
        return None;
    }
    let effective = properties(styles, "graphic", Some(HOLDER)).ok()?;
    if effective.len() != 2
        || effective.get("draw:fill").map(String::as_str) != Some("none")
        || effective.get("draw:stroke").map(String::as_str) != Some("none")
    {
        return None;
    }
    Some(annotation)
}
struct Rich<'a, 'b> {
    styles: &'a Styles<'b>,
}
impl BodyCodec for Rich<'_, '_> {
    fn encode(&mut self, _: &TextBody) -> Result<EncodedBody> {
        Err(OdpError("annotation reader cannot encode".into()))
    }
    fn decode(&mut self, el: &El) -> Result<DecodedBody> {
        super::comment_body(el, self.styles)
    }
}
pub(super) struct State {
    projection: CommentProjection,
    owners: BTreeMap<String, usize>,
    maps: BTreeMap<String, WireMap>,
    shapes: BTreeMap<String, BTreeMap<String, ShapeId>>,
    budget: codec::Budget,
    warnings: Vec<&'static str>,
}
impl State {
    pub fn new() -> Self {
        Self {
            projection: CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: Vec::new() },
            owners: BTreeMap::new(),
            maps: BTreeMap::new(),
            shapes: BTreeMap::new(),
            budget: Default::default(),
            warnings: Vec::new(),
        }
    }
    pub fn page(&mut self, page: &El, index: usize, slide: &mut Slide, p: &mut Presentation, styles: &Styles<'_>) -> Result<()> {
        fn source_index<'a>(el: &'a El, out: &mut BTreeMap<usize, &'a El>) {
            out.insert(el.source_start(), el);
            for child in el.elements() {
                if child.name != "office:annotation" {
                    source_index(child, out);
                }
            }
        }
        fn has_annotation(el: &El) -> bool {
            el.elements().any(|child| matches!(child.name.as_str(), "office:annotation" | "office:annotation-end") || has_annotation(child))
        }
        if !has_annotation(page) {
            return Ok(());
        }
        let mut elements = BTreeMap::new();
        source_index(page, &mut elements);
        let owner = format!("slide:{index}");
        let mut texts = Vec::new();
        let mut wiremap = WireMap { owner: owner.clone(), texts: BTreeMap::new(), holder_style: HOLDER.into() };
        let mut aliases = BTreeMap::new();
        let mut duplicate = false;
        fn source<'a>(shape: &Shape, elements: &BTreeMap<usize, &'a El>) -> Option<&'a El> {
            if !shape.tags.iter().any(|(k, v)| k == "odp:source-part" && v == "content.xml") {
                return None;
            }
            shape
                .tags
                .iter()
                .find(|(k, _)| k == "odp:source-range")
                .and_then(|(_, range)| range.strip_prefix("v1:"))
                .and_then(|v| v.split(':').next())
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|v| elements.get(&v).copied())
        }
        deckcraft_model::walk(&slide.shapes, &mut |shape, _| {
            let source = source(shape, &elements);
            let key = shape
                .tags
                .iter()
                .find(|(k, _)| k == "odp:id")
                .map(|(_, v)| v.clone())
                .or_else(|| source.map(|e| format!("@source:{}", e.source_start())));
            if let Some(key) = key {
                if aliases.insert(key.clone(), shape.id).is_some() {
                    duplicate = true;
                }
                if let (Some(body), Some(_)) = (&shape.text, source) {
                    wiremap.texts.insert(shape.id, WireText { key, body: body.clone() });
                }
            }
        });
        deckcraft_model::walk(&slide.shapes, &mut |shape, _| {
            if let (Some(source), Some(body), Some(wire)) = (source(shape, &elements), shape.text.as_ref(), wiremap.texts.get(&shape.id))
                && has_annotation(source)
            {
                texts.push(ReadText { element: source, shape: shape.id, wire: &wire.key, body });
            }
        });
        if duplicate {
            return Err(OdpError("comment source shape IDs are ambiguous".into()));
        }
        let floating = page.elements().filter_map(|el| holder(el, styles)).map(El::source_start).collect::<BTreeSet<_>>();
        let mut count = 0usize;
        fn census(el: &El, count: &mut usize) {
            for child in el.elements() {
                if child.name == "office:annotation" {
                    *count = count.saturating_add(1);
                }
                census(child, count);
            }
        }
        census(page, &mut count);
        let mut ids = codec::IdPool::reserve(&mut p.next_id, count.checked_mul(2).ok_or_else(|| OdpError("comment count overflow".into()))?)?;
        let read = codec::read_page(page, &owner, &texts, &floating, &mut Rich { styles }, &mut ids, &mut self.budget)?;
        self.warnings.extend(read.issues);
        slide.comments = read.comments;
        self.projection.owners.push(read.projection);
        self.owners.insert(owner.clone(), index);
        self.maps.insert(owner.clone(), wiremap);
        self.shapes.insert(owner, aliases);
        Ok(())
    }
    pub fn finish(self, p: &mut Presentation, zip: &mut zip::ZipArchive<Cursor<&[u8]>>, manifest: &El, total: &mut u64) -> Result<()> {
        let mut warnings = self.warnings;
        if zip.index_for_name(crate::comments_extension::PART).is_some() {
            let restored = (|| -> Result<()> {
                if !warnings.is_empty() {
                    return Err(OdpError("incomplete actual comment projection cannot restore extension".into()));
                }
                fn declaration(manifest: &El, path: &str, mime: &str) -> bool {
                    manifest.elements().filter(|el| el.name == "manifest:file-entry" && el.attr("manifest:full-path") == Some(path)).count() == 1
                        && manifest.elements().any(|el| {
                            el.name == "manifest:file-entry"
                                && el.attr("manifest:full-path") == Some(path)
                                && el.attr("manifest:media-type") == Some(mime)
                        })
                }
                if !declaration(manifest, crate::comments_extension::PART, crate::comments_extension::MIME) {
                    return Err(OdpError("comment extension manifest differs".into()));
                }
                let extension = CommentExtension::decode(&part(zip, crate::comments_extension::PART, total)?).map_err(|e| OdpError(e.to_string()))?;
                let mut candidate = p.clone();
                let mut media = BTreeMap::new();
                for binding in &extension.media {
                    if !declaration(manifest, &binding.part, &binding.content_type) {
                        return Err(OdpError("comment resource manifest differs".into()));
                    }
                    let bytes = part(zip, &binding.part, total)?;
                    if bytes.len() as u64 != binding.size {
                        return Err(OdpError("comment resource size differs".into()));
                    }
                    let id = MediaId(super::id(&mut candidate)?);
                    candidate.media.push(MediaItem {
                        id,
                        name: binding.part.rsplit('/').next().unwrap_or("attachment").into(),
                        content_type: binding.content_type.clone(),
                        data: Arc::new(bytes),
                        link: None,
                    });
                    media.insert(binding.id, id);
                }
                let mut maps = BTreeMap::new();
                let mut shapes = BTreeMap::new();
                for owner in &extension.owners {
                    let actual = self.maps.get(&owner.key).ok_or_else(|| OdpError("comment source owner missing".into()))?;
                    let aliases = extension.shape_bindings.get(&owner.key).ok_or_else(|| OdpError("comment shape aliases missing".into()))?;
                    let mut map = actual.clone();
                    map.texts.clear();
                    let mut remap = BTreeMap::new();
                    for (native, wire) in aliases {
                        let actual_id = *self
                            .shapes
                            .get(&owner.key)
                            .and_then(|aliases| aliases.get(wire))
                            .ok_or_else(|| OdpError("comment shape alias has no actual wire owner".into()))?;
                        remap.insert(*native, actual_id);
                        if let Some(text) = actual.texts.get(&actual_id) {
                            map.texts.insert(*native, text.clone());
                        }
                    }
                    maps.insert(owner.key.clone(), map);
                    shapes.insert(owner.key.clone(), remap);
                }
                let expected = crate::write::comments_writer::expected(&extension, &maps, &candidate, &self.owners)?;
                extension
                    .restore(&self.projection, &mut candidate, &self.owners, &media, &shapes, |_, _| Ok(expected))
                    .map_err(|e| OdpError(e.to_string()))?;
                let mut slide_map = BTreeMap::new();
                for (native, wire) in &extension.slide_bindings {
                    let mut matches = candidate.slides.iter().filter(|slide| slide.tags.iter().any(|(k, v)| k == "odp:id" && v == wire));
                    let actual = matches.next().ok_or_else(|| OdpError("comment slide link target missing".into()))?;
                    if matches.next().is_some() {
                        return Err(OdpError("comment slide link target ambiguous".into()));
                    }
                    slide_map.insert(*native, actual.id);
                }
                for slide in &mut candidate.slides {
                    comments::resources::map_slides(&mut Arc::make_mut(slide).comments, &slide_map).map_err(|e| OdpError(e.to_string()))?;
                }
                comments::validate(&candidate, true).map_err(|e| OdpError(e.to_string()))?;
                *p = candidate;
                Ok(())
            })();
            if restored.is_err() {
                warnings.push("ODF comment extension does not match independently parsed standard annotations; standard comments remain authoritative and the original package is retained");
            }
        }
        for warning in warnings {
            extras::warn(p, warning);
            if !p.tags.iter().any(|(key, value)| key == "odp:warning" && value == warning) {
                return Err(OdpError("comment diagnostic could not be retained".into()));
            }
        }
        Ok(())
    }
}
