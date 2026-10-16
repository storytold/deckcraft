//! Bounded comment closure capture and identity-safe copy/reuse/review transfer.
use crate::{EngineError, Result};
use deckcraft_model::{
    Comment, MediaId, Presentation, ShapeId, Slide, SlideId,
    comments::{self, CommentAuthorIdentity, CommentTaskAuthor},
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
fn err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("comment transfer: {e}"))
}
#[derive(Default)]
pub(crate) struct Context {
    authors: BTreeMap<u32, u32>,
    wire: BTreeMap<u32, String>,
}
/// Complete referenced-only task author records before source ownership is discarded. Account
/// metadata is copied only from a validated explicit relationship, never guessed from a name.
pub(crate) struct Capture<'a> {
    authors: BTreeMap<u32, (&'a CommentAuthorIdentity, &'a str, &'a str)>,
}
impl<'a> Capture<'a> {
    pub fn new(source: &'a Presentation) -> Result<Self> {
        comments::validate(source, true).map_err(err)?;
        let mut authors = BTreeMap::new();
        fn index<'a>(items: &'a [Comment], out: &mut BTreeMap<u32, (&'a CommentAuthorIdentity, &'a str, &'a str)>) {
            for c in items {
                if let Some(a) = &c.author_identity {
                    out.insert(a.id, (a, &c.author, &c.initials));
                }
                if let Some(task) = &c.task {
                    for a in &task.authors {
                        out.insert(a.identity.id, (&a.identity, &a.name, &a.initials));
                    }
                }
                index(&c.replies, out);
            }
        }
        for slide in &source.slides {
            index(&slide.comments, &mut authors);
        }
        Ok(Self { authors })
    }
    pub fn enrich(&self, slides: &mut [Slide]) -> Result<()> {
        fn tasks(items: &mut [Comment], authors: &BTreeMap<u32, (&CommentAuthorIdentity, &str, &str)>) -> Result<()> {
            for c in items {
                if let Some(task) = &mut c.task {
                    for id in &task.assigned_to {
                        if !task.authors.iter().any(|a| a.identity.id == *id) {
                            let (a, name, initials) = authors.get(id).ok_or_else(|| err("assignee identity is outside source scope"))?;
                            task.authors.push(CommentTaskAuthor { identity: (**a).clone(), name: (*name).into(), initials: (*initials).into() });
                        }
                    }
                }
                tasks(&mut c.replies, authors)?;
            }
            Ok(())
        }
        let mut budget = comments::Budget::default();
        for slide in slides {
            tasks(&mut slide.comments, &self.authors)?;
            comments::validate_metadata(&slide.comments, &mut budget).map_err(err)?;
        }
        Ok(())
    }
}
pub(crate) fn capture(source: &Presentation, slides: &mut [Slide]) -> Result<()> {
    Capture::new(source)?.enrich(slides)
}
/// Call before any generic media walker rewrites the copied comments. The supplied maps must
/// include unchanged identities too, and must come from actual staged owners/resources.
pub(crate) fn remap(
    doc: &mut Presentation,
    items: &mut [Comment],
    shapes: &BTreeMap<ShapeId, ShapeId>,
    media: &BTreeMap<MediaId, MediaId>,
    slides: &BTreeMap<SlideId, SlideId>,
    context: &mut Context,
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    comments::remap(items, &mut doc.next_id, shapes, media, &mut context.authors).map_err(err)?;
    comments::resources::map_slides(items, slides).map_err(err)?;
    fn wire(items: &mut [Comment], ids: &mut BTreeMap<u32, String>) -> Result<()> {
        for c in items {
            c.wire_guid = Some(crate::comment_identity::fresh_guid()?);
            if let Some(a) = &mut c.author_identity {
                let value = if let Some(value) = ids.get(&a.id) {
                    value.clone()
                } else {
                    let value = crate::comment_identity::fresh_guid()?;
                    ids.insert(a.id, value.clone());
                    value
                };
                a.wire_guid = Some(value);
            }
            if let Some(task) = &mut c.task {
                for a in &mut task.authors {
                    let value = if let Some(value) = ids.get(&a.identity.id) {
                        value.clone()
                    } else {
                        let value = crate::comment_identity::fresh_guid()?;
                        ids.insert(a.identity.id, value.clone());
                        value
                    };
                    a.identity.wire_guid = Some(value);
                }
            }
            wire(&mut c.replies, ids)?;
        }
        Ok(())
    }
    wire(items, &mut context.wire)
}
pub(crate) fn identity_media(items: &[Comment], changed: &HashMap<MediaId, MediaId>) -> BTreeMap<MediaId, MediaId> {
    let mut map = BTreeMap::new();
    comments::resources::visit::comments(items, &mut |id| {
        map.insert(*id, changed.get(id).copied().unwrap_or(*id));
    });
    map
}
pub(crate) fn identity_shapes(shapes: &[deckcraft_model::Shape], changed: &HashMap<ShapeId, ShapeId>) -> BTreeMap<ShapeId, ShapeId> {
    let mut map = BTreeMap::new();
    deckcraft_model::walk(shapes, &mut |shape, _| {
        map.insert(shape.id, changed.get(&shape.id).copied().unwrap_or(shape.id));
    });
    map
}
pub(crate) fn identity_slides(doc: &Presentation) -> BTreeMap<SlideId, SlideId> {
    doc.slides.iter().map(|slide| (slide.id, slide.id)).collect()
}
/// One comparison operation owns the correspondence. IDs are joined only by matched
/// message/task roles on actually paired slides, never by display names or numeric coincidence.
/// Ambiguous splits/merges deliberately receive distinct canonical identities.
pub(crate) struct Comparison<'a> {
    current: Capture<'a>,
    revised: Capture<'a>,
    current_authors: BTreeMap<u32, u32>,
    revised_authors: BTreeMap<u32, u32>,
    current_slides: BTreeMap<SlideId, SlideId>,
    revised_slides: BTreeMap<SlideId, SlideId>,
    current_media: BTreeMap<MediaId, MediaId>,
    revised_media: BTreeMap<MediaId, MediaId>,
}
impl<'a> Comparison<'a> {
    pub fn new(current: &'a Presentation, revised: &'a Presentation, pairs: &[(usize, usize)]) -> Result<Self> {
        let acapture = Capture::new(current)?;
        let bcapture = Capture::new(revised)?;
        let mut forward = BTreeMap::new();
        let mut reverse = BTreeMap::new();
        fn relate(a: u32, b: u32, forward: &mut BTreeMap<u32, Option<u32>>, reverse: &mut BTreeMap<u32, Option<u32>>) {
            forward
                .entry(a)
                .and_modify(|old| {
                    if *old != Some(b) {
                        *old = None;
                    }
                })
                .or_insert(Some(b));
            reverse
                .entry(b)
                .and_modify(|old| {
                    if *old != Some(a) {
                        *old = None;
                    }
                })
                .or_insert(Some(a));
        }
        fn roles(a: &[Comment], b: &[Comment], forward: &mut BTreeMap<u32, Option<u32>>, reverse: &mut BTreeMap<u32, Option<u32>>) {
            for (a, b) in a.iter().zip(b) {
                if let (Some(a), Some(b)) = (&a.author_identity, &b.author_identity) {
                    relate(a.id, b.id, forward, reverse);
                }
                if let (Some(a), Some(b)) = (&a.task, &b.task) {
                    for (a, b) in a.assigned_to.iter().zip(&b.assigned_to) {
                        relate(*a, *b, forward, reverse);
                    }
                    for (a, b) in a.authors.iter().zip(&b.authors) {
                        relate(a.identity.id, b.identity.id, forward, reverse);
                    }
                }
                roles(&a.replies, &b.replies, forward, reverse);
            }
        }
        for (a, b) in pairs {
            let a = current.slides.get(*a).ok_or_else(|| err("matched current slide is missing"))?;
            let b = revised.slides.get(*b).ok_or_else(|| err("matched revised slide is missing"))?;
            roles(&a.comments, &b.comments, &mut forward, &mut reverse);
        }
        fn fresh(next: &mut u32) -> Result<u32> {
            *next = next.checked_add(1).ok_or_else(|| err("comparison identity count overflow"))?;
            Ok(*next)
        }
        let mut next = 0;
        let mut current_authors = BTreeMap::new();
        for id in acapture.authors.keys() {
            current_authors.insert(*id, fresh(&mut next)?);
        }
        let mut revised_authors = BTreeMap::new();
        for id in bcapture.authors.keys() {
            let matched = reverse.get(id).copied().flatten().filter(|a| forward.get(a) == Some(&Some(*id)));
            let canonical = if let Some(a) = matched {
                *current_authors.get(&a).ok_or_else(|| err("matched author has no actual identity"))?
            } else {
                fresh(&mut next)?
            };
            revised_authors.insert(*id, canonical);
        }
        let mut current_slides = BTreeMap::new();
        for slide in &current.slides {
            if current_slides.insert(slide.id, SlideId(fresh(&mut next)?)).is_some() {
                return Err(err("current slide ownership is ambiguous"));
            }
        }
        let mut revised_slides = BTreeMap::new();
        for (a, b) in pairs {
            let a = current.slides.get(*a).ok_or_else(|| err("matched current slide is missing"))?;
            let b = revised.slides.get(*b).ok_or_else(|| err("matched revised slide is missing"))?;
            let id = *current_slides.get(&a.id).ok_or_else(|| err("matched slide has no actual identity"))?;
            if revised_slides.insert(b.id, id).is_some() {
                return Err(err("matched revised slide ownership is ambiguous"));
            }
        }
        let mut actual_revised = BTreeSet::new();
        for slide in &revised.slides {
            if !actual_revised.insert(slide.id) {
                return Err(err("revised slide ownership is ambiguous"));
            }
            if let std::collections::btree_map::Entry::Vacant(e) = revised_slides.entry(slide.id) {
                e.insert(SlideId(fresh(&mut next)?));
            }
        }
        // Hash each actual referenced resource once for the whole operation. A shared
        // attachment on many slides must not rehash its potentially large bytes per slide.
        fn media<'a>(p: &'a Presentation, facts: &mut HashMap<(&'a str, &'a [u8]), MediaId>, next: &mut u32) -> Result<BTreeMap<MediaId, MediaId>> {
            let mut used = BTreeSet::new();
            for slide in &p.slides {
                comments::resources::visit::comments(&slide.comments, &mut |id| {
                    used.insert(*id);
                });
            }
            let mut map = BTreeMap::new();
            for item in p.media.iter().filter(|item| used.contains(&item.id)) {
                if item.link.is_some() || item.data.is_empty() {
                    return Err(err("comparison needs actual embedded resource bytes"));
                }
                let key = (item.content_type.as_str(), item.data.as_slice());
                let canonical = if let Some(id) = facts.get(&key) {
                    *id
                } else {
                    let id = MediaId(fresh(next)?);
                    facts.insert(key, id);
                    id
                };
                map.insert(item.id, canonical);
            }
            if map.len() != used.len() {
                return Err(err("comparison resource is missing or ambiguous"));
            }
            Ok(map)
        }
        let mut facts = HashMap::new();
        let current_media = media(current, &mut facts, &mut next)?;
        let revised_media = media(revised, &mut facts, &mut next)?;
        Ok(Self {
            current: acapture,
            revised: bcapture,
            current_authors,
            revised_authors,
            current_slides,
            revised_slides,
            current_media,
            revised_media,
        })
    }
}
/// Compare logical relationships and real resource bytes while ignoring newly generated local
/// aliases/wire GUIDs. Shared author topology, all statuses/body/task/attachments and reply parents
/// still participate. Actual matched shape owners establish anchor equivalence.
pub(crate) fn equivalent(
    a: &[Comment],
    b: &[Comment],
    comparison: &Comparison<'_>,
    mut same_shape: impl FnMut(ShapeId, ShapeId) -> bool,
) -> Result<bool> {
    comments::validate_metadata(a, &mut comments::Budget::default()).map_err(err)?;
    comments::validate_metadata(b, &mut comments::Budget::default()).map_err(err)?;
    fn anchors(a: &[Comment], b: &[Comment], same: &mut impl FnMut(ShapeId, ShapeId) -> bool) -> bool {
        if a.len() != b.len() {
            return false;
        }
        for (a, b) in a.iter().zip(b) {
            use comments::CommentAnchor::*;
            let equal = match (a.comment_anchor(), b.comment_anchor()) {
                (Slide, Slide) | (Unknown, Unknown) => true,
                (Shape { shape: a }, Shape { shape: b }) => same(a, b),
                (TextRange { shape: a, start: as_, end: ae }, TextRange { shape: b, start: bs, end: be }) => as_ == bs && ae == be && same(a, b),
                _ => false,
            };
            if !equal || !anchors(&a.replies, &b.replies, same) {
                return false;
            }
        }
        true
    }
    let mut ac = Slide { comments: a.to_vec(), ..Default::default() };
    let mut bc = Slide { comments: b.to_vec(), ..Default::default() };
    comparison.current.enrich(std::slice::from_mut(&mut ac))?;
    comparison.revised.enrich(std::slice::from_mut(&mut bc))?;
    let a = ac.comments.as_slice();
    let b = bc.comments.as_slice();
    if !anchors(a, b, &mut same_shape) {
        return Ok(false);
    }
    fn normalized(
        items: &[Comment],
        media: &BTreeMap<MediaId, MediaId>,
        authors: &BTreeMap<u32, u32>,
        slides: &BTreeMap<SlideId, SlideId>,
    ) -> Result<Vec<Comment>> {
        let mut out = items.to_vec();
        comments::resources::map_media(&mut out, media).map_err(err)?;
        comments::resources::map_slides(&mut out, slides).map_err(err)?;
        fn id(value: &mut CommentAuthorIdentity, authors: &BTreeMap<u32, u32>) -> Result<()> {
            value.id = *authors.get(&value.id).ok_or_else(|| err("comparison author has no operation-level identity"))?;
            value.wire_guid = None;
            Ok(())
        }
        fn walk(items: &mut [Comment], authors: &BTreeMap<u32, u32>) -> Result<()> {
            for c in items {
                c.comment_id = None;
                c.wire_guid = None;
                c.set_comment_status(c.comment_status());
                c.set_comment_anchor(comments::CommentAnchor::Slide);
                if let Some(a) = &mut c.author_identity {
                    id(a, authors)?;
                }
                if let Some(task) = &mut c.task {
                    for a in &mut task.authors {
                        id(&mut a.identity, authors)?;
                    }
                    for assigned in &mut task.assigned_to {
                        *assigned = *authors.get(assigned).ok_or_else(|| err("comparison assignee has no operation-level identity"))?;
                    }
                }
                for (i, a) in c.attachments.iter_mut().enumerate() {
                    a.id = u32::try_from(i).map_err(err)?;
                }
                walk(&mut c.replies, authors)?;
            }
            Ok(())
        }
        walk(&mut out, authors)?;
        Ok(out)
    }
    Ok(normalized(a, &comparison.current_media, &comparison.current_authors, &comparison.current_slides)?
        == normalized(b, &comparison.revised_media, &comparison.revised_authors, &comparison.revised_slides)?)
}

#[cfg(test)]
#[path = "comment_transfer_tests.rs"]
mod tests;
