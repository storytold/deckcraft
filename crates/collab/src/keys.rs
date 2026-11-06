//! Session-scoped keys for DeckCraft collaboration (ADR 0002).
//!
//! File ids cannot identify an object for the length of a session: a `.pptx` shape id is unique
//! only within its slide (`crates/pptx/src/read/shapes.rs:22-50`), and the writer renumbers
//! slides positionally (`p:sldId@id = 256 + i`, `crates/pptx/src/write.rs:1112`). The map below
//! issues keys once per session and keeps both directions, so an operation names a key and the
//! adapter resolves it to the model id the engine uses.

use std::collections::HashMap;

use deckcraft_collab_proto::{ShapeKey, SlideKey};
use deckcraft_model::{Presentation, Shape, ShapeId, SlideId};

/// Deepest group nesting walked (the model's other walkers stop at 64).
const MAX_DEPTH: usize = 64;

/// Issues keys. The prefix separates sessions of the same document; tests pass a fixed one.
#[derive(Clone, Debug)]
pub struct KeyIssuer {
    prefix: String,
    next: u64,
}

impl KeyIssuer {
    pub fn new(prefix: impl Into<String>) -> Self {
        KeyIssuer { prefix: prefix.into(), next: 0 }
    }

    fn take(&mut self, tag: &str) -> String {
        self.next = self.next.saturating_add(1);
        format!("{}-{}{}", self.prefix, tag, self.next)
    }

    pub fn slide(&mut self) -> SlideKey {
        SlideKey(self.take("s"))
    }

    pub fn shape(&mut self) -> ShapeKey {
        ShapeKey(self.take("h"))
    }
}

/// Both directions of the key map. Keys are never removed: a deleted object keeps its key so a
/// tombstone can refuse a late operation (ADR 0002 §3).
#[derive(Debug, Default)]
pub struct KeyMap {
    slide_key: HashMap<SlideId, SlideKey>,
    slide_id: HashMap<SlideKey, SlideId>,
    /// Per slide, because a shape id repeats across slides.
    shape_key: HashMap<SlideId, HashMap<ShapeId, ShapeKey>>,
    shape_id: HashMap<SlideId, HashMap<ShapeKey, ShapeId>>,
    /// Every key ever issued for a shape that no longer exists, with its slide.
    dead_shapes: HashMap<ShapeKey, SlideId>,
}

impl KeyMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Give every slide and shape in `doc` a key it does not have yet. Idempotent: keys are
    /// stable for the session, so applying the same presentation twice issues nothing.
    pub fn sync(&mut self, doc: &Presentation, issuer: &mut KeyIssuer) {
        for slide in &doc.slides {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.slide_key.entry(slide.id) {
                let key = issuer.slide();
                entry.insert(key.clone());
                self.slide_id.insert(key, slide.id);
            }
            self.sync_shapes(slide.id, &slide.shapes, issuer, 0);
        }
    }

    fn sync_shapes(&mut self, slide: SlideId, shapes: &[Shape], issuer: &mut KeyIssuer, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for shape in shapes {
            let known = self.shape_key.get(&slide).is_some_and(|m| m.contains_key(&shape.id));
            if !known {
                let key = issuer.shape();
                self.shape_key.entry(slide).or_default().insert(shape.id, key.clone());
                self.shape_id.entry(slide).or_default().insert(key, shape.id);
            }
            let children = shape.children();
            if !children.is_empty() {
                self.sync_shapes(slide, children, issuer, depth + 1);
            }
        }
    }

    pub fn slide_key(&self, id: SlideId) -> Option<&SlideKey> {
        self.slide_key.get(&id)
    }

    pub fn slide_id(&self, key: &SlideKey) -> Option<SlideId> {
        self.slide_id.get(key).copied()
    }

    pub fn shape_key(&self, slide: SlideId, shape: ShapeId) -> Option<&ShapeKey> {
        self.shape_key.get(&slide)?.get(&shape)
    }

    pub fn shape_id(&self, slide: SlideId, key: &ShapeKey) -> Option<ShapeId> {
        self.shape_id.get(&slide)?.get(key).copied()
    }

    /// A shape that had a key but is gone from the model now.
    pub fn is_dead_shape(&self, slide: SlideId, key: &ShapeKey) -> bool {
        match self.shape_id.get(&slide).and_then(|m| m.get(key)) {
            Some(_) => false,
            None => self.dead_shapes.get(key).is_some_and(|s| *s == slide),
        }
    }

    /// Remember that a shape is gone, so a late operation is refused rather than resurrected.
    pub fn bury_shape(&mut self, slide: SlideId, shape: ShapeId) {
        let key = self.shape_key.get_mut(&slide).and_then(|m| m.remove(&shape));
        if let Some(key) = key {
            if let Some(ids) = self.shape_id.get_mut(&slide) {
                ids.remove(&key);
            }
            self.dead_shapes.insert(key, slide);
        }
    }

    pub fn key_count(&self) -> (usize, usize) {
        (self.slide_id.len(), self.shape_id.values().map(HashMap::len).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_engine::Session;
    use serde_json::json;

    /// A deck with one blank slide and two shapes, built through the same commands on every
    /// replica so ids and keys line up.
    fn deck() -> Session {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank slide");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop the title slide");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("insert");
        s.apply_remote("shape.insert", &json!({"preset": "ellipse", "rect": [400, 100, 120, 100]})).expect("insert");
        s
    }

    #[test]
    fn keys_are_stable_and_both_ways() {
        let s = deck();
        let doc = s.doc().expect("doc").doc.as_ref().clone();
        let mut issuer = KeyIssuer::new("a");
        let mut map = KeyMap::new();
        map.sync(&doc, &mut issuer);
        let (slides, shapes) = map.key_count();
        assert_eq!(slides, 1);
        assert_eq!(shapes, 2);

        let slide = doc.slides.first().expect("slide");
        let key = map.slide_key(slide.id).expect("key").clone();
        let shape = slide.shapes.first().expect("shape").id;
        let skey = map.shape_key(slide.id, shape).expect("key").clone();

        map.sync(&doc, &mut issuer);
        assert_eq!(map.key_count(), (slides, shapes), "sync is idempotent");
        assert_eq!(map.slide_key(slide.id), Some(&key), "a key never changes");
        assert_eq!(map.shape_key(slide.id, shape), Some(&skey), "a key never changes");

        assert_eq!(map.slide_id(&key), Some(slide.id));
        assert_eq!(map.shape_id(slide.id, &skey), Some(shape));
    }

    #[test]
    fn a_dead_shape_is_known_by_its_key() {
        let s = deck();
        let doc = s.doc().expect("doc").doc.as_ref().clone();
        let mut issuer = KeyIssuer::new("a");
        let mut map = KeyMap::new();
        map.sync(&doc, &mut issuer);
        let slide = doc.slides.first().expect("slide").id;
        let shape = doc.slides.first().expect("slide").shapes.first().expect("shape").id;
        let key = map.shape_key(slide, shape).expect("key").clone();
        map.bury_shape(slide, shape);
        assert!(map.is_dead_shape(slide, &key));
        assert!(map.shape_id(slide, &key).is_none());
    }
}
