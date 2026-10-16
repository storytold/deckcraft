//! Keep annotation owners and Unicode scalar ranges synchronized with real edits.
use crate::{EngineError, Result};
use deckcraft_model::comments::{self, CommentAnchor};
use deckcraft_model::{Comment, Presentation, ShapeId, SlideId};
use std::collections::HashMap;
use std::sync::Arc;

struct Owner {
    anchors: HashMap<u32, CommentAnchor>,
    text: HashMap<ShapeId, String>,
}
pub(crate) struct Snapshot(HashMap<SlideId, Owner>);
fn error(e: comments::CommentError) -> EngineError {
    EngineError::Other(e.to_string())
}
impl Snapshot {
    pub(crate) fn capture(doc: &Presentation) -> Result<Self> {
        comments::validate(doc, true).map_err(error)?;
        let mut owners = HashMap::new();
        for slide in &doc.slides {
            if slide.comments.is_empty() {
                continue;
            }
            let mut owner = Owner { anchors: HashMap::new(), text: HashMap::new() };
            let mut pending: Vec<&Comment> = slide.comments.iter().collect();
            while let Some(comment) = pending.pop() {
                if let Some(id) = comment.comment_id {
                    owner.anchors.insert(id, comment.comment_anchor());
                }
                if let CommentAnchor::TextRange { shape, .. } = comment.comment_anchor()
                    && let Some(body) = deckcraft_model::find_shape(&slide.shapes, shape).and_then(|shape| shape.text.as_ref())
                {
                    owner.text.entry(shape).or_insert_with(|| body.text());
                }
                pending.extend(&comment.replies);
            }
            owners.insert(slide.id, owner);
        }
        Ok(Self(owners))
    }
    pub(crate) fn reconcile(&self, doc: &mut Presentation) -> Result<()> {
        for slide in &mut doc.slides {
            if slide.comments.is_empty() {
                continue;
            }
            let Some(old) = self.0.get(&slide.id) else { continue };
            let slide = Arc::make_mut(slide);
            let mut shapes = std::collections::BTreeSet::new();
            deckcraft_model::walk(&slide.shapes, &mut |shape, _| {
                shapes.insert(shape.id);
            });
            let mut changes = HashMap::new();
            for (id, before) in &old.text {
                if let Some(after) = deckcraft_model::find_shape(&slide.shapes, *id).and_then(|shape| shape.text.as_ref()).map(|body| body.text())
                    && before != &after
                {
                    changes.insert(*id, Difference::new(before, &after)?);
                }
            }
            fn apply(
                items: &mut [Comment],
                old: &Owner,
                shapes: &std::collections::BTreeSet<ShapeId>,
                changes: &HashMap<ShapeId, Difference>,
                depth: usize,
            ) {
                if depth > comments::MAX_DEPTH {
                    return;
                }
                for comment in items {
                    let anchor = comment.comment_anchor();
                    if anchor.shape().is_some_and(|id| !shapes.contains(&id)) {
                        comment.set_comment_anchor(CommentAnchor::Slide);
                    } else if comment.comment_id.and_then(|id| old.anchors.get(&id)).is_some_and(|prior| *prior == anchor)
                        && let CommentAnchor::TextRange { shape, start, end } = anchor
                        && let Some(change) = changes.get(&shape)
                    {
                        let (start, end) = change.range(start, end);
                        comment.set_comment_anchor(CommentAnchor::TextRange { shape, start, end });
                    }
                    apply(&mut comment.replies, old, shapes, changes, depth + 1);
                }
            }
            apply(&mut slide.comments, old, &shapes, &changes, 0);
        }
        comments::validate(doc, true).map_err(error)
    }
}
struct Difference {
    start: u32,
    old_end: u32,
    new_end: u32,
}
impl Difference {
    fn new(before: &str, after: &str) -> Result<Self> {
        let prefix = before.chars().zip(after.chars()).take_while(|(a, b)| a == b).count();
        let old_len = before.chars().count();
        let new_len = after.chars().count();
        let suffix = before
            .chars()
            .rev()
            .take(old_len.saturating_sub(prefix))
            .zip(after.chars().rev().take(new_len.saturating_sub(prefix)))
            .take_while(|(a, b)| a == b)
            .count();
        let checked = |value| u32::try_from(value).map_err(|_| EngineError::Other("comment text owner exceeds scalar range".into()));
        Ok(Self { start: checked(prefix)?, old_end: checked(old_len - suffix)?, new_end: checked(new_len - suffix)? })
    }
    fn range(&self, start: u32, end: u32) -> (u32, u32) {
        let shifted = |position: u32| self.new_end.saturating_add(position.saturating_sub(self.old_end));
        if self.start == self.old_end {
            let first = if start >= self.start { shifted(start) } else { start };
            let last = if end > self.start || start == end && end == self.start { shifted(end) } else { end };
            return (first, last.max(first));
        }
        let first = if start < self.start {
            start
        } else if start >= self.old_end {
            shifted(start)
        } else {
            self.start
        };
        let last = if end <= self.start {
            end
        } else if end >= self.old_end {
            shifted(end)
        } else {
            self.new_end
        };
        (first, last.max(first))
    }
}
