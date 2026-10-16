//! Bounded editable comment identities and projection-bound interchange metadata.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{Comment, MediaId, Presentation, ShapeId, SlideId};
#[path = "comment_resources.rs"]
pub mod resources;

pub const MAX_MESSAGES: usize = 100_000;
pub const MAX_ROOTS: usize = 10_000;
pub const MAX_DEPTH: usize = 32;
pub const MAX_ATTACHMENTS: usize = 64;
pub const MAX_TEXT: usize = 1024 * 1024;
pub const MAX_SCALAR: usize = 4096;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_EXTENSION_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_WORK: usize = 2_000_000;
pub const MAX_ATTACHMENT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TOTAL_ATTACHMENT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("comments: {0}")]
pub struct CommentError(pub String);
pub type Result<T> = std::result::Result<T, CommentError>;
fn bad(message: &str) -> CommentError {
    CommentError(message.into())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommentStatus {
    #[default]
    Active,
    Resolved,
    Closed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthorOrigin {
    Local,
    Imported,
    #[default]
    Unknown,
}

/// An application-local relationship, not a guessed email/account identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentAuthorIdentity {
    pub id: u32,
    pub origin: AuthorOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Exact optional initials in the originating author record; None stays absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_initials: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_guid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CommentSourceId {
    ModernPptx { guid: String },
    LegacyPptx { author: String, index: u32 },
    Odf { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentAttachment {
    pub id: u32,
    pub media: MediaId,
    pub name: String,
}

/// Character offsets count Unicode scalar values in TextBody plain text, including separators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CommentAnchor {
    Slide,
    Shape { shape: ShapeId },
    TextRange { shape: ShapeId, start: u32, end: u32 },
    Unknown,
}
impl CommentAnchor {
    pub fn shape(self) -> Option<ShapeId> {
        match self {
            Self::Shape { shape } | Self::TextRange { shape, .. } => Some(shape),
            Self::Slide | Self::Unknown => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentTaskAuthor {
    pub identity: CommentAuthorIdentity,
    pub name: String,
    pub initials: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentTask {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(default)]
    pub assigned_to: Vec<u32>,
    /// Referenced-only authors have explicit metadata rather than fabricated authored messages.
    #[serde(default)]
    pub authors: Vec<CommentTaskAuthor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Core metadata must not silently turn a nonfinite optional number into JSON null.
pub fn checked_body_text(body: &crate::text::TextBody) -> Result<String> {
    if body.paragraphs.len() > MAX_ROOTS {
        return Err(bad("rich body paragraph count exceeds limit"));
    }
    let mut out = String::new();
    let mut runs = 0usize;
    for (i, paragraph) in body.paragraphs.iter().enumerate() {
        if i > 0 {
            if out.len() >= MAX_TEXT {
                return Err(bad("rich body text exceeds size limit"));
            }
            out.try_reserve(1).map_err(|_| bad("cannot allocate rich body text"))?;
            out.push('\n');
        }
        runs = runs.checked_add(paragraph.runs.len()).ok_or_else(|| bad("rich body run count overflow"))?;
        if runs > MAX_MESSAGES {
            return Err(bad("rich body run count exceeds limit"));
        }
        for run in &paragraph.runs {
            let text = run.content();
            let length = out.len().checked_add(text.len()).ok_or_else(|| bad("rich body text size overflow"))?;
            if length > MAX_TEXT {
                return Err(bad("rich body text exceeds size limit"));
            }
            out.try_reserve(text.len()).map_err(|_| bad("cannot allocate rich body text"))?;
            out.push_str(text);
        }
    }
    Ok(out)
}
/// Decode a command body only after a nonrecursive bound on Value cloning/serialization.
pub fn parse_body(value: &serde_json::Value) -> Result<crate::text::TextBody> {
    let mut stack = vec![(value, 0usize)];
    let mut budget = 0usize;
    while let Some((value, depth)) = stack.pop() {
        if depth > 64 {
            return Err(bad("rich body input nesting exceeds limit"));
        }
        budget = budget.checked_add(128).ok_or_else(|| bad("rich body input overflow"))?;
        match value {
            serde_json::Value::String(text) => {
                budget = budget.checked_add(text.len().saturating_mul(6)).ok_or_else(|| bad("rich body input overflow"))?
            }
            serde_json::Value::Array(values) => {
                if values.len() > MAX_MESSAGES {
                    return Err(bad("rich body input count exceeds limit"));
                }
                stack.try_reserve(values.len()).map_err(|_| bad("cannot allocate rich body input work"))?;
                for value in values {
                    stack.push((value, depth + 1));
                }
            }
            serde_json::Value::Object(values) => {
                if values.len() > MAX_ROOTS {
                    return Err(bad("rich body input field count exceeds limit"));
                }
                stack.try_reserve(values.len()).map_err(|_| bad("cannot allocate rich body input work"))?;
                for (key, value) in values {
                    budget = budget.saturating_add(key.len().saturating_mul(6));
                    stack.push((value, depth + 1));
                }
            }
            _ => {}
        }
        if budget > MAX_TEXT {
            return Err(bad("rich body command metadata exceeds size limit"));
        }
    }
    let bytes = serde_json::to_vec(value).map_err(|_| bad("cannot encode rich body command"))?;
    let body: crate::text::TextBody = serde_json::from_slice(&bytes).map_err(|_| bad("invalid rich body command"))?;
    checked_body_text(&body)?;
    body_bytes(&body, &mut Budget::default())?;
    Ok(body)
}

fn body_bytes(body: &crate::text::TextBody, budget: &mut Budget) -> Result<()> {
    let runs = body.list_style.levels.iter().flatten().map(|level| &level.run).chain(body.paragraphs.iter().flat_map(|paragraph| {
        std::iter::once(&paragraph.def_props).chain(std::iter::once(&paragraph.end_props)).chain(paragraph.runs.iter().map(|run| &run.props))
    }));
    for run in runs {
        for link in [&run.link, &run.hover].into_iter().flatten() {
            if let Some(name) = &link.sound_name
                && (link.sound.is_none() || name.len() > MAX_SCALAR || name.chars().any(|ch| {
                    !matches!(ch, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
                }))
            {
                return Err(bad("sound label requires a sound resource and a bounded XML string"));
            }
        }
    }
    struct Buffer(Vec<u8>);
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let length = self.0.len().checked_add(bytes.len()).ok_or_else(|| std::io::Error::other("rich body size overflow"))?;
            if length > MAX_TEXT {
                return Err(std::io::Error::other("rich body metadata exceeds size limit"));
            }
            self.0.try_reserve(bytes.len()).map_err(|_| std::io::Error::other("cannot allocate rich body metadata"))?;
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.checked_add(bytes.len()).ok_or_else(|| std::io::Error::other("rich body size overflow"))?;
            if self.0 > MAX_TEXT {
                return Err(std::io::Error::other("rich body metadata exceeds size limit"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, body).map_err(|_| bad("rich body metadata exceeds size limit"))?;
    budget.charge(count.0.saturating_mul(3), count.0 / 16 + 1)?;
    let mut buffer = Buffer(Vec::new());
    serde_json::to_writer(&mut buffer, body).map_err(|_| bad("rich body metadata exceeds size limit"))?;
    let decoded: crate::text::TextBody = serde_json::from_slice(&buffer.0).map_err(|_| bad("invalid rich body metadata"))?;
    if &decoded != body {
        return Err(bad("rich body has nonfinite or unsupported numerical metadata"));
    }
    Ok(())
}

/// Decode old bool-only state while detecting explicitly contradictory new state.
impl<'de> Deserialize<'de> for Comment {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Default, Deserialize)]
        #[serde(default, rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            author: String,
            initials: String,
            #[serde(deserialize_with = "present_text")]
            text: Option<String>,
            date: String,
            x: f64,
            y: f64,
            #[serde(deserialize_with = "present_bool")]
            resolved: Option<bool>,
            replies: Vec<Comment>,
            #[serde(deserialize_with = "present_shape")]
            shape: Option<Option<ShapeId>>,
            display_date: Option<String>,
            comment_id: Option<u32>,
            author_identity: Option<CommentAuthorIdentity>,
            status: Option<CommentStatus>,
            attachments: Vec<CommentAttachment>,
            source_id: Option<CommentSourceId>,
            body: Option<crate::text::TextBody>,
            anchor: Option<CommentAnchor>,
            task: Option<CommentTask>,
            wire_guid: Option<String>,
        }
        fn present_shape<'a, D: serde::Deserializer<'a>>(d: D) -> std::result::Result<Option<Option<ShapeId>>, D::Error> {
            Option::<ShapeId>::deserialize(d).map(Some)
        }
        fn present_text<'a, D: serde::Deserializer<'a>>(d: D) -> std::result::Result<Option<String>, D::Error> {
            String::deserialize(d).map(Some)
        }
        fn present_bool<'a, D: serde::Deserializer<'a>>(d: D) -> std::result::Result<Option<bool>, D::Error> {
            bool::deserialize(d).map(Some)
        }
        fn shallow<E: serde::de::Error>(value: serde_json::Value) -> std::result::Result<Comment, E> {
            let w = Wire::deserialize(value).map_err(E::custom)?;
            if let (Some(status), Some(resolved)) = (w.status, w.resolved)
                && resolved != (status != CommentStatus::Active)
            {
                return Err(serde::de::Error::custom("comment status conflicts with legacy resolved state"));
            }
            let shape = if let Some(anchor) = w.anchor {
                if w.shape.is_some_and(|shape| shape != anchor.shape()) {
                    return Err(serde::de::Error::custom("comment anchor conflicts with legacy shape"));
                }
                anchor.shape()
            } else {
                w.shape.flatten()
            };
            let text = if let Some(body) = &w.body {
                let plain = checked_body_text(body).map_err(serde::de::Error::custom)?;
                if w.text.as_ref().is_some_and(|text| text != &plain) {
                    return Err(serde::de::Error::custom("rich comment body conflicts with plain text"));
                }
                plain
            } else {
                w.text.unwrap_or_default()
            };
            let resolved = w.status.map_or(w.resolved.unwrap_or(false), |status| status != CommentStatus::Active);
            Ok(Comment {
                author: w.author,
                initials: w.initials,
                text,
                date: w.date,
                x: w.x,
                y: w.y,
                resolved,
                replies: w.replies,
                shape,
                display_date: w.display_date,
                comment_id: w.comment_id,
                author_identity: w.author_identity,
                status: w.status,
                attachments: w.attachments,
                source_id: w.source_id,
                body: w.body,
                anchor: w.anchor,
                task: w.task,
                wire_guid: w.wire_guid,
            })
        }
        struct Node {
            comment: Comment,
            children: Vec<usize>,
        }
        let value = crate::serde_graph::captured(deserializer)?;
        let mut pending = vec![Some((value, 0usize))];
        let mut nodes: Vec<Option<Node>> = Vec::new();
        let mut at = 0usize;
        let mut budget = Budget::default();
        while at < pending.len() {
            let (mut value, depth) =
                pending.get_mut(at).and_then(Option::take).ok_or_else(|| serde::de::Error::custom("invalid comment decode graph"))?;
            if depth > MAX_DEPTH {
                return Err(serde::de::Error::custom("reply depth exceeds limit"));
            }
            let children = match crate::serde_graph::field_mut(&mut value, "replies", 7) {
                Some(serde_json::Value::Array(children)) => std::mem::take(children),
                _ => Vec::new(),
            };
            let comment = shallow::<D::Error>(value)?;
            validate_metadata(std::slice::from_ref(&comment), &mut budget).map_err(serde::de::Error::custom)?;
            if pending.len().checked_add(children.len()).is_none_or(|n| n > MAX_MESSAGES) {
                return Err(serde::de::Error::custom("comment message count exceeds limit"));
            }
            let mut child_ids = Vec::new();
            child_ids.try_reserve_exact(children.len()).map_err(|_| serde::de::Error::custom("cannot allocate comment decode graph"))?;
            pending.try_reserve(children.len()).map_err(|_| serde::de::Error::custom("cannot allocate comment decode graph"))?;
            for child in children {
                child_ids.push(pending.len());
                pending.push(Some((child, depth + 1)));
            }
            nodes.try_reserve(1).map_err(|_| serde::de::Error::custom("cannot allocate comment decode graph"))?;
            nodes.push(Some(Node { comment, children: child_ids }));
            at += 1;
        }
        for at in (0..nodes.len()).rev() {
            let mut node = nodes.get_mut(at).and_then(Option::take).ok_or_else(|| serde::de::Error::custom("invalid comment decode graph"))?;
            node.comment.replies.try_reserve_exact(node.children.len()).map_err(|_| serde::de::Error::custom("cannot allocate comment replies"))?;
            for child in node.children {
                let child = nodes.get_mut(child).and_then(Option::take).ok_or_else(|| serde::de::Error::custom("invalid comment decode graph"))?;
                node.comment.replies.push(child.comment);
            }
            *nodes.get_mut(at).ok_or_else(|| serde::de::Error::custom("invalid comment decode graph"))? =
                Some(Node { comment: node.comment, children: Vec::new() });
        }
        nodes.get_mut(0).and_then(Option::take).map(|node| node.comment).ok_or_else(|| serde::de::Error::custom("missing comment decode root"))
    }
}

impl Comment {
    pub fn comment_anchor(&self) -> CommentAnchor {
        self.anchor.unwrap_or_else(|| self.shape.map_or(CommentAnchor::Slide, |shape| CommentAnchor::Shape { shape }))
    }
    pub fn set_comment_anchor(&mut self, anchor: CommentAnchor) {
        self.shape = anchor.shape();
        self.anchor = Some(anchor);
    }
    pub fn comment_status(&self) -> CommentStatus {
        self.status.unwrap_or(if self.resolved { CommentStatus::Resolved } else { CommentStatus::Active })
    }
    pub fn set_comment_status(&mut self, status: CommentStatus) {
        self.status = Some(status);
        self.resolved = status != CommentStatus::Active;
    }
}

#[derive(Default)]
pub struct Budget {
    bytes: usize,
    messages: usize,
    work: usize,
}
impl Budget {
    pub fn charge(&mut self, bytes: usize, work: usize) -> Result<()> {
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(|| bad("byte accounting overflow"))?;
        self.work = self.work.checked_add(work).ok_or_else(|| bad("work accounting overflow"))?;
        if self.bytes > MAX_BYTES || self.work > MAX_WORK {
            return Err(bad("metadata allocation/work budget exceeded"));
        }
        Ok(())
    }
    fn message(&mut self) -> Result<()> {
        self.messages = self.messages.checked_add(1).ok_or_else(|| bad("message count overflow"))?;
        if self.messages > MAX_MESSAGES {
            return Err(bad("message count exceeds limit"));
        }
        self.charge(std::mem::size_of::<Comment>().saturating_mul(2), 1)
    }
    fn string(&mut self, value: &str, limit: usize) -> Result<()> {
        if value.len() > limit {
            return Err(bad("scalar/body exceeds size limit"));
        }
        self.charge(value.len().saturating_mul(2).saturating_add(64), 1)
    }
}

pub fn valid_guid(guid: &str) -> bool {
    let raw = guid.strip_prefix('{').and_then(|s| s.strip_suffix('}')).unwrap_or(guid);
    raw.len() == 36 && raw.bytes().enumerate().all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() })
}

/// Exact supported lexical pattern from the public shared ST_PositiveFixedPercentage schema.
pub fn valid_completion(value: &str) -> bool {
    let Some(number) = value.strip_suffix('%') else {
        return false;
    };
    let mut parts = number.split('.');
    let integer = parts.next().unwrap_or("");
    let fraction = parts.next();
    if parts.next().is_some() || !(integer == "100" || (!integer.is_empty() && integer.len() <= 2 && integer.bytes().all(|b| b.is_ascii_digit()))) {
        return false;
    }
    fraction.is_none_or(|fraction| {
        !fraction.is_empty()
            && fraction.len() <= 2
            && fraction.bytes().all(|b| b.is_ascii_digit())
            && (integer != "100" || fraction.bytes().all(|b| b == b'0'))
    })
}

/// Validate strings/counts before any graph clone or identity index allocation.
pub fn validate_metadata(items: &[Comment], budget: &mut Budget) -> Result<()> {
    fn walk(items: &[Comment], depth: usize, budget: &mut Budget) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        if depth > MAX_DEPTH {
            return Err(bad("reply depth exceeds limit"));
        }
        if depth == 0 && items.len() > MAX_ROOTS {
            return Err(bad("root count exceeds slide limit"));
        }
        for c in items {
            budget.message()?;
            for value in [&c.author, &c.initials] {
                budget.string(value, MAX_SCALAR)?;
            }
            budget.string(&c.text, MAX_TEXT)?;
            if let Some(body) = &c.body {
                if checked_body_text(body)? != c.text {
                    return Err(bad("rich body conflicts with plain text"));
                }
                body_bytes(body, budget)?;
            }
            if c.anchor.is_some_and(|anchor| anchor.shape() != c.shape) {
                return Err(bad("anchor conflicts with legacy shape"));
            }
            if let Some(CommentAnchor::TextRange { start, end, .. }) = c.anchor
                && start > end
            {
                return Err(bad("invalid text anchor range"));
            }
            if let Some(guid) = &c.wire_guid {
                if !valid_guid(guid) {
                    return Err(bad("invalid persistent wire GUID"));
                }
                budget.string(guid, MAX_SCALAR)?;
            }
            if let Some(task) = &c.task {
                if task.assigned_to.len() > MAX_ATTACHMENTS || task.authors.len() > MAX_ATTACHMENTS {
                    return Err(bad("task author count exceeds limit"));
                }
                for date in [&task.start_date, &task.due_date].into_iter().flatten() {
                    budget.string(date, 256)?;
                    if !crate::datetime::valid_date_time(date) {
                        return Err(bad("invalid task date"));
                    }
                }
                if let Some(title) = &task.title {
                    budget.string(title, MAX_SCALAR)?;
                }
                if let Some(complete) = &task.complete {
                    budget.string(complete, 256)?;
                    if !valid_completion(complete) {
                        return Err(bad("task completion is outside the supported fixed-percentage profile"));
                    }
                }
                for author in &task.authors {
                    budget.string(&author.name, MAX_SCALAR)?;
                    budget.string(&author.initials, MAX_SCALAR)?;
                    if author.identity.wire_guid.as_ref().is_some_and(|guid| !valid_guid(guid)) {
                        return Err(bad("invalid task author wire GUID"));
                    }
                    if author.identity.id == 0 {
                        return Err(bad("zero task author identity"));
                    }
                    for value in [
                        &author.identity.external_id,
                        &author.identity.user_id,
                        &author.identity.provider_id,
                        &author.identity.source_initials,
                        &author.identity.source_scope,
                        &author.identity.wire_guid,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        budget.string(value, MAX_SCALAR)?;
                    }
                }
            }
            budget.string(&c.date, 256)?;
            if !c.date.is_empty() && !crate::datetime::valid_date_time(&c.date) {
                return Err(bad("invalid creation timestamp"));
            }
            if let Some(label) = &c.display_date {
                budget.string(label, MAX_SCALAR)?;
            }
            if !c.x.is_finite() || !c.y.is_finite() || c.x.abs() > 1.0e6 || c.y.abs() > 1.0e6 {
                return Err(bad("invalid comment position"));
            }
            if c.comment_id == Some(0) {
                return Err(bad("zero message identity"));
            }
            if c.status.is_some_and(|status| c.resolved != (status != CommentStatus::Active)) {
                return Err(bad("status conflicts with legacy resolved state"));
            }
            if let Some(a) = &c.author_identity {
                if a.wire_guid.as_ref().is_some_and(|guid| !valid_guid(guid)) {
                    return Err(bad("invalid author wire GUID"));
                }
                if a.id == 0 {
                    return Err(bad("zero author identity"));
                }
                for value in [&a.external_id, &a.user_id, &a.provider_id, &a.source_initials, &a.source_scope, &a.wire_guid].into_iter().flatten() {
                    budget.string(value, MAX_SCALAR)?;
                }
            }
            if let Some(source) = &c.source_id {
                match source {
                    CommentSourceId::ModernPptx { guid } => {
                        if !valid_guid(guid) {
                            return Err(bad("invalid source comment GUID"));
                        }
                        budget.string(guid, MAX_SCALAR)?;
                    }
                    CommentSourceId::LegacyPptx { author, .. } => budget.string(author, MAX_SCALAR)?,
                    CommentSourceId::Odf { name } => budget.string(name, MAX_SCALAR)?,
                }
            }
            if c.attachments.len() > MAX_ATTACHMENTS {
                return Err(bad("attachment count exceeds limit"));
            }
            budget.charge(c.attachments.len().saturating_mul(std::mem::size_of::<CommentAttachment>()).saturating_mul(2), c.attachments.len())?;
            for attachment in &c.attachments {
                if attachment.id == 0 {
                    return Err(bad("zero attachment identity"));
                }
                budget.string(&attachment.name, MAX_SCALAR)?;
            }
            walk(&c.replies, depth + 1, budget)?;
        }
        Ok(())
    }
    walk(items, 0, budget)
}

/// Stable path used by commands/UI without exposing vector indices as identity.
pub fn path(items: &[Comment], id: u32) -> Option<Vec<usize>> {
    fn walk(items: &[Comment], id: u32, depth: usize, path: &mut Vec<usize>) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        for (i, c) in items.iter().enumerate() {
            path.push(i);
            if c.comment_id == Some(id) || walk(&c.replies, id, depth + 1, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut found = Vec::new();
    if walk(items, id, 0, &mut found) { Some(found) } else { None }
}
pub fn at<'a>(items: &'a [Comment], path: &[usize]) -> Option<&'a Comment> {
    if path.len() > MAX_DEPTH + 1 {
        return None;
    }
    let (first, rest) = path.split_first()?;
    let c = items.get(*first)?;
    if rest.is_empty() { Some(c) } else { at(&c.replies, rest) }
}
pub fn at_mut<'a>(items: &'a mut [Comment], path: &[usize]) -> Option<&'a mut Comment> {
    if path.len() > MAX_DEPTH + 1 {
        return None;
    }
    let (first, rest) = path.split_first()?;
    let c = items.get_mut(*first)?;
    if rest.is_empty() { Some(c) } else { at_mut(&mut c.replies, rest) }
}
pub fn remove(items: &mut Vec<Comment>, path: &[usize]) -> Result<Comment> {
    if path.len() > MAX_DEPTH + 1 {
        return Err(bad("message path exceeds depth limit"));
    }
    let (first, rest) = path.split_first().ok_or_else(|| bad("missing message path"))?;
    if rest.is_empty() {
        if *first >= items.len() {
            return Err(bad("message no longer exists"));
        }
        Ok(items.remove(*first))
    } else {
        remove(&mut items.get_mut(*first).ok_or_else(|| bad("message no longer exists"))?.replies, rest)
    }
}

struct Identities<'a> {
    messages: BTreeSet<u32>,
    attachments: BTreeSet<u32>,
    authors: BTreeMap<u32, (&'a CommentAuthorIdentity, &'a str, &'a str)>,
}
impl<'a> Identities<'a> {
    fn new() -> Self {
        Self { messages: BTreeSet::new(), attachments: BTreeSet::new(), authors: BTreeMap::new() }
    }
    fn message(&mut self, c: &'a Comment) -> Result<()> {
        if let Some(id) = c.comment_id
            && !self.messages.insert(id)
        {
            return Err(bad("duplicate message identity"));
        }
        if let Some(identity) = &c.author_identity {
            let value = (identity, c.author.as_str(), c.initials.as_str());
            if let Some(previous) = self.authors.get(&identity.id) {
                if previous != &value {
                    return Err(bad("one author identity has conflicting metadata"));
                }
            } else {
                self.authors.insert(identity.id, value);
            }
        }
        if let Some(task) = &c.task {
            for author in &task.authors {
                let value = (&author.identity, author.name.as_str(), author.initials.as_str());
                if let Some(previous) = self.authors.get(&author.identity.id) {
                    if previous != &value {
                        return Err(bad("one task author identity has conflicting metadata"));
                    }
                } else {
                    self.authors.insert(author.identity.id, value);
                }
            }
        }
        for attachment in &c.attachments {
            if !self.attachments.insert(attachment.id) {
                return Err(bad("duplicate attachment identity"));
            }
        }
        for r in &c.replies {
            self.message(r)?;
        }
        Ok(())
    }
}

fn owner_references(doc: &Presentation, slide: usize, items: &[Comment], require_bytes: bool, budget: &mut Budget) -> Result<()> {
    let owner = doc.slides.get(slide).ok_or_else(|| bad("missing slide owner"))?;
    // Build one owner-local ID set, avoiding a shape-tree search for every message.
    fn shape_ids<'a>(
        items: &'a [crate::Shape],
        depth: usize,
        count: &mut usize,
        ids: &mut BTreeMap<ShapeId, &'a crate::Shape>,
        budget: &mut Budget,
    ) -> Result<()> {
        if depth > 64 {
            return Err(bad("shape owner exceeds depth limit"));
        }
        for shape in items {
            *count = count.checked_add(1).ok_or_else(|| bad("shape count overflow"))?;
            if *count > MAX_MESSAGES {
                return Err(bad("shape owner exceeds count limit"));
            }
            budget.charge(64, 1)?;
            if ids.insert(shape.id, shape).is_some() {
                return Err(bad("ambiguous shape owner identity"));
            }
            shape_ids(shape.children(), depth + 1, count, ids, budget)?;
        }
        Ok(())
    }
    let mut shapes = BTreeMap::new();
    shape_ids(&owner.shapes, 0, &mut 0, &mut shapes, budget)?;
    budget.charge(doc.media.len().saturating_mul(64), doc.media.len())?;
    let media: BTreeMap<_, _> = doc.media.iter().map(|media| (media.id, media)).collect();
    fn walk(
        items: &[Comment],
        shapes: &BTreeMap<ShapeId, &crate::Shape>,
        media: &BTreeMap<MediaId, &crate::MediaItem>,
        require_bytes: bool,
    ) -> Result<()> {
        for c in items {
            if c.shape.is_some_and(|id| !shapes.contains_key(&id)) {
                return Err(bad("shape attachment is outside its slide owner"));
            }
            if let CommentAnchor::TextRange { shape, start, end } = c.comment_anchor() {
                let body = shapes.get(&shape).and_then(|shape| shape.text.as_ref()).ok_or_else(|| bad("text range has no owning text body"))?;
                let text = checked_body_text(body)?;
                let length = u32::try_from(text.chars().count()).map_err(|_| bad("text owner length overflow"))?;
                if start > end || end > length {
                    return Err(bad("text range is outside owning shape text"));
                }
            }
            let mut referenced: BTreeSet<MediaId> = c.attachments.iter().map(|a| a.media).collect();
            if let Some(body) = &c.body {
                resources::visit::body(body, &mut |id| {
                    referenced.insert(*id);
                });
            }
            for id in referenced {
                let item = media.get(&id).ok_or_else(|| bad("attachment media is missing"))?;
                if item.content_type.is_empty()
                    || item.content_type.len() > MAX_SCALAR
                    || item.name.len() > MAX_SCALAR
                    || item.content_type.chars().any(char::is_control)
                    || item.data.len() > MAX_ATTACHMENT_BYTES
                {
                    return Err(bad("attachment media has invalid declared metadata"));
                }
                if item.link.is_some() || (require_bytes && item.data.is_empty()) {
                    return Err(bad("attachments require actual embedded bytes; linked media is unsupported"));
                }
            }
            walk(&c.replies, shapes, media, require_bytes)?;
        }
        Ok(())
    }
    walk(items, &shapes, &media, require_bytes)
}

pub fn validate(doc: &Presentation, require_bytes: bool) -> Result<()> {
    let mut budget = Budget::default();
    for slide in &doc.slides {
        validate_metadata(&slide.comments, &mut budget)?;
    }
    if doc.slides.iter().all(|slide| slide.comments.is_empty()) {
        return Ok(());
    }
    if doc.media.len() > MAX_MESSAGES || doc.slides.len() > MAX_ROOTS {
        return Err(bad("owner/media count exceeds limit"));
    }
    fn used(items: &[Comment], out: &mut BTreeSet<MediaId>) {
        for c in items {
            for a in &c.attachments {
                out.insert(a.media);
            }
            if let Some(body) = &c.body {
                resources::visit::body(body, &mut |id| {
                    out.insert(*id);
                });
            }
            used(&c.replies, out);
        }
    }
    let mut referenced = BTreeSet::new();
    for slide in &doc.slides {
        used(&slide.comments, &mut referenced);
    }
    budget.charge(referenced.len().saturating_mul(64), referenced.len())?;
    let mut seen = BTreeSet::new();
    let mut total_bytes = 0usize;
    for media in &doc.media {
        if referenced.contains(&media.id) {
            if !seen.insert(media.id) {
                return Err(bad("attachment media identity is ambiguous"));
            }
            total_bytes = total_bytes.checked_add(media.data.len()).ok_or_else(|| bad("attachment byte total overflow"))?;
            if total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
                return Err(bad("attachment byte total exceeds limit"));
            }
        }
    }
    let mut identities = Identities::new();
    for (i, slide) in doc.slides.iter().enumerate() {
        for c in &slide.comments {
            identities.message(c)?;
        }
        if !slide.comments.is_empty() {
            owner_references(doc, i, &slide.comments, require_bytes, &mut budget)?;
        }
    }
    fn tasks(items: &[Comment], authors: &BTreeMap<u32, (&CommentAuthorIdentity, &str, &str)>) -> Result<()> {
        for c in items {
            if let Some(task) = &c.task {
                for id in &task.assigned_to {
                    if !authors.contains_key(id) {
                        return Err(bad("task assignee identity is missing"));
                    }
                }
            }
            tasks(&c.replies, authors)?;
        }
        Ok(())
    }
    for slide in &doc.slides {
        tasks(&slide.comments, &identities.authors)?;
    }
    Ok(())
}

/// Assign missing local IDs without inferring a missing timestamp or foreign identity.
/// Call in an existing staging/undo transaction; errors must not publish partial migration.
pub fn normalize(doc: &mut Presentation) -> Result<()> {
    validate(doc, false)?;
    doc.fix_next_id();
    fn maximum(items: &[Comment], max: &mut u32) {
        for c in items {
            *max = (*max).max(c.comment_id.unwrap_or(0)).max(c.author_identity.as_ref().map_or(0, |a| a.id));
            if let Some(task) = &c.task {
                for author in &task.authors {
                    *max = (*max).max(author.identity.id);
                }
            }
            for attachment in &c.attachments {
                *max = (*max).max(attachment.id);
            }
            maximum(&c.replies, max);
        }
    }
    let mut max = doc.next_id;
    for slide in &doc.slides {
        maximum(&slide.comments, &mut max);
    }
    doc.next_id = max;
    fn alloc(next: &mut u32) -> Result<u32> {
        *next = (*next).max(1).checked_add(1).ok_or_else(|| bad("identity space exhausted"))?;
        Ok(*next)
    }
    fn visit(items: &mut [Comment], next: &mut u32) -> Result<()> {
        for c in items {
            if c.comment_id.is_none() {
                c.comment_id = Some(alloc(next)?);
            }
            if c.author_identity.is_none() {
                c.author_identity = Some(CommentAuthorIdentity {
                    id: alloc(next)?,
                    origin: AuthorOrigin::Unknown,
                    external_id: None,
                    user_id: None,
                    provider_id: None,
                    source_initials: None,
                    source_scope: None,
                    wire_guid: None,
                });
            }
            visit(&mut c.replies, next)?;
        }
        Ok(())
    }
    fn needs_ids(items: &[Comment]) -> bool {
        items.iter().any(|c| c.comment_id.is_none() || c.author_identity.is_none() || needs_ids(&c.replies))
    }
    for slide in &mut doc.slides {
        if needs_ids(&slide.comments) {
            visit(&mut Arc::make_mut(slide).comments, &mut doc.next_id)?;
        }
    }
    validate(doc, false)
}

/// Fresh message/attachment IDs; explicit maps preserve owner-local anchors/media and shared authors.
pub fn remap(
    items: &mut [Comment],
    next_id: &mut u32,
    shapes: &BTreeMap<ShapeId, ShapeId>,
    media: &BTreeMap<MediaId, MediaId>,
    authors: &mut BTreeMap<u32, u32>,
) -> Result<()> {
    validate_metadata(items, &mut Budget::default())?;
    fn alloc(next: &mut u32) -> Result<u32> {
        *next = (*next).max(1).checked_add(1).ok_or_else(|| bad("identity space exhausted"))?;
        Ok(*next)
    }
    fn walk(
        items: &mut [Comment],
        next: &mut u32,
        shapes: &BTreeMap<ShapeId, ShapeId>,
        media: &BTreeMap<MediaId, MediaId>,
        authors: &mut BTreeMap<u32, u32>,
    ) -> Result<()> {
        for c in items {
            c.comment_id = Some(alloc(next)?);
            // Keep source aliases as provenance; the copied message needs a fresh wire identity.
            c.wire_guid = None;
            if let Some(identity) = &mut c.author_identity {
                let id = match authors.get(&identity.id) {
                    Some(id) => *id,
                    None => {
                        let id = alloc(next)?;
                        authors.insert(identity.id, id);
                        id
                    }
                };
                if identity.id != id {
                    identity.wire_guid = None;
                }
                identity.id = id;
            }
            if let Some(task) = &mut c.task {
                for author in &mut task.authors {
                    let old = author.identity.id;
                    let id = match authors.get(&old) {
                        Some(id) => *id,
                        None => {
                            let id = alloc(next)?;
                            authors.insert(old, id);
                            id
                        }
                    };
                    if author.identity.id != id {
                        author.identity.wire_guid = None;
                    }
                    author.identity.id = id;
                }
                for assigned in &mut task.assigned_to {
                    let old = *assigned;
                    *assigned = match authors.get(&old) {
                        Some(id) => *id,
                        None => {
                            let id = alloc(next)?;
                            authors.insert(old, id);
                            id
                        }
                    };
                }
            }
            if let Some(shape) = c.shape {
                let mapped = *shapes.get(&shape).ok_or_else(|| bad("copied comment is missing its shape owner"))?;
                {
                    let anchor = match c.comment_anchor() {
                        CommentAnchor::TextRange { start, end, .. } => CommentAnchor::TextRange { shape: mapped, start, end },
                        _ => CommentAnchor::Shape { shape: mapped },
                    };
                    c.set_comment_anchor(anchor);
                }
            }
            if let Some(body) = &mut c.body {
                let mut missing = false;
                resources::visit_mut::body(body, &mut |id| {
                    if let Some(new) = media.get(id) {
                        *id = *new;
                    } else {
                        missing = true;
                    }
                });
                if missing {
                    return Err(bad("copied rich comment is missing its media closure"));
                }
            }
            for attachment in &mut c.attachments {
                attachment.id = alloc(next)?;
                attachment.media = *media.get(&attachment.media).ok_or_else(|| bad("copied attachment is missing its media closure"))?;
            }
            walk(&mut c.replies, next, shapes, media, authors)?;
        }
        Ok(())
    }
    walk(items, next_id, shapes, media, authors)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionAuthor {
    pub key: String,
    pub name: String,
    pub initials: Option<String>,
    pub user_id: Option<String>,
    pub provider_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionTask {
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub assigned_to: Vec<String>,
    pub complete: Option<String>,
    pub title: Option<String>,
}

/// Explicit visible format facts. Codec owners build this from actual emitted/parsed parts.
/// Strings retain lexical timestamps/coordinates/anchor descriptors, including absence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionMessage {
    pub key: String,
    pub author_key: String,
    pub author: String,
    pub initials: Option<String>,
    pub text: String,
    pub body: Option<crate::text::TextBody>,
    /// Bounded format canonical rich-body projection, freshly regenerated on restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_xml: Option<String>,
    pub task: Option<ProjectionTask>,
    pub date: Option<String>,
    pub display_date: Option<String>,
    pub status: Option<String>,
    pub anchor: Vec<String>,
    pub x: Option<String>,
    pub y: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionOwner {
    pub key: String,
    pub present: bool,
    pub authors: Vec<ProjectionAuthor>,
    pub messages: Vec<ProjectionMessage>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentProjection {
    pub profile: String,
    pub owners: Vec<ProjectionOwner>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionOwner {
    pub key: String,
    pub projection_owners: Vec<String>,
    pub comments: Vec<Comment>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionMedia {
    pub id: MediaId,
    pub part: String,
    pub content_type: String,
    pub size: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentExtension {
    pub version: u32,
    pub projection: CommentProjection,
    pub owners: Vec<ExtensionOwner>,
    pub media: Vec<ExtensionMedia>,
    /// Local native shape aliases bound to independently verified format wire owners.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shape_bindings: BTreeMap<String, BTreeMap<ShapeId, String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slide_bindings: BTreeMap<SlideId, String>,
}

pub fn validate_projection(projection: &CommentProjection, budget: &mut Budget) -> Result<()> {
    budget.string(&projection.profile, MAX_SCALAR)?;
    if projection.owners.len() > MAX_ROOTS.saturating_mul(2) {
        return Err(bad("projection owner count exceeds limit"));
    }
    let mut owners = BTreeSet::new();
    let mut count = 0usize;
    for owner in &projection.owners {
        budget.string(&owner.key, MAX_SCALAR)?;
        if !owners.insert(&owner.key) {
            return Err(bad("duplicate projection owner"));
        }
        if !owner.present && !owner.messages.is_empty() {
            return Err(bad("absent projection part has messages"));
        }
        if owner.authors.len() > MAX_ROOTS {
            return Err(bad("projection author count exceeds limit"));
        }
        let mut authors = BTreeMap::new();
        for author in &owner.authors {
            budget.string(&author.key, MAX_SCALAR)?;
            budget.string(&author.name, MAX_SCALAR)?;
            for value in [&author.initials, &author.user_id, &author.provider_id].into_iter().flatten() {
                budget.string(value, MAX_SCALAR)?;
            }
            if authors.insert(&author.key, author).is_some() {
                return Err(bad("duplicate projection author identity"));
            }
        }
        let mut keys = BTreeSet::new();
        for message in &owner.messages {
            count = count.checked_add(1).ok_or_else(|| bad("projection count overflow"))?;
            if count > MAX_MESSAGES.saturating_mul(2) {
                return Err(bad("projection message count exceeds limit"));
            }
            for value in [&message.key, &message.author_key, &message.author] {
                budget.string(value, MAX_SCALAR)?;
            }
            if !keys.insert(&message.key) {
                return Err(bad("duplicate projection message key"));
            }
            let author = authors.get(&message.author_key).ok_or_else(|| bad("projection message author relationship missing"))?;
            if author.name != message.author || author.initials != message.initials {
                return Err(bad("projection author snapshot conflicts with relationship"));
            }
            budget.string(&message.text, MAX_TEXT)?;
            if let Some(body) = &message.body {
                if checked_body_text(body)? != message.text {
                    return Err(bad("projection rich body conflicts with text"));
                }
                body_bytes(body, budget)?;
            }
            if let Some(xml) = &message.body_xml {
                budget.string(xml, MAX_TEXT * 8)?;
            }
            if let Some(task) = &message.task {
                for value in [&task.start_date, &task.due_date, &task.complete, &task.title].into_iter().flatten() {
                    budget.string(value, MAX_SCALAR)?;
                }
                if task.assigned_to.len() > MAX_ATTACHMENTS {
                    return Err(bad("projection assignee count exceeds limit"));
                }
                for assignee in &task.assigned_to {
                    budget.string(assignee, MAX_SCALAR)?;
                    if !authors.contains_key(assignee) {
                        return Err(bad("projection task author relationship missing"));
                    }
                }
            }
            for value in [&message.initials, &message.display_date, &message.status, &message.x, &message.y].into_iter().flatten() {
                budget.string(value, MAX_SCALAR)?;
            }
            if let Some(date) = &message.date {
                budget.string(date, 256)?;
            }
            if message.anchor.len() > MAX_ATTACHMENTS {
                return Err(bad("projection anchor count exceeds limit"));
            }
            for anchor in &message.anchor {
                budget.string(anchor, MAX_SCALAR)?;
            }
        }
    }
    Ok(())
}

impl CommentExtension {
    /// Capture after codec projection, using exactly the package's owner/media bindings.
    pub fn capture(
        doc: &Presentation,
        projection: CommentProjection,
        bindings: &[(String, usize, Vec<String>)],
        media: Vec<ExtensionMedia>,
    ) -> Result<Self> {
        validate(doc, true)?;
        if bindings.len() > MAX_ROOTS {
            return Err(bad("extension owner count exceeds limit"));
        }
        let mut budget = Budget::default();
        validate_projection(&projection, &mut budget)?;
        let mut keys = BTreeSet::new();
        let mut indices = BTreeSet::new();
        for (key, index, projections) in bindings {
            budget.string(key, MAX_SCALAR)?;
            if !keys.insert(key) || !indices.insert(index) {
                return Err(bad("capture owner binding is not one-to-one"));
            }
            if projections.is_empty() || projections.len() > 2 {
                return Err(bad("capture needs one or two visible projection owners"));
            }
            for key in projections {
                budget.string(key, MAX_SCALAR)?;
            }
            let slide = doc.slides.get(*index).ok_or_else(|| bad("capture slide owner missing"))?;
            validate_metadata(&slide.comments, &mut budget)?;
        }
        let mut owners = Vec::new();
        owners.try_reserve_exact(bindings.len()).map_err(|_| bad("cannot allocate extension owners"))?;
        for (key, index, projection_owners) in bindings {
            let slide = doc.slides.get(*index).ok_or_else(|| bad("capture slide owner missing"))?;
            owners.push(ExtensionOwner { key: key.clone(), projection_owners: projection_owners.clone(), comments: slide.comments.clone() });
        }
        for binding in &media {
            let item = doc.media(binding.id).ok_or_else(|| bad("capture attachment media missing"))?;
            if item.link.is_some() || item.data.is_empty() || item.data.len() as u64 != binding.size || item.content_type != binding.content_type {
                return Err(bad("capture attachment declaration differs from actual bytes"));
            }
        }
        let value = Self { version: 1, projection, owners, media, shape_bindings: BTreeMap::new(), slide_bindings: BTreeMap::new() };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(bad("unsupported extension version"));
        }
        let mut budget = Budget::default();
        validate_projection(&self.projection, &mut budget)?;
        if self.owners.len() > MAX_ROOTS || self.media.len() > MAX_MESSAGES {
            return Err(bad("extension owner/media count exceeds limit"));
        }
        let mut owners = BTreeSet::new();
        let mut covered = BTreeSet::new();
        let projections: BTreeMap<_, _> = self.projection.owners.iter().map(|owner| (&owner.key, owner)).collect();
        let mut identities = Identities::new();
        for owner in &self.owners {
            budget.string(&owner.key, MAX_SCALAR)?;
            if !owners.insert(&owner.key) {
                return Err(bad("duplicate extension owner"));
            }
            validate_metadata(&owner.comments, &mut budget)?;
            for c in &owner.comments {
                identities.message(c)?;
            }
            if owner.projection_owners.is_empty() || owner.projection_owners.len() > 2 {
                return Err(bad("extension owner needs one or two visible projections"));
            }
            fn flatten<'a>(items: &'a [Comment], out: &mut Vec<&'a Comment>) {
                for c in items {
                    out.push(c);
                    flatten(&c.replies, out);
                }
            }
            let mut flat = Vec::new();
            flatten(&owner.comments, &mut flat);
            for key in &owner.projection_owners {
                budget.string(key, MAX_SCALAR)?;
                if !covered.insert(key) {
                    return Err(bad("visible projection has more than one native owner"));
                }
                let projection = projections.get(key).ok_or_else(|| bad("native owner has no visible projection"))?;
                if flat.len() != projection.messages.len() {
                    return Err(bad("native/visible message relationship is not one-to-one"));
                }
                // The standard format may reorder flat annotations and normalize legal XML
                // text (e.g. legacy PPTX soft breaks). Only the format-specific mandatory
                // fresh regeneration below can compare those semantics authoritatively.
            }
        }
        if self.shape_bindings.len() > MAX_ROOTS {
            return Err(bad("shape binding owner count exceeds limit"));
        }
        if self.slide_bindings.len() > MAX_ROOTS {
            return Err(bad("slide bindings exceed limit"));
        }
        let mut slide_wires = BTreeSet::new();
        for wire in self.slide_bindings.values() {
            budget.string(wire, MAX_SCALAR)?;
            if wire.is_empty() || !slide_wires.insert(wire) {
                return Err(bad("slide wire bindings ambiguous"));
            }
        }
        for (owner, bindings) in &self.shape_bindings {
            budget.string(owner, MAX_SCALAR)?;
            if !owners.contains(owner) || bindings.len() > MAX_MESSAGES {
                return Err(bad("shape bindings have no unique native owner or exceed limit"));
            }
            let mut wires = BTreeSet::new();
            for wire in bindings.values() {
                budget.string(wire, MAX_SCALAR)?;
                if wire.is_empty() || !wires.insert(wire) {
                    return Err(bad("shape wire bindings are empty or ambiguous"));
                }
            }
        }
        fn tasks(items: &[Comment], authors: &BTreeMap<u32, (&CommentAuthorIdentity, &str, &str)>) -> Result<()> {
            for c in items {
                if let Some(task) = &c.task {
                    for id in &task.assigned_to {
                        if !authors.contains_key(id) {
                            return Err(bad("extension task assignee identity missing"));
                        }
                    }
                }
                tasks(&c.replies, authors)?;
            }
            Ok(())
        }
        for owner in &self.owners {
            tasks(&owner.comments, &identities.authors)?;
        }
        if covered.len() != projections.len() {
            return Err(bad("visible projection has an unbound owner"));
        }
        let mut mids = BTreeSet::new();
        let mut attachment_bytes = 0u64;
        let mut parts = BTreeSet::new();
        for media in &self.media {
            if !mids.insert(media.id) || !parts.insert(&media.part) {
                return Err(bad("duplicate extension media binding"));
            }
            budget.string(&media.part, MAX_SCALAR)?;
            budget.string(&media.content_type, MAX_SCALAR)?;
            if media.part.is_empty()
                || media.part.starts_with('/')
                || media.part.contains('\\')
                || media.part.split('/').any(|p| p.is_empty() || p == "." || p == "..")
                || media.part.contains(':')
            {
                return Err(bad("unsafe extension media part path"));
            }
            attachment_bytes = attachment_bytes.checked_add(media.size).ok_or_else(|| bad("attachment total overflow"))?;
            if attachment_bytes > MAX_TOTAL_ATTACHMENT_BYTES as u64 {
                return Err(bad("attachment total exceeds limit"));
            }
            if media.content_type.is_empty() || media.size == 0 || media.size > MAX_ATTACHMENT_BYTES as u64 {
                return Err(bad("invalid extension media declaration"));
            }
        }
        fn references(items: &[Comment], used: &mut BTreeSet<MediaId>) {
            for c in items {
                for a in &c.attachments {
                    used.insert(a.media);
                }
                if let Some(body) = &c.body {
                    resources::visit::body(body, &mut |id| {
                        used.insert(*id);
                    });
                }
                references(&c.replies, used);
            }
        }
        let mut used = BTreeSet::new();
        for owner in &self.owners {
            references(&owner.comments, &mut used);
        }
        if used != mids {
            return Err(bad("attachment media declarations are not an exact referenced closure"));
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        struct Buffer(Vec<u8>);
        impl std::io::Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                let len = self.0.len().checked_add(bytes.len()).ok_or_else(|| std::io::Error::other("comment extension size overflow"))?;
                if len > MAX_EXTENSION_BYTES {
                    return Err(std::io::Error::other("comment extension exceeds byte limit"));
                }
                self.0.try_reserve(bytes.len()).map_err(|_| std::io::Error::other("cannot allocate comment extension"))?;
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut buffer = Buffer(Vec::new());
        serde_json::to_writer(&mut buffer, self).map_err(|_| bad("cannot encode comment extension"))?;
        Ok(buffer.0)
    }
    /// Input preflight must run before serde buffering. Lexical byte/token/depth limits below
    /// are deliberately independent of the subsequent typed semantic validation.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_EXTENSION_BYTES {
            return Err(bad("comment extension exceeds byte limit"));
        }
        preflight_json(bytes)?;
        let value: Self = serde_json::from_slice(bytes).map_err(|_| bad("malformed comment extension"))?;
        value.validate()?;
        Ok(value)
    }
    /// Restore only after exact one-to-one comparison to the actual standard projection.
    /// `owners` maps extension slide keys to destination indices; `media` is built only from
    /// successfully loaded declared embedded parts (including MIME/size checks by the codec).
    pub fn restore<F>(
        &self,
        actual: &CommentProjection,
        doc: &mut Presentation,
        owners: &BTreeMap<String, usize>,
        media: &BTreeMap<MediaId, MediaId>,
        shapes: &BTreeMap<String, BTreeMap<ShapeId, ShapeId>>,
        regenerate: F,
    ) -> Result<()>
    where
        F: FnOnce(&[ExtensionOwner], &[ExtensionMedia]) -> Result<CommentProjection>,
    {
        self.validate()?;
        // A payload-supplied witness alone is not authority. The codec reconstructs all
        // core-representable facts independently from typed native payload + verified IDs.
        let expected = regenerate(&self.owners, &self.media)?;
        validate_projection(&expected, &mut Budget::default())?;
        if expected != self.projection {
            return Err(bad("comment extension payload does not regenerate its standard projection"));
        }
        validate_projection(actual, &mut Budget::default())?;
        if &self.projection != actual {
            return Err(bad("stale comment extension: actual standard projection differs"));
        }
        if owners.len() != self.owners.len() {
            return Err(bad("extension slide ownership is not one-to-one"));
        }
        let mut destinations = BTreeSet::new();
        for owner in &self.owners {
            let destination = *owners.get(&owner.key).ok_or_else(|| bad("extension slide owner missing"))?;
            if !destinations.insert(destination) || doc.slides.get(destination).is_none() {
                return Err(bad("ambiguous extension slide owner"));
            }
        }
        for binding in &self.media {
            let id = media.get(&binding.id).ok_or_else(|| bad("extension attachment media binding missing"))?;
            let item = doc.media(*id).ok_or_else(|| bad("extension attachment actual media missing"))?;
            if item.link.is_some() || item.content_type != binding.content_type || item.data.len() as u64 != binding.size || item.data.is_empty() {
                return Err(bad("extension attachment actual bytes do not match declaration"));
            }
        }
        // Stage only bounded comment owners. Never publish partially restored state.
        let mut staged = Vec::new();
        staged.try_reserve_exact(self.owners.len()).map_err(|_| bad("cannot allocate restored owners"))?;
        fn map_refs(items: &mut [Comment], media: &BTreeMap<MediaId, MediaId>, shapes: Option<&BTreeMap<ShapeId, ShapeId>>) -> Result<()> {
            for c in items {
                if let Some(id) = c.shape {
                    let mapped = *shapes.and_then(|map| map.get(&id)).ok_or_else(|| bad("shape attachment has no verified owner-local mapping"))?;
                    let anchor = match c.comment_anchor() {
                        CommentAnchor::TextRange { start, end, .. } => CommentAnchor::TextRange { shape: mapped, start, end },
                        _ => CommentAnchor::Shape { shape: mapped },
                    };
                    c.set_comment_anchor(anchor);
                }
                for a in &mut c.attachments {
                    a.media = *media.get(&a.media).ok_or_else(|| bad("attachment has no verified media binding"))?;
                }
                if let Some(body) = &mut c.body {
                    let mut missing = false;
                    resources::visit_mut::body(body, &mut |id| {
                        if let Some(new) = media.get(id) {
                            *id = *new;
                        } else {
                            missing = true;
                        }
                    });
                    if missing {
                        return Err(bad("rich body media has no verified binding"));
                    }
                }
                map_refs(&mut c.replies, media, shapes)?;
            }
            Ok(())
        }
        let mut reference_budget = Budget::default();
        for owner in &self.owners {
            let destination = *owners.get(&owner.key).ok_or_else(|| bad("extension slide owner missing"))?;
            let mut comments = owner.comments.clone();
            map_refs(&mut comments, media, shapes.get(&owner.key))?;
            owner_references(doc, destination, &comments, true, &mut reference_budget)?;
            staged.push((destination, comments));
        }
        let staged_index: BTreeMap<_, _> = staged.iter().map(|(index, comments)| (*index, comments)).collect();
        let mut identities = Identities::new();
        for (index, slide) in doc.slides.iter().enumerate() {
            let comments = staged_index.get(&index).map_or(slide.comments.as_slice(), |comments| comments.as_slice());
            for c in comments {
                identities.message(c)?;
            }
        }
        for (destination, comments) in staged {
            Arc::make_mut(doc.slides.get_mut(destination).ok_or_else(|| bad("extension slide owner missing"))?).comments = comments;
        }
        Ok(())
    }
}

/// Streaming serde walk avoids tagged-enum Content allocation until all input is bounded.
fn preflight_json(bytes: &[u8]) -> Result<()> {
    use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
    use std::fmt;
    struct Scan<'a> {
        budget: &'a mut Budget,
        depth: usize,
    }
    impl<'de> DeserializeSeed<'de> for Scan<'_> {
        type Value = ();
        fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
            if self.depth > 96 {
                return Err(serde::de::Error::custom("comment extension nesting exceeds limit"));
            }
            self.budget.charge(128, 1).map_err(serde::de::Error::custom)?;
            d.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for Scan<'_> {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("bounded comment extension JSON")
        }
        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<(), E> {
            Ok(())
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<(), E> {
            self.budget.string(value, MAX_TEXT * 8).map_err(E::custom)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
            while seq.next_element_seed(Scan { budget: self.budget, depth: self.depth + 1 })?.is_some() {}
            Ok(())
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
            while let Some(key) = map.next_key::<String>()? {
                self.budget.string(&key, MAX_SCALAR).map_err(serde::de::Error::custom)?;
                map.next_value_seed(Scan { budget: self.budget, depth: self.depth + 1 })?;
            }
            Ok(())
        }
    }
    // Limit the parser's unescape scratch before invoking it.
    let mut at = 0usize;
    while let Some(byte) = bytes.get(at) {
        if *byte != b'"' {
            at += 1;
            continue;
        }
        at += 1;
        let start = at;
        loop {
            match bytes.get(at) {
                Some(b'"') => break,
                Some(b'\\') => at = at.checked_add(2).ok_or_else(|| bad("JSON token overflow"))?,
                Some(_) => at += 1,
                None => return Err(bad("unterminated JSON token")),
            }
            if at.saturating_sub(start) > MAX_TEXT * 8 {
                return Err(bad("extension JSON token exceeds limit"));
            }
        }
        at += 1;
    }
    let mut budget = Budget::default();
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    Scan { budget: &mut budget, depth: 0 }.deserialize(&mut parser).map_err(|_| bad("extension JSON resource limit or syntax error"))?;
    parser.end().map_err(|_| bad("trailing extension JSON"))?;
    Ok(())
}

#[cfg(test)]
#[path = "comments_tests.rs"]
mod tests;
