//! Real ODF annotation/range projection. Thread/status/identity/file semantics use the bound
//! extension, while every message and supported text range remains visible in standard ODF.
use crate::{
    OdpError, Result,
    xml::{El, Node},
};
use deckcraft_model::{
    Comment, ShapeId,
    comments::{
        self, AuthorOrigin, CommentAnchor, CommentAuthorIdentity, CommentProjection, CommentSourceId, ProjectionAuthor, ProjectionMessage,
        ProjectionOwner,
    },
    text::TextBody,
};
use std::collections::{BTreeMap, BTreeSet};
fn err(message: &str) -> OdpError {
    OdpError(format!("comments: {message}"))
}
fn model(error: comments::CommentError) -> OdpError {
    OdpError(error.to_string())
}
const MAX_TEXT: usize = 1_048_576;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_NODES: usize = 1_000_000;
#[derive(Default)]
pub(crate) struct Budget {
    nodes: usize,
    bytes: usize,
    messages: usize,
    model: comments::Budget,
}
impl Budget {
    fn node(&mut self) -> Result<()> {
        self.nodes = self.nodes.checked_add(1).ok_or_else(|| err("work overflow"))?;
        if self.nodes > MAX_NODES {
            return Err(err("work limit exceeded"));
        }
        Ok(())
    }
    fn charge(&mut self, count: usize) -> Result<()> {
        self.bytes = self.bytes.checked_add(count).ok_or_else(|| err("size overflow"))?;
        if self.bytes > MAX_BYTES {
            return Err(err("metadata size limit exceeded"));
        }
        Ok(())
    }
    fn message(&mut self) -> Result<()> {
        self.messages = self.messages.checked_add(1).ok_or_else(|| err("message overflow"))?;
        if self.messages > 100_000 {
            return Err(err("message limit exceeded"));
        }
        self.charge(std::mem::size_of::<Comment>())
    }
}
/// Reserve before the typography adapter borrows the main importer. No saturating identity reuse.
pub(crate) struct IdPool {
    next: u32,
    end: u32,
}
impl IdPool {
    pub fn reserve(next: &mut u32, count: usize) -> Result<Self> {
        if count > 300_000 {
            return Err(err("identity reservation limit exceeded"));
        }
        let count = u32::try_from(count).map_err(|_| err("identity count overflow"))?;
        let start = *next;
        *next = start.checked_add(count).ok_or_else(|| err("identity space exhausted"))?;
        Ok(Self { next: start, end: *next })
    }
    fn alloc(&mut self) -> Result<u32> {
        let id = self.next.checked_add(1).ok_or_else(|| err("identity space exhausted"))?;
        if id > self.end || id == 0 {
            return Err(err("identity reservation exhausted"));
        }
        self.next = id;
        Ok(id)
    }
}
pub(crate) struct EncodedBody {
    /// Actual text:p/text:list XML with resolved style definitions.
    pub xml: String,
    /// Native->ODF->native effective body under the actual text/style adapter.
    pub projected: TextBody,
}
pub(crate) struct DecodedBody {
    pub body: TextBody,
    pub unmapped: bool,
}
pub(crate) trait BodyCodec {
    fn encode(&mut self, body: &TextBody) -> Result<EncodedBody>;
    fn decode(&mut self, annotation: &El) -> Result<DecodedBody>;
}
#[derive(Clone)]
pub(crate) struct WireText {
    pub key: String,
    pub body: TextBody,
}
/// Construct only from the current payload's verified shape/text serialization map. A shape
/// without an editable text owner cannot carry ODF text markers: its anchor stays in extension.
#[derive(Clone)]
pub(crate) struct WireMap {
    pub owner: String,
    pub texts: BTreeMap<ShapeId, WireText>,
    pub holder_style: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Placement {
    pub shape: Option<ShapeId>,
    pub wire: String,
    pub start: u32,
    pub end: Option<u32>,
    pub proven: bool,
}
impl Placement {
    fn projection(&self) -> Vec<String> {
        if !self.proven {
            vec!["unknown".into()]
        } else if self.shape.is_none() {
            vec!["slide".into()]
        } else if let Some(end) = self.end {
            vec!["text-range".into(), self.wire.clone(), self.start.to_string(), end.to_string()]
        } else {
            vec!["shape".into(), self.wire.clone()]
        }
    }
    fn native(&self) -> CommentAnchor {
        if !self.proven {
            CommentAnchor::Unknown
        } else {
            match (self.shape, self.end) {
                (Some(shape), Some(end)) => CommentAnchor::TextRange { shape, start: self.start, end },
                (Some(shape), None) => CommentAnchor::Shape { shape },
                _ => CommentAnchor::Slide,
            }
        }
    }
}
#[derive(Default)]
pub(crate) struct Markers {
    by_offset: BTreeMap<u32, Vec<String>>,
    position: u32,
}
impl Markers {
    fn insert(&mut self, offset: u32, xml: String) -> Result<()> {
        let markers = self.by_offset.entry(offset).or_default();
        markers.try_reserve(1).map_err(|_| err("cannot allocate markers"))?;
        markers.push(xml);
        Ok(())
    }
    /// Call inside text:p before each scalar/field/break, and at the final paragraph end.
    pub fn flush(&mut self, out: &mut String) -> Result<()> {
        if let Some(items) = self.by_offset.remove(&self.position) {
            for item in items {
                append(out, &item)?;
            }
        }
        Ok(())
    }
    /// Text emitted through this path keeps styles in the surrounding span. ODF whitespace
    /// is explicit, so annotation boundaries never depend on whitespace normalization.
    pub fn text(&mut self, text: &str, out: &mut String) -> Result<()> {
        for ch in text.chars() {
            self.flush(out)?;
            match ch {
                ' ' => append(out, "<text:s/>")?,
                '\t' => append(out, "<text:tab/>")?,
                '\n' | '\u{b}' => append(out, "<text:line-break/>")?,
                _ => escape(&ch.to_string(), out, false)?,
            }
            self.advance(1)?;
        }
        Ok(())
    }
    /// Advance after an already emitted typed field/break by its *actual displayed* scalar
    /// count. Root splits fields at marker boundaries only when that field codec supports it;
    /// otherwise reject export rather than silently placing the range elsewhere.
    pub fn advance(&mut self, count: u32) -> Result<()> {
        let end = self.position.checked_add(count).ok_or_else(|| err("text offset overflow"))?;
        if self.by_offset.range((std::ops::Bound::Excluded(self.position), std::ops::Bound::Excluded(end))).next().is_some() {
            return Err(err("annotation falls inside an unsplittable field; retain source and report"));
        }
        self.position = end;
        Ok(())
    }
    /// A native paragraph separator is one LF scalar. Flush before closing the old text:p,
    /// advance(1), then flush inside the new text:p before its first scalar.
    pub fn finish(mut self, out: &mut String) -> Result<()> {
        self.flush(out)?;
        if !self.by_offset.is_empty() {
            return Err(err("unemitted annotation marker"));
        }
        Ok(())
    }
}
pub(crate) struct Written {
    pub floating: Vec<String>,
    pub markers: BTreeMap<ShapeId, Markers>,
    pub projection: ProjectionOwner,
    pub issues: BTreeSet<&'static str>,
}
fn append(out: &mut String, s: &str) -> Result<()> {
    if out.len().checked_add(s.len()).is_none_or(|n| n > MAX_BYTES) {
        return Err(err("output limit exceeded"));
    }
    out.try_reserve(s.len()).map_err(|_| err("cannot allocate output"))?;
    out.push_str(s);
    Ok(())
}
fn escape(value: &str, out: &mut String, attribute: bool) -> Result<()> {
    for ch in value.chars() {
        match ch {
            '&' => append(out, "&amp;")?,
            '<' => append(out, "&lt;")?,
            '>' => append(out, "&gt;")?,
            '"' if attribute => append(out, "&quot;")?,
            '\r' => append(out, "&#13;")?,
            '\n' if attribute => append(out, "&#10;")?,
            '\t' if attribute => append(out, "&#9;")?,
            c if c == '\t' || c == '\n' || c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}' => append(out, &c.to_string())?,
            _ => return Err(err("invalid XML 1.0 character")),
        }
    }
    Ok(())
}
fn attr(out: &mut String, key: &str, value: &str) -> Result<()> {
    append(out, " ")?;
    append(out, key)?;
    append(out, "=\"")?;
    escape(value, out, true)?;
    append(out, "\"")
}
fn scalar(out: &mut String, key: &str, value: &str) -> Result<()> {
    append(out, "<")?;
    append(out, key)?;
    append(out, ">")?;
    escape(value, out, false)?;
    append(out, "</")?;
    append(out, key)?;
    append(out, ">")
}
fn names(items: &[Comment]) -> Result<Vec<(&Comment, String)>> {
    fn walk<'a>(items: &'a [Comment], out: &mut Vec<(&'a Comment, String)>, used: &mut BTreeSet<String>) -> Result<()> {
        for c in items {
            let id = c.comment_id.ok_or_else(|| err("normalize identities before ODF export"))?;
            let base = match &c.source_id {
                Some(CommentSourceId::Odf { name }) if !name.is_empty() => name.clone(),
                _ => format!("deckcraft-comment-{id}"),
            };
            let name = if used.contains(&base) { format!("deckcraft-comment-{id}") } else { base };
            if !used.insert(name.clone()) {
                return Err(err("annotation names collide"));
            }
            out.push((c, name));
            walk(&c.replies, out, used)?;
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(items, &mut out, &mut BTreeSet::new())?;
    Ok(out)
}
fn position(c: &Comment, map: &WireMap, issues: &mut BTreeSet<&'static str>) -> Result<Placement> {
    let (shape, start, end) = match c.comment_anchor() {
        CommentAnchor::Slide => return Ok(Placement { shape: None, wire: String::new(), start: 0, end: None, proven: true }),
        CommentAnchor::Unknown => {
            issues.insert("ODF annotation anchor is unknown; exact native metadata remains in the bound extension");
            return Ok(Placement { shape: None, wire: String::new(), start: 0, end: None, proven: true });
        }
        CommentAnchor::Shape { shape } => (shape, 0, None),
        CommentAnchor::TextRange { shape, start, end } => (shape, start, Some(end)),
    };
    let Some(text) = map.texts.get(&shape) else {
        issues.insert("ODF annotation shape has no verified editable text owner; standard annotation floats and the exact anchor remains in the bound extension");
        return Ok(Placement { shape: None, wire: String::new(), start: 0, end: None, proven: true });
    };
    let len =
        u32::try_from(comments::checked_body_text(&text.body).map_err(model)?.chars().count()).map_err(|_| err("text range length overflow"))?;
    if start > len || end.is_some_and(|end| end < start || end > len) {
        return Err(err("annotation range exceeds verified text owner"));
    }
    Ok(Placement { shape: Some(shape), wire: text.key.clone(), start, end, proven: true })
}
fn message_projection(
    c: &Comment,
    name: &str,
    body: TextBody,
    placement: &Placement,
    x: Option<String>,
    y: Option<String>,
) -> Result<ProjectionMessage> {
    Ok(ProjectionMessage {
        body_xml: None,
        key: name.into(),
        author_key: name.into(),
        author: c.author.clone(),
        initials: Some(c.initials.clone()),
        text: comments::checked_body_text(&body).map_err(model)?,
        body: Some(body),
        task: None,
        date: if c.date.is_empty() { None } else { Some(c.date.clone()) },
        display_date: c.display_date.clone(),
        status: None,
        anchor: placement.projection(),
        x,
        y,
    })
}
/// Emit every reply recursively as a separate named standard annotation. Markers are returned
/// for injection into the real rich shape text emitter; floating holders go directly on page.
pub(crate) fn write_page(items: &[Comment], map: &WireMap, body: &mut impl BodyCodec) -> Result<Written> {
    comments::validate_metadata(items, &mut comments::Budget::default()).map_err(model)?;
    if map.texts.values().any(|v| v.key.is_empty()) || map.holder_style.is_empty() {
        return Err(err("unverified text map or neutral holder style"));
    }
    let mut wire_keys = BTreeSet::new();
    for value in map.texts.values() {
        if !wire_keys.insert(&value.key) {
            return Err(err("text owner wire keys collide"));
        }
    }
    let mut emitted = 0usize;
    let mut out = Written {
        floating: Vec::new(),
        markers: BTreeMap::new(),
        projection: ProjectionOwner { key: map.owner.clone(), present: true, authors: Vec::new(), messages: Vec::new() },
        issues: BTreeSet::new(),
    };
    for (c, name) in names(items)? {
        let placement = position(c, map, &mut out.issues)?;
        let native = c.body.clone().unwrap_or_else(|| TextBody::from_text(&c.text));
        // VT maps to text:line-break; other raw text must be XML-safe before
        // any text writer can discard an unrepresentable input scalar.
        for run in native.paragraphs.iter().flat_map(|paragraph| &paragraph.runs) {
            if run
                .text
                .chars()
                .any(|ch| !matches!(ch, '\t' | '\n' | '\r' | '\u{b}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
            {
                return Err(err("invalid XML 1.0 character in annotation body"));
            }
        }
        let encoded = body.encode(&native)?;
        if encoded.xml.len() > 8 * MAX_TEXT {
            return Err(err("annotation body XML exceeds limit"));
        }
        let x = format!("{}pt", c.x);
        let y = format!("{}pt", c.y);
        if !c.date.is_empty() && !crate::annotation_metadata::valid_date(&c.date) {
            return Err(err("date outside supported XML Schema dateTime profile"));
        }
        let mut xml = String::from("<office:annotation");
        attr(&mut xml, "office:name", &name)?;
        attr(&mut xml, "svg:x", &x)?;
        attr(&mut xml, "svg:y", &y)?;
        append(&mut xml, ">")?;
        scalar(&mut xml, "dc:creator", &c.author)?;
        if !c.date.is_empty() {
            scalar(&mut xml, "dc:date", &c.date)?;
        }
        if let Some(date) = &c.display_date {
            scalar(&mut xml, "meta:date-string", date)?;
        }
        // ODF annotation metadata is a sequence: creator, date, display date,
        // then initials, followed by the rich text paragraphs/lists.
        scalar(&mut xml, "meta:creator-initials", &c.initials)?;
        append(&mut xml, &encoded.xml)?;
        append(&mut xml, "</office:annotation>")?;
        emitted = emitted.checked_add(xml.len().saturating_add(1024)).ok_or_else(|| err("annotation output size overflow"))?;
        if emitted > MAX_BYTES {
            return Err(err("total annotation output exceeds limit"));
        }
        out.projection.authors.push(ProjectionAuthor {
            key: name.clone(),
            name: c.author.clone(),
            initials: Some(c.initials.clone()),
            user_id: None,
            provider_id: None,
        });
        out.projection.messages.push(message_projection(c, &name, encoded.projected, &placement, Some(x), Some(y))?);
        if let Some(shape) = placement.shape {
            let markers = out.markers.entry(shape).or_default();
            markers.insert(placement.start, xml)?;
            if let Some(end) = placement.end {
                let mut close = String::from("<office:annotation-end");
                attr(&mut close, "office:name", &name)?;
                append(&mut close, "/>")?;
                markers.insert(end, close)?;
            }
        } else {
            let id = c.comment_id.ok_or_else(|| err("identity missing"))?;
            let mut frame = String::from("<draw:frame");
            attr(&mut frame, "draw:name", &format!("deckcraft-comment-holder-{id}"))?;
            attr(&mut frame, "draw:style-name", &map.holder_style)?;
            append(&mut frame, " svg:x=\"0pt\" svg:y=\"0pt\" svg:width=\"1pt\" svg:height=\"1pt\"><draw:text-box><text:p>")?;
            append(&mut frame, &xml)?;
            append(&mut frame, "</text:p></draw:text-box></draw:frame>")?;
            out.floating.push(frame);
        }
    }
    out.projection.authors.sort_by(|a, b| a.key.cmp(&b.key));
    out.projection.messages.sort_by(|a, b| a.key.cmp(&b.key));
    comments::validate_projection(
        &CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: vec![out.projection.clone()] },
        &mut comments::Budget::default(),
    )
    .map_err(model)?;
    Ok(out)
}
/// Root may omit a holder from editable shapes only after this exact structural proof and
/// independent proof that holder_style resolves to an empty/no-stroke/no-fill graphic style.
pub(crate) fn holder_annotation<'a>(el: &'a El, holder_style: &str) -> Option<&'a El> {
    if el.name != "draw:frame"
        || el.attr("draw:style-name") != Some(holder_style)
        || !el.attr("draw:name")?.strip_prefix("deckcraft-comment-holder-")?.parse::<u32>().is_ok_and(|id| id > 0)
    {
        return None;
    }
    if el.attrs.len() != 6
        || el.attr("svg:x") != Some("0pt")
        || el.attr("svg:y") != Some("0pt")
        || el.attr("svg:width") != Some("1pt")
        || el.attr("svg:height") != Some("1pt")
    {
        return None;
    }
    let box_el = only_child(el, "draw:text-box")?;
    if !box_el.attrs.is_empty() {
        return None;
    }
    let p = only_child(box_el, "text:p")?;
    if !p.attrs.is_empty() {
        return None;
    }
    only_child(p, "office:annotation")
}
fn only_child<'a>(el: &'a El, name: &str) -> Option<&'a El> {
    if el.children.iter().any(|n| matches!(n,Node::Text(s) if !s.trim().is_empty())) {
        return None;
    }
    let mut children = el.elements();
    let child = children.next()?;
    if children.next().is_some() || child.name != name {
        return None;
    }
    Some(child)
}
/// A shape's actual parsed text container and independently imported native body. Root builds
/// this only for unambiguous, current source owners (including nested groups, not table cells).
pub(crate) struct ReadText<'a> {
    pub element: &'a El,
    pub shape: ShapeId,
    pub wire: &'a str,
    pub body: &'a TextBody,
}
#[derive(Default)]
pub(crate) struct Scanned {
    pub placements: BTreeMap<usize, Placement>,
    pub ends: BTreeSet<usize>,
    pub issues: BTreeSet<&'static str>,
}
struct ScanState {
    text: String,
    scalars: u32,
    end_sources: BTreeSet<usize>,
    started: bool,
    pending: bool,
    uncertain: bool,
    starts: Vec<(usize, Option<String>, u32)>,
    ends: BTreeMap<String, u32>,
    paragraphs: usize,
}
impl ScanState {
    fn offset(&self) -> Result<u32> {
        Ok(self.scalars)
    }
    fn put(&mut self, text: &str) -> Result<()> {
        if self.text.len().checked_add(text.len()).is_none_or(|len| len > MAX_TEXT) {
            return Err(err("owner text exceeds range scanning limit"));
        }
        self.scalars = self
            .scalars
            .checked_add(u32::try_from(text.chars().count()).map_err(|_| err("text offset overflow"))?)
            .ok_or_else(|| err("text offset overflow"))?;
        self.text.push_str(text);
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        if self.pending {
            self.pending = false;
            self.put(" ")?;
        }
        Ok(())
    }
    fn normal(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            if matches!(ch, ' ' | '\t' | '\r' | '\n') {
                if self.started {
                    self.pending = true;
                }
            } else {
                self.flush()?;
                self.put(&ch.to_string())?;
                self.started = true;
            }
        }
        Ok(())
    }
    fn inline(&mut self, el: &El, budget: &mut Budget, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(err("range XML nesting exceeds limit"));
        }
        budget.node()?;
        for node in &el.children {
            match node {
                Node::Text(text) => self.normal(text)?,
                Node::El(e) => {
                    budget.node()?;
                    match e.name.as_str() {
                        "office:annotation" => {
                            if self.pending {
                                self.uncertain = true;
                            }
                            self.starts.push((e.source_start(), e.attr("office:name").map(str::to_owned), self.offset()?));
                        }
                        "office:annotation-end" => {
                            if self.pending {
                                self.uncertain = true;
                            }
                            let name = e.attr("office:name").filter(|v| !v.is_empty()).ok_or_else(|| err("annotation-end lacks required name"))?;
                            if !e.children.is_empty() || e.attrs.len() != 1 {
                                return Err(err("unsupported annotation-end structure"));
                            }
                            self.end_sources.insert(e.source_start());
                            if self.ends.insert(name.into(), self.offset()?).is_some() {
                                return Err(err("duplicate annotation end name"));
                            }
                        }
                        "text:s" => {
                            self.flush()?;
                            let count = e.attr("text:c").unwrap_or("1").parse::<usize>().map_err(|_| err("invalid explicit space count"))?;
                            if count > 100_000 {
                                return Err(err("explicit space count exceeds limit"));
                            }
                            self.put(&" ".repeat(count))?;
                            self.started = true;
                        }
                        "text:tab" => {
                            self.flush()?;
                            self.put("\t")?;
                            self.started = true;
                        }
                        "text:line-break" => {
                            self.flush()?;
                            self.put("\u{b}")?;
                            self.started = true;
                        }
                        "text:span" | "text:a" | "text:page-number" | "text:date" | "text:time" => self.inline(e, budget, depth + 1)?,
                        // These presentation fields have an empty native displayed string.
                        "presentation:header" | "presentation:footer" | "presentation:date-time" => {
                            if e.elements().next().is_some() || e.children.iter().any(|n| matches!(n,Node::Text(t) if !t.trim().is_empty())) {
                                self.uncertain = true;
                            }
                        }
                        _ => {
                            self.uncertain = true;
                            self.inline(e, budget, depth + 1)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn blocks(&mut self, el: &El, budget: &mut Budget, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(err("range XML nesting exceeds limit"));
        }
        budget.node()?;
        for child in el.elements() {
            if child.name == "office:annotation" {
                continue;
            }
            if matches!(child.name.as_str(), "text:p" | "text:h") {
                if self.paragraphs > 0 {
                    self.put("\n")?;
                }
                self.paragraphs = self.paragraphs.checked_add(1).ok_or_else(|| err("paragraph count overflow"))?;
                if self.paragraphs > 10_000 {
                    return Err(err("paragraph count exceeds limit"));
                }
                self.started = false;
                self.pending = false;
                self.inline(child, budget, depth + 1)?;
                self.pending = false;
            } else {
                self.blocks(child, budget, depth + 1)?;
            }
        }
        Ok(())
    }
}
/// Match start/end pairs only after reconstructing exactly the reader's native scalar text.
/// Ambiguous whitespace, unknown inline constructs and foreign text never receive invented
/// offsets: their annotations import as Unknown and their original XML must remain recoverable.
pub(crate) fn scan_placements(owners: &[ReadText<'_>], budget: &mut Budget) -> Result<Scanned> {
    let mut out = Scanned::default();
    let mut wires = BTreeSet::new();
    for owner in owners {
        if owner.wire.is_empty() || !wires.insert(owner.wire) {
            return Err(err("text owner wire map is ambiguous"));
        }
        let mut state = ScanState {
            text: String::new(),
            scalars: 0,
            end_sources: BTreeSet::new(),
            started: false,
            pending: false,
            uncertain: false,
            starts: Vec::new(),
            ends: BTreeMap::new(),
            paragraphs: 0,
        };
        state.blocks(owner.element, budget, 0)?;
        let exact = !state.uncertain && state.text == comments::checked_body_text(owner.body).map_err(model)?;
        if !exact && (!state.starts.is_empty() || !state.ends.is_empty()) {
            out.issues.insert("ODF annotation range text could not be proved against the imported Unicode-scalar body; exact source is retained and its anchor is Unknown");
        }
        out.ends.extend(state.end_sources);
        let mut seen = BTreeSet::new();
        for (source, name, start) in state.starts {
            let end = if let Some(name) = &name {
                if !seen.insert(name.clone()) {
                    return Err(err("duplicate annotation name within text owner"));
                }
                state.ends.remove(name)
            } else {
                None
            };
            if end.is_some_and(|end| end < start) {
                return Err(err("annotation end precedes start"));
            }
            let placement = Placement { shape: Some(owner.shape), wire: owner.wire.into(), start, end, proven: exact };
            if out.placements.insert(source, placement).is_some() {
                return Err(err("one annotation belongs to multiple text owners"));
            }
        }
        if !state.ends.is_empty() {
            return Err(err("annotation end has no start in its text owner"));
        }
    }
    Ok(out)
}
fn collect<'a>(el: &'a El, out: &mut Vec<&'a El>, ends: &mut Vec<usize>, budget: &mut Budget, depth: usize, in_annotation: bool) -> Result<()> {
    if depth > 64 {
        return Err(err("annotation XML nesting exceeds limit"));
    }
    budget.node()?;
    for child in el.elements() {
        if child.name == "office:annotation-end" {
            if in_annotation {
                return Err(err("annotation-end inside annotation body"));
            }
            ends.push(child.source_start());
        }
        if child.name == "office:annotation" {
            if in_annotation {
                return Err(err("nested ODF annotations are outside supported schema profile"));
            }
            budget.message()?;
            out.try_reserve(1).map_err(|_| err("cannot allocate annotation list"))?;
            out.push(child);
            collect(child, out, ends, budget, depth + 1, true)?;
        } else {
            collect(child, out, ends, budget, depth + 1, in_annotation)?;
        }
    }
    Ok(())
}
fn read_scalar(el: &El, name: &str, budget: &mut Budget) -> Result<Option<String>> {
    let mut children = el.elements().filter(|c| c.name == name);
    let Some(child) = children.next() else {
        return Ok(None);
    };
    if children.next().is_some() || child.elements().next().is_some() || !child.attrs.is_empty() {
        return Err(err("annotation scalar is duplicated or structured"));
    }
    let mut value = String::new();
    for node in &child.children {
        if let Node::Text(text) = node {
            if value.len().checked_add(text.len()).is_none_or(|n| n > crate::annotation_metadata::MAX_SCALAR.min(comments::MAX_SCALAR)) {
                return Err(err("annotation scalar exceeds limit"));
            }
            value.push_str(text);
        }
    }
    budget.charge(value.len())?;
    Ok(Some(value))
}
fn preflight(el: &El, budget: &mut Budget, depth: usize, paragraphs: &mut usize) -> Result<()> {
    if depth > 64 {
        return Err(err("annotation rich body nesting exceeds limit"));
    }
    budget.node()?;
    if matches!(el.name.as_str(), "text:p" | "text:h") {
        *paragraphs = paragraphs.checked_add(1).ok_or_else(|| err("paragraph count overflow"))?;
        if *paragraphs > 10_000 {
            return Err(err("annotation paragraph count exceeds limit"));
        }
    }
    for (key, value) in &el.attrs {
        budget.charge(key.len().saturating_add(value.len()))?;
        if value.len() > 8192 {
            return Err(err("annotation attribute exceeds limit"));
        }
    }
    for node in &el.children {
        match node {
            Node::Text(text) => budget.charge(text.len())?,
            Node::El(child) => preflight(child, budget, depth + 1, paragraphs)?,
        }
    }
    Ok(())
}
pub(crate) struct Read {
    pub comments: Vec<Comment>,
    pub projection: ProjectionOwner,
    pub issues: BTreeSet<&'static str>,
    pub retain_source: bool,
}
/// Standard import is always authoritative. `floating` contains only source offsets proved by
/// holder_annotation plus the independently resolved neutral style; it cannot be sidecar data.
pub(crate) fn read_page(
    page: &El,
    owner: &str,
    texts: &[ReadText<'_>],
    floating: &BTreeSet<usize>,
    body: &mut impl BodyCodec,
    ids: &mut IdPool,
    budget: &mut Budget,
) -> Result<Read> {
    let mut scanned = scan_placements(texts, budget)?;
    let mut elements = Vec::new();
    let mut ends = Vec::new();
    collect(page, &mut elements, &mut ends, budget, 0, false)?;
    if ends.iter().any(|end| !scanned.ends.contains(end)) {
        scanned.issues.insert("ODF annotation end lies outside a verified editable text owner; exact range source is retained");
    }
    let mut out = Read {
        comments: Vec::new(),
        projection: ProjectionOwner { key: owner.into(), present: true, authors: Vec::new(), messages: Vec::new() },
        issues: std::mem::take(&mut scanned.issues),
        retain_source: false,
    };
    let mut names = BTreeSet::new();
    for el in elements {
        preflight(el, budget, 0, &mut 0)?;
        let source_name = el.attr("office:name");
        if source_name.is_some_and(|name| name.is_empty() || name.len() > crate::annotation_metadata::MAX_SCALAR.min(comments::MAX_SCALAR)) {
            return Err(err("invalid annotation name"));
        }
        let key = source_name.map(str::to_owned).unwrap_or_else(|| format!("@source:{}", el.source_start()));
        if !names.insert(key.clone()) {
            return Err(err("annotation names collide"));
        }
        let author = read_scalar(el, "dc:creator", budget)?.unwrap_or_default();
        let initials = read_scalar(el, "meta:creator-initials", budget)?;
        let date = read_scalar(el, "dc:date", budget)?;
        if date.as_ref().is_some_and(|date| !crate::annotation_metadata::valid_date(date)) {
            return Err(err("date outside supported XML Schema dateTime profile"));
        }
        let display_date = read_scalar(el, "meta:date-string", budget)?;
        let x = el.attr("svg:x");
        let y = el.attr("svg:y");
        let number = |value: Option<&str>| -> Result<f64> {
            value.map_or(Ok(0.0), |value| crate::read::length(value).filter(|n| n.is_finite()).ok_or_else(|| err("invalid annotation coordinate")))
        };
        let placement = if let Some(placement) = scanned.placements.remove(&el.source_start()) {
            placement
        } else if floating.contains(&el.source_start()) {
            Placement { shape: None, wire: String::new(), start: 0, end: None, proven: true }
        } else {
            out.issues.insert("ODF annotation has no verified shape text or neutral floating holder; exact anchor is retained as Unknown");
            Placement { shape: None, wire: String::new(), start: 0, end: None, proven: false }
        };
        let decoded = body.decode(el)?;
        if decoded.unmapped {
            out.issues.insert("ODF annotation rich body contains source semantics outside the active text adapter; exact source is retained");
        }
        if el.attrs.keys().any(|key| !matches!(key.as_str(), "office:name" | "svg:x" | "svg:y"))
            || el.elements().any(|child| {
                !matches!(
                    child.name.as_str(),
                    "dc:creator" | "dc:date" | "meta:creator-initials" | "meta:date-string" | "text:p" | "text:h" | "text:list"
                )
            })
            || el.children.iter().any(|node| matches!(node,Node::Text(text) if !text.trim().is_empty()))
        {
            out.issues.insert("ODF annotation has unrepresented attributes or children; exact source is retained");
        }
        let text = comments::checked_body_text(&decoded.body).map_err(model)?;
        let mut comment = Comment {
            comment_id: Some(ids.alloc()?),
            author_identity: Some(CommentAuthorIdentity {
                id: ids.alloc()?,
                origin: AuthorOrigin::Unknown,
                source_initials: initials.clone(),
                source_scope: Some(owner.into()),
                external_id: None,
                user_id: None,
                provider_id: None,
                wire_guid: None,
            }),
            source_id: source_name.map(|name| CommentSourceId::Odf { name: name.into() }),
            author: author.clone(),
            initials: initials.clone().unwrap_or_default(),
            text: text.clone(),
            body: Some(decoded.body.clone()),
            date: date.clone().unwrap_or_default(),
            display_date: display_date.clone(),
            x: number(x)?,
            y: number(y)?,
            ..Default::default()
        };
        comment.set_comment_anchor(placement.native());
        comments::validate_metadata(std::slice::from_ref(&comment), &mut budget.model).map_err(model)?;
        out.projection.authors.push(ProjectionAuthor {
            key: key.clone(),
            name: author.clone(),
            initials: initials.clone(),
            user_id: None,
            provider_id: None,
        });
        out.projection.messages.push(ProjectionMessage {
            body_xml: None,
            key: key.clone(),
            author_key: key,
            author,
            initials,
            text,
            body: Some(decoded.body),
            task: None,
            date,
            display_date,
            status: None,
            anchor: placement.projection(),
            x: x.map(str::to_owned),
            y: y.map(str::to_owned),
        });
        out.comments.push(comment);
    }
    if !scanned.placements.is_empty() {
        return Err(err("text annotation has no actual standard annotation"));
    }
    out.projection.authors.sort_by(|a, b| a.key.cmp(&b.key));
    out.projection.messages.sort_by(|a, b| a.key.cmp(&b.key));
    out.retain_source = !out.issues.is_empty();
    comments::validate_projection(
        &CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: vec![out.projection.clone()] },
        &mut comments::Budget::default(),
    )
    .map_err(model)?;
    Ok(out)
}
/// Mandatory extension restoration hook: regenerate from typed payload and independently
/// verified CURRENT native->wire text maps; never use the stored projection as an oracle.
#[cfg(test)]
pub(crate) fn regenerate(
    owners: &[comments::ExtensionOwner],
    maps: &BTreeMap<String, WireMap>,
    body: &mut impl BodyCodec,
) -> Result<CommentProjection> {
    let mut projection = CommentProjection { profile: crate::comments_extension::PROFILE.into(), owners: Vec::new() };
    let mut used = BTreeSet::new();
    for owner in owners {
        if owner.projection_owners != vec![owner.key.clone()] || !used.insert(&owner.key) {
            return Err(err("ODF sidecar owner mapping is not one-to-one"));
        }
        let map = maps.get(&owner.key).ok_or_else(|| err("verified annotation owner missing"))?;
        let mut written = write_page(&owner.comments, map, body)?;
        written.projection.authors.sort_by(|a, b| a.key.cmp(&b.key));
        written.projection.messages.sort_by(|a, b| a.key.cmp(&b.key));
        projection.owners.push(written.projection);
    }
    Ok(projection)
}
#[cfg(test)]
#[path = "comments_codec_tests.rs"]
mod tests;
