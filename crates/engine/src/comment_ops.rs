//! Actual comment mutations shared by UI, CLI, control and MCP commands.
use std::sync::Arc;

use deckcraft_model::{
    Comment, MediaId, Presentation, ShapeId, SlideId,
    comments::{self, AuthorOrigin, CommentAnchor, CommentAttachment, CommentAuthorIdentity, CommentStatus},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Result, Session, cmd::bad};

fn model(error: comments::CommentError) -> crate::EngineError {
    crate::EngineError::Other(error.to_string())
}
fn number(p: &Value, key: &str, command: &str) -> Result<Option<u32>> {
    match p.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|id| u32::try_from(id).ok())
            .filter(|id| *id != 0)
            .map(Some)
            .ok_or_else(|| bad(command, format!("invalid `{key}`"))),
    }
}
fn index(p: &Value, key: &str, command: &str) -> Result<Option<usize>> {
    match p.get(key) {
        None => Ok(None),
        Some(value) => value.as_u64().and_then(|v| usize::try_from(v).ok()).map(Some).ok_or_else(|| bad(command, format!("invalid `{key}`"))),
    }
}
fn string<'a>(p: &'a Value, key: &str, command: &str, required: bool, limit: usize) -> Result<Option<&'a str>> {
    match p.get(key) {
        None if !required => Ok(None),
        Some(Value::String(value)) if value.len() <= limit => Ok(Some(value)),
        _ => Err(bad(command, format!("missing, invalid or over-limit `{key}`"))),
    }
}
fn alloc(doc: &mut Presentation, command: &str) -> Result<u32> {
    doc.alloc_id().ok_or_else(|| bad(command, "identity space exhausted"))
}

fn owner(doc: &Presentation, current: usize, p: &Value, command: &str) -> Result<usize> {
    let stable = number(p, "slideId", command)?.map(SlideId);
    let legacy = index(p, "slide", command)?;
    let owner = if let Some(id) = stable { doc.slide_index(id).ok_or_else(|| bad(command, "no such slide ID"))? } else { legacy.unwrap_or(current) };
    if stable.is_some() && legacy.is_some_and(|index| index != owner) {
        return Err(bad(command, "slide selectors disagree"));
    }
    if doc.slides.get(owner).is_none() {
        return Err(bad(command, "missing slide owner"));
    }
    Ok(owner)
}

/// Resolve a stable message ID globally. Legacy root indices remain owner-local.
fn target(doc: &Presentation, current: usize, p: &Value, command: &str) -> Result<(usize, Vec<usize>)> {
    let slide_id = number(p, "slideId", command)?.map(SlideId);
    let supplied_slide = index(p, "slide", command)?;
    let owner = if let Some(id) = slide_id {
        doc.slide_index(id).ok_or_else(|| bad(command, "no such slide ID"))?
    } else {
        supplied_slide.unwrap_or(current)
    };
    if let Some(i) = supplied_slide
        && slide_id.is_some()
        && i != owner
    {
        return Err(bad(command, "slide selectors disagree"));
    }
    let legacy = index(p, "index", command)?;
    if let Some(id) = number(p, "commentId", command)? {
        let mut found = None;
        for (si, slide) in doc.slides.iter().enumerate() {
            if let Some(path) = comments::path(&slide.comments, id) {
                if found.is_some() {
                    return Err(bad(command, "ambiguous comment identity"));
                }
                found = Some((si, path));
            }
        }
        let found = found.ok_or_else(|| bad(command, "no such comment ID"))?;
        if (slide_id.is_some() || supplied_slide.is_some()) && found.0 != owner {
            return Err(bad(command, "comment does not belong to supplied slide"));
        }
        if legacy.is_some_and(|i| found.1.as_slice() != [i]) {
            return Err(bad(command, "comment selectors disagree"));
        }
        Ok(found)
    } else {
        let i = legacy.ok_or_else(|| bad(command, "missing `commentId` or `index`"))?;
        if doc.slides.get(owner).and_then(|sl| sl.comments.get(i)).is_none() {
            return Err(bad(command, "no such comment"));
        }
        Ok((owner, vec![i]))
    }
}

fn author(doc: &mut Presentation, p: &Value, name: &str, command: &str) -> Result<(String, String, CommentAuthorIdentity)> {
    if name.len() > comments::MAX_SCALAR {
        return Err(bad(command, "author preference exceeds size limit"));
    }
    if let Some(id) = number(p, "authorId", command)? {
        fn find(items: &[Comment], id: u32) -> Option<&Comment> {
            for c in items {
                if c.author_identity.as_ref().is_some_and(|a| a.id == id) {
                    return Some(c);
                }
                if let Some(c) = find(&c.replies, id) {
                    return Some(c);
                }
            }
            None
        }
        let c = doc.slides.iter().find_map(|sl| find(&sl.comments, id)).ok_or_else(|| bad(command, "no such author identity"))?;
        let identity = c.author_identity.clone().ok_or_else(|| bad(command, "author identity missing"))?;
        return Ok((c.author.clone(), c.initials.clone(), identity));
    }
    // A new local identity claims no external account/provider. A display name alone never
    // links it to a same-named imported author; explicit authorId establishes reuse.
    let initials: String = name.split_whitespace().filter_map(|word| word.chars().next()).take(32).collect();
    let identity = CommentAuthorIdentity {
        id: alloc(doc, command)?,
        origin: AuthorOrigin::Local,
        external_id: None,
        user_id: None,
        provider_id: None,
        source_initials: Some(initials.clone()),
        source_scope: None,
        wire_guid: Some(crate::comment_identity::fresh_guid()?),
    };
    Ok((name.to_string(), initials, identity))
}

fn selected_shape(doc: &Presentation, owner: usize, p: &Value, fallback: Option<ShapeId>, command: &str) -> Result<Option<ShapeId>> {
    let shape_id = number(p, "shape", command)?;
    let legacy_id = number(p, "id", command)?;
    if shape_id.is_some() && legacy_id.is_some() && shape_id != legacy_id {
        return Err(bad(command, "shape selectors disagree"));
    }
    let explicit = shape_id.or(legacy_id).map(ShapeId);
    let shape = explicit.or(fallback);
    if let Some(id) = shape {
        let slide = doc.slides.get(owner).ok_or_else(|| bad(command, "missing slide"))?;
        if slide.shape(id).is_none() {
            return Err(bad(command, "shape attachment is outside its slide owner"));
        }
    }
    Ok(shape)
}

fn desired_anchor(doc: &Presentation, owner: usize, p: &Value, fallback: Option<ShapeId>, command: &str) -> Result<CommentAnchor> {
    if let Some(value) = p.get("anchor") {
        let object = value.as_object().ok_or_else(|| bad(command, "anchor must be an object"))?;
        if object.len() > 4 {
            return Err(bad(command, "unknown anchor fields"));
        }
        let kind = object.get("kind").and_then(Value::as_str).ok_or_else(|| bad(command, "missing anchor kind"))?;
        let shape = || -> Result<ShapeId> {
            Ok(ShapeId(
                object
                    .get("shape")
                    .and_then(Value::as_u64)
                    .and_then(|id| u32::try_from(id).ok())
                    .filter(|id| *id != 0)
                    .ok_or_else(|| bad(command, "invalid anchor shape"))?,
            ))
        };
        let scalar = |key: &str| -> Result<u32> {
            object.get(key).and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or_else(|| bad(command, "invalid scalar range position"))
        };
        let anchor = match kind {
            "slide" => CommentAnchor::Slide,
            "unknown" => CommentAnchor::Unknown,
            "shape" => CommentAnchor::Shape { shape: shape()? },
            "textRange" => CommentAnchor::TextRange { shape: shape()?, start: scalar("start")?, end: scalar("end")? },
            _ => return Err(bad(command, "unknown anchor kind")),
        };
        if object.keys().any(|key| !matches!(key.as_str(), "kind" | "shape" | "start" | "end")) {
            return Err(bad(command, "unknown anchor field"));
        }
        if let Some(shape) = anchor.shape()
            && doc.slides.get(owner).and_then(|slide| slide.shape(shape)).is_none()
        {
            return Err(bad(command, "anchor shape is outside its slide owner"));
        }
        if p.get("shape").is_some() || p.get("id").is_some() {
            let legacy = selected_shape(doc, owner, p, None, command)?;
            if legacy != anchor.shape() {
                return Err(bad(command, "anchor and shape selectors disagree"));
            }
        }
        Ok(anchor)
    } else {
        Ok(selected_shape(doc, owner, p, fallback, command)?.map_or(CommentAnchor::Slide, |shape| CommentAnchor::Shape { shape }))
    }
}

pub(crate) fn mutate(s: &mut Session, p: &Value, command: &str, date: &str) -> Result<Value> {
    for key in ["all", "reopen"] {
        if let Some(value) = p.get(key)
            && value.as_bool().is_none()
        {
            return Err(bad(command, format!("invalid `{key}`")));
        }
    }
    let task = if command == "comment.task.set" {
        let task = p.get("task").ok_or_else(|| bad(command, "missing task"))?;
        if task.is_null() {
            Some(None)
        } else {
            for key in ["assignedTo", "authors"] {
                if task.get(key).and_then(Value::as_array).is_some_and(|items| items.len() > comments::MAX_ROOTS) {
                    return Err(bad(command, "task author count exceeds limit"));
                }
            }
            let task = deckcraft_model::comments::CommentTask::deserialize(task).map_err(|_| bad(command, "invalid task record"))?;
            Some(Some(task))
        }
    } else {
        None
    };
    let name = s.prefs.author.clone();
    let body = p.get("body").map(comments::parse_body).transpose().map_err(model)?;
    let text = string(p, "text", command, matches!(command, "comment.add" | "comment.reply" | "comment.edit") && body.is_none(), comments::MAX_TEXT)?
        .map(str::to_string);
    let text = if let Some(body) = &body {
        let plain = comments::checked_body_text(body).map_err(model)?;
        if text.as_ref().is_some_and(|text| text != &plain) {
            return Err(bad(command, "body and plain text disagree"));
        }
        Some(plain)
    } else {
        text
    };
    if !date.is_empty() && !deckcraft_model::datetime::valid_date_time(date) {
        return Err(bad(command, "invalid current timestamp"));
    }
    // Native metadata graph validation precedes Arc::make_mut and any recursive clone.
    comments::validate(&s.doc()?.doc, false).map_err(model)?;
    s.edit_resources(|doc, sel| {
        comments::normalize(doc).map_err(model)?;
        if command == "comment.delete" && p.get("all").is_some_and(|v| v == true) {
            for slide in &mut doc.slides {
                Arc::make_mut(slide).comments.clear();
            }
            return Ok(json!({"deleted":"all"}));
        }
        if command == "comment.delete" && p.get("commentId").is_none() && p.get("index").is_none() {
            let owner = owner(doc, sel.slide, p, command)?;
            Arc::make_mut(doc.slides.get_mut(owner).ok_or_else(|| bad(command, "missing slide"))?).comments.clear();
            return Ok(json!({"slide":owner}));
        }
        if command == "comment.add" {
            let owner = owner(doc, sel.slide, p, command)?;
            let anchor = desired_anchor(doc, owner, p, if owner == sel.slide { sel.shapes.first().copied() } else { None }, command)?;
            let (author, initials, identity) = author(doc, p, &name, command)?;
            let comment_id = alloc(doc, command)?;
            let coordinate = |key: &str| -> Result<f64> {
                match p.get(key) {
                    None => Ok(10.0),
                    Some(value) => {
                        value.as_f64().filter(|v| v.is_finite() && v.abs() <= 1.0e6).ok_or_else(|| bad(command, "invalid comment position"))
                    }
                }
            };
            let c = Comment {
                author,
                initials,
                text: text.clone().ok_or_else(|| bad(command, "missing text"))?,
                date: date.into(),
                x: coordinate("x")?,
                y: coordinate("y")?,
                comment_id: Some(comment_id),
                wire_guid: Some(crate::comment_identity::fresh_guid()?),
                author_identity: Some(identity),
                status: Some(CommentStatus::Active),
                shape: anchor.shape(),
                anchor: Some(anchor),
                body: body.clone(),
                ..Default::default()
            };
            let slide = Arc::make_mut(doc.slides.get_mut(owner).ok_or_else(|| bad(command, "missing slide"))?);
            if slide.comments.len() >= comments::MAX_ROOTS {
                return Err(bad(command, "root count exceeds slide limit"));
            }
            slide.comments.try_reserve(1).map_err(|_| bad(command, "cannot allocate comment"))?;
            slide.comments.push(c);
            let result = json!({"commentId":comment_id,"slideId":slide.id.0,"slide":owner,"index":slide.comments.len()-1});
            comments::validate(doc, true).map_err(model)?;
            return Ok(result);
        }
        let (owner, mut path) = target(doc, sel.slide, p, command)?;
        if command == "comment.reply" {
            if let Some(reply_to) = number(p, "replyTo", command)? {
                let candidate = comments::path(&doc.slides.get(owner).ok_or_else(|| bad(command, "missing slide"))?.comments, reply_to)
                    .ok_or_else(|| bad(command, "replyTo is outside target thread"))?;
                if candidate.first() != path.first() {
                    return Err(bad(command, "replyTo is outside target thread"));
                }
                path = candidate;
            }
            if path.len() > comments::MAX_DEPTH {
                return Err(bad(command, "reply depth exceeds limit"));
            }
            let (author, initials, identity) = author(doc, p, &name, command)?;
            let comment_id = alloc(doc, command)?;
            let slide = Arc::make_mut(doc.slides.get_mut(owner).ok_or_else(|| bad(command, "missing slide"))?);
            let root_index = *path.first().ok_or_else(|| bad(command, "missing thread"))?;
            if slide.comments.get(root_index).is_some_and(|root| root.comment_status() != CommentStatus::Active) {
                if p.get("reopen").is_some_and(|v| v == true) {
                    slide.comments.get_mut(root_index).ok_or_else(|| bad(command, "missing root"))?.set_comment_status(CommentStatus::Active);
                } else {
                    return Err(bad(command, "reopen the thread before replying, or supply reopen:true"));
                }
            }
            let parent = comments::at_mut(&mut slide.comments, &path).ok_or_else(|| bad(command, "missing parent"))?;
            parent.replies.try_reserve(1).map_err(|_| bad(command, "cannot allocate reply"))?;
            parent.replies.push(Comment {
                author,
                initials,
                text: text.clone().ok_or_else(|| bad(command, "missing text"))?,
                date: date.into(),
                body: body.clone(),
                comment_id: Some(comment_id),
                wire_guid: Some(crate::comment_identity::fresh_guid()?),
                author_identity: Some(identity),
                status: Some(CommentStatus::Active),
                ..Default::default()
            });
            let result = json!({"commentId":comment_id,"slideId":slide.id.0,"slide":owner,"index":root_index});
            comments::validate(doc, true).map_err(model)?;
            return Ok(result);
        }
        if command == "comment.delete" {
            let slide = Arc::make_mut(doc.slides.get_mut(owner).ok_or_else(|| bad(command, "missing slide"))?);
            let c = comments::remove(&mut slide.comments, &path).map_err(model)?;
            return Ok(json!({"commentId":c.comment_id,"slideId":slide.id.0,"slide":owner}));
        }
        let attachment = if command == "comment.attachment.add" {
            let media_id = MediaId(number(p, "mediaId", command)?.ok_or_else(|| bad(command, "missing mediaId"))?);
            let media = doc.media(media_id).ok_or_else(|| bad(command, "attachment media is missing"))?;
            if media.data.is_empty() || media.link.is_some() || media.content_type.is_empty() {
                return Err(bad(command, "attachment requires embedded bytes and declared MIME type"));
            }
            let name = string(p, "name", command, false, comments::MAX_SCALAR)?.unwrap_or(&media.name).to_string();
            Some(CommentAttachment { id: alloc(doc, command)?, media: media_id, name })
        } else {
            None
        };
        let anchor = if command == "comment.attach" {
            if p.get("anchor").is_none() && p.get("shape").is_none() && p.get("id").is_none() {
                return Err(bad(command, "missing anchor or shape"));
            }
            Some(desired_anchor(doc, owner, p, None, command)?)
        } else {
            None
        };
        let slide = Arc::make_mut(doc.slides.get_mut(owner).ok_or_else(|| bad(command, "missing slide"))?);
        let c = comments::at_mut(&mut slide.comments, &path).ok_or_else(|| bad(command, "missing comment"))?;
        let mut attachment_id = None;
        match command {
            "comment.task.set" => c.task = task.clone().ok_or_else(|| bad(command, "missing task"))?,
            "comment.edit" => {
                let text = text.clone().ok_or_else(|| bad(command, "missing text"))?;
                if let Some(body) = &body {
                    c.body = Some(body.clone());
                } else if let Some(body) = &mut c.body {
                    deckcraft_model::edit::replace_text(body, &text);
                }
                c.text = text;
            }
            "comment.resolve" => {
                let status = if let Some(status) = p.get("status") {
                    match status.as_str() {
                        Some("active") => CommentStatus::Active,
                        Some("resolved") => CommentStatus::Resolved,
                        Some("closed") => CommentStatus::Closed,
                        _ => return Err(bad(command, "invalid status")),
                    }
                } else {
                    match p.get("resolved") {
                        None | Some(Value::Bool(true)) => CommentStatus::Resolved,
                        Some(Value::Bool(false)) => CommentStatus::Active,
                        _ => return Err(bad(command, "invalid resolved state")),
                    }
                };
                c.set_comment_status(status);
            }
            "comment.reopen" => c.set_comment_status(CommentStatus::Active),
            "comment.close" => c.set_comment_status(CommentStatus::Closed),
            "comment.attach" => c.set_comment_anchor(anchor.ok_or_else(|| bad(command, "missing anchor"))?),
            "comment.detach" => c.set_comment_anchor(CommentAnchor::Slide),
            "comment.attachment.add" => {
                if c.attachments.len() >= comments::MAX_ATTACHMENTS {
                    return Err(bad(command, "attachment count exceeds limit"));
                }
                let attachment = attachment.ok_or_else(|| bad(command, "missing attachment"))?;
                attachment_id = Some(attachment.id);
                c.attachments.try_reserve(1).map_err(|_| bad(command, "cannot allocate attachment"))?;
                c.attachments.push(attachment);
            }
            "comment.attachment.remove" => {
                let id = number(p, "attachmentId", command)?.ok_or_else(|| bad(command, "missing attachmentId"))?;
                let index = c.attachments.iter().position(|a| a.id == id).ok_or_else(|| bad(command, "no such attachment"))?;
                c.attachments.remove(index);
                attachment_id = Some(id);
            }
            _ => return Err(bad(command, "unknown comment operation")),
        }
        let result = json!({"commentId":c.comment_id,"slideId":slide.id.0,"slide":owner,"attachmentId":attachment_id});
        comments::validate(doc, true).map_err(model)?;
        Ok(result)
    })
}

pub(crate) fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let doc = &s.doc()?.doc;
    comments::validate(doc, false).map_err(model)?;
    let owner = owner(doc, s.doc()?.selection.slide, p, "comment.list")?;
    let slide = doc.slides.get(owner).ok_or_else(|| bad("comment.list", "no such slide"))?;
    if p.get("commentId").is_some() {
        let (owner, path) = target(doc, owner, p, "comment.list")?;
        let c = comments::at(&doc.slides.get(owner).ok_or_else(|| bad("comment.list", "missing slide"))?.comments, &path)
            .ok_or_else(|| bad("comment.list", "missing comment"))?;
        serde_json::to_value(c).map_err(|_| bad("comment.list", "cannot serialize comment"))
    } else {
        serde_json::to_value(&slide.comments).map_err(|_| bad("comment.list", "cannot serialize comments"))
    }
}

#[cfg(test)]
#[path = "comment_ops_tests.rs"]
mod tests;

/// Import one actual attachment in the same history transaction as its reference.
pub(crate) fn import_attachment(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "comment.attachment.import";
    comments::validate(&s.doc()?.doc, true).map_err(model)?;
    target(&s.doc()?.doc, s.doc()?.selection.slide, p, CMD)?;
    let encoded = string(p, "data", CMD, true, (comments::MAX_ATTACHMENT_BYTES / 3 + 1) * 4)?.ok_or_else(|| bad(CMD, "missing attachment bytes"))?;
    let bytes = crate::cmd::base64_decode(encoded)
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= comments::MAX_ATTACHMENT_BYTES)
        .ok_or_else(|| bad(CMD, "invalid or oversized attachment bytes"))?;
    let name = string(p, "name", CMD, true, comments::MAX_SCALAR)?.ok_or_else(|| bad(CMD, "missing filename"))?.to_string();
    let mime = string(p, "contentType", CMD, false, comments::MAX_SCALAR)?.unwrap_or("application/octet-stream").to_string();
    s.edit_resources(|doc, sel| {
        comments::normalize(doc).map_err(model)?;
        let (owner, path) = target(doc, sel.slide, p, CMD)?;
        let media = doc.add_media(&name, &mime, bytes).ok_or_else(|| bad(CMD, "cannot allocate attachment media"))?;
        let attachment = CommentAttachment { id: alloc(doc, CMD)?, media, name };
        let id = attachment.id;
        let slide = Arc::make_mut(doc.slides.get_mut(owner).ok_or_else(|| bad(CMD, "missing slide"))?);
        let comment = comments::at_mut(&mut slide.comments, &path).ok_or_else(|| bad(CMD, "missing comment"))?;
        if comment.attachments.len() >= comments::MAX_ATTACHMENTS {
            return Err(bad(CMD, "attachment count exceeds limit"));
        }
        comment.attachments.push(attachment);
        let result = json!({"commentId":comment.comment_id,"attachmentId":id});
        comments::validate(doc, true).map_err(model)?;
        Ok(result)
    })
}
pub(crate) fn get_attachment(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "comment.attachment.get";
    let doc = &s.doc()?.doc;
    comments::validate(doc, true).map_err(model)?;
    let (owner, path) = target(doc, s.doc()?.selection.slide, p, CMD)?;
    let id = number(p, "attachmentId", CMD)?.ok_or_else(|| bad(CMD, "missing attachmentId"))?;
    let attachment = doc
        .slides
        .get(owner)
        .and_then(|slide| comments::at(&slide.comments, &path))
        .and_then(|comment| comment.attachments.iter().find(|attachment| attachment.id == id))
        .ok_or_else(|| bad(CMD, "attachment does not belong to the comment"))?;
    let media = doc.media(attachment.media).ok_or_else(|| bad(CMD, "attachment bytes are missing"))?;
    Ok(json!({"name":attachment.name,"contentType":media.content_type,"data":crate::cmd::base64_encode(&media.data)}))
}
