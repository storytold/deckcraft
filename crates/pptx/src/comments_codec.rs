//! Actual public-schema comment/author codecs and deterministic native projections.
//! Typography delegates to the application's existing DrawingML body codec.
use crate::{
    PptxError,
    comments_dom::{self as dom, Element, Node, Xml},
};
use deckcraft_model::{
    Comment, ShapeId,
    comments::{
        self, AuthorOrigin, CommentAnchor, CommentAuthorIdentity, CommentSourceId, CommentStatus, CommentTask, CommentTaskAuthor, ExtensionOwner,
        ProjectionAuthor, ProjectionMessage, ProjectionOwner, ProjectionTask,
    },
    text::TextBody,
};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, PptxError>;
fn error(message: &str) -> PptxError {
    PptxError::Unsupported(format!("comments: {message}"))
}
fn model(error: comments::CommentError) -> PptxError {
    PptxError::Unsupported(error.to_string())
}
fn required<'a>(el: &'a Element, key: &str) -> Result<&'a str> {
    el.attr(key).ok_or_else(|| error("required attribute missing"))
}
fn unsigned(value: &str) -> Result<u32> {
    value.trim().parse().map_err(|_| error("invalid unsigned identifier"))
}
fn status(value: Option<&str>) -> Result<CommentStatus> {
    match value {
        None | Some("active") => Ok(CommentStatus::Active),
        Some("resolved") => Ok(CommentStatus::Resolved),
        Some("closed") => Ok(CommentStatus::Closed),
        _ => Err(error("unknown modern status")),
    }
}
fn status_text(status: CommentStatus) -> &'static str {
    match status {
        CommentStatus::Active => "active",
        CommentStatus::Resolved => "resolved",
        CommentStatus::Closed => "closed",
    }
}
fn guid(value: &str) -> bool {
    value.starts_with('{') && value.ends_with('}') && comments::valid_guid(value)
}

/// Reserve a checked disjoint ID range before body decoders borrow the main import state.
#[derive(Debug)]
pub(crate) struct IdPool {
    next: u32,
    end: u32,
}
impl IdPool {
    pub fn reserve(global_next: &mut u32, count: usize) -> Result<Self> {
        let count = u32::try_from(count).map_err(|_| error("ID reservation exceeds limit"))?;
        if count > 400_000 {
            return Err(error("ID reservation exceeds limit"));
        }
        let start = *global_next;
        let end = start.checked_add(count).ok_or_else(|| error("identity space exhausted"))?;
        *global_next = end;
        Ok(Self { next: start, end })
    }
    fn alloc(&mut self) -> Result<u32> {
        let id = self.next.checked_add(1).ok_or_else(|| error("identity space exhausted"))?;
        if id > self.end || id == 0 {
            return Err(error("reserved identity range exhausted"));
        }
        self.next = id;
        Ok(id)
    }
}

pub(crate) struct EncodedBody {
    /// One p188:txBody, with all actual relation IDs established by the root adapter.
    pub xml: String,
    /// Effective body obtained by the native->DML mapping, not an untested clone.
    pub projected: TextBody,
}
pub(crate) struct DecodedBody {
    pub body: TextBody,
    /// True only for unrepresented source semantics; root retains original part/package.
    pub unmapped: bool,
}
pub(crate) trait BodyCodec {
    fn projection(&self, _body: &dom::Element) -> Result<Option<String>> {
        Ok(None)
    }
    fn encode(&mut self, body: &TextBody) -> Result<EncodedBody>;
    fn decode(&mut self, body: &crate::xml::El) -> Result<DecodedBody>;
}

#[derive(Clone, Debug)]
pub(crate) struct Author {
    pub identity: CommentAuthorIdentity,
    pub name: String,
    pub initials: Option<String>,
    pub legacy_id: u32,
}
impl Author {
    fn projection(&self, modern: bool) -> Result<ProjectionAuthor> {
        Ok(ProjectionAuthor {
            key: self.key(modern)?,
            name: self.name.clone(),
            initials: if modern { self.initials.clone() } else { Some(self.initials.clone().unwrap_or_default()) },
            user_id: if modern { Some(self.identity.user_id.clone().unwrap_or_default()) } else { None },
            provider_id: if modern { Some(self.identity.provider_id.clone().unwrap_or_default()) } else { None },
        })
    }
    fn key(&self, modern: bool) -> Result<String> {
        if modern {
            self.identity.wire_guid.clone().filter(|value| guid(value)).ok_or_else(|| error("modern author wire GUID missing"))
        } else {
            Ok(self.legacy_id.to_string())
        }
    }
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Authors {
    pub by_local: BTreeMap<u32, Author>,
    pub by_wire: BTreeMap<String, u32>,
    pub modern: bool,
    pub unmapped: bool,
}
impl Authors {
    /// Construct from the typed payload only. Explicit verified legacy overrides preserve
    /// source numeric IDs where possible; no name-only identity deduplication occurs.
    pub fn native(owners: &[ExtensionOwner], legacy_overrides: &BTreeMap<u32, u32>) -> Result<Self> {
        let mut budget = comments::Budget::default();
        for owner in owners {
            comments::validate_metadata(&owner.comments, &mut budget).map_err(model)?;
        }
        fn put(table: &mut Authors, identity: &CommentAuthorIdentity, name: &str, initials: &str, overrides: &BTreeMap<u32, u32>) -> Result<()> {
            let source_initials =
                identity.source_initials.clone().or_else(|| if identity.origin != AuthorOrigin::Imported { Some(initials.into()) } else { None });
            let author = Author {
                identity: identity.clone(),
                name: name.into(),
                initials: source_initials,
                legacy_id: overrides.get(&identity.id).copied().unwrap_or(identity.id),
            };
            if let Some(old) = table.by_local.get(&identity.id) {
                if old.identity != author.identity || old.name != author.name || old.initials != author.initials {
                    return Err(error("author relationship metadata conflicts"));
                }
            } else {
                table.by_local.insert(identity.id, author);
            }
            Ok(())
        }
        fn visit(items: &[Comment], table: &mut Authors, overrides: &BTreeMap<u32, u32>) -> Result<()> {
            for c in items {
                let identity = c.author_identity.as_ref().ok_or_else(|| error("normalize native comment identities before projection"))?;
                put(table, identity, &c.author, &c.initials, overrides)?;
                if let Some(task) = &c.task {
                    for author in &task.authors {
                        put(table, &author.identity, &author.name, &author.initials, overrides)?;
                    }
                }
                visit(&c.replies, table, overrides)?;
            }
            Ok(())
        }
        let mut table = Self::default();
        for owner in owners {
            visit(&owner.comments, &mut table, legacy_overrides)?;
        }
        if table.by_local.len() > 10_000 {
            return Err(error("author count exceeds limit"));
        }
        let mut legacy = BTreeSet::new();
        let mut modern = BTreeSet::new();
        for author in table.by_local.values() {
            if !legacy.insert(author.legacy_id) {
                return Err(error("legacy author wire IDs collide; supply a verified collision-free map"));
            }
            if let Some(id) = &author.identity.wire_guid
                && (!guid(id) || !modern.insert(id.to_ascii_uppercase()))
            {
                return Err(error("invalid or duplicate modern author wire identity"));
            }
        }
        Ok(table)
    }
    fn get(&self, local: u32) -> Result<&Author> {
        self.by_local.get(&local).ok_or_else(|| error("author relationship missing"))
    }
    fn wire(&self, key: &str) -> Result<&Author> {
        let canonical = if self.modern { key.to_ascii_uppercase() } else { unsigned(key)?.to_string() };
        self.by_wire.get(&canonical).and_then(|id| self.by_local.get(id)).ok_or_else(|| error("source author relationship missing"))
    }
}

/// Independent actual author-part parse, with exact optional initials and provider/user values.
pub(crate) fn read_authors(bytes: &[u8], scope: &str, ids: &mut IdPool, budget: &mut dom::Budget) -> Result<Authors> {
    if scope.len() > 4096 {
        return Err(error("source scope exceeds limit"));
    }
    let root = dom::parse(bytes, budget)?;
    let modern = root.is(dom::M, "authorLst");
    let legacy = root.is(dom::P, "cmAuthorLst") || root.is(dom::PS, "cmAuthorLst");
    if !modern && !legacy {
        return Err(error("unexpected author root/namespace"));
    }
    let mut table = Authors { modern, ..Default::default() };
    let mut count = 0usize;
    for el in root.elements() {
        if !(modern && el.is(dom::M, "author") || legacy && (el.is(dom::P, "cmAuthor") || el.is(dom::PS, "cmAuthor"))) {
            table.unmapped = true;
            continue;
        }
        count += 1;
        if count > 10_000 {
            return Err(error("author count exceeds limit"));
        }
        let raw = required(el, "id")?;
        if modern && !guid(raw) {
            return Err(error("invalid modern author GUID"));
        }
        let legacy_id = if modern { 0 } else { unsigned(raw)? };
        let key = if modern { raw.to_ascii_uppercase() } else { legacy_id.to_string() };
        if table.by_wire.contains_key(&key) {
            return Err(error("duplicate source author identity"));
        }
        let name = required(el, "name")?.to_string();
        let initials = if legacy { Some(required(el, "initials")?.to_string()) } else { el.attr("initials").map(str::to_string) };
        if name.len() > 4096 || initials.as_ref().is_some_and(|value| value.len() > 4096) {
            return Err(error("author scalar exceeds limit"));
        }
        let user = if modern { Some(required(el, "userId")?.to_string()) } else { None };
        let provider = if modern { Some(required(el, "providerId")?.to_string()) } else { None };
        for name in el.attrs.keys() {
            let admitted = if modern {
                matches!(name.local.as_str(), "id" | "name" | "initials" | "userId" | "providerId")
            } else {
                matches!(name.local.as_str(), "id" | "name" | "initials" | "lastIdx" | "clrIdx")
            };
            if !name.ns.is_empty() || !admitted {
                table.unmapped = true;
            }
        }
        if el.elements().next().is_some() {
            table.unmapped = true;
        }
        let id = ids.alloc()?;
        let identity = CommentAuthorIdentity {
            id,
            origin: AuthorOrigin::Imported,
            external_id: Some(raw.into()),
            user_id: user,
            provider_id: provider,
            source_initials: initials.clone(),
            source_scope: Some(scope.into()),
            wire_guid: if modern { Some(raw.into()) } else { None },
        };
        table.by_wire.insert(key, id);
        table.by_local.insert(id, Author { identity, name, initials, legacy_id });
    }
    Ok(table)
}

pub(crate) fn write_authors(authors: &Authors, modern: bool, last_indices: &BTreeMap<u32, u32>) -> Result<Vec<u8>> {
    let mut xml = Xml::new();
    xml.raw("<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
    if modern {
        xml.raw("<p188:authorLst xmlns:p188=\"")?;
        xml.raw(dom::M)?;
        xml.raw("\">")?;
    } else {
        xml.raw("<p:cmAuthorLst xmlns:p=\"")?;
        xml.raw(dom::P)?;
        xml.raw("\">")?;
    }
    for author in authors.by_local.values() {
        xml.raw(if modern { "<p188:author" } else { "<p:cmAuthor" })?;
        xml.attr("id", &author.key(modern)?)?;
        xml.attr("name", &author.name)?;
        if modern {
            if let Some(initials) = &author.initials {
                xml.attr("initials", initials)?;
            }
            xml.attr("userId", author.identity.user_id.as_deref().unwrap_or(""))?;
            xml.attr("providerId", author.identity.provider_id.as_deref().unwrap_or(""))?;
        } else {
            xml.attr("initials", author.initials.as_deref().unwrap_or(""))?;
            xml.attr("lastIdx", &last_indices.get(&author.legacy_id).copied().unwrap_or(0).to_string())?;
            xml.attr("clrIdx", &(author.legacy_id % 8).to_string())?;
        }
        xml.raw("/>")?;
    }
    xml.raw(if modern { "</p188:authorLst>" } else { "</p:cmAuthorLst>" })?;
    Ok(xml.0.into_bytes())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShapeKind {
    Shape,
    Group,
    Picture,
    Connector,
    GraphicFrame,
}
impl ShapeKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Shape => "spMk",
            Self::Group => "grpSpMk",
            Self::Picture => "picMk",
            Self::Connector => "cxnSpMk",
            Self::GraphicFrame => "graphicFrameMk",
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct WireShape {
    pub id: u32,
    pub kind: ShapeKind,
    pub origin_x: f64,
    pub origin_y: f64,
    pub creation_id: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct WireMap {
    pub owner: String,
    pub slide_id: u32,
    pub shapes: BTreeMap<ShapeId, WireShape>,
}
impl WireMap {
    fn shape(&self, id: ShapeId) -> Result<&WireShape> {
        self.shapes.get(&id).ok_or_else(|| error("shape attachment has no verified serialized owner ID"))
    }
    fn native_shape(&self, wire: u32) -> Result<(ShapeId, &WireShape)> {
        let mut matches = self.shapes.iter().filter(|(_, shape)| shape.id == wire);
        let Some((id, shape)) = matches.next() else {
            return Err(error("source shape moniker target is missing"));
        };
        if matches.next().is_some() {
            return Err(error("source shape moniker target is ambiguous"));
        }
        Ok((*id, shape))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Legacy,
    Modern,
}
pub(crate) struct Written {
    pub bytes: Vec<u8>,
    pub projection: ProjectionOwner,
    pub lane: Lane,
    pub last_indices: BTreeMap<u32, u32>,
    pub notes: BTreeSet<&'static str>,
}
pub(crate) struct Read {
    pub comments: Vec<Comment>,
    pub projection: ProjectionOwner,
    pub modern: bool,
    pub issues: BTreeSet<&'static str>,
    /// Root must preserve the original bytes/package if true, and keep a loss diagnostic.
    pub retain_source: bool,
}
fn all_modern(items: &[Comment], authors: &Authors) -> bool {
    items.iter().all(|c| {
        !c.date.is_empty()
            && c.wire_guid.as_ref().is_some_and(|id| guid(id))
            && c.author_identity
                .as_ref()
                .and_then(|a| authors.by_local.get(&a.id))
                .is_some_and(|a| a.identity.wire_guid.as_ref().is_some_and(|id| guid(id)))
            && c.task.as_ref().is_none_or(|task| {
                task.assigned_to.iter().all(|id| authors.by_local.get(id).and_then(|a| a.identity.wire_guid.as_ref()).is_some_and(|id| guid(id)))
            })
            && all_modern(&c.replies, authors)
    })
}
fn flatten<'a>(items: &'a [Comment], out: &mut Vec<&'a Comment>) -> Result<()> {
    for c in items {
        out.try_reserve(1).map_err(|_| error("cannot allocate comment projection index"))?;
        out.push(c);
        flatten(&c.replies, out)?;
    }
    Ok(())
}
fn coordinate(value: f64, scale: f64) -> Result<i64> {
    let scaled = value * scale;
    if !value.is_finite() || value.abs() > 2.0e6 || !scaled.is_finite() || scaled.abs() > 1.0e14 {
        return Err(error("comment coordinate exceeds profile"));
    }
    Ok(scaled.round() as i64)
}
fn moniker_prefix(xml: &mut Xml, map: &WireMap) -> Result<()> {
    xml.raw("<pc:docMk/><pc:sldMk")?;
    xml.attr("sldId", &map.slide_id.to_string())?;
    xml.raw("/>")
}
fn write_anchor(xml: &mut Xml, anchor: CommentAnchor, map: &WireMap, notes: &mut BTreeSet<&'static str>) -> Result<(Vec<String>, f64, f64)> {
    match anchor {
        CommentAnchor::Slide => {
            xml.raw("<pc:sldMkLst>")?;
            moniker_prefix(xml, map)?;
            xml.raw("</pc:sldMkLst>")?;
            Ok((vec!["slide".into(), map.slide_id.to_string()], 0.0, 0.0))
        }
        CommentAnchor::Unknown => {
            xml.raw("<p188:unknownAnchor/>")?;
            Ok((vec!["unknown".into()], 0.0, 0.0))
        }
        CommentAnchor::Shape { shape } | CommentAnchor::TextRange { shape, .. } => {
            if matches!(anchor, CommentAnchor::TextRange { .. }) {
                notes.insert("Native text range is retained in DeckCraft metadata; core projects its verified shape because OfficeArt CP/context units are not yet proven");
            }
            let wire = map.shape(shape)?;
            if wire.creation_id.as_ref().is_some_and(|id| !guid(id)) {
                return Err(error("invalid shape creation GUID"));
            }
            xml.raw("<ac:deMkLst>")?;
            moniker_prefix(xml, map)?;
            xml.raw("<ac:")?;
            xml.raw(wire.kind.tag())?;
            xml.attr("id", &wire.id.to_string())?;
            if let Some(id) = &wire.creation_id {
                xml.attr("creationId", id)?;
            }
            xml.raw("/></ac:deMkLst>")?;
            Ok((
                vec![
                    "shape".into(),
                    map.slide_id.to_string(),
                    wire.kind.tag().into(),
                    wire.id.to_string(),
                    wire.creation_id.clone().unwrap_or_default(),
                ],
                wire.origin_x,
                wire.origin_y,
            ))
        }
    }
}
fn task_projection(task: &CommentTask, authors: &Authors) -> Result<ProjectionTask> {
    Ok(ProjectionTask {
        start_date: task.start_date.clone(),
        due_date: task.due_date.clone(),
        assigned_to: task.assigned_to.iter().map(|id| authors.get(*id)?.key(true)).collect::<Result<Vec<_>>>()?,
        complete: task.complete.clone(),
        title: task.title.clone(),
    })
}
fn write_task(xml: &mut Xml, task: &ProjectionTask) -> Result<()> {
    for (key, value) in [("startDate", &task.start_date), ("dueDate", &task.due_date), ("complete", &task.complete), ("title", &task.title)] {
        if let Some(value) = value {
            xml.attr(key, value)?;
        }
    }
    if !task.assigned_to.is_empty() {
        xml.attr("assignedTo", &task.assigned_to.join(" "))?;
    }
    Ok(())
}

/// The returned projection is freshly derived from typed native payload and verified maps.
/// It never reads a payload-supplied projection witness.
pub(crate) fn write_page(items: &[Comment], authors: &Authors, map: &WireMap, body: &mut dyn BodyCodec, force_legacy: bool) -> Result<Written> {
    comments::validate_metadata(items, &mut comments::Budget::default()).map_err(model)?;
    if map.slide_id < 256 {
        return Err(error("serialized slide ID is outside ST_SlideId"));
    }
    let lane = if !force_legacy && all_modern(items, authors) { Lane::Modern } else { Lane::Legacy };
    let modern = lane == Lane::Modern;
    let mut xml = Xml::new();
    xml.raw("<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
    xml.raw(if modern { "<p188:cmLst" } else { "<p:cmLst" })?;
    for (prefix, namespace) in [("p", dom::P), ("p188", dom::M), ("a", dom::A), ("r", dom::R), ("pc", dom::PC), ("ac", dom::AC)] {
        xml.attr(&format!("xmlns:{prefix}"), namespace)?;
    }
    xml.raw(">")?;
    let mut projection = ProjectionOwner {
        key: format!("{}:{}", map.owner, if modern { "modern" } else { "legacy" }),
        present: true,
        authors: Vec::new(),
        messages: Vec::new(),
    };
    let mut used = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut last = BTreeMap::new();
    let mut notes = BTreeSet::new();
    struct Writer<'a> {
        xml: &'a mut Xml,
        projection: &'a mut ProjectionOwner,
        used: &'a mut BTreeSet<u32>,
        ids: &'a mut BTreeSet<String>,
        last: &'a mut BTreeMap<u32, u32>,
        notes: &'a mut BTreeSet<&'static str>,
        authors: &'a Authors,
        map: &'a WireMap,
        body: &'a mut dyn BodyCodec,
    }
    impl Writer<'_> {
        fn one(&mut self, c: &Comment, reply: bool, modern: bool) -> Result<()> {
            let identity = c.author_identity.as_ref().ok_or_else(|| error("normalize native author identity before writing"))?;
            let author = self.authors.get(identity.id)?;
            if author.identity != *identity || author.name != c.author || author.initials.as_ref().is_some_and(|initials| initials != &c.initials) {
                return Err(error("native author payload differs from export author table"));
            }
            self.used.insert(identity.id);
            let author_projection = author.projection(modern)?;
            let key = if modern {
                c.wire_guid.clone().filter(|id| guid(id)).ok_or_else(|| error("missing modern wire identity"))?
            } else {
                let index = match &c.source_id {
                    Some(CommentSourceId::LegacyPptx { author: source, index }) if unsigned(source).ok() == Some(author.legacy_id) => *index,
                    _ => c.comment_id.ok_or_else(|| error("normalize native message identity before writing"))?,
                };
                self.last.entry(author.legacy_id).and_modify(|old| *old = (*old).max(index)).or_insert(index);
                format!("{}:{index}", author.legacy_id)
            };
            if !self.ids.insert(key.to_ascii_uppercase()) {
                return Err(error("wire message identities collide; provide a verified legacy reindex mapping"));
            }
            let task = if modern && !reply { c.task.as_ref().map(|task| task_projection(task, self.authors)).transpose()? } else { None };
            if let Some(task) = &c.task {
                for id in &task.assigned_to {
                    self.used.insert(*id);
                }
            }
            self.xml.raw(if modern { if reply { "<p188:reply" } else { "<p188:cm" } } else { "<p:cm" })?;
            self.xml.attr("authorId", &author_projection.key)?;
            if modern {
                self.xml.attr("id", &key)?;
                self.xml.attr("created", &c.date)?;
                self.xml.attr("status", status_text(c.comment_status()))?;
                if let Some(task) = &task {
                    write_task(self.xml, task)?;
                }
            } else {
                let index = key.rsplit_once(':').map(|(_, index)| index).ok_or_else(|| error("invalid legacy key"))?;
                self.xml.attr("idx", index)?;
                if !c.date.is_empty() {
                    self.xml.attr("dt", &c.date)?;
                }
            }
            self.xml.raw(">")?;
            let (anchor, origin_x, origin_y) = if modern && !reply {
                write_anchor(self.xml, c.comment_anchor(), self.map, self.notes)?
            } else {
                (if reply { vec!["reply".into()] } else { vec!["legacy-slide".into()] }, 0.0, 0.0)
            };
            let position = if !reply {
                let scale = if modern { 12700.0 } else { 8.0 };
                let x = coordinate(c.x - origin_x, scale)?;
                let y = coordinate(c.y - origin_y, scale)?;
                self.xml.raw(if modern { "<p188:pos" } else { "<p:pos" })?;
                self.xml.attr("x", &x.to_string())?;
                self.xml.attr("y", &y.to_string())?;
                self.xml.raw("/>")?;
                (Some(x.to_string()), Some(y.to_string()))
            } else {
                (None, None)
            };
            let text = if modern { c.text.clone() } else { c.text.replace('\u{b}', "\n") };
            let projected_body = if modern {
                let native = c.body.clone().unwrap_or_else(|| TextBody::from_text(&c.text));
                let encoded = self.body.encode(&native)?;

                Some(encoded)
            } else {
                None
            };
            let text = projected_body.as_ref().map(|body| comments::checked_body_text(&body.projected).map_err(model)).transpose()?.unwrap_or(text);
            self.projection.messages.try_reserve(1).map_err(|_| error("cannot allocate projection messages"))?;
            let rich_xml = projected_body
                .as_ref()
                .map(|body| {
                    let source=format!("<x xmlns:p188=\"{}\" xmlns:a=\"{}\" xmlns:r=\"{}\" xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" xmlns:p14=\"http://schemas.microsoft.com/office/powerpoint/2010/main\">{}</x>",dom::M,dom::A,dom::R,body.xml);
                    let doc=dom::parse(source.as_bytes(),&mut dom::Budget::default())?;
                    let tx=doc.one(dom::M,"txBody")?.ok_or_else(||error("encoded rich body missing"))?;
                    self.body.projection(tx)
                })
                .transpose()?
                .flatten();
            self.projection.messages.push(ProjectionMessage {
                body_xml: rich_xml.clone(),
                key: key.clone(),
                author_key: author_projection.key.clone(),
                author: author_projection.name.clone(),
                initials: author_projection.initials.clone(),
                text: text.clone(),
                body: if rich_xml.is_some() { None } else { projected_body.as_ref().map(|body| body.projected.clone()) },
                task: task.clone(),
                date: if c.date.is_empty() { None } else { Some(c.date.clone()) },
                display_date: None,
                status: if modern { Some(status_text(c.comment_status()).into()) } else { None },
                anchor,
                x: position.0,
                y: position.1,
            });
            if modern && !reply && !c.replies.is_empty() {
                self.xml.raw("<p188:replyLst>")?;
                let mut replies = Vec::new();
                flatten(&c.replies, &mut replies)?;
                for reply in replies {
                    self.one(reply, true, true)?;
                }
                self.xml.raw("</p188:replyLst>")?;
            }
            if let Some(encoded) = projected_body {
                self.xml.raw(&encoded.xml)?;
            } else {
                self.xml.raw("<p:text>")?;
                self.xml.escaped(&text, false)?;
                self.xml.raw("</p:text>")?;
            }
            self.xml.raw(if modern { if reply { "</p188:reply>" } else { "</p188:cm>" } } else { "</p:cm>" })?;
            Ok(())
        }
    }
    {
        let mut writer = Writer {
            xml: &mut xml,
            projection: &mut projection,
            used: &mut used,
            ids: &mut ids,
            last: &mut last,
            notes: &mut notes,
            authors,
            map,
            body,
        };
        if modern {
            for c in items {
                writer.one(c, false, true)?;
            }
        } else {
            let mut flat = Vec::new();
            flatten(items, &mut flat)?;
            for c in flat {
                writer.one(c, false, false)?;
            }
        }
    }
    for id in used {
        projection.authors.push(authors.get(id)?.projection(modern)?);
    }
    projection.authors.sort_by(|a, b| a.key.cmp(&b.key));
    xml.raw(if modern { "</p188:cmLst>" } else { "</p:cmLst>" })?;
    comments::validate_projection(
        &deckcraft_model::comments::CommentProjection { profile: "pptx-comments-dual-v1".into(), owners: vec![projection.clone()] },
        &mut comments::Budget::default(),
    )
    .map_err(model)?;
    Ok(Written { bytes: xml.0.into_bytes(), projection, lane, last_indices: last, notes })
}

fn unknown_source(issues: &mut BTreeSet<&'static str>, code: &'static str) {
    issues.insert(code);
}
fn known_attrs(el: &Element, allowed: &[&str], issues: &mut BTreeSet<&'static str>) {
    if el.attrs.keys().any(|name| !name.ns.is_empty() || !allowed.contains(&name.local.as_str())) {
        unknown_source(issues, "Unmapped comment attribute is retained in its original source part");
    }
}
fn canonical(el: &Element) -> Result<String> {
    fn walk(el: &Element, out: &mut Xml) -> Result<()> {
        out.raw("{")?;
        out.escaped(&el.name.ns, false)?;
        out.raw("}")?;
        out.raw(&el.name.local)?;
        out.raw("[")?;
        for (name, value) in &el.attrs {
            out.raw("{")?;
            out.escaped(&name.ns, false)?;
            out.raw("}")?;
            out.raw(&name.local)?;
            out.raw("=")?;
            out.escaped(value, true)?;
            out.raw(";")?;
        }
        out.raw("]")?;
        for node in &el.children {
            match node {
                Node::Element(el) => walk(el, out)?,
                Node::Text(text) => {
                    out.raw("#")?;
                    out.escaped(text, false)?;
                }
            }
        }
        out.raw("/")
    }
    let mut out = Xml::new();
    walk(el, &mut out)?;
    if out.0.len() > 4096 {
        return Err(error("unmapped anchor descriptor exceeds profile; retain source before editing"));
    }
    Ok(out.0)
}
fn read_anchor(el: &Element, map: &WireMap, issues: &mut BTreeSet<&'static str>) -> Result<(CommentAnchor, Vec<String>, f64, f64)> {
    let anchors: Vec<_> = el
        .elements()
        .filter(|child| {
            child.is(dom::PC, "sldMkLst") || child.is(dom::AC, "deMkLst") || child.is(dom::AC, "txMkLst") || child.is(dom::M, "unknownAnchor")
        })
        .collect();
    if anchors.len() != 1 {
        return Err(error("comment must have exactly one supported anchor group"));
    }
    let anchor = *anchors.first().ok_or_else(|| error("anchor missing"))?;
    if anchor.is(dom::M, "unknownAnchor") {
        if !anchor.attrs.is_empty() || anchor.elements().next().is_some() {
            unknown_source(issues, "Structured unknown comment anchor requires source retention");
        }
        return Ok((CommentAnchor::Unknown, vec!["unknown".into()], 0.0, 0.0));
    }
    if anchor.is(dom::AC, "txMkLst") {
        unknown_source(
            issues,
            "OfficeArt text CP/context units are unproven: exact text-range anchor is retained in the original comment source; no scalar conversion was guessed",
        );
        return Ok((CommentAnchor::Unknown, vec!["unmapped-range".into(), canonical(anchor)?], 0.0, 0.0));
    }
    let sequence: Vec<_> = anchor.elements().collect();
    let Some(doc) = sequence.first() else {
        return Err(error("anchor document moniker missing"));
    };
    let Some(slide) = sequence.get(1) else {
        return Err(error("anchor slide moniker missing"));
    };
    if !doc.is(dom::PC, "docMk")
        || !doc.attrs.is_empty()
        || doc.elements().next().is_some()
        || !slide.is(dom::PC, "sldMk")
        || unsigned(required(slide, "sldId")?)? != map.slide_id
    {
        return Err(error("anchor owner/moniker sequence does not match verified slide"));
    }
    if slide.attrs.keys().any(|name| !name.ns.is_empty() || !matches!(name.local.as_str(), "sldId" | "cId")) {
        unknown_source(issues, "Unmapped slide-moniker attribute requires source retention");
    }
    if slide.attr("cId").is_some() {
        unknown_source(issues, "Slide-moniker creation ID is retained in source until verified slide creation metadata is mapped");
        return Ok((CommentAnchor::Unknown, vec!["unmapped-slide-creation".into(), canonical(anchor)?], 0.0, 0.0));
    }
    if !anchor.attrs.is_empty()
        || slide.elements().next().is_some()
        || anchor.children.iter().any(|n| matches!(n, Node::Text(text) if !text.trim().is_empty()))
    {
        unknown_source(issues, "Unmapped moniker-list structure requires source retention");
    }
    if anchor.is(dom::PC, "sldMkLst") {
        if sequence.len() != 2 {
            return Err(error("unexpected slide-moniker suffix"));
        }
        return Ok((CommentAnchor::Slide, vec!["slide".into(), map.slide_id.to_string()], 0.0, 0.0));
    }
    if sequence.len() != 3 {
        return Err(error("unsupported drawing-moniker sequence requires source retention"));
    }
    let target = *sequence.get(2).ok_or_else(|| error("shape moniker missing"))?;
    let wire = unsigned(required(target, "id")?)?;
    let (native, shape) = map.native_shape(wire)?;
    if target.name.ns != dom::AC || target.name.local != shape.kind.tag() {
        return Err(error("shape moniker kind differs from actual owning drawing element"));
    }
    let creation = target.attr("creationId");
    if creation.is_some_and(|id| !guid(id)) {
        return Err(error("invalid shape creation GUID"));
    }
    if creation.is_some() && shape.creation_id.is_none() {
        unknown_source(issues, "Shape creation identity has no independently verified current owner metadata; exact source anchor is retained");
        return Ok((CommentAnchor::Unknown, vec!["unmapped-shape-creation".into(), canonical(anchor)?], 0.0, 0.0));
    }
    if let (Some(source), Some(actual)) = (creation, shape.creation_id.as_deref())
        && !source.eq_ignore_ascii_case(actual)
    {
        return Err(error("shape creation identity differs from actual owner"));
    }
    if target.attrs.keys().any(|name| !name.ns.is_empty() || !matches!(name.local.as_str(), "id" | "creationId")) {
        unknown_source(issues, "Unmapped shape-moniker attribute requires source retention");
    }
    Ok((
        CommentAnchor::Shape { shape: native },
        vec!["shape".into(), map.slide_id.to_string(), shape.kind.tag().into(), wire.to_string(), creation.unwrap_or("").into()],
        shape.origin_x,
        shape.origin_y,
    ))
}
fn body_namespace_unmapped(el: &Element) -> bool {
    let known = matches!(
        el.name.ns.as_str(),
        dom::A
            | dom::AS
            | dom::M
            | "http://schemas.openxmlformats.org/officeDocument/2006/math"
            | "http://schemas.microsoft.com/office/drawing/2010/main"
            | "http://schemas.microsoft.com/office/powerpoint/2010/main"
    );
    // OMML descendants are retained verbatim by dml::paragraph in RunKind::Math;
    // their own namespaced attributes (e.g. m:val) therefore remain represented.
    let math = "http://schemas.openxmlformats.org/officeDocument/2006/math";
    !known
        || el.attrs.keys().any(|name| !(matches!(name.ns.as_str(), "" | dom::R | dom::XML) || (el.name.ns == math && name.ns == math)))
        || el.elements().any(body_namespace_unmapped)
}
fn read_task(el: &Element, authors: &Authors) -> Result<(Option<CommentTask>, Option<ProjectionTask>)> {
    let any = ["startDate", "dueDate", "assignedTo", "complete", "title"].iter().any(|key| el.attr(key).is_some());
    if !any {
        return Ok((None, None));
    }
    let start = el.attr("startDate").map(str::to_string);
    let due = el.attr("dueDate").map(str::to_string);
    for date in [&start, &due].into_iter().flatten() {
        if !deckcraft_model::datetime::valid_date_time(date) {
            return Err(error("invalid task date"));
        }
    }
    let complete = el.attr("complete").map(str::to_string);
    if complete.as_ref().is_some_and(|value| !comments::valid_completion(value)) {
        return Err(error("task completion is outside supported public fixed-percentage profile"));
    }
    let title = el.attr("title").map(str::to_string);
    let mut assigned = Vec::new();
    let mut task_authors = Vec::new();
    let mut wire = Vec::new();
    if let Some(value) = el.attr("assignedTo") {
        for key in value.split_whitespace() {
            if assigned.len() >= comments::MAX_ATTACHMENTS {
                return Err(error("task assignee count exceeds limit"));
            }
            let author = authors.wire(key)?;
            assigned.push(author.identity.id);
            wire.push(key.into());
            if !task_authors.iter().any(|a: &CommentTaskAuthor| a.identity.id == author.identity.id) {
                task_authors.push(CommentTaskAuthor {
                    identity: author.identity.clone(),
                    name: author.name.clone(),
                    initials: author.initials.clone().unwrap_or_default(),
                });
            }
        }
    }
    Ok((
        Some(CommentTask {
            start_date: start.clone(),
            due_date: due.clone(),
            assigned_to: assigned,
            authors: task_authors,
            complete: complete.clone(),
            title: title.clone(),
        }),
        Some(ProjectionTask { start_date: start, due_date: due, assigned_to: wire, complete, title }),
    ))
}

/// Parse every message under a cumulative XML budget. There are no take/depth truncations.
pub(crate) fn read_page(
    bytes: &[u8],
    authors: &Authors,
    map: &WireMap,
    body: &mut dyn BodyCodec,
    ids: &mut IdPool,
    budget: &mut dom::Budget,
) -> Result<Read> {
    let root = dom::parse(bytes, budget)?;
    let modern = root.is(dom::M, "cmLst");
    let namespace = if modern {
        dom::M
    } else if root.is(dom::P, "cmLst") {
        dom::P
    } else if root.is(dom::PS, "cmLst") {
        dom::PS
    } else {
        return Err(error("unexpected comment root/namespace"));
    };
    if authors.modern != modern {
        return Err(error("comment part refers to wrong author schema"));
    }
    let mut comments = Vec::new();
    let mut projection = ProjectionOwner {
        key: format!("{}:{}", map.owner, if modern { "modern" } else { "legacy" }),
        present: true,
        authors: Vec::new(),
        messages: Vec::new(),
    };
    let mut issues = BTreeSet::new();
    known_attrs(&root, &[], &mut issues);
    let mut used = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut count = 0usize;
    struct Reader<'a> {
        authors: &'a Authors,
        map: &'a WireMap,
        body: &'a mut dyn BodyCodec,
        ids: &'a mut IdPool,
        projection: &'a mut ProjectionOwner,
        issues: &'a mut BTreeSet<&'static str>,
        used: &'a mut BTreeSet<u32>,
        keys: &'a mut BTreeSet<String>,
        count: &'a mut usize,
        namespace: &'a str,
        modern: bool,
        model_budget: &'a mut comments::Budget,
    }
    impl Reader<'_> {
        fn one(&mut self, el: &Element, reply: bool) -> Result<Comment> {
            *self.count = self.count.checked_add(1).ok_or_else(|| error("message count overflow"))?;
            if *self.count > comments::MAX_MESSAGES {
                return Err(error("message count exceeds limit"));
            }
            known_attrs(
                el,
                if self.modern && !reply {
                    &["id", "authorId", "created", "status", "startDate", "dueDate", "assignedTo", "complete", "title"]
                } else if self.modern {
                    &["id", "authorId", "created", "status"]
                } else {
                    &["authorId", "dt", "idx"]
                },
                self.issues,
            );
            let author_key = required(el, "authorId")?;
            let author = self.authors.wire(author_key)?;
            self.used.insert(author.identity.id);
            let created = el.attr(if self.modern { "created" } else { "dt" });
            if self.modern && created.is_none() {
                return Err(error("modern created is required and cannot be invented"));
            }
            if created.is_some_and(|date| !deckcraft_model::datetime::valid_date_time(date)) {
                return Err(error("invalid creation timestamp"));
            }
            let state = if self.modern { status(el.attr("status"))? } else { CommentStatus::Active };
            let key = if self.modern {
                let key = required(el, "id")?;
                if !guid(key) {
                    return Err(error("invalid modern message GUID"));
                }
                key.into()
            } else {
                format!("{}:{}", unsigned(author_key)?, unsigned(required(el, "idx")?)?)
            };
            if !self.keys.insert(key.to_ascii_uppercase()) {
                return Err(error("duplicate source message identity"));
            }
            let (anchor, anchor_projection, origin_x, origin_y) = if self.modern && !reply {
                read_anchor(el, self.map, self.issues)?
            } else {
                (CommentAnchor::Slide, if reply { vec!["reply".into()] } else { vec!["legacy-slide".into()] }, 0.0, 0.0)
            };
            let position = el.one(self.namespace, "pos")?;
            if !self.modern && position.is_none() {
                return Err(error("legacy position is required"));
            }
            if reply && position.is_some() {
                return Err(error("core modern reply cannot carry its own position"));
            }
            let read_coordinate = |key: &str| -> Result<Option<String>> {
                position
                    .map(|pos| {
                        required(pos, key).and_then(|value| {
                            value.trim().parse::<i64>().map_err(|_| error("invalid position integer"))?;
                            Ok(value.into())
                        })
                    })
                    .transpose()
            };
            let x = read_coordinate("x")?;
            let y = read_coordinate("y")?;
            let scale = if self.modern { 12700.0 } else { 8.0 };
            let coordinate = |value: &Option<String>, origin: f64| -> Result<f64> {
                let value =
                    value.as_deref().unwrap_or("0").trim().parse::<i64>().map_err(|_| error("invalid position integer"))? as f64 / scale + origin;
                if !value.is_finite() || value.abs() > 1.0e6 {
                    return Err(error("source position exceeds profile"));
                }
                Ok(value)
            };
            let tx = el.one(self.namespace, if self.modern { "txBody" } else { "text" })?;
            if self.modern && tx.is_some_and(body_namespace_unmapped) {
                unknown_source(self.issues, "Foreign namespace in rich comment body requires original-source retention");
            }
            if !self.modern && tx.is_some_and(|tx| !tx.attrs.is_empty()) {
                unknown_source(self.issues, "Unmapped legacy comment text attribute requires original-source retention");
            }
            let decoded = if self.modern { tx.map(|tx| self.body.decode(&tx.legacy())).transpose()? } else { None };
            if decoded.as_ref().is_some_and(|decoded| decoded.unmapped) {
                unknown_source(self.issues, "Unmapped rich comment body is retained in its original source part");
            }
            let native_body = decoded.map(|decoded| decoded.body);
            let text = if let Some(body) = &native_body {
                comments::checked_body_text(body).map_err(model)?
            } else {
                tx.map(Element::scalar).transpose()?.unwrap_or_default()
            };
            if text.len() > comments::MAX_TEXT {
                return Err(error("source body exceeds limit"));
            }
            let (task, task_projection) = if self.modern && !reply { read_task(el, self.authors)? } else { (None, None) };
            if let Some(task) = &task {
                for id in &task.assigned_to {
                    self.used.insert(*id);
                }
            }
            let mut c = Comment {
                author: author.name.clone(),
                initials: author.initials.clone().unwrap_or_default(),
                text: text.clone(),
                date: created.unwrap_or("").into(),
                x: coordinate(&x, origin_x)?,
                y: coordinate(&y, origin_y)?,
                comment_id: Some(self.ids.alloc()?),
                author_identity: Some(author.identity.clone()),
                body: native_body.clone(),
                task,
                wire_guid: if self.modern { Some(key.clone()) } else { None },
                source_id: Some(if self.modern {
                    CommentSourceId::ModernPptx { guid: key.clone() }
                } else {
                    CommentSourceId::LegacyPptx { author: author_key.into(), index: unsigned(required(el, "idx")?)? }
                }),
                ..Default::default()
            };
            c.set_comment_status(state);
            c.set_comment_anchor(anchor);
            comments::validate_metadata(std::slice::from_ref(&c), self.model_budget).map_err(model)?;
            self.projection.messages.try_reserve(1).map_err(|_| error("cannot allocate actual projection"))?;
            let rich_xml = if self.modern { tx.map(|tx| self.body.projection(tx)).transpose()?.flatten() } else { None };
            self.projection.messages.push(ProjectionMessage {
                body_xml: rich_xml.clone(),
                key,
                author_key: if self.modern { author_key.into() } else { unsigned(author_key)?.to_string() },
                author: c.author.clone(),
                initials: if self.modern { author.initials.clone() } else { Some(c.initials.clone()) },
                text,
                body: if rich_xml.is_some() { None } else { native_body },
                task: task_projection,
                date: created.map(str::to_string),
                display_date: None,
                status: if self.modern { el.attr("status").map(str::to_string) } else { None },
                anchor: anchor_projection,
                x,
                y,
            });
            for child in el.elements() {
                let admitted = if self.modern && reply {
                    child.is(dom::M, "txBody")
                } else if self.modern {
                    child.is(dom::M, "txBody")
                        || child.is(dom::M, "pos")
                        || child.is(dom::M, "replyLst")
                        || child.is(dom::M, "unknownAnchor")
                        || child.is(dom::PC, "sldMkLst")
                        || child.is(dom::AC, "deMkLst")
                        || child.is(dom::AC, "txMkLst")
                } else {
                    child.is(self.namespace, "pos") || child.is(self.namespace, "text")
                };
                if !admitted {
                    unknown_source(self.issues, "Unmapped comment child/extension is retained in its original source part");
                }
            }
            if let Some(replies) = el.one(self.namespace, "replyLst")? {
                if reply || !self.modern {
                    return Err(error("reply hierarchy is not representable by the public core schema"));
                }
                for child in replies.elements() {
                    if !child.is(dom::M, "reply") {
                        return Err(error("unmapped reply-list child requires source retention"));
                    }
                    c.replies.try_reserve(1).map_err(|_| error("cannot allocate source replies"))?;
                    c.replies.push(self.one(child, true)?);
                }
            }
            Ok(c)
        }
    }
    {
        let mut reader = Reader {
            authors,
            map,
            body,
            ids,
            projection: &mut projection,
            issues: &mut issues,
            used: &mut used,
            keys: &mut keys,
            count: &mut count,
            namespace,
            modern,
            model_budget: if modern { &mut budget.model_modern } else { &mut budget.model_legacy },
        };
        for el in root.elements() {
            if !el.is(namespace, "cm") {
                return Err(error("unmapped comment-list child requires source retention"));
            }
            comments.try_reserve(1).map_err(|_| error("cannot allocate source roots"))?;
            comments.push(reader.one(el, false)?);
        }
    }
    for id in used {
        projection.authors.push(authors.get(id)?.projection(modern)?);
    }
    projection.authors.sort_by(|a, b| a.key.cmp(&b.key));
    let retain_source = !issues.is_empty();
    Ok(Read { comments, projection, modern, issues, retain_source })
}

/// A fresh payload projection suitable for CommentExtension::restore's mandatory callback.
/// The wire map/body adapter are independently supplied by the package owner; no saved witness
/// participates. Root must combine every actual modern AND legacy part, including absent owners.
#[cfg(test)]
pub(crate) fn regenerate(
    owners: &[ExtensionOwner],
    legacy_overrides: &BTreeMap<u32, u32>,
    maps: &BTreeMap<String, WireMap>,
    body: &mut dyn BodyCodec,
    dual: bool,
) -> Result<comments::CommentProjection> {
    let authors = Authors::native(owners, legacy_overrides)?;
    let mut used = BTreeSet::new();
    let mut projections = Vec::new();
    for owner in owners {
        let map = maps.get(&owner.key).ok_or_else(|| error("verified owner wire map missing"))?;
        if !used.insert(&owner.key) {
            return Err(error("duplicate extension owner"));
        }
        let first = write_page(&owner.comments, &authors, map, body, false)?;
        let modern = first.lane == Lane::Modern;
        let mut keys = vec![first.projection.key.clone()];
        if dual && modern {
            keys.push(format!("{}:legacy", map.owner));
        }
        if owner.projection_owners != keys {
            return Err(error("extension owners do not bind one-to-one to current standard parts"));
        }
        projections.push(first.projection);
        if dual && modern {
            projections.push(write_page(&owner.comments, &authors, map, body, true)?.projection);
        }
    }
    Ok(comments::CommentProjection { profile: "pptx-comments-dual-v1".into(), owners: projections })
}

#[cfg(test)]
#[path = "comments_codec_tests.rs"]
mod tests;
