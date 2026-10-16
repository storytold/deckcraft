//! Standard annotations, production rich-body codec, and package extension capture.
use super::*;
use crate::comments_codec::{self as codec, BodyCodec, DecodedBody, EncodedBody, WireMap, WireText};
use deckcraft_model::comments::{self, CommentExtension, CommentProjection, ExtensionMedia};
use std::collections::BTreeSet;
const HOLDER: &str = "DeckCraftCommentHolder";
pub(super) struct State {
    projection: CommentProjection,
    bindings: Vec<(String, usize, Vec<String>)>,
    shapes: BTreeMap<String, BTreeMap<ShapeId, String>>,
}
impl State {
    pub fn new() -> Self {
        Self {
            projection: CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: Vec::new() },
            bindings: Vec::new(),
            shapes: BTreeMap::new(),
        }
    }
    pub fn page(&mut self, writer: &mut Writer, ctx: &Ctx<'_>, slide: &Slide, index: usize) -> Result<Vec<String>> {
        if slide.comments.is_empty() {
            return Ok(Vec::new());
        }
        let owner = format!("slide:{index}");
        let mut map = WireMap { owner: owner.clone(), texts: BTreeMap::new(), holder_style: HOLDER.into() };
        let mut aliases = BTreeMap::new();
        let mut referenced = BTreeSet::new();
        fn anchors(items: &[Comment], out: &mut BTreeSet<ShapeId>) {
            for c in items {
                if let Some(id) = c.shape {
                    out.insert(id);
                }
                anchors(&c.replies, out);
            }
        }
        anchors(&slide.comments, &mut referenced);
        let mut duplicate = false;
        deckcraft_model::walk(&slide.shapes, &mut |shape, _| {
            let key = writer.shape_identity(shape.id);
            if referenced.contains(&shape.id) && aliases.insert(shape.id, key.clone()).is_some() {
                duplicate = true;
            }
            if let Some(body) = &shape.text
                && matches!(shape.kind, ShapeKind::Shape | ShapeKind::Connector { .. })
            {
                map.texts.insert(shape.id, WireText { key, body: body.clone() });
            }
        });
        if duplicate {
            return Err(OdpError("ambiguous comment shape IDs".into()));
        }
        self.shapes.insert(owner.clone(), aliases);
        let written = codec::write_page(&slide.comments, &map, &mut Rich { writer, ctx })?;
        // Native-only distinctions are deliberately preserved by the bound extension.
        let _native_only_projection_notes = &written.issues;
        writer.comment_markers = written.markers;
        self.projection.owners.push(written.projection);
        self.bindings.push((owner.clone(), index, vec![owner]));
        Ok(written.floating)
    }
    pub fn finish(self, p: &Presentation) -> Result<Option<CommentExtension>> {
        if self.bindings.is_empty() {
            return Ok(None);
        }
        let mut used = BTreeSet::new();
        for slide in &p.slides {
            comments::resources::visit::comments(&slide.comments, &mut |id| {
                used.insert(*id);
            });
        }
        let mut media = Vec::new();
        for id in used {
            let m = p.media(id).ok_or_else(|| OdpError("comment resource missing".into()))?;
            media.push(ExtensionMedia {
                id,
                part: format!("DeckCraft/attachments/{}.bin", id.0),
                content_type: m.content_type.clone(),
                size: m.data.len() as u64,
            });
        }
        let mut extension = CommentExtension::capture(p, self.projection, &self.bindings, media).map_err(|e| OdpError(e.to_string()))?;
        extension.shape_bindings = self.shapes;
        extension.slide_bindings = p.slides.iter().map(|slide| (slide.id, timing::page_id(slide.id))).collect();
        Ok(Some(extension))
    }
}
struct Rich<'a, 'b, 'c> {
    writer: &'a mut Writer<'b>,
    ctx: &'a Ctx<'c>,
}
impl BodyCodec for Rich<'_, '_, '_> {
    fn encode(&mut self, body: &TextBody) -> Result<EncodedBody> {
        let shape = Shape { id: ShapeId(0), text: Some(body.clone()), ..Default::default() };
        let mut xml = String::new();
        self.writer.text_content(self.ctx, &shape, body, &mut xml)?;
        let source = format!(
            "{}<office:automatic-styles>{}</office:automatic-styles><office:body><office:annotation>{xml}</office:annotation></office:body></office:document-content>",
            header("office:document-content"),
            self.writer.styles
        );
        let root = crate::xml::parse(source.as_bytes())?;
        let mut styles = crate::read::Styles::new();
        crate::read::comment_styles(&root, &mut styles)?;
        let annotation =
            root.child("office:body").and_then(|b| b.child("office:annotation")).ok_or_else(|| OdpError("encoded annotation body missing".into()))?;
        let decoded = crate::read::comment_body(annotation, &styles)?;
        if decoded.unmapped {
            return Err(OdpError("annotation body adapter produced unrepresented source semantics".into()));
        }
        Ok(EncodedBody { xml, projected: decoded.body })
    }
    fn decode(&mut self, _: &crate::xml::El) -> Result<DecodedBody> {
        Err(OdpError("annotation writer cannot decode".into()))
    }
}
pub(super) fn holder_style() -> &'static str {
    HOLDER
}
pub(crate) fn expected(
    extension: &CommentExtension,
    maps: &BTreeMap<String, WireMap>,
    p: &Presentation,
    owners: &BTreeMap<String, usize>,
) -> Result<CommentProjection> {
    let mut aliases = p.clone();
    for slide in &mut aliases.slides {
        let actual = slide.as_ref();
        let native =
            extension.slide_bindings.iter().find(|(_, wire)| actual.tags.iter().any(|(k, v)| k == "odp:id" && v == *wire)).map(|(id, _)| *id);
        if let Some(id) = native {
            std::sync::Arc::make_mut(slide).id = id;
        }
    }
    let p = &aliases;
    let mut writer = Writer {
        transform: deckcraft_geom::Affine::IDENTITY,
        owner: String::new(),
        styles: String::new(),
        definitions: String::new(),
        objects: Vec::new(),
        notes_layouts: String::new(),
        notes_declarations: String::new(),
        next: 0,
        depth: 0,
        images: BTreeMap::new(),
        pres: p,
        comment_markers: BTreeMap::new(),
    };
    let mut projection = CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: Vec::new() };
    for owner in &extension.owners {
        if owner.projection_owners != vec![owner.key.clone()] {
            return Err(OdpError("comment owner projection binding differs".into()));
        }
        let index = *owners.get(&owner.key).ok_or_else(|| OdpError("comment owner missing".into()))?;
        let slide = p.slides.get(index).ok_or_else(|| OdpError("comment slide missing".into()))?;
        let ctx = Ctx::for_slide(p, slide).ok_or_else(|| OdpError("comment slide context missing".into()))?;
        writer.owner = format!("slide{}", slide.id.0);
        let map = maps.get(&owner.key).ok_or_else(|| OdpError("comment wire map missing".into()))?;
        projection.owners.push(codec::write_page(&owner.comments, map, &mut Rich { writer: &mut writer, ctx: &ctx })?.projection);
    }
    Ok(projection)
}
