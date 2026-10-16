//! Actual standard comment parts and projection-bound typed metadata.
use super::*;
use crate::comments_codec::{self as codec, BodyCodec, DecodedBody, EncodedBody, Lane, ShapeKind, WireMap, WireShape};
use deckcraft_model::comments::{self, CommentExtension, CommentProjection, ExtensionMedia, ExtensionOwner};
use std::collections::{BTreeMap, BTreeSet};
fn error(s: impl std::fmt::Display) -> PptxError {
    PptxError::Unsupported(format!("comments: {s}"))
}
pub(super) struct State {
    pub owners: Vec<ExtensionOwner>,
    authors: codec::Authors,
    projection: CommentProjection,
    bindings: Vec<(String, usize, Vec<String>)>,
    shapes: BTreeMap<String, BTreeMap<ShapeId, String>>,
    last: BTreeMap<u32, u32>,
    modern: bool,
}
impl State {
    pub fn new(p: &Presentation) -> Result<Self, PptxError> {
        let owners = p
            .slides
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.comments.is_empty())
            .map(|(i, s)| ExtensionOwner { key: format!("slide:{i}"), projection_owners: Vec::new(), comments: s.comments.clone() })
            .collect::<Vec<_>>();
        let authors = codec::Authors::native(&owners, &BTreeMap::new())?;
        Ok(Self {
            owners,
            authors,
            projection: CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: Vec::new() },
            bindings: Vec::new(),
            shapes: BTreeMap::new(),
            last: BTreeMap::new(),
            modern: false,
        })
    }
    pub fn page(&mut self, x: &mut Exp, slide: &Slide, index: usize, so: &mut Out) -> Result<Option<String>, PptxError> {
        if slide.comments.is_empty() {
            return Ok(None);
        }
        let owner = format!("slide:{index}");
        let ids = shapes::IdMap::build(&slide.shapes);
        let map =
            wire_map(x.p, slide, &owner, u32::try_from(index).map_err(error)?.checked_add(256).ok_or_else(|| error("slide ID overflow"))?, &ids.map)?;
        let mut referenced = BTreeSet::new();
        fn anchors(items: &[deckcraft_model::Comment], out: &mut BTreeSet<ShapeId>) {
            for c in items {
                if let Some(id) = c.shape {
                    out.insert(id);
                }
                anchors(&c.replies, out);
            }
        }
        anchors(&slide.comments, &mut referenced);
        self.shapes.insert(
            owner.clone(),
            map.shapes.iter().filter(|(id, _)| referenced.contains(id)).map(|(id, shape)| (*id, shape.id.to_string())).collect(),
        );
        let legacy_name = format!("ppt/comments/comment{}.xml", self.bindings.len() + 1);
        let modern_name = format!("ppt/comments/modernComment{}.xml", self.bindings.len() + 1);
        let mut mo = Out::new(&modern_name);
        let first = codec::write_page(&slide.comments, &self.authors, &map, &mut Rich { x, out: &mut mo }, false)?;
        for note in &first.notes {
            log::debug!("{note}");
        }
        let mut keys = vec![first.projection.key.clone()];
        self.projection.owners.push(first.projection);
        let mut rid = None;
        if first.lane == Lane::Modern {
            self.modern = true;
            rid = Some(so.rels.add(RT_COMMENTS_MODERN, &opc::relative(&so.name, &modern_name), false));
            x.pkg.xml_part(&modern_name, CT_COMMENTS_MODERN, first.bytes);
            x.pkg.rels(&modern_name, &mo.rels);
            let mut lo = Out::new(&legacy_name);
            let legacy = codec::write_page(&slide.comments, &self.authors, &map, &mut Rich { x, out: &mut lo }, true)?;
            keys.push(legacy.projection.key.clone());
            self.projection.owners.push(legacy.projection);
            for (k, v) in legacy.last_indices {
                self.last.entry(k).and_modify(|a| *a = (*a).max(v)).or_insert(v);
            }
            x.pkg.xml_part(&legacy_name, CT_COMMENTS, legacy.bytes);
        } else {
            for (k, v) in first.last_indices {
                self.last.entry(k).and_modify(|a| *a = (*a).max(v)).or_insert(v);
            }
            x.pkg.xml_part(&legacy_name, CT_COMMENTS, first.bytes);
        }
        so.rels.add("comments", &opc::relative(&so.name, &legacy_name), false);
        self.bindings.push((owner, index, keys));
        Ok(rid)
    }
    pub fn finish(self, x: &mut Exp, pres: &mut Out) -> Result<(), PptxError> {
        if self.bindings.is_empty() {
            return Ok(());
        }
        x.pkg.xml_part("ppt/commentAuthors.xml", CT_AUTHORS, codec::write_authors(&self.authors, false, &self.last)?);
        pres.rels.add("commentAuthors", "commentAuthors.xml", false);
        if self.modern {
            x.pkg.xml_part("ppt/authors.xml", CT_AUTHORS_MODERN, codec::write_authors(&self.authors, true, &self.last)?);
            pres.rels.add(RT_AUTHORS_MODERN, "authors.xml", false);
        }
        let mut used = BTreeSet::new();
        for owner in &self.owners {
            comments::resources::visit::comments(&owner.comments, &mut |id| {
                used.insert(*id);
            });
        }
        let mut media = Vec::new();
        for id in used {
            let m = x.p.media(id).ok_or_else(|| error("attachment media missing"))?.clone();
            let part = x.media_part(id).ok_or_else(|| error("embedded comment resource missing"))?;
            media.push(ExtensionMedia { id, part: part.clone(), content_type: m.content_type.clone(), size: m.data.len() as u64 });
        }
        let mut extension = CommentExtension::capture(x.p, self.projection, &self.bindings, media).map_err(error)?;
        extension.shape_bindings = self.shapes;
        extension.slide_bindings = x.slide_parts.iter().map(|(id, part)| (*id, part.clone())).collect();
        let bytes = extension.encode().map_err(error)?;
        x.pkg.xml_part(crate::comments_extension::PART, crate::comments_extension::MIME, bytes);
        pres.rels.add(crate::comments_extension::REL, "deckcraft/comments.json", false);
        Ok(())
    }
}
pub(crate) fn wire_map(p: &Presentation, slide: &Slide, owner: &str, slide_id: u32, ids: &HashMap<ShapeId, u32>) -> Result<WireMap, PptxError> {
    let ctx = deckcraft_model::resolve::Ctx::for_slide(p, slide).ok_or_else(|| error("missing slide context"))?;
    fn walk(
        items: &[Shape],
        ctx: &deckcraft_model::resolve::Ctx<'_>,
        ids: &HashMap<ShapeId, u32>,
        parent: deckcraft_geom::Affine,
        out: &mut BTreeMap<ShapeId, WireShape>,
        depth: usize,
    ) -> Result<(), PptxError> {
        if depth > 32 {
            return Err(error("shape tree exceeds limit"));
        }
        for s in items {
            let id = *ids.get(&s.id).ok_or_else(|| error("shape wire mapping missing"))?;
            let mut x = deckcraft_model::resolve::xfrm(ctx, s);
            for v in [&mut x.x, &mut x.y, &mut x.w, &mut x.h] {
                *v = (*v * 12700.0).round() / 12700.0;
            }
            x.rot = (x.rot * 60000.0).round() / 60000.0;
            let pos = parent * deckcraft_geom::Point::new(x.x, x.y);
            let kind = match &s.kind {
                deckcraft_model::ShapeKind::Group { .. } => ShapeKind::Group,
                deckcraft_model::ShapeKind::Picture { .. } | deckcraft_model::ShapeKind::Media(_) => ShapeKind::Picture,
                deckcraft_model::ShapeKind::Connector { .. } => ShapeKind::Connector,
                deckcraft_model::ShapeKind::Table(_) | deckcraft_model::ShapeKind::Chart(_) => ShapeKind::GraphicFrame,
                _ => ShapeKind::Shape,
            };
            if out.insert(s.id, WireShape { id, kind, origin_x: pos.x, origin_y: pos.y, creation_id: None }).is_some() {
                return Err(error("ambiguous duplicate shape identity"));
            }
            if let deckcraft_model::ShapeKind::Group { children, child } = &s.kind {
                walk(
                    children,
                    ctx,
                    ids,
                    parent
                        * deckcraft_geom::group_child_affine(
                            &x,
                            deckcraft_geom::Rect::new(
                                (child.x * 12700.0).round() / 12700.0,
                                (child.y * 12700.0).round() / 12700.0,
                                ((child.x + child.w) * 12700.0).round() / 12700.0,
                                ((child.y + child.h) * 12700.0).round() / 12700.0,
                            ),
                        ),
                    out,
                    depth + 1,
                )?;
            }
        }
        Ok(())
    }
    let mut shapes = BTreeMap::new();
    walk(&slide.shapes, &ctx, ids, deckcraft_geom::Affine::IDENTITY, &mut shapes, 0)?;
    Ok(WireMap { owner: owner.into(), slide_id, shapes })
}
struct Rich<'a, 'b> {
    x: &'a mut Exp<'b>,
    out: &'a mut Out,
}
impl BodyCodec for Rich<'_, '_> {
    fn encode(&mut self, body: &TextBody) -> Result<EncodedBody, PptxError> {
        let mut w = W::new();
        // This body is embedded in the comment part, so it must be a fragment
        // without a second XML declaration inside the document element.
        w.s.clear();
        dml::text_body(&mut w, self.x, self.out, "p188:txBody", body, true);
        if let Some(error) = self.x.error.take() {
            return Err(error);
        }
        let xml = w.s;
        let projected = crate::read::comments::effective_body(&xml, &self.out.name, &self.out.rels)?;
        Ok(EncodedBody { xml, projected })
    }
    fn decode(&mut self, _: &crate::xml::El) -> Result<DecodedBody, PptxError> {
        Err(error("writer cannot decode source"))
    }
    fn projection(&self, body: &crate::comments_dom::Element) -> Result<Option<String>, PptxError> {
        let rels = opc::rels_from_bytes(&self.out.rels.to_xml(), &self.out.name);
        crate::comments_dom::body_projection(body, &rels).map(Some)
    }
}

pub(crate) fn expected(
    extension: &CommentExtension,
    maps: &BTreeMap<String, WireMap>,
    parts: &BTreeMap<String, String>,
    p: &Presentation,
    media_ids: &BTreeMap<MediaId, MediaId>,
) -> Result<CommentProjection, PptxError> {
    let mut aliases = p.clone();
    aliases.media.clear();
    for binding in &extension.media {
        let actual = *media_ids.get(&binding.id).ok_or_else(|| error("expected media alias missing"))?;
        let mut m = p.media(actual).ok_or_else(|| error("expected media missing"))?.clone();
        m.id = binding.id;
        aliases.media.push(m);
    }
    let mut x = Exp {
        p: &aliases,
        pkg: Default::default(),
        media_parts: extension.media.iter().map(|m| (m.id, m.part.clone())).collect(),
        n_image: 0,
        n_media: 0,
        n_chart: 0,
        n_diagram: 0,
        n_tags: 0,
        n_chartex: 0,
        n_kept: 0,
        slide_parts: extension.slide_bindings.iter().map(|(id, part)| (*id, part.clone())).collect(),
        table_styles: Vec::new(),
        table_hashes: HashMap::new(),
        placeholder: None,
        error: None,
    };
    let authors = codec::Authors::native(&extension.owners, &BTreeMap::new())?;
    let mut projection = CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: Vec::new() };
    for owner in &extension.owners {
        let map = maps.get(&owner.key).ok_or_else(|| error("expected owner missing"))?;
        let mut expected_keys = Vec::new();
        for key in &owner.projection_owners {
            let part = parts.get(key).ok_or_else(|| error("actual core lane missing"))?;
            let mut out = Out::new(part);
            let force_legacy = key.ends_with(":legacy");
            let written = codec::write_page(&owner.comments, &authors, map, &mut Rich { x: &mut x, out: &mut out }, force_legacy)?;
            expected_keys.push(written.projection.key.clone());
            projection.owners.push(written.projection);
        }
        if expected_keys != owner.projection_owners {
            return Err(error("expected lane bindings differ"));
        }
    }
    Ok(projection)
}
